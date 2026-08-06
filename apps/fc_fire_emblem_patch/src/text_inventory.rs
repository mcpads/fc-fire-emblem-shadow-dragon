use std::{
    collections::{BTreeMap, BTreeSet},
    fs,
    path::Path,
};

use anyhow::{Context, Result, ensure};
use serde::Serialize;

use crate::{
    rom::{EXPECTED_SOURCE_SHA1, HEADER_SIZE, PRG_SIZE, Rom},
    sha1_hex,
};

const PRG_BANK_SIZE: usize = 16 * 1024;
const FIXED_BANK_CPU_BASE: u16 = 0xC000;
const FIXED_BANK_FILE_OFFSET: usize = HEADER_SIZE + PRG_SIZE - PRG_BANK_SIZE;
const PRG_FILE_END: usize = HEADER_SIZE + PRG_SIZE;
const MAX_ENTRY_BYTES: usize = 256;

struct TextTableSpec {
    id: &'static str,
    role: &'static str,
    table_file_offset: usize,
    pointer_count: usize,
    terminator: u8,
    consumer_file_offset: usize,
    consumer_bytes: [u8; 10],
    protected_positions: &'static [ProtectedPosition],
}

struct ProtectedPosition {
    entry_index: usize,
    byte_offset: usize,
    code: u8,
    glyph: &'static str,
}

const TEXT_TABLE_SPECS: [TextTableSpec; 7] = [
    TextTableSpec {
        id: "class-names",
        role: "class names",
        table_file_offset: 0x3DA2F,
        pointer_count: 0x17,
        terminator: 0xEF,
        consumer_file_offset: 0x14D63,
        consumer_bytes: [0xBD, 0x1F, 0xDA, 0x85, 0x00, 0xBD, 0x20, 0xDA, 0x85, 0x01],
        protected_positions: &[],
    },
    TextTableSpec {
        id: "item-names",
        role: "item names",
        table_file_offset: 0x3DAE5,
        pointer_count: 0x5B,
        terminator: 0xEF,
        consumer_file_offset: 0x0DC63,
        consumer_bytes: [0xB9, 0xD5, 0xDA, 0x85, 0x00, 0xB9, 0xD6, 0xDA, 0x85, 0x01],
        protected_positions: &[ProtectedPosition {
            entry_index: 60,
            byte_offset: 1,
            code: 0x9B,
            glyph: ".",
        }],
    },
    TextTableSpec {
        id: "unit-names",
        role: "playable unit names",
        table_file_offset: 0x3DE3B,
        pointer_count: 0x34,
        terminator: 0xEF,
        consumer_file_offset: 0x19B48,
        consumer_bytes: [0xB9, 0x2B, 0xDE, 0x85, 0x00, 0xB9, 0x2C, 0xDE, 0x85, 0x01],
        protected_positions: &[],
    },
    TextTableSpec {
        id: "enemy-names",
        role: "enemy names",
        table_file_offset: 0x3DFB4,
        pointer_count: 0x44,
        terminator: 0xEF,
        consumer_file_offset: 0x2CEAA,
        consumer_bytes: [0xB9, 0xA4, 0xDF, 0x85, 0x00, 0xB9, 0xA5, 0xDF, 0x85, 0x01],
        protected_positions: &[],
    },
    TextTableSpec {
        id: "terrain-names",
        role: "terrain names",
        table_file_offset: 0x3E601,
        pointer_count: 0x0F,
        terminator: 0xEF,
        consumer_file_offset: 0x1C497,
        consumer_bytes: [0xB9, 0xF1, 0xE5, 0x85, 0x08, 0xB9, 0xF2, 0xE5, 0x85, 0x09],
        protected_positions: &[],
    },
    TextTableSpec {
        id: "location-names",
        role: "location names",
        table_file_offset: 0x3EFC7,
        pointer_count: 0x18,
        terminator: 0xED,
        consumer_file_offset: 0x121D0,
        consumer_bytes: [0xB9, 0xB7, 0xEF, 0x85, 0x04, 0xB9, 0xB8, 0xEF, 0x85, 0x05],
        protected_positions: &[],
    },
    TextTableSpec {
        id: "chapter-names",
        role: "chapter names",
        table_file_offset: 0x3EE18,
        pointer_count: 0x18,
        terminator: 0xED,
        consumer_file_offset: 0x2CEF2,
        consumer_bytes: [0xB9, 0x08, 0xEE, 0x85, 0x00, 0xB9, 0x09, 0xEE, 0x85, 0x01],
        protected_positions: &[],
    },
];

