use std::{
    collections::{BTreeMap, BTreeSet},
    fs,
    path::Path,
};

use anyhow::{Context, Result, ensure};
use serde::Deserialize;

use crate::{
    font::{load_dalmoori, rasterize_glyph},
    font_slots::{FONT_PAGE_SIZE, FONT_TILE_SIZE, active_hangul_codes},
    hangul_page_plan::{ScreenPagePairPlan, plan_screen_page_pair},
};

pub(crate) const ROSTER_TEXT_PRG_BANK: u8 = 0x0B;
pub(crate) const ROSTER_HEADER_CPU_ADDRESS: u16 = 0x917C;
pub(crate) const SOURCE_ROSTER_HEADER: [u8; 12] = [
    0x15, 0x20, 0x03, 0xFF, 0xFF, 0xFF, 0x75, 0x7F, 0xFF, 0x71, 0x79, 0xED,
];

const ROSTER_HEADER_JAPANESE_FIELD_LEN: usize = 3;
const EXPECTED_SCREEN_ROLE: &str = "unit_roster";
const EXPECTED_ENTRY_ID: &str = "name_header";
const EXPECTED_SOURCE_JAPANESE: &str = "なまえ";
const EXPECTED_SOURCE_CODES: [u8; ROSTER_HEADER_JAPANESE_FIELD_LEN] = [0x15, 0x20, 0x03];
const ROSTER_PAGE_IDS: [&str; 2] = ["roster_page_a", "roster_page_b"];
const ROSTER_PAGE_LOCAL_PROOF_GLYPH_COUNT: usize = 105;
const ROSTER_VISIBLE_FD_CODES: [u8; 29] = [
    0x03, 0x0F, 0x15, 0x20, 0x30, 0x31, 0x35, 0x39, 0x3B, 0x3C, 0x3F, 0x40, 0x44, 0x4D, 0x50, 0x5A,
    0x5F, 0x60, 0x61, 0x62, 0x66, 0x68, 0x71, 0x75, 0x79, 0x7F, 0xA9, 0xFE, 0xFF,
];

#[derive(Debug, Clone, Deserialize)]
pub(crate) struct RosterLocalization {
    format_version: u8,
    translate_from: String,
    translate_to: String,
    preserve_existing_english: bool,
    status: String,
    screen_role: String,
    entry: RosterEntry,
    glyphs: Vec<RosterGlyphAssignment>,
}

#[derive(Debug, Clone, Deserialize)]
struct RosterEntry {
    id: String,
    source_japanese: String,
    korean: String,
    source_codes: Vec<u8>,
    korean_codes: Vec<u8>,
}

#[derive(Debug, Clone, Deserialize)]
struct RosterGlyphAssignment {
    code: u8,
    character: char,
}

#[derive(Debug, Clone)]
pub(crate) struct ValidatedRosterLocalization {
    pub(crate) replacement_header: [u8; SOURCE_ROSTER_HEADER.len()],
    pub(crate) tiles: BTreeMap<u8, [u8; FONT_TILE_SIZE]>,
    pub(crate) review_complete: bool,
    characters_by_code: BTreeMap<u8, char>,
}

impl RosterLocalization {
    pub(crate) fn from_path(path: &Path) -> Result<Self> {
        let data = fs::read(path)
            .with_context(|| format!("read roster localization {}", path.display()))?;
        serde_json::from_slice(&data)
            .with_context(|| format!("parse roster localization {}", path.display()))
    }

    pub(crate) fn validate(&self) -> Result<ValidatedRosterLocalization> {
        ensure!(self.format_version == 1, "format_version must be 1");
        ensure!(self.translate_from == "ja", "translate_from must be ja");
        ensure!(self.translate_to == "ko", "translate_to must be ko");
        ensure!(
            self.preserve_existing_english,
            "preserve_existing_english must be true"
        );
        ensure!(
            matches!(self.status.as_str(), "needs_human_review" | "complete"),
            "status must be needs_human_review or complete"
        );
        ensure!(
            self.screen_role == EXPECTED_SCREEN_ROLE,
            "screen_role must be {EXPECTED_SCREEN_ROLE}"
        );
        ensure!(
            self.entry.id == EXPECTED_ENTRY_ID,
            "roster entry must be {EXPECTED_ENTRY_ID}"
        );
        ensure!(
            self.entry.source_japanese == EXPECTED_SOURCE_JAPANESE,
            "roster Japanese source text changed"
        );
        ensure!(
            self.entry.source_codes == EXPECTED_SOURCE_CODES,
            "roster Japanese source codes changed"
        );
        ensure!(!self.entry.korean.is_empty(), "roster Korean text is empty");
        ensure!(
            self.entry.korean_codes.len() <= ROSTER_HEADER_JAPANESE_FIELD_LEN,
            "roster Korean text exceeds the fixed Japanese header field"
        );
        ensure!(
            self.entry
                .korean_codes
                .iter()
                .all(|code| active_hangul_codes().contains(code)),
            "roster Korean codes must use active Hangul slots"
        );
        ensure!(
            self.entry
                .korean_codes
                .iter()
                .copied()
                .collect::<BTreeSet<_>>()
                .len()
                == self.entry.korean_codes.len(),
            "roster Korean codes contain duplicates"
        );

        let font = load_dalmoori()?;
        let mut characters_by_code = BTreeMap::new();
        let mut tiles = BTreeMap::new();
        for glyph in &self.glyphs {
            ensure!(
                self.entry.korean_codes.contains(&glyph.code),
                "roster glyph code {:02X} is not used by the Korean header",
                glyph.code
            );
            ensure!(
                characters_by_code
                    .insert(glyph.code, glyph.character)
                    .is_none(),
                "duplicate roster glyph code {:02X}",
                glyph.code
            );
            ensure!(
                tiles
                    .insert(glyph.code, rasterize_glyph(&font, glyph.character)?)
                    .is_none(),
                "duplicate roster glyph tile {:02X}",
                glyph.code
            );
        }
        ensure!(
            tiles.len() == self.entry.korean_codes.len(),
            "every roster Korean code must have exactly one glyph"
        );
        let rendered = self
            .entry
            .korean_codes
            .iter()
            .map(|code| {
                characters_by_code.get(code).copied().ok_or_else(|| {
                    anyhow::anyhow!("roster Korean code {code:02X} has no glyph assignment")
                })
            })
            .collect::<Result<String>>()?;
        ensure!(
            rendered == self.entry.korean,
            "roster Korean text does not match its glyph sequence"
        );

        let mut replacement_header = SOURCE_ROSTER_HEADER;
        replacement_header[..ROSTER_HEADER_JAPANESE_FIELD_LEN].fill(0xFF);
        replacement_header[..self.entry.korean_codes.len()]
            .copy_from_slice(&self.entry.korean_codes);
        ensure!(
            replacement_header[ROSTER_HEADER_JAPANESE_FIELD_LEN..]
                == SOURCE_ROSTER_HEADER[ROSTER_HEADER_JAPANESE_FIELD_LEN..],
            "roster localization changed the original LV/HP header bytes"
        );

        Ok(ValidatedRosterLocalization {
            replacement_header,
            tiles,
            review_complete: self.status == "complete",
            characters_by_code,
        })
    }
}

