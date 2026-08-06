use std::{collections::BTreeSet, fs, io::Cursor, path::Path};

use anyhow::{Context, Result, ensure};
use serde::Serialize;

use crate::{
    options::{OPTIONS_TABLE_OFFSET, SOURCE_OPTIONS_TABLE},
    rom::{EXPECTED_CHR_SHA1, EXPECTED_SOURCE_SHA1, PRG_SIZE, Rom},
    sha1_hex,
    static_analysis::{AbsoluteTransferCandidate, find_absolute_transfer_candidates},
};

const CHR_PAGE_SIZE: usize = 4 * 1024;
const TILE_SIZE: usize = 16;
const TILES_PER_PAGE: usize = CHR_PAGE_SIZE / TILE_SIZE;
const FONT_PAGE_INDEX: usize = 0;
const STATUS_LABELS_OFFSET: usize = 0x3447B;
const SOURCE_STATUS_LABELS: [u8; 32] = [
    0x7C, 0x7D, 0x7B, 0x8D, // STR:
    0x7C, 0x74, 0x72, 0x8D, // SKI:
    0x80, 0x75, 0x7F, 0x8D, // WLV:
    0x6A, 0x70, 0x72, 0x8D, // AGI:
    0x6D, 0x6E, 0x6F, 0x8D, // DEF:
    0x76, 0x78, 0x7F, 0x8D, // MOV:
    0x71, 0x9B, 0x79, 0x8D, // H.P:
    0x6E, 0x81, 0x79, 0x8D, // EXP:
];

const ENTRY_SEPARATOR: u8 = 0xED;
const TABLE_TERMINATOR: u8 = 0xEF;
const MMC4_LATCH_CODES: [u8; 2] = [0xFD, 0xFE];
const PRG_BANK_SIZE: usize = 16 * 1024;

struct Mmc4ChrWriter {
    cpu_address: u16,
    shadow_address: u8,
    hardware_register: u16,
    latch_domain: &'static str,
    expected: [u8; 8],
}

const MMC4_CHR_WRITERS: [Mmc4ChrWriter; 4] = [
    Mmc4ChrWriter {
        cpu_address: 0xC9AE,
        shadow_address: 0x59,
        hardware_register: 0xB000,
        latch_domain: "ppu_0000_fd",
        expected: [0x85, 0x59, 0x05, 0x52, 0x8D, 0x00, 0xB0, 0x60],
    },
    Mmc4ChrWriter {
        cpu_address: 0xC9B6,
        shadow_address: 0x5A,
        hardware_register: 0xC000,
        latch_domain: "ppu_0000_fe",
        expected: [0x85, 0x5A, 0x05, 0x52, 0x8D, 0x00, 0xC0, 0x60],
    },
    Mmc4ChrWriter {
        cpu_address: 0xC9BE,
        shadow_address: 0x5B,
        hardware_register: 0xD000,
        latch_domain: "ppu_1000_fd",
        expected: [0x85, 0x5B, 0x05, 0x52, 0x8D, 0x00, 0xD0, 0x60],
    },
    Mmc4ChrWriter {
        cpu_address: 0xC9C6,
        shadow_address: 0x5C,
        hardware_register: 0xE000,
        latch_domain: "ppu_1000_fe",
        expected: [0x85, 0x5C, 0x05, 0x52, 0x8D, 0x00, 0xE0, 0x60],
    },
];

const HEX_GLYPHS: [[u8; 5]; 16] = [
    [0b111, 0b101, 0b101, 0b101, 0b111],
    [0b010, 0b110, 0b010, 0b010, 0b111],
    [0b110, 0b001, 0b010, 0b100, 0b111],
    [0b110, 0b001, 0b010, 0b001, 0b110],
    [0b101, 0b101, 0b111, 0b001, 0b001],
    [0b111, 0b100, 0b110, 0b001, 0b110],
    [0b011, 0b100, 0b110, 0b101, 0b010],
    [0b111, 0b001, 0b010, 0b010, 0b010],
    [0b010, 0b101, 0b010, 0b101, 0b010],
    [0b010, 0b101, 0b011, 0b001, 0b110],
    [0b010, 0b101, 0b111, 0b101, 0b101],
    [0b110, 0b101, 0b110, 0b101, 0b110],
    [0b011, 0b100, 0b100, 0b100, 0b011],
    [0b110, 0b101, 0b101, 0b101, 0b110],
    [0b111, 0b100, 0b110, 0b100, 0b111],
    [0b111, 0b100, 0b110, 0b100, 0b100],
];

