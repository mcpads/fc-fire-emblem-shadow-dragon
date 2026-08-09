use super::*;

#[test]
fn chapter_title_table_includes_the_twenty_fifth_pointer() {
    assert_eq!(CHAPTER_TITLE_POINTER_TABLE_BYTES.len(), 50);
    let pointers = CHAPTER_TITLE_POINTER_TABLE_BYTES
        .chunks_exact(2)
        .map(|bytes| u16::from_le_bytes([bytes[0], bytes[1]]))
        .collect::<Vec<_>>();

    assert_eq!(pointers.len(), 25);
    assert_eq!(pointers.first(), Some(&0xEE3A));
    assert_eq!(pointers.last(), Some(&0xEFA8));
    assert!(pointers.windows(2).all(|pair| pair[0] < pair[1]));
}

#[test]
fn transition_routes_separate_each_observed_screen_lifetime() {
    let screens = transition_screens();
    let roles = screens
        .iter()
        .map(|screen| screen.screen_role)
        .collect::<Vec<_>>();

    assert_eq!(
        roles,
        [
            "chapter_clear_epilogue_dialogue",
            "next_story_banner",
            "chapter_save_offer",
            "chapter_save_complete_continue_prompt",
            "chapter_save_complete_power_off_notice",
            "sound_test",
            "chapter_transition_blackout",
            "chapter_intro_title_dialogue_composite",
        ]
    );
    assert!(screens.iter().all(|screen| screen.runtime_observed));
    assert_eq!(screens[1].translation_target, "none");
    assert_eq!(screens[1].preserved_original, ["NEXT STORY"]);
    assert!(
        screens
            .iter()
            .all(|screen| !screen.focus_elements.is_empty())
    );
    assert!(
        screens[2]
            .input_actions
            .iter()
            .any(|action| action.may_cause_persistent_gameplay_mutation)
    );
    assert!(screens[2].input_actions.iter().any(|action| {
        action.input.contains("no choice")
            && !action.may_cause_persistent_gameplay_mutation
            && action.next_role == "chapter_transition_blackout"
    }));
    assert_eq!(
        [
            screens[3].observed_chr_pair.left_fd,
            screens[3].observed_chr_pair.left_fe,
            screens[3].observed_chr_pair.right_fd,
            screens[3].observed_chr_pair.right_fe,
        ],
        [0x1C, 0x1C, 0x00, 0x18]
    );
    let power_off_notice = screens
        .iter()
        .find(|screen| screen.screen_role == "chapter_save_complete_power_off_notice")
        .unwrap();
    assert!(
        power_off_notice
            .input_actions
            .iter()
            .any(|action| action.next_role == "sound_test")
    );
    let sound_test = screens
        .iter()
        .find(|screen| screen.screen_role == "sound_test")
        .unwrap();
    assert_eq!(sound_test.translation_target, "none");
    assert_eq!(
        sound_test.preserved_original,
        ["all English labels", "digits"]
    );
    let blackout = screens
        .iter()
        .find(|screen| screen.screen_role == "chapter_transition_blackout")
        .unwrap();
    assert_eq!(blackout.input_behavior, "automatic");
}

#[test]
fn fixed_label_indices_match_their_pointer_table_cells() {
    let pointer_table_address = 0x8FC2_u16;

    assert_eq!(pointer_table_address + 2 * 0x3E, 0x903E);
    assert_eq!(pointer_table_address + 2 * 0x32, 0x9026);
    assert_eq!(u16::from_le_bytes([0xFB, 0x91]), 0x91FB);
    assert_eq!(u16::from_le_bytes([0xAA, 0x91]), 0x91AA);
}

