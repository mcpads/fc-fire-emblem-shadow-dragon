use anyhow::{Context, Result, ensure};

use super::{FixedLabelBinding, SourceRegionBinding};
use crate::{
    rom::{HEADER_SIZE, Rom},
    sha1_hex,
};

const PRG_BANK_SIZE: usize = 16 * 1024;
const SWITCHABLE_CPU_START: u16 = 0x8000;
const SWITCHABLE_CPU_END_EXCLUSIVE: u16 = 0xC000;
const FIXED_CPU_START: u16 = 0xC000;
const FIXED_PRG_BANK: u8 = 0x0F;

pub(super) const MAIN_STATE_ADDRESS: u16 = 0x0084;
pub(super) const COMPOSITE_STATE_ADDRESS: u16 = 0x05E8;
pub(super) const MENU_RESULT_ADDRESS: u16 = 0x05EB;
pub(super) const MENU_CONTROLLER_INDEX_ADDRESS: u16 = 0x05CE;
pub(super) const MENU_CHOICE_MASK_BASE_ADDRESS: u16 = 0x7FEE;
pub(super) const MENU_SELECTION_BASE_ADDRESS: u16 = 0x7FF3;
pub(super) const SELECTED_ITEM_ADDRESS: u16 = 0x77B0;
pub(super) const SELECTED_ITEM_SLOT_ADDRESS: u16 = 0x77B1;
pub(super) const SELECTED_ITEM_ACTION_ADDRESS: u16 = 0x77B2;
pub(super) const ELIGIBLE_RECIPIENT_COUNT_ADDRESS: u16 = 0x7750;
pub(super) const ITEM_ACTION_RESULT_DIALOGUE_INDICES: [u8; 4] = [0x19, 0x1A, 0x1B, 0x1C];
pub(super) const VULNERARY_ITEM_ID: u8 = 0x40;
pub(super) const ITEM_DEFAULT_USES_TABLE_ADDRESS: u16 = 0xD87F;
pub(super) const ITEM_ACTION_FLAGS_TABLE_ADDRESS: u16 = 0xD9C3;
pub(super) const ITEM_COUNT: usize = 91;
pub(super) const ITEM_USE_ACTION_FLAG: u8 = 0x40;
pub(super) const VULNERARY_DEFAULT_USES: u8 = 5;
pub(super) const VULNERARY_ACTION_FLAGS: u8 = 0x41;

pub(super) const MAP_STATE_POINTER_TABLE_ADDRESS: u16 = 0x8967;
pub(super) const COMPOSITE_POINTER_TABLE_ADDRESS: u16 = 0x8006;
pub(super) const COMMAND_ACTION_POINTER_TABLE_ADDRESS: u16 = 0x906B;
pub(super) const FIXED_STRING_POINTER_TABLE_ADDRESS: u16 = 0x8FC2;

pub(super) const ITEM_FLOW_STATES: &[(u8, &str, u16)] = &[
    (0x0E, "reopen_unit_command_menu", 0x9023),
    (0x0F, "wait_for_unit_command_input", 0x9042),
    (0x10, "dispatch_unit_command_result", 0x905D),
    (0x1A, "open_item_inventory_list", 0x93D4),
    (0x1B, "wait_for_item_inventory_input", 0x93E5),
    (0x1C, "wait_for_item_action_input", 0x9425),
    (0x1D, "execute_selected_item_action", 0x944C),
    (0x1E, "run_item_action_result", 0x9579),
    (0x26, "close_nested_unit_window", 0xAF66),
];
pub(super) const ITEM_COMPOSITE_STATES: &[(u8, u16, &str)] = &[
    (0x07, 0x85BE, "compose_item_inventory_rows"),
    (0x09, 0x8613, "compose_item_action_menu"),
];

#[derive(Clone, Copy)]
pub(super) struct SourceRegionSpec {
    role: &'static str,
    prg_bank: u8,
    cpu_address: u16,
    byte_count: usize,
    expected_sha1: &'static str,
}

