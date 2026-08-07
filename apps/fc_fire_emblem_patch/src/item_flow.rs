use std::{fs, path::Path};

use anyhow::{Context, Result};
use serde::Serialize;

use crate::{
    rom::{EXPECTED_SOURCE_SHA1, Rom},
    sha1_hex,
};

mod source_contract;

use source_contract::{
    COMPOSITE_STATE_ADDRESS, ELIGIBLE_RECIPIENT_COUNT_ADDRESS, ITEM_ACTION_LABELS,
    ITEM_FLOW_STATES, MAIN_STATE_ADDRESS, MENU_CHOICE_MASK_BASE_ADDRESS,
    MENU_CONTROLLER_INDEX_ADDRESS, MENU_RESULT_ADDRESS, MENU_SELECTION_BASE_ADDRESS,
    SELECTED_ITEM_ACTION_ADDRESS, SELECTED_ITEM_ADDRESS, SELECTED_ITEM_SLOT_ADDRESS,
    SOURCE_REGIONS, bind_source_region, validate_item_action_labels, validate_state_routes,
};

#[derive(Debug, Serialize)]
struct ItemFlowReport {
    schema: u8,
    source_sha1: &'static str,
    scope: Scope,
    route: ItemRoute,
    screens: Vec<ItemScreen>,
    action_choices: Vec<ItemActionChoice>,
    empty_inventory_label: FixedLabelBinding,
    runtime_observations: Vec<RuntimeObservation>,
    source_regions: Vec<SourceRegionBinding>,
    unresolved_downstream_roles: Vec<&'static str>,
    release_eligible: bool,
}

#[derive(Debug, Serialize)]
struct Scope {
    translation_direction: &'static str,
    preserve_existing_english_and_digits: bool,
    dialogue_content_emitted: bool,
    proof_boundary: &'static str,
}

#[derive(Debug, Serialize)]
struct ItemRoute {
    command_result: u8,
    command_result_hex: &'static str,
    main_state_address: u16,
    composite_state_address: u16,
    menu_result_address: u16,
    menu_controller_index_address: u16,
    menu_choice_mask_base_address: u16,
    menu_selection_base_address: u16,
    selected_item_address: u16,
    selected_item_slot_address: u16,
    selected_item_action_address: u16,
    eligible_recipient_count_address: u16,
    inventory_list_composite_state: u8,
    item_action_composite_state: u8,
    main_states: Vec<StateBinding>,
    command_dispatch: CodeLocation,
    inventory_command_handler: CodeLocation,
    item_list_b_route: &'static str,
    item_action_b_route: &'static str,
}

#[derive(Debug, Serialize)]
struct StateBinding {
    state: u8,
    state_hex: String,
    role: &'static str,
    handler: CodeLocation,
}

#[derive(Debug, Serialize)]
struct CodeLocation {
    prg_bank: u8,
    prg_bank_hex: String,
    cpu_address: u16,
    cpu_address_hex: String,
}

#[derive(Debug, Serialize)]
struct ItemScreen {
    screen_role: &'static str,
    runtime_observed: bool,
    input_behavior: &'static str,
    main_states: &'static [u8],
    composite_state: Option<u8>,
    translation_target: &'static str,
    preserved_original: &'static [&'static str],
    visible_components: &'static [&'static str],
    input_actions: Vec<InputAction>,
    next_gate: &'static str,
}

#[derive(Debug, Serialize)]
struct InputAction {
    input: &'static str,
    immediate_effect: &'static str,
    may_cause_persistent_gameplay_mutation: bool,
    next_role: &'static str,
}

#[derive(Debug, Serialize)]
struct ItemActionChoice {
    action_code: u8,
    action_code_hex: String,
    label: FixedLabelBinding,
    availability: &'static str,
    mutation_boundary: &'static str,
    next_role: &'static str,
}

