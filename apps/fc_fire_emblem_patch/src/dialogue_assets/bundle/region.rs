use std::collections::BTreeSet;

use anyhow::{Context, Result, ensure};

use crate::dialogue_inventory::{MainDialogueStorageRecord, switchable_file_to_cpu};

use super::{
    LogicalBundleRegion, LogicalDialogueByte, LogicalDialogueRecord, MainDialoguePointerWrite,
    OwnedStorageRange, WorkspaceRecord, build_logical_dialogue_record, pack_logical_records,
};

pub(super) fn plan_region(
    source: &[u8],
    source_records: &[MainDialogueStorageRecord],
    workspace_records: &[WorkspaceRecord],
    target_indices: &BTreeSet<usize>,
    region: OwnedStorageRange,
) -> Result<LogicalBundleRegion> {
    let mut indexed_records = source_records
        .iter()
        .enumerate()
        .filter(|(_, record)| {
            record.source_prg_bank == region.source_prg_bank
                && region.start <= record.file_offset
                && record.end_file_offset_exclusive <= region.end_exclusive
        })
        .collect::<Vec<_>>();
    indexed_records.sort_unstable_by_key(|(_, record)| record.file_offset);
    ensure!(
        !indexed_records.is_empty(),
        "main dialogue bundle affected region has no records"
    );
    let logical_records = indexed_records
        .iter()
        .map(|(index, record)| {
            if target_indices.contains(index) {
                build_logical_dialogue_record(source, record, &workspace_records[*index])
            } else {
                source_logical_record(source, record)
            }
        })
        .collect::<Result<Vec<_>>>()?;
    let logical_refs = logical_records.iter().collect::<Vec<_>>();
    let (mut logical_storage, placements) = pack_logical_records(&logical_refs);
    let capacity = region.end_exclusive - region.start;
    ensure!(
        logical_storage.len() <= capacity,
        "main dialogue bundle region in PRG bank {:02X} needs {} bytes but owns only {capacity}",
        region.source_prg_bank,
        logical_storage.len()
    );
    logical_storage.extend(
        source[region.start + logical_storage.len()..region.end_exclusive]
            .iter()
            .copied()
            .map(LogicalDialogueByte::Encoded),
    );
    ensure!(
        logical_storage.len() == capacity,
        "main dialogue bundle did not fill its exact owned region"
    );
    let mut pointer_writes = Vec::new();
    for ((_, source_record), placement) in indexed_records.iter().zip(placements) {
        let planned_file_offset = region
            .start
            .checked_add(placement)
            .context("main dialogue bundle pointer placement overflow")?;
        let planned_pointer = switchable_file_to_cpu(region.source_prg_bank, planned_file_offset)?;
        for pointer_file_offset in &source_record.pointer_file_offsets {
            pointer_writes.push(MainDialoguePointerWrite {
                record_id: format!(
                    "{}:{:03}",
                    source_record.table_id, source_record.canonical_entry_index
                ),
                file_offset: *pointer_file_offset,
                source_pointer: source_record.pointer_cpu_address,
                planned_pointer,
            });
        }
    }

    Ok(LogicalBundleRegion {
        file_offset: region.start,
        source_storage: source[region.start..region.end_exclusive].to_vec(),
        logical_storage,
        pointer_writes,
    })
}

fn source_logical_record(
    source: &[u8],
    record: &MainDialogueStorageRecord,
) -> Result<LogicalDialogueRecord> {
    let bytes = source
        .get(record.file_offset..record.end_file_offset_exclusive)
        .context("main dialogue bundle source record is outside the ROM")?
        .iter()
        .copied()
        .map(LogicalDialogueByte::Encoded)
        .collect();
    Ok(LogicalDialogueRecord {
        id: format!("{}:{:03}", record.table_id, record.canonical_entry_index),
        source_prg_bank: record.source_prg_bank,
        source_pointer_cpu_address: record.pointer_cpu_address,
        pointer_file_offsets: record.pointer_file_offsets.clone(),
        source_file_offset: record.file_offset,
        source_storage_byte_count: record.storage_byte_count,
        translated_line_count: 0,
        bytes,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn source_record_keeps_every_byte_and_pointer_binding() {
        let source = [0x11, 0x22, 0x33];
        let record = MainDialogueStorageRecord {
            table_id: "table",
            source_prg_bank: 1,
            canonical_entry_index: 2,
            entry_indices: vec![2],
            pointer_file_offsets: vec![0x10],
            pointer_cpu_address: 0x8000,
            file_offset: 0,
            end_file_offset_exclusive: 3,
            storage_byte_count: 3,
            storage_sha1: String::new(),
            prefix_byte_count: 0,
            boundary_control: 0xEF,
            literal_file_offsets: vec![0, 1, 2],
            lines: Vec::new(),
        };

        let logical = source_logical_record(&source, &record).unwrap();

        assert_eq!(
            logical.bytes,
            vec![
                LogicalDialogueByte::Encoded(0x11),
                LogicalDialogueByte::Encoded(0x22),
                LogicalDialogueByte::Encoded(0x33),
            ]
        );
        assert_eq!(logical.pointer_file_offsets, vec![0x10]);
    }
}
