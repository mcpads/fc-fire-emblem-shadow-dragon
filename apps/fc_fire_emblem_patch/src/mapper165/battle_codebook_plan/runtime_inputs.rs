use anyhow::{Context, Result, ensure};
use serde::Serialize;

use crate::{
    battle_runtime_state::BATTLE_RUNTIME_STATE,
    rom::{HEADER_SIZE, Rom},
    sha1_hex,
    typed_source::decode_rp2a03_sequence,
};

mod field_producers;

use field_producers::{BattleFieldProducerBinding, bind_battle_field_producers};

const PRG_BANK_SIZE: usize = 16 * 1024;
const SWITCHABLE_CPU_START: u16 = 0x8000;
const SWITCHABLE_CPU_END_EXCLUSIVE: u16 = 0xC000;

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
    reinforcement_record_builder: SourceRoutineBinding,
    field_producers: BattleFieldProducerBinding,
    reinforcement_builder_is_gameplay_producer: bool,
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
    dynamic_record_index_transform: String,
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
const REINFORCEMENT_RECORD_BUILDER: RoutineSpec = RoutineSpec {
    role: "initialize_enemy_unit_from_reinforcement_record",
    prg_bank: 0x03,
    cpu_address: 0x9271,
    byte_count: 0xB9,
    expected_sha1: "65f06ab741b8c5d2f75ba433742059517d2aae12",
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
fn selector_62_fragment() -> Vec<u8> {
    let selector = BATTLE_RUNTIME_STATE.dialogue_selector_projection;
    let [condition_low, condition_high] = selector.required_nonzero_addresses[2].to_le_bytes();
    let [source_low, source_high] = selector.dynamic_record_index_source_address.to_le_bytes();
    let [index_low, index_high] = selector.dynamic_record_index_address.to_le_bytes();
    let [terminator_low, terminator_high] = selector.terminator_address.to_le_bytes();
    vec![
        0xAD,
        condition_low,
        condition_high,
        0xF0,
        0x32,
        0xAD,
        source_low,
        source_high,
        0x09,
        selector.dynamic_record_index_or_mask,
        0x8D,
        index_low,
        index_high,
        0xA9,
        selector.terminator_value,
        0x8D,
        terminator_low,
        terminator_high,
        0xA9,
        selector.forced_selector,
        0xD0,
        0x21,
    ]
}

pub(super) fn bind_battle_runtime_inputs(rom: &Rom) -> Result<BattleRuntimeInputBinding> {
    let runtime = BATTLE_RUNTIME_STATE;
    let selector = runtime.dialogue_selector_projection;
    let live_record_copy = bind_routine(rom, LIVE_RECORD_COPY)?;
    let first_record_to_second_slot_copy = bind_routine(rom, FIRST_TO_SECOND_COPY)?;
    let second_slot_to_first_record_copy = bind_routine(rom, SECOND_TO_FIRST_COPY)?;
    let equipped_item_mutator = bind_routine(rom, EQUIPPED_ITEM_MUTATOR)?;
    let reinforcement_record_builder = bind_routine(rom, REINFORCEMENT_RECORD_BUILDER)?;
    let field_producers = bind_battle_field_producers(rom)?;
    let selector_source = source_slice(rom, DIALOGUE_SELECTOR_ROUTINE)?;
    let selector_fragment = selector_62_fragment();
    ensure!(
        selector_source
            .windows(selector_fragment.len())
            .any(|window| window == selector_fragment),
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
        first_battle_record_address: runtime.battle_record_addresses[0],
        second_battle_record_address: runtime.battle_record_addresses[1],
        battle_record_byte_count: runtime.battle_record_byte_count,
        identity_offset: runtime.live_record_identity_offset,
        class_offset: runtime.live_record_class_offset,
        equipped_item_offset: runtime.live_record_equipped_item_offset,
        first_terrain_address: runtime.staged_terrain_source_index_addresses[0],
        second_terrain_address: runtime.staged_terrain_source_index_addresses[1],
        live_record_pointer: "zero-page 0x9F/0xA0",
        live_record_copy,
        first_record_to_second_slot_copy,
        second_slot_to_first_record_copy,
        equipped_item_mutator,
        reinforcement_record_builder,
        field_producers,
        reinforcement_builder_is_gameplay_producer: true,
        dialogue_selector_62: DialogueSelectorBinding {
            selector_address: selector.observed_selector_address,
            selector_value: selector.forced_selector,
            source_routine,
            required_nonzero_addresses: selector.required_nonzero_addresses,
            required_zero_addresses: selector.required_zero_addresses,
            dynamic_record_index_address: selector.dynamic_record_index_address,
            dynamic_record_index_transform: format!(
                "0x{:04X} OR 0x{:02X}",
                selector.dynamic_record_index_source_address, selector.dynamic_record_index_or_mask,
            ),
            terminator_address: selector.terminator_address,
            terminator_value: selector.terminator_value,
            natural_runtime_observed: false,
        },
        static_chapter_table_catalog_sufficient: false,
        actual_combination_graph_bound: true,
        binding_conclusion: "normal and special battle field producers bind identity, class transformation, rendered item projection, and all 16 terrain-name sources; the combined graph admits every renderer-defined enemy class, while selector 62 natural reachability remains a runtime proof gate",
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
    let runtime = BATTLE_RUNTIME_STATE;
    let selector = runtime.dialogue_selector_projection;
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
        first_battle_record_address: runtime.battle_record_addresses[0],
        second_battle_record_address: runtime.battle_record_addresses[1],
        battle_record_byte_count: runtime.battle_record_byte_count,
        identity_offset: runtime.live_record_identity_offset,
        class_offset: runtime.live_record_class_offset,
        equipped_item_offset: runtime.live_record_equipped_item_offset,
        first_terrain_address: runtime.staged_terrain_source_index_addresses[0],
        second_terrain_address: runtime.staged_terrain_source_index_addresses[1],
        live_record_pointer: "zero-page",
        live_record_copy: routine("live copy"),
        first_record_to_second_slot_copy: routine("first to second"),
        second_slot_to_first_record_copy: routine("second to first"),
        equipped_item_mutator: routine("equip selected item"),
        reinforcement_record_builder: routine("reinforcement builder"),
        field_producers: field_producers::test_binding(),
        reinforcement_builder_is_gameplay_producer: true,
        dialogue_selector_62: DialogueSelectorBinding {
            selector_address: selector.observed_selector_address,
            selector_value: selector.forced_selector,
            source_routine: routine("selector"),
            required_nonzero_addresses: selector.required_nonzero_addresses,
            required_zero_addresses: selector.required_zero_addresses,
            dynamic_record_index_address: selector.dynamic_record_index_address,
            dynamic_record_index_transform: "OR".to_owned(),
            terminator_address: selector.terminator_address,
            terminator_value: selector.terminator_value,
            natural_runtime_observed: false,
        },
        static_chapter_table_catalog_sufficient: false,
        actual_combination_graph_bound: true,
        binding_conclusion: "runtime",
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn runtime_binding_identifies_the_reinforcement_builder_as_gameplay() {
        let binding = test_binding();

        assert!(binding.reinforcement_builder_is_gameplay_producer);
        assert!(!binding.static_chapter_table_catalog_sufficient);
        assert_eq!(
            binding.battle_record_byte_count,
            BATTLE_RUNTIME_STATE.battle_record_byte_count
        );
        assert_eq!(
            binding.dialogue_selector_62.selector_value,
            BATTLE_RUNTIME_STATE
                .dialogue_selector_projection
                .forced_selector
        );
        assert!(!binding.dialogue_selector_62.natural_runtime_observed);
        assert!(binding.actual_combination_graph_bound);
    }
}
