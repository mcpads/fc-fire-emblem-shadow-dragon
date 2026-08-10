use std::{
    collections::{BTreeMap, BTreeSet},
    fs,
    path::Path,
};

use anyhow::{Context, Result, ensure};

#[cfg(test)]
use crate::dialogue_inventory::MainDialogueTransitionEdgeReport;
use crate::{dialogue_inventory::MainDialogueGraphReport, rom::Rom, sha1_hex};

use super::*;

const DIALOGUE_PREFIX_CONTROL_CODE: u8 = 0xEA;
const DIALOGUE_PREFIX_OUTPUT_CODES: [u8; 2] = [0x9E, 0xAB];

pub(crate) struct MainDialogueSlicePlan {
    pub(crate) workspace_sha1: String,
    pub(crate) record_id: String,
    pub(crate) source_file_offset: usize,
    pub(crate) source_storage_byte_count: usize,
    pub(crate) translated_line_count: usize,
    pub(crate) transition_chain_record_count: usize,
    pub(crate) preserved_source_codes: BTreeSet<u8>,
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
            .all(|line| line.status != TranslationStatus::Untranslated
                || line.japanese_source_byte_count == 0),
        "main dialogue slice record {record_id} has untranslated Japanese lines"
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
    let expected_translated_line_count = workspace_record
        .lines
        .iter()
        .filter(|line| line.status != TranslationStatus::Untranslated)
        .count();
    ensure!(
        logical.translated_line_count == expected_translated_line_count,
        "main dialogue slice record {record_id} translated-line count changed"
    );
    ensure!(
        logical.bytes.len() <= logical.source_storage_byte_count,
        "main dialogue slice record {record_id} needs {} bytes but owns only {}",
        logical.bytes.len(),
        logical.source_storage_byte_count
    );
    let (transition_chain_record_count, mut preserved_source_codes) =
        collect_followup_literal_codes(
            rom.data(),
            &source_records,
            &inspect_main_dialogue_graph(rom.data())?,
            &workspace_record.table_id,
            workspace_record.canonical_entry_index,
        )?;
    preserved_source_codes.extend(logical.bytes.iter().filter_map(|byte| match byte {
        LogicalDialogueByte::Encoded(value) => Some(*value),
        LogicalDialogueByte::TargetGlyph(_) => None,
    }));
    preserved_source_codes.extend(runtime_generated_literal_codes(&logical.bytes));

    Ok(MainDialogueSlicePlan {
        workspace_sha1: sha1_hex(&workspace_bytes),
        record_id: logical.id,
        source_file_offset: logical.source_file_offset,
        source_storage_byte_count: logical.source_storage_byte_count,
        translated_line_count: logical.translated_line_count,
        transition_chain_record_count,
        preserved_source_codes,
        logical_bytes: logical.bytes,
    })
}

fn runtime_generated_literal_codes(bytes: &[LogicalDialogueByte]) -> BTreeSet<u8> {
    let mut codes = BTreeSet::new();
    if bytes.iter().any(|byte| {
        matches!(
            byte,
            LogicalDialogueByte::Encoded(DIALOGUE_PREFIX_CONTROL_CODE)
        )
    }) {
        codes.extend(DIALOGUE_PREFIX_OUTPUT_CODES);
    }
    codes
}

