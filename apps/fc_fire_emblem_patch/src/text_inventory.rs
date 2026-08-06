use std::{
    collections::{BTreeMap, BTreeSet},
    fs,
    path::Path,
};

use anyhow::{Context, Result, ensure};
use serde::Serialize;

use crate::{
    rom::{EXPECTED_SOURCE_SHA1, HEADER_SIZE, PRG_SIZE, Rom},
    sha1_hex,
    static_analysis::{AbsoluteTransferCandidate, find_absolute_transfer_candidates},
};

const PRG_BANK_SIZE: usize = 16 * 1024;
const FIXED_BANK_CPU_BASE: u16 = 0xC000;
const FIXED_BANK_FILE_OFFSET: usize = HEADER_SIZE + PRG_SIZE - PRG_BANK_SIZE;
const PRG_FILE_END: usize = HEADER_SIZE + PRG_SIZE;
const CHR_TILE_BYTES: usize = 16;
const FIRST_FONT_PAGE_BYTES: usize = 4 * 1024;
const MAX_ENTRY_BYTES: usize = 256;
const COMPOSITE_SEGMENT_SEPARATOR_CODE: u8 = 0xED;
const COMPOSITE_END_CODE: u8 = 0xEF;
const COMPOSITE_OVERLAY_BLANK_CODE: u8 = 0xFF;
const DIALOGUE_LINE_END_CODE: u8 = 0xED;
const DIALOGUE_STAGE_WIDTH_MASK: u8 = 0x1F;
const DIALOGUE_TWO_PLANE_HEADER_FLAG: u8 = 0x40;
const DIALOGUE_LINE_BUFFER_ADDRESSES: [u16; 6] = [0x7832, 0x7852, 0x7872, 0x7892, 0x78B2, 0x78D2];
pub(crate) const DIALOGUE_SCRIPT_CONTROL_CODES: [u8; 15] = [
    0xEA, 0xE0, 0xE9, 0xE3, 0xE2, 0xE1, 0xDF, 0xEF, 0xE7, 0xE4, 0xE6, 0xEE, 0xEB, 0xED, 0xEC,
];

struct TextTableSpec {
    id: &'static str,
    role: &'static str,
    table_file_offset: usize,
    pointer_count: usize,
    terminator: u8,
    consumer_file_offset: usize,
    consumer_bytes: [u8; 10],
    transfer: TextTransferSpec,
    protected_positions: &'static [ProtectedPosition],
}

struct TextTransferSpec {
    source_pointer: &'static str,
    destination: &'static str,
    recognized_stop_codes: &'static [u8],
    destination_end_code: u8,
    destination_end_origin: &'static str,
    explicit_copy_byte_limit: Option<usize>,
    code_regions: &'static [TransferCodeSpec],
}

struct TransferCodeSpec {
    role: &'static str,
    file_offset: usize,
    bytes: &'static [u8],
}

struct ProtectedPosition {
    entry_index: usize,
    byte_offset: usize,
    code: u8,
    glyph: &'static str,
}

pub(crate) struct DialogueControlSpec {
    pub(crate) code: u8,
    pub(crate) current_pointer_advance_bytes: usize,
    pub(crate) inline_operand_byte_count: usize,
    pub(crate) transition_target_byte_count: usize,
    line_effect: &'static str,
    output_effect: &'static str,
    state_effect: &'static str,
    operand_contract: &'static str,
}

const COMPOSITE_TEXT_LAYOUT_CODES: [u8; 2] = [0x0F, 0x1F];

pub(crate) const DIALOGUE_CONTROL_SPECS: [DialogueControlSpec; 15] = [
    DialogueControlSpec {
        code: 0xEA,
        current_pointer_advance_bytes: 1,
        inline_operand_byte_count: 0,
        transition_target_byte_count: 0,
        line_effect: "continue_current_line",
        output_effect: "replace_two_reserved_prefix_cells_with_9E_AB",
        state_effect: "none",
        operand_contract: "none",
    },
    DialogueControlSpec {
        code: 0xE0,
        current_pointer_advance_bytes: 1,
        inline_operand_byte_count: 0,
        transition_target_byte_count: 0,
        line_effect: "continue_current_line",
        output_effect: "none",
        state_effect: "increment_0x7811",
        operand_contract: "none",
    },
    DialogueControlSpec {
        code: 0xE9,
        current_pointer_advance_bytes: 2,
        inline_operand_byte_count: 1,
        transition_target_byte_count: 0,
        line_effect: "continue_current_line",
        output_effect: "none",
        state_effect: "store_operand_in_0x77FF",
        operand_contract: "any_byte",
    },
    DialogueControlSpec {
        code: 0xE3,
        current_pointer_advance_bytes: 1,
        inline_operand_byte_count: 0,
        transition_target_byte_count: 0,
        line_effect: "continue_current_line",
        output_effect: "none",
        state_effect: "increment_0x780E",
        operand_contract: "none",
    },
    DialogueControlSpec {
        code: 0xE2,
        current_pointer_advance_bytes: 1,
        inline_operand_byte_count: 0,
        transition_target_byte_count: 0,
        line_effect: "continue_current_line",
        output_effect: "none",
        state_effect: "increment_0x780F",
        operand_contract: "none",
    },
    DialogueControlSpec {
        code: 0xE1,
        current_pointer_advance_bytes: 2,
        inline_operand_byte_count: 1,
        transition_target_byte_count: 0,
        line_effect: "continue_current_line",
        output_effect: "none",
        state_effect: "store_operand_in_0x7810",
        operand_contract: "any_byte",
    },
    DialogueControlSpec {
        code: 0xDF,
        current_pointer_advance_bytes: 2,
        inline_operand_byte_count: 1,
        transition_target_byte_count: 0,
        line_effect: "continue_current_line",
        output_effect: "none",
        state_effect: "select_0x06F0_slot_and_bit_from_operand_nibbles_when_0x767A_is_zero",
        operand_contract: "high_nibble_selects_slot; low_nibble_indexes_8_byte_bit_table",
    },
    DialogueControlSpec {
        code: 0xEF,
        current_pointer_advance_bytes: 1,
        inline_operand_byte_count: 0,
        transition_target_byte_count: 0,
        line_effect: "finish_current_line_with_0xED",
        output_effect: "none",
        state_effect: "increment_0x7802",
        operand_contract: "none",
    },
    DialogueControlSpec {
        code: 0xE7,
        current_pointer_advance_bytes: 1,
        inline_operand_byte_count: 0,
        transition_target_byte_count: 0,
        line_effect: "finish_current_line_with_0xED",
        output_effect: "none",
        state_effect: "increment_0x7808",
        operand_contract: "none",
    },
    DialogueControlSpec {
        code: 0xE4,
        current_pointer_advance_bytes: 1,
        inline_operand_byte_count: 0,
        transition_target_byte_count: 2,
        line_effect: "finish_current_line_with_0xED",
        output_effect: "none",
        state_effect: "increment_0x780B_and_0x7806; copy_two_transition_target_bytes_to_0x780D_and_0x780C",
        operand_contract: "two_transition_target_bytes_are_read_without_current_pointer_advance",
    },
    DialogueControlSpec {
        code: 0xE6,
        current_pointer_advance_bytes: 1,
        inline_operand_byte_count: 0,
        transition_target_byte_count: 2,
        line_effect: "finish_current_line_with_0xED",
        output_effect: "none",
        state_effect: "increment_0x780A_and_0x7804; copy_two_transition_target_bytes_to_0x780D_and_0x780C",
        operand_contract: "two_transition_target_bytes_are_read_without_current_pointer_advance",
    },
    DialogueControlSpec {
        code: 0xEE,
        current_pointer_advance_bytes: 1,
        inline_operand_byte_count: 0,
        transition_target_byte_count: 0,
        line_effect: "finish_current_line_with_0xED",
        output_effect: "none",
        state_effect: "increment_0x7804",
        operand_contract: "none",
    },
    DialogueControlSpec {
        code: 0xEB,
        current_pointer_advance_bytes: 1,
        inline_operand_byte_count: 0,
        transition_target_byte_count: 0,
        line_effect: "finish_current_line_with_0xED",
        output_effect: "none",
        state_effect: "increment_0x7805_and_0x7806",
        operand_contract: "none",
    },
    DialogueControlSpec {
        code: 0xED,
        current_pointer_advance_bytes: 1,
        inline_operand_byte_count: 0,
        transition_target_byte_count: 0,
        line_effect: "finish_current_line_with_0xED",
        output_effect: "none",
        state_effect: "increment_0x7806",
        operand_contract: "none",
    },
    DialogueControlSpec {
        code: 0xEC,
        current_pointer_advance_bytes: 2,
        inline_operand_byte_count: 1,
        transition_target_byte_count: 0,
        line_effect: "continue_current_line",
        output_effect: "append_selected_sram_string_excluding_0xEF",
        state_effect: "none",
        operand_contract: "operand_0_through_3_selects_one_of_four_sram_strings",
    },
];

const COMPOSITE_TEXT_LAYOUT_CODE_REGIONS: [TransferCodeSpec; 3] = [
    TransferCodeSpec {
        role: "first_pass_decrement_before_append",
        file_offset: 0x2CF59,
        bytes: &[
            0xAD, 0x50, 0x04, 0x48, 0x20, 0xA9, 0x8F, 0xA5, 0x06, 0xC9, 0xEF, 0xF0, 0x3B, 0xC9,
            0xED, 0xF0, 0x0D, 0xC9, 0x0F, 0xF0, 0x08, 0xC9, 0x1F, 0xF0, 0x04, 0xA9, 0xFF, 0xD0,
            0x01, 0xCA, 0x20, 0x9B, 0x8F, 0xA5, 0x06, 0xC9, 0xED, 0xD0, 0xDD,
        ],
    },
    TransferCodeSpec {
        role: "second_pass_skip_combining_codes",
        file_offset: 0x2CF84,
        bytes: &[
            0x20, 0xA9, 0x8F, 0xA5, 0x06, 0xC9, 0xED, 0xF0, 0x08, 0xC9, 0x0F, 0xF0, 0xF3, 0xC9,
            0x1F, 0xF0, 0xEF, 0x20, 0x9B, 0x8F, 0xA5, 0x06, 0xC9, 0xED, 0xD0, 0xE6, 0x4C, 0x49,
            0x8F,
        ],
    },
    TransferCodeSpec {
        role: "append_and_advance_output_cell",
        file_offset: 0x2CFAB,
        bytes: &[
            0x9D, 0x11, 0x03, 0xE8, 0xE0, 0xFF, 0x90, 0x05, 0xA9, 0xEF, 0x9D, 0x10, 0x03, 0x60,
        ],
    },
];

const COMPOSITE_TEXT_CONSUMER_CODE_REGIONS: [TransferCodeSpec; 3] = [
    TransferCodeSpec {
        role: "invoke_two_output_stages",
        file_offset: 0x2D618,
        bytes: &[
            0x20, 0x1C, 0x96, 0xEE, 0xD4, 0x05, 0x20, 0x1C, 0x96, 0x20, 0x01, 0x97, 0xEE, 0xD4,
            0x05, 0xA9, 0x01, 0x85, 0x21, 0x60,
        ],
    },
    TransferCodeSpec {
        role: "bind_composite_output_pointer",
        file_offset: 0x2D62C,
        bytes: &[0xA9, 0x11, 0x85, 0x06, 0xA9, 0x03, 0x85, 0x07],
    },
    TransferCodeSpec {
        role: "consume_with_shared_cursor_and_blank_separator",
        file_offset: 0x2D6F7,
        bytes: &[
            0xAC, 0x10, 0x03, 0xB1, 0x06, 0xC9, 0xED, 0xD0, 0x04, 0xE6, 0x0A, 0xA9, 0xFF, 0xC8,
            0x8C, 0x10, 0x03, 0x9D, 0x01, 0x07, 0xE8, 0xC6, 0x05, 0xD0, 0xDF, 0x60,
        ],
    },
];

const COMPOSITE_TEXT_PPU_CODE_REGIONS: [TransferCodeSpec; 6] = [
    TransferCodeSpec {
        role: "bind_stage_buffer_and_invoke_serializer",
        file_offset: 0x2D6A9,
        bytes: &[
            0xA9, 0x00, 0x85, 0x02, 0xA9, 0x07, 0x85, 0x03, 0xAD, 0xCF, 0x05, 0x09, 0x20, 0x8D,
            0x00, 0x07, 0x20, 0x4E, 0xC8, 0xA9, 0x00, 0x85, 0x21, 0x60,
        ],
    },
    TransferCodeSpec {
        role: "read_stage_descriptor",
        file_offset: 0x3C85E,
        bytes: &[
            0xA0, 0x00, 0xB1, 0x02, 0x29, 0x1F, 0x85, 0x05, 0xB1, 0x02, 0x20, 0x99, 0xC3, 0x85,
            0x04, 0xAE, 0x80, 0x07,
        ],
    },
    TransferCodeSpec {
        role: "serialize_stage_payload",
        file_offset: 0x3C88F,
        bytes: &[
            0xA5, 0x01, 0x20, 0xA2, 0xC4, 0xA5, 0x00, 0x20, 0xA2, 0xC4, 0xA5, 0x06, 0x20, 0xA2,
            0xC4, 0xC8, 0xB1, 0x02, 0x20, 0xA2, 0xC4, 0xC6, 0x06, 0xD0, 0xF6,
        ],
    },
    TransferCodeSpec {
        role: "append_ppu_command_byte",
        file_offset: 0x3C4B2,
        bytes: &[
            0x9D, 0x81, 0x07, 0xE8, 0xE0, 0x5F, 0x90, 0x0A, 0xAE, 0x80, 0x07, 0xA9, 0x00, 0x9D,
            0x81, 0x07, 0x68, 0x68, 0x60,
        ],
    },
    TransferCodeSpec {
        role: "flush_ready_ppu_command_queue",
        file_offset: 0x3C3B5,
        bytes: &[
            0xA5, 0x21, 0xF0, 0x15, 0xA9, 0x81, 0x85, 0x00, 0xA9, 0x07, 0x85, 0x01, 0x20, 0xE7,
            0xC3, 0xA9, 0x00, 0x8D, 0x80, 0x07, 0x8D, 0x81, 0x07, 0x85, 0x21, 0x60,
        ],
    },
    TransferCodeSpec {
        role: "write_queued_codes_to_ppu",
        file_offset: 0x3C3CF,
        bytes: &[
            0x8D, 0x06, 0x20, 0xC8, 0xB1, 0x00, 0x8D, 0x06, 0x20, 0xC8, 0xB1, 0x00, 0x0A, 0x20,
            0xF3, 0xC3, 0x0A, 0xB1, 0x00, 0x29, 0x3F, 0xAA, 0x90, 0x01, 0xC8, 0xB0, 0x01, 0xC8,
            0xB1, 0x00, 0x8D, 0x07, 0x20, 0xCA, 0xD0, 0xF5, 0xC8, 0x20, 0x78, 0xC3, 0xAE, 0x02,
            0x20, 0xA0, 0x00, 0xB1, 0x00, 0xD0, 0xCF, 0x4C, 0x6A, 0xC3,
        ],
    },
];