#[derive(Debug)]
pub struct TextInventorySummary {
    pub report_sha1: String,
    pub table_count: usize,
    pub pointer_count: usize,
    pub unique_string_count: usize,
    pub referenced_protected_original_byte_count: usize,
}

#[derive(Debug, Serialize)]
struct TextInventoryReport {
    schema_version: u8,
    scope: ReportScope,
    summary: ReportSummary,
    tables: Vec<TextTableReport>,
    unknowns: Vec<&'static str>,
}

#[derive(Debug, Serialize)]
struct ReportScope {
    source_sha1: &'static str,
    translation_direction: &'static str,
    preserve_existing_english: bool,
    proof_boundary: &'static str,
}

#[derive(Debug, Serialize)]
struct ReportSummary {
    table_count: usize,
    pointer_count: usize,
    unique_string_count: usize,
    referenced_text_byte_count: usize,
    unique_text_storage_byte_count: usize,
    referenced_protected_original_byte_count: usize,
    unique_protected_original_byte_count: usize,
    referenced_unresolved_byte_count: usize,
    unique_unresolved_byte_count: usize,
}

#[derive(Debug, Serialize)]
struct TextTableReport {
    id: &'static str,
    role: &'static str,
    table_file_offset: usize,
    table_file_offset_hex: String,
    table_cpu_address: u16,
    table_cpu_address_hex: String,
    pointer_count: usize,
    unique_string_count: usize,
    pointer_table_sha1: String,
    terminator: u8,
    terminator_hex: String,
    consumer: ConsumerEvidence,
    data_file_start: usize,
    data_file_start_hex: String,
    data_file_end_exclusive: usize,
    data_file_end_exclusive_hex: String,
    referenced_text_byte_count: usize,
    unique_text_storage_byte_count: usize,
    referenced_protected_original_byte_count: usize,
    unique_protected_original_byte_count: usize,
    referenced_unresolved_byte_count: usize,
    unique_unresolved_byte_count: usize,
    entries: Vec<TextEntryReport>,
}

#[derive(Debug, Serialize)]
struct ConsumerEvidence {
    file_offset: usize,
    file_offset_hex: String,
    prg_bank: usize,
    prg_bank_hex: String,
    cpu_address: u16,
    cpu_address_hex: String,
    instruction_bytes_hex: String,
    pointer_load_mode: &'static str,
    destination_pointer: String,
}

#[derive(Debug, Serialize)]
struct TextEntryReport {
    index: usize,
    pointer_cpu_address: u16,
    pointer_cpu_address_hex: String,
    file_offset: usize,
    file_offset_hex: String,
    byte_length: usize,
    raw_bytes_hex: String,
    raw_sha1: String,
    alias_entry_indices: Vec<usize>,
    protected_original: Vec<ProtectedByte>,
    unresolved_byte_count: usize,
}

#[derive(Debug, Serialize)]
struct ProtectedByte {
    byte_offset: usize,
    code: u8,
    code_hex: String,
    glyph: String,
}

