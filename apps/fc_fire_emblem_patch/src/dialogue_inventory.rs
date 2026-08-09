mod battle_dialogue;
mod main_dialogue;
mod report;
mod source_binding;
mod source_spec;
#[cfg(test)]
mod tests;

use battle_dialogue::*;
use main_dialogue::*;
pub(crate) use report::*;
pub(crate) use source_binding::switchable_file_to_cpu;
use source_binding::{
    extract_dialogue_table, fixed_cpu_to_file_offset, switchable_bank_file_start,
    switchable_cpu_to_file_offset,
};
use source_spec::*;

use std::{
    collections::{BTreeMap, BTreeSet},
    fs,
    path::Path,
};

use anyhow::{Context, Result, ensure};
use serde::Serialize;

use crate::{
    japanese_encoding::is_japanese_text_code,
    rom::{EXPECTED_SOURCE_SHA1, HEADER_SIZE, PRG_SIZE, Rom},
    sha1_hex,
    source_literals::{
        TranslationSurfaceLiteralInventory, classify_translation_surface_literal_codes,
    },
    text_inventory::{DIALOGUE_CONTROL_SPECS, DIALOGUE_SCRIPT_CONTROL_CODES},
    typed_source::decode_rp2a03_sequence,
};

#[derive(Debug)]
pub struct DialogueStructureSummary {
    pub report_sha1: String,
    pub table_count: usize,
    pub pointer_count: usize,
    pub unique_target_count: usize,
    pub alias_group_count: usize,
}

pub fn analyze_dialogue_structure(
    source_path: &Path,
    report_path: &Path,
) -> Result<DialogueStructureSummary> {
    let rom = Rom::from_path(source_path)?;
    rom.verify_supported_japanese()?;
    let report = build_report(rom.data())?;
    let mut report_bytes =
        serde_json::to_vec_pretty(&report).context("serialize dialogue structure report")?;
    report_bytes.push(b'\n');

    if let Some(parent) = report_path.parent() {
        fs::create_dir_all(parent)
            .with_context(|| format!("create output directory {}", parent.display()))?;
    }
    fs::write(report_path, &report_bytes)
        .with_context(|| format!("write {}", report_path.display()))?;

    Ok(DialogueStructureSummary {
        report_sha1: sha1_hex(&report_bytes),
        table_count: report.summary.table_count,
        pointer_count: report.summary.pointer_count,
        unique_target_count: report.summary.unique_target_count,
        alias_group_count: report.summary.alias_group_count,
    })
}

pub(crate) fn inspect_main_dialogue_storage(
    source: &[u8],
) -> Result<MainDialogueStorageInspection> {
    let report = build_report(source)?;
    let records = report
        .tables
        .iter()
        .filter(|table| table.directory_binding.is_some())
        .flat_map(|table| {
            table
                .entries
                .iter()
                .filter(|entry| {
                    entry.target_kind == "script_entry_start" && is_canonical_dialogue_entry(entry)
                })
                .map(move |entry| (table, entry))
        })
        .map(|(table, entry)| {
            let storage = entry.main_record_storage.as_ref().with_context(|| {
                format!(
                    "{} canonical entry {} has no record-storage range",
                    table.id, entry.index
                )
            })?;
            let literal_file_offsets = entry
                .main_linear_segment
                .as_ref()
                .context("canonical main dialogue entry has no linear segment")?
                .lines
                .iter()
                .flat_map(|line| line.literal_file_offsets.iter().copied())
                .collect();
            let lines = entry
                .main_linear_segment
                .as_ref()
                .context("canonical main dialogue entry has no linear segment")?
                .lines
                .iter()
                .map(|line| MainDialogueStorageLine {
                    file_offset: line.file_offset,
                    storage_byte_count: line.storage_byte_count,
                    storage_sha1: line.storage_sha1.clone(),
                    line_end_control: line.line_end_control,
                    literal_file_offsets: line.literal_file_offsets.clone(),
                })
                .collect();
            Ok(MainDialogueStorageRecord {
                table_id: table.id,
                source_prg_bank: table.source_prg_bank,
                canonical_entry_index: canonical_dialogue_entry_index(entry),
                entry_indices: dialogue_entry_indices(entry),
                pointer_file_offsets: dialogue_entry_indices(entry)
                    .iter()
                    .map(|index| table.pointer_table_file_offset + index * 2)
                    .collect(),
                pointer_cpu_address: entry.pointer_cpu_address,
                file_offset: storage.file_offset,
                end_file_offset_exclusive: storage.end_file_offset_exclusive,
                storage_byte_count: storage.storage_byte_count,
                storage_sha1: storage.storage_sha1.clone(),
                prefix_byte_count: storage.prefix_byte_count,
                boundary_control: storage.boundary_control,
                literal_file_offsets,
                lines,
            })
        })
        .collect::<Result<Vec<_>>>()?;
    ensure!(
        records.len() == report.summary.main_record_count,
        "main dialogue storage record export lost coverage"
    );
    Ok(MainDialogueStorageInspection {
        records,
        safe_japanese_translation_source_byte_count: report
            .summary
            .main_safe_japanese_translation_source_byte_count,
    })
}

