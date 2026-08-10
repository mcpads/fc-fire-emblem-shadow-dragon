use anyhow::Result;

use crate::rp2a03::{Instruction, assemble_at};

use super::{FIRST_EXTENSION_CHR_PAGE, SELECT_RIGHT_FD_CHR_BANK_FOR_PAIR_ADDRESS};

pub(super) const PHYSICAL_CHR_PAGES: [u8; 2] =
    [FIRST_EXTENSION_CHR_PAGE + 2, FIRST_EXTENSION_CHR_PAGE + 3];
pub(super) const PAGE_REGISTERS: [u8; 2] = [0x90, 0x94];
pub(super) const OWNER_CONSTRUCTOR_PRG_BANK: u8 = 0x0B;
pub(super) const OWNER_CONSTRUCTOR_ADDRESS: u16 = 0x89DB;
pub(super) const OWNER_CONSTRUCTOR_SIGNATURE: [u8; 18] = [
    0xA9, 0x12, 0x8D, 0xCF, 0x05, 0xA9, 0x04, 0x8D, 0xD0, 0x05, 0xA9, 0x30, 0x85, 0x70, 0xA9, 0x40,
    0x85, 0x71,
];
pub(super) const HEADER_CALL_ADDRESS: u16 = 0x89F2;
pub(super) const HEADER_RESOURCE_ID: u8 = 0x2B;
pub(super) const CENTRAL_RIGHT_FD_SELECTOR_CALL_ADDRESS: u16 = 0xC9C2;
pub(super) const CENTRAL_RIGHT_FE_COMPANION_FD_REFRESH_CALL_ADDRESS: u16 = 0xFABB;
pub(super) const PAGE_ROUTINE_ADDRESS: u16 = 0xFB80;
pub(super) const PAGE_ROUTINE_END: u16 = 0xFBD4;

pub(super) fn central_right_fd_selector_call(target: u16) -> Result<Vec<u8>> {
    assemble_at(
        CENTRAL_RIGHT_FD_SELECTOR_CALL_ADDRESS,
        &[Instruction::JsrAbsolute(target)],
    )
}

pub(super) fn central_right_fe_companion_fd_refresh_call(target: u16) -> Result<Vec<u8>> {
    assemble_at(
        CENTRAL_RIGHT_FE_COMPANION_FD_REFRESH_CALL_ADDRESS,
        &[Instruction::JsrAbsolute(target)],
    )
}

pub(super) fn build_page_routine(page_a_register: u8, page_b_register: u8) -> Result<Vec<u8>> {
    build_page_routine_with_fallback(
        page_a_register,
        page_b_register,
        SELECT_RIGHT_FD_CHR_BANK_FOR_PAIR_ADDRESS,
    )
}

pub(super) fn build_page_routine_with_fallback(
    page_a_register: u8,
    page_b_register: u8,
    fallback_target: u16,
) -> Result<Vec<u8>> {
    const PAGE_A_ADDRESS: u16 = 0xFBBC;
    const PAGE_B_ADDRESS: u16 = 0xFBC0;
    const WRITE_MAPPER_ADDRESS: u16 = 0xFBC2;
    const FALLBACK_ADDRESS: u16 = 0xFBCF;

    assemble_at(
        PAGE_ROUTINE_ADDRESS,
        &[
            Instruction::Php,
            Instruction::Pha,
            Instruction::LdaAbsolute(0x05CF),
            Instruction::CmpImmediate(0x12),
            Instruction::BneAbsolute(FALLBACK_ADDRESS),
            Instruction::LdaAbsolute(0x05D0),
            Instruction::CmpImmediate(0x04),
            Instruction::BneAbsolute(FALLBACK_ADDRESS),
            Instruction::LdaZeroPage(0x70),
            Instruction::CmpImmediate(0x30),
            Instruction::BneAbsolute(FALLBACK_ADDRESS),
            Instruction::LdaZeroPage(0x71),
            Instruction::CmpImmediate(0x40),
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
            Instruction::BeqAbsolute(PAGE_B_ADDRESS),
            Instruction::CmpImmediate(0x18),
            Instruction::BeqAbsolute(PAGE_A_ADDRESS),
            Instruction::CmpImmediate(0x19),
            Instruction::BneAbsolute(FALLBACK_ADDRESS),
            Instruction::LdaImmediate(page_a_register),
            Instruction::BneAbsolute(WRITE_MAPPER_ADDRESS),
            Instruction::LdaImmediate(page_b_register),
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
            Instruction::JmpAbsolute(fallback_target),
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
    fn companion_fd_refresh_can_enter_the_same_lifetime_selector_chain() {
        assert_eq!(
            central_right_fe_companion_fd_refresh_call(SELECT_RIGHT_FD_CHR_BANK_FOR_PAIR_ADDRESS)
                .unwrap(),
            [0x20, 0xC0, 0xFA]
        );
        assert_eq!(
            central_right_fe_companion_fd_refresh_call(PAGE_ROUTINE_ADDRESS).unwrap(),
            [0x20, 0x80, 0xFB]
        );
    }

    #[test]
    fn page_routine_fits_its_cave_and_falls_back_to_the_pair_selector() {
        let routine = build_page_routine(PAGE_REGISTERS[0], PAGE_REGISTERS[1]).unwrap();

        assert_eq!(routine.len(), 0x54);
        assert_eq!(
            PAGE_ROUTINE_ADDRESS as usize + routine.len(),
            PAGE_ROUTINE_END as usize
        );
        assert!(routine.windows(2).any(|bytes| bytes == [0xA9, 0x90]));
        assert!(routine.windows(2).any(|bytes| bytes == [0xA9, 0x94]));
        assert_eq!(&routine[routine.len() - 3..], &[0x4C, 0xC0, 0xFA]);
    }

    #[test]
    fn fallback_can_chain_to_another_screen_lifetime_selector() {
        let routine =
            build_page_routine_with_fallback(PAGE_REGISTERS[0], PAGE_REGISTERS[1], 0xFBD8).unwrap();

        assert_eq!(&routine[routine.len() - 3..], &[0x4C, 0xD8, 0xFB]);
    }

    #[test]
    fn observed_roster_backing_route_selects_a_b_a_pages() {
        let routine = build_page_routine(PAGE_REGISTERS[0], PAGE_REGISTERS[1]).unwrap();

        assert_eq!(
            &routine[2..28],
            &[
                0xAD, 0xCF, 0x05, 0xC9, 0x12, 0xD0, 0x46, 0xAD, 0xD0, 0x05, 0xC9, 0x04, 0xD0, 0x3F,
                0xA5, 0x70, 0xC9, 0x30, 0xD0, 0x39, 0xA5, 0x71, 0xC9, 0x40, 0xD0, 0x33,
            ]
        );
        assert_eq!(
            &routine[46..66],
            &[
                0xA5, 0x5C, 0xC9, 0x15, 0xF0, 0x0C, 0xC9, 0x18, 0xF0, 0x04, 0xC9, 0x19, 0xD0, 0x13,
                0xA9, 0x90, 0xD0, 0x02, 0xA9, 0x94,
            ]
        );
    }
}
