use std::collections::BTreeSet;

use anyhow::{Context, Result, ensure};
use retro_rp2a03::decode_bytes;
use serde::Serialize;

use crate::{
    rom::Rom,
    sha1_hex,
    typed_source::{Rp2a03DirectControlFlow, rp2a03_direct_control_flow},
};

use super::{
    background_payloads::BATTLE_BANK_PUBLISH_SITES,
    source_window::{prg_bank, source_bytes},
};

const INLINE_POINTER_DISPATCH_ADDRESS: u16 = 0xC34C;

pub(super) const PRIMARY_PHASE_POINTERS: [u16; 32] = [
    0x82C7, 0x8830, 0x8C5D, 0x8830, 0x8C5D, 0x881C, 0x8827, 0x8CD3, 0x9304, 0x82F1, 0x8505, 0x8522,
    0x85DE, 0x8341, 0x83FD, 0x8522, 0x83D3, 0x8467, 0x8475, 0x8353, 0x84D5, 0x8522, 0x837F, 0x8341,
    0x86E1, 0x8725, 0x8250, 0x829C, 0x82E9, 0x835B, 0x8368, 0x81A9,
];
pub(super) const UNIT_PANEL_PHASE_POINTERS: [u16; 12] = [
    0x884E, 0x8874, 0x8863, 0x8946, 0x89AA, 0x89D7, 0x8A39, 0x8A64, 0x8A94, 0x8AD8, 0x8BA5, 0x8852,
];
pub(super) const ANIMATION_PHASE_POINTERS: [u16; 41] = [
    0x97A1, 0x936C, 0x93E9, 0x9435, 0x943C, 0x945D, 0x9495, 0x94C4, 0x98D5, 0x98D9, 0x97CF, 0x97E8,
    0x95C3, 0x9603, 0x962D, 0x963F, 0x9648, 0x96A3, 0x96C0, 0x950B, 0x958D, 0x9596, 0x9620, 0x9717,
    0x972A, 0x99B7, 0x9801, 0x97EF, 0x984D, 0x98D5, 0x98D9, 0x9829, 0x97EF, 0xA059, 0x98D5, 0xAE70,
    0xAE87, 0xAED2, 0x98D9, 0xAEEB, 0x8830,
];
const ANIMATION_COMMAND_PHASE_POINTERS: [u16; 29] = [
    0x9A20, 0x9B03, 0x9B66, 0x9B76, 0x9B8F, 0x9BD8, 0x9BF3, 0x9C05, 0x9C37, 0x9C41, 0x9C4B, 0x9DBC,
    0x9F01, 0xA046, 0x9EFD, 0xA05D, 0xA070, 0x9AC2, 0xA083, 0xA0AA, 0xA0CE, 0x9A53, 0xA0D5, 0xA0FD,
    0xA109, 0xA12B, 0xA157, 0xA176, 0xA186,
];
const EFFECT_OBJECT_PHASE_POINTERS: [u16; 51] = [
    0xC73D, 0xA2B5, 0xA2C2, 0xA2DA, 0xA36A, 0xA3A4, 0xA3E2, 0xA42E, 0xA509, 0xA539, 0xA55B, 0xA59B,
    0xA5B9, 0xA65E, 0xA6B9, 0xA6F3, 0xA703, 0xA73A, 0xA743, 0xA797, 0xA7D5, 0xA826, 0xA843, 0xA875,
    0xA886, 0xA8AD, 0xA8E8, 0xA91D, 0xA9AA, 0xA9E2, 0xA9E7, 0xAA2C, 0xA96B, 0xAA3C, 0xAA80, 0xAA9A,
    0xAAA6, 0xAAD8, 0xAAD8, 0xAAD8, 0xAAD8, 0xAB2B, 0xAB70, 0xABD5, 0xAC2B, 0xAC4D, 0xAC84, 0xACCF,
    0xAD0D, 0xAD1A, 0xAD64,
];
const ANIMATION_CLEANUP_PHASE_POINTERS: [u16; 8] = [
    0xC73D, 0xAF16, 0xAF2B, 0xAF44, 0xAF61, 0xAF6C, 0xAF76, 0xAFFC,
];
const DIALOGUE_BOX_PHASE_POINTERS: [u16; 6] = [0xC73D, 0x8012, 0x8012, 0x8012, 0x8012, 0x80D8];