#[test]
fn source_region_addresses_map_to_the_verified_file_offsets() {
    assert_eq!(source_file_offset(0x0B, 0x886A).unwrap(), 0x2C87A);
    assert_eq!(source_file_offset(0x0B, 0x88C4).unwrap(), 0x2C8D4);
    assert_eq!(source_file_offset(0x0B, 0x8AE6).unwrap(), 0x2CAF6);
    assert_eq!(source_file_offset(0x0B, 0x9AD0).unwrap(), 0x2DAE0);
    assert_eq!(source_file_offset(0x0B, 0x9D52).unwrap(), 0x2DD62);
    assert_eq!(source_file_offset(0x0B, 0x9FA8).unwrap(), 0x2DFB8);
    assert_eq!(source_file_offset(0x06, 0x8400).unwrap(), 0x18410);
    assert_eq!(source_file_offset(0x06, 0xB6F3).unwrap(), 0x1B703);
    assert_eq!(source_file_offset(0x06, 0xB737).unwrap(), 0x1B747);
    assert_eq!(source_file_offset(0x0B, 0x9333).unwrap(), 0x2D343);
    assert_eq!(source_file_offset(0x06, 0xB771).unwrap(), 0x1B781);
    assert_eq!(source_file_offset(0x06, 0xB7CB).unwrap(), 0x1B7DB);
    assert_eq!(source_file_offset(0x0B, 0x995F).unwrap(), 0x2D96F);
    assert_eq!(source_file_offset(0x0B, 0x9B35).unwrap(), 0x2DB45);
    assert_eq!(source_file_offset(0x0B, 0x9BA0).unwrap(), 0x2DBB0);
    assert_eq!(source_file_offset(0x0B, 0x9BCF).unwrap(), 0x2DBDF);
    assert_eq!(source_file_offset(0x0B, 0x9C17).unwrap(), 0x2DC27);
    assert_eq!(source_file_offset(0x07, 0xAA2B).unwrap(), 0x1EA3B);
    assert_eq!(source_file_offset(0x04, 0x9EC6).unwrap(), 0x11ED6);
    assert_eq!(
        source_file_offset(0x0F, CHAPTER_TITLE_POINTER_TABLE_ADDRESS).unwrap(),
        0x3EE18
    );
}

#[test]
fn save_offer_no_choice_owns_a_distinct_close_and_blackout_route() {
    let contract = save_offer_no_branch_contract();

    assert_eq!(contract.offer_outer_screen_state, 0x0D);
    assert_eq!(contract.owned_main_state_sequence, [0x07, 0x08, 0x09, 0x00]);
    assert_eq!(contract.observed_menu_depth, 2);
    assert_eq!(contract.active_selection_address, 0x7FF4);
    assert_eq!(contract.no_selection, 2);
    assert_eq!(contract.no_committed_result, 2);
    assert_eq!(contract.no_branch_exit_outer_state, 1);
    assert!(!contract.persistent_save_route_entered);
    assert_eq!(contract.next_role, "chapter_transition_blackout");
    assert_eq!(
        [
            contract.no_branch_blackout_chr_pair.left_fd,
            contract.no_branch_blackout_chr_pair.left_fe,
            contract.no_branch_blackout_chr_pair.right_fd,
            contract.no_branch_blackout_chr_pair.right_fe,
        ],
        [0x1B, 0x1B, 0x18, 0x18]
    );
    assert_eq!(contract.stable_sample_offsets_frames.last(), Some(&565));
}

#[test]
fn save_complete_no_choice_owns_a_terminal_notice_and_sound_test_unlock() {
    let contract = save_complete_no_branch_contract();

    assert_eq!(contract.outer_screen_state, 0x0E);
    assert_eq!(contract.main_state, 0x04);
    assert_eq!(contract.owned_dialogue_substate_sequence, [7, 8, 9, 10]);
    assert_eq!(contract.observed_menu_depth, 3);
    assert_eq!(contract.active_selection_address, 0x7FF5);
    assert_eq!(contract.no_selection, 2);
    assert_eq!(contract.no_committed_result, 2);
    assert_eq!(contract.next_role, "chapter_save_complete_power_off_notice");
    assert_eq!(
        contract.hidden_unlock_inputs,
        ["up", "down", "left", "right", "up", "A"]
    );
    assert_eq!(contract.hidden_unlock_next_role, "sound_test");
    assert_eq!(
        contract.settled_notice_sample_offsets_frames,
        [130, 259, 516, 900]
    );
    assert_eq!(
        [
            contract.sound_test_chr_pair.left_fd,
            contract.sound_test_chr_pair.left_fe,
            contract.sound_test_chr_pair.right_fd,
            contract.sound_test_chr_pair.right_fe,
        ],
        [0x1C, 0x1C, 0x00, 0x18]
    );
}

