use super::*;

#[test]
fn registry_covers_every_observed_chr_pair() {
    let report = build_report(REGISTRY_JSON, OBSERVED_CHR_PAIRS).unwrap();

    assert_eq!(report.screen_count, 45);
    assert_eq!(report.unpartitioned_surface_family_count, 0);
    assert_eq!(report.runtime_observed_screen_count, 45);
    assert_eq!(report.chr_pair_observed_screen_count, 42);
    assert_eq!(report.mixed_original_latin_screen_count, 19);
    assert_eq!(report.preserved_original_only_screen_count, 5);
    assert_eq!(report.page_switch_verified_screen_count, 1);
    assert_eq!(report.mixed_text_page_verified_screen_count, 1);
    assert!(report.unresolved_surface_families.is_empty());
    assert!(report.unpartitioned_surface_families.is_empty());
    assert!(report.screens.iter().any(|screen| {
        screen.screen_role == "ending_character_epilogue"
            && screen.runtime_observed
            && screen.input_behavior == InputBehavior::Automatic
    }));
}

#[test]
fn command_menu_keeps_remaining_labels_and_actions_as_open_work() {
    let report = build_report(REGISTRY_JSON, OBSERVED_CHR_PAIRS).unwrap();
    let command_menu = report
        .screens
        .iter()
        .find(|screen| screen.screen_role == "unit_command_menu")
        .unwrap();

    assert_eq!(command_menu.screen_role, "unit_command_menu");
    assert!(command_menu.runtime_observed);
    assert!(command_menu.chr_pair_observed);
    assert_eq!(
        command_menu.translation_scope,
        TranslationScope::JapaneseOnly
    );
    assert!(command_menu.next_gate.contains("expected state effect"));
    assert!(
        command_menu
            .unresolved_focus
            .iter()
            .any(|focus| focus.contains("remaining 9 command labels"))
    );
    assert!(
        !command_menu
            .unresolved_focus
            .iter()
            .any(|focus| focus.contains("00/19"))
    );
    assert!(
        command_menu
            .known_focus
            .iter()
            .any(|focus| focus.contains("00/19"))
    );
    assert!(
        command_menu
            .known_focus
            .iter()
            .any(|focus| focus.contains("C9C2") && focus.contains("00/15"))
    );
    assert!(
        command_menu
            .known_focus
            .iter()
            .any(|focus| focus.contains("こうげき") && focus.contains("しろ"))
    );
}

#[test]
fn next_observation_gate_reuses_real_screen_roles_without_becoming_a_screen() {
    let report = build_report(REGISTRY_JSON, OBSERVED_CHR_PAIRS).unwrap();

    assert_eq!(
        report.next_observation_gate.gate_role,
        "ending_character_epilogue_variant_union"
    );
    assert_eq!(
        report.next_observation_gate.gate_kind,
        ObservationGateKind::ScreenSequence
    );
    assert_eq!(
        report.next_observation_gate.focus_screen_roles,
        ["ending_character_epilogue"]
    );
    assert!(
        report
            .next_observation_gate
            .focus_screen_roles
            .iter()
            .all(|role| report
                .screens
                .iter()
                .any(|screen| &screen.screen_role == role))
    );
    assert!(
        !report
            .screens
            .iter()
            .any(|screen| { screen.screen_role == report.next_observation_gate.gate_role })
    );
}

#[test]
fn observation_gate_cannot_masquerade_as_a_screen_role() {
    let invalid_registry = REGISTRY_JSON.replacen(
        "\"gate_role\": \"ending_character_epilogue_variant_union\"",
        "\"gate_role\": \"title\"",
        1,
    );

    let error = build_report(&invalid_registry, OBSERVED_CHR_PAIRS)
        .unwrap_err()
        .to_string();
    assert!(error.contains("must not masquerade as a screen role"));
}

#[test]
fn ending_lifetimes_keep_translation_scopes_and_static_terminal_distinct() {
    let report = build_report(REGISTRY_JSON, OBSERVED_CHR_PAIRS).unwrap();
    let ending = report
        .screens
        .iter()
        .filter(|screen| screen.surface_family == "ending")
        .collect::<Vec<_>>();

    assert_eq!(ending.len(), 5);
    assert!(ending.iter().any(|screen| {
        screen.screen_role == "ending_opening_and_cast_scroll"
            && screen.translation_scope == TranslationScope::PreservedOriginalOnly
    }));
    assert!(ending.iter().any(|screen| {
        screen.screen_role == "ending_chapter_record_scroll"
            && screen.translation_scope == TranslationScope::JapaneseWithPreservedOriginalLatin
    }));
    assert!(ending.iter().any(|screen| {
        screen.screen_role == "ending_staff_credits"
            && screen.translation_scope == TranslationScope::PreservedOriginalOnly
    }));
    assert!(ending.iter().any(|screen| {
        screen.screen_role == "ending_character_epilogue"
            && screen.translation_scope == TranslationScope::JapaneseOnly
    }));
    assert!(ending.iter().any(|screen| {
        screen.screen_role == "ending_final_signature"
            && screen
                .temporal_behavior
                .contains("keeps the original signature")
            && screen.unresolved_focus.is_empty()
    }));
}

