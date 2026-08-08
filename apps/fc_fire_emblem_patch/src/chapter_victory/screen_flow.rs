use serde::Serialize;

#[derive(Debug, Serialize)]
pub(super) struct VictoryRouteStep {
    order: u8,
    role: &'static str,
    entry_condition: &'static str,
    control: &'static str,
    focused_elements: &'static [&'static str],
    exit_condition: &'static str,
    proof_status: &'static str,
}

#[derive(Debug, Serialize)]
pub(super) struct RuntimeMapSample {
    sample_role: &'static str,
    outer_screen_state: u8,
    outer_screen_state_hex: &'static str,
    runtime_row_pointer_table: u16,
    runtime_row_pointer_table_hex: &'static str,
    runtime_row_zero_address: u16,
    runtime_row_zero_address_hex: &'static str,
    row_stride: u8,
    source_victory_coordinates: &'static [(u8, u8)],
    runtime_values_at_source_victory_coordinates: &'static [u8],
    runtime_values_hex: &'static [&'static str],
    interpretation: &'static str,
    proof_limit: &'static str,
}

#[derive(Debug, Serialize)]
pub(super) struct ObservationPlan {
    pub(super) next_gate: &'static str,
    allowed_progression: &'static [&'static str],
    screen_sampling: &'static [&'static str],
    forbidden_shortcuts: &'static [&'static str],
    later_failure_validation: &'static str,
}

pub(super) fn victory_route_steps() -> Vec<VictoryRouteStep> {
    vec![
        VictoryRouteStep {
            order: 1,
            role: "chapter_eleven_map",
            entry_condition: "chapter-eleven intro reaches its terminal page and exits automatically after the final state-coupled A",
            control: "player-controlled map",
            focused_elements: &[
                "Marth and blocker coordinates",
                "two source castle tiles at row 8 columns 5 and 6",
                "terrain restored after unit occupancy changes",
            ],
            exit_condition: "Marth reaches either source castle coordinate through ordinary movement and opens the unit command menu",
            proof_status: "observed through ordinary movement, turn endings, and combat with declared movement and HP accelerations; no coordinate write was used",
        },
        VictoryRouteStep {
            order: 2,
            role: "unit_command_menu_castle_variant",
            entry_condition: "unit id 0x01 stands on source tile 0x4B and the unit command menu is composed",
            control: "input wait",
            focused_elements: &[
                "Japanese しろ label",
                "selection cursor flashing phase union",
                "stable menu rows",
                "main state 0x0F before selection",
            ],
            exit_condition: "A on しろ changes the command result and main state through the bound command dispatcher",
            proof_status: "observed at row 8 column 6 with four stable labels, flashing cursor, raw selection 0x02, and main-state transition 0x0F to 0x10",
        },
        VictoryRouteStep {
            order: 3,
            role: "chapter_victory_action",
            entry_condition: "terrain command bit 3 selects handler 06:907B, tile 0x4B selects main state 0x3C, and 06:9390 calls the bank-03 victory routine",
            control: "automatic staged action after one consequential A",
            focused_elements: &[
                "outer screen state 0x0C",
                "victory stage 0x053E",
                "map and unit animation",
                "first visible dialogue or banner entry",
            ],
            exit_condition: "the staged handler reaches the first stable chapter-transition screen or an explicit input-wait state",
            proof_status: "static route and chapter-eleven runtime execution observed through main state 0x3C, victory stage 0x02, and the first completed epilogue page",
        },
        VictoryRouteStep {
            order: 4,
            role: "chapter_eleven_to_twelve_transition",
            entry_condition: "chapter victory action enters the transition sequence",
            control: "screen-specific automatic draw and input waits",
            focused_elements: &[
                "epilogue dialogue pages",
                "original English NEXT STORY",
                "save offer and cursor",
                "save-complete prompt",
                "chapter-twelve title and dialogue",
                "flashing-marker phase unions",
                "CHR pairs per screen lifetime",
            ],
            exit_condition: "chapter-twelve title and dialogue composite is visible with every intermediate screen and observed default-yes input effect recorded",
            proof_status: "continuous chapter-eleven route observed through four epilogue pages, NEXT STORY, save offer, save-complete prompt, automatic blackout, and chapter-twelve intro",
        },
    ]
}

