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
    rom::{EXPECTED_SOURCE_SHA1, Rom},
    sha1_hex,
    text_inventory::{FixedTextLogicalByte, encode_target_markup, is_japanese_character},
};

use super::source::{
    ClassProfileSourceEntry, DESCRIPTION_LINE_BREAK, DESCRIPTION_TERMINATOR, PROFILE_COUNT,
    TITLE_TERMINATOR, extract_source_entries,
};

const MAXIMUM_VISIBLE_LINE_CELLS: usize = 28;
pub(crate) const PROFILE_PAGE_SPLIT_INDEX: usize = 11;

#[derive(Debug)]
pub(crate) struct ClassProfileWorkspaceSummary {
    pub(crate) workspace_sha1: String,
    pub(crate) entry_count: usize,
    pub(crate) description_line_count: usize,
    pub(crate) preserved_translation_count: usize,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
struct ClassProfileWorkspace {
    format_version: u8,
    source_sha1: String,
    translate_from: String,
    translate_to: String,
    preserve_existing_english_and_digits: bool,
    entries: Vec<ClassProfileEntry>,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
struct ClassProfileEntry {
    id: String,
    profile_index: usize,
    title_pointer_cpu_address_hex: String,
    title_source_file_offset_hex: String,
    title_source_storage_byte_count: usize,
    title_source_bytes_hex: String,
    title_source_sha1: String,
    japanese_title_markup: String,
    korean_title_markup: String,
    description_pointer_cpu_address_hex: String,
    description_source_file_offset_hex: String,
    description_source_storage_byte_count: usize,
    description_source_bytes_hex: String,
    description_source_sha1: String,
    japanese_description_lines: Vec<String>,
    korean_description_lines: Vec<String>,
    status: String,
}

#[derive(Clone, Debug)]
pub(crate) struct ClassProfilePlannedEntry {
    pub(crate) id: String,
    pub(crate) profile_index: usize,
    pub(crate) title_file_offset: usize,
    pub(crate) title_source_storage_byte_count: usize,
    title_logical_bytes: Vec<FixedTextLogicalByte>,
    pub(crate) description_file_offset: usize,
    pub(crate) description_source_storage_byte_count: usize,
    description_line_capacities: Vec<usize>,
    description_line_logical_bytes: Vec<Vec<FixedTextLogicalByte>>,
}

impl ClassProfilePlannedEntry {
    pub(crate) fn unique_glyphs(&self) -> BTreeSet<char> {
        self.title_logical_bytes
            .iter()
            .chain(self.description_line_logical_bytes.iter().flatten())
            .filter_map(|byte| match byte {
                FixedTextLogicalByte::TargetGlyph(glyph) => Some(*glyph),
                FixedTextLogicalByte::Encoded(_) => None,
            })
            .collect()
    }

    pub(crate) fn encoded_codes(&self) -> BTreeSet<u8> {
        self.title_logical_bytes
            .iter()
            .chain(self.description_line_logical_bytes.iter().flatten())
            .filter_map(|byte| match byte {
                FixedTextLogicalByte::Encoded(code) => Some(*code),
                FixedTextLogicalByte::TargetGlyph(_) => None,
            })
            .collect()
    }

    pub(crate) fn description_line_count(&self) -> usize {
        self.description_line_logical_bytes.len()
    }

    pub(crate) fn encoded_title_storage(
        &self,
        assignments: &BTreeMap<char, u8>,
    ) -> Result<Vec<u8>> {
        encode_padded_storage(
            &self.id,
            &self.title_logical_bytes,
            self.title_source_storage_byte_count,
            TITLE_TERMINATOR,
            assignments,
        )
    }