const COMPOSITE_PLANE_PACKING_CODE_REGIONS: [TransferCodeSpec; 4] = [
    TransferCodeSpec {
        role: "first_parser_call_then_pack",
        file_offset: 0x2C147,
        bytes: &[0x20, 0x39, 0x8F, 0x4C, 0x63, 0x81],
    },
    TransferCodeSpec {
        role: "find_separator_and_prepare_overlapping_copy",
        file_offset: 0x2C173,
        bytes: &[
            0xA2, 0x00, 0xE8, 0xBD, 0x11, 0x03, 0xC9, 0xED, 0xD0, 0xF8, 0x86, 0x04, 0x8A, 0x38,
            0x69, 0x11, 0x85, 0x00, 0x85, 0x02, 0xC6, 0x02, 0xA9, 0x03, 0x85, 0x01, 0x85, 0x03,
            0xA9, 0x00, 0x85, 0x05, 0x20, 0x09, 0xC2, 0x60,
        ],
    },
    TransferCodeSpec {
        role: "second_parser_call_then_pack",
        file_offset: 0x2CB8A,
        bytes: &[0x20, 0x39, 0x8F, 0x4C, 0x63, 0x81],
    },
    TransferCodeSpec {
        role: "copy_base_plane_over_separator",
        file_offset: 0x3C219,
        bytes: &[
            0xA0, 0x00, 0xA6, 0x04, 0xF0, 0x02, 0xE6, 0x05, 0xB1, 0x00, 0x91, 0x02, 0xC8, 0xD0,
            0x04, 0xE6, 0x01, 0xE6, 0x03, 0xC6, 0x04, 0xD0, 0xF1, 0xC6, 0x05, 0xD0, 0xED, 0x60,
        ],
    },
];

const DIALOGUE_SCRIPT_CODE_REGIONS: [TransferCodeSpec; 10] = [
    TransferCodeSpec {
        role: "initialize_sram_line_buffer",
        file_offset: 0x281FA,
        bytes: &[
            0x20, 0x68, 0x86, 0xA9, 0x00, 0x8D, 0xFA, 0x77, 0x8D, 0x07, 0x78, 0x20, 0x3A, 0x83,
            0x20, 0x48, 0x83, 0xA0, 0x00, 0xA9, 0xFF, 0x91, 0x06, 0xC8, 0x91, 0x06, 0xC8, 0x8C,
            0xFB, 0x77,
        ],
    },
    TransferCodeSpec {
        role: "dispatch_script_controls",
        file_offset: 0x28218,
        bytes: &[
            0xAC, 0xFA, 0x77, 0x20, 0x9C, 0xE6, 0xAD, 0x34, 0x79, 0xC9, 0xEA, 0xD0, 0x10, 0xA0,
            0x00, 0xA9, 0x9E, 0x91, 0x06, 0xC8, 0xA9, 0xAB, 0x91, 0x06, 0xEE, 0xFA, 0x77, 0xD0,
            0xE3, 0xC9, 0xE0, 0xD0, 0x08, 0xEE, 0x11, 0x78, 0xEE, 0xFA, 0x77, 0xD0, 0xD7, 0xC9,
            0xE9, 0xD0, 0x05, 0x20, 0x2E, 0x83, 0xD0, 0xCE, 0xC9, 0xE3, 0xD0, 0x08, 0xEE, 0x0E,
            0x78, 0xEE, 0xFA, 0x77, 0xD0, 0xC2, 0xC9, 0xE2, 0xD0, 0x08, 0xEE, 0x0F, 0x78, 0xEE,
            0xFA, 0x77, 0xD0, 0xB6, 0xC9, 0xE1, 0xD0, 0x0D, 0xC8, 0x20, 0x9C, 0xE6, 0x8D, 0x10,
            0x78, 0xC8, 0x8C, 0xFA, 0x77, 0xD0, 0xA5, 0xC9, 0xDF, 0xD0, 0x06, 0x20, 0x1F, 0x83,
            0x4C, 0x08, 0x82, 0xC9, 0xEF, 0xF0, 0x33, 0xC9, 0xE7, 0xF0, 0x34, 0xC9, 0xE4, 0xF0,
            0x35, 0xC9, 0xE6, 0xF0, 0x45, 0xC9, 0xEE, 0xF0, 0x52, 0xC9, 0xEB, 0xF0, 0x53, 0xC9,
            0xED, 0xF0, 0x52, 0xC9, 0xEC, 0xD0, 0x03, 0x4C, 0x64, 0x83,
        ],
    },
    TransferCodeSpec {
        role: "copy_literal_script_byte_to_line",
        file_offset: 0x282A0,
        bytes: &[
            0xAC, 0xFA, 0x77, 0x20, 0x9C, 0xE6, 0xAC, 0xFB, 0x77, 0x91, 0x06, 0xEE, 0xFA, 0x77,
            0xEE, 0xFB, 0x77, 0x4C, 0x08, 0x82,
        ],
    },
    TransferCodeSpec {
        role: "finish_line_controls_and_two_byte_transition_target",
        file_offset: 0x282B4,
        bytes: &[
            0xEE, 0x02, 0x78, 0xD0, 0x35, 0xEE, 0x08, 0x78, 0xD0, 0x30, 0xEE, 0x0B, 0x78, 0xC8,
            0x20, 0x9C, 0xE6, 0x8D, 0x0D, 0x78, 0xC8, 0x20, 0x9C, 0xE6, 0x8D, 0x0C, 0x78, 0x4C,
            0xDB, 0x82, 0xEE, 0x0A, 0x78, 0xC8, 0x20, 0x9C, 0xE6, 0x8D, 0x0D, 0x78, 0xC8, 0x20,
            0x9C, 0xE6, 0x8D, 0x0C, 0x78, 0xEE, 0x04, 0x78, 0xD0, 0x06, 0xEE, 0x05, 0x78, 0xEE,
            0x06, 0x78,
        ],
    },
    TransferCodeSpec {
        role: "finish_line_with_terminator",
        file_offset: 0x282EE,
        bytes: &[
            0xEE, 0xFA, 0x77, 0xA9, 0xED, 0xAC, 0xFB, 0x77, 0x91, 0x06, 0xAE, 0xF0, 0x77, 0xAD,
            0xFA, 0x77, 0x18, 0x7D, 0x12, 0x78, 0x9D, 0x12, 0x78, 0x90, 0x03, 0xFE, 0x14, 0x78,
            0xA9, 0x00, 0x85, 0x2C, 0xA9, 0x01, 0x8D, 0xFC, 0x77, 0xAD, 0xFF, 0x77, 0x85, 0x2D,
            0xEE, 0xF7, 0x77, 0x60,
        ],
    },
    TransferCodeSpec {
        role: "read_banked_script_byte_and_restore_dialogue_bank",
        file_offset: 0x3E6AC,
        bytes: &[
            0xAD, 0xF2, 0x77, 0xF0, 0x03, 0x8D, 0x00, 0xA0, 0xB1, 0x76, 0x8D, 0x34, 0x79, 0xA9,
            0x0A, 0x8D, 0x00, 0xA0, 0xAD, 0x34, 0x79, 0x60,
        ],
    },
    TransferCodeSpec {
        role: "apply_packed_state_bit_operand",
        file_offset: 0x2832F,
        bytes: &[
            0xC8, 0x20, 0x9C, 0xE6, 0x85, 0x02, 0xC8, 0x8C, 0xFA, 0x77, 0xA5, 0x02, 0x4C, 0x9F,
            0xF1,
        ],
    },
    TransferCodeSpec {
        role: "store_progress_delay_operand",
        file_offset: 0x2833E,
        bytes: &[
            0xC8, 0x20, 0x9C, 0xE6, 0x8D, 0xFF, 0x77, 0xC8, 0x8C, 0xFA, 0x77, 0x60,
        ],
    },
    TransferCodeSpec {
        role: "bind_sram_line_buffer_by_slot",
        file_offset: 0x28358,
        bytes: &[
            0xAD, 0xF8, 0x77, 0x0A, 0xA8, 0xB9, 0x58, 0x83, 0x85, 0x06, 0xB9, 0x59, 0x83, 0x85,
            0x07, 0x60, 0x32, 0x78, 0x52, 0x78, 0x72, 0x78, 0x92, 0x78, 0xB2, 0x78, 0xD2, 0x78,
        ],
    },
    TransferCodeSpec {
        role: "append_selected_sram_string",
        file_offset: 0x28374,
        bytes: &[
            0xC8, 0x20, 0x9C, 0xE6, 0xC8, 0x8C, 0xFA, 0x77, 0x0A, 0xA8, 0xB9, 0x97, 0x83, 0x85,
            0x08, 0xC8, 0xB9, 0x97, 0x83, 0x85, 0x09, 0xA0, 0x00, 0x8C, 0xFE, 0x77, 0xAC, 0xFE,
            0x77, 0xB1, 0x08, 0xC9, 0xEF, 0xF0, 0x0D, 0xAC, 0xFB, 0x77, 0x91, 0x06, 0xEE, 0xFB,
            0x77, 0xEE, 0xFE, 0x77, 0xD0, 0xEA, 0x4C, 0x08, 0x82, 0xF2, 0x78, 0x02, 0x79, 0x12,
            0x79, 0x22, 0x79,
        ],
    },
];

const DIALOGUE_PACKED_STATE_BIT_CODE_REGIONS: [TransferCodeSpec; 1] = [TransferCodeSpec {
    role: "select_sram_state_slot_and_bit",
    file_offset: 0x3F1AF,
    bytes: &[
        0xAE, 0x7A, 0x76, 0xD0, 0x12, 0x48, 0x29, 0x0F, 0xAA, 0x68, 0x29, 0xF0, 0x4A, 0x4A, 0x4A,
        0x4A, 0xA8, 0xBD, 0xB7, 0xF1, 0x99, 0xF0, 0x06, 0x60, 0x01, 0x02, 0x04, 0x08, 0x10, 0x20,
        0x40, 0x80,
    ],
}];

const DIALOGUE_RENDERER_CODE_REGIONS: [TransferCodeSpec; 4] = [
    TransferCodeSpec {
        role: "initialize_progressive_two_plane_line",
        file_offset: 0x283CA,
        bytes: &[
            0x20, 0x48, 0x83, 0xA2, 0x00, 0xA0, 0x00, 0x8C, 0xFD, 0x77, 0x8C, 0x07, 0x78,
        ],
    },
    TransferCodeSpec {
        role: "emit_combining_overlay_plane",
        file_offset: 0x283D7,
        bytes: &[
            0xB1, 0x06, 0xC9, 0x0F, 0xF0, 0x0F, 0xC9, 0x1F, 0xF0, 0x0B, 0xC9, 0xED, 0xF0, 0x17,
            0xA9, 0xFF, 0xEE, 0xFD, 0x77, 0xD0, 0x01, 0xCA, 0x9D, 0x11, 0x03, 0xE8, 0xC8, 0xAD,
            0xFD, 0x77, 0xCD, 0xFC, 0x77, 0x90, 0xDD, 0xF0, 0xDB, 0xAD, 0x1A, 0x78, 0x38, 0xED,
            0xFD, 0x77, 0xF0, 0x0C, 0x90, 0x0A, 0xA8, 0xA9, 0xFF, 0x9D, 0x11, 0x03, 0xE8, 0x88,
            0xD0, 0xF9,
        ],
    },
    TransferCodeSpec {
        role: "emit_base_plane",
        file_offset: 0x28411,
        bytes: &[
            0xA0, 0x00, 0x8C, 0xFD, 0x77, 0xB1, 0x06, 0xC9, 0x0F, 0xF0, 0x0D, 0xC9, 0x1F, 0xF0,
            0x09, 0xC9, 0xED, 0xD0, 0x08, 0xEE, 0x07, 0x78, 0xD0, 0x13, 0xC8, 0xD0, 0xEA, 0xEE,
            0xFD, 0x77, 0x9D, 0x11, 0x03, 0xE8, 0xC8, 0xAD, 0xFD, 0x77, 0xCD, 0xFC, 0x77, 0xD0,
            0xDA, 0xAD, 0x1A, 0x78, 0x38, 0xED, 0xFD, 0x77, 0xF0, 0x0E, 0x90, 0x0C, 0xA8, 0xA9,
            0xFF, 0x9D, 0x11, 0x03, 0xE8, 0x88, 0xD0, 0xF9, 0xF0, 0x03, 0xEE, 0x07, 0x78,
        ],
    },
    TransferCodeSpec {
        role: "serialize_two_plane_line_to_ppu_queue",
        file_offset: 0x28456,
        bytes: &[
            0xAD, 0x16, 0x78, 0x85, 0x00, 0xAD, 0x17, 0x78, 0x85, 0x01, 0xAE, 0xF8, 0x77, 0xF0,
            0x09, 0x20, 0x1C, 0xC8, 0x20, 0x1C, 0xC8, 0xCA, 0xD0, 0xF7, 0xA6, 0x00, 0xA4, 0x01,
            0xAD, 0x1A, 0x78, 0x29, 0x1F, 0x09, 0x40, 0x8D, 0x10, 0x03, 0x20, 0x42, 0xC8, 0xAD,
            0x07, 0x78, 0xF0, 0x03, 0xEE, 0xF7, 0x77, 0xEE, 0xFC, 0x77, 0x60,
        ],
    },
];

