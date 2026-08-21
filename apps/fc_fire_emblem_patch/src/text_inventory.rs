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

mod battle_message_templates;
mod dialogue_path;
mod fixed_workspace;
mod report;
mod source_spec;
mod table_analysis;
#[cfg(test)]
mod tests;

use dialogue_path::*;
pub(crate) use fixed_workspace::{
    FixedTextLogicalByte, FixedTextPlan, FixedTextPlannedEntry, decode_source_markup,
    encode_target_markup, extract_fixed_text_workspace, extract_location_name_workspace,
    extract_unit_name_workspace, is_japanese_character, plan_fixed_text, plan_location_name_text,
    plan_unit_name_text,
};
pub use report::TextInventorySummary;
use report::*;
use source_spec::*;
pub(crate) use source_spec::{DIALOGUE_CONTROL_SPECS, DIALOGUE_SCRIPT_CONTROL_CODES};
use table_analysis::*;

/// The dialogue renderer consumes dakuten and handakuten as overlay marks on the
/// preceding source glyph. They occupy storage bytes, but do not advance the
/// rendered text cursor by another cell.
pub(crate) fn dialogue_literal_display_cell_count(code: u8) -> usize {
    usize::from(!COMPOSITE_TEXT_LAYOUT_CODES.contains(&code))
}

#[derive(Clone, Debug)]
pub(crate) struct TextTableBudget {
    pub(crate) id: &'static str,
    pub(crate) pointer_count: usize,
    pub(crate) unique_string_count: usize,
    pub(crate) referenced_text_byte_count: usize,
    pub(crate) unique_text_storage_byte_count: usize,
    pub(crate) max_entry_byte_count: usize,
    pub(crate) source_codes: BTreeSet<u8>,
}

pub(crate) fn scoped_text_table_budgets(
    source: &[u8],
    requested_ids: &[&str],
) -> Result<Vec<TextTableBudget>> {
    requested_text_table_specs(requested_ids)?
        .into_iter()
        .map(|spec| {
            let table = extract_table(source, spec)?;
            Ok(TextTableBudget {
                id: table.id,
                pointer_count: table.pointer_count,
                unique_string_count: table.unique_string_count,
                referenced_text_byte_count: table.referenced_text_byte_count,
                unique_text_storage_byte_count: table.unique_text_storage_byte_count,
                max_entry_byte_count: table
                    .entries
                    .iter()
                    .map(|entry| entry.byte_length)
                    .max()
                    .unwrap_or(0),
                source_codes: table
                    .source_code_usage
                    .iter()
                    .map(|usage| usage.code)
                    .collect(),
            })
        })
        .collect()
}

fn requested_text_table_specs(requested_ids: &[&str]) -> Result<Vec<&'static TextTableSpec>> {
    let mut seen = BTreeSet::new();
    requested_ids
        .iter()
        .map(|requested_id| {
            ensure!(
                seen.insert(*requested_id),
                "duplicate text table id {requested_id}"
            );
            TEXT_TABLE_SPECS
                .iter()
                .find(|spec| spec.id == *requested_id)
                .with_context(|| format!("unknown text table id {requested_id}"))
        })
        .collect()
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
        schema_version: 13,
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

pub(crate) fn protected_alphanumeric_glyph(code: u8) -> Option<&'static str> {
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
