use std::{
    collections::{BTreeMap, BTreeSet},
    fs,
    path::Path,
};

use anyhow::{Context, Result, ensure};
use serde::{Deserialize, Serialize};

use crate::{
    japanese_encoding::japanese_text_glyph,
    mmc5_chr::switchable_bank_file_offset,
    mmc5_prg::fixed_bank_file_offset,
    rom::{EXPECTED_SOURCE_SHA1, Rom},
    sha1_hex,
    text_inventory::{
        FixedTextLogicalByte, encode_target_markup, is_japanese_character,
        protected_alphanumeric_glyph,
    },
};

use super::source_spec::{
    COMPOSITE_DISPATCH_BINDING, COMPOSITE_STATE_WRITER_ADDRESS, COMPOSITE_STATE_WRITER_BINDING,
    FIXED_STRING_POINTER_TABLE_ADDRESS, MENU_LABEL_SPECS, RECORD_MENU_LABEL_BINDING,
    RECORD_MENU_LABEL_BINDING_ADDRESS, SAVE_SLOT_ROUTE_BINDING, SAVE_SLOT_ROUTE_BINDING_ADDRESS,
    SAVE_SLOT_ROUTE_SOURCE_PRG_BANK, SOURCE_PRG_BANK, START_MENU_LABEL_BINDING,
    START_MENU_LABEL_BINDING_ADDRESS,
};

#[derive(Debug)]
pub(crate) struct FrontEndMenuWorkspaceSummary {
    pub(crate) workspace_sha1: String,
    pub(crate) entry_count: usize,
    pub(crate) preserved_translation_count: usize,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
struct FrontEndMenuWorkspace {
    format_version: u8,
    source_sha1: String,
    translate_from: String,
    translate_to: String,
    preserve_existing_english_and_digits: bool,
    entries: Vec<FrontEndMenuEntry>,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
struct FrontEndMenuEntry {
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
pub(crate) struct FrontEndMenuPlannedEntry {
    pub(crate) id: String,
    pub(crate) file_offset: usize,
    pub(crate) source_storage_byte_count: usize,
    terminator: u8,
    logical_bytes: Vec<FixedTextLogicalByte>,
}

impl FrontEndMenuPlannedEntry {
    pub(crate) fn unique_glyphs(&self) -> BTreeSet<char> {
        self.logical_bytes
            .iter()
            .filter_map(|byte| match byte {
                FixedTextLogicalByte::TargetGlyph(glyph) => Some(*glyph),
                FixedTextLogicalByte::Encoded(_) => None,
            })
            .collect()
    }

    pub(crate) fn preserved_source_codes(&self) -> BTreeSet<u8> {
        self.logical_bytes
            .iter()
            .filter_map(|byte| match byte {
                FixedTextLogicalByte::Encoded(value) => Some(*value),
                FixedTextLogicalByte::TargetGlyph(_) => None,
            })
            .collect()
    }

    pub(crate) fn encoded_storage_bytes(
        &self,
        assignments: &BTreeMap<char, u8>,
    ) -> Result<Vec<u8>> {
        let body_capacity = self
            .source_storage_byte_count
            .checked_sub(1)
            .context("front-end menu storage has no terminator")?;
        let mut encoded = self
            .logical_bytes
            .iter()
            .map(|byte| match byte {
                FixedTextLogicalByte::Encoded(value) => Ok(*value),
                FixedTextLogicalByte::TargetGlyph(glyph) => assignments
                    .get(glyph)
                    .copied()
                    .with_context(|| format!("missing front-end menu code for {glyph:?}")),
            })
            .collect::<Result<Vec<_>>>()?;
        ensure!(
            encoded.len() <= body_capacity,
            "{} needs {} body bytes but owns only {body_capacity}",
            self.id,
            encoded.len()
        );
        encoded.resize(body_capacity, 0xFF);
        encoded.push(self.terminator);
        Ok(encoded)
    }
}

#[derive(Clone, Debug)]
pub(crate) struct FrontEndMenuPlan {
    pub(crate) workspace_sha1: String,
    pub(crate) review_complete: bool,
    pub(crate) entries: Vec<FrontEndMenuPlannedEntry>,
}

impl FrontEndMenuPlan {
    pub(crate) fn unique_glyphs(&self) -> BTreeSet<char> {
        self.entries
            .iter()
            .flat_map(FrontEndMenuPlannedEntry::unique_glyphs)
            .collect()
    }

