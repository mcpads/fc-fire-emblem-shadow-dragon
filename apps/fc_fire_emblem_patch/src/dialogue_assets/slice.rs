use std::{
    collections::{BTreeMap, BTreeSet},
    fs,
    ops::Range,
    path::Path,
};

use anyhow::{Context, Result, ensure};

#[cfg(test)]
use crate::dialogue_inventory::MainDialogueTransitionEdgeReport;
use crate::{
    dialogue_inventory::MainDialogueGraphReport,
    font::{load_dalmoori, rasterize_glyph},
    font_slots::{FONT_PAGE_SIZE, FONT_TILE_SIZE, active_hangul_codes},
    rom::Rom,
    sha1_hex,
};

use super::*;

const DIALOGUE_PREFIX_CONTROL_CODE: u8 = 0xEA;
const DIALOGUE_PREFIX_OUTPUT_CODES: [u8; 2] = [0x9E, 0xAB];
const PAGE_BOUNDARY_TOPOLOGY_DIGEST_VERSION: u8 = 1;
const PAGE_BOUNDARY_TOPOLOGY_DIGEST_DOMAIN: &[u8] =
    b"fc-fire-emblem/main-dialogue-page-boundary-topology";

#[derive(Debug)]
pub(crate) struct MainDialoguePageBoundaryTopologySummary {
    pub(crate) workspace_sha1: String,
    pub(crate) record_id: String,
    pub(crate) topology_sha1: String,
    pub(crate) source_pointer_cpu_address: u16,
    pub(crate) logical_byte_count: usize,
    pub(crate) line_count: usize,
}

pub(crate) struct MainDialogueSlicePlan {
    pub(crate) workspace_sha1: String,
    pub(crate) record_id: String,
    pub(crate) source_file_offset: usize,
    pub(crate) source_storage_byte_count: usize,
    pub(crate) translated_line_count: usize,
    pub(crate) transition_chain_record_count: usize,
    pub(crate) preserved_source_codes: BTreeSet<u8>,
    source_pointer_cpu_address: u16,
    logical_line_ranges: Vec<Range<usize>>,
    logical_bytes: Vec<LogicalDialogueByte>,
}

impl MainDialogueSlicePlan {
    pub(crate) fn page_boundary_topology_sha1(&self) -> String {
        let mut canonical = Vec::new();
        append_digest_field(&mut canonical, PAGE_BOUNDARY_TOPOLOGY_DIGEST_DOMAIN);
        canonical.push(PAGE_BOUNDARY_TOPOLOGY_DIGEST_VERSION);
        append_digest_field(&mut canonical, self.record_id.as_bytes());
        canonical.extend_from_slice(&(self.source_file_offset as u64).to_le_bytes());
        canonical.extend_from_slice(&(self.source_storage_byte_count as u64).to_le_bytes());
        canonical.extend_from_slice(&self.source_pointer_cpu_address.to_le_bytes());
        canonical.extend_from_slice(&(self.translated_line_count as u64).to_le_bytes());
        canonical.extend_from_slice(&(self.logical_line_ranges.len() as u64).to_le_bytes());
        for range in &self.logical_line_ranges {
            canonical.extend_from_slice(&(range.start as u64).to_le_bytes());
            canonical.extend_from_slice(&(range.end as u64).to_le_bytes());
        }
        canonical.extend_from_slice(&(self.logical_bytes.len() as u64).to_le_bytes());
        for byte in &self.logical_bytes {
            match byte {
                LogicalDialogueByte::Encoded(value) => {
                    canonical.push(0);
                    canonical.push(*value);
                }
                LogicalDialogueByte::TargetGlyph(_) => {
                    canonical.push(1);
                }
            }
        }
        sha1_hex(&canonical)
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
        encode_logical_bytes(&self.logical_bytes, assignments)
    }

    pub(crate) fn line_count(&self) -> usize {
        self.logical_line_ranges.len()
    }