#[derive(Clone, Debug, Serialize)]
struct FixedLabelBinding {
    index: u8,
    index_hex: String,
    source_text: &'static str,
    translation_scope: &'static str,
    pointer: u16,
    pointer_hex: String,
    bytes_hex: String,
}

#[derive(Debug, Serialize)]
struct RuntimeObservation {
    screen_role: &'static str,
    main_state: u8,
    menu_controller_index: u8,
    effective_choice_mask_address: u16,
    effective_choice_mask_address_hex: String,
    choice_mask: u8,
    choice_mask_hex: String,
    left_chr_pair: &'static str,
    right_chr_pair: &'static str,
    screenshot_phase_sha256: &'static [&'static str],
    temporal_observation: &'static str,
    source_items: &'static [&'static str],
    source_record_before: &'static str,
    source_record_after: &'static str,
    mutation_observed: bool,
}

#[derive(Debug, Serialize)]
struct SourceRegionBinding {
    role: &'static str,
    prg_bank: u8,
    prg_bank_hex: String,
    cpu_address: u16,
    cpu_address_hex: String,
    file_offset: usize,
    file_offset_hex: String,
    byte_count: usize,
    source_sha1: String,
}

pub struct ItemFlowSummary {
    pub report_sha1: String,
    pub screen_count: usize,
    pub source_region_count: usize,
    pub action_count: usize,
    pub next_screen_role: &'static str,
}

pub fn analyze_item_flow(source_path: &Path, report_path: &Path) -> Result<ItemFlowSummary> {
    let rom = Rom::from_path(source_path)?;
    rom.verify_supported_japanese()?;
    let report = build_report(&rom)?;
    let mut report_bytes =
        serde_json::to_vec_pretty(&report).context("serialize item-flow report")?;
    report_bytes.push(b'\n');
    let report_sha1 = sha1_hex(&report_bytes);

    if let Some(parent) = report_path.parent() {
        fs::create_dir_all(parent)
            .with_context(|| format!("create output directory {}", parent.display()))?;
    }
    fs::write(report_path, report_bytes)
        .with_context(|| format!("write {}", report_path.display()))?;

    Ok(ItemFlowSummary {
        report_sha1,
        screen_count: report.screens.len(),
        source_region_count: report.source_regions.len(),
        action_count: report.action_choices.len(),
        next_screen_role: report.unresolved_downstream_roles[0],
    })
}

fn build_report(rom: &Rom) -> Result<ItemFlowReport> {
    validate_state_routes(rom)?;
    let labels = validate_item_action_labels(rom)?;
    let source_regions = SOURCE_REGIONS
        .iter()
        .map(|spec| bind_source_region(rom, *spec))
        .collect::<Result<Vec<_>>>()?;
    let action_choices = action_choices(&labels);
    let empty_inventory_label = labels
        .into_iter()
        .find(|label| label.index == 0x17)
        .context("missing NO ITEM label")?;

    Ok(ItemFlowReport {
        schema: 1,
        source_sha1: EXPECTED_SOURCE_SHA1,
        scope: Scope {
            translation_direction: "Japanese to Korean",
            preserve_existing_english_and_digits: true,
            dialogue_content_emitted: false,
            proof_boundary: "source-bound item screen flow plus two observed non-mutating surfaces; no translated dialogue or ROM mutation",
        },
        route: ItemRoute {
            command_result: 6,
            command_result_hex: "0x06",
            main_state_address: MAIN_STATE_ADDRESS,
            composite_state_address: COMPOSITE_STATE_ADDRESS,
            menu_result_address: MENU_RESULT_ADDRESS,
            menu_controller_index_address: MENU_CONTROLLER_INDEX_ADDRESS,
            menu_choice_mask_base_address: MENU_CHOICE_MASK_BASE_ADDRESS,
            menu_selection_base_address: MENU_SELECTION_BASE_ADDRESS,
            selected_item_address: SELECTED_ITEM_ADDRESS,
            selected_item_slot_address: SELECTED_ITEM_SLOT_ADDRESS,
            selected_item_action_address: SELECTED_ITEM_ACTION_ADDRESS,
            eligible_recipient_count_address: ELIGIBLE_RECIPIENT_COUNT_ADDRESS,
            inventory_list_composite_state: 0x07,
            item_action_composite_state: 0x09,
            main_states: ITEM_FLOW_STATES
                .iter()
                .map(|(state, role, handler)| StateBinding {
                    state: *state,
                    state_hex: format!("0x{state:02X}"),
                    role,
                    handler: location(0x06, *handler),
                })
                .collect(),
            command_dispatch: location(0x06, 0x905D),
            inventory_command_handler: location(0x06, 0x90B6),
            item_list_b_route: "menu result 0 selects main state 0x0E, which rebuilds unit_command_menu and returns to input state 0x0F without changing the unit record",
            item_action_b_route: "menu result 0 sets return state 0x1A, passes through shared close state 0x26, and rebuilds item_inventory_list at input state 0x1B without changing the unit record",
        },
        screens: item_screens(),
        action_choices,
        empty_inventory_label,
        runtime_observations: runtime_observations(),
        source_regions,
        unresolved_downstream_roles: vec!["item_action_result", "item_transfer_target_selection"],
        release_eligible: false,
    })
}

