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

struct DialogueTableSpec {
    id: &'static str,
    role: &'static str,
    source_prg_bank: u8,
    pointer_table_file_offset: usize,
    pointer_count: usize,
    data_file_start: usize,
    directory_group: Option<u8>,
    separate_consumer: Option<SeparateConsumerSpec>,
    allowed_fixed_handlers: &'static [FixedHandlerSpec],
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

struct FixedHandlerSpec {
    cpu_address: u16,
    role: &'static str,
    expected_code: &'static [u8],
}

const NO_FIXED_HANDLERS: &[FixedHandlerSpec] = &[];

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
        allowed_fixed_handlers: NO_FIXED_HANDLERS,
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
        allowed_fixed_handlers: NO_FIXED_HANDLERS,
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
        allowed_fixed_handlers: NO_FIXED_HANDLERS,
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
        allowed_fixed_handlers: NO_FIXED_HANDLERS,
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
        allowed_fixed_handlers: NO_FIXED_HANDLERS,
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
        allowed_fixed_handlers: NO_FIXED_HANDLERS,
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
        allowed_fixed_handlers: NO_FIXED_HANDLERS,
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
        allowed_fixed_handlers: NO_FIXED_HANDLERS,
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
    alias_group_count: usize,
    aliased_entry_count: usize,
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
        alias_group_count: tables.iter().map(|table| table.alias_group_count).sum(),
        aliased_entry_count: tables.iter().map(|table| table.aliased_entry_count).sum(),
    };

    Ok(DialogueStructureReport {
        schema_version: 1,
        scope: ReportScope {
            source_sha1: EXPECTED_SOURCE_SHA1,
            translation_direction: "ja_to_ko",
            preserve_existing_english: true,
            proof_boundary: "exact pointer-table ranges, switchable-bank target mapping, aliases, seven main dialogue-directory roots, and the separate battle pointer loader; no dialogue bytes or translations are emitted",
        },
        summary,
        tables,
        unknowns: vec![
            "Pointer targets are entry starts, not proven script byte ranges.",
            "The outer dialogue record state machine and complete entry termination rules remain unresolved.",
            "Role labels began as external map candidates and do not prove every entry's gameplay context.",
            "Existing English and numeric content remains protected and is not a translation target.",
        ],
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
            .allowed_fixed_handlers
            .iter()
            .find(|handler| handler.cpu_address == pointer)
        {
            let file_offset = validate_fixed_handler(source, handler, spec.id, index)?;
            entries.push(DialogueEntryReport {
                index,
                pointer_cpu_address: pointer,
                pointer_cpu_address_hex: format!("0x{pointer:04X}"),
                target_kind: "fixed_handler",
                file_offset,
                file_offset_hex: format!("0x{file_offset:05X}"),
                handler_role: Some(handler.role),
                alias_entry_indices,
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
        entries.push(DialogueEntryReport {
            index,
            pointer_cpu_address: pointer,
            pointer_cpu_address_hex: format!("0x{pointer:04X}"),
            target_kind: "script_entry_start",
            file_offset,
            file_offset_hex: format!("0x{file_offset:05X}"),
            handler_role: None,
            alias_entry_indices,
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
        alias_group_count,
        aliased_entry_count,
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

fn validate_fixed_handler(
    source: &[u8],
    handler: &FixedHandlerSpec,
    table_id: &str,
    entry_index: usize,
) -> Result<usize> {
    ensure!(
        !handler.expected_code.is_empty(),
        "{table_id} entry {entry_index} declares an empty fixed-handler signature"
    );
    let file_offset = fixed_cpu_to_file_offset(handler.cpu_address)
        .with_context(|| format!("{table_id} entry {entry_index}"))?;
    let end = file_offset
        .checked_add(handler.expected_code.len())
        .context("fixed-handler signature range overflow")?;
    ensure!(
        source.get(file_offset..end) == Some(handler.expected_code),
        "{table_id} entry {entry_index} fixed handler {} changed",
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
    const TEST_FIXED_HANDLER: FixedHandlerSpec = FixedHandlerSpec {
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
            allowed_fixed_handlers: NO_FIXED_HANDLERS,
        }
    }

    fn write_pointer(source: &mut [u8], index: usize, pointer: u16) {
        let offset = SYNTHETIC_TABLE_OFFSET + index * 2;
        source[offset..offset + 2].copy_from_slice(&pointer.to_le_bytes());
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
    fn admits_only_an_exact_declared_fixed_handler() {
        let mut source = synthetic_source();
        let mut spec = synthetic_spec(2);
        spec.allowed_fixed_handlers = &[TEST_FIXED_HANDLER];
        write_pointer(&mut source, 0, 0x8200);
        write_pointer(&mut source, 1, TEST_FIXED_HANDLER.cpu_address);
        let handler_file_offset = fixed_cpu_to_file_offset(TEST_FIXED_HANDLER.cpu_address).unwrap();
        source[handler_file_offset] = 0x60;

        let report = extract_dialogue_table(&source, &spec).unwrap();
        assert_eq!(report.entries[1].target_kind, "fixed_handler");
        assert_eq!(
            report.entries[1].handler_role,
            Some("empty_dialogue_handler")
        );

        source[handler_file_offset] = 0xEA;
        let error = extract_dialogue_table(&source, &spec)
            .unwrap_err()
            .to_string();
        assert!(error.contains("fixed handler empty_dialogue_handler changed"));
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
