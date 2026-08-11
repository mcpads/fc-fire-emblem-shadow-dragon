use std::{fs, path::Path};

use anyhow::{Context, Result};
use serde::Serialize;

use crate::{
    rom::{EXPECTED_SOURCE_SHA1, Rom},
    sha1_hex,
};

mod item_use_families;
mod runtime_evidence;
mod screen_roles;
mod source_contract;
mod special_use_runtime;
mod translation_workspace;

use runtime_evidence::{RuntimeObservation, runtime_observations};
use screen_roles::{ItemActionChoice, ItemScreen, action_choices, item_screens};
use source_contract::{
    COMPOSITE_STATE_ADDRESS, ELIGIBLE_RECIPIENT_COUNT_ADDRESS, ITEM_FLOW_STATES,
    MAIN_STATE_ADDRESS, MENU_CHOICE_MASK_BASE_ADDRESS, MENU_CONTROLLER_INDEX_ADDRESS,
    MENU_RESULT_ADDRESS, MENU_SELECTION_BASE_ADDRESS, SELECTED_ITEM_ACTION_ADDRESS,
    SELECTED_ITEM_ADDRESS, SELECTED_ITEM_SLOT_ADDRESS, SOURCE_REGIONS, bind_source_region,
    validate_action_result_dialogue_indices, validate_item_action_labels, validate_state_routes,
    validate_vulnerary_family,
};
pub(crate) use translation_workspace::plan_item_action_labels;

#[derive(Debug, Serialize)]
struct ItemFlowReport {
    schema: u8,
    source_sha1: &'static str,
    scope: Scope,
    route: ItemRoute,
    screens: Vec<ItemScreen>,
    action_choices: Vec<ItemActionChoice>,
    item_use_catalog: item_use_families::ItemUseCatalog,
    empty_inventory_label: FixedLabelBinding,
    runtime_observations: Vec<RuntimeObservation>,
    special_use_runtime_observations: Vec<special_use_runtime::SpecialUseRuntimeObservation>,
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

#[derive(Clone, Debug, Serialize)]
pub(super) struct FixedLabelBinding {
    index: u8,
    index_hex: String,
    source_text: &'static str,
    translation_scope: &'static str,
    pointer: u16,
    pointer_hex: String,
    bytes_hex: String,
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
    pub usable_item_count: usize,
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
        usable_item_count: report.item_use_catalog.usable_item_count,
        next_screen_role: report.unresolved_downstream_roles[0],
    })
}

pub(crate) fn inspect_item_action_label_count(rom: &Rom) -> Result<usize> {
    validate_state_routes(rom)?;
    Ok(validate_item_action_labels(rom)?
        .into_iter()
        .filter(|label| label.translation_scope == "japanese_only")
        .count())
}

pub(crate) fn validate_item_lifetime_source(rom: &Rom) -> Result<()> {
    validate_state_routes(rom)?;
    validate_action_result_dialogue_indices(rom)?;
    for spec in SOURCE_REGIONS {
        bind_source_region(rom, *spec)?;
    }
    item_use_families::inspect(rom)?;
    Ok(())
}

pub(crate) fn item_use_result_dialogue_sequences() -> Vec<Vec<u8>> {
    item_use_families::common_result_dialogue_sequences()
}