fn item_screens() -> Vec<ItemScreen> {
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
            runtime_observed: false,
            input_behavior: "input_wait",
            main_states: &[0x1D],
            composite_state: None,
            translation_target: "Japanese recipient names and item text; preserve inherited Latin and digits",
            preserved_original: &["inherited stat abbreviations", "digits"],
            visible_components: &["source unit", "candidate recipient", "selected item"],
            input_actions: vec![],
            next_gate: "bind the target selector opened by action code 2 and distinguish cancel, no-capacity, and completed-transfer outcomes before input",
        },
        ItemScreen {
            screen_role: "item_action_result",
            runtime_observed: false,
            input_behavior: "mixed",
            main_states: &[0x1D, 0x1E],
            composite_state: None,
            translation_target: "Japanese action and item-effect result dialogue only",
            preserved_original: &["effect numbers", "inherited Latin item text"],
            visible_components: &["action-dependent result dialogue", "map or target context"],
            input_actions: vec![],
            next_gate: "sample equip, use, give, discard, depleted-use, and failed-effect results separately; do not generalize one dialogue state to all item families",
        },
    ]
}

fn action_choices(labels: &[FixedLabelBinding]) -> Vec<ItemActionChoice> {
    let specs = [
        (
            0,
            "item eligibility helper leaves carry set",
            "swap the selected item and durability with record offsets 0x13 and 0x17 at 06:A5CE",
            "item_action_result",
        ),
        (
            1,
            "item flag byte at 0xD9C3[item-1] has bit 0x40 set",
            "state 0x1E selects the item-specific effect and decrements durability; zero durability clears the item",
            "item_action_result",
        ),
        (
            2,
            "recipient scan has made 0x7750 nonzero",
            "after recipient approval, 06:952A moves item and durability to the target buffer and clears the source slot",
            "item_transfer_target_selection",
        ),
        (
            3,
            "unconditional final action",
            "06:946D clears the selected item and durability before 06:955A compacts the source inventory",
            "item_action_result",
        ),
    ];

    specs
        .into_iter()
        .map(
            |(code, availability, mutation_boundary, next_role)| ItemActionChoice {
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
                next_role,
            },
        )
        .collect()
}