pub fn analyze_text_tables(source_path: &Path, report_path: &Path) -> Result<TextInventorySummary> {
    let rom = Rom::from_path(source_path)?;
    rom.verify_supported_japanese()?;
    let report = build_report(rom.data())?;
    let mut report_bytes = serde_json::to_vec_pretty(&report).context("serialize text report")?;
    report_bytes.push(b'\n');

    if let Some(parent) = report_path.parent() {
        fs::create_dir_all(parent)
            .with_context(|| format!("create output directory {}", parent.display()))?;
    }
    fs::write(report_path, &report_bytes)
        .with_context(|| format!("write {}", report_path.display()))?;

    Ok(TextInventorySummary {
        report_sha1: sha1_hex(&report_bytes),
        table_count: report.summary.table_count,
        pointer_count: report.summary.pointer_count,
        unique_string_count: report.summary.unique_string_count,
        referenced_protected_original_byte_count: report
            .summary
            .referenced_protected_original_byte_count,
    })
}

fn build_report(source: &[u8]) -> Result<TextInventoryReport> {
    let tables: Vec<TextTableReport> = TEXT_TABLE_SPECS
        .iter()
        .map(|spec| extract_table(source, spec))
        .collect::<Result<_>>()?;
    let pointer_count = tables.iter().map(|table| table.pointer_count).sum();
    let unique_string_count = tables.iter().map(|table| table.unique_string_count).sum();
    let referenced_text_byte_count = tables
        .iter()
        .map(|table| table.referenced_text_byte_count)
        .sum();
    let unique_text_storage_byte_count = tables
        .iter()
        .map(|table| table.unique_text_storage_byte_count)
        .sum();
    let referenced_protected_original_byte_count = tables
        .iter()
        .map(|table| table.referenced_protected_original_byte_count)
        .sum();
    let unique_protected_original_byte_count = tables
        .iter()
        .map(|table| table.unique_protected_original_byte_count)
        .sum();
    let referenced_unresolved_byte_count = tables
        .iter()
        .map(|table| table.referenced_unresolved_byte_count)
        .sum();
    let unique_unresolved_byte_count = tables
        .iter()
        .map(|table| table.unique_unresolved_byte_count)
        .sum();

    Ok(TextInventoryReport {
        schema_version: 1,
        scope: ReportScope {
            source_sha1: EXPECTED_SOURCE_SHA1,
            translation_direction: "ja_to_ko",
            preserve_existing_english: true,
            proof_boundary: "confirmed pointer tables and one exact static consumer per table",
        },
        summary: ReportSummary {
            table_count: tables.len(),
            pointer_count,
            unique_string_count,
            referenced_text_byte_count,
            unique_text_storage_byte_count,
            referenced_protected_original_byte_count,
            unique_protected_original_byte_count,
            referenced_unresolved_byte_count,
            unique_unresolved_byte_count,
        },
        tables,
        unknowns: vec![
            "This is not the complete game text population.",
            "Non-Latin bytes remain unresolved Japanese, layout, icon, or control codes until decoder semantics are proven.",
            "No entry is translation-ready until control tokens, layout, and relocation policy are declared.",
        ],
    })
}

