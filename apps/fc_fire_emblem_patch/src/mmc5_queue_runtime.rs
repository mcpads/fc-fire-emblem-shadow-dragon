use anyhow::{Result, ensure};

use crate::rp2a03::{Instruction, assemble_at};

pub(crate) const PAYLOAD_LEN: usize = 0x200;
pub(crate) const REPLAY_QUEUE_ADDRESS: u16 = 0x6000;
pub(crate) const INITIALIZE_ADDRESS: u16 = 0x6120;
pub(crate) const CLEAR_PHYSICAL_NAMETABLE_ZERO_ADDRESS: u16 = 0x6180;
pub(crate) const STATE_START: u16 = 0x67E0;
pub(crate) const STATE_LEN: u8 = 0x20;
pub(crate) const MAGIC: &[u8; 4] = b"NQB1";
pub(crate) const PHYSICAL_NAMETABLE_START: u16 = 0x6800;
pub(crate) const PHYSICAL_NAMETABLE_END: u16 = 0x7000;

pub(crate) const SOURCE_QUEUE_START: u16 = 0x0781;
const SOURCE_MIRRORING_SHADOW: u8 = 0xC8;

const MIRROR_DATA_ADDRESS: u16 = 0x6060;
const CHECK_NAMETABLE_ADDRESS: u16 = 0x6080;
const VERTICAL_MIRROR_ADDRESS: u16 = 0x60A0;
const PHYSICAL_ADDRESS: u16 = 0x60B0;
const INCREMENT_ADDRESS: u16 = 0x60D0;
const INCREMENT_ACROSS_ADDRESS: u16 = 0x60E8;
const MASK_AND_RETURN_ADDRESS: u16 = 0x60F8;

const BATCH_COUNT: u16 = 0x67E0;
const COMMAND_COUNT: u16 = 0x67E2;
const DATA_COUNT: u16 = 0x67E4;
const NAMETABLE_DATA_COUNT: u16 = 0x67E6;
const DIRECT_CLEAR_COUNT: u16 = 0x67E8;
const VALUE: u16 = 0x67F0;
const DESCRIPTOR: u16 = 0x67F1;
const PPU_ADDRESS_HIGH: u16 = 0x67F4;
const PPU_ADDRESS_LOW: u16 = 0x67F5;
const LOGICAL_NAMETABLE: u16 = 0x67F6;
const MAGIC_START: u16 = 0x67F8;
const REMAINING: u16 = 0x67FC;

pub(crate) fn build_payload() -> Result<Vec<u8>> {
    let mut payload = vec![0xFF; PAYLOAD_LEN];
    for (address, instructions) in [
        (REPLAY_QUEUE_ADDRESS, replay_queue()?),
        (MIRROR_DATA_ADDRESS, mirror_data()?),
        (CHECK_NAMETABLE_ADDRESS, check_nametable()?),
        (VERTICAL_MIRROR_ADDRESS, vertical_mirror()?),
        (PHYSICAL_ADDRESS, physical_address()?),
        (INCREMENT_ADDRESS, increment_address()?),
        (INCREMENT_ACROSS_ADDRESS, increment_across()?),
        (MASK_AND_RETURN_ADDRESS, mask_and_return()?),
        (INITIALIZE_ADDRESS, initialize()?),
        (
            CLEAR_PHYSICAL_NAMETABLE_ZERO_ADDRESS,
            clear_physical_nametable_zero()?,
        ),
    ] {
        let start = usize::from(address - REPLAY_QUEUE_ADDRESS);
        let end = start
            .checked_add(instructions.len())
            .ok_or_else(|| anyhow::anyhow!("queue-shadow runtime payload range overflow"))?;
        ensure!(
            end <= payload.len(),
            "queue-shadow runtime routine at {address:04X} exceeds the payload"
        );
        ensure!(
            payload[start..end].iter().all(|byte| *byte == 0xFF),
            "queue-shadow runtime routine at {address:04X} overlaps another routine"
        );
        payload[start..end].copy_from_slice(&instructions);
    }
    Ok(payload)
}