pub(super) const SOURCE_REGIONS: &[SourceRegionSpec] = &[
    region(
        "handle_unit_command_input",
        0x06,
        0x9042,
        27,
        "dc438c877f31518b82bc7bcbde7652afe56ddd16",
    ),
    region(
        "dispatch_unit_command_action",
        0x06,
        0x905D,
        107,
        "2d5074bc6862f0004d4013113def3788c5639927",
    ),
    region(
        "select_item_transfer_target",
        0x06,
        0x914B,
        156,
        "81ccd909dce8881aa6b8eefbfe24a339bd6a4484",
    ),
    region(
        "enter_item_inventory_flow",
        0x06,
        0x93D4,
        17,
        "f72fdc44ecc0f1dd80cbc2c661c71a4c48738de9",
    ),
    region(
        "handle_item_inventory_input",
        0x06,
        0x93E5,
        64,
        "0579b80d041e8b877e1b8ae5226c882fefc5fdbf",
    ),
    region(
        "handle_item_action_input",
        0x06,
        0x9425,
        39,
        "4465508fb6e7fbf1dd33ffd33348bd4e00b69279",
    ),
    region(
        "execute_item_action",
        0x06,
        0x944C,
        202,
        "9601f0ae0d7fba3fbf7896d6ea9bb9cee5cd1ba9",
    ),
    region(
        "select_item_action_result_dialogue",
        0x06,
        0x9516,
        4,
        "dd10ce1c999a9f53c8dd5dfdda02c757ac100836",
    ),
    region(
        "count_inventory_items",
        0x06,
        0x951A,
        16,
        "bd3cb4115211641db9008c7ae1cbb6bd89330ea3",
    ),
    region(
        "move_or_clear_item",
        0x06,
        0x952A,
        48,
        "1e766440e2943b651edb439bdea727132d51d18a",
    ),
    region(
        "compact_inventory_after_mutation",
        0x06,
        0x955A,
        31,
        "d551403b3b5092b301f217cc4c9474e9163e5f16",
    ),
    region(
        "dispatch_item_action_result_stage",
        0x06,
        0x9579,
        48,
        "6d7ef375255c3a4e8631178d988eef46cee1bbff",
    ),
    region(
        "dispatch_item_use_effect_family",
        0x06,
        0x95A9,
        121,
        "96b9cc408a1d906e5c545dda3cc33f63cf4213ae",
    ),
    region(
        "finalize_item_use_consumption",
        0x06,
        0x9622,
        49,
        "b8c4e76ba9b5bdc6c704b69bb147a8da77651425",
    ),
    region(
        "apply_vulnerary_heal",
        0x06,
        0x9653,
        61,
        "d168df1f45306ddbe91c0939483464048b3bf2ac",
    ),
    region(
        "apply_map_key",
        0x06,
        0x9690,
        102,
        "87f86132e2b81da84492552f4173893a4d0dff5f",
    ),
    region(
        "find_map_key_target",
        0x06,
        0x96FA,
        146,
        "9640e820ca029923d344d4d9ffb60e91f82e64c6",
    ),
    region(
        "apply_stat_booster",
        0x06,
        0x978C,
        78,
        "0b0e530c7881795f6e5e71f6a0c744c23f53e0bf",
    ),
    region(
        "validate_and_begin_class_change_item",
        0x06,
        0x97DA,
        195,
        "ef92cbe3aa061633174edc096efec775e8865849",
    ),
    region(
        "class_change_primary_source_class_table",
        0x06,
        0x989D,
        5,
        "17b70a24bb0ce2fade5cb98f17cf48c741f051ce",
    ),
    region(
        "class_change_alternate_source_class_table",
        0x06,
        0x98A2,
        5,
        "7e041a5c23dabf41622a95110728f63f62b24051",
    ),
    region(
        "class_change_target_class_table",
        0x06,
        0x98A7,
        5,
        "9c0cb7b405cb54cea6eb7e049a332666bdff1cc0",
    ),
    region(
        "complete_class_change_item_sequence",
        0x06,
        0x98AC,
        15,
        "698039edc69d1714ba92801a1a31e79fdfe90a0c",
    ),
    region(
        "apply_earth_orb_effect",
        0x06,
        0x98BB,
        170,
        "fcbef26571889af796e332195ac11d0b74c5b9fd",
    ),
    region(
        "earth_orb_displacement_tables",
        0x06,
        0x9965,
        16,
        "3957ce10399e6b21b1fdeb89387c3335b03a8a4d",
    ),
    region(
        "apply_earth_orb_to_unit_records",
        0x06,
        0x9975,
        37,
        "28f7483dcee005d7b74f88297605d31201fa755a",
    ),
    region(
        "advance_class_change_battle_states",
        0x06,
        0x9D3C,
        46,
        "16b99f01d5052e6b8ef9ec2f002d39ed93ddda5e",
    ),
    region(
        "acknowledge_completed_class_change_battle_dialogue",
        0x04,
        0x827D,
        52,
        "f880ea430f0131545c93bdc1c15852bfb147a481",
    ),
    region(
        "complete_shared_class_change_battle",
        0x01,
        0xB956,
        13,
        "e98e9929a9e81be8e3af55219e7ffdad29a28f2d",
    ),
    region(
        "restore_map_after_class_change",
        0x06,
        0xB97F,
        45,
        "1899b1359343674080178fd1f55385b40aa3eb3a",
    ),
    region(
        "map_restore_effect_table",
        0x06,
        0xB9AC,
        6,
        "417588215dd056e4cb64f210039a15f96c5e435b",
    ),
    region(
        "swap_selected_item_to_equipped_slot",
        0x06,
        0xA5CE,
        41,
        "de1f6ff348cb45a0ad091d4ca52a54502c278d0e",
    ),
    region(
        "finalize_conditional_item_menu",
        0x0B,
        0x8595,
        41,
        "f8938a98aa8f741983d4bfda7c830d3d2673a55b",
    ),
    region(
        "compose_item_inventory_rows",
        0x0B,
        0x85BE,
        39,
        "d31c7036f1d4c94878d0d5c29344e2361e4294b5",
    ),
    region(
        "compose_item_action_menu",
        0x0B,
        0x8613,
        106,
        "5432a4f469087a09b8f3017047eeed4968a418ce",
    ),
    region(
        "annotate_item_eligibility",
        0x0B,
        0x871C,
        67,
        "4d07e742b7263172e713c833d7ad50c3d778478c",
    ),
    region(
        "append_item_name_and_durability",
        0x0B,
        0x875F,
        38,
        "f001445e83f088b3b21415603193f4efb9ef3f40",
    ),
    region(
        "count_and_map_menu_choices",
        0x0B,
        0x9840,
        24,
        "14de03d771684fdd3c61a885ecb70fb9ddc86eff",
    ),
    region(
        "normalize_item_action_mask",
        0x0F,
        0xC39A,
        5,
        "32367567aa0ecae3cff1f09dce7659fc68a97d5e",
    ),
    region(
        "item_default_uses_table",
        0x0F,
        ITEM_DEFAULT_USES_TABLE_ADDRESS,
        ITEM_COUNT,
        "cb37390f803c0e99ba02feb0dbf8a60e0582807d",
    ),
    region(
        "item_action_flags_table",
        0x0F,
        ITEM_ACTION_FLAGS_TABLE_ADDRESS,
        ITEM_COUNT,
        "17c5bdab2181218617fdc1d7f1f6866ce437eea5",
    ),
];

