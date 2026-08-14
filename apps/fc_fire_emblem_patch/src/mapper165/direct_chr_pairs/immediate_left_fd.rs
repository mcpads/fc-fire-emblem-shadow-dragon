use anyhow::{Context, Result, ensure};

use crate::{
    rom::Rom,
    rp2a03::{Instruction, assemble_at},
};

use super::{PairLocation, switchable, verify_bytes};
use crate::mapper165::{
    SELECT_LEFT_FD_CHR_BANK_ADDRESS,
    writer_sites::{DIRECT_CHR_WRITERS, WriterLocation},
};

const TERRAIN_PREVIEW_ROLE: &str = "terrain preview sprite page $16 selection";
const EFFECT_PAGE_08_ROLE: &str = "battle effect-object sprite page $08 selection";
const EFFECT_PAGE_09_ROLE: &str = "battle effect-object sprite page $09 selection";
const EFFECT_PAGE_0C_ROLE: &str = "battle effect-object sprite page $0C selection";
const EFFECT_PAGE_0D_ROLE: &str = "battle effect-object sprite page $0D selection";
const EFFECT_PAGE_0E_ROLE: &str = "battle effect-object sprite page $0E selection";
const EFFECT_PAGE_17_ROLE: &str = "battle effect-object sprite page $17 selection";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) struct ImmediateLeftFdWriter {
    pub(super) role: &'static str,
    pub(super) writer_address: u16,
    pub(super) source_page: u8,
}

pub(super) const LOCATION: PairLocation = switchable(0x05);
pub(super) const REGISTER: u16 = 0xB000;
pub(super) const WRITERS: &[ImmediateLeftFdWriter] = &[
    terrain_preview_writer(0x962F),
    effect_object_writer(0xA2F8, 0x08),
    effect_object_writer(0xA382, 0x08),
    effect_object_writer(0xA3D6, 0x09),
    effect_object_writer(0xA413, 0x17),
    effect_object_writer(0xA527, 0x17),
    effect_object_writer(0xA584, 0x17),
    effect_object_writer(0xA642, 0x17),
    effect_object_writer(0xA6DE, 0x17),
    effect_object_writer(0xA77E, 0x17),
    effect_object_writer(0xA807, 0x17),
    effect_object_writer(0xA86E, 0x17),
    effect_object_writer(0xA890, 0x17),
    effect_object_writer(0xA911, 0x0C),
    effect_object_writer(0xA962, 0x0C),
    effect_object_writer(0xA9A1, 0x0C),
    effect_object_writer(0xA9F1, 0x0C),
    effect_object_writer(0xAA64, 0x0D),
    effect_object_writer(0xAAD1, 0x0E),
    effect_object_writer(0xAB16, 0x17),
    effect_object_writer(0xAB69, 0x17),
    effect_object_writer(0xACB6, 0x17),
];

const fn terrain_preview_writer(writer_address: u16) -> ImmediateLeftFdWriter {
    ImmediateLeftFdWriter {
        role: TERRAIN_PREVIEW_ROLE,
        writer_address,
        source_page: 0x16,
    }
}

const fn effect_object_writer(writer_address: u16, source_page: u8) -> ImmediateLeftFdWriter {
    let role = match source_page {
        0x08 => EFFECT_PAGE_08_ROLE,
        0x09 => EFFECT_PAGE_09_ROLE,
        0x0C => EFFECT_PAGE_0C_ROLE,
        0x0D => EFFECT_PAGE_0D_ROLE,
        0x0E => EFFECT_PAGE_0E_ROLE,
        0x17 => EFFECT_PAGE_17_ROLE,
        _ => panic!("unsupported immediate effect-object source page"),
    };
    ImmediateLeftFdWriter {
        role,
        writer_address,
        source_page,
    }
}

pub(super) fn verify_source_sequences(source_rom: &Rom) -> Result<()> {
    for writer in WRITERS {
        let start = writer.writer_address.checked_sub(2).with_context(|| {
            format!(
                "{} writer address has no room for LDA immediate",
                writer.role
            )
        })?;
        let expected = assemble_at(
            start,
            &[
                Instruction::LdaImmediate(writer.source_page),
                Instruction::StaAbsolute(REGISTER),
            ],
        )?;
        verify_bytes(source_rom, LOCATION, start, &expected, writer.role)?;
    }
    Ok(())
}

pub(super) fn verify_inventory_inclusion() -> Result<()> {
    for expected in WRITERS {
        let matches = DIRECT_CHR_WRITERS
            .iter()
            .filter(|actual| {
                matches!(
                    actual.location,
                    WriterLocation::Switchable { prg_bank: 0x05 }
                ) && actual.source_address == expected.writer_address
            })
            .collect::<Vec<_>>();
        ensure!(
            matches.len() == 1,
            "{} must have exactly one direct CHR writer at bank 05:${:04X}",
            expected.role,
            expected.writer_address
        );
        let actual = matches[0];
        ensure!(
            actual.role == expected.role,
            "direct CHR writer role changed at bank 05:${:04X}",
            expected.writer_address
        );
        ensure!(
            actual.source_register == REGISTER,
            "{} no longer writes the left FD source register",
            expected.role
        );
        ensure!(
            actual.target_routine == SELECT_LEFT_FD_CHR_BANK_ADDRESS,
            "{} no longer redirects to the left FD selector",
            expected.role
        );
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn writers_are_included_once_with_their_semantic_roles_and_left_fd_redirect() {
        verify_inventory_inclusion().unwrap();
    }
}