pub(crate) fn inspect_chapter_intro_contexts(
    source: &[u8],
) -> Result<Vec<ChapterIntroContextBinding>> {
    let report = build_report(source)?;
    let table = report
        .tables
        .iter()
        .find(|table| table.id == "chapter-intro-dialogue")
        .context("chapter-intro dialogue table is absent")?;

    table
        .entries
        .iter()
        .filter(|entry| {
            entry.target_kind == "script_entry_start" && is_canonical_dialogue_entry(entry)
        })
        .filter(|entry| {
            entry
                .main_record_prefix
                .as_ref()
                .is_some_and(|prefix| prefix.e5_prefix_present)
        })
        .map(|entry| {
            let prefix_end = entry
                .file_offset
                .checked_add(OPTIONAL_PREFIX_BYTE_COUNT)
                .context("chapter-intro E5 prefix range overflow")?;
            let prefix = source
                .get(entry.file_offset..prefix_end)
                .context("chapter-intro E5 prefix is outside the source")?;
            ensure!(
                prefix[0] == OPTIONAL_E5_PREFIX_CODE,
                "chapter-intro E5 prefix marker changed"
            );
            let prefix_payload: [u8; OPTIONAL_PREFIX_BYTE_COUNT - 1] = prefix[1..]
                .try_into()
                .expect("E5 payload has a fixed length");
            let mut entry_indices = vec![entry.index];
            entry_indices.extend(entry.alias_entry_indices.iter().copied());
            entry_indices.sort_unstable();

            Ok(ChapterIntroContextBinding {
                entry_indices,
                file_offset: entry.file_offset,
                chapter_index: prefix_payload[4],
                prefix_payload,
            })
        })
        .collect()
}

pub(crate) fn inspect_shop_dialogue_table(source: &[u8]) -> Result<ShopDialogueTableBinding> {
    let spec = DIALOGUE_TABLE_SPECS
        .iter()
        .find(|spec| spec.id == "shop-and-item-dialogue")
        .context("shop-and-item dialogue table is absent")?;
    let report = extract_dialogue_table(source, spec)?;
    let directory = report
        .directory_binding
        .as_ref()
        .context("shop-and-item dialogue table has no directory binding")?;
    let first_entry = report
        .entries
        .first()
        .context("shop-and-item dialogue table is empty")?;

    Ok(ShopDialogueTableBinding {
        table_id: report.id,
        source_prg_bank: report.source_prg_bank,
        source_prg_bank_hex: report.source_prg_bank_hex,
        directory_selector: directory.selector,
        directory_selector_hex: directory.selector_hex.clone(),
        directory_entry_cpu_address: directory.directory_entry_cpu_address,
        directory_entry_cpu_address_hex: directory.directory_entry_cpu_address_hex.clone(),
        pointer_table_cpu_address: report.pointer_table_cpu_address,
        pointer_table_cpu_address_hex: report.pointer_table_cpu_address_hex,
        pointer_table_sha1: report.pointer_table_sha1,
        pointer_count: report.pointer_count,
        unique_target_count: report.unique_target_count,
        first_entry_pointer_cpu_address: first_entry.pointer_cpu_address,
        first_entry_pointer_cpu_address_hex: first_entry.pointer_cpu_address_hex.clone(),
    })
}