struct KnownReference {
    id: &'static str,
    file_offset: usize,
    expected: &'static [u8],
    displayed_text: &'static str,
    consumer: &'static str,
    scope: ReferenceScope,
    evidence: &'static str,
}

const KNOWN_REFERENCES: [KnownReference; 2] = [
    KnownReference {
        id: "options-label-table",
        file_offset: OPTIONS_TABLE_OFFSET,
        expected: &SOURCE_OPTIONS_TABLE,
        displayed_text: "サウンド / アニメーション / ウエイトタイマー",
        consumer: "options labels",
        scope: ReferenceScope::TranslatedJapanese,
        evidence: "confirmed static consumer and runtime display",
    },
    KnownReference {
        id: "status-label-table",
        file_offset: STATUS_LABELS_OFFSET,
        expected: &SOURCE_STATUS_LABELS,
        displayed_text: "STR: / SKI: / WLV: / AGI: / DEF: / MOV: / H.P: / EXP:",
        consumer: "status labels",
        scope: ReferenceScope::PreservedOriginal,
        evidence: "confirmed table bytes and runtime display",
    },
];

#[derive(Debug)]
pub struct FontSupplySummary {
    pub report_sha1: String,
    pub page_count: usize,
    pub protected_code_count: usize,
    pub unresolved_code_count: usize,
}

#[derive(Debug, Serialize)]
struct FontSupplyReport {
    schema_version: u8,
    scope: ReportScope,
    tile_format: TileFormat,
    summary: ReportSummary,
    mmc4_chr_bank_writers: Vec<Mmc4ChrWriterReport>,
    known_references: Vec<ReferenceReport>,
    pages: Vec<PageReport>,
    font_page: FontPageReport,
    unknowns: Vec<&'static str>,
}

#[derive(Debug, Serialize)]
struct ReportScope {
    source_sha1: &'static str,
    chr_sha1: &'static str,
    mapper: u16,
    font_page_index: usize,
}

#[derive(Debug, Serialize)]
struct TileFormat {
    width: u8,
    height: u8,
    bits_per_pixel: u8,
    bytes_per_tile: usize,
    chr_page_size: usize,
    tiles_per_page: usize,
}

#[derive(Debug, Serialize)]
struct ReportSummary {
    page_count: usize,
    tile_count: usize,
    nonblank_tile_count: usize,
    blank_pattern_count: usize,
    protected_font_code_count: usize,
    unresolved_font_code_count: usize,
    available_font_code_count: usize,
}

#[derive(Debug, Serialize)]
struct Mmc4ChrWriterReport {
    cpu_address: u16,
    cpu_address_hex: String,
    shadow_address: u8,
    shadow_address_hex: String,
    page_group_shadow_address: u8,
    page_group_shadow_address_hex: String,
    hardware_register: u16,
    hardware_register_hex: String,
    latch_domain: &'static str,
    routine_bytes_hex: String,
    direct_jsr_candidates: Vec<AbsoluteTransferCandidate>,
    direct_jmp_candidates: Vec<AbsoluteTransferCandidate>,
}

#[derive(Debug, Serialize)]
struct ReferenceReport {
    id: &'static str,
    file_offset: usize,
    file_offset_hex: String,
    byte_length: usize,
    bytes_hex: String,
    displayed_text: &'static str,
    consumer: &'static str,
    scope: ReferenceScope,
    evidence: &'static str,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
enum ReferenceScope {
    TranslatedJapanese,
    PreservedOriginal,
}

#[derive(Debug, Serialize)]
struct PageReport {
    page_index: usize,
    chr_offset: usize,
    chr_offset_hex: String,
    sha1: String,
    nonblank_tile_count: usize,
    blank_pattern_count: usize,
    low_plane_only_count: usize,
    high_plane_only_count: usize,
    dual_plane_count: usize,
    distinct_pattern_count: usize,
    blank_pattern_codes: Vec<String>,
}

#[derive(Debug, Serialize)]
struct FontPageReport {
    page_index: usize,
    chr_offset: usize,
    chr_offset_hex: String,
    slots: Vec<SlotReport>,
}

#[derive(Debug, Serialize)]
struct SlotReport {
    code: u8,
    code_hex: String,
    chr_offset: usize,
    chr_offset_hex: String,
    tile_sha1: String,
    plane_usage: PlaneUsage,
    nonzero_pixel_count: u32,
    declared_glyph: Option<String>,
    reference_occurrences: Vec<ReferenceOccurrence>,
    matching_codes: Vec<String>,
    code_assignment: Decision,
    code_assignment_reasons: Vec<&'static str>,
    tile_reuse: Decision,
    tile_reuse_reasons: Vec<&'static str>,
}

#[derive(Debug, Serialize)]
struct ReferenceOccurrence {
    reference_id: &'static str,
    count: usize,
    scope: ReferenceScope,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
enum PlaneUsage {
    Blank,
    LowOnly,
    HighOnly,
    Dual,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
enum Decision {
    Protected,
    Unresolved,
}

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
    let mmc4_chr_bank_writers = describe_mmc4_chr_writers(rom.prg())?;
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

