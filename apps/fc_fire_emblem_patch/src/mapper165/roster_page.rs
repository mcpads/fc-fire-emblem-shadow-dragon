use anyhow::Result;

use crate::rp2a03::{Instruction, assemble_at};

use super::{FIRST_EXTENSION_CHR_PAGE, SELECT_RIGHT_FD_CHR_BANK_FOR_PAIR_ADDRESS};

pub(super) const PHYSICAL_CHR_PAGE: u8 = FIRST_EXTENSION_CHR_PAGE + 2;
pub(super) const PAGE_REGISTER: u8 = 0x90;
pub(super) const ALIGNMENT_PADDING_PHYSICAL_CHR_PAGE: u8 = FIRST_EXTENSION_CHR_PAGE + 3;
pub(super) const CENTRAL_RIGHT_FD_SELECTOR_CALL_ADDRESS: u16 = 0xC9C2;
pub(super) const PAGE_ROUTINE_ADDRESS: u16 = 0xFB80;
pub(super) const PAGE_ROUTINE_END: u16 = 0xFBBC;

pub(super) fn central_right_fd_selector_call(target: u16) -> Result<Vec<u8>> {
    assemble_at(
        CENTRAL_RIGHT_FD_SELECTOR_CALL_ADDRESS,
        &[Instruction::JsrAbsolute(target)],
    )
}

pub(super) fn build_page_routine(roster_page_register: u8) -> Result<Vec<u8>> {
    const ROSTER_PAGE_ADDRESS: u16 = 0xFBA8;
    const FALLBACK_ADDRESS: u16 = 0xFBB7;

    assemble_at(
        PAGE_ROUTINE_ADDRESS,
        &[
            Instruction::Php,
            Instruction::Pha,
            Instruction::LdaZeroPage(0x52),
            Instruction::CmpImmediate(0x00),
            Instruction::BneAbsolute(FALLBACK_ADDRESS),
            Instruction::LdaZeroPage(0x59),
            Instruction::CmpImmediate(0x18),
            Instruction::BneAbsolute(FALLBACK_ADDRESS),
            Instruction::LdaZeroPage(0x5A),
            Instruction::CmpImmediate(0x18),
            Instruction::BneAbsolute(FALLBACK_ADDRESS),
            Instruction::LdaZeroPage(0x5B),
            Instruction::CmpImmediate(0x00),
            Instruction::BneAbsolute(FALLBACK_ADDRESS),
            Instruction::LdaZeroPage(0x5C),
            Instruction::CmpImmediate(0x15),
            Instruction::BeqAbsolute(ROSTER_PAGE_ADDRESS),
            Instruction::CmpImmediate(0x18),
            Instruction::BeqAbsolute(ROSTER_PAGE_ADDRESS),
            Instruction::CmpImmediate(0x19),
            Instruction::BneAbsolute(FALLBACK_ADDRESS),
            Instruction::LdaImmediate(roster_page_register),
            Instruction::Pha,
            Instruction::LdaImmediate(2),
            Instruction::StaAbsolute(0x8000),
            Instruction::Pla,
            Instruction::StaAbsolute(0x8001),
            Instruction::Pla,
            Instruction::Plp,
            Instruction::Rts,
            Instruction::Pla,
            Instruction::Plp,
            Instruction::JmpAbsolute(SELECT_RIGHT_FD_CHR_BANK_FOR_PAIR_ADDRESS),
        ],
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn selector_replaces_only_the_existing_pair_aware_call() {
        assert_eq!(
            central_right_fd_selector_call(SELECT_RIGHT_FD_CHR_BANK_FOR_PAIR_ADDRESS).unwrap(),
            [0x20, 0xC0, 0xFA]
        );
        assert_eq!(
            central_right_fd_selector_call(PAGE_ROUTINE_ADDRESS).unwrap(),
            [0x20, 0x80, 0xFB]
        );
    }

    #[test]
    fn page_routine_fits_its_cave_and_falls_back_to_the_pair_selector() {
        let routine = build_page_routine(PAGE_REGISTER).unwrap();

        assert_eq!(routine.len(), 0x3C);
        assert_eq!(
            PAGE_ROUTINE_ADDRESS as usize + routine.len(),
            PAGE_ROUTINE_END as usize
        );
        assert!(routine.windows(2).any(|bytes| bytes == [0xA9, 0x90]));
        assert_eq!(&routine[routine.len() - 3..], &[0x4C, 0xC0, 0xFA]);
    }
}
