use std::{collections::BTreeSet, fs, path::Path};

use anyhow::{Context, Result, ensure};
use serde::{Deserialize, Serialize};

use crate::{
    dialogue_inventory::{
        MainDialogueStorageLine, MainDialogueStorageRecord,
        inspect_battle_dialogue_physical_layout, inspect_battle_dialogue_translation_records,
        inspect_main_dialogue_graph, inspect_main_dialogue_storage, switchable_file_to_cpu,
    },
    japanese_encoding::{is_japanese_text_code, japanese_text_glyph},
    rom::{EXPECTED_SOURCE_SHA1, Rom},
    sha1_hex,
    text_inventory::DIALOGUE_CONTROL_SPECS,
    tracked::TrackedImage,
};

mod glyph_workset;
mod battle_workspace;
mod layout_packing;
mod markup;
mod model;
mod slice;
mod source_asset;
#[cfg(test)]
mod tests;
mod workspace;

pub(crate) use glyph_workset::analyze_main_dialogue_glyph_workset;
pub(crate) use battle_workspace::{
    extract_battle_dialogue_workspace, import_battle_dialogue_draft,
    plan_battle_dialogue_reinsertion, validate_battle_dialogue_workspace,
};
use layout_packing::*;
use markup::*;
use model::*;
pub use model::{
    DialogueLayoutPlanSummary, DialogueSourceAssetSummary, DialogueSourceRoundtripSummary,
    DialogueWorkspaceSummary, DialogueWorkspaceValidationSummary,
};
pub(crate) use slice::{MainDialogueSlicePlan, plan_main_dialogue_slice};
use source_asset::*;
use workspace::*;

pub fn extract_main_dialogue_source(
    source_path: &Path,
    asset_path: &Path,
) -> Result<DialogueSourceAssetSummary> {
    let rom = Rom::from_path(source_path)?;
    rom.verify_supported_japanese()?;
    let asset = build_source_asset(rom.data())?;
    let mut asset_bytes =
        serde_json::to_vec_pretty(&asset).context("serialize main dialogue source asset")?;
    asset_bytes.push(b'\n');
    write_file(asset_path, &asset_bytes)?;

    Ok(DialogueSourceAssetSummary {
        asset_sha1: sha1_hex(&asset_bytes),
        storage_region_count: asset.storage_regions.len(),
        record_count: asset.records.len(),
        unique_storage_byte_count: asset
            .storage_regions
            .iter()
            .map(|region| region.storage_byte_count)
            .sum(),
    })
}
pub fn extract_main_dialogue_workspace(
    source_path: &Path,
    workspace_path: &Path,
) -> Result<DialogueWorkspaceSummary> {
    let rom = Rom::from_path(source_path)?;
    rom.verify_supported_japanese()?;
    let mut workspace = build_workspace(rom.data())?;
    let preserved_translation_line_count = if workspace_path.exists() {
        let existing_bytes = fs::read(workspace_path).with_context(|| {
            format!(
                "read existing main dialogue workspace {}",
                workspace_path.display()
            )
        })?;
        let existing: MainDialogueWorkspace = serde_json::from_slice(&existing_bytes)
            .with_context(|| {
                format!(
                    "parse existing main dialogue workspace {}",
                    workspace_path.display()
                )
            })?;
        preserve_workspace_translations(&mut workspace, &existing)?
    } else {
        0
    };
    let line_count = workspace
        .records
        .iter()
        .map(|record| record.lines.len())
        .sum();
    let blocked_line_count = workspace
        .records
        .iter()
        .flat_map(|record| &record.lines)
        .filter(|line| line.requires_relocation)
        .count();
    let mut workspace_bytes =
        serde_json::to_vec_pretty(&workspace).context("serialize main dialogue workspace")?;
    workspace_bytes.push(b'\n');
    write_file_atomically(workspace_path, &workspace_bytes)?;

    Ok(DialogueWorkspaceSummary {
        workspace_sha1: sha1_hex(&workspace_bytes),
        record_count: workspace.records.len(),
        line_count,
        safe_japanese_source_byte_count: workspace.safe_japanese_source_byte_count,
        blocked_line_count,
        preserved_translation_line_count,
    })
}

