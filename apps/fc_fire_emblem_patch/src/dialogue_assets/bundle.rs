use std::{
    collections::{BTreeMap, BTreeSet},
    fs,
    path::Path,
};

use anyhow::{Context, Result, ensure};

use crate::{dialogue_inventory::inspect_main_dialogue_storage, rom::Rom, sha1_hex};

use super::*;

mod region;
mod validation;

use region::plan_region;
use validation::{validate_target_records, validate_transition_closure};

const DIALOGUE_PREFIX_CONTROL_CODE: u8 = 0xEA;
const DIALOGUE_PREFIX_OUTPUT_CODES: [u8; 2] = [0x9E, 0xAB];

pub(crate) struct MainDialogueBundlePlan {
    pub(crate) workspace_sha1: String,
    pub(crate) record_ids: Vec<String>,
    pub(crate) translated_line_count: usize,
    pub(crate) source_record_storage_byte_count: usize,
    pub(crate) planned_record_storage_byte_count: usize,
    pub(crate) preserved_source_codes: BTreeSet<u8>,
    target_records: Vec<LogicalDialogueRecord>,
    regions: Vec<LogicalBundleRegion>,
}

impl MainDialogueBundlePlan {
    pub(crate) fn unique_glyphs(&self) -> BTreeSet<char> {
        self.target_records
            .iter()
            .flat_map(|record| &record.bytes)
            .filter_map(|byte| match byte {
                LogicalDialogueByte::TargetGlyph(glyph) => Some(*glyph),
                LogicalDialogueByte::Encoded(_) => None,
            })
            .collect()
    }

    pub(crate) fn encoded(
        &self,
        assignments: &BTreeMap<char, u8>,
    ) -> Result<EncodedMainDialogueBundle> {
        let regions = self
            .regions
            .iter()
            .map(|region| {
                Ok(EncodedMainDialogueRegion {
                    file_offset: region.file_offset,
                    source_storage: region.source_storage.clone(),
                    encoded_storage: encode_logical_bytes(&region.logical_storage, assignments)?,
                })
            })
            .collect::<Result<Vec<_>>>()?;
        let pointer_writes = self
            .regions
            .iter()
            .flat_map(|region| region.pointer_writes.iter().cloned())
            .collect();
        Ok(EncodedMainDialogueBundle {
            regions,
            pointer_writes,
        })
    }
}

pub(crate) struct EncodedMainDialogueBundle {
    pub(crate) regions: Vec<EncodedMainDialogueRegion>,
    pub(crate) pointer_writes: Vec<MainDialoguePointerWrite>,
}

pub(crate) struct EncodedMainDialogueRegion {
    pub(crate) file_offset: usize,
    pub(crate) source_storage: Vec<u8>,
    pub(crate) encoded_storage: Vec<u8>,
}

#[derive(Clone)]
pub(crate) struct MainDialoguePointerWrite {
    pub(crate) record_id: String,
    pub(crate) file_offset: usize,
    pub(crate) source_pointer: u16,
    pub(crate) planned_pointer: u16,
}

struct LogicalBundleRegion {
    file_offset: usize,
    source_storage: Vec<u8>,
    logical_storage: Vec<LogicalDialogueByte>,
    pointer_writes: Vec<MainDialoguePointerWrite>,
}