pub(crate) fn inspect_translation_surface_dialogue_tables(
    source: &[u8],
) -> Result<Vec<TranslationSurfaceDialogueTableBinding>> {
    const TABLE_IDS: [&str; 3] = [
        "battle-dialogue",
        "epilogue-dialogue",
        "epilogue-routing-dialogue",
    ];

    TABLE_IDS
        .into_iter()
        .map(|table_id| {
            let spec = DIALOGUE_TABLE_SPECS
                .iter()
                .find(|spec| spec.id == table_id)
                .with_context(|| {
                    format!("translation-surface dialogue table {table_id} is absent")
                })?;
            let report = extract_dialogue_table(source, spec)?;
            let directory_selector = report
                .directory_binding
                .as_ref()
                .map(|directory| directory.selector);
            let directory_selector_hex = report
                .directory_binding
                .as_ref()
                .map(|directory| directory.selector_hex.clone());
            let separate_loader_cpu_address = report
                .separate_consumer_binding
                .as_ref()
                .map(|consumer| consumer.loader_cpu_address);
            let separate_loader_cpu_address_hex = report
                .separate_consumer_binding
                .as_ref()
                .map(|consumer| consumer.loader_cpu_address_hex.clone());
            let proven_record_count = report
                .main_record_storage_summary
                .as_ref()
                .map(|summary| summary.unique_record_count)
                .or_else(|| {
                    report
                        .battle_record_storage_summary
                        .as_ref()
                        .map(|summary| summary.pointer_referenced_record_count)
                });
            let unique_record_storage_byte_count = report
                .main_record_storage_summary
                .as_ref()
                .map(|summary| summary.unique_storage_byte_count)
                .or_else(|| {
                    report
                        .battle_record_storage_summary
                        .as_ref()
                        .map(|summary| summary.unique_storage_byte_count)
                });
            let unreferenced_record_count = report
                .battle_record_storage_summary
                .as_ref()
                .map(|summary| summary.unreferenced_record_count);
            let (literal_inventory, literal_file_offsets) =
                translation_surface_literal_inventory(source, &report)?;

            Ok(TranslationSurfaceDialogueTableBinding {
                table_id: report.id,
                source_prg_bank: report.source_prg_bank,
                source_prg_bank_hex: report.source_prg_bank_hex,
                pointer_table_cpu_address: report.pointer_table_cpu_address,
                pointer_table_cpu_address_hex: report.pointer_table_cpu_address_hex,
                pointer_table_sha1: report.pointer_table_sha1,
                pointer_count: report.pointer_count,
                unique_target_count: report.unique_target_count,
                consumer_binding_status: report.consumer_binding_status,
                directory_selector,
                directory_selector_hex,
                separate_loader_cpu_address,
                separate_loader_cpu_address_hex,
                proven_record_count,
                unique_record_storage_byte_count,
                unreferenced_record_count,
                literal_inventory,
                literal_file_offsets,
            })
        })
        .collect()
}

fn translation_surface_literal_inventory(
    source: &[u8],
    report: &DialogueTableReport,
) -> Result<(TranslationSurfaceLiteralInventory, BTreeSet<usize>)> {
    let literal_file_offsets = report
        .entries
        .iter()
        .filter(|entry| {
            entry.target_kind == "script_entry_start" && is_canonical_dialogue_entry(entry)
        })
        .map(|entry| {
            if report.id == BATTLE_DIALOGUE_TABLE_ID {
                entry
                    .battle_record_storage
                    .as_ref()
                    .context("canonical battle-dialogue entry has no literal boundaries")
                    .map(|record| record.literal_file_offsets.clone())
            } else {
                entry
                    .main_linear_segment
                    .as_ref()
                    .context("canonical epilogue entry has no literal boundaries")
                    .map(|segment| {
                        segment
                            .lines
                            .iter()
                            .flat_map(|line| line.literal_file_offsets.iter().copied())
                            .collect()
                    })
            }
        })
        .collect::<Result<Vec<Vec<usize>>>>()?
        .into_iter()
        .flatten()
        .collect::<BTreeSet<_>>();

    let inventory = literal_inventory_from_file_offsets(source, &literal_file_offsets, report.id)?;
    Ok((inventory, literal_file_offsets))
}

pub(crate) fn aggregate_translation_surface_dialogue_literal_inventory(
    source: &[u8],
    tables: &[TranslationSurfaceDialogueTableBinding],
    requested_table_ids: &[&str],
) -> Result<TranslationSurfaceLiteralInventory> {
    let mut seen_table_ids = BTreeSet::new();
    let mut literal_file_offsets = BTreeSet::new();
    let mut source_offset_count = 0;
    for table_id in requested_table_ids {
        ensure!(
            seen_table_ids.insert(*table_id),
            "duplicate translation-surface dialogue table id {table_id}"
        );
        let table = tables
            .iter()
            .find(|table| table.table_id == *table_id)
            .with_context(|| format!("translation-surface dialogue table {table_id} is absent"))?;
        source_offset_count += table.literal_file_offsets.len();
        literal_file_offsets.extend(table.literal_file_offsets.iter().copied());
    }
    ensure!(
        source_offset_count == literal_file_offsets.len(),
        "translation-surface dialogue tables overlap literal storage"
    );

    literal_inventory_from_file_offsets(source, &literal_file_offsets, "dialogue-table aggregate")
}

