use std::{fs, path::Path};

use anyhow::{Context, Result, ensure};
use serde::{Deserialize, Serialize};

use crate::{
    dialogue_inventory::{MainDialogueStorageRecord, inspect_main_dialogue_storage},
    rom::{EXPECTED_SOURCE_SHA1, Rom},
    sha1_hex,
    tracked::TrackedImage,
};

const SOURCE_ASSET_FORMAT_VERSION: u8 = 1;

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
}
