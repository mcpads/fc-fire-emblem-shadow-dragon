use anyhow::{Result, ensure};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Instruction {
    LdaImmediate(u8),
    LdaZeroPage(u8),
    LdaAbsolute(u16),
    LdaAbsoluteX(u16),
    LdxImmediate(u8),
    LdyImmediate(u8),
    LdyAbsoluteX(u16),
    StaZeroPage(u8),
    StaAbsolute(u16),
    StaAbsoluteX(u16),
    StaIndirectY(u8),
    AslAccumulator,
    LsrAccumulator,
    AndImmediate(u8),
    AdcImmediate(u8),
    AdcZeroPage(u8),
    SbcImmediate(u8),
    CmpImmediate(u8),
    CpxImmediate(u8),
    CpyImmediate(u8),
    IncAbsolute(u16),
    Inx,
    Tax,
    Txa,
    Tay,
    Tya,
    Tsx,
    OraImmediate(u8),
    OraZeroPage(u8),
    Clc,
    Sec,
    Pha,
    Php,
    Pla,
    Plp,
    JmpAbsolute(u16),
    JsrAbsolute(u16),
    BeqAbsolute(u16),
    BccAbsolute(u16),
    BcsAbsolute(u16),
    BneAbsolute(u16),
    Rts,
    Nop,
}

impl Instruction {
    fn encoded_len(self) -> usize {
        match self {
            Self::AslAccumulator
            | Self::LsrAccumulator
            | Self::Pha
            | Self::Php
            | Self::Pla
            | Self::Plp
            | Self::Inx
            | Self::Tax
            | Self::Txa
            | Self::Tay
            | Self::Tya
            | Self::Tsx
            | Self::Clc
            | Self::Sec
            | Self::Rts
            | Self::Nop => 1,
            Self::LdaImmediate(_)
            | Self::LdaZeroPage(_)
            | Self::LdxImmediate(_)
            | Self::LdyImmediate(_)
            | Self::StaZeroPage(_)
            | Self::StaIndirectY(_)
            | Self::AndImmediate(_)
            | Self::AdcImmediate(_)
            | Self::AdcZeroPage(_)
            | Self::SbcImmediate(_)
            | Self::CmpImmediate(_)
            | Self::CpxImmediate(_)
            | Self::CpyImmediate(_)
            | Self::OraImmediate(_)
            | Self::OraZeroPage(_)
            | Self::BeqAbsolute(_)
            | Self::BccAbsolute(_)
            | Self::BcsAbsolute(_)
            | Self::BneAbsolute(_) => 2,
            Self::LdaAbsolute(_)
            | Self::LdaAbsoluteX(_)
            | Self::LdyAbsoluteX(_)
            | Self::StaAbsolute(_)
            | Self::StaAbsoluteX(_)
            | Self::IncAbsolute(_)
            | Self::JmpAbsolute(_)
            | Self::JsrAbsolute(_) => 3,
        }
    }

