use std::collections::{BTreeMap, BTreeSet};

use super::layout::pack_record_sizes;
use super::*;

pub(crate) struct BattleDialogueReinsertionPlan {
    pub(crate) workspace_sha1: String,
    pub(crate) records: Vec<BattleDialoguePlannedRecord>,
    pub(crate) capacity_byte_count: usize,
    pub(crate) translated_record_storage_byte_count: usize,
    pub(crate) preserved_unreferenced_file_offset: usize,
    pub(crate) preserved_unreferenced_end_file_offset_exclusive: usize,
    pub(crate) preserved_unreferenced_storage_sha1: String,
    pub(crate) remaining_storage_byte_count: usize,
    pub(crate) translated_line_count: usize,
}

pub(crate) struct BattleDialoguePlannedRecord {
    pub(crate) canonical_entry_index: usize,
    pub(crate) entry_indices: Vec<usize>,
    pub(crate) pointer_file_offsets: Vec<usize>,
    pub(crate) planned_pointer_cpu_address: u16,
    pub(crate) planned_file_offset: usize,
    pub(crate) source_file_offset: usize,
    pub(crate) source_storage_byte_count: usize,
    logical_bytes: Vec<LogicalDialogueByte>,
}

impl BattleDialoguePlannedRecord {
    pub(crate) fn storage_byte_count(&self) -> usize {
        self.logical_bytes.len()
    }

    pub(crate) fn unique_glyphs(&self) -> BTreeSet<char> {
        self.logical_bytes
            .iter()
            .filter_map(|byte| match byte {
                LogicalDialogueByte::TargetGlyph(character) => Some(*character),
                LogicalDialogueByte::Encoded(_) => None,
            })
            .collect()
    }

    pub(crate) fn encoded_bytes(&self, assignments: &BTreeMap<char, u8>) -> Result<Vec<u8>> {
        self.logical_bytes
            .iter()
            .map(|byte| match byte {
                LogicalDialogueByte::Encoded(value) => Ok(*value),
                LogicalDialogueByte::TargetGlyph(character) => {
                    assignments.get(character).copied().with_context(|| {
                        format!("missing battle-dialogue code assignment for {character:?}")
                    })
                }
            })
            .collect()
    }
}

pub(crate) struct EncodedBattleDialogueRecord {
    pub(crate) canonical_entry_index: usize,
    pub(crate) pointer_file_offsets: Vec<usize>,
    pub(crate) planned_pointer_cpu_address: u16,
    pub(crate) planned_file_offset: usize,
    pub(crate) bytes: Vec<u8>,
}

impl BattleDialogueReinsertionPlan {
    pub(crate) fn unique_glyphs(&self) -> BTreeSet<char> {
        self.records
            .iter()
            .flat_map(|record| &record.logical_bytes)
            .filter_map(|byte| match byte {
                LogicalDialogueByte::TargetGlyph(character) => Some(*character),
                LogicalDialogueByte::Encoded(_) => None,
            })
            .collect()
    }

    pub(crate) fn max_record_unique_glyph_count(&self) -> usize {
        self.records
            .iter()
            .map(|record| record.unique_glyphs().len())
            .max()
            .unwrap_or(0)
    }

    pub(crate) fn encoded_records(
        &self,
        assignments: &BTreeMap<char, u8>,
    ) -> Result<Vec<EncodedBattleDialogueRecord>> {
        self.records
            .iter()
            .map(|record| {
                let bytes = record.encoded_bytes(assignments)?;
                Ok(EncodedBattleDialogueRecord {
                    canonical_entry_index: record.canonical_entry_index,
                    pointer_file_offsets: record.pointer_file_offsets.clone(),
                    planned_pointer_cpu_address: record.planned_pointer_cpu_address,
                    planned_file_offset: record.planned_file_offset,
                    bytes,
                })
            })
            .collect()
    }
}

