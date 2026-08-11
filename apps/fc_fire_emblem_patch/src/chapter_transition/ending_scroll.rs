use std::collections::BTreeSet;

use anyhow::{Context, Result, ensure};
use serde::Serialize;

use crate::{
    font_slots::active_hangul_codes,
    japanese_encoding::is_japanese_text_code,
    rom::Rom,
    sha1_hex,
    source_literals::{
        SourceLiteralCodeClass, TranslationSurfaceLiteralInventory, classify_source_literal_code,
        classify_translation_surface_literal_codes,
    },
    text_inventory::decode_source_markup,
};

use super::{
    CHAPTER_TITLE_DATA_END_EXCLUSIVE, CHAPTER_TITLE_DIGIT_COUNT,
    CHAPTER_TITLE_POINTER_TABLE_ADDRESS, CHAPTER_TITLE_POINTER_TABLE_BYTES,
    CHAPTER_TITLE_TERMINATOR, source_file_offset,
};

const ENDING_SCROLL_STREAM_ADDRESS: u16 = 0xA826;
const ENDING_SCROLL_STREAM_END_EXCLUSIVE_ADDRESS: u16 = 0xACC8;
const ENDING_SCROLL_RECORD_END: u8 = 0xEF;
const ENDING_SCROLL_TERMINAL: u8 = 0xEE;
const ENDING_SCROLL_TURN_INTERPOLATION: u8 = 0xED;
const ENDING_SCROLL_PRESERVED_RECORD_COUNT: usize = 43;
const ENDING_SCROLL_CHAPTER_RECORD_COUNT: usize = 25;
const ENDING_SCROLL_AGGREGATE_RECORD_INDEX: usize = 93;
const ENDING_SCROLL_TRAILING_BLANK_RECORD_COUNT: usize = 19;

#[derive(Debug, Serialize)]
pub(super) struct EndingChapterRecordTranslationSurface {
    screen_role: &'static str,
    ending_phase_address: u16,
    ending_phase_address_hex: &'static str,
    ending_phase: u8,
    ending_phase_hex: &'static str,
    inner_state_address: u16,
    inner_state_address_hex: &'static str,
    stream: SourceRange,
    stream_sha1: String,
    record_count: usize,
    preserved_original_record_count: usize,
    chapter_record_count: usize,
    aggregate_record_count: usize,
    chapter_title_semantic_match_count: usize,
    protected_original_chapter_digit_byte_count: usize,
    japanese_literal_byte_count: usize,
    literal_inventory: TranslationSurfaceLiteralInventory,
    turn_interpolation_control: u8,
    turn_interpolation_control_hex: &'static str,
    chapter_turn_slot_range: &'static str,
    aggregate_turn_slot: u8,
    aggregate_turn_slot_hex: &'static str,
    trailing_blank_record_count: usize,
    semantic_source_policy: &'static str,
    translation_handling: &'static str,
}

#[derive(Debug, Serialize)]
struct SourceRange {
    prg_bank: u8,
    prg_bank_hex: String,
    cpu_address: u16,
    cpu_address_hex: String,
    end_exclusive_cpu_address: u16,
    end_exclusive_cpu_address_hex: String,
    file_offset: usize,
    file_offset_hex: String,
    byte_count: usize,
}

#[derive(Clone, Copy)]
struct EndingScrollRecord {
    header: u8,
    payload_start: usize,
    payload_end_exclusive: usize,
}

pub(super) struct EndingAggregateLabelSource {
    pub(super) japanese_markup: String,
    pub(super) max_visible_cells: usize,
    pub(super) source_reclaimable_active_codes: std::collections::BTreeSet<u8>,
}

pub(crate) struct EndingChapterRecordLifetimeSource {
    pub(crate) record_count: usize,
    pub(crate) target_record_count: usize,
    pub(crate) source_reclaimable_active_codes: BTreeSet<u8>,
    pub(crate) preserved_active_stream_codes: BTreeSet<u8>,
}