const TEXT_TABLE_SPECS: [TextTableSpec; 7] = [
    TextTableSpec {
        id: "class-names",
        role: "class names",
        table_file_offset: 0x3DA2F,
        pointer_count: 0x17,
        terminator: 0xEF,
        consumer_file_offset: 0x14D63,
        consumer_bytes: [0xBD, 0x1F, 0xDA, 0x85, 0x00, 0xBD, 0x20, 0xDA, 0x85, 0x01],
        transfer: TextTransferSpec {
            source_pointer: "0x00/0x01",
            destination: "0x7A2B,Y",
            recognized_stop_codes: &[0xEF],
            destination_end_code: 0xEF,
            destination_end_origin: "copied_source_terminator",
            explicit_copy_byte_limit: None,
            code_regions: &[TransferCodeSpec {
                role: "copy_loop",
                file_offset: 0x14D6D,
                bytes: &[
                    0xA0, 0x00, 0xB1, 0x00, 0x99, 0x2B, 0x7A, 0xC8, 0xC9, 0xEF, 0xD0, 0xF6,
                ],
            }],
        },
        protected_positions: &[],
    },
    TextTableSpec {
        id: "item-names",
        role: "item names",
        table_file_offset: 0x3DAE5,
        pointer_count: 0x5B,
        terminator: 0xEF,
        consumer_file_offset: 0x0DC63,
        consumer_bytes: [0xB9, 0xD5, 0xDA, 0x85, 0x00, 0xB9, 0xD6, 0xDA, 0x85, 0x01],
        transfer: TextTransferSpec {
            source_pointer: "0x00/0x01",
            destination: "0x78F2,Y",
            recognized_stop_codes: &[0xEF],
            destination_end_code: 0xEF,
            destination_end_origin: "copied_source_terminator",
            explicit_copy_byte_limit: Some(16),
            code_regions: &[TransferCodeSpec {
                role: "bounded_copy_loop",
                file_offset: 0x0DC6D,
                bytes: &[
                    0xA0, 0x00, 0xB1, 0x00, 0x99, 0xF2, 0x78, 0xC9, 0xEF, 0xF0, 0x05, 0xC8, 0xC0,
                    0x10, 0xD0, 0xF2, 0x60,
                ],
            }],
        },
        protected_positions: &[ProtectedPosition {
            entry_index: 60,
            byte_offset: 1,
            code: 0x9B,
            glyph: ".",
        }],
    },
    TextTableSpec {
        id: "unit-names",
        role: "playable unit names",
        table_file_offset: 0x3DE3B,
        pointer_count: 0x34,
        terminator: 0xEF,
        consumer_file_offset: 0x19B48,
        consumer_bytes: [0xB9, 0x2B, 0xDE, 0x85, 0x00, 0xB9, 0x2C, 0xDE, 0x85, 0x01],
        transfer: TextTransferSpec {
            source_pointer: "0x00/0x01",
            destination: "(0x02/0x03),Y",
            recognized_stop_codes: &[0xEF],
            destination_end_code: 0xEF,
            destination_end_origin: "copied_source_terminator",
            explicit_copy_byte_limit: None,
            code_regions: &[TransferCodeSpec {
                role: "copy_loop",
                file_offset: 0x19B52,
                bytes: &[
                    0xA0, 0x00, 0xB1, 0x00, 0x91, 0x02, 0xC8, 0xC9, 0xEF, 0xD0, 0xF7, 0x60,
                ],
            }],
        },
        protected_positions: &[],
    },
    TextTableSpec {
        id: "enemy-names",
        role: "enemy names",
        table_file_offset: 0x3DFB4,
        pointer_count: 0x44,
        terminator: 0xEF,
        consumer_file_offset: 0x2CEAA,
        consumer_bytes: [0xB9, 0xA4, 0xDF, 0x85, 0x00, 0xB9, 0xA5, 0xDF, 0x85, 0x01],
        transfer: TextTransferSpec {
            source_pointer: "0x00/0x01",
            destination: "0x0451,X",
            recognized_stop_codes: &[0xED, 0xEF],
            destination_end_code: 0xED,
            destination_end_origin: "synthesized_segment_separator",
            explicit_copy_byte_limit: None,
            code_regions: &[
                TransferCodeSpec {
                    role: "call_shared_copy_and_append_separator",
                    file_offset: 0x2CEC0,
                    bytes: &[0x20, 0xFA, 0x8E, 0xA9, 0xED, 0x9D, 0x51, 0x04, 0xE8, 0x60],
                },
                TransferCodeSpec {
                    role: "shared_copy_loop",
                    file_offset: 0x2CF0A,
                    bytes: &[
                        0xA0, 0x00, 0xB1, 0x00, 0xC9, 0xEF, 0xF0, 0x0B, 0x9D, 0x51, 0x04, 0xE8,
                        0xC9, 0xED, 0xF0, 0x03, 0xC8, 0xD0, 0xEF, 0x60,
                    ],
                },
            ],
        },
        protected_positions: &[],
    },
    TextTableSpec {
        id: "terrain-names",
        role: "terrain names",
        table_file_offset: 0x3E601,
        pointer_count: 0x0F,
        terminator: 0xEF,
        consumer_file_offset: 0x1C497,
        consumer_bytes: [0xB9, 0xF1, 0xE5, 0x85, 0x08, 0xB9, 0xF2, 0xE5, 0x85, 0x09],
        transfer: TextTransferSpec {
            source_pointer: "0x08/0x09",
            destination: "0x7953,X",
            recognized_stop_codes: &[0xEF],
            destination_end_code: 0xEF,
            destination_end_origin: "copied_source_terminator",
            explicit_copy_byte_limit: None,
            code_regions: &[TransferCodeSpec {
                role: "copy_loop",
                file_offset: 0x1C2DC,
                bytes: &[
                    0xA0, 0x00, 0xB1, 0x08, 0x9D, 0x53, 0x79, 0xC9, 0xEF, 0xF0, 0x04, 0xE8, 0xC8,
                    0xD0, 0xF3, 0x60,
                ],
            }],
        },
        protected_positions: &[],
    },
    TextTableSpec {
        id: "location-names",
        role: "location names",
        table_file_offset: 0x3EFC7,
        pointer_count: 0x18,
        terminator: 0xED,
        consumer_file_offset: 0x121D0,
        consumer_bytes: [0xB9, 0xB7, 0xEF, 0x85, 0x04, 0xB9, 0xB8, 0xEF, 0x85, 0x05],
        transfer: TextTransferSpec {
            source_pointer: "0x04/0x05",
            destination: "0x7902,Y",
            recognized_stop_codes: &[0xED],
            destination_end_code: 0xEF,
            destination_end_origin: "synthesized_buffer_terminator",
            explicit_copy_byte_limit: None,
            code_regions: &[TransferCodeSpec {
                role: "copy_and_normalize_terminator",
                file_offset: 0x121DA,
                bytes: &[
                    0xA0, 0x00, 0xB1, 0x04, 0xC9, 0xED, 0xF0, 0x06, 0x99, 0x02, 0x79, 0xC8, 0xD0,
                    0xF4, 0xA9, 0xEF, 0x99, 0x02, 0x79, 0x60,
                ],
            }],
        },
        protected_positions: &[],
    },
    TextTableSpec {
        id: "chapter-names",
        role: "chapter names",
        table_file_offset: 0x3EE18,
        pointer_count: 0x18,
        terminator: 0xED,
        consumer_file_offset: 0x2CEF2,
        consumer_bytes: [0xB9, 0x08, 0xEE, 0x85, 0x00, 0xB9, 0x09, 0xEE, 0x85, 0x01],
        transfer: TextTransferSpec {
            source_pointer: "0x00/0x01",
            destination: "0x0451,X",
            recognized_stop_codes: &[0xED, 0xEF],
            destination_end_code: 0xED,
            destination_end_origin: "copied_source_terminator",
            explicit_copy_byte_limit: None,
            code_regions: &[
                TransferCodeSpec {
                    role: "branch_to_shared_copy_loop",
                    file_offset: 0x2CEFC,
                    bytes: &[0xD0, 0x0C],
                },
                TransferCodeSpec {
                    role: "shared_copy_loop",
                    file_offset: 0x2CF0A,
                    bytes: &[
                        0xA0, 0x00, 0xB1, 0x00, 0xC9, 0xEF, 0xF0, 0x0B, 0x9D, 0x51, 0x04, 0xE8,
                        0xC9, 0xED, 0xF0, 0x03, 0xC8, 0xD0, 0xEF, 0x60,
                    ],
                },
            ],
        },
        protected_positions: &[],
    },
];

#[derive(Debug)]
pub struct TextInventorySummary {
    pub report_sha1: String,
    pub table_count: usize,
    pub pointer_count: usize,
    pub unique_string_count: usize,
    pub referenced_protected_original_byte_count: usize,
}

#[derive(Debug, Serialize)]
struct TextInventoryReport {
    schema_version: u8,
    scope: ReportScope,
    summary: ReportSummary,
    source_code_usage: Vec<SourceCodeUsage>,
    layout_controls: Vec<LayoutControlEvidence>,
    dialogue_text_path: DialogueTextPathEvidence,
    tables: Vec<TextTableReport>,
    unknowns: Vec<&'static str>,
}

#[derive(Debug, Serialize)]
struct ReportScope {
    source_sha1: &'static str,
    translation_direction: &'static str,
    preserve_existing_english: bool,
    proof_boundary: &'static str,
}

#[derive(Debug, Serialize)]
struct ReportSummary {
    table_count: usize,
    pointer_count: usize,
    unique_string_count: usize,
    referenced_text_byte_count: usize,
    unique_text_storage_byte_count: usize,
    referenced_protected_original_byte_count: usize,
    unique_protected_original_byte_count: usize,
    referenced_unresolved_byte_count: usize,
    unique_unresolved_byte_count: usize,
    referenced_unresolved_nonblank_font_tile_byte_count: usize,
    unique_unresolved_nonblank_font_tile_byte_count: usize,
    referenced_unresolved_blank_font_tile_byte_count: usize,
    unique_unresolved_blank_font_tile_byte_count: usize,
    distinct_source_code_count: usize,
    distinct_unresolved_nonblank_font_code_count: usize,
    distinct_unresolved_blank_font_code_count: usize,
}

#[derive(Debug, Serialize)]
struct TextTableReport {
    id: &'static str,
    role: &'static str,
    table_file_offset: usize,
    table_file_offset_hex: String,
    table_cpu_address: u16,
    table_cpu_address_hex: String,
    pointer_count: usize,
    unique_string_count: usize,
    pointer_table_sha1: String,
    terminator: u8,
    terminator_hex: String,
    consumer: ConsumerEvidence,
    transfer: TextTransferEvidence,
    data_file_start: usize,
    data_file_start_hex: String,
    data_file_end_exclusive: usize,
    data_file_end_exclusive_hex: String,
    referenced_text_byte_count: usize,
    unique_text_storage_byte_count: usize,
    referenced_protected_original_byte_count: usize,
    unique_protected_original_byte_count: usize,
    referenced_unresolved_byte_count: usize,
    unique_unresolved_byte_count: usize,
    referenced_unresolved_nonblank_font_tile_byte_count: usize,
    unique_unresolved_nonblank_font_tile_byte_count: usize,
    referenced_unresolved_blank_font_tile_byte_count: usize,
    unique_unresolved_blank_font_tile_byte_count: usize,
    source_code_usage: Vec<SourceCodeUsage>,
    entries: Vec<TextEntryReport>,
}

#[derive(Clone, Debug, Serialize)]
struct SourceCodeUsage {
    code: u8,
    code_hex: String,
    font_tile_sha1: String,
    font_tile_all_zero: bool,
    referenced_byte_count: usize,
    unique_storage_byte_count: usize,
    referenced_protected_original_byte_count: usize,
    unique_protected_original_byte_count: usize,
    referenced_unresolved_nonblank_font_tile_byte_count: usize,
    unique_unresolved_nonblank_font_tile_byte_count: usize,
    referenced_unresolved_blank_font_tile_byte_count: usize,
    unique_unresolved_blank_font_tile_byte_count: usize,
}

#[derive(Default)]
struct CodeUsageCounts {
    referenced_byte_count: usize,
    unique_storage_byte_count: usize,
    referenced_protected_original_byte_count: usize,
    unique_protected_original_byte_count: usize,
    referenced_unresolved_nonblank_font_tile_byte_count: usize,
    unique_unresolved_nonblank_font_tile_byte_count: usize,
    referenced_unresolved_blank_font_tile_byte_count: usize,
    unique_unresolved_blank_font_tile_byte_count: usize,
}

#[derive(Debug, Serialize)]
struct ConsumerEvidence {
    file_offset: usize,
    file_offset_hex: String,
    prg_bank: usize,
    prg_bank_hex: String,
    cpu_address: u16,
    cpu_address_hex: String,
    instruction_bytes_hex: String,
    pointer_load_mode: &'static str,
    destination_pointer: String,
}

