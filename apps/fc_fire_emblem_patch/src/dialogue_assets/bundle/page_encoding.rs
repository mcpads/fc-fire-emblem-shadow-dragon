use std::{collections::BTreeMap, ops::Range};

use anyhow::{Context, Result, ensure};

use super::*;

impl MainDialogueBundlePlan {
    pub(crate) fn encoded_by_page_groups(
        &self,
        workset_page_indices: &[usize],
        group_assignments: &[BTreeMap<char, u8>],
    ) -> Result<EncodedMainDialogueBundle> {
        ensure!(
            workset_page_indices.len() == self.page_worksets.len(),
            "main-dialogue page-group selectors lost visible pages"
        );
        ensure!(
            workset_page_indices
                .iter()
                .all(|page| *page < group_assignments.len()),
            "main-dialogue visible page selects a missing code group"
        );
        let page_groups_by_record_id =
            page_groups_by_record_id(&self.page_worksets, workset_page_indices)?;
        ensure!(
            page_groups_by_record_id.len() == self.target_records.len(),
            "main-dialogue page-group selectors lost target records"
        );

        let mut regions = Vec::with_capacity(self.regions.len());
        let mut pointer_writes = Vec::new();
        for region in &self.regions {
            let encoded_records = region
                .logical_records
                .iter()
                .map(|record| {
                    if let Some(page_ranges) = self.visible_page_ranges_by_record_id.get(&record.id)
                    {
                        let page_groups = page_groups_by_record_id
                            .get(record.id.as_str())
                            .with_context(|| {
                                format!("{} has no page-group selectors", record.id)
                            })?;
                        encode_page_bound_record(
                            &record.id,
                            &record.bytes,
                            page_ranges,
                            page_groups,
                            group_assignments,
                        )
                    } else {
                        encode_source_record(&record.id, &record.bytes)
                    }
                })
                .collect::<Result<Vec<_>>>()?;
            let encoded_refs = encoded_records
                .iter()
                .map(Vec::as_slice)
                .collect::<Vec<_>>();
            let (mut encoded_storage, placements) = pack_record_bytes(&encoded_refs);
            let used_storage_byte_count = encoded_storage.len();
            ensure!(
                used_storage_byte_count <= region.source_storage.len(),
                "page-local main-dialogue region in PRG bank {:02X} needs {} bytes but owns only {}",
                region.source_prg_bank,
                used_storage_byte_count,
                region.source_storage.len()
            );
            encoded_storage.extend_from_slice(&region.source_storage[used_storage_byte_count..]);
            ensure!(
                encoded_storage.len() == region.source_storage.len(),
                "page-local main-dialogue encoding did not fill its exact owned region"
            );

            for (record, placement) in region.logical_records.iter().zip(placements) {
                let planned_file_offset = region
                    .file_offset
                    .checked_add(placement)
                    .context("page-local main-dialogue pointer placement overflow")?;
                let planned_pointer = crate::dialogue_inventory::switchable_file_to_cpu(
                    region.source_prg_bank,
                    planned_file_offset,
                )?;
                for pointer_file_offset in &record.pointer_file_offsets {
                    pointer_writes.push(MainDialoguePointerWrite {
                        record_id: record.id.clone(),
                        file_offset: *pointer_file_offset,
                        source_pointer: record.source_pointer_cpu_address,
                        planned_pointer,
                    });
                }
            }
            regions.push(EncodedMainDialogueRegion {
                file_offset: region.file_offset,
                source_storage: region.source_storage.clone(),
                encoded_storage,
                used_storage_byte_count,
            });
        }
        ensure!(
            pointer_writes.len()
                == self
                    .regions
                    .iter()
                    .map(|region| region.pointer_writes.len())
                    .sum::<usize>(),
            "page-local main-dialogue pointer coverage changed"
        );
        Ok(EncodedMainDialogueBundle {
            regions,
            pointer_writes,
        })
    }
}

pub(super) fn visible_page_ranges(
    source_record: &MainDialogueStorageRecord,
    workspace_record: &WorkspaceRecord,
    logical_byte_count: usize,
) -> Result<Vec<Range<usize>>> {
    ensure!(
        source_record.lines.len() == workspace_record.lines.len(),
        "{} visible-page line coverage changed",
        workspace_record.id
    );
    let mut line_ranges = Vec::with_capacity(workspace_record.lines.len());
    let mut cursor = source_record.prefix_byte_count;
    for (source_line, workspace_line) in source_record.lines.iter().zip(&workspace_record.lines) {
        let line_byte_count = if workspace_line.status == TranslationStatus::Untranslated {
            source_line.storage_byte_count
        } else {
            encode_korean_markup(&workspace_line.korean)?.len()
        };
        let end = cursor
            .checked_add(line_byte_count)
            .context("main-dialogue visible-page range overflow")?;
        line_ranges.push(cursor..end);
        cursor = end;
    }
    ensure!(
        cursor == logical_byte_count,
        "{} visible-page ranges do not cover the logical record",
        workspace_record.id
    );
    let pages = line_ranges
        .chunks(MAIN_DIALOGUE_VISIBLE_LINES_PER_PAGE)
        .enumerate()
        .map(|(page_index, lines)| {
            let first = lines.first().expect("line chunks are nonempty");
            let last = lines.last().expect("line chunks are nonempty");
            if page_index == 0 {
                0..last.end
            } else {
                first.start..last.end
            }
        })
        .collect::<Vec<_>>();
    ensure!(
        !pages.is_empty()
            && pages.first().is_some_and(|range| range.start == 0)
            && pages
                .last()
                .is_some_and(|range| range.end == logical_byte_count)
            && pages.windows(2).all(|pair| pair[0].end == pair[1].start),
        "{} visible pages do not partition the logical record",
        workspace_record.id
    );
    Ok(pages)
}