fn literal_inventory_from_file_offsets(
    source: &[u8],
    literal_file_offsets: &BTreeSet<usize>,
    inventory_role: &str,
) -> Result<TranslationSurfaceLiteralInventory> {
    let codes = literal_file_offsets
        .iter()
        .map(|file_offset| {
            source
                .get(*file_offset)
                .copied()
                .context("translation-surface literal offset is outside the source")
        })
        .collect::<Result<Vec<_>>>()?;
    classify_translation_surface_literal_codes(codes, inventory_role)
}

fn build_report(source: &[u8]) -> Result<DialogueStructureReport> {
    let main_dialogue_state_machine = build_main_dialogue_state_machine(source)?;
    let battle_dialogue_state_machine = build_battle_dialogue_state_machine(source)?;
    let tables = DIALOGUE_TABLE_SPECS
        .iter()
        .map(|spec| extract_dialogue_table(source, spec))
        .collect::<Result<Vec<_>>>()?;
    let main_dialogue_graph = build_main_dialogue_graph(&tables)?;
    let main_literal_storage_summary = summarize_main_literal_storage(source, &tables)?;
    let main_first_line_count = tables
        .iter()
        .filter_map(|table| table.main_first_line_summary.as_ref())
        .map(|summary| summary.unique_line_count)
        .sum();
    let max_main_first_line_storage_byte_count = tables
        .iter()
        .filter_map(|table| table.main_first_line_summary.as_ref())
        .map(|summary| summary.max_storage_byte_count)
        .max()
        .unwrap_or(0);
    let main_first_line_japanese_literal_byte_count = tables
        .iter()
        .filter_map(|table| table.main_first_line_summary.as_ref())
        .map(|summary| summary.japanese_literal_byte_count)
        .sum();
    let main_first_line_non_japanese_literal_byte_count = tables
        .iter()
        .filter_map(|table| table.main_first_line_summary.as_ref())
        .map(|summary| summary.non_japanese_literal_byte_count)
        .sum();
    let main_first_line_protected_original_alphanumeric_literal_byte_count = tables
        .iter()
        .filter_map(|table| table.main_first_line_summary.as_ref())
        .map(|summary| summary.protected_original_alphanumeric_literal_byte_count)
        .sum();
    let mut main_first_line_end_control_count_map = BTreeMap::new();
    for usage in tables
        .iter()
        .filter_map(|table| table.main_first_line_summary.as_ref())
        .flat_map(|summary| &summary.line_end_control_counts)
    {
        *main_first_line_end_control_count_map
            .entry(usage.code)
            .or_insert(0) += usage.count;
    }
    let main_first_line_end_control_counts =
        control_usage_reports(main_first_line_end_control_count_map, &MAIN_LINE_END_CODES);
    let main_linear_segment_count = tables
        .iter()
        .filter_map(|table| table.main_linear_segment_summary.as_ref())
        .map(|summary| summary.unique_segment_count)
        .sum();
    let main_linear_line_count = tables
        .iter()
        .filter_map(|table| table.main_linear_segment_summary.as_ref())
        .map(|summary| summary.total_line_count)
        .sum();
    let max_main_linear_segment_line_count = tables
        .iter()
        .filter_map(|table| table.main_linear_segment_summary.as_ref())
        .map(|summary| summary.max_line_count)
        .max()
        .unwrap_or(0);
    let main_linear_segment_japanese_literal_byte_count = tables
        .iter()
        .filter_map(|table| table.main_linear_segment_summary.as_ref())
        .map(|summary| summary.japanese_literal_byte_count)
        .sum();
    let main_linear_segment_non_japanese_literal_byte_count = tables
        .iter()
        .filter_map(|table| table.main_linear_segment_summary.as_ref())
        .map(|summary| summary.non_japanese_literal_byte_count)
        .sum();
    let main_linear_segment_protected_original_alphanumeric_literal_byte_count = tables
        .iter()
        .filter_map(|table| table.main_linear_segment_summary.as_ref())
        .map(|summary| summary.protected_original_alphanumeric_literal_byte_count)
        .sum();
    let mut main_linear_segment_boundary_control_count_map = BTreeMap::new();
    for usage in tables
        .iter()
        .filter_map(|table| table.main_linear_segment_summary.as_ref())
        .flat_map(|summary| &summary.boundary_control_counts)
    {
        *main_linear_segment_boundary_control_count_map
            .entry(usage.code)
            .or_insert(0) += usage.count;
    }
    let main_linear_segment_boundary_control_counts = control_usage_reports(
        main_linear_segment_boundary_control_count_map,
        &MAIN_LINEAR_SEGMENT_BOUNDARY_CODES,
    );
    let main_linear_segment_transition_count = tables
        .iter()
        .filter_map(|table| table.main_linear_segment_summary.as_ref())
        .map(|summary| summary.transition_count)
        .sum();
    let main_record_ranges = tables
        .iter()
        .filter(|table| table.directory_binding.is_some())
        .flat_map(|table| &table.entries)
        .filter(|entry| {
            entry.target_kind == "script_entry_start" && is_canonical_dialogue_entry(entry)
        })
        .map(|entry| {
            let storage = entry
                .main_record_storage
                .as_ref()
                .context("canonical main dialogue entry has no record-storage range")?;
            Ok(MainRecordStorageRange {
                start: storage.file_offset,
                end_exclusive: storage.end_file_offset_exclusive,
            })
        })
        .collect::<Result<Vec<_>>>()?;
    let main_record_storage_summary = summarize_main_record_storage(&main_record_ranges)?;
    let main_record_count = main_record_storage_summary.unique_record_count;
    let main_unique_script_entry_count: usize = tables
        .iter()
        .filter(|table| table.directory_binding.is_some())
        .map(|table| table.unique_script_entry_count)
        .sum();
    ensure!(
        main_first_line_count == main_unique_script_entry_count,
        "main first-line coverage does not match the directory-bound script entries"
    );
    ensure!(
        main_linear_segment_count == main_unique_script_entry_count,
        "main linear-segment coverage does not match the directory-bound script entries"
    );
    ensure!(
        main_record_count == main_unique_script_entry_count,
        "main record-storage coverage does not match the directory-bound script entries"
    );
    let battle_record_storage_summary = tables
        .iter()
        .find(|table| table.id == BATTLE_DIALOGUE_TABLE_ID)
        .and_then(|table| table.battle_record_storage_summary.as_ref())
        .context("battle-dialogue table has no record-storage summary")?;
    let battle_pointer_referenced_record_count =
        battle_record_storage_summary.pointer_referenced_record_count;
    let battle_unreferenced_record_count = battle_record_storage_summary.unreferenced_record_count;
    let battle_pointer_referenced_storage_byte_count =
        battle_record_storage_summary.unique_storage_byte_count;
    let battle_physical_record_storage_byte_count =
        battle_record_storage_summary.physical_record_storage_byte_count;
    let summary = ReportSummary {
        table_count: tables.len(),
        directory_bound_table_count: tables
            .iter()
            .filter(|table| table.directory_binding.is_some())
            .count(),
        separate_consumer_bound_table_count: tables
            .iter()
            .filter(|table| table.separate_consumer_binding.is_some())
            .count(),
        consumer_bound_table_count: tables
            .iter()
            .filter(|table| {
                table.directory_binding.is_some() || table.separate_consumer_binding.is_some()
            })
            .count(),
        unresolved_consumer_table_count: tables
            .iter()
            .filter(|table| {
                table.directory_binding.is_none() && table.separate_consumer_binding.is_none()
            })
            .count(),
        pointer_count: tables.iter().map(|table| table.pointer_count).sum(),
        unique_target_count: tables.iter().map(|table| table.unique_target_count).sum(),
        unique_script_entry_count: tables
            .iter()
            .map(|table| table.unique_script_entry_count)
            .sum(),
        handler_target_entry_count: tables
            .iter()
            .map(|table| table.handler_target_entry_count)
            .sum(),
        main_first_line_count,
        max_main_first_line_storage_byte_count,
        main_first_line_japanese_literal_byte_count,
        main_first_line_non_japanese_literal_byte_count,
        main_first_line_protected_original_alphanumeric_literal_byte_count,
        main_first_line_end_control_counts,
        main_linear_segment_count,
        main_linear_line_count,
        max_main_linear_segment_line_count,
        main_linear_segment_japanese_literal_byte_count,
        main_linear_segment_non_japanese_literal_byte_count,
        main_linear_segment_protected_original_alphanumeric_literal_byte_count,
        main_unique_japanese_literal_storage_byte_count: main_literal_storage_summary
            .unique_japanese_literal_storage_byte_count,
        main_unique_non_japanese_literal_storage_byte_count: main_literal_storage_summary
            .unique_non_japanese_literal_storage_byte_count,
        main_literal_kind_conflict_storage_byte_count: main_literal_storage_summary
            .literal_kind_conflict_storage_byte_count,
        main_literal_structural_conflict_storage_byte_count: main_literal_storage_summary
            .literal_structural_conflict_storage_byte_count,
        main_safe_japanese_translation_source_byte_count: main_literal_storage_summary
            .safe_japanese_translation_source_byte_count,
        main_linear_segment_boundary_control_counts,
        main_linear_segment_transition_count,
        main_record_count,
        main_record_consumed_storage_byte_count: main_record_storage_summary
            .consumed_storage_byte_count,
        main_record_unique_storage_byte_count: main_record_storage_summary
            .unique_storage_byte_count,
        main_record_shared_storage_byte_count: main_record_storage_summary
            .shared_storage_byte_count,
        main_record_overlapping_pair_count: main_record_storage_summary
            .overlapping_record_pair_count,
        max_main_record_overlap_depth: main_record_storage_summary.max_overlap_depth,
        max_main_record_storage_byte_count: main_record_storage_summary.max_storage_byte_count,
        battle_pointer_referenced_record_count,
        battle_unreferenced_record_count,
        battle_pointer_referenced_storage_byte_count,
        battle_physical_record_storage_byte_count,
        alias_group_count: tables.iter().map(|table| table.alias_group_count).sum(),
        aliased_entry_count: tables.iter().map(|table| table.aliased_entry_count).sum(),
    };

    Ok(DialogueStructureReport {
        schema_version: 11,
        scope: ReportScope {
            source_sha1: EXPECTED_SOURCE_SHA1,
            translation_direction: "ja_to_ko",
            preserve_existing_english: true,
            proof_boundary: "exact pointer-table ranges, switchable-bank target mapping, aliases, all nine consumer roots, the selector-41 epilogue-routing use, the main dialogue record-prefix state path, every main entry's bounded consumed storage range and measured shared storage, the separate battle state machine and all EF-terminated battle record ranges, Japanese 00-5F and 84-8B literal classification with 60-83 Latin preservation, all explicit E4/E6 graph edges, the E7 caller-handoff contract, and eleven confirmed direct outer dispatch bindings; no dialogue bytes or translations are emitted",
        },
        summary,
        main_dialogue_state_machine,
        battle_dialogue_state_machine,
        main_dialogue_graph,
        tables,
        unknowns: vec![
            "All directory-bound script entries and all twenty-eight pointer-referenced battle records have bounded consumed storage ranges; main records may share bytes, while battle records are disjoint and one additional unreferenced structural record remains preserved but not admitted as a translation target.",
            "Battle record boundaries are proven, but the favorable gameplay battle and remaining ending temporal glyph, portrait, and sprite variants remain open before Hangul page budgeting.",
            "The E5, fixed four-byte, and E8 record prefix, each initial linear segment, all E4/E6 graph edges, and the E7 caller handoff are confirmed, but caller-specific outcomes after the handoff remain unresolved.",
            "Eleven direct outer dispatch bindings reuse four observer handlers across twenty-two state slots; indirect bindings are not excluded, and bank 04:A20F has no confirmed direct dispatch binding.",
            "Ten of the eighteen main dialogue state handlers remain structurally named but semantically unresolved.",
            "Role labels began as external map candidates and do not prove every entry's gameplay context.",
            "Existing English and numeric content remains protected and is not a translation target.",
        ],
    })
}

fn control_usage_reports(
    counts: BTreeMap<u8, usize>,
    declared_order: &[u8],
) -> Vec<ControlUsageReport> {
    declared_order
        .iter()
        .filter_map(|code| {
            counts.get(code).map(|count| ControlUsageReport {
                code: *code,
                code_hex: format!("{code:02X}"),
                count: *count,
            })
        })
        .collect()
}
