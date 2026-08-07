use std::{collections::BTreeSet, fs, path::Path};

use anyhow::{Context, Result, ensure};
use serde::Serialize;

use crate::{
    font::{load_dalmoori, rasterize_glyph},
    font_slots::{
        ACTIVE_HANGUL_SLOT_COUNT, FONT_PAGE_SIZE, FONT_TILE_SIZE, active_hangul_codes,
        protected_original_codes, reserved_font_codes,
    },
    localization::OptionsLocalization,
    mapper165::{FIRST_EXTENSION_CHR_PAGE, MAXIMUM_CHR_PAGE_COUNT, encode_chr_page_register},
    rom::Rom,
    sha1_hex,
};

const PAGE_LOCAL_PROOF_GLYPH_COUNT: usize = 100;
const PROOF_PAGE_IDS: [&str; 2] = ["page_a", "page_b"];
const FIRST_HANGUL_SYLLABLE: u32 = 0xAC00;
const SHARED_SCREEN_ROLE: &str = "options_labels";

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
    shared_screen_role: &'static str,
    shared_screen_glyph_count: usize,
    page_local_proof_glyph_count: usize,
    page_union_glyph_count: usize,
    page_union_exceeds_active_slots: bool,
    shared_screen_assignments_identical: bool,
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
    shared_screen_glyph_count: usize,
    page_local_proof_glyph_count: usize,
    page_sha1: String,
    protected_original_bytes_preserved: bool,
    assignments: Vec<GlyphAssignmentReport>,
}

