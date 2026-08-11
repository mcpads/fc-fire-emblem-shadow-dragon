use anyhow::{Context, Result, ensure};

use crate::rom::Rom;

const PRG_BANK_SIZE: usize = 16 * 1024;

pub(super) fn source_bytes(rom: &Rom, bank: u8, address: u16, byte_count: usize) -> Result<&[u8]> {
    ensure!(
        address >= 0x8000,
        "source address ${address:04X} is below the PRG window"
    );
    let bank_offset = if bank == 0x0F {
        usize::from(
            address
                .checked_sub(0xC000)
                .context("fixed-bank source address is below the fixed CPU window")?,
        )
    } else {
        usize::from(address - 0x8000)
    };
    let start = usize::from(bank)
        .checked_mul(PRG_BANK_SIZE)
        .and_then(|offset| offset.checked_add(bank_offset))
        .context("source PRG offset overflow")?;
    rom.prg()
        .get(start..start + byte_count)
        .with_context(|| format!("source {bank:02X}:${address:04X} is outside PRG"))
}

pub(super) fn prg_bank(rom: &Rom, bank: u8) -> Result<&[u8]> {
    let start = usize::from(bank)
        .checked_mul(PRG_BANK_SIZE)
        .context("source PRG bank offset overflow")?;
    rom.prg()
        .get(start..start + PRG_BANK_SIZE)
        .with_context(|| format!("source PRG bank {bank:02X} is absent"))
}
