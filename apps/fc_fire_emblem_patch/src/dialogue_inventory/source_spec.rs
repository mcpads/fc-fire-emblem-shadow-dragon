pub(super) const PRG_BANK_SIZE: usize = 16 * 1024;
pub(super) const SWITCHABLE_CPU_START: u16 = 0x8000;
pub(super) const SWITCHABLE_CPU_END_EXCLUSIVE: u16 = 0xC000;
pub(super) const FIXED_CPU_START: u16 = 0xC000;
pub(super) const DIALOGUE_DIRECTORY_CPU_ADDRESS: u16 = 0xBFE0;
pub(super) const MAIN_DIALOGUE_PRG_BANK: u8 = 0x0A;
pub(super) const MAIN_DIALOGUE_STATE_ADDRESS: u16 = 0x77F7;
pub(super) const MAIN_DIALOGUE_DISPATCHER_CPU_ADDRESS: u16 = 0x8000;
pub(super) const MAIN_DIALOGUE_HANDLER_TABLE_CPU_ADDRESS: u16 = 0x8006;
pub(super) const OPTIONAL_E5_PREFIX_CODE: u8 = 0xE5;
pub(super) const OPTIONAL_E8_PREFIX_CODE: u8 = 0xE8;
pub(super) const OPTIONAL_PREFIX_BYTE_COUNT: usize = 6;
pub(super) const FIXED_RECORD_HEADER_BYTE_COUNT: usize = 4;
/// 이 값부터가 대사 제어 코드다. 표시 글자는 모두 이보다 작다.
pub(super) const FIRST_MAIN_CONTROL_CODE: u8 = 0xE0;
pub(super) const MAX_MAIN_LINE_SCAN_BYTES: usize = 256;
pub(super) const MAX_MAIN_LINEAR_SEGMENT_LINES: usize = 64;
pub(super) const MAIN_LINE_END_CODES: [u8; 7] = [0xEF, 0xE7, 0xE4, 0xE6, 0xEE, 0xEB, 0xED];
pub(super) const MAIN_LINEAR_SEGMENT_BOUNDARY_CODES: [u8; 4] = [0xEF, 0xE7, 0xE4, 0xE6];
pub(super) const MAIN_LINEAR_CONTINUATION_CODES: [u8; 3] = [0xEE, 0xEB, 0xED];

pub(super) const MAIN_DIALOGUE_DISPATCHER_CODE: &[u8] = &[0xAD, 0xF7, 0x77, 0x20, 0x4C, 0xC3];
pub(super) const MAIN_DIALOGUE_STATE_HANDLERS: [u16; 18] = [
    0xC73D, 0x802A, 0x80A2, 0x81B7, 0x80E6, 0x8119, 0x8126, 0x81B7, 0x81BE, 0x81EA, 0x839F, 0x847B,
    0x84DA, 0x852F, 0x8588, 0x8613, 0x8634, 0x8719,
];
pub(super) const MAIN_DIALOGUE_HANDLER_ROLES: [&str; 18] = [
    "no_op",
    "initialize_entry_and_resolve_pointer",
    "inspect_optional_E5_prefix",
    "unresolved_handler_03",
    "consume_fixed_four_byte_record_header",
    "unresolved_handler_05",
    "inspect_optional_E8_prefix",
    "unresolved_handler_07",
    "prepare_output_pointer",
    "decode_line_into_sram",
    "unresolved_handler_10",
    "unresolved_handler_11",
    "unresolved_handler_12",
    "unresolved_handler_13",
    "unresolved_handler_14",
    "unresolved_handler_15",
    "unresolved_handler_16",
    "resolve_selected_entry_after_caller_handoff",
];

pub(super) struct CodeRegionSpec {
    pub(super) role: &'static str,
    pub(super) cpu_address: u16,
    pub(super) bytes: &'static [u8],
}