pub fn validate_main_dialogue_workspace(
    source_path: &Path,
    workspace_path: &Path,
) -> Result<DialogueWorkspaceValidationSummary> {
    let rom = Rom::from_path(source_path)?;
    rom.verify_supported_japanese()?;
    let workspace_bytes = fs::read(workspace_path)
        .with_context(|| format!("read main dialogue workspace {}", workspace_path.display()))?;
    let workspace: MainDialogueWorkspace = serde_json::from_slice(&workspace_bytes)
        .with_context(|| format!("parse main dialogue workspace {}", workspace_path.display()))?;
    let expected = build_workspace(rom.data())?;
    validate_workspace_binding(&workspace, &expected)?;
    let counts = validate_workspace_translations(&workspace)?;

    Ok(DialogueWorkspaceValidationSummary {
        workspace_sha1: sha1_hex(&workspace_bytes),
        record_count: workspace.records.len(),
        line_count: workspace
            .records
            .iter()
            .map(|record| record.lines.len())
            .sum(),
        filled_line_count: counts.filled_line_count,
        complete_line_count: counts.complete_line_count,
        target_glyph_count: counts.target_glyph_count,
    })
}

pub fn plan_main_dialogue_reinsertion(
    source_path: &Path,
    workspace_path: &Path,
    report_path: &Path,
) -> Result<DialogueLayoutPlanSummary> {
    let rom = Rom::from_path(source_path)?;
    rom.verify_supported_japanese()?;
    let workspace_bytes = fs::read(workspace_path)
        .with_context(|| format!("read main dialogue workspace {}", workspace_path.display()))?;
    let workspace: MainDialogueWorkspace = serde_json::from_slice(&workspace_bytes)
        .with_context(|| format!("parse main dialogue workspace {}", workspace_path.display()))?;
    let expected_workspace = build_workspace(rom.data())?;
    validate_workspace_binding(&workspace, &expected_workspace)?;
    let translation_counts = validate_workspace_translations(&workspace)?;

    let source_records = inspect_main_dialogue_storage(rom.data())?.records;
    ensure!(
        source_records.len() == workspace.records.len(),
        "main dialogue layout lost workspace records"
    );
    let owned_ranges = normalize_storage_ranges(&source_records)?;
    let logical_records = source_records
        .iter()
        .zip(&workspace.records)
        .map(|(source_record, workspace_record)| {
            build_logical_dialogue_record(rom.data(), source_record, workspace_record)
        })
        .collect::<Result<Vec<_>>>()?;

    let mut region_reports = Vec::new();
    let mut record_reports = Vec::new();
    for (region_index, region) in owned_ranges.iter().copied().enumerate() {
        let mut region_records = logical_records
            .iter()
            .filter(|record| {
                record.source_prg_bank == region.source_prg_bank
                    && region.start <= record.source_file_offset
                    && record.source_file_offset + record.source_storage_byte_count
                        <= region.end_exclusive
            })
            .collect::<Vec<_>>();
        region_records.sort_unstable_by_key(|record| record.source_file_offset);
        ensure!(
            !region_records.is_empty(),
            "main dialogue owned region {region_index} has no records"
        );
        let changed = region_records
            .iter()
            .any(|record| record.translated_line_count != 0);
        let (planned_storage, placements) = if changed {
            pack_logical_records(&region_records)
        } else {
            let source_storage = rom
                .data()
                .get(region.start..region.end_exclusive)
                .context("main dialogue owned region is outside the source")?
                .iter()
                .copied()
                .map(LogicalDialogueByte::Encoded)
                .collect::<Vec<_>>();
            let placements = region_records
                .iter()
                .map(|record| record.source_file_offset - region.start)
                .collect();
            (source_storage, placements)
        };
        let capacity = region.end_exclusive - region.start;
        ensure!(
            planned_storage.len() <= capacity,
            "main dialogue region {region_index} in PRG bank {:02X} needs {} bytes but owns only {capacity}",
            region.source_prg_bank,
            planned_storage.len()
        );
        let source_equivalent_layout = planned_storage
            == rom.data()[region.start..region.end_exclusive]
                .iter()
                .copied()
                .map(LogicalDialogueByte::Encoded)
                .collect::<Vec<_>>();
        region_reports.push(LayoutRegionReport {
            index: region_index,
            source_prg_bank: region.source_prg_bank,
            source_prg_bank_hex: format!("0x{:02X}", region.source_prg_bank),
            file_offset: region.start,
            file_offset_hex: format!("0x{:05X}", region.start),
            end_file_offset_exclusive: region.end_exclusive,
            end_file_offset_exclusive_hex: format!("0x{:05X}", region.end_exclusive),
            capacity_byte_count: capacity,
            planned_storage_byte_count: planned_storage.len(),
            remaining_storage_byte_count: capacity - planned_storage.len(),
            record_count: region_records.len(),
            source_equivalent_layout,
        });

        for (record, region_relative_offset) in region_records.iter().zip(placements) {
            let planned_file_offset = region
                .start
                .checked_add(region_relative_offset)
                .context("main dialogue planned record offset overflow")?;
            let planned_pointer_cpu_address =
                switchable_file_to_cpu(region.source_prg_bank, planned_file_offset)?;
            record_reports.push(LayoutRecordReport {
                id: record.id.clone(),
                source_prg_bank: record.source_prg_bank,
                source_prg_bank_hex: format!("0x{:02X}", record.source_prg_bank),
                source_pointer_cpu_address: record.source_pointer_cpu_address,
                source_pointer_cpu_address_hex: format!(
                    "0x{:04X}",
                    record.source_pointer_cpu_address
                ),
                planned_pointer_cpu_address,
                planned_pointer_cpu_address_hex: format!("0x{planned_pointer_cpu_address:04X}"),
                pointer_file_offsets: record.pointer_file_offsets.clone(),
                pointer_file_offsets_hex: record
                    .pointer_file_offsets
                    .iter()
                    .map(|offset| format!("0x{offset:05X}"))
                    .collect(),
                source_storage_byte_count: record.source_storage_byte_count,
                planned_storage_byte_count: record.bytes.len(),
                translated_line_count: record.translated_line_count,
                changed: record.translated_line_count != 0,
                storage_region_index: region_index,
                region_relative_offset,
            });
        }
    }
    record_reports.sort_unstable_by(|left, right| left.id.cmp(&right.id));
    ensure!(
        record_reports.len() == 504,
        "main dialogue layout plan must contain exactly 504 records"
    );
    let pointer_write_count = record_reports
        .iter()
        .map(|record| record.pointer_file_offsets.len())
        .sum::<usize>();
    ensure!(
        pointer_write_count == 517,
        "main dialogue layout plan pointer coverage changed"
    );
    let source_owned_storage_byte_count = region_reports
        .iter()
        .map(|region| region.capacity_byte_count)
        .sum::<usize>();
    let planned_storage_byte_count = region_reports
        .iter()
        .map(|region| region.planned_storage_byte_count)
        .sum::<usize>();
    let remaining_storage_byte_count = region_reports
        .iter()
        .map(|region| region.remaining_storage_byte_count)
        .sum::<usize>();
    let changed_record_count = record_reports
        .iter()
        .filter(|record| record.changed)
        .count();
    let line_count = workspace
        .records
        .iter()
        .map(|record| record.lines.len())
        .sum::<usize>();
    let translation_input_complete = translation_counts.complete_line_count == line_count;
    let release_eligible = false;
    let report = MainDialogueLayoutReport {
        schema_version: 1,
        scope: LayoutReportScope {
            source_sha1: EXPECTED_SOURCE_SHA1,
            workspace_sha1: sha1_hex(&workspace_bytes),
            translation_direction: "ja-to-ko-only",
            preserve_existing_english: true,
            layout_mode: "logical-one-byte-target-glyphs-within-proven-source-owned-regions",
            output_boundary: "layout-and-pointer-write-plan-only; no encoded bytes or ROM output",
        },
        summary: LayoutReportSummary {
            storage_region_count: region_reports.len(),
            record_count: record_reports.len(),
            pointer_write_count,
            source_owned_storage_byte_count,
            planned_storage_byte_count,
            remaining_storage_byte_count,
            changed_record_count,
            filled_line_count: translation_counts.filled_line_count,
            complete_line_count: translation_counts.complete_line_count,
            translation_input_complete,
            release_eligible,
        },
        regions: region_reports,
        records: record_reports,
        unknowns: vec![
            "Target glyphs have logical one-byte width but no Hangul code assignment until the dynamic font contract is implemented.",
            "A successful plan does not prove screen line width, glyph working-set capacity, runtime display, or mapper conversion equivalence.",
        ],
    };
    let mut report_bytes =
        serde_json::to_vec_pretty(&report).context("serialize main dialogue layout plan")?;
    report_bytes.push(b'\n');
    write_file(report_path, &report_bytes)?;

    Ok(DialogueLayoutPlanSummary {
        report_sha1: sha1_hex(&report_bytes),
        region_count: report.summary.storage_region_count,
        record_count: report.summary.record_count,
        pointer_write_count,
        planned_storage_byte_count,
        remaining_storage_byte_count,
        changed_record_count,
        translation_input_complete,
        release_eligible,
    })
}

