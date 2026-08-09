use std::collections::BTreeMap;

use super::*;

#[cfg(test)]
mod tests;

pub(super) fn validate_workspace_translations(
    workspace: &MainDialogueWorkspace,
) -> Result<WorkspaceTranslationCounts> {
    let mut counts = WorkspaceTranslationCounts {
        filled_line_count: 0,
        complete_line_count: 0,
        target_glyph_count: 0,
    };
    for record in &workspace.records {
        for line in &record.lines {
            match line.status {
                TranslationStatus::Untranslated => ensure!(
                    line.korean.is_empty(),
                    "{} is untranslated but its korean field is not empty",
                    line.id
                ),
                _ => {
                    ensure!(
                        !line.korean.is_empty(),
                        "{} has status other than untranslated but its korean field is empty",
                        line.id
                    );
                    counts.filled_line_count += 1;
                    if line.status == TranslationStatus::Complete {
                        counts.complete_line_count += 1;
                    }
                    counts.target_glyph_count += validate_translation_markup(line)?;
                }
            }
        }
    }
    Ok(counts)
}

pub(super) fn preserve_workspace_translations(
    fresh: &mut MainDialogueWorkspace,
    existing: &MainDialogueWorkspace,
) -> Result<usize> {
    ensure!(
        existing.format_version <= WORKSPACE_FORMAT_VERSION,
        "existing main dialogue workspace format is newer than this tool"
    );
    ensure!(
        existing.source_sha1 == fresh.source_sha1
            && existing.translate_from == fresh.translate_from
            && existing.translate_to == fresh.translate_to
            && existing.preserve_existing_english == fresh.preserve_existing_english
            && existing.purpose == fresh.purpose,
        "existing main dialogue workspace translation scope changed"
    );
    validate_workspace_translations(existing)?;

    let mut existing_lines = BTreeMap::new();
    for record in &existing.records {
        for line in &record.lines {
            ensure!(
                existing_lines.insert(line.id.as_str(), line).is_none(),
                "existing main dialogue workspace has duplicate line ID {}",
                line.id
            );
        }
    }

    let mut merged = fresh.clone();
    let mut fresh_line_ids = BTreeSet::new();
    let mut preserved_count = 0;
    for record in &mut merged.records {
        for line in &mut record.lines {
            ensure!(
                fresh_line_ids.insert(line.id.clone()),
                "fresh main dialogue workspace has duplicate line ID {}",
                line.id
            );
            let Some(existing_line) = existing_lines.remove(line.id.as_str()) else {
                continue;
            };
            if existing_line.status == TranslationStatus::Untranslated {
                continue;
            }
            ensure!(
                existing_line.source_markup == line.source_markup,
                "translated source changed at {}; refusing to overwrite the existing workspace",
                line.id
            );
            line.korean.clone_from(&existing_line.korean);
            line.status = existing_line.status;
            preserved_count += 1;
        }
    }

    if let Some(orphaned) = existing_lines
        .values()
        .find(|line| line.status != TranslationStatus::Untranslated)
    {
        anyhow::bail!(
            "translated line {} no longer exists; refusing to overwrite the existing workspace",
            orphaned.id
        );
    }
    validate_workspace_translations(&merged)?;
    *fresh = merged;
    Ok(preserved_count)
}

pub(super) fn build_workspace(source: &[u8]) -> Result<MainDialogueWorkspace> {
    let inspection = inspect_main_dialogue_storage(source)?;
    let records = inspection.records;
    let safe_japanese_offsets = safe_japanese_literal_offsets(source, &records)?;
    let workspace_records = records
        .iter()
        .map(|record| build_workspace_record(source, record, &safe_japanese_offsets))
        .collect::<Result<Vec<_>>>()?;
    let line_count = workspace_records
        .iter()
        .map(|record| record.lines.len())
        .sum::<usize>();
    let workspace = MainDialogueWorkspace {
        format_version: WORKSPACE_FORMAT_VERSION,
        source_sha1: EXPECTED_SOURCE_SHA1.to_owned(),
        translate_from: "ja".to_owned(),
        translate_to: "ko".to_owned(),
        preserve_existing_english: true,
        purpose: "private_translation_workspace".to_owned(),
        safe_japanese_source_byte_count: safe_japanese_offsets.len(),
        records: workspace_records,
    };
    ensure!(
        workspace.records.len() == 504,
        "main dialogue workspace must contain exactly 504 canonical records"
    );
    ensure!(
        line_count == 2_732,
        "main dialogue workspace must contain exactly 2732 source lines"
    );
    ensure!(
        safe_japanese_offsets.len() == inspection.safe_japanese_translation_source_byte_count,
        "main dialogue workspace Japanese source boundary disagrees with the dialogue inventory"
    );
    Ok(workspace)
}

