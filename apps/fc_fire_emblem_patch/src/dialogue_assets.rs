use std::{collections::BTreeSet, fs, path::Path};

use anyhow::{Context, Result, ensure};
use serde::{Deserialize, Serialize};

use crate::{
    dialogue_inventory::{
        MainDialogueStorageLine, MainDialogueStorageRecord, inspect_main_dialogue_storage,
    },
    japanese_encoding::{is_japanese_text_code, japanese_text_glyph},
    rom::{EXPECTED_SOURCE_SHA1, Rom},
    sha1_hex,
    text_inventory::DIALOGUE_CONTROL_SPECS,
    tracked::TrackedImage,
};

const SOURCE_ASSET_FORMAT_VERSION: u8 = 1;
const WORKSPACE_FORMAT_VERSION: u8 = 1;

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

#[derive(Debug, Serialize)]
struct MainDialogueWorkspace {
    format_version: u8,
    source_sha1: String,
    translate_from: &'static str,
    translate_to: &'static str,
    preserve_existing_english: bool,
    purpose: &'static str,
    safe_japanese_source_byte_count: usize,
    records: Vec<WorkspaceRecord>,
}

#[derive(Debug, Serialize)]
struct WorkspaceRecord {
    table_id: &'static str,
    source_prg_bank: u8,
    canonical_entry_index: usize,
    entry_indices: Vec<usize>,
    pointer_cpu_address_hex: String,
    prefix_byte_count: usize,
    boundary_control_hex: String,
    lines: Vec<WorkspaceLine>,
}

#[derive(Debug, Serialize)]
struct WorkspaceLine {
    index: usize,
    file_offset_hex: String,
    source_storage_sha1: String,
    source_markup: String,
    korean: String,
    japanese_source_byte_count: usize,
    safe_japanese_source_byte_count: usize,
    requires_relocation: bool,
    conflicting_file_offsets_hex: Vec<String>,
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
    let records = inspect_main_dialogue_storage(rom.data())?;
    let safe_japanese_offsets = safe_japanese_literal_offsets(rom.data(), &records)?;
    let workspace_records = records
        .iter()
        .map(|record| build_workspace_record(rom.data(), record, &safe_japanese_offsets))
        .collect::<Result<Vec<_>>>()?;
    let line_count = workspace_records
        .iter()
        .map(|record| record.lines.len())
        .sum();
    let blocked_line_count = workspace_records
        .iter()
        .flat_map(|record| &record.lines)
        .filter(|line| line.requires_relocation)
        .count();
    let workspace = MainDialogueWorkspace {
        format_version: WORKSPACE_FORMAT_VERSION,
        source_sha1: EXPECTED_SOURCE_SHA1.to_owned(),
        translate_from: "ja",
        translate_to: "ko",
        preserve_existing_english: true,
        purpose: "private_translation_workspace",
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
        safe_japanese_offsets.len() == 27_900,
        "main dialogue workspace Japanese source boundary changed"
    );
    let mut workspace_bytes =
        serde_json::to_vec_pretty(&workspace).context("serialize main dialogue workspace")?;
    workspace_bytes.push(b'\n');
    write_file(workspace_path, &workspace_bytes)?;

    Ok(DialogueWorkspaceSummary {
        workspace_sha1: sha1_hex(&workspace_bytes),
        record_count: workspace.records.len(),
        line_count,
        safe_japanese_source_byte_count: safe_japanese_offsets.len(),
        blocked_line_count,
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
                index,
                file_offset_hex: format!("0x{:05X}", line.file_offset),
                source_storage_sha1: line.storage_sha1.clone(),
                source_markup: decode_line_markup(source, line)?,
                korean: String::new(),
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
        table_id: record.table_id,
        source_prg_bank: record.source_prg_bank,
        canonical_entry_index: record.canonical_entry_index,
        entry_indices: record.entry_indices.clone(),
        pointer_cpu_address_hex: format!("0x{:04X}", record.pointer_cpu_address),
        prefix_byte_count: record.prefix_byte_count,
        boundary_control_hex: format!("{:02X}", record.boundary_control),
        lines,
    })
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
    let records = inspect_main_dialogue_storage(source)?;
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
}
