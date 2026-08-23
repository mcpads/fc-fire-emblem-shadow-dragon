use super::{dynamic_assignment::build_dynamic_assignment_routines, runtime::*, *};
use crate::battle_runtime_state::{
    BATTLE_COMPOSITION_LIFETIME_START_WRITES, BATTLE_RUNTIME_STATE,
    SAME_BATTLE_ROUND_ACTIVATION_WRITE,
};

fn test_recipe_directories() -> RecipeDirectoryAddresses {
    RecipeDirectoryAddresses {
        unit: 0xB020,
        enemy: 0xB088,
        class: 0xB112,
        item: 0xB142,
        terrain: 0xB1F8,
        dialogue: 0xB218,
    }
}

#[test]
fn runtime_routines_fit_the_fixed_cave_without_overlap() {
    let routines = build_runtime_routines(RecipeDirectoryAddresses {
        unit: 0xB020,
        enemy: 0xB088,
        class: 0xB112,
        item: 0xB142,
        terrain: 0xB1F8,
        dialogue: 0xB218,
    })
    .unwrap();

    assert!(routines.windows(2).all(|pair| {
        pair[0].address as usize + pair[0].bytes.len() <= pair[1].address as usize
    }));
    let primary_last = routines
        .iter()
        .find(|routine| routine.address == PROJECT_COLOR_ADDRESS)
        .unwrap();
    assert!(
        primary_last.address as usize + primary_last.bytes.len() <= FIXED_CAVE_END_ADDRESS as usize
    );
    let post_data = routines.last().unwrap();
    assert_eq!(
        post_data.address,
        CENTRAL_RIGHT_FE_RESUPPLY_SELECTOR_ADDRESS
    );
    assert!(
        post_data.address as usize + post_data.bytes.len() <= POST_DATA_CAVE_END_ADDRESS as usize
    );
}

#[test]
fn cumulative_layout_preserves_existing_selector_ranges() {
    let layout = CUMULATIVE_RUNTIME_LAYOUT;
    let routines = build_runtime_routines_for_layout(
        RecipeDirectoryAddresses {
            unit: 0xB020,
            enemy: 0xB088,
            class: 0xB112,
            item: 0xB142,
            terrain: 0xB1F8,
            dialogue: 0xB218,
        },
        layout,
        0xFB80,
    )
    .unwrap();

    let protected = [(0xFB20_usize, 0xFC20_usize), (0xFC60, 0xFC99)];
    assert!(routines.windows(2).all(|pair| {
        pair[0].address as usize + pair[0].bytes.len() <= pair[1].address as usize
    }));
    assert!(routines.iter().all(|routine| {
        let start = usize::from(routine.address);
        let end = start + routine.bytes.len();
        protected.iter().all(|(protected_start, protected_end)| {
            end <= *protected_start || start >= *protected_end
        })
    }));
    let primary_last = routines
        .iter()
        .find(|routine| routine.address == layout.project_color)
        .unwrap();
    assert_eq!(
        usize::from(primary_last.address) + primary_last.bytes.len(),
        0xFF8F
    );
    let post_data = routines.last().unwrap();
    assert_eq!(post_data.address, layout.central_right_fe_resupply_selector);
    assert!(usize::from(post_data.address) + post_data.bytes.len() <= 0xFFC0);
}

#[test]
fn dynamic_assignment_routines_fit_the_material_page_without_overlap() {
    let routines = build_dynamic_assignment_routines(RecipeDirectoryAddresses {
        unit: 0xB020,
        enemy: 0xB088,
        class: 0xB112,
        item: 0xB142,
        terrain: 0xB1F8,
        dialogue: 0xB218,
    })
    .unwrap();

    assert_eq!(routines.len(), 7);
    assert!(routines.windows(2).all(|pair| {
        pair[0].address as usize + pair[0].bytes.len() <= pair[1].address as usize
    }));
    assert_eq!(routines[0].address, DYNAMIC_ASSIGNMENT_CODE_CPU_ADDRESS);
}