pub(super) fn battle_phase_roots() -> Vec<(u8, u16)> {
    PRIMARY_PHASE_POINTERS
        .into_iter()
        .chain(UNIT_PANEL_PHASE_POINTERS)
        .chain(ANIMATION_PHASE_POINTERS)
        .chain(ANIMATION_COMMAND_PHASE_POINTERS)
        .chain(EFFECT_OBJECT_PHASE_POINTERS)
        .chain(ANIMATION_CLEANUP_PHASE_POINTERS)
        .map(|address| (0x05, address))
        .chain(
            DIALOGUE_BOX_PHASE_POINTERS
                .into_iter()
                .map(|address| (0x07, address)),
        )
        .collect()
}

pub(in crate::mapper165) fn battle_phase_reachable_instruction_starts(
    rom: &Rom,
) -> Result<BTreeSet<(u8, u16)>> {
    bind_nested_dispatchers(rom)?;
    let mut reachable = BTreeSet::new();
    for (bank_number, _, pointers) in phase_groups() {
        let bank = prg_bank(rom, bank_number)?;
        for handler in pointers {
            let trace = trace_switchable_control_flow(bank, *handler, &BTreeSet::new())?;
            reachable.extend(
                trace
                    .visited_instructions
                    .into_iter()
                    .map(|address| (bank_number, address)),
            );
        }
    }
    Ok(reachable)
}

#[derive(Clone, Debug, Serialize)]
pub(super) struct BattlePhasePublisherReachability {
    phase_group_count: usize,
    phase_entry_count: usize,
    distinct_phase_handler_count: usize,
    traced_instruction_count: usize,
    reached_publisher_count: usize,
    publisher_count: usize,
    phase_catalog_sha1: String,
    groups: Vec<PhaseGroupReachability>,
    every_publisher_reached_from_declared_phase: bool,
    conservative_full_lifetime_bound_complete: bool,
    exact_phase_to_text_cooccurrence_required_for_capacity: bool,
}

#[derive(Clone, Debug, Serialize)]
struct PhaseGroupReachability {
    role: &'static str,
    bank_hex: String,
    phase_count: usize,
    distinct_handler_count: usize,
    reached_publisher_count: usize,
    phases: Vec<PhaseReachability>,
}

#[derive(Clone, Debug, Serialize)]
struct PhaseReachability {
    phase_index: usize,
    handler_address_hex: String,
    traced_instruction_count: usize,
    publisher_addresses_hex: Vec<String>,
}

pub(super) fn bind_phase_publisher_reachability(
    rom: &Rom,
) -> Result<BattlePhasePublisherReachability> {
    bind_nested_dispatchers(rom)?;
    let groups = phase_groups()
        .into_iter()
        .map(|(bank_number, role, pointers)| {
            trace_group(
                prg_bank(rom, bank_number)?,
                bank_number,
                role,
                pointers,
                &publisher_addresses(bank_number),
            )
        })
        .collect::<Result<Vec<_>>>()?;

    let reached_publishers = groups
        .iter()
        .flat_map(|group| {
            group.phases.iter().flat_map(|phase| {
                phase
                    .publisher_addresses_hex
                    .iter()
                    .map(|address| format!("{}:{address}", group.bank_hex))
            })
        })
        .collect::<BTreeSet<_>>();
    ensure!(
        reached_publishers.len() == BATTLE_BANK_PUBLISH_SITES.len(),
        "battle phase graph reaches {} of {} background publishers",
        reached_publishers.len(),
        BATTLE_BANK_PUBLISH_SITES.len()
    );

    let distinct_handlers = PRIMARY_PHASE_POINTERS
        .into_iter()
        .chain(UNIT_PANEL_PHASE_POINTERS)
        .chain(ANIMATION_PHASE_POINTERS)
        .chain(ANIMATION_COMMAND_PHASE_POINTERS)
        .chain(EFFECT_OBJECT_PHASE_POINTERS)
        .chain(ANIMATION_CLEANUP_PHASE_POINTERS)
        .chain(DIALOGUE_BOX_PHASE_POINTERS)
        .collect::<BTreeSet<_>>();
    let mut catalog = Vec::new();
    for group in &groups {
        catalog.extend_from_slice(group.role.as_bytes());
        catalog.push(0);
        catalog.extend_from_slice(group.bank_hex.as_bytes());
        catalog.push(0);
        for phase in &group.phases {
            catalog.extend_from_slice(&(phase.phase_index as u64).to_le_bytes());
            catalog.extend_from_slice(phase.handler_address_hex.as_bytes());
            catalog.push(0);
            for publisher in &phase.publisher_addresses_hex {
                catalog.extend_from_slice(publisher.as_bytes());
                catalog.push(0);
            }
        }
    }
    let traced_instruction_count = groups
        .iter()
        .flat_map(|group| &group.phases)
        .map(|phase| phase.traced_instruction_count)
        .sum();

    Ok(BattlePhasePublisherReachability {
        phase_group_count: groups.len(),
        phase_entry_count: groups.iter().map(|group| group.phase_count).sum(),
        distinct_phase_handler_count: distinct_handlers.len(),
        traced_instruction_count,
        reached_publisher_count: reached_publishers.len(),
        publisher_count: BATTLE_BANK_PUBLISH_SITES.len(),
        phase_catalog_sha1: sha1_hex(&catalog),
        groups,
        every_publisher_reached_from_declared_phase: true,
        conservative_full_lifetime_bound_complete: true,
        exact_phase_to_text_cooccurrence_required_for_capacity: false,
    })
}