fn extract_table(source: &[u8], spec: &TextTableSpec) -> Result<TextTableReport> {
    ensure!(
        source.len() >= PRG_FILE_END,
        "source is shorter than the PRG region"
    );
    validate_consumer(source, spec)?;

    let table_byte_length = spec
        .pointer_count
        .checked_mul(2)
        .context("pointer table length overflow")?;
    let table_end = spec
        .table_file_offset
        .checked_add(table_byte_length)
        .context("pointer table range overflow")?;
    ensure!(
        (FIXED_BANK_FILE_OFFSET..=PRG_FILE_END).contains(&spec.table_file_offset)
            && table_end <= PRG_FILE_END,
        "text table {} is outside the fixed PRG bank",
        spec.id
    );
    let table_bytes = &source[spec.table_file_offset..table_end];
    let pointers: Vec<u16> = table_bytes
        .chunks_exact(2)
        .map(|bytes| u16::from_le_bytes([bytes[0], bytes[1]]))
        .collect();
    let mut pointer_indices: BTreeMap<u16, Vec<usize>> = BTreeMap::new();
    for (index, pointer) in pointers.iter().enumerate() {
        pointer_indices.entry(*pointer).or_default().push(index);
    }

    let mut ranges = Vec::new();
    let mut entries = Vec::with_capacity(pointers.len());
    for (index, pointer) in pointers.iter().enumerate() {
        let file_offset = fixed_cpu_to_file_offset(*pointer)
            .with_context(|| format!("{} entry {index}", spec.id))?;
        ensure!(
            file_offset >= table_end,
            "{} entry {index} points into or before its pointer table",
            spec.id
        );
        let search_end = file_offset
            .checked_add(MAX_ENTRY_BYTES + 1)
            .unwrap_or(PRG_FILE_END)
            .min(PRG_FILE_END);
        let terminator_offset = source[file_offset..search_end]
            .iter()
            .position(|byte| *byte == spec.terminator)
            .map(|relative| file_offset + relative)
            .with_context(|| {
                format!(
                    "{} entry {index} has no {:02X} terminator within {MAX_ENTRY_BYTES} bytes",
                    spec.id, spec.terminator
                )
            })?;
        let raw = &source[file_offset..terminator_offset];
        for position in spec
            .protected_positions
            .iter()
            .filter(|position| position.entry_index == index)
        {
            ensure!(
                raw.get(position.byte_offset) == Some(&position.code),
                "protected original byte changed for {} entry {index} at byte {}",
                spec.id,
                position.byte_offset
            );
        }
        let alias_entry_indices = pointer_indices[pointer]
            .iter()
            .copied()
            .filter(|other| *other != index)
            .collect();
        let mut protected_original = Vec::new();
        for (byte_offset, code) in raw.iter().enumerate() {
            let declared = spec.protected_positions.iter().find(|position| {
                position.entry_index == index && position.byte_offset == byte_offset
            });
            let glyph = if let Some(position) = declared {
                ensure!(
                    *code == position.code,
                    "protected original byte changed for {} entry {index} at byte {byte_offset}",
                    spec.id
                );
                Some(position.glyph)
            } else {
                protected_alphanumeric_glyph(*code)
            };
            if let Some(glyph) = glyph {
                protected_original.push(ProtectedByte {
                    byte_offset,
                    code: *code,
                    code_hex: format!("{code:02X}"),
                    glyph: glyph.to_owned(),
                });
            }
        }
        let unresolved_byte_count = raw.len() - protected_original.len();

        ranges.push((file_offset, terminator_offset + 1));
        entries.push(TextEntryReport {
            index,
            pointer_cpu_address: *pointer,
            pointer_cpu_address_hex: format!("0x{pointer:04X}"),
            file_offset,
            file_offset_hex: format!("0x{file_offset:05X}"),
            byte_length: raw.len(),
            raw_bytes_hex: hex_bytes(raw),
            raw_sha1: sha1_hex(raw),
            alias_entry_indices,
            protected_original,
            unresolved_byte_count,
        });
    }
    validate_unique_ranges(spec.id, &ranges)?;

    let data_file_start = ranges
        .iter()
        .map(|(start, _)| *start)
        .min()
        .context("text table has no entries")?;
    let data_file_end_exclusive = ranges
        .iter()
        .map(|(_, end)| *end)
        .max()
        .context("text table has no entries")?;
    let referenced_protected_original_byte_count = entries
        .iter()
        .map(|entry| entry.protected_original.len())
        .sum();
    let referenced_unresolved_byte_count = entries
        .iter()
        .map(|entry| entry.unresolved_byte_count)
        .sum();
    let referenced_text_byte_count = entries.iter().map(|entry| entry.byte_length).sum();
    let first_entry_for_pointer = pointers
        .iter()
        .enumerate()
        .filter(|(index, pointer)| pointer_indices[pointer][0] == *index)
        .map(|(index, _)| &entries[index])
        .collect::<Vec<_>>();
    let unique_text_storage_byte_count = first_entry_for_pointer
        .iter()
        .map(|entry| entry.byte_length)
        .sum();
    let unique_protected_original_byte_count = first_entry_for_pointer
        .iter()
        .map(|entry| entry.protected_original.len())
        .sum();
    let unique_unresolved_byte_count = first_entry_for_pointer
        .iter()
        .map(|entry| entry.unresolved_byte_count)
        .sum();
    let table_cpu_address = fixed_file_to_cpu_address(spec.table_file_offset)?;
    let (consumer_prg_bank, consumer_cpu_address) = prg_file_location(spec.consumer_file_offset)?;
    let destination_pointer = format!(
        "0x{:02X}/0x{:02X}",
        spec.consumer_bytes[4], spec.consumer_bytes[9]
    );

    Ok(TextTableReport {
        id: spec.id,
        role: spec.role,
        table_file_offset: spec.table_file_offset,
        table_file_offset_hex: format!("0x{:05X}", spec.table_file_offset),
        table_cpu_address,
        table_cpu_address_hex: format!("0x{table_cpu_address:04X}"),
        pointer_count: spec.pointer_count,
        unique_string_count: pointer_indices.len(),
        pointer_table_sha1: sha1_hex(table_bytes),
        terminator: spec.terminator,
        terminator_hex: format!("{:02X}", spec.terminator),
        consumer: ConsumerEvidence {
            file_offset: spec.consumer_file_offset,
            file_offset_hex: format!("0x{:05X}", spec.consumer_file_offset),
            prg_bank: consumer_prg_bank,
            prg_bank_hex: format!("0x{consumer_prg_bank:02X}"),
            cpu_address: consumer_cpu_address,
            cpu_address_hex: format!("0x{consumer_cpu_address:04X}"),
            instruction_bytes_hex: hex_bytes(&spec.consumer_bytes),
            pointer_load_mode: if spec.consumer_bytes[0] == 0xBD {
                "absolute_x"
            } else {
                "absolute_y"
            },
            destination_pointer,
        },
        data_file_start,
        data_file_start_hex: format!("0x{data_file_start:05X}"),
        data_file_end_exclusive,
        data_file_end_exclusive_hex: format!("0x{data_file_end_exclusive:05X}"),
        referenced_text_byte_count,
        unique_text_storage_byte_count,
        referenced_protected_original_byte_count,
        unique_protected_original_byte_count,
        referenced_unresolved_byte_count,
        unique_unresolved_byte_count,
        entries,
    })
}