#[test]
fn dynamic_assignment_collects_every_runtime_field_and_projected_dialogue() {
    let directories = RecipeDirectoryAddresses {
        unit: 0xB020,
        enemy: 0xB088,
        class: 0xB112,
        item: 0xB142,
        terrain: 0xB1F8,
        dialogue: 0xB218,
    };
    let bytes = &build_dynamic_assignment_routines(directories).unwrap()[0].bytes;
    for address in runtime_recipe_fields(directories)
        .into_iter()
        .filter_map(|field| field.index_source.staging_address())
    {
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

    let participant_name_call = [0x20, 0x60, 0x97];
    let unit_name_collection = [
        0xA9,
        directories.unit as u8,
        0x85,
        DIRECTORY_POINTER_LOW,
        0xA9,
        (directories.unit >> 8) as u8,
        0x85,
        DIRECTORY_POINTER_HIGH,
        0xAD,
        0x04,
        0x03,
        participant_name_call[0],
        participant_name_call[1],
        participant_name_call[2],
    ];
    let enemy_name_collection = [
        0xA9,
        directories.enemy as u8,
        0x85,
        DIRECTORY_POINTER_LOW,
        0xA9,
        (directories.enemy >> 8) as u8,
        0x85,
        DIRECTORY_POINTER_HIGH,
        0xAD,
        0x05,
        0x03,
        0x29,
        0x7F,
        participant_name_call[0],
        participant_name_call[1],
        participant_name_call[2],
    ];
    assert!(
        bytes
            .windows(unit_name_collection.len())
            .any(|window| window == unit_name_collection)
    );
    assert!(
        bytes
            .windows(enemy_name_collection.len())
            .any(|window| window == enemy_name_collection)
    );
    assert!(!bytes.windows(2).any(|window| window == [0xC9, 0x80]));
}

#[test]
fn successful_allocation_records_the_projected_dialogue_cache_key() {
    let routines = build_dynamic_assignment_routines(RecipeDirectoryAddresses {
        unit: 0xB020,
        enemy: 0xB088,
        class: 0xB112,
        item: 0xB142,
        terrain: 0xB1F8,
        dialogue: 0xB218,
    })
    .unwrap();
    let bytes = &routines
        .iter()
        .find(|routine| routine.role == "protected-color remap allocation")
        .unwrap()
        .bytes;

    assert!(bytes.windows(11).any(|window| {
        window
            == [
                0x8D,
                REMAP_STATE_ADDRESS as u8,
                (REMAP_STATE_ADDRESS >> 8) as u8,
                0x20,
                PROJECT_DIALOGUE_SELECTOR_ADDRESS as u8,
                (PROJECT_DIALOGUE_SELECTOR_ADDRESS >> 8) as u8,
                0x8D,
                CACHED_DIALOGUE_SELECTOR_ADDRESS as u8,
                (CACHED_DIALOGUE_SELECTOR_ADDRESS >> 8) as u8,
                0xA9,
                0x00,
            ]
    }));
}

#[test]
fn participant_recipe_dispatch_uses_side_owned_directories_and_normalizes_enemy_identity() {
    let directories = RecipeDirectoryAddresses {
        unit: 0xB020,
        enemy: 0xB088,
        class: 0xB112,
        item: 0xB142,
        terrain: 0xB1F8,
        dialogue: 0xB218,
    };
    let bytes = apply_participant(directories).unwrap();

    assert!(!bytes.windows(2).any(|window| window == [0xC9, 0x80]));
    assert_eq!(
        bytes
            .windows(2)
            .filter(|window| *window == [0x29, 0x7F])
            .count(),
        1
    );

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
fn participant_name_call_sites_select_source_owned_unit_and_enemy_directories() {
    let bytes = compose_page(test_recipe_directories()).unwrap();
    let unit_call = [
        0x20,
        APPLY_PARTICIPANT_ADDRESS as u8,
        (APPLY_PARTICIPANT_ADDRESS >> 8) as u8,
    ];
    let enemy_entry = enemy_participant_name_entry().unwrap();
    let enemy_call = [0x20, enemy_entry as u8, (enemy_entry >> 8) as u8];

    assert!(
        bytes
            .windows(6)
            .any(|window| window[..3] == [0xAD, 0x04, 0x03] && window[3..] == unit_call)
    );
    assert!(
        bytes
            .windows(6)
            .any(|window| window[..3] == [0xAD, 0x05, 0x03] && window[3..] == enemy_call)
    );
}

#[test]
fn dispatch_and_composer_restore_post_scan_registers_and_borrowed_scratch() {
    let dispatch = composition_dispatch().unwrap();
    assert!(dispatch.starts_with(&[0x20, 0xD9, 0xC2, 0x08, 0x48, 0x8A, 0x48, 0x98, 0x48]));
    assert!(dispatch.ends_with(&[0x68, 0xA8, 0x68, 0xAA, 0x68, 0x28, 0x60]));
    assert!(dispatch.windows(5).any(|window| {
        window
            == [
                0xA9,
                0x00,
                0x8D,
                REMAP_STATE_ADDRESS as u8,
                (REMAP_STATE_ADDRESS >> 8) as u8,
            ]
    }));
    assert!(dispatch.windows(5).any(|window| {
        window[..3]
            == [
                0xAD,
                BATTLE_RUNTIME_STATE.active_flag_address as u8,
                (BATTLE_RUNTIME_STATE.active_flag_address >> 8) as u8,
            ]
            && window[3] == 0xF0
    }));

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
    let assignment_call = compose
        .windows(3)
        .position(|window| {
            window
                == [
                    0x20,
                    DYNAMIC_ASSIGNMENT_CODE_CPU_ADDRESS as u8,
                    (DYNAMIC_ASSIGNMENT_CODE_CPU_ADDRESS >> 8) as u8,
                ]
        })
        .unwrap();
    let chr_ram_selection = compose
        .windows(5)
        .position(|window| window == [0xA9, 0x02, 0x20, 0x58, 0xFA])
        .unwrap();
    assert!(assignment_call < chr_ram_selection);
}

#[test]
fn battle_surface_visibility_covers_every_host_and_rejects_non_battle_phase_zero() {
    let lifetime = BATTLE_RUNTIME_STATE.surface_lifetime;
    let visible = |main_state, dialogue_substate, sound_test_phase, shared_phase| {
        run_battle_surface_visibility_predicate(BattleSurfaceInputs {
            main_state,
            dialogue_substate,
            sound_test_phase,
            shared_phase,
        })
    };

    assert!(!visible(0x00, 0x00, 0x00, 0x00));
    assert!(!visible(0x00, 0x00, 0x00, 0x08));
    assert!(visible(lifetime.player_battle_main_state, 0, 0, 0x00));
    assert!(visible(
        lifetime.player_battle_main_state,
        0,
        0,
        BATTLE_RUNTIME_STATE.terminal_shared_phase() - 1
    ));
    assert!(visible(lifetime.enemy_battle_main_state, 0, 0, 0x08));
    assert!(visible(lifetime.arena_battle_main_state, 0, 0, 0x00));
    assert!(visible(
        lifetime.sound_test_main_state,
        lifetime.sound_test_battle_substate,
        lifetime.sound_test_shared_battle_phase,
        0x00
    ));
    assert!(!visible(
        lifetime.sound_test_main_state,
        lifetime.sound_test_battle_substate - 1,
        lifetime.sound_test_shared_battle_phase,
        0x00
    ));
    assert!(!visible(
        lifetime.sound_test_main_state,
        lifetime.sound_test_battle_substate,
        lifetime.sound_test_shared_battle_phase - 1,
        0x00
    ));
    for shared_phase in [
        BATTLE_RUNTIME_STATE.terminal_shared_phase(),
        BATTLE_RUNTIME_STATE.shared_phase_count,
        0xFF,
    ] {
        assert!(!visible(
            lifetime.player_battle_main_state,
            0,
            0,
            shared_phase
        ));
    }
}

#[derive(Clone, Copy)]
struct BattleSurfaceInputs {
    main_state: u8,
    dialogue_substate: u8,
    sound_test_phase: u8,
    shared_phase: u8,
}

fn run_battle_surface_visibility_predicate(inputs: BattleSurfaceInputs) -> bool {
    const ZERO: u8 = 0x02;
    const CARRY: u8 = 0x01;

    let bytes = battle_surface_visible().unwrap();
    let origin = PROBE_RUNTIME_LAYOUT.battle_surface_visible;
    let lifetime = BATTLE_RUNTIME_STATE.surface_lifetime;
    let mut a = 0;
    let mut status = 0;
    let mut pc = origin;

    for _ in 0..32 {
        let offset = usize::from(pc - origin);
        let opcode = bytes[offset];
        pc += 1;
        match opcode {
            0x60 => return status & ZERO == 0,
            0xB0 | 0xD0 | 0xF0 => {
                let displacement = bytes[usize::from(pc - origin)] as i8;
                pc += 1;
                let taken = match opcode {
                    0xB0 => status & CARRY != 0,
                    0xD0 => status & ZERO == 0,
                    0xF0 => status & ZERO != 0,
                    _ => unreachable!(),
                };
                if taken {
                    pc = pc.wrapping_add_signed(i16::from(displacement));
                }
            }
            0xA9 => {
                a = bytes[usize::from(pc - origin)];
                pc += 1;
                status = (status & !ZERO) | if a == 0 { ZERO } else { 0 };
            }
            0xA5 => {
                let address = u16::from(bytes[usize::from(pc - origin)]);
                pc += 1;
                assert_eq!(address, lifetime.main_state_address);
                a = inputs.main_state;
                status = (status & !ZERO) | if a == 0 { ZERO } else { 0 };
            }
            0xAD => {
                let operand = usize::from(pc - origin);
                let address = u16::from_le_bytes([bytes[operand], bytes[operand + 1]]);
                pc += 2;
                a = match address {
                    address if address == BATTLE_RUNTIME_STATE.shared_phase_address => {
                        inputs.shared_phase
                    }
                    address if address == lifetime.dialogue_substate_address => {
                        inputs.dialogue_substate
                    }
                    address if address == lifetime.sound_test_phase_address => {
                        inputs.sound_test_phase
                    }
                    _ => panic!("unexpected predicate input ${address:04X}"),
                };
                status = (status & !ZERO) | if a == 0 { ZERO } else { 0 };
            }
            0xC9 => {
                let expected = bytes[usize::from(pc - origin)];
                pc += 1;
                status &= !(CARRY | ZERO);
                if a >= expected {
                    status |= CARRY;
                }
                if a == expected {
                    status |= ZERO;
                }
            }
            _ => panic!("unexpected predicate opcode ${opcode:02X}"),
        }
    }
    panic!("battle-surface visibility predicate did not return")
}

#[test]
fn battle_initializer_preserves_source_effect_and_reopens_composition() {
    let initializer = initialize_battle_remap().unwrap();
    assert!(initializer.starts_with(&[
        0x8D,
        BATTLE_RUNTIME_STATE.active_flag_address as u8,
        (BATTLE_RUNTIME_STATE.active_flag_address >> 8) as u8,
        0x08,
        0x48,
    ]));
    assert!(initializer.windows(5).any(|window| {
        window
            == [
                0xA9,
                0x00,
                0x8D,
                REMAP_STATE_ADDRESS as u8,
                (REMAP_STATE_ADDRESS >> 8) as u8,
            ]
    }));
    assert!(initializer.ends_with(&[0x68, 0x28, 0x60]));
}

#[test]
fn battle_lifetime_starts_reopen_composition_without_clearing_same_battle_rounds() {
    let mut bytes = crate::test_support::synthetic_mapper165_rom_bytes(0xFF);
    for writer in BATTLE_COMPOSITION_LIFETIME_START_WRITES {
        let bank = writer.prg_bank;
        let address = writer.cpu_address;
        let offset = switchable_bank_file_offset(bank, address).unwrap();
        bytes[offset..offset + 3].copy_from_slice(
            &assemble_at(
                address,
                &[Instruction::StaAbsolute(
                    BATTLE_RUNTIME_STATE.active_flag_address,
                )],
            )
            .unwrap(),
        );
    }
    let round_bank = SAME_BATTLE_ROUND_ACTIVATION_WRITE.prg_bank;
    let round_address = SAME_BATTLE_ROUND_ACTIVATION_WRITE.cpu_address;
    let round_offset = switchable_bank_file_offset(round_bank, round_address).unwrap();
    bytes[round_offset..round_offset + 3].copy_from_slice(
        &assemble_at(
            round_address,
            &[Instruction::StaAbsolute(
                BATTLE_RUNTIME_STATE.active_flag_address,
            )],
        )
        .unwrap(),
    );
    let mut image = TrackedImage::new(bytes);

    install_battle_lifetime_remap_initializers(&mut image, PROBE_RUNTIME_LAYOUT).unwrap();

    assert_eq!(
        image.writes().len(),
        BATTLE_COMPOSITION_LIFETIME_START_WRITES.len()
    );
    let output = image.into_data();
    for writer in BATTLE_COMPOSITION_LIFETIME_START_WRITES {
        let bank = writer.prg_bank;
        let address = writer.cpu_address;
        let offset = switchable_bank_file_offset(bank, address).unwrap();
        assert_eq!(
            &output[offset..offset + 3],
            assemble_at(
                address,
                &[Instruction::JsrAbsolute(
                    PROBE_RUNTIME_LAYOUT.initialize_battle_remap,
                )],
            )
            .unwrap()
        );
    }
    assert_eq!(
        &output[round_offset..round_offset + 3],
        assemble_at(
            round_address,
            &[Instruction::StaAbsolute(
                BATTLE_RUNTIME_STATE.active_flag_address,
            )]
        )
        .unwrap()
        .as_slice()
    );
}

#[test]
fn recipe_upload_and_shared_text_use_the_same_remap_projection() {
    let project_call = [
        0x20,
        PROJECT_COLOR_ADDRESS as u8,
        (PROJECT_COLOR_ADDRESS >> 8) as u8,
    ];
    assert!(
        apply_recipe()
            .unwrap()
            .windows(project_call.len())
            .any(|window| window == project_call)
    );

    let wrapper = text_projection_wrapper().unwrap();
    assert!(wrapper.starts_with(&[0x8A, 0x48, 0xB1, RECIPE_POINTER_LOW]));
    assert!(wrapper.ends_with(&[0x68, 0xAA, 0xA5, PHYSICAL_TILE_CODE, 0xC9, 0xEF, 0x60]));
    assert!(
        wrapper
            .windows(project_call.len())
            .any(|window| window == project_call)
    );
}

#[test]
fn runtime_consumers_require_a_visible_battle_surface_and_persistent_remap_state() {
    for bytes in [
        battle_right_selector(BATTLE_RIGHT_FD_SELECTOR_ADDRESS, 2).unwrap(),
        battle_right_selector(BATTLE_RIGHT_FE_SELECTOR_ADDRESS, 4).unwrap(),
        battle_central_right_fd_selector().unwrap(),
        central_right_fe_resupply_selector_for_layout(PROBE_RUNTIME_LAYOUT).unwrap(),
        text_projection_wrapper().unwrap(),
    ] {
        assert!(bytes.windows(3).any(|window| {
            window
                == [
                    0x20,
                    BATTLE_SURFACE_VISIBLE_ADDRESS as u8,
                    (BATTLE_SURFACE_VISIBLE_ADDRESS >> 8) as u8,
                ]
        }));
        assert!(bytes.windows(3).any(|window| {
            window
                == [
                    0xAD,
                    REMAP_STATE_ADDRESS as u8,
                    (REMAP_STATE_ADDRESS >> 8) as u8,
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
fn central_resupply_keeps_both_composed_right_pages_until_the_battle_surface_releases_them() {
    let fd = battle_central_right_fd_selector().unwrap();
    assert!(
        fd.windows(5)
            .any(|window| window == [0xA9, 0x02, 0x20, 0x58, 0xFA])
    );
    assert!(
        fd.windows(5)
            .any(|window| window == [0xA9, 0x00, 0x8D, 0x01, 0x80])
    );
    assert!(!fd.windows(2).any(|window| window == [0x29, 0x1F]));

    let fe = central_right_fe_resupply_selector_for_layout(PROBE_RUNTIME_LAYOUT).unwrap();
    assert!(fe.windows(5).any(|window| {
        window
            == [
                0xA9,
                0x00,
                0x20,
                BATTLE_RIGHT_FE_SELECTOR_ADDRESS as u8,
                (BATTLE_RIGHT_FE_SELECTOR_ADDRESS >> 8) as u8,
            ]
    }));
    assert!(fe.ends_with(&[0x68, 0x28, 0x60]));
    let natural_tail =
        central_right_fe_resupply_natural_tail_for_layout(PROBE_RUNTIME_LAYOUT).unwrap();
    assert_eq!(
        natural_tail,
        [
            0x68,
            0x28,
            0x4C,
            BATTLE_RIGHT_FE_SELECTOR_ADDRESS as u8,
            (BATTLE_RIGHT_FE_SELECTOR_ADDRESS >> 8) as u8,
        ]
    );
}

#[test]
fn zero_right_page_selects_its_mapper_register_before_writing_chr_ram() {
    for (address, mapper_register) in [
        (BATTLE_RIGHT_FD_SELECTOR_ADDRESS, 2),
        (BATTLE_RIGHT_FE_SELECTOR_ADDRESS, 4),
    ] {
        let bytes = battle_right_selector(address, mapper_register).unwrap();
        let zero_page_branch = bytes
            .windows(5)
            .position(|window| window[0..3] == [0xA9, 0x00, 0x4C])
            .unwrap();
        let target = u16::from_le_bytes([bytes[zero_page_branch + 3], bytes[zero_page_branch + 4]]);
        let target_offset = usize::from(target - address);

        assert_eq!(
            &bytes[target_offset..target_offset + 10],
            &[
                0x48,
                0xA9,
                mapper_register,
                0x20,
                0x58,
                0xFA,
                0x68,
                0x8D,
                0x01,
                0x80
            ]
        );
    }
}

#[test]
fn composition_report_omits_translation_content_and_private_paths() {
    let report = BattleCompositionRuntimeReport {
        schema: 4,
        source_sha1: EXPECTED_SOURCE_SHA1,
        base_report_sha1: "base-report".to_owned(),
        base_output_sha1: "base".to_owned(),
        temporal_manifest_sha1: "temporal".to_owned(),
        output_sha1: "output".to_owned(),
        output_mapper: OUTPUT_MAPPER,
        prg_size: EXPANDED_PRG_SIZE,
        chr_size: 0,
        observed_runtime_tuple_count: 5,
        runtime_field_count: 9,
        maximum_observed_unique_overlay_count: 88,
        maximum_observed_raw_glyph_reference_count: 100,
        source_page_ppu_write_count: FONT_PAGE_SIZE,
        maximum_observed_overlay_ppu_write_count: 1600,
        maximum_observed_total_ppu_write_count: 5696,
        glyph_atlas_mmc3_page: GLYPH_ATLAS_MMC3_PAGE,
        source_and_recipe_mmc3_page: SOURCE_PAGE_MMC3_PAGE,
        atlas_cpu_address_hex: "0x8000".to_owned(),
        canonical_code_table_cpu_address_hex: "0x9400".to_owned(),
        source_page_cpu_address_hex: "0xA000".to_owned(),
        recipe_blob_cpu_address_hex: "0xB000".to_owned(),
        fixed_cave_start_cpu_address_hex: "0xFAF3".to_owned(),
        fixed_cave_end_cpu_address_exclusive_hex: "0xFFA0".to_owned(),
        fixed_cave_byte_count: 1197,
        post_data_cave_start_cpu_address_hex: "0xFFA8".to_owned(),
        post_data_cave_end_cpu_address_exclusive_hex: "0xFFC0".to_owned(),
        post_data_cave_byte_count: 24,
        fixed_runtime_routine_count: 16,
        fixed_runtime_routine_byte_count: 700,
        material_runtime_start_cpu_address_hex: "0x95C0".to_owned(),
        material_runtime_end_cpu_address_exclusive_hex: "0x986A".to_owned(),
        material_runtime_routine_count: 7,
        material_runtime_routine_byte_count: 300,
        total_runtime_routine_byte_count: 1000,
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
        observed_tuple_gate_installed: false,
        modeled_runtime_inputs_enabled: true,
        selected_color_bitmap_address_hex: "0x07C4".to_owned(),
        selected_color_bitmap_byte_count: 27,
        remap_state_address_hex: "0x07FE".to_owned(),
        remap_pair_table_address_hex: "0x07E0".to_owned(),
        maximum_remap_pair_count: 8,
        remap_overflow_aborts_composition: true,
        shared_text_projection_hook_address_hex: "0xE57F".to_owned(),
        shared_text_projection_installed: true,
        battle_initializer_hook_count: BATTLE_COMPOSITION_LIFETIME_START_WRITES.len(),
        battle_initializers_reopen_composition: true,
        sound_test_battle_initializer_hook_address_hex: "0x07:0xAC17".to_owned(),
        sound_test_shared_battle_activation_installed: true,
        sound_test_battle_recomposition_boundary_installed: true,
        battle_zero_right_page_uses_chr_ram_after_success: true,
        central_battle_resupply_keeps_composed_page: true,
        non_battle_right_pages_use_natural_selection: true,
        dynamic_assignment_source_contract_complete: true,
        hp_bar_queue_publishes_emitted_payload_length: true,
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
