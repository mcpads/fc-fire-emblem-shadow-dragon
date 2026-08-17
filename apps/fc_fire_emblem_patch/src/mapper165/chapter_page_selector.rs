use anyhow::{Context, Result, ensure};

use crate::rp2a03::{Instruction, assemble_at};

const MAPPER_REGISTER_STRIDE: u8 = 8;
const MISMATCH_OFFSET: u16 = 0x47;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) struct ChapterPageSequence {
    pub(super) admitted_chapter_count: u8,
    pub(super) first_mapper_register: u8,
}

pub(super) fn build_chapter_page_selector(
    routine_address: u16,
    sequence: ChapterPageSequence,
    fallback_target: u16,
) -> Result<Vec<u8>> {
    ensure!(
        sequence.admitted_chapter_count != 0,
        "chapter page selector has no admitted chapters"
    );
    ensure!(
        sequence.first_mapper_register != 0,
        "chapter page selector cannot use mapper register zero"
    );
    let last_chapter_index = sequence.admitted_chapter_count - 1;
    let last_register_offset = last_chapter_index
        .checked_mul(MAPPER_REGISTER_STRIDE)
        .context("chapter page selector register offset overflow")?;
    sequence
        .first_mapper_register
        .checked_add(last_register_offset)
        .context("chapter page selector register range overflow")?;

    let mismatch_address = routine_address
        .checked_add(MISMATCH_OFFSET)
        .context("chapter page mismatch address overflow")?;
    assemble_at(
        routine_address,
        &[
            Instruction::Php,
            Instruction::Pha,
            Instruction::LdaZeroPage(0x24),
            Instruction::CmpImmediate(0x0B),
            Instruction::BneAbsolute(mismatch_address),
            Instruction::LdaZeroPage(0x84),
            Instruction::CmpImmediate(0x00),
            Instruction::BneAbsolute(mismatch_address),
            Instruction::LdaZeroPage(0x59),
            Instruction::CmpImmediate(0x1A),
            Instruction::BneAbsolute(mismatch_address),
            Instruction::LdaZeroPage(0x5A),
            Instruction::CmpImmediate(0x1A),
            Instruction::BneAbsolute(mismatch_address),
            Instruction::LdaZeroPage(0x5B),
            Instruction::CmpImmediate(0x00),
            Instruction::BneAbsolute(mismatch_address),
            Instruction::LdaZeroPage(0x5C),
            Instruction::CmpImmediate(0x18),
            Instruction::BneAbsolute(mismatch_address),
            Instruction::LdaAbsolute(0x77F7),
            Instruction::CmpImmediate(0x03),
            Instruction::BneAbsolute(mismatch_address),
            Instruction::LdaAbsolute(0x781D),
            Instruction::CmpImmediate(sequence.admitted_chapter_count),
            Instruction::BcsAbsolute(mismatch_address),
            Instruction::AslAccumulator,
            Instruction::AslAccumulator,
            Instruction::AslAccumulator,
            Instruction::Clc,
            Instruction::AdcImmediate(sequence.first_mapper_register),
            Instruction::Pha,
            Instruction::LdaImmediate(2),
            crate::mapper165::selector_safety::select_register_instruction(),
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
    fn selector_computes_contiguous_chapter_page_registers() {
        let selector = build_chapter_page_selector(
            0xFBD4,
            ChapterPageSequence {
                admitted_chapter_count: 2,
                first_mapper_register: 0x98,
            },
            0xFAC0,
        )
        .unwrap();

        assert_eq!(selector.len(), 0x4C);
        assert!(selector.windows(12).any(|bytes| bytes
            == [
                0xAD, 0x1D, 0x78, 0xC9, 0x02, 0xB0, 0x13, 0x0A, 0x0A, 0x0A, 0x18, 0x69
            ]));
        assert!(selector.windows(2).any(|bytes| bytes == [0x69, 0x98]));
        assert_eq!(&selector[selector.len() - 3..], &[0x4C, 0xC0, 0xFA]);
    }

    #[test]
    fn maximum_non_overflowing_sequence_keeps_the_same_layout() {
        let selector = build_chapter_page_selector(
            0xFBD4,
            ChapterPageSequence {
                admitted_chapter_count: 13,
                first_mapper_register: 0x98,
            },
            0xFAC0,
        )
        .unwrap();

        assert_eq!(selector.len(), 0x4C);
        assert!(selector.windows(2).any(|bytes| bytes == [0xC9, 0x0D]));
    }

    #[test]
    fn register_range_must_fit_one_byte() {
        let error = build_chapter_page_selector(
            0xFBD4,
            ChapterPageSequence {
                admitted_chapter_count: 14,
                first_mapper_register: 0x98,
            },
            0xFAC0,
        )
        .unwrap_err()
        .to_string();

        assert!(error.contains("register range overflow"));
    }
}
