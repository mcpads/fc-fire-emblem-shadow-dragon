mod chr;

pub(super) use chr::{CENTRAL_CHR_WRITERS, DIRECT_CHR_WRITERS};

use super::SELECT_PRG_BANK_ADDRESS;

pub(super) const SOURCE_PRG_BANK_WRITERS: &[DirectWriter] = &[
    DirectWriter::fixed(
        "boot temporary PRG bank selection",
        0xC1FD,
        0xA000,
        SELECT_PRG_BANK_ADDRESS,
    ),
    DirectWriter::fixed(
        "boot PRG bank restoration",
        0xC205,
        0xA000,
        SELECT_PRG_BANK_ADDRESS,
    ),
    DirectWriter::fixed(
        "indirect copy PRG bank selection",
        0xC99F,
        0xA000,
        SELECT_PRG_BANK_ADDRESS,
    ),
    DirectWriter::fixed(
        "unit data PRG bank selection",
        0xE422,
        0xA000,
        SELECT_PRG_BANK_ADDRESS,
    ),
    DirectWriter::fixed(
        "unit data PRG bank restoration",
        0xE43E,
        0xA000,
        SELECT_PRG_BANK_ADDRESS,
    ),
    DirectWriter::fixed(
        "dialogue byte PRG bank selection",
        0xE6A1,
        0xA000,
        SELECT_PRG_BANK_ADDRESS,
    ),
    DirectWriter::fixed(
        "dialogue byte PRG bank restoration",
        0xE6AB,
        0xA000,
        SELECT_PRG_BANK_ADDRESS,
    ),
    DirectWriter::fixed(
        "indexed pointer table PRG bank selection",
        0xE6BA,
        0xA000,
        SELECT_PRG_BANK_ADDRESS,
    ),
    DirectWriter::fixed(
        "indexed pointer table PRG bank restoration",
        0xE6F1,
        0xA000,
        SELECT_PRG_BANK_ADDRESS,
    ),
    DirectWriter::fixed(
        "PPU queue source PRG bank selection",
        0xE71D,
        0xA000,
        SELECT_PRG_BANK_ADDRESS,
    ),
    DirectWriter::fixed(
        "PPU queue source PRG bank restoration",
        0xE736,
        0xA000,
        SELECT_PRG_BANK_ADDRESS,
    ),
];

#[derive(Debug, Clone, Copy)]
pub(super) enum WriterLocation {
    Fixed,
    Switchable { prg_bank: u8 },
}

#[derive(Debug, Clone, Copy)]
pub(super) struct DirectWriter {
    pub(super) role: &'static str,
    pub(super) location: WriterLocation,
    pub(super) source_address: u16,
    pub(super) source_register: u16,
    pub(super) target_routine: u16,
}

impl DirectWriter {
    const fn fixed(
        role: &'static str,
        source_address: u16,
        source_register: u16,
        target_routine: u16,
    ) -> Self {
        Self {
            role,
            location: WriterLocation::Fixed,
            source_address,
            source_register,
            target_routine,
        }
    }

    const fn switchable(
        role: &'static str,
        prg_bank: u8,
        source_address: u16,
        source_register: u16,
        target_routine: u16,
    ) -> Self {
        Self {
            role,
            location: WriterLocation::Switchable { prg_bank },
            source_address,
            source_register,
            target_routine,
        }
    }
}

#[derive(Debug, Clone, Copy)]
pub(super) struct CentralChrWriter {
    pub(super) role: &'static str,
    pub(super) source_address: u16,
    pub(super) shadow_address: u8,
    pub(super) source_register: u16,
    pub(super) target_routine: u16,
}
