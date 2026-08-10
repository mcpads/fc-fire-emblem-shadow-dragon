use super::{runtime::*, *};

fn input(seed: u8) -> BattleRuntimeRecipeInput {
    BattleRuntimeRecipeInput {
        participant_record_identities: [seed, 0x80 | seed],
        class_record_identities: [seed, seed],
        item_source_indices: [seed, seed],
        terrain_source_indices: [seed, seed],
        dialogue_selector: seed,
    }
}

#[test]
fn runtime_routines_fit_the_fixed_cave_without_overlap() {
    let inputs = (1..=5).map(input).collect::<BTreeSet<_>>();
    let routines = build_runtime_routines(
        &inputs,
        RecipeDirectoryAddresses {
            unit: 0xB020,
            enemy: 0xB088,
            class: 0xB112,
            item: 0xB142,
            terrain: 0xB1F8,
            dialogue: 0xB218,
        },
    )
    .unwrap();

    assert_eq!(routines.len(), 10);
    assert!(routines.windows(2).all(|pair| {
        pair[0].address as usize + pair[0].bytes.len() <= pair[1].address as usize
    }));
    let last = routines.last().unwrap();
    assert!(last.address as usize + last.bytes.len() <= FIXED_CAVE_END_ADDRESS as usize);
}

#[test]
fn tuple_predicate_contains_every_field_and_projection_call() {
    let bytes = admitted_tuple_predicate(&[input(1)].into_iter().collect()).unwrap();
    for address in RUNTIME_FIELD_ADDRESSES {
        assert!(
            bytes
                .windows(3)
                .any(|window| window == [0xAD, address as u8, (address >> 8) as u8])
        );
    }
    assert!(bytes.windows(3).any(|window| {
        window
            == [
                0x20,
                PROJECT_DIALOGUE_SELECTOR_ADDRESS as u8,
                (PROJECT_DIALOGUE_SELECTOR_ADDRESS >> 8) as u8,
            ]
    }));
}

#[test]
fn participant_recipe_dispatch_preserves_the_computed_source_index() {
    let directories = RecipeDirectoryAddresses {
        unit: 0xB020,
        enemy: 0xB088,
        class: 0xB112,
        item: 0xB142,
        terrain: 0xB1F8,
        dialogue: 0xB218,
    };
    let bytes = apply_participant(directories).unwrap();

    for directory in [directories.enemy, directories.unit] {
        let preserved_dispatch = [
            0xAA,
            0xA9,
            directory as u8,
            0x85,
            DIRECTORY_POINTER_LOW,
            0xA9,
            (directory >> 8) as u8,
            0x85,
            DIRECTORY_POINTER_HIGH,
            0x8A,
            0x4C,
            APPLY_DIRECTORY_ADDRESS as u8,
            (APPLY_DIRECTORY_ADDRESS >> 8) as u8,
        ];
        assert!(
            bytes
                .windows(preserved_dispatch.len())
                .any(|window| window == preserved_dispatch)
        );
    }
}

#[test]
fn dispatch_and_composer_restore_post_scan_registers_and_borrowed_scratch() {
    let dispatch = composition_dispatch().unwrap();
    assert!(dispatch.starts_with(&[0x20, 0xD9, 0xC2, 0x08, 0x48, 0x8A, 0x48, 0x98, 0x48]));
    assert!(dispatch.ends_with(&[0x68, 0xA8, 0x68, 0xAA, 0x68, 0x28, 0x60]));

    let compose = compose_page(RecipeDirectoryAddresses {
        unit: 0xB020,
        enemy: 0xB088,
        class: 0xB112,
        item: 0xB142,
        terrain: 0xB1F8,
        dialogue: 0xB218,
    })
    .unwrap();
    for address in BORROWED_SCRATCH {
        assert!(
            compose
                .windows(3)
                .any(|window| window == [0xA5, address, 0x48])
        );
        assert!(
            compose
                .windows(3)
                .any(|window| window == [0x68, 0x85, address])
        );
    }
    assert!(compose.windows(3).any(|window| {
        window
            == [
                0x20,
                SOURCE_PRG_BANK_SELECTOR as u8,
                (SOURCE_PRG_BANK_SELECTOR >> 8) as u8,
            ]
    }));
}