fn phase_groups() -> [(u8, &'static str, &'static [u16]); 7] {
    [
        (0x05, "primary", PRIMARY_PHASE_POINTERS.as_slice()),
        (0x05, "unit_panel", UNIT_PANEL_PHASE_POINTERS.as_slice()),
        (0x05, "animation", ANIMATION_PHASE_POINTERS.as_slice()),
        (
            0x05,
            "animation_command",
            ANIMATION_COMMAND_PHASE_POINTERS.as_slice(),
        ),
        (
            0x05,
            "effect_object",
            EFFECT_OBJECT_PHASE_POINTERS.as_slice(),
        ),
        (
            0x05,
            "animation_cleanup",
            ANIMATION_CLEANUP_PHASE_POINTERS.as_slice(),
        ),
        (0x07, "dialogue_box", DIALOGUE_BOX_PHASE_POINTERS.as_slice()),
    ]
}

impl BattlePhasePublisherReachability {
    #[cfg(test)]
    pub(super) fn test_model() -> Self {
        Self {
            phase_group_count: 0,
            phase_entry_count: 0,
            distinct_phase_handler_count: 0,
            traced_instruction_count: 0,
            reached_publisher_count: 0,
            publisher_count: 0,
            phase_catalog_sha1: String::new(),
            groups: Vec::new(),
            every_publisher_reached_from_declared_phase: false,
            conservative_full_lifetime_bound_complete: false,
            exact_phase_to_text_cooccurrence_required_for_capacity: true,
        }
    }
}

fn bind_nested_dispatchers(rom: &Rom) -> Result<()> {
    for spec in [
        InlineDispatcherSpec {
            bank: 0x05,
            address: 0x99D9,
            bytes: &[
                0xAD, 0xC4, 0x03, 0x0A, 0xA8, 0x8C, 0x74, 0x03, 0xB1, 0x00, 0x20, 0x4C, 0xC3,
            ],
            table_address: 0x99E6,
            pointers: &ANIMATION_COMMAND_PHASE_POINTERS,
            role: "battle animation command phase",
        },
        InlineDispatcherSpec {
            bank: 0x05,
            address: 0xA242,
            bytes: &[
                0xAD, 0xC7, 0x03, 0x0A, 0xA8, 0x8C, 0x74, 0x03, 0xB1, 0x00, 0x20, 0x4C, 0xC3,
            ],
            table_address: 0xA24F,
            pointers: &EFFECT_OBJECT_PHASE_POINTERS,
            role: "battle effect-object phase",
        },
        InlineDispatcherSpec {
            bank: 0x05,
            address: 0xAF00,
            bytes: &[0xAD, 0x78, 0x04, 0x20, 0x4C, 0xC3],
            table_address: 0xAF06,
            pointers: &ANIMATION_CLEANUP_PHASE_POINTERS,
            role: "battle animation cleanup phase",
        },
        InlineDispatcherSpec {
            bank: 0x07,
            address: 0x8000,
            bytes: &[0xAD, 0x67, 0x04, 0x20, 0x4C, 0xC3],
            table_address: 0x8006,
            pointers: &DIALOGUE_BOX_PHASE_POINTERS,
            role: "battle dialogue-box phase",
        },
    ] {
        ensure!(
            source_bytes(rom, spec.bank, spec.address, spec.bytes.len())? == spec.bytes,
            "{} dispatcher changed",
            spec.role
        );
        let table = source_bytes(rom, spec.bank, spec.table_address, spec.pointers.len() * 2)?;
        let pointers = table
            .chunks_exact(2)
            .map(|bytes| u16::from_le_bytes([bytes[0], bytes[1]]))
            .collect::<Vec<_>>();
        ensure!(
            pointers == spec.pointers,
            "{} pointer table changed",
            spec.role
        );
    }
    Ok(())
}

struct InlineDispatcherSpec {
    bank: u8,
    address: u16,
    bytes: &'static [u8],
    table_address: u16,
    pointers: &'static [u16],
    role: &'static str,
}

fn publisher_addresses(bank: u8) -> BTreeSet<u16> {
    BATTLE_BANK_PUBLISH_SITES
        .iter()
        .filter(|(publisher_bank, _, _)| *publisher_bank == bank)
        .map(|(_, address, _)| *address)
        .collect()
}