fn validate_consumer(source: &[u8], spec: &TextTableSpec) -> Result<()> {
    let end = spec
        .consumer_file_offset
        .checked_add(spec.consumer_bytes.len())
        .context("consumer range overflow")?;
    ensure!(end <= PRG_FILE_END, "consumer {} is outside PRG", spec.id);
    ensure!(
        source[spec.consumer_file_offset..end] == spec.consumer_bytes,
        "consumer bytes changed for {} at {:#X}",
        spec.id,
        spec.consumer_file_offset
    );

    let table_cpu_address = fixed_file_to_cpu_address(spec.table_file_offset)?;
    let next_address = table_cpu_address + 1;
    let opcode = spec.consumer_bytes[0];
    ensure!(
        [0xBD, 0xB9].contains(&opcode)
            && spec.consumer_bytes[3] == 0x85
            && spec.consumer_bytes[5] == opcode
            && spec.consumer_bytes[8] == 0x85,
        "consumer {} is not the declared indexed pointer load",
        spec.id
    );
    ensure!(
        spec.consumer_bytes[1..3] == table_cpu_address.to_le_bytes()
            && spec.consumer_bytes[6..8] == next_address.to_le_bytes(),
        "consumer {} does not load its pointer table",
        spec.id
    );
    ensure!(
        spec.consumer_bytes[9] == spec.consumer_bytes[4] + 1,
        "consumer {} does not store an adjacent pointer pair",
        spec.id
    );
    Ok(())
}

