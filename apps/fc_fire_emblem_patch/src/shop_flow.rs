use std::{fs, path::Path};

use anyhow::{Context, Result, ensure};
use serde::Serialize;

use crate::{
    dialogue_inventory::{ShopDialogueTableBinding, inspect_shop_dialogue_table},
    rom::{EXPECTED_SOURCE_SHA1, HEADER_SIZE, Rom},
    sha1_hex,
};

const PRG_BANK_SIZE: usize = 16 * 1024;
const SWITCHABLE_CPU_START: u16 = 0x8000;
const SWITCHABLE_CPU_END_EXCLUSIVE: u16 = 0xC000;
const FIXED_CPU_START: u16 = 0xC000;
const FIXED_PRG_BANK: usize = 0x0F;

const SHOP_OUTER_STATE_ADDRESS: u16 = 0x05DB;
const MENU_CONTROLLER_INDEX_ADDRESS: u16 = 0x05CE;
const MENU_CONTROLLER_STATE_ADDRESS: u16 = 0x05DE;
const MENU_CHOICE_MASK_ADDRESS: u16 = 0x7FEE;
const MENU_SELECTION_BASE_ADDRESS: u16 = 0x7FF3;
const MENU_RESULT_ADDRESS: u16 = 0x05EB;
const SELECTED_FACILITY_ADDRESS: u16 = 0x77D0;
const DIALOGUE_ENTRY_INDEX_ADDRESS: u16 = 0x77F1;
const DIALOGUE_DIRECTORY_SELECTOR_ADDRESS: u16 = 0x77F4;
const STORED_FUNDS_ADDRESS: u16 = 0x7678;

const SHOP_STATE_HANDLERS: [u16; 13] = [
    0x99CC, 0xA13E, 0x99F1, 0x99FB, 0x9A0E, 0xA13E, 0x9B7A, 0x9B86, 0xA122, 0x9C02, 0xA13E, 0x9B7A,
    0x9C1A,
];
const MENU_CONTROLLER_HANDLERS: [u16; 7] = [0xC73D, 0x9265, 0x92A2, 0x92C9, 0x92FB, 0x9333, 0x93E0];

#[derive(Clone, Copy)]
struct SourceRegionSpec {
    role: &'static str,
    prg_bank: u8,
    cpu_address: u16,
    byte_count: usize,
    expected_sha1: &'static str,
}

const SOURCE_REGIONS: [SourceRegionSpec; 13] = [
    SourceRegionSpec {
        role: "dispatch_shop_outer_state",
        prg_bank: 0x06,
        cpu_address: 0x99AC,
        byte_count: 32,
        expected_sha1: "84933e684dbb18ff2cbfbd01f736fc9517cc5051",
    },
    SourceRegionSpec {
        role: "initialize_facility_dialogue",
        prg_bank: 0x06,
        cpu_address: 0x99CC,
        byte_count: 31,
        expected_sha1: "7f11d9aad54a0f5de2531f89c073d04f95aadc8e",
    },
    SourceRegionSpec {
        role: "handle_item_list_selection_and_preflight",
        prg_bank: 0x06,
        cpu_address: 0x9A0E,
        byte_count: 139,
        expected_sha1: "6dbd72473d14a71c3c88c586c5e7c1beb0ca7fe0",
    },
    SourceRegionSpec {
        role: "select_preflight_dialogue_entry",
        prg_bank: 0x06,
        cpu_address: 0x9A99,
        byte_count: 24,
        expected_sha1: "a15ffda682d6311a0fcd9fb732e3df2a8ec62f9e",
    },
    SourceRegionSpec {
        role: "handle_purchase_confirmation",
        prg_bank: 0x06,
        cpu_address: 0x9B86,
        byte_count: 106,
        expected_sha1: "a42a4c7b801e1a7a64875b94d37fc27b97c0d2ae",
    },
    SourceRegionSpec {
        role: "select_purchase_outcome_dialogue_entry",
        prg_bank: 0x06,
        cpu_address: 0x9BF0,
        byte_count: 18,
        expected_sha1: "542146bb01530e2daa83d21410a457e850c14e27",
    },
    SourceRegionSpec {
        role: "handle_continue_shopping_prompt",
        prg_bank: 0x06,
        cpu_address: 0x9C1A,
        byte_count: 24,
        expected_sha1: "8d745b05523bfe32e51efee584ab9f6cbdf9463f",
    },
    SourceRegionSpec {
        role: "complete_shop_exit_after_dialogue",
        prg_bank: 0x06,
        cpu_address: 0xA122,
        byte_count: 28,
        expected_sha1: "ae07c98348c1225d4b50ad705919bbd453c0d8a3",
    },
    SourceRegionSpec {
        role: "handle_dialogue_advance_input",
        prg_bank: 0x0A,
        cpu_address: 0x8588,
        byte_count: 94,
        expected_sha1: "037bc1e987031ddef73d60e149dc89712e14a04c",
    },
    SourceRegionSpec {
        role: "dispatch_shared_menu_controller",
        prg_bank: 0x0B,
        cpu_address: 0x9251,
        byte_count: 20,
        expected_sha1: "6a47e88c3ba87070c4b144c2aac233de7f604fe8",
    },
    SourceRegionSpec {
        role: "handle_shared_menu_input",
        prg_bank: 0x0B,
        cpu_address: 0x9333,
        byte_count: 121,
        expected_sha1: "ae003b21ace9212154d7616e38f2d542893c9c47",
    },
    SourceRegionSpec {
        role: "evaluate_unit_item_eligibility",
        prg_bank: 0x06,
        cpu_address: 0xA35E,
        byte_count: 0x73,
        expected_sha1: "9557d82d7b1984b51602540018b8666c07c07aec",
    },
    SourceRegionSpec {
        role: "item_family_allowed_class_lists",
        prg_bank: 0x06,
        cpu_address: 0xA3D1,
        byte_count: 0x42,
        expected_sha1: "3630b571e27d741cf416f146c822c9ff09dcc2a1",
    },
];

#[derive(Debug, Serialize)]
struct ShopFlowReport {
    schema: u8,
    source_sha1: &'static str,
    scope: Scope,
    route: ShopRoute,
    screens: Vec<ShopScreen>,
    preflight_branches: Vec<PreflightBranch>,
    purchase_mutation: PurchaseMutation,
    runtime_purchase_observation: RuntimePurchaseObservation,
    runtime_exit_observation: RuntimeExitObservation,
    runtime_inventory_full_observation: RuntimeInventoryFullObservation,
    runtime_insufficient_funds_observation: RuntimeInsufficientFundsObservation,
    runtime_item_restriction_observation: RuntimeItemRestrictionObservation,
    dialogue_table: ShopDialogueTableBinding,
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
struct ShopRoute {
    command_result: u8,
    command_result_hex: String,
    selected_facility_address: u16,
    selected_facility_address_hex: String,
    weapon_shop_facility_index: u8,
    initial_dialogue_entry_index: u8,
    initial_dialogue_entry_index_address: u16,
    dialogue_directory_selector: u8,
    dialogue_directory_selector_hex: String,
    dialogue_directory_selector_address: u16,
    outer_state_address: u16,
    menu_controller_index_address: u16,
    menu_selection_base_address: u16,
    observed_menu_controller_index: u8,
    observed_menu_selection_address: u16,
    outer_state_dispatcher: CodeLocation,
    outer_state_handlers: Vec<StateHandler>,
}

#[derive(Debug, Serialize)]
struct ShopScreen {
    screen_role: &'static str,
    runtime_observed: bool,
    outer_state: u8,
    menu_controller_state: Option<u8>,
    selectable_entry_count: usize,
    choice_mask: u8,
    choice_mask_hex: String,
    chr_pair: ChrPair,
    translation_target: &'static str,
    preserved_original: &'static [&'static str],
    visible_components: &'static [&'static str],
    temporal_observation: &'static str,
    input_actions: Vec<InputAction>,
}

