use anyhow::Result;

use crate::rp2a03::{Instruction, assemble_at};

pub(super) const ROW_PRG_BANK: u8 = 0x0B;
pub(super) const ROW_HOOK_ADDRESS: u16 = 0x93B7;
pub(super) const ROW_HOOK_LEN: usize = 11;
pub(super) const PAGE_ROUTINE_ADDRESS: u16 = 0xFB20;
pub(super) const PAGE_ROUTINE_END: u16 = 0xFB68;
pub(super) const PAGE_A_REGISTER: u8 = 0x88;
pub(super) const PAGE_B_REGISTER: u8 = 0x8C;

pub(super) fn row_calculation() -> Result<Vec<u8>> {
    assemble_at(
        ROW_HOOK_ADDRESS,
        &[
            Instruction::LdyImmediate(4),
            Instruction::LdaIndirectY(0x6E),
            Instruction::Clc,
            Instruction::AdcAbsoluteX(0x93D8),
            Instruction::StaZeroPage(0x34),
            Instruction::Iny,
        ],
    )
}

pub(super) fn row_hook() -> Result<Vec<u8>> {
    let mut instructions = vec![Instruction::JsrAbsolute(PAGE_ROUTINE_ADDRESS)];
    instructions.extend(std::iter::repeat_n(Instruction::Nop, ROW_HOOK_LEN - 3));
    assemble_at(ROW_HOOK_ADDRESS, &instructions)
}

pub(super) fn build_page_routine_with_fallback(
    page_a_register: u8,
    page_b_register: u8,
    fallback_target: u16,
) -> Result<Vec<u8>> {
    const PAGE_B_ADDRESS: u16 = 0xFB55;
    const WRITE_MAPPER_ADDRESS: u16 = 0xFB57;
    const FALLBACK_ADDRESS: u16 = 0xFB63;

    assemble_at(
        PAGE_ROUTINE_ADDRESS,
        &[
            Instruction::LdyImmediate(4),
            Instruction::LdaIndirectY(0x6E),
            Instruction::Clc,
            Instruction::AdcAbsoluteX(0x93D8),
            Instruction::StaZeroPage(0x34),
            Instruction::Iny,
            Instruction::Php,
            Instruction::LdaZeroPage(0x52),
            Instruction::CmpImmediate(0x00),
            Instruction::BneAbsolute(FALLBACK_ADDRESS),
            Instruction::LdaZeroPage(0x59),
            Instruction::CmpImmediate(0x1A),
            Instruction::BneAbsolute(FALLBACK_ADDRESS),
            Instruction::LdaZeroPage(0x5A),
            Instruction::CmpImmediate(0x1A),
            Instruction::BneAbsolute(FALLBACK_ADDRESS),
            Instruction::LdaZeroPage(0x5B),
            Instruction::CmpImmediate(0x00),
            Instruction::BneAbsolute(FALLBACK_ADDRESS),
            Instruction::LdaZeroPage(0x5C),
            Instruction::CmpImmediate(0x15),
            Instruction::BneAbsolute(FALLBACK_ADDRESS),
            Instruction::LdaZeroPage(0x34),
            Instruction::CmpImmediate(0x30),
            Instruction::BeqAbsolute(PAGE_B_ADDRESS),
            Instruction::LdaImmediate(page_a_register),
            Instruction::JmpAbsolute(WRITE_MAPPER_ADDRESS),
            Instruction::LdaImmediate(page_b_register),
            Instruction::Pha,
            Instruction::LdaImmediate(2),
            Instruction::StaAbsolute(0x8000),
            Instruction::Pla,
            Instruction::StaAbsolute(0x8001),
            Instruction::Plp,
            Instruction::Rts,
            Instruction::JsrAbsolute(fallback_target),
            Instruction::Plp,
            Instruction::Rts,
        ],
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn row_hook_preserves_the_original_span_and_calls_the_fixed_routine() {
        let original = row_calculation().unwrap();
        let hook = row_hook().unwrap();

        assert_eq!(
            original,
            [
                0xA0, 0x04, 0xB1, 0x6E, 0x18, 0x7D, 0xD8, 0x93, 0x85, 0x34, 0xC8
            ]
        );
        assert_eq!(hook.len(), original.len());
        assert_eq!(&hook[..3], &[0x20, 0x20, 0xFB]);
        assert!(hook[3..].iter().all(|byte| *byte == 0xEA));
    }

    #[test]
    fn page_routine_fits_its_proven_cave_and_has_a_pair_aware_fallback() {
        let routine = build_page_routine_with_fallback(
            PAGE_A_REGISTER,
            PAGE_B_REGISTER,
            super::super::SELECT_RIGHT_FD_CHR_BANK_FOR_PAIR_ADDRESS,
        )
        .unwrap();

        assert_eq!(routine.len(), 0x48);
        assert_eq!(
            PAGE_ROUTINE_ADDRESS as usize + routine.len(),
            PAGE_ROUTINE_END as usize
        );
        assert!(routine.windows(2).any(|bytes| bytes == [0xA9, 0x88]));
        assert!(routine.windows(2).any(|bytes| bytes == [0xA9, 0x8C]));
        assert!(routine.windows(3).any(|bytes| bytes == [0x20, 0xC0, 0xFA]));
        assert_eq!(&routine[..11], row_calculation().unwrap());
    }

    #[test]
    fn non_options_rows_can_continue_through_another_screen_lifetime_selector() {
        let routine = build_page_routine_with_fallback(
            PAGE_A_REGISTER,
            PAGE_B_REGISTER,
            super::super::roster_page::PAGE_ROUTINE_ADDRESS,
        )
        .unwrap();

        assert_eq!(routine.len(), 0x48);
        assert!(routine.windows(3).any(|bytes| bytes == [0x20, 0x80, 0xFB]));
        assert!(!routine.windows(3).any(|bytes| bytes == [0x20, 0xC0, 0xFA]));
    }
}