pub(super) const MAIN_DIALOGUE_STATE_CODE_REGIONS: [CodeRegionSpec; 8] = [
    CodeRegionSpec {
        role: "inspect_and_consume_optional_E5_prefix",
        cpu_address: 0x80A2,
        bytes: &[
            0x20, 0x3A, 0x83, 0xA0, 0x00, 0x20, 0x9C, 0xE6, 0xC9, 0xE5, 0xF0, 0x07, 0xA9, 0x04,
            0x8D, 0xF7, 0x77, 0xD0, 0x30, 0xC8, 0x20, 0x9C, 0xE6, 0x85, 0x71, 0xC8, 0x20, 0x9C,
            0xE6, 0x85, 0x70, 0xC8, 0x20, 0x9C, 0xE6, 0x8D, 0xCF, 0x05, 0xC8, 0x20, 0x9C, 0xE6,
            0x8D, 0xD0, 0x05, 0xC8, 0x20, 0x9C, 0xE6, 0x8D, 0x1D, 0x78, 0xC8, 0x8C, 0xFA, 0x77,
            0x20, 0x0C, 0x83, 0xA9, 0x12, 0x20, 0x90, 0xE6, 0xEE, 0xF7, 0x77, 0x60,
        ],
    },
    CodeRegionSpec {
        role: "consume_fixed_four_byte_record_header",
        cpu_address: 0x80E6,
        bytes: &[
            0x20, 0x3A, 0x83, 0xA0, 0x00, 0x20, 0x9C, 0xE6, 0x8D, 0x18, 0x78, 0xC8, 0x20, 0x9C,
            0xE6, 0x8D, 0x19, 0x78, 0xC8, 0x20, 0x9C, 0xE6, 0x8D, 0x1A, 0x78, 0xC8, 0x20, 0x9C,
            0xE6, 0x38, 0xE9, 0x01, 0x8D, 0x1B, 0x78, 0xC8, 0x8C, 0xFA, 0x77, 0x20, 0x0C, 0x83,
            0xA9, 0x1F, 0x20, 0x90, 0xE6, 0xEE, 0xF7, 0x77, 0x60,
        ],
    },
    CodeRegionSpec {
        role: "inspect_and_consume_optional_E8_prefix",
        cpu_address: 0x8126,
        bytes: &[
            0x20, 0x3A, 0x83, 0xA0, 0x00, 0x20, 0x9C, 0xE6, 0xC9, 0xE8, 0xF0, 0x09, 0xAD, 0x0A,
            0x78, 0xD0, 0x31, 0xA0, 0x4F, 0xD0, 0x4A, 0xC8, 0xAE, 0xF0, 0x77, 0x20, 0x9C, 0xE6,
            0x9D, 0x1F, 0x78, 0xC8, 0x20, 0x9C, 0xE6, 0x9D, 0x21, 0x78, 0xC8, 0x20, 0x9C, 0xE6,
            0x9D, 0x23, 0x78, 0xC8, 0x20, 0x9C, 0xE6, 0x9D, 0x25, 0x78, 0xC8, 0x20, 0x9C, 0xE6,
            0x9D, 0x27, 0x78, 0xC8, 0x8C, 0xFA, 0x77, 0x20, 0x0C, 0x83,
        ],
    },
    CodeRegionSpec {
        role: "advance_current_entry_pointer_by_source_index",
        cpu_address: 0x830C,
        bytes: &[
            0xAE, 0xF0, 0x77, 0xAD, 0xFA, 0x77, 0x18, 0x7D, 0x12, 0x78, 0x9D, 0x12, 0x78, 0x90,
            0x03, 0xFE, 0x14, 0x78, 0x60,
        ],
    },
    CodeRegionSpec {
        role: "bind_current_entry_pointer_for_banked_read",
        cpu_address: 0x833A,
        bytes: &[
            0xAE, 0xF0, 0x77, 0xBD, 0x12, 0x78, 0x85, 0x76, 0xBD, 0x14, 0x78, 0x85, 0x77, 0x60,
        ],
    },
    CodeRegionSpec {
        role: "route_caller_handoff_from_line_advance",
        cpu_address: 0x84B2,
        bytes: &[0xAD, 0x08, 0x78, 0xF0, 0x03, 0x4C, 0x5B, 0x85],
    },
    CodeRegionSpec {
        role: "raise_caller_handoff_flag_and_select_state_17",
        cpu_address: 0x8556,
        bytes: &[
            0xAD, 0x08, 0x78, 0xF0, 0x0C, 0xA9, 0x01, 0x8D, 0x31, 0x78, 0xEE, 0x09, 0x78, 0xA9,
            0x11, 0xD0, 0x18,
        ],
    },
    CodeRegionSpec {
        role: "resolve_selected_entry_and_clear_caller_handoff",
        cpu_address: 0x8719,
        bytes: &[
            0x20, 0x68, 0x86, 0x20, 0xB2, 0xE6, 0xA9, 0x00, 0x8D, 0x08, 0x78, 0x8D, 0x09, 0x78,
            0x8D, 0x31, 0x78, 0xA9, 0x09, 0x8D, 0xF7, 0x77, 0x60,
        ],
    },
];