pub(crate) fn bind_ending_chapter_record_lifetime_source(
    rom: &Rom,
) -> Result<EndingChapterRecordLifetimeSource> {
    bind_ending_chapter_record_translation_surface(rom)?;
    let stream_file_offset = source_file_offset(0x04, ENDING_SCROLL_STREAM_ADDRESS)?;
    let stream_end_file_offset =
        source_file_offset(0x04, ENDING_SCROLL_STREAM_END_EXCLUSIVE_ADDRESS)?;
    let stream = &rom.data()[stream_file_offset..stream_end_file_offset];
    let records = parse_records(stream)?;
    let active_codes = active_hangul_codes().into_iter().collect::<BTreeSet<_>>();
    let mut source_reclaimable_active_codes = BTreeSet::new();
    let mut preserved_active_stream_codes = BTreeSet::new();
    let mut target_record_count = 0;

    for (record_index, record) in records.iter().enumerate() {
        let is_chapter_record = (0..ENDING_SCROLL_CHAPTER_RECORD_COUNT).any(|chapter_index| {
            record_index == ENDING_SCROLL_PRESERVED_RECORD_COUNT + chapter_index * 2
        });
        let is_target_record =
            is_chapter_record || record_index == ENDING_SCROLL_AGGREGATE_RECORD_INDEX;
        target_record_count += usize::from(is_target_record);
        let payload = &stream[record.payload_start..record.payload_end_exclusive];
        for code in ending_scroll_literal_codes(payload, "ending lifetime record")? {
            if !active_codes.contains(&code) {
                continue;
            }
            if is_target_record && is_japanese_text_code(code) {
                source_reclaimable_active_codes.insert(code);
            } else {
                preserved_active_stream_codes.insert(code);
            }
        }
    }
    ensure!(
        target_record_count == ENDING_SCROLL_CHAPTER_RECORD_COUNT + 1,
        "ending chapter-record lifetime target count changed"
    );
    ensure!(
        source_reclaimable_active_codes.is_disjoint(&preserved_active_stream_codes),
        "ending chapter-record stream reclaims a code used by preserved output"
    );

    Ok(EndingChapterRecordLifetimeSource {
        record_count: records.len(),
        target_record_count,
        source_reclaimable_active_codes,
        preserved_active_stream_codes,
    })
}

pub(super) fn bind_ending_aggregate_label_source(rom: &Rom) -> Result<EndingAggregateLabelSource> {
    bind_ending_chapter_record_translation_surface(rom)?;
    let stream_file_offset = source_file_offset(0x04, ENDING_SCROLL_STREAM_ADDRESS)?;
    let stream_end_file_offset =
        source_file_offset(0x04, ENDING_SCROLL_STREAM_END_EXCLUSIVE_ADDRESS)?;
    let stream = &rom.data()[stream_file_offset..stream_end_file_offset];
    let records = parse_records(stream)?;
    let aggregate = records[ENDING_SCROLL_AGGREGATE_RECORD_INDEX];
    let payload = &stream[aggregate.payload_start..aggregate.payload_end_exclusive];
    let interpolation_offset = payload
        .iter()
        .position(|byte| *byte == ENDING_SCROLL_TURN_INTERPOLATION)
        .context("ending aggregate record has no turn interpolation")?;
    ensure!(
        payload.get(interpolation_offset + 1) == Some(&0x19)
            && !payload[interpolation_offset + 2..].contains(&ENDING_SCROLL_TURN_INTERPOLATION),
        "ending aggregate interpolation changed"
    );
    let mut japanese_markup = decode_source_markup(&payload[..interpolation_offset]);
    japanese_markup.push_str("{ED}{19}");
    japanese_markup.push_str(&decode_source_markup(&payload[interpolation_offset + 2..]));
    let active_codes = active_hangul_codes()
        .into_iter()
        .collect::<std::collections::BTreeSet<_>>();
    let source_reclaimable_active_codes =
        ending_scroll_literal_codes(payload, "ending aggregate record")?
            .into_iter()
            .filter(|code| is_japanese_text_code(*code) && active_codes.contains(code))
            .collect();
    Ok(EndingAggregateLabelSource {
        japanese_markup,
        max_visible_cells: payload.len() - 2,
        source_reclaimable_active_codes,
    })
}

