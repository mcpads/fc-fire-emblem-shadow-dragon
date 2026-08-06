use serde::Serialize;

use crate::rom::{HEADER_SIZE, PRG_SIZE};

const PRG_BANK_SIZE: usize = 16 * 1024;

#[derive(Debug, PartialEq, Eq, Serialize)]
pub(crate) struct AbsoluteTransferCandidate {
    pub(crate) prg_bank: usize,
    pub(crate) prg_bank_hex: String,
    pub(crate) prg_offset: usize,
    pub(crate) prg_offset_hex: String,
    pub(crate) file_offset: usize,
    pub(crate) file_offset_hex: String,
    pub(crate) cpu_address: u16,
    pub(crate) cpu_address_hex: String,
}

#[derive(Debug, PartialEq, Eq, Serialize)]
pub(crate) struct AbsoluteWriteCandidate {
    pub(crate) opcode: u8,
    pub(crate) opcode_hex: String,
    pub(crate) mnemonic: &'static str,
    pub(crate) prg_bank: usize,
    pub(crate) prg_bank_hex: String,
    pub(crate) prg_offset: usize,
    pub(crate) prg_offset_hex: String,
    pub(crate) file_offset: usize,
    pub(crate) file_offset_hex: String,
    pub(crate) cpu_address: u16,
    pub(crate) cpu_address_hex: String,
}

pub(crate) fn find_absolute_transfer_candidates(
    prg: &[u8],
    target: u16,
    opcode: u8,
) -> Vec<AbsoluteTransferCandidate> {
    let [target_low, target_high] = target.to_le_bytes();
    prg.windows(3)
        .enumerate()
        .filter(|(_, bytes)| bytes == &[opcode, target_low, target_high])
        .map(|(prg_offset, _)| {
            let prg_bank = prg_offset / PRG_BANK_SIZE;
            let offset_in_bank = prg_offset % PRG_BANK_SIZE;
            let cpu_base = if prg_bank == PRG_SIZE / PRG_BANK_SIZE - 1 {
                0xC000
            } else {
                0x8000
            };
            let cpu_address = cpu_base + offset_in_bank as u16;
            let file_offset = HEADER_SIZE + prg_offset;
            AbsoluteTransferCandidate {
                prg_bank,
                prg_bank_hex: format!("0x{prg_bank:02X}"),
                prg_offset,
                prg_offset_hex: format!("0x{prg_offset:05X}"),
                file_offset,
                file_offset_hex: format!("0x{file_offset:05X}"),
                cpu_address,
                cpu_address_hex: format!("0x{cpu_address:04X}"),
            }
        })
        .collect()
}

pub(crate) fn find_absolute_write_candidates(
    prg: &[u8],
    target: u16,
) -> Vec<AbsoluteWriteCandidate> {
    let [target_low, target_high] = target.to_le_bytes();
    [(0x8D, "sta"), (0x8E, "stx"), (0x8C, "sty")]
        .into_iter()
        .flat_map(|(opcode, mnemonic)| {
            prg.windows(3)
                .enumerate()
                .filter(move |(_, bytes)| bytes == &[opcode, target_low, target_high])
                .map(move |(prg_offset, _)| {
                    let prg_bank = prg_offset / PRG_BANK_SIZE;
                    let offset_in_bank = prg_offset % PRG_BANK_SIZE;
                    let cpu_base = if prg_bank == PRG_SIZE / PRG_BANK_SIZE - 1 {
                        0xC000
                    } else {
                        0x8000
                    };
                    let cpu_address = cpu_base + offset_in_bank as u16;
                    let file_offset = HEADER_SIZE + prg_offset;
                    AbsoluteWriteCandidate {
                        opcode,
                        opcode_hex: format!("{opcode:02X}"),
                        mnemonic,
                        prg_bank,
                        prg_bank_hex: format!("0x{prg_bank:02X}"),
                        prg_offset,
                        prg_offset_hex: format!("0x{prg_offset:05X}"),
                        file_offset,
                        file_offset_hex: format!("0x{file_offset:05X}"),
                        cpu_address,
                        cpu_address_hex: format!("0x{cpu_address:04X}"),
                    }
                })
        })
        .collect()
}