pub(crate) fn plan_battle_dialogue_records(
    rom: &Rom,
    workspace_path: &Path,
) -> Result<BattleDialogueReinsertionPlan> {
    rom.verify_supported_japanese()?;
    let workspace_bytes =
        fs::read(workspace_path).with_context(|| format!("read {}", workspace_path.display()))?;
    let workspace: BattleDialogueWorkspace = serde_json::from_slice(&workspace_bytes)
        .with_context(|| format!("parse {}", workspace_path.display()))?;
    validate_workspace_binding(rom.data(), &workspace)?;
    validate_translation_fields(&workspace)?;

    let source_records = inspect_battle_dialogue_translation_records(rom.data())?;
    let physical = inspect_battle_dialogue_physical_layout(rom.data())?;
    ensure!(
        source_records.len() == workspace.records.len(),
        "battle reinsertion lost workspace records"
    );

    let mut translated_line_count = 0;
    let logical_records = source_records
        .iter()
        .zip(&workspace.records)
        .map(|(source_record, workspace_record)| {
            let header_end = source_record
                .file_offset
                .checked_add(4)
                .context("battle record header range overflow")?;
            let mut logical_bytes = rom
                .data()
                .get(source_record.file_offset..header_end)
                .context("battle record header is outside the source")?
                .iter()
                .copied()
                .map(LogicalDialogueByte::Encoded)
                .collect::<Vec<_>>();
            for line in &workspace_record.lines {
                let markup = if line.status == TranslationStatus::Untranslated {
                    ensure!(
                        line.japanese_source_byte_count == 0,
                        "{} still contains untranslated Japanese",
                        line.id
                    );
                    &line.source_markup
                } else {
                    translated_line_count += 1;
                    &line.korean
                };
                logical_bytes.extend(
                    encode_korean_markup(markup)
                        .with_context(|| format!("encode battle markup at {}", line.id))?,
                );
            }
            Ok(logical_bytes)
        })
        .collect::<Result<Vec<_>>>()?;
    let record_sizes = logical_records.iter().map(Vec::len).collect::<Vec<_>>();
    let segments = [
        (
            physical.data_file_start,
            physical.preserved_unreferenced_file_offset,
        ),
        (
            physical.preserved_unreferenced_end_file_offset_exclusive,
            physical.data_file_end_exclusive,
        ),
    ];
    let placements = pack_record_sizes(&record_sizes, &segments)?;
    let records = source_records
        .iter()
        .zip(logical_records)
        .zip(placements)
        .map(|((source_record, logical_bytes), planned_file_offset)| {
            Ok(BattleDialoguePlannedRecord {
                canonical_entry_index: source_record.canonical_entry_index,
                entry_indices: source_record.entry_indices.clone(),
                pointer_file_offsets: source_record.pointer_file_offsets.clone(),
                planned_pointer_cpu_address: switchable_file_to_cpu(
                    source_record.source_prg_bank,
                    planned_file_offset,
                )?,
                planned_file_offset,
                source_file_offset: source_record.file_offset,
                source_storage_byte_count: source_record.end_file_offset_exclusive
                    - source_record.file_offset,
                logical_bytes,
            })
        })
        .collect::<Result<Vec<_>>>()?;
    let capacity_byte_count = physical.data_file_end_exclusive - physical.data_file_start;
    let translated_record_storage_byte_count = record_sizes.iter().sum::<usize>();
    let preserved_storage_byte_count = physical.preserved_unreferenced_end_file_offset_exclusive
        - physical.preserved_unreferenced_file_offset;
    let used_storage_byte_count = translated_record_storage_byte_count
        .checked_add(preserved_storage_byte_count)
        .context("battle reinsertion used size overflow")?;
    ensure!(
        used_storage_byte_count <= capacity_byte_count,
        "battle reinsertion exceeds physical storage"
    );

    Ok(BattleDialogueReinsertionPlan {
        workspace_sha1: sha1_hex(&workspace_bytes),
        records,
        capacity_byte_count,
        translated_record_storage_byte_count,
        preserved_unreferenced_file_offset: physical.preserved_unreferenced_file_offset,
        preserved_unreferenced_end_file_offset_exclusive: physical
            .preserved_unreferenced_end_file_offset_exclusive,
        preserved_unreferenced_storage_sha1: physical.preserved_unreferenced_storage_sha1,
        remaining_storage_byte_count: capacity_byte_count - used_storage_byte_count,
        translated_line_count,
    })
}
