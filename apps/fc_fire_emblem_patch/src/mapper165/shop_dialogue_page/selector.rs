use anyhow::{Result, ensure};

use crate::rp2a03::{Instruction, assemble_at};

pub(crate) const PAGE_ROUTINE_ADDRESS: u16 = 0xF748;
pub(crate) const PAGE_ROUTINE_END: u16 = 0xF798;
pub(crate) const PAGE_ROUTINE_CAVE_END: u16 = PAGE_ROUTINE_END;

const WRITE_PAGE_ADDRESS: u16 = 0xF784;
const FALLBACK_ADDRESS: u16 = 0xF793;

pub(crate) fn build_page_selector(mapper_register: u8, fallback_target: u16) -> Result<Vec<u8>> {
    ensure!(
        mapper_register != 0,
        "weapon-shop page register cannot be zero"
    );
    let routine = assemble_at(
        PAGE_ROUTINE_ADDRESS,
        &[
            Instruction::Php,
            Instruction::Pha,
            Instruction::LdaAbsolute(0x05DB),
            Instruction::BeqAbsolute(FALLBACK_ADDRESS),
            Instruction::CmpImmediate(0x0D),
            Instruction::BcsAbsolute(FALLBACK_ADDRESS),
            Instruction::LdaAbsolute(0x77D0),
            Instruction::CmpImmediate(0x01),
            Instruction::BneAbsolute(FALLBACK_ADDRESS),
            Instruction::LdaAbsolute(0x77F2),
            Instruction::CmpImmediate(0x0B),
            Instruction::BneAbsolute(FALLBACK_ADDRESS),
            Instruction::LdaAbsolute(0x77F4),
            Instruction::CmpImmediate(0xB1),
            Instruction::BneAbsolute(FALLBACK_ADDRESS),
            Instruction::LdaZeroPage(0x59),
            Instruction::CmpImmediate(0x1E),
            Instruction::BneAbsolute(FALLBACK_ADDRESS),
            Instruction::LdaZeroPage(0x5A),
            Instruction::CmpImmediate(0x1E),
            Instruction::BneAbsolute(FALLBACK_ADDRESS),
            Instruction::LdaZeroPage(0x5B),
            Instruction::CmpImmediate(0x00),
            Instruction::BneAbsolute(FALLBACK_ADDRESS),
            Instruction::LdaZeroPage(0x5C),
            Instruction::CmpImmediate(0x15),
            Instruction::BeqAbsolute(WRITE_PAGE_ADDRESS),
            Instruction::CmpImmediate(0x18),
            Instruction::BneAbsolute(FALLBACK_ADDRESS),
            Instruction::LdaImmediate(mapper_register),
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
    )?;
    ensure!(
        usize::from(PAGE_ROUTINE_ADDRESS) + routine.len() == usize::from(PAGE_ROUTINE_END),
        "weapon-shop selector ends at 0x{:04X}, expected 0x{PAGE_ROUTINE_END:04X}",
        usize::from(PAGE_ROUTINE_ADDRESS) + routine.len()
    );
    Ok(routine)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn selector_requires_the_complete_weapon_shop_identity() {
        let routine = build_page_selector(0xC0, 0xFC60).unwrap();

        assert_eq!(
            usize::from(PAGE_ROUTINE_ADDRESS) + routine.len(),
            usize::from(PAGE_ROUTINE_END)
        );
        assert!(
            routine
                .windows(5)
                .any(|bytes| bytes == [0xAD, 0xDB, 0x05, 0xF0, 0x44])
        );
        assert!(
            routine
                .windows(5)
                .any(|bytes| bytes == [0xAD, 0xD0, 0x77, 0xC9, 0x01])
        );
        assert!(
            routine
                .windows(5)
                .any(|bytes| bytes == [0xAD, 0xF2, 0x77, 0xC9, 0x0B])
        );
        assert!(
            routine
                .windows(5)
                .any(|bytes| bytes == [0xAD, 0xF4, 0x77, 0xC9, 0xB1])
        );
        assert!(routine.windows(2).any(|bytes| bytes == [0xA9, 0xC0]));
        assert_eq!(&routine[routine.len() - 3..], &[0x4C, 0x60, 0xFC]);
    }
}