pub(super) const MAIN_DIALOGUE_POINTER_RESOLVER_CPU_ADDRESS: u16 = 0xE6B2;
pub(super) const MAIN_DIALOGUE_POINTER_RESOLVER_CODE: &[u8] = &[
    0xAD, 0xF2, 0x77, 0xF0, 0x06, 0xAD, 0xF2, 0x77, 0x8D, 0x00, 0xA0, 0xAD, 0xF4, 0x77, 0x29, 0x0F,
    0x0A, 0xA8, 0xB9, 0xE0, 0xBF, 0x85, 0x04, 0xB9, 0xE1, 0xBF, 0x85, 0x05, 0xAD, 0xF1, 0x77, 0x0A,
    0x90, 0x02, 0xE6, 0x05, 0x18, 0x65, 0x04, 0x85, 0x04, 0x90, 0x02, 0xE6, 0x05, 0xA0, 0x00, 0xB1,
    0x04, 0xAE, 0xF0, 0x77, 0x9D, 0x12, 0x78, 0xC8, 0xB1, 0x04, 0x9D, 0x14, 0x78, 0xA9, 0x0A, 0x8D,
    0x00, 0xA0, 0x60,
];
pub(super) const CALLER_HANDOFF_FLAG_LOAD: [u8; 3] = [0xAD, 0x09, 0x78];

#[derive(Clone, Copy)]
pub(super) struct CallerHandoffObserverSpec {
    pub(super) prg_bank: u8,
    pub(super) cpu_address: u16,
    pub(super) handler_cpu_address: u16,
}

pub(super) const CALLER_HANDOFF_OBSERVER_SPECS: [CallerHandoffObserverSpec; 5] = [
    CallerHandoffObserverSpec {
        prg_bank: 0x02,
        cpu_address: 0xA978,
        handler_cpu_address: 0xA975,
    },
    CallerHandoffObserverSpec {
        prg_bank: 0x04,
        cpu_address: 0xA223,
        handler_cpu_address: 0xA20F,
    },
    CallerHandoffObserverSpec {
        prg_bank: 0x04,
        cpu_address: 0xA242,
        handler_cpu_address: 0xA233,
    },
    CallerHandoffObserverSpec {
        prg_bank: 0x06,
        cpu_address: 0xA141,
        handler_cpu_address: 0xA13E,
    },
    CallerHandoffObserverSpec {
        prg_bank: 0x0B,
        cpu_address: 0x9B1D,
        handler_cpu_address: 0x9B14,
    },
];

#[derive(Clone, Copy)]
pub(super) struct CallerHandoffDispatchSpec {
    pub(super) prg_bank: u8,
    pub(super) state_address: u16,
    pub(super) dispatcher_cpu_address: u16,
    pub(super) handler_table_cpu_address: u16,
    pub(super) handler_cpu_address: u16,
    pub(super) handlers: &'static [u16],
    pub(super) handler_state_indices: &'static [usize],
}