#[test]
fn chapter_transition_screens_keep_distinct_lifetimes_and_translation_scopes() {
    let report = build_report(REGISTRY_JSON, OBSERVED_CHR_PAIRS).unwrap();
    let chapter_screens = report
        .screens
        .iter()
        .filter(|screen| screen.surface_family == "chapter_transition")
        .collect::<Vec<_>>();

    assert_eq!(chapter_screens.len(), 8);
    for role in [
        "chapter_clear_epilogue_dialogue",
        "next_story_banner",
        "chapter_save_offer",
        "chapter_save_complete_continue_prompt",
        "chapter_save_complete_power_off_notice",
        "sound_test",
        "chapter_transition_blackout",
        "chapter_intro_title_dialogue_composite",
    ] {
        assert!(
            chapter_screens
                .iter()
                .any(|screen| screen.screen_role == role && screen.runtime_observed)
        );
    }
    let next_story = chapter_screens
        .iter()
        .find(|screen| screen.screen_role == "next_story_banner")
        .unwrap();
    assert_eq!(
        next_story.translation_scope,
        TranslationScope::PreservedOriginalOnly
    );
    assert!(
        chapter_screens
            .iter()
            .all(|screen| screen.chr_pair_observed)
    );
    let blackout = chapter_screens
        .iter()
        .find(|screen| screen.screen_role == "chapter_transition_blackout")
        .unwrap();
    assert_eq!(blackout.input_behavior, InputBehavior::Automatic);
    assert_eq!(blackout.translation_scope, TranslationScope::NoText);
    assert!(
        blackout
            .known_focus
            .iter()
            .any(|focus| focus.contains("outer state 01") && focus.contains("1B/1B"))
    );
    assert!(OBSERVED_CHR_PAIRS.iter().any(|pair| {
        pair.screen_role == "chapter_transition_blackout"
            && pair.pattern_window == PatternWindow::Left
            && pair.fd_source_page == 0x1B
            && pair.fe_source_page == 0x1B
    }));
    let save_offer = chapter_screens
        .iter()
        .find(|screen| screen.screen_role == "chapter_save_offer")
        .unwrap();
    assert!(
        save_offer
            .known_focus
            .iter()
            .any(|focus| focus.contains("7FF4") && focus.contains("01 to 02"))
    );
    assert!(
        save_offer
            .unresolved_focus
            .iter()
            .all(|focus| !focus.contains("no-choice"))
    );
    let save_complete = chapter_screens
        .iter()
        .find(|screen| screen.screen_role == "chapter_save_complete_continue_prompt")
        .unwrap();
    assert!(
        save_complete
            .unresolved_focus
            .iter()
            .all(|focus| !focus.contains("no-choice"))
    );
    let power_off_notice = chapter_screens
        .iter()
        .find(|screen| screen.screen_role == "chapter_save_complete_power_off_notice")
        .unwrap();
    assert_eq!(
        power_off_notice.input_behavior,
        InputBehavior::TerminalInstruction
    );
    let sound_test = chapter_screens
        .iter()
        .find(|screen| screen.screen_role == "sound_test")
        .unwrap();
    assert_eq!(
        sound_test.translation_scope,
        TranslationScope::PreservedOriginalOnly
    );
    let intro = chapter_screens
        .iter()
        .find(|screen| screen.screen_role == "chapter_intro_title_dialogue_composite")
        .unwrap();
    assert!(intro.chr_pair_observed);
    assert_eq!(
        intro.translation_scope,
        TranslationScope::JapaneseWithPreservedOriginalLatin
    );
}

#[test]
fn item_action_results_remain_distinct_screen_roles() {
    let report = build_report(REGISTRY_JSON, OBSERVED_CHR_PAIRS).unwrap();
    let item_screens = report
        .screens
        .iter()
        .filter(|screen| screen.surface_family == "item")
        .collect::<Vec<_>>();

    assert_eq!(item_screens.len(), 7);
    assert!(
        item_screens
            .iter()
            .all(|screen| screen.screen_role != "item_action_result")
    );
    for role in [
        "item_equip_result",
        "item_use_result",
        "item_transfer_target_selection",
        "item_transfer_result",
        "item_discard_result",
    ] {
        assert!(
            item_screens
                .iter()
                .any(|screen| screen.screen_role == role && screen.runtime_observed)
        );
    }
}

#[test]
fn observed_shop_screens_keep_japanese_and_preserved_latin_separate() {
    let report = build_report(REGISTRY_JSON, OBSERVED_CHR_PAIRS).unwrap();
    let shop_screens = report
        .screens
        .iter()
        .filter(|screen| screen.surface_family == "weapon_shop")
        .collect::<Vec<_>>();

    assert_eq!(shop_screens.len(), 9);
    assert_eq!(
        shop_screens
            .iter()
            .filter(|screen| screen.runtime_observed)
            .count(),
        9
    );
    assert!(shop_screens.iter().all(|screen| screen.chr_pair_observed));
    assert_eq!(
        shop_screens
            .iter()
            .filter(|screen| {
                screen.translation_scope == TranslationScope::JapaneseWithPreservedOriginalLatin
            })
            .count(),
        8
    );
    let declined_prompt = shop_screens
        .iter()
        .find(|screen| screen.screen_role == "weapon_shop_declined_continue_prompt")
        .unwrap();
    assert_eq!(
        declined_prompt.translation_scope,
        TranslationScope::JapaneseOnly
    );
}

#[test]
fn unknown_chr_pair_role_is_rejected() {
    let unknown = [ObservedChrPair::new("unknown", PatternWindow::Right, 0, 0)];

    assert!(build_report(REGISTRY_JSON, &unknown).is_err());
}
