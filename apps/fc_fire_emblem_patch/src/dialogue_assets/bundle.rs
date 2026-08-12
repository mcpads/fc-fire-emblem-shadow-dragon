use std::{
    collections::{BTreeMap, BTreeSet},
    fs,
    ops::Range,
    path::Path,
};

use anyhow::{Context, Result, ensure};

use crate::{
    dialogue_inventory::inspect_main_dialogue_storage, font_slots::active_hangul_codes,
    japanese_encoding::is_japanese_text_code, rom::Rom, sha1_hex,
};

use super::*;

mod page_encoding;
mod paired_entry_storage;
mod region;
mod validation;

use page_encoding::visible_page_ranges;
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
    pub(crate) source_reclaimable_active_codes: BTreeSet<u8>,
    pub(crate) page_worksets: Vec<MainDialoguePageWorkset>,
    target_records: Vec<LogicalDialogueRecord>,
    visible_page_ranges_by_record_id: BTreeMap<String, Vec<Range<usize>>>,
    regions: Vec<LogicalBundleRegion>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum MainDialogueDisplayMode {
    Canonical,
    Direct,
    Transition,
}

#[derive(Clone)]
pub(crate) struct MainDialogueDisplayPath {
    pub(crate) record_id: String,
    pub(crate) display_path_id: String,
    pub(crate) source_prg_bank: u8,
    pub(crate) mode: MainDialogueDisplayMode,
    pub(crate) logical_bytes: Vec<LogicalDialogueByte>,
    pub(crate) visible_page_ranges: Vec<Range<usize>>,
}

#[derive(Clone)]
pub(crate) struct MainDialoguePageWorkset {
    pub(crate) record_id: String,
    pub(crate) display_path_id: String,
    pub(crate) page_index: usize,
    pub(crate) target_glyphs: BTreeSet<char>,
    pub(crate) dynamic_string_selectors: BTreeSet<u8>,
    pub(crate) dynamic_string_selector_counts: BTreeMap<u8, usize>,
    pub(crate) dynamic_string_control_count: usize,
    pub(crate) source_reclaimable_active_codes: BTreeSet<u8>,
    pub(crate) preserved_target_active_codes: BTreeSet<u8>,
}

pub(crate) struct MainDialogueRegionStorageBudget {
    pub(crate) source_prg_bank: u8,
    pub(crate) capacity_byte_count: usize,
    pub(crate) used_byte_count: usize,
    pub(crate) logical_record_byte_counts: BTreeMap<String, usize>,
}

impl MainDialogueBundlePlan {
    pub(crate) fn canonical_display_paths(&self) -> Result<Vec<MainDialogueDisplayPath>> {
        self.target_records
            .iter()
            .map(|record| {
                let visible_page_ranges = self
                    .visible_page_ranges_by_record_id
                    .get(&record.id)
                    .with_context(|| format!("{} has no canonical visible pages", record.id))?
                    .clone();
                Ok(MainDialogueDisplayPath {
                    record_id: record.id.clone(),
                    display_path_id: record.id.clone(),
                    source_prg_bank: record.source_prg_bank,
                    mode: MainDialogueDisplayMode::Canonical,
                    logical_bytes: record.bytes.clone(),
                    visible_page_ranges,
                })
            })
            .collect()
    }

    pub(crate) fn logical_record_byte_counts(&self) -> BTreeMap<&str, usize> {
        self.target_records
            .iter()
            .map(|record| (record.id.as_str(), record.bytes.len()))
            .collect()
    }

    pub(crate) fn region_storage_budgets(&self) -> Vec<MainDialogueRegionStorageBudget> {
        self.regions
            .iter()
            .map(|region| MainDialogueRegionStorageBudget {
                source_prg_bank: region.source_prg_bank,
                capacity_byte_count: region.source_storage.len(),
                used_byte_count: region.used_storage_byte_count,
                logical_record_byte_counts: region
                    .logical_records
                    .iter()
                    .map(|record| (record.id.clone(), record.bytes.len()))
                    .collect(),
            })
            .collect()
    }

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
                    used_storage_byte_count: region.used_storage_byte_count,
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
    pub(crate) used_storage_byte_count: usize,
}

