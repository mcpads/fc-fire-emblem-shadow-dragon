use std::{collections::BTreeMap, fs, path::Path};

use anyhow::{Context, Result, ensure};
use serde::{Deserialize, Serialize};

use super::*;
use crate::{
    japanese_encoding::japanese_text_glyph,
    rom::{EXPECTED_SOURCE_SHA1, Rom},
};

const TABLE_IDS: [&str; 4] = ["class-names", "item-names", "unit-names", "enemy-names"];

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
    source_bytes_hex: String,
    source_sha1: String,
    japanese_markup: String,
    korean_markup: String,
    status: String,
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
                source_bytes_hex: entry.raw_bytes_hex.clone(),
                source_sha1: entry.raw_sha1.clone(),
                japanese_markup: decode_source_markup(&raw),
                korean_markup: String::new(),
                status: "untranslated".to_owned(),
            });
        }
    }
    ensure!(
        entries.len() == 233,
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
}
