use std::{collections::BTreeSet, fs, path::Path};

use anyhow::{Context, Result, ensure};
use serde::Serialize;

use crate::{
    font::{load_dalmoori, rasterize_glyph},
    font_slots::{
        ACTIVE_HANGUL_SLOT_COUNT, FONT_PAGE_SIZE, FONT_TILE_SIZE, active_hangul_codes,
        protected_original_codes, reserved_font_codes,
    },
    mapper165::{FIRST_EXTENSION_CHR_PAGE, MAXIMUM_CHR_PAGE_COUNT, encode_chr_page_register},
    rom::Rom,
    sha1_hex,
};

const PROOF_GLYPHS_PER_PAGE: usize = 106;
const PROOF_PAGE_IDS: [&str; 2] = ["page_a", "page_b"];
const FIRST_HANGUL_SYLLABLE: u32 = 0xAC00;

#[derive(Debug, Serialize)]
struct HangulPageProofReport {
    schema: u32,
    source_sha1: &'static str,
    source_font_page_sha1: String,
    storage_strategy: &'static str,
    protected_original_code_count: usize,
    reserved_code_count: usize,
    active_hangul_slot_count: usize,
    maximum_extension_page_count: usize,
    maximum_page_local_glyph_assignments: usize,
    page_union_glyph_count: usize,
    page_union_exceeds_active_slots: bool,
    page_pack_sha1: String,
    pages: Vec<HangulPageReport>,
    runtime_bound: bool,
    release_eligible: bool,
}

#[derive(Debug, Serialize)]
struct HangulPageReport {
    id: &'static str,
    physical_4k_page: u8,
    mapper_register_value: u8,
    glyph_count: usize,
    page_sha1: String,
    protected_original_bytes_preserved: bool,
    assignments: Vec<GlyphAssignmentReport>,
}

#[derive(Debug, Serialize)]
struct GlyphAssignmentReport {
    code: u8,
    code_hex: String,
    character: char,
}

#[derive(Debug)]
struct PlannedPage {
    report: HangulPageReport,
    bytes: Vec<u8>,
}

pub(crate) struct HangulPageProofSummary {
    pub(crate) report_sha1: String,
    pub(crate) page_pack_sha1: String,
    pub(crate) active_hangul_slot_count: usize,
    pub(crate) page_union_glyph_count: usize,
    pub(crate) maximum_extension_page_count: usize,
}

pub(crate) fn plan_hangul_page_proof(
    source_path: &Path,
    page_pack_path: &Path,
    report_path: &Path,
) -> Result<HangulPageProofSummary> {
    let source_rom = Rom::from_path(source_path)?;
    source_rom.verify_supported_japanese()?;
    let source_font_page = &source_rom.chr()[..FONT_PAGE_SIZE];
    let requested_glyphs = proof_glyphs()?;
    let font = load_dalmoori()?;

    let pages = PROOF_PAGE_IDS
        .iter()
        .enumerate()
        .map(|(index, id)| {
            let start = index * PROOF_GLYPHS_PER_PAGE;
            let end = start + PROOF_GLYPHS_PER_PAGE;
            plan_page(
                id,
                FIRST_EXTENSION_CHR_PAGE + u8::try_from(index)?,
                source_font_page,
                &requested_glyphs[start..end],
                &font,
            )
        })
        .collect::<Result<Vec<_>>>()?;

    let page_union_glyph_count = pages
        .iter()
        .flat_map(|page| {
            page.report
                .assignments
                .iter()
                .map(|assignment| assignment.character)
        })
        .collect::<BTreeSet<_>>()
        .len();
    ensure!(
        pages
            .iter()
            .all(|page| page.report.glyph_count <= ACTIVE_HANGUL_SLOT_COUNT),
        "a proof page exceeds the active Hangul slot count"
    );
    ensure!(
        page_union_glyph_count > ACTIVE_HANGUL_SLOT_COUNT,
        "proof page union must exceed one active Hangul page"
    );

    let page_pack = pages
        .iter()
        .flat_map(|page| page.bytes.iter().copied())
        .collect::<Vec<_>>();
    ensure!(
        page_pack.len() == PROOF_PAGE_IDS.len() * FONT_PAGE_SIZE,
        "Hangul proof page pack size mismatch"
    );
    let page_pack_sha1 = sha1_hex(&page_pack);
    let maximum_extension_page_count =
        usize::from(MAXIMUM_CHR_PAGE_COUNT - FIRST_EXTENSION_CHR_PAGE);
    let report = HangulPageProofReport {
        schema: 1,
        source_sha1: crate::rom::EXPECTED_SOURCE_SHA1,
        source_font_page_sha1: sha1_hex(source_font_page),
        storage_strategy: "expanded_chr_rom_pages",
        protected_original_code_count: protected_original_codes().len(),
        reserved_code_count: reserved_font_codes().len(),
        active_hangul_slot_count: ACTIVE_HANGUL_SLOT_COUNT,
        maximum_extension_page_count,
        maximum_page_local_glyph_assignments: maximum_extension_page_count
            * ACTIVE_HANGUL_SLOT_COUNT,
        page_union_glyph_count,
        page_union_exceeds_active_slots: true,
        page_pack_sha1: page_pack_sha1.clone(),
        pages: pages.into_iter().map(|page| page.report).collect(),
        runtime_bound: false,
        release_eligible: false,
    };
    let report_bytes =
        serde_json::to_vec_pretty(&report).context("serialize Hangul page proof report")?;
    let report_sha1 = sha1_hex(&report_bytes);

    write_file(page_pack_path, &page_pack)?;
    write_file(report_path, &report_bytes)?;
    Ok(HangulPageProofSummary {
        report_sha1,
        page_pack_sha1,
        active_hangul_slot_count: ACTIVE_HANGUL_SLOT_COUNT,
        page_union_glyph_count,
        maximum_extension_page_count,
    })
}

