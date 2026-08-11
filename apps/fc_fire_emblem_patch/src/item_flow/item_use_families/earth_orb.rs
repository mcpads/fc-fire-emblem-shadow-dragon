use anyhow::{Result, ensure};
use serde::Serialize;

use crate::{rom::Rom, typed_source::decode_rp2a03_sequence};

use super::super::{CodeLocation, location, source_contract::source_slice};

const PRG_BANK: u8 = 0x06;
const ITEM_ID: u8 = 0x55;
const EFFECT_ADDRESS: u16 = 0x98BB;
const EFFECT_CODE_LENGTH: usize = 170;
const DISPLACEMENT_TABLE_ADDRESS: u16 = 0x9965;
const DISPLACEMENT_TABLE_LENGTH: usize = 16;
const RECORD_EFFECT_ADDRESS: u16 = 0x9975;
const RECORD_EFFECT_LENGTH: usize = 37;
const EFFECT_FRAME_COUNT: u8 = 0x20;
const ALLIED_RECORD_BASE: u16 = 0x6A90;
const ENEMY_RECORD_BASE: u16 = 0x7078;
const RESULT_DIALOGUE_INDEX: u8 = 0x33;

const EXPECTED_DISPLACEMENT_TABLE: [u8; DISPLACEMENT_TABLE_LENGTH] = [
    0xFF, 0x00, 0x01, 0x00, 0x00, 0x01, 0x00, 0xFF, 0x00, 0x02, 0x00, 0xFE, 0x00, 0x03, 0x00, 0xFD,
];

#[derive(Debug, Serialize)]
pub(super) struct EarthOrbContract {
    item_id: u8,
    item_id_hex: String,
    effect_frame_count: u8,
    effect_frame_count_hex: String,
    allied_record_base: u16,
    allied_record_base_hex: String,
    enemy_record_base: u16,
    enemy_record_base_hex: String,
    result_dialogue_index: u8,
    result_dialogue_index_hex: String,
    result_progression_state: u8,
    result_progression_state_hex: String,
    typed_instruction_count: usize,
    effect: CodeLocation,
    record_effect: CodeLocation,
    static_conclusion: &'static str,
    runtime_gate: &'static str,
}

pub(super) fn inspect(rom: &Rom) -> Result<EarthOrbContract> {
    let displacement_table = source_slice(
        rom,
        PRG_BANK,
        DISPLACEMENT_TABLE_ADDRESS,
        DISPLACEMENT_TABLE_LENGTH,
    )?;
    ensure!(
        displacement_table == EXPECTED_DISPLACEMENT_TABLE,
        "earth-orb displacement table changed"
    );
    let effect = source_slice(rom, PRG_BANK, EFFECT_ADDRESS, EFFECT_CODE_LENGTH)?;
    let record_effect = source_slice(rom, PRG_BANK, RECORD_EFFECT_ADDRESS, RECORD_EFFECT_LENGTH)?;
    let typed_instruction_count =
        decode_rp2a03_sequence(effect, EFFECT_ADDRESS, "earth-orb visible effect")?.len()
            + decode_rp2a03_sequence(
                record_effect,
                RECORD_EFFECT_ADDRESS,
                "earth-orb unit-record effect",
            )?
            .len();

    Ok(EarthOrbContract {
        item_id: ITEM_ID,
        item_id_hex: format!("0x{ITEM_ID:02X}"),
        effect_frame_count: EFFECT_FRAME_COUNT,
        effect_frame_count_hex: format!("0x{EFFECT_FRAME_COUNT:02X}"),
        allied_record_base: ALLIED_RECORD_BASE,
        allied_record_base_hex: format!("0x{ALLIED_RECORD_BASE:04X}"),
        enemy_record_base: ENEMY_RECORD_BASE,
        enemy_record_base_hex: format!("0x{ENEMY_RECORD_BASE:04X}"),
        result_dialogue_index: RESULT_DIALOGUE_INDEX,
        result_dialogue_index_hex: format!("0x{RESULT_DIALOGUE_INDEX:02X}"),
        result_progression_state: 0x03,
        result_progression_state_hex: "0x03".to_owned(),
        typed_instruction_count,
        effect: location(PRG_BANK, EFFECT_ADDRESS),
        record_effect: location(PRG_BANK, RECORD_EFFECT_ADDRESS),
        static_conclusion: "item 0x55 runs a synchronous 32-step map-displacement effect inside result substate 0x02, applies the effect to allied and enemy record lists, selects result dialogue 0x33, and then enters the common input-wait substate 0x03; it does not enter the class-change progression",
        runtime_gate: "capture the automatic 32-step effect at irregular phases and the final substate 0x03 result without input during the effect; verify whether any intermediate text is visible and which CHR pairs remain live",
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn displacement_table_has_four_offsets_for_each_phase() {
        assert_eq!(EXPECTED_DISPLACEMENT_TABLE.len(), 4 * 4);
        assert_eq!(EFFECT_FRAME_COUNT, 32);
    }
}