    pub(crate) fn preserved_source_codes(&self) -> BTreeSet<u8> {
        self.entries
            .iter()
            .flat_map(FrontEndMenuPlannedEntry::preserved_source_codes)
            .collect()
    }
}

pub(crate) fn extract_front_end_menu_workspace(
    source_path: &Path,
    workspace_path: &Path,
) -> Result<FrontEndMenuWorkspaceSummary> {
    let rom = Rom::from_path(source_path)?;
    rom.verify_supported_japanese()?;
    let mut fresh = build_workspace(&rom)?;
    let preserved_translation_count = if workspace_path.exists() {
        let bytes = fs::read(workspace_path)
            .with_context(|| format!("read {}", workspace_path.display()))?;
        let existing: FrontEndMenuWorkspace = serde_json::from_slice(&bytes)
            .with_context(|| format!("parse {}", workspace_path.display()))?;
        preserve_translations(&mut fresh, &existing)?
    } else {
        0
    };
    let mut bytes =
        serde_json::to_vec_pretty(&fresh).context("serialize front-end menu workspace")?;
    bytes.push(b'\n');
    if let Some(parent) = workspace_path.parent() {
        fs::create_dir_all(parent).with_context(|| format!("create {}", parent.display()))?;
    }
    fs::write(workspace_path, &bytes)
        .with_context(|| format!("write {}", workspace_path.display()))?;
    Ok(FrontEndMenuWorkspaceSummary {
        workspace_sha1: sha1_hex(&bytes),
        entry_count: fresh.entries.len(),
        preserved_translation_count,
    })
}

pub(crate) fn plan_front_end_menu(rom: &Rom, workspace_path: &Path) -> Result<FrontEndMenuPlan> {
    rom.verify_supported_japanese()?;
    let bytes =
        fs::read(workspace_path).with_context(|| format!("read {}", workspace_path.display()))?;
    let workspace: FrontEndMenuWorkspace = serde_json::from_slice(&bytes)
        .with_context(|| format!("parse {}", workspace_path.display()))?;
    let fresh = build_workspace(rom)?;
    validate_scope(&workspace, &fresh)?;
    ensure!(
        workspace.entries.len() == fresh.entries.len(),
        "front-end menu workspace entry count changed"
    );

    let mut entries = Vec::with_capacity(workspace.entries.len());
    let mut review_complete = true;
    for (entry, source) in workspace.entries.iter().zip(&fresh.entries) {
        validate_source_binding(entry, source)?;
        validate_translation(entry, source)?;
        ensure!(
            entry.status != "untranslated",
            "{} is not translated",
            entry.id
        );
        review_complete &= entry.status == "complete";
        let logical_bytes = encode_target_markup(&entry.korean_markup)
            .with_context(|| format!("encode {}", entry.id))?;
        let source_bytes = decode_hex(&entry.source_bytes_hex)?;
        let source_preserved = source_bytes[..source_bytes.len() - 1]
            .iter()
            .copied()
            .filter(|code| japanese_text_glyph(*code).is_none())
            .collect::<Vec<_>>();
        let target_preserved = logical_bytes
            .iter()
            .filter_map(|byte| match byte {
                FixedTextLogicalByte::Encoded(value) => Some(*value),
                FixedTextLogicalByte::TargetGlyph(_) => None,
            })
            .collect::<Vec<_>>();
        ensure!(
            target_preserved == source_preserved,
            "protected English, digits, or layout changed for {}",
            entry.id
        );
        ensure!(
            logical_bytes.len() < entry.source_storage_byte_count,
            "{} translation does not fit its owned storage",
            entry.id
        );
        entries.push(FrontEndMenuPlannedEntry {
            id: entry.id.clone(),
            file_offset: decode_usize_hex(&entry.source_file_offset_hex)?,
            source_storage_byte_count: entry.source_storage_byte_count,
            terminator: *source_bytes
                .last()
                .context("front-end source has no terminator")?,
            logical_bytes,
        });
    }
    Ok(FrontEndMenuPlan {
        workspace_sha1: sha1_hex(&bytes),
        review_complete,
        entries,
    })
}

fn build_workspace(rom: &Rom) -> Result<FrontEndMenuWorkspace> {
    validate_consumers(rom)?;
    let mut entries = Vec::with_capacity(MENU_LABEL_SPECS.len());
    for spec in MENU_LABEL_SPECS {
        let pointer_address = FIXED_STRING_POINTER_TABLE_ADDRESS + u16::from(spec.index) * 2;
        let pointer_offset = switchable_bank_file_offset(SOURCE_PRG_BANK, pointer_address)?;
        let pointer =
            u16::from_le_bytes([rom.data()[pointer_offset], rom.data()[pointer_offset + 1]]);
        ensure!(
            pointer == spec.pointer,
            "front-end fixed-string pointer changed for {}",
            spec.id
        );
        let file_offset = switchable_bank_file_offset(SOURCE_PRG_BANK, pointer)?;
        let source = rom
            .data()
            .get(file_offset..file_offset + spec.expected.len())
            .context("front-end fixed string is outside the source")?;
        ensure!(
            source == spec.expected,
            "front-end fixed-string bytes changed for {}",
            spec.id
        );
        entries.push(FrontEndMenuEntry {
            id: spec.id.to_owned(),
            fixed_string_index: spec.index,
            pointer_cpu_address_hex: format!("0x{pointer:04X}"),
            source_file_offset_hex: format!("0x{file_offset:05X}"),
            source_storage_byte_count: source.len(),
            source_bytes_hex: encode_hex(source),
            source_sha1: sha1_hex(source),
            japanese_markup: decode_source_markup(&source[..source.len() - 1]),
            korean_markup: String::new(),
            status: "untranslated".to_owned(),
        });
    }
    Ok(FrontEndMenuWorkspace {
        format_version: 1,
        source_sha1: EXPECTED_SOURCE_SHA1.to_owned(),
        translate_from: "ja".to_owned(),
        translate_to: "ko".to_owned(),
        preserve_existing_english_and_digits: true,
        entries,
    })
}

fn validate_consumers(rom: &Rom) -> Result<()> {
    let dispatch_offset = switchable_bank_file_offset(SOURCE_PRG_BANK, 0x8000)?;
    ensure!(
        rom.data()[dispatch_offset..dispatch_offset + COMPOSITE_DISPATCH_BINDING.len()]
            == *COMPOSITE_DISPATCH_BINDING,
        "front-end composite dispatch binding changed"
    );
    for (address, expected, role) in [
        (
            START_MENU_LABEL_BINDING_ADDRESS,
            START_MENU_LABEL_BINDING,
            "start menu",
        ),
        (
            RECORD_MENU_LABEL_BINDING_ADDRESS,
            RECORD_MENU_LABEL_BINDING,
            "record menu",
        ),
    ] {
        let offset = switchable_bank_file_offset(SOURCE_PRG_BANK, address)?;
        ensure!(
            rom.data()[offset..offset + expected.len()] == *expected,
            "front-end {role} consumer binding changed"
        );
    }
    let save_slot_route_offset = switchable_bank_file_offset(
        SAVE_SLOT_ROUTE_SOURCE_PRG_BANK,
        SAVE_SLOT_ROUTE_BINDING_ADDRESS,
    )?;
    ensure!(
        rom.data()[save_slot_route_offset..save_slot_route_offset + SAVE_SLOT_ROUTE_BINDING.len()]
            == *SAVE_SLOT_ROUTE_BINDING,
        "front-end save-slot selection route binding changed"
    );
    let composite_state_writer_offset = fixed_bank_file_offset(COMPOSITE_STATE_WRITER_ADDRESS)?;
    ensure!(
        rom.data()[composite_state_writer_offset
            ..composite_state_writer_offset + COMPOSITE_STATE_WRITER_BINDING.len()]
            == *COMPOSITE_STATE_WRITER_BINDING,
        "front-end composite-state writer binding changed"
    );
    Ok(())
}

fn preserve_translations(
    fresh: &mut FrontEndMenuWorkspace,
    existing: &FrontEndMenuWorkspace,
) -> Result<usize> {
    validate_scope(existing, fresh)?;
    let existing_by_id = existing
        .entries
        .iter()
        .map(|entry| (entry.id.as_str(), entry))
        .collect::<BTreeMap<_, _>>();
    ensure!(
        existing_by_id.len() == existing.entries.len(),
        "duplicate front-end menu workspace ID"
    );
    let mut preserved = 0;
    for entry in &mut fresh.entries {
        if let Some(old) = existing_by_id.get(entry.id.as_str()) {
            validate_source_binding(old, entry)?;
            validate_translation(old, entry)?;
            entry.korean_markup.clone_from(&old.korean_markup);
            entry.status.clone_from(&old.status);
            preserved += usize::from(old.status != "untranslated");
        }
    }
    Ok(preserved)
}

fn validate_scope(actual: &FrontEndMenuWorkspace, expected: &FrontEndMenuWorkspace) -> Result<()> {
    ensure!(
        actual.format_version == expected.format_version
            && actual.source_sha1 == expected.source_sha1
            && actual.translate_from == expected.translate_from
            && actual.translate_to == expected.translate_to
            && actual.preserve_existing_english_and_digits
                == expected.preserve_existing_english_and_digits,
        "front-end menu workspace scope changed"
    );
    Ok(())
}

fn validate_source_binding(actual: &FrontEndMenuEntry, expected: &FrontEndMenuEntry) -> Result<()> {
    ensure!(
        actual.id == expected.id
            && actual.fixed_string_index == expected.fixed_string_index
            && actual.pointer_cpu_address_hex == expected.pointer_cpu_address_hex
            && actual.source_file_offset_hex == expected.source_file_offset_hex
            && actual.source_storage_byte_count == expected.source_storage_byte_count
            && actual.source_bytes_hex == expected.source_bytes_hex
            && actual.source_sha1 == expected.source_sha1
            && actual.japanese_markup == expected.japanese_markup,
        "front-end menu source binding changed for {}",
        expected.id
    );
    Ok(())
}

fn validate_translation(entry: &FrontEndMenuEntry, source: &FrontEndMenuEntry) -> Result<()> {
    ensure!(
        ["untranslated", "needs_human_review", "complete"].contains(&entry.status.as_str()),
        "invalid status for {}",
        entry.id
    );
    ensure!(
        (entry.status == "untranslated") == entry.korean_markup.is_empty(),
        "translation status and text disagree for {}",
        entry.id
    );
    ensure!(
        !entry.korean_markup.chars().any(is_japanese_character),
        "Korean front-end menu still contains Japanese for {}",
        entry.id
    );
    let protected = source
        .japanese_markup
        .chars()
        .filter(|character| character.is_ascii())
        .collect::<String>();
    let retained = entry
        .korean_markup
        .chars()
        .filter(|character| character.is_ascii())
        .collect::<String>();
    ensure!(
        protected == retained,
        "existing ASCII changed for {}",
        entry.id
    );
    Ok(())
}

fn decode_source_markup(raw: &[u8]) -> String {
    raw.iter()
        .map(|code| {
            japanese_text_glyph(*code)
                .map(str::to_owned)
                .or_else(|| protected_alphanumeric_glyph(*code).map(str::to_owned))
                .unwrap_or_else(|| format!("{{{code:02X}}}"))
        })
        .collect()
}

fn encode_hex(bytes: &[u8]) -> String {
    bytes
        .iter()
        .map(|byte| format!("{byte:02X}"))
        .collect::<Vec<_>>()
        .join(" ")
}

fn decode_hex(encoded: &str) -> Result<Vec<u8>> {
    encoded
        .split_whitespace()
        .map(|byte| u8::from_str_radix(byte, 16).context("decode front-end menu source byte"))
        .collect()
}

fn decode_usize_hex(encoded: &str) -> Result<usize> {
    usize::from_str_radix(encoded.trim_start_matches("0x"), 16)
        .context("decode front-end menu file offset")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn encoded_menu_entry_pads_owned_storage_and_restores_terminator() {
        let entry = FrontEndMenuPlannedEntry {
            id: "front-end-menu:new-game".to_owned(),
            file_offset: 0,
            source_storage_byte_count: 5,
            terminator: 0xED,
            logical_bytes: vec![
                FixedTextLogicalByte::TargetGlyph('처'),
                FixedTextLogicalByte::TargetGlyph('음'),
            ],
        };
        let assignments = BTreeMap::from([('처', 0xC0), ('음', 0xC1)]);

        assert_eq!(
            entry.encoded_storage_bytes(&assignments).unwrap(),
            vec![0xC0, 0xC1, 0xFF, 0xFF, 0xED]
        );
    }
}
