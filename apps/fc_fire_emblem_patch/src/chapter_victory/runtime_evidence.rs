use serde::Serialize;

#[derive(Debug, Serialize)]
pub(super) struct ContinuousVictoryRuntimeEvidence {
    evidence_kind: &'static str,
    progression_interventions: &'static [&'static str],
    direct_coordinate_write_used: bool,
    direct_main_state_write_used: bool,
    direct_victory_stage_write_used: bool,
    active_enemy_record_rule: ActiveEnemyRecordRule,
    initial_castle_occupants: &'static [CastleOccupant],
    castle_opening: CastleOpening,
    castle_command_menu: CastleCommandMenu,
    epilogue_page_variants: &'static [EpiloguePageVariant],
    screen_sequence: Vec<RuntimeScreenLifetime>,
    continuous_gate_closed: bool,
    proof_limit: &'static str,
}

impl ContinuousVictoryRuntimeEvidence {
    pub(super) fn screen_count(&self) -> usize {
        self.screen_sequence.len()
    }

    pub(super) fn continuous_gate_closed(&self) -> bool {
        self.continuous_gate_closed
    }
}

#[derive(Debug, Serialize)]
struct ActiveEnemyRecordRule {
    action_byte_offset: u8,
    inactive_action_value: u8,
    inactive_action_value_hex: &'static str,
    interpretation: &'static str,
}

#[derive(Debug, Serialize)]
struct CastleOccupant {
    slot: u8,
    unit_id: u8,
    unit_id_hex: &'static str,
    row: u8,
    column: u8,
    activity_evidence: &'static str,
}

#[derive(Debug, Serialize)]
struct CastleOpening {
    opening_cause: &'static str,
    moved_enemy_slot: u8,
    enemy_start_row: u8,
    enemy_start_column: u8,
    enemy_observed_row: u8,
    enemy_observed_column: u8,
    freed_castle_row: u8,
    freed_castle_column: u8,
    marth_final_row: u8,
    marth_final_column: u8,
}

#[derive(Debug, Serialize)]
struct CastleCommandMenu {
    main_state: u8,
    main_state_hex: &'static str,
    outer_screen_state: u8,
    outer_screen_state_hex: &'static str,
    labels_in_visual_order: &'static [&'static str],
    selected_label: &'static str,
    raw_selected_value: u8,
    raw_selected_value_hex: &'static str,
    temporal_behavior: &'static str,
    confirmed_input_effect: &'static str,
}

#[derive(Debug, Serialize)]
struct EpiloguePageVariant {
    order: u8,
    portrait_visible: bool,
}

#[derive(Debug, Serialize)]
struct RuntimeScreenLifetime {
    sequence_order: u8,
    screen_role: &'static str,
    outer_screen_state: u8,
    outer_screen_state_hex: &'static str,
    main_state: u8,
    main_state_hex: &'static str,
    victory_stage: Option<u8>,
    dialogue_state: Option<u8>,
    chapter_number_one_based: Option<u8>,
    chr_pair: Option<ChrPair>,
    input_behavior: &'static str,
    translation_target: &'static str,
    preserved_original: &'static [&'static str],
    temporal_evidence: &'static str,
}

#[derive(Debug, Serialize)]
struct ChrPair {
    left_fd: u8,
    left_fe: u8,
    right_fd: u8,
    right_fe: u8,
}

const INITIAL_CASTLE_OCCUPANTS: &[CastleOccupant] = &[
    CastleOccupant {
        slot: 0,
        unit_id: 0x83,
        unit_id_hex: "0x83",
        row: 8,
        column: 5,
        activity_evidence: "action byte was not 0xFF",
    },
    CastleOccupant {
        slot: 6,
        unit_id: 0xBF,
        unit_id_hex: "0xBF",
        row: 8,
        column: 6,
        activity_evidence: "action byte was not 0xFF",
    },
];

const EPILOGUE_PAGE_VARIANTS: &[EpiloguePageVariant] = &[
    EpiloguePageVariant {
        order: 1,
        portrait_visible: true,
    },
    EpiloguePageVariant {
        order: 2,
        portrait_visible: true,
    },
    EpiloguePageVariant {
        order: 3,
        portrait_visible: false,
    },
    EpiloguePageVariant {
        order: 4,
        portrait_visible: true,
    },
];