fn validate_unique_ranges(id: &str, ranges: &[(usize, usize)]) -> Result<()> {
    let unique: BTreeSet<(usize, usize)> = ranges.iter().copied().collect();
    let sorted = unique.iter().copied().collect::<Vec<_>>();
    for pair in sorted.windows(2) {
        ensure!(
            pair[0].1 <= pair[1].0,
            "text table {id} contains overlapping string ranges"
        );
    }
    Ok(())
}

fn fixed_cpu_to_file_offset(cpu_address: u16) -> Result<usize> {
    ensure!(
        cpu_address >= FIXED_BANK_CPU_BASE,
        "pointer ${cpu_address:04X} is outside the fixed PRG bank"
    );
    Ok(FIXED_BANK_FILE_OFFSET + usize::from(cpu_address - FIXED_BANK_CPU_BASE))
}

fn fixed_file_to_cpu_address(file_offset: usize) -> Result<u16> {
    ensure!(
        (FIXED_BANK_FILE_OFFSET..PRG_FILE_END).contains(&file_offset),
        "file offset {file_offset:#X} is outside the fixed PRG bank"
    );
    Ok(FIXED_BANK_CPU_BASE + (file_offset - FIXED_BANK_FILE_OFFSET) as u16)
}

fn prg_file_location(file_offset: usize) -> Result<(usize, u16)> {
    ensure!(
        (HEADER_SIZE..PRG_FILE_END).contains(&file_offset),
        "file offset {file_offset:#X} is outside PRG"
    );
    let prg_offset = file_offset - HEADER_SIZE;
    let prg_bank = prg_offset / PRG_BANK_SIZE;
    let offset_in_bank = prg_offset % PRG_BANK_SIZE;
    let cpu_base = if prg_bank == PRG_SIZE / PRG_BANK_SIZE - 1 {
        0xC000
    } else {
        0x8000
    };
    Ok((prg_bank, cpu_base + offset_in_bank as u16))
}

fn protected_alphanumeric_glyph(code: u8) -> Option<&'static str> {
    const DIGITS: [&str; 10] = ["0", "1", "2", "3", "4", "5", "6", "7", "8", "9"];
    const UPPERCASE: [&str; 26] = [
        "A", "B", "C", "D", "E", "F", "G", "H", "I", "J", "K", "L", "M", "N", "O", "P", "Q", "R",
        "S", "T", "U", "V", "W", "X", "Y", "Z",
    ];
    match code {
        0x60..=0x69 => Some(DIGITS[(code - 0x60) as usize]),
        0x6A..=0x83 => Some(UPPERCASE[(code - 0x6A) as usize]),
        _ => None,
    }
}