pub(super) fn bind_ending_chapter_record_translation_surface(
    rom: &Rom,
) -> Result<EndingChapterRecordTranslationSurface> {
    let stream_file_offset = source_file_offset(0x04, ENDING_SCROLL_STREAM_ADDRESS)?;
    let stream_end_file_offset =
        source_file_offset(0x04, ENDING_SCROLL_STREAM_END_EXCLUSIVE_ADDRESS)?;
    let stream = rom
        .data()
        .get(stream_file_offset..stream_end_file_offset)
        .context("ending scroll stream is outside the ROM")?;
    ensure!(
        stream.last() == Some(&ENDING_SCROLL_TERMINAL),
        "ending scroll stream terminal changed"
    );

    let records = parse_records(stream)?;
    ensure!(
        records[..ENDING_SCROLL_PRESERVED_RECORD_COUNT]
            .iter()
            .all(|record| {
                let payload = &stream[record.payload_start..record.payload_end_exclusive];
                payload.iter().all(|code| {
                    classify_source_literal_code(*code) != SourceLiteralCodeClass::Japanese
                })
            }),
        "preserved ending opening-and-cast records gained Japanese literals"
    );

    let chapter_title_pointer_file_offset =
        source_file_offset(0x0F, CHAPTER_TITLE_POINTER_TABLE_ADDRESS)?;
    let chapter_title_pointers = rom.data()[chapter_title_pointer_file_offset
        ..chapter_title_pointer_file_offset + CHAPTER_TITLE_POINTER_TABLE_BYTES.len()]
        .chunks_exact(2)
        .map(|bytes| u16::from_le_bytes([bytes[0], bytes[1]]))
        .collect::<Vec<_>>();

    let mut protected_original_chapter_digit_byte_count = 0;
    let mut chapter_title_semantic_match_count = 0;
    let mut chapter_record_literal_codes = Vec::new();
    for (chapter_index, chapter_title_pointer) in chapter_title_pointers.iter().enumerate() {
        let record_index = ENDING_SCROLL_PRESERVED_RECORD_COUNT + chapter_index * 2;
        let record = records[record_index];
        ensure!(
            record.header == 0x00,
            "ending chapter-record header changed"
        );
        let payload = &stream[record.payload_start..record.payload_end_exclusive];
        let interpolation_offset = payload
            .iter()
            .position(|byte| *byte == ENDING_SCROLL_TURN_INTERPOLATION)
            .with_context(|| {
                format!("ending chapter record {chapter_index} has no turn interpolation")
            })?;
        ensure!(
            payload.get(interpolation_offset + 1) == Some(&(chapter_index as u8))
                && !payload[interpolation_offset + 2..].contains(&ENDING_SCROLL_TURN_INTERPOLATION),
            "ending chapter record {chapter_index} turn slot changed"
        );
        protected_original_chapter_digit_byte_count += payload[..interpolation_offset]
            .iter()
            .filter(|byte| (0x60..=0x69).contains(*byte))
            .count();
        chapter_record_literal_codes.extend(ending_scroll_literal_codes(
            payload,
            &format!("ending chapter record {chapter_index}"),
        )?);

        let chapter_title_file_offset = source_file_offset(0x0F, *chapter_title_pointer)?;
        let chapter_title_end = rom.data()
            [chapter_title_file_offset..CHAPTER_TITLE_DATA_END_EXCLUSIVE]
            .iter()
            .position(|byte| *byte == CHAPTER_TITLE_TERMINATOR)
            .with_context(|| format!("chapter title {chapter_index} has no terminator"))?;
        let chapter_title =
            &rom.data()[chapter_title_file_offset..chapter_title_file_offset + chapter_title_end];
        ensure!(
            semantic_title_bytes(&payload[..interpolation_offset])
                == semantic_title_bytes(chapter_title),
            "ending chapter record {chapter_index} diverges from its chapter-title semantic bytes"
        );
        chapter_title_semantic_match_count += 1;

        let spacer = records[record_index + 1];
        ensure!(
            stream[spacer.payload_start..spacer.payload_end_exclusive] == [0xFF],
            "ending chapter-record spacer {chapter_index} changed"
        );
    }
    ensure!(
        protected_original_chapter_digit_byte_count == CHAPTER_TITLE_DIGIT_COUNT,
        "ending chapter-record protected digit count changed"
    );

    let aggregate_record = records[ENDING_SCROLL_AGGREGATE_RECORD_INDEX];
    let aggregate_payload =
        &stream[aggregate_record.payload_start..aggregate_record.payload_end_exclusive];
    let aggregate_interpolation_offset = aggregate_payload
        .iter()
        .position(|byte| *byte == ENDING_SCROLL_TURN_INTERPOLATION)
        .context("ending aggregate record has no turn interpolation")?;
    ensure!(
        aggregate_payload.get(aggregate_interpolation_offset + 1) == Some(&0x19),
        "ending aggregate turn slot changed"
    );
    chapter_record_literal_codes.extend(ending_scroll_literal_codes(
        aggregate_payload,
        "ending aggregate record",
    )?);
    let literal_inventory = classify_translation_surface_literal_codes(
        chapter_record_literal_codes,
        "ending chapter-record surface",
    )?;

    let trailing_blank_records = &records[ENDING_SCROLL_AGGREGATE_RECORD_INDEX + 1..];
    ensure!(
        trailing_blank_records.len() == ENDING_SCROLL_TRAILING_BLANK_RECORD_COUNT
            && trailing_blank_records.iter().all(|record| {
                stream[record.payload_start..record.payload_end_exclusive] == [0xFF]
            }),
        "ending scroll trailing blank lifetime changed"
    );

    Ok(EndingChapterRecordTranslationSurface {
        screen_role: "ending_chapter_record_scroll",
        ending_phase_address: 0x7731,
        ending_phase_address_hex: "0x7731",
        ending_phase: 0x01,
        ending_phase_hex: "0x01",
        inner_state_address: 0x7733,
        inner_state_address_hex: "0x7733",
        stream: SourceRange {
            prg_bank: 0x04,
            prg_bank_hex: "0x04".to_owned(),
            cpu_address: ENDING_SCROLL_STREAM_ADDRESS,
            cpu_address_hex: format!("0x{ENDING_SCROLL_STREAM_ADDRESS:04X}"),
            end_exclusive_cpu_address: ENDING_SCROLL_STREAM_END_EXCLUSIVE_ADDRESS,
            end_exclusive_cpu_address_hex: format!(
                "0x{ENDING_SCROLL_STREAM_END_EXCLUSIVE_ADDRESS:04X}"
            ),
            file_offset: stream_file_offset,
            file_offset_hex: format!("0x{stream_file_offset:05X}"),
            byte_count: stream.len(),
        },
        stream_sha1: sha1_hex(stream),
        record_count: records.len(),
        preserved_original_record_count: ENDING_SCROLL_PRESERVED_RECORD_COUNT,
        chapter_record_count: ENDING_SCROLL_CHAPTER_RECORD_COUNT,
        aggregate_record_count: 1,
        chapter_title_semantic_match_count,
        protected_original_chapter_digit_byte_count,
        japanese_literal_byte_count: literal_inventory.japanese_literal_storage_byte_count,
        literal_inventory,
        turn_interpolation_control: ENDING_SCROLL_TURN_INTERPOLATION,
        turn_interpolation_control_hex: "0xED",
        chapter_turn_slot_range: "0x76D2..0x76EA selected by inline indices 0x00..0x18",
        aggregate_turn_slot: 0x19,
        aggregate_turn_slot_hex: "0x19",
        trailing_blank_record_count: trailing_blank_records.len(),
        semantic_source_policy: "the twenty-five ending rows duplicate the semantic Japanese bytes of the chapter-name table with different layout and turn interpolation; one approved chapter-title translation must drive both physical encodings",
        translation_handling: "translate Japanese chapter titles and the aggregate/turn labels only; preserve the forty-one chapter-number digits and every runtime-generated turn digit",
    })
}

