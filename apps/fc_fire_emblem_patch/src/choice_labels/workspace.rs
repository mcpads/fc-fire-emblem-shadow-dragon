use std::{
    collections::{BTreeMap, BTreeSet},
    fs,
    path::Path,
};

use anyhow::{Context, Result, ensure};
use serde::Deserialize;

use crate::{
    font_slots::active_hangul_codes,
    japanese_encoding::is_japanese_text_code,
    mmc5_chr::switchable_bank_file_offset,
    rom::{EXPECTED_SOURCE_SHA1, Rom},
    sha1_hex,
    text_inventory::{
        FixedTextLogicalByte, decode_source_markup, encode_target_markup, is_japanese_character,
    },
};

use super::source_spec::{
    LABEL_SPECS, POINTER_LOAD_ADDRESS, POINTER_LOAD_BYTES, POINTER_TABLE_ADDRESS,
    SHOP_CHOICE_COMPOSER_ADDRESS, SHOP_CHOICE_COMPOSER_BYTES, SOURCE_PRG_BANK,
};

#[derive(Debug, Deserialize)]
struct ChoiceLabelWorkspace {
    format_version: u8,
    source_sha1: String,
    translate_from: String,
    translate_to: String,
    preserve_existing_english_and_digits: bool,
    entries: Vec<ChoiceLabelEntry>,
}

#[derive(Debug, Deserialize)]
struct ChoiceLabelEntry {
    id: String,
    fixed_string_index: u8,
    pointer_cpu_address_hex: String,
    source_file_offset_hex: String,
    source_storage_byte_count: usize,
    source_bytes_hex: String,
    source_sha1: String,
    japanese_markup: String,
    korean_markup: String,
    status: String,
}

#[derive(Clone, Debug)]
pub(crate) struct ChoiceLabelPlannedEntry {
    pub(crate) id: String,
    pub(crate) fixed_string_index: u8,
    logical_bytes: Vec<FixedTextLogicalByte>,
    terminator: u8,
}

impl ChoiceLabelPlannedEntry {
    pub(crate) fn logical_bytes(&self) -> &[FixedTextLogicalByte] {
        &self.logical_bytes
    }

    pub(crate) fn unique_glyphs(&self) -> BTreeSet<char> {
        self.logical_bytes
            .iter()
            .filter_map(|byte| match byte {
                FixedTextLogicalByte::TargetGlyph(glyph) => Some(*glyph),
                FixedTextLogicalByte::Encoded(_) => None,
            })
            .collect()
    }

    pub(crate) fn encoded_bytes(&self, assignments: &BTreeMap<char, u8>) -> Result<Vec<u8>> {
        let mut encoded = self
            .logical_bytes
            .iter()
            .map(|byte| match byte {
                FixedTextLogicalByte::Encoded(value) => Ok(*value),
                FixedTextLogicalByte::TargetGlyph(glyph) => assignments
                    .get(glyph)
                    .copied()
                    .with_context(|| format!("missing choice-label code for {glyph:?}")),
            })
            .collect::<Result<Vec<_>>>()?;
        encoded.push(self.terminator);
        Ok(encoded)
    }
}

#[derive(Clone, Debug)]
pub(crate) struct ChoiceLabelPlan {
    pub(crate) workspace_sha1: String,
    pub(crate) review_complete: bool,
    pub(crate) entries: Vec<ChoiceLabelPlannedEntry>,
    pub(crate) preserved_active_codes: BTreeSet<u8>,
    pub(crate) source_reclaimable_active_codes: BTreeSet<u8>,
}

impl ChoiceLabelPlan {
    pub(crate) fn entry(&self, id: &str) -> Result<&ChoiceLabelPlannedEntry> {
        self.entries
            .iter()
            .find(|entry| entry.id == id)
            .with_context(|| format!("choice-label plan lost {id}"))
    }

    pub(crate) fn unique_glyphs(&self) -> BTreeSet<char> {
        self.entries
            .iter()
            .flat_map(ChoiceLabelPlannedEntry::unique_glyphs)
            .collect()
    }
}