fn trace_group(
    bank: &[u8],
    bank_number: u8,
    role: &'static str,
    pointers: &[u16],
    publisher_addresses: &BTreeSet<u16>,
) -> Result<PhaseGroupReachability> {
    let phases = pointers
        .iter()
        .copied()
        .enumerate()
        .map(|(phase_index, handler)| {
            let trace = trace_switchable_control_flow(bank, handler, publisher_addresses)
                .with_context(|| format!("trace {role} phase {phase_index} at ${handler:04X}"))?;
            Ok(PhaseReachability {
                phase_index,
                handler_address_hex: format!("0x{handler:04X}"),
                traced_instruction_count: trace.visited_instructions.len(),
                publisher_addresses_hex: trace
                    .reached_target_addresses
                    .iter()
                    .map(|address| format!("0x{address:04X}"))
                    .collect(),
            })
        })
        .collect::<Result<Vec<_>>>()?;
    let reached_publisher_count = phases
        .iter()
        .flat_map(|phase| phase.publisher_addresses_hex.iter())
        .collect::<BTreeSet<_>>()
        .len();
    Ok(PhaseGroupReachability {
        role,
        bank_hex: format!("0x{bank_number:02X}"),
        phase_count: pointers.len(),
        distinct_handler_count: pointers.iter().copied().collect::<BTreeSet<_>>().len(),
        reached_publisher_count,
        phases,
    })
}

pub(super) struct SwitchableControlFlowTrace {
    pub(super) visited_instructions: BTreeSet<u16>,
    pub(super) reached_target_addresses: BTreeSet<u16>,
}

pub(super) fn trace_switchable_control_flow(
    bank: &[u8],
    start: u16,
    target_addresses: &BTreeSet<u16>,
) -> Result<SwitchableControlFlowTrace> {
    if !(0x8000..0xC000).contains(&start) {
        ensure!(
            start == 0xC73D,
            "battle phase handler is outside its switchable bank"
        );
        return Ok(SwitchableControlFlowTrace {
            visited_instructions: BTreeSet::new(),
            reached_target_addresses: BTreeSet::new(),
        });
    }
    let mut pending = vec![start];
    let mut visited_instructions = BTreeSet::new();
    let mut reached_target_addresses = BTreeSet::new();
    while let Some(address) = pending.pop() {
        if !(0x8000..0xC000).contains(&address) || !visited_instructions.insert(address) {
            continue;
        }
        let offset = usize::from(address - 0x8000);
        let instruction = decode_bytes(
            bank.get(offset..)
                .context("battle phase address is outside its PRG bank")?,
        )
        .with_context(|| format!("decode battle phase instruction at ${address:04X}"))?;
        ensure!(
            instruction.opcode_is_documented(),
            "battle phase graph reached undocumented selector at ${address:04X}"
        );
        if target_addresses.contains(&address) {
            reached_target_addresses.insert(address);
        }
        match rp2a03_direct_control_flow(&instruction, address)? {
            Rp2a03DirectControlFlow::FallThrough { next } => pending.push(next),
            Rp2a03DirectControlFlow::Branch {
                target,
                fallthrough,
            } => {
                pending.push(target);
                if let Some(fallthrough) = fallthrough {
                    pending.push(fallthrough);
                }
            }
            Rp2a03DirectControlFlow::Jump { target } => pending.extend(target),
            Rp2a03DirectControlFlow::Call {
                target,
                return_address,
            } => {
                if target != INLINE_POINTER_DISPATCH_ADDRESS {
                    pending.push(return_address);
                }
                pending.push(target);
            }
            Rp2a03DirectControlFlow::Return
            | Rp2a03DirectControlFlow::Interrupt
            | Rp2a03DirectControlFlow::Stop => {}
        }
    }
    Ok(SwitchableControlFlowTrace {
        visited_instructions,
        reached_target_addresses,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn typed_trace_follows_calls_and_both_branch_arms() {
        let mut bank = vec![0x60; 0x4000];
        bank[0..8].copy_from_slice(&[0x20, 0x08, 0x80, 0xF0, 0x02, 0x84, 0x21, 0x60]);
        bank[8..11].copy_from_slice(&[0x86, 0x21, 0x60]);
        let trace = trace_switchable_control_flow(&bank, 0x8000, &BTreeSet::from([0x8005, 0x8008]))
            .unwrap();

        assert_eq!(
            trace.reached_target_addresses,
            BTreeSet::from([0x8005, 0x8008])
        );
        assert!(trace.visited_instructions.contains(&0x8007));
    }
}
