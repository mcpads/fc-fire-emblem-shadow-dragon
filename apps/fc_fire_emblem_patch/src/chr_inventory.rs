use std::{collections::BTreeSet, fs, io::Cursor, path::Path};

use anyhow::{Context, Result, ensure};
use serde::Serialize;

use crate::{
    font_slots::{
        LATCH_TRIGGER_CODES, LAYOUT_RESERVED_CODES, PRESERVED_DISPLAY_CODES,
        protected_original_codes,
    },
    options::{OPTIONS_TABLE_OFFSET, SOURCE_OPTIONS_TABLE},
    rom::{EXPECTED_CHR_SHA1, EXPECTED_SOURCE_SHA1, PRG_SIZE, Rom},
    sha1_hex,
    static_analysis::{
        AbsoluteTransferCandidate, AbsoluteWriteCandidate, find_absolute_transfer_candidates,
        find_absolute_write_candidates,
    },
};

mod report;
mod sheet_render;
mod source_analysis;
mod source_spec;
#[cfg(test)]
mod tests;
mod tile_analysis;

pub use report::FontSupplySummary;
use report::*;
use sheet_render::*;
use source_analysis::*;
use source_spec::*;
use tile_analysis::*;

pub fn analyze_font_supply(
    source_path: &Path,
    report_path: &Path,
    sheet_path: &Path,
    scale: u32,
) -> Result<FontSupplySummary> {
    ensure!(
        report_path != sheet_path,
        "report and sheet paths must differ"
    );
    ensure!((1..=8).contains(&scale), "sheet scale must be from 1 to 8");

    let rom = Rom::from_path(source_path)?;
    rom.verify_supported_japanese()?;
    let report = build_report(&rom)?;
    let sheet =
        render_font_page_sheet(&rom.chr()[..CHR_PAGE_SIZE], &report.font_page.slots, scale)?;
    let mut report_bytes = serde_json::to_vec_pretty(&report).context("serialize font report")?;
    report_bytes.push(b'\n');

    write_file(report_path, &report_bytes)?;
    write_file(sheet_path, &sheet)?;

    Ok(FontSupplySummary {
        report_sha1: sha1_hex(&report_bytes),
        page_count: report.summary.page_count,
        protected_code_count: report.summary.protected_font_code_count,
        unresolved_code_count: report.summary.unresolved_font_code_count,
    })
}
fn build_report(rom: &Rom) -> Result<FontSupplyReport> {
    validate_known_references(rom.data())?;
    let mmc4_control_routines = describe_mmc4_control_routines(rom.prg())?;
    let mmc4_chr_bank_writers = describe_mmc4_chr_writers(rom.prg())?;
    let mmc4_register_write_candidates = MMC4_REGISTER_SPECS
        .iter()
        .map(|(register_address, role)| Mmc4RegisterWriteInventory {
            register_address: *register_address,
            register_address_hex: format!("0x{register_address:04X}"),
            role,
            candidates: find_absolute_write_candidates(rom.prg(), *register_address),
        })
        .collect();
    let mmc4_adjacent_chr_write_candidate_groups =
        find_adjacent_chr_write_candidate_groups(rom.prg());
    ensure!(
        rom.chr().len().is_multiple_of(CHR_PAGE_SIZE),
        "CHR size is not aligned to 4 KiB pages"
    );

    let pages: Vec<PageReport> = rom
        .chr()
        .chunks_exact(CHR_PAGE_SIZE)
        .enumerate()
        .map(|(page_index, page)| summarize_page(page_index, page))
        .collect();
    let slots = describe_font_page(&rom.chr()[..CHR_PAGE_SIZE]);
    let protected_font_code_count = slots
        .iter()
        .filter(|slot| slot.code_assignment == Decision::Protected)
        .count();
    let unresolved_font_code_count = slots.len() - protected_font_code_count;
    let nonblank_tile_count = pages.iter().map(|page| page.nonblank_tile_count).sum();
    let blank_pattern_count = pages.iter().map(|page| page.blank_pattern_count).sum();
    let active_slot_ceiling = calculate_active_slot_ceiling(&slots)?;

    Ok(FontSupplyReport {
        schema_version: 6,
        scope: ReportScope {
            source_sha1: EXPECTED_SOURCE_SHA1,
            chr_sha1: EXPECTED_CHR_SHA1,
            mapper: rom.mapper(),
            font_page_index: FONT_PAGE_INDEX,
        },
        tile_format: TileFormat {
            width: 8,
            height: 8,
            bits_per_pixel: 2,
            bytes_per_tile: TILE_SIZE,
            chr_page_size: CHR_PAGE_SIZE,
            tiles_per_page: TILES_PER_PAGE,
        },
        summary: ReportSummary {
            page_count: pages.len(),
            tile_count: rom.chr().len() / TILE_SIZE,
            nonblank_tile_count,
            blank_pattern_count,
            protected_font_code_count,
            unresolved_font_code_count,
            available_font_code_count: 0,
        },
        mmc4_control_routines,
        mmc4_chr_bank_writers,
        mmc4_register_write_candidates,
        mmc4_adjacent_chr_write_candidate_groups,
        known_references: KNOWN_REFERENCES
            .iter()
            .map(|reference| ReferenceReport {
                id: reference.id,
                file_offset: reference.file_offset,
                file_offset_hex: format!("0x{:05X}", reference.file_offset),
                byte_length: reference.expected.len(),
                bytes_hex: hex_bytes(reference.expected),
                displayed_text: reference.displayed_text,
                consumer: reference.consumer,
                scope: reference.scope,
                evidence: reference.evidence,
            })
            .collect(),
        pages,
        font_page: FontPageReport {
            page_index: FONT_PAGE_INDEX,
            chr_offset: 0,
            chr_offset_hex: "0x00000".to_owned(),
            slots,
        },
        active_slot_ceiling,
        unknowns: vec![
            "No font slot is classified as available until every consumer and runtime state is excluded.",
            "References list only confirmed tables; it is not the complete text or tile reference population.",
            "Direct JSR and JMP candidates are byte-pattern matches; instruction boundaries and render-path semantics remain unconfirmed.",
            "Direct absolute mapper-register write candidates may include data; runtime execution or disassembly is required before patching them.",
            "Adjacent CHR-write groups are prioritization hints, not proof that any member is executable code.",
            "The current Hangul slot ceiling is not a final per-screen budget; unresolved consumers may reserve more codes.",
        ],
    })
}
