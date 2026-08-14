use std::{
    collections::{BTreeMap, BTreeSet},
    fs,
    path::Path,
};

use anyhow::{Context, Result, ensure};
use serde::{Deserialize, Serialize};

use crate::{
    font_slots::active_hangul_codes,
    japanese_encoding::is_japanese_text_code,
    japanese_encoding::japanese_text_glyph,
    rom::{EXPECTED_SOURCE_SHA1, Rom},
    sha1_hex,
    text_inventory::{
        FixedTextLogicalByte, encode_target_markup, is_japanese_character,
        protected_alphanumeric_glyph,
    },
};

use super::{
    source_binding::source_file_offset,
    source_spec::{
        CHAPTER_TITLE_COUNT, CHAPTER_TITLE_DATA_END_EXCLUSIVE, CHAPTER_TITLE_DATA_START,
        CHAPTER_TITLE_POINTER_TABLE_ADDRESS, CHAPTER_TITLE_TERMINATOR,
    },
};

#[derive(Debug)]
pub(crate) struct ChapterTitleWorkspaceSummary {
    pub(crate) workspace_sha1: String,
    pub(crate) entry_count: usize,
    pub(crate) japanese_entry_count: usize,
    pub(crate) preserved_translation_count: usize,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
struct ChapterTitleWorkspace {
    format_version: u8,
    source_sha1: String,
    translate_from: String,
    translate_to: String,
    preserve_existing_english_and_digits: bool,
    entries: Vec<ChapterTitleEntry>,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
struct ChapterTitleEntry {
    id: String,
    chapter_index: u8,
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
pub(crate) struct ChapterTitlePlannedEntry {
    pub(crate) id: String,
    pub(crate) chapter_index: u8,
    pub(crate) file_offset: usize,
    pub(crate) source_storage_byte_count: usize,
    logical_bytes: Vec<FixedTextLogicalByte>,
}

impl ChapterTitlePlannedEntry {
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

    pub(crate) fn source_reclaimable_active_codes(&self, rom: &Rom) -> Result<BTreeSet<u8>> {
        let body_end = self
            .file_offset
            .checked_add(
                self.source_storage_byte_count
                    .checked_sub(1)
                    .context("chapter-title source storage has no terminator")?,
            )
            .context("chapter-title source body range overflow")?;
        let source_body = rom
            .data()
            .get(self.file_offset..body_end)
            .context("chapter-title source body is outside the ROM")?;
        let active_codes = active_hangul_codes().into_iter().collect::<BTreeSet<_>>();
        let reclaimable = source_body
            .iter()
            .copied()
            .filter(|code| is_japanese_text_code(*code) && active_codes.contains(code))
            .collect::<BTreeSet<_>>();
        ensure!(
            !reclaimable.is_empty(),
            "{} has no reclaimable Japanese title codes",
            self.id
        );
        Ok(reclaimable)
    }

    pub(crate) fn encoded_storage_bytes(
        &self,
        assignments: &BTreeMap<char, u8>,
    ) -> Result<Vec<u8>> {
        let body_capacity = self
            .source_storage_byte_count
            .checked_sub(1)
            .context("chapter-title storage has no terminator byte")?;
        let mut encoded = self
            .logical_bytes
            .iter()
            .map(|byte| match byte {
                FixedTextLogicalByte::Encoded(value) => Ok(*value),
                FixedTextLogicalByte::TargetGlyph(glyph) => {
                    assignments.get(glyph).copied().with_context(|| {
                        format!("missing chapter-title code assignment for {glyph:?}")
                    })
                }
            })
            .collect::<Result<Vec<_>>>()?;
        ensure!(
            encoded.len() <= body_capacity,
            "{} needs {} body bytes but owns only {body_capacity}",
            self.id,
            encoded.len()
        );
        encoded.resize(body_capacity, 0xFF);
        encoded.push(CHAPTER_TITLE_TERMINATOR);
        Ok(encoded)
    }
}

#[derive(Clone, Debug)]
pub(crate) struct ChapterTitlePlan {
    pub(crate) workspace_sha1: String,
    pub(crate) entry_count: usize,
    pub(crate) translated_entry_count: usize,
    pub(crate) review_complete: bool,
    pub(crate) entries: Vec<ChapterTitlePlannedEntry>,
}

impl ChapterTitlePlan {
    pub(crate) fn unique_glyphs(&self) -> BTreeSet<char> {
        self.entries
            .iter()
            .flat_map(ChapterTitlePlannedEntry::unique_glyphs)
            .collect()
    }

