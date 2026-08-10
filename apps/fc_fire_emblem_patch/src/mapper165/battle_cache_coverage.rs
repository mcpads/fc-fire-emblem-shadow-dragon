use anyhow::Result;

use crate::rp2a03::{Instruction, assemble_at};

pub(super) const PREDICATE_ADDRESS: u16 = 0xFC21;
pub(super) const FIELD_PREDICATE_ADDRESS: u16 = 0xFD00;
pub(super) const PLAYER_INITIATED_STATE: u8 = 0x16;
pub(super) const ENEMY_INITIATED_STATE: u8 = 0x32;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) struct BattleTextCoverage {
    pub(super) participant_record_identities: [u8; 2],
    pub(super) class_record_identities: [u8; 2],
    pub(super) item_record_identities: [u8; 2],
    pub(super) terrain_source_indices: [u8; 2],
}

impl BattleTextCoverage {
    pub(super) fn from_source_indices(
        player_name: usize,
        enemy_name: usize,
        classes: [usize; 2],
        items: [usize; 2],
        terrains: [usize; 2],
    ) -> Result<Self> {
        let player = one_based_identity(player_name)?;
        let enemy = one_based_identity(enemy_name)? | 0x80;
        Ok(Self {
            participant_record_identities: [player, enemy],
            class_record_identities: identities(classes, true)?,
            item_record_identities: identities(items, true)?,
            terrain_source_indices: identities(terrains, false)?,
        })
    }
}

fn identities(indices: [usize; 2], one_based: bool) -> Result<[u8; 2]> {
    let convert = |index| {
        if one_based {
            one_based_identity(index)
        } else {
            u8::try_from(index).map_err(Into::into)
        }
    };
    let left = convert(indices[0])?;
    let right = convert(indices[1])?;
    Ok([left, right])
}

fn one_based_identity(source_index: usize) -> Result<u8> {
    u8::try_from(
        source_index
            .checked_add(1)
            .ok_or_else(|| anyhow::anyhow!("battle text source index overflow"))?,
    )
    .map_err(Into::into)
}

const MAIN_STATE: u8 = 0x84;
const BATTLE_RECORD_ONE: u16 = 0x76F4;
const BATTLE_RECORD_TWO: u16 = 0x7715;
const CLASS_IDENTITY_OFFSET: u16 = 1;
const EQUIPPED_ITEM_IDENTITY_OFFSET: u16 = 0x13;
const TERRAIN_ONE: u16 = 0x0322;
const TERRAIN_TWO: u16 = 0x0323;

pub(super) fn build_participant_predicate(coverage: &BattleTextCoverage) -> Result<Vec<u8>> {
    let pair = PREDICATE_ADDRESS + 10;
    let player_first = PREDICATE_ADDRESS + 29;
    let fields = PREDICATE_ADDRESS + 36;
    let mismatch = PREDICATE_ADDRESS + 39;
    assemble_at(
        PREDICATE_ADDRESS,
        &[
            Instruction::LdaZeroPage(MAIN_STATE),
            Instruction::CmpImmediate(PLAYER_INITIATED_STATE),
            Instruction::BeqAbsolute(pair),
            Instruction::CmpImmediate(ENEMY_INITIATED_STATE),
            Instruction::BneAbsolute(mismatch),
            Instruction::LdaAbsolute(BATTLE_RECORD_ONE),
            Instruction::CmpImmediate(coverage.participant_record_identities[0]),
            Instruction::BeqAbsolute(player_first),
            Instruction::CmpImmediate(coverage.participant_record_identities[1]),
            Instruction::BneAbsolute(mismatch),
            Instruction::LdaAbsolute(BATTLE_RECORD_TWO),
            Instruction::CmpImmediate(coverage.participant_record_identities[0]),
            Instruction::BeqAbsolute(fields),
            Instruction::Rts,
            Instruction::LdaAbsolute(BATTLE_RECORD_TWO),
            Instruction::CmpImmediate(coverage.participant_record_identities[1]),
            Instruction::BneAbsolute(mismatch),
            Instruction::JmpAbsolute(FIELD_PREDICATE_ADDRESS),
            Instruction::Rts,
        ],
    )
}