    pub(crate) fn completed_page_pointers(&self, lines_per_page: usize) -> Result<Vec<u16>> {
        ensure!(
            lines_per_page > 0,
            "dialogue page must contain at least one line"
        );
        ensure!(
            !self.logical_line_ranges.is_empty(),
            "dialogue has no logical lines to partition"
        );

        let mut pointers =
            Vec::with_capacity(self.logical_line_ranges.len().div_ceil(lines_per_page));
        for lines in self.logical_line_ranges.chunks(lines_per_page) {
            let logical_end = lines
                .last()
                .context("dialogue page lost its final logical line")?
                .end;
            let pointer_offset = u16::try_from(logical_end)
                .context("dialogue completed-page offset does not fit u16")?;
            pointers.push(
                self.source_pointer_cpu_address
                    .checked_add(pointer_offset)
                    .context("dialogue completed-page pointer overflow")?,
            );
        }
        ensure!(
            pointers.last().copied()
                == self.source_pointer_cpu_address.checked_add(
                    u16::try_from(self.logical_bytes.len())
                        .context("dialogue logical byte count does not fit u16")?,
                ),
            "dialogue derived page pointers do not consume the logical record"
        );
        Ok(pointers)
    }

    pub(crate) fn page_unique_glyphs(
        &self,
        completed_page_pointers: &[u16],
    ) -> Result<Vec<BTreeSet<char>>> {
        Ok(self
            .page_byte_ranges(completed_page_pointers)?
            .into_iter()
            .map(|range| {
                self.logical_bytes[range]
                    .iter()
                    .filter_map(|byte| match byte {
                        LogicalDialogueByte::TargetGlyph(character) => Some(*character),
                        LogicalDialogueByte::Encoded(_) => None,
                    })
                    .collect()
            })
            .collect())
    }

    pub(crate) fn encoded_bytes_by_page_group(
        &self,
        completed_page_pointers: &[u16],
        page_groups: &[usize],
        group_assignments: &[BTreeMap<char, u8>],
    ) -> Result<Vec<u8>> {
        let page_ranges = self.page_byte_ranges(completed_page_pointers)?;
        ensure!(
            page_groups.len() == page_ranges.len(),
            "dialogue page-group coverage changed"
        );
        ensure!(
            page_groups
                .iter()
                .all(|group| *group < group_assignments.len()),
            "dialogue page selects a missing assignment group"
        );

        let mut encoded = Vec::with_capacity(self.logical_bytes.len());
        for (page_index, range) in page_ranges.into_iter().enumerate() {
            encoded.extend(encode_logical_bytes(
                &self.logical_bytes[range],
                &group_assignments[page_groups[page_index]],
            )?);
        }
        ensure!(
            encoded.len() == self.logical_bytes.len(),
            "dialogue page ranges do not consume the logical record"
        );
        Ok(encoded)
    }

    pub(crate) fn verify_encoded_page_rendering(
        &self,
        encoded_record: &[u8],
        completed_page_pointers: &[u16],
        page_groups: &[usize],
        font_pages: &[&[u8]],
    ) -> Result<usize> {
        ensure!(
            encoded_record.len() == self.logical_bytes.len(),
            "dialogue encoded record length changed"
        );
        ensure!(
            font_pages.iter().all(|page| page.len() == FONT_PAGE_SIZE),
            "dialogue font page length changed"
        );
        let page_ranges = self.page_byte_ranges(completed_page_pointers)?;
        ensure!(
            page_groups.len() == page_ranges.len(),
            "dialogue page-group coverage changed"
        );
        ensure!(
            page_groups.iter().all(|group| *group < font_pages.len()),
            "dialogue page selects a missing font page"
        );

        let active_codes = active_hangul_codes().into_iter().collect::<BTreeSet<_>>();
        let font = load_dalmoori()?;
        let mut target_glyph_byte_count = 0;
        for (page_index, range) in page_ranges.into_iter().enumerate() {
            let font_page = font_pages[page_groups[page_index]];
            for logical_index in range {
                let encoded = encoded_record[logical_index];
                match self.logical_bytes[logical_index] {
                    LogicalDialogueByte::Encoded(expected) => ensure!(
                        encoded == expected,
                        "dialogue preserved byte changed at logical offset {logical_index}"
                    ),
                    LogicalDialogueByte::TargetGlyph(character) => {
                        ensure!(
                            active_codes.contains(&encoded),
                            "dialogue target glyph uses an inactive code at logical offset {logical_index}"
                        );
                        let tile_start = usize::from(encoded) * FONT_TILE_SIZE;
                        let tile_end = tile_start + FONT_TILE_SIZE;
                        let actual = font_page
                            .get(tile_start..tile_end)
                            .context("dialogue target glyph tile is outside its font page")?;
                        ensure!(
                            actual == rasterize_glyph(&font, character)?,
                            "dialogue target glyph raster changed at logical offset {logical_index}"
                        );
                        target_glyph_byte_count += 1;
                    }
                }
            }
        }
        Ok(target_glyph_byte_count)
    }