pub(crate) struct EncodedMainDialogueDisplayStorage {
    pub(crate) direct_regions: Vec<EncodedMainDialogueRegion>,
    pub(crate) pointer_writes: Vec<MainDialoguePointerWrite>,
    pub(crate) transition_mirrors: Vec<MainDialogueTransitionMirror>,
    pub(crate) direct_used_storage_byte_count: usize,
    pub(crate) transition_payload_byte_count: usize,
    pub(crate) normalized_record_count: usize,
}

pub(crate) struct MainDialogueTransitionMirror {
    pub(crate) source_prg_bank: u8,
    pub(crate) material: Vec<u8>,
    pub(crate) payload_byte_count: usize,
    pub(crate) record_count: usize,
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
    source_prg_bank: u8,
    source_storage: Vec<u8>,
    logical_storage: Vec<LogicalDialogueByte>,
    logical_records: Vec<LogicalDialogueRecord>,
    used_storage_byte_count: usize,
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
    validate_target_records(&workspace, &source_records, &requested)?;
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
    let visible_page_ranges_by_record_id = target_indices
        .iter()
        .zip(&target_records)
        .map(|(index, record)| {
            Ok((
                record.id.clone(),
                visible_page_ranges(
                    &source_records[*index],
                    &workspace.records[*index],
                    record.bytes.len(),
                )?,
            ))
        })
        .collect::<Result<BTreeMap<_, _>>>()?;
    let page_worksets = target_indices
        .iter()
        .flat_map(|index| {
            record_page_worksets(
                rom.data(),
                &source_records[*index],
                &workspace.records[*index],
            )
        })
        .collect::<Result<Vec<_>>>()?;
    ensure!(
        !page_worksets.is_empty(),
        "main dialogue bundle has no visible page worksets"
    );
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
    let active_codes = active_hangul_codes().into_iter().collect::<BTreeSet<_>>();
    let mut source_reclaimable_active_codes = BTreeSet::new();
    for index in &target_indices {
        for (source_line, workspace_line) in source_records[*index]
            .lines
            .iter()
            .zip(&workspace.records[*index].lines)
        {
            if workspace_line.status == TranslationStatus::Untranslated {
                continue;
            }
            for file_offset in &source_line.literal_file_offsets {
                let code = *rom
                    .data()
                    .get(*file_offset)
                    .context("main dialogue literal reclamation offset is outside the ROM")?;
                if is_japanese_text_code(code) && active_codes.contains(&code) {
                    source_reclaimable_active_codes.insert(code);
                }
            }
        }
    }
    source_reclaimable_active_codes.retain(|code| !preserved_source_codes.contains(code));
    ensure!(
        !source_reclaimable_active_codes.is_empty(),
        "main dialogue bundle has no exact source Japanese codes to reclaim"
    );

    Ok(MainDialogueBundlePlan {
        workspace_sha1: sha1_hex(&workspace_bytes),
        record_ids: record_ids.iter().map(|id| (*id).to_owned()).collect(),
        translated_line_count,
        source_record_storage_byte_count,
        planned_record_storage_byte_count,
        preserved_source_codes,
        source_reclaimable_active_codes,
        page_worksets,
        target_records,
        visible_page_ranges_by_record_id,
        regions,
    })
}

pub(crate) fn plan_all_main_dialogue_records(
    rom: &Rom,
    workspace_path: &Path,
) -> Result<MainDialogueBundlePlan> {
    let workspace_bytes = fs::read(workspace_path)
        .with_context(|| format!("read main dialogue workspace {}", workspace_path.display()))?;
    let workspace: MainDialogueWorkspace = serde_json::from_slice(&workspace_bytes)
        .with_context(|| format!("parse main dialogue workspace {}", workspace_path.display()))?;
    ensure!(
        workspace.records.len() == 504,
        "all-record main dialogue installation must contain exactly 504 records"
    );
    let record_ids = workspace
        .records
        .iter()
        .map(|record| record.id.as_str())
        .collect::<Vec<_>>();
    let plan = plan_main_dialogue_bundle(rom, workspace_path, &record_ids)?;
    ensure!(
        plan.record_ids.len() == workspace.records.len(),
        "all-record main dialogue installation lost records"
    );
    Ok(plan)
}