#[test]
fn sound_test_controls_bind_two_runtime_partitioned_downstream_families() {
    let contract = sound_test_control_contract();

    assert_eq!(contract.sound_number_address, 0x775C);
    assert_eq!(contract.initial_sound_number, 0);
    assert_eq!(contract.upper_boundary, 0x50);
    assert_eq!(contract.sound_event_base_address, 0x06F0);
    assert_eq!(contract.sound_event_slot_count, 8);
    assert_eq!(contract.controls.len(), 6);
    for (input, mask) in [
        ("up", 0x08),
        ("down", 0x04),
        ("A", 0x80),
        ("B", 0x40),
        ("Start", 0x10),
        ("Select", 0x20),
    ] {
        assert!(
            contract
                .controls
                .iter()
                .any(|control| control.input == input && control.input_mask == mask)
        );
    }
    let battle_test = contract
        .downstream_families
        .iter()
        .find(|family| family.family_role == "battle_animation_test_sequence")
        .unwrap();
    assert_eq!(battle_test.entry_dialogue_substate, 0x0D);
    assert_eq!(battle_test.prg_bank, 0x07);
    assert_eq!(battle_test.bank_handler_index, 0x03);
    assert_eq!(battle_test.entry_point, 0xAA2B);
    assert_eq!(battle_test.phase_pointer_count, 6);
    assert!(battle_test.runtime_observed);
    assert_eq!(battle_test.visible_screen_roles, ["battle_animation"]);
    let ending = contract
        .downstream_families
        .iter()
        .find(|family| family.family_role == "ending_sequence")
        .unwrap();
    assert_eq!(ending.entry_dialogue_substate, 0x0E);
    assert_eq!(ending.prg_bank, 0x04);
    assert_eq!(ending.bank_handler_index, 0x04);
    assert_eq!(ending.entry_point, 0x9EC6);
    assert_eq!(ending.phase_pointer_count, 30);
    assert!(ending.runtime_observed);
    assert_eq!(
        ending.visible_screen_roles,
        [
            "ending_opening_and_cast_scroll",
            "ending_chapter_record_scroll",
            "ending_staff_credits",
            "ending_character_epilogue",
            "ending_final_signature",
        ]
    );
    assert!(contract.controls_runtime_observed);
}

#[test]
fn chapter_transition_code_regions_use_typed_rp2a03_decode() {
    for spec in SOURCE_REGIONS {
        if matches!(spec.kind, RegionKind::Code) {
            match spec.expectation {
                RegionExpectation::Bytes(bytes) => {
                    let instructions =
                        decode_rp2a03_sequence(bytes, spec.cpu_address, spec.role).unwrap();
                    assert!(
                        !instructions.is_empty(),
                        "{} has no instructions",
                        spec.role
                    );
                }
                RegionExpectation::Sha1 {
                    byte_count,
                    expected_sha1,
                } => {
                    assert!(byte_count != 0, "{} has an empty code range", spec.role);
                    assert_eq!(
                        expected_sha1.len(),
                        40,
                        "{} has no SHA-1 expectation",
                        spec.role
                    );
                }
            }
        }
    }
}

#[test]
fn later_intro_samples_keep_entry_methods_and_proof_limits_distinct() {
    let samples = chapter_intro_runtime_samples();
    let chapter_eleven = samples
        .iter()
        .find(|sample| sample.chapter_number_one_based == 11)
        .unwrap();
    let chapter_twelve = samples
        .iter()
        .find(|sample| sample.chapter_number_one_based == 12)
        .unwrap();

    assert_eq!(chapter_eleven.chapter_index_zero_based, 10);
    assert_eq!(
        [
            chapter_eleven.left_fd_chr_page,
            chapter_eleven.left_fe_chr_page,
            chapter_eleven.right_fd_chr_page,
            chapter_eleven.right_fe_chr_page,
        ],
        [0x1A, 0x1A, 0x00, 0x18]
    );
    assert!(chapter_eleven.proof_limit.contains("not chapter-ten"));
    assert!(!chapter_eleven.portrait_visible_in_sample);
    assert_eq!(chapter_twelve.chapter_index_zero_based, 11);
    assert!(chapter_twelve.portrait_visible_in_sample);
    assert_eq!(
        [
            chapter_twelve.left_fd_chr_page,
            chapter_twelve.left_fe_chr_page,
            chapter_twelve.right_fd_chr_page,
            chapter_twelve.right_fe_chr_page,
        ],
        [0x0F, 0x0F, 0x00, 0x18]
    );
    assert!(
        chapter_twelve
            .entry_method
            .contains("continuous chapter-eleven")
    );
    assert!(chapter_twelve.proof_limit.contains("baseline difficulty"));
    assert!(!regular_save_reachability().natural_progression_claimed);
}
