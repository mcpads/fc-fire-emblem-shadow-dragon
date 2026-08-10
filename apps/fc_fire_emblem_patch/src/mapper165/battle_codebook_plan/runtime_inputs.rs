use anyhow::{Context, Result, ensure};
use serde::Serialize;

use crate::{
    rom::{HEADER_SIZE, Rom},
    sha1_hex,
    typed_source::decode_rp2a03_sequence,
};

const PRG_BANK_SIZE: usize = 16 * 1024;
const SWITCHABLE_CPU_START: u16 = 0x8000;
const SWITCHABLE_CPU_END_EXCLUSIVE: u16 = 0xC000;
const BATTLE_RECORD_BYTE_COUNT: usize = 0x1B;

#[derive(Debug, Serialize)]
pub(super) struct BattleRuntimeInputBinding {
    first_battle_record_address: u16,
    second_battle_record_address: u16,
    battle_record_byte_count: usize,
    identity_offset: u8,
    class_offset: u8,
    equipped_item_offset: u8,
    first_terrain_address: u16,
    second_terrain_address: u16,
    live_record_pointer: &'static str,
    live_record_copy: SourceRoutineBinding,
    first_record_to_second_slot_copy: SourceRoutineBinding,
    second_slot_to_first_record_copy: SourceRoutineBinding,
    equipped_item_mutator: SourceRoutineBinding,
    sound_test_record_builder: SourceRoutineBinding,
    sound_test_builder_is_gameplay_catalog: bool,
    dialogue_selector_62: DialogueSelectorBinding,
    static_chapter_table_catalog_sufficient: bool,
    actual_combination_graph_bound: bool,
    binding_conclusion: &'static str,
}

#[derive(Debug, Serialize)]
struct SourceRoutineBinding {
    role: &'static str,
    prg_bank: u8,
    cpu_address: u16,
    byte_count: usize,
    source_sha1: String,
    typed_instruction_count: usize,
}

#[derive(Debug, Serialize)]
struct DialogueSelectorBinding {
    selector_address: u16,
    selector_value: u8,
    source_routine: SourceRoutineBinding,
    required_nonzero_addresses: [u16; 3],
    required_zero_addresses: [u16; 1],
    dynamic_record_index_address: u16,
    dynamic_record_index_transform: &'static str,
    terminator_address: u16,
    terminator_value: u8,
    natural_runtime_observed: bool,
}

#[derive(Clone, Copy)]
struct RoutineSpec {
    role: &'static str,
    prg_bank: u8,
    cpu_address: u16,
    byte_count: usize,
    expected_sha1: &'static str,
}

const LIVE_RECORD_COPY: RoutineSpec = RoutineSpec {
    role: "copy_live_unit_record_to_first_battle_slot",
    prg_bank: 0x03,
    cpu_address: 0x9E5D,
    byte_count: 0x0D,
    expected_sha1: "506002609469d041a105de0a4ceb832695219fed",
};
const FIRST_TO_SECOND_COPY: RoutineSpec = RoutineSpec {
    role: "copy_first_battle_slot_to_second_slot",
    prg_bank: 0x03,
    cpu_address: 0x9E84,
    byte_count: 0x0E,
    expected_sha1: "6ee5d2bb0206210a4327ec0037c64371ad56d8f7",
};
const SECOND_TO_FIRST_COPY: RoutineSpec = RoutineSpec {
    role: "restore_second_battle_slot_to_first_slot",
    prg_bank: 0x03,
    cpu_address: 0x9E92,
    byte_count: 0x0E,
    expected_sha1: "bf20608ed771c44cacc37e57aa7bc72b81e50e4b",
};
const SOUND_TEST_RECORD_BUILDER: RoutineSpec = RoutineSpec {
    role: "build_sound_test_battle_record_from_compact_source",
    prg_bank: 0x03,
    cpu_address: 0x9271,
    byte_count: 0xC9,
    expected_sha1: "94baf5701a7ca9862a137ee873c96047be0dad9f",
};
const EQUIPPED_ITEM_MUTATOR: RoutineSpec = RoutineSpec {
    role: "swap_selected_item_to_equipped_slot",
    prg_bank: 0x06,
    cpu_address: 0xA5CE,
    byte_count: 0x29,
    expected_sha1: "de1f6ff348cb45a0ad091d4ca52a54502c278d0e",
};
const DIALOGUE_SELECTOR_ROUTINE: RoutineSpec = RoutineSpec {
    role: "select_and_compose_gameplay_battle_dialogue",
    prg_bank: 0x05,
    cpu_address: 0x85DE,
    byte_count: 0xCF,
    expected_sha1: "f81f49a58e82048d10a073e65a55e065ee38989e",
};

