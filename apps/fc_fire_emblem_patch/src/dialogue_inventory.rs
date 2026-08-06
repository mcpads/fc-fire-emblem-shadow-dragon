use std::{collections::BTreeMap, fs, path::Path};

use anyhow::{Context, Result, ensure};
use serde::Serialize;

use crate::{
    rom::{EXPECTED_SOURCE_SHA1, HEADER_SIZE, PRG_SIZE, Rom},
    sha1_hex,
};

const PRG_BANK_SIZE: usize = 16 * 1024;
const SWITCHABLE_CPU_START: u16 = 0x8000;
const SWITCHABLE_CPU_END_EXCLUSIVE: u16 = 0xC000;
const FIXED_CPU_START: u16 = 0xC000;
const DIALOGUE_DIRECTORY_CPU_ADDRESS: u16 = 0xBFE0;
const MAIN_DIALOGUE_PRG_BANK: u8 = 0x0A;
const MAIN_DIALOGUE_STATE_ADDRESS: u16 = 0x77F7;
const MAIN_DIALOGUE_DISPATCHER_CPU_ADDRESS: u16 = 0x8000;
const MAIN_DIALOGUE_HANDLER_TABLE_CPU_ADDRESS: u16 = 0x8006;
const OPTIONAL_E5_PREFIX_CODE: u8 = 0xE5;
const OPTIONAL_E8_PREFIX_CODE: u8 = 0xE8;
const OPTIONAL_PREFIX_BYTE_COUNT: usize = 6;
const FIXED_RECORD_HEADER_BYTE_COUNT: usize = 4;

const MAIN_DIALOGUE_DISPATCHER_CODE: &[u8] = &[0xAD, 0xF7, 0x77, 0x20, 0x4C, 0xC3];
const MAIN_DIALOGUE_STATE_HANDLERS: [u16; 18] = [
    0xC73D, 0x802A, 0x80A2, 0x81B7, 0x80E6, 0x8119, 0x8126, 0x81B7, 0x81BE, 0x81EA, 0x839F, 0x847B,
    0x84DA, 0x852F, 0x8588, 0x8613, 0x8634, 0x8719,
];
const MAIN_DIALOGUE_HANDLER_ROLES: [&str; 18] = [
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
    "unresolved_handler_17",
];

struct CodeRegionSpec {
    role: &'static str,
    cpu_address: u16,
    bytes: &'static [u8],
}

const MAIN_DIALOGUE_PREFIX_CODE_REGIONS: [CodeRegionSpec; 5] = [
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
];

struct DialogueTableSpec {
    id: &'static str,
    role: &'static str,
    source_prg_bank: u8,
    pointer_table_file_offset: usize,
    pointer_count: usize,
    data_file_start: usize,
    directory_group: Option<u8>,
    separate_consumer: Option<SeparateConsumerSpec>,
    allowed_handler_targets: &'static [HandlerTargetSpec],
}

#[derive(Clone, Copy)]
struct SeparateConsumerSpec {
    prg_bank: u8,
    loader_cpu_address: u16,
    loader_code: &'static [u8],
    table_set_index: u8,
    table_root_cell_cpu_address: u16,
    table_set_selector: &'static str,
    entry_index_selector: &'static str,
    destination_pointer: &'static str,
}

struct HandlerTargetSpec {
    cpu_address: u16,
    role: &'static str,
    expected_code: &'static [u8],
}

const NO_HANDLER_TARGETS: &[HandlerTargetSpec] = &[];
const RECRUITMENT_HANDLER_TARGETS: &[HandlerTargetSpec] = &[HandlerTargetSpec {
    cpu_address: 0xAA2B,
    role: "recruitment_non_dialogue_handler",
    expected_code: &[
        0xA9, 0x00, 0x85, 0xD0, 0x20, 0x4A, 0xAA, 0x20, 0x36, 0xC3, 0xE6, 0x30, 0xA9, 0x00, 0x85,
        0x20, 0x85, 0xD0, 0xA5, 0x20, 0xD0, 0x03, 0x4C, 0x3D, 0xAA, 0x20, 0x4E, 0xC0, 0x4C, 0x2B,
        0xAA,
    ],
}];

