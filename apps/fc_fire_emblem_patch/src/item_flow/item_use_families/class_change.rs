use std::collections::BTreeSet;

use anyhow::{Result, ensure};
use serde::Serialize;

use crate::{rom::Rom, typed_source::decode_rp2a03_sequence};

use super::super::{CodeLocation, location, source_contract::source_slice};

const PRG_BANK: u8 = 0x06;
const ITEM_ID_BASE: u8 = 0x50;
const ITEM_COUNT: usize = 5;
const MINIMUM_LEVEL: u8 = 10;
const FAILURE_DIALOGUE_INDEX: u8 = 0x30;

const ENTRY_ADDRESS: u16 = 0x97DA;
const ENTRY_CODE_LENGTH: usize = 195;
const PRIMARY_CLASS_TABLE_ADDRESS: u16 = 0x989D;
const ALTERNATE_CLASS_TABLE_ADDRESS: u16 = 0x98A2;
const TARGET_CLASS_TABLE_ADDRESS: u16 = 0x98A7;
const RESULT_DISPATCH_TABLE_ADDRESS: u16 = 0x959B;
const RESULT_DISPATCH_HANDLER_COUNT: usize = 7;
const BATTLE_STATE_HANDLERS_ADDRESS: u16 = 0x9D3C;
const BATTLE_STATE_HANDLERS_LENGTH: usize = 46;
const COMPLETION_ADDRESS: u16 = 0x98AC;
const COMPLETION_LENGTH: usize = 15;
const MAP_RESTORE_ADDRESS: u16 = 0xB97F;
const MAP_RESTORE_LENGTH: usize = 45;

const EXPECTED_PRIMARY_CLASSES: [u8; ITEM_COUNT] = [0x01, 0x06, 0x12, 0x0B, 0x03];
const EXPECTED_ALTERNATE_CLASSES: [u8; ITEM_COUNT] = [0x01, 0x06, 0x13, 0x0B, 0x03];
const EXPECTED_TARGET_CLASSES: [u8; ITEM_COUNT] = [0x04, 0x0A, 0x14, 0x0F, 0x05];
const EXPECTED_RESULT_HANDLERS: [u16; RESULT_DISPATCH_HANDLER_COUNT] =
    [0xA122, 0xA13E, 0x95A9, 0xA122, 0x9D3C, 0x9D5E, 0x98AC];

#[derive(Debug, Serialize)]
pub(super) struct ClassChangeContract {
    minimum_level: u8,
    minimum_level_hex: String,
    failure_dialogue_index: u8,
    failure_dialogue_index_hex: String,
    eligibility_routes: Vec<ClassChangeRoute>,
    result_progression_states: Vec<ResultProgressionState>,
    typed_instruction_count: usize,
    entry: CodeLocation,
    battle_state_handlers: CodeLocation,
    completion: CodeLocation,
    map_restore: CodeLocation,
    static_conclusion: &'static str,
    runtime_gate: &'static str,
}

#[derive(Debug, Serialize)]
struct ClassChangeRoute {
    item_id: u8,
    item_id_hex: String,
    eligible_source_class_ids: Vec<u8>,
    eligible_source_class_ids_hex: Vec<String>,
    target_class_id: u8,
    target_class_id_hex: String,
}

#[derive(Debug, Serialize)]
struct ResultProgressionState {
    state: u8,
    state_hex: String,
    role: &'static str,
    handler: CodeLocation,
}

