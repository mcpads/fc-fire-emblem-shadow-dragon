use std::collections::BTreeSet;

use anyhow::{Context, Result, ensure};
use retro_rp2a03::{Location, MemoryAddress, Rp2A03, decode_bytes};
use typed_isa_core::{AccessKind, StaticSemantics};

const PRG_BANK_BYTE_COUNT: usize = 16 * 1024;
const PRG_BANK_COUNT: usize = 16;

#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub(super) struct DirectStateWriter {
    pub(super) physical_prg_bank: u8,
    pub(super) cpu_address: u16,
    pub(super) opcode: u8,
    pub(super) target_address: u16,
}

impl DirectStateWriter {
    pub(super) const fn in_map_preparation_bank(
        cpu_address: u16,
        opcode: u8,
        target_address: u16,
    ) -> Self {
        Self {
            physical_prg_bank: 0x03,
            cpu_address,
            opcode,
            target_address,
        }
    }
}

pub(super) fn scan_direct_state_writers(
    prg: &[u8],
    target_addresses: &[u16],
) -> Result<BTreeSet<DirectStateWriter>> {
    ensure!(
        prg.len() == PRG_BANK_COUNT * PRG_BANK_BYTE_COUNT,
        "map-preparation writer census requires the supported 256 KiB PRG"
    );
    ensure!(
        !target_addresses.is_empty()
            && target_addresses
                .iter()
                .copied()
                .collect::<BTreeSet<_>>()
                .len()
                == target_addresses.len(),
        "map-preparation writer census needs unique target addresses"
    );
    let target_addresses = target_addresses.iter().copied().collect::<BTreeSet<_>>();
    let fixed = &prg[(PRG_BANK_COUNT - 1) * PRG_BANK_BYTE_COUNT..];
    let mut writers = BTreeSet::new();

    for physical_bank in 0..PRG_BANK_COUNT {
        let bank =
            &prg[physical_bank * PRG_BANK_BYTE_COUNT..(physical_bank + 1) * PRG_BANK_BYTE_COUNT];
        let cpu_start = if physical_bank == PRG_BANK_COUNT - 1 {
            0xC000
        } else {
            0x8000
        };
        for (offset, window) in bank.windows(3).enumerate() {
            record_direct_state_writer(
                &mut writers,
                &target_addresses,
                u8::try_from(physical_bank)?,
                cpu_start + u16::try_from(offset)?,
                window,
            )?;
        }
        if physical_bank < PRG_BANK_COUNT - 1 {
            let bffe = [bank[0x3FFE], bank[0x3FFF], fixed[0]];
            let bfff = [bank[0x3FFF], fixed[0], fixed[1]];
            record_direct_state_writer(
                &mut writers,
                &target_addresses,
                u8::try_from(physical_bank)?,
                0xBFFE,
                &bffe,
            )?;
            record_direct_state_writer(
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
            !completion_may_write_state(&[fixed[0x3FFE], fixed[0x3FFF]], target_address)
                && !completion_may_write_state(&[fixed[0x3FFF]], target_address),
            "fixed-bank terminal instruction may write map-preparation state ${target_address:04X} through runtime RAM operand bytes"
        );
    }
    Ok(writers)
}

fn record_direct_state_writer(
    writers: &mut BTreeSet<DirectStateWriter>,
    target_addresses: &BTreeSet<u16>,
    physical_prg_bank: u8,
    cpu_address: u16,
    bytes: &[u8],
) -> Result<()> {
    let Ok(instruction) = decode_bytes(bytes) else {
        return Ok(());
    };
    let semantics = Rp2A03::semantics(&instruction, &cpu_address)
        .context("derive map-preparation writer semantics")?;
    for access in semantics.location_accesses {
        let Location::Memory(MemoryAddress::Direct(target_address)) = access.location else {
            continue;
        };
        if access.kind == AccessKind::Write && target_addresses.contains(&target_address) {
            writers.insert(DirectStateWriter {
                physical_prg_bank,
                cpu_address,
                opcode: bytes[0],
                target_address,
            });
        }
    }
    Ok(())
}

fn completion_may_write_state(available: &[u8], target_address: u16) -> bool {
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

    #[test]
    fn writer_census_uses_the_fixed_bank_at_switchable_boundaries() {
        let mut prg = vec![0x02; PRG_BANK_COUNT * PRG_BANK_BYTE_COUNT];
        let internal = 3 * PRG_BANK_BYTE_COUNT + 0x0100;
        prg[internal..internal + 3].copy_from_slice(&[0xEE, 0x3F, 0x05]);

        let false_adjacency = PRG_BANK_BYTE_COUNT - 1;
        prg[false_adjacency] = 0xEE;
        prg[PRG_BANK_BYTE_COUNT..PRG_BANK_BYTE_COUNT + 2].copy_from_slice(&[0x3F, 0x05]);

        let mapped_boundary = 2 * PRG_BANK_BYTE_COUNT + 0x3FFE;
        prg[mapped_boundary..mapped_boundary + 2].copy_from_slice(&[0xEE, 0x3F]);
        prg[15 * PRG_BANK_BYTE_COUNT] = 0x05;

        assert_eq!(
            scan_direct_state_writers(&prg, &[0x053F]).unwrap(),
            BTreeSet::from([
                DirectStateWriter {
                    physical_prg_bank: 2,
                    cpu_address: 0xBFFE,
                    opcode: 0xEE,
                    target_address: 0x053F,
                },
                DirectStateWriter::in_map_preparation_bank(0x8100, 0xEE, 0x053F),
            ])
        );
    }

    #[test]
    fn unknown_fixed_terminal_operands_fail_closed() {
        let mut prg = vec![0x02; PRG_BANK_COUNT * PRG_BANK_BYTE_COUNT];
        prg[PRG_BANK_COUNT * PRG_BANK_BYTE_COUNT - 1] = 0xEE;
        assert!(scan_direct_state_writers(&prg, &[0x053F]).is_err());
    }
}