pub(super) fn build_field_predicate(coverage: &BattleTextCoverage) -> Result<Vec<u8>> {
    let fields = [
        (
            BATTLE_RECORD_ONE + CLASS_IDENTITY_OFFSET,
            coverage.class_record_identities,
        ),
        (
            BATTLE_RECORD_TWO + CLASS_IDENTITY_OFFSET,
            coverage.class_record_identities,
        ),
        (
            BATTLE_RECORD_ONE + EQUIPPED_ITEM_IDENTITY_OFFSET,
            coverage.item_record_identities,
        ),
        (
            BATTLE_RECORD_TWO + EQUIPPED_ITEM_IDENTITY_OFFSET,
            coverage.item_record_identities,
        ),
        (TERRAIN_ONE, coverage.terrain_source_indices),
        (TERRAIN_TWO, coverage.terrain_source_indices),
    ];
    let mismatch = FIELD_PREDICATE_ADDRESS + 64;
    let mut instructions = Vec::with_capacity(30);
    for (index, (address, supported)) in fields.into_iter().enumerate() {
        instructions.extend([
            Instruction::LdaAbsolute(address),
            Instruction::CmpImmediate(supported[0]),
            Instruction::BeqAbsolute(if index == 5 {
                mismatch
            } else {
                FIELD_PREDICATE_ADDRESS + u16::try_from((index + 1) * 11)?
            }),
            Instruction::CmpImmediate(supported[1]),
        ]);
        if index < 5 {
            instructions.push(Instruction::BneAbsolute(mismatch));
        }
    }
    instructions.push(Instruction::Rts);
    assemble_at(FIELD_PREDICATE_ADDRESS, &instructions)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn participant_predicate_requires_state_and_unordered_pair_before_fields() {
        assert_eq!(
            build_participant_predicate(
                &BattleTextCoverage::from_source_indices(3, 4, [0, 7], [11, 26], [0, 11]).unwrap(),
            )
            .unwrap(),
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
                0x1D,
                0xAD,
                0xF4,
                0x76,
                0xC9,
                0x04,
                0xF0,
                0x0C,
                0xC9,
                0x85,
                0xD0,
                0x12,
                0xAD,
                0x15,
                0x77,
                0xC9,
                0x04,
                0xF0,
                0x08,
                0x60,
                0xAD,
                0x15,
                0x77,
                0xC9,
                0x85,
                0xD0,
                0x03,
                0x4C,
                0x00,
                0xFD,
                0x60,
            ]
        );
    }

    #[test]
    fn field_predicate_accepts_each_value_from_the_cached_source_sets() {
        assert_eq!(
            build_field_predicate(
                &BattleTextCoverage::from_source_indices(3, 4, [0, 7], [11, 26], [0, 11]).unwrap(),
            )
            .unwrap(),
            [
                0xAD, 0xF5, 0x76, 0xC9, 0x01, 0xF0, 0x04, 0xC9, 0x08, 0xD0, 0x35, 0xAD, 0x16, 0x77,
                0xC9, 0x01, 0xF0, 0x04, 0xC9, 0x08, 0xD0, 0x2A, 0xAD, 0x07, 0x77, 0xC9, 0x0C, 0xF0,
                0x04, 0xC9, 0x1B, 0xD0, 0x1F, 0xAD, 0x28, 0x77, 0xC9, 0x0C, 0xF0, 0x04, 0xC9, 0x1B,
                0xD0, 0x14, 0xAD, 0x22, 0x03, 0xC9, 0x00, 0xF0, 0x04, 0xC9, 0x0B, 0xD0, 0x09, 0xAD,
                0x23, 0x03, 0xC9, 0x00, 0xF0, 0x02, 0xC9, 0x0B, 0x60,
            ]
        );
    }

    #[test]
    fn coverage_is_generated_from_the_selected_source_entries() {
        assert_eq!(
            BattleTextCoverage::from_source_indices(3, 4, [0, 7], [11, 26], [0, 11]).unwrap(),
            BattleTextCoverage {
                participant_record_identities: [0x04, 0x85],
                class_record_identities: [0x01, 0x08],
                item_record_identities: [0x0C, 0x1B],
                terrain_source_indices: [0x00, 0x0B],
            }
        );
    }
}