#[test]
fn right_page_selectors_require_a_battle_main_state_and_uploaded_cache() {
    for bytes in [
        battle_right_selector(BATTLE_RIGHT_FD_SELECTOR_ADDRESS, 2).unwrap(),
        battle_right_selector(BATTLE_RIGHT_FE_SELECTOR_ADDRESS, 4).unwrap(),
        battle_central_right_fd_selector().unwrap(),
    ] {
        assert!(bytes.windows(3).any(|window| {
            window
                == [
                    0xAD,
                    MAIN_STATE_ADDRESS as u8,
                    (MAIN_STATE_ADDRESS >> 8) as u8,
                ]
        }));
        assert!(
            bytes
                .windows(2)
                .any(|window| { window == [0xC9, PLAYER_INITIATED_BATTLE_STATE] })
        );
        assert!(
            bytes
                .windows(2)
                .any(|window| { window == [0xC9, ENEMY_INITIATED_BATTLE_STATE] })
        );
        assert!(bytes.windows(3).any(|window| {
            window
                == [
                    0xAD,
                    BATTLE_ACTIVE_FLAG as u8,
                    (BATTLE_ACTIVE_FLAG >> 8) as u8,
                ]
        }));
        assert!(
            bytes
                .windows(2)
                .any(|window| window == [0x29, CACHE_UPLOADED_MARKER])
        );
    }
}

#[test]
fn composition_report_omits_translation_content_and_private_paths() {
    let report = BattleCompositionLoaderProbeReport {
        schema: 1,
        source_sha1: EXPECTED_SOURCE_SHA1,
        base_report_sha1: "base-report".to_owned(),
        base_output_sha1: "base".to_owned(),
        temporal_manifest_sha1: "temporal".to_owned(),
        output_sha1: "output".to_owned(),
        output_mapper: OUTPUT_MAPPER,
        prg_size: EXPANDED_PRG_SIZE,
        chr_size: 0,
        admitted_runtime_tuple_count: 5,
        runtime_field_count: 9,
        maximum_observed_unique_overlay_count: 88,
        maximum_observed_raw_glyph_reference_count: 100,
        source_page_ppu_write_count: FONT_PAGE_SIZE,
        maximum_observed_overlay_ppu_write_count: 1600,
        maximum_observed_total_ppu_write_count: 5696,
        glyph_atlas_mmc3_page: GLYPH_ATLAS_MMC3_PAGE,
        source_and_recipe_mmc3_page: SOURCE_PAGE_MMC3_PAGE,
        atlas_cpu_address_hex: "0x8000".to_owned(),
        physical_code_table_cpu_address_hex: "0x9400".to_owned(),
        source_page_cpu_address_hex: "0xA000".to_owned(),
        recipe_blob_cpu_address_hex: "0xB000".to_owned(),
        fixed_cave_start_cpu_address_hex: "0xFAF3".to_owned(),
        fixed_cave_end_cpu_address_exclusive_hex: "0xFFA0".to_owned(),
        fixed_cave_byte_count: 1197,
        runtime_routine_count: 10,
        runtime_routine_byte_count: 900,
        runtime_tracked_write_count: 15,
        source_raw_direct_cave_transfer_pattern_count: 101,
        raw_direct_transfer_patterns_are_code_proof: false,
        borrowed_scratch_byte_count: 8,
        borrowed_scratch_restored: true,
        ppu_address_latch_reset_before_composition: true,
        sequential_ppu_increment_during_composition: true,
        rendering_disabled_during_composition: true,
        nmi_disabled_during_composition: true,
        pending_vblank_cleared_before_nmi_restore: true,
        source_prg_bank_restored_from_shadow: true,
        runtime_recipe_duplicates_replayed: true,
        source_bound_dialogue_projection_installed: true,
        admitted_tuple_gate_installed: true,
        battle_zero_right_page_uses_chr_ram_after_success: true,
        non_battle_right_pages_use_natural_selection: true,
        physical_assignment_catalog_complete: false,
        runtime_cycle_budget_measured: false,
        runtime_verified: false,
        release_eligible: false,
        translation_text_emitted: false,
        glyph_characters_emitted: false,
        next_gate: "runtime proof",
    };

    let json = serde_json::to_string(&report).unwrap();
    assert!(!json.contains("private/"));
    assert!(!json.contains('한'));
    assert!(!json.contains("korean"));
}
