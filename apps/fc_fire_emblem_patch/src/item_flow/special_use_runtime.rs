use serde::Serialize;

#[derive(Debug, Serialize)]
pub(super) struct SpecialUseRuntimeObservation {
    effect_family: &'static str,
    item_id: u8,
    item_id_hex: &'static str,
    runtime_artifact_role: &'static str,
    proof_boundary: &'static str,
    initial_dialogue_index: u8,
    initial_dialogue_index_hex: &'static str,
    outer_result_states: &'static [u8],
    outer_result_states_hex: &'static [&'static str],
    visible_lifetimes: &'static [VisibleLifetime],
    input_boundaries: &'static [InputBoundary],
    mutation_observation: &'static str,
    phase_screenshot_sha256: &'static [&'static str],
}

#[derive(Debug, Serialize)]
struct VisibleLifetime {
    screen_role: &'static str,
    outer_result_states: &'static [u8],
    left_chr_pair: &'static str,
    right_chr_pair: &'static str,
    translation_consumers: &'static [&'static str],
    preserved_original: &'static [&'static str],
    temporal_observation: &'static str,
}

#[derive(Debug, Serialize)]
struct InputBoundary {
    condition: &'static str,
    input: &'static str,
    effect: &'static str,
    input_before_condition: &'static str,
}

pub(super) fn observations() -> Vec<SpecialUseRuntimeObservation> {
    vec![class_change(), earth_orb()]
}

fn class_change() -> SpecialUseRuntimeObservation {
    SpecialUseRuntimeObservation {
        effect_family: "class_change",
        item_id: 0x50,
        item_id_hex: "0x50",
        runtime_artifact_role: "mapper-165 parity probe preserving the source consumer path; not final cumulative Korean rendering proof",
        proof_boundary: "forced level and item setup proves consumer reachability only, not natural acquisition or progression",
        initial_dialogue_index: 0x1A,
        initial_dialogue_index_hex: "0x1A",
        outer_result_states: &[0x01, 0x04, 0x05, 0x06],
        outer_result_states_hex: &["0x01", "0x04", "0x05", "0x06"],
        visible_lifetimes: &[
            VisibleLifetime {
                screen_role: "item_use_result",
                outer_result_states: &[0x01, 0x04],
                left_chr_pair: "1A/1A",
                right_chr_pair: "00/15",
                translation_consumers: &["initial use dialogue", "unit name", "selected item name"],
                preserved_original: &[],
                temporal_observation: "the initial use sentence completed before the class-change route replaced the map-text lifetime with the shared battle presentation",
            },
            VisibleLifetime {
                screen_role: "battle_animation",
                outer_result_states: &[0x05],
                left_chr_pair: "1A/1A",
                right_chr_pair: "00/00",
                translation_consumers: &[
                    "battle unit name",
                    "source class name",
                    "target class name",
                    "equipment-empty label",
                    "class-change battle dialogue",
                ],
                preserved_original: &["LV", "HIT", "digits"],
                temporal_observation: "irregular samples covered panel construction, blinking sprite phases, source and target class panels, and the completed class-change sentence",
            },
        ],
        input_boundaries: &[InputBoundary {
            condition: "outer result state 0x05 with nested battle-dialogue state 0x04, decode activity 0x76ED equal to zero, and published-row flag 0x794A nonzero",
            input: "A",
            effect: "acknowledge the completed class-change sentence; shared battle cleanup then clears 0x05ED and outer state 0x06 restores the map",
            input_before_condition: "not permitted while the class-change presentation or nested sentence is still being constructed",
        }],
        mutation_observation: "class changed from 0x01 to 0x04, level reset to 0x01, and the selected class-change item was consumed before the shared battle presentation",
        phase_screenshot_sha256: &[
            "cb0a5f65468aeda0e87fd4340c1493c3b183310e79788d65947cfeb947209e39",
            "3e9bfc934ccbd149e715fb4db7625bf214d577ffb60ae3bed24868b525e033f8",
            "cc6f604d51121f3699b3436df05666c7b3876ced157862f296f8a6213d4b8fb4",
            "aafb9808c357b3fdf9903893fa22bedce5c8e0953d62309206965a05ecc57f8f",
            "1cdaea0f8280d7a8720f6439fc04e534ccf552cfaf888985ad1d4814b5570c13",
            "53565405eeb71f2e6ad889b99eba9321fa218ea073aaf197648064fc199ca140",
            "c0483796c1689f9783d962261f1a7a3a49224c9f1f3cd720f9e9ba80b41d913b",
            "2f86dd272224ae2b5653a7ad847a3ca6f74cbea52d97eb8fd14f82fd3085fb8c",
        ],
    }
}

