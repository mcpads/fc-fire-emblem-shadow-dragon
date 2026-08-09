use super::*;

pub(super) fn decode_line_markup(source: &[u8], line: &MainDialogueStorageLine) -> Result<String> {
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

pub(super) fn append_literal_markup(markup: &mut String, code: u8) {
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

pub(super) fn build_source_asset(source: &[u8]) -> Result<MainDialogueSourceAsset> {
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

pub(super) fn normalize_storage_ranges(
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

pub(super) fn build_record_reference(
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

pub(super) fn encode_hex(bytes: &[u8]) -> String {
    bytes.iter().map(|byte| format!("{byte:02x}")).collect()
}

pub(super) fn decode_hex(encoded: &str) -> Result<Vec<u8>> {
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

pub(super) fn decode_hex_digit(digit: u8) -> Result<u8> {
    match digit {
        b'0'..=b'9' => Ok(digit - b'0'),
        b'a'..=b'f' => Ok(digit - b'a' + 10),
        b'A'..=b'F' => Ok(digit - b'A' + 10),
        _ => anyhow::bail!("invalid hex digit {}", char::from(digit)),
    }
}

pub(super) fn write_file(path: &Path, data: &[u8]) -> Result<()> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)
            .with_context(|| format!("create output directory {}", parent.display()))?;
    }
    fs::write(path, data).with_context(|| format!("write {}", path.display()))
}

pub(super) fn write_file_atomically(path: &Path, data: &[u8]) -> Result<()> {
    use std::{
        fs::OpenOptions,
        io::Write,
        time::{SystemTime, UNIX_EPOCH},
    };

    let parent = path.parent().unwrap_or_else(|| Path::new("."));
    fs::create_dir_all(parent)
        .with_context(|| format!("create output directory {}", parent.display()))?;
    let file_name = path
        .file_name()
        .context("atomic output path must name a file")?
        .to_string_lossy();
    let nonce = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .context("system clock is before Unix epoch")?
        .as_nanos();
    let temporary_path = parent.join(format!(".{file_name}.tmp-{}-{nonce}", std::process::id()));
    let result = (|| -> Result<()> {
        let mut temporary = OpenOptions::new()
            .create_new(true)
            .write(true)
            .open(&temporary_path)
            .with_context(|| format!("create temporary output {}", temporary_path.display()))?;
        temporary
            .write_all(data)
            .with_context(|| format!("write temporary output {}", temporary_path.display()))?;
        temporary
            .sync_all()
            .with_context(|| format!("sync temporary output {}", temporary_path.display()))?;
        fs::rename(&temporary_path, path).with_context(|| {
            format!(
                "replace {} with temporary output {}",
                path.display(),
                temporary_path.display()
            )
        })?;
        Ok(())
    })();
    if result.is_err() {
        let _ = fs::remove_file(&temporary_path);
    }
    result
}