    pub(crate) fn entry(&self, chapter_index: u8) -> Result<&ChapterTitlePlannedEntry> {
        self.entries
            .iter()
            .find(|entry| entry.chapter_index == chapter_index)
            .with_context(|| format!("chapter-title plan has no chapter index {chapter_index}"))
    }
}

pub(crate) fn extract_chapter_title_workspace(
    source_path: &Path,
    workspace_path: &Path,
) -> Result<ChapterTitleWorkspaceSummary> {
    let rom = Rom::from_path(source_path)?;
    rom.verify_supported_japanese()?;
    let mut fresh = build_workspace(&rom)?;
    let preserved_translation_count = if workspace_path.exists() {
        let bytes = fs::read(workspace_path)
            .with_context(|| format!("read {}", workspace_path.display()))?;
        let existing: ChapterTitleWorkspace = serde_json::from_slice(&bytes)
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
    let mut bytes =
        serde_json::to_vec_pretty(&fresh).context("serialize chapter-title workspace")?;
    bytes.push(b'\n');
    if let Some(parent) = workspace_path.parent() {
        fs::create_dir_all(parent).with_context(|| format!("create {}", parent.display()))?;
    }
    fs::write(workspace_path, &bytes)
        .with_context(|| format!("write {}", workspace_path.display()))?;
    Ok(ChapterTitleWorkspaceSummary {
        workspace_sha1: sha1_hex(&bytes),
        entry_count: fresh.entries.len(),
        japanese_entry_count,
        preserved_translation_count,
    })
}

pub(crate) fn plan_chapter_titles(rom: &Rom, workspace_path: &Path) -> Result<ChapterTitlePlan> {
    rom.verify_supported_japanese()?;
    let bytes =
        fs::read(workspace_path).with_context(|| format!("read {}", workspace_path.display()))?;
    let workspace: ChapterTitleWorkspace = serde_json::from_slice(&bytes)
        .with_context(|| format!("parse {}", workspace_path.display()))?;
    let fresh = build_workspace(rom)?;
    validate_scope(&workspace, &fresh)?;
    ensure!(
        workspace.entries.len() == fresh.entries.len(),
        "chapter-title workspace entry count changed"
    );

    let mut entries = Vec::with_capacity(workspace.entries.len());
    let mut review_complete = true;
    for (entry, source) in workspace.entries.iter().zip(&fresh.entries) {
        validate_source_binding(entry, source)?;
        validate_translation(entry)?;
        ensure!(
            entry.status != "untranslated",
            "{} is not translated",
            entry.id
        );
        review_complete &= entry.status == "complete";
        let logical_bytes = encode_target_markup(&entry.korean_markup)
            .with_context(|| format!("encode {}", entry.id))?;
        let encoded_digits = logical_bytes
            .iter()
            .filter_map(|byte| match byte {
                FixedTextLogicalByte::Encoded(value) if (0x60..=0x69).contains(value) => {
                    Some(*value)
                }
                _ => None,
            })
            .collect::<Vec<_>>();
        let source_digits =
            source_digit_codes(&decode_hex(&entry.source_bytes_hex)?).collect::<Vec<_>>();
        ensure!(
            encoded_digits == source_digits,
            "original chapter-number digits changed for {}",
            entry.id
        );
        let body_capacity = entry
            .source_storage_byte_count
            .checked_sub(1)
            .context("chapter-title source storage has no terminator")?;
        ensure!(
            logical_bytes.len() <= body_capacity,
            "{} needs {} body bytes but owns only {body_capacity}",
            entry.id,
            logical_bytes.len()
        );
        entries.push(ChapterTitlePlannedEntry {
            id: entry.id.clone(),
            chapter_index: entry.chapter_index,
            file_offset: decode_usize_hex(&entry.source_file_offset_hex)?,
            source_storage_byte_count: entry.source_storage_byte_count,
            logical_bytes,
        });
    }

    Ok(ChapterTitlePlan {
        workspace_sha1: sha1_hex(&bytes),
        entry_count: entries.len(),
        translated_entry_count: entries.len(),
        review_complete,
        entries,
    })
}

fn build_workspace(rom: &Rom) -> Result<ChapterTitleWorkspace> {
    let pointer_table_file_offset = source_file_offset(0x0F, CHAPTER_TITLE_POINTER_TABLE_ADDRESS)?;
    let pointer_table = rom
        .data()
        .get(pointer_table_file_offset..pointer_table_file_offset + CHAPTER_TITLE_COUNT * 2)
        .context("chapter-title pointer table is outside the source")?;
    let pointers = pointer_table
        .chunks_exact(2)
        .map(|bytes| u16::from_le_bytes([bytes[0], bytes[1]]))
        .collect::<Vec<_>>();
    ensure!(
        pointers.len() == CHAPTER_TITLE_COUNT,
        "chapter-title pointer count changed"
    );

    let mut entries = Vec::with_capacity(CHAPTER_TITLE_COUNT);
    for (chapter_index, pointer) in pointers.into_iter().enumerate() {
        let file_offset = source_file_offset(0x0F, pointer)?;
        let next_file_offset = if chapter_index + 1 == CHAPTER_TITLE_COUNT {
            CHAPTER_TITLE_DATA_END_EXCLUSIVE
        } else {
            source_file_offset(
                0x0F,
                u16::from_le_bytes([
                    pointer_table[(chapter_index + 1) * 2],
                    pointer_table[(chapter_index + 1) * 2 + 1],
                ]),
            )?
        };
        ensure!(
            (CHAPTER_TITLE_DATA_START..CHAPTER_TITLE_DATA_END_EXCLUSIVE).contains(&file_offset)
                && file_offset < next_file_offset,
            "chapter-title entry {chapter_index} source range changed"
        );
        let raw = rom
            .data()
            .get(file_offset..next_file_offset)
            .context("chapter-title entry is outside the source")?;
        ensure!(
            raw.last() == Some(&CHAPTER_TITLE_TERMINATOR)
                && !raw[..raw.len() - 1].contains(&CHAPTER_TITLE_TERMINATOR),
            "chapter-title entry {chapter_index} terminator changed"
        );
        entries.push(ChapterTitleEntry {
            id: format!("chapter-title:{:03}", chapter_index + 1),
            chapter_index: u8::try_from(chapter_index).context("chapter index does not fit u8")?,
            pointer_cpu_address_hex: format!("0x{pointer:04X}"),
            source_file_offset_hex: format!("0x{file_offset:05X}"),
            source_storage_byte_count: raw.len(),
            source_bytes_hex: encode_hex(raw),
            source_sha1: sha1_hex(raw),
            japanese_markup: decode_source_markup(&raw[..raw.len() - 1]),
            korean_markup: String::new(),
            status: "untranslated".to_owned(),
        });
    }
    ensure!(
        entries.len() == CHAPTER_TITLE_COUNT,
        "chapter-title entry count changed"
    );

    Ok(ChapterTitleWorkspace {
        format_version: 1,
        source_sha1: EXPECTED_SOURCE_SHA1.to_owned(),
        translate_from: "ja".to_owned(),
        translate_to: "ko".to_owned(),
        preserve_existing_english_and_digits: true,
        entries,
    })
}

fn preserve_translations(
    fresh: &mut ChapterTitleWorkspace,
    existing: &ChapterTitleWorkspace,
) -> Result<usize> {
    validate_scope(existing, fresh)?;
    let existing_by_id = existing
        .entries
        .iter()
        .map(|entry| (entry.id.as_str(), entry))
        .collect::<BTreeMap<_, _>>();
    ensure!(
        existing_by_id.len() == existing.entries.len(),
        "duplicate chapter-title workspace ID"
    );
    let mut preserved = 0;
    for entry in &mut fresh.entries {
        if let Some(old) = existing_by_id.get(entry.id.as_str()) {
            validate_source_binding(old, entry)?;
            validate_translation(old)?;
            entry.korean_markup.clone_from(&old.korean_markup);
            entry.status.clone_from(&old.status);
            preserved += usize::from(old.status != "untranslated");
        }
    }
    Ok(preserved)
}

fn validate_scope(actual: &ChapterTitleWorkspace, expected: &ChapterTitleWorkspace) -> Result<()> {
    ensure!(
        actual.format_version == expected.format_version
            && actual.source_sha1 == expected.source_sha1
            && actual.translate_from == expected.translate_from
            && actual.translate_to == expected.translate_to
            && actual.preserve_existing_english_and_digits
                == expected.preserve_existing_english_and_digits,
        "chapter-title workspace scope changed"
    );
    Ok(())
}

fn validate_source_binding(actual: &ChapterTitleEntry, expected: &ChapterTitleEntry) -> Result<()> {
    ensure!(
        actual.id == expected.id
            && actual.chapter_index == expected.chapter_index
            && actual.pointer_cpu_address_hex == expected.pointer_cpu_address_hex
            && actual.source_file_offset_hex == expected.source_file_offset_hex
            && actual.source_storage_byte_count == expected.source_storage_byte_count
            && actual.source_bytes_hex == expected.source_bytes_hex
            && actual.source_sha1 == expected.source_sha1
            && actual.japanese_markup == expected.japanese_markup,
        "chapter-title source binding changed for {}",
        expected.id
    );
    Ok(())
}

fn validate_translation(entry: &ChapterTitleEntry) -> Result<()> {
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
        "Korean chapter title still contains Japanese for {}",
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

fn source_digit_codes(raw: &[u8]) -> impl Iterator<Item = u8> + '_ {
    raw.iter()
        .copied()
        .filter(|byte| (0x60..=0x69).contains(byte))
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
        .map(|byte| u8::from_str_radix(byte, 16).context("decode chapter-title source byte"))
        .collect()
}

fn decode_usize_hex(encoded: &str) -> Result<usize> {
    usize::from_str_radix(encoded.trim_start_matches("0x"), 16)
        .context("decode chapter-title file offset")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn encoded_title_pads_owned_storage_and_restores_terminator() {
        let entry = ChapterTitlePlannedEntry {
            id: "chapter-title:001".to_owned(),
            chapter_index: 0,
            file_offset: 0,
            source_storage_byte_count: 6,
            logical_bytes: vec![
                FixedTextLogicalByte::Encoded(0x61),
                FixedTextLogicalByte::TargetGlyph('장'),
            ],
        };
        let assignments = BTreeMap::from([('장', 0xC0)]);

        assert_eq!(
            entry.encoded_storage_bytes(&assignments).unwrap(),
            vec![0x61, 0xC0, 0xFF, 0xFF, 0xFF, CHAPTER_TITLE_TERMINATOR]
        );
    }

    #[test]
    fn encoded_title_rejects_overflow() {
        let entry = ChapterTitlePlannedEntry {
            id: "chapter-title:001".to_owned(),
            chapter_index: 0,
            file_offset: 0,
            source_storage_byte_count: 2,
            logical_bytes: vec![
                FixedTextLogicalByte::Encoded(0x61),
                FixedTextLogicalByte::TargetGlyph('장'),
            ],
        };
        let assignments = BTreeMap::from([('장', 0xC0)]);

        assert!(entry.encoded_storage_bytes(&assignments).is_err());
    }
}