    pub(crate) fn encoded_description_storage(
        &self,
        assignments: &BTreeMap<char, u8>,
    ) -> Result<Vec<u8>> {
        ensure!(
            self.description_line_capacities.len() == self.description_line_logical_bytes.len(),
            "{} description line plan changed",
            self.id
        );
        let mut encoded = Vec::with_capacity(self.description_source_storage_byte_count);
        for (line_index, (logical, capacity)) in self
            .description_line_logical_bytes
            .iter()
            .zip(&self.description_line_capacities)
            .enumerate()
        {
            let mut line = encode_logical_bytes(logical, assignments)
                .with_context(|| format!("encode {} description line {line_index}", self.id))?;
            ensure!(
                line.len() <= *capacity,
                "{} description line {line_index} needs {} cells but owns only {capacity}",
                self.id,
                line.len()
            );
            line.resize(*capacity, 0xFF);
            encoded.extend(line);
            encoded.push(DESCRIPTION_LINE_BREAK);
        }
        encoded.push(DESCRIPTION_TERMINATOR);
        ensure!(
            encoded.len() == self.description_source_storage_byte_count,
            "{} encoded description storage size changed",
            self.id
        );
        Ok(encoded)
    }
}

#[derive(Clone, Debug)]
pub(crate) struct ClassProfilePlan {
    pub(crate) workspace_sha1: String,
    pub(crate) review_complete: bool,
    pub(crate) entries: Vec<ClassProfilePlannedEntry>,
}

impl ClassProfilePlan {
    pub(crate) fn unique_glyphs(&self) -> BTreeSet<char> {
        self.entries
            .iter()
            .flat_map(ClassProfilePlannedEntry::unique_glyphs)
            .collect()
    }

    pub(crate) fn encoded_codes(&self) -> BTreeSet<u8> {
        self.entries
            .iter()
            .flat_map(ClassProfilePlannedEntry::encoded_codes)
            .collect()
    }

    pub(crate) fn description_line_count(&self) -> usize {
        self.entries
            .iter()
            .map(ClassProfilePlannedEntry::description_line_count)
            .sum()
    }