pub fn verify_main_dialogue_source_roundtrip(
    source_path: &Path,
    asset_path: &Path,
    output_path: &Path,
) -> Result<DialogueSourceRoundtripSummary> {
    let rom = Rom::from_path(source_path)?;
    rom.verify_supported_japanese()?;
    let asset_bytes = fs::read(asset_path)
        .with_context(|| format!("read main dialogue source asset {}", asset_path.display()))?;
    let asset: MainDialogueSourceAsset = serde_json::from_slice(&asset_bytes)
        .with_context(|| format!("parse main dialogue source asset {}", asset_path.display()))?;
    let expected_asset = build_source_asset(rom.data())?;
    ensure!(
        asset == expected_asset,
        "main dialogue source asset does not exactly match the supported Japanese source extraction"
    );

    let source = rom.data().to_vec();
    let mut image = TrackedImage::new(source.clone());
    for region in &asset.storage_regions {
        let replacement = decode_hex(&region.storage_hex)
            .with_context(|| format!("decode source storage region {}", region.index))?;
        ensure!(
            replacement.len() == region.storage_byte_count,
            "source storage region {} length changed",
            region.index
        );
        let expected = source
            .get(region.file_offset..region.end_file_offset_exclusive)
            .with_context(|| {
                format!("source storage region {} is outside the ROM", region.index)
            })?;
        ensure!(
            sha1_hex(expected) == region.storage_sha1,
            "source storage region {} hash changed",
            region.index
        );
        image.write_expected(
            format!("main dialogue source region {}", region.index),
            region.file_offset,
            expected,
            &replacement,
        )?;
    }
    image.verify_all_changes_tracked(&source)?;
    let output = image.into_data();
    ensure!(
        output == source,
        "main dialogue source roundtrip did not reproduce the supported ROM exactly"
    );
    Rom::parse(output.clone())?.verify_supported_japanese()?;
    write_file(output_path, &output)?;

    Ok(DialogueSourceRoundtripSummary {
        output_sha1: sha1_hex(&output),
        storage_region_count: asset.storage_regions.len(),
        record_count: asset.records.len(),
    })
}
