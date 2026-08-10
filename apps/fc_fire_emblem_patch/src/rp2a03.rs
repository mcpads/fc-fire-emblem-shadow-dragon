//! Patch-domain RP2A03 instructions lowered through the complete typed ISA.

use anyhow::{Context, Result, ensure};
use retro_rp2a03::{AddressingMode, Assembler, Instruction as TypedInstruction, Mnemonic, Operand};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Instruction {
    LdaImmediate(u8),
    LdaZeroPage(u8),
    LdaAbsolute(u16),
    LdaAbsoluteX(u16),
    LdaAbsoluteY(u16),
    LdaIndirectY(u8),
    LdxImmediate(u8),
    LdyImmediate(u8),
    LdyAbsoluteX(u16),
    StaZeroPage(u8),
    StyZeroPage(u8),
    StaAbsolute(u16),
    StaAbsoluteX(u16),
    StaIndirectY(u8),
    AslAccumulator,
    AslZeroPage(u8),
    RolZeroPage(u8),
    LsrAccumulator,
    AndImmediate(u8),
    AdcImmediate(u8),
    AdcZeroPage(u8),
    AdcAbsoluteX(u16),
    SbcImmediate(u8),
    CmpImmediate(u8),
    CpxImmediate(u8),
    CpyImmediate(u8),
    IncAbsolute(u16),
    DecAbsolute(u16),
    Inx,
    Dex,
    Iny,
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
            | Self::Dex
            | Self::Iny
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
            | Self::LdaIndirectY(_)
            | Self::LdxImmediate(_)
            | Self::LdyImmediate(_)
            | Self::StaZeroPage(_)
            | Self::StyZeroPage(_)
            | Self::StaIndirectY(_)
            | Self::AslZeroPage(_)
            | Self::RolZeroPage(_)
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
            | Self::LdaAbsoluteY(_)
            | Self::AdcAbsoluteX(_)
            | Self::LdyAbsoluteX(_)
            | Self::StaAbsolute(_)
            | Self::StaAbsoluteX(_)
            | Self::IncAbsolute(_)
            | Self::DecAbsolute(_)
            | Self::JmpAbsolute(_)
            | Self::JsrAbsolute(_) => 3,
        }
    }

    fn lower(self, pc: u16) -> Result<TypedInstruction> {
        let (mnemonic, mode, operand) = match self {
            Self::LdaImmediate(value) => immediate(Mnemonic::Lda, value),
            Self::LdaZeroPage(address) => zero_page(Mnemonic::Lda, address),
            Self::LdaAbsolute(address) => absolute(Mnemonic::Lda, address),
            Self::LdaAbsoluteX(address) => absolute_x(Mnemonic::Lda, address),
            Self::LdaAbsoluteY(address) => absolute_y(Mnemonic::Lda, address),
            Self::LdaIndirectY(address) => (
                Mnemonic::Lda,
                AddressingMode::ZeroPageIndirectIndexedY,
                Operand::Byte(address),
            ),
            Self::LdxImmediate(value) => immediate(Mnemonic::Ldx, value),
            Self::LdyImmediate(value) => immediate(Mnemonic::Ldy, value),
            Self::LdyAbsoluteX(address) => absolute_x(Mnemonic::Ldy, address),
            Self::StaZeroPage(address) => zero_page(Mnemonic::Sta, address),
            Self::StyZeroPage(address) => zero_page(Mnemonic::Sty, address),
            Self::StaAbsolute(address) => absolute(Mnemonic::Sta, address),
            Self::StaAbsoluteX(address) => absolute_x(Mnemonic::Sta, address),
            Self::StaIndirectY(address) => (
                Mnemonic::Sta,
                AddressingMode::ZeroPageIndirectIndexedY,
                Operand::Byte(address),
            ),
            Self::AslAccumulator => implied(Mnemonic::Asl, AddressingMode::Accumulator),
            Self::AslZeroPage(address) => zero_page(Mnemonic::Asl, address),
            Self::RolZeroPage(address) => zero_page(Mnemonic::Rol, address),
            Self::LsrAccumulator => implied(Mnemonic::Lsr, AddressingMode::Accumulator),
            Self::AndImmediate(value) => immediate(Mnemonic::And, value),
            Self::AdcImmediate(value) => immediate(Mnemonic::Adc, value),
            Self::AdcZeroPage(address) => zero_page(Mnemonic::Adc, address),
            Self::AdcAbsoluteX(address) => absolute_x(Mnemonic::Adc, address),
            Self::SbcImmediate(value) => immediate(Mnemonic::Sbc, value),
            Self::CmpImmediate(value) => immediate(Mnemonic::Cmp, value),
            Self::CpxImmediate(value) => immediate(Mnemonic::Cpx, value),
            Self::CpyImmediate(value) => immediate(Mnemonic::Cpy, value),
            Self::IncAbsolute(address) => absolute(Mnemonic::Inc, address),
            Self::DecAbsolute(address) => absolute(Mnemonic::Dec, address),
            Self::Inx => implied(Mnemonic::Inx, AddressingMode::Implied),
            Self::Dex => implied(Mnemonic::Dex, AddressingMode::Implied),
            Self::Iny => implied(Mnemonic::Iny, AddressingMode::Implied),
            Self::Tax => implied(Mnemonic::Tax, AddressingMode::Implied),
            Self::Txa => implied(Mnemonic::Txa, AddressingMode::Implied),
            Self::Tay => implied(Mnemonic::Tay, AddressingMode::Implied),
            Self::Tya => implied(Mnemonic::Tya, AddressingMode::Implied),
            Self::Tsx => implied(Mnemonic::Tsx, AddressingMode::Implied),
            Self::OraImmediate(value) => immediate(Mnemonic::Ora, value),
            Self::OraZeroPage(address) => zero_page(Mnemonic::Ora, address),
            Self::Clc => implied(Mnemonic::Clc, AddressingMode::Implied),
            Self::Sec => implied(Mnemonic::Sec, AddressingMode::Implied),
            Self::Pha => implied(Mnemonic::Pha, AddressingMode::Implied),
            Self::Php => implied(Mnemonic::Php, AddressingMode::Implied),
            Self::Pla => implied(Mnemonic::Pla, AddressingMode::Implied),
            Self::Plp => implied(Mnemonic::Plp, AddressingMode::Implied),
            Self::JmpAbsolute(address) => absolute(Mnemonic::Jmp, address),
            Self::JsrAbsolute(address) => absolute(Mnemonic::Jsr, address),
            Self::BeqAbsolute(target) => relative(Mnemonic::Beq, "BEQ", pc, target)?,
            Self::BccAbsolute(target) => relative(Mnemonic::Bcc, "BCC", pc, target)?,
            Self::BcsAbsolute(target) => relative(Mnemonic::Bcs, "BCS", pc, target)?,
            Self::BneAbsolute(target) => relative(Mnemonic::Bne, "BNE", pc, target)?,
            Self::Rts => implied(Mnemonic::Rts, AddressingMode::Implied),
            Self::Nop => implied(Mnemonic::Nop, AddressingMode::Implied),
        };
        TypedInstruction::new(mnemonic, mode, operand)
            .with_context(|| format!("cannot lower {self:?} at {pc:04X} to RP2A03"))
    }
}

