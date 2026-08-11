use serde::Serialize;

use super::FixedLabelBinding;
use super::source_contract::{ITEM_ACTION_LABELS, ITEM_ACTION_RESULT_DIALOGUE_INDICES};

#[derive(Debug, Serialize)]
pub(super) struct ItemScreen {
    pub(super) screen_role: &'static str,
    runtime_observed: bool,
    input_behavior: &'static str,
    main_states: &'static [u8],
    composite_state: Option<u8>,
    translation_target: &'static str,
    preserved_original: &'static [&'static str],
    visible_components: &'static [&'static str],
    pub(super) input_actions: Vec<InputAction>,
    next_gate: &'static str,
}

#[derive(Debug, Serialize)]
pub(super) struct InputAction {
    pub(super) input: &'static str,
    immediate_effect: &'static str,
    pub(super) may_cause_persistent_gameplay_mutation: bool,
    pub(super) next_role: &'static str,
}

#[derive(Debug, Serialize)]
pub(super) struct ItemActionChoice {
    action_code: u8,
    action_code_hex: String,
    pub(super) label: FixedLabelBinding,
    pub(super) availability: &'static str,
    mutation_boundary: &'static str,
    pub(super) result_dialogue_index: u8,
    result_dialogue_index_hex: String,
    pub(super) return_route: &'static str,
    pub(super) next_role: &'static str,
}

pub(super) fn item_screens() -> Vec<ItemScreen> {
    vec![
        ItemScreen {
            screen_role: "item_inventory_list",
            runtime_observed: true,
            input_behavior: "input_wait",
            main_states: &[0x1B],
            composite_state: Some(0x07),
            translation_target: "Japanese item names only",
            preserved_original: &["durability digits", "NO ITEM"],
            visible_components: &[
                "one to four item-name and durability rows",
                "selection marker",
                "map background and unit sprites",
            ],
            input_actions: vec![
                InputAction {
                    input: "up or down",
                    immediate_effect: "change the selected item ordinal with wraparound",
                    may_cause_persistent_gameplay_mutation: false,
                    next_role: "item_inventory_list",
                },
                InputAction {
                    input: "A",
                    immediate_effect: "store the selected item and slot, evaluate action eligibility, and open the conditional action menu",
                    may_cause_persistent_gameplay_mutation: false,
                    next_role: "item_action_menu",
                },
                InputAction {
                    input: "B",
                    immediate_effect: "close the list and rebuild the unit command menu through main state 0x0E",
                    may_cause_persistent_gameplay_mutation: false,
                    next_role: "unit_command_menu",
                },
            ],
            next_gate: "observe an empty inventory separately before treating the preserved NO ITEM label as runtime-displayed",
        },
        ItemScreen {
            screen_role: "item_action_menu",
            runtime_observed: true,
            input_behavior: "input_wait",
            main_states: &[0x1C],
            composite_state: Some(0x09),
            translation_target: "conditional Japanese action labels",
            preserved_original: &["item durability digits in the retained parent list"],
            visible_components: &[
                "retained item list",
                "conditional action rows",
                "nested selection cursor",
                "map background and unit sprites",
            ],
            input_actions: vec![
                InputAction {
                    input: "A",
                    immediate_effect: "map the selected ordinal through the normalized availability mask to action code 0 through 3",
                    may_cause_persistent_gameplay_mutation: true,
                    next_role: "action-dependent item surface",
                },
                InputAction {
                    input: "B",
                    immediate_effect: "close only the nested action menu and rebuild the item list",
                    may_cause_persistent_gameplay_mutation: false,
                    next_role: "item_inventory_list",
                },
            ],
            next_gate: "exercise equip, use, give, and discard from reversible states only after each selected action and downstream screen are declared",
        },
        ItemScreen {
            screen_role: "item_transfer_target_selection",
            runtime_observed: true,
            input_behavior: "input_wait",
            main_states: &[0x1D],
            composite_state: None,
            translation_target: "none on the observed textless map overlay",
            preserved_original: &[],
            visible_components: &[
                "map background and unit sprites",
                "flashing candidate unit marker or sprite phase",
            ],
            input_actions: vec![
                InputAction {
                    input: "direction",
                    immediate_effect: "move within the eligible-recipient list and redraw the flashing candidate",
                    may_cause_persistent_gameplay_mutation: false,
                    next_role: "item_transfer_target_selection",
                },
                InputAction {
                    input: "A",
                    immediate_effect: "approve the candidate; remain with capacity feedback if all four target slots are full, otherwise copy the target record and move the selected item and durability",
                    may_cause_persistent_gameplay_mutation: true,
                    next_role: "item_transfer_result or item_transfer_target_selection",
                },
                InputAction {
                    input: "B",
                    immediate_effect: "cancel recipient selection and rebuild the item inventory list",
                    may_cause_persistent_gameplay_mutation: false,
                    next_role: "item_inventory_list",
                },
            ],
            next_gate: "observe a full-recipient capacity failure and multiple-candidate movement without treating a missing sprite in one flashing phase as a missing UI element",
        },
        ItemScreen {
            screen_role: "item_equip_result",
            runtime_observed: true,
            input_behavior: "input_wait",
            main_states: &[0x1E],
            composite_state: None,
            translation_target: "Japanese equip-result dialogue only",
            preserved_original: &[],
            visible_components: &["result dialogue window", "map background and unit sprites"],
            input_actions: vec![],
            next_gate: "bind dialogue dismissal and verify right 00/18 and 00/19 backing variants before choosing a font-page lifetime",
        },
        ItemScreen {
            screen_role: "item_use_result",
            runtime_observed: true,
            input_behavior: "mixed",
            main_states: &[0x1E],
            composite_state: None,
            translation_target: "Japanese use and item-effect result dialogue only",
            preserved_original: &["effect numbers", "existing Latin item text if present"],
            visible_components: &[
                "initial use dialogue",
                "item-family-specific effect or failure result",
                "map or target context",
            ],
            input_actions: vec![
                InputAction {
                    input: "A after progression state 3",
                    immediate_effect: "dismiss the completed result dialogue, restore main state 0x19, and complete the unit action",
                    may_cause_persistent_gameplay_mutation: true,
                    next_role: "map_idle",
                },
                InputAction {
                    input: "A after the successful class-change presentation reaches nested battle-dialogue state 0x04 with 0x76ED=0 and 0x794A nonzero",
                    immediate_effect: "acknowledge the completed class-change sentence; shared battle cleanup and outer result state 0x06 then return to the map automatically",
                    may_cause_persistent_gameplay_mutation: true,
                    next_role: "map_idle",
                },
            ],
            next_gate: "retain the source-enumerated 18-route capacity bound and regress the item_use_result and battle_animation lifetimes when their installed text changes",
        },
        ItemScreen {
            screen_role: "item_transfer_result",
            runtime_observed: true,
            input_behavior: "input_wait",
            main_states: &[0x1E],
            composite_state: None,
            translation_target: "Japanese transfer-result dialogue only",
            preserved_original: &[],
            visible_components: &["result dialogue window", "map background and unit sprites"],
            input_actions: vec![],
            next_gate: "observe the full-recipient capacity failure separately and bind dialogue dismissal and font-page re-entry",
        },
        ItemScreen {
            screen_role: "item_discard_result",
            runtime_observed: true,
            input_behavior: "input_wait",
            main_states: &[0x1E],
            composite_state: None,
            translation_target: "Japanese discard-result dialogue only",
            preserved_original: &[],
            visible_components: &["result dialogue window", "map background and unit sprites"],
            input_actions: vec![],
            next_gate: "bind dialogue dismissal and verify the empty-after-discard return-state variant separately",
        },
    ]
}