fn parse_records(stream: &[u8]) -> Result<Vec<EndingScrollRecord>> {
    ensure!(
        stream.last() == Some(&ENDING_SCROLL_TERMINAL),
        "ending scroll stream terminal changed"
    );
    let mut records = Vec::new();
    let mut cursor = 0;
    while cursor < stream.len() {
        let header = stream[cursor];
        if header == ENDING_SCROLL_TERMINAL {
            ensure!(
                cursor + 1 == stream.len(),
                "ending scroll has bytes after its terminal"
            );
            break;
        }
        ensure!(
            header != 0xEC,
            "ending scroll reached an unexpected EC terminal"
        );
        let payload_start = cursor + 1;
        let relative_end = stream[payload_start..]
            .iter()
            .position(|byte| *byte == ENDING_SCROLL_RECORD_END)
            .context("ending scroll record has no EF terminator")?;
        let payload_end_exclusive = payload_start + relative_end;
        records.push(EndingScrollRecord {
            header,
            payload_start,
            payload_end_exclusive,
        });
        cursor = payload_end_exclusive + 1;
    }
    ensure!(records.len() == 113, "ending scroll record count changed");
    Ok(records)
}

fn ending_scroll_literal_codes(payload: &[u8], record_role: &str) -> Result<Vec<u8>> {
    let mut codes = Vec::new();
    let mut cursor = 0;
    while cursor < payload.len() {
        if payload[cursor] == ENDING_SCROLL_TURN_INTERPOLATION {
            ensure!(
                cursor + 1 < payload.len(),
                "{record_role} has a turn interpolation without an inline slot"
            );
            cursor += 2;
        } else {
            codes.push(payload[cursor]);
            cursor += 1;
        }
    }
    Ok(codes)
}

