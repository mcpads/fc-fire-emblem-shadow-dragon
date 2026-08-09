use std::collections::BTreeSet;

use anyhow::{Context, Result, ensure};
use serde::Serialize;

use crate::{
    dialogue_inventory::{
        TranslationSurfaceDialogueTableBinding,
        aggregate_translation_surface_dialogue_literal_inventory,
        inspect_translation_surface_dialogue_tables,
    },
    rom::Rom,
    sha1_hex,
    source_literals::{
        SourceLiteralCodeClass, TranslationSurfaceLiteralInventory, classify_source_literal_code,
        classify_translation_surface_literal_codes,
    },
    text_inventory::scoped_text_table_budgets,
};

use super::{
    CHAPTER_TITLE_DATA_END_EXCLUSIVE, CHAPTER_TITLE_DIGIT_COUNT,
    CHAPTER_TITLE_POINTER_TABLE_ADDRESS, CHAPTER_TITLE_POINTER_TABLE_BYTES,
    CHAPTER_TITLE_TERMINATOR, CodeLocation, location, source_file_offset,
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
pub(super) struct TranslationSurfaceContracts {
    battle_animation: BattleAnimationTranslationSurface,
    ending_chapter_record_scroll: EndingChapterRecordTranslationSurface,
    ending_character_epilogue: EndingCharacterEpilogueTranslationSurface,
    dialogue_tables: Vec<TranslationSurfaceDialogueTableBinding>,
    proof_boundary: &'static str,
}

#[derive(Debug, Serialize)]
struct BattleAnimationTranslationSurface {
    screen_role: &'static str,
    sound_test_outer_phase_address: u16,
    sound_test_outer_phase_address_hex: &'static str,
    shared_engine_outer_phase: u8,
    shared_engine_outer_phase_hex: &'static str,
    shared_engine_entry: CodeLocation,
    shared_phase_address: u16,
    shared_phase_address_hex: &'static str,
    shared_phase_count: usize,
    terminal_shared_phase: u8,
    terminal_shared_phase_hex: &'static str,
    repeated_outer_phase: u8,
    repeated_outer_phase_hex: &'static str,
    fixed_text_tables: Vec<BattleTextTableBinding>,
    fixed_text_code_union: SourceCodePartition,
    dialogue_table_id: &'static str,
    dialogue_literal_inventory: TranslationSurfaceLiteralInventory,
    dialogue_selector_address: u16,
    dialogue_selector_address_hex: &'static str,
    dialogue_table_set_address: u16,
    dialogue_table_set_address_hex: &'static str,
    writer_roles: &'static [&'static str],
    translation_handling: &'static str,
    unresolved: &'static [&'static str],
}

#[derive(Debug, Serialize)]
struct BattleTextTableBinding {
    table_id: &'static str,
    table_cpu_address: u16,
    table_cpu_address_hex: &'static str,
    pointer_count: usize,
    unique_string_count: usize,
    referenced_text_byte_count: usize,
    unique_text_storage_byte_count: usize,
    max_entry_byte_count: usize,
    distinct_source_code_count: usize,
    source_code_partition: SourceCodePartition,
    writer_role: &'static str,
    translation_handling: &'static str,
}

#[derive(Debug, Serialize)]
struct SourceCodePartition {
    distinct_source_code_count: usize,
    japanese_codes_hex: Vec<String>,
    preserved_original_codes_hex: Vec<String>,
    layout_codes_hex: Vec<String>,
    unresolved_codes_hex: Vec<String>,
}

