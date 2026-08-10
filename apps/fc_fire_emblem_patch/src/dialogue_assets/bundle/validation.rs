use std::collections::BTreeSet;

use anyhow::{Result, ensure};

use crate::{
    dialogue_inventory::{MainDialogueStorageRecord, inspect_main_dialogue_graph},
    rom::Rom,
};

use super::{MainDialogueWorkspace, TranslationStatus};

pub(super) fn validate_target_records(
    workspace: &MainDialogueWorkspace,
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
        ensure!(
            record.lines.iter().all(|line| !line.requires_relocation),
            "main dialogue bundle record {} requires a structural relocation contract",
            record.id
        );
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