#[derive(Debug, Serialize)]
struct ChrPair {
    left_fd: u8,
    left_fe: u8,
    right_fd: u8,
    right_fe: u8,
}

#[derive(Debug, Serialize)]
struct InputAction {
    input: &'static str,
    immediate_effect: &'static str,
    persistent_gameplay_mutation: bool,
    next_role: &'static str,
}

#[derive(Debug, Serialize)]
struct PreflightBranch {
    condition: &'static str,
    dialogue_entry_index: u8,
    first_outer_state: u8,
    settled_outer_state: u8,
    mutates_funds_or_inventory: bool,
    next_role: &'static str,
}

#[derive(Debug, Serialize)]
struct PurchaseMutation {
    accepted_menu_result: u8,
    declined_menu_results: [u8; 2],
    selected_item_address: u16,
    selected_item_address_hex: String,
    stored_funds_address: u16,
    stored_funds_address_hex: String,
    stored_funds_unit: &'static str,
    inventory_destination: &'static str,
    durability_destination: &'static str,
    accepted_dialogue_entry_index: u8,
    declined_dialogue_entry_index: u8,
    mutation_boundary: &'static str,
}

#[derive(Debug, Serialize)]
struct RuntimePurchaseObservation {
    source_screen_role: &'static str,
    result_screen_role: &'static str,
    stored_funds_before: u16,
    stored_funds_after: u16,
    displayed_funds_before: u16,
    displayed_funds_after: u16,
    item_destination_address: u16,
    item_destination_address_hex: String,
    item_before: u8,
    item_after: u8,
    durability_destination_address: u16,
    durability_destination_address_hex: String,
    durability_before: u8,
    durability_after: u8,
    result_outer_state: u8,
    result_chr_pair: ChrPair,
    result_screenshot_sha256: &'static str,
}

#[derive(Debug, Serialize)]
struct RuntimeExitObservation {
    source_screen_role: &'static str,
    exit_dialogue_entry_index: u8,
    exit_outer_state: u8,
    branch_mutated_funds_or_inventory: bool,
    exit_screenshot_sha256: &'static str,
    exit_temporal_observation: &'static str,
    advance_input: &'static str,
    completion_flag_address: u16,
    completion_flag_address_hex: String,
    completion_flag_value: u8,
    outer_state_after_completion: u8,
    returned_screen_role: &'static str,
    returned_chr_pair: ChrPair,
    returned_screenshot_sha256: &'static str,
    completion_effect: &'static str,
}

#[derive(Debug, Serialize)]
struct RuntimeInventoryFullObservation {
    setup_kind: &'static str,
    setup_inventory_items: [u8; 4],
    setup_inventory_durability: [u8; 4],
    outer_state_sequence: [u8; 3],
    dialogue_entry_sequence: [u8; 2],
    stored_funds_before: u16,
    stored_funds_after: u16,
    inventory_items_after: [u8; 4],
    inventory_durability_after: [u8; 4],
    branch_mutated_funds_or_inventory: bool,
    screenshot_sha256: &'static str,
    chr_pair: ChrPair,
    temporal_observation: &'static str,
    advance_input: &'static str,
    outer_state_after_completion: u8,
    completion_flag_value: u8,
    returned_screen_role: &'static str,
    returned_screenshot_sha256: &'static str,
    evidence_scope: &'static str,
}

#[derive(Debug, Serialize)]
struct RuntimeInsufficientFundsObservation {
    setup_kind: &'static str,
    stored_funds_before_setup: u16,
    stored_funds_after_setup: u16,
    inventory_items: [u8; 4],
    inventory_durability: [u8; 4],
    outer_state_sequence: [u8; 6],
    dialogue_entry_sequence: [u8; 2],
    branch_mutated_funds_or_inventory: bool,
    screenshot_sha256: &'static str,
    chr_pair: ChrPair,
    temporal_observation: &'static str,
    continue_input: &'static str,
    outer_state_after_continue: u8,
    funds_after_continue: u16,
    inventory_items_after_continue: [u8; 4],
    returned_screen_role: &'static str,
    returned_screenshot_sha256: &'static str,
    evidence_scope: &'static str,
}

#[derive(Debug, Serialize)]
struct RuntimeItemRestrictionObservation {
    setup_kind: &'static str,
    eligibility_case: ItemEligibilityCase,
    warning_outer_state_sequence: [u8; 4],
    warning_dialogue_entry_index: u8,
    warning_mutated_funds_or_inventory: bool,
    warning_screenshot_sha256: &'static str,
    chr_pair: ChrPair,
    warning_temporal_observation: &'static str,
    decline_route: RestrictionDeclineRoute,
    accepted_route: RestrictionAcceptedRoute,
    evidence_scope: &'static str,
}

#[derive(Debug, Serialize)]
struct ItemEligibilityCase {
    selected_unit_id: u8,
    selected_unit_class: u8,
    selected_unit_weapon_level: u8,
    selected_shop_ordinal: u8,
    selected_item_id: u8,
    required_weapon_level: u8,
    item_flag_byte: u8,
    allowed_class_ids: [u8; 4],
    failure_reason: &'static str,
    menu_controller_index_address: u16,
    menu_controller_index_value: u8,
    menu_selection_base_address: u16,
    effective_menu_selection_address: u16,
}

#[derive(Debug, Serialize)]
struct RestrictionDeclineRoute {
    input: &'static str,
    outer_state_sequence: [u8; 6],
    dialogue_entry_index: u8,
    mutated_funds_or_inventory: bool,
    prompt_screen_role: &'static str,
    prompt_screenshot_sha256: &'static str,
    prompt_temporal_observation: &'static str,
    continue_input: &'static str,
    returned_outer_state: u8,
    returned_screen_role: &'static str,
    returned_screenshot_sha256: &'static str,
}