impl ValidatedRosterLocalization {
    pub(crate) fn glyph_assignments(&self) -> BTreeMap<u8, char> {
        self.characters_by_code.clone()
    }
}

pub(crate) fn roster_visible_codes() -> BTreeSet<u8> {
    ROSTER_VISIBLE_FD_CODES.into_iter().collect()
}

pub(crate) fn build_roster_page_pair(
    source_font_page: &[u8],
    localization: &ValidatedRosterLocalization,
    physical_pages: [u8; 2],
) -> Result<ScreenPagePairPlan> {
    let visible_codes = ROSTER_VISIBLE_FD_CODES.into_iter().collect::<BTreeSet<_>>();
    let shared_glyphs = localization
        .characters_by_code
        .iter()
        .map(|(code, character)| (*code, *character))
        .collect::<Vec<_>>();
    let pages = plan_screen_page_pair(
        source_font_page,
        ROSTER_PAGE_IDS,
        physical_pages,
        &shared_glyphs,
        &visible_codes,
        ROSTER_PAGE_LOCAL_PROOF_GLYPH_COUNT,
    )?;

    for page in pages.page_pack.chunks_exact(FONT_PAGE_SIZE) {
        let changed_visible_codes = visible_codes
            .iter()
            .copied()
            .filter(|code| !localization.tiles.contains_key(code))
            .filter(|code| {
                let start = usize::from(*code) * FONT_TILE_SIZE;
                source_font_page[start..start + FONT_TILE_SIZE]
                    != page[start..start + FONT_TILE_SIZE]
            })
            .collect::<Vec<_>>();
        ensure!(
            changed_visible_codes.is_empty(),
            "roster page changed untranslated visible codes: {changed_visible_codes:02X?}"
        );
    }

    Ok(pages)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn source_font_page() -> Vec<u8> {
        (0..FONT_PAGE_SIZE)
            .map(|index| (index % 251) as u8)
            .collect()
    }

    fn roster_localization() -> RosterLocalization {
        serde_json::from_str(include_str!(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../../assets/translation/roster.ko.json"
        )))
        .unwrap()
    }

    #[test]
    fn replaces_only_the_japanese_name_header_and_keeps_lv_hp() {
        let validated = roster_localization().validate().unwrap();

        assert_eq!(
            validated.replacement_header,
            [
                0x15, 0x20, 0xFF, 0xFF, 0xFF, 0xFF, 0x75, 0x7F, 0xFF, 0x71, 0x79, 0xED,
            ]
        );
        assert_eq!(
            validated.tiles.keys().copied().collect::<Vec<_>>(),
            [0x15, 0x20]
        );
    }

    #[test]
    fn roster_page_pair_exceeds_one_page_without_changing_other_visible_codes() {
        let source = source_font_page();
        let validated = roster_localization().validate().unwrap();
        let pages = build_roster_page_pair(&source, &validated, [36, 37]).unwrap();

        assert_eq!(pages.assignment_count_per_page, 107);
        assert_eq!(pages.page_local_proof_glyph_count, 105);
        assert_eq!(pages.page_union_glyph_count, 212);
        assert_eq!(pages.page_pack.len(), 2 * FONT_PAGE_SIZE);
        assert_ne!(pages.page_sha1s[0], pages.page_sha1s[1]);
    }

    #[test]
    fn rejects_translating_from_english_or_unprotecting_english() {
        let mut localization = roster_localization();
        localization.translate_from = "en".to_owned();
        assert!(
            localization
                .validate()
                .unwrap_err()
                .to_string()
                .contains("translate_from must be ja")
        );

        let mut localization = roster_localization();
        localization.preserve_existing_english = false;
        assert!(
            localization
                .validate()
                .unwrap_err()
                .to_string()
                .contains("preserve_existing_english must be true")
        );
    }
}
