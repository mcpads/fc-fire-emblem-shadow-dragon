use std::collections::BTreeSet;

use anyhow::{Context, Result, ensure};

use crate::{
    rom::Rom,
    rp2a03::{Instruction, assemble_at},
    sha1_hex,
    typed_source::decode_rp2a03_sequence,
};

const SOURCE_BANK: u8 = 0x0B;
const SELECTOR_ROUTINE_ADDRESS: u16 = 0xA09F;
const GAME_OVER_DIALOGUE_DIRECTORY_SELECTOR: u8 = 0xB0;
const NO_SAVE_RECORD: u8 = 0x0A;
const FIRST_SURVIVOR_RECORD: u8 = 0x06;
const SECOND_SURVIVOR_RECORD: u8 = 0x07;
const EARLY_CHAPTER_RECORD: u8 = 0x09;
const LATE_CHAPTER_RECORD: u8 = 0x08;
const SOURCE_SELECTED_GAME_OVER_RECORDS: [u8; 5] = [
    FIRST_SURVIVOR_RECORD,
    SECOND_SURVIVOR_RECORD,
    LATE_CHAPTER_RECORD,
    EARLY_CHAPTER_RECORD,
    NO_SAVE_RECORD,
];

pub(crate) struct GameOverDialogueSourceBinding {
    pub(crate) record_indices: BTreeSet<usize>,
    pub(crate) selector_routine_sha1: String,
}

pub(crate) fn bind_game_over_dialogue_source(
    source: &Rom,
) -> Result<GameOverDialogueSourceBinding> {
    source.verify_supported_japanese()?;
    let expected = selector_routine()?;
    let bank_offset = usize::from(SOURCE_BANK) * 0x4000;
    let cpu_offset = usize::from(SELECTOR_ROUTINE_ADDRESS - 0x8000);
    let start = bank_offset + cpu_offset;
    let actual = source
        .prg()
        .get(start..start + expected.len())
        .context("game-over dialogue selector routine is outside source PRG")?;
    bind_selector_routine(actual)
}

pub(crate) fn is_source_selected_game_over_dialogue(
    directory_selector: u8,
    entry_selector: u8,
) -> bool {
    directory_selector == GAME_OVER_DIALOGUE_DIRECTORY_SELECTOR
        && SOURCE_SELECTED_GAME_OVER_RECORDS.contains(&entry_selector)
}

pub(crate) fn source_selected_game_over_dialogue_family_hex() -> String {
    let entries = SOURCE_SELECTED_GAME_OVER_RECORDS
        .iter()
        .map(|entry| format!("{entry:02X}"))
        .collect::<Vec<_>>()
        .join("/");
    format!("{GAME_OVER_DIALOGUE_DIRECTORY_SELECTOR:02X}:{entries}")
}

fn bind_selector_routine(actual: &[u8]) -> Result<GameOverDialogueSourceBinding> {
    let expected = selector_routine()?;
    ensure!(
        actual == expected,
        "game-over dialogue selector routine changed at bank {SOURCE_BANK:02X}:${SELECTOR_ROUTINE_ADDRESS:04X}"
    );
    decode_rp2a03_sequence(
        actual,
        SELECTOR_ROUTINE_ADDRESS,
        "game-over dialogue selector routine",
    )?;

    let record_indices = SOURCE_SELECTED_GAME_OVER_RECORDS
        .into_iter()
        .map(usize::from)
        .collect::<BTreeSet<_>>();
    ensure!(
        record_indices.len() == SOURCE_SELECTED_GAME_OVER_RECORDS.len(),
        "game-over dialogue selector family repeats a record"
    );
    Ok(GameOverDialogueSourceBinding {
        record_indices,
        selector_routine_sha1: sha1_hex(actual),
    })
}

fn selector_routine() -> Result<Vec<u8>> {
    assemble_at(
        SELECTOR_ROUTINE_ADDRESS,
        &[
            Instruction::LdaImmediate(0x02),
            Instruction::StaAbsolute(0x06F6),
            Instruction::JsrAbsolute(0x9D25),
            Instruction::LdaZeroPage(0x61),
            Instruction::BneAbsolute(0xA0B4),
            Instruction::LdaImmediate(LATE_CHAPTER_RECORD),
            Instruction::StaAbsolute(0x05EE),
            Instruction::LdaImmediate(NO_SAVE_RECORD),
            Instruction::BneAbsolute(0xA0EB),
            Instruction::JsrAbsolute(0xF111),
            Instruction::LdaImmediate(0x02),
            Instruction::JsrAbsolute(0xF09E),
            Instruction::BcsAbsolute(0xA0CA),
            Instruction::LdyImmediate(0x12),
            Instruction::LdaIndirectY(0x00),
            Instruction::CmpImmediate(0xFF),
            Instruction::BeqAbsolute(0xA0CA),
            Instruction::LdaImmediate(FIRST_SURVIVOR_RECORD),
            Instruction::BneAbsolute(0xA0EB),
            Instruction::JsrAbsolute(0xF111),
            Instruction::LdaImmediate(0x03),
            Instruction::JsrAbsolute(0xF09E),
            Instruction::BcsAbsolute(0xA0E0),
            Instruction::LdyImmediate(0x12),
            Instruction::LdaIndirectY(0x00),
            Instruction::CmpImmediate(0xFF),
            Instruction::BeqAbsolute(0xA0E0),
            Instruction::LdaImmediate(SECOND_SURVIVOR_RECORD),
            Instruction::BneAbsolute(0xA0EB),
            Instruction::LdaImmediate(EARLY_CHAPTER_RECORD),
            Instruction::LdyAbsolute(0x7674),
            Instruction::CpyImmediate(0x05),
            Instruction::BccAbsolute(0xA0EB),
            Instruction::LdaImmediate(LATE_CHAPTER_RECORD),
            Instruction::StaAbsolute(0x77F1),
            Instruction::LdaImmediate(0x00),
            Instruction::StaAbsolute(0x77F0),
            Instruction::LdaImmediate(GAME_OVER_DIALOGUE_DIRECTORY_SELECTOR),
            Instruction::StaAbsolute(0x77F4),
            Instruction::LdaImmediate(0x01),
            Instruction::StaAbsolute(0x77F7),
            Instruction::IncAbsolute(0x05EE),
            Instruction::Rts,
        ],
    )
}

#[cfg(test)]
pub(crate) fn test_game_over_dialogue_source_binding() -> GameOverDialogueSourceBinding {
    bind_selector_routine(&selector_routine().unwrap()).unwrap()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn selector_binding_rejects_a_changed_record_choice() {
        let mut candidate = selector_routine().unwrap();
        let first_record = candidate
            .windows(4)
            .position(|window| window == [0xA9, FIRST_SURVIVOR_RECORD, 0xD0, 0x21])
            .unwrap()
            + 1;
        candidate[first_record] = 0x05;

        let error = bind_selector_routine(&candidate).err().unwrap().to_string();
        assert!(error.contains("selector routine changed"));
    }

    #[test]
    fn selector_binding_owns_all_five_mutually_exclusive_records() {
        let binding = test_game_over_dialogue_source_binding();
        assert_eq!(binding.record_indices, BTreeSet::from([6, 7, 8, 9, 10]));
    }
}
