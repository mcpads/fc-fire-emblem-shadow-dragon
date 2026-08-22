use anyhow::{Result, ensure};

use crate::rom::{HEADER_SIZE, PRG_SIZE};

const SWITCHABLE_PRG_BANK_SIZE: usize = 0x4000;

pub(crate) const SOURCE_RESET_ADDRESS: u16 = 0xC075;

pub(crate) fn fixed_bank_file_offset(cpu_address: u16) -> Result<usize> {
    ensure!(
        cpu_address >= 0xC000,
        "CPU address {cpu_address:04X} is outside the fixed PRG bank"
    );
    Ok(HEADER_SIZE + (PRG_SIZE - SWITCHABLE_PRG_BANK_SIZE) + (cpu_address as usize - 0xC000))
}

pub(crate) fn switchable_bank_file_offset(prg_bank: u8, cpu_address: u16) -> Result<usize> {
    ensure!(
        cpu_address < 0xC000,
        "CPU address {cpu_address:04X} is outside the switchable PRG window"
    );
    ensure!(
        cpu_address >= 0x8000,
        "CPU address {cpu_address:04X} is below the switchable PRG window"
    );
    let bank_offset = (prg_bank as usize)
        .checked_mul(SWITCHABLE_PRG_BANK_SIZE)
        .ok_or_else(|| anyhow::anyhow!("switchable PRG bank offset overflow"))?;
    ensure!(
        bank_offset < PRG_SIZE,
        "PRG bank {prg_bank:02X} is outside the source image"
    );
    Ok(HEADER_SIZE + bank_offset + (cpu_address as usize - 0x8000))
}

pub(crate) fn count_direct_transfers_to_range(prg: &[u8], start: u16, end: u16) -> Result<usize> {
    ensure!(start < end, "direct transfer target range is empty");
    Ok(prg
        .windows(3)
        .filter(|window| {
            matches!(window[0], 0x20 | 0x4C) && {
                let target = u16::from_le_bytes([window[1], window[2]]);
                (start..end).contains(&target)
            }
        })
        .count())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn source_cpu_addresses_map_to_their_prg_file_offsets() {
        assert_eq!(fixed_bank_file_offset(0xC000).unwrap(), 0x3C010);
        assert_eq!(fixed_bank_file_offset(0xFA00).unwrap(), 0x3FA10);
        assert_eq!(fixed_bank_file_offset(0xFFFC).unwrap(), 0x4000C);
        assert!(fixed_bank_file_offset(0xBFFF).is_err());

        assert_eq!(switchable_bank_file_offset(0x0D, 0x8036).unwrap(), 0x34046);
        assert_eq!(switchable_bank_file_offset(0x0D, 0x83AB).unwrap(), 0x343BB);
        assert!(switchable_bank_file_offset(0, 0x7FFF).is_err());
        assert!(switchable_bank_file_offset(0, 0xC000).is_err());
        assert!(switchable_bank_file_offset(0x10, 0x8000).is_err());
    }

    #[test]
    fn direct_transfer_scan_counts_jsr_and_jmp_targets_only() {
        let mut prg = vec![0xEA; 32];
        prg[0..3].copy_from_slice(&[0x20, 0x10, 0xFB]);
        prg[3..6].copy_from_slice(&[0x4C, 0x7F, 0xFB]);
        prg[6..9].copy_from_slice(&[0x20, 0x80, 0xFB]);
        prg[9..12].copy_from_slice(&[0xAD, 0x10, 0xFB]);

        assert_eq!(
            count_direct_transfers_to_range(&prg, 0xFB00, 0xFB80).unwrap(),
            2
        );
    }
}
