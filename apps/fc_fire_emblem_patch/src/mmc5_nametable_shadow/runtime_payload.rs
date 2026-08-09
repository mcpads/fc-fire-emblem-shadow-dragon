use super::*;

pub(super) fn runtime_payload() -> Result<Vec<u8>> {
    let mut payload = vec![0xFF; RUNTIME_PAYLOAD_LEN];
    for (address, instructions) in [
        (RUNTIME_DISPATCH_ADDRESS, runtime_dispatch()?),
        (RUNTIME_ADDRESS_HIGH_ADDRESS, runtime_address_high()?),
        (RUNTIME_ADDRESS_LOW_ADDRESS, runtime_address_low()?),
        (RUNTIME_DATA_ADDRESS, runtime_data_prepare()?),
        (RUNTIME_CHECK_NAMETABLE_ADDRESS, runtime_check_nametable()?),
        (RUNTIME_VERTICAL_MIRROR_ADDRESS, runtime_vertical_mirror()?),
        (RUNTIME_PHYSICAL_ADDRESS, runtime_physical_address()?),
        (RUNTIME_INCREMENT_ADDRESS, runtime_increment_address()?),
        (
            RUNTIME_INCREMENT_ACROSS_ADDRESS,
            runtime_increment_across()?,
        ),
        (
            RUNTIME_MASK_AND_RESTORE_ADDRESS,
            runtime_mask_and_restore()?,
        ),
        (RUNTIME_INITIALIZE_ADDRESS, runtime_initialize()?),
    ] {
        let start = usize::from(address - RUNTIME_DISPATCH_ADDRESS);
        let end = start
            .checked_add(instructions.len())
            .ok_or_else(|| anyhow::anyhow!("runtime payload range overflow"))?;
        ensure!(
            end <= payload.len(),
            "runtime routine at {address:04X} exceeds the payload"
        );
        ensure!(
            payload[start..end].iter().all(|byte| *byte == 0xFF),
            "runtime routine at {address:04X} overlaps another routine"
        );
        payload[start..end].copy_from_slice(&instructions);
    }
    Ok(payload)
}

pub(super) fn runtime_dispatch() -> Result<Vec<u8>> {
    assemble_at(
        RUNTIME_DISPATCH_ADDRESS,
        &[
            Instruction::CpyImmediate(OPERATION_ADDRESS_HIGH),
            Instruction::BeqAbsolute(RUNTIME_ADDRESS_HIGH_ADDRESS),
            Instruction::CpyImmediate(OPERATION_ADDRESS_LOW),
            Instruction::BeqAbsolute(RUNTIME_ADDRESS_LOW_ADDRESS),
            Instruction::CpyImmediate(OPERATION_DATA),
            Instruction::BeqAbsolute(RUNTIME_DATA_ADDRESS),
            Instruction::Rts,
        ],
    )
}

pub(super) fn runtime_address_high() -> Result<Vec<u8>> {
    assemble_at(
        RUNTIME_ADDRESS_HIGH_ADDRESS,
        &[
            Instruction::AndImmediate(0x3F),
            Instruction::StaAbsolute(RUNTIME_PPU_ADDRESS_HIGH),
            Instruction::IncAbsolute(RUNTIME_ADDRESS_HIGH_COUNT),
            Instruction::BneAbsolute(RUNTIME_ADDRESS_HIGH_ADDRESS + 0x0D),
            Instruction::IncAbsolute(RUNTIME_ADDRESS_HIGH_COUNT + 1),
            Instruction::Rts,
        ],
    )
}

pub(super) fn runtime_address_low() -> Result<Vec<u8>> {
    assemble_at(
        RUNTIME_ADDRESS_LOW_ADDRESS,
        &[
            Instruction::StaAbsolute(RUNTIME_PPU_ADDRESS_LOW),
            Instruction::IncAbsolute(RUNTIME_ADDRESS_LOW_COUNT),
            Instruction::BneAbsolute(RUNTIME_ADDRESS_LOW_ADDRESS + 0x0B),
            Instruction::IncAbsolute(RUNTIME_ADDRESS_LOW_COUNT + 1),
            Instruction::Rts,
        ],
    )
}

pub(super) fn runtime_data_prepare() -> Result<Vec<u8>> {
    assemble_at(
        RUNTIME_DATA_ADDRESS,
        &[
            Instruction::IncAbsolute(RUNTIME_DATA_COUNT),
            Instruction::BneAbsolute(RUNTIME_DATA_ADDRESS + 0x08),
            Instruction::IncAbsolute(RUNTIME_DATA_COUNT + 1),
            Instruction::StaAbsolute(RUNTIME_VALUE),
            Instruction::LdaZeroPage(0x00),
            Instruction::StaAbsolute(RUNTIME_SAVED_ZERO_PAGE_0),
            Instruction::LdaZeroPage(0x01),
            Instruction::StaAbsolute(RUNTIME_SAVED_ZERO_PAGE_1),
            Instruction::LdaAbsolute(RUNTIME_PPU_ADDRESS_HIGH),
            Instruction::CmpImmediate(0x3F),
            Instruction::BcsAbsolute(RUNTIME_INCREMENT_ADDRESS),
            Instruction::CmpImmediate(0x30),
            Instruction::BccAbsolute(RUNTIME_CHECK_NAMETABLE_ADDRESS),
            Instruction::Sec,
            Instruction::SbcImmediate(0x10),
            Instruction::JmpAbsolute(RUNTIME_CHECK_NAMETABLE_ADDRESS),
        ],
    )
}