    /// Reconstructs the two installed profile-page codebooks from the exact
    /// title and description storage in a later cumulative artifact.
    ///
    /// The profile renderer changes pages at index 11, so the same byte may
    /// name different Hangul glyphs on the two sides of that boundary. Within
    /// either page, however, the mapping must remain injective and every
    /// protected byte, padding cell, line break, and terminator must still
    /// match the source-owned storage contract.
    pub(crate) fn bind_installed_glyph_codes(
        &self,
        candidate: &[u8],
    ) -> Result<[BTreeMap<char, u8>; 2]> {
        let active_codes = active_hangul_codes().into_iter().collect::<BTreeSet<_>>();
        let mut assignments = [BTreeMap::<char, u8>::new(), BTreeMap::<char, u8>::new()];
        let mut glyphs_by_code = [BTreeMap::<u8, char>::new(), BTreeMap::<u8, char>::new()];

        ensure!(
            self.entries.len() == super::source::PROFILE_COUNT
                && self
                    .entries
                    .iter()
                    .enumerate()
                    .all(|(index, entry)| entry.profile_index == index),
            "class-profile installed-code binding lost the ordered profile population"
        );

        for entry in &self.entries {
            let page_index = usize::from(entry.profile_index >= PROFILE_PAGE_SPLIT_INDEX);
            let title = candidate
                .get(
                    entry.title_file_offset
                        ..entry.title_file_offset + entry.title_source_storage_byte_count,
                )
                .with_context(|| format!("{} title storage is outside the candidate", entry.id))?;
            bind_installed_logical_bytes(
                &entry.id,
                "title",
                &entry.title_logical_bytes,
                &title[..title.len() - 1],
                &active_codes,
                &mut assignments[page_index],
                &mut glyphs_by_code[page_index],
            )?;
            ensure!(
                title[entry.title_logical_bytes.len()..title.len() - 1]
                    .iter()
                    .all(|byte| *byte == 0xFF)
                    && title.last() == Some(&TITLE_TERMINATOR),
                "{} installed title padding or terminator changed",
                entry.id
            );

            let description = candidate
                .get(
                    entry.description_file_offset
                        ..entry.description_file_offset
                            + entry.description_source_storage_byte_count,
                )
                .with_context(|| {
                    format!("{} description storage is outside the candidate", entry.id)
                })?;
            let mut cursor = 0;
            for (line_index, (logical, capacity)) in entry
                .description_line_logical_bytes
                .iter()
                .zip(&entry.description_line_capacities)
                .enumerate()
            {
                let body = description
                    .get(cursor..cursor + *capacity)
                    .with_context(|| {
                        format!("{} description line {line_index} exceeds storage", entry.id)
                    })?;
                bind_installed_logical_bytes(
                    &entry.id,
                    "description",
                    logical,
                    body,
                    &active_codes,
                    &mut assignments[page_index],
                    &mut glyphs_by_code[page_index],
                )?;
                ensure!(
                    body[logical.len()..].iter().all(|byte| *byte == 0xFF),
                    "{} installed description line {line_index} padding changed",
                    entry.id
                );
                cursor += *capacity;
                ensure!(
                    description.get(cursor) == Some(&DESCRIPTION_LINE_BREAK),
                    "{} installed description line {line_index} break changed",
                    entry.id
                );
                cursor += 1;
            }
            ensure!(
                cursor + 1 == description.len()
                    && description.get(cursor) == Some(&DESCRIPTION_TERMINATOR),
                "{} installed description terminator changed",
                entry.id
            );
        }

        for (page_index, expected) in [
            self.entries[..PROFILE_PAGE_SPLIT_INDEX]
                .iter()
                .flat_map(ClassProfilePlannedEntry::unique_glyphs)
                .collect::<BTreeSet<_>>(),
            self.entries[PROFILE_PAGE_SPLIT_INDEX..]
                .iter()
                .flat_map(ClassProfilePlannedEntry::unique_glyphs)
                .collect::<BTreeSet<_>>(),
        ]
        .into_iter()
        .enumerate()
        {
            ensure!(
                assignments[page_index]
                    .keys()
                    .copied()
                    .collect::<BTreeSet<_>>()
                    == expected,
                "class-profile installed code binding lost a page-{page_index} glyph"
            );
        }
        Ok(assignments)
    }
}

fn bind_installed_logical_bytes(
    id: &str,
    role: &str,
    logical: &[FixedTextLogicalByte],
    installed: &[u8],
    active_codes: &BTreeSet<u8>,
    assignments: &mut BTreeMap<char, u8>,
    glyphs_by_code: &mut BTreeMap<u8, char>,
) -> Result<()> {
    ensure!(
        logical.len() <= installed.len(),
        "{id} installed {role} storage is shorter than its logical text"
    );
    for (offset, byte) in logical.iter().enumerate() {
        let actual = installed[offset];
        match byte {
            FixedTextLogicalByte::Encoded(expected) => ensure!(
                actual == *expected,
                "{id} installed {role} protected byte changed at offset {offset}"
            ),
            FixedTextLogicalByte::TargetGlyph(glyph) => {
                ensure!(
                    active_codes.contains(&actual),
                    "{id} installed {role} glyph {glyph:?} uses reserved code {actual:02X}"
                );
                if let Some(previous) = assignments.insert(*glyph, actual) {
                    ensure!(
                        previous == actual,
                        "class-profile glyph {glyph:?} has two installed codes on one page"
                    );
                }
                if let Some(previous) = glyphs_by_code.insert(actual, *glyph) {
                    ensure!(
                        previous == *glyph,
                        "class-profile installed code {actual:02X} names two glyphs on one page"
                    );
                }
            }
        }
    }
    Ok(())
}

pub(crate) fn extract_class_profile_workspace(
    source_path: &Path,
    workspace_path: &Path,
) -> Result<ClassProfileWorkspaceSummary> {
    let rom = Rom::from_path(source_path)?;
    rom.verify_supported_japanese()?;
    let mut fresh = build_workspace(&rom)?;
    let preserved_translation_count = if workspace_path.exists() {
        let bytes = fs::read(workspace_path)
            .with_context(|| format!("read {}", workspace_path.display()))?;
        let existing: ClassProfileWorkspace = serde_json::from_slice(&bytes)
            .with_context(|| format!("parse {}", workspace_path.display()))?;
        preserve_translations(&mut fresh, &existing)?
    } else {
        0
    };
    let description_line_count = fresh
        .entries
        .iter()
        .map(|entry| entry.japanese_description_lines.len())
        .sum();
    let mut bytes =
        serde_json::to_vec_pretty(&fresh).context("serialize class-profile workspace")?;
    bytes.push(b'\n');
    if let Some(parent) = workspace_path.parent() {
        fs::create_dir_all(parent).with_context(|| format!("create {}", parent.display()))?;
    }
    fs::write(workspace_path, &bytes)
        .with_context(|| format!("write {}", workspace_path.display()))?;
    Ok(ClassProfileWorkspaceSummary {
        workspace_sha1: sha1_hex(&bytes),
        entry_count: fresh.entries.len(),
        description_line_count,
        preserved_translation_count,
    })
}

pub(crate) fn plan_class_profiles(rom: &Rom, workspace_path: &Path) -> Result<ClassProfilePlan> {
    rom.verify_supported_japanese()?;
    let bytes =
        fs::read(workspace_path).with_context(|| format!("read {}", workspace_path.display()))?;
    let workspace: ClassProfileWorkspace = serde_json::from_slice(&bytes)
        .with_context(|| format!("parse {}", workspace_path.display()))?;
    let fresh = build_workspace(rom)?;
    validate_scope(&workspace, &fresh)?;
    ensure!(
        workspace.entries.len() == PROFILE_COUNT && workspace.entries.len() == fresh.entries.len(),
        "class-profile workspace entry count changed"
    );

    let mut review_complete = true;
    let mut entries = Vec::with_capacity(PROFILE_COUNT);
    for (entry, source) in workspace.entries.iter().zip(&fresh.entries) {
        validate_source_binding(entry, source)?;
        validate_translation(entry)?;
        ensure!(
            entry.status != "untranslated",
            "{} is not translated",
            entry.id
        );
        review_complete &= entry.status == "complete";

        let title_logical_bytes = encode_target_markup(&entry.korean_title_markup)
            .with_context(|| format!("encode {} title", entry.id))?;
        ensure!(
            title_logical_bytes.len() <= MAXIMUM_VISIBLE_LINE_CELLS,
            "{} title exceeds the visible row",
            entry.id
        );
        validate_protected_codes(
            &decode_hex(&entry.title_source_bytes_hex)?,
            &title_logical_bytes,
            &entry.id,
            "title",
        )?;
        ensure!(
            title_logical_bytes.len() < entry.title_source_storage_byte_count,
            "{} title exceeds its owned storage",
            entry.id
        );

        let description_source = decode_hex(&entry.description_source_bytes_hex)?;
        let description_line_capacities = description_line_capacities(&description_source)?;
        ensure!(
            entry.korean_description_lines.len() == description_line_capacities.len(),
            "{} must preserve the source description line count",
            entry.id
        );
        let description_line_logical_bytes = entry
            .korean_description_lines
            .iter()
            .enumerate()
            .map(|(line_index, markup)| {
                let logical = encode_target_markup(markup)
                    .with_context(|| format!("encode {} line {line_index}", entry.id))?;
                ensure!(
                    logical.len() <= MAXIMUM_VISIBLE_LINE_CELLS,
                    "{} description line {line_index} exceeds the visible row",
                    entry.id
                );
                ensure!(
                    logical.len() <= description_line_capacities[line_index],
                    "{} description line {line_index} exceeds its source-owned row",
                    entry.id
                );
                Ok(logical)
            })
            .collect::<Result<Vec<_>>>()?;
        let flattened_description = description_line_logical_bytes
            .iter()
            .enumerate()
            .flat_map(|(index, line)| {
                line.iter().cloned().chain(
                    (index + 1 < description_line_capacities.len())
                        .then_some(FixedTextLogicalByte::Encoded(DESCRIPTION_LINE_BREAK)),
                )
            })
            .collect::<Vec<_>>();
        validate_protected_codes(
            &description_source,
            &flattened_description,
            &entry.id,
            "description",
        )?;

        entries.push(ClassProfilePlannedEntry {
            id: entry.id.clone(),
            profile_index: entry.profile_index,
            title_file_offset: decode_usize_hex(&entry.title_source_file_offset_hex)?,
            title_source_storage_byte_count: entry.title_source_storage_byte_count,
            title_logical_bytes,
            description_file_offset: decode_usize_hex(&entry.description_source_file_offset_hex)?,
            description_source_storage_byte_count: entry.description_source_storage_byte_count,
            description_line_capacities,
            description_line_logical_bytes,
        });
    }

    Ok(ClassProfilePlan {
        workspace_sha1: sha1_hex(&bytes),
        review_complete,
        entries,
    })
}

fn build_workspace(rom: &Rom) -> Result<ClassProfileWorkspace> {
    let entries = extract_source_entries(rom)?
        .into_iter()
        .map(workspace_entry)
        .collect();
    Ok(ClassProfileWorkspace {
        format_version: 1,
        source_sha1: EXPECTED_SOURCE_SHA1.to_owned(),
        translate_from: "ja".to_owned(),
        translate_to: "ko".to_owned(),
        preserve_existing_english_and_digits: true,
        entries,
    })
}

fn workspace_entry(source: ClassProfileSourceEntry) -> ClassProfileEntry {
    ClassProfileEntry {
        id: format!("class-profile:{:02}", source.index),
        profile_index: source.index,
        title_pointer_cpu_address_hex: format!("0x{:04X}", source.title_pointer),
        title_source_file_offset_hex: format!("0x{:05X}", source.title_file_offset),
        title_source_storage_byte_count: source.title_storage_byte_count,
        title_source_bytes_hex: encode_hex(&source.title_bytes),
        title_source_sha1: sha1_hex(&source.title_bytes),
        japanese_title_markup: source.title_markup,
        korean_title_markup: String::new(),
        description_pointer_cpu_address_hex: format!("0x{:04X}", source.description_pointer),
        description_source_file_offset_hex: format!("0x{:05X}", source.description_file_offset),
        description_source_storage_byte_count: source.description_storage_byte_count,
        description_source_bytes_hex: encode_hex(&source.description_bytes),
        description_source_sha1: sha1_hex(&source.description_bytes),
        japanese_description_lines: source.description_lines,
        korean_description_lines: Vec::new(),
        status: "untranslated".to_owned(),
    }
}

fn preserve_translations(
    fresh: &mut ClassProfileWorkspace,
    existing: &ClassProfileWorkspace,
) -> Result<usize> {
    validate_scope(existing, fresh)?;
    let existing_by_id = existing
        .entries
        .iter()
        .map(|entry| (entry.id.as_str(), entry))
        .collect::<BTreeMap<_, _>>();
    ensure!(
        existing_by_id.len() == existing.entries.len(),
        "duplicate class-profile workspace ID"
    );
    let mut preserved = 0;
    for entry in &mut fresh.entries {
        if let Some(old) = existing_by_id.get(entry.id.as_str()) {
            validate_source_binding(old, entry)?;
            validate_translation(old)?;
            entry
                .korean_title_markup
                .clone_from(&old.korean_title_markup);
            entry
                .korean_description_lines
                .clone_from(&old.korean_description_lines);
            entry.status.clone_from(&old.status);
            preserved += usize::from(old.status != "untranslated");
        }
    }
    Ok(preserved)
}

fn validate_scope(actual: &ClassProfileWorkspace, expected: &ClassProfileWorkspace) -> Result<()> {
    ensure!(
        actual.format_version == expected.format_version
            && actual.source_sha1 == expected.source_sha1
            && actual.translate_from == expected.translate_from
            && actual.translate_to == expected.translate_to
            && actual.preserve_existing_english_and_digits
                == expected.preserve_existing_english_and_digits,
        "class-profile workspace scope changed"
    );
    Ok(())
}

fn validate_source_binding(actual: &ClassProfileEntry, expected: &ClassProfileEntry) -> Result<()> {
    ensure!(
        actual.id == expected.id
            && actual.profile_index == expected.profile_index
            && actual.title_pointer_cpu_address_hex == expected.title_pointer_cpu_address_hex
            && actual.title_source_file_offset_hex == expected.title_source_file_offset_hex
            && actual.title_source_storage_byte_count == expected.title_source_storage_byte_count
            && actual.title_source_bytes_hex == expected.title_source_bytes_hex
            && actual.title_source_sha1 == expected.title_source_sha1
            && actual.japanese_title_markup == expected.japanese_title_markup
            && actual.description_pointer_cpu_address_hex
                == expected.description_pointer_cpu_address_hex
            && actual.description_source_file_offset_hex
                == expected.description_source_file_offset_hex
            && actual.description_source_storage_byte_count
                == expected.description_source_storage_byte_count
            && actual.description_source_bytes_hex == expected.description_source_bytes_hex
            && actual.description_source_sha1 == expected.description_source_sha1
            && actual.japanese_description_lines == expected.japanese_description_lines,
        "class-profile source binding changed for {}",
        expected.id
    );
    Ok(())
}

fn validate_translation(entry: &ClassProfileEntry) -> Result<()> {
    ensure!(
        ["untranslated", "needs_human_review", "complete"].contains(&entry.status.as_str()),
        "invalid status for {}",
        entry.id
    );
    let empty = entry.korean_title_markup.is_empty() && entry.korean_description_lines.is_empty();
    ensure!(
        (entry.status == "untranslated") == empty,
        "translation status and content disagree for {}",
        entry.id
    );
    ensure!(
        !entry
            .korean_title_markup
            .chars()
            .chain(
                entry
                    .korean_description_lines
                    .iter()
                    .flat_map(|line| line.chars())
            )
            .any(is_japanese_character),
        "Korean class-profile text still contains Japanese for {}",
        entry.id
    );
    Ok(())
}

fn validate_protected_codes(
    source: &[u8],
    target: &[FixedTextLogicalByte],
    id: &str,
    role: &str,
) -> Result<()> {
    let source = source
        .iter()
        .copied()
        .filter(|code| is_protected_source_code(*code))
        .collect::<Vec<_>>();
    let target = target
        .iter()
        .filter_map(|byte| match byte {
            FixedTextLogicalByte::Encoded(code) if is_protected_target_code(*code) => Some(*code),
            _ => None,
        })
        .collect::<Vec<_>>();
    ensure!(
        source == target,
        "protected original codes changed for {id} {role}"
    );
    Ok(())
}

fn is_protected_source_code(code: u8) -> bool {
    matches!(code, 0x4F | 0x8E | 0x8F)
        || (!is_japanese_text_code(code)
            && !matches!(code, 0xFF | TITLE_TERMINATOR | DESCRIPTION_TERMINATOR))
}

fn is_protected_target_code(code: u8) -> bool {
    !matches!(code, 0xFF | TITLE_TERMINATOR | DESCRIPTION_TERMINATOR)
}

fn description_line_capacities(source: &[u8]) -> Result<Vec<usize>> {
    ensure!(
        source.last() == Some(&DESCRIPTION_TERMINATOR),
        "class-profile description lost its record terminator"
    );
    let body = &source[..source.len() - 1];
    ensure!(
        body.last() == Some(&DESCRIPTION_LINE_BREAK),
        "class-profile description lost its final line terminator"
    );
    let capacities = body[..body.len() - 1]
        .split(|byte| *byte == DESCRIPTION_LINE_BREAK)
        .map(<[u8]>::len)
        .collect::<Vec<_>>();
    ensure!(
        (1..=4).contains(&capacities.len()),
        "class-profile description line count changed"
    );
    Ok(capacities)
}

fn encode_padded_storage(
    id: &str,
    logical: &[FixedTextLogicalByte],
    storage_byte_count: usize,
    terminator: u8,
    assignments: &BTreeMap<char, u8>,
) -> Result<Vec<u8>> {
    let capacity = storage_byte_count
        .checked_sub(1)
        .context("class-profile storage has no terminator")?;
    let mut encoded = encode_logical_bytes(logical, assignments)?;
    ensure!(
        encoded.len() <= capacity,
        "{id} needs {} cells but owns only {capacity}",
        encoded.len()
    );
    encoded.resize(capacity, 0xFF);
    encoded.push(terminator);
    Ok(encoded)
}

fn encode_logical_bytes(
    logical: &[FixedTextLogicalByte],
    assignments: &BTreeMap<char, u8>,
) -> Result<Vec<u8>> {
    logical
        .iter()
        .map(|byte| match byte {
            FixedTextLogicalByte::Encoded(value) => Ok(*value),
            FixedTextLogicalByte::TargetGlyph(glyph) => assignments
                .get(glyph)
                .copied()
                .with_context(|| format!("missing class-profile glyph assignment for {glyph:?}")),
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
        .map(|byte| u8::from_str_radix(byte, 16).context("decode class-profile source byte"))
        .collect()
}

fn decode_usize_hex(encoded: &str) -> Result<usize> {
    usize::from_str_radix(encoded.trim_start_matches("0x"), 16)
        .context("decode class-profile file offset")
}
