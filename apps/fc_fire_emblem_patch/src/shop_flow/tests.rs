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