    pub(crate) fn verify_encoded_page_topology(
        &self,
        encoded_record: &[u8],
        completed_page_pointers: &[u16],
    ) -> Result<usize> {
        ensure!(
            encoded_record.len() == self.logical_bytes.len(),
            "dialogue encoded record length changed"
        );
        let active_codes = active_hangul_codes().into_iter().collect::<BTreeSet<_>>();
        let mut target_glyph_byte_count = 0;
        for range in self.page_byte_ranges(completed_page_pointers)? {
            for logical_index in range {
                let encoded = encoded_record[logical_index];
                match self.logical_bytes[logical_index] {
                    LogicalDialogueByte::Encoded(expected) => ensure!(
                        encoded == expected,
                        "dialogue preserved byte changed at logical offset {logical_index}"
                    ),
                    LogicalDialogueByte::TargetGlyph(_) => {
                        ensure!(
                            active_codes.contains(&encoded),
                            "dialogue target glyph uses an inactive code at logical offset {logical_index}"
                        );
                        target_glyph_byte_count += 1;
                    }
                }
            }
        }
        Ok(target_glyph_byte_count)
    }

    pub(crate) fn source_pointer_cpu_address(&self) -> u16 {
        self.source_pointer_cpu_address
    }

    fn page_byte_ranges(&self, completed_page_pointers: &[u16]) -> Result<Vec<Range<usize>>> {
        ensure!(
            !completed_page_pointers.is_empty(),
            "dialogue has no completed-page pointers"
        );
        let mut ranges = Vec::with_capacity(completed_page_pointers.len());
        let mut start = 0usize;
        for pointer in completed_page_pointers {
            let end = usize::from(
                pointer
                    .checked_sub(self.source_pointer_cpu_address)
                    .context("dialogue completed-page pointer precedes the record")?,
            );
            ensure!(
                start < end && end <= self.logical_bytes.len(),
                "dialogue completed-page pointers do not form increasing in-record ranges"
            );
            ranges.push(start..end);
            start = end;
        }
        ensure!(
            start == self.logical_bytes.len(),
            "dialogue final completed-page pointer does not consume the record"
        );
        Ok(ranges)
    }
}

fn append_digest_field(output: &mut Vec<u8>, value: &[u8]) {
    output.extend_from_slice(&(value.len() as u64).to_le_bytes());
    output.extend_from_slice(value);
}

fn encode_logical_bytes(
    bytes: &[LogicalDialogueByte],
    assignments: &BTreeMap<char, u8>,
) -> Result<Vec<u8>> {
    bytes
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
    let mut logical_line_ranges = Vec::with_capacity(workspace_record.lines.len());
    let mut logical_cursor = source_record.prefix_byte_count;
    for (source_line, workspace_line) in source_record.lines.iter().zip(&workspace_record.lines) {
        let logical_line_len = if workspace_line.status == TranslationStatus::Untranslated {
            source_line.storage_byte_count
        } else {
            encode_korean_markup(&workspace_line.korean)?.len()
        };
        let logical_end = logical_cursor
            .checked_add(logical_line_len)
            .context("main dialogue logical line range overflow")?;
        logical_line_ranges.push(logical_cursor..logical_end);
        logical_cursor = logical_end;
    }
    ensure!(
        logical_cursor == logical.bytes.len(),
        "main dialogue logical line ranges do not cover the record"
    );
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
        source_pointer_cpu_address: logical.source_pointer_cpu_address,
        logical_line_ranges,
        logical_bytes: logical.bytes,
    })
}

