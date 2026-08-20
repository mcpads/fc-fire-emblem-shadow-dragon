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
    LdxAbsolute(u16),
    LdxZeroPage(u8),
    LdyImmediate(u8),
    LdyZeroPage(u8),
    LdyAbsolute(u16),
    LdyAbsoluteX(u16),
    StaZeroPage(u8),
    StxZeroPage(u8),
    StyZeroPage(u8),
    StaAbsolute(u16),
    StaAbsoluteX(u16),
    StaAbsoluteY(u16),
    StaIndirectY(u8),
    AslAccumulator,
    AslZeroPage(u8),
    RolZeroPage(u8),
    LsrAccumulator,
    AndImmediate(u8),
    AndZeroPage(u8),
    AdcImmediate(u8),
    AdcZeroPage(u8),
    AdcAbsolute(u16),
    AdcAbsoluteX(u16),
    SbcImmediate(u8),
    SbcAbsolute(u16),
    CmpImmediate(u8),
    CmpAbsolute(u16),
    CmpZeroPage(u8),
    CpxImmediate(u8),
    CpyImmediate(u8),
    IncAbsolute(u16),
    IncAbsoluteX(u16),
    IncZeroPage(u8),
    DecAbsolute(u16),
    Inx,
    Dex,
    Iny,
    Dey,
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
    Rti,
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
            | Self::Dey
            | Self::Tax
            | Self::Txa
            | Self::Tay
            | Self::Tya
            | Self::Tsx
            | Self::Clc
            | Self::Sec
            | Self::Rts
            | Self::Rti
            | Self::Nop => 1,
            Self::LdaImmediate(_)
            | Self::LdaZeroPage(_)
            | Self::LdaIndirectY(_)
            | Self::LdxImmediate(_)
            | Self::LdxZeroPage(_)
            | Self::LdyImmediate(_)
            | Self::LdyZeroPage(_)
            | Self::StaZeroPage(_)
            | Self::StxZeroPage(_)
            | Self::StyZeroPage(_)
            | Self::StaIndirectY(_)
            | Self::AslZeroPage(_)
            | Self::RolZeroPage(_)
            | Self::AndImmediate(_)
            | Self::AndZeroPage(_)
            | Self::AdcImmediate(_)
            | Self::AdcZeroPage(_)
            | Self::SbcImmediate(_)
            | Self::CmpImmediate(_)
            | Self::CmpZeroPage(_)
            | Self::CpxImmediate(_)
            | Self::CpyImmediate(_)
            | Self::OraImmediate(_)
            | Self::OraZeroPage(_)
            | Self::IncZeroPage(_)
            | Self::BeqAbsolute(_)
            | Self::BccAbsolute(_)
            | Self::BcsAbsolute(_)
            | Self::BneAbsolute(_) => 2,
            Self::LdaAbsolute(_)
            | Self::LdaAbsoluteX(_)
            | Self::LdaAbsoluteY(_)
            | Self::AdcAbsolute(_)
            | Self::AdcAbsoluteX(_)
            | Self::LdxAbsolute(_)
            | Self::CmpAbsolute(_)
            | Self::SbcAbsolute(_)
            | Self::LdyAbsolute(_)
            | Self::LdyAbsoluteX(_)
            | Self::StaAbsolute(_)
            | Self::StaAbsoluteX(_)
            | Self::StaAbsoluteY(_)
            | Self::IncAbsolute(_)
            | Self::IncAbsoluteX(_)
            | Self::DecAbsolute(_)
            | Self::JmpAbsolute(_)
            | Self::JsrAbsolute(_) => 3,
        }
    }

    /// 이 명령이 쓸 수 있는 가장 많은 사이클이다.
    ///
    /// vblank 안에서 도는 코드의 예산을 세우는 데만 쓴다. 그래서 페이지 경계를
    /// 넘는 색인 접근과 분기 성립을 전부 «일어난다»로 본다. 실제보다 크게 잡히는
    /// 쪽이라 예산이 낙관적으로 기울지 않는다.
    pub fn worst_case_cycles(self) -> u8 {
        match self {
            Self::AslAccumulator
            | Self::LsrAccumulator
            | Self::Inx
            | Self::Dex
            | Self::Iny
            | Self::Dey
            | Self::Tax
            | Self::Txa
            | Self::Tay
            | Self::Tya
            | Self::Tsx
            | Self::Clc
            | Self::Sec
            | Self::Nop
            | Self::LdaImmediate(_)
            | Self::LdxImmediate(_)
            | Self::LdyImmediate(_)
            | Self::AndImmediate(_)
            | Self::AdcImmediate(_)
            | Self::SbcImmediate(_)
            | Self::CmpImmediate(_)
            | Self::CpxImmediate(_)
            | Self::CpyImmediate(_)
            | Self::OraImmediate(_) => 2,
            Self::Pha | Self::Php => 3,
            Self::LdaZeroPage(_)
            | Self::LdxZeroPage(_)
            | Self::LdyZeroPage(_)
            | Self::StaZeroPage(_)
            | Self::StxZeroPage(_)
            | Self::StyZeroPage(_)
            | Self::AndZeroPage(_)
            | Self::AdcZeroPage(_)
            | Self::CmpZeroPage(_)
            | Self::OraZeroPage(_) => 3,
            Self::Pla | Self::Plp => 4,
            Self::JmpAbsolute(_) => 3,
            Self::LdaAbsolute(_)
            | Self::StaAbsolute(_)
            | Self::AdcAbsolute(_)
            | Self::LdxAbsolute(_)
            | Self::LdyAbsolute(_)
            | Self::CmpAbsolute(_)
            | Self::SbcAbsolute(_) => 4,
            // 색인 적재는 페이지 경계를 넘으면 한 사이클 더 쓴다.
            Self::LdaAbsoluteX(_)
            | Self::LdaAbsoluteY(_)
            | Self::LdyAbsoluteX(_)
            | Self::AdcAbsoluteX(_) => 5,
            Self::StaAbsoluteX(_) | Self::StaAbsoluteY(_) => 5,
            Self::AslZeroPage(_) | Self::RolZeroPage(_) | Self::IncZeroPage(_) => 5,
            Self::LdaIndirectY(_) => 6,
            Self::StaIndirectY(_) => 6,
            Self::IncAbsolute(_)
            | Self::DecAbsolute(_)
            | Self::JsrAbsolute(_)
            | Self::Rts
            | Self::Rti => 6,
            Self::IncAbsoluteX(_) => 7,
            // 분기는 성립하고 페이지도 넘는 경우를 최악으로 본다.
            Self::BeqAbsolute(_)
            | Self::BccAbsolute(_)
            | Self::BcsAbsolute(_)
            | Self::BneAbsolute(_) => 4,
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
            Self::LdxAbsolute(address) => absolute(Mnemonic::Ldx, address),
            Self::LdxZeroPage(address) => zero_page(Mnemonic::Ldx, address),
            Self::LdyImmediate(value) => immediate(Mnemonic::Ldy, value),
            Self::LdyZeroPage(address) => zero_page(Mnemonic::Ldy, address),
            Self::LdyAbsolute(address) => absolute(Mnemonic::Ldy, address),
            Self::LdyAbsoluteX(address) => absolute_x(Mnemonic::Ldy, address),
            Self::StaZeroPage(address) => zero_page(Mnemonic::Sta, address),
            Self::StxZeroPage(address) => zero_page(Mnemonic::Stx, address),
            Self::StyZeroPage(address) => zero_page(Mnemonic::Sty, address),
            Self::StaAbsolute(address) => absolute(Mnemonic::Sta, address),
            Self::StaAbsoluteX(address) => absolute_x(Mnemonic::Sta, address),
            Self::StaAbsoluteY(address) => absolute_y(Mnemonic::Sta, address),
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
            Self::AndZeroPage(address) => zero_page(Mnemonic::And, address),
            Self::AdcImmediate(value) => immediate(Mnemonic::Adc, value),
            Self::AdcZeroPage(address) => zero_page(Mnemonic::Adc, address),
            Self::AdcAbsolute(address) => absolute(Mnemonic::Adc, address),
            Self::AdcAbsoluteX(address) => absolute_x(Mnemonic::Adc, address),
            Self::SbcImmediate(value) => immediate(Mnemonic::Sbc, value),
            Self::SbcAbsolute(address) => absolute(Mnemonic::Sbc, address),
            Self::CmpImmediate(value) => immediate(Mnemonic::Cmp, value),
            Self::CmpAbsolute(address) => absolute(Mnemonic::Cmp, address),
            Self::CmpZeroPage(address) => zero_page(Mnemonic::Cmp, address),
            Self::CpxImmediate(value) => immediate(Mnemonic::Cpx, value),
            Self::CpyImmediate(value) => immediate(Mnemonic::Cpy, value),
            Self::IncAbsolute(address) => absolute(Mnemonic::Inc, address),
            Self::IncAbsoluteX(address) => absolute_x(Mnemonic::Inc, address),
            Self::IncZeroPage(address) => zero_page(Mnemonic::Inc, address),
            Self::DecAbsolute(address) => absolute(Mnemonic::Dec, address),
            Self::Inx => implied(Mnemonic::Inx, AddressingMode::Implied),
            Self::Dex => implied(Mnemonic::Dex, AddressingMode::Implied),
            Self::Iny => implied(Mnemonic::Iny, AddressingMode::Implied),
            Self::Dey => implied(Mnemonic::Dey, AddressingMode::Implied),
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
            Self::Rti => implied(Mnemonic::Rti, AddressingMode::Implied),
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
    fn encodes_absolute_y_load() {
        assert_eq!(
            assemble_at(0x8000, &[Instruction::LdyAbsolute(0x7674)]).unwrap(),
            [0xAC, 0x74, 0x76]
        );
        assert_eq!(Instruction::LdyAbsolute(0x7674).worst_case_cycles(), 4);
    }

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
                Instruction::StaAbsoluteY(0x7FEE),
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
                Instruction::Dey,
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
                0x00, 0x5C, 0x99, 0xEE, 0x7F, 0x91, 0x00, 0x0A, 0x06, 0x02, 0x26, 0x03, 0x4A, 0x29,
                0x3F, 0x69, 0x20, 0x65, 0x01, 0xE9, 0x10, 0xC9, 0x18, 0xE0, 0x20, 0xC0, 0x02, 0xEE,
                0xF4, 0x67, 0xCE, 0xFC, 0x67, 0xE8, 0xCA, 0x88, 0xAA, 0x8A, 0xA8, 0x98, 0xBA, 0x09,
                0x80, 0x05, 0x52, 0x18, 0x38, 0x48, 0x08, 0x68, 0x28, 0x20, 0x30, 0xFB, 0xF0, 0xAC,
                0x90, 0xAA, 0xB0, 0xA8, 0xD0, 0xA6, 0x4C, 0x75, 0xC0, 0x60, 0xEA,
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

    /// 전송은 이번 프레임 몫을 X에 두고 그 값을 제로 페이지로도 옮긴다.
    #[test]
    fn encodes_the_index_store_the_transport_batch_uses() {
        let bytes = assemble_at(0xA000, &[Instruction::StxZeroPage(0x05)]).unwrap();

        assert_eq!(bytes, [0x86, 0x05]);
    }

    /// vblank 예산은 이 값들 위에 세워진다. 실제보다 작게 잡으면 예산이 낙관적으로
    /// 기울어 실기에서 화면이 깨지므로, 조건부 추가 사이클은 전부 «일어난다»로 본다.
    #[test]
    fn worst_case_cycles_never_understate_the_conditional_penalties() {
        for (instruction, cycles) in [
            (Instruction::Nop, 2),
            (Instruction::LdaZeroPage(0x00), 3),
            (Instruction::LdaAbsolute(0x2002), 4),
            (Instruction::StaAbsolute(0x2007), 4),
            // 페이지 경계를 넘는 색인 적재의 최악값이다.
            (Instruction::LdaAbsoluteX(0xB0FF), 5),
            (Instruction::LdaIndirectY(0x00), 6),
            (Instruction::DecAbsolute(0x07F8), 6),
            (Instruction::JsrAbsolute(0xB000), 6),
            (Instruction::Rts, 6),
            // 분기가 성립하고 페이지도 넘는 최악값이다.
            (Instruction::BneAbsolute(0xB000), 4),
        ] {
            assert_eq!(
                instruction.worst_case_cycles(),
                cycles,
                "{instruction:?} worst case"
            );
        }
    }

    #[test]
    fn encodes_dynamic_assignment_zero_page_forms() {
        let bytes = assemble_at(
            0x9000,
            &[
                Instruction::LdxZeroPage(0x06),
                Instruction::AndZeroPage(0x00),
                Instruction::CmpZeroPage(0x07),
                Instruction::IncZeroPage(0x05),
            ],
        )
        .unwrap();

        assert_eq!(bytes, [0xA6, 0x06, 0x25, 0x00, 0xC5, 0x07, 0xE6, 0x05]);
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