const fn region(
    role: &'static str,
    prg_bank: u8,
    cpu_address: u16,
    byte_count: usize,
    expected_sha1: &'static str,
) -> SourceRegionSpec {
    SourceRegionSpec {
        role,
        prg_bank,
        cpu_address,
        byte_count,
        expected_sha1,
    }
}

pub(crate) struct ItemActionLabelSpec {
    pub(crate) action_code: Option<u8>,
    pub(crate) index: u8,
    pub(crate) source_text: &'static str,
    pub(crate) translation_scope: &'static str,
    pub(crate) pointer: u16,
    pub(crate) expected: &'static [u8],
}

pub(crate) const ITEM_ACTION_LABELS: &[ItemActionLabelSpec] = &[
    action_label(
        Some(0),
        0x13,
        "そうび",
        "japanese_only",
        0x90DA,
        &[0x0E, 0x02, 0x1B, 0x0F, 0xED],
    ),
    action_label(
        Some(1),
        0x14,
        "つかう",
        "japanese_only",
        0x90DF,
        &[0x12, 0x05, 0x02, 0xED],
    ),
    action_label(
        Some(2),
        0x15,
        "わたす",
        "japanese_only",
        0x90E3,
        &[0x2D, 0x10, 0x0C, 0xED],
    ),
    action_label(
        Some(3),
        0x16,
        "すてる",
        "japanese_only",
        0x90E7,
        &[0x0C, 0x13, 0x2A, 0xED],
    ),
    action_label(
        None,
        0x17,
        "NO ITEM",
        "preserve_original_latin",
        0x90EB,
        &[0x77, 0x78, 0xFF, 0x72, 0x7D, 0x6E, 0x76, 0xED],
    ),
];