pub(crate) fn summarize_main_dialogue_page_boundary_topology(
    source_path: &Path,
    workspace_path: &Path,
    record_id: &str,
) -> Result<MainDialoguePageBoundaryTopologySummary> {
    let rom = Rom::from_path(source_path)?;
    let plan = plan_main_dialogue_slice(&rom, workspace_path, record_id)?;
    Ok(MainDialoguePageBoundaryTopologySummary {
        workspace_sha1: plan.workspace_sha1.clone(),
        record_id: plan.record_id.clone(),
        topology_sha1: plan.page_boundary_topology_sha1(),
        source_pointer_cpu_address: plan.source_pointer_cpu_address(),
        logical_byte_count: plan.logical_bytes.len(),
        line_count: plan.line_count(),
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
            source_pointer_cpu_address: 0x8000,
            logical_line_ranges: std::iter::once(0..2).collect(),
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
    fn completed_page_boundaries_drive_storage_codes_and_glyph_sets() {
        let plan = MainDialogueSlicePlan {
            workspace_sha1: "workspace".to_owned(),
            record_id: "record".to_owned(),
            source_file_offset: 0,
            source_storage_byte_count: 8,
            translated_line_count: 3,
            transition_chain_record_count: 1,
            preserved_source_codes: BTreeSet::new(),
            source_pointer_cpu_address: 0x8FF0,
            logical_line_ranges: vec![1..3, 3..5, 5..7],
            logical_bytes: vec![
                LogicalDialogueByte::Encoded(0xE5),
                LogicalDialogueByte::TargetGlyph('가'),
                LogicalDialogueByte::Encoded(0xED),
                LogicalDialogueByte::TargetGlyph('나'),
                LogicalDialogueByte::Encoded(0xED),
                LogicalDialogueByte::TargetGlyph('다'),
                LogicalDialogueByte::Encoded(0xEF),
            ],
        };
        let assignments = [
            BTreeMap::from([('가', 0x01), ('나', 0x02)]),
            BTreeMap::from([('다', 0x01)]),
        ];
        let completed_page_pointers = [0x8FF4, 0x8FF7];

        assert_eq!(
            plan.encoded_bytes_by_page_group(&completed_page_pointers, &[0, 1], &assignments,)
                .unwrap(),
            [0xE5, 0x01, 0xED, 0x02, 0xED, 0x01, 0xEF]
        );
        assert_eq!(
            plan.page_unique_glyphs(&completed_page_pointers)
                .unwrap()
                .into_iter()
                .map(|set| set.len())
                .collect::<Vec<_>>(),
            [2, 1]
        );
    }

    #[test]
    fn completed_page_pointers_follow_current_line_lengths() {
        let baseline = MainDialogueSlicePlan {
            workspace_sha1: "workspace-a".to_owned(),
            record_id: "record".to_owned(),
            source_file_offset: 0,
            source_storage_byte_count: 16,
            translated_line_count: 3,
            transition_chain_record_count: 1,
            preserved_source_codes: BTreeSet::new(),
            source_pointer_cpu_address: 0x8FF0,
            logical_line_ranges: vec![1..3, 3..5, 5..7],
            logical_bytes: vec![LogicalDialogueByte::Encoded(0); 7],
        };
        let reflowed = MainDialogueSlicePlan {
            workspace_sha1: "workspace-b".to_owned(),
            logical_line_ranges: vec![1..4, 4..7, 7..10],
            logical_bytes: vec![LogicalDialogueByte::Encoded(0); 10],
            ..baseline
        };

        assert_eq!(
            reflowed.completed_page_pointers(2).unwrap(),
            [0x8FF7, 0x8FFA]
        );
        assert_ne!(
            reflowed.completed_page_pointers(2).unwrap(),
            [0x8FF5, 0x8FF7]
        );
        assert!(reflowed.completed_page_pointers(0).is_err());
    }

    #[test]
    fn page_boundary_topology_ignores_workspace_and_target_glyph_changes() {
        let first = boundary_digest_plan(
            "workspace-a",
            vec![
                LogicalDialogueByte::TargetGlyph('가'),
                LogicalDialogueByte::Encoded(0xED),
            ],
            vec![0..2],
        );
        let second = boundary_digest_plan(
            "workspace-b",
            vec![
                LogicalDialogueByte::TargetGlyph('가'),
                LogicalDialogueByte::Encoded(0xED),
            ],
            vec![0..2],
        );

        assert_eq!(
            first.page_boundary_topology_sha1(),
            second.page_boundary_topology_sha1()
        );
        let changed_glyph = boundary_digest_plan(
            "workspace-c",
            vec![
                LogicalDialogueByte::TargetGlyph('나'),
                LogicalDialogueByte::Encoded(0xED),
            ],
            vec![0..2],
        );
        assert_eq!(
            first.page_boundary_topology_sha1(),
            changed_glyph.page_boundary_topology_sha1()
        );
    }

    #[test]
    fn page_boundary_topology_changes_with_control_bytes_or_line_boundaries() {
        let baseline = boundary_digest_plan(
            "workspace",
            vec![
                LogicalDialogueByte::TargetGlyph('가'),
                LogicalDialogueByte::Encoded(0xED),
            ],
            vec![0..2],
        );
        let changed_kind = boundary_digest_plan(
            "workspace",
            vec![
                LogicalDialogueByte::Encoded(0x00),
                LogicalDialogueByte::Encoded(0xED),
            ],
            vec![0..2],
        );
        let changed_lines = boundary_digest_plan(
            "workspace",
            vec![
                LogicalDialogueByte::TargetGlyph('가'),
                LogicalDialogueByte::Encoded(0xED),
            ],
            vec![0..1, 1..2],
        );

        assert_ne!(
            baseline.page_boundary_topology_sha1(),
            changed_kind.page_boundary_topology_sha1()
        );
        assert_ne!(
            baseline.page_boundary_topology_sha1(),
            changed_lines.page_boundary_topology_sha1()
        );
    }

    #[test]
    fn observed_page_boundaries_must_consume_the_whole_record_once() {
        let plan = MainDialogueSlicePlan {
            workspace_sha1: "workspace".to_owned(),
            record_id: "record".to_owned(),
            source_file_offset: 0,
            source_storage_byte_count: 3,
            translated_line_count: 1,
            transition_chain_record_count: 1,
            preserved_source_codes: BTreeSet::new(),
            source_pointer_cpu_address: 0x8000,
            logical_line_ranges: std::iter::once(0..3).collect(),
            logical_bytes: vec![
                LogicalDialogueByte::TargetGlyph('가'),
                LogicalDialogueByte::TargetGlyph('나'),
                LogicalDialogueByte::Encoded(0xEF),
            ],
        };

        assert!(plan.page_unique_glyphs(&[0x8002]).is_err());
        assert!(plan.page_unique_glyphs(&[0x8002, 0x8001]).is_err());
        assert!(plan.page_unique_glyphs(&[0x7FFF, 0x8003]).is_err());
        assert_eq!(
            plan.page_unique_glyphs(&[0x8002, 0x8003])
                .unwrap()
                .into_iter()
                .map(|set| set.len())
                .collect::<Vec<_>>(),
            [2, 0]
        );
    }

    #[test]
    fn page_topology_accepts_reassigned_target_codes_but_rejects_structural_drift() {
        let plan = MainDialogueSlicePlan {
            workspace_sha1: "workspace".to_owned(),
            record_id: "record".to_owned(),
            source_file_offset: 0,
            source_storage_byte_count: 3,
            translated_line_count: 1,
            transition_chain_record_count: 1,
            preserved_source_codes: BTreeSet::new(),
            source_pointer_cpu_address: 0x8000,
            logical_line_ranges: std::iter::once(0..3).collect(),
            logical_bytes: vec![
                LogicalDialogueByte::TargetGlyph('가'),
                LogicalDialogueByte::Encoded(0xED),
                LogicalDialogueByte::TargetGlyph('나'),
            ],
        };

        assert_eq!(
            plan.verify_encoded_page_topology(&[0x00, 0xED, 0x01], &[0x8003])
                .unwrap(),
            2
        );
        assert!(
            plan.verify_encoded_page_topology(&[0x00, 0xEF, 0x01], &[0x8003])
                .is_err()
        );
        assert!(
            plan.verify_encoded_page_topology(&[0x00, 0xED, 0x60], &[0x8003])
                .is_err()
        );
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

    fn boundary_digest_plan(
        workspace_sha1: &str,
        logical_bytes: Vec<LogicalDialogueByte>,
        logical_line_ranges: Vec<Range<usize>>,
    ) -> MainDialogueSlicePlan {
        MainDialogueSlicePlan {
            workspace_sha1: workspace_sha1.to_owned(),
            record_id: "record".to_owned(),
            source_file_offset: 0x1234,
            source_storage_byte_count: 8,
            translated_line_count: logical_line_ranges.len(),
            transition_chain_record_count: 1,
            preserved_source_codes: BTreeSet::new(),
            source_pointer_cpu_address: 0x8FF0,
            logical_line_ranges,
            logical_bytes,
        }
    }
}
