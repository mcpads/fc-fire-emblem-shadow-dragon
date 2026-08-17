use std::{fs, path::Path};

use anyhow::{Context, Result, ensure};
use serde::Serialize;

use crate::{
    dialogue_inventory::{ShopDialogueTableBinding, inspect_shop_dialogue_table},
    rom::{EXPECTED_SOURCE_SHA1, HEADER_SIZE, Rom},
    sha1_hex,
};

mod report;
mod screen_roles;
mod source_binding;
mod source_spec;
#[cfg(test)]
mod tests;

pub use report::ShopFlowSummary;
use report::*;
use screen_roles::*;
use source_binding::*;
pub(crate) use source_binding::{SharedMenuControllerSource, bind_shared_menu_controller_source};
use source_spec::*;

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

pub(crate) fn validate_shop_lifetime_source(rom: &Rom) -> Result<()> {
    for spec in SOURCE_REGIONS {
        bind_source_region(rom, spec)?;
    }
    validate_state_tables(rom)?;
    validate_item_eligibility_case(rom)?;
    let dialogue_table = inspect_shop_dialogue_table(rom.data())?;
    ensure!(
        dialogue_table.directory_selector == 0xB1,
        "shop dialogue directory selector changed"
    );
    Ok(())
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
        runtime_e7_handoff_observation: purchase_question_handoff_observation(),
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
