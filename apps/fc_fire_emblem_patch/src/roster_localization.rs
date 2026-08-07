use std::{
    collections::{BTreeMap, BTreeSet},
    fs,
    path::Path,
};

use anyhow::{Context, Result, ensure};
use serde::Deserialize;

use crate::{
    font::{load_dalmoori, rasterize_glyph},
    font_slots::{FONT_PAGE_SIZE, FONT_TILE_SIZE, protected_original_codes},
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
            self.status == "technical_poc",
            "status must be technical_poc"
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
                .all(|code| EXPECTED_SOURCE_CODES.contains(code)),
            "roster Korean codes must reuse Japanese header slots"
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
        })
    }
}

pub(crate) fn build_roster_font_page(
    source_font_page: &[u8],
    localization: &ValidatedRosterLocalization,
) -> Result<Vec<u8>> {
    ensure!(
        source_font_page.len() == FONT_PAGE_SIZE,
        "roster source font page must be exactly 4 KiB"
    );
    let mut page = source_font_page.to_vec();
    for (code, tile) in &localization.tiles {
        let start = usize::from(*code) * FONT_TILE_SIZE;
        page[start..start + FONT_TILE_SIZE].copy_from_slice(tile);
    }

    let changed_codes = (u8::MIN..=u8::MAX)
        .filter(|code| {
            let start = usize::from(*code) * FONT_TILE_SIZE;
            source_font_page[start..start + FONT_TILE_SIZE] != page[start..start + FONT_TILE_SIZE]
        })
        .collect::<BTreeSet<_>>();
    ensure!(
        changed_codes == localization.tiles.keys().copied().collect(),
        "roster page changed tiles outside its Korean glyph assignments"
    );
    ensure!(
        protected_original_codes().into_iter().all(|code| {
            let start = usize::from(code) * FONT_TILE_SIZE;
            source_font_page[start..start + FONT_TILE_SIZE] == page[start..start + FONT_TILE_SIZE]
        }),
        "roster page changed a protected original English, digit, control, or latch tile"
    );
    Ok(page)
}

#[cfg(test)]
mod tests {
    use super::*;

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
    fn roster_page_changes_only_two_japanese_tiles() {
        let source = (0..FONT_PAGE_SIZE)
            .map(|index| (index % 251) as u8)
            .collect::<Vec<_>>();
        let validated = roster_localization().validate().unwrap();
        let page = build_roster_font_page(&source, &validated).unwrap();

        for code in u8::MIN..=u8::MAX {
            let start = usize::from(code) * FONT_TILE_SIZE;
            assert_eq!(
                source[start..start + FONT_TILE_SIZE] != page[start..start + FONT_TILE_SIZE],
                [0x15, 0x20].contains(&code)
            );
        }
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