fn replay_queue() -> Result<Vec<u8>> {
    assemble_at(
        REPLAY_QUEUE_ADDRESS,
        &[
            Instruction::IncAbsolute(BATCH_COUNT),
            Instruction::BneAbsolute(REPLAY_QUEUE_ADDRESS + 0x08),
            Instruction::IncAbsolute(BATCH_COUNT + 1),
            Instruction::LdxImmediate(0),
            Instruction::LdaAbsoluteX(SOURCE_QUEUE_START),
            Instruction::BeqAbsolute(REPLAY_QUEUE_ADDRESS + 0x57),
            Instruction::StaAbsolute(PPU_ADDRESS_HIGH),
            Instruction::Inx,
            Instruction::LdaAbsoluteX(SOURCE_QUEUE_START),
            Instruction::StaAbsolute(PPU_ADDRESS_LOW),
            Instruction::Inx,
            Instruction::LdaAbsoluteX(SOURCE_QUEUE_START),
            Instruction::StaAbsolute(DESCRIPTOR),
            Instruction::AndImmediate(0x3F),
            Instruction::StaAbsolute(REMAINING),
            Instruction::IncAbsolute(COMMAND_COUNT),
            Instruction::BneAbsolute(REPLAY_QUEUE_ADDRESS + 0x2D),
            Instruction::IncAbsolute(COMMAND_COUNT + 1),
            Instruction::Inx,
            Instruction::LdaAbsoluteX(SOURCE_QUEUE_START),
            Instruction::StaAbsolute(VALUE),
            Instruction::IncAbsolute(DATA_COUNT),
            Instruction::BneAbsolute(REPLAY_QUEUE_ADDRESS + 0x3C),
            Instruction::IncAbsolute(DATA_COUNT + 1),
            Instruction::JsrAbsolute(MIRROR_DATA_ADDRESS),
            Instruction::LdaAbsolute(DESCRIPTOR),
            Instruction::AndImmediate(0x40),
            Instruction::BneAbsolute(REPLAY_QUEUE_ADDRESS + 0x47),
            Instruction::Inx,
            Instruction::DecAbsolute(REMAINING),
            Instruction::BneAbsolute(REPLAY_QUEUE_ADDRESS + 0x2E),
            Instruction::LdaAbsolute(DESCRIPTOR),
            Instruction::AndImmediate(0x40),
            Instruction::BeqAbsolute(REPLAY_QUEUE_ADDRESS + 0x54),
            Instruction::Inx,
            Instruction::JmpAbsolute(REPLAY_QUEUE_ADDRESS + 0x0A),
            Instruction::Rts,
        ],
    )
}

fn mirror_data() -> Result<Vec<u8>> {
    assemble_at(
        MIRROR_DATA_ADDRESS,
        &[
            Instruction::LdaAbsolute(PPU_ADDRESS_HIGH),
            Instruction::CmpImmediate(0x3F),
            Instruction::BcsAbsolute(INCREMENT_ADDRESS),
            Instruction::CmpImmediate(0x30),
            Instruction::BccAbsolute(CHECK_NAMETABLE_ADDRESS),
            Instruction::Sec,
            Instruction::SbcImmediate(0x10),
            Instruction::JmpAbsolute(CHECK_NAMETABLE_ADDRESS),
        ],
    )
}

fn check_nametable() -> Result<Vec<u8>> {
    assemble_at(
        CHECK_NAMETABLE_ADDRESS,
        &[
            Instruction::CmpImmediate(0x20),
            Instruction::BccAbsolute(INCREMENT_ADDRESS),
            Instruction::Sec,
            Instruction::SbcImmediate(0x20),
            Instruction::Pha,
            Instruction::AndImmediate(0x03),
            Instruction::Clc,
            Instruction::AdcImmediate((PHYSICAL_NAMETABLE_START >> 8) as u8),
            Instruction::StaZeroPage(0x01),
            Instruction::Pla,
            Instruction::LsrAccumulator,
            Instruction::LsrAccumulator,
            Instruction::StaAbsolute(LOGICAL_NAMETABLE),
            Instruction::LdaZeroPage(SOURCE_MIRRORING_SHADOW),
            Instruction::BeqAbsolute(VERTICAL_MIRROR_ADDRESS),
            Instruction::LdaAbsolute(LOGICAL_NAMETABLE),
            Instruction::LsrAccumulator,
            Instruction::JmpAbsolute(PHYSICAL_ADDRESS),
        ],
    )
}