const BATTLE_DIALOGUE_CONSUMER: SeparateConsumerSpec = SeparateConsumerSpec {
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

// Candidate locations came from the pinned Basilisk map and are admitted only
// after all ranges, directory roots, and entry pointers validate against the
// exact supported Japanese ROM.
const DIALOGUE_TABLE_SPECS: [DialogueTableSpec; 8] = [
    DialogueTableSpec {
        id: "chapter-intro-dialogue",
        role: "chapter_intro_dialogue",
        source_prg_bank: 0x08,
        pointer_table_file_offset: 0x21F3B,
        pointer_count: 51,
        data_file_start: 0x21FA1,
        directory_group: Some(0),
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
        separate_consumer: None,
        allowed_handler_targets: NO_HANDLER_TARGETS,
    },
];

#[derive(Debug)]
pub struct DialogueStructureSummary {
    pub report_sha1: String,
    pub table_count: usize,
    pub pointer_count: usize,
    pub unique_target_count: usize,
    pub alias_group_count: usize,
}

#[derive(Debug, Serialize)]
struct DialogueStructureReport {
    schema_version: u8,
    scope: ReportScope,
    summary: ReportSummary,
    main_dialogue_state_machine: MainDialogueStateMachineReport,
    tables: Vec<DialogueTableReport>,
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
    directory_bound_table_count: usize,
    separate_consumer_bound_table_count: usize,
    consumer_bound_table_count: usize,
    unresolved_consumer_table_count: usize,
    pointer_count: usize,
    unique_target_count: usize,
    unique_script_entry_count: usize,
    handler_target_entry_count: usize,
    alias_group_count: usize,
    aliased_entry_count: usize,
}

#[derive(Debug, Serialize)]
struct DialogueTableReport {
    id: &'static str,
    role: &'static str,
    source_prg_bank: u8,
    source_prg_bank_hex: String,
    pointer_table_cpu_address: u16,
    pointer_table_cpu_address_hex: String,
    pointer_table_file_offset: usize,
    pointer_table_file_offset_hex: String,
    pointer_table_file_end_exclusive: usize,
    pointer_table_file_end_exclusive_hex: String,
    pointer_table_byte_count: usize,
    pointer_table_sha1: String,
    pointer_count: usize,
    unique_target_count: usize,
    unique_script_entry_count: usize,
    handler_target_entry_count: usize,
    alias_group_count: usize,
    aliased_entry_count: usize,
    main_record_prefix_summary: Option<MainRecordPrefixSummary>,
    data_file_start: usize,
    data_file_start_hex: String,
    directory_binding: Option<DirectoryBindingReport>,
    separate_consumer_binding: Option<SeparateConsumerBindingReport>,
    consumer_binding_status: &'static str,
    entries: Vec<DialogueEntryReport>,
}

#[derive(Debug, Serialize)]
struct DirectoryBindingReport {
    selector: u8,
    selector_hex: String,
    directory_group: u8,
    directory_entry_cpu_address: u16,
    directory_entry_cpu_address_hex: String,
    directory_entry_file_offset: usize,
    directory_entry_file_offset_hex: String,
    resolved_pointer_table_cpu_address: u16,
    resolved_pointer_table_cpu_address_hex: String,
}

#[derive(Debug, Serialize)]
struct SeparateConsumerBindingReport {
    prg_bank: u8,
    prg_bank_hex: String,
    loader_cpu_address: u16,
    loader_cpu_address_hex: String,
    loader_file_offset: usize,
    loader_file_offset_hex: String,
    loader_code_sha1: String,
    table_set_selector: &'static str,
    table_set_index: u8,
    entry_index_selector: &'static str,
    destination_pointer: &'static str,
    table_root_cell_cpu_address: u16,
    table_root_cell_cpu_address_hex: String,
    table_root_cell_file_offset: usize,
    table_root_cell_file_offset_hex: String,
    resolved_pointer_table_cpu_address: u16,
    resolved_pointer_table_cpu_address_hex: String,
}

#[derive(Debug, Serialize)]
struct DialogueEntryReport {
    index: usize,
    pointer_cpu_address: u16,
    pointer_cpu_address_hex: String,
    target_kind: &'static str,
    file_offset: usize,
    file_offset_hex: String,
    handler_role: Option<&'static str>,
    alias_entry_indices: Vec<usize>,
    main_record_prefix: Option<MainRecordPrefixReport>,
}

#[derive(Debug, Serialize)]
struct MainDialogueStateMachineReport {
    prg_bank: u8,
    prg_bank_hex: String,
    state_address: u16,
    state_address_hex: String,
    dispatcher_cpu_address: u16,
    dispatcher_cpu_address_hex: String,
    dispatcher_file_offset: usize,
    dispatcher_file_offset_hex: String,
    dispatcher_code_sha1: String,
    dispatch_helper_cpu_address: u16,
    dispatch_helper_cpu_address_hex: String,
    handler_table_cpu_address: u16,
    handler_table_cpu_address_hex: String,
    handler_table_file_offset: usize,
    handler_table_file_offset_hex: String,
    handler_table_sha1: String,
    handler_count: usize,
    handlers: Vec<DialogueStateHandlerReport>,
    record_prefix_contract: MainRecordPrefixContract,
    code_regions: Vec<CodeRegionReport>,
}

#[derive(Debug, Serialize)]
struct DialogueStateHandlerReport {
    state: usize,
    cpu_address: u16,
    cpu_address_hex: String,
    file_offset: usize,
    file_offset_hex: String,
    structural_role: &'static str,
    alias_state_indices: Vec<usize>,
}

#[derive(Debug, Serialize)]
struct MainRecordPrefixContract {
    optional_e5_prefix_code: u8,
    optional_e5_prefix_code_hex: String,
    optional_e5_prefix_byte_count: usize,
    fixed_record_header_byte_count: usize,
    optional_e8_prefix_code: u8,
    optional_e8_prefix_code_hex: String,
    optional_e8_prefix_byte_count: usize,
}

#[derive(Debug, Serialize)]
struct CodeRegionReport {
    role: &'static str,
    cpu_address: u16,
    cpu_address_hex: String,
    file_offset: usize,
    file_offset_hex: String,
    byte_count: usize,
    code_sha1: String,
}

#[derive(Debug, Serialize)]
struct MainRecordPrefixSummary {
    unique_target_count: usize,
    e5_prefix_unique_target_count: usize,
    e8_prefix_unique_target_count: usize,
    both_optional_prefixes_unique_target_count: usize,
    no_optional_prefix_unique_target_count: usize,
}

#[derive(Debug, Clone, Serialize)]
struct MainRecordPrefixReport {
    e5_prefix_present: bool,
    e5_prefix_byte_count: usize,
    fixed_record_header_file_offset: usize,
    fixed_record_header_file_offset_hex: String,
    fixed_record_header_byte_count: usize,
    e8_prefix_present: bool,
    e8_prefix_byte_count: usize,
    first_line_file_offset: usize,
    first_line_file_offset_hex: String,
    total_prefix_byte_count: usize,
}

pub fn analyze_dialogue_structure(
    source_path: &Path,
    report_path: &Path,
) -> Result<DialogueStructureSummary> {
    let rom = Rom::from_path(source_path)?;
    rom.verify_supported_japanese()?;
    let report = build_report(rom.data())?;
    let mut report_bytes =
        serde_json::to_vec_pretty(&report).context("serialize dialogue structure report")?;
    report_bytes.push(b'\n');

    if let Some(parent) = report_path.parent() {
        fs::create_dir_all(parent)
            .with_context(|| format!("create output directory {}", parent.display()))?;
    }
    fs::write(report_path, &report_bytes)
        .with_context(|| format!("write {}", report_path.display()))?;

    Ok(DialogueStructureSummary {
        report_sha1: sha1_hex(&report_bytes),
        table_count: report.summary.table_count,
        pointer_count: report.summary.pointer_count,
        unique_target_count: report.summary.unique_target_count,
        alias_group_count: report.summary.alias_group_count,
    })
}

fn build_report(source: &[u8]) -> Result<DialogueStructureReport> {
    let main_dialogue_state_machine = build_main_dialogue_state_machine(source)?;
    let tables = DIALOGUE_TABLE_SPECS
        .iter()
        .map(|spec| extract_dialogue_table(source, spec))
        .collect::<Result<Vec<_>>>()?;
    let summary = ReportSummary {
        table_count: tables.len(),
        directory_bound_table_count: tables
            .iter()
            .filter(|table| table.directory_binding.is_some())
            .count(),
        separate_consumer_bound_table_count: tables
            .iter()
            .filter(|table| table.separate_consumer_binding.is_some())
            .count(),
        consumer_bound_table_count: tables
            .iter()
            .filter(|table| {
                table.directory_binding.is_some() || table.separate_consumer_binding.is_some()
            })
            .count(),
        unresolved_consumer_table_count: tables
            .iter()
            .filter(|table| {
                table.directory_binding.is_none() && table.separate_consumer_binding.is_none()
            })
            .count(),
        pointer_count: tables.iter().map(|table| table.pointer_count).sum(),
        unique_target_count: tables.iter().map(|table| table.unique_target_count).sum(),
        unique_script_entry_count: tables
            .iter()
            .map(|table| table.unique_script_entry_count)
            .sum(),
        handler_target_entry_count: tables
            .iter()
            .map(|table| table.handler_target_entry_count)
            .sum(),
        alias_group_count: tables.iter().map(|table| table.alias_group_count).sum(),
        aliased_entry_count: tables.iter().map(|table| table.aliased_entry_count).sum(),
    };

    Ok(DialogueStructureReport {
        schema_version: 2,
        scope: ReportScope {
            source_sha1: EXPECTED_SOURCE_SHA1,
            translation_direction: "ja_to_ko",
            preserve_existing_english: true,
            proof_boundary: "exact pointer-table ranges, switchable-bank target mapping, aliases, all eight consumer roots, and the main dialogue record-prefix state path; no dialogue bytes or translations are emitted",
        },
        summary,
        main_dialogue_state_machine,
        tables,
        unknowns: vec![
            "Script targets are entry starts, not proven script byte ranges; declared code handlers are kept separate.",
            "The E5, fixed four-byte, and E8 record prefix is confirmed, but complete entry termination rules remain unresolved.",
            "Eleven of the eighteen main dialogue state handlers remain structurally named but semantically unresolved.",
            "Role labels began as external map candidates and do not prove every entry's gameplay context.",
            "Existing English and numeric content remains protected and is not a translation target.",
        ],
    })
}

fn build_main_dialogue_state_machine(source: &[u8]) -> Result<MainDialogueStateMachineReport> {
    let dispatcher_file_offset = switchable_cpu_to_file_offset(
        MAIN_DIALOGUE_PRG_BANK,
        MAIN_DIALOGUE_DISPATCHER_CPU_ADDRESS,
    )?;
    let dispatcher_end = dispatcher_file_offset + MAIN_DIALOGUE_DISPATCHER_CODE.len();
    ensure!(
        source.get(dispatcher_file_offset..dispatcher_end) == Some(MAIN_DIALOGUE_DISPATCHER_CODE),
        "main dialogue state dispatcher changed"
    );

    let handler_table_file_offset = switchable_cpu_to_file_offset(
        MAIN_DIALOGUE_PRG_BANK,
        MAIN_DIALOGUE_HANDLER_TABLE_CPU_ADDRESS,
    )?;
    let handler_table_byte_count = MAIN_DIALOGUE_STATE_HANDLERS.len() * 2;
    let handler_table_end = handler_table_file_offset + handler_table_byte_count;
    let handler_table_bytes = source
        .get(handler_table_file_offset..handler_table_end)
        .context("main dialogue handler table is outside the source")?;
    let actual_handlers = handler_table_bytes
        .chunks_exact(2)
        .map(|bytes| u16::from_le_bytes([bytes[0], bytes[1]]))
        .collect::<Vec<_>>();
    ensure!(
        actual_handlers == MAIN_DIALOGUE_STATE_HANDLERS,
        "main dialogue state handler table changed"
    );

    let mut indices_by_handler: BTreeMap<u16, Vec<usize>> = BTreeMap::new();
    for (state, handler) in MAIN_DIALOGUE_STATE_HANDLERS.iter().copied().enumerate() {
        indices_by_handler.entry(handler).or_default().push(state);
    }
    let handlers = MAIN_DIALOGUE_STATE_HANDLERS
        .iter()
        .copied()
        .enumerate()
        .map(|(state, cpu_address)| {
            let file_offset = if cpu_address >= FIXED_CPU_START {
                fixed_cpu_to_file_offset(cpu_address)?
            } else {
                switchable_cpu_to_file_offset(MAIN_DIALOGUE_PRG_BANK, cpu_address)?
            };
            Ok(DialogueStateHandlerReport {
                state,
                cpu_address,
                cpu_address_hex: format!("0x{cpu_address:04X}"),
                file_offset,
                file_offset_hex: format!("0x{file_offset:05X}"),
                structural_role: MAIN_DIALOGUE_HANDLER_ROLES[state],
                alias_state_indices: indices_by_handler[&cpu_address]
                    .iter()
                    .copied()
                    .filter(|other| *other != state)
                    .collect(),
            })
        })
        .collect::<Result<Vec<_>>>()?;
    let no_op_file_offset = fixed_cpu_to_file_offset(MAIN_DIALOGUE_STATE_HANDLERS[0])?;
    ensure!(
        source.get(no_op_file_offset) == Some(&0x60),
        "main dialogue no-op state handler changed"
    );

    let code_regions = MAIN_DIALOGUE_PREFIX_CODE_REGIONS
        .iter()
        .map(|region| {
            let file_offset =
                switchable_cpu_to_file_offset(MAIN_DIALOGUE_PRG_BANK, region.cpu_address)?;
            let end = file_offset
                .checked_add(region.bytes.len())
                .context("main dialogue prefix code range overflow")?;
            ensure!(
                source.get(file_offset..end) == Some(region.bytes),
                "main dialogue prefix code {} changed",
                region.role
            );
            Ok(CodeRegionReport {
                role: region.role,
                cpu_address: region.cpu_address,
                cpu_address_hex: format!("0x{:04X}", region.cpu_address),
                file_offset,
                file_offset_hex: format!("0x{file_offset:05X}"),
                byte_count: region.bytes.len(),
                code_sha1: sha1_hex(region.bytes),
            })
        })
        .collect::<Result<Vec<_>>>()?;

    Ok(MainDialogueStateMachineReport {
        prg_bank: MAIN_DIALOGUE_PRG_BANK,
        prg_bank_hex: format!("0x{MAIN_DIALOGUE_PRG_BANK:02X}"),
        state_address: MAIN_DIALOGUE_STATE_ADDRESS,
        state_address_hex: format!("0x{MAIN_DIALOGUE_STATE_ADDRESS:04X}"),
        dispatcher_cpu_address: MAIN_DIALOGUE_DISPATCHER_CPU_ADDRESS,
        dispatcher_cpu_address_hex: format!("0x{MAIN_DIALOGUE_DISPATCHER_CPU_ADDRESS:04X}"),
        dispatcher_file_offset,
        dispatcher_file_offset_hex: format!("0x{dispatcher_file_offset:05X}"),
        dispatcher_code_sha1: sha1_hex(MAIN_DIALOGUE_DISPATCHER_CODE),
        dispatch_helper_cpu_address: 0xC34C,
        dispatch_helper_cpu_address_hex: "0xC34C".to_owned(),
        handler_table_cpu_address: MAIN_DIALOGUE_HANDLER_TABLE_CPU_ADDRESS,
        handler_table_cpu_address_hex: format!("0x{MAIN_DIALOGUE_HANDLER_TABLE_CPU_ADDRESS:04X}"),
        handler_table_file_offset,
        handler_table_file_offset_hex: format!("0x{handler_table_file_offset:05X}"),
        handler_table_sha1: sha1_hex(handler_table_bytes),
        handler_count: handlers.len(),
        handlers,
        record_prefix_contract: MainRecordPrefixContract {
            optional_e5_prefix_code: OPTIONAL_E5_PREFIX_CODE,
            optional_e5_prefix_code_hex: format!("{OPTIONAL_E5_PREFIX_CODE:02X}"),
            optional_e5_prefix_byte_count: OPTIONAL_PREFIX_BYTE_COUNT,
            fixed_record_header_byte_count: FIXED_RECORD_HEADER_BYTE_COUNT,
            optional_e8_prefix_code: OPTIONAL_E8_PREFIX_CODE,
            optional_e8_prefix_code_hex: format!("{OPTIONAL_E8_PREFIX_CODE:02X}"),
            optional_e8_prefix_byte_count: OPTIONAL_PREFIX_BYTE_COUNT,
        },
        code_regions,
    })
}

fn extract_dialogue_table(source: &[u8], spec: &DialogueTableSpec) -> Result<DialogueTableReport> {
    ensure!(
        source.len() >= HEADER_SIZE + PRG_SIZE,
        "source is shorter than the PRG region"
    );
    ensure!(
        spec.source_prg_bank < 0x0F,
        "{} uses fixed or unavailable PRG bank {:02X}",
        spec.id,
        spec.source_prg_bank
    );

    let bank_start = switchable_bank_file_start(spec.source_prg_bank);
    let bank_end = bank_start + PRG_BANK_SIZE;
    let pointer_table_byte_count = spec
        .pointer_count
        .checked_mul(2)
        .context("dialogue pointer table length overflow")?;
    let pointer_table_end = spec
        .pointer_table_file_offset
        .checked_add(pointer_table_byte_count)
        .context("dialogue pointer table range overflow")?;
    ensure!(
        spec.pointer_count != 0,
        "{} declares an empty pointer table",
        spec.id
    );
    ensure!(
        spec.pointer_table_file_offset >= bank_start && pointer_table_end <= bank_end,
        "{} pointer table is outside source PRG bank {:02X}",
        spec.id,
        spec.source_prg_bank
    );
    ensure!(
        spec.data_file_start >= pointer_table_end && spec.data_file_start < bank_end,
        "{} data start is outside the post-table source-bank range",
        spec.id
    );

    let pointer_table_cpu_address =
        switchable_file_to_cpu(spec.source_prg_bank, spec.pointer_table_file_offset)?;
    let directory_binding = spec
        .directory_group
        .map(|group| {
            validate_directory_binding(
                source,
                spec.source_prg_bank,
                group,
                pointer_table_cpu_address,
                spec.id,
            )
        })
        .transpose()?;
    ensure!(
        !(spec.directory_group.is_some() && spec.separate_consumer.is_some()),
        "{} declares two consumer bindings",
        spec.id
    );
    let separate_consumer_binding = spec
        .separate_consumer
        .map(|consumer| {
            validate_separate_consumer(source, consumer, pointer_table_cpu_address, spec.id)
        })
        .transpose()?;

    let pointer_table_bytes = &source[spec.pointer_table_file_offset..pointer_table_end];
    let pointers = pointer_table_bytes
        .chunks_exact(2)
        .map(|bytes| u16::from_le_bytes([bytes[0], bytes[1]]))
        .collect::<Vec<_>>();
    let mut indices_by_pointer: BTreeMap<u16, Vec<usize>> = BTreeMap::new();
    for (index, pointer) in pointers.iter().copied().enumerate() {
        indices_by_pointer.entry(pointer).or_default().push(index);
    }

    let mut entries = Vec::with_capacity(pointers.len());
    let mut ordinary_target_file_offsets = Vec::new();
    for (index, pointer) in pointers.iter().copied().enumerate() {
        let alias_entry_indices = indices_by_pointer[&pointer]
            .iter()
            .copied()
            .filter(|other| *other != index)
            .collect();
        if let Some(handler) = spec
            .allowed_handler_targets
            .iter()
            .find(|handler| handler.cpu_address == pointer)
        {
            let file_offset =
                validate_handler_target(source, spec.source_prg_bank, handler, spec.id, index)?;
            entries.push(DialogueEntryReport {
                index,
                pointer_cpu_address: pointer,
                pointer_cpu_address_hex: format!("0x{pointer:04X}"),
                target_kind: "code_handler",
                file_offset,
                file_offset_hex: format!("0x{file_offset:05X}"),
                handler_role: Some(handler.role),
                alias_entry_indices,
                main_record_prefix: None,
            });
            continue;
        }

        let file_offset = switchable_cpu_to_file_offset(spec.source_prg_bank, pointer)
            .with_context(|| {
                format!(
                    "{} entry {index} pointer {pointer:04X} is outside its switchable PRG window",
                    spec.id
                )
            })?;
        ensure!(
            file_offset >= spec.data_file_start && file_offset < bank_end,
            "{} entry {index} points outside its declared data region",
            spec.id
        );
        ordinary_target_file_offsets.push(file_offset);
        let main_record_prefix = spec
            .directory_group
            .map(|_| inspect_main_record_prefix(source, file_offset, bank_end, spec.id, index))
            .transpose()?;
        entries.push(DialogueEntryReport {
            index,
            pointer_cpu_address: pointer,
            pointer_cpu_address_hex: format!("0x{pointer:04X}"),
            target_kind: "script_entry_start",
            file_offset,
            file_offset_hex: format!("0x{file_offset:05X}"),
            handler_role: None,
            alias_entry_indices,
            main_record_prefix,
        });
    }
    ensure!(
        ordinary_target_file_offsets.iter().min().copied() == Some(spec.data_file_start),
        "{} first declared data byte is not referenced by its pointer table",
        spec.id
    );

    let alias_groups = indices_by_pointer
        .values()
        .filter(|indices| indices.len() > 1)
        .collect::<Vec<_>>();
    let alias_group_count = alias_groups.len();
    let aliased_entry_count = alias_groups.iter().map(|indices| indices.len()).sum();
    let handler_target_entry_count = entries
        .iter()
        .filter(|entry| entry.target_kind == "code_handler")
        .count();
    let unique_script_entry_count = entries
        .iter()
        .filter(|entry| {
            entry.target_kind == "script_entry_start"
                && indices_by_pointer[&entry.pointer_cpu_address][0] == entry.index
        })
        .count();
    let main_record_prefix_summary = if spec.directory_group.is_some() {
        let unique_prefixes = entries
            .iter()
            .filter(|entry| indices_by_pointer[&entry.pointer_cpu_address][0] == entry.index)
            .filter_map(|entry| entry.main_record_prefix.as_ref())
            .collect::<Vec<_>>();
        ensure!(
            unique_prefixes.len() == unique_script_entry_count,
            "{} main record prefix coverage does not match its unique targets",
            spec.id
        );
        Some(MainRecordPrefixSummary {
            unique_target_count: unique_prefixes.len(),
            e5_prefix_unique_target_count: unique_prefixes
                .iter()
                .filter(|prefix| prefix.e5_prefix_present)
                .count(),
            e8_prefix_unique_target_count: unique_prefixes
                .iter()
                .filter(|prefix| prefix.e8_prefix_present)
                .count(),
            both_optional_prefixes_unique_target_count: unique_prefixes
                .iter()
                .filter(|prefix| prefix.e5_prefix_present && prefix.e8_prefix_present)
                .count(),
            no_optional_prefix_unique_target_count: unique_prefixes
                .iter()
                .filter(|prefix| !prefix.e5_prefix_present && !prefix.e8_prefix_present)
                .count(),
        })
    } else {
        None
    };

    Ok(DialogueTableReport {
        id: spec.id,
        role: spec.role,
        source_prg_bank: spec.source_prg_bank,
        source_prg_bank_hex: format!("0x{:02X}", spec.source_prg_bank),
        pointer_table_cpu_address,
        pointer_table_cpu_address_hex: format!("0x{pointer_table_cpu_address:04X}"),
        pointer_table_file_offset: spec.pointer_table_file_offset,
        pointer_table_file_offset_hex: format!("0x{:05X}", spec.pointer_table_file_offset),
        pointer_table_file_end_exclusive: pointer_table_end,
        pointer_table_file_end_exclusive_hex: format!("0x{pointer_table_end:05X}"),
        pointer_table_byte_count,
        pointer_table_sha1: sha1_hex(pointer_table_bytes),
        pointer_count: pointers.len(),
        unique_target_count: indices_by_pointer.len(),
        unique_script_entry_count,
        handler_target_entry_count,
        alias_group_count,
        aliased_entry_count,
        main_record_prefix_summary,
        data_file_start: spec.data_file_start,
        data_file_start_hex: format!("0x{:05X}", spec.data_file_start),
        directory_binding,
        separate_consumer_binding,
        consumer_binding_status: if spec.directory_group.is_some() {
            "main_dialogue_directory_root_confirmed"
        } else if spec.separate_consumer.is_some() {
            "separate_pointer_loader_confirmed"
        } else {
            "unresolved"
        },
        entries,
    })
}

fn inspect_main_record_prefix(
    source: &[u8],
    entry_file_offset: usize,
    bank_end: usize,
    table_id: &str,
    entry_index: usize,
) -> Result<MainRecordPrefixReport> {
    ensure!(
        entry_file_offset < bank_end,
        "{table_id} entry {entry_index} begins outside its source bank"
    );
    let e5_prefix_present = source[entry_file_offset] == OPTIONAL_E5_PREFIX_CODE;
    let e5_prefix_byte_count = if e5_prefix_present {
        OPTIONAL_PREFIX_BYTE_COUNT
    } else {
        0
    };
    let fixed_record_header_file_offset = entry_file_offset
        .checked_add(e5_prefix_byte_count)
        .context("main record E5 prefix range overflow")?;
    let after_fixed_record_header = fixed_record_header_file_offset
        .checked_add(FIXED_RECORD_HEADER_BYTE_COUNT)
        .context("main fixed record header range overflow")?;
    ensure!(
        after_fixed_record_header < bank_end,
        "{table_id} entry {entry_index} record prefix crosses its source bank"
    );
    let e8_prefix_present = source[after_fixed_record_header] == OPTIONAL_E8_PREFIX_CODE;
    let e8_prefix_byte_count = if e8_prefix_present {
        OPTIONAL_PREFIX_BYTE_COUNT
    } else {
        0
    };
    let first_line_file_offset = after_fixed_record_header
        .checked_add(e8_prefix_byte_count)
        .context("main record E8 prefix range overflow")?;
    ensure!(
        first_line_file_offset < bank_end,
        "{table_id} entry {entry_index} first line begins outside its source bank"
    );

    Ok(MainRecordPrefixReport {
        e5_prefix_present,
        e5_prefix_byte_count,
        fixed_record_header_file_offset,
        fixed_record_header_file_offset_hex: format!("0x{fixed_record_header_file_offset:05X}"),
        fixed_record_header_byte_count: FIXED_RECORD_HEADER_BYTE_COUNT,
        e8_prefix_present,
        e8_prefix_byte_count,
        first_line_file_offset,
        first_line_file_offset_hex: format!("0x{first_line_file_offset:05X}"),
        total_prefix_byte_count: first_line_file_offset - entry_file_offset,
    })
}

fn validate_separate_consumer(
    source: &[u8],
    consumer: SeparateConsumerSpec,
    expected_table_cpu_address: u16,
    table_id: &str,
) -> Result<SeparateConsumerBindingReport> {
    let loader_file_offset =
        switchable_cpu_to_file_offset(consumer.prg_bank, consumer.loader_cpu_address)?;
    let loader_end = loader_file_offset
        .checked_add(consumer.loader_code.len())
        .context("separate dialogue consumer range overflow")?;
    ensure!(
        source.get(loader_file_offset..loader_end) == Some(consumer.loader_code),
        "{table_id} separate pointer loader changed"
    );
    let table_root_cell_cpu_address = consumer
        .table_root_cell_cpu_address
        .checked_add(u16::from(consumer.table_set_index) * 2)
        .context("separate dialogue table-root cell overflow")?;
    let table_root_cell_file_offset =
        switchable_cpu_to_file_offset(consumer.prg_bank, table_root_cell_cpu_address)?;
    let resolved_pointer_table_cpu_address = u16::from_le_bytes([
        source[table_root_cell_file_offset],
        source[table_root_cell_file_offset + 1],
    ]);
    ensure!(
        resolved_pointer_table_cpu_address == expected_table_cpu_address,
        "{table_id} separate pointer-table root changed: expected {expected_table_cpu_address:04X}, found {resolved_pointer_table_cpu_address:04X}"
    );

    Ok(SeparateConsumerBindingReport {
        prg_bank: consumer.prg_bank,
        prg_bank_hex: format!("0x{:02X}", consumer.prg_bank),
        loader_cpu_address: consumer.loader_cpu_address,
        loader_cpu_address_hex: format!("0x{:04X}", consumer.loader_cpu_address),
        loader_file_offset,
        loader_file_offset_hex: format!("0x{loader_file_offset:05X}"),
        loader_code_sha1: sha1_hex(consumer.loader_code),
        table_set_selector: consumer.table_set_selector,
        table_set_index: consumer.table_set_index,
        entry_index_selector: consumer.entry_index_selector,
        destination_pointer: consumer.destination_pointer,
        table_root_cell_cpu_address,
        table_root_cell_cpu_address_hex: format!("0x{table_root_cell_cpu_address:04X}"),
        table_root_cell_file_offset,
        table_root_cell_file_offset_hex: format!("0x{table_root_cell_file_offset:05X}"),
        resolved_pointer_table_cpu_address,
        resolved_pointer_table_cpu_address_hex: format!(
            "0x{resolved_pointer_table_cpu_address:04X}"
        ),
    })
}

fn validate_directory_binding(
    source: &[u8],
    source_prg_bank: u8,
    group: u8,
    expected_table_cpu_address: u16,
    table_id: &str,
) -> Result<DirectoryBindingReport> {
    ensure!(
        group < 0x10,
        "{table_id} dialogue directory group is outside one selector nibble"
    );
    let directory_entry_cpu_address = DIALOGUE_DIRECTORY_CPU_ADDRESS
        .checked_add(u16::from(group) * 2)
        .context("dialogue directory CPU address overflow")?;
    ensure!(
        directory_entry_cpu_address + 1 < SWITCHABLE_CPU_END_EXCLUSIVE,
        "{table_id} dialogue directory entry is outside the source bank"
    );
    let directory_entry_file_offset =
        switchable_cpu_to_file_offset(source_prg_bank, directory_entry_cpu_address)?;
    let resolved_pointer_table_cpu_address = u16::from_le_bytes([
        source[directory_entry_file_offset],
        source[directory_entry_file_offset + 1],
    ]);
    ensure!(
        resolved_pointer_table_cpu_address == expected_table_cpu_address,
        "{table_id} dialogue directory root changed: expected {expected_table_cpu_address:04X}, found {resolved_pointer_table_cpu_address:04X}"
    );
    let selector = (source_prg_bank << 4) | group;

    Ok(DirectoryBindingReport {
        selector,
        selector_hex: format!("0x{selector:02X}"),
        directory_group: group,
        directory_entry_cpu_address,
        directory_entry_cpu_address_hex: format!("0x{directory_entry_cpu_address:04X}"),
        directory_entry_file_offset,
        directory_entry_file_offset_hex: format!("0x{directory_entry_file_offset:05X}"),
        resolved_pointer_table_cpu_address,
        resolved_pointer_table_cpu_address_hex: format!(
            "0x{resolved_pointer_table_cpu_address:04X}"
        ),
    })
}

fn validate_handler_target(
    source: &[u8],
    source_prg_bank: u8,
    handler: &HandlerTargetSpec,
    table_id: &str,
    entry_index: usize,
) -> Result<usize> {
    ensure!(
        !handler.expected_code.is_empty(),
        "{table_id} entry {entry_index} declares an empty fixed-handler signature"
    );
    let file_offset = if handler.cpu_address >= FIXED_CPU_START {
        fixed_cpu_to_file_offset(handler.cpu_address)
    } else {
        switchable_cpu_to_file_offset(source_prg_bank, handler.cpu_address)
    }
    .with_context(|| format!("{table_id} entry {entry_index}"))?;
    let end = file_offset
        .checked_add(handler.expected_code.len())
        .context("fixed-handler signature range overflow")?;
    ensure!(
        source.get(file_offset..end) == Some(handler.expected_code),
        "{table_id} entry {entry_index} code handler {} changed",
        handler.role
    );
    Ok(file_offset)
}

fn switchable_bank_file_start(bank: u8) -> usize {
    HEADER_SIZE + usize::from(bank) * PRG_BANK_SIZE
}

fn switchable_file_to_cpu(bank: u8, file_offset: usize) -> Result<u16> {
    let bank_start = switchable_bank_file_start(bank);
    let relative = file_offset
        .checked_sub(bank_start)
        .with_context(|| format!("file offset {file_offset:05X} is before PRG bank {bank:02X}"))?;
    ensure!(
        relative < PRG_BANK_SIZE,
        "file offset {file_offset:05X} is outside PRG bank {bank:02X}"
    );
    Ok(SWITCHABLE_CPU_START + relative as u16)
}

fn switchable_cpu_to_file_offset(bank: u8, cpu_address: u16) -> Result<usize> {
    ensure!(
        (SWITCHABLE_CPU_START..SWITCHABLE_CPU_END_EXCLUSIVE).contains(&cpu_address),
        "CPU address {cpu_address:04X} is outside the switchable PRG window"
    );
    Ok(switchable_bank_file_start(bank) + usize::from(cpu_address - SWITCHABLE_CPU_START))
}

fn fixed_cpu_to_file_offset(cpu_address: u16) -> Result<usize> {
    ensure!(
        cpu_address >= FIXED_CPU_START,
        "CPU address {cpu_address:04X} is outside the fixed PRG window"
    );
    Ok(HEADER_SIZE + PRG_SIZE - PRG_BANK_SIZE + usize::from(cpu_address - FIXED_CPU_START))
}

#[cfg(test)]
mod tests {
    use super::*;

    const SYNTHETIC_BANK: u8 = 0x02;
    const SYNTHETIC_TABLE_OFFSET: usize = HEADER_SIZE + 2 * PRG_BANK_SIZE + 0x0100;
    const SYNTHETIC_DATA_START: usize = HEADER_SIZE + 2 * PRG_BANK_SIZE + 0x0200;
    const TEST_FIXED_HANDLER: HandlerTargetSpec = HandlerTargetSpec {
        cpu_address: 0xC73D,
        role: "empty_dialogue_handler",
        expected_code: &[0x60],
    };

    fn synthetic_source() -> Vec<u8> {
        vec![0; HEADER_SIZE + PRG_SIZE]
    }

    fn synthetic_spec(pointer_count: usize) -> DialogueTableSpec {
        DialogueTableSpec {
            id: "synthetic-dialogue",
            role: "synthetic_dialogue",
            source_prg_bank: SYNTHETIC_BANK,
            pointer_table_file_offset: SYNTHETIC_TABLE_OFFSET,
            pointer_count,
            data_file_start: SYNTHETIC_DATA_START,
            directory_group: None,
            separate_consumer: None,
            allowed_handler_targets: NO_HANDLER_TARGETS,
        }
    }

    fn write_pointer(source: &mut [u8], index: usize, pointer: u16) {
        let offset = SYNTHETIC_TABLE_OFFSET + index * 2;
        source[offset..offset + 2].copy_from_slice(&pointer.to_le_bytes());
    }

    fn write_main_dialogue_state_machine(source: &mut [u8]) {
        let dispatcher_file_offset = switchable_cpu_to_file_offset(
            MAIN_DIALOGUE_PRG_BANK,
            MAIN_DIALOGUE_DISPATCHER_CPU_ADDRESS,
        )
        .unwrap();
        source
            [dispatcher_file_offset..dispatcher_file_offset + MAIN_DIALOGUE_DISPATCHER_CODE.len()]
            .copy_from_slice(MAIN_DIALOGUE_DISPATCHER_CODE);
        let handler_table_file_offset = switchable_cpu_to_file_offset(
            MAIN_DIALOGUE_PRG_BANK,
            MAIN_DIALOGUE_HANDLER_TABLE_CPU_ADDRESS,
        )
        .unwrap();
        for (state, handler) in MAIN_DIALOGUE_STATE_HANDLERS.iter().enumerate() {
            let offset = handler_table_file_offset + state * 2;
            source[offset..offset + 2].copy_from_slice(&handler.to_le_bytes());
        }
        for region in &MAIN_DIALOGUE_PREFIX_CODE_REGIONS {
            let file_offset =
                switchable_cpu_to_file_offset(MAIN_DIALOGUE_PRG_BANK, region.cpu_address).unwrap();
            source[file_offset..file_offset + region.bytes.len()].copy_from_slice(region.bytes);
        }
        let no_op_file_offset = fixed_cpu_to_file_offset(MAIN_DIALOGUE_STATE_HANDLERS[0]).unwrap();
        source[no_op_file_offset] = 0x60;
    }

    #[test]
    fn reports_aliases_without_reading_dialogue_bytes() {
        let mut source = synthetic_source();
        let spec = synthetic_spec(3);
        write_pointer(&mut source, 0, 0x8200);
        write_pointer(&mut source, 1, 0x8200);
        write_pointer(&mut source, 2, 0x8210);

        let report = extract_dialogue_table(&source, &spec).unwrap();

        assert_eq!(report.pointer_count, 3);
        assert_eq!(report.unique_target_count, 2);
        assert_eq!(report.alias_group_count, 1);
        assert_eq!(report.aliased_entry_count, 2);
        assert_eq!(report.entries[0].alias_entry_indices, vec![1]);
        assert_eq!(report.entries[1].alias_entry_indices, vec![0]);
        assert_eq!(report.entries[2].alias_entry_indices, Vec::<usize>::new());
    }

    #[test]
    fn validates_the_main_dialogue_state_dispatch_and_prefix_code() {
        let mut source = synthetic_source();
        write_main_dialogue_state_machine(&mut source);

        let report = build_main_dialogue_state_machine(&source).unwrap();

        assert_eq!(report.handler_count, 18);
        assert_eq!(report.handlers[3].alias_state_indices, vec![7]);
        assert_eq!(report.handlers[7].alias_state_indices, vec![3]);
        assert_eq!(
            report.record_prefix_contract.optional_e5_prefix_byte_count,
            6
        );
        assert_eq!(
            report.record_prefix_contract.fixed_record_header_byte_count,
            4
        );
        assert_eq!(
            report.record_prefix_contract.optional_e8_prefix_byte_count,
            6
        );

        let e5_file_offset = switchable_cpu_to_file_offset(MAIN_DIALOGUE_PRG_BANK, 0x80A2).unwrap();
        source[e5_file_offset + 9] ^= 0x01;
        let error = build_main_dialogue_state_machine(&source)
            .unwrap_err()
            .to_string();
        assert!(error.contains("prefix code inspect_and_consume_optional_E5_prefix changed"));
    }

    #[test]
    fn locates_the_first_line_after_only_declared_record_prefixes() {
        let mut source = synthetic_source();
        let bank_end = switchable_bank_file_start(SYNTHETIC_BANK) + PRG_BANK_SIZE;
        let entry_file_offset = SYNTHETIC_DATA_START;
        source[entry_file_offset] = OPTIONAL_E5_PREFIX_CODE;
        source[entry_file_offset + OPTIONAL_PREFIX_BYTE_COUNT + FIXED_RECORD_HEADER_BYTE_COUNT] =
            OPTIONAL_E8_PREFIX_CODE;

        let full = inspect_main_record_prefix(
            &source,
            entry_file_offset,
            bank_end,
            "synthetic-dialogue",
            0,
        )
        .unwrap();
        assert!(full.e5_prefix_present);
        assert!(full.e8_prefix_present);
        assert_eq!(full.total_prefix_byte_count, 16);
        assert_eq!(full.first_line_file_offset, entry_file_offset + 16);

        let plain_entry_file_offset = entry_file_offset + 0x40;
        let plain = inspect_main_record_prefix(
            &source,
            plain_entry_file_offset,
            bank_end,
            "synthetic-dialogue",
            1,
        )
        .unwrap();
        assert!(!plain.e5_prefix_present);
        assert!(!plain.e8_prefix_present);
        assert_eq!(plain.total_prefix_byte_count, 4);
        assert_eq!(plain.first_line_file_offset, plain_entry_file_offset + 4);
    }

    #[test]
    fn rejects_a_pointer_outside_the_declared_source_bank_window() {
        let mut source = synthetic_source();
        let spec = synthetic_spec(1);
        write_pointer(&mut source, 0, 0xC000);

        let error = extract_dialogue_table(&source, &spec)
            .unwrap_err()
            .to_string();

        assert!(error.contains("outside its switchable PRG window"));
    }

    #[test]
    fn admits_only_an_exact_declared_code_handler() {
        let mut source = synthetic_source();
        let mut spec = synthetic_spec(2);
        spec.allowed_handler_targets = &[TEST_FIXED_HANDLER];
        write_pointer(&mut source, 0, 0x8200);
        write_pointer(&mut source, 1, TEST_FIXED_HANDLER.cpu_address);
        let handler_file_offset = fixed_cpu_to_file_offset(TEST_FIXED_HANDLER.cpu_address).unwrap();
        source[handler_file_offset] = 0x60;

        let report = extract_dialogue_table(&source, &spec).unwrap();
        assert_eq!(report.entries[1].target_kind, "code_handler");
        assert_eq!(
            report.entries[1].handler_role,
            Some("empty_dialogue_handler")
        );

        source[handler_file_offset] = 0xEA;
        let error = extract_dialogue_table(&source, &spec)
            .unwrap_err()
            .to_string();
        assert!(error.contains("code handler empty_dialogue_handler changed"));
    }

    #[test]
    fn rejects_a_pointer_table_that_crosses_its_prg_bank() {
        let source = synthetic_source();
        let mut spec = synthetic_spec(2);
        spec.pointer_table_file_offset =
            switchable_bank_file_start(SYNTHETIC_BANK) + PRG_BANK_SIZE - 2;

        let error = extract_dialogue_table(&source, &spec)
            .unwrap_err()
            .to_string();

        assert!(error.contains("pointer table is outside source PRG bank"));
    }

    #[test]
    fn rejects_a_changed_dialogue_directory_root() {
        let mut source = synthetic_source();
        let mut spec = synthetic_spec(1);
        spec.directory_group = Some(0);
        write_pointer(&mut source, 0, 0x8200);
        let directory_file_offset =
            switchable_cpu_to_file_offset(SYNTHETIC_BANK, DIALOGUE_DIRECTORY_CPU_ADDRESS).unwrap();
        source[directory_file_offset..directory_file_offset + 2]
            .copy_from_slice(&0x8300_u16.to_le_bytes());

        let error = extract_dialogue_table(&source, &spec)
            .unwrap_err()
            .to_string();

        assert!(error.contains("dialogue directory root changed"));
    }

    #[test]
    fn rejects_a_changed_separate_pointer_loader() {
        let mut source = synthetic_source();
        let mut spec = synthetic_spec(1);
        const CONSUMER_CODE: &[u8] = &[0xA9, 0x00, 0x60];
        spec.separate_consumer = Some(SeparateConsumerSpec {
            prg_bank: SYNTHETIC_BANK,
            loader_cpu_address: 0x8000,
            loader_code: CONSUMER_CODE,
            table_set_index: 0,
            table_root_cell_cpu_address: 0x8010,
            table_set_selector: "synthetic_table_set",
            entry_index_selector: "synthetic_entry_index",
            destination_pointer: "synthetic_destination",
        });
        let loader_file_offset = switchable_bank_file_start(SYNTHETIC_BANK);
        source[loader_file_offset..loader_file_offset + CONSUMER_CODE.len()]
            .copy_from_slice(CONSUMER_CODE);
        source[loader_file_offset + 0x10..loader_file_offset + 0x12]
            .copy_from_slice(&0x8300_u16.to_le_bytes());
        write_pointer(&mut source, 0, 0x8200);

        let error = extract_dialogue_table(&source, &spec)
            .unwrap_err()
            .to_string();

        assert!(error.contains("separate pointer-table root changed"));
    }
}