#[derive(Debug, PartialEq, Eq, Serialize)]
struct GlyphAssignmentReport {
    code: u8,
    code_hex: String,
    character: char,
    scope: &'static str,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct PlannedGlyph {
    code: u8,
    character: char,
    scope: &'static str,
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
    localization_path: &Path,
    page_pack_path: &Path,
    report_path: &Path,
) -> Result<HangulPageProofSummary> {
    let source_rom = Rom::from_path(source_path)?;
    source_rom.verify_supported_japanese()?;
    let localization = OptionsLocalization::from_path(localization_path)?;
    let pages = plan_hangul_pages(&source_rom, &localization)?;

    let shared_screen_glyph_count = pages[0].report.shared_screen_glyph_count;
    let shared_screen_assignments_identical = pages[0].report.assignments
        [..shared_screen_glyph_count]
        == pages[1].report.assignments[..shared_screen_glyph_count];
    ensure!(
        shared_screen_assignments_identical,
        "screen-shared assignments differ between Hangul proof pages"
    );

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

    let page_pack = page_pack_bytes(&pages)?;
    let page_pack_sha1 = sha1_hex(&page_pack);
    let maximum_extension_page_count =
        usize::from(MAXIMUM_CHR_PAGE_COUNT - FIRST_EXTENSION_CHR_PAGE);
    let report = HangulPageProofReport {
        schema: 1,
        source_sha1: crate::rom::EXPECTED_SOURCE_SHA1,
        source_font_page_sha1: sha1_hex(&source_rom.chr()[..FONT_PAGE_SIZE]),
        storage_strategy: "expanded_chr_rom_pages",
        protected_original_code_count: protected_original_codes().len(),
        reserved_code_count: reserved_font_codes().len(),
        active_hangul_slot_count: ACTIVE_HANGUL_SLOT_COUNT,
        maximum_extension_page_count,
        maximum_page_local_glyph_assignments: maximum_extension_page_count
            * ACTIVE_HANGUL_SLOT_COUNT,
        shared_screen_role: SHARED_SCREEN_ROLE,
        shared_screen_glyph_count,
        page_local_proof_glyph_count: PAGE_LOCAL_PROOF_GLYPH_COUNT,
        page_union_glyph_count,
        page_union_exceeds_active_slots: true,
        shared_screen_assignments_identical,
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

pub(crate) fn assemble_hangul_page_pack(
    source_rom: &Rom,
    localization: &OptionsLocalization,
) -> Result<Vec<u8>> {
    page_pack_bytes(&plan_hangul_pages(source_rom, localization)?)
}

fn plan_hangul_pages(
    source_rom: &Rom,
    localization: &OptionsLocalization,
) -> Result<Vec<PlannedPage>> {
    ensure!(
        source_rom.chr().len() >= FONT_PAGE_SIZE,
        "source ROM has no complete font page"
    );
    let source_font_page = &source_rom.chr()[..FONT_PAGE_SIZE];
    let requested_pages = proof_page_glyphs(localization)?;
    let font = load_dalmoori()?;

    PROOF_PAGE_IDS
        .iter()
        .enumerate()
        .map(|(index, id)| {
            plan_page(
                id,
                FIRST_EXTENSION_CHR_PAGE + u8::try_from(index)?,
                source_font_page,
                &requested_pages[index],
                &font,
            )
        })
        .collect()
}

fn page_pack_bytes(pages: &[PlannedPage]) -> Result<Vec<u8>> {
    let page_pack = pages
        .iter()
        .flat_map(|page| page.bytes.iter().copied())
        .collect::<Vec<_>>();
    ensure!(
        page_pack.len() == PROOF_PAGE_IDS.len() * FONT_PAGE_SIZE,
        "Hangul proof page pack size mismatch"
    );
    Ok(page_pack)
}

fn plan_page(
    id: &'static str,
    physical_page: u8,
    source_font_page: &[u8],
    glyphs: &[PlannedGlyph],
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
        glyphs
            .iter()
            .map(|glyph| glyph.character)
            .collect::<BTreeSet<_>>()
            .len()
            == glyphs.len(),
        "Hangul page {id} contains duplicate glyphs"
    );
    ensure!(
        glyphs
            .iter()
            .map(|glyph| glyph.code)
            .collect::<BTreeSet<_>>()
            .len()
            == glyphs.len(),
        "Hangul page {id} contains duplicate codes"
    );
    let active_codes = active_hangul_codes().into_iter().collect::<BTreeSet<_>>();
    let invalid_codes = glyphs
        .iter()
        .map(|glyph| glyph.code)
        .filter(|code| !active_codes.contains(code))
        .collect::<Vec<_>>();
    ensure!(
        invalid_codes.is_empty(),
        "Hangul page {id} assigns reserved codes: {invalid_codes:02X?}"
    );

    let mut page = source_font_page.to_vec();
    let assignments = glyphs
        .iter()
        .map(|glyph| {
            let tile = rasterize_glyph(font, glyph.character)?;
            let start = usize::from(glyph.code) * FONT_TILE_SIZE;
            page[start..start + FONT_TILE_SIZE].copy_from_slice(&tile);
            Ok(GlyphAssignmentReport {
                code: glyph.code,
                code_hex: format!("{:02X}", glyph.code),
                character: glyph.character,
                scope: glyph.scope,
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
            shared_screen_glyph_count: assignments
                .iter()
                .filter(|assignment| assignment.scope == "screen_shared")
                .count(),
            page_local_proof_glyph_count: assignments
                .iter()
                .filter(|assignment| assignment.scope == "page_local_proof")
                .count(),
            page_sha1: sha1_hex(&page),
            protected_original_bytes_preserved: true,
            assignments,
        },
        bytes: page,
    })
}

fn proof_page_glyphs(localization: &OptionsLocalization) -> Result<Vec<Vec<PlannedGlyph>>> {
    localization.validate()?;
    ensure!(
        localization.glyphs.len() == 12,
        "options screen proof must contain exactly 12 shared Hangul glyphs"
    );
    let shared = localization
        .glyphs
        .iter()
        .map(|glyph| PlannedGlyph {
            code: glyph.code,
            character: glyph.character,
            scope: "screen_shared",
        })
        .collect::<Vec<_>>();
    let shared_characters = shared
        .iter()
        .map(|glyph| glyph.character)
        .collect::<BTreeSet<_>>();
    ensure!(
        shared_characters.len() == shared.len(),
        "options screen proof contains duplicate shared Hangul glyphs"
    );

    let shared_codes = shared
        .iter()
        .map(|glyph| glyph.code)
        .collect::<BTreeSet<_>>();
    let page_local_codes = active_hangul_codes()
        .into_iter()
        .filter(|code| !shared_codes.contains(code))
        .take(PAGE_LOCAL_PROOF_GLYPH_COUNT)
        .collect::<Vec<_>>();
    ensure!(
        page_local_codes.len() == PAGE_LOCAL_PROOF_GLYPH_COUNT,
        "not enough page-local Hangul codes after shared screen assignments"
    );
    let proof_glyphs = proof_glyphs(&shared_characters)?;

    (0..PROOF_PAGE_IDS.len())
        .map(|page_index| {
            let start = page_index * PAGE_LOCAL_PROOF_GLYPH_COUNT;
            let end = start + PAGE_LOCAL_PROOF_GLYPH_COUNT;
            let mut page = shared.clone();
            page.extend(
                page_local_codes
                    .iter()
                    .copied()
                    .zip(proof_glyphs[start..end].iter().copied())
                    .map(|(code, character)| PlannedGlyph {
                        code,
                        character,
                        scope: "page_local_proof",
                    }),
            );
            Ok(page)
        })
        .collect()
}

fn proof_glyphs(excluded: &BTreeSet<char>) -> Result<Vec<char>> {
    let required_count = PAGE_LOCAL_PROOF_GLYPH_COUNT * PROOF_PAGE_IDS.len();
    let mut glyphs = Vec::with_capacity(required_count);
    let mut scalar = FIRST_HANGUL_SYLLABLE;
    while glyphs.len() < required_count {
        let character = char::from_u32(scalar)
            .ok_or_else(|| anyhow::anyhow!("invalid Hangul proof scalar {scalar:04X}"))?;
        if !excluded.contains(&character) {
            glyphs.push(character);
        }
        scalar += 1;
    }
    Ok(glyphs)
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

    fn options_localization() -> OptionsLocalization {
        serde_json::from_str(include_str!(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../../assets/translation/options.ko.json"
        )))
        .unwrap()
    }

    #[test]
    fn two_page_proof_exceeds_one_page_without_changing_reserved_tiles() {
        let source = source_font_page();
        let requested_pages = proof_page_glyphs(&options_localization()).unwrap();
        let font = load_dalmoori().unwrap();
        let page_a = plan_page(
            "page_a",
            FIRST_EXTENSION_CHR_PAGE,
            &source,
            &requested_pages[0],
            &font,
        )
        .unwrap();
        let page_b = plan_page(
            "page_b",
            FIRST_EXTENSION_CHR_PAGE + 1,
            &source,
            &requested_pages[1],
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
        assert_eq!(page_a.report.glyph_count, 112);
        assert_eq!(page_b.report.glyph_count, 112);
        assert_eq!(page_a.report.shared_screen_glyph_count, 12);
        assert_eq!(page_b.report.shared_screen_glyph_count, 12);
        assert_eq!(page_a.report.page_local_proof_glyph_count, 100);
        assert_eq!(page_b.report.page_local_proof_glyph_count, 100);
        assert_eq!(
            page_a.report.assignments[..12],
            page_b.report.assignments[..12]
        );
        assert_eq!(union.len(), 212);
        assert!(union.len() > ACTIVE_HANGUL_SLOT_COUNT);
        verify_reserved_bytes(&source, &page_a.bytes).unwrap();
        verify_reserved_bytes(&source, &page_b.bytes).unwrap();
    }

    #[test]
    fn rejects_a_page_that_exceeds_the_active_slot_count() {
        let source = source_font_page();
        let glyphs = (0..=ACTIVE_HANGUL_SLOT_COUNT)
            .map(|index| PlannedGlyph {
                code: index as u8,
                character: char::from_u32(FIRST_HANGUL_SYLLABLE + index as u32).unwrap(),
                scope: "page_local_proof",
            })
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
            &[
                PlannedGlyph {
                    code: 0x30,
                    character: '한',
                    scope: "page_local_proof",
                },
                PlannedGlyph {
                    code: 0x31,
                    character: '한',
                    scope: "page_local_proof",
                },
            ],
            &load_dalmoori().unwrap(),
        )
        .unwrap_err()
        .to_string();
        assert!(error.contains("duplicate glyphs"));
    }

    #[test]
    fn rejects_a_reserved_code_assignment() {
        let source = source_font_page();
        let error = plan_page(
            "reserved",
            FIRST_EXTENSION_CHR_PAGE,
            &source,
            &[PlannedGlyph {
                code: 0x60,
                character: '한',
                scope: "page_local_proof",
            }],
            &load_dalmoori().unwrap(),
        )
        .unwrap_err()
        .to_string();

        assert!(error.contains("assigns reserved codes"));
    }
}