pub(super) fn safe_japanese_literal_offsets(
    source: &[u8],
    records: &[MainDialogueStorageRecord],
) -> Result<BTreeSet<usize>> {
    let mut japanese_literal_offsets = BTreeSet::new();
    let mut structural_offsets = BTreeSet::new();
    for record in records {
        let record_literal_offsets = record
            .literal_file_offsets
            .iter()
            .copied()
            .collect::<BTreeSet<_>>();
        ensure!(
            record_literal_offsets.iter().all(|offset| {
                (record.file_offset..record.end_file_offset_exclusive).contains(offset)
            }),
            "{} entry {} has a literal outside its storage range",
            record.table_id,
            record.canonical_entry_index
        );
        for offset in record.file_offset..record.end_file_offset_exclusive {
            if record_literal_offsets.contains(&offset) {
                let code = *source
                    .get(offset)
                    .context("main dialogue workspace literal is outside the source")?;
                if is_japanese_text_code(code) {
                    japanese_literal_offsets.insert(offset);
                }
            } else {
                structural_offsets.insert(offset);
            }
        }
    }
    Ok(japanese_literal_offsets
        .difference(&structural_offsets)
        .copied()
        .collect())
}

pub(super) fn build_workspace_record(
    source: &[u8],
    record: &MainDialogueStorageRecord,
    safe_japanese_offsets: &BTreeSet<usize>,
) -> Result<WorkspaceRecord> {
    let record_id = format!("{}:{:03}", record.table_id, record.canonical_entry_index);
    let lines = record
        .lines
        .iter()
        .enumerate()
        .map(|(index, line)| {
            let japanese_offsets = line
                .literal_file_offsets
                .iter()
                .copied()
                .filter(|offset| {
                    source
                        .get(*offset)
                        .copied()
                        .is_some_and(is_japanese_text_code)
                })
                .collect::<Vec<_>>();
            let conflicting_offsets = japanese_offsets
                .iter()
                .copied()
                .filter(|offset| !safe_japanese_offsets.contains(offset))
                .collect::<Vec<_>>();
            Ok(WorkspaceLine {
                id: format!("{record_id}:line:{index:02}"),
                index,
                file_offset_hex: format!("0x{:05X}", line.file_offset),
                source_storage_sha1: line.storage_sha1.clone(),
                source_markup: decode_line_markup(source, line)?,
                korean: String::new(),
                status: TranslationStatus::Untranslated,
                japanese_source_byte_count: japanese_offsets.len(),
                safe_japanese_source_byte_count: japanese_offsets.len() - conflicting_offsets.len(),
                requires_relocation: !conflicting_offsets.is_empty(),
                conflicting_file_offsets_hex: conflicting_offsets
                    .iter()
                    .map(|offset| format!("0x{offset:05X}"))
                    .collect(),
            })
        })
        .collect::<Result<Vec<_>>>()?;
    Ok(WorkspaceRecord {
        id: record_id,
        table_id: record.table_id.to_owned(),
        source_prg_bank: record.source_prg_bank,
        canonical_entry_index: record.canonical_entry_index,
        entry_indices: record.entry_indices.clone(),
        pointer_cpu_address_hex: format!("0x{:04X}", record.pointer_cpu_address),
        prefix_byte_count: record.prefix_byte_count,
        boundary_control_hex: format!("{:02X}", record.boundary_control),
        lines,
    })
}

pub(super) fn validate_workspace_binding(
    workspace: &MainDialogueWorkspace,
    expected: &MainDialogueWorkspace,
) -> Result<()> {
    let mut actual_header = workspace.clone();
    actual_header.records.clear();
    let mut expected_header = expected.clone();
    expected_header.records.clear();
    ensure!(
        actual_header == expected_header,
        "main dialogue workspace header does not match the supported Japanese source"
    );
    ensure!(
        workspace.records.len() == expected.records.len(),
        "main dialogue workspace record count changed"
    );

    for (actual_record, expected_record) in workspace.records.iter().zip(&expected.records) {
        let mut actual_record_binding = actual_record.clone();
        actual_record_binding.lines.clear();
        let mut expected_record_binding = expected_record.clone();
        expected_record_binding.lines.clear();
        ensure!(
            actual_record_binding == expected_record_binding,
            "main dialogue workspace record binding changed at {}",
            expected_record.id
        );
        ensure!(
            actual_record.lines.len() == expected_record.lines.len(),
            "main dialogue workspace line count changed at {}",
            expected_record.id
        );
        for (actual_line, expected_line) in actual_record.lines.iter().zip(&expected_record.lines) {
            let mut actual_line_binding = actual_line.clone();
            actual_line_binding.korean.clear();
            actual_line_binding.status = TranslationStatus::Untranslated;
            ensure!(
                actual_line_binding == *expected_line,
                "main dialogue workspace protected source fields changed at {}",
                expected_line.id
            );
        }
    }
    Ok(())
}