pub(super) const CALLER_HANDOFF_DISPATCH_SPECS: [CallerHandoffDispatchSpec; 11] = [
    CallerHandoffDispatchSpec {
        prg_bank: 0x02,
        state_address: 0x05DB,
        dispatcher_cpu_address: 0xA780,
        handler_table_cpu_address: 0xA786,
        handler_cpu_address: 0xA975,
        handlers: &[0xA792, 0xA975, 0xA98C, 0xA7B9, 0xA7C9, 0xA961],
        handler_state_indices: &[1],
    },
    CallerHandoffDispatchSpec {
        prg_bank: 0x04,
        state_address: 0x7731,
        dispatcher_cpu_address: 0x9F15,
        handler_table_cpu_address: 0x9F1B,
        handler_cpu_address: 0xA233,
        handlers: &[
            0xA3A5, 0xA3E0, 0x9FED, 0xA054, 0xA0E9, 0x9FFA, 0xA011, 0xA02D, 0xA054, 0xA071, 0x9F64,
            0x9F83, 0xA054, 0x9F57, 0xA123, 0xA165, 0xA233, 0xA252, 0xA25D, 0xA269, 0xA27E, 0xA294,
            0xA384, 0x9FCA, 0xA02D, 0xA054, 0xA0D3, 0xA508, 0xA535, 0xC73D,
        ],
        handler_state_indices: &[16],
    },
    CallerHandoffDispatchSpec {
        prg_bank: 0x06,
        state_address: 0x05DB,
        dispatcher_cpu_address: 0x9595,
        handler_table_cpu_address: 0x959B,
        handler_cpu_address: 0xA13E,
        handlers: &[0xA122, 0xA13E, 0x95A9, 0xA122, 0x9D3C, 0x9D5E, 0x98AC],
        handler_state_indices: &[1],
    },
    CallerHandoffDispatchSpec {
        prg_bank: 0x06,
        state_address: 0x05DB,
        dispatcher_cpu_address: 0x99AC,
        handler_table_cpu_address: 0x99B2,
        handler_cpu_address: 0xA13E,
        handlers: &[
            0x99CC, 0xA13E, 0x99F1, 0x99FB, 0x9A0E, 0xA13E, 0x9B7A, 0x9B86, 0xA122, 0x9C02, 0xA13E,
            0x9B7A, 0x9C1A,
        ],
        handler_state_indices: &[1, 5, 10],
    },
    CallerHandoffDispatchSpec {
        prg_bank: 0x06,
        state_address: 0x05DB,
        dispatcher_cpu_address: 0x9C63,
        handler_table_cpu_address: 0x9C69,
        handler_cpu_address: 0xA13E,
        handlers: &[
            0x99CC, 0xA13E, 0x9B7A, 0x9C8B, 0x9CC5, 0xA13E, 0x9B7A, 0x9CD4, 0xA13E, 0x9D25, 0x9D2E,
            0x9D3C, 0x9D5E, 0x9D6A, 0xA13E, 0x9D8E, 0xA122,
        ],
        handler_state_indices: &[1, 5, 8, 14],
    },
    CallerHandoffDispatchSpec {
        prg_bank: 0x06,
        state_address: 0x05DB,
        dispatcher_cpu_address: 0x9DBE,
        handler_table_cpu_address: 0x9DC4,
        handler_cpu_address: 0xA13E,
        handlers: &[
            0x99CC, 0xA13E, 0x99F1, 0x9E07, 0x9E15, 0xA13E, 0x9EAC, 0x9EC1, 0xA13E, 0x9DE6, 0x9F16,
            0x9F99, 0x9C02, 0xA13E, 0x9B7A, 0xA07D, 0xA122,
        ],
        handler_state_indices: &[1, 5, 8, 13],
    },
    CallerHandoffDispatchSpec {
        prg_bank: 0x06,
        state_address: 0x05DB,
        dispatcher_cpu_address: 0xB10D,
        handler_table_cpu_address: 0xB113,
        handler_cpu_address: 0xA13E,
        handlers: &[
            0xB125, 0xA13E, 0xB17A, 0xB182, 0xB19C, 0xA13E, 0xB1F7, 0xB210, 0xA122,
        ],
        handler_state_indices: &[1, 5],
    },
    CallerHandoffDispatchSpec {
        prg_bank: 0x06,
        state_address: 0x05DB,
        dispatcher_cpu_address: 0xB7F1,
        handler_table_cpu_address: 0xB7F7,
        handler_cpu_address: 0xA13E,
        handlers: &[0xB7FD, 0xA13E, 0xB858],
        handler_state_indices: &[1],
    },
    CallerHandoffDispatchSpec {
        prg_bank: 0x0B,
        state_address: 0x05EE,
        dispatcher_cpu_address: 0x995F,
        handler_table_cpu_address: 0x9965,
        handler_cpu_address: 0x9B14,
        handlers: &[
            0xC73D, 0x9985, 0x9A33, 0x9A99, 0x9AFC, 0x9B14, 0x9B2B, 0x9B35, 0x9B8A, 0x9B14, 0x9BA0,
            0x9BCF, 0x9C17, 0x9C09, 0x9CF0, 0x9D0C,
        ],
        handler_state_indices: &[5, 9],
    },
    CallerHandoffDispatchSpec {
        prg_bank: 0x0B,
        state_address: 0x05EE,
        dispatcher_cpu_address: 0xA01C,
        handler_table_cpu_address: 0xA022,
        handler_cpu_address: 0x9B14,
        handlers: &[
            0xC73D, 0xA09F, 0x9B14, 0x9B2B, 0x9B35, 0xA03E, 0xA067, 0xA073, 0xC075, 0xA088, 0xA03E,
            0xA06A, 0xA076, 0xC075,
        ],
        handler_state_indices: &[2],
    },
    CallerHandoffDispatchSpec {
        prg_bank: 0x0B,
        state_address: 0x05EE,
        dispatcher_cpu_address: 0xB369,
        handler_table_cpu_address: 0xB36F,
        handler_cpu_address: 0x9B14,
        handlers: &[
            0xC73D, 0xB383, 0x9B14, 0x9B2B, 0xB3C0, 0xB3DC, 0xB3F8, 0x9B14, 0xB421, 0xC075,
        ],
        handler_state_indices: &[2, 7],
    },
];