pub(super) fn inspect(rom: &Rom) -> Result<ClassChangeContract> {
    let primary = fixed_array::<ITEM_COUNT>(rom, PRIMARY_CLASS_TABLE_ADDRESS)?;
    let alternate = fixed_array::<ITEM_COUNT>(rom, ALTERNATE_CLASS_TABLE_ADDRESS)?;
    let target = fixed_array::<ITEM_COUNT>(rom, TARGET_CLASS_TABLE_ADDRESS)?;
    ensure!(
        primary == EXPECTED_PRIMARY_CLASSES
            && alternate == EXPECTED_ALTERNATE_CLASSES
            && target == EXPECTED_TARGET_CLASSES,
        "class-change eligibility or target-class table changed"
    );

    let result_handlers = source_slice(
        rom,
        PRG_BANK,
        RESULT_DISPATCH_TABLE_ADDRESS,
        RESULT_DISPATCH_HANDLER_COUNT * 2,
    )?
    .chunks_exact(2)
    .map(|bytes| u16::from_le_bytes([bytes[0], bytes[1]]))
    .collect::<Vec<_>>();
    ensure!(
        result_handlers == EXPECTED_RESULT_HANDLERS,
        "item-result progression handler table changed"
    );

    let typed_instruction_count = [
        (ENTRY_ADDRESS, ENTRY_CODE_LENGTH, "class-change entry"),
        (
            BATTLE_STATE_HANDLERS_ADDRESS,
            BATTLE_STATE_HANDLERS_LENGTH,
            "class-change battle states",
        ),
        (
            COMPLETION_ADDRESS,
            COMPLETION_LENGTH,
            "class-change completion",
        ),
        (
            MAP_RESTORE_ADDRESS,
            MAP_RESTORE_LENGTH,
            "class-change map restore",
        ),
    ]
    .into_iter()
    .map(|(address, length, role)| {
        let code = source_slice(rom, PRG_BANK, address, length)?;
        Ok(decode_rp2a03_sequence(code, address, role)?.len())
    })
    .sum::<Result<usize>>()?;

    let eligibility_routes = (0..ITEM_COUNT)
        .map(|index| {
            let eligible_source_class_ids = [primary[index], alternate[index]]
                .into_iter()
                .collect::<BTreeSet<_>>()
                .into_iter()
                .collect::<Vec<_>>();
            ClassChangeRoute {
                item_id: ITEM_ID_BASE + index as u8,
                item_id_hex: format!("0x{:02X}", ITEM_ID_BASE + index as u8),
                eligible_source_class_ids_hex: hex_values(&eligible_source_class_ids),
                eligible_source_class_ids,
                target_class_id: target[index],
                target_class_id_hex: format!("0x{:02X}", target[index]),
            }
        })
        .collect();

    Ok(ClassChangeContract {
        minimum_level: MINIMUM_LEVEL,
        minimum_level_hex: format!("0x{MINIMUM_LEVEL:02X}"),
        failure_dialogue_index: FAILURE_DIALOGUE_INDEX,
        failure_dialogue_index_hex: format!("0x{FAILURE_DIALOGUE_INDEX:02X}"),
        eligibility_routes,
        result_progression_states: vec![
            progression(0x04, "initialize shared battle presentation", 0x9D3C),
            progression(
                0x05,
                "run shared battle presentation until state 0x05ED clears",
                0x9D5E,
            ),
            progression(0x06, "leave item flow and restore the map", 0x98AC),
        ],
        typed_instruction_count,
        entry: location(PRG_BANK, ENTRY_ADDRESS),
        battle_state_handlers: location(PRG_BANK, BATTLE_STATE_HANDLERS_ADDRESS),
        completion: location(PRG_BANK, COMPLETION_ADDRESS),
        map_restore: location(PRG_BANK, MAP_RESTORE_ADDRESS),
        static_conclusion: "success requires level 10 or higher plus the selected item's declared source class; it changes the class, enters result substates 0x04 through 0x06, runs the shared battle presentation, and returns to the map without selecting a common result dialogue",
        runtime_gate: "observe the shared battle presentation as a consumer: visible text producers, CHR phases, automatic completion, and map return; do not press during automatic substates 0x04 through 0x06",
    })
}

fn fixed_array<const N: usize>(rom: &Rom, address: u16) -> Result<[u8; N]> {
    Ok(source_slice(rom, PRG_BANK, address, N)?.try_into()?)
}

fn progression(state: u8, role: &'static str, handler: u16) -> ResultProgressionState {
    ResultProgressionState {
        state,
        state_hex: format!("0x{state:02X}"),
        role,
        handler: location(PRG_BANK, handler),
    }
}

fn hex_values(values: &[u8]) -> Vec<String> {
    values
        .iter()
        .map(|value| format!("0x{value:02X}"))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_the_third_item_has_two_source_classes() {
        let route_widths = EXPECTED_PRIMARY_CLASSES
            .into_iter()
            .zip(EXPECTED_ALTERNATE_CLASSES)
            .map(|(primary, alternate)| usize::from(primary != alternate) + 1)
            .collect::<Vec<_>>();
        assert_eq!(route_widths, [1, 1, 2, 1, 1]);
    }

    #[test]
    fn success_progression_uses_three_non_dialogue_handlers() {
        assert_eq!(&EXPECTED_RESULT_HANDLERS[4..], [0x9D3C, 0x9D5E, 0x98AC]);
    }
}
