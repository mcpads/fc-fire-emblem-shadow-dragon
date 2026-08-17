use std::collections::{BTreeMap, BTreeSet};

use anyhow::{Context, Result};
use retro_rp2a03::{Location, MemoryAddress, Rp2A03, decode_bytes};
use serde::Serialize;
use typed_isa_core::{AccessKind, StaticSemantics};

use crate::rom::Rom;

const FIXED_PRG_BANK: u8 = 0x0F;
const SOURCE_PRG_BANK_BYTE_COUNT: usize = 16 * 1024;
const SCHEDULER_STATE: u16 = 0x0025;
const PRG_BANK_SHADOW: u16 = 0x0029;

#[derive(Clone, Debug, Eq, Ord, PartialEq, PartialOrd, Serialize)]
pub(in crate::mapper165::source_mapper_write_audit) struct PositiveStateAccess {
    prg_bank_hex: String,
    cpu_address_hex: String,
    variable: &'static str,
    direction: &'static str,
    instruction: String,
    source_slices: Vec<&'static str>,
}

pub(super) fn bind_positive_state_accesses(
    source: &Rom,
    instruction_roles: &BTreeMap<(u8, u16), BTreeSet<&'static str>>,
) -> Result<Vec<PositiveStateAccess>> {
    let mut accesses = Vec::new();
    for (&(bank, address), roles) in instruction_roles {
        let instruction = decode_bytes(source_instruction_bytes(source, bank, address)?)
            .with_context(|| {
                format!("decode positive state access at {bank:02X}:${address:04X}")
            })?;
        let semantics = Rp2A03::semantics(&instruction, &address)
            .expect("RP2A03 static semantics are infallible");
        for access in semantics.location_accesses {
            let Location::Memory(MemoryAddress::Direct(target)) = access.location else {
                continue;
            };
            let variable = match target {
                SCHEDULER_STATE => "fixed_scheduler_state_25",
                PRG_BANK_SHADOW => "prg_bank_shadow_29",
                _ => continue,
            };
            accesses.push(PositiveStateAccess {
                prg_bank_hex: format!("0x{bank:02X}"),
                cpu_address_hex: format!("0x{address:04X}"),
                variable,
                direction: match access.kind {
                    AccessKind::Read => "read",
                    AccessKind::Write => "write",
                },
                instruction: format!("{instruction:?}"),
                source_slices: roles.iter().copied().collect(),
            });
        }
    }
    accesses.sort();
    accesses.dedup();
    Ok(accesses)
}

fn source_instruction_bytes(source: &Rom, bank: u8, address: u16) -> Result<&[u8]> {
    let (physical_bank, relative) = if address >= 0xC000 {
        (FIXED_PRG_BANK, usize::from(address - 0xC000))
    } else {
        (bank, usize::from(address - 0x8000))
    };
    let offset = usize::from(physical_bank)
        .checked_mul(SOURCE_PRG_BANK_BYTE_COUNT)
        .and_then(|base| base.checked_add(relative))
        .context("positive state access PRG offset overflow")?;
    source
        .prg()
        .get(offset..offset + 3)
        .context("positive state access instruction exceeds source PRG")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::rom::HEADER_SIZE;

    #[test]
    fn reports_only_typed_accesses_to_the_two_owned_state_bytes() {
        let mut bytes = vec![0x60; HEADER_SIZE + 16 * SOURCE_PRG_BANK_BYTE_COUNT];
        bytes[..HEADER_SIZE].fill(0);
        bytes[..4].copy_from_slice(b"NES\x1A");
        bytes[4] = 16;
        bytes[6] = 0xA0;
        let fixed = HEADER_SIZE + 15 * SOURCE_PRG_BANK_BYTE_COUNT;
        bytes[fixed..fixed + 6].copy_from_slice(&[0xA5, 0x25, 0x85, 0x29, 0x85, 0x24]);
        let source = Rom::parse(bytes).unwrap();
        let roles = BTreeMap::from([
            ((FIXED_PRG_BANK, 0xC000), BTreeSet::from(["test"])),
            ((FIXED_PRG_BANK, 0xC002), BTreeSet::from(["test"])),
            ((FIXED_PRG_BANK, 0xC004), BTreeSet::from(["test"])),
        ]);

        let accesses = bind_positive_state_accesses(&source, &roles).unwrap();

        assert_eq!(accesses.len(), 2);
        assert!(accesses.iter().any(|access| {
            access.variable == "fixed_scheduler_state_25" && access.direction == "read"
        }));
        assert!(accesses.iter().any(|access| {
            access.variable == "prg_bank_shadow_29" && access.direction == "write"
        }));
    }
}