#[derive(Debug, Serialize)]
struct EndingChapterRecordTranslationSurface {
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
struct EndingCharacterEpilogueTranslationSurface {
    screen_role: &'static str,
    selector_phase: u8,
    selector_phase_hex: &'static str,
    visible_dialogue_phase: u8,
    visible_dialogue_phase_hex: &'static str,
    table_selector_address: u16,
    table_selector_address_hex: &'static str,
    entry_selector_address: u16,
    entry_selector_address_hex: &'static str,
    direct_dialogue_table_id: &'static str,
    routing_dialogue_table_id: &'static str,
    direct_selector: u8,
    direct_selector_hex: &'static str,
    routing_selector: u8,
    routing_selector_hex: &'static str,
    dialogue_literal_inventory: TranslationSurfaceLiteralInventory,
    dialogue_literal_inventory_scope: &'static str,
    selector_writer: CodeLocation,
    dialogue_wait_handler: CodeLocation,
    input_behavior: &'static str,
    translation_handling: &'static str,
    unresolved: &'static [&'static str],
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

pub(super) fn bind_translation_surfaces(rom: &Rom) -> Result<TranslationSurfaceContracts> {
    let dialogue_tables = inspect_translation_surface_dialogue_tables(rom.data())?;
    ensure!(
        dialogue_tables.len() == 3,
        "translation-surface dialogue table count changed"
    );
    let battle_dialogue = dialogue_tables
        .iter()
        .find(|table| table.table_id == "battle-dialogue")
        .context("battle-dialogue surface binding is absent")?;
    ensure!(
        battle_dialogue.pointer_count == 65
            && battle_dialogue.unique_target_count == 28
            && battle_dialogue.separate_loader_cpu_address == Some(0x8000)
            && battle_dialogue.proven_record_count == Some(28)
            && battle_dialogue.unique_record_storage_byte_count == Some(1152)
            && battle_dialogue.unreferenced_record_count == Some(1),
        "battle-dialogue surface structure changed"
    );
    let direct_epilogue = dialogue_tables
        .iter()
        .find(|table| table.table_id == "epilogue-dialogue")
        .context("epilogue-dialogue surface binding is absent")?;
    ensure!(
        direct_epilogue.directory_selector == Some(0x40)
            && direct_epilogue.pointer_count == 66
            && direct_epilogue.proven_record_count == Some(66),
        "direct epilogue-dialogue surface structure changed"
    );
    let routing_epilogue = dialogue_tables
        .iter()
        .find(|table| table.table_id == "epilogue-routing-dialogue")
        .context("epilogue-routing surface binding is absent")?;
    ensure!(
        routing_epilogue.directory_selector == Some(0x41)
            && routing_epilogue.pointer_count == 54
            && routing_epilogue.proven_record_count == Some(52),
        "routing epilogue-dialogue surface structure changed"
    );

    let ending_dialogue_literal_inventory =
        aggregate_translation_surface_dialogue_literal_inventory(
            rom.data(),
            &dialogue_tables,
            &["epilogue-dialogue", "epilogue-routing-dialogue"],
        )?;

    Ok(TranslationSurfaceContracts {
        battle_animation: bind_battle_animation_translation_surface(rom, &dialogue_tables)?,
        ending_chapter_record_scroll: bind_ending_chapter_record_translation_surface(rom)?,
        ending_character_epilogue: EndingCharacterEpilogueTranslationSurface {
            screen_role: "ending_character_epilogue",
            selector_phase: 0x0F,
            selector_phase_hex: "0x0F",
            visible_dialogue_phase: 0x10,
            visible_dialogue_phase_hex: "0x10",
            table_selector_address: 0x77F4,
            table_selector_address_hex: "0x77F4",
            entry_selector_address: 0x77F1,
            entry_selector_address_hex: "0x77F1",
            direct_dialogue_table_id: "epilogue-dialogue",
            routing_dialogue_table_id: "epilogue-routing-dialogue",
            direct_selector: 0x40,
            direct_selector_hex: "0x40",
            routing_selector: 0x41,
            routing_selector_hex: "0x41",
            dialogue_literal_inventory: ending_dialogue_literal_inventory,
            dialogue_literal_inventory_scope: "all canonical first linear segments in selector tables 0x40 and 0x41; every routing-table transition targets the included direct epilogue table",
            selector_writer: location(0x04, 0xA17E),
            dialogue_wait_handler: location(0x04, 0xA233),
            input_behavior: "automatic; phase 0x0F selects one of the two structurally bounded dialogue tables and phase 0x10 waits for the shared dialogue engine before advancing",
            translation_handling: "translate Japanese character names and epilogue lines only; preserve original Latin and digit codes",
            unresolved: &[
                "complete portrait and CHR-page union across all character entries",
                "runtime coverage of every direct and routing epilogue entry",
            ],
        },
        dialogue_tables,
        proof_boundary: "the supported Japanese ROM binds the common battle engine to four fixed text tables and the separate battle-dialogue loader, binds the ending record stream and dynamic turn interpolation, and binds the automatic character epilogue to selectors 0x40 and 0x41; only code sets and structural counts are emitted",
    })
}

fn bind_battle_animation_translation_surface(
    rom: &Rom,
    dialogue_tables: &[TranslationSurfaceDialogueTableBinding],
) -> Result<BattleAnimationTranslationSurface> {
    const TABLE_IDS: [&str; 4] = ["unit-names", "enemy-names", "class-names", "item-names"];
    let budgets = scoped_text_table_budgets(rom.data(), &TABLE_IDS)?;
    let mut fixed_text_code_union = BTreeSet::new();
    let fixed_text_tables = budgets
        .into_iter()
        .map(|budget| {
            let (table_cpu_address, table_cpu_address_hex, writer_role) = match budget.id {
                "unit-names" => (0xDE2B, "0xDE2B", "compose_battle_unit_name"),
                "enemy-names" => (0xDFA4, "0xDFA4", "compose_battle_unit_name"),
                "class-names" => (0xDA1F, "0xDA1F", "compose_battle_class_name"),
                "item-names" => (0xDAD5, "0xDAD5", "compose_battle_item_name"),
                other => return Err(anyhow::anyhow!("unexpected battle text table {other}")),
            };
            fixed_text_code_union.extend(budget.source_codes.iter().copied());
            let source_code_partition =
                partition_source_codes(budget.source_codes.iter().copied());
            Ok(BattleTextTableBinding {
                table_id: budget.id,
                table_cpu_address,
                table_cpu_address_hex,
                pointer_count: budget.pointer_count,
                unique_string_count: budget.unique_string_count,
                referenced_text_byte_count: budget.referenced_text_byte_count,
                unique_text_storage_byte_count: budget.unique_text_storage_byte_count,
                max_entry_byte_count: budget.max_entry_byte_count,
                distinct_source_code_count: budget.source_codes.len(),
                source_code_partition,
                writer_role,
                translation_handling: "translate Japanese glyph bytes only; preserve original Latin and digit positions",
            })
        })
        .collect::<Result<Vec<_>>>()?;
    ensure!(
        fixed_text_tables.len() == TABLE_IDS.len(),
        "battle fixed-text table coverage changed"
    );
    let dialogue_literal_inventory = aggregate_translation_surface_dialogue_literal_inventory(
        rom.data(),
        dialogue_tables,
        &["battle-dialogue"],
    )?;

    Ok(BattleAnimationTranslationSurface {
        screen_role: "battle_animation",
        sound_test_outer_phase_address: 0x7730,
        sound_test_outer_phase_address_hex: "0x7730",
        shared_engine_outer_phase: 0x05,
        shared_engine_outer_phase_hex: "0x05",
        shared_engine_entry: location(0x05, 0x8161),
        shared_phase_address: 0x047C,
        shared_phase_address_hex: "0x047C",
        shared_phase_count: 32,
        terminal_shared_phase: 0x1F,
        terminal_shared_phase_hex: "0x1F",
        repeated_outer_phase: 0x03,
        repeated_outer_phase_hex: "0x03",
        fixed_text_tables,
        fixed_text_code_union: partition_source_codes(fixed_text_code_union),
        dialogue_table_id: "battle-dialogue",
        dialogue_literal_inventory,
        dialogue_selector_address: 0x7936,
        dialogue_selector_address_hex: "0x7936",
        dialogue_table_set_address: 0x7935,
        dialogue_table_set_address_hex: "0x7935",
        writer_roles: &[
            "select_battle_unit_name_source",
            "compose_battle_unit_name",
            "compose_battle_class_name",
            "compose_battle_item_name",
            "compose_battle_item_and_dialogue",
            "override_battle_dialogue_selector",
            "compose_battle_dialogue",
            "compose_battle_dialogue_continuation_one",
            "compose_battle_dialogue_continuation_two",
            "compose_battle_class_and_dialogue",
        ],
        translation_handling: "the debug route reuses the gameplay battle engine and its shared text sources; translate Japanese names, labels, and messages while preserving LV, HIT, EXP, HP bars, percentages, and digits",
        unresolved: &[
            "complete CHR, sprite, and temporal union across ordinary, debug, defeat, and unfavorable battle variants",
        ],
    })
}

#[derive(Clone, Copy)]
struct EndingScrollRecord {
    header: u8,
    payload_start: usize,
    payload_end_exclusive: usize,
}

fn bind_ending_chapter_record_translation_surface(
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

    let mut records = Vec::new();
    let mut cursor = 0_usize;
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

fn partition_source_codes(codes: impl IntoIterator<Item = u8>) -> SourceCodePartition {
    let mut japanese_codes = BTreeSet::new();
    let mut preserved_original_codes = BTreeSet::new();
    let mut layout_codes = BTreeSet::new();
    let mut unresolved_codes = BTreeSet::new();
    for code in codes {
        match classify_source_literal_code(code) {
            SourceLiteralCodeClass::Japanese => {
                japanese_codes.insert(code);
            }
            SourceLiteralCodeClass::PreservedOriginal => {
                preserved_original_codes.insert(code);
            }
            SourceLiteralCodeClass::Layout => {
                layout_codes.insert(code);
            }
            SourceLiteralCodeClass::Unresolved => {
                unresolved_codes.insert(code);
            }
        }
    }
    let distinct_source_code_count = japanese_codes.len()
        + preserved_original_codes.len()
        + layout_codes.len()
        + unresolved_codes.len();

    SourceCodePartition {
        distinct_source_code_count,
        japanese_codes_hex: hex_codes(japanese_codes),
        preserved_original_codes_hex: hex_codes(preserved_original_codes),
        layout_codes_hex: hex_codes(layout_codes),
        unresolved_codes_hex: hex_codes(unresolved_codes),
    }
}

fn hex_codes(codes: BTreeSet<u8>) -> Vec<String> {
    codes
        .into_iter()
        .map(|code| format!("{code:02X}"))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ending_literal_scan_excludes_turn_control_and_inline_slot() {
        assert_eq!(
            ending_scroll_literal_codes(&[0x01, 0xED, 0x19, 0x60, 0xFF], "test").unwrap(),
            [0x01, 0x60, 0xFF]
        );
    }

    #[test]
    fn source_code_partition_keeps_translation_and_preservation_distinct() {
        let partition = partition_source_codes([0x01, 0x60, 0x9B, 0xFF, 0x8C]);

        assert_eq!(partition.distinct_source_code_count, 5);
        assert_eq!(partition.japanese_codes_hex, ["01"]);
        assert_eq!(partition.preserved_original_codes_hex, ["60", "9B"]);
        assert_eq!(partition.layout_codes_hex, ["FF"]);
        assert_eq!(partition.unresolved_codes_hex, ["8C"]);
    }
}