pub(super) struct DialogueTableSpec {
    pub(super) id: &'static str,
    pub(super) role: &'static str,
    pub(super) source_prg_bank: u8,
    pub(super) pointer_table_file_offset: usize,
    pub(super) pointer_count: usize,
    pub(super) data_file_start: usize,
    pub(super) directory_group: Option<u8>,
    pub(super) directory_selector_use: Option<DirectorySelectorUseSpec>,
    pub(super) separate_consumer: Option<SeparateConsumerSpec>,
    pub(super) allowed_handler_targets: &'static [HandlerTargetSpec],
}

#[derive(Clone, Copy)]
pub(super) struct DirectorySelectorUseSpec {
    pub(super) role: &'static str,
    pub(super) prg_bank: u8,
    pub(super) cpu_address: u16,
    pub(super) code: &'static [u8],
}

#[derive(Clone, Copy)]
pub(super) struct SeparateConsumerSpec {
    pub(super) prg_bank: u8,
    pub(super) loader_cpu_address: u16,
    pub(super) loader_code: &'static [u8],
    pub(super) table_set_index: u8,
    pub(super) table_root_cell_cpu_address: u16,
    pub(super) table_set_selector: &'static str,
    pub(super) entry_index_selector: &'static str,
    pub(super) destination_pointer: &'static str,
}

pub(super) struct HandlerTargetSpec {
    pub(super) cpu_address: u16,
    pub(super) role: &'static str,
    pub(super) expected_code: &'static [u8],
}

pub(super) const NO_HANDLER_TARGETS: &[HandlerTargetSpec] = &[];
pub(super) const RECRUITMENT_HANDLER_TARGETS: &[HandlerTargetSpec] = &[HandlerTargetSpec {
    cpu_address: 0xAA2B,
    role: "recruitment_non_dialogue_handler",
    expected_code: &[
        0xA9, 0x00, 0x85, 0xD0, 0x20, 0x4A, 0xAA, 0x20, 0x36, 0xC3, 0xE6, 0x30, 0xA9, 0x00, 0x85,
        0x20, 0x85, 0xD0, 0xA5, 0x20, 0xD0, 0x03, 0x4C, 0x3D, 0xAA, 0x20, 0x4E, 0xC0, 0x4C, 0x2B,
        0xAA,
    ],
}];