#[derive(Debug, Serialize)]
struct TextTransferEvidence {
    source_pointer: &'static str,
    destination: &'static str,
    recognized_stop_codes: Vec<u8>,
    recognized_stop_codes_hex: Vec<String>,
    declared_source_terminator: u8,
    declared_source_terminator_hex: String,
    destination_end_code: u8,
    destination_end_code_hex: String,
    destination_end_origin: &'static str,
    explicit_copy_byte_limit: Option<usize>,
    code_regions: Vec<TransferCodeEvidence>,
}

#[derive(Debug, Serialize)]
struct TransferCodeEvidence {
    role: &'static str,
    file_offset: usize,
    file_offset_hex: String,
    prg_bank: usize,
    prg_bank_hex: String,
    cpu_address: u16,
    cpu_address_hex: String,
    instruction_bytes_hex: String,
}

#[derive(Debug, Serialize)]
struct LayoutControlEvidence {
    scope: &'static str,
    entry_cpu_address: u16,
    entry_cpu_address_hex: String,
    source_buffer: &'static str,
    output_buffer: &'static str,
    segment_separator_code: u8,
    segment_separator_code_hex: String,
    end_code: u8,
    end_code_hex: String,
    overlay_blank_code: u8,
    overlay_blank_code_hex: String,
    first_pass_behavior: &'static str,
    second_pass_behavior: &'static str,
    segment_output_order: &'static str,
    codes: Vec<u8>,
    codes_hex: Vec<String>,
    observed_behavior: &'static str,
    inventory_referenced_byte_count: usize,
    inventory_unique_storage_byte_count: usize,
    code_regions: Vec<TransferCodeEvidence>,
    downstream_consumer: CompositeTextConsumerEvidence,
    plane_packing: CompositePlanePackingEvidence,
    direct_jsr_candidates: Vec<AbsoluteTransferCandidate>,
    direct_jmp_candidates: Vec<AbsoluteTransferCandidate>,
}

#[derive(Debug, Serialize)]
struct CompositeTextConsumerEvidence {
    entry_cpu_address: u16,
    entry_cpu_address_hex: String,
    source_buffer_pointer: &'static str,
    source_cursor: &'static str,
    stage_output_buffer: &'static str,
    output_stage_call_count: usize,
    segment_separator_replacement_code: u8,
    segment_separator_replacement_code_hex: String,
    observed_behavior: &'static str,
    code_regions: Vec<TransferCodeEvidence>,
    ppu_transfer: PpuTransferEvidence,
}

#[derive(Debug, Serialize)]
struct PpuTransferEvidence {
    stage_descriptor_buffer: &'static str,
    queued_command_buffer: &'static str,
    queued_command_length: &'static str,
    ready_flag: &'static str,
    serializer_cpu_address: u16,
    serializer_cpu_address_hex: String,
    flush_cpu_address: u16,
    flush_cpu_address_hex: String,
    ppu_address_register: &'static str,
    ppu_data_register: &'static str,
    observed_behavior: &'static str,
    code_regions: Vec<TransferCodeEvidence>,
}

#[derive(Debug, Serialize)]
struct CompositePlanePackingEvidence {
    entry_cpu_address: u16,
    entry_cpu_address_hex: String,
    caller_cpu_addresses: Vec<u16>,
    caller_cpu_addresses_hex: Vec<String>,
    input_buffer: &'static str,
    separator_scan_start_index: usize,
    separator_code: u8,
    separator_code_hex: String,
    copy_source: &'static str,
    copy_destination: &'static str,
    copy_byte_count: &'static str,
    copy_routine_cpu_address: u16,
    copy_routine_cpu_address_hex: String,
    output_layout: &'static str,
    observed_behavior: &'static str,
    code_regions: Vec<TransferCodeEvidence>,
}

#[derive(Debug, Serialize)]
struct DialogueTextPathEvidence {
    script: DialogueScriptEvidence,
    renderer: DialogueRendererEvidence,
    runtime_observation: DialogueRuntimeObservation,
}

#[derive(Debug, Serialize)]
struct DialogueScriptEvidence {
    reader_entry_cpu_address: u16,
    reader_entry_cpu_address_hex: String,
    source_bank_state: &'static str,
    source_pointer: &'static str,
    source_index: &'static str,
    readback_byte: &'static str,
    restored_dialogue_prg_bank: u8,
    restored_dialogue_prg_bank_hex: String,
    line_destination_pointer: &'static str,
    destination_index: &'static str,
    line_buffer_addresses: Vec<u16>,
    line_buffer_addresses_hex: Vec<String>,
    line_buffer_stride_bytes: usize,
    line_end_code: u8,
    line_end_code_hex: String,
    recognized_control_codes: Vec<u8>,
    recognized_control_codes_hex: Vec<String>,
    controls: Vec<DialogueControlEvidence>,
    synthesized_pair_control_code: u8,
    synthesized_pair_control_code_hex: String,
    synthesized_pair_codes: Vec<u8>,
    synthesized_pair_codes_hex: Vec<String>,
    code_regions: Vec<TransferCodeEvidence>,
    packed_state_bit_code_regions: Vec<TransferCodeEvidence>,
}

#[derive(Debug, Serialize)]
struct DialogueControlEvidence {
    code: u8,
    code_hex: String,
    stream_storage_byte_count: usize,
    current_pointer_advance_bytes: usize,
    inline_operand_byte_count: usize,
    transition_target_byte_count: usize,
    line_effect: &'static str,
    output_effect: &'static str,
    state_effect: &'static str,
    operand_contract: &'static str,
}

#[derive(Debug, Serialize)]
struct DialogueRendererEvidence {
    entry_cpu_address: u16,
    entry_cpu_address_hex: String,
    source_pointer: &'static str,
    line_end_code: u8,
    line_end_code_hex: String,
    combining_codes: Vec<u8>,
    combining_codes_hex: Vec<String>,
    overlay_blank_code: u8,
    overlay_blank_code_hex: String,
    line_width_state: &'static str,
    line_width_mask: u8,
    line_width_mask_hex: String,
    visible_code_count: &'static str,
    processed_code_count: &'static str,
    stage_descriptor_buffer: &'static str,
    stage_payload_buffer: &'static str,
    two_plane_header_flag: u8,
    two_plane_header_flag_hex: String,
    encoded_stage_count: usize,
    stage_serializer_entry_cpu_address: u16,
    stage_serializer_entry_cpu_address_hex: String,
    queued_command_buffer: &'static str,
    output_layout: &'static str,
    code_regions: Vec<TransferCodeEvidence>,
}

#[derive(Debug, Serialize)]
struct DialogueRuntimeObservation {
    screen: &'static str,
    source_prg_bank: u8,
    source_prg_bank_hex: String,
    source_cpu_address: u16,
    source_cpu_address_hex: String,
    source_file_offset: usize,
    source_file_offset_hex: String,
    destination_line_buffer_address: u16,
    destination_line_buffer_address_hex: String,
    observed_control_code: u8,
    observed_control_code_hex: String,
    observed_written_code: u8,
    observed_written_code_hex: String,
    source_write_instruction_cpu_address: u16,
    source_write_instruction_cpu_address_hex: String,
    source_write_event_pc: u16,
    source_write_event_pc_hex: String,
    source_write_dropped_event_count: usize,
    observed_stage_descriptor: u8,
    observed_stage_descriptor_hex: String,
    observed_line_width: usize,
    observed_stage_count: usize,
    stage_descriptor_write_instruction_cpu_address: u16,
    stage_descriptor_write_instruction_cpu_address_hex: String,
    stage_descriptor_write_event_pc: u16,
    stage_descriptor_write_event_pc_hex: String,
    stage_descriptor_write_dropped_event_count: usize,
}

#[derive(Debug, Serialize)]
struct TextEntryReport {
    index: usize,
    pointer_cpu_address: u16,
    pointer_cpu_address_hex: String,
    file_offset: usize,
    file_offset_hex: String,
    byte_length: usize,
    raw_bytes_hex: String,
    raw_sha1: String,
    alias_entry_indices: Vec<usize>,
    protected_original: Vec<ProtectedByte>,
    unresolved_byte_count: usize,
}

#[derive(Debug, Serialize)]
struct ProtectedByte {
    byte_offset: usize,
    code: u8,
    code_hex: String,
    glyph: String,
}

pub fn analyze_text_tables(source_path: &Path, report_path: &Path) -> Result<TextInventorySummary> {
    let rom = Rom::from_path(source_path)?;
    rom.verify_supported_japanese()?;
    let report = build_report(rom.data())?;
    let mut report_bytes = serde_json::to_vec_pretty(&report).context("serialize text report")?;
    report_bytes.push(b'\n');

    if let Some(parent) = report_path.parent() {
        fs::create_dir_all(parent)
            .with_context(|| format!("create output directory {}", parent.display()))?;
    }
    fs::write(report_path, &report_bytes)
        .with_context(|| format!("write {}", report_path.display()))?;

    Ok(TextInventorySummary {
        report_sha1: sha1_hex(&report_bytes),
        table_count: report.summary.table_count,
        pointer_count: report.summary.pointer_count,
        unique_string_count: report.summary.unique_string_count,
        referenced_protected_original_byte_count: report
            .summary
            .referenced_protected_original_byte_count,
    })
}

fn build_report(source: &[u8]) -> Result<TextInventoryReport> {
    let tables: Vec<TextTableReport> = TEXT_TABLE_SPECS
        .iter()
        .map(|spec| extract_table(source, spec))
        .collect::<Result<_>>()?;
    let pointer_count = tables.iter().map(|table| table.pointer_count).sum();
    let unique_string_count = tables.iter().map(|table| table.unique_string_count).sum();
    let referenced_text_byte_count = tables
        .iter()
        .map(|table| table.referenced_text_byte_count)
        .sum();
    let unique_text_storage_byte_count = tables
        .iter()
        .map(|table| table.unique_text_storage_byte_count)
        .sum();
    let referenced_protected_original_byte_count = tables
        .iter()
        .map(|table| table.referenced_protected_original_byte_count)
        .sum();
    let unique_protected_original_byte_count = tables
        .iter()
        .map(|table| table.unique_protected_original_byte_count)
        .sum();
    let referenced_unresolved_byte_count = tables
        .iter()
        .map(|table| table.referenced_unresolved_byte_count)
        .sum();
    let unique_unresolved_byte_count = tables
        .iter()
        .map(|table| table.unique_unresolved_byte_count)
        .sum();
    let referenced_unresolved_nonblank_font_tile_byte_count = tables
        .iter()
        .map(|table| table.referenced_unresolved_nonblank_font_tile_byte_count)
        .sum();
    let unique_unresolved_nonblank_font_tile_byte_count = tables
        .iter()
        .map(|table| table.unique_unresolved_nonblank_font_tile_byte_count)
        .sum();
    let referenced_unresolved_blank_font_tile_byte_count = tables
        .iter()
        .map(|table| table.referenced_unresolved_blank_font_tile_byte_count)
        .sum();
    let unique_unresolved_blank_font_tile_byte_count = tables
        .iter()
        .map(|table| table.unique_unresolved_blank_font_tile_byte_count)
        .sum();
    let source_code_usage = aggregate_source_code_usage(source, &tables)?;
    let distinct_source_code_count = source_code_usage.len();
    let distinct_unresolved_nonblank_font_code_count = source_code_usage
        .iter()
        .filter(|usage| usage.referenced_unresolved_nonblank_font_tile_byte_count != 0)
        .count();
    let distinct_unresolved_blank_font_code_count = source_code_usage
        .iter()
        .filter(|usage| usage.referenced_unresolved_blank_font_tile_byte_count != 0)
        .count();

    let layout_controls = build_layout_control_evidence(source, &source_code_usage)?;
    let dialogue_text_path = build_dialogue_text_path_evidence(source)?;

    Ok(TextInventoryReport {
        schema_version: 12,
        scope: ReportScope {
            source_sha1: EXPECTED_SOURCE_SHA1,
            translation_direction: "ja_to_ko",
            preserve_existing_english: true,
            proof_boundary: "confirmed pointer tables, transfer code, first-page CHR tile storage, the bank 0B menu and title composite path, and the bank 0A dialogue ROM-to-SRAM-to-PPU path; the complete text population remains unresolved",
        },
        summary: ReportSummary {
            table_count: tables.len(),
            pointer_count,
            unique_string_count,
            referenced_text_byte_count,
            unique_text_storage_byte_count,
            referenced_protected_original_byte_count,
            unique_protected_original_byte_count,
            referenced_unresolved_byte_count,
            unique_unresolved_byte_count,
            referenced_unresolved_nonblank_font_tile_byte_count,
            unique_unresolved_nonblank_font_tile_byte_count,
            referenced_unresolved_blank_font_tile_byte_count,
            unique_unresolved_blank_font_tile_byte_count,
            distinct_source_code_count,
            distinct_unresolved_nonblank_font_code_count,
            distinct_unresolved_blank_font_code_count,
        },
        source_code_usage,
        layout_controls,
        dialogue_text_path,
        tables,
        unknowns: vec![
            "This is not the complete game text population.",
            "Non-Latin bytes remain unresolved Japanese, layout, icon, or control codes until decoder semantics are proven.",
            "Direct composite-parser JSR and JMP candidates are byte-pattern matches; instruction boundaries and caller roles remain unconfirmed.",
            "Dialogue control pointer progression, storage spans, and structural effects are confirmed, but their complete gameplay meaning and valid arguments across the full script population remain unresolved.",
            "The runtime observation identifies one chapter 1 script location and line buffer, not the complete dialogue script population.",
            "No entry is translation-ready until control tokens, layout, and relocation policy are declared.",
        ],
    })
}