pub(super) fn action_choices(labels: &[FixedLabelBinding]) -> Vec<ItemActionChoice> {
    let specs = [
        (
            0,
            "item eligibility helper leaves carry set",
            "swap the selected item and durability with record offsets 0x13 and 0x17 at 06:A5CE",
            "return state 0x19 after the shared dialogue wait",
            "item_equip_result",
        ),
        (
            1,
            "item flag byte at 0xD9C3[item-1] has bit 0x40 set",
            "state 0x1E first opens dialogue index 0x1A, then progression state 2 selects the item-specific effect; positive effects can decrement durability or clear an exhausted item while a no-effect result can preserve the use count",
            "return state 0x19 after item-specific progression completes",
            "item_use_result",
        ),
        (
            2,
            "recipient scan has made 0x7750 nonzero",
            "after recipient approval, 06:952A moves item and durability to the target buffer and clears the source slot",
            "return state 0x1A when source inventory remains nonempty, otherwise 0x19",
            "item_transfer_target_selection",
        ),
        (
            3,
            "unconditional final action",
            "06:946D clears the selected item and durability before 06:955A compacts the source inventory",
            "return state 0x1A when source inventory remains nonempty, otherwise 0x19",
            "item_discard_result",
        ),
    ];

    specs
        .into_iter()
        .map(
            |(code, availability, mutation_boundary, return_route, next_role)| ItemActionChoice {
                action_code: code,
                action_code_hex: format!("0x{code:02X}"),
                label: labels
                    .iter()
                    .find(|label| {
                        ITEM_ACTION_LABELS
                            .iter()
                            .any(|spec| spec.action_code == Some(code) && spec.index == label.index)
                    })
                    .unwrap_or_else(|| panic!("missing item action label for code {code}"))
                    .clone(),
                availability,
                mutation_boundary,
                result_dialogue_index: ITEM_ACTION_RESULT_DIALOGUE_INDICES[usize::from(code)],
                result_dialogue_index_hex: format!(
                    "0x{:02X}",
                    ITEM_ACTION_RESULT_DIALOGUE_INDICES[usize::from(code)]
                ),
                return_route,
                next_role,
            },
        )
        .collect()
}