pub(super) fn chapter_eleven_runtime_map_sample() -> RuntimeMapSample {
    RuntimeMapSample {
        sample_role: "chapter_eleven_map_after_intro",
        outer_screen_state: 0x0C,
        outer_screen_state_hex: "0x0C",
        runtime_row_pointer_table: 0xED3D,
        runtime_row_pointer_table_hex: "0xED3D",
        runtime_row_zero_address: 0x72AF,
        runtime_row_zero_address_hex: "0x72AF",
        row_stride: 32,
        source_victory_coordinates: &[(8, 5), (8, 6)],
        runtime_values_at_source_victory_coordinates: &[0x1B, 0x1B],
        runtime_values_hex: &["0x1B", "0x1B"],
        interpretation: "initial unit occupancy overlays both source castle tiles in the runtime map buffer; this is not evidence that the terrain or command is absent",
        proof_limit: "the values describe the initial original-Japanese map sample; later ordinary enemy processing moved slot 6 away from row 8 column 6 and the continuous route used that freed castle tile",
    }
}

pub(super) fn observation_plan() -> ObservationPlan {
    ObservationPlan {
        next_gate: "validate the save-complete no choice, remaining chapter variants, and failure paths as separate gates",
        allowed_progression: &[
            "inspect current unit and blocker positions before choosing a movement route",
            "use the already verified movement-range acceleration only when its effect is explicit",
            "resolve combat through the game action path and re-apply the complete documented enemy-HP bundle only when needed because one-time writes can be refreshed by game processing",
            "release A on the first bound state change and stop automatic execution at the next stable screen or input wait",
        ],
        screen_sampling: &[
            "capture the stable composition and several irregularly spaced frames",
            "union cursor, completion marker, sprite, and other flashing elements across samples",
            "record entry state, automatic draw states, input-wait state, consequential input, and exit state separately",
            "bind text, CHR pairs, command state, and outer screen state to the same screen lifetime",
        ],
        forbidden_shortcuts: &[
            "writing Marth coordinates directly",
            "writing terrain action state 0x3C directly",
            "writing victory stage 0x053E or outer screen state directly",
            "repeated or unsupervised button presses before the current screen reaction is known",
        ],
        later_failure_validation: "run defeat and unfavorable branches with progression cheats disabled or an explicitly adverse intervention; accelerated victory is reachability evidence, not failure-path proof",
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn keeps_the_actual_command_screen_separate_from_the_victory_route_gate() {
        let steps = victory_route_steps();
        assert_eq!(
            steps.iter().map(|step| step.role).collect::<Vec<_>>(),
            [
                "chapter_eleven_map",
                "unit_command_menu_castle_variant",
                "chapter_victory_action",
                "chapter_eleven_to_twelve_transition"
            ]
        );
        assert_eq!(steps[1].control, "input wait");
        assert_eq!(
            steps[2].control,
            "automatic staged action after one consequential A"
        );
    }

    #[test]
    fn forbids_state_and_coordinate_shortcuts_for_completion_proof() {
        let plan = observation_plan();
        assert!(
            plan.forbidden_shortcuts
                .iter()
                .any(|rule| rule.contains("coordinates"))
        );
        assert!(
            plan.forbidden_shortcuts
                .iter()
                .any(|rule| rule.contains("0x3C"))
        );
        assert!(plan.later_failure_validation.contains("disabled"));
    }

    #[test]
    fn runtime_overlay_sample_stays_an_initial_state_not_the_route_outcome() {
        let sample = chapter_eleven_runtime_map_sample();
        assert_eq!(sample.source_victory_coordinates, [(8, 5), (8, 6)]);
        assert_eq!(
            sample.runtime_values_at_source_victory_coordinates,
            [0x1B, 0x1B]
        );
        assert!(sample.proof_limit.contains("initial"));
        assert!(sample.proof_limit.contains("slot 6"));
        assert!(!sample.proof_limit.contains("no movement"));
    }
}