fn extract_table(source: &[u8], spec: &TextTableSpec) -> Result<TextTableReport> {
    ensure!(
        source.len() >= PRG_FILE_END + FIRST_FONT_PAGE_BYTES,
        "source is shorter than the PRG region and first CHR font page"
    );
    validate_consumer(source, spec)?;
    let transfer = build_transfer_evidence(source, spec)?;

    let table_byte_length = spec
        .pointer_count
        .checked_mul(2)
        .context("pointer table length overflow")?;
    let table_end = spec
        .table_file_offset
        .checked_add(table_byte_length)
        .context("pointer table range overflow")?;
    ensure!(
        (FIXED_BANK_FILE_OFFSET..=PRG_FILE_END).contains(&spec.table_file_offset)
            && table_end <= PRG_FILE_END,
        "text table {} is outside the fixed PRG bank",
        spec.id
    );
    let table_bytes = &source[spec.table_file_offset..table_end];
    let pointers: Vec<u16> = table_bytes
        .chunks_exact(2)
        .map(|bytes| u16::from_le_bytes([bytes[0], bytes[1]]))
        .collect();
    let mut pointer_indices: BTreeMap<u16, Vec<usize>> = BTreeMap::new();
    for (index, pointer) in pointers.iter().enumerate() {
        pointer_indices.entry(*pointer).or_default().push(index);
    }

    let mut ranges = Vec::new();
    let mut entries = Vec::with_capacity(pointers.len());
    let mut code_usage_counts: BTreeMap<u8, CodeUsageCounts> = BTreeMap::new();
    for (index, pointer) in pointers.iter().enumerate() {
        let file_offset = fixed_cpu_to_file_offset(*pointer)
            .with_context(|| format!("{} entry {index}", spec.id))?;
        ensure!(
            file_offset >= table_end,
            "{} entry {index} points into or before its pointer table",
            spec.id
        );
        let search_end = file_offset
            .checked_add(MAX_ENTRY_BYTES + 1)
            .unwrap_or(PRG_FILE_END)
            .min(PRG_FILE_END);
        let terminator_offset = source[file_offset..search_end]
            .iter()
            .position(|byte| *byte == spec.terminator)
            .map(|relative| file_offset + relative)
            .with_context(|| {
                format!(
                    "{} entry {index} has no {:02X} terminator within {MAX_ENTRY_BYTES} bytes",
                    spec.id, spec.terminator
                )
            })?;
        let raw = &source[file_offset..terminator_offset];
        if let Some(limit) = spec.transfer.explicit_copy_byte_limit {
            ensure!(
                raw.len() < limit,
                "{} entry {index} needs {} bytes including its terminator, beyond the consumer limit {limit}",
                spec.id,
                raw.len() + 1
            );
        }
        for position in spec
            .protected_positions
            .iter()
            .filter(|position| position.entry_index == index)
        {
            ensure!(
                raw.get(position.byte_offset) == Some(&position.code),
                "protected original byte changed for {} entry {index} at byte {}",
                spec.id,
                position.byte_offset
            );
        }
        let alias_entry_indices = pointer_indices[pointer]
            .iter()
            .copied()
            .filter(|other| *other != index)
            .collect();
        let mut protected_original = Vec::new();
        for (byte_offset, code) in raw.iter().enumerate() {
            let declared = spec.protected_positions.iter().find(|position| {
                position.entry_index == index && position.byte_offset == byte_offset
            });
            let glyph = if let Some(position) = declared {
                ensure!(
                    *code == position.code,
                    "protected original byte changed for {} entry {index} at byte {byte_offset}",
                    spec.id
                );
                Some(position.glyph)
            } else {
                protected_alphanumeric_glyph(*code)
            };
            if let Some(glyph) = glyph {
                protected_original.push(ProtectedByte {
                    byte_offset,
                    code: *code,
                    code_hex: format!("{code:02X}"),
                    glyph: glyph.to_owned(),
                });
            }
        }
        let unresolved_byte_count = raw.len() - protected_original.len();
        let protected_offsets: BTreeSet<usize> = protected_original
            .iter()
            .map(|protected| protected.byte_offset)
            .collect();
        let is_unique_storage = pointer_indices[pointer][0] == index;
        for (byte_offset, code) in raw.iter().enumerate() {
            let counts = code_usage_counts.entry(*code).or_default();
            counts.referenced_byte_count += 1;
            counts.unique_storage_byte_count += usize::from(is_unique_storage);
            if protected_offsets.contains(&byte_offset) {
                counts.referenced_protected_original_byte_count += 1;
                counts.unique_protected_original_byte_count += usize::from(is_unique_storage);
            } else if font_tile(source, *code)?.iter().all(|byte| *byte == 0) {
                counts.referenced_unresolved_blank_font_tile_byte_count += 1;
                counts.unique_unresolved_blank_font_tile_byte_count +=
                    usize::from(is_unique_storage);
            } else {
                counts.referenced_unresolved_nonblank_font_tile_byte_count += 1;
                counts.unique_unresolved_nonblank_font_tile_byte_count +=
                    usize::from(is_unique_storage);
            }
        }

        ranges.push((file_offset, terminator_offset + 1));
        entries.push(TextEntryReport {
            index,
            pointer_cpu_address: *pointer,
            pointer_cpu_address_hex: format!("0x{pointer:04X}"),
            file_offset,
            file_offset_hex: format!("0x{file_offset:05X}"),
            byte_length: raw.len(),
            raw_bytes_hex: hex_bytes(raw),
            raw_sha1: sha1_hex(raw),
            alias_entry_indices,
            protected_original,
            unresolved_byte_count,
        });
    }
    validate_unique_ranges(spec.id, &ranges)?;

    let data_file_start = ranges
        .iter()
        .map(|(start, _)| *start)
        .min()
        .context("text table has no entries")?;
    let data_file_end_exclusive = ranges
        .iter()
        .map(|(_, end)| *end)
        .max()
        .context("text table has no entries")?;
    let referenced_protected_original_byte_count = entries
        .iter()
        .map(|entry| entry.protected_original.len())
        .sum();
    let referenced_unresolved_byte_count = entries
        .iter()
        .map(|entry| entry.unresolved_byte_count)
        .sum();
    let referenced_text_byte_count = entries.iter().map(|entry| entry.byte_length).sum();
    let first_entry_for_pointer = pointers
        .iter()
        .enumerate()
        .filter(|(index, pointer)| pointer_indices[pointer][0] == *index)
        .map(|(index, _)| &entries[index])
        .collect::<Vec<_>>();
    let unique_text_storage_byte_count = first_entry_for_pointer
        .iter()
        .map(|entry| entry.byte_length)
        .sum();
    let unique_protected_original_byte_count = first_entry_for_pointer
        .iter()
        .map(|entry| entry.protected_original.len())
        .sum();
    let unique_unresolved_byte_count = first_entry_for_pointer
        .iter()
        .map(|entry| entry.unresolved_byte_count)
        .sum();
    let source_code_usage = source_code_usage(source, code_usage_counts)?;
    let referenced_unresolved_nonblank_font_tile_byte_count = source_code_usage
        .iter()
        .map(|usage| usage.referenced_unresolved_nonblank_font_tile_byte_count)
        .sum();
    let unique_unresolved_nonblank_font_tile_byte_count = source_code_usage
        .iter()
        .map(|usage| usage.unique_unresolved_nonblank_font_tile_byte_count)
        .sum();
    let referenced_unresolved_blank_font_tile_byte_count = source_code_usage
        .iter()
        .map(|usage| usage.referenced_unresolved_blank_font_tile_byte_count)
        .sum();
    let unique_unresolved_blank_font_tile_byte_count = source_code_usage
        .iter()
        .map(|usage| usage.unique_unresolved_blank_font_tile_byte_count)
        .sum();
    ensure!(
        referenced_unresolved_byte_count
            == referenced_unresolved_nonblank_font_tile_byte_count
                + referenced_unresolved_blank_font_tile_byte_count,
        "font-tile classification does not cover referenced unresolved bytes for {}",
        spec.id
    );
    ensure!(
        unique_unresolved_byte_count
            == unique_unresolved_nonblank_font_tile_byte_count
                + unique_unresolved_blank_font_tile_byte_count,
        "font-tile classification does not cover unique unresolved bytes for {}",
        spec.id
    );
    let table_cpu_address = fixed_file_to_cpu_address(spec.table_file_offset)?;
    let (consumer_prg_bank, consumer_cpu_address) = prg_file_location(spec.consumer_file_offset)?;
    let destination_pointer = format!(
        "0x{:02X}/0x{:02X}",
        spec.consumer_bytes[4], spec.consumer_bytes[9]
    );

    Ok(TextTableReport {
        id: spec.id,
        role: spec.role,
        table_file_offset: spec.table_file_offset,
        table_file_offset_hex: format!("0x{:05X}", spec.table_file_offset),
        table_cpu_address,
        table_cpu_address_hex: format!("0x{table_cpu_address:04X}"),
        pointer_count: spec.pointer_count,
        unique_string_count: pointer_indices.len(),
        pointer_table_sha1: sha1_hex(table_bytes),
        terminator: spec.terminator,
        terminator_hex: format!("{:02X}", spec.terminator),
        consumer: ConsumerEvidence {
            file_offset: spec.consumer_file_offset,
            file_offset_hex: format!("0x{:05X}", spec.consumer_file_offset),
            prg_bank: consumer_prg_bank,
            prg_bank_hex: format!("0x{consumer_prg_bank:02X}"),
            cpu_address: consumer_cpu_address,
            cpu_address_hex: format!("0x{consumer_cpu_address:04X}"),
            instruction_bytes_hex: hex_bytes(&spec.consumer_bytes),
            pointer_load_mode: if spec.consumer_bytes[0] == 0xBD {
                "absolute_x"
            } else {
                "absolute_y"
            },
            destination_pointer,
        },
        transfer,
        data_file_start,
        data_file_start_hex: format!("0x{data_file_start:05X}"),
        data_file_end_exclusive,
        data_file_end_exclusive_hex: format!("0x{data_file_end_exclusive:05X}"),
        referenced_text_byte_count,
        unique_text_storage_byte_count,
        referenced_protected_original_byte_count,
        unique_protected_original_byte_count,
        referenced_unresolved_byte_count,
        unique_unresolved_byte_count,
        referenced_unresolved_nonblank_font_tile_byte_count,
        unique_unresolved_nonblank_font_tile_byte_count,
        referenced_unresolved_blank_font_tile_byte_count,
        unique_unresolved_blank_font_tile_byte_count,
        source_code_usage,
        entries,
    })
}

fn validate_consumer(source: &[u8], spec: &TextTableSpec) -> Result<()> {
    let end = spec
        .consumer_file_offset
        .checked_add(spec.consumer_bytes.len())
        .context("consumer range overflow")?;
    ensure!(end <= PRG_FILE_END, "consumer {} is outside PRG", spec.id);
    ensure!(
        source[spec.consumer_file_offset..end] == spec.consumer_bytes,
        "consumer bytes changed for {} at {:#X}",
        spec.id,
        spec.consumer_file_offset
    );

    let table_cpu_address = fixed_file_to_cpu_address(spec.table_file_offset)?;
    let next_address = table_cpu_address + 1;
    let opcode = spec.consumer_bytes[0];
    ensure!(
        [0xBD, 0xB9].contains(&opcode)
            && spec.consumer_bytes[3] == 0x85
            && spec.consumer_bytes[5] == opcode
            && spec.consumer_bytes[8] == 0x85,
        "consumer {} is not the declared indexed pointer load",
        spec.id
    );
    ensure!(
        spec.consumer_bytes[1..3] == table_cpu_address.to_le_bytes()
            && spec.consumer_bytes[6..8] == next_address.to_le_bytes(),
        "consumer {} does not load its pointer table",
        spec.id
    );
    ensure!(
        spec.consumer_bytes[9] == spec.consumer_bytes[4] + 1,
        "consumer {} does not store an adjacent pointer pair",
        spec.id
    );
    Ok(())
}

fn build_transfer_evidence(source: &[u8], spec: &TextTableSpec) -> Result<TextTransferEvidence> {
    ensure!(
        spec.transfer
            .recognized_stop_codes
            .contains(&spec.terminator),
        "transfer for {} does not recognize its declared terminator",
        spec.id
    );
    ensure!(
        !spec.transfer.code_regions.is_empty(),
        "transfer for {} has no code evidence",
        spec.id
    );

    let code_regions =
        build_code_region_evidence(source, spec.transfer.code_regions, "transfer", spec.id)?;

    Ok(TextTransferEvidence {
        source_pointer: spec.transfer.source_pointer,
        destination: spec.transfer.destination,
        recognized_stop_codes: spec.transfer.recognized_stop_codes.to_vec(),
        recognized_stop_codes_hex: spec
            .transfer
            .recognized_stop_codes
            .iter()
            .map(|code| format!("{code:02X}"))
            .collect(),
        declared_source_terminator: spec.terminator,
        declared_source_terminator_hex: format!("{:02X}", spec.terminator),
        destination_end_code: spec.transfer.destination_end_code,
        destination_end_code_hex: format!("{:02X}", spec.transfer.destination_end_code),
        destination_end_origin: spec.transfer.destination_end_origin,
        explicit_copy_byte_limit: spec.transfer.explicit_copy_byte_limit,
        code_regions,
    })
}