fn build_report(rom: &Rom) -> Result<ItemFlowReport> {
    validate_state_routes(rom)?;
    validate_action_result_dialogue_indices(rom)?;
    validate_vulnerary_family(rom)?;
    let item_use_catalog = item_use_families::inspect(rom)?;
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
        schema: 4,
        source_sha1: EXPECTED_SOURCE_SHA1,
        scope: Scope {
            translation_direction: "Japanese to Korean",
            preserve_existing_english_and_digits: true,
            dialogue_content_emitted: false,
            proof_boundary: "source-bound item screen flow, complete use-effect families, typed class-change and earth-orb downstream code, plus runtime-observed equip, use, transfer, discard, successful class-change, and earth-orb branches; forced special-item setups prove consumer reachability rather than natural acquisition, and no translated dialogue or ROM mutation is emitted",
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
        item_use_catalog,
        empty_inventory_label,
        runtime_observations: runtime_observations(),
        special_use_runtime_observations: special_use_runtime::observations(),
        source_regions,
        unresolved_downstream_roles: vec!["item_transfer_target_selection"],
        release_eligible: false,
    })
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
        FIXED_STRING_POINTER_TABLE_ADDRESS, ITEM_ACTION_FLAGS_TABLE_ADDRESS, ITEM_ACTION_LABELS,
        ITEM_ACTION_RESULT_DIALOGUE_INDICES, ITEM_DEFAULT_USES_TABLE_ADDRESS,
        MAP_STATE_POINTER_TABLE_ADDRESS, VULNERARY_ACTION_FLAGS, VULNERARY_DEFAULT_USES,
        VULNERARY_ITEM_ID, source_file_offset,
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
        let result_table = source_file_offset(0x06, 0x9516).unwrap();
        source[result_table..result_table + ITEM_ACTION_RESULT_DIALOGUE_INDICES.len()]
            .copy_from_slice(&ITEM_ACTION_RESULT_DIALOGUE_INDICES);
        let vulnerary_index = u16::from(VULNERARY_ITEM_ID - 1);
        write_byte(
            &mut source,
            0x0F,
            ITEM_DEFAULT_USES_TABLE_ADDRESS + vulnerary_index,
            VULNERARY_DEFAULT_USES,
        );
        write_byte(
            &mut source,
            0x0F,
            ITEM_ACTION_FLAGS_TABLE_ADDRESS + vulnerary_index,
            VULNERARY_ACTION_FLAGS,
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

    fn write_byte(source: &mut [u8], bank: u8, address: u16, value: u8) {
        let offset = source_file_offset(bank, address).unwrap();
        source[offset] = value;
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
        assert_eq!(actions[1].result_dialogue_index, 0x1A);
        assert_eq!(actions[1].next_role, "item_use_result");
        assert_eq!(actions[3].availability, "unconditional final action");
        let empty = labels.iter().find(|label| label.index == 0x17).unwrap();
        assert_eq!(empty.source_text, "NO ITEM");
        assert_eq!(empty.translation_scope, "preserve_original_latin");
    }

    #[test]
    fn vulnerary_family_binds_use_eligibility_and_default_uses() {
        let rom = Rom::parse(source_fixture()).unwrap();
        validate_vulnerary_family(&rom).unwrap();

        assert_eq!(VULNERARY_ITEM_ID, 0x40);
        assert_eq!(VULNERARY_ACTION_FLAGS, 0x41);
        assert_eq!(VULNERARY_DEFAULT_USES, 5);
    }

    #[test]
    fn rejects_changed_vulnerary_use_metadata() {
        let mut source = source_fixture();
        let item_index = u16::from(VULNERARY_ITEM_ID - 1);
        write_byte(
            &mut source,
            0x0F,
            ITEM_ACTION_FLAGS_TABLE_ADDRESS + item_index,
            0x01,
        );
        let rom = Rom::parse(source).unwrap();
        assert!(validate_vulnerary_family(&rom).is_err());
    }

    #[test]
    fn action_results_keep_distinct_roles_dialogues_and_return_routes() {
        let rom = Rom::parse(source_fixture()).unwrap();
        validate_action_result_dialogue_indices(&rom).unwrap();
        let labels = validate_item_action_labels(&rom).unwrap();
        let actions = action_choices(&labels);

        assert_eq!(
            actions
                .iter()
                .map(|action| action.next_role)
                .collect::<Vec<_>>(),
            vec![
                "item_equip_result",
                "item_use_result",
                "item_transfer_target_selection",
                "item_discard_result",
            ]
        );
        assert_eq!(
            actions
                .iter()
                .map(|action| action.result_dialogue_index)
                .collect::<Vec<_>>(),
            ITEM_ACTION_RESULT_DIALOGUE_INDICES
        );
        assert!(actions[2].return_route.contains("otherwise 0x19"));
    }

    #[test]
    fn observed_item_use_keeps_success_no_effect_and_exhaustion_distinct() {
        let observations = serde_json::to_value(runtime_observations()).unwrap();
        let item_use = observations
            .as_array()
            .unwrap()
            .iter()
            .find(|entry| entry["screen_role"] == "item_use_result")
            .unwrap();
        let variants = item_use["variants"].as_array().unwrap();

        assert_eq!(variants.len(), 3);
        assert_eq!(variants[0]["role"], "positive_heal");
        assert_eq!(variants[1]["role"], "full_hp_no_effect");
        assert_eq!(variants[1]["result_code_hex"], "0x30");
        assert_eq!(variants[2]["role"], "exhausted_use");
        assert_eq!(item_use["left_chr_pair"], "1A/1A");
        assert_eq!(item_use["right_chr_pair"], "00/15");
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
        let transfer = screens
            .iter()
            .find(|screen| screen.screen_role == "item_transfer_target_selection")
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
        let transfer_b = transfer
            .input_actions
            .iter()
            .find(|input| input.input == "B")
            .unwrap();
        assert!(!list_b.may_cause_persistent_gameplay_mutation);
        assert_eq!(list_b.next_role, "unit_command_menu");
        assert!(!action_b.may_cause_persistent_gameplay_mutation);
        assert_eq!(action_b.next_role, "item_inventory_list");
        assert!(!transfer_b.may_cause_persistent_gameplay_mutation);
        assert_eq!(transfer_b.next_role, "item_inventory_list");
    }
}