    Ok(FontSupplyReport {
        schema_version: 3,
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
        mmc4_chr_bank_writers,
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
        unknowns: vec![
            "No font slot is classified as available until every consumer and runtime state is excluded.",
            "References list only confirmed tables; it is not the complete text or tile reference population.",
            "Direct JSR and JMP candidates are byte-pattern matches; instruction boundaries and render-path semantics remain unconfirmed.",
            "Active Hangul slot capacity remains unknown until every target render path is measured.",
        ],
    })
}

fn describe_mmc4_chr_writers(prg: &[u8]) -> Result<Vec<Mmc4ChrWriterReport>> {
    ensure!(
        prg.len() == PRG_SIZE,
        "unexpected PRG size for MMC4 writer inventory"
    );

    MMC4_CHR_WRITERS
        .iter()
        .map(|writer| {
            let prg_offset = fixed_bank_prg_offset(writer.cpu_address)?;
            let end = prg_offset + writer.expected.len();
            ensure!(
                prg[prg_offset..end] == writer.expected,
                "MMC4 CHR writer at ${:04X} changed",
                writer.cpu_address
            );

            Ok(Mmc4ChrWriterReport {
                cpu_address: writer.cpu_address,
                cpu_address_hex: format!("0x{:04X}", writer.cpu_address),
                shadow_address: writer.shadow_address,
                shadow_address_hex: format!("0x{:02X}", writer.shadow_address),
                page_group_shadow_address: 0x52,
                page_group_shadow_address_hex: "0x52".to_owned(),
                hardware_register: writer.hardware_register,
                hardware_register_hex: format!("0x{:04X}", writer.hardware_register),
                latch_domain: writer.latch_domain,
                routine_bytes_hex: hex_bytes(&writer.expected),
                direct_jsr_candidates: find_absolute_transfer_candidates(
                    prg,
                    writer.cpu_address,
                    0x20,
                ),
                direct_jmp_candidates: find_absolute_transfer_candidates(
                    prg,
                    writer.cpu_address,
                    0x4C,
                ),
            })
        })
        .collect()
}

fn fixed_bank_prg_offset(cpu_address: u16) -> Result<usize> {
    ensure!(
        cpu_address >= 0xC000,
        "fixed-bank CPU address must be at or above $C000"
    );
    Ok(PRG_SIZE - PRG_BANK_SIZE + usize::from(cpu_address - 0xC000))
}

fn validate_known_references(source: &[u8]) -> Result<()> {
    for reference in &KNOWN_REFERENCES {
        let end = reference
            .file_offset
            .checked_add(reference.expected.len())
            .context("known reference range overflow")?;
        ensure!(
            end <= source.len(),
            "known reference {} is outside the source image",
            reference.id
        );
        ensure!(
            source[reference.file_offset..end] == *reference.expected,
            "known reference {} bytes changed at {:#X}",
            reference.id,
            reference.file_offset
        );
    }
    Ok(())
}

fn summarize_page(page_index: usize, page: &[u8]) -> PageReport {
    let tiles: Vec<&[u8]> = page.chunks_exact(TILE_SIZE).collect();
    let mut blank_pattern_codes = Vec::new();
    let mut low_plane_only_count = 0;
    let mut high_plane_only_count = 0;
    let mut dual_plane_count = 0;
    let mut patterns = BTreeSet::new();

    for (code, tile) in tiles.iter().enumerate() {
        patterns.insert((*tile).to_vec());
        match plane_usage(tile) {
            PlaneUsage::Blank => blank_pattern_codes.push(format!("{code:02X}")),
            PlaneUsage::LowOnly => low_plane_only_count += 1,
            PlaneUsage::HighOnly => high_plane_only_count += 1,
            PlaneUsage::Dual => dual_plane_count += 1,
        }
    }

    PageReport {
        page_index,
        chr_offset: page_index * CHR_PAGE_SIZE,
        chr_offset_hex: format!("0x{:05X}", page_index * CHR_PAGE_SIZE),
        sha1: sha1_hex(page),
        nonblank_tile_count: TILES_PER_PAGE - blank_pattern_codes.len(),
        blank_pattern_count: blank_pattern_codes.len(),
        low_plane_only_count,
        high_plane_only_count,
        dual_plane_count,
        distinct_pattern_count: patterns.len(),
        blank_pattern_codes,
    }
}

fn describe_font_page(page: &[u8]) -> Vec<SlotReport> {
    let tiles: Vec<&[u8]> = page.chunks_exact(TILE_SIZE).collect();
    tiles
        .iter()
        .enumerate()
        .map(|(code, tile)| {
            let code = code as u8;
            let reference_occurrences: Vec<ReferenceOccurrence> = KNOWN_REFERENCES
                .iter()
                .filter_map(|reference| {
                    let count = reference
                        .expected
                        .iter()
                        .filter(|value| **value == code)
                        .count();
                    (count > 0).then_some(ReferenceOccurrence {
                        reference_id: reference.id,
                        count,
                        scope: reference.scope,
                    })
                })
                .collect();
            let preserved_reference = reference_occurrences
                .iter()
                .any(|occurrence| occurrence.scope == ReferenceScope::PreservedOriginal);
            let is_preserved_glyph = is_declared_preserved_glyph(code);
            let is_control = [ENTRY_SEPARATOR, TABLE_TERMINATOR].contains(&code);
            let is_latch = MMC4_LATCH_CODES.contains(&code);

            let mut code_assignment_reasons = Vec::new();
            if is_preserved_glyph {
                code_assignment_reasons
                    .push("declared original digit, Latin, or attached punctuation");
            }
            if preserved_reference {
                code_assignment_reasons.push("confirmed preserved-original table reference");
            }
            if is_control {
                code_assignment_reasons.push("confirmed text control code");
            }
            if is_latch {
                code_assignment_reasons.push("MMC4 tile-fetch latch code");
            }
            let code_assignment = if code_assignment_reasons.is_empty() {
                code_assignment_reasons.push("consumer population is incomplete");
                Decision::Unresolved
            } else {
                Decision::Protected
            };

            let mut tile_reuse_reasons = Vec::new();
            if is_preserved_glyph || preserved_reference {
                tile_reuse_reasons.push("preserved original display depends on this tile");
            }
            if is_latch {
                tile_reuse_reasons.push("MMC4 latch behavior reserves this tile code");
            }
            let tile_reuse = if tile_reuse_reasons.is_empty() {
                if plane_usage(tile) == PlaneUsage::Blank {
                    tile_reuse_reasons.push("blank pattern is not free-space proof");
                } else {
                    tile_reuse_reasons.push("all tile consumers have not been excluded");
                }
                Decision::Unresolved
            } else {
                Decision::Protected
            };

            let matching_codes = tiles
                .iter()
                .enumerate()
                .filter(|(other_code, other_tile)| {
                    *other_code != code as usize && *other_tile == tile
                })
                .map(|(other_code, _)| format!("{other_code:02X}"))
                .collect();
            let chr_offset = code as usize * TILE_SIZE;

            SlotReport {
                code,
                code_hex: format!("{code:02X}"),
                chr_offset,
                chr_offset_hex: format!("0x{chr_offset:05X}"),
                tile_sha1: sha1_hex(tile),
                plane_usage: plane_usage(tile),
                nonzero_pixel_count: nonzero_pixel_count(tile),
                declared_glyph: declared_glyph(code),
                reference_occurrences,
                matching_codes,
                code_assignment,
                code_assignment_reasons,
                tile_reuse,
                tile_reuse_reasons,
            }
        })
        .collect()
}

fn is_declared_preserved_glyph(code: u8) -> bool {
    (0x60..=0x83).contains(&code) || [0x8D, 0x9B].contains(&code)
}

fn declared_glyph(code: u8) -> Option<String> {
    match code {
        0x60..=0x69 => Some(char::from(b'0' + code - 0x60).to_string()),
        0x6A..=0x83 => Some(char::from(b'A' + code - 0x6A).to_string()),
        0x8D => Some(":".to_owned()),
        0x9B => Some(".".to_owned()),
        0x0F => Some("゛".to_owned()),
        0x30 => Some("ア".to_owned()),
        0x31 => Some("イ".to_owned()),
        0x32 => Some("ウ".to_owned()),
        0x33 => Some("エ".to_owned()),
        0x3A => Some("サ".to_owned()),
        0x3B => Some("シ".to_owned()),
        0x3F => Some("ー".to_owned()),
        0x40 => Some("タ".to_owned()),
        0x44 => Some("ト".to_owned()),
        0x46 => Some("ニ".to_owned()),
        0x50 => Some("マ".to_owned()),
        0x53 => Some("メ".to_owned()),
        0x5F => Some("ン".to_owned()),
        0x8B => Some("ョ".to_owned()),
        _ => None,
    }
}

fn plane_usage(tile: &[u8]) -> PlaneUsage {
    let low = tile[..8].iter().any(|byte| *byte != 0);
    let high = tile[8..].iter().any(|byte| *byte != 0);
    match (low, high) {
        (false, false) => PlaneUsage::Blank,
        (true, false) => PlaneUsage::LowOnly,
        (false, true) => PlaneUsage::HighOnly,
        (true, true) => PlaneUsage::Dual,
    }
}

fn nonzero_pixel_count(tile: &[u8]) -> u32 {
    tile[..8]
        .iter()
        .zip(&tile[8..])
        .map(|(low, high)| (low | high).count_ones())
        .sum()
}

fn render_font_page_sheet(page: &[u8], slots: &[SlotReport], scale: u32) -> Result<Vec<u8>> {
    ensure!(page.len() == CHR_PAGE_SIZE, "font page must be 4 KiB");
    ensure!(
        slots.len() == TILES_PER_PAGE,
        "font page slot count mismatch"
    );

    let label_scale = if scale >= 3 { 2 } else { 1 };
    let tile_pixels = 8 * scale;
    let cell_width = tile_pixels + 4;
    let cell_height = tile_pixels + 5 * label_scale + 7;
    let width = 16 * cell_width;
    let height = 16 * cell_height;
    let mut pixels = vec![0x12_u8; (width * height * 3) as usize];

    for (index, slot) in slots.iter().enumerate() {
        let left = (index as u32 % 16) * cell_width;
        let top = (index as u32 / 16) * cell_height;
        let border = match (slot.tile_reuse, slot.code_assignment, slot.plane_usage) {
            (Decision::Protected, _, _) => [0xFF, 0x5A, 0x5F],
            (_, Decision::Protected, _) => [0xFF, 0xA5, 0x30],
            (_, _, PlaneUsage::Blank) => [0x54, 0xA8, 0xFF],
            _ => [0x4E, 0x58, 0x69],
        };
        draw_border(
            &mut pixels,
            width,
            left,
            top,
            cell_width,
            cell_height,
            border,
        );
        let tile = &page[index * TILE_SIZE..(index + 1) * TILE_SIZE];
        draw_tile(&mut pixels, width, tile, left + 2, top + 2, scale);
        draw_hex_label(
            &mut pixels,
            width,
            slot.code,
            left + (cell_width - 7 * label_scale) / 2,
            top + tile_pixels + 4,
            label_scale,
        );
    }

    encode_rgb_png(width, height, &pixels)
}

fn draw_tile(pixels: &mut [u8], width: u32, tile: &[u8], left: u32, top: u32, scale: u32) {
    const PALETTE: [[u8; 3]; 4] = [
        [0x08, 0x0C, 0x12],
        [0x6A, 0x7C, 0x92],
        [0xB9, 0xC7, 0xD8],
        [0xF4, 0xF7, 0xFB],
    ];
    for row in 0..8 {
        for column in 0..8 {
            let shift = 7 - column;
            let value = ((tile[row] >> shift) & 1) | (((tile[row + 8] >> shift) & 1) << 1);
            for y in 0..scale {
                for x in 0..scale {
                    set_pixel(
                        pixels,
                        width,
                        left + column as u32 * scale + x,
                        top + row as u32 * scale + y,
                        PALETTE[value as usize],
                    );
                }
            }
        }
    }
}

fn draw_hex_label(pixels: &mut [u8], width: u32, code: u8, left: u32, top: u32, scale: u32) {
    for (digit_index, digit) in [code >> 4, code & 0x0F].iter().enumerate() {
        for (row, bits) in HEX_GLYPHS[*digit as usize].iter().enumerate() {
            for column in 0..3 {
                if bits & (1 << (2 - column)) == 0 {
                    continue;
                }
                for y in 0..scale {
                    for x in 0..scale {
                        set_pixel(
                            pixels,
                            width,
                            left + (digit_index as u32 * 4 + column) * scale + x,
                            top + row as u32 * scale + y,
                            [0xE8, 0xEC, 0xF2],
                        );
                    }
                }
            }
        }
    }
}

fn draw_border(
    pixels: &mut [u8],
    width: u32,
    left: u32,
    top: u32,
    box_width: u32,
    box_height: u32,
    color: [u8; 3],
) {
    for x in left..left + box_width {
        set_pixel(pixels, width, x, top, color);
        set_pixel(pixels, width, x, top + box_height - 1, color);
    }
    for y in top..top + box_height {
        set_pixel(pixels, width, left, y, color);
        set_pixel(pixels, width, left + box_width - 1, y, color);
    }
}

fn set_pixel(pixels: &mut [u8], width: u32, x: u32, y: u32, color: [u8; 3]) {
    let offset = ((y * width + x) * 3) as usize;
    pixels[offset..offset + 3].copy_from_slice(&color);
}

fn encode_rgb_png(width: u32, height: u32, pixels: &[u8]) -> Result<Vec<u8>> {
    let mut encoded = Cursor::new(Vec::new());
    {
        let mut encoder = png::Encoder::new(&mut encoded, width, height);
        encoder.set_color(png::ColorType::Rgb);
        encoder.set_depth(png::BitDepth::Eight);
        let mut writer = encoder.write_header().context("write font sheet header")?;
        writer
            .write_image_data(pixels)
            .context("write font sheet pixels")?;
    }
    Ok(encoded.into_inner())
}

fn write_file(path: &Path, data: &[u8]) -> Result<()> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)
            .with_context(|| format!("create output directory {}", parent.display()))?;
    }
    fs::write(path, data).with_context(|| format!("write {}", path.display()))
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
    use crate::rom::HEADER_SIZE;

    fn source_with_known_references() -> Vec<u8> {
        let mut source = vec![0_u8; STATUS_LABELS_OFFSET + SOURCE_STATUS_LABELS.len()];
        source[OPTIONS_TABLE_OFFSET..OPTIONS_TABLE_OFFSET + SOURCE_OPTIONS_TABLE.len()]
            .copy_from_slice(&SOURCE_OPTIONS_TABLE);
        source[STATUS_LABELS_OFFSET..STATUS_LABELS_OFFSET + SOURCE_STATUS_LABELS.len()]
            .copy_from_slice(&SOURCE_STATUS_LABELS);
        source
    }

    #[test]
    fn protects_declared_latin_and_confirmed_english_punctuation() {
        let slots = describe_font_page(&vec![0_u8; CHR_PAGE_SIZE]);

        for slot in &slots[0x60..=0x83] {
            assert_eq!(slot.code_assignment, Decision::Protected);
            assert_eq!(slot.tile_reuse, Decision::Protected);
        }
        for code in [0x8D, 0x9B] {
            assert_eq!(slots[code].code_assignment, Decision::Protected);
            assert_eq!(slots[code].tile_reuse, Decision::Protected);
        }
    }

    #[test]
    fn leaves_a_blank_pattern_unresolved_instead_of_calling_it_available() {
        let slots = describe_font_page(&vec![0_u8; CHR_PAGE_SIZE]);
        let slot = &slots[0x95];

        assert_eq!(slot.plane_usage, PlaneUsage::Blank);
        assert_eq!(slot.code_assignment, Decision::Unresolved);
        assert_eq!(slot.tile_reuse, Decision::Unresolved);
        assert!(
            slot.tile_reuse_reasons
                .contains(&"blank pattern is not free-space proof")
        );
    }

    #[test]
    fn rejects_a_known_reference_when_its_source_bytes_change() {
        let mut source = source_with_known_references();
        source[STATUS_LABELS_OFFSET] ^= 0x01;

        let error = validate_known_references(&source).unwrap_err().to_string();
        assert!(error.contains("status-label-table bytes changed"));
    }

    #[test]
    fn sheet_contains_all_codes_at_the_requested_scale() {
        let page = vec![0_u8; CHR_PAGE_SIZE];
        let slots = describe_font_page(&page);
        let png = render_font_page_sheet(&page, &slots, 2).unwrap();

        assert_eq!(&png[..8], b"\x89PNG\r\n\x1a\n");
        assert_eq!(u32::from_be_bytes(png[16..20].try_into().unwrap()), 320);
        assert_eq!(u32::from_be_bytes(png[20..24].try_into().unwrap()), 448);
    }

    #[test]
    fn page_summary_separates_storage_planes_and_duplicate_patterns() {
        let mut page = vec![0_u8; CHR_PAGE_SIZE];
        page[TILE_SIZE] = 0x80;
        page[TILE_SIZE * 2 + 8] = 0x80;
        page[TILE_SIZE * 3] = 0x80;
        page[TILE_SIZE * 3 + 8] = 0x80;
        let summary = summarize_page(2, &page);

        assert_eq!(summary.page_index, 2);
        assert_eq!(summary.blank_pattern_count, 253);
        assert_eq!(summary.low_plane_only_count, 1);
        assert_eq!(summary.high_plane_only_count, 1);
        assert_eq!(summary.dual_plane_count, 1);
        assert_eq!(summary.distinct_pattern_count, 4);
    }

    #[test]
    fn direct_jsr_candidate_inventory_preserves_bank_coordinates() {
        let mut prg = vec![0_u8; PRG_SIZE];
        prg[0x0123..0x0126].copy_from_slice(&[0x20, 0xBE, 0xC9]);
        let fixed_call = PRG_SIZE - PRG_BANK_SIZE + 0x0234;
        prg[fixed_call..fixed_call + 3].copy_from_slice(&[0x20, 0xBE, 0xC9]);

        let candidates = find_absolute_transfer_candidates(&prg, 0xC9BE, 0x20);

        assert_eq!(candidates.len(), 2);
        assert_eq!(candidates[0].prg_bank, 0);
        assert_eq!(candidates[0].file_offset, HEADER_SIZE + 0x0123);
        assert_eq!(candidates[0].cpu_address, 0x8123);
        assert_eq!(candidates[1].prg_bank, 15);
        assert_eq!(candidates[1].file_offset, HEADER_SIZE + fixed_call);
        assert_eq!(candidates[1].cpu_address, 0xC234);
    }

    #[test]
    fn absolute_jump_candidates_are_separate_from_jsr_candidates() {
        let mut prg = vec![0_u8; PRG_SIZE];
        prg[0x0123..0x0126].copy_from_slice(&[0x20, 0xC6, 0xC9]);
        prg[0x0456..0x0459].copy_from_slice(&[0x4C, 0xC6, 0xC9]);

        let jsr = find_absolute_transfer_candidates(&prg, 0xC9C6, 0x20);
        let jmp = find_absolute_transfer_candidates(&prg, 0xC9C6, 0x4C);

        assert_eq!(jsr.len(), 1);
        assert_eq!(jsr[0].cpu_address, 0x8123);
        assert_eq!(jmp.len(), 1);
        assert_eq!(jmp[0].cpu_address, 0x8456);
    }

    #[test]
    fn writer_inventory_rejects_a_changed_fixed_bank_routine() {
        let mut prg = vec![0_u8; PRG_SIZE];
        for writer in &MMC4_CHR_WRITERS {
            let offset = fixed_bank_prg_offset(writer.cpu_address).unwrap();
            prg[offset..offset + writer.expected.len()].copy_from_slice(&writer.expected);
        }
        describe_mmc4_chr_writers(&prg).unwrap();

        let offset = fixed_bank_prg_offset(0xC9BE).unwrap();
        prg[offset] ^= 0x01;
        let error = describe_mmc4_chr_writers(&prg).unwrap_err().to_string();

        assert!(error.contains("MMC4 CHR writer at $C9BE changed"));
    }
}