const fn action_label(
    action_code: Option<u8>,
    index: u8,
    source_text: &'static str,
    translation_scope: &'static str,
    pointer: u16,
    expected: &'static [u8],
) -> ItemActionLabelSpec {
    ItemActionLabelSpec {
        action_code,
        index,
        source_text,
        translation_scope,
        pointer,
        expected,
    }
}

pub(super) fn validate_state_routes(rom: &Rom) -> Result<()> {
    for (state, role, expected_handler) in ITEM_FLOW_STATES {
        let address = MAP_STATE_POINTER_TABLE_ADDRESS + u16::from(*state) * 2;
        let actual = read_u16(rom, 0x06, address)?;
        ensure!(
            actual == *expected_handler,
            "{role} state 0x{state:02X} changed: expected {expected_handler:04X}, found {actual:04X}"
        );
    }

    for (state, expected_handler, role) in ITEM_COMPOSITE_STATES {
        let address = COMPOSITE_POINTER_TABLE_ADDRESS + u16::from(*state) * 2;
        let actual = read_u16(rom, 0x0B, address)?;
        ensure!(
            actual == *expected_handler,
            "{role} for composite state 0x{state:02X} changed"
        );
    }

    let inventory_result = 6_u16;
    let address = COMMAND_ACTION_POINTER_TABLE_ADDRESS + (inventory_result - 1) * 2;
    let handler = read_u16(rom, 0x06, address)?;
    ensure!(
        handler == 0x90B6,
        "unit command result 6 no longer enters item flow"
    );
    Ok(())
}

pub(super) fn validate_action_result_dialogue_indices(rom: &Rom) -> Result<()> {
    let bytes = source_slice(rom, 0x06, 0x9516, ITEM_ACTION_RESULT_DIALOGUE_INDICES.len())?;
    ensure!(
        bytes == ITEM_ACTION_RESULT_DIALOGUE_INDICES,
        "item action result dialogue index table changed"
    );
    Ok(())
}

pub(super) fn validate_vulnerary_family(rom: &Rom) -> Result<()> {
    let item_index = u16::from(VULNERARY_ITEM_ID - 1);
    let default_uses = source_slice(
        rom,
        FIXED_PRG_BANK,
        ITEM_DEFAULT_USES_TABLE_ADDRESS + item_index,
        1,
    )?[0];
    ensure!(
        default_uses == VULNERARY_DEFAULT_USES,
        "item 0x{VULNERARY_ITEM_ID:02X} default uses changed: expected {VULNERARY_DEFAULT_USES}, found {default_uses}"
    );

    let action_flags = source_slice(
        rom,
        FIXED_PRG_BANK,
        ITEM_ACTION_FLAGS_TABLE_ADDRESS + item_index,
        1,
    )?[0];
    ensure!(
        action_flags == VULNERARY_ACTION_FLAGS,
        "item 0x{VULNERARY_ITEM_ID:02X} action flags changed: expected 0x{VULNERARY_ACTION_FLAGS:02X}, found 0x{action_flags:02X}"
    );
    ensure!(
        action_flags & 0x40 != 0,
        "item 0x{VULNERARY_ITEM_ID:02X} no longer exposes the use action"
    );
    Ok(())
}