fn hex_bytes(bytes: &[u8]) -> String {
    bytes
        .iter()
        .map(|byte| format!("{byte:02X}"))
        .collect::<Vec<_>>()
        .join(" ")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn synthetic_spec(table_file_offset: usize, consumer_file_offset: usize) -> TextTableSpec {
        let table_cpu_address = fixed_file_to_cpu_address(table_file_offset).unwrap();
        let [low, high] = table_cpu_address.to_le_bytes();
        let [next_low, next_high] = (table_cpu_address + 1).to_le_bytes();
        TextTableSpec {
            id: "synthetic-names",
            role: "synthetic names",
            table_file_offset,
            pointer_count: 2,
            terminator: 0xEF,
            consumer_file_offset,
            consumer_bytes: [
                0xB9, low, high, 0x85, 0x00, 0xB9, next_low, next_high, 0x85, 0x01,
            ],
            protected_positions: &[],
        }
    }

    #[test]
    fn extracts_aliases_without_translating_preserved_latin() {
        let table_file_offset = FIXED_BANK_FILE_OFFSET + 0x0100;
        let consumer_file_offset = HEADER_SIZE + 0x0200;
        let spec = synthetic_spec(table_file_offset, consumer_file_offset);
        let mut source = vec![0_u8; PRG_FILE_END];
        source[consumer_file_offset..consumer_file_offset + 10]
            .copy_from_slice(&spec.consumer_bytes);
        let text_cpu_address = FIXED_BANK_CPU_BASE + 0x0200;
        let pointer = text_cpu_address.to_le_bytes();
        source[table_file_offset..table_file_offset + 2].copy_from_slice(&pointer);
        source[table_file_offset + 2..table_file_offset + 4].copy_from_slice(&pointer);
        let text_file_offset = FIXED_BANK_FILE_OFFSET + 0x0200;
        source[text_file_offset..text_file_offset + 4].copy_from_slice(&[0x6A, 0x30, 0x60, 0xEF]);

        let report = extract_table(&source, &spec).unwrap();

        assert_eq!(report.pointer_count, 2);
        assert_eq!(report.unique_string_count, 1);
        assert_eq!(report.entries[0].alias_entry_indices, vec![1]);
        assert_eq!(report.entries[1].alias_entry_indices, vec![0]);
        assert_eq!(report.entries[0].protected_original.len(), 2);
        assert_eq!(report.entries[0].protected_original[0].glyph, "A");
        assert_eq!(report.entries[0].protected_original[1].glyph, "0");
        assert_eq!(report.entries[0].unresolved_byte_count, 1);
    }

    #[test]
    fn protects_punctuation_only_at_a_declared_token_position() {
        let table_file_offset = FIXED_BANK_FILE_OFFSET + 0x0100;
        let consumer_file_offset = HEADER_SIZE + 0x0200;
        let mut spec = synthetic_spec(table_file_offset, consumer_file_offset);
        spec.protected_positions = &[ProtectedPosition {
            entry_index: 0,
            byte_offset: 1,
            code: 0x9B,
            glyph: ".",
        }];
        let mut source = vec![0_u8; PRG_FILE_END];
        source[consumer_file_offset..consumer_file_offset + 10]
            .copy_from_slice(&spec.consumer_bytes);
        for (index, text_offset) in [0x0200_u16, 0x0210].iter().enumerate() {
            let pointer = (FIXED_BANK_CPU_BASE + *text_offset).to_le_bytes();
            let pointer_offset = table_file_offset + index * 2;
            source[pointer_offset..pointer_offset + 2].copy_from_slice(&pointer);
            let text_file_offset = FIXED_BANK_FILE_OFFSET + usize::from(*text_offset);
            source[text_file_offset..text_file_offset + 4]
                .copy_from_slice(&[0x76, 0x9B, 0x30, 0xEF]);
        }

        let report = extract_table(&source, &spec).unwrap();

        assert_eq!(report.entries[0].protected_original.len(), 2);
        assert_eq!(report.entries[0].protected_original[0].glyph, "M");
        assert_eq!(report.entries[0].protected_original[1].glyph, ".");
        assert_eq!(report.entries[1].protected_original.len(), 1);
        assert_eq!(report.entries[1].unresolved_byte_count, 2);
    }

    #[test]
    fn rejects_consumer_bytes_that_no_longer_load_the_table() {
        let table_file_offset = FIXED_BANK_FILE_OFFSET + 0x0100;
        let consumer_file_offset = HEADER_SIZE + 0x0200;
        let spec = synthetic_spec(table_file_offset, consumer_file_offset);
        let mut source = vec![0_u8; PRG_FILE_END];
        source[consumer_file_offset..consumer_file_offset + 10]
            .copy_from_slice(&spec.consumer_bytes);
        source[consumer_file_offset + 1] ^= 0x01;

        let error = validate_consumer(&source, &spec).unwrap_err().to_string();

        assert!(error.contains("consumer bytes changed for synthetic-names"));
    }
}