fn runtime_observations() -> Vec<RuntimeObservation> {
    const UNIT_RECORD: &str = "05010112120030060706070207090400081800020f00002a160000";
    vec![
        RuntimeObservation {
            screen_role: "item_inventory_list",
            main_state: 0x1B,
            menu_controller_index: 1,
            effective_choice_mask_address: 0x7FEE,
            effective_choice_mask_address_hex: "0x7FEE".to_owned(),
            choice_mask: 0x03,
            choice_mask_hex: "0x03".to_owned(),
            left_chr_pair: "1A/1A",
            right_chr_pair: "00/15",
            screenshot_phase_sha256: &[
                "d2cd864619c55a6128fc3611ef2991d8f451c606a7604b6ae434379d8aa3f3f3",
                "ab447c688ce3d79f6430ae48d3731d36d9e36a81318b44155c8897ec4fe09e11",
                "0444f7efdd0ee4664f8c669278d2be4e8a6ed79b0f81f256c46abebb088298cc",
            ],
            temporal_observation: "152 regular plus 168 irregularly spaced input-free frames kept both item rows and CHR fixed while cursor and map sprites cycled through three screenshot phases",
            source_items: &[
                "item 02 / durability 2A / displayed てつのつるぎ 42",
                "item 0F / durability 16 / displayed てやり 22",
            ],
            source_record_before: UNIT_RECORD,
            source_record_after: UNIT_RECORD,
            mutation_observed: false,
        },
        RuntimeObservation {
            screen_role: "item_action_menu",
            main_state: 0x1C,
            menu_controller_index: 2,
            effective_choice_mask_address: 0x7FEF,
            effective_choice_mask_address_hex: "0x7FEF".to_owned(),
            choice_mask: 0x0D,
            choice_mask_hex: "0x0D".to_owned(),
            left_chr_pair: "1A/1A",
            right_chr_pair: "00/15",
            screenshot_phase_sha256: &[
                "b14877d722366f39faa1c5a265babfe508b1fc97019a90dd64102ad12be8262f",
                "aa0d289b4a62ef84e61ef537eab263965ba6ab841c696eacb2d1fcd9f39456a1",
                "02ad14ba7245db9a03852ea4be0180f1122f9076fb14720fde8b85c22737a8ef",
            ],
            temporal_observation: "for item 02, 152 regular plus 168 irregularly spaced input-free frames kept そうび, わたす, すてる and CHR fixed while cursor and map sprites cycled through three screenshot phases",
            source_items: &[
                "selected item 02 at record offset 13",
                "normalized action mask 0D selects action codes 0, 2, and 3",
            ],
            source_record_before: UNIT_RECORD,
            source_record_after: UNIT_RECORD,
            mutation_observed: false,
        },
    ]
}

fn location(prg_bank: u8, cpu_address: u16) -> CodeLocation {
    CodeLocation {
        prg_bank,
        prg_bank_hex: format!("0x{prg_bank:02X}"),
        cpu_address,
        cpu_address_hex: format!("0x{cpu_address:04X}"),
    }
}

#[cfg(test)]
mod tests {
    use super::source_contract::{
        COMMAND_ACTION_POINTER_TABLE_ADDRESS, COMPOSITE_POINTER_TABLE_ADDRESS,
        FIXED_STRING_POINTER_TABLE_ADDRESS, MAP_STATE_POINTER_TABLE_ADDRESS, source_file_offset,
    };
    use super::*;
    use crate::rom::{CHR_SIZE, EXPECTED_HEADER, HEADER_SIZE, PRG_SIZE};