    fn encode_into(self, pc: u16, output: &mut Vec<u8>) -> Result<()> {
        match self {
            Self::LdaImmediate(value) => output.extend_from_slice(&[0xA9, value]),
            Self::LdaZeroPage(address) => output.extend_from_slice(&[0xA5, address]),
            Self::LdaAbsolute(address) => {
                output.push(0xAD);
                output.extend_from_slice(&address.to_le_bytes());
            }
            Self::LdaAbsoluteX(address) => {
                output.push(0xBD);
                output.extend_from_slice(&address.to_le_bytes());
            }
            Self::LdxImmediate(value) => output.extend_from_slice(&[0xA2, value]),
            Self::LdyImmediate(value) => output.extend_from_slice(&[0xA0, value]),
            Self::LdyAbsoluteX(address) => {
                output.push(0xBC);
                output.extend_from_slice(&address.to_le_bytes());
            }
            Self::StaZeroPage(address) => output.extend_from_slice(&[0x85, address]),
            Self::StaAbsolute(address) => {
                output.push(0x8D);
                output.extend_from_slice(&address.to_le_bytes());
            }
            Self::StaAbsoluteX(address) => {
                output.push(0x9D);
                output.extend_from_slice(&address.to_le_bytes());
            }
            Self::StaIndirectY(address) => output.extend_from_slice(&[0x91, address]),
            Self::AslAccumulator => output.push(0x0A),
            Self::LsrAccumulator => output.push(0x4A),
            Self::AndImmediate(value) => output.extend_from_slice(&[0x29, value]),
            Self::AdcImmediate(value) => output.extend_from_slice(&[0x69, value]),
            Self::AdcZeroPage(address) => output.extend_from_slice(&[0x65, address]),
            Self::SbcImmediate(value) => output.extend_from_slice(&[0xE9, value]),
            Self::CmpImmediate(value) => output.extend_from_slice(&[0xC9, value]),
            Self::CpxImmediate(value) => output.extend_from_slice(&[0xE0, value]),
            Self::CpyImmediate(value) => output.extend_from_slice(&[0xC0, value]),
            Self::IncAbsolute(address) => {
                output.push(0xEE);
                output.extend_from_slice(&address.to_le_bytes());
            }
            Self::Inx => output.push(0xE8),
            Self::Tax => output.push(0xAA),
            Self::Txa => output.push(0x8A),
            Self::Tay => output.push(0xA8),
            Self::Tya => output.push(0x98),
            Self::Tsx => output.push(0xBA),
            Self::OraImmediate(value) => output.extend_from_slice(&[0x09, value]),
            Self::OraZeroPage(address) => output.extend_from_slice(&[0x05, address]),
            Self::Clc => output.push(0x18),
            Self::Sec => output.push(0x38),
            Self::Pha => output.push(0x48),
            Self::Php => output.push(0x08),
            Self::Pla => output.push(0x68),
            Self::Plp => output.push(0x28),
            Self::JmpAbsolute(address) => {
                output.push(0x4C);
                output.extend_from_slice(&address.to_le_bytes());
            }
            Self::JsrAbsolute(address) => {
                output.push(0x20);
                output.extend_from_slice(&address.to_le_bytes());
            }
            Self::BeqAbsolute(target) => encode_relative_branch(0xF0, "BEQ", pc, target, output)?,
            Self::BccAbsolute(target) => encode_relative_branch(0x90, "BCC", pc, target, output)?,
            Self::BcsAbsolute(target) => encode_relative_branch(0xB0, "BCS", pc, target, output)?,
            Self::BneAbsolute(target) => {
                encode_relative_branch(0xD0, "BNE", pc, target, output)?;
            }
            Self::Rts => output.push(0x60),
            Self::Nop => output.push(0xEA),
        }
        Ok(())
    }
}

fn encode_relative_branch(
    opcode: u8,
    mnemonic: &str,
    pc: u16,
    target: u16,
    output: &mut Vec<u8>,
) -> Result<()> {
    let relative = i32::from(target) - (i32::from(pc) + 2);
    ensure!(
        (-128..=127).contains(&relative),
        "{mnemonic} at {pc:04X} cannot reach {target:04X}"
    );
    output.extend_from_slice(&[opcode, relative as i8 as u8]);
    Ok(())
}