pub(super) const EPILOGUE_ROUTING_HANDLER_TARGETS: &[HandlerTargetSpec] = &[HandlerTargetSpec {
    cpu_address: 0xC73D,
    role: "fixed_no_op_handler",
    expected_code: &[0x60],
}];

pub(super) const EPILOGUE_ROUTING_SELECTOR_USE: DirectorySelectorUseSpec =
    DirectorySelectorUseSpec {
        role: "select_epilogue_dialogue_table_and_entry",
        prg_bank: 0x04,
        cpu_address: 0xA17E,
        code: &[
            0xA9, 0x40, 0x8D, 0xF4, 0x77, 0xA6, 0x04, 0xE0, 0x02, 0xF0, 0xDC, 0xE0, 0x01, 0xD0,
            0x08, 0xA9, 0x41, 0x8D, 0xF4, 0x77, 0x20, 0xB7, 0xA1, 0xAE, 0x3B, 0x77, 0xE8, 0x8A,
            0x8D, 0xF1, 0x77, 0xA9, 0x00, 0x8D, 0xF0, 0x77, 0x8D, 0x5D, 0x77, 0xA9, 0x01, 0x8D,
            0xF7, 0x77,
        ],
    };

pub(super) const BATTLE_DIALOGUE_CONSUMER: SeparateConsumerSpec = SeparateConsumerSpec {
    prg_bank: 0x04,
    loader_cpu_address: 0x8000,
    loader_code: &[
        0xAD, 0x35, 0x79, 0x0A, 0xA8, 0xB9, 0x2D, 0x80, 0x85, 0x00, 0xB9, 0x2E, 0x80, 0x85, 0x01,
        0xAD, 0x36, 0x79, 0x0A, 0xA8, 0xB1, 0x00, 0x85, 0x76, 0xC8, 0xB1, 0x00, 0x85, 0x77, 0x90,
        0x0D, 0xA5, 0x76, 0x18, 0x69, 0x04, 0x85, 0x76, 0xA5, 0x77, 0x69, 0x00, 0x85, 0x77, 0x60,
    ],
    table_set_index: 0,
    table_root_cell_cpu_address: 0x802D,
    table_set_selector: "0x7935",
    entry_index_selector: "0x7936",
    destination_pointer: "0x76/0x77",
};

pub(super) const BATTLE_DIALOGUE_TABLE_ID: &str = "battle-dialogue";
pub(super) const BATTLE_DIALOGUE_PRG_BANK: u8 = 0x04;
pub(super) const BATTLE_DIALOGUE_STATE_ADDRESS: u16 = 0x7937;
pub(super) const BATTLE_DIALOGUE_DISPATCHER_CPU_ADDRESS: u16 = 0x8031;
pub(super) const BATTLE_DIALOGUE_HANDLER_TABLE_CPU_ADDRESS: u16 = 0x8037;
pub(super) const BATTLE_DIALOGUE_STATE_HANDLERS: [u16; 9] = [
    0xC73D, 0x8063, 0x80C2, 0x8237, 0x827D, 0x83B8, 0x8309, 0x8369, 0x8049,
];
pub(super) const BATTLE_DIALOGUE_STATE_ROLES: [&str; 9] = [
    "no_op",
    "consume_fixed_four_byte_header",
    "decode_record_body",
    "publish_decoded_row",
    "advance_or_finish_record",
    "wait_for_published_rows",
    "initialize_publish_buffers",
    "publish_next_row",
    "initialize_record_state",
];
pub(super) const BATTLE_DIALOGUE_DATA_END_EXCLUSIVE_CPU_ADDRESS: u16 = 0x896D;
pub(super) const BATTLE_DIALOGUE_FIXED_HEADER_BYTE_COUNT: usize = 4;
pub(super) const BATTLE_DIALOGUE_END_CONTROL: u8 = 0xEF;
pub(super) const BATTLE_DIALOGUE_DYNAMIC_CONTROL: u8 = 0xEC;
pub(super) const BATTLE_DIALOGUE_DYNAMIC_SELECTOR_MAX: u8 = 3;
pub(super) const BATTLE_DIALOGUE_CONTROL_CODES: [u8; 6] = [0xAB, 0xAC, 0xEC, 0xED, 0xEE, 0xEF];
pub(super) const BATTLE_DIALOGUE_REFERENCED_HEADERS: [[u8; 4]; 2] =
    [[0x08, 0x13, 0x10, 0x04], [0x08, 0x12, 0x10, 0x04]];