fn collect_followup_literal_codes(
    source: &[u8],
    records: &[MainDialogueStorageRecord],
    graph: &MainDialogueGraphReport,
    start_table_id: &str,
    start_canonical_entry_index: usize,
) -> Result<(usize, BTreeSet<u8>)> {
    let records_by_key = records
        .iter()
        .map(|record| {
            (
                (record.table_id.to_owned(), record.canonical_entry_index),
                record,
            )
        })
        .collect::<BTreeMap<_, _>>();
    let next_record = graph
        .transition_edges
        .iter()
        .map(|edge| {
            (
                (
                    edge.source_table_id.to_owned(),
                    edge.source_canonical_entry_index,
                ),
                (
                    edge.target_table_id.to_owned(),
                    edge.target_canonical_entry_index,
                ),
            )
        })
        .collect::<BTreeMap<_, _>>();
    let start = (start_table_id.to_owned(), start_canonical_entry_index);
    ensure!(
        records_by_key.contains_key(&start),
        "main dialogue slice start record is missing from storage"
    );

    let mut current = start.clone();
    let mut visited = BTreeSet::new();
    let mut preserved_codes = BTreeSet::new();
    loop {
        ensure!(
            visited.insert(current.clone()),
            "main dialogue slice transition chain contains a cycle"
        );
        if current != start {
            let record = records_by_key
                .get(&current)
                .context("main dialogue slice transition target is missing from storage")?;
            for offset in &record.literal_file_offsets {
                preserved_codes.insert(
                    *source
                        .get(*offset)
                        .context("main dialogue followup literal is outside the source")?,
                );
            }
        }
        let Some(next) = next_record.get(&current) else {
            break;
        };
        current = next.clone();
    }
    Ok((visited.len(), preserved_codes))
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
            transition_chain_record_count: 1,
            preserved_source_codes: BTreeSet::new(),
            logical_bytes: vec![
                LogicalDialogueByte::TargetGlyph('한'),
                LogicalDialogueByte::Encoded(0xED),
            ],
        };

        let assignments = BTreeMap::from([('한', 0x01)]);
        assert_eq!(plan.encoded_bytes(&assignments).unwrap(), [0x01, 0xED]);
        assert!(plan.encoded_bytes(&BTreeMap::new()).is_err());
    }

    #[test]
    fn preserves_followup_literals_but_not_replaced_start_literals() {
        let source = [0x11, 0x22, 0x33, 0x44];
        let records = vec![
            storage_record("chapter-intro-dialogue", 0, vec![0, 1]),
            storage_record("chapter-intro-dialogue", 2, vec![2, 3]),
        ];
        let graph = MainDialogueGraphReport {
            node_count: 2,
            transition_edge_count: 1,
            terminal_reachable_node_count: 2,
            caller_handoff_boundary_reachable_node_count: 0,
            max_transition_edge_count_to_boundary: 1,
            cycle_count: 0,
            unresolved_node_count: 0,
            transition_edges: vec![MainDialogueTransitionEdgeReport {
                source_table_id: "chapter-intro-dialogue",
                source_canonical_entry_index: 0,
                source_entry_indices: vec![0],
                source_pointer_cpu_address: 0x8000,
                source_pointer_cpu_address_hex: "0x8000".to_owned(),
                source_file_offset: 0,
                source_file_offset_hex: "0x00000".to_owned(),
                control: 0xE6,
                control_hex: "E6".to_owned(),
                target_table_id: "chapter-intro-dialogue",
                target_entry_index: 2,
                target_canonical_entry_index: 2,
                target_pointer_cpu_address: 0x8002,
                target_pointer_cpu_address_hex: "0x8002".to_owned(),
                target_file_offset: 2,
                target_file_offset_hex: "0x00002".to_owned(),
            }],
        };

        let (record_count, codes) =
            collect_followup_literal_codes(&source, &records, &graph, "chapter-intro-dialogue", 0)
                .unwrap();

        assert_eq!(record_count, 2);
        assert_eq!(codes, BTreeSet::from([0x33, 0x44]));
    }

    #[test]
    fn preserves_runtime_prefix_glyphs_emitted_by_dialogue_control() {
        let bytes = vec![
            LogicalDialogueByte::Encoded(0xE9),
            LogicalDialogueByte::Encoded(0x03),
            LogicalDialogueByte::Encoded(DIALOGUE_PREFIX_CONTROL_CODE),
            LogicalDialogueByte::TargetGlyph('한'),
            LogicalDialogueByte::Encoded(0xED),
        ];

        assert_eq!(
            runtime_generated_literal_codes(&bytes),
            BTreeSet::from(DIALOGUE_PREFIX_OUTPUT_CODES)
        );
        assert!(runtime_generated_literal_codes(&bytes[..2]).is_empty());
    }

    fn storage_record(
        table_id: &'static str,
        canonical_entry_index: usize,
        literal_file_offsets: Vec<usize>,
    ) -> MainDialogueStorageRecord {
        let file_offset = literal_file_offsets[0];
        MainDialogueStorageRecord {
            table_id,
            source_prg_bank: 0,
            canonical_entry_index,
            entry_indices: vec![canonical_entry_index],
            pointer_file_offsets: Vec::new(),
            pointer_cpu_address: 0x8000 + u16::try_from(file_offset).unwrap(),
            file_offset,
            end_file_offset_exclusive: literal_file_offsets.last().unwrap() + 1,
            storage_byte_count: literal_file_offsets.len(),
            storage_sha1: "storage".to_owned(),
            prefix_byte_count: 0,
            boundary_control: 0xEF,
            literal_file_offsets,
            lines: Vec::new(),
        }
    }
}