pub fn assemble_at(origin: u16, instructions: &[Instruction]) -> Result<Vec<u8>> {
    let encoded_len = instructions
        .iter()
        .try_fold(0_usize, |total, instruction| {
            total
                .checked_add(instruction.encoded_len())
                .ok_or_else(|| anyhow::anyhow!("RP2A03 program length overflow"))
        })?;
    ensure!(
        origin as usize + encoded_len <= 0x1_0000,
        "RP2A03 program at {origin:04X} extends past the CPU address space"
    );

    let mut output = Vec::with_capacity(encoded_len);
    for instruction in instructions {
        let pc = (origin as usize + output.len()) as u16;
        instruction.encode_into(pc, &mut output)?;
    }
    ensure!(
        output.len() == encoded_len,
        "RP2A03 instruction length declaration disagrees with encoding"
    );
    Ok(output)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn encodes_only_the_declared_addressing_forms() {
        let bytes = assemble_at(
            0x8000,
            &[
                Instruction::LdaImmediate(0x9F),
                Instruction::LdaZeroPage(0x5B),
                Instruction::LdaAbsolute(0x67F0),
                Instruction::LdaAbsoluteX(0xFB00),
                Instruction::LdxImmediate(0),
                Instruction::LdyImmediate(0),
                Instruction::LdyAbsoluteX(0x0103),
                Instruction::StaZeroPage(0x29),
                Instruction::StaAbsolute(0x5117),
                Instruction::StaAbsoluteX(0x5C00),
                Instruction::StaIndirectY(0x00),
                Instruction::AslAccumulator,
                Instruction::LsrAccumulator,
                Instruction::AndImmediate(0x3F),
                Instruction::AdcImmediate(0x20),
                Instruction::AdcZeroPage(0x01),
                Instruction::SbcImmediate(0x10),
                Instruction::CmpImmediate(0x18),
                Instruction::CpxImmediate(0x20),
                Instruction::CpyImmediate(2),
                Instruction::IncAbsolute(0x67F4),
                Instruction::Inx,
                Instruction::Tax,
                Instruction::Txa,
                Instruction::Tay,
                Instruction::Tya,
                Instruction::Tsx,
                Instruction::OraImmediate(0x80),
                Instruction::OraZeroPage(0x52),
                Instruction::Clc,
                Instruction::Sec,
                Instruction::Pha,
                Instruction::Php,
                Instruction::Pla,
                Instruction::Plp,
                Instruction::JsrAbsolute(0xFB30),
                Instruction::BeqAbsolute(0x8000),
                Instruction::BccAbsolute(0x8000),
                Instruction::BcsAbsolute(0x8000),
                Instruction::BneAbsolute(0x8000),
                Instruction::JmpAbsolute(0xC075),
                Instruction::Rts,
                Instruction::Nop,
            ],
        )
        .unwrap();

        assert_eq!(
            bytes,
            [
                0xA9, 0x9F, 0xA5, 0x5B, 0xAD, 0xF0, 0x67, 0xBD, 0x00, 0xFB, 0xA2, 0x00, 0xA0, 0x00,
                0xBC, 0x03, 0x01, 0x85, 0x29, 0x8D, 0x17, 0x51, 0x9D, 0x00, 0x5C, 0x91, 0x00, 0x0A,
                0x4A, 0x29, 0x3F, 0x69, 0x20, 0x65, 0x01, 0xE9, 0x10, 0xC9, 0x18, 0xE0, 0x20, 0xC0,
                0x02, 0xEE, 0xF4, 0x67, 0xE8, 0xAA, 0x8A, 0xA8, 0x98, 0xBA, 0x09, 0x80, 0x05, 0x52,
                0x18, 0x38, 0x48, 0x08, 0x68, 0x28, 0x20, 0x30, 0xFB, 0xF0, 0xBD, 0x90, 0xBB, 0xB0,
                0xB9, 0xD0, 0xB7, 0x4C, 0x75, 0xC0, 0x60, 0xEA,
            ]
        );
    }

    #[test]
    fn rejects_a_relative_branch_target_outside_signed_byte_range() {
        for instruction in [
            Instruction::BeqAbsolute(0x8100),
            Instruction::BccAbsolute(0x8100),
            Instruction::BcsAbsolute(0x8100),
            Instruction::BneAbsolute(0x8100),
        ] {
            let error = assemble_at(0x8000, &[instruction]).unwrap_err().to_string();
            assert!(error.contains("cannot reach"));
        }
    }

    #[test]
    fn rejects_a_program_that_crosses_the_cpu_address_space() {
        let error = assemble_at(0xFFFF, &[Instruction::LdaImmediate(0)])
            .unwrap_err()
            .to_string();
        assert!(error.contains("extends past the CPU address space"));
    }
}