pub(super) const BATTLE_DIALOGUE_PHYSICAL_HEADERS: [[u8; 4]; 3] = [
    [0x08, 0x13, 0x10, 0x04],
    [0x08, 0x12, 0x10, 0x04],
    [0x08, 0x13, 0x10, 0x03],
];

#[derive(Clone, Copy)]
pub(super) struct BattleDialogueCodeRegionSpec {
    pub(super) role: &'static str,
    pub(super) cpu_address: u16,
    pub(super) byte_count: usize,
    pub(super) expected_sha1: &'static str,
}

pub(super) const BATTLE_DIALOGUE_CODE_REGIONS: [BattleDialogueCodeRegionSpec; 10] = [
    BattleDialogueCodeRegionSpec {
        role: "resolve_battle_dialogue_pointer",
        cpu_address: 0x8000,
        byte_count: 0x2D,
        expected_sha1: "0c97c4fb8cd2c09f0c8ababe521154ba1ce5665a",
    },
    BattleDialogueCodeRegionSpec {
        role: "dispatch_battle_dialogue_state",
        cpu_address: 0x8031,
        byte_count: 0x06,
        expected_sha1: "4b0ccb33e0ec85c5b884f5d709cfb16270d7f23b",
    },
    BattleDialogueCodeRegionSpec {
        role: "initialize_battle_dialogue_record",
        cpu_address: 0x8049,
        byte_count: 0x1A,
        expected_sha1: "f4e53298f80aa2030355cccb8552d4d1fd61dba0",
    },
    BattleDialogueCodeRegionSpec {
        role: "consume_battle_dialogue_header",
        cpu_address: 0x8063,
        byte_count: 0x5F,
        expected_sha1: "80254a37725d891f523b692d59442d88e312c954",
    },
    BattleDialogueCodeRegionSpec {
        role: "decode_battle_dialogue_record_body",
        cpu_address: 0x80C2,
        byte_count: 0x104,
        expected_sha1: "890f33d0d6fe77f5326540ed949bbf2a29852d36",
    },
    BattleDialogueCodeRegionSpec {
        role: "expand_battle_dialogue_dynamic_value",
        cpu_address: 0x81C6,
        byte_count: 0x60,
        expected_sha1: "dbabf7222f10c6a99d3006f70aac4158155e9863",
    },
    BattleDialogueCodeRegionSpec {
        role: "advance_battle_dialogue_source_pointer",
        cpu_address: 0x822E,
        byte_count: 0x09,
        expected_sha1: "1beb5f51a8a6c2fcec271bd4fd4611fc72460eb1",
    },
    BattleDialogueCodeRegionSpec {
        role: "publish_battle_dialogue_row",
        cpu_address: 0x8237,
        byte_count: 0x46,
        expected_sha1: "8e579c652aab0b97bd12f80a672779a9cff70d9e",
    },
    BattleDialogueCodeRegionSpec {
        role: "advance_or_finish_battle_dialogue_record",
        cpu_address: 0x827D,
        byte_count: 0x34,
        expected_sha1: "f880ea430f0131545c93bdc1c15852bfb147a481",
    },
    BattleDialogueCodeRegionSpec {
        role: "bind_battle_dialogue_source_read_pointer",
        cpu_address: 0x83F2,
        byte_count: 0x17,
        expected_sha1: "e81790a878c29f94167fb8efae78a4ef35c176b2",
    },
];

