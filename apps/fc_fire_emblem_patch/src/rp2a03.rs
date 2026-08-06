use anyhow::{Result, ensure};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Instruction {
    LdaImmediate(u8),
    StaZeroPage(u8),
    StaAbsolute(u16),
    AslAccumulator,
    OraImmediate(u8),
    Pha,
    Php,
    Pla,
    Plp,
    JmpAbsolute(u16),
    JsrAbsolute(u16),
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
            | Self::Rts
            | Self::Nop => 1,
            Self::LdaImmediate(_) | Self::StaZeroPage(_) | Self::OraImmediate(_) => 2,
            Self::StaAbsolute(_) | Self::JmpAbsolute(_) | Self::JsrAbsolute(_) => 3,
        }
    }

    fn encode_into(self, output: &mut Vec<u8>) {
        match self {
            Self::LdaImmediate(value) => output.extend_from_slice(&[0xA9, value]),
            Self::StaZeroPage(address) => output.extend_from_slice(&[0x85, address]),
            Self::StaAbsolute(address) => {
                output.push(0x8D);
                output.extend_from_slice(&address.to_le_bytes());
            }
            Self::AslAccumulator => output.push(0x0A),
            Self::OraImmediate(value) => output.extend_from_slice(&[0x09, value]),
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
            Self::Rts => output.push(0x60),
            Self::Nop => output.push(0xEA),
        }
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
        instruction.encode_into(&mut output);
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
                Instruction::StaZeroPage(0x29),
                Instruction::StaAbsolute(0x5117),
                Instruction::AslAccumulator,
                Instruction::OraImmediate(0x80),
                Instruction::Pha,
                Instruction::Php,
                Instruction::Pla,
                Instruction::Plp,
                Instruction::JsrAbsolute(0xFB30),
                Instruction::JmpAbsolute(0xC075),
                Instruction::Rts,
                Instruction::Nop,
            ],
        )
        .unwrap();

        assert_eq!(
            bytes,
            [
                0xA9, 0x9F, 0x85, 0x29, 0x8D, 0x17, 0x51, 0x0A, 0x09, 0x80, 0x48, 0x08, 0x68, 0x28,
                0x20, 0x30, 0xFB, 0x4C, 0x75, 0xC0, 0x60, 0xEA,
            ]
        );
    }

    #[test]
    fn rejects_a_program_that_crosses_the_cpu_address_space() {
        let error = assemble_at(0xFFFF, &[Instruction::LdaImmediate(0)])
            .unwrap_err()
            .to_string();
        assert!(error.contains("extends past the CPU address space"));
    }
}
