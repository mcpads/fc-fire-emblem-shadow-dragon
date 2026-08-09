use std::collections::BTreeSet;

use anyhow::{Result, ensure};
use serde::Serialize;

use crate::{
    font_slots::{LAYOUT_RESERVED_CODES, PRESERVED_DISPLAY_CODES},
    japanese_encoding::is_japanese_text_code,
};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum SourceLiteralCodeClass {
    Japanese,
    PreservedOriginal,
    Layout,
    Unresolved,
}

pub(crate) fn classify_source_literal_code(code: u8) -> SourceLiteralCodeClass {
    if is_japanese_text_code(code) {
        SourceLiteralCodeClass::Japanese
    } else if PRESERVED_DISPLAY_CODES.contains(&code) {
        SourceLiteralCodeClass::PreservedOriginal
    } else if LAYOUT_RESERVED_CODES.contains(&code) {
        SourceLiteralCodeClass::Layout
    } else {
        SourceLiteralCodeClass::Unresolved
    }
}

#[derive(Debug, Serialize)]
pub(crate) struct TranslationSurfaceLiteralInventory {
    pub(crate) unique_literal_storage_byte_count: usize,
    pub(crate) japanese_literal_storage_byte_count: usize,
    pub(crate) preserved_original_literal_storage_byte_count: usize,
    pub(crate) layout_literal_storage_byte_count: usize,
    pub(crate) unresolved_literal_storage_byte_count: usize,
    pub(crate) japanese_codes_hex: Vec<String>,
    pub(crate) preserved_original_codes_hex: Vec<String>,
    pub(crate) layout_codes_hex: Vec<String>,
    pub(crate) unresolved_codes_hex: Vec<String>,
}

pub(crate) fn classify_translation_surface_literal_codes(
    codes: impl IntoIterator<Item = u8>,
    inventory_role: &str,
) -> Result<TranslationSurfaceLiteralInventory> {
    let mut japanese_literal_storage_byte_count = 0;
    let mut preserved_original_literal_storage_byte_count = 0;
    let mut layout_literal_storage_byte_count = 0;
    let mut unresolved_literal_storage_byte_count = 0;
    let mut japanese_codes = BTreeSet::new();
    let mut preserved_original_codes = BTreeSet::new();
    let mut layout_codes = BTreeSet::new();
    let mut unresolved_codes = BTreeSet::new();
    let mut unique_literal_storage_byte_count = 0;

    for code in codes {
        unique_literal_storage_byte_count += 1;
        match classify_source_literal_code(code) {
            SourceLiteralCodeClass::Japanese => {
                japanese_literal_storage_byte_count += 1;
                japanese_codes.insert(code);
            }
            SourceLiteralCodeClass::PreservedOriginal => {
                preserved_original_literal_storage_byte_count += 1;
                preserved_original_codes.insert(code);
            }
            SourceLiteralCodeClass::Layout => {
                layout_literal_storage_byte_count += 1;
                layout_codes.insert(code);
            }
            SourceLiteralCodeClass::Unresolved => {
                unresolved_literal_storage_byte_count += 1;
                unresolved_codes.insert(code);
            }
        }
    }
    ensure!(
        japanese_literal_storage_byte_count
            + preserved_original_literal_storage_byte_count
            + layout_literal_storage_byte_count
            + unresolved_literal_storage_byte_count
            == unique_literal_storage_byte_count,
        "{inventory_role} literal classification lost storage bytes"
    );

    Ok(TranslationSurfaceLiteralInventory {
        unique_literal_storage_byte_count,
        japanese_literal_storage_byte_count,
        preserved_original_literal_storage_byte_count,
        layout_literal_storage_byte_count,
        unresolved_literal_storage_byte_count,
        japanese_codes_hex: hex_codes(japanese_codes),
        preserved_original_codes_hex: hex_codes(preserved_original_codes),
        layout_codes_hex: hex_codes(layout_codes),
        unresolved_codes_hex: hex_codes(unresolved_codes),
    })
}

fn hex_codes(codes: BTreeSet<u8>) -> Vec<String> {
    codes
        .into_iter()
        .map(|code| format!("{code:02X}"))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn source_literal_classification_keeps_translation_and_preservation_distinct() {
        assert_eq!(
            classify_source_literal_code(0x0F),
            SourceLiteralCodeClass::Japanese
        );
        assert_eq!(
            classify_source_literal_code(0x60),
            SourceLiteralCodeClass::PreservedOriginal
        );
        assert_eq!(
            classify_source_literal_code(0x9B),
            SourceLiteralCodeClass::PreservedOriginal
        );
        assert_eq!(
            classify_source_literal_code(0x9D),
            SourceLiteralCodeClass::PreservedOriginal
        );
        assert_eq!(
            classify_source_literal_code(0xFF),
            SourceLiteralCodeClass::Layout
        );
        assert_eq!(
            classify_source_literal_code(0x8C),
            SourceLiteralCodeClass::Unresolved
        );
    }

    #[test]
    fn literal_inventory_preserves_occurrence_counts_and_distinct_code_sets() {
        let inventory = classify_translation_surface_literal_codes(
            [0x01, 0x01, 0x60, 0xFF, 0x8C],
            "test inventory",
        )
        .unwrap();

        assert_eq!(inventory.unique_literal_storage_byte_count, 5);
        assert_eq!(inventory.japanese_literal_storage_byte_count, 2);
        assert_eq!(inventory.preserved_original_literal_storage_byte_count, 1);
        assert_eq!(inventory.layout_literal_storage_byte_count, 1);
        assert_eq!(inventory.unresolved_literal_storage_byte_count, 1);
        assert_eq!(inventory.japanese_codes_hex, ["01"]);
        assert_eq!(inventory.preserved_original_codes_hex, ["60"]);
        assert_eq!(inventory.layout_codes_hex, ["FF"]);
        assert_eq!(inventory.unresolved_codes_hex, ["8C"]);
    }
}
