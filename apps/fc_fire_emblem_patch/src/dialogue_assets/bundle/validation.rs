use std::collections::BTreeSet;

use anyhow::{Context, Result, ensure};

use crate::{
    dialogue_inventory::{MainDialogueStorageRecord, inspect_main_dialogue_graph},
    rom::Rom,
};

use super::{MainDialogueWorkspace, TranslationStatus};

pub(super) fn validate_target_records(
    workspace: &MainDialogueWorkspace,
    source_records: &[MainDialogueStorageRecord],
    requested: &BTreeSet<&str>,
) -> Result<()> {
    for record in workspace
        .records
        .iter()
        .filter(|record| requested.contains(record.id.as_str()))
    {
        ensure!(
            record.lines.iter().all(|line| {
                line.status != TranslationStatus::Untranslated
                    || line.japanese_source_byte_count == 0
            }),
            "main dialogue bundle record {} has untranslated Japanese lines",
            record.id
        );
        for line in record.lines.iter().filter(|line| line.requires_relocation) {
            for encoded_offset in &line.conflicting_file_offsets_hex {
                let offset = usize::from_str_radix(encoded_offset.trim_start_matches("0x"), 16)
                    .with_context(|| {
                        format!(
                            "decode structural overlap for main dialogue line {}",
                            line.id
                        )
                    })?;
                let overlapping_records = source_records
                    .iter()
                    .filter(|source_record| {
                        (source_record.file_offset..source_record.end_file_offset_exclusive)
                            .contains(&offset)
                    })
                    .collect::<Vec<_>>();
                ensure!(
                    !overlapping_records.is_empty(),
                    "main dialogue line {} names an overlap outside owned dialogue storage",
                    line.id
                );
                ensure!(
                    overlapping_records.iter().all(|source_record| {
                        let record_id = format!(
                            "{}:{:03}",
                            source_record.table_id, source_record.canonical_entry_index
                        );
                        requested.contains(record_id.as_str())
                    }),
                    "main dialogue bundle record {} requires every record overlapping {}",
                    record.id,
                    encoded_offset
                );
            }
        }
    }
    Ok(())
}

pub(super) fn validate_transition_closure(
    rom: &Rom,
    source_records: &[MainDialogueStorageRecord],
    requested: &BTreeSet<&str>,
) -> Result<()> {
    let requested_keys = source_records
        .iter()
        .filter(|record| {
            requested.contains(
                format!("{}:{:03}", record.table_id, record.canonical_entry_index).as_str(),
            )
        })
        .map(|record| (record.table_id, record.canonical_entry_index))
        .collect::<BTreeSet<_>>();
    for edge in inspect_main_dialogue_graph(rom.data())?.transition_edges {
        if requested_keys.contains(&(edge.source_table_id, edge.source_canonical_entry_index)) {
            ensure!(
                requested_keys.contains(&(edge.target_table_id, edge.target_canonical_entry_index)),
                "main dialogue bundle is not transition-closed at {}:{:03}",
                edge.source_table_id,
                edge.source_canonical_entry_index
            );
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::dialogue_assets::{
        MainDialogueWorkspace, WORKSPACE_FORMAT_VERSION, WorkspaceLine, WorkspaceRecord,
    };

    fn source_record(
        index: usize,
        start: usize,
        end_exclusive: usize,
    ) -> MainDialogueStorageRecord {
        MainDialogueStorageRecord {
            table_id: "synthetic-dialogue",
            source_prg_bank: 2,
            canonical_entry_index: index,
            entry_indices: vec![index],
            pointer_file_offsets: vec![index * 2],
            pointer_cpu_address: 0x8000 + index as u16 * 2,
            file_offset: start,
            end_file_offset_exclusive: end_exclusive,
            storage_byte_count: end_exclusive - start,
            storage_sha1: String::new(),
            prefix_byte_count: 4,
            boundary_control: 0xEF,
            literal_file_offsets: Vec::new(),
            lines: Vec::new(),
        }
    }

    fn workspace_record(index: usize, overlap: Option<usize>) -> WorkspaceRecord {
        WorkspaceRecord {
            id: format!("synthetic-dialogue:{index:03}"),
            table_id: "synthetic-dialogue".to_owned(),
            source_prg_bank: 2,
            canonical_entry_index: index,
            entry_indices: vec![index],
            pointer_cpu_address_hex: format!("0x{:04X}", 0x8000 + index * 2),
            prefix_byte_count: 4,
            boundary_control_hex: "EF".to_owned(),
            lines: vec![WorkspaceLine {
                id: format!("synthetic-dialogue:{index:03}:line:00"),
                index: 0,
                file_offset_hex: "0x00000".to_owned(),
                source_storage_sha1: String::new(),
                source_markup: "あ{EF}".to_owned(),
                korean: "가{EF}".to_owned(),
                status: TranslationStatus::NeedsHumanReview,
                japanese_source_byte_count: 1,
                safe_japanese_source_byte_count: 1,
                requires_relocation: overlap.is_some(),
                conflicting_file_offsets_hex: overlap
                    .into_iter()
                    .map(|offset| format!("0x{offset:05X}"))
                    .collect(),
            }],
        }
    }

    fn workspace(records: Vec<WorkspaceRecord>) -> MainDialogueWorkspace {
        MainDialogueWorkspace {
            format_version: WORKSPACE_FORMAT_VERSION,
            source_sha1: String::new(),
            translate_from: "ja".to_owned(),
            translate_to: "ko".to_owned(),
            preserve_existing_english: true,
            purpose: "test".to_owned(),
            safe_japanese_source_byte_count: records.len(),
            source_preservation_line_ids: Vec::new(),
            records,
        }
    }

    #[test]
    fn structural_overlap_requires_every_owner_in_the_same_bundle() {
        let source_records = [source_record(0, 10, 20), source_record(1, 15, 25)];
        let workspace = workspace(vec![
            workspace_record(0, Some(16)),
            workspace_record(1, None),
        ]);

        let partial = BTreeSet::from(["synthetic-dialogue:000"]);
        assert!(
            validate_target_records(&workspace, &source_records, &partial)
                .unwrap_err()
                .to_string()
                .contains("requires every record overlapping")
        );

        let all = BTreeSet::from(["synthetic-dialogue:000", "synthetic-dialogue:001"]);
        validate_target_records(&workspace, &source_records, &all).unwrap();
    }
}
