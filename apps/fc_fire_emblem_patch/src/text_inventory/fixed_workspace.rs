use std::{
    collections::{BTreeMap, BTreeSet},
    fs,
    path::Path,
};

use anyhow::{Context, Result, ensure};
use serde::{Deserialize, Serialize};

use super::battle_message_templates::extract_battle_message_templates;
use super::*;
use crate::{
    japanese_encoding::japanese_text_glyph,
    mmc5_prg::fixed_bank_file_offset,
    rom::{EXPECTED_SOURCE_SHA1, Rom},
};

const TABLE_IDS: [&str; 5] = [
    "class-names",
    "item-names",
    "unit-names",
    "enemy-names",
    "terrain-names",
];

#[derive(Debug)]
pub(crate) struct FixedTextWorkspaceSummary {
    pub(crate) workspace_sha1: String,
    pub(crate) entry_count: usize,
    pub(crate) japanese_entry_count: usize,
    pub(crate) preserved_translation_count: usize,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
struct FixedTextWorkspace {
    format_version: u8,
    source_sha1: String,
    translate_from: String,
    translate_to: String,
    preserve_existing_english: bool,
    entries: Vec<FixedTextEntry>,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
struct FixedTextEntry {
    id: String,
    table_id: String,
    source_index: usize,
    alias_indices: Vec<usize>,
    pointer_cpu_address_hex: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    source_file_offset_hex: Option<String>,
    source_bytes_hex: String,
    source_sha1: String,
    japanese_markup: String,
    korean_markup: String,
    status: String,
}

#[derive(Clone, Debug)]
pub(crate) enum FixedTextLogicalByte {
    TargetGlyph(char),
    Encoded(u8),
}

#[derive(Clone, Debug)]
pub(crate) struct FixedTextPlannedEntry {
    pub(crate) id: String,
    pub(crate) table_id: String,
    pub(crate) source_index: usize,
    pub(crate) alias_indices: Vec<usize>,
    pub(crate) file_offset: usize,
    pub(crate) source_storage_byte_count: usize,
    pub(crate) logical_bytes: Vec<FixedTextLogicalByte>,
}

impl FixedTextPlannedEntry {
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
        self.logical_bytes
            .iter()
            .map(|byte| match byte {
                FixedTextLogicalByte::Encoded(value) => Ok(*value),
                FixedTextLogicalByte::TargetGlyph(glyph) => assignments
                    .get(glyph)
                    .copied()
                    .with_context(|| format!("missing fixed-text code assignment for {glyph:?}")),
            })
            .collect()
    }
}

#[derive(Clone, Debug)]
pub(crate) struct FixedTextPlan {
    pub(crate) entries: Vec<FixedTextPlannedEntry>,
}

impl FixedTextPlan {
    pub(crate) fn entry_for_source_index(
        &self,
        table_id: &str,
        source_index: usize,
    ) -> Option<&FixedTextPlannedEntry> {
        self.entries.iter().find(|entry| {
            entry.table_id == table_id
                && (entry.source_index == source_index
                    || entry.alias_indices.contains(&source_index))
        })
    }

    pub(crate) fn unique_glyphs(&self) -> BTreeSet<char> {
        self.entries
            .iter()
            .flat_map(|entry| &entry.logical_bytes)
            .filter_map(|byte| match byte {
                FixedTextLogicalByte::TargetGlyph(glyph) => Some(*glyph),
                FixedTextLogicalByte::Encoded(_) => None,
            })
            .collect()
    }

    pub(crate) fn table_max_entry_glyph_count(&self, table_id: &str) -> usize {
        self.entries
            .iter()
            .filter(|entry| entry.table_id == table_id)
            .map(|entry| {
                entry
                    .logical_bytes
                    .iter()
                    .filter(|byte| matches!(byte, FixedTextLogicalByte::TargetGlyph(_)))
                    .count()
            })
            .max()
            .unwrap_or(0)
    }

