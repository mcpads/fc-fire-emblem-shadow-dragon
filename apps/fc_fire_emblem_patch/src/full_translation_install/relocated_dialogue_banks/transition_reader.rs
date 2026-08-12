use anyhow::Result;

use crate::rp2a03::{Instruction, assemble_at};

pub(super) const TRANSITION_BANK_MARKER: u8 = 0x80;
pub(super) const SOURCE_POINTER_RESOLVER: u16 = 0xE6B2;
const SOURCE_PRG_SELECTOR: u16 = 0xFA20;
pub(super) const SELECTOR_CAVE_START: u16 = 0xF558;
pub(super) const TRANSITION_POINTER_RESOLVER: u16 = SELECTOR_CAVE_START;
pub(super) const TRANSITION_BANK_SELECTOR: u16 = 0xF568;
pub(super) const SELECTOR_CAVE_END_EXCLUSIVE: u16 = 0xF600;

pub(super) struct TransitionReaderRoutines {
    pub(super) pointer_resolver: Vec<u8>,
    pub(super) bank_selector: Vec<u8>,
}

pub(super) fn assemble_transition_reader_routines() -> Result<TransitionReaderRoutines> {
    Ok(TransitionReaderRoutines {
        pointer_resolver: assemble_transition_pointer_resolver()?,
        bank_selector: assemble_transition_bank_selector()?,
    })
}

fn assemble_transition_pointer_resolver() -> Result<Vec<u8>> {
    assemble_at(
        TRANSITION_POINTER_RESOLVER,
        &[
            Instruction::JsrAbsolute(SOURCE_POINTER_RESOLVER),
            Instruction::Php,
            Instruction::Pha,
            Instruction::LdaAbsolute(0x77F2),
            Instruction::OraImmediate(TRANSITION_BANK_MARKER),
            Instruction::StaAbsolute(0x77F2),
            Instruction::Pla,
            Instruction::Plp,
            Instruction::Rts,
        ],
    )
}

fn assemble_transition_bank_selector() -> Result<Vec<u8>> {
    const ROUTE_04: u16 = 0xF583;
    const ROUTE_07: u16 = 0xF588;
    const ROUTE_08: u16 = 0xF58D;
    const ROUTE_0B: u16 = 0xF592;
    const ROUTE_0C: u16 = 0xF597;
    const WRITE_MMC3_PAIR: u16 = 0xF59C;
    let marked = |source_bank| TRANSITION_BANK_MARKER | source_bank;
    assemble_at(
        TRANSITION_BANK_SELECTOR,
        &[
            Instruction::Php,
            Instruction::Pha,
            Instruction::CmpImmediate(marked(0x04)),
            Instruction::BeqAbsolute(ROUTE_04),
            Instruction::CmpImmediate(marked(0x07)),
            Instruction::BeqAbsolute(ROUTE_07),
            Instruction::CmpImmediate(marked(0x08)),
            Instruction::BeqAbsolute(ROUTE_08),
            Instruction::CmpImmediate(marked(0x0B)),
            Instruction::BeqAbsolute(ROUTE_0B),
            Instruction::CmpImmediate(marked(0x0C)),
            Instruction::BeqAbsolute(ROUTE_0C),
            Instruction::Pla,
            Instruction::Plp,
            Instruction::JmpAbsolute(SOURCE_PRG_SELECTOR),
            Instruction::LdaImmediate(0x22),
            Instruction::JmpAbsolute(WRITE_MMC3_PAIR),
            Instruction::LdaImmediate(0x24),
            Instruction::JmpAbsolute(WRITE_MMC3_PAIR),
            Instruction::LdaImmediate(0x26),
            Instruction::JmpAbsolute(WRITE_MMC3_PAIR),
            Instruction::LdaImmediate(0x28),
            Instruction::JmpAbsolute(WRITE_MMC3_PAIR),
            Instruction::LdaImmediate(0x2A),
            Instruction::JmpAbsolute(WRITE_MMC3_PAIR),
            Instruction::Pha,
            Instruction::LdaImmediate(0x06),
            Instruction::StaAbsolute(0x8000),
            Instruction::Pla,
            Instruction::StaAbsolute(0x8001),
            Instruction::Clc,
            Instruction::AdcImmediate(1),
            Instruction::Pha,
            Instruction::LdaImmediate(0x07),
            Instruction::StaAbsolute(0x8000),
            Instruction::Pla,
            Instruction::StaAbsolute(0x8001),
            Instruction::Pla,
            Instruction::Plp,
            Instruction::Rts,
        ],
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn transition_pointer_resolver_fits_before_the_reader_selector() {
        let bytes = assemble_transition_pointer_resolver().unwrap();

        assert_eq!(
            bytes.len(),
            usize::from(TRANSITION_BANK_SELECTOR - SELECTOR_CAVE_START)
        );
    }

    #[test]
    fn transition_reader_selector_preserves_fallback_and_maps_every_mirror() {
        let bytes = assemble_transition_bank_selector().unwrap();

        assert!(bytes.len() <= usize::from(SELECTOR_CAVE_END_EXCLUSIVE - TRANSITION_BANK_SELECTOR));
        for first_page in [0x22, 0x24, 0x26, 0x28, 0x2A] {
            assert!(bytes.windows(2).any(|pair| pair == [0xA9, first_page]));
        }
        assert!(bytes.windows(3).any(|triple| triple == [0x4C, 0x20, 0xFA]));
    }
}
