use super::*;

#[test]
fn irregular_sampling_rejects_a_fixed_step_series_before_reading_captures() {
    let route = RouteInput {
        route_role: "sound_test_shared_battle".to_owned(),
        entry_action: "START".to_owned(),
        source_bound_effect: "enter shared battle engine".to_owned(),
        negative_case: false,
        samples: [0, 10, 20, 30]
            .into_iter()
            .map(|frame_offset| SampleInput {
                frame_offset,
                screen_role: "battle_animation".to_owned(),
                capture_dir: PathBuf::from("absent"),
                expected_memory: vec![MemoryExpectation {
                    region: MemoryRegion::PrgRam,
                    address: 0x7730,
                    bytes_hex: "05".to_owned(),
                    reason: "shared battle outer phase".to_owned(),
                }],
            })
            .collect(),
    };

    let error = analyze_route(&route, Path::new("."))
        .unwrap_err()
        .to_string();
    assert!(error.contains("irregular"));
}

#[test]
fn visible_sprite_union_excludes_hidden_oam_entries() {
    let mut oam = vec![0xFF; OAM_BYTE_COUNT];
    oam[0..4].copy_from_slice(&[0x20, 0x31, 0x00, 0x40]);
    oam[4..8].copy_from_slice(&[0xEF, 0x32, 0x00, 0x40]);
    oam[8..12].copy_from_slice(&[0xEE, 0x33, 0x00, 0x40]);

    let (codes, count) = visible_sprite_tile_codes_for(&oam);

    assert_eq!(count, 2);
    assert_eq!(codes, BTreeSet::from([0x31, 0x33]));
}

#[test]
fn nametable_union_reads_tile_bytes_from_both_physical_pages_only() {
    let mut nametable = vec![0; NAMETABLE_BYTE_COUNT];
    nametable[0] = 0x11;
    nametable[NAMETABLE_PAGE_BYTE_COUNT] = 0x22;
    nametable[NAMETABLE_TILE_BYTE_COUNT] = 0x33;
    nametable[NAMETABLE_PAGE_BYTE_COUNT + NAMETABLE_TILE_BYTE_COUNT] = 0x44;

    let codes = nametable_tile_codes_for(&nametable);

    assert!(codes.contains(&0x11));
    assert!(codes.contains(&0x22));
    assert!(!codes.contains(&0x33));
    assert!(!codes.contains(&0x44));
}

#[test]
fn memory_expectations_use_cpu_addresses_for_each_dump_region() {
    let mut files = CaptureFiles {
        screenshot: b"\x89PNG\r\n\x1A\n".to_vec(),
        state: Vec::new(),
        internal_ram: vec![0; INTERNAL_RAM_BYTE_COUNT],
        prg_ram: vec![0; PRG_RAM_BYTE_COUNT],
        nametable: vec![0; NAMETABLE_BYTE_COUNT],
        oam: vec![0xFF; OAM_BYTE_COUNT],
        palette: vec![0; PALETTE_BYTE_COUNT],
    };
    files.internal_ram[usize::from(BATTLE_RUNTIME_STATE.shared_phase_address)] = 0x1F;
    files.prg_ram[0x1730] = 0x05;
    let sample = SampleInput {
        frame_offset: 19,
        screen_role: "battle_animation".to_owned(),
        capture_dir: PathBuf::new(),
        expected_memory: vec![
            MemoryExpectation {
                region: MemoryRegion::InternalRam,
                address: usize::from(BATTLE_RUNTIME_STATE.shared_phase_address),
                bytes_hex: "1F".to_owned(),
                reason: "shared engine terminal phase".to_owned(),
            },
            MemoryExpectation {
                region: MemoryRegion::PrgRam,
                address: 0x7730,
                bytes_hex: "05".to_owned(),
                reason: "sound-test outer phase".to_owned(),
            },
        ],
    };

    validate_memory_expectations(&sample, &files).unwrap();
}

#[test]
fn required_route_polarity_keeps_favorable_and_negative_cases_distinct() {
    let route = RouteInput {
        route_role: "gameplay_battle_unfavorable".to_owned(),
        entry_action: "source-bound gameplay attack".to_owned(),
        source_bound_effect: "attacker misses and receives damage".to_owned(),
        negative_case: false,
        samples: Vec::new(),
    };

    let error = validate_route_contract(&route).unwrap_err().to_string();
    assert!(error.contains("at least"));

    let mut route = route;
    route.samples = (0..MIN_IRREGULAR_SAMPLE_COUNT)
        .map(|index| SampleInput {
            frame_offset: u64::try_from(index * index + index).unwrap(),
            screen_role: "battle_animation".to_owned(),
            capture_dir: PathBuf::new(),
            expected_memory: Vec::new(),
        })
        .collect();
    let error = validate_route_contract(&route).unwrap_err().to_string();
    assert!(error.contains("negative route"));
}

