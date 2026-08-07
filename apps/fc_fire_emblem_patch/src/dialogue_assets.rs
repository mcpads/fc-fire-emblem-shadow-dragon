use std::{collections::BTreeSet, fs, path::Path};

use anyhow::{Context, Result, ensure};
use serde::{Deserialize, Serialize};

use crate::{
    dialogue_inventory::{
        MainDialogueStorageLine, MainDialogueStorageRecord, inspect_main_dialogue_storage,
        switchable_file_to_cpu,
    },
    japanese_encoding::{is_japanese_text_code, japanese_text_glyph},
    rom::{EXPECTED_SOURCE_SHA1, Rom},
    sha1_hex,
    text_inventory::DIALOGUE_CONTROL_SPECS,
    tracked::TrackedImage,
};

const SOURCE_ASSET_FORMAT_VERSION: u8 = 2;
const WORKSPACE_FORMAT_VERSION: u8 = 2;

#[derive(Debug)]
pub struct DialogueSourceAssetSummary {
    pub asset_sha1: String,
    pub storage_region_count: usize,
    pub record_count: usize,
    pub unique_storage_byte_count: usize,
}

#[derive(Debug)]
pub struct DialogueSourceRoundtripSummary {
    pub output_sha1: String,
    pub storage_region_count: usize,
    pub record_count: usize,
}

#[derive(Debug)]
pub struct DialogueWorkspaceSummary {
    pub workspace_sha1: String,
    pub record_count: usize,
    pub line_count: usize,
    pub safe_japanese_source_byte_count: usize,
    pub blocked_line_count: usize,
}

#[derive(Debug)]
pub struct DialogueWorkspaceValidationSummary {
    pub workspace_sha1: String,
    pub record_count: usize,
    pub line_count: usize,
    pub filled_line_count: usize,
    pub complete_line_count: usize,
    pub target_glyph_count: usize,
}

#[derive(Debug)]
pub struct DialogueLayoutPlanSummary {
    pub report_sha1: String,
    pub region_count: usize,
    pub record_count: usize,
    pub pointer_write_count: usize,
    pub planned_storage_byte_count: usize,
    pub remaining_storage_byte_count: usize,
    pub changed_record_count: usize,
    pub translation_input_complete: bool,
    pub release_eligible: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
struct MainDialogueWorkspace {
    format_version: u8,
    source_sha1: String,
    translate_from: String,
    translate_to: String,
    preserve_existing_english: bool,
    purpose: String,
    safe_japanese_source_byte_count: usize,
    records: Vec<WorkspaceRecord>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
struct WorkspaceRecord {
    id: String,
    table_id: String,
    source_prg_bank: u8,
    canonical_entry_index: usize,
    entry_indices: Vec<usize>,
    pointer_cpu_address_hex: String,
    prefix_byte_count: usize,
    boundary_control_hex: String,
    lines: Vec<WorkspaceLine>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
struct WorkspaceLine {
    id: String,
    index: usize,
    file_offset_hex: String,
    source_storage_sha1: String,
    source_markup: String,
    korean: String,
    status: TranslationStatus,
    japanese_source_byte_count: usize,
    safe_japanese_source_byte_count: usize,
    requires_relocation: bool,
    conflicting_file_offsets_hex: Vec<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
enum TranslationStatus {
    Untranslated,
    InProgress,
    NeedsReview,
    NeedsHumanReview,
    Complete,
}

#[derive(Debug, Serialize)]
struct MainDialogueLayoutReport {
    schema_version: u8,
    scope: LayoutReportScope,
    summary: LayoutReportSummary,
    regions: Vec<LayoutRegionReport>,
    records: Vec<LayoutRecordReport>,
    unknowns: Vec<&'static str>,
}

#[derive(Debug, Serialize)]
struct LayoutReportScope {
    source_sha1: &'static str,
    workspace_sha1: String,
    translation_direction: &'static str,
    preserve_existing_english: bool,
    layout_mode: &'static str,
    output_boundary: &'static str,
}

#[derive(Debug, Serialize)]
struct LayoutReportSummary {
    storage_region_count: usize,
    record_count: usize,
    pointer_write_count: usize,
    source_owned_storage_byte_count: usize,
    planned_storage_byte_count: usize,
    remaining_storage_byte_count: usize,
    changed_record_count: usize,
    filled_line_count: usize,
    complete_line_count: usize,
    translation_input_complete: bool,
    release_eligible: bool,
}

#[derive(Debug, Serialize)]
struct LayoutRegionReport {
    index: usize,
    source_prg_bank: u8,
    source_prg_bank_hex: String,
    file_offset: usize,
    file_offset_hex: String,
    end_file_offset_exclusive: usize,
    end_file_offset_exclusive_hex: String,
    capacity_byte_count: usize,
    planned_storage_byte_count: usize,
    remaining_storage_byte_count: usize,
    record_count: usize,
    source_equivalent_layout: bool,
}

#[derive(Debug, Serialize)]
struct LayoutRecordReport {
    id: String,
    source_prg_bank: u8,
    source_prg_bank_hex: String,
    source_pointer_cpu_address: u16,
    source_pointer_cpu_address_hex: String,
    planned_pointer_cpu_address: u16,
    planned_pointer_cpu_address_hex: String,
    pointer_file_offsets: Vec<usize>,
    pointer_file_offsets_hex: Vec<String>,
    source_storage_byte_count: usize,
    planned_storage_byte_count: usize,
    translated_line_count: usize,
    changed: bool,
    storage_region_index: usize,
    region_relative_offset: usize,
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum LogicalDialogueByte {
    Encoded(u8),
    TargetGlyph(char),
}

#[derive(Debug)]
struct LogicalDialogueRecord {
    id: String,
    source_prg_bank: u8,
    source_pointer_cpu_address: u16,
    pointer_file_offsets: Vec<usize>,
    source_file_offset: usize,
    source_storage_byte_count: usize,
    translated_line_count: usize,
    bytes: Vec<LogicalDialogueByte>,
}

#[derive(Debug)]
struct WorkspaceTranslationCounts {
    filled_line_count: usize,
    complete_line_count: usize,
    target_glyph_count: usize,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
struct MainDialogueSourceAsset {
    format_version: u8,
    source_sha1: String,
    translate_from: String,
    translate_to: String,
    preserve_existing_english: bool,
    purpose: String,
    storage_regions: Vec<SourceStorageRegion>,
    records: Vec<SourceRecordReference>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
struct SourceStorageRegion {
    index: usize,
    source_prg_bank: u8,
    file_offset: usize,
    file_offset_hex: String,
    end_file_offset_exclusive: usize,
    end_file_offset_exclusive_hex: String,
    storage_byte_count: usize,
    storage_sha1: String,
    storage_hex: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
struct SourceRecordReference {
    table_id: String,
    source_prg_bank: u8,
    canonical_entry_index: usize,
    entry_indices: Vec<usize>,
    pointer_file_offsets: Vec<usize>,
    pointer_file_offsets_hex: Vec<String>,
    pointer_cpu_address: u16,
    pointer_cpu_address_hex: String,
    storage_region_index: usize,
    region_relative_offset: usize,
    storage_byte_count: usize,
    storage_sha1: String,
    prefix_byte_count: usize,
    boundary_control: u8,
    boundary_control_hex: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct OwnedStorageRange {
    source_prg_bank: u8,
    start: usize,
    end_exclusive: usize,
}

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
    let workspace = build_workspace(rom.data())?;
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
    write_file(workspace_path, &workspace_bytes)?;