fn semantic_title_bytes(bytes: &[u8]) -> Vec<u8> {
    bytes
        .iter()
        .copied()
        .filter(|byte| *byte != 0xFF && !(0x60..=0x69).contains(byte))
        .collect()
}

#[cfg(test)]
mod tests {
    use std::path::Path;

    use super::*;

    #[test]
    fn ending_literal_scan_excludes_turn_control_and_inline_slot() {
        assert_eq!(
            ending_scroll_literal_codes(&[0x01, 0xED, 0x19, 0x60, 0xFF], "test").unwrap(),
            [0x01, 0x60, 0xFF]
        );
    }

    #[test]
    fn ending_aggregate_label_source_is_semantically_bound() {
        let source = Path::new(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../../roms/Fire Emblem - Ankoku Ryuu to Hikari no Tsurugi (Japan).nes"
        ));
        if !source.exists() {
            return;
        }
        let rom = Rom::from_path(source).unwrap();
        let label = bind_ending_aggregate_label_source(&rom).unwrap();
        assert_eq!(label.japanese_markup, "せ゛んターンすう{ED}{19}");
        assert!(!label.source_reclaimable_active_codes.is_empty());
    }

    #[test]
    fn ending_chapter_record_stream_has_no_unreserved_preserved_literals() {
        let source = Path::new(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../../roms/Fire Emblem - Ankoku Ryuu to Hikari no Tsurugi (Japan).nes"
        ));
        if !source.exists() {
            return;
        }
        let rom = Rom::from_path(source).unwrap();
        let lifetime = bind_ending_chapter_record_lifetime_source(&rom).unwrap();

        assert_eq!(lifetime.record_count, 113);
        assert_eq!(lifetime.target_record_count, 26);
        assert_eq!(lifetime.source_reclaimable_active_codes.len(), 73);
        assert!(lifetime.preserved_active_stream_codes.is_empty());
    }
}