fn build_layout_control_evidence(
    source: &[u8],
    source_code_usage: &[SourceCodeUsage],
) -> Result<Vec<LayoutControlEvidence>> {
    let mut inventory_referenced_byte_count = 0;
    let mut inventory_unique_storage_byte_count = 0;
    for code in COMPOSITE_TEXT_LAYOUT_CODES {
        let usage = source_code_usage
            .iter()
            .find(|usage| usage.code == code)
            .with_context(|| format!("layout code {code:02X} is absent from the text inventory"))?;
        inventory_referenced_byte_count += usage.referenced_byte_count;
        inventory_unique_storage_byte_count += usage.unique_storage_byte_count;
    }

    Ok(vec![LayoutControlEvidence {
        scope: "bank_0B_composite_text_parser",
        entry_cpu_address: 0x8F39,
        entry_cpu_address_hex: "0x8F39".to_owned(),
        source_buffer: "0x0451",
        output_buffer: "0x0311",
        segment_separator_code: COMPOSITE_SEGMENT_SEPARATOR_CODE,
        segment_separator_code_hex: format!("{COMPOSITE_SEGMENT_SEPARATOR_CODE:02X}"),
        end_code: COMPOSITE_END_CODE,
        end_code_hex: format!("{COMPOSITE_END_CODE:02X}"),
        overlay_blank_code: COMPOSITE_OVERLAY_BLANK_CODE,
        overlay_blank_code_hex: format!("{COMPOSITE_OVERLAY_BLANK_CODE:02X}"),
        first_pass_behavior: "emit_blank_cells_and_replace_the_previous_blank_with_a_zero_cell_combining_code",
        second_pass_behavior: "emit_base_codes_while_skipping_combining_codes",
        segment_output_order: "combining_overlay_then_base_codes",
        codes: COMPOSITE_TEXT_LAYOUT_CODES.to_vec(),
        codes_hex: COMPOSITE_TEXT_LAYOUT_CODES
            .iter()
            .map(|code| format!("{code:02X}"))
            .collect(),
        observed_behavior: "zero_cell_combining_diacritic",
        inventory_referenced_byte_count,
        inventory_unique_storage_byte_count,
        code_regions: build_code_region_evidence(
            source,
            &COMPOSITE_TEXT_LAYOUT_CODE_REGIONS,
            "layout control",
            "bank_0B_composite_text_parser",
        )?,
        downstream_consumer: CompositeTextConsumerEvidence {
            entry_cpu_address: 0x9608,
            entry_cpu_address_hex: "0x9608".to_owned(),
            source_buffer_pointer: "0x06/0x07 = 0x0311",
            source_cursor: "0x0310",
            stage_output_buffer: "0x0701,X",
            output_stage_call_count: 2,
            segment_separator_replacement_code: COMPOSITE_OVERLAY_BLANK_CODE,
            segment_separator_replacement_code_hex: format!("{COMPOSITE_OVERLAY_BLANK_CODE:02X}"),
            observed_behavior: "two_output_stage_calls_consume_the_composite_buffer_through_one_shared_cursor",
            code_regions: build_code_region_evidence(
                source,
                &COMPOSITE_TEXT_CONSUMER_CODE_REGIONS,
                "downstream consumer",
                "bank_0B_composite_text_parser",
            )?,
            ppu_transfer: PpuTransferEvidence {
                stage_descriptor_buffer: "0x0700",
                queued_command_buffer: "0x0781",
                queued_command_length: "0x0780",
                ready_flag: "0x21",
                serializer_cpu_address: 0xC84E,
                serializer_cpu_address_hex: "0xC84E".to_owned(),
                flush_cpu_address: 0xC3A5,
                flush_cpu_address_hex: "0xC3A5".to_owned(),
                ppu_address_register: "0x2006",
                ppu_data_register: "0x2007",
                observed_behavior: "serialize_stage_codes_into_a_command_queue_then_flush_them_to_ppu",
                code_regions: build_code_region_evidence(
                    source,
                    &COMPOSITE_TEXT_PPU_CODE_REGIONS,
                    "PPU transfer",
                    "bank_0B_composite_text_parser",
                )?,
            },
        },
        plane_packing: CompositePlanePackingEvidence {
            entry_cpu_address: 0x8163,
            entry_cpu_address_hex: "0x8163".to_owned(),
            caller_cpu_addresses: vec![0x8137, 0x8B7A],
            caller_cpu_addresses_hex: vec!["0x8137".to_owned(), "0x8B7A".to_owned()],
            input_buffer: "0x0311",
            separator_scan_start_index: 1,
            separator_code: COMPOSITE_SEGMENT_SEPARATOR_CODE,
            separator_code_hex: format!("{COMPOSITE_SEGMENT_SEPARATOR_CODE:02X}"),
            copy_source: "byte_after_first_0xED",
            copy_destination: "first_0xED",
            copy_byte_count: "first_0xED_index",
            copy_routine_cpu_address: 0xC209,
            copy_routine_cpu_address_hex: "0xC209".to_owned(),
            output_layout: "combining_overlay_then_base_codes_without_interplane_separator",
            observed_behavior: "shift_base_plane_left_over_first_separator_by_overlay_plane_width",
            code_regions: build_code_region_evidence(
                source,
                &COMPOSITE_PLANE_PACKING_CODE_REGIONS,
                "plane packing",
                "bank_0B_composite_text_parser",
            )?,
        },
        direct_jsr_candidates: find_absolute_transfer_candidates(
            &source[HEADER_SIZE..PRG_FILE_END],
            0x8F39,
            0x20,
        ),
        direct_jmp_candidates: find_absolute_transfer_candidates(
            &source[HEADER_SIZE..PRG_FILE_END],
            0x8F39,
            0x4C,
        ),
    }])
}

fn build_dialogue_text_path_evidence(source: &[u8]) -> Result<DialogueTextPathEvidence> {
    let recognized_control_codes = DIALOGUE_SCRIPT_CONTROL_CODES.to_vec();
    let controls = DIALOGUE_CONTROL_SPECS
        .iter()
        .map(|control| DialogueControlEvidence {
            code: control.code,
            code_hex: format!("{:02X}", control.code),
            stream_storage_byte_count: 1
                + control.inline_operand_byte_count
                + control.transition_target_byte_count,
            current_pointer_advance_bytes: control.current_pointer_advance_bytes,
            inline_operand_byte_count: control.inline_operand_byte_count,
            transition_target_byte_count: control.transition_target_byte_count,
            line_effect: control.line_effect,
            output_effect: control.output_effect,
            state_effect: control.state_effect,
            operand_contract: control.operand_contract,
        })
        .collect::<Vec<_>>();
    ensure!(
        controls
            .iter()
            .map(|control| control.code)
            .eq(DIALOGUE_SCRIPT_CONTROL_CODES),
        "dialogue control declarations do not match the dispatcher order"
    );
    ensure!(
        controls
            .iter()
            .all(|control| control.current_pointer_advance_bytes
                == control.inline_operand_byte_count + 1),
        "dialogue control source advance does not match its consumed operands"
    );
    ensure!(
        controls
            .iter()
            .all(|control| control.stream_storage_byte_count
                == control.inline_operand_byte_count + control.transition_target_byte_count + 1),
        "dialogue control storage length does not cover its inline and transition bytes"
    );
    let synthesized_pair_codes = vec![0x9E, 0xAB];
    let combining_codes = COMPOSITE_TEXT_LAYOUT_CODES.to_vec();

    Ok(DialogueTextPathEvidence {
        script: DialogueScriptEvidence {
            reader_entry_cpu_address: 0xE69C,
            reader_entry_cpu_address_hex: "0xE69C".to_owned(),
            source_bank_state: "0x77F2",
            source_pointer: "0x76/0x77",
            source_index: "0x77FA",
            readback_byte: "0x7934",
            restored_dialogue_prg_bank: 0x0A,
            restored_dialogue_prg_bank_hex: "0x0A".to_owned(),
            line_destination_pointer: "0x06/0x07",
            destination_index: "0x77FB",
            line_buffer_addresses: DIALOGUE_LINE_BUFFER_ADDRESSES.to_vec(),
            line_buffer_addresses_hex: DIALOGUE_LINE_BUFFER_ADDRESSES
                .iter()
                .map(|address| format!("0x{address:04X}"))
                .collect(),
            line_buffer_stride_bytes: 0x20,
            line_end_code: DIALOGUE_LINE_END_CODE,
            line_end_code_hex: format!("{DIALOGUE_LINE_END_CODE:02X}"),
            recognized_control_codes,
            recognized_control_codes_hex: DIALOGUE_SCRIPT_CONTROL_CODES
                .iter()
                .map(|code| format!("{code:02X}"))
                .collect(),
            controls,
            synthesized_pair_control_code: 0xEA,
            synthesized_pair_control_code_hex: "EA".to_owned(),
            synthesized_pair_codes: synthesized_pair_codes.clone(),
            synthesized_pair_codes_hex: synthesized_pair_codes
                .iter()
                .map(|code| format!("{code:02X}"))
                .collect(),
            code_regions: build_code_region_evidence(
                source,
                &DIALOGUE_SCRIPT_CODE_REGIONS,
                "dialogue script",
                "bank_0A_dialogue_script_loader",
            )?,
            packed_state_bit_code_regions: build_code_region_evidence(
                source,
                &DIALOGUE_PACKED_STATE_BIT_CODE_REGIONS,
                "dialogue state bit",
                "packed_dialogue_state_operand",
            )?,
        },
        renderer: DialogueRendererEvidence {
            entry_cpu_address: 0x83BA,
            entry_cpu_address_hex: "0x83BA".to_owned(),
            source_pointer: "0x06/0x07",
            line_end_code: DIALOGUE_LINE_END_CODE,
            line_end_code_hex: format!("{DIALOGUE_LINE_END_CODE:02X}"),
            combining_codes: combining_codes.clone(),
            combining_codes_hex: combining_codes
                .iter()
                .map(|code| format!("{code:02X}"))
                .collect(),
            overlay_blank_code: COMPOSITE_OVERLAY_BLANK_CODE,
            overlay_blank_code_hex: format!("{COMPOSITE_OVERLAY_BLANK_CODE:02X}"),
            line_width_state: "0x781A",
            line_width_mask: DIALOGUE_STAGE_WIDTH_MASK,
            line_width_mask_hex: format!("{DIALOGUE_STAGE_WIDTH_MASK:02X}"),
            visible_code_count: "0x77FC",
            processed_code_count: "0x77FD",
            stage_descriptor_buffer: "0x0310",
            stage_payload_buffer: "0x0311",
            two_plane_header_flag: DIALOGUE_TWO_PLANE_HEADER_FLAG,
            two_plane_header_flag_hex: format!("{DIALOGUE_TWO_PLANE_HEADER_FLAG:02X}"),
            encoded_stage_count: usize::from(DIALOGUE_TWO_PLANE_HEADER_FLAG >> 5),
            stage_serializer_entry_cpu_address: 0xC842,
            stage_serializer_entry_cpu_address_hex: "0xC842".to_owned(),
            queued_command_buffer: "0x0781",
            output_layout: "combining_overlay_then_base_codes_with_equal_masked_width",
            code_regions: build_code_region_evidence(
                source,
                &DIALOGUE_RENDERER_CODE_REGIONS,
                "dialogue renderer",
                "bank_0A_progressive_dialogue_renderer",
            )?,
        },
        runtime_observation: DialogueRuntimeObservation {
            screen: "chapter_1_intro_dialogue",
            source_prg_bank: 0x08,
            source_prg_bank_hex: "0x08".to_owned(),
            source_cpu_address: 0x9FCE,
            source_cpu_address_hex: "0x9FCE".to_owned(),
            source_file_offset: 0x21FDE,
            source_file_offset_hex: "0x21FDE".to_owned(),
            destination_line_buffer_address: 0x78B2,
            destination_line_buffer_address_hex: "0x78B2".to_owned(),
            observed_control_code: 0xEA,
            observed_control_code_hex: "EA".to_owned(),
            observed_written_code: 0x9E,
            observed_written_code_hex: "9E".to_owned(),
            source_write_instruction_cpu_address: 0x8219,
            source_write_instruction_cpu_address_hex: "0x8219".to_owned(),
            source_write_event_pc: 0x821B,
            source_write_event_pc_hex: "0x821B".to_owned(),
            source_write_dropped_event_count: 0,
            observed_stage_descriptor: 0x52,
            observed_stage_descriptor_hex: "52".to_owned(),
            observed_line_width: usize::from(0x52 & DIALOGUE_STAGE_WIDTH_MASK),
            observed_stage_count: usize::from(0x52_u8 >> 5),
            stage_descriptor_write_instruction_cpu_address: 0x8469,
            stage_descriptor_write_instruction_cpu_address_hex: "0x8469".to_owned(),
            stage_descriptor_write_event_pc: 0x846C,
            stage_descriptor_write_event_pc_hex: "0x846C".to_owned(),
            stage_descriptor_write_dropped_event_count: 0,
        },
    })
}

fn build_code_region_evidence(
    source: &[u8],
    regions: &[TransferCodeSpec],
    evidence_kind: &str,
    owner: &str,
) -> Result<Vec<TransferCodeEvidence>> {
    regions
        .iter()
        .map(|region| {
            let end = region
                .file_offset
                .checked_add(region.bytes.len())
                .with_context(|| format!("{evidence_kind} code range overflow"))?;
            ensure!(
                end <= PRG_FILE_END,
                "{evidence_kind} code {} for {owner} is outside PRG",
                region.role
            );
            ensure!(
                source[region.file_offset..end] == *region.bytes,
                "{evidence_kind} code {} changed for {owner} at {:#X}",
                region.role,
                region.file_offset
            );
            let (prg_bank, cpu_address) = prg_file_location(region.file_offset)?;
            Ok(TransferCodeEvidence {
                role: region.role,
                file_offset: region.file_offset,
                file_offset_hex: format!("0x{:05X}", region.file_offset),
                prg_bank,
                prg_bank_hex: format!("0x{prg_bank:02X}"),
                cpu_address,
                cpu_address_hex: format!("0x{cpu_address:04X}"),
                instruction_bytes_hex: hex_bytes(region.bytes),
            })
        })
        .collect()
}

