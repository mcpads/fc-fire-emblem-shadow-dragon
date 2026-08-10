use anyhow::Result;

use crate::rp2a03::{Instruction, assemble_at};

pub(super) const PREDICATE_ADDRESS: u16 = 0xFC21;
pub(super) const PLAYER_INITIATED_STATE: u8 = 0x16;
pub(super) const ENEMY_INITIATED_STATE: u8 = 0x32;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) struct BattleTextSignature {
    pub(super) participant_record_identities: [u8; 2],
    pub(super) class_identity_sum: u8,
    pub(super) item_identity_sum: u8,
    pub(super) terrain_identity_sum: u8,
}

impl BattleTextSignature {
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
            class_identity_sum: identity_sum(classes, true)?,
            item_identity_sum: identity_sum(items, true)?,
            terrain_identity_sum: identity_sum(terrains, false)?,
        })
    }
}

fn identity_sum(indices: [usize; 2], one_based: bool) -> Result<u8> {
    let convert = |index| {
        if one_based {
            one_based_identity(index)
        } else {
            u8::try_from(index).map_err(Into::into)
        }
    };
    let left = convert(indices[0])?;
    let right = convert(indices[1])?;
    left.checked_add(right)
        .ok_or_else(|| anyhow::anyhow!("battle text identity sum overflow"))
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

pub(super) fn build_predicate(signature: &BattleTextSignature) -> Result<Vec<u8>> {
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
            Instruction::CmpImmediate(signature.participant_record_identities[0]),
            Instruction::BeqAbsolute(cain_first),
            Instruction::CmpImmediate(signature.participant_record_identities[1]),
            Instruction::BneAbsolute(mismatch),
            Instruction::LdaAbsolute(BATTLE_RECORD_TWO),
            Instruction::CmpImmediate(signature.participant_record_identities[0]),
            Instruction::Rts,
            Instruction::LdaAbsolute(BATTLE_RECORD_TWO),
            Instruction::CmpImmediate(signature.participant_record_identities[1]),
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
            build_predicate(
                &BattleTextSignature::from_source_indices(3, 4, [0, 7], [11, 26], [0, 11]).unwrap(),
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
                0x16,
                0xAD,
                0xF4,
                0x76,
                0xC9,
                0x04,
                0xF0,
                0x0A,
                0xC9,
                0x85,
                0xD0,
                0x0B,
                0xAD,
                0x15,
                0x77,
                0xC9,
                0x04,
                0x60,
                0xAD,
                0x15,
                0x77,
                0xC9,
                0x85,
                0x60,
            ]
        );
    }

    #[test]
    fn signature_is_generated_from_the_selected_source_entries() {
        assert_eq!(
            BattleTextSignature::from_source_indices(3, 4, [0, 7], [11, 26], [0, 11]).unwrap(),
            BattleTextSignature {
                participant_record_identities: [0x04, 0x85],
                class_identity_sum: 0x09,
                item_identity_sum: 0x27,
                terrain_identity_sum: 0x0B,
            }
        );
    }
}