fn plan_page(
    id: &'static str,
    physical_page: u8,
    source_font_page: &[u8],
    glyphs: &[char],
    font: &fontdue::Font,
) -> Result<PlannedPage> {
    ensure!(
        source_font_page.len() == FONT_PAGE_SIZE,
        "source font page must be exactly 4 KiB"
    );
    ensure!(
        glyphs.len() <= ACTIVE_HANGUL_SLOT_COUNT,
        "Hangul page {id} needs {} glyphs but only {} slots are active",
        glyphs.len(),
        ACTIVE_HANGUL_SLOT_COUNT
    );
    ensure!(
        glyphs.iter().copied().collect::<BTreeSet<_>>().len() == glyphs.len(),
        "Hangul page {id} contains duplicate glyphs"
    );

    let mut page = source_font_page.to_vec();
    let assignments = active_hangul_codes()
        .into_iter()
        .zip(glyphs.iter().copied())
        .map(|(code, character)| {
            let tile = rasterize_glyph(font, character)?;
            let start = usize::from(code) * FONT_TILE_SIZE;
            page[start..start + FONT_TILE_SIZE].copy_from_slice(&tile);
            Ok(GlyphAssignmentReport {
                code,
                code_hex: format!("{code:02X}"),
                character,
            })
        })
        .collect::<Result<Vec<_>>>()?;

    verify_reserved_bytes(source_font_page, &page)?;
    let mapper_register_value = encode_chr_page_register(physical_page)?;
    Ok(PlannedPage {
        report: HangulPageReport {
            id,
            physical_4k_page: physical_page,
            mapper_register_value,
            glyph_count: assignments.len(),
            page_sha1: sha1_hex(&page),
            protected_original_bytes_preserved: true,
            assignments,
        },
        bytes: page,
    })
}

fn proof_glyphs() -> Result<Vec<char>> {
    (0..PROOF_GLYPHS_PER_PAGE * PROOF_PAGE_IDS.len())
        .map(|index| {
            char::from_u32(FIRST_HANGUL_SYLLABLE + u32::try_from(index)?)
                .ok_or_else(|| anyhow::anyhow!("invalid Hangul proof scalar at index {index}"))
        })
        .collect()
}

fn verify_reserved_bytes(source: &[u8], planned: &[u8]) -> Result<()> {
    let mismatches = reserved_font_codes()
        .into_iter()
        .filter(|code| {
            let start = usize::from(*code) * FONT_TILE_SIZE;
            source[start..start + FONT_TILE_SIZE] != planned[start..start + FONT_TILE_SIZE]
        })
        .collect::<Vec<_>>();
    ensure!(
        mismatches.is_empty(),
        "reserved font codes changed: {mismatches:02X?}"
    );
    Ok(())
}

fn write_file(path: &Path, data: &[u8]) -> Result<()> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)
            .with_context(|| format!("create output directory {}", parent.display()))?;
    }
    fs::write(path, data).with_context(|| format!("write {}", path.display()))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn source_font_page() -> Vec<u8> {
        (0..FONT_PAGE_SIZE).map(|index| index as u8).collect()
    }

    #[test]
    fn two_page_proof_exceeds_one_page_without_changing_reserved_tiles() {
        let source = source_font_page();
        let glyphs = proof_glyphs().unwrap();
        let font = load_dalmoori().unwrap();
        let page_a = plan_page(
            "page_a",
            FIRST_EXTENSION_CHR_PAGE,
            &source,
            &glyphs[..PROOF_GLYPHS_PER_PAGE],
            &font,
        )
        .unwrap();
        let page_b = plan_page(
            "page_b",
            FIRST_EXTENSION_CHR_PAGE + 1,
            &source,
            &glyphs[PROOF_GLYPHS_PER_PAGE..],
            &font,
        )
        .unwrap();

        let union = page_a
            .report
            .assignments
            .iter()
            .chain(&page_b.report.assignments)
            .map(|assignment| assignment.character)
            .collect::<BTreeSet<_>>();
        assert_eq!(page_a.report.glyph_count, PROOF_GLYPHS_PER_PAGE);
        assert_eq!(page_b.report.glyph_count, PROOF_GLYPHS_PER_PAGE);
        assert_eq!(union.len(), 212);
        assert!(union.len() > ACTIVE_HANGUL_SLOT_COUNT);
        verify_reserved_bytes(&source, &page_a.bytes).unwrap();
        verify_reserved_bytes(&source, &page_b.bytes).unwrap();
    }

    #[test]
    fn rejects_a_page_that_exceeds_the_active_slot_count() {
        let source = source_font_page();
        let glyphs = (0..=ACTIVE_HANGUL_SLOT_COUNT)
            .map(|index| char::from_u32(FIRST_HANGUL_SYLLABLE + index as u32).unwrap())
            .collect::<Vec<_>>();

        let error = plan_page(
            "overflow",
            FIRST_EXTENSION_CHR_PAGE,
            &source,
            &glyphs,
            &load_dalmoori().unwrap(),
        )
        .unwrap_err()
        .to_string();
        assert!(error.contains("only 211 slots are active"));
    }

    #[test]
    fn rejects_a_duplicate_glyph_assignment() {
        let source = source_font_page();
        let error = plan_page(
            "duplicate",
            FIRST_EXTENSION_CHR_PAGE,
            &source,
            &['한', '한'],
            &load_dalmoori().unwrap(),
        )
        .unwrap_err()
        .to_string();
        assert!(error.contains("duplicate glyphs"));
    }
}