fn earth_orb() -> SpecialUseRuntimeObservation {
    SpecialUseRuntimeObservation {
        effect_family: "earth_orb",
        item_id: 0x55,
        item_id_hex: "0x55",
        runtime_artifact_role: "mapper-165 parity probe preserving the source consumer path; not final cumulative Korean rendering proof",
        proof_boundary: "forced item setup proves consumer reachability only, not natural acquisition or progression",
        initial_dialogue_index: 0x1A,
        initial_dialogue_index_hex: "0x1A",
        outer_result_states: &[0x01, 0x02, 0x03],
        outer_result_states_hex: &["0x01", "0x02", "0x03"],
        visible_lifetimes: &[VisibleLifetime {
            screen_role: "item_use_result",
            outer_result_states: &[0x01, 0x02, 0x03],
            left_chr_pair: "1A/1A",
            right_chr_pair: "00/15",
            translation_consumers: &[
                "initial use dialogue",
                "unit name",
                "selected item name",
                "final result dialogue 0x33",
            ],
            preserved_original: &[],
            temporal_observation: "the use sentence and CHR stayed resident through every value of the 32-step counter; no intermediate text appeared, and result 0x33 then replaced the sentence in the same lifetime",
        }],
        input_boundaries: &[InputBoundary {
            condition: "outer result state 0x03 after the 32-step counter reached zero and result dialogue 0x33 completed",
            input: "A",
            effect: "dismiss the completed result and finish the unit action",
            input_before_condition: "not permitted during outer result state 0x02 or while result 0x33 is still being constructed",
        }],
        mutation_observation: "the 32-step effect completed, result 0x33 was selected, and the selected item's durability decreased from 3 to 2",
        phase_screenshot_sha256: &[
            "7eb8be69817fac6c671eddf0094591478be3f44d223e302824f26ae68a113501",
            "8b10819d2e594f0aad1d18467b837671cf8ab7ee6a2df1f019dd2b278c36a7e4",
            "d2625e4c25e1611e73e7ecee8684b032f1bcfa1a43a87beb47b2a18344c9fd80",
            "4e1f06c9599441f6b847ab68a87b938478b7927fdab2ca8e0482901f4184423d",
            "eafff7c9854e4674ecd4c8e28a4c3375ccabc9d883eaa2bb8ebd53e2f94d7871",
            "824ec19a4151aac6185fa95f161acffae1491318c6730d74296ba931a576d119",
            "c0060eca8315b24c224ea9cd9d3d835a702d9214a6a00f3402e82bc83362fbf1",
        ],
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn special_routes_keep_distinct_visible_lifetimes_and_input_boundaries() {
        let observations = observations();
        assert_eq!(observations.len(), 2);
        assert_eq!(observations[0].visible_lifetimes.len(), 2);
        assert_eq!(
            observations[0].visible_lifetimes[1].screen_role,
            "battle_animation"
        );
        assert_eq!(observations[1].visible_lifetimes.len(), 1);
        assert_eq!(
            observations[1].visible_lifetimes[0].screen_role,
            "item_use_result"
        );
        assert!(observations.iter().all(|observation| {
            observation.input_boundaries.len() == 1 && observation.input_boundaries[0].input == "A"
        }));
    }
}