pub(crate) fn plan_choice_labels(rom: &Rom, workspace_path: &Path) -> Result<ChoiceLabelPlan> {
    rom.verify_supported_japanese()?;
    validate_consumers(rom)?;
    let workspace_bytes =
        fs::read(workspace_path).with_context(|| format!("read {}", workspace_path.display()))?;
    let workspace: ChoiceLabelWorkspace = serde_json::from_slice(&workspace_bytes)
        .with_context(|| format!("parse {}", workspace_path.display()))?;
    ensure!(
        workspace.format_version == 1
            && workspace.source_sha1 == EXPECTED_SOURCE_SHA1
            && workspace.translate_from == "ja"
            && workspace.translate_to == "ko"
            && workspace.preserve_existing_english_and_digits,
        "choice-label workspace scope changed"
    );
    ensure!(
        workspace.entries.len() == LABEL_SPECS.len(),
        "choice-label workspace entry count changed"
    );

    let mut entries = Vec::with_capacity(LABEL_SPECS.len());
    let mut review_complete = true;
    for (entry, spec) in workspace.entries.iter().zip(LABEL_SPECS) {
        let pointer_offset = switchable_bank_file_offset(
            SOURCE_PRG_BANK,
            POINTER_TABLE_ADDRESS + u16::from(spec.index) * 2,
        )?;
        let pointer =
            u16::from_le_bytes([rom.data()[pointer_offset], rom.data()[pointer_offset + 1]]);
        let source_file_offset = switchable_bank_file_offset(SOURCE_PRG_BANK, pointer)?;
        let source = rom
            .data()
            .get(source_file_offset..source_file_offset + spec.expected.len())
            .context("choice-label source is outside the ROM")?;
        ensure!(
            entry.id == spec.id
                && entry.fixed_string_index == spec.index
                && entry.pointer_cpu_address_hex == format!("0x{:04X}", spec.pointer)
                && entry.source_file_offset_hex == format!("0x{source_file_offset:05X}")
                && entry.source_storage_byte_count == spec.expected.len()
                && entry.source_bytes_hex == encode_hex(spec.expected)
                && entry.source_sha1 == sha1_hex(spec.expected)
                && entry.japanese_markup
                    == decode_source_markup(&spec.expected[..spec.expected.len() - 1]),
            "choice-label workspace binding changed for {}",
            spec.id
        );
        ensure!(
            pointer == spec.pointer && source == spec.expected,
            "choice-label ROM binding changed for {}",
            spec.id
        );
        ensure!(
            ["needs_human_review", "complete"].contains(&entry.status.as_str())
                && !entry.korean_markup.is_empty()
                && !entry.korean_markup.chars().any(is_japanese_character),
            "choice-label translation is incomplete for {}",
            spec.id
        );
        let logical_bytes = encode_target_markup(&entry.korean_markup)
            .with_context(|| format!("encode {}", spec.id))?;
        ensure!(
            logical_bytes.len() < entry.source_storage_byte_count,
            "choice-label translation must fit the source storage for {}",
            spec.id
        );
        review_complete &= entry.status == "complete";
        entries.push(ChoiceLabelPlannedEntry {
            id: entry.id.clone(),
            fixed_string_index: entry.fixed_string_index,
            logical_bytes,
            terminator: *spec.expected.last().unwrap(),
        });
    }
    let active_codes = active_hangul_codes().into_iter().collect::<BTreeSet<_>>();
    let preserved_active_codes = entries
        .iter()
        .flat_map(|entry| &entry.logical_bytes)
        .filter_map(|byte| match byte {
            FixedTextLogicalByte::Encoded(code) if active_codes.contains(code) => Some(*code),
            FixedTextLogicalByte::Encoded(_) | FixedTextLogicalByte::TargetGlyph(_) => None,
        })
        .collect::<BTreeSet<_>>();
    let source_reclaimable_active_codes = LABEL_SPECS
        .iter()
        .flat_map(|spec| spec.expected[..spec.expected.len() - 1].iter().copied())
        .filter(|code| {
            is_japanese_text_code(*code)
                && active_codes.contains(code)
                && !preserved_active_codes.contains(code)
        })
        .collect::<BTreeSet<_>>();
    ensure!(
        !source_reclaimable_active_codes.is_empty(),
        "choice-label plan has no exact source Japanese codes to reclaim"
    );
    Ok(ChoiceLabelPlan {
        workspace_sha1: sha1_hex(&workspace_bytes),
        review_complete,
        entries,
        preserved_active_codes,
        source_reclaimable_active_codes,
    })
}

fn validate_consumers(rom: &Rom) -> Result<()> {
    for (address, expected, role) in [
        (
            POINTER_LOAD_ADDRESS,
            POINTER_LOAD_BYTES.as_slice(),
            "fixed-label pointer loader",
        ),
        (
            SHOP_CHOICE_COMPOSER_ADDRESS,
            SHOP_CHOICE_COMPOSER_BYTES.as_slice(),
            "weapon-shop choice composer",
        ),
    ] {
        let offset = switchable_bank_file_offset(SOURCE_PRG_BANK, address)?;
        ensure!(
            rom.data()[offset..offset + expected.len()] == *expected,
            "choice-label {role} changed"
        );
    }
    Ok(())
}

fn encode_hex(bytes: &[u8]) -> String {
    bytes
        .iter()
        .map(|byte| format!("{byte:02X}"))
        .collect::<Vec<_>>()
        .join(" ")
}

#[cfg(test)]
mod tests {
    use std::path::Path;

    use super::*;

    #[test]
    fn public_choice_labels_bind_the_supported_source() {
        let source = Path::new(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../../roms/Fire Emblem - Ankoku Ryuu to Hikari no Tsurugi (Japan).nes"
        ));
        if !source.exists() {
            return;
        }
        let workspace = Path::new(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../../assets/translation/choice-labels.ko.json"
        ));
        let rom = Rom::from_path(source).unwrap();
        let plan = plan_choice_labels(&rom, workspace).unwrap();

        assert_eq!(plan.entries.len(), 2);
        assert_eq!(plan.entries[0].fixed_string_index, 0x22);
        assert_eq!(plan.entries[1].fixed_string_index, 0x23);
        assert_eq!(
            plan.unique_glyphs(),
            BTreeSet::from(['니', '아', '예', '오'])
        );
    }
}
