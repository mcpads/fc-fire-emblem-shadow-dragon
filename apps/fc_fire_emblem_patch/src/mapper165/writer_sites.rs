use super::{
    SELECT_CENTRAL_RIGHT_FE_CHR_BANK_ADDRESS, SELECT_LEFT_FD_CHR_BANK_ADDRESS,
    SELECT_LEFT_FE_CHR_BANK_ADDRESS, SELECT_PRG_BANK_ADDRESS, SELECT_RIGHT_FD_CHR_BANK_ADDRESS,
    SELECT_RIGHT_FD_CHR_BANK_FOR_PAIR_ADDRESS, SELECT_RIGHT_FE_CHR_BANK_ADDRESS,
};

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

pub(super) const CENTRAL_CHR_WRITERS: &[CentralChrWriter] = &[
    CentralChrWriter {
        role: "PPU $0000 FD source",
        source_address: 0xC9AE,
        shadow_address: 0x59,
        source_register: 0xB000,
        target_routine: SELECT_LEFT_FD_CHR_BANK_ADDRESS,
    },
    CentralChrWriter {
        role: "PPU $0000 FE source",
        source_address: 0xC9B6,
        shadow_address: 0x5A,
        source_register: 0xC000,
        target_routine: SELECT_LEFT_FE_CHR_BANK_ADDRESS,
    },
    CentralChrWriter {
        role: "PPU $1000 FD source",
        source_address: 0xC9BE,
        shadow_address: 0x5B,
        source_register: 0xD000,
        target_routine: SELECT_RIGHT_FD_CHR_BANK_FOR_PAIR_ADDRESS,
    },
    CentralChrWriter {
        role: "PPU $1000 FE source",
        source_address: 0xC9C6,
        shadow_address: 0x5C,
        source_register: 0xE000,
        target_routine: SELECT_CENTRAL_RIGHT_FE_CHR_BANK_ADDRESS,
    },
];

