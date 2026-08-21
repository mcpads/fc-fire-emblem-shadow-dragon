use std::collections::BTreeSet;

use anyhow::{Context, Result, ensure};
use retro_rp2a03::{Location, MemoryAddress, Rp2A03, decode_bytes};
use typed_isa_core::{AccessKind, StaticSemantics};

const PRG_BANK_BYTE_COUNT: usize = 16 * 1024;

#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub(crate) struct DirectMemoryWriter {
    pub(crate) physical_prg_bank: u8,
    pub(crate) cpu_address: u16,
    pub(crate) opcode: u8,
    pub(crate) target_address: u16,
}

impl DirectMemoryWriter {
    pub(crate) const fn new(
        physical_prg_bank: u8,
        cpu_address: u16,
        opcode: u8,
        target_address: u16,
    ) -> Self {
        Self {
            physical_prg_bank,
            cpu_address,
            opcode,
            target_address,
        }
    }
}

pub(crate) fn scan_direct_memory_writers(
    prg: &[u8],
    target_addresses: &[u16],
) -> Result<BTreeSet<DirectMemoryWriter>> {
    ensure!(
        prg.len().is_multiple_of(PRG_BANK_BYTE_COUNT) && prg.len() >= 2 * PRG_BANK_BYTE_COUNT,
        "direct writer census requires complete 16 KiB PRG banks"
    );
    let bank_count = prg.len() / PRG_BANK_BYTE_COUNT;
    ensure!(
        bank_count <= usize::from(u8::MAX) + 1,
        "direct writer census has too many physical PRG banks"
    );
    ensure!(
        !target_addresses.is_empty()
            && target_addresses
                .iter()
                .copied()
                .collect::<BTreeSet<_>>()
                .len()
                == target_addresses.len(),
        "direct writer census needs unique target addresses"
    );
    let target_addresses = target_addresses.iter().copied().collect::<BTreeSet<_>>();
    let fixed = &prg[(bank_count - 1) * PRG_BANK_BYTE_COUNT..];
    let mut writers = BTreeSet::new();

    for physical_bank in 0..bank_count {
        let bank =
            &prg[physical_bank * PRG_BANK_BYTE_COUNT..(physical_bank + 1) * PRG_BANK_BYTE_COUNT];
        let cpu_start = if physical_bank == bank_count - 1 {
            0xC000
        } else {
            0x8000
        };
        for (offset, window) in bank.windows(3).enumerate() {
            record_direct_memory_writer(
                &mut writers,
                &target_addresses,
                u8::try_from(physical_bank)?,
                cpu_start + u16::try_from(offset)?,
                window,
            )?;
        }
        if physical_bank < bank_count - 1 {
            let bffe = [bank[0x3FFE], bank[0x3FFF], fixed[0]];
            let bfff = [bank[0x3FFF], fixed[0], fixed[1]];
            record_direct_memory_writer(
                &mut writers,
                &target_addresses,
                u8::try_from(physical_bank)?,
                0xBFFE,
                &bffe,
            )?;
            record_direct_memory_writer(
                &mut writers,
                &target_addresses,
                u8::try_from(physical_bank)?,
                0xBFFF,
                &bfff,
            )?;
        }
    }

    for target_address in target_addresses {
        ensure!(
            !completion_may_write_address(&[fixed[0x3FFE], fixed[0x3FFF]], target_address)
                && !completion_may_write_address(&[fixed[0x3FFF]], target_address),
            "fixed-bank terminal instruction may write ${target_address:04X} through runtime RAM operand bytes"
        );
    }
    Ok(writers)
}

