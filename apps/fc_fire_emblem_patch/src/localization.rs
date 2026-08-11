use std::{
    collections::{BTreeMap, BTreeSet},
    fs,
    path::Path,
};

use anyhow::{Context, Result, ensure};
use serde::Deserialize;

use crate::font::{load_dalmoori, rasterize_glyph};

const ENTRY_SEPARATOR: u8 = 0xED;
const TABLE_TERMINATOR: u8 = 0xEF;
const EXPECTED_ENTRY_IDS: [&str; 3] = ["sound", "animation", "wait_timer"];
const EXPECTED_SOURCE_CODES: [&[u8]; 3] = [
    &[0x3A, 0x32, 0x5F, 0x44, 0x0F],
    &[0x30, 0x46, 0x53, 0x3F, 0x3B, 0x8B, 0x5F],
    &[0x32, 0x33, 0x31, 0x44, 0x40, 0x31, 0x50, 0x3F],
];

#[derive(Debug, Clone, Deserialize)]
pub struct OptionsLocalization {
    pub format_version: u8,
    pub translate_from: String,
    pub translate_to: String,
    pub preserve_existing_english: bool,
    pub status: String,
    pub entries: Vec<OptionEntry>,
    pub glyphs: Vec<GlyphAssignment>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct OptionEntry {
    pub id: String,
    pub source_japanese: String,
    pub korean: String,
    pub source_codes: Vec<u8>,
    pub korean_codes: Vec<u8>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct GlyphAssignment {
    pub code: u8,
    pub character: char,
}

#[derive(Debug, Clone)]
pub struct ValidatedLocalization {
    pub entries: Vec<OptionEntry>,
    pub tiles: BTreeMap<u8, [u8; 16]>,
    pub replacement_table: [u8; 24],
    pub review_complete: bool,
}

impl OptionsLocalization {
    pub fn from_path(path: &Path) -> Result<Self> {
        let data =
            fs::read(path).with_context(|| format!("read localization {}", path.display()))?;
        serde_json::from_slice(&data)
            .with_context(|| format!("parse localization {}", path.display()))
    }

    pub fn validate(&self) -> Result<ValidatedLocalization> {
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
            self.entries.len() == 3,
            "exactly three option entries are required"
        );

        let source_japanese_codes: BTreeSet<u8> = EXPECTED_SOURCE_CODES
            .iter()
            .flat_map(|codes| codes.iter().copied())
            .collect();
        let mut characters_by_code = BTreeMap::new();
        let font = load_dalmoori()?;
        let mut tiles = BTreeMap::new();
        for glyph in &self.glyphs {
            ensure!(
                source_japanese_codes.contains(&glyph.code),
                "glyph code {:02X} is not sourced from the Japanese option labels",
                glyph.code
            );
            ensure!(
                characters_by_code
                    .insert(glyph.code, glyph.character)
                    .is_none(),
                "duplicate glyph code {:02X}",
                glyph.code
            );
            ensure!(
                tiles
                    .insert(glyph.code, rasterize_glyph(&font, glyph.character)?)
                    .is_none(),
                "duplicate glyph tile {:02X}",
                glyph.code
            );
        }

        let mut replacement = Vec::new();
        for (index, entry) in self.entries.iter().enumerate() {
            ensure!(
                entry.id == EXPECTED_ENTRY_IDS[index],
                "unexpected option entry order"
            );
            ensure!(
                entry.source_codes == EXPECTED_SOURCE_CODES[index],
                "Japanese source codes changed for {}",
                entry.id
            );
            ensure!(
                !entry.source_japanese.is_empty(),
                "Japanese source text is empty"
            );
            ensure!(!entry.korean.is_empty(), "Korean text is empty");
            let rendered: Result<String> = entry
                .korean_codes
                .iter()
                .map(|code| {
                    characters_by_code.get(code).copied().ok_or_else(|| {
                        anyhow::anyhow!("Korean code {code:02X} has no glyph assignment")
                    })
                })
                .collect();
            ensure!(
                rendered? == entry.korean,
                "Korean text does not match the glyph sequence for {}",
                entry.id
            );
            replacement.extend_from_slice(&entry.korean_codes);
            replacement.push(ENTRY_SEPARATOR);
        }
        replacement.push(TABLE_TERMINATOR);
        ensure!(
            replacement.len() <= 24,
            "localized option table exceeds its fixed 24-byte slot"
        );
        replacement.resize(24, TABLE_TERMINATOR);

        Ok(ValidatedLocalization {
            entries: self.entries.clone(),
            tiles,
            replacement_table: replacement.try_into().unwrap(),
            review_complete: self.status == "complete",
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn valid_localization() -> OptionsLocalization {
        serde_json::from_str(include_str!(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../../assets/translation/options.ko.json"
        )))
        .unwrap()
    }

    #[test]
    fn accepts_only_japanese_source_codes_for_hangul_tiles() {
        let validated = valid_localization().validate().unwrap();

        assert_eq!(
            validated.replacement_table,
            [
                0x30, 0x31, 0x32, 0xED, 0x33, 0x3A, 0x3B, 0x3F, 0x40, 0xED, 0x44, 0x46, 0x50, 0x53,
                0xED, 0xEF, 0xEF, 0xEF, 0xEF, 0xEF, 0xEF, 0xEF, 0xEF, 0xEF,
            ]
        );
        assert!(
            validated
                .tiles
                .values()
                .all(|tile| tile[..8].iter().any(|byte| *byte != 0)
                    && tile[8..].iter().all(|byte| *byte == 0))
        );
    }

    #[test]
    fn rejects_an_english_translation_source() {
        let mut localization = valid_localization();
        localization.translate_from = "en".to_owned();

        assert!(
            localization
                .validate()
                .unwrap_err()
                .to_string()
                .contains("translate_from must be ja")
        );
    }

    #[test]
    fn rejects_disabling_existing_english_protection() {
        let mut localization = valid_localization();
        localization.preserve_existing_english = false;

        assert!(
            localization
                .validate()
                .unwrap_err()
                .to_string()
                .contains("preserve_existing_english must be true")
        );
    }

    #[test]
    fn rejects_a_hangul_tile_code_outside_the_japanese_source_labels() {
        let mut localization = valid_localization();
        localization.glyphs[0].code = 0x60;

        assert!(localization.validate().is_err());
    }
}
