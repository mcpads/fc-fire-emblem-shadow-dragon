use anyhow::Result;

use crate::rp2a03::{Instruction, assemble_at};

pub(super) const PREDICATE_ADDRESS: u16 = 0xFC21;
pub(super) const PLAYER_INITIATED_STATE: u8 = 0x16;
pub(super) const ENEMY_INITIATED_STATE: u8 = 0x32;
pub(super) const CAIN_RECORD_IDENTITY: u8 = 0x04;
pub(super) const GARUDA_SOLDIER_RECORD_IDENTITY: u8 = 0x85;

const MAIN_STATE: u8 = 0x84;
const BATTLE_RECORD_ONE: u16 = 0x76F4;
const BATTLE_RECORD_TWO: u16 = 0x7715;

pub(super) fn build_predicate() -> Result<Vec<u8>> {
    let pair = PREDICATE_ADDRESS + 10;
    let cain_first = PREDICATE_ADDRESS + 27;
    let mismatch = PREDICATE_ADDRESS + 32;
    assemble_at(
        PREDICATE_ADDRESS,
        &[
            Instruction::LdaZeroPage(MAIN_STATE),
            Instruction::CmpImmediate(PLAYER_INITIATED_STATE),
            Instruction::BeqAbsolute(pair),
            Instruction::CmpImmediate(ENEMY_INITIATED_STATE),
            Instruction::BneAbsolute(mismatch),
            Instruction::LdaAbsolute(BATTLE_RECORD_ONE),
            Instruction::CmpImmediate(CAIN_RECORD_IDENTITY),
            Instruction::BeqAbsolute(cain_first),
            Instruction::CmpImmediate(GARUDA_SOLDIER_RECORD_IDENTITY),
            Instruction::BneAbsolute(mismatch),
            Instruction::LdaAbsolute(BATTLE_RECORD_TWO),
            Instruction::CmpImmediate(CAIN_RECORD_IDENTITY),
            Instruction::Rts,
            Instruction::LdaAbsolute(BATTLE_RECORD_TWO),
            Instruction::CmpImmediate(GARUDA_SOLDIER_RECORD_IDENTITY),
            Instruction::Rts,
        ],
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn predicate_requires_state_and_unordered_participant_pair() {
        assert_eq!(
            build_predicate().unwrap(),
            [
                0xA5,
                MAIN_STATE,
                0xC9,
                PLAYER_INITIATED_STATE,
                0xF0,
                0x04,
                0xC9,
                ENEMY_INITIATED_STATE,
                0xD0,
                0x16,
                0xAD,
                0xF4,
                0x76,
                0xC9,
                CAIN_RECORD_IDENTITY,
                0xF0,
                0x0A,
                0xC9,
                GARUDA_SOLDIER_RECORD_IDENTITY,
                0xD0,
                0x0B,
                0xAD,
                0x15,
                0x77,
                0xC9,
                CAIN_RECORD_IDENTITY,
                0x60,
                0xAD,
                0x15,
                0x77,
                0xC9,
                GARUDA_SOLDIER_RECORD_IDENTITY,
                0x60,
            ]
        );
    }
}