fn record_page_worksets<'a>(
    source: &'a [u8],
    source_record: &'a MainDialogueStorageRecord,
    workspace_record: &'a WorkspaceRecord,
) -> impl Iterator<Item = Result<MainDialoguePageWorkset>> + 'a {
    let active_codes = active_hangul_codes().into_iter().collect::<BTreeSet<_>>();
    let prefix_uses_dynamic_output = source
        .get(source_record.file_offset..source_record.file_offset + source_record.prefix_byte_count)
        .is_some_and(|prefix| prefix.contains(&DIALOGUE_PREFIX_CONTROL_CODE));
    source_record
        .lines
        .chunks(MAIN_DIALOGUE_VISIBLE_LINES_PER_PAGE)
        .zip(
            workspace_record
                .lines
                .chunks(MAIN_DIALOGUE_VISIBLE_LINES_PER_PAGE),
        )
        .enumerate()
        .map(move |(page_index, (source_lines, workspace_lines))| {
            ensure!(
                source_lines.len() == workspace_lines.len(),
                "{} visible-page source and workspace line counts differ",
                workspace_record.id
            );
            let mut target_glyphs = BTreeSet::new();
            let mut dynamic_string_selector_counts = BTreeMap::new();
            let mut dynamic_string_control_count = 0;
            let mut preserved_target_active_codes = BTreeSet::new();
            let mut source_reclaimable_active_codes = BTreeSet::new();
            for (source_line, workspace_line) in source_lines.iter().zip(workspace_lines) {
                if workspace_line.status == TranslationStatus::Untranslated {
                    continue;
                }
                let logical_line = encode_korean_markup(&workspace_line.korean)?;
                let line_selectors = dynamic_string_controls(&logical_line)?;
                let line_control_count = line_selectors.values().sum::<usize>();
                for (selector, count) in line_selectors {
                    *dynamic_string_selector_counts.entry(selector).or_default() += count;
                }
                dynamic_string_control_count += line_control_count;
                for byte in logical_line {
                    match byte {
                        LogicalDialogueByte::TargetGlyph(glyph) => {
                            target_glyphs.insert(glyph);
                        }
                        LogicalDialogueByte::Encoded(code) if active_codes.contains(&code) => {
                            preserved_target_active_codes.insert(code);
                        }
                        LogicalDialogueByte::Encoded(_) => {}
                    }
                }
                for file_offset in &source_line.literal_file_offsets {
                    let code = *source
                        .get(*file_offset)
                        .context("main-dialogue page literal is outside the ROM")?;
                    if is_japanese_text_code(code) && active_codes.contains(&code) {
                        source_reclaimable_active_codes.insert(code);
                    }
                }
            }
            if prefix_uses_dynamic_output {
                preserved_target_active_codes.extend(DIALOGUE_PREFIX_OUTPUT_CODES);
            }
            source_reclaimable_active_codes
                .retain(|code| !preserved_target_active_codes.contains(code));
            Ok(MainDialoguePageWorkset {
                record_id: workspace_record.id.clone(),
                display_path_id: workspace_record.id.clone(),
                page_index,
                target_glyphs,
                dynamic_string_selectors: dynamic_string_selector_counts.keys().copied().collect(),
                dynamic_string_selector_counts,
                dynamic_string_control_count,
                source_reclaimable_active_codes,
                preserved_target_active_codes,
            })
        })
}

pub(super) fn dynamic_string_controls(
    bytes: &[LogicalDialogueByte],
) -> Result<BTreeMap<u8, usize>> {
    let mut selectors = BTreeMap::new();
    let mut index = 0;
    while index < bytes.len() {
        if bytes[index] != LogicalDialogueByte::Encoded(0xEC) {
            index += 1;
            continue;
        }
        let selector = match bytes.get(index + 1) {
            Some(LogicalDialogueByte::Encoded(selector)) if *selector <= 3 => *selector,
            _ => anyhow::bail!("main-dialogue EC control lost its selector operand"),
        };
        *selectors.entry(selector).or_default() += 1;
        index += 2;
    }
    Ok(selectors)
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn dynamic_string_inventory_counts_controls_and_unique_selectors() {
        let logical = encode_korean_markup("{EC:00}한{EC:00}{EC:02}{EF}").unwrap();

        let selectors = dynamic_string_controls(&logical).unwrap();

        assert_eq!(selectors, BTreeMap::from([(0, 2), (2, 1)]));
    }
}
