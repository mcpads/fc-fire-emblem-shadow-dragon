use std::collections::BTreeMap;

use anyhow::{Context, Result, ensure};

use crate::dialogue_inventory::switchable_file_to_cpu;

use super::{page_encoding::encode_page_bound_record, *};

const PRG_BANK_SIZE: usize = 16 * 1024;
const SWITCHABLE_CPU_START: u16 = 0x8000;
const EMPTY_BYTE: u8 = 0xFF;

impl MainDialogueBundlePlan {
    pub(crate) fn encoded_display_storage_by_page_groups(
        &self,
        source: &Rom,
        display: &MainDialogueDisplayPlan,
        workset_page_indices: &[usize],
        group_assignments: &[BTreeMap<char, u8>],
    ) -> Result<EncodedMainDialogueDisplayStorage> {
        ensure!(
            workset_page_indices.len() == display.page_worksets.len(),
            "display storage page-group selectors lost visible pages"
        );
        ensure!(
            display.display_paths.len() == display.display_path_count,
            "display storage path population changed"
        );
        let page_groups = page_groups_by_display_path(
            &display.page_worksets,
            workset_page_indices,
            group_assignments.len(),
        )?;
        let encoded_paths = display
            .display_paths
            .iter()
            .map(|path| {
                let path_page_groups = page_groups
                    .get(path.display_path_id.as_str())
                    .with_context(|| {
                        format!("{} has no page-group selectors", path.display_path_id)
                    })?;
                let encoded = encode_page_bound_record(
                    &path.display_path_id,
                    &path.logical_bytes,
                    &path.visible_page_ranges,
                    path_page_groups,
                    group_assignments,
                )?;
                Ok((path.display_path_id.as_str(), encoded))
            })
            .collect::<Result<BTreeMap<_, _>>>()?;
        ensure!(
            encoded_paths.len() == display.display_paths.len(),
            "display storage contains duplicate path IDs"
        );

        let paths_by_record = display.display_paths.iter().fold(
            BTreeMap::<&str, Vec<&MainDialogueDisplayPath>>::new(),
            |mut paths, path| {
                paths.entry(path.record_id.as_str()).or_default().push(path);
                paths
            },
        );
        ensure!(
            paths_by_record.len() == self.target_records.len(),
            "display storage lost canonical records"
        );

        let mut direct_regions = Vec::with_capacity(self.regions.len());
        let mut pointer_writes = Vec::new();
        let mut mirror_material_by_bank =
            BTreeMap::<u8, (Vec<u8>, Vec<bool>, Vec<Range<usize>>, usize, usize)>::new();
        let mut normalized_record_count = 0;
        for region in &self.regions {
            let mut encoded_storage = Vec::new();
            for record in &region.logical_records {
                let paths = paths_by_record
                    .get(record.id.as_str())
                    .with_context(|| format!("{} has no stored display path", record.id))?;
                let direct_path = paths
                    .iter()
                    .copied()
                    .find(|path| {
                        matches!(
                            path.mode,
                            MainDialogueDisplayMode::Canonical | MainDialogueDisplayMode::Direct
                        )
                    })
                    .with_context(|| format!("{} has no direct display path", record.id))?;
                let transition_path = paths
                    .iter()
                    .copied()
                    .find(|path| path.mode == MainDialogueDisplayMode::Transition);
                ensure!(
                    paths.len() == if transition_path.is_some() { 2 } else { 1 },
                    "{} has an unsupported display-path mode set",
                    record.id
                );
                ensure!(
                    paths
                        .iter()
                        .all(|path| path.source_prg_bank == region.source_prg_bank),
                    "{} display path moved across source-bank ownership",
                    record.id
                );
                let direct = &encoded_paths[direct_path.display_path_id.as_str()];
                let transition =
                    transition_path.map(|path| &encoded_paths[path.display_path_id.as_str()]);
                let slot_byte_count =
                    transition.map_or(direct.len(), |bytes| direct.len().max(bytes.len()));
                let placement = append_paired_display_slot(
                    &mut encoded_storage,
                    direct,
                    transition.map(Vec::as_slice),
                );
                ensure!(
                    encoded_storage.len() == placement + slot_byte_count,
                    "{} paired display slot length changed",
                    record.id
                );
                let planned_file_offset = region
                    .file_offset
                    .checked_add(placement)
                    .context("paired display-path placement overflow")?;
                let planned_pointer =
                    switchable_file_to_cpu(region.source_prg_bank, planned_file_offset)?;
                for pointer_file_offset in &record.pointer_file_offsets {
                    pointer_writes.push(MainDialoguePointerWrite {
                        record_id: record.id.clone(),
                        file_offset: *pointer_file_offset,
                        source_pointer: record.source_pointer_cpu_address,
                        planned_pointer,
                    });
                }
                if let Some(transition) = transition {
                    normalized_record_count += 1;
                    let mirror_offset = usize::from(planned_pointer - SWITCHABLE_CPU_START);
                    let mirror_end = mirror_offset
                        .checked_add(transition.len())
                        .context("transition mirror range overflow")?;
                    ensure!(
                        mirror_end <= PRG_BANK_SIZE,
                        "{} transition mirror crosses its 16 KiB bank",
                        record.id
                    );
                    let mirror = mirror_material_by_bank
                        .entry(region.source_prg_bank)
                        .or_insert_with(|| {
                            let start = usize::from(region.source_prg_bank) * PRG_BANK_SIZE;
                            let end = start + PRG_BANK_SIZE;
                            (
                                source.prg()[start..end].to_vec(),
                                vec![false; PRG_BANK_SIZE],
                                Vec::new(),
                                0,
                                0,
                            )
                        });
                    ensure!(
                        mirror.1[mirror_offset..mirror_end]
                            .iter()
                            .all(|occupied| !*occupied),
                        "{} transition mirror overlaps another record",
                        record.id
                    );
                    mirror.0[mirror_offset..mirror_end].copy_from_slice(transition);
                    mirror.1[mirror_offset..mirror_end].fill(true);
                    mirror.2.push(mirror_offset..mirror_end);
                    mirror.3 += transition.len();
                    mirror.4 += 1;
                }
            }
            let used_storage_byte_count = encoded_storage.len();
            ensure!(
                used_storage_byte_count <= region.source_storage.len(),
                "paired display-path region in PRG bank {:02X} needs {} bytes but owns only {}",
                region.source_prg_bank,
                used_storage_byte_count,
                region.source_storage.len()
            );
            encoded_storage.extend_from_slice(&region.source_storage[used_storage_byte_count..]);
            direct_regions.push(EncodedMainDialogueRegion {
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
            "paired display-path pointer coverage changed"
        );
        ensure!(
            normalized_record_count == display.dual_entry_record_count,
            "paired display-path storage lost normalized records"
        );
        let transition_mirrors = mirror_material_by_bank
            .into_iter()
            .map(
                |(
                    source_prg_bank,
                    (material, payload_occupancy, payload_ranges, payload_byte_count, record_count),
                )| {
                    ensure!(
                        payload_occupancy
                            .iter()
                            .filter(|occupied| **occupied)
                            .count()
                            == payload_byte_count,
                        "transition mirror payload occupancy changed"
                    );
                    Ok(MainDialogueTransitionMirror {
                        source_prg_bank,
                        material,
                        payload_ranges,
                        payload_byte_count,
                        record_count,
                    })
                },
            )
            .collect::<Result<Vec<_>>>()?;
        let direct_used_storage_byte_count = direct_regions
            .iter()
            .map(|region| region.used_storage_byte_count)
            .sum();
        let transition_payload_byte_count = transition_mirrors
            .iter()
            .map(|mirror| mirror.payload_byte_count)
            .sum();
        Ok(EncodedMainDialogueDisplayStorage {
            direct_regions,
            pointer_writes,
            transition_mirrors,
            direct_used_storage_byte_count,
            transition_payload_byte_count,
            normalized_record_count,
        })
    }
}

fn append_paired_display_slot(
    direct_storage: &mut Vec<u8>,
    direct: &[u8],
    transition: Option<&[u8]>,
) -> usize {
    let placement = direct_storage.len();
    let slot_byte_count = transition.map_or(direct.len(), |bytes| direct.len().max(bytes.len()));
    direct_storage.extend_from_slice(direct);
    direct_storage.resize(placement + slot_byte_count, EMPTY_BYTE);
    placement
}

fn page_groups_by_display_path<'a>(
    worksets: &'a [MainDialoguePageWorkset],
    workset_page_indices: &[usize],
    group_count: usize,
) -> Result<BTreeMap<&'a str, Vec<usize>>> {
    let mut pages = BTreeMap::<&str, Vec<(usize, usize)>>::new();
    for (workset, page_group) in worksets.iter().zip(workset_page_indices) {
        ensure!(
            *page_group < group_count,
            "{} page selects missing code group {page_group}",
            workset.display_path_id
        );
        pages
            .entry(workset.display_path_id.as_str())
            .or_default()
            .push((workset.page_index, *page_group));
    }
    pages
        .into_iter()
        .map(|(display_path_id, mut path_pages)| {
            path_pages.sort_unstable_by_key(|(page_index, _)| *page_index);
            ensure!(
                path_pages
                    .iter()
                    .enumerate()
                    .all(|(expected, (actual, _))| expected == *actual),
                "display path {display_path_id} has a page-group selector gap"
            );
            Ok((
                display_path_id,
                path_pages
                    .into_iter()
                    .map(|(_, page_group)| page_group)
                    .collect(),
            ))
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn direct_and_transition_variants_reserve_one_shared_address_slot() {
        let mut direct_storage = Vec::new();

        let first = append_paired_display_slot(
            &mut direct_storage,
            &[0x10, 0x11],
            Some(&[0x20, 0x21, 0x22]),
        );
        let second = append_paired_display_slot(
            &mut direct_storage,
            &[0x30, 0x31, 0x32, 0x33],
            Some(&[0x40]),
        );

        assert_eq!(first, 0);
        assert_eq!(second, 3);
        assert_eq!(direct_storage, [0x10, 0x11, 0xFF, 0x30, 0x31, 0x32, 0x33]);
    }
}