pub(super) fn continuous_chapter_eleven_victory_evidence() -> ContinuousVictoryRuntimeEvidence {
    ContinuousVictoryRuntimeEvidence {
        evidence_kind: "accelerated ordinary-control reachability",
        progression_interventions: &[
            "Marth movement value was set to 15 before ordinary movement commands",
            "Marth current HP was restored between actual turns and combats",
            "the documented enemy-current-HP-one bundle was applied as one-time writes and rechecked because game processing can refresh those values",
        ],
        direct_coordinate_write_used: false,
        direct_main_state_write_used: false,
        direct_victory_stage_write_used: false,
        active_enemy_record_rule: ActiveEnemyRecordRule {
            action_byte_offset: 18,
            inactive_action_value: 0xFF,
            inactive_action_value_hex: "0xFF",
            interpretation: "unit ID or HP alone does not prove that an enemy remains active; action byte 0xFF marks an inactive or defeated record",
        },
        initial_castle_occupants: INITIAL_CASTLE_OCCUPANTS,
        castle_opening: CastleOpening {
            opening_cause: "enemy slot 6 moved under ordinary enemy-phase processing",
            moved_enemy_slot: 6,
            enemy_start_row: 8,
            enemy_start_column: 6,
            enemy_observed_row: 5,
            enemy_observed_column: 6,
            freed_castle_row: 8,
            freed_castle_column: 6,
            marth_final_row: 8,
            marth_final_column: 6,
        },
        castle_command_menu: CastleCommandMenu {
            main_state: 0x0F,
            main_state_hex: "0x0F",
            outer_screen_state: 0x0C,
            outer_screen_state_hex: "0x0C",
            labels_in_visual_order: &["こうげき", "しろ", "もちもの", "たいき"],
            selected_label: "しろ",
            raw_selected_value: 0x02,
            raw_selected_value_hex: "0x02",
            temporal_behavior: "irregular samples retained every label while only the selection cursor blinked",
            confirmed_input_effect: "A on しろ changed main state from 0x0F to 0x10 and entered the staged chapter-victory action",
        },
        epilogue_page_variants: EPILOGUE_PAGE_VARIANTS,
        screen_sequence: runtime_screen_sequence(),
        continuous_gate_closed: true,
        proof_limit: "proves the chapter-eleven castle-command through chapter-twelve-intro route with declared progression accelerations; alternate save choices are separately observed, but this route does not prove baseline difficulty, unaccelerated combat equivalence, defeat, or unfavorable branches",
    }
}

fn runtime_screen_sequence() -> Vec<RuntimeScreenLifetime> {
    vec![
        RuntimeScreenLifetime {
            sequence_order: 1,
            screen_role: "unit_command_menu_castle_variant",
            outer_screen_state: 0x0C,
            outer_screen_state_hex: "0x0C",
            main_state: 0x0F,
            main_state_hex: "0x0F",
            victory_stage: None,
            dialogue_state: None,
            chapter_number_one_based: Some(11),
            chr_pair: None,
            input_behavior: "input_wait",
            translation_target: "Japanese menu labels only",
            preserved_original: &[],
            temporal_evidence: "stable labels plus a flashing selection cursor",
        },
        RuntimeScreenLifetime {
            sequence_order: 2,
            screen_role: "chapter_clear_epilogue_dialogue",
            outer_screen_state: 0x0C,
            outer_screen_state_hex: "0x0C",
            main_state: 0x3C,
            main_state_hex: "0x3C",
            victory_stage: Some(0x02),
            dialogue_state: Some(0x0E),
            chapter_number_one_based: Some(11),
            chr_pair: Some(chr_pair(0x11, 0x11, 0x00, 0x18)),
            input_behavior: "mixed",
            translation_target: "Japanese dialogue only",
            preserved_original: &[],
            temporal_evidence: "four completed page variants waited for A; portrait visibility was true, true, false, true",
        },
        RuntimeScreenLifetime {
            sequence_order: 3,
            screen_role: "next_story_banner",
            outer_screen_state: 0x0D,
            outer_screen_state_hex: "0x0D",
            main_state: 0x03,
            main_state_hex: "0x03",
            victory_stage: Some(0x00),
            dialogue_state: Some(0x00),
            chapter_number_one_based: Some(11),
            chr_pair: Some(chr_pair(0x1B, 0x1B, 0x00, 0x18)),
            input_behavior: "input_wait",
            translation_target: "none",
            preserved_original: &["NEXT STORY"],
            temporal_evidence: "the same composition persisted for 1,200 input-free frames",
        },
        RuntimeScreenLifetime {
            sequence_order: 4,
            screen_role: "chapter_save_offer",
            outer_screen_state: 0x0D,
            outer_screen_state_hex: "0x0D",
            main_state: 0x07,
            main_state_hex: "0x07",
            victory_stage: None,
            dialogue_state: None,
            chapter_number_one_based: Some(11),
            chr_pair: Some(chr_pair(0x1B, 0x1B, 0x00, 0x18)),
            input_behavior: "input_wait",
            translation_target: "Japanese question and choices only",
            preserved_original: &[],
            temporal_evidence: "the default yes cursor blinked; A performed the persistent game save",
        },
        RuntimeScreenLifetime {
            sequence_order: 5,
            screen_role: "chapter_save_complete_continue_prompt",
            outer_screen_state: 0x0E,
            outer_screen_state_hex: "0x0E",
            main_state: 0x04,
            main_state_hex: "0x04",
            victory_stage: None,
            dialogue_state: Some(0x11),
            chapter_number_one_based: Some(11),
            chr_pair: Some(chr_pair(0x1C, 0x1C, 0x00, 0x18)),
            input_behavior: "input_wait",
            translation_target: "Japanese dialogue and choices only",
            preserved_original: &[],
            temporal_evidence: "A on the observed default yes choice started continuation",
        },
        RuntimeScreenLifetime {
            sequence_order: 6,
            screen_role: "chapter_transition_blackout",
            outer_screen_state: 0x09,
            outer_screen_state_hex: "0x09",
            main_state: 0x00,
            main_state_hex: "0x00",
            victory_stage: None,
            dialogue_state: None,
            chapter_number_one_based: None,
            chr_pair: Some(chr_pair(0x1A, 0x1A, 0x18, 0x18)),
            input_behavior: "automatic",
            translation_target: "none",
            preserved_original: &[],
            temporal_evidence: "the screen was fully black and advanced without input",
        },
        RuntimeScreenLifetime {
            sequence_order: 7,
            screen_role: "chapter_intro_title_dialogue_composite",
            outer_screen_state: 0x0B,
            outer_screen_state_hex: "0x0B",
            main_state: 0x00,
            main_state_hex: "0x00",
            victory_stage: None,
            dialogue_state: Some(0x0E),
            chapter_number_one_based: Some(12),
            chr_pair: Some(chr_pair(0x0F, 0x0F, 0x00, 0x18)),
            input_behavior: "mixed",
            translation_target: "Japanese chapter title and dialogue only",
            preserved_original: &["chapter-number digits"],
            temporal_evidence: "chapter-twelve title, dialogue, portrait, and completion marker shared one visible lifetime",
        },
    ]
}

