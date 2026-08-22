pub(super) const PRG_BANK_SIZE: usize = 16 * 1024;
pub(super) const SWITCHABLE_CPU_START: u16 = 0x8000;
pub(super) const SWITCHABLE_CPU_END_EXCLUSIVE: u16 = 0xC000;
pub(super) const FIXED_CPU_START: u16 = 0xC000;
pub(super) const FIXED_PRG_BANK: usize = 0x0F;

pub(super) const SHOP_OUTER_STATE_ADDRESS: u16 =
    MAIN_DIALOGUE_RUNTIME_STATE.map_dialogue_outer_state_address;
pub(super) const MENU_CONTROLLER_INDEX_ADDRESS: u16 = 0x05CE;
pub(super) const MENU_CONTROLLER_STATE_ADDRESS: u16 = 0x05DE;
pub(super) const MENU_CHOICE_MASK_ADDRESS: u16 = 0x7FEE;
pub(super) const MENU_SELECTION_BASE_ADDRESS: u16 = 0x7FF3;
pub(super) const MENU_RESULT_ADDRESS: u16 = 0x05EB;
pub(super) const SELECTED_FACILITY_ADDRESS: u16 = 0x77D0;
pub(super) const DIALOGUE_ENTRY_INDEX_ADDRESS: u16 =
    MAIN_DIALOGUE_RUNTIME_STATE.entry_index_address;
pub(super) const DIALOGUE_DIRECTORY_SELECTOR_ADDRESS: u16 =
    MAIN_DIALOGUE_RUNTIME_STATE.directory_selector_address;
pub(super) const STORED_FUNDS_ADDRESS: u16 = 0x7678;

pub(super) const SHOP_STATE_HANDLERS: [u16; 13] = [
    0x99CC, 0xA13E, 0x99F1, 0x99FB, 0x9A0E, 0xA13E, 0x9B7A, 0x9B86, 0xA122, 0x9C02, 0xA13E, 0x9B7A,
    0x9C1A,
];
pub(super) const MENU_CONTROLLER_HANDLERS: [u16; 7] =
    [0xC73D, 0x9265, 0x92A2, 0x92C9, 0x92FB, 0x9333, 0x93E0];

#[derive(Clone, Copy)]
pub(super) struct SourceRegionSpec {
    pub(super) role: &'static str,
    pub(super) prg_bank: u8,
    pub(super) cpu_address: u16,
    pub(super) byte_count: usize,
    pub(super) expected_sha1: &'static str,
}

pub(super) const SOURCE_REGIONS: [SourceRegionSpec; 13] = [
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
use crate::dialogue_runtime_state::MAIN_DIALOGUE_RUNTIME_STATE;
