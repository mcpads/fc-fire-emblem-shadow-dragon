use std::{
    collections::{BTreeMap, BTreeSet},
    fs,
    path::Path,
};

use anyhow::{Context, Result, ensure};

use crate::{rom::Rom, sha1_hex};

use super::*;

pub(crate) struct MainDialogueSlicePlan {
    pub(crate) workspace_sha1: String,
    pub(crate) record_id: String,
    pub(crate) source_file_offset: usize,
    pub(crate) source_storage_byte_count: usize,
    pub(crate) translated_line_count: usize,
    logical_bytes: Vec<LogicalDialogueByte>,
}

impl MainDialogueSlicePlan {
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
                LogicalDialogueByte::TargetGlyph(character) => assignments
                    .get(character)
                    .copied()
                    .with_context(|| format!("missing code assignment for {character:?}")),
            })
            .collect()
    }
}

pub(crate) fn plan_main_dialogue_slice(
    rom: &Rom,
    workspace_path: &Path,
    record_id: &str,
) -> Result<MainDialogueSlicePlan> {
    rom.verify_supported_japanese()?;
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
        "main dialogue slice lost workspace records"
    );
    let record_index = workspace
        .records
        .iter()
        .position(|record| record.id == record_id)
        .with_context(|| format!("main dialogue slice record {record_id} does not exist"))?;
    let workspace_record = &workspace.records[record_index];
    ensure!(
        workspace_record
            .lines
            .iter()
            .all(|line| line.status != TranslationStatus::Untranslated),
        "main dialogue slice record {record_id} has untranslated lines"
    );
    ensure!(
        workspace_record
            .lines
            .iter()
            .all(|line| !line.requires_relocation),
        "main dialogue slice record {record_id} requires a relocation contract"
    );

    let source_record = &source_records[record_index];
    let source_start = source_record.file_offset;
    let source_end = source_record.end_file_offset_exclusive;
    ensure!(
        source_records.iter().enumerate().all(|(index, other)| {
            index == record_index
                || source_end <= other.file_offset
                || other.end_file_offset_exclusive <= source_start
        }),
        "main dialogue slice record {record_id} shares source storage with another record"
    );
    let logical = build_logical_dialogue_record(rom.data(), source_record, workspace_record)?;
    ensure!(
        logical.translated_line_count == workspace_record.lines.len(),
        "main dialogue slice record {record_id} is not fully translated"
    );
    ensure!(
        logical.bytes.len() <= logical.source_storage_byte_count,
        "main dialogue slice record {record_id} needs {} bytes but owns only {}",
        logical.bytes.len(),
        logical.source_storage_byte_count
    );

    Ok(MainDialogueSlicePlan {
        workspace_sha1: sha1_hex(&workspace_bytes),
        record_id: logical.id,
        source_file_offset: logical.source_file_offset,
        source_storage_byte_count: logical.source_storage_byte_count,
        translated_line_count: logical.translated_line_count,
        logical_bytes: logical.bytes,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn encodes_only_declared_target_glyph_assignments() {
        let plan = MainDialogueSlicePlan {
            workspace_sha1: "workspace".to_owned(),
            record_id: "record".to_owned(),
            source_file_offset: 0,
            source_storage_byte_count: 3,
            translated_line_count: 1,
            logical_bytes: vec![
                LogicalDialogueByte::TargetGlyph('한'),
                LogicalDialogueByte::Encoded(0xED),
            ],
        };

        let assignments = BTreeMap::from([('한', 0x01)]);
        assert_eq!(plan.encoded_bytes(&assignments).unwrap(), [0x01, 0xED]);
        assert!(plan.encoded_bytes(&BTreeMap::new()).is_err());
    }
}