fn immediate(mnemonic: Mnemonic, value: u8) -> (Mnemonic, AddressingMode, Operand) {
    (mnemonic, AddressingMode::Immediate, Operand::Byte(value))
}

fn zero_page(mnemonic: Mnemonic, address: u8) -> (Mnemonic, AddressingMode, Operand) {
    (mnemonic, AddressingMode::ZeroPage, Operand::Byte(address))
}

fn absolute(mnemonic: Mnemonic, address: u16) -> (Mnemonic, AddressingMode, Operand) {
    (mnemonic, AddressingMode::Absolute, Operand::Word(address))
}

fn absolute_x(mnemonic: Mnemonic, address: u16) -> (Mnemonic, AddressingMode, Operand) {
    (mnemonic, AddressingMode::AbsoluteX, Operand::Word(address))
}

fn absolute_y(mnemonic: Mnemonic, address: u16) -> (Mnemonic, AddressingMode, Operand) {
    (mnemonic, AddressingMode::AbsoluteY, Operand::Word(address))
}

fn implied(mnemonic: Mnemonic, mode: AddressingMode) -> (Mnemonic, AddressingMode, Operand) {
    (mnemonic, mode, Operand::None)
}

fn relative(
    mnemonic: Mnemonic,
    display_name: &str,
    pc: u16,
    target: u16,
) -> Result<(Mnemonic, AddressingMode, Operand)> {
    let relative = i32::from(target) - (i32::from(pc) + 2);
    ensure!(
        (-128..=127).contains(&relative),
        "{display_name} at {pc:04X} cannot reach {target:04X}"
    );
    Ok((
        mnemonic,
        AddressingMode::Relative,
        Operand::Relative(relative as i8),
    ))
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

    let mut assembler = Assembler::new();
    let mut offset = 0_usize;
    for instruction in instructions {
        let pc = (origin as usize + offset) as u16;
        assembler.emit(instruction.lower(pc)?);
        offset += instruction.encoded_len();
    }
    let output = assembler
        .assemble(origin)
        .context("cannot assemble checked RP2A03 program")?
        .into_bytes();
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
                Instruction::LdaAbsoluteY(0xB700),
                Instruction::LdxImmediate(0),
                Instruction::LdyImmediate(0),
                Instruction::LdyAbsoluteX(0x0103),
                Instruction::StaZeroPage(0x29),
                Instruction::StyZeroPage(0x21),
                Instruction::StaAbsolute(0x5117),
                Instruction::StaAbsoluteX(0x5C00),
                Instruction::StaIndirectY(0x00),
                Instruction::AslAccumulator,
                Instruction::AslZeroPage(0x02),
                Instruction::RolZeroPage(0x03),
                Instruction::LsrAccumulator,
                Instruction::AndImmediate(0x3F),
                Instruction::AdcImmediate(0x20),
                Instruction::AdcZeroPage(0x01),
                Instruction::SbcImmediate(0x10),
                Instruction::CmpImmediate(0x18),
                Instruction::CpxImmediate(0x20),
                Instruction::CpyImmediate(2),
                Instruction::IncAbsolute(0x67F4),
                Instruction::DecAbsolute(0x67FC),
                Instruction::Inx,
                Instruction::Dex,
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
                0xA9, 0x9F, 0xA5, 0x5B, 0xAD, 0xF0, 0x67, 0xBD, 0x00, 0xFB, 0xB9, 0x00, 0xB7, 0xA2,
                0x00, 0xA0, 0x00, 0xBC, 0x03, 0x01, 0x85, 0x29, 0x84, 0x21, 0x8D, 0x17, 0x51, 0x9D,
                0x00, 0x5C, 0x91, 0x00, 0x0A, 0x06, 0x02, 0x26, 0x03, 0x4A, 0x29, 0x3F, 0x69, 0x20,
                0x65, 0x01, 0xE9, 0x10, 0xC9, 0x18, 0xE0, 0x20, 0xC0, 0x02, 0xEE, 0xF4, 0x67, 0xCE,
                0xFC, 0x67, 0xE8, 0xCA, 0xAA, 0x8A, 0xA8, 0x98, 0xBA, 0x09, 0x80, 0x05, 0x52, 0x18,
                0x38, 0x48, 0x08, 0x68, 0x28, 0x20, 0x30, 0xFB, 0xF0, 0xB0, 0x90, 0xAE, 0xB0, 0xAC,
                0xD0, 0xAA, 0x4C, 0x75, 0xC0, 0x60, 0xEA,
            ]
        );
    }

    #[test]
    fn encodes_options_row_calculation_addressing_forms() {
        let bytes = assemble_at(
            0x93B7,
            &[
                Instruction::LdaIndirectY(0x6E),
                Instruction::AdcAbsoluteX(0x93D8),
                Instruction::Iny,
            ],
        )
        .unwrap();

        assert_eq!(bytes, [0xB1, 0x6E, 0x7D, 0xD8, 0x93, 0xC8]);
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