    Ok(DialogueWorkspaceSummary {
        workspace_sha1: sha1_hex(&workspace_bytes),
        record_count: workspace.records.len(),
        line_count,
        safe_japanese_source_byte_count: workspace.safe_japanese_source_byte_count,
        blocked_line_count,
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

fn validate_workspace_translations(
    workspace: &MainDialogueWorkspace,
) -> Result<WorkspaceTranslationCounts> {
    let mut counts = WorkspaceTranslationCounts {
        filled_line_count: 0,
        complete_line_count: 0,
        target_glyph_count: 0,
    };
    for record in &workspace.records {
        for line in &record.lines {
            match line.status {
                TranslationStatus::Untranslated => ensure!(
                    line.korean.is_empty(),
                    "{} is untranslated but its korean field is not empty",
                    line.id
                ),
                _ => {
                    ensure!(
                        !line.korean.is_empty(),
                        "{} has status other than untranslated but its korean field is empty",
                        line.id
                    );
                    counts.filled_line_count += 1;
                    if line.status == TranslationStatus::Complete {
                        counts.complete_line_count += 1;
                    }
                    counts.target_glyph_count += validate_translation_markup(line)?;
                }
            }
        }
    }
    Ok(counts)
}

fn build_workspace(source: &[u8]) -> Result<MainDialogueWorkspace> {
    let inspection = inspect_main_dialogue_storage(source)?;
    let records = inspection.records;
    let safe_japanese_offsets = safe_japanese_literal_offsets(source, &records)?;
    let workspace_records = records
        .iter()
        .map(|record| build_workspace_record(source, record, &safe_japanese_offsets))
        .collect::<Result<Vec<_>>>()?;
    let line_count = workspace_records
        .iter()
        .map(|record| record.lines.len())
        .sum::<usize>();
    let workspace = MainDialogueWorkspace {
        format_version: WORKSPACE_FORMAT_VERSION,
        source_sha1: EXPECTED_SOURCE_SHA1.to_owned(),
        translate_from: "ja".to_owned(),
        translate_to: "ko".to_owned(),
        preserve_existing_english: true,
        purpose: "private_translation_workspace".to_owned(),
        safe_japanese_source_byte_count: safe_japanese_offsets.len(),
        records: workspace_records,
    };
    ensure!(
        workspace.records.len() == 504,
        "main dialogue workspace must contain exactly 504 canonical records"
    );
    ensure!(
        line_count == 2_732,
        "main dialogue workspace must contain exactly 2732 source lines"
    );
    ensure!(
        safe_japanese_offsets.len() == inspection.safe_japanese_translation_source_byte_count,
        "main dialogue workspace Japanese source boundary disagrees with the dialogue inventory"
    );
    Ok(workspace)
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

fn safe_japanese_literal_offsets(
    source: &[u8],
    records: &[MainDialogueStorageRecord],
) -> Result<BTreeSet<usize>> {
    let mut japanese_literal_offsets = BTreeSet::new();
    let mut structural_offsets = BTreeSet::new();
    for record in records {
        let record_literal_offsets = record
            .literal_file_offsets
            .iter()
            .copied()
            .collect::<BTreeSet<_>>();
        ensure!(
            record_literal_offsets.iter().all(|offset| {
                (record.file_offset..record.end_file_offset_exclusive).contains(offset)
            }),
            "{} entry {} has a literal outside its storage range",
            record.table_id,
            record.canonical_entry_index
        );
        for offset in record.file_offset..record.end_file_offset_exclusive {
            if record_literal_offsets.contains(&offset) {
                let code = *source
                    .get(offset)
                    .context("main dialogue workspace literal is outside the source")?;
                if is_japanese_text_code(code) {
                    japanese_literal_offsets.insert(offset);
                }
            } else {
                structural_offsets.insert(offset);
            }
        }
    }
    Ok(japanese_literal_offsets
        .difference(&structural_offsets)
        .copied()
        .collect())
}

fn build_workspace_record(
    source: &[u8],
    record: &MainDialogueStorageRecord,
    safe_japanese_offsets: &BTreeSet<usize>,
) -> Result<WorkspaceRecord> {
    let record_id = format!("{}:{:03}", record.table_id, record.canonical_entry_index);
    let lines = record
        .lines
        .iter()
        .enumerate()
        .map(|(index, line)| {
            let japanese_offsets = line
                .literal_file_offsets
                .iter()
                .copied()
                .filter(|offset| {
                    source
                        .get(*offset)
                        .copied()
                        .is_some_and(is_japanese_text_code)
                })
                .collect::<Vec<_>>();
            let conflicting_offsets = japanese_offsets
                .iter()
                .copied()
                .filter(|offset| !safe_japanese_offsets.contains(offset))
                .collect::<Vec<_>>();
            Ok(WorkspaceLine {
                id: format!("{record_id}:line:{index:02}"),
                index,
                file_offset_hex: format!("0x{:05X}", line.file_offset),
                source_storage_sha1: line.storage_sha1.clone(),
                source_markup: decode_line_markup(source, line)?,
                korean: String::new(),
                status: TranslationStatus::Untranslated,
                japanese_source_byte_count: japanese_offsets.len(),
                safe_japanese_source_byte_count: japanese_offsets.len() - conflicting_offsets.len(),
                requires_relocation: !conflicting_offsets.is_empty(),
                conflicting_file_offsets_hex: conflicting_offsets
                    .iter()
                    .map(|offset| format!("0x{offset:05X}"))
                    .collect(),
            })
        })
        .collect::<Result<Vec<_>>>()?;
    Ok(WorkspaceRecord {
        id: record_id,
        table_id: record.table_id.to_owned(),
        source_prg_bank: record.source_prg_bank,
        canonical_entry_index: record.canonical_entry_index,
        entry_indices: record.entry_indices.clone(),
        pointer_cpu_address_hex: format!("0x{:04X}", record.pointer_cpu_address),
        prefix_byte_count: record.prefix_byte_count,
        boundary_control_hex: format!("{:02X}", record.boundary_control),
        lines,
    })
}

fn validate_workspace_binding(
    workspace: &MainDialogueWorkspace,
    expected: &MainDialogueWorkspace,
) -> Result<()> {
    let mut actual_header = workspace.clone();
    actual_header.records.clear();
    let mut expected_header = expected.clone();
    expected_header.records.clear();
    ensure!(
        actual_header == expected_header,
        "main dialogue workspace header does not match the supported Japanese source"
    );
    ensure!(
        workspace.records.len() == expected.records.len(),
        "main dialogue workspace record count changed"
    );

    for (actual_record, expected_record) in workspace.records.iter().zip(&expected.records) {
        let mut actual_record_binding = actual_record.clone();
        actual_record_binding.lines.clear();
        let mut expected_record_binding = expected_record.clone();
        expected_record_binding.lines.clear();
        ensure!(
            actual_record_binding == expected_record_binding,
            "main dialogue workspace record binding changed at {}",
            expected_record.id
        );
        ensure!(
            actual_record.lines.len() == expected_record.lines.len(),
            "main dialogue workspace line count changed at {}",
            expected_record.id
        );
        for (actual_line, expected_line) in actual_record.lines.iter().zip(&expected_record.lines) {
            let mut actual_line_binding = actual_line.clone();
            actual_line_binding.korean.clear();
            actual_line_binding.status = TranslationStatus::Untranslated;
            ensure!(
                actual_line_binding == *expected_line,
                "main dialogue workspace protected source fields changed at {}",
                expected_line.id
            );
        }
    }
    Ok(())
}

fn validate_translation_markup(line: &WorkspaceLine) -> Result<usize> {
    let source = inspect_markup(&line.source_markup, MarkupRole::Source)
        .with_context(|| format!("inspect protected source markup at {}", line.id))?;
    let target = inspect_markup(&line.korean, MarkupRole::KoreanTarget)
        .with_context(|| format!("inspect korean markup at {}", line.id))?;
    ensure!(
        target.protected_items == source.protected_items,
        "{} changed, removed, or added a protected control token or existing English character",
        line.id
    );
    let final_control = source
        .protected_items
        .last()
        .filter(|item| item.starts_with('{'))
        .context("source line does not end in a protected control token")?;
    ensure!(
        line.korean.ends_with(final_control),
        "{} must keep its line-end control token at the end",
        line.id
    );
    Ok(target.editable_glyph_count)
}

#[derive(Debug, Clone, Copy)]
enum MarkupRole {
    Source,
    KoreanTarget,
}

#[derive(Debug, PartialEq, Eq)]
struct MarkupInspection {
    protected_items: Vec<String>,
    editable_glyph_count: usize,
}

fn inspect_markup(markup: &str, role: MarkupRole) -> Result<MarkupInspection> {
    let mut protected_items = Vec::new();
    let mut editable_glyph_count = 0;
    let mut chars = markup.char_indices().peekable();
    while let Some((start, character)) = chars.next() {
        if character == '{' {
            let end = chars
                .by_ref()
                .find_map(|(index, candidate)| (candidate == '}').then_some(index))
                .context("markup token has no closing brace")?;
            let token = &markup[start..=end];
            ensure!(
                !token[1..token.len() - 1].contains(['{', '}']),
                "markup token contains a nested brace"
            );
            protected_items.push(token.to_owned());
            continue;
        }
        ensure!(
            character != '}',
            "markup contains a closing brace without an opening brace"
        );

        if character.is_ascii_uppercase()
            || character.is_ascii_digit()
            || matches!(character, ':' | '.')
        {
            protected_items.push(character.to_string());
            continue;
        }

        match role {
            MarkupRole::Source => ensure!(
                is_japanese_markup_character(character),
                "source markup contains an unclassified character {character:?}"
            ),
            MarkupRole::KoreanTarget => {
                ensure!(
                    !is_japanese_markup_character(character),
                    "korean markup still contains Japanese character {character:?}"
                );
                ensure!(
                    is_korean_target_character(character),
                    "korean markup contains unsupported character {character:?}"
                );
                editable_glyph_count += 1;
            }
        }
    }
    Ok(MarkupInspection {
        protected_items,
        editable_glyph_count,
    })
}

fn is_japanese_markup_character(character: char) -> bool {
    (0..=u8::MAX)
        .any(|code| japanese_text_glyph(code).is_some_and(|glyph| glyph.starts_with(character)))
}

fn is_korean_target_character(character: char) -> bool {
    matches!(character, '\u{AC00}'..='\u{D7A3}')
        || matches!(
            character,
            ',' | '!' | '?' | '…' | '·' | '~' | '-' | '\'' | '“' | '”' | '‘' | '’' | '(' | ')'
        )
}

fn build_logical_dialogue_record(
    source: &[u8],
    source_record: &MainDialogueStorageRecord,
    workspace_record: &WorkspaceRecord,
) -> Result<LogicalDialogueRecord> {
    ensure!(
        workspace_record.id
            == format!(
                "{}:{:03}",
                source_record.table_id, source_record.canonical_entry_index
            ),
        "main dialogue logical record binding changed"
    );
    ensure!(
        source_record.lines.len() == workspace_record.lines.len(),
        "{} logical line coverage changed",
        workspace_record.id
    );
    ensure!(
        source_record.pointer_file_offsets.len() == source_record.entry_indices.len(),
        "{} pointer write coverage changed",
        workspace_record.id
    );
    for pointer_file_offset in &source_record.pointer_file_offsets {
        ensure!(
            source.get(*pointer_file_offset..*pointer_file_offset + 2)
                == Some(&source_record.pointer_cpu_address.to_le_bytes()),
            "{} pointer table source bytes changed at 0x{pointer_file_offset:05X}",
            workspace_record.id
        );
    }

    let prefix_end = source_record
        .file_offset
        .checked_add(source_record.prefix_byte_count)
        .context("main dialogue record prefix range overflow")?;
    let mut bytes = source
        .get(source_record.file_offset..prefix_end)
        .with_context(|| format!("{} prefix is outside the source", workspace_record.id))?
        .iter()
        .copied()
        .map(LogicalDialogueByte::Encoded)
        .collect::<Vec<_>>();
    let mut source_cursor = prefix_end;
    let mut translated_line_count = 0;
    for (source_line, workspace_line) in source_record.lines.iter().zip(&workspace_record.lines) {
        ensure!(
            source_line.file_offset == source_cursor,
            "{} source lines are not contiguous",
            workspace_record.id
        );
        let source_line_end = source_line
            .file_offset
            .checked_add(source_line.storage_byte_count)
            .context("main dialogue source line range overflow")?;
        if workspace_line.status == TranslationStatus::Untranslated {
            bytes.extend(
                source
                    .get(source_line.file_offset..source_line_end)
                    .with_context(|| {
                        format!("{} source storage is outside the ROM", workspace_line.id)
                    })?
                    .iter()
                    .copied()
                    .map(LogicalDialogueByte::Encoded),
            );
        } else {
            translated_line_count += 1;
            bytes.extend(
                encode_korean_markup(&workspace_line.korean)
                    .with_context(|| format!("encode logical markup at {}", workspace_line.id))?,
            );
        }
        source_cursor = source_line_end;
    }
    ensure!(
        source_cursor == source_record.end_file_offset_exclusive,
        "{} logical record did not consume its exact source range",
        workspace_record.id
    );

    Ok(LogicalDialogueRecord {
        id: workspace_record.id.clone(),
        source_prg_bank: source_record.source_prg_bank,
        source_pointer_cpu_address: source_record.pointer_cpu_address,
        pointer_file_offsets: source_record.pointer_file_offsets.clone(),
        source_file_offset: source_record.file_offset,
        source_storage_byte_count: source_record.storage_byte_count,
        translated_line_count,
        bytes,
    })
}

fn encode_korean_markup(markup: &str) -> Result<Vec<LogicalDialogueByte>> {
    let mut encoded = Vec::new();
    let mut chars = markup.char_indices().peekable();
    while let Some((start, character)) = chars.next() {
        if character == '{' {
            let end = chars
                .by_ref()
                .find_map(|(index, candidate)| (candidate == '}').then_some(index))
                .context("markup token has no closing brace")?;
            encoded.extend(
                decode_protected_token(&markup[start..=end])?
                    .into_iter()
                    .map(LogicalDialogueByte::Encoded),
            );
            continue;
        }
        ensure!(character != '}', "markup has an unmatched closing brace");
        if let Some(code) = encode_protected_literal(character) {
            encoded.push(LogicalDialogueByte::Encoded(code));
        } else {
            ensure!(
                is_korean_target_character(character),
                "unsupported korean target character {character:?}"
            );
            encoded.push(LogicalDialogueByte::TargetGlyph(character));
        }
    }
    Ok(encoded)
}

fn encode_protected_literal(character: char) -> Option<u8> {
    match character {
        '0'..='9' => Some(0x60 + (character as u8 - b'0')),
        'A'..='Z' => Some(0x6A + (character as u8 - b'A')),
        ':' => Some(0x8D),
        '.' => Some(0x9B),
        _ => None,
    }
}

fn decode_protected_token(token: &str) -> Result<Vec<u8>> {
    ensure!(
        token.starts_with('{') && token.ends_with('}'),
        "protected token is missing braces"
    );
    let body = &token[1..token.len() - 1];
    if body == "SP" {
        return Ok(vec![0xFF]);
    }
    if let Some(literal) = body.strip_prefix("LIT:") {
        return Ok(vec![decode_hex_byte(literal)?]);
    }
    let bytes = body
        .split(':')
        .map(decode_hex_byte)
        .collect::<Result<Vec<_>>>()?;
    let (control_code, operands) = bytes
        .split_first()
        .context("protected control token is empty")?;
    let control = DIALOGUE_CONTROL_SPECS
        .iter()
        .find(|control| control.code == *control_code)
        .with_context(|| format!("unknown dialogue control {control_code:02X}"))?;
    let expected_operand_count =
        control.inline_operand_byte_count + control.transition_target_byte_count;
    ensure!(
        operands.len() == expected_operand_count,
        "dialogue control {control_code:02X} requires {expected_operand_count} stored operands, found {}",
        operands.len()
    );
    Ok(bytes)
}

fn decode_hex_byte(encoded: &str) -> Result<u8> {
    ensure!(
        encoded.len() == 2 && encoded.is_ascii(),
        "hex byte must contain exactly two ASCII digits"
    );
    let digits = encoded.as_bytes();
    Ok((decode_hex_digit(digits[0])? << 4) | decode_hex_digit(digits[1])?)
}

fn pack_logical_records(
    records: &[&LogicalDialogueRecord],
) -> (Vec<LogicalDialogueByte>, Vec<usize>) {
    let mut storage = Vec::new();
    let mut placements = Vec::with_capacity(records.len());
    for record in records {
        if let Some(existing_offset) = find_subsequence(&storage, &record.bytes) {
            placements.push(existing_offset);
            continue;
        }
        let overlap = (1..=storage.len().min(record.bytes.len()))
            .rev()
            .find(|overlap| storage[storage.len() - overlap..] == record.bytes[..*overlap])
            .unwrap_or(0);
        placements.push(storage.len() - overlap);
        storage.extend_from_slice(&record.bytes[overlap..]);
    }
    (storage, placements)
}

fn find_subsequence(
    storage: &[LogicalDialogueByte],
    record: &[LogicalDialogueByte],
) -> Option<usize> {
    (!record.is_empty())
        .then(|| {
            storage
                .windows(record.len())
                .position(|window| window == record)
        })
        .flatten()
}

fn decode_line_markup(source: &[u8], line: &MainDialogueStorageLine) -> Result<String> {
    let end = line
        .file_offset
        .checked_add(line.storage_byte_count)
        .context("main dialogue workspace line range overflow")?;
    ensure!(
        end <= source.len(),
        "main dialogue workspace line is outside the source"
    );
    let literal_offsets = line
        .literal_file_offsets
        .iter()
        .copied()
        .collect::<BTreeSet<_>>();
    let mut markup = String::new();
    let mut cursor = line.file_offset;
    let mut final_control = None;
    while cursor < end {
        let code = source[cursor];
        if literal_offsets.contains(&cursor) {
            append_literal_markup(&mut markup, code);
            cursor += 1;
            continue;
        }

        let control = DIALOGUE_CONTROL_SPECS
            .iter()
            .find(|control| control.code == code)
            .with_context(|| {
                format!("workspace structural byte {code:02X} is not a dialogue control")
            })?;
        let control_storage_byte_count =
            1 + control.inline_operand_byte_count + control.transition_target_byte_count;
        let control_end = cursor
            .checked_add(control_storage_byte_count)
            .context("workspace control range overflow")?;
        ensure!(
            control_end <= end,
            "workspace control {code:02X} crosses its source line"
        );
        markup.push('{');
        markup.push_str(&format!("{code:02X}"));
        for operand in &source[cursor + 1..control_end] {
            markup.push(':');
            markup.push_str(&format!("{operand:02X}"));
        }
        markup.push('}');
        final_control = Some(code);
        cursor = control_end;
    }
    ensure!(
        final_control == Some(line.line_end_control),
        "workspace line end control changed"
    );
    Ok(markup)
}

fn append_literal_markup(markup: &mut String, code: u8) {
    if let Some(glyph) = japanese_text_glyph(code) {
        markup.push_str(glyph);
        return;
    }
    match code {
        0x60..=0x69 => markup.push(char::from(b'0' + code - 0x60)),
        0x6A..=0x83 => markup.push(char::from(b'A' + code - 0x6A)),
        0x8D => markup.push(':'),
        0x9B => markup.push('.'),
        0xFF => markup.push_str("{SP}"),
        _ => markup.push_str(&format!("{{LIT:{code:02X}}}")),
    }
}

fn build_source_asset(source: &[u8]) -> Result<MainDialogueSourceAsset> {
    let records = inspect_main_dialogue_storage(source)?.records;
    let owned_ranges = normalize_storage_ranges(&records)?;
    let storage_regions = owned_ranges
        .iter()
        .copied()
        .enumerate()
        .map(|(index, range)| {
            let storage = source
                .get(range.start..range.end_exclusive)
                .with_context(|| {
                    format!("main dialogue source region {index} is outside the ROM")
                })?;
            Ok(SourceStorageRegion {
                index,
                source_prg_bank: range.source_prg_bank,
                file_offset: range.start,
                file_offset_hex: format!("0x{:05X}", range.start),
                end_file_offset_exclusive: range.end_exclusive,
                end_file_offset_exclusive_hex: format!("0x{:05X}", range.end_exclusive),
                storage_byte_count: storage.len(),
                storage_sha1: sha1_hex(storage),
                storage_hex: encode_hex(storage),
            })
        })
        .collect::<Result<Vec<_>>>()?;
    let record_references = records
        .iter()
        .map(|record| build_record_reference(record, &owned_ranges))
        .collect::<Result<Vec<_>>>()?;

    ensure!(
        record_references.len() == 504,
        "main dialogue source asset must contain exactly 504 canonical records"
    );
    Ok(MainDialogueSourceAsset {
        format_version: SOURCE_ASSET_FORMAT_VERSION,
        source_sha1: EXPECTED_SOURCE_SHA1.to_owned(),
        translate_from: "ja".to_owned(),
        translate_to: "ko".to_owned(),
        preserve_existing_english: true,
        purpose: "exact_source_roundtrip_only".to_owned(),
        storage_regions,
        records: record_references,
    })
}

fn normalize_storage_ranges(
    records: &[MainDialogueStorageRecord],
) -> Result<Vec<OwnedStorageRange>> {
    let mut ranges = records
        .iter()
        .map(|record| OwnedStorageRange {
            source_prg_bank: record.source_prg_bank,
            start: record.file_offset,
            end_exclusive: record.end_file_offset_exclusive,
        })
        .collect::<Vec<_>>();
    ranges.sort_unstable_by_key(|range| (range.source_prg_bank, range.start, range.end_exclusive));

    let mut owned_ranges: Vec<OwnedStorageRange> = Vec::new();
    for range in ranges {
        ensure!(
            range.start < range.end_exclusive,
            "main dialogue source record has an empty or reversed storage range"
        );
        if let Some(previous) = owned_ranges.last_mut()
            && previous.source_prg_bank == range.source_prg_bank
            && range.start <= previous.end_exclusive
        {
            previous.end_exclusive = previous.end_exclusive.max(range.end_exclusive);
            continue;
        }
        owned_ranges.push(range);
    }
    ensure!(
        owned_ranges.windows(2).all(|pair| {
            pair[0].source_prg_bank != pair[1].source_prg_bank
                || pair[0].end_exclusive < pair[1].start
        }),
        "normalized main dialogue source regions overlap or touch"
    );
    Ok(owned_ranges)
}

fn build_record_reference(
    record: &MainDialogueStorageRecord,
    owned_ranges: &[OwnedStorageRange],
) -> Result<SourceRecordReference> {
    let containing_regions = owned_ranges
        .iter()
        .enumerate()
        .filter(|(_, region)| {
            region.source_prg_bank == record.source_prg_bank
                && region.start <= record.file_offset
                && record.end_file_offset_exclusive <= region.end_exclusive
        })
        .collect::<Vec<_>>();
    ensure!(
        containing_regions.len() == 1,
        "{} entry {} is not owned by exactly one normalized source region",
        record.table_id,
        record.canonical_entry_index
    );
    let (storage_region_index, region) = containing_regions[0];
    ensure!(
        record.storage_byte_count == record.end_file_offset_exclusive - record.file_offset,
        "{} entry {} storage length changed",
        record.table_id,
        record.canonical_entry_index
    );

    Ok(SourceRecordReference {
        table_id: record.table_id.to_owned(),
        source_prg_bank: record.source_prg_bank,
        canonical_entry_index: record.canonical_entry_index,
        entry_indices: record.entry_indices.clone(),
        pointer_file_offsets: record.pointer_file_offsets.clone(),
        pointer_file_offsets_hex: record
            .pointer_file_offsets
            .iter()
            .map(|offset| format!("0x{offset:05X}"))
            .collect(),
        pointer_cpu_address: record.pointer_cpu_address,
        pointer_cpu_address_hex: format!("0x{:04X}", record.pointer_cpu_address),
        storage_region_index,
        region_relative_offset: record.file_offset - region.start,
        storage_byte_count: record.storage_byte_count,
        storage_sha1: record.storage_sha1.clone(),
        prefix_byte_count: record.prefix_byte_count,
        boundary_control: record.boundary_control,
        boundary_control_hex: format!("{:02X}", record.boundary_control),
    })
}

fn encode_hex(bytes: &[u8]) -> String {
    bytes.iter().map(|byte| format!("{byte:02x}")).collect()
}

fn decode_hex(encoded: &str) -> Result<Vec<u8>> {
    ensure!(
        encoded.len().is_multiple_of(2),
        "hex storage has an odd number of digits"
    );
    encoded
        .as_bytes()
        .chunks_exact(2)
        .map(|pair| {
            let high = decode_hex_digit(pair[0])?;
            let low = decode_hex_digit(pair[1])?;
            Ok((high << 4) | low)
        })
        .collect()
}

fn decode_hex_digit(digit: u8) -> Result<u8> {
    match digit {
        b'0'..=b'9' => Ok(digit - b'0'),
        b'a'..=b'f' => Ok(digit - b'a' + 10),
        b'A'..=b'F' => Ok(digit - b'A' + 10),
        _ => anyhow::bail!("invalid hex digit {}", char::from(digit)),
    }
}

fn write_file(path: &Path, data: &[u8]) -> Result<()> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)
            .with_context(|| format!("create output directory {}", parent.display()))?;
    }
    fs::write(path, data).with_context(|| format!("write {}", path.display()))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn record(
        source_prg_bank: u8,
        file_offset: usize,
        end_file_offset_exclusive: usize,
    ) -> MainDialogueStorageRecord {
        MainDialogueStorageRecord {
            table_id: "synthetic-dialogue",
            source_prg_bank,
            canonical_entry_index: 0,
            entry_indices: vec![0],
            pointer_file_offsets: vec![0],
            pointer_cpu_address: 0x8000,
            file_offset,
            end_file_offset_exclusive,
            storage_byte_count: end_file_offset_exclusive - file_offset,
            storage_sha1: String::new(),
            prefix_byte_count: 4,
            boundary_control: 0xEF,
            literal_file_offsets: Vec::new(),
            lines: Vec::new(),
        }
    }

    fn workspace_line(source_markup: &str, korean: &str) -> WorkspaceLine {
        WorkspaceLine {
            id: "synthetic-dialogue:000:line:00".to_owned(),
            index: 0,
            file_offset_hex: "0x00000".to_owned(),
            source_storage_sha1: "source-line".to_owned(),
            source_markup: source_markup.to_owned(),
            korean: korean.to_owned(),
            status: TranslationStatus::Complete,
            japanese_source_byte_count: 3,
            safe_japanese_source_byte_count: 3,
            requires_relocation: false,
            conflicting_file_offsets_hex: Vec::new(),
        }
    }

    fn logical_record(id: &str, bytes: &[u8]) -> LogicalDialogueRecord {
        LogicalDialogueRecord {
            id: id.to_owned(),
            source_prg_bank: 2,
            source_pointer_cpu_address: 0x8000,
            pointer_file_offsets: vec![0],
            source_file_offset: 0,
            source_storage_byte_count: bytes.len(),
            translated_line_count: 0,
            bytes: bytes
                .iter()
                .copied()
                .map(LogicalDialogueByte::Encoded)
                .collect(),
        }
    }

    #[test]
    fn normalizes_shared_and_adjacent_records_into_disjoint_owned_regions() {
        let records = [
            record(2, 10, 20),
            record(2, 15, 25),
            record(2, 25, 30),
            record(2, 40, 45),
            record(3, 10, 15),
        ];

        let ranges = normalize_storage_ranges(&records).unwrap();

        assert_eq!(
            ranges,
            vec![
                OwnedStorageRange {
                    source_prg_bank: 2,
                    start: 10,
                    end_exclusive: 30,
                },
                OwnedStorageRange {
                    source_prg_bank: 2,
                    start: 40,
                    end_exclusive: 45,
                },
                OwnedStorageRange {
                    source_prg_bank: 3,
                    start: 10,
                    end_exclusive: 15,
                },
            ]
        );
    }

    #[test]
    fn hex_storage_roundtrips_and_rejects_non_hex_input() {
        let bytes = [0x00, 0x7F, 0x80, 0xFF];

        assert_eq!(decode_hex(&encode_hex(&bytes)).unwrap(), bytes);
        assert!(decode_hex("0").is_err());
        assert!(decode_hex("gg").is_err());
    }

    #[test]
    fn decodes_japanese_latin_unknown_literals_and_controls_without_mixing_them() {
        let source = [0x3A, 0x32, 0x5F, 0x44, 0x0F, 0xED];
        let line = MainDialogueStorageLine {
            file_offset: 0,
            storage_byte_count: source.len(),
            storage_sha1: String::new(),
            line_end_control: 0xED,
            literal_file_offsets: (0..5).collect(),
        };
        assert_eq!(
            decode_line_markup(&source, &line).unwrap(),
            "サウント゛{ED}"
        );

        let source = [0x7C, 0x7D, 0x7B, 0xFF, 0x9D, 0xE9, 0x03, 0xEF];
        let line = MainDialogueStorageLine {
            file_offset: 0,
            storage_byte_count: source.len(),
            storage_sha1: String::new(),
            line_end_control: 0xEF,
            literal_file_offsets: (0..5).collect(),
        };
        assert_eq!(
            decode_line_markup(&source, &line).unwrap(),
            "STR{SP}{LIT:9D}{E9:03}{EF}"
        );
    }

    #[test]
    fn excludes_a_japanese_literal_that_an_overlapping_record_reads_as_structure() {
        let source = vec![0x00; 8];
        let mut first = record(2, 0, 4);
        first.literal_file_offsets = vec![0, 1];
        let mut second = record(2, 1, 4);
        second.literal_file_offsets = vec![2];

        let safe = safe_japanese_literal_offsets(&source, &[first, second]).unwrap();

        assert_eq!(safe, BTreeSet::from([0]));
    }

    #[test]
    fn accepts_hangul_while_preserving_existing_english_and_control_tokens() {
        let line = workspace_line("マルスSTR{SP}{E9:03}{EF}", "마르스STR{SP}{E9:03}{EF}");

        assert_eq!(validate_translation_markup(&line).unwrap(), 3);
    }

    #[test]
    fn rejects_changed_existing_english_in_a_korean_target() {
        let line = workspace_line("マルスSTR{SP}{E9:03}{EF}", "마르스SKI{SP}{E9:03}{EF}");

        let error = validate_translation_markup(&line).unwrap_err().to_string();
        assert!(error.contains("existing English"));
    }

    #[test]
    fn rejects_changed_or_moved_control_tokens() {
        let changed = workspace_line("マルス{E9:03}{EF}", "마르스{E9:04}{EF}");
        assert!(
            validate_translation_markup(&changed)
                .unwrap_err()
                .to_string()
                .contains("protected control token")
        );

        let moved = workspace_line("マルス{EF}", "마르스{EF}님");
        assert!(
            validate_translation_markup(&moved)
                .unwrap_err()
                .to_string()
                .contains("line-end control token")
        );
    }

    #[test]
    fn rejects_japanese_remaining_in_a_korean_target() {
        let line = workspace_line("マルス{EF}", "마르ス{EF}");

        let error = validate_translation_markup(&line).unwrap_err().to_string();
        assert!(error.contains("inspect korean markup"));
    }

    #[test]
    fn encodes_target_glyphs_as_logical_bytes_and_preserves_source_codes() {
        assert_eq!(
            encode_korean_markup("한STR{SP}{E9:03}{EF}").unwrap(),
            vec![
                LogicalDialogueByte::TargetGlyph('한'),
                LogicalDialogueByte::Encoded(0x7C),
                LogicalDialogueByte::Encoded(0x7D),
                LogicalDialogueByte::Encoded(0x7B),
                LogicalDialogueByte::Encoded(0xFF),
                LogicalDialogueByte::Encoded(0xE9),
                LogicalDialogueByte::Encoded(0x03),
                LogicalDialogueByte::Encoded(0xEF),
            ]
        );
        assert!(decode_protected_token("{E9}").is_err());
        assert!(decode_protected_token("{E9:03:04}").is_err());
    }

    #[test]
    fn packs_shared_suffixes_once_and_splits_changed_records() {
        let whole = logical_record("whole", &[0x10, 0x20, 0x30]);
        let shared_tail = logical_record("tail", &[0x20, 0x30]);
        let (storage, placements) = pack_logical_records(&[&whole, &shared_tail]);
        assert_eq!(
            storage,
            vec![
                LogicalDialogueByte::Encoded(0x10),
                LogicalDialogueByte::Encoded(0x20),
                LogicalDialogueByte::Encoded(0x30),
            ]
        );
        assert_eq!(placements, vec![0, 1]);

        let changed_tail = LogicalDialogueRecord {
            bytes: vec![
                LogicalDialogueByte::TargetGlyph('한'),
                LogicalDialogueByte::Encoded(0x30),
            ],
            translated_line_count: 1,
            ..logical_record("changed-tail", &[0x20, 0x30])
        };
        let (storage, placements) = pack_logical_records(&[&whole, &changed_tail]);
        assert_eq!(storage.len(), 5);
        assert_eq!(placements, vec![0, 3]);
    }
}
