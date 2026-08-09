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
    files.internal_ram[0x47C] = 0x1F;
    files.prg_ram[0x1730] = 0x05;
    let sample = SampleInput {
        frame_offset: 19,
        screen_role: "battle_animation".to_owned(),
        capture_dir: PathBuf::new(),
        expected_memory: vec![
            MemoryExpectation {
                region: MemoryRegion::InternalRam,
                address: 0x047C,
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