const LIVE_RECORD_COPY_BYTES: [u8; 13] = [
    0xA0, 0x00, 0xB1, 0x9F, 0x99, 0xF4, 0x76, 0xC8, 0xC0, 0x1B, 0x90, 0xF6, 0x60,
];
const FIRST_TO_SECOND_COPY_BYTES: [u8; 14] = [
    0xA0, 0x00, 0xB9, 0xF4, 0x76, 0x99, 0x15, 0x77, 0xC8, 0xC0, 0x1B, 0x90, 0xF5, 0x60,
];
const SECOND_TO_FIRST_COPY_BYTES: [u8; 14] = [
    0xA0, 0x00, 0xB9, 0x15, 0x77, 0x99, 0xF4, 0x76, 0xC8, 0xC0, 0x1B, 0x90, 0xF5, 0x60,
];
const SELECTOR_62_FRAGMENT: [u8; 22] = [
    0xAD, 0x35, 0x03, 0xF0, 0x32, 0xAD, 0x79, 0x04, 0x09, 0x60, 0x8D, 0x4B, 0x7A, 0xA9, 0xEF, 0x8D,
    0x4C, 0x7A, 0xA9, 0x3E, 0xD0, 0x21,
];

pub(super) fn bind_battle_runtime_inputs(rom: &Rom) -> Result<BattleRuntimeInputBinding> {
    let live_record_copy = bind_routine(rom, LIVE_RECORD_COPY)?;
    let first_record_to_second_slot_copy = bind_routine(rom, FIRST_TO_SECOND_COPY)?;
    let second_slot_to_first_record_copy = bind_routine(rom, SECOND_TO_FIRST_COPY)?;
    let equipped_item_mutator = bind_routine(rom, EQUIPPED_ITEM_MUTATOR)?;
    let sound_test_record_builder = bind_routine(rom, SOUND_TEST_RECORD_BUILDER)?;
    let selector_source = source_slice(rom, DIALOGUE_SELECTOR_ROUTINE)?;
    ensure!(
        selector_source
            .windows(SELECTOR_62_FRAGMENT.len())
            .any(|window| window == SELECTOR_62_FRAGMENT),
        "battle-dialogue selector 62 source path changed"
    );
    let source_routine = bind_routine(rom, DIALOGUE_SELECTOR_ROUTINE)?;

    ensure!(
        source_slice(rom, LIVE_RECORD_COPY)? == LIVE_RECORD_COPY_BYTES,
        "live battle-record copy changed"
    );
    ensure!(
        source_slice(rom, FIRST_TO_SECOND_COPY)? == FIRST_TO_SECOND_COPY_BYTES,
        "first-to-second battle-record copy changed"
    );
    ensure!(
        source_slice(rom, SECOND_TO_FIRST_COPY)? == SECOND_TO_FIRST_COPY_BYTES,
        "second-to-first battle-record copy changed"
    );

    Ok(BattleRuntimeInputBinding {
        first_battle_record_address: 0x76F4,
        second_battle_record_address: 0x7715,
        battle_record_byte_count: BATTLE_RECORD_BYTE_COUNT,
        identity_offset: 0x00,
        class_offset: 0x01,
        equipped_item_offset: 0x13,
        first_terrain_address: 0x0322,
        second_terrain_address: 0x0323,
        live_record_pointer: "zero-page 0x9F/0xA0",
        live_record_copy,
        first_record_to_second_slot_copy,
        second_slot_to_first_record_copy,
        equipped_item_mutator,
        sound_test_record_builder,
        sound_test_builder_is_gameplay_catalog: false,
        dialogue_selector_62: DialogueSelectorBinding {
            selector_address: 0x7936,
            selector_value: 0x3E,
            source_routine,
            required_nonzero_addresses: [0x0334, 0x0479, 0x0335],
            required_zero_addresses: [0x05DF],
            dynamic_record_index_address: 0x7A4B,
            dynamic_record_index_transform: "0x0479 OR 0x60",
            terminator_address: 0x7A4C,
            terminator_value: 0xEF,
            natural_runtime_observed: false,
        },
        static_chapter_table_catalog_sufficient: false,
        actual_combination_graph_bound: false,
        binding_conclusion: "gameplay battle inputs are copied from mutable live unit records, while selector 62 also depends on live control state; use these runtime indices to select cache-owned encoded text instead of treating the sound-test builder or static chapter tables as a complete catalog",
    })
}