pub(crate) fn plan_main_dialogue_bundle(
    rom: &Rom,
    workspace_path: &Path,
    record_ids: &[&str],
) -> Result<MainDialogueBundlePlan> {
    rom.verify_supported_japanese()?;
    ensure!(
        !record_ids.is_empty(),
        "main dialogue bundle has no records"
    );
    let requested = record_ids.iter().copied().collect::<BTreeSet<_>>();
    ensure!(
        requested.len() == record_ids.len(),
        "main dialogue bundle contains duplicate record IDs"
    );
    let workspace_bytes = fs::read(workspace_path)
        .with_context(|| format!("read main dialogue workspace {}", workspace_path.display()))?;
    let workspace: MainDialogueWorkspace = serde_json::from_slice(&workspace_bytes)
        .with_context(|| format!("parse main dialogue workspace {}", workspace_path.display()))?;
    let expected = build_workspace(rom.data())?;
    validate_workspace_binding(&workspace, &expected)?;
    validate_workspace_translations(&workspace)?;

    let source_records = inspect_main_dialogue_storage(rom.data())?.records;
    ensure!(
        source_records.len() == workspace.records.len(),
        "main dialogue bundle lost workspace records"
    );
    let record_index_by_id = workspace
        .records
        .iter()
        .enumerate()
        .map(|(index, record)| (record.id.as_str(), index))
        .collect::<BTreeMap<_, _>>();
    ensure!(
        requested
            .iter()
            .all(|record_id| record_index_by_id.contains_key(record_id)),
        "main dialogue bundle contains an unknown record ID"
    );
    validate_target_records(&workspace, &requested)?;
    validate_transition_closure(rom, &source_records, &requested)?;

    let target_indices = requested
        .iter()
        .map(|record_id| record_index_by_id[record_id])
        .collect::<BTreeSet<_>>();
    let target_records = target_indices
        .iter()
        .map(|index| {
            build_logical_dialogue_record(
                rom.data(),
                &source_records[*index],
                &workspace.records[*index],
            )
        })
        .collect::<Result<Vec<_>>>()?;
    let target_regions = normalize_storage_ranges(
        &source_records
            .iter()
            .enumerate()
            .filter(|(index, _)| target_indices.contains(index))
            .map(|(_, record)| record.clone())
            .collect::<Vec<_>>(),
    )?;
    let owned_regions = normalize_storage_ranges(&source_records)?;
    let affected_regions = owned_regions
        .into_iter()
        .filter(|region| {
            target_regions.iter().any(|target| {
                region.source_prg_bank == target.source_prg_bank
                    && target.start < region.end_exclusive
                    && region.start < target.end_exclusive
            })
        })
        .collect::<Vec<_>>();
    ensure!(
        !affected_regions.is_empty(),
        "main dialogue bundle has no affected storage region"
    );

    let mut regions = Vec::new();
    for region in affected_regions {
        regions.push(plan_region(
            rom.data(),
            &source_records,
            &workspace.records,
            &target_indices,
            region,
        )?);
    }

    let translated_line_count = target_records
        .iter()
        .map(|record| record.translated_line_count)
        .sum();
    let source_record_storage_byte_count = target_indices
        .iter()
        .map(|index| source_records[*index].storage_byte_count)
        .sum();
    let planned_record_storage_byte_count =
        target_records.iter().map(|record| record.bytes.len()).sum();
    let mut preserved_source_codes = target_records
        .iter()
        .flat_map(|record| &record.bytes)
        .filter_map(|byte| match byte {
            LogicalDialogueByte::Encoded(value) => Some(*value),
            LogicalDialogueByte::TargetGlyph(_) => None,
        })
        .collect::<BTreeSet<_>>();
    if target_records.iter().any(|record| {
        record.bytes.iter().any(|byte| {
            matches!(
                byte,
                LogicalDialogueByte::Encoded(DIALOGUE_PREFIX_CONTROL_CODE)
            )
        })
    }) {
        preserved_source_codes.extend(DIALOGUE_PREFIX_OUTPUT_CODES);
    }

    Ok(MainDialogueBundlePlan {
        workspace_sha1: sha1_hex(&workspace_bytes),
        record_ids: record_ids.iter().map(|id| (*id).to_owned()).collect(),
        translated_line_count,
        source_record_storage_byte_count,
        planned_record_storage_byte_count,
        preserved_source_codes,
        target_records,
        regions,
    })
}

fn encode_logical_bytes(
    bytes: &[LogicalDialogueByte],
    assignments: &BTreeMap<char, u8>,
) -> Result<Vec<u8>> {
    bytes
        .iter()
        .map(|byte| match byte {
            LogicalDialogueByte::Encoded(value) => Ok(*value),
            LogicalDialogueByte::TargetGlyph(glyph) => assignments
                .get(glyph)
                .copied()
                .with_context(|| format!("missing main-dialogue bundle code for {glyph:?}")),
        })
        .collect()
}
