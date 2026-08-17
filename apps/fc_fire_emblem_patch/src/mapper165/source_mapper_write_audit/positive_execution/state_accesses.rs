use std::collections::{BTreeMap, BTreeSet};

use anyhow::{Context, Result, ensure};
use retro_rp2a03::{Location, MemoryAddress, Rp2A03, decode_bytes};
use serde::Serialize;
use typed_isa_core::{AccessKind, StaticSemantics};

use crate::rom::Rom;

use super::control_state::{ObservedControlStateWrites, positive_control_state};

const FIXED_PRG_BANK: u8 = 0x0F;
const SOURCE_PRG_BANK_BYTE_COUNT: usize = 16 * 1024;

#[derive(Clone, Debug, Eq, Ord, PartialEq, PartialOrd, Serialize)]
pub(in crate::mapper165::source_mapper_write_audit) struct PositiveStateAccess {
    prg_bank_hex: String,
    cpu_address_hex: String,
    variable: &'static str,
    direction: &'static str,
    instruction: String,
    source_slices: Vec<&'static str>,
    stateful_write_observed: bool,
    observed_write_values_hex: Option<Vec<String>>,
}

pub(super) fn bind_positive_state_accesses(
    source: &Rom,
    instruction_roles: &BTreeMap<(u8, u16), BTreeSet<&'static str>>,
    observed_writes: &ObservedControlStateWrites,
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
            let Some(variable) = positive_control_state(target) else {
                continue;
            };
            let observed_write = (access.kind == AccessKind::Write)
                .then(|| observed_writes.get(&(bank, address, target)))
                .flatten();
            accesses.push(PositiveStateAccess {
                prg_bank_hex: format!("0x{bank:02X}"),
                cpu_address_hex: format!("0x{address:04X}"),
                variable: variable.role,
                direction: match access.kind {
                    AccessKind::Read => "read",
                    AccessKind::Write => "write",
                },
                instruction: format!("{instruction:?}"),
                source_slices: roles.iter().copied().collect(),
                stateful_write_observed: observed_write.is_some(),
                observed_write_values_hex: observed_write.and_then(|values| {
                    values.as_ref().map(|values| {
                        values
                            .iter()
                            .map(|value| format!("0x{value:02X}"))
                            .collect()
                    })
                }),
            });
        }
    }
    accesses.sort();
    accesses.dedup();
    let missing_observations = accesses
        .iter()
        .filter(|access| access.direction == "write" && !access.stateful_write_observed)
        .map(|access| {
            format!(
                "{}:{}:{}",
                access.prg_bank_hex, access.cpu_address_hex, access.variable
            )
        })
        .collect::<Vec<_>>();
    ensure!(
        missing_observations.is_empty(),
        "positive execution control-state writes lack stateful value observations: {missing_observations:?}"
    );
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

    fn source_with_fixed_program(program: &[u8]) -> Rom {
        let mut bytes = vec![0x60; HEADER_SIZE + 16 * SOURCE_PRG_BANK_BYTE_COUNT];
        bytes[..HEADER_SIZE].fill(0);
        bytes[..4].copy_from_slice(b"NES\x1A");
        bytes[4] = 16;
        bytes[6] = 0xA0;
        let fixed = HEADER_SIZE + 15 * SOURCE_PRG_BANK_BYTE_COUNT;
        bytes[fixed..fixed + program.len()].copy_from_slice(program);
        Rom::parse(bytes).unwrap()
    }

    #[test]
    fn reports_typed_accesses_to_screen_and_mapper_control_state() {
        let source = source_with_fixed_program(&[
            0xA5, 0x25, // LDA $25
            0x85, 0x29, // STA $29
            0x85, 0x24, // STA $24
            0x8D, 0xDB, 0x05, // STA $05DB
            0x8D, 0xEE, 0x05, // STA $05EE
            0x8D, 0x34, 0x12, // STA $1234; deliberately unowned
        ]);
        let roles = BTreeMap::from([
            ((FIXED_PRG_BANK, 0xC000), BTreeSet::from(["test"])),
            ((FIXED_PRG_BANK, 0xC002), BTreeSet::from(["test"])),
            ((FIXED_PRG_BANK, 0xC004), BTreeSet::from(["test"])),
            ((FIXED_PRG_BANK, 0xC006), BTreeSet::from(["test"])),
            ((FIXED_PRG_BANK, 0xC009), BTreeSet::from(["test"])),
            ((FIXED_PRG_BANK, 0xC00C), BTreeSet::from(["test"])),
            ((FIXED_PRG_BANK, 0xC00F), BTreeSet::from(["test"])),
        ]);

        let observed_writes = ObservedControlStateWrites::from([
            (
                (FIXED_PRG_BANK, 0xC002, 0x0029),
                Some(BTreeSet::from([0x06])),
            ),
            (
                (FIXED_PRG_BANK, 0xC004, 0x0024),
                Some(BTreeSet::from([0x03])),
            ),
            ((FIXED_PRG_BANK, 0xC006, 0x05DB), None),
            ((FIXED_PRG_BANK, 0xC009, 0x05EE), None),
        ]);
        let accesses = bind_positive_state_accesses(&source, &roles, &observed_writes).unwrap();

        assert_eq!(accesses.len(), 5);
        assert!(accesses.iter().any(|access| {
            access.variable == "fixed_scheduler_state_25" && access.direction == "read"
        }));
        assert!(accesses.iter().any(|access| {
            access.variable == "prg_bank_shadow_29" && access.direction == "write"
        }));
        assert!(accesses.iter().any(|access| {
            access.variable == "outer_screen_state_24"
                && access.direction == "write"
                && access.stateful_write_observed
                && access.observed_write_values_hex == Some(vec!["0x03".to_owned()])
        }));
        assert!(accesses.iter().any(|access| {
            access.variable == "map_dialogue_outer_state_05DB"
                && access.direction == "write"
                && access.stateful_write_observed
                && access.observed_write_values_hex.is_none()
        }));
        assert!(accesses.iter().any(|access| {
            access.variable == "dialogue_or_sound_state_05EE" && access.direction == "write"
        }));
    }

    #[test]
    fn a_positive_control_state_write_without_a_value_observation_fails_closed() {
        let source = source_with_fixed_program(&[0x85, 0x24]);
        let roles = BTreeMap::from([((FIXED_PRG_BANK, 0xC000), BTreeSet::from(["test"]))]);

        let error =
            bind_positive_state_accesses(&source, &roles, &ObservedControlStateWrites::new())
                .unwrap_err();

        assert!(
            error
                .to_string()
                .contains("control-state writes lack stateful value observations")
        );
    }
}