    pub(crate) fn encoded_original_byte_count(&self) -> usize {
        self.entries
            .iter()
            .flat_map(|entry| &entry.logical_bytes)
            .filter_map(|byte| match byte {
                FixedTextLogicalByte::Encoded(value) => Some(*value),
                FixedTextLogicalByte::TargetGlyph(_) => None,
            })
            .count()
    }
}

pub(crate) fn plan_fixed_text(rom: &Rom, workspace_path: &Path) -> Result<FixedTextPlan> {
    rom.verify_supported_japanese()?;
    let bytes =
        fs::read(workspace_path).with_context(|| format!("read {}", workspace_path.display()))?;
    let workspace: FixedTextWorkspace = serde_json::from_slice(&bytes)
        .with_context(|| format!("parse {}", workspace_path.display()))?;
    let fresh = build_workspace(rom.data())?;
    ensure!(
        workspace.entries.len() == fresh.entries.len(),
        "fixed text workspace entry count changed"
    );
    for (entry, source) in workspace.entries.iter().zip(&fresh.entries) {
        ensure!(
            entry.id == source.id
                && entry.table_id == source.table_id
                && entry.source_index == source.source_index
                && entry.alias_indices == source.alias_indices
                && entry.pointer_cpu_address_hex == source.pointer_cpu_address_hex
                && entry.source_file_offset_hex == source.source_file_offset_hex
                && entry.source_bytes_hex == source.source_bytes_hex
                && entry.source_sha1 == source.source_sha1
                && entry.japanese_markup == source.japanese_markup,
            "fixed text workspace binding changed for {}",
            source.id
        );
        validate_translation(entry)?;
        ensure!(
            entry.status != "untranslated",
            "{} is not translated",
            entry.id
        );
    }
    let entries = workspace
        .entries
        .iter()
        .map(|entry| {
            let logical_bytes = encode_target_markup(&entry.korean_markup)
                .with_context(|| format!("encode {}", entry.id))?;
            let source_len = entry.source_bytes_hex.split_whitespace().count();
            ensure!(
                logical_bytes.len() <= source_len,
                "{} needs {} bytes but owns only {}",
                entry.id,
                logical_bytes.len(),
                source_len
            );
            let file_offset = if let Some(encoded) = &entry.source_file_offset_hex {
                usize::from_str_radix(encoded.trim_start_matches("0x"), 16)
                    .with_context(|| format!("decode source file offset for {}", entry.id))?
            } else {
                let pointer_cpu_address =
                    u16::from_str_radix(entry.pointer_cpu_address_hex.trim_start_matches("0x"), 16)
                        .with_context(|| format!("decode pointer for {}", entry.id))?;
                fixed_bank_file_offset(pointer_cpu_address)?
            };
            Ok(FixedTextPlannedEntry {
                id: entry.id.clone(),
                table_id: entry.table_id.clone(),
                source_index: entry.source_index,
                alias_indices: entry.alias_indices.clone(),
                file_offset,
                source_storage_byte_count: source_len,
                logical_bytes,
            })
        })
        .collect::<Result<Vec<_>>>()?;
    Ok(FixedTextPlan { entries })
}

pub(crate) fn extract_fixed_text_workspace(
    source_path: &Path,
    workspace_path: &Path,
) -> Result<FixedTextWorkspaceSummary> {
    let rom = Rom::from_path(source_path)?;
    rom.verify_supported_japanese()?;
    let mut fresh = build_workspace(rom.data())?;
    let preserved_translation_count = if workspace_path.exists() {
        let bytes = fs::read(workspace_path)
            .with_context(|| format!("read {}", workspace_path.display()))?;
        let existing: FixedTextWorkspace = serde_json::from_slice(&bytes)
            .with_context(|| format!("parse {}", workspace_path.display()))?;
        preserve_translations(&mut fresh, &existing)?
    } else {
        0
    };
    let japanese_entry_count = fresh
        .entries
        .iter()
        .filter(|entry| entry.japanese_markup.chars().any(is_japanese_character))
        .count();
    let mut bytes = serde_json::to_vec_pretty(&fresh).context("serialize fixed text workspace")?;
    bytes.push(b'\n');
    if let Some(parent) = workspace_path.parent() {
        fs::create_dir_all(parent).with_context(|| format!("create {}", parent.display()))?;
    }
    fs::write(workspace_path, &bytes)
        .with_context(|| format!("write {}", workspace_path.display()))?;
    Ok(FixedTextWorkspaceSummary {
        workspace_sha1: sha1_hex(&bytes),
        entry_count: fresh.entries.len(),
        japanese_entry_count,
        preserved_translation_count,
    })
}

fn build_workspace(source: &[u8]) -> Result<FixedTextWorkspace> {
    let mut entries = Vec::new();
    for spec in requested_text_table_specs(&TABLE_IDS)? {
        let table = extract_table(source, spec)?;
        for entry in &table.entries {
            if entry
                .alias_entry_indices
                .iter()
                .any(|alias| *alias < entry.index)
            {
                continue;
            }
            let raw = entry
                .raw_bytes_hex
                .split_whitespace()
                .map(|encoded| u8::from_str_radix(encoded, 16).context("decode source byte"))
                .collect::<Result<Vec<_>>>()?;
            entries.push(FixedTextEntry {
                id: format!("{}:{:03}", table.id, entry.index),
                table_id: table.id.to_owned(),
                source_index: entry.index,
                alias_indices: entry.alias_entry_indices.clone(),
                pointer_cpu_address_hex: entry.pointer_cpu_address_hex.clone(),
                source_file_offset_hex: None,
                source_bytes_hex: entry.raw_bytes_hex.clone(),
                source_sha1: entry.raw_sha1.clone(),
                japanese_markup: decode_source_markup(&raw),
                korean_markup: String::new(),
                status: "untranslated".to_owned(),
            });
        }
    }
    for template in extract_battle_message_templates(source)? {
        entries.push(FixedTextEntry {
            id: format!("battle-message-templates:{:03}", template.index),
            table_id: "battle-message-templates".to_owned(),
            source_index: template.index,
            alias_indices: vec![template.index],
            pointer_cpu_address_hex: format!("0x{:04X}", template.pointer_cpu_address),
            source_file_offset_hex: Some(format!("0x{:05X}", template.file_offset)),
            source_bytes_hex: template
                .raw_bytes
                .iter()
                .map(|byte| format!("{byte:02X}"))
                .collect::<Vec<_>>()
                .join(" "),
            source_sha1: sha1_hex(&template.raw_bytes),
            japanese_markup: decode_source_markup(&template.raw_bytes),
            korean_markup: String::new(),
            status: "untranslated".to_owned(),
        });
    }
    ensure!(
        entries.len() == 271,
        "battle fixed-text unique entry count changed"
    );
    Ok(FixedTextWorkspace {
        format_version: 1,
        source_sha1: EXPECTED_SOURCE_SHA1.to_owned(),
        translate_from: "ja".to_owned(),
        translate_to: "ko".to_owned(),
        preserve_existing_english: true,
        entries,
    })
}

fn preserve_translations(
    fresh: &mut FixedTextWorkspace,
    existing: &FixedTextWorkspace,
) -> Result<usize> {
    ensure!(
        existing.format_version == fresh.format_version
            && existing.source_sha1 == fresh.source_sha1
            && existing.translate_from == fresh.translate_from
            && existing.translate_to == fresh.translate_to
            && existing.preserve_existing_english == fresh.preserve_existing_english,
        "existing fixed text workspace scope changed"
    );
    let existing_by_id: BTreeMap<_, _> = existing
        .entries
        .iter()
        .map(|entry| (entry.id.as_str(), entry))
        .collect();
    ensure!(
        existing_by_id.len() == existing.entries.len(),
        "duplicate fixed text workspace ID"
    );
    let mut preserved = 0;
    for entry in &mut fresh.entries {
        if let Some(old) = existing_by_id.get(entry.id.as_str()) {
            ensure!(
                old.table_id == entry.table_id
                    && old.source_index == entry.source_index
                    && old.alias_indices == entry.alias_indices
                    && old.pointer_cpu_address_hex == entry.pointer_cpu_address_hex
                    && old.source_file_offset_hex == entry.source_file_offset_hex
                    && old.source_bytes_hex == entry.source_bytes_hex
                    && old.source_sha1 == entry.source_sha1
                    && old.japanese_markup == entry.japanese_markup,
                "fixed text source binding changed for {}",
                entry.id
            );
            validate_translation(old)?;
            entry.korean_markup.clone_from(&old.korean_markup);
            entry.status.clone_from(&old.status);
            preserved += usize::from(old.status != "untranslated");
        }
    }
    Ok(preserved)
}

fn validate_translation(entry: &FixedTextEntry) -> Result<()> {
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
    if !entry.korean_markup.is_empty() {
        ensure!(
            !entry.korean_markup.chars().any(is_japanese_character),
            "Korean fixed text still contains Japanese for {}",
            entry.id
        );
        let protected = entry
            .japanese_markup
            .chars()
            .filter(|c| c.is_ascii())
            .collect::<String>();
        let retained = entry
            .korean_markup
            .chars()
            .filter(|c| c.is_ascii())
            .collect::<String>();
        ensure!(
            protected == retained,
            "existing ASCII changed for {}",
            entry.id
        );
    }
    Ok(())
}

fn decode_source_markup(raw: &[u8]) -> String {
    raw.iter()
        .map(|code| {
            japanese_text_glyph(*code)
                .map(str::to_owned)
                .or_else(|| protected_alphanumeric_glyph(*code).map(str::to_owned))
                .or_else(|| (*code == 0x9B).then(|| ".".to_owned()))
                .unwrap_or_else(|| format!("{{{code:02X}}}"))
        })
        .collect()
}

fn is_japanese_character(character: char) -> bool {
    ('\u{3040}'..='\u{30ff}').contains(&character)
        || character == '。'
        || character == '「'
        || character == '」'
}

fn encode_target_markup(markup: &str) -> Result<Vec<FixedTextLogicalByte>> {
    let chars = markup.chars().collect::<Vec<_>>();
    let mut output = Vec::new();
    let mut index = 0;
    while index < chars.len() {
        if chars[index] == '{' {
            ensure!(
                index + 3 < chars.len() && chars[index + 3] == '}',
                "invalid fixed text token"
            );
            let encoded = format!("{}{}", chars[index + 1], chars[index + 2]);
            output.push(FixedTextLogicalByte::Encoded(
                u8::from_str_radix(&encoded, 16).context("decode fixed text token")?,
            ));
            index += 4;
        } else if ('가'..='힣').contains(&chars[index]) {
            output.push(FixedTextLogicalByte::TargetGlyph(chars[index]));
            index += 1;
        } else if chars[index].is_ascii() {
            let code = (0u8..=u8::MAX)
                .find(|code| protected_alphanumeric_glyph(*code) == Some(&chars[index].to_string()))
                .or_else(|| (chars[index] == '.').then_some(0x9B))
                .with_context(|| format!("unsupported preserved ASCII {:?}", chars[index]))?;
            output.push(FixedTextLogicalByte::Encoded(code));
            index += 1;
        } else {
            anyhow::bail!("unsupported fixed text character {:?}", chars[index]);
        }
    }
    Ok(output)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn preserves_existing_ascii_in_translation() {
        let mut entry = FixedTextEntry {
            id: "class-names:000".to_owned(),
            table_id: "class-names".to_owned(),
            source_index: 0,
            alias_indices: vec![],
            pointer_cpu_address_hex: "0x0000".to_owned(),
            source_file_offset_hex: None,
            source_bytes_hex: String::new(),
            source_sha1: String::new(),
            japanese_markup: "Sナイト".to_owned(),
            korean_markup: "S나이트".to_owned(),
            status: "needs_human_review".to_owned(),
        };
        validate_translation(&entry).unwrap();
        entry.korean_markup = "나이트".to_owned();
        assert!(validate_translation(&entry).is_err());
    }

    #[test]
    fn target_markup_keeps_layout_tokens_and_one_byte_hangul() {
        let encoded = encode_target_markup("활{FF}").unwrap();
        assert!(matches!(
            encoded[0],
            FixedTextLogicalByte::TargetGlyph('활')
        ));
        assert!(matches!(encoded[1], FixedTextLogicalByte::Encoded(0xFF)));
        assert_eq!(encoded.len(), 2);
    }

    #[test]
    fn target_markup_rejects_japanese_and_malformed_tokens() {
        assert!(encode_target_markup("ゆみ").is_err());
        assert!(encode_target_markup("활{F}").is_err());
    }
}