fn page_groups_by_record_id<'a>(
    worksets: &'a [MainDialoguePageWorkset],
    workset_page_indices: &[usize],
) -> Result<BTreeMap<&'a str, Vec<usize>>> {
    let mut pages = BTreeMap::<&str, Vec<(usize, usize)>>::new();
    for (workset, page_group) in worksets.iter().zip(workset_page_indices) {
        pages
            .entry(workset.record_id.as_str())
            .or_default()
            .push((workset.page_index, *page_group));
    }
    pages
        .into_iter()
        .map(|(record_id, mut record_pages)| {
            record_pages.sort_unstable_by_key(|(page_index, _)| *page_index);
            ensure!(
                record_pages
                    .iter()
                    .enumerate()
                    .all(|(expected, (actual, _))| expected == *actual),
                "main-dialogue record {record_id} has a page-group selector gap"
            );
            Ok((
                record_id,
                record_pages
                    .into_iter()
                    .map(|(_, page_group)| page_group)
                    .collect(),
            ))
        })
        .collect()
}

pub(super) fn encode_page_bound_record(
    record_id: &str,
    logical_bytes: &[LogicalDialogueByte],
    page_ranges: &[Range<usize>],
    page_groups: &[usize],
    group_assignments: &[BTreeMap<char, u8>],
) -> Result<Vec<u8>> {
    ensure!(
        page_ranges.len() == page_groups.len(),
        "{record_id} page-range and code-group counts differ"
    );
    ensure!(
        !page_ranges.is_empty()
            && page_ranges.first().is_some_and(|range| range.start == 0)
            && page_ranges
                .last()
                .is_some_and(|range| range.end == logical_bytes.len())
            && page_ranges
                .windows(2)
                .all(|pair| pair[0].end == pair[1].start),
        "{record_id} page ranges do not partition the logical bytes"
    );
    let mut encoded = Vec::with_capacity(logical_bytes.len());
    for (page_index, (range, page_group)) in page_ranges.iter().zip(page_groups).enumerate() {
        let assignments = group_assignments
            .get(*page_group)
            .with_context(|| format!("{record_id} page {page_index} selects a missing group"))?;
        for byte in &logical_bytes[range.clone()] {
            encoded.push(match byte {
                LogicalDialogueByte::Encoded(value) => *value,
                LogicalDialogueByte::TargetGlyph(glyph) => {
                    let code = assignments.get(glyph).copied().with_context(|| {
                        format!(
                            "{record_id} page {page_index} group {page_group} has no code for {glyph:?}"
                        )
                    })?;
                    ensure!(
                        !DIALOGUE_SCRIPT_CONTROL_CODES.contains(&code),
                        "{record_id} page {page_index} assigns dialogue control {code:02X} to target glyph {glyph:?}"
                    );
                    code
                }
            });
        }
    }
    ensure!(
        encoded.len() == logical_bytes.len(),
        "{record_id} page-local encoding changed record length"
    );
    Ok(encoded)
}

fn encode_source_record(record_id: &str, logical_bytes: &[LogicalDialogueByte]) -> Result<Vec<u8>> {
    logical_bytes
        .iter()
        .map(|byte| match byte {
            LogicalDialogueByte::Encoded(value) => Ok(*value),
            LogicalDialogueByte::TargetGlyph(glyph) => {
                anyhow::bail!("{record_id} target glyph {glyph:?} has no visible-page binding")
            }
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn one_record_uses_each_visible_pages_selected_code_group() {
        let logical = vec![
            LogicalDialogueByte::TargetGlyph('가'),
            LogicalDialogueByte::Encoded(0x6A),
            LogicalDialogueByte::Encoded(0xEF),
            LogicalDialogueByte::TargetGlyph('가'),
            LogicalDialogueByte::Encoded(0xEF),
        ];
        let assignments = [
            BTreeMap::from([('가', 0x10)]),
            BTreeMap::from([('가', 0x20)]),
        ];

        let encoded =
            encode_page_bound_record("record", &logical, &[0..3, 3..5], &[0, 1], &assignments)
                .unwrap();

        assert_eq!(encoded, [0x10, 0x6A, 0xEF, 0x20, 0xEF]);
    }

    #[test]
    fn suffix_sharing_compares_page_local_encoded_bytes() {
        let logical = vec![
            LogicalDialogueByte::TargetGlyph('가'),
            LogicalDialogueByte::Encoded(0xEF),
        ];
        let assignments = [
            BTreeMap::from([('가', 0x10)]),
            BTreeMap::from([('가', 0x20)]),
        ];
        let one_page = std::iter::once(0..logical.len()).collect::<Vec<_>>();
        let first =
            encode_page_bound_record("first", &logical, &one_page, &[0], &assignments).unwrap();
        let second =
            encode_page_bound_record("second", &logical, &one_page, &[1], &assignments).unwrap();

        let (storage, placements) = pack_record_bytes(&[&first, &second]);

        assert_eq!(storage, [0x10, 0xEF, 0x20, 0xEF]);
        assert_eq!(placements, [0, 2]);
    }

    #[test]
    fn target_glyph_cannot_encode_as_a_dialogue_control() {
        let logical = vec![LogicalDialogueByte::TargetGlyph('가')];
        let assignments = [BTreeMap::from([('가', 0xE0)])];

        let error = encode_page_bound_record(
            "record",
            &logical,
            std::slice::from_ref(&(0..1)),
            &[0],
            &assignments,
        )
        .unwrap_err();

        assert!(error.to_string().contains("dialogue control E0"));
    }
}