fn vertical_mirror() -> Result<Vec<u8>> {
    assemble_at(
        VERTICAL_MIRROR_ADDRESS,
        &[
            Instruction::LdaAbsolute(LOGICAL_NAMETABLE),
            Instruction::AndImmediate(0x01),
            Instruction::JmpAbsolute(PHYSICAL_ADDRESS),
        ],
    )
}

fn physical_address() -> Result<Vec<u8>> {
    assemble_at(
        PHYSICAL_ADDRESS,
        &[
            Instruction::AslAccumulator,
            Instruction::AslAccumulator,
            Instruction::Clc,
            Instruction::AdcZeroPage(0x01),
            Instruction::StaZeroPage(0x01),
            Instruction::LdaAbsolute(PPU_ADDRESS_LOW),
            Instruction::StaZeroPage(0x00),
            Instruction::LdyImmediate(0),
            Instruction::LdaAbsolute(VALUE),
            Instruction::StaIndirectY(0x00),
            Instruction::IncAbsolute(NAMETABLE_DATA_COUNT),
            Instruction::BneAbsolute(PHYSICAL_ADDRESS + 0x1B),
            Instruction::IncAbsolute(NAMETABLE_DATA_COUNT + 1),
            Instruction::JmpAbsolute(INCREMENT_ADDRESS),
        ],
    )
}

fn increment_address() -> Result<Vec<u8>> {
    assemble_at(
        INCREMENT_ADDRESS,
        &[
            Instruction::LdaAbsolute(DESCRIPTOR),
            Instruction::AndImmediate(0x80),
            Instruction::BeqAbsolute(INCREMENT_ACROSS_ADDRESS),
            Instruction::LdaAbsolute(PPU_ADDRESS_LOW),
            Instruction::Clc,
            Instruction::AdcImmediate(32),
            Instruction::StaAbsolute(PPU_ADDRESS_LOW),
            Instruction::BccAbsolute(MASK_AND_RETURN_ADDRESS),
            Instruction::IncAbsolute(PPU_ADDRESS_HIGH),
            Instruction::JmpAbsolute(MASK_AND_RETURN_ADDRESS),
        ],
    )
}

fn increment_across() -> Result<Vec<u8>> {
    assemble_at(
        INCREMENT_ACROSS_ADDRESS,
        &[
            Instruction::IncAbsolute(PPU_ADDRESS_LOW),
            Instruction::BneAbsolute(MASK_AND_RETURN_ADDRESS),
            Instruction::IncAbsolute(PPU_ADDRESS_HIGH),
            Instruction::JmpAbsolute(MASK_AND_RETURN_ADDRESS),
        ],
    )
}

fn mask_and_return() -> Result<Vec<u8>> {
    assemble_at(
        MASK_AND_RETURN_ADDRESS,
        &[
            Instruction::LdaAbsolute(PPU_ADDRESS_HIGH),
            Instruction::AndImmediate(0x3F),
            Instruction::StaAbsolute(PPU_ADDRESS_HIGH),
            Instruction::Rts,
        ],
    )
}

