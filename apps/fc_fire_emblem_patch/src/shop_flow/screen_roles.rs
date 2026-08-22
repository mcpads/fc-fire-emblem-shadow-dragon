use super::*;
use crate::dialogue_runtime_state::MAIN_DIALOGUE_RUNTIME_STATE;

pub(super) fn purchase_question_handoff_observation() -> RuntimeE7HandoffObservation {
    RuntimeE7HandoffObservation {
        source_screen_role: "weapon_shop_item_list",
        input: "A on the first item",
        source_outer_state: 4,
        handoff_outer_state: 5,
        settled_outer_state: 7,
        caller_flag_address: MAIN_DIALOGUE_RUNTIME_STATE.caller_handoff_flag_address,
        caller_flag_address_hex: format!(
            "0x{:04X}",
            MAIN_DIALOGUE_RUNTIME_STATE.caller_handoff_flag_address
        ),
        caller_flag_value: 1,
        observer_prg_bank: 0x06,
        observer_prg_bank_hex: "0x06",
        observer_read_cpu_address: 0xA144,
        observer_read_cpu_address_hex: "0xA144",
        chr_pair_at_handoff: ChrPair {
            left_fd: 0x1E,
            left_fe: 0x1E,
            right_fd: 0x00,
            right_fe: 0x18,
        },
        item_list_screenshot_sha256: "a35a6c07fdf13777188dc19371adf3a5681d571d5725e24a77c267460bc10261",
        handoff_screenshot_sha256: "98f4c941d6e3c115863c7896732c8eef8e16aa0ea04470401bd39b4f5b0c372a",
        settled_screenshot_sha256: "74ed3fade742ab3cf6738e48f3e3907b0989fa201e43d12e9baec066f76a8dde",
        item_list_nametable_sha256: "9152ae537608912ac13e8e2efa77168b5d2a7d57a18a9ad5eec23c9651329469",
        handoff_nametable_sha256: "bfd547fdbcc8eac92baee4163ae0e4fe0c96571d07dcb600c53571b59e6fe2ea",
        settled_nametable_sha256: "a500c9ec7a1949bc077f6f6e5bfe6af1445051a738508aaf741a62994233b4df",
        item_list_to_handoff_changed_byte_count: 16,
        handoff_to_settled_changed_byte_count: 40,
        retained_visible_content: &[
            "six item-name and price rows",
            "current funds and original G label",
            "character portrait",
            "purchase question",
        ],
        page_lifetime_requirement: "the retained item list and the selected purchase-question dialogue must share a compatible font-page assignment across the E7 caller handoff; switching to a dialogue-only page at the $7809 observer is invalid",
    }
}

pub(super) fn item_list_screen() -> ShopScreen {
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

pub(super) fn purchase_confirmation_screen() -> ShopScreen {
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

pub(super) fn purchase_result_screen() -> ShopScreen {
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

pub(super) fn exit_message_screen() -> ShopScreen {
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

pub(super) fn inventory_full_message_screen() -> ShopScreen {
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

pub(super) fn insufficient_funds_message_screen() -> ShopScreen {
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

pub(super) fn item_restriction_confirmation_screen() -> ShopScreen {
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

pub(super) fn declined_continue_prompt_screen() -> ShopScreen {
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

pub(super) fn purchase_inventory_full_exit_screen() -> ShopScreen {
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

pub(super) fn observed_shop_chr_pair() -> ChrPair {
    ChrPair {
        left_fd: 0x1E,
        left_fe: 0x1E,
        right_fd: 0x00,
        right_fe: 0x15,
    }
}