// Initial candidate locations came from the pinned Basilisk map. Every table,
// including later directory discoveries, is admitted only after its ranges,
// roots, selector evidence when declared, and pointers validate against the
// exact supported Japanese ROM.
pub(super) const DIALOGUE_TABLE_SPECS: [DialogueTableSpec; 9] = [
    DialogueTableSpec {
        id: "chapter-intro-dialogue",
        role: "chapter_intro_dialogue",
        source_prg_bank: 0x08,
        pointer_table_file_offset: 0x21F3B,
        pointer_count: 51,
        data_file_start: 0x21FA1,
        directory_group: Some(0),
        directory_selector_use: None,
        separate_consumer: None,
        allowed_handler_targets: NO_HANDLER_TARGETS,
    },
    DialogueTableSpec {
        id: "village-and-outro-dialogue",
        role: "village_and_outro_dialogue",
        source_prg_bank: 0x0C,
        pointer_table_file_offset: 0x30010,
        pointer_count: 94,
        data_file_start: 0x300CC,
        directory_group: Some(0),
        directory_selector_use: None,
        separate_consumer: None,
        allowed_handler_targets: NO_HANDLER_TARGETS,
    },
    DialogueTableSpec {
        id: "recruitment-dialogue",
        role: "recruitment_dialogue",
        source_prg_bank: 0x07,
        pointer_table_file_offset: 0x1C863,
        pointer_count: 109,
        data_file_start: 0x1C93D,
        directory_group: Some(1),
        directory_selector_use: None,
        separate_consumer: None,
        allowed_handler_targets: RECRUITMENT_HANDLER_TARGETS,
    },
    DialogueTableSpec {
        id: "victory-and-defeat-dialogue",
        role: "victory_and_defeat_dialogue",
        source_prg_bank: 0x0B,
        pointer_table_file_offset: 0x2DD95,
        pointer_count: 11,
        data_file_start: 0x2DDAB,
        directory_group: Some(0),
        directory_selector_use: None,
        separate_consumer: None,
        allowed_handler_targets: NO_HANDLER_TARGETS,
    },
    DialogueTableSpec {
        id: "shop-and-item-dialogue",
        role: "shop_and_item_dialogue",
        source_prg_bank: 0x0B,
        pointer_table_file_offset: 0x2E776,
        pointer_count: 88,
        data_file_start: 0x2E826,
        directory_group: Some(1),
        directory_selector_use: None,
        separate_consumer: None,
        allowed_handler_targets: NO_HANDLER_TARGETS,
    },
    DialogueTableSpec {
        id: "house-dialogue",
        role: "house_dialogue",
        source_prg_bank: 0x03,
        pointer_table_file_offset: 0x0E477,
        pointer_count: 50,
        data_file_start: 0x0E4DB,
        directory_group: Some(0),
        directory_selector_use: None,
        separate_consumer: None,
        allowed_handler_targets: NO_HANDLER_TARGETS,
    },
    DialogueTableSpec {
        id: "battle-dialogue",
        role: "battle_dialogue",
        source_prg_bank: 0x04,
        pointer_table_file_offset: 0x1046B,
        pointer_count: 65,
        data_file_start: 0x104ED,
        directory_group: None,
        directory_selector_use: None,
        separate_consumer: Some(BATTLE_DIALOGUE_CONSUMER),
        allowed_handler_targets: NO_HANDLER_TARGETS,
    },
    DialogueTableSpec {
        id: "epilogue-dialogue",
        role: "epilogue_dialogue",
        source_prg_bank: 0x04,
        pointer_table_file_offset: 0x12DFD,
        pointer_count: 66,
        data_file_start: 0x12E81,
        directory_group: Some(0),
        directory_selector_use: None,
        separate_consumer: None,
        allowed_handler_targets: NO_HANDLER_TARGETS,
    },
    DialogueTableSpec {
        id: "epilogue-routing-dialogue",
        role: "epilogue_routing_dialogue",
        source_prg_bank: 0x04,
        pointer_table_file_offset: 0x1397C,
        pointer_count: 54,
        data_file_start: 0x139E8,
        directory_group: Some(1),
        directory_selector_use: Some(EPILOGUE_ROUTING_SELECTOR_USE),
        separate_consumer: None,
        allowed_handler_targets: EPILOGUE_ROUTING_HANDLER_TARGETS,
    },
];