fn aggregate_source_code_usage(
    source: &[u8],
    tables: &[TextTableReport],
) -> Result<Vec<SourceCodeUsage>> {
    let mut aggregate: BTreeMap<u8, CodeUsageCounts> = BTreeMap::new();
    for usage in tables
        .iter()
        .flat_map(|table| table.source_code_usage.iter())
    {
        let counts = aggregate.entry(usage.code).or_default();
        counts.referenced_byte_count += usage.referenced_byte_count;
        counts.unique_storage_byte_count += usage.unique_storage_byte_count;
        counts.referenced_protected_original_byte_count +=
            usage.referenced_protected_original_byte_count;
        counts.unique_protected_original_byte_count += usage.unique_protected_original_byte_count;
        counts.referenced_unresolved_nonblank_font_tile_byte_count +=
            usage.referenced_unresolved_nonblank_font_tile_byte_count;
        counts.unique_unresolved_nonblank_font_tile_byte_count +=
            usage.unique_unresolved_nonblank_font_tile_byte_count;
        counts.referenced_unresolved_blank_font_tile_byte_count +=
            usage.referenced_unresolved_blank_font_tile_byte_count;
        counts.unique_unresolved_blank_font_tile_byte_count +=
            usage.unique_unresolved_blank_font_tile_byte_count;
    }
    source_code_usage(source, aggregate)
}

fn source_code_usage(
    source: &[u8],
    counts_by_code: BTreeMap<u8, CodeUsageCounts>,
) -> Result<Vec<SourceCodeUsage>> {
    counts_by_code
        .into_iter()
        .map(|(code, counts)| {
            let tile = font_tile(source, code)?;
            Ok(SourceCodeUsage {
                code,
                code_hex: format!("{code:02X}"),
                font_tile_sha1: sha1_hex(tile),
                font_tile_all_zero: tile.iter().all(|byte| *byte == 0),
                referenced_byte_count: counts.referenced_byte_count,
                unique_storage_byte_count: counts.unique_storage_byte_count,
                referenced_protected_original_byte_count: counts
                    .referenced_protected_original_byte_count,
                unique_protected_original_byte_count: counts.unique_protected_original_byte_count,
                referenced_unresolved_nonblank_font_tile_byte_count: counts
                    .referenced_unresolved_nonblank_font_tile_byte_count,
                unique_unresolved_nonblank_font_tile_byte_count: counts
                    .unique_unresolved_nonblank_font_tile_byte_count,
                referenced_unresolved_blank_font_tile_byte_count: counts
                    .referenced_unresolved_blank_font_tile_byte_count,
                unique_unresolved_blank_font_tile_byte_count: counts
                    .unique_unresolved_blank_font_tile_byte_count,
            })
        })
        .collect()
}

fn font_tile(source: &[u8], code: u8) -> Result<&[u8]> {
    let start = PRG_FILE_END + usize::from(code) * CHR_TILE_BYTES;
    let end = start + CHR_TILE_BYTES;
    source
        .get(start..end)
        .with_context(|| format!("font tile {code:02X} is outside the source image"))
}

fn validate_unique_ranges(id: &str, ranges: &[(usize, usize)]) -> Result<()> {
    let unique: BTreeSet<(usize, usize)> = ranges.iter().copied().collect();
    let sorted = unique.iter().copied().collect::<Vec<_>>();
    for pair in sorted.windows(2) {
        ensure!(
            pair[0].1 <= pair[1].0,
            "text table {id} contains overlapping string ranges"
        );
    }
    Ok(())
}

fn fixed_cpu_to_file_offset(cpu_address: u16) -> Result<usize> {
    ensure!(
        cpu_address >= FIXED_BANK_CPU_BASE,
        "pointer ${cpu_address:04X} is outside the fixed PRG bank"
    );
    Ok(FIXED_BANK_FILE_OFFSET + usize::from(cpu_address - FIXED_BANK_CPU_BASE))
}

fn fixed_file_to_cpu_address(file_offset: usize) -> Result<u16> {
    ensure!(
        (FIXED_BANK_FILE_OFFSET..PRG_FILE_END).contains(&file_offset),
        "file offset {file_offset:#X} is outside the fixed PRG bank"
    );
    Ok(FIXED_BANK_CPU_BASE + (file_offset - FIXED_BANK_FILE_OFFSET) as u16)
}

fn prg_file_location(file_offset: usize) -> Result<(usize, u16)> {
    ensure!(
        (HEADER_SIZE..PRG_FILE_END).contains(&file_offset),
        "file offset {file_offset:#X} is outside PRG"
    );
    let prg_offset = file_offset - HEADER_SIZE;
    let prg_bank = prg_offset / PRG_BANK_SIZE;
    let offset_in_bank = prg_offset % PRG_BANK_SIZE;
    let cpu_base = if prg_bank == PRG_SIZE / PRG_BANK_SIZE - 1 {
        0xC000
    } else {
        0x8000
    };
    Ok((prg_bank, cpu_base + offset_in_bank as u16))
}

fn protected_alphanumeric_glyph(code: u8) -> Option<&'static str> {
    const DIGITS: [&str; 10] = ["0", "1", "2", "3", "4", "5", "6", "7", "8", "9"];
    const UPPERCASE: [&str; 26] = [
        "A", "B", "C", "D", "E", "F", "G", "H", "I", "J", "K", "L", "M", "N", "O", "P", "Q", "R",
        "S", "T", "U", "V", "W", "X", "Y", "Z",
    ];
    match code {
        0x60..=0x69 => Some(DIGITS[(code - 0x60) as usize]),
        0x6A..=0x83 => Some(UPPERCASE[(code - 0x6A) as usize]),
        _ => None,
    }
}