fn record_direct_memory_writer(
    writers: &mut BTreeSet<DirectMemoryWriter>,
    target_addresses: &BTreeSet<u16>,
    physical_prg_bank: u8,
    cpu_address: u16,
    bytes: &[u8],
) -> Result<()> {
    let Ok(instruction) = decode_bytes(bytes) else {
        return Ok(());
    };
    let semantics =
        Rp2A03::semantics(&instruction, &cpu_address).context("derive direct writer semantics")?;
    for access in semantics.location_accesses {
        let Location::Memory(MemoryAddress::Direct(target_address)) = access.location else {
            continue;
        };
        if access.kind == AccessKind::Write && target_addresses.contains(&target_address) {
            writers.insert(DirectMemoryWriter::new(
                physical_prg_bank,
                cpu_address,
                bytes[0],
                target_address,
            ));
        }
    }
    Ok(())
}

fn completion_may_write_address(available: &[u8], target_address: u16) -> bool {
    let mut bytes = available.to_vec();
    bytes.extend_from_slice(&target_address.to_le_bytes()[available.len().saturating_sub(1)..]);
    bytes.truncate(3);
    if bytes.len() != 3 {
        return true;
    }
    let Ok(instruction) = decode_bytes(&bytes) else {
        return false;
    };
    Rp2A03::semantics(&instruction, &0_u16).is_ok_and(|semantics| {
        semantics.location_accesses.into_iter().any(|access| {
            access.kind == AccessKind::Write
                && access.location == Location::Memory(MemoryAddress::Direct(target_address))
        })
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    const TEST_BANK_COUNT: usize = 16;

    #[test]
    fn writer_census_uses_the_fixed_bank_at_switchable_boundaries() {
        let mut prg = vec![0x02; TEST_BANK_COUNT * PRG_BANK_BYTE_COUNT];
        let internal = 3 * PRG_BANK_BYTE_COUNT + 0x0100;
        prg[internal..internal + 3].copy_from_slice(&[0xEE, 0x3F, 0x05]);

        let false_adjacency = PRG_BANK_BYTE_COUNT - 1;
        prg[false_adjacency] = 0xEE;
        prg[PRG_BANK_BYTE_COUNT..PRG_BANK_BYTE_COUNT + 2].copy_from_slice(&[0x3F, 0x05]);

        let mapped_boundary = 2 * PRG_BANK_BYTE_COUNT + 0x3FFE;
        prg[mapped_boundary..mapped_boundary + 2].copy_from_slice(&[0xEE, 0x3F]);
        prg[15 * PRG_BANK_BYTE_COUNT] = 0x05;

        assert_eq!(
            scan_direct_memory_writers(&prg, &[0x053F]).unwrap(),
            BTreeSet::from([
                DirectMemoryWriter::new(2, 0xBFFE, 0xEE, 0x053F),
                DirectMemoryWriter::new(3, 0x8100, 0xEE, 0x053F),
            ])
        );
    }

    #[test]
    fn writer_census_keeps_same_target_writers_from_separate_physical_banks() {
        let mut prg = vec![0x02; TEST_BANK_COUNT * PRG_BANK_BYTE_COUNT];
        let bank_six_writer = 6 * PRG_BANK_BYTE_COUNT + usize::from(0x8916_u16 - 0x8000);
        let bank_eight_writer = 8 * PRG_BANK_BYTE_COUNT + usize::from(0xBA8F_u16 - 0x8000);
        prg[bank_six_writer..bank_six_writer + 3].copy_from_slice(&[0x8D, 0xEA, 0x05]);
        prg[bank_eight_writer..bank_eight_writer + 3].copy_from_slice(&[0x8D, 0xEA, 0x05]);

        assert_eq!(
            scan_direct_memory_writers(&prg, &[0x05EA]).unwrap(),
            BTreeSet::from([
                DirectMemoryWriter::new(6, 0x8916, 0x8D, 0x05EA),
                DirectMemoryWriter::new(8, 0xBA8F, 0x8D, 0x05EA),
            ])
        );
    }

    #[test]
    fn unknown_fixed_terminal_operands_fail_closed() {
        let mut prg = vec![0x02; TEST_BANK_COUNT * PRG_BANK_BYTE_COUNT];
        prg[TEST_BANK_COUNT * PRG_BANK_BYTE_COUNT - 1] = 0xEE;
        assert!(scan_direct_memory_writers(&prg, &[0x053F]).is_err());
    }
}