fn initialize() -> Result<Vec<u8>> {
    let fill_loop_address = INITIALIZE_ADDRESS + 0x04;
    let state_loop_address = INITIALIZE_ADDRESS + 0x23;
    let mut instructions = vec![
        Instruction::LdaImmediate(0xFF),
        Instruction::LdxImmediate(0),
        Instruction::StaAbsoluteX(0x6800),
        Instruction::StaAbsoluteX(0x6900),
        Instruction::StaAbsoluteX(0x6A00),
        Instruction::StaAbsoluteX(0x6B00),
        Instruction::StaAbsoluteX(0x6C00),
        Instruction::StaAbsoluteX(0x6D00),
        Instruction::StaAbsoluteX(0x6E00),
        Instruction::StaAbsoluteX(0x6F00),
        Instruction::Inx,
        Instruction::BneAbsolute(fill_loop_address),
        Instruction::LdaImmediate(0),
        Instruction::LdxImmediate(0),
        Instruction::StaAbsoluteX(STATE_START),
        Instruction::Inx,
        Instruction::CpxImmediate(STATE_LEN),
        Instruction::BneAbsolute(state_loop_address),
    ];
    for (index, byte) in MAGIC.iter().copied().enumerate() {
        instructions.push(Instruction::LdaImmediate(byte));
        instructions.push(Instruction::StaAbsolute(MAGIC_START + index as u16));
    }
    instructions.push(Instruction::Rts);
    assemble_at(INITIALIZE_ADDRESS, &instructions)
}

fn clear_physical_nametable_zero() -> Result<Vec<u8>> {
    let fill_loop_address = CLEAR_PHYSICAL_NAMETABLE_ZERO_ADDRESS + 0x0C;
    let attribute_loop_address = CLEAR_PHYSICAL_NAMETABLE_ZERO_ADDRESS + 0x1F;
    assemble_at(
        CLEAR_PHYSICAL_NAMETABLE_ZERO_ADDRESS,
        &[
            Instruction::IncAbsolute(DIRECT_CLEAR_COUNT),
            Instruction::BneAbsolute(CLEAR_PHYSICAL_NAMETABLE_ZERO_ADDRESS + 0x08),
            Instruction::IncAbsolute(DIRECT_CLEAR_COUNT + 1),
            Instruction::LdaImmediate(0xFF),
            Instruction::LdxImmediate(0),
            Instruction::StaAbsoluteX(0x6800),
            Instruction::StaAbsoluteX(0x6900),
            Instruction::StaAbsoluteX(0x6A00),
            Instruction::StaAbsoluteX(0x6B00),
            Instruction::Inx,
            Instruction::BneAbsolute(fill_loop_address),
            Instruction::LdaImmediate(0),
            Instruction::LdxImmediate(0),
            Instruction::StaAbsoluteX(0x6BC0),
            Instruction::Inx,
            Instruction::CpxImmediate(0x40),
            Instruction::BneAbsolute(attribute_loop_address),
            Instruction::Rts,
        ],
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn runtime_payload_routines_fit_without_overlap() {
        let payload = build_payload().unwrap();
        let replay = replay_queue().unwrap();
        let initializer = initialize().unwrap();
        let clear = clear_physical_nametable_zero().unwrap();
        assert_eq!(payload.len(), PAYLOAD_LEN);
        assert!(replay.len() <= usize::from(MIRROR_DATA_ADDRESS - REPLAY_QUEUE_ADDRESS));
        assert!(initializer.len() < 0x100);
        assert!(clear.len() <= usize::from(STATE_START - CLEAR_PHYSICAL_NAMETABLE_ZERO_ADDRESS));
        assert_eq!(
            &payload[usize::from(INITIALIZE_ADDRESS - REPLAY_QUEUE_ADDRESS)
                ..usize::from(INITIALIZE_ADDRESS - REPLAY_QUEUE_ADDRESS) + initializer.len()],
            initializer
        );
    }

    #[test]
    fn runtime_state_and_two_nametables_are_disjoint_from_code() {
        assert_eq!(PHYSICAL_NAMETABLE_END - PHYSICAL_NAMETABLE_START, 0x0800);
        assert!(REPLAY_QUEUE_ADDRESS + PAYLOAD_LEN as u16 <= STATE_START);
        assert!(STATE_START + u16::from(STATE_LEN) <= PHYSICAL_NAMETABLE_START);
    }
}