#[test]
fn producer_frame_deltas_must_match_declared_exact_steps() {
    validate_producer_frame_deltas(
        "sound_test_shared_battle",
        &[43, 82, 171],
        &[10_043, 10_082, 10_171],
    )
    .unwrap();

    let error = validate_producer_frame_deltas(
        "sound_test_shared_battle",
        &[43, 82, 171],
        &[10_043, 10_083, 10_171],
    )
    .unwrap_err()
    .to_string();
    assert!(error.contains("exact-step delta"));
}

#[test]
fn battle_runtime_input_projects_selector_62_before_the_late_write() {
    let runtime = crate::battle_runtime_state::BATTLE_RUNTIME_STATE;
    let selector = runtime.dialogue_selector_projection;
    let mut files = CaptureFiles {
        screenshot: b"\x89PNG\r\n\x1A\n".to_vec(),
        state: Vec::new(),
        internal_ram: vec![0; INTERNAL_RAM_BYTE_COUNT],
        prg_ram: vec![0; PRG_RAM_BYTE_COUNT],
        nametable: vec![0; NAMETABLE_BYTE_COUNT],
        oam: vec![0xFF; OAM_BYTE_COUNT],
        palette: vec![0; PALETTE_BYTE_COUNT],
    };
    for (addresses, values) in [
        (runtime.staged_participant_identity_addresses, [0x04, 0x85]),
        (runtime.staged_class_identity_addresses, [0x01, 0x08]),
        (runtime.staged_item_source_index_addresses, [0x0B, 0x1A]),
        (runtime.staged_terrain_source_index_addresses, [0x00, 0x0B]),
    ] {
        for (address, value) in addresses.into_iter().zip(values) {
            files.internal_ram[usize::from(address)] = value;
        }
    }
    for address in selector.required_nonzero_addresses {
        files.internal_ram[usize::from(address)] = 1;
    }

    let input = observed_battle_runtime_input(&files).unwrap();

    assert_eq!(input.observed_dialogue_selector, 0);
    assert_eq!(input.projected_dialogue_selector, selector.forced_selector);
    assert!(input.selector_62_predicate_matched);

    files.internal_ram[usize::from(selector.required_zero_addresses[0])] = 1;
    let input = observed_battle_runtime_input(&files).unwrap();
    assert_eq!(input.projected_dialogue_selector, 0);
    assert!(!input.selector_62_predicate_matched);
}

#[test]
fn game_over_samples_accept_every_source_selected_dialogue_record() {
    let mut files = CaptureFiles {
        screenshot: b"\x89PNG\r\n\x1A\n".to_vec(),
        state: Vec::new(),
        internal_ram: vec![0; INTERNAL_RAM_BYTE_COUNT],
        prg_ram: vec![0; PRG_RAM_BYTE_COUNT],
        nametable: vec![0; NAMETABLE_BYTE_COUNT],
        oam: vec![0xFF; OAM_BYTE_COUNT],
        palette: vec![0; PALETTE_BYTE_COUNT],
    };
    files.prg_ram[GAME_OVER_DIALOGUE_DIRECTORY_SELECTOR_ADDRESS - 0x6000] = 0xB0;
    for entry_selector in [0x06, 0x07, 0x08, 0x09, 0x0A] {
        files.prg_ram[GAME_OVER_DIALOGUE_ENTRY_SELECTOR_ADDRESS - 0x6000] = entry_selector;
        assert_eq!(
            validate_game_over_dialogue_selector(&files).unwrap(),
            format!("B0:{entry_selector:02X}")
        );
    }

    files.prg_ram[GAME_OVER_DIALOGUE_ENTRY_SELECTOR_ADDRESS - 0x6000] = 0x05;
    let error = validate_game_over_dialogue_selector(&files)
        .unwrap_err()
        .to_string();
    assert!(error.contains("outside source-selected family"));

    files.prg_ram[GAME_OVER_DIALOGUE_DIRECTORY_SELECTOR_ADDRESS - 0x6000] = 0xAF;
    files.prg_ram[GAME_OVER_DIALOGUE_ENTRY_SELECTOR_ADDRESS - 0x6000] = 0x06;
    let error = validate_game_over_dialogue_selector(&files)
        .unwrap_err()
        .to_string();
    assert!(error.contains("AF:06"));
}
