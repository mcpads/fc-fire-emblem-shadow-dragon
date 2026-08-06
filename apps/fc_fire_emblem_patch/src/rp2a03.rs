use anyhow::{Result, ensure};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Instruction {
    LdaImmediate(u8),
    LdaZeroPage(u8),
    LdaAbsoluteX(u16),
    LdxImmediate(u8),
    StaZeroPage(u8),
    StaAbsolute(u16),
    StaAbsoluteX(u16),
    AslAccumulator,
    CmpImmediate(u8),
    Inx,
    Tax,
    Txa,
    OraImmediate(u8),
    OraZeroPage(u8),
    Pha,
    Php,
    Pla,
    Plp,
    JmpAbsolute(u16),
    JsrAbsolute(u16),
    BneAbsolute(u16),
    Rts,
    Nop,
}

impl Instruction {
    fn encoded_len(self) -> usize {
        match self {
            Self::AslAccumulator
            | Self::Pha
            | Self::Php
            | Self::Pla
            | Self::Plp
            | Self::Inx
            | Self::Tax
            | Self::Txa
            | Self::Rts
            | Self::Nop => 1,
            Self::LdaImmediate(_)
            | Self::LdaZeroPage(_)
            | Self::LdxImmediate(_)
            | Self::StaZeroPage(_)
            | Self::CmpImmediate(_)
            | Self::OraImmediate(_)
            | Self::OraZeroPage(_)
            | Self::BneAbsolute(_) => 2,
            Self::LdaAbsoluteX(_)
            | Self::StaAbsolute(_)
            | Self::StaAbsoluteX(_)
            | Self::JmpAbsolute(_)
            | Self::JsrAbsolute(_) => 3,
        }
    }

    fn encode_into(self, pc: u16, output: &mut Vec<u8>) -> Result<()> {
        match self {
            Self::LdaImmediate(value) => output.extend_from_slice(&[0xA9, value]),
            Self::LdaZeroPage(address) => output.extend_from_slice(&[0xA5, address]),
            Self::LdaAbsoluteX(address) => {
                output.push(0xBD);
                output.extend_from_slice(&address.to_le_bytes());
            }
            Self::LdxImmediate(value) => output.extend_from_slice(&[0xA2, value]),
            Self::StaZeroPage(address) => output.extend_from_slice(&[0x85, address]),
            Self::StaAbsolute(address) => {
                output.push(0x8D);
                output.extend_from_slice(&address.to_le_bytes());
            }
            Self::StaAbsoluteX(address) => {
                output.push(0x9D);
                output.extend_from_slice(&address.to_le_bytes());
            }
            Self::AslAccumulator => output.push(0x0A),
            Self::CmpImmediate(value) => output.extend_from_slice(&[0xC9, value]),
            Self::Inx => output.push(0xE8),
            Self::Tax => output.push(0xAA),
            Self::Txa => output.push(0x8A),
            Self::OraImmediate(value) => output.extend_from_slice(&[0x09, value]),
            Self::OraZeroPage(address) => output.extend_from_slice(&[0x05, address]),
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
            Self::BneAbsolute(target) => {
                let relative = i32::from(target) - (i32::from(pc) + 2);
                ensure!(
                    (-128..=127).contains(&relative),
                    "BNE at {pc:04X} cannot reach {target:04X}"
                );
                output.extend_from_slice(&[0xD0, relative as i8 as u8]);
            }
            Self::Rts => output.push(0x60),
            Self::Nop => output.push(0xEA),
        }
        Ok(())
    }
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
                Instruction::LdaAbsoluteX(0xFB00),
                Instruction::LdxImmediate(0),
                Instruction::StaZeroPage(0x29),
                Instruction::StaAbsolute(0x5117),
                Instruction::StaAbsoluteX(0x5C00),
                Instruction::AslAccumulator,
                Instruction::CmpImmediate(0x18),
                Instruction::Inx,
                Instruction::Tax,
                Instruction::Txa,
                Instruction::OraImmediate(0x80),
                Instruction::OraZeroPage(0x52),
                Instruction::Pha,
                Instruction::Php,
                Instruction::Pla,
                Instruction::Plp,
                Instruction::JsrAbsolute(0xFB30),
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
                0xA9, 0x9F, 0xA5, 0x5B, 0xBD, 0x00, 0xFB, 0xA2, 0x00, 0x85, 0x29, 0x8D, 0x17, 0x51,
                0x9D, 0x00, 0x5C, 0x0A, 0xC9, 0x18, 0xE8, 0xAA, 0x8A, 0x09, 0x80, 0x05, 0x52, 0x48,
                0x08, 0x68, 0x28, 0x20, 0x30, 0xFB, 0xD0, 0xDC, 0x4C, 0x75, 0xC0, 0x60, 0xEA,
            ]
        );
    }

    #[test]
    fn rejects_a_relative_branch_target_outside_signed_byte_range() {
        let error = assemble_at(0x8000, &[Instruction::BneAbsolute(0x8100)])
            .unwrap_err()
            .to_string();

        assert!(error.contains("cannot reach"));
    }

    #[test]
    fn rejects_a_program_that_crosses_the_cpu_address_space() {
        let error = assemble_at(0xFFFF, &[Instruction::LdaImmediate(0)])
            .unwrap_err()
            .to_string();
        assert!(error.contains("extends past the CPU address space"));
    }
}