const fn chr_pair(left_fd: u8, left_fe: u8, right_fd: u8, right_fe: u8) -> ChrPair {
    ChrPair {
        left_fd,
        left_fe,
        right_fd,
        right_fe,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn continuous_route_keeps_real_screen_lifetimes_in_order() {
        let evidence = continuous_chapter_eleven_victory_evidence();
        assert_eq!(
            evidence
                .screen_sequence
                .iter()
                .map(|screen| screen.screen_role)
                .collect::<Vec<_>>(),
            [
                "unit_command_menu_castle_variant",
                "chapter_clear_epilogue_dialogue",
                "next_story_banner",
                "chapter_save_offer",
                "chapter_save_complete_continue_prompt",
                "chapter_transition_blackout",
                "chapter_intro_title_dialogue_composite",
            ]
        );
        assert!(evidence.continuous_gate_closed);
    }

    #[test]
    fn route_proof_rejects_direct_shortcuts_and_preserves_only_observed_english() {
        let evidence = continuous_chapter_eleven_victory_evidence();
        assert!(!evidence.direct_coordinate_write_used);
        assert!(!evidence.direct_main_state_write_used);
        assert!(!evidence.direct_victory_stage_write_used);

        let preserved = evidence
            .screen_sequence
            .iter()
            .flat_map(|screen| screen.preserved_original.iter().copied())
            .collect::<Vec<_>>();
        assert_eq!(preserved, ["NEXT STORY", "chapter-number digits"]);
    }

    #[test]
    fn epilogue_variants_and_chapter_twelve_chr_pair_are_bound() {
        let evidence = continuous_chapter_eleven_victory_evidence();
        assert_eq!(
            evidence
                .epilogue_page_variants
                .iter()
                .map(|page| page.portrait_visible)
                .collect::<Vec<_>>(),
            [true, true, false, true]
        );

        let chapter_twelve = evidence.screen_sequence.last().unwrap();
        let pair = chapter_twelve.chr_pair.as_ref().unwrap();
        assert_eq!(chapter_twelve.chapter_number_one_based, Some(12));
        assert_eq!(
            [pair.left_fd, pair.left_fe, pair.right_fd, pair.right_fe],
            [0x0F, 0x0F, 0x00, 0x18]
        );
    }
}