pub(super) fn validate_item_action_labels(rom: &Rom) -> Result<Vec<FixedLabelBinding>> {
    ITEM_ACTION_LABELS
        .iter()
        .map(|spec| {
            let pointer_address = FIXED_STRING_POINTER_TABLE_ADDRESS + u16::from(spec.index) * 2;
            let actual_pointer = read_u16(rom, 0x0B, pointer_address)?;
            ensure!(
                actual_pointer == spec.pointer,
                "item label 0x{:02X} pointer changed: expected {:04X}, found {:04X}",
                spec.index,
                spec.pointer,
                actual_pointer
            );
            let bytes = source_slice(rom, 0x0B, spec.pointer, spec.expected.len())?;
            ensure!(
                bytes == spec.expected,
                "item label 0x{:02X} bytes changed",
                spec.index
            );
            Ok(FixedLabelBinding {
                index: spec.index,
                index_hex: format!("0x{:02X}", spec.index),
                source_text: spec.source_text,
                translation_scope: spec.translation_scope,
                pointer: spec.pointer,
                pointer_hex: format!("0x{:04X}", spec.pointer),
                bytes_hex: hex(spec.expected),
            })
        })
        .collect()
}

pub(super) fn bind_source_region(rom: &Rom, spec: SourceRegionSpec) -> Result<SourceRegionBinding> {
    let bytes = source_slice(rom, spec.prg_bank, spec.cpu_address, spec.byte_count)?;
    let actual_sha1 = sha1_hex(bytes);
    ensure!(
        actual_sha1 == spec.expected_sha1,
        "{} source changed: expected {}, found {}",
        spec.role,
        spec.expected_sha1,
        actual_sha1
    );
    let file_offset = source_file_offset(spec.prg_bank, spec.cpu_address)?;
    Ok(SourceRegionBinding {
        role: spec.role,
        prg_bank: spec.prg_bank,
        prg_bank_hex: format!("0x{:02X}", spec.prg_bank),
        cpu_address: spec.cpu_address,
        cpu_address_hex: format!("0x{:04X}", spec.cpu_address),
        file_offset,
        file_offset_hex: format!("0x{file_offset:05X}"),
        byte_count: spec.byte_count,
        source_sha1: actual_sha1,
    })
}

pub(super) fn validate_source_region_role(rom: &Rom, role: &str) -> Result<SourceRegionBinding> {
    let spec = SOURCE_REGIONS
        .iter()
        .find(|spec| spec.role == role)
        .with_context(|| format!("unknown item-flow source region role {role}"))?;
    bind_source_region(rom, *spec)
}

fn read_u16(rom: &Rom, prg_bank: u8, cpu_address: u16) -> Result<u16> {
    let bytes = source_slice(rom, prg_bank, cpu_address, 2)?;
    Ok(u16::from_le_bytes([bytes[0], bytes[1]]))
}

pub(super) fn source_slice(rom: &Rom, prg_bank: u8, cpu_address: u16, len: usize) -> Result<&[u8]> {
    let file_offset = source_file_offset(prg_bank, cpu_address)?;
    let end = file_offset
        .checked_add(len)
        .context("item-flow source range overflow")?;
    rom.data()
        .get(file_offset..end)
        .with_context(|| format!("item-flow source range exceeds ROM at {file_offset:05X}"))
}

pub(super) fn source_file_offset(prg_bank: u8, cpu_address: u16) -> Result<usize> {
    if prg_bank == FIXED_PRG_BANK {
        ensure!(
            cpu_address >= FIXED_CPU_START,
            "fixed item-flow address {cpu_address:04X} is below the fixed window"
        );
        return Ok(HEADER_SIZE
            + usize::from(FIXED_PRG_BANK) * PRG_BANK_SIZE
            + usize::from(cpu_address - FIXED_CPU_START));
    }
    ensure!(
        prg_bank < FIXED_PRG_BANK,
        "unavailable PRG bank {prg_bank:02X}"
    );
    ensure!(
        (SWITCHABLE_CPU_START..SWITCHABLE_CPU_END_EXCLUSIVE).contains(&cpu_address),
        "switchable item-flow address {cpu_address:04X} is outside the CPU window"
    );
    Ok(HEADER_SIZE
        + usize::from(prg_bank) * PRG_BANK_SIZE
        + usize::from(cpu_address - SWITCHABLE_CPU_START))
}

fn hex(bytes: &[u8]) -> String {
    bytes
        .iter()
        .map(|byte| format!("{byte:02X}"))
        .collect::<Vec<_>>()
        .join(" ")
}