fn hex_bytes(bytes: &[u8]) -> String {
    bytes
        .iter()
        .map(|byte| format!("{byte:02X}"))
        .collect::<Vec<_>>()
        .join(" ")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn synthetic_spec(table_file_offset: usize, consumer_file_offset: usize) -> TextTableSpec {
        let table_cpu_address = fixed_file_to_cpu_address(table_file_offset).unwrap();
        let [low, high] = table_cpu_address.to_le_bytes();
        let [next_low, next_high] = (table_cpu_address + 1).to_le_bytes();
        TextTableSpec {
            id: "synthetic-names",
            role: "synthetic names",
            table_file_offset,
            pointer_count: 2,
            terminator: 0xEF,
            consumer_file_offset,
            consumer_bytes: [
                0xB9, low, high, 0x85, 0x00, 0xB9, next_low, next_high, 0x85, 0x01,
            ],
            transfer: TextTransferSpec {
                source_pointer: "0x00/0x01",
                destination: "synthetic-buffer",
                recognized_stop_codes: &[0xEF],
                destination_end_code: 0xEF,
                destination_end_origin: "copied_source_terminator",
                explicit_copy_byte_limit: None,
                code_regions: &[TransferCodeSpec {
                    role: "copy_loop",
                    file_offset: HEADER_SIZE + 0x0300,
                    bytes: &[0xA0, 0x00, 0xB1, 0x00, 0xC9, 0xEF, 0x60],
                }],
            },
            protected_positions: &[],
        }
    }

    fn write_declared_code(source: &mut [u8], spec: &TextTableSpec) {
        source[spec.consumer_file_offset..spec.consumer_file_offset + spec.consumer_bytes.len()]
            .copy_from_slice(&spec.consumer_bytes);
        for region in spec.transfer.code_regions {
            source[region.file_offset..region.file_offset + region.bytes.len()]
                .copy_from_slice(region.bytes);
        }
    }

    fn write_code_regions(source: &mut [u8], regions: &[TransferCodeSpec]) {
        for region in regions {
            source[region.file_offset..region.file_offset + region.bytes.len()]
                .copy_from_slice(region.bytes);
        }
    }

    #[test]
    fn extracts_aliases_without_translating_preserved_latin() {
        let table_file_offset = FIXED_BANK_FILE_OFFSET + 0x0100;
        let consumer_file_offset = HEADER_SIZE + 0x0200;
        let spec = synthetic_spec(table_file_offset, consumer_file_offset);
        let mut source = vec![0_u8; PRG_FILE_END + FIRST_FONT_PAGE_BYTES];
        write_declared_code(&mut source, &spec);
        let text_cpu_address = FIXED_BANK_CPU_BASE + 0x0200;
        let pointer = text_cpu_address.to_le_bytes();
        source[table_file_offset..table_file_offset + 2].copy_from_slice(&pointer);
        source[table_file_offset + 2..table_file_offset + 4].copy_from_slice(&pointer);
        let text_file_offset = FIXED_BANK_FILE_OFFSET + 0x0200;
        source[text_file_offset..text_file_offset + 4].copy_from_slice(&[0x6A, 0x30, 0x60, 0xEF]);
        source[PRG_FILE_END + 0x30 * CHR_TILE_BYTES] = 0x80;

        let report = extract_table(&source, &spec).unwrap();

        assert_eq!(report.pointer_count, 2);
        assert_eq!(report.unique_string_count, 1);
        assert_eq!(report.entries[0].alias_entry_indices, vec![1]);
        assert_eq!(report.entries[1].alias_entry_indices, vec![0]);
        assert_eq!(report.entries[0].protected_original.len(), 2);
        assert_eq!(report.entries[0].protected_original[0].glyph, "A");
        assert_eq!(report.entries[0].protected_original[1].glyph, "0");
        assert_eq!(report.entries[0].unresolved_byte_count, 1);
        assert_eq!(
            report.referenced_unresolved_nonblank_font_tile_byte_count,
            2
        );
        assert_eq!(report.unique_unresolved_nonblank_font_tile_byte_count, 1);
        let usage = report
            .source_code_usage
            .iter()
            .find(|usage| usage.code == 0x30)
            .unwrap();
        assert!(!usage.font_tile_all_zero);
        assert_eq!(usage.referenced_byte_count, 2);
        assert_eq!(usage.unique_storage_byte_count, 1);
    }

    #[test]
    fn protects_punctuation_only_at_a_declared_token_position() {
        let table_file_offset = FIXED_BANK_FILE_OFFSET + 0x0100;
        let consumer_file_offset = HEADER_SIZE + 0x0200;
        let mut spec = synthetic_spec(table_file_offset, consumer_file_offset);
        spec.protected_positions = &[ProtectedPosition {
            entry_index: 0,
            byte_offset: 1,
            code: 0x9B,
            glyph: ".",
        }];
        let mut source = vec![0_u8; PRG_FILE_END + FIRST_FONT_PAGE_BYTES];
        write_declared_code(&mut source, &spec);
        for (index, text_offset) in [0x0200_u16, 0x0210].iter().enumerate() {
            let pointer = (FIXED_BANK_CPU_BASE + *text_offset).to_le_bytes();
            let pointer_offset = table_file_offset + index * 2;
            source[pointer_offset..pointer_offset + 2].copy_from_slice(&pointer);
            let text_file_offset = FIXED_BANK_FILE_OFFSET + usize::from(*text_offset);
            source[text_file_offset..text_file_offset + 4]
                .copy_from_slice(&[0x76, 0x9B, 0x30, 0xEF]);
        }

        let report = extract_table(&source, &spec).unwrap();

        assert_eq!(report.entries[0].protected_original.len(), 2);
        assert_eq!(report.entries[0].protected_original[0].glyph, "M");
        assert_eq!(report.entries[0].protected_original[1].glyph, ".");
        assert_eq!(report.entries[1].protected_original.len(), 1);
        assert_eq!(report.entries[1].unresolved_byte_count, 2);
        assert_eq!(report.referenced_unresolved_blank_font_tile_byte_count, 3);
        assert_eq!(
            report.referenced_unresolved_nonblank_font_tile_byte_count,
            0
        );
    }

    #[test]
    fn rejects_consumer_bytes_that_no_longer_load_the_table() {
        let table_file_offset = FIXED_BANK_FILE_OFFSET + 0x0100;
        let consumer_file_offset = HEADER_SIZE + 0x0200;
        let spec = synthetic_spec(table_file_offset, consumer_file_offset);
        let mut source = vec![0_u8; PRG_FILE_END + FIRST_FONT_PAGE_BYTES];
        source[consumer_file_offset..consumer_file_offset + 10]
            .copy_from_slice(&spec.consumer_bytes);
        source[consumer_file_offset + 1] ^= 0x01;

        let error = validate_consumer(&source, &spec).unwrap_err().to_string();

        assert!(error.contains("consumer bytes changed for synthetic-names"));
    }

    #[test]
    fn rejects_transfer_code_that_no_longer_implements_the_declared_path() {
        let table_file_offset = FIXED_BANK_FILE_OFFSET + 0x0100;
        let consumer_file_offset = HEADER_SIZE + 0x0200;
        let spec = synthetic_spec(table_file_offset, consumer_file_offset);
        let mut source = vec![0_u8; PRG_FILE_END + FIRST_FONT_PAGE_BYTES];
        write_declared_code(&mut source, &spec);
        source[HEADER_SIZE + 0x0300] ^= 0x01;

        let error = build_transfer_evidence(&source, &spec)
            .unwrap_err()
            .to_string();

        assert!(error.contains("transfer code copy_loop changed for synthetic-names"));
    }

    #[test]
    fn rejects_composite_layout_code_that_no_longer_preserves_combining_width() {
        let mut source = vec![0_u8; PRG_FILE_END + FIRST_FONT_PAGE_BYTES];
        for region in &COMPOSITE_TEXT_LAYOUT_CODE_REGIONS {
            source[region.file_offset..region.file_offset + region.bytes.len()]
                .copy_from_slice(region.bytes);
        }
        source[COMPOSITE_TEXT_LAYOUT_CODE_REGIONS[0].file_offset + 28] ^= 0x01;

        let error = build_code_region_evidence(
            &source,
            &COMPOSITE_TEXT_LAYOUT_CODE_REGIONS,
            "layout control",
            "bank_0B_composite_text_parser",
        )
        .unwrap_err()
        .to_string();

        assert!(error.contains(
            "layout control code first_pass_decrement_before_append changed for bank_0B_composite_text_parser"
        ));
    }

    #[test]
    fn declares_composite_overlay_and_base_passes_separately() {
        let mut source = vec![0_u8; PRG_FILE_END + FIRST_FONT_PAGE_BYTES];
        for region in &COMPOSITE_TEXT_LAYOUT_CODE_REGIONS {
            source[region.file_offset..region.file_offset + region.bytes.len()]
                .copy_from_slice(region.bytes);
        }
        for region in &COMPOSITE_TEXT_CONSUMER_CODE_REGIONS {
            source[region.file_offset..region.file_offset + region.bytes.len()]
                .copy_from_slice(region.bytes);
        }
        for region in &COMPOSITE_TEXT_PPU_CODE_REGIONS {
            source[region.file_offset..region.file_offset + region.bytes.len()]
                .copy_from_slice(region.bytes);
        }
        for region in &COMPOSITE_PLANE_PACKING_CODE_REGIONS {
            source[region.file_offset..region.file_offset + region.bytes.len()]
                .copy_from_slice(region.bytes);
        }
        let source_code_usage = COMPOSITE_TEXT_LAYOUT_CODES.map(|code| SourceCodeUsage {
            code,
            code_hex: format!("{code:02X}"),
            font_tile_sha1: String::new(),
            font_tile_all_zero: false,
            referenced_byte_count: 1,
            unique_storage_byte_count: 1,
            referenced_protected_original_byte_count: 0,
            unique_protected_original_byte_count: 0,
            referenced_unresolved_nonblank_font_tile_byte_count: 1,
            unique_unresolved_nonblank_font_tile_byte_count: 1,
            referenced_unresolved_blank_font_tile_byte_count: 0,
            unique_unresolved_blank_font_tile_byte_count: 0,
        });

        let evidence = build_layout_control_evidence(&source, &source_code_usage).unwrap();
        let composite = &evidence[0];

        assert_eq!(composite.source_buffer, "0x0451");
        assert_eq!(composite.output_buffer, "0x0311");
        assert_eq!(composite.segment_separator_code, 0xED);
        assert_eq!(composite.end_code, 0xEF);
        assert_eq!(composite.overlay_blank_code, 0xFF);
        assert_eq!(
            composite.first_pass_behavior,
            "emit_blank_cells_and_replace_the_previous_blank_with_a_zero_cell_combining_code"
        );
        assert_eq!(
            composite.second_pass_behavior,
            "emit_base_codes_while_skipping_combining_codes"
        );
        assert_eq!(
            composite.segment_output_order,
            "combining_overlay_then_base_codes"
        );
        assert_eq!(
            composite.downstream_consumer.source_buffer_pointer,
            "0x06/0x07 = 0x0311"
        );
        assert_eq!(composite.downstream_consumer.source_cursor, "0x0310");
        assert_eq!(composite.downstream_consumer.output_stage_call_count, 2);
        assert_eq!(
            composite
                .downstream_consumer
                .segment_separator_replacement_code,
            0xFF
        );
        assert_eq!(
            composite
                .downstream_consumer
                .ppu_transfer
                .stage_descriptor_buffer,
            "0x0700"
        );
        assert_eq!(
            composite
                .downstream_consumer
                .ppu_transfer
                .queued_command_buffer,
            "0x0781"
        );
        assert_eq!(
            composite
                .downstream_consumer
                .ppu_transfer
                .ppu_address_register,
            "0x2006"
        );
        assert_eq!(
            composite.downstream_consumer.ppu_transfer.ppu_data_register,
            "0x2007"
        );
        assert_eq!(composite.plane_packing.entry_cpu_address, 0x8163);
        assert_eq!(
            composite.plane_packing.caller_cpu_addresses,
            vec![0x8137, 0x8B7A]
        );
        assert_eq!(composite.plane_packing.copy_source, "byte_after_first_0xED");
        assert_eq!(composite.plane_packing.copy_destination, "first_0xED");
        assert_eq!(
            composite.plane_packing.output_layout,
            "combining_overlay_then_base_codes_without_interplane_separator"
        );
        assert_eq!(
            composite
                .direct_jsr_candidates
                .iter()
                .map(|candidate| candidate.cpu_address)
                .collect::<Vec<_>>(),
            composite.plane_packing.caller_cpu_addresses
        );
    }

    #[test]
    fn rejects_changed_composite_downstream_consumer_code() {
        let mut source = vec![0_u8; PRG_FILE_END + FIRST_FONT_PAGE_BYTES];
        for region in &COMPOSITE_TEXT_CONSUMER_CODE_REGIONS {
            source[region.file_offset..region.file_offset + region.bytes.len()]
                .copy_from_slice(region.bytes);
        }
        source[COMPOSITE_TEXT_CONSUMER_CODE_REGIONS[1].file_offset + 1] ^= 0x01;

        let error = build_code_region_evidence(
            &source,
            &COMPOSITE_TEXT_CONSUMER_CODE_REGIONS,
            "downstream consumer",
            "bank_0B_composite_text_parser",
        )
        .unwrap_err()
        .to_string();

        assert!(error.contains(
            "downstream consumer code bind_composite_output_pointer changed for bank_0B_composite_text_parser"
        ));
    }

    #[test]
    fn rejects_changed_composite_ppu_transfer_code() {
        let mut source = vec![0_u8; PRG_FILE_END + FIRST_FONT_PAGE_BYTES];
        for region in &COMPOSITE_TEXT_PPU_CODE_REGIONS {
            source[region.file_offset..region.file_offset + region.bytes.len()]
                .copy_from_slice(region.bytes);
        }
        source[COMPOSITE_TEXT_PPU_CODE_REGIONS[5].file_offset + 31] ^= 0x01;

        let error = build_code_region_evidence(
            &source,
            &COMPOSITE_TEXT_PPU_CODE_REGIONS,
            "PPU transfer",
            "bank_0B_composite_text_parser",
        )
        .unwrap_err()
        .to_string();

        assert!(error.contains(
            "PPU transfer code write_queued_codes_to_ppu changed for bank_0B_composite_text_parser"
        ));
    }

    #[test]
    fn rejects_changed_composite_plane_packing_code() {
        let mut source = vec![0_u8; PRG_FILE_END + FIRST_FONT_PAGE_BYTES];
        for region in &COMPOSITE_PLANE_PACKING_CODE_REGIONS {
            source[region.file_offset..region.file_offset + region.bytes.len()]
                .copy_from_slice(region.bytes);
        }
        source[COMPOSITE_PLANE_PACKING_CODE_REGIONS[1].file_offset + 19] ^= 0x01;

        let error = build_code_region_evidence(
            &source,
            &COMPOSITE_PLANE_PACKING_CODE_REGIONS,
            "plane packing",
            "bank_0B_composite_text_parser",
        )
        .unwrap_err()
        .to_string();

        assert!(error.contains(
            "plane packing code find_separator_and_prepare_overlapping_copy changed for bank_0B_composite_text_parser"
        ));
    }

    #[test]
    fn declares_dialogue_script_and_progressive_two_plane_renderer() {
        let mut source = vec![0_u8; PRG_FILE_END + FIRST_FONT_PAGE_BYTES];
        write_code_regions(&mut source, &DIALOGUE_SCRIPT_CODE_REGIONS);
        write_code_regions(&mut source, &DIALOGUE_PACKED_STATE_BIT_CODE_REGIONS);
        write_code_regions(&mut source, &DIALOGUE_RENDERER_CODE_REGIONS);

        let evidence = build_dialogue_text_path_evidence(&source).unwrap();

        assert_eq!(evidence.script.reader_entry_cpu_address, 0xE69C);
        assert_eq!(evidence.script.source_bank_state, "0x77F2");
        assert_eq!(evidence.script.source_pointer, "0x76/0x77");
        assert_eq!(evidence.script.line_destination_pointer, "0x06/0x07");
        assert_eq!(
            evidence.script.line_buffer_addresses,
            DIALOGUE_LINE_BUFFER_ADDRESSES
        );
        assert_eq!(evidence.script.line_buffer_stride_bytes, 0x20);
        assert_eq!(evidence.script.line_end_code, 0xED);
        assert_eq!(
            evidence.script.recognized_control_codes,
            DIALOGUE_SCRIPT_CONTROL_CODES
        );
        let finish_with_transition = evidence
            .script
            .controls
            .iter()
            .find(|control| control.code == 0xE4)
            .unwrap();
        assert_eq!(finish_with_transition.stream_storage_byte_count, 3);
        assert_eq!(finish_with_transition.current_pointer_advance_bytes, 1);
        assert_eq!(finish_with_transition.inline_operand_byte_count, 0);
        assert_eq!(finish_with_transition.transition_target_byte_count, 2);
        assert_eq!(
            finish_with_transition.line_effect,
            "finish_current_line_with_0xED"
        );
        let insert_sram_string = evidence
            .script
            .controls
            .iter()
            .find(|control| control.code == 0xEC)
            .unwrap();
        assert_eq!(insert_sram_string.current_pointer_advance_bytes, 2);
        assert_eq!(insert_sram_string.stream_storage_byte_count, 2);
        assert_eq!(insert_sram_string.inline_operand_byte_count, 1);
        assert_eq!(
            insert_sram_string.output_effect,
            "append_selected_sram_string_excluding_0xEF"
        );
        assert_eq!(evidence.script.synthesized_pair_control_code, 0xEA);
        assert_eq!(evidence.script.synthesized_pair_codes, vec![0x9E, 0xAB]);
        assert_eq!(evidence.renderer.entry_cpu_address, 0x83BA);
        assert_eq!(evidence.renderer.combining_codes, vec![0x0F, 0x1F]);
        assert_eq!(evidence.renderer.line_width_mask, 0x1F);
        assert_eq!(evidence.renderer.two_plane_header_flag, 0x40);
        assert_eq!(evidence.renderer.encoded_stage_count, 2);
        assert_eq!(evidence.renderer.stage_serializer_entry_cpu_address, 0xC842);
        assert_eq!(evidence.runtime_observation.source_prg_bank, 0x08);
        assert_eq!(evidence.runtime_observation.source_cpu_address, 0x9FCE);
        assert_eq!(evidence.runtime_observation.source_file_offset, 0x21FDE);
        assert_eq!(
            evidence.runtime_observation.destination_line_buffer_address,
            0x78B2
        );
        assert_eq!(evidence.runtime_observation.source_write_event_pc, 0x821B);
        assert_eq!(evidence.runtime_observation.observed_stage_descriptor, 0x52);
        assert_eq!(evidence.runtime_observation.observed_line_width, 18);
        assert_eq!(evidence.runtime_observation.observed_stage_count, 2);
        assert_eq!(
            evidence.runtime_observation.stage_descriptor_write_event_pc,
            0x846C
        );
    }

    #[test]
    fn rejects_changed_dialogue_script_or_renderer_code() {
        let mut source = vec![0_u8; PRG_FILE_END + FIRST_FONT_PAGE_BYTES];
        write_code_regions(&mut source, &DIALOGUE_SCRIPT_CODE_REGIONS);
        write_code_regions(&mut source, &DIALOGUE_PACKED_STATE_BIT_CODE_REGIONS);
        write_code_regions(&mut source, &DIALOGUE_RENDERER_CODE_REGIONS);
        source[DIALOGUE_SCRIPT_CODE_REGIONS[2].file_offset + 9] ^= 0x01;

        let error = build_dialogue_text_path_evidence(&source)
            .unwrap_err()
            .to_string();

        assert!(error.contains(
            "dialogue script code copy_literal_script_byte_to_line changed for bank_0A_dialogue_script_loader"
        ));

        write_code_regions(&mut source, &DIALOGUE_SCRIPT_CODE_REGIONS);
        source[DIALOGUE_RENDERER_CODE_REGIONS[3].file_offset + 31] ^= 0x01;

        let error = build_dialogue_text_path_evidence(&source)
            .unwrap_err()
            .to_string();

        assert!(error.contains(
            "dialogue renderer code serialize_two_plane_line_to_ppu_queue changed for bank_0A_progressive_dialogue_renderer"
        ));
    }

    #[test]
    fn rejects_changed_dialogue_packed_state_bit_routine() {
        let mut source = vec![0_u8; PRG_FILE_END + FIRST_FONT_PAGE_BYTES];
        write_code_regions(&mut source, &DIALOGUE_SCRIPT_CODE_REGIONS);
        write_code_regions(&mut source, &DIALOGUE_PACKED_STATE_BIT_CODE_REGIONS);
        write_code_regions(&mut source, &DIALOGUE_RENDERER_CODE_REGIONS);
        source[DIALOGUE_PACKED_STATE_BIT_CODE_REGIONS[0].file_offset + 18] ^= 0x01;

        let error = build_dialogue_text_path_evidence(&source)
            .unwrap_err()
            .to_string();

        assert!(error.contains(
            "dialogue state bit code select_sram_state_slot_and_bit changed for packed_dialogue_state_operand"
        ));
    }

    #[test]
    fn rejects_text_that_exceeds_the_consumers_explicit_copy_limit() {
        let table_file_offset = FIXED_BANK_FILE_OFFSET + 0x0100;
        let consumer_file_offset = HEADER_SIZE + 0x0200;
        let mut spec = synthetic_spec(table_file_offset, consumer_file_offset);
        spec.transfer.explicit_copy_byte_limit = Some(3);
        let mut source = vec![0_u8; PRG_FILE_END + FIRST_FONT_PAGE_BYTES];
        write_declared_code(&mut source, &spec);
        let text_cpu_address = FIXED_BANK_CPU_BASE + 0x0200;
        let pointer = text_cpu_address.to_le_bytes();
        source[table_file_offset..table_file_offset + 2].copy_from_slice(&pointer);
        source[table_file_offset + 2..table_file_offset + 4].copy_from_slice(&pointer);
        let text_file_offset = FIXED_BANK_FILE_OFFSET + 0x0200;
        source[text_file_offset..text_file_offset + 4].copy_from_slice(&[0x30, 0x31, 0x32, 0xEF]);

        let error = extract_table(&source, &spec).unwrap_err().to_string();

        assert!(error.contains("needs 4 bytes including its terminator"));
        assert!(error.contains("consumer limit 3"));
    }
}