#[derive(Debug, Serialize)]
struct RestrictionAcceptedRoute {
    input: &'static str,
    outer_state_sequence: [u8; 2],
    dialogue_entry_sequence: [u8; 2],
    stored_funds_before: u16,
    stored_funds_after: u16,
    item_destination_address: u16,
    item_value: u8,
    durability_destination_address: u16,
    durability_value: u8,
    result_screen_role: &'static str,
    result_screenshot_sha256: &'static str,
    result_temporal_observation: &'static str,
    completion_input: &'static str,
    completion_flag_value: u8,
    outer_state_after_completion: u8,
    returned_screen_role: &'static str,
    returned_screenshot_sha256: &'static str,
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

#[derive(Debug, Serialize)]
struct CodeLocation {
    prg_bank: u8,
    prg_bank_hex: String,
    cpu_address: u16,
    cpu_address_hex: String,
}

#[derive(Debug, Serialize)]
struct StateHandler {
    state: usize,
    cpu_address: u16,
    cpu_address_hex: String,
}

pub struct ShopFlowSummary {
    pub report_sha1: String,
    pub screen_count: usize,
    pub source_region_count: usize,
    pub next_screen_role: &'static str,
}

pub fn analyze_shop_flow(source_path: &Path, report_path: &Path) -> Result<ShopFlowSummary> {
    let rom = Rom::from_path(source_path)?;
    rom.verify_supported_japanese()?;
    let report = build_report(&rom)?;
    let mut report_bytes =
        serde_json::to_vec_pretty(&report).context("serialize shop-flow report")?;
    report_bytes.push(b'\n');
    let report_sha1 = sha1_hex(&report_bytes);

    if let Some(parent) = report_path.parent() {
        fs::create_dir_all(parent)
            .with_context(|| format!("create output directory {}", parent.display()))?;
    }
    fs::write(report_path, report_bytes)
        .with_context(|| format!("write {}", report_path.display()))?;

    Ok(ShopFlowSummary {
        report_sha1,
        screen_count: report.screens.len(),
        source_region_count: report.source_regions.len(),
        next_screen_role: report.unresolved_downstream_roles[0],
    })
}

fn build_report(rom: &Rom) -> Result<ShopFlowReport> {
    let source_regions = SOURCE_REGIONS
        .iter()
        .map(|spec| bind_source_region(rom, *spec))
        .collect::<Result<Vec<_>>>()?;
    validate_state_tables(rom)?;
    validate_item_eligibility_case(rom)?;
    let dialogue_table = inspect_shop_dialogue_table(rom.data())?;
    ensure!(
        dialogue_table.directory_selector == 0xB1,
        "shop dialogue directory selector changed"
    );
    ensure!(
        dialogue_table.pointer_table_cpu_address == 0xA766,
        "shop dialogue pointer table changed"
    );
    ensure!(
        dialogue_table.first_entry_pointer_cpu_address == 0xA816,
        "shop dialogue first entry changed"
    );

    Ok(ShopFlowReport {
        schema: 1,
        source_sha1: EXPECTED_SOURCE_SHA1,
        scope: Scope {
            translation_direction: "Japanese to Korean",
            preserve_existing_english_and_digits: true,
            dialogue_content_emitted: false,
            proof_boundary: "source-bound control flow plus observed screen contracts; no translated dialogue or ROM mutation",
        },
        route: ShopRoute {
            command_result: 7,
            command_result_hex: "0x07".to_owned(),
            selected_facility_address: SELECTED_FACILITY_ADDRESS,
            selected_facility_address_hex: hex_u16(SELECTED_FACILITY_ADDRESS),
            weapon_shop_facility_index: 1,
            initial_dialogue_entry_index: 0,
            initial_dialogue_entry_index_address: DIALOGUE_ENTRY_INDEX_ADDRESS,
            dialogue_directory_selector: 0xB1,
            dialogue_directory_selector_hex: "0xB1".to_owned(),
            dialogue_directory_selector_address: DIALOGUE_DIRECTORY_SELECTOR_ADDRESS,
            outer_state_address: SHOP_OUTER_STATE_ADDRESS,
            menu_controller_index_address: MENU_CONTROLLER_INDEX_ADDRESS,
            menu_selection_base_address: MENU_SELECTION_BASE_ADDRESS,
            observed_menu_controller_index: 3,
            observed_menu_selection_address: 0x7FF6,
            outer_state_dispatcher: location(0x06, 0x99AC),
            outer_state_handlers: SHOP_STATE_HANDLERS
                .iter()
                .copied()
                .enumerate()
                .map(|(state, cpu_address)| StateHandler {
                    state,
                    cpu_address,
                    cpu_address_hex: hex_u16(cpu_address),
                })
                .collect(),
        },
        screens: vec![
            item_list_screen(),
            purchase_confirmation_screen(),
            purchase_result_screen(),
            exit_message_screen(),
            inventory_full_message_screen(),
            insufficient_funds_message_screen(),
            item_restriction_confirmation_screen(),
            declined_continue_prompt_screen(),
            purchase_inventory_full_exit_screen(),
        ],
        preflight_branches: vec![
            PreflightBranch {
                condition: "selected unit has no free inventory slot",
                dialogue_entry_index: 3,
                first_outer_state: 7,
                settled_outer_state: 8,
                mutates_funds_or_inventory: false,
                next_role: "weapon_shop_inventory_full_message",
            },
            PreflightBranch {
                condition: "stored funds are lower than the selected price",
                dialogue_entry_index: 2,
                first_outer_state: 8,
                settled_outer_state: 12,
                mutates_funds_or_inventory: false,
                next_role: "weapon_shop_insufficient_funds_message",
            },
            PreflightBranch {
                condition: "item-specific eligibility check returns carry set",
                dialogue_entry_index: 4,
                first_outer_state: 5,
                settled_outer_state: 7,
                mutates_funds_or_inventory: false,
                next_role: "weapon_shop_item_restriction_confirmation",
            },
            PreflightBranch {
                condition: "inventory, funds, and item-specific checks pass",
                dialogue_entry_index: 1,
                first_outer_state: 5,
                settled_outer_state: 7,
                mutates_funds_or_inventory: false,
                next_role: "weapon_shop_purchase_confirmation",
            },
        ],
        purchase_mutation: PurchaseMutation {
            accepted_menu_result: 1,
            declined_menu_results: [0, 2],
            selected_item_address: 0x77B0,
            selected_item_address_hex: "0x77B0".to_owned(),
            stored_funds_address: STORED_FUNDS_ADDRESS,
            stored_funds_address_hex: hex_u16(STORED_FUNDS_ADDRESS),
            stored_funds_unit: "displayed G divided by 10",
            inventory_destination: "first zero byte in selected-unit slots +0x13 through +0x16 via pointer $74",
            durability_destination: "the selected inventory slot plus four bytes",
            accepted_dialogue_entry_index: 5,
            declined_dialogue_entry_index: 0x36,
            mutation_boundary: "only confirmation menu result 1 reaches $9BA4 and subtracts funds before inserting item and durability",
        },
        runtime_purchase_observation: RuntimePurchaseObservation {
            source_screen_role: "weapon_shop_purchase_confirmation",
            result_screen_role: "weapon_shop_purchase_result",
            stored_funds_before: 0x00C8,
            stored_funds_after: 0x00A8,
            displayed_funds_before: 2000,
            displayed_funds_after: 1680,
            item_destination_address: 0x7709,
            item_destination_address_hex: "0x7709".to_owned(),
            item_before: 0x00,
            item_after: 0x02,
            durability_destination_address: 0x770D,
            durability_destination_address_hex: "0x770D".to_owned(),
            durability_before: 0x00,
            durability_after: 0x2A,
            result_outer_state: 12,
            result_chr_pair: observed_shop_chr_pair(),
            result_screenshot_sha256: "c16554e171b10bf6c212b2a01e0754180ffc0e2a937eee7468423d7fdb00f106",
        },
        runtime_exit_observation: RuntimeExitObservation {
            source_screen_role: "weapon_shop_purchase_result",
            exit_dialogue_entry_index: 6,
            exit_outer_state: 8,
            branch_mutated_funds_or_inventory: false,
            exit_screenshot_sha256: "15d1d0adf1fd2a56863bc87a56df7aeff10c022948128de9ef3cb045ea403b6f",
            exit_temporal_observation: "CHR 1E/1E + 00/15 and the complete screenshot stayed stable across 152 regular and 168 irregularly spaced input-free frames",
            advance_input: "A",
            completion_flag_address: 0x7803,
            completion_flag_address_hex: "0x7803".to_owned(),
            completion_flag_value: 1,
            outer_state_after_completion: 0,
            returned_screen_role: "map_idle",
            returned_chr_pair: ChrPair {
                left_fd: 0x1A,
                left_fe: 0x1A,
                right_fd: 0x18,
                right_fe: 0x18,
            },
            returned_screenshot_sha256: "7c2bb933bf6876a2f032c36710fc53495add36774e1415f615efd44d343c534a",
            completion_effect: "returns to the map and completes the unit facility action without another funds or inventory write",
        },
        runtime_inventory_full_observation: RuntimeInventoryFullObservation {
            setup_kind: "reversible runtime copy of the third item and durability into the fourth empty slot",
            setup_inventory_items: [0x02, 0x0F, 0x02, 0x02],
            setup_inventory_durability: [0x2A, 0x16, 0x2A, 0x2A],
            outer_state_sequence: [4, 7, 8],
            dialogue_entry_sequence: [3, 6],
            stored_funds_before: 0x00A8,
            stored_funds_after: 0x00A8,
            inventory_items_after: [0x02, 0x0F, 0x02, 0x02],
            inventory_durability_after: [0x2A, 0x16, 0x2A, 0x2A],
            branch_mutated_funds_or_inventory: false,
            screenshot_sha256: "90f436083be6ff4e0af8ac3f88d8c70061abec9aff664995cc0be7c09c96d0ea",
            chr_pair: observed_shop_chr_pair(),
            temporal_observation: "CHR 1E/1E + 00/15 and the complete screenshot stayed stable across 152 regular and 168 irregularly spaced input-free frames",
            advance_input: "A",
            outer_state_after_completion: 0,
            completion_flag_value: 1,
            returned_screen_role: "map_idle",
            returned_screenshot_sha256: "93946a26fd1d900c03c119051f161214537a5e49b1405af9bb9208a55ac24114",
            evidence_scope: "the branch is runtime-observed from a declared reversible RAM setup, not a natural-play reproduction",
        },
        runtime_insufficient_funds_observation: RuntimeInsufficientFundsObservation {
            setup_kind: "reversible runtime write of stored funds from 0x00A8 to 0x0000 while retaining one free inventory slot",
            stored_funds_before_setup: 0x00A8,
            stored_funds_after_setup: 0x0000,
            inventory_items: [0x02, 0x0F, 0x02, 0x00],
            inventory_durability: [0x2A, 0x16, 0x2A, 0x00],
            outer_state_sequence: [4, 8, 9, 10, 11, 12],
            dialogue_entry_sequence: [2, 0x36],
            branch_mutated_funds_or_inventory: false,
            screenshot_sha256: "d74c34516389161786e8dc4dc93cabd07807b0bdd7baafbfa6d9731ed93b2c9a",
            chr_pair: observed_shop_chr_pair(),
            temporal_observation: "CHR 1E/1E + 00/15 and the complete screenshot stayed stable across 152 regular and 168 irregularly spaced input-free frames",
            continue_input: "A on yes",
            outer_state_after_continue: 4,
            funds_after_continue: 0x0000,
            inventory_items_after_continue: [0x02, 0x0F, 0x02, 0x00],
            returned_screen_role: "weapon_shop_item_list",
            returned_screenshot_sha256: "1f3945c0dab791a70cc31b4f3f0f3d36d0e9bdd3cbe58437895ba069df3a00bb",
            evidence_scope: "the branch is runtime-observed from a declared reversible RAM setup, not a natural-play reproduction",
        },
        runtime_item_restriction_observation: RuntimeItemRestrictionObservation {
            setup_kind: "selected the second live weapon-shop item after restoring funds from the prior reversible insufficient-funds setup; no unit, item, price, or eligibility bytes were changed",
            eligibility_case: ItemEligibilityCase {
                selected_unit_id: 0x05,
                selected_unit_class: 0x01,
                selected_unit_weapon_level: 0x06,
                selected_shop_ordinal: 2,
                selected_item_id: 0x11,
                required_weapon_level: 0x01,
                item_flag_byte: 0x0C,
                allowed_class_ids: [0x0B, 0x0C, 0x0E, 0x0F],
                failure_reason: "weapon level meets the requirement and item flag bit 0 is clear, but class 01 is absent from the item-family class list",
                menu_controller_index_address: MENU_CONTROLLER_INDEX_ADDRESS,
                menu_controller_index_value: 3,
                menu_selection_base_address: MENU_SELECTION_BASE_ADDRESS,
                effective_menu_selection_address: 0x7FF6,
            },
            warning_outer_state_sequence: [4, 5, 6, 7],
            warning_dialogue_entry_index: 4,
            warning_mutated_funds_or_inventory: false,
            warning_screenshot_sha256: "a468c110024cd91785dc0739ce88e4f881c3f564dccc41ce2b63d32a13872d15",
            chr_pair: observed_shop_chr_pair(),
            warning_temporal_observation: "CHR 1E/1E + 00/15 and the complete screenshot stayed stable across 152 regular and 168 irregularly spaced input-free frames",
            decline_route: RestrictionDeclineRoute {
                input: "B",
                outer_state_sequence: [7, 8, 9, 10, 11, 12],
                dialogue_entry_index: 0x36,
                mutated_funds_or_inventory: false,
                prompt_screen_role: "weapon_shop_declined_continue_prompt",
                prompt_screenshot_sha256: "d6960eb7bd3a5c5557b834e4a675be43c0c1edb46b8c75572c2707fda7f40081",
                prompt_temporal_observation: "CHR 1E/1E + 00/15 and the complete screenshot stayed stable across 152 regular and 168 irregularly spaced input-free frames",
                continue_input: "A on yes",
                returned_outer_state: 4,
                returned_screen_role: "weapon_shop_item_list",
                returned_screenshot_sha256: "2a122f8f48709c39bd737d1be072e49a94abeaf142d89d8a502f99c6158eacf4",
            },
            accepted_route: RestrictionAcceptedRoute {
                input: "A on yes",
                outer_state_sequence: [7, 8],
                dialogue_entry_sequence: [5, 6],
                stored_funds_before: 0x00A8,
                stored_funds_after: 0x0080,
                item_destination_address: 0x770A,
                item_value: 0x11,
                durability_destination_address: 0x770E,
                durability_value: 0x21,
                result_screen_role: "weapon_shop_purchase_inventory_full_exit",
                result_screenshot_sha256: "690761339ca7db244303260dd2aa73da4a3c2b7626b79a046cacfa533853d587",
                result_temporal_observation: "the accepted warning filled the last inventory slot; CHR 1E/1E + 00/15 and the complete purchase-and-exit screenshot stayed stable across 152 regular and 168 irregularly spaced input-free frames",
                completion_input: "A",
                completion_flag_value: 1,
                outer_state_after_completion: 0,
                returned_screen_role: "map_idle",
                returned_screenshot_sha256: "3639fe437ea05b04be815bf391147fcb483fb984481c44d60b8d0b8acfe9fbc1",
            },
            evidence_scope: "the item and unit predicate is source-bound and runtime-observed; only stored funds were restored after the separately declared insufficient-funds setup",
        },
        dialogue_table,
        source_regions,
        unresolved_downstream_roles: vec!["item_flow"],
        release_eligible: false,
    })
}

fn item_list_screen() -> ShopScreen {
    ShopScreen {
        screen_role: "weapon_shop_item_list",
        runtime_observed: true,
        outer_state: 4,
        menu_controller_state: Some(5),
        selectable_entry_count: 6,
        choice_mask: 0x3F,
        choice_mask_hex: "0x3F".to_owned(),
        chr_pair: observed_shop_chr_pair(),
        translation_target: "Japanese item names and shop dialogue only",
        preserved_original: &["decimal price digits", "decimal funds digits", "G"],
        visible_components: &[
            "six item-name and price rows",
            "current funds",
            "character portrait",
            "shop dialogue window",
            "selection marker",
            "map background and unit sprites",
        ],
        temporal_observation: "CHR 1E/1E + 00/15 and the complete screenshot stayed stable for 152 input-free frames",
        input_actions: vec![
            InputAction {
                input: "up or down",
                immediate_effect: "change only the selected ordinal with wraparound",
                persistent_gameplay_mutation: false,
                next_role: "weapon_shop_item_list",
            },
            InputAction {
                input: "A",
                immediate_effect: "map the selected ordinal to result 1..6 and run inventory, funds, and item-specific preflight",
                persistent_gameplay_mutation: false,
                next_role: "preflight-dependent shop message",
            },
            InputAction {
                input: "B",
                immediate_effect: "write menu result 0 and select the weapon-shop exit dialogue route",
                persistent_gameplay_mutation: false,
                next_role: "weapon_shop_exit_message",
            },
        ],
    }
}

fn purchase_confirmation_screen() -> ShopScreen {
    ShopScreen {
        screen_role: "weapon_shop_purchase_confirmation",
        runtime_observed: true,
        outer_state: 7,
        menu_controller_state: Some(5),
        selectable_entry_count: 2,
        choice_mask: 0x03,
        choice_mask_hex: "0x03".to_owned(),
        chr_pair: observed_shop_chr_pair(),
        translation_target: "Japanese confirmation dialogue and yes/no labels only",
        preserved_original: &["item-list price digits", "funds digits", "G"],
        visible_components: &[
            "retained six-row item list and prices",
            "retained current funds",
            "character portrait",
            "purchase question",
            "two-choice window",
            "sprite selection cursor",
            "map background and unit sprites",
        ],
        temporal_observation: "CHR 1E/1E + 00/15 and screenshot SHA-256 0bce2fe4...8318f stayed stable across 152 regular and 168 irregularly spaced input-free frames",
        input_actions: vec![
            InputAction {
                input: "up or down",
                immediate_effect: "toggle only the yes/no selected ordinal",
                persistent_gameplay_mutation: false,
                next_role: "weapon_shop_purchase_confirmation",
            },
            InputAction {
                input: "A on yes",
                immediate_effect: "enter the accepted purchase handler",
                persistent_gameplay_mutation: true,
                next_role: "weapon_shop_purchase_result",
            },
            InputAction {
                input: "A on no or B",
                immediate_effect: "select the declined dialogue path without entering the mutation block",
                persistent_gameplay_mutation: false,
                next_role: "weapon_shop_declined_continue_prompt",
            },
        ],
    }
}

fn purchase_result_screen() -> ShopScreen {
    ShopScreen {
        screen_role: "weapon_shop_purchase_result",
        runtime_observed: true,
        outer_state: 12,
        menu_controller_state: Some(5),
        selectable_entry_count: 2,
        choice_mask: 0x03,
        choice_mask_hex: "0x03".to_owned(),
        chr_pair: observed_shop_chr_pair(),
        translation_target: "Japanese purchase result, continue-shopping question, and yes/no labels only",
        preserved_original: &["updated funds digits", "G"],
        visible_components: &[
            "updated current funds",
            "character portrait",
            "purchase result and continue-shopping dialogue",
            "two-choice window",
            "sprite selection cursor",
            "map background and unit sprites",
        ],
        temporal_observation: "CHR 1E/1E + 00/15 and screenshot SHA-256 c16554e1...00f106 stayed stable across 152 regular and 168 irregularly spaced input-free frames",
        input_actions: vec![
            InputAction {
                input: "up or down",
                immediate_effect: "toggle only the continue-shopping selected ordinal",
                persistent_gameplay_mutation: false,
                next_role: "weapon_shop_purchase_result",
            },
            InputAction {
                input: "A on yes",
                immediate_effect: "set outer state 3 and rebuild the item list",
                persistent_gameplay_mutation: false,
                next_role: "weapon_shop_item_list",
            },
            InputAction {
                input: "A on no or B",
                immediate_effect: "select the weapon-shop exit dialogue route",
                persistent_gameplay_mutation: false,
                next_role: "weapon_shop_exit_message",
            },
        ],
    }
}

fn exit_message_screen() -> ShopScreen {
    ShopScreen {
        screen_role: "weapon_shop_exit_message",
        runtime_observed: true,
        outer_state: 8,
        menu_controller_state: None,
        selectable_entry_count: 0,
        choice_mask: 0,
        choice_mask_hex: "0x00".to_owned(),
        chr_pair: observed_shop_chr_pair(),
        translation_target: "Japanese exit dialogue only",
        preserved_original: &["updated funds digits", "G"],
        visible_components: &[
            "updated current funds",
            "character portrait",
            "exit dialogue",
            "map background and unit sprites",
        ],
        temporal_observation: "CHR 1E/1E + 00/15 and screenshot SHA-256 15d1d0ad...403b6f stayed stable across 152 regular and 168 irregularly spaced input-free frames",
        input_actions: vec![InputAction {
            input: "A",
            immediate_effect: "finish the dialogue, reset shop outer state to 0, return to map idle, and complete the facility action",
            persistent_gameplay_mutation: true,
            next_role: "map_idle",
        }],
    }
}

fn inventory_full_message_screen() -> ShopScreen {
    ShopScreen {
        screen_role: "weapon_shop_inventory_full_message",
        runtime_observed: true,
        outer_state: 8,
        menu_controller_state: None,
        selectable_entry_count: 0,
        choice_mask: 0,
        choice_mask_hex: "0x00".to_owned(),
        chr_pair: observed_shop_chr_pair(),
        translation_target: "Japanese inventory-full and automatic exit dialogue only",
        preserved_original: &["item-list price digits", "funds digits", "G"],
        visible_components: &[
            "retained six-row item list and prices",
            "retained current funds",
            "character portrait",
            "inventory-full dialogue followed by exit dialogue in the same window",
            "map background and unit sprites",
        ],
        temporal_observation: "CHR 1E/1E + 00/15 and screenshot SHA-256 90f43608...96d0ea stayed stable across 152 regular and 168 irregularly spaced input-free frames",
        input_actions: vec![InputAction {
            input: "A",
            immediate_effect: "finish the dialogue, reset shop outer state to 0, return to map idle, and complete the facility action",
            persistent_gameplay_mutation: true,
            next_role: "map_idle",
        }],
    }
}

fn insufficient_funds_message_screen() -> ShopScreen {
    ShopScreen {
        screen_role: "weapon_shop_insufficient_funds_message",
        runtime_observed: true,
        outer_state: 12,
        menu_controller_state: Some(5),
        selectable_entry_count: 2,
        choice_mask: 0x03,
        choice_mask_hex: "0x03".to_owned(),
        chr_pair: observed_shop_chr_pair(),
        translation_target: "Japanese insufficient-funds and continue-shopping dialogue plus yes/no labels only",
        preserved_original: &["funds digits", "G"],
        visible_components: &[
            "current funds",
            "insufficient-funds and continue-shopping dialogue",
            "two-choice window",
            "sprite selection cursor",
            "map background and unit sprites",
        ],
        temporal_observation: "CHR 1E/1E + 00/15 and screenshot SHA-256 d74c3451...3b2c9a stayed stable across 152 regular and 168 irregularly spaced input-free frames",
        input_actions: vec![
            InputAction {
                input: "up or down",
                immediate_effect: "toggle only the continue-shopping selected ordinal",
                persistent_gameplay_mutation: false,
                next_role: "weapon_shop_insufficient_funds_message",
            },
            InputAction {
                input: "A on yes",
                immediate_effect: "set outer state 3 and rebuild the item list",
                persistent_gameplay_mutation: false,
                next_role: "weapon_shop_item_list",
            },
            InputAction {
                input: "A on no or B",
                immediate_effect: "select the weapon-shop exit dialogue route",
                persistent_gameplay_mutation: false,
                next_role: "weapon_shop_exit_message",
            },
        ],
    }
}

fn item_restriction_confirmation_screen() -> ShopScreen {
    ShopScreen {
        screen_role: "weapon_shop_item_restriction_confirmation",
        runtime_observed: true,
        outer_state: 7,
        menu_controller_state: Some(5),
        selectable_entry_count: 2,
        choice_mask: 0x03,
        choice_mask_hex: "0x03".to_owned(),
        chr_pair: observed_shop_chr_pair(),
        translation_target: "Japanese item-restriction warning and yes/no labels only",
        preserved_original: &["item-list price digits", "funds digits", "G"],
        visible_components: &[
            "retained six-row item list and prices",
            "retained current funds",
            "character portrait",
            "item-restriction warning and buy-anyway question",
            "two-choice window",
            "sprite selection cursor",
            "map background and unit sprites",
        ],
        temporal_observation: "CHR 1E/1E + 00/15 and screenshot SHA-256 a468c110...72d15 stayed stable across 152 regular and 168 irregularly spaced input-free frames",
        input_actions: vec![
            InputAction {
                input: "up or down",
                immediate_effect: "toggle only the buy-anyway selected ordinal",
                persistent_gameplay_mutation: false,
                next_role: "weapon_shop_item_restriction_confirmation",
            },
            InputAction {
                input: "A on yes",
                immediate_effect: "enter the ordinary purchase mutation block despite the eligibility warning; the observed last free slot then selects the inventory-full exit result",
                persistent_gameplay_mutation: true,
                next_role: "weapon_shop_purchase_inventory_full_exit",
            },
            InputAction {
                input: "A on no or B",
                immediate_effect: "skip the purchase mutation and advance to the continue-shopping prompt",
                persistent_gameplay_mutation: false,
                next_role: "weapon_shop_declined_continue_prompt",
            },
        ],
    }
}

fn declined_continue_prompt_screen() -> ShopScreen {
    ShopScreen {
        screen_role: "weapon_shop_declined_continue_prompt",
        runtime_observed: true,
        outer_state: 12,
        menu_controller_state: Some(5),
        selectable_entry_count: 2,
        choice_mask: 0x03,
        choice_mask_hex: "0x03".to_owned(),
        chr_pair: observed_shop_chr_pair(),
        translation_target: "Japanese continue-shopping dialogue and yes/no labels only",
        preserved_original: &[],
        visible_components: &[
            "continue-shopping dialogue",
            "two-choice window",
            "sprite selection cursor",
            "map background and unit sprites",
        ],
        temporal_observation: "CHR 1E/1E + 00/15 and screenshot SHA-256 d6960eb7...f40081 stayed stable across 152 regular and 168 irregularly spaced input-free frames",
        input_actions: vec![
            InputAction {
                input: "up or down",
                immediate_effect: "toggle only the continue-shopping selected ordinal",
                persistent_gameplay_mutation: false,
                next_role: "weapon_shop_declined_continue_prompt",
            },
            InputAction {
                input: "A on yes",
                immediate_effect: "set outer state 3 and rebuild the item list",
                persistent_gameplay_mutation: false,
                next_role: "weapon_shop_item_list",
            },
            InputAction {
                input: "A on no or B",
                immediate_effect: "select the weapon-shop exit dialogue route",
                persistent_gameplay_mutation: false,
                next_role: "weapon_shop_exit_message",
            },
        ],
    }
}

fn purchase_inventory_full_exit_screen() -> ShopScreen {
    ShopScreen {
        screen_role: "weapon_shop_purchase_inventory_full_exit",
        runtime_observed: true,
        outer_state: 8,
        menu_controller_state: None,
        selectable_entry_count: 0,
        choice_mask: 0,
        choice_mask_hex: "0x00".to_owned(),
        chr_pair: observed_shop_chr_pair(),
        translation_target: "Japanese purchase result and automatic exit dialogue only",
        preserved_original: &["item-list price digits", "updated funds digits", "G"],
        visible_components: &[
            "retained six-row item list and prices",
            "updated current funds",
            "character portrait",
            "purchase result followed by exit dialogue",
            "map background and unit sprites",
        ],
        temporal_observation: "CHR 1E/1E + 00/15 and screenshot SHA-256 69076133...53d587 stayed stable across 152 regular and 168 irregularly spaced input-free frames",
        input_actions: vec![InputAction {
            input: "A",
            immediate_effect: "finish the dialogue, reset shop outer state to 0, return to map idle, and complete the facility action",
            persistent_gameplay_mutation: true,
            next_role: "map_idle",
        }],
    }
}

fn observed_shop_chr_pair() -> ChrPair {
    ChrPair {
        left_fd: 0x1E,
        left_fe: 0x1E,
        right_fd: 0x00,
        right_fe: 0x15,
    }
}

fn validate_state_tables(rom: &Rom) -> Result<()> {
    let shop_dispatch = switchable_slice(rom, 0x06, 0x99AC, 32)?;
    ensure!(
        shop_dispatch[..6] == [0xAD, 0xDB, 0x05, 0x20, 0x4C, 0xC3],
        "shop outer-state dispatcher changed"
    );
    let actual_shop_handlers = shop_dispatch[6..]
        .chunks_exact(2)
        .map(|bytes| u16::from_le_bytes([bytes[0], bytes[1]]))
        .collect::<Vec<_>>();
    ensure!(
        actual_shop_handlers == SHOP_STATE_HANDLERS,
        "shop outer-state handler table changed"
    );

    let menu_dispatch = switchable_slice(rom, 0x0B, 0x9251, 20)?;
    ensure!(
        menu_dispatch[..6] == [0xAD, 0xDE, 0x05, 0x20, 0x4C, 0xC3],
        "shared menu-controller dispatcher changed"
    );
    let actual_menu_handlers = menu_dispatch[6..]
        .chunks_exact(2)
        .map(|bytes| u16::from_le_bytes([bytes[0], bytes[1]]))
        .collect::<Vec<_>>();
    ensure!(
        actual_menu_handlers == MENU_CONTROLLER_HANDLERS,
        "shared menu-controller handler table changed"
    );

    ensure!(
        MENU_CONTROLLER_INDEX_ADDRESS == 0x05CE,
        "menu controller index address drift"
    );
    ensure!(
        MENU_CONTROLLER_STATE_ADDRESS == 0x05DE,
        "menu controller address drift"
    );
    ensure!(
        MENU_CHOICE_MASK_ADDRESS == 0x7FEE,
        "menu choice-mask address drift"
    );
    ensure!(
        MENU_SELECTION_BASE_ADDRESS == 0x7FF3,
        "menu selection base address drift"
    );
    ensure!(MENU_RESULT_ADDRESS == 0x05EB, "menu result address drift");
    Ok(())
}

fn validate_item_eligibility_case(rom: &Rom) -> Result<()> {
    let requirement = fixed_slice(rom, 0xD6C3, 1)?[0];
    let flags = fixed_slice(rom, 0xD9D3, 1)?[0];
    let allowed_classes = switchable_slice(rom, 0x06, 0xA3FE, 5)?;

    ensure!(
        requirement == 0x01,
        "representative item 11 weapon-level requirement changed"
    );
    ensure!(
        flags == 0x0C,
        "representative item 11 eligibility flags changed"
    );
    ensure!(
        allowed_classes == [0x0B, 0x0C, 0x0E, 0x0F, 0xEF],
        "representative item 11 allowed-class list changed"
    );
    Ok(())
}

fn bind_source_region(rom: &Rom, spec: SourceRegionSpec) -> Result<SourceRegionBinding> {
    let bytes = switchable_slice(rom, spec.prg_bank, spec.cpu_address, spec.byte_count)?;
    let actual_sha1 = sha1_hex(bytes);
    ensure!(
        actual_sha1 == spec.expected_sha1,
        "{} code changed: expected {}, found {}",
        spec.role,
        spec.expected_sha1,
        actual_sha1
    );
    let file_offset = switchable_file_offset(spec.prg_bank, spec.cpu_address)?;

    Ok(SourceRegionBinding {
        role: spec.role,
        prg_bank: spec.prg_bank,
        prg_bank_hex: format!("0x{:02X}", spec.prg_bank),
        cpu_address: spec.cpu_address,
        cpu_address_hex: hex_u16(spec.cpu_address),
        file_offset,
        file_offset_hex: format!("0x{file_offset:05X}"),
        byte_count: spec.byte_count,
        source_sha1: actual_sha1,
    })
}

fn switchable_slice(rom: &Rom, prg_bank: u8, cpu_address: u16, byte_count: usize) -> Result<&[u8]> {
    let file_offset = switchable_file_offset(prg_bank, cpu_address)?;
    let end = file_offset
        .checked_add(byte_count)
        .context("shop-flow code range overflow")?;
    rom.data()
        .get(file_offset..end)
        .with_context(|| format!("shop-flow code range exceeds ROM at {file_offset:05X}"))
}

fn fixed_slice(rom: &Rom, cpu_address: u16, byte_count: usize) -> Result<&[u8]> {
    let file_offset = fixed_file_offset(cpu_address)?;
    let end = file_offset
        .checked_add(byte_count)
        .context("shop-flow fixed source range overflow")?;
    rom.data()
        .get(file_offset..end)
        .with_context(|| format!("shop-flow fixed source range exceeds ROM at {file_offset:05X}"))
}

fn fixed_file_offset(cpu_address: u16) -> Result<usize> {
    ensure!(
        cpu_address >= FIXED_CPU_START,
        "fixed CPU address {cpu_address:04X} is below the fixed window"
    );
    Ok(HEADER_SIZE + FIXED_PRG_BANK * PRG_BANK_SIZE + usize::from(cpu_address - FIXED_CPU_START))
}

fn switchable_file_offset(prg_bank: u8, cpu_address: u16) -> Result<usize> {
    ensure!(prg_bank < 0x0F, "shop-flow code uses unavailable PRG bank");
    ensure!(
        (SWITCHABLE_CPU_START..SWITCHABLE_CPU_END_EXCLUSIVE).contains(&cpu_address),
        "shop-flow CPU address {cpu_address:04X} is outside the switchable window"
    );
    Ok(HEADER_SIZE
        + usize::from(prg_bank) * PRG_BANK_SIZE
        + usize::from(cpu_address - SWITCHABLE_CPU_START))
}

fn location(prg_bank: u8, cpu_address: u16) -> CodeLocation {
    CodeLocation {
        prg_bank,
        prg_bank_hex: format!("0x{prg_bank:02X}"),
        cpu_address,
        cpu_address_hex: hex_u16(cpu_address),
    }
}

fn hex_u16(value: u16) -> String {
    format!("0x{value:04X}")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::rom::{CHR_SIZE, EXPECTED_HEADER, PRG_SIZE};

    fn source_fixture() -> Vec<u8> {
        let mut source = vec![0; HEADER_SIZE + PRG_SIZE + CHR_SIZE];
        source[..HEADER_SIZE].copy_from_slice(&EXPECTED_HEADER);
        for spec in SOURCE_REGIONS {
            let file_offset = switchable_file_offset(spec.prg_bank, spec.cpu_address).unwrap();
            let expected = match spec.role {
                "dispatch_shop_outer_state" => {
                    let mut bytes = vec![0xAD, 0xDB, 0x05, 0x20, 0x4C, 0xC3];
                    bytes.extend(
                        SHOP_STATE_HANDLERS
                            .iter()
                            .flat_map(|handler| handler.to_le_bytes()),
                    );
                    bytes
                }
                "dispatch_shared_menu_controller" => {
                    let mut bytes = vec![0xAD, 0xDE, 0x05, 0x20, 0x4C, 0xC3];
                    bytes.extend(
                        MENU_CONTROLLER_HANDLERS
                            .iter()
                            .flat_map(|handler| handler.to_le_bytes()),
                    );
                    bytes
                }
                _ => continue,
            };
            source[file_offset..file_offset + expected.len()].copy_from_slice(&expected);
        }
        let requirement_offset = fixed_file_offset(0xD6C3).unwrap();
        let flags_offset = fixed_file_offset(0xD9D3).unwrap();
        let classes_offset = switchable_file_offset(0x06, 0xA3FE).unwrap();
        source[requirement_offset] = 0x01;
        source[flags_offset] = 0x0C;
        source[classes_offset..classes_offset + 5].copy_from_slice(&[0x0B, 0x0C, 0x0E, 0x0F, 0xEF]);
        source
    }

    #[test]
    fn state_tables_route_item_list_and_confirmation_to_distinct_handlers() {
        let source = source_fixture();
        let rom = Rom::parse(source).unwrap();

        validate_state_tables(&rom).unwrap();
        assert_eq!(SHOP_STATE_HANDLERS[4], 0x9A0E);
        assert_eq!(SHOP_STATE_HANDLERS[7], 0x9B86);
        assert_eq!(MENU_CONTROLLER_HANDLERS[5], 0x9333);
    }

    #[test]
    fn item_list_a_is_preflight_only_but_confirmation_yes_crosses_mutation_boundary() {
        let item_list = item_list_screen();
        let confirmation = purchase_confirmation_screen();

        let item_a = item_list
            .input_actions
            .iter()
            .find(|action| action.input == "A")
            .unwrap();
        let confirmation_yes = confirmation
            .input_actions
            .iter()
            .find(|action| action.input == "A on yes")
            .unwrap();

        assert!(!item_a.persistent_gameplay_mutation);
        assert_eq!(item_a.next_role, "preflight-dependent shop message");
        assert!(confirmation_yes.persistent_gameplay_mutation);
        assert_eq!(confirmation_yes.next_role, "weapon_shop_purchase_result");
    }

    #[test]
    fn confirmation_cancel_and_no_share_the_non_mutating_path() {
        let confirmation = purchase_confirmation_screen();
        let decline = confirmation
            .input_actions
            .iter()
            .find(|action| action.input == "A on no or B")
            .unwrap();

        assert!(!decline.persistent_gameplay_mutation);
        assert_eq!(decline.next_role, "weapon_shop_declined_continue_prompt");
    }

    #[test]
    fn purchase_result_yes_returns_to_the_known_item_list_without_another_purchase() {
        let result = purchase_result_screen();
        let continue_shopping = result
            .input_actions
            .iter()
            .find(|action| action.input == "A on yes")
            .unwrap();

        assert_eq!(result.outer_state, 12);
        assert!(!continue_shopping.persistent_gameplay_mutation);
        assert_eq!(continue_shopping.next_role, "weapon_shop_item_list");
    }

    #[test]
    fn exit_message_advance_returns_to_map_and_completes_the_facility_action() {
        let exit = exit_message_screen();
        let advance = exit.input_actions.first().unwrap();

        assert_eq!(exit.outer_state, 8);
        assert_eq!(exit.menu_controller_state, None);
        assert!(advance.persistent_gameplay_mutation);
        assert_eq!(advance.next_role, "map_idle");
    }

    #[test]
    fn inventory_full_message_is_a_non_purchase_branch_that_finishes_the_facility_action() {
        let screen = inventory_full_message_screen();
        let advance = screen.input_actions.first().unwrap();

        assert_eq!(screen.outer_state, 8);
        assert_eq!(screen.selectable_entry_count, 0);
        assert!(advance.persistent_gameplay_mutation);
        assert_eq!(advance.next_role, "map_idle");
    }

    #[test]
    fn insufficient_funds_message_can_return_to_the_item_list_without_a_purchase() {
        let screen = insufficient_funds_message_screen();
        let continue_shopping = screen
            .input_actions
            .iter()
            .find(|action| action.input == "A on yes")
            .unwrap();

        assert_eq!(screen.outer_state, 12);
        assert_eq!(screen.selectable_entry_count, 2);
        assert!(!continue_shopping.persistent_gameplay_mutation);
        assert_eq!(continue_shopping.next_role, "weapon_shop_item_list");
    }

    #[test]
    fn item_restriction_warning_preserves_both_the_decline_and_buy_anyway_routes() {
        let screen = item_restriction_confirmation_screen();
        let decline = screen
            .input_actions
            .iter()
            .find(|action| action.input == "A on no or B")
            .unwrap();
        let accept = screen
            .input_actions
            .iter()
            .find(|action| action.input == "A on yes")
            .unwrap();

        assert_eq!(screen.outer_state, 7);
        assert_eq!(screen.selectable_entry_count, 2);
        assert!(!decline.persistent_gameplay_mutation);
        assert_eq!(decline.next_role, "weapon_shop_declined_continue_prompt");
        assert!(accept.persistent_gameplay_mutation);
        assert_eq!(accept.next_role, "weapon_shop_purchase_inventory_full_exit");
    }

    #[test]
    fn declined_continue_prompt_can_rebuild_the_item_list_without_a_purchase() {
        let screen = declined_continue_prompt_screen();
        let continue_shopping = screen
            .input_actions
            .iter()
            .find(|action| action.input == "A on yes")
            .unwrap();

        assert_eq!(screen.outer_state, 12);
        assert!(screen.preserved_original.is_empty());
        assert!(!continue_shopping.persistent_gameplay_mutation);
        assert_eq!(continue_shopping.next_role, "weapon_shop_item_list");
    }

    #[test]
    fn purchase_that_fills_inventory_uses_the_automatic_exit_screen() {
        let screen = purchase_inventory_full_exit_screen();
        let advance = screen.input_actions.first().unwrap();

        assert_eq!(screen.outer_state, 8);
        assert_eq!(screen.selectable_entry_count, 0);
        assert!(advance.persistent_gameplay_mutation);
        assert_eq!(advance.next_role, "map_idle");
    }

    #[test]
    fn changed_state_table_is_rejected() {
        let mut source = source_fixture();
        let offset = switchable_file_offset(0x06, 0x99B2).unwrap();
        source[offset] ^= 1;
        let rom = Rom::parse(source).unwrap();

        assert!(validate_state_tables(&rom).is_err());
    }

    #[test]
    fn changed_item_eligibility_requirement_is_rejected() {
        let mut source = source_fixture();
        let offset = fixed_file_offset(0xD6C3).unwrap();
        source[offset] ^= 1;
        let rom = Rom::parse(source).unwrap();

        assert!(validate_item_eligibility_case(&rom).is_err());
    }
}