pub(super) const DIRECT_CHR_WRITERS: &[DirectWriter] = &[
    DirectWriter::switchable(
        "bank 05 left FD initialization",
        0x05,
        0x810E,
        0xB000,
        SELECT_LEFT_FD_CHR_BANK_ADDRESS,
    ),
    DirectWriter::switchable(
        "bank 05 left FE initialization",
        0x05,
        0x8113,
        0xC000,
        SELECT_LEFT_FE_CHR_BANK_ADDRESS,
    ),
    DirectWriter::switchable(
        "bank 05 left FD reset",
        0x05,
        0x85E9,
        0xB000,
        SELECT_LEFT_FD_CHR_BANK_ADDRESS,
    ),
    DirectWriter::switchable(
        "bank 05 left FE reset",
        0x05,
        0x85EC,
        0xC000,
        SELECT_LEFT_FE_CHR_BANK_ADDRESS,
    ),
    DirectWriter::switchable(
        "bank 05 right FD reset",
        0x05,
        0x880E,
        0xD000,
        SELECT_RIGHT_FD_CHR_BANK_ADDRESS,
    ),
    DirectWriter::switchable(
        "bank 05 right FE reset",
        0x05,
        0x8811,
        0xE000,
        SELECT_RIGHT_FE_CHR_BANK_ADDRESS,
    ),
    DirectWriter::switchable(
        "bank 05 alternate left FD reset",
        0x05,
        0x8D25,
        0xB000,
        SELECT_LEFT_FD_CHR_BANK_ADDRESS,
    ),
    DirectWriter::switchable(
        "bank 05 alternate left FE reset",
        0x05,
        0x8D28,
        0xC000,
        SELECT_LEFT_FE_CHR_BANK_ADDRESS,
    ),
    DirectWriter::switchable(
        "bank 07 left FD reset",
        0x07,
        0xAC35,
        0xB000,
        SELECT_LEFT_FD_CHR_BANK_ADDRESS,
    ),
    DirectWriter::switchable(
        "bank 07 left FE reset",
        0x07,
        0xAC38,
        0xC000,
        SELECT_LEFT_FE_CHR_BANK_ADDRESS,
    ),
    DirectWriter::switchable(
        "bank 07 right FD reset",
        0x07,
        0xAC3B,
        0xD000,
        SELECT_RIGHT_FD_CHR_BANK_ADDRESS,
    ),
    DirectWriter::switchable(
        "bank 07 right FE reset",
        0x07,
        0xAC3E,
        0xE000,
        SELECT_RIGHT_FE_CHR_BANK_ADDRESS,
    ),
    DirectWriter::switchable(
        "bank 0B first left FD reset",
        0x0B,
        0x9BF2,
        0xB000,
        SELECT_LEFT_FD_CHR_BANK_ADDRESS,
    ),
    DirectWriter::switchable(
        "bank 0B first left FE reset",
        0x0B,
        0x9BF5,
        0xC000,
        SELECT_LEFT_FE_CHR_BANK_ADDRESS,
    ),
    DirectWriter::switchable(
        "bank 0B first right FD reset",
        0x0B,
        0x9BF8,
        0xD000,
        SELECT_RIGHT_FD_CHR_BANK_ADDRESS,
    ),
    DirectWriter::switchable(
        "bank 0B first right FE reset",
        0x0B,
        0x9BFB,
        0xE000,
        SELECT_RIGHT_FE_CHR_BANK_ADDRESS,
    ),
    DirectWriter::switchable(
        "bank 0B second left FD reset",
        0x0B,
        0x9EAE,
        0xB000,
        SELECT_LEFT_FD_CHR_BANK_ADDRESS,
    ),
    DirectWriter::switchable(
        "bank 0B second left FE reset",
        0x0B,
        0x9EB1,
        0xC000,
        SELECT_LEFT_FE_CHR_BANK_ADDRESS,
    ),
    DirectWriter::switchable(
        "bank 0B second right FD reset",
        0x0B,
        0x9EB4,
        0xD000,
        SELECT_RIGHT_FD_CHR_BANK_ADDRESS,
    ),
    DirectWriter::switchable(
        "bank 0B second right FE reset",
        0x0B,
        0x9EB7,
        0xE000,
        SELECT_RIGHT_FE_CHR_BANK_ADDRESS,
    ),
    DirectWriter::switchable(
        "automatic status right FD source",
        0x0D,
        0x8036,
        0xD000,
        SELECT_RIGHT_FD_CHR_BANK_ADDRESS,
    ),
    DirectWriter::switchable(
        "automatic status right FE source",
        0x0D,
        0x8039,
        0xE000,
        SELECT_RIGHT_FE_CHR_BANK_ADDRESS,
    ),
    DirectWriter::switchable(
        "automatic status left FD source",
        0x0D,
        0x83AB,
        0xB000,
        SELECT_LEFT_FD_CHR_BANK_ADDRESS,
    ),
    DirectWriter::switchable(
        "automatic status left FE source",
        0x0D,
        0x83AE,
        0xC000,
        SELECT_LEFT_FE_CHR_BANK_ADDRESS,
    ),
    DirectWriter::fixed(
        "reset right FD source",
        0xC1B7,
        0xD000,
        SELECT_RIGHT_FD_CHR_BANK_ADDRESS,
    ),
    DirectWriter::fixed(
        "reset right FE source",
        0xC1BA,
        0xE000,
        SELECT_RIGHT_FE_CHR_BANK_ADDRESS,
    ),
    DirectWriter::fixed(
        "NMI right FD source",
        0xC1F2,
        0xD000,
        SELECT_RIGHT_FD_CHR_BANK_ADDRESS,
    ),
    DirectWriter::fixed(
        "NMI right FE source",
        0xC1F7,
        0xE000,
        SELECT_RIGHT_FE_CHR_BANK_ADDRESS,
    ),
    DirectWriter::fixed(
        "screen clear right FD source",
        0xCF28,
        0xD000,
        SELECT_RIGHT_FD_CHR_BANK_ADDRESS,
    ),
    DirectWriter::fixed(
        "screen clear right FE source",
        0xCF2B,
        0xE000,
        SELECT_RIGHT_FE_CHR_BANK_ADDRESS,
    ),
    DirectWriter::fixed(
        "unit data left FD source",
        0xE414,
        0xB000,
        SELECT_LEFT_FD_CHR_BANK_ADDRESS,
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