    fn source_fixture() -> Vec<u8> {
        let mut source = vec![0; HEADER_SIZE + PRG_SIZE + CHR_SIZE];
        source[..HEADER_SIZE].copy_from_slice(&EXPECTED_HEADER);
        for (state, _, handler) in ITEM_FLOW_STATES {
            write_u16(
                &mut source,
                0x06,
                MAP_STATE_POINTER_TABLE_ADDRESS + u16::from(*state) * 2,
                *handler,
            );
        }
        write_u16(
            &mut source,
            0x0B,
            COMPOSITE_POINTER_TABLE_ADDRESS + 0x07 * 2,
            0x85BE,
        );
        write_u16(
            &mut source,
            0x0B,
            COMPOSITE_POINTER_TABLE_ADDRESS + 0x09 * 2,
            0x8613,
        );
        write_u16(
            &mut source,
            0x06,
            COMMAND_ACTION_POINTER_TABLE_ADDRESS + (6 - 1) * 2,
            0x90B6,
        );
        for spec in ITEM_ACTION_LABELS {
            write_u16(
                &mut source,
                0x0B,
                FIXED_STRING_POINTER_TABLE_ADDRESS + u16::from(spec.index) * 2,
                spec.pointer,
            );
            let offset = source_file_offset(0x0B, spec.pointer).unwrap();
            source[offset..offset + spec.expected.len()].copy_from_slice(spec.expected);
        }
        source
    }

    fn write_u16(source: &mut [u8], bank: u8, address: u16, value: u16) {
        let offset = source_file_offset(bank, address).unwrap();
        source[offset..offset + 2].copy_from_slice(&value.to_le_bytes());
    }

    #[test]
    fn command_result_six_reaches_distinct_item_list_and_action_states() {
        let rom = Rom::parse(source_fixture()).unwrap();
        validate_state_routes(&rom).unwrap();
        assert_eq!(
            ITEM_FLOW_STATES[3],
            (0x1A, "open_item_inventory_list", 0x93D4)
        );
        assert_eq!(
            ITEM_FLOW_STATES[5],
            (0x1C, "wait_for_item_action_input", 0x9425)
        );
    }

    #[test]
    fn rejects_a_changed_inventory_command_route() {
        let mut source = source_fixture();
        write_u16(
            &mut source,
            0x06,
            COMMAND_ACTION_POINTER_TABLE_ADDRESS + (6 - 1) * 2,
            0x90BF,
        );
        let rom = Rom::parse(source).unwrap();
        assert!(validate_state_routes(&rom).is_err());
    }

    #[test]
    fn action_menu_keeps_japanese_actions_conditional_and_no_item_latin_preserved() {
        let rom = Rom::parse(source_fixture()).unwrap();
        let labels = validate_item_action_labels(&rom).unwrap();
        let actions = action_choices(&labels);
        assert_eq!(actions.len(), 4);
        assert_eq!(actions[1].label.source_text, "つかう");
        assert!(actions[1].availability.contains("0x40"));
        assert_eq!(actions[3].availability, "unconditional final action");
        let empty = labels.iter().find(|label| label.index == 0x17).unwrap();
        assert_eq!(empty.source_text, "NO ITEM");
        assert_eq!(empty.translation_scope, "preserve_original_latin");
    }

    #[test]
    fn rejects_a_changed_item_action_label_pointer() {
        let mut source = source_fixture();
        write_u16(
            &mut source,
            0x0B,
            FIXED_STRING_POINTER_TABLE_ADDRESS + 0x14 * 2,
            0x90E3,
        );
        let rom = Rom::parse(source).unwrap();
        assert!(validate_item_action_labels(&rom).is_err());
    }

    #[test]
    fn cancel_paths_are_non_mutating_and_return_to_the_correct_parent() {
        let screens = item_screens();
        let list = screens
            .iter()
            .find(|screen| screen.screen_role == "item_inventory_list")
            .unwrap();
        let action = screens
            .iter()
            .find(|screen| screen.screen_role == "item_action_menu")
            .unwrap();
        let list_b = list
            .input_actions
            .iter()
            .find(|input| input.input == "B")
            .unwrap();
        let action_b = action
            .input_actions
            .iter()
            .find(|input| input.input == "B")
            .unwrap();
        assert!(!list_b.may_cause_persistent_gameplay_mutation);
        assert_eq!(list_b.next_role, "unit_command_menu");
        assert!(!action_b.may_cause_persistent_gameplay_mutation);
        assert_eq!(action_b.next_role, "item_inventory_list");
    }
}