fn bind_routine(rom: &Rom, spec: RoutineSpec) -> Result<SourceRoutineBinding> {
    let bytes = source_slice(rom, spec)?;
    let actual_sha1 = sha1_hex(bytes);
    ensure!(
        actual_sha1 == spec.expected_sha1,
        "{} source changed: expected {}, found {}",
        spec.role,
        spec.expected_sha1,
        actual_sha1
    );
    let typed = decode_rp2a03_sequence(bytes, spec.cpu_address, spec.role)?;
    Ok(SourceRoutineBinding {
        role: spec.role,
        prg_bank: spec.prg_bank,
        cpu_address: spec.cpu_address,
        byte_count: spec.byte_count,
        source_sha1: actual_sha1,
        typed_instruction_count: typed.len(),
    })
}

fn source_slice(rom: &Rom, spec: RoutineSpec) -> Result<&[u8]> {
    ensure!(
        spec.prg_bank < 0x0F,
        "battle runtime input binding requires a switchable PRG bank"
    );
    ensure!(
        (SWITCHABLE_CPU_START..SWITCHABLE_CPU_END_EXCLUSIVE).contains(&spec.cpu_address),
        "battle runtime input source is outside the switchable CPU window"
    );
    let file_offset = HEADER_SIZE
        + usize::from(spec.prg_bank) * PRG_BANK_SIZE
        + usize::from(spec.cpu_address - SWITCHABLE_CPU_START);
    let end = file_offset
        .checked_add(spec.byte_count)
        .context("battle runtime input source range overflow")?;
    rom.data()
        .get(file_offset..end)
        .with_context(|| format!("{} source is outside the ROM", spec.role))
}

#[cfg(test)]
pub(super) fn test_binding() -> BattleRuntimeInputBinding {
    fn routine(role: &'static str) -> SourceRoutineBinding {
        SourceRoutineBinding {
            role,
            prg_bank: 3,
            cpu_address: 0x8000,
            byte_count: 1,
            source_sha1: "source".to_owned(),
            typed_instruction_count: 1,
        }
    }

    BattleRuntimeInputBinding {
        first_battle_record_address: 0x76F4,
        second_battle_record_address: 0x7715,
        battle_record_byte_count: BATTLE_RECORD_BYTE_COUNT,
        identity_offset: 0,
        class_offset: 1,
        equipped_item_offset: 0x13,
        first_terrain_address: 0x0322,
        second_terrain_address: 0x0323,
        live_record_pointer: "zero-page",
        live_record_copy: routine("live copy"),
        first_record_to_second_slot_copy: routine("first to second"),
        second_slot_to_first_record_copy: routine("second to first"),
        equipped_item_mutator: routine("equip selected item"),
        sound_test_record_builder: routine("sound test"),
        sound_test_builder_is_gameplay_catalog: false,
        dialogue_selector_62: DialogueSelectorBinding {
            selector_address: 0x7936,
            selector_value: 0x3E,
            source_routine: routine("selector"),
            required_nonzero_addresses: [0x0334, 0x0479, 0x0335],
            required_zero_addresses: [0x05DF],
            dynamic_record_index_address: 0x7A4B,
            dynamic_record_index_transform: "OR",
            terminator_address: 0x7A4C,
            terminator_value: 0xEF,
            natural_runtime_observed: false,
        },
        static_chapter_table_catalog_sufficient: false,
        actual_combination_graph_bound: false,
        binding_conclusion: "runtime",
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn runtime_binding_keeps_sound_test_and_gameplay_sources_distinct() {
        let binding = test_binding();

        assert!(!binding.sound_test_builder_is_gameplay_catalog);
        assert!(!binding.static_chapter_table_catalog_sufficient);
        assert_eq!(binding.battle_record_byte_count, 0x1B);
        assert_eq!(binding.dialogue_selector_62.selector_value, 0x3E);
        assert!(!binding.dialogue_selector_62.natural_runtime_observed);
    }
}