pub(super) fn runtime_check_nametable() -> Result<Vec<u8>> {
    assemble_at(
        RUNTIME_CHECK_NAMETABLE_ADDRESS,
        &[
            Instruction::CmpImmediate(0x20),
            Instruction::BccAbsolute(RUNTIME_INCREMENT_ADDRESS),
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
            Instruction::StaAbsolute(RUNTIME_LOGICAL_NAMETABLE),
            Instruction::LdaZeroPage(SOURCE_MIRRORING_SHADOW),
            Instruction::BeqAbsolute(RUNTIME_VERTICAL_MIRROR_ADDRESS),
            Instruction::LdaAbsolute(RUNTIME_LOGICAL_NAMETABLE),
            Instruction::LsrAccumulator,
            Instruction::JmpAbsolute(RUNTIME_PHYSICAL_ADDRESS),
        ],
    )
}

pub(super) fn runtime_vertical_mirror() -> Result<Vec<u8>> {
    assemble_at(
        RUNTIME_VERTICAL_MIRROR_ADDRESS,
        &[
            Instruction::LdaAbsolute(RUNTIME_LOGICAL_NAMETABLE),
            Instruction::AndImmediate(0x01),
            Instruction::JmpAbsolute(RUNTIME_PHYSICAL_ADDRESS),
        ],
    )
}

pub(super) fn runtime_physical_address() -> Result<Vec<u8>> {
    assemble_at(
        RUNTIME_PHYSICAL_ADDRESS,
        &[
            Instruction::AslAccumulator,
            Instruction::AslAccumulator,
            Instruction::Clc,
            Instruction::AdcZeroPage(0x01),
            Instruction::StaZeroPage(0x01),
            Instruction::LdaAbsolute(RUNTIME_PPU_ADDRESS_LOW),
            Instruction::StaZeroPage(0x00),
            Instruction::LdyImmediate(0),
            Instruction::LdaAbsolute(RUNTIME_VALUE),
            Instruction::StaIndirectY(0x00),
            Instruction::JmpAbsolute(RUNTIME_INCREMENT_ADDRESS),
        ],
    )
}

pub(super) fn runtime_increment_address() -> Result<Vec<u8>> {
    assemble_at(
        RUNTIME_INCREMENT_ADDRESS,
        &[
            Instruction::LdaZeroPage(SOURCE_PPU_CONTROL_SHADOW),
            Instruction::AndImmediate(0x04),
            Instruction::BeqAbsolute(RUNTIME_INCREMENT_ACROSS_ADDRESS),
            Instruction::LdaAbsolute(RUNTIME_PPU_ADDRESS_LOW),
            Instruction::Clc,
            Instruction::AdcImmediate(32),
            Instruction::StaAbsolute(RUNTIME_PPU_ADDRESS_LOW),
            Instruction::BccAbsolute(RUNTIME_MASK_AND_RESTORE_ADDRESS),
            Instruction::IncAbsolute(RUNTIME_PPU_ADDRESS_HIGH),
            Instruction::JmpAbsolute(RUNTIME_MASK_AND_RESTORE_ADDRESS),
        ],
    )
}

pub(super) fn runtime_increment_across() -> Result<Vec<u8>> {
    assemble_at(
        RUNTIME_INCREMENT_ACROSS_ADDRESS,
        &[
            Instruction::IncAbsolute(RUNTIME_PPU_ADDRESS_LOW),
            Instruction::BneAbsolute(RUNTIME_MASK_AND_RESTORE_ADDRESS),
            Instruction::IncAbsolute(RUNTIME_PPU_ADDRESS_HIGH),
            Instruction::JmpAbsolute(RUNTIME_MASK_AND_RESTORE_ADDRESS),
        ],
    )
}

pub(super) fn runtime_mask_and_restore() -> Result<Vec<u8>> {
    assemble_at(
        RUNTIME_MASK_AND_RESTORE_ADDRESS,
        &[
            Instruction::LdaAbsolute(RUNTIME_PPU_ADDRESS_HIGH),
            Instruction::AndImmediate(0x3F),
            Instruction::StaAbsolute(RUNTIME_PPU_ADDRESS_HIGH),
            Instruction::LdaAbsolute(RUNTIME_SAVED_ZERO_PAGE_0),
            Instruction::StaZeroPage(0x00),
            Instruction::LdaAbsolute(RUNTIME_SAVED_ZERO_PAGE_1),
            Instruction::StaZeroPage(0x01),
            Instruction::Rts,
        ],
    )
}

pub(super) fn runtime_initialize() -> Result<Vec<u8>> {
    let clear_loop_address = RUNTIME_INITIALIZE_ADDRESS + 0x04;
    let state_loop_address = RUNTIME_INITIALIZE_ADDRESS + 0x23;
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
        Instruction::BneAbsolute(clear_loop_address),
        Instruction::LdaImmediate(0),
        Instruction::LdxImmediate(0),
        Instruction::StaAbsoluteX(RUNTIME_STATE_START),
        Instruction::Inx,
        Instruction::CpxImmediate(RUNTIME_STATE_LEN),
        Instruction::BneAbsolute(state_loop_address),
    ];
    for (index, byte) in RUNTIME_MAGIC.iter().copied().enumerate() {
        instructions.push(Instruction::LdaImmediate(byte));
        instructions.push(Instruction::StaAbsolute(RUNTIME_MAGIC_START + index as u16));
    }
    instructions.push(Instruction::Rts);
    assemble_at(RUNTIME_INITIALIZE_ADDRESS, &instructions)
}
