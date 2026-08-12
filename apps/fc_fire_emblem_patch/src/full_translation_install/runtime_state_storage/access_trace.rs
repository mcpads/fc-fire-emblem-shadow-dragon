use std::collections::BTreeSet;

use anyhow::{Context, Result, ensure};
use retro_rp2a03::{AddressingMode, Location, MemoryAddress, Operand, Rp2A03, decode_bytes};
use typed_isa_core::{AccessKind, StaticSemantics};

use crate::{
    rom::{HEADER_SIZE, Rom},
    typed_source::{Rp2a03DirectControlFlow, rp2a03_direct_control_flow},
};

use super::{CANDIDATE_END, CANDIDATE_START};

const PRG_BANK_SIZE: usize = 16 * 1024;
const FIXED_BANK: u8 = 0x0F;
const MAIN_DIALOGUE_BANK: u8 = 0x0A;
const INLINE_POINTER_DISPATCH_ADDRESS: u16 = 0xC34C;

pub(super) struct RuntimeAccessTrace {
    pub(super) visited: BTreeSet<(u8, u16)>,
    pub(super) direct_overlaps: BTreeSet<AccessSite>,
    pub(super) indexed_potential_overlaps: BTreeSet<AccessSite>,
    pub(super) indirect_sites: BTreeSet<AccessSite>,
    pub(super) switchable_boundaries: BTreeSet<u16>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub(super) struct AccessSite {
    pub(super) bank: u8,
    pub(super) address: u16,
    pub(super) access: AccessDirection,
    pub(super) form: AccessForm,
    pub(super) operand: u16,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub(super) enum AccessDirection {
    Read,
    Write,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub(super) enum AccessForm {
    Direct,
    AbsoluteX,
    AbsoluteY,
    IndexedIndirectX,
    IndirectIndexedY,
}

pub(super) fn trace_main_dialogue_accesses(
    source: &Rom,
    roots: &[u16],
) -> Result<RuntimeAccessTrace> {
    trace_switchable_accesses(source, MAIN_DIALOGUE_BANK, roots)
}

pub(super) fn trace_switchable_accesses(
    source: &Rom,
    switchable_bank: u8,
    roots: &[u16],
) -> Result<RuntimeAccessTrace> {
    trace_accesses(source, switchable_bank, roots, false)
}

pub(super) fn trace_fixed_interrupt_accesses(
    source: &Rom,
    roots: &[u16],
) -> Result<RuntimeAccessTrace> {
    trace_accesses(source, FIXED_BANK, roots, true)
}

fn trace_accesses(
    source: &Rom,
    switchable_bank: u8,
    roots: &[u16],
    fixed_only: bool,
) -> Result<RuntimeAccessTrace> {
    let mut pending = roots
        .iter()
        .copied()
        .map(|address| (switchable_bank, address))
        .collect::<Vec<_>>();
    let mut visited = BTreeSet::new();
    let mut direct_overlaps = BTreeSet::new();
    let mut indexed_potential_overlaps = BTreeSet::new();
    let mut indirect_sites = BTreeSet::new();
    let mut switchable_boundaries = BTreeSet::new();

    while let Some((switchable_bank, address)) = pending.pop() {
        ensure!(
            address >= 0x8000,
            "RP2A03 trace escaped executable CPU space"
        );
        if fixed_only && address < 0xC000 {
            switchable_boundaries.insert(address);
            continue;
        }
        if !visited.insert((switchable_bank, address)) {
            continue;
        }
        let actual_bank = if address >= 0xC000 {
            FIXED_BANK
        } else {
            switchable_bank
        };
        let bytes = source_instruction_bytes(source, actual_bank, address)?;
        let instruction = decode_bytes(bytes).with_context(|| {
            format!("decode runtime access at {actual_bank:02X}:${address:04X}")
        })?;
        ensure!(
            instruction.opcode_is_documented(),
            "runtime access trace reached undocumented selector at {actual_bank:02X}:${address:04X}"
        );
        let semantics = Rp2A03::semantics(&instruction, &address)
            .expect("RP2A03 static semantics are infallible");
        for access in semantics.location_accesses {
            let Location::Memory(memory) = access.location else {
                continue;
            };
            let direction = match access.kind {
                AccessKind::Read => AccessDirection::Read,
                AccessKind::Write => AccessDirection::Write,
            };
            match memory {
                MemoryAddress::Direct(target) => {
                    let site = AccessSite {
                        bank: actual_bank,
                        address,
                        access: direction,
                        form: AccessForm::Direct,
                        operand: target,
                    };
                    if (CANDIDATE_START..=CANDIDATE_END).contains(&target) {
                        direct_overlaps.insert(site);
                    }
                }
                MemoryAddress::Effective {
                    mode: AddressingMode::AbsoluteX,
                    operand: Operand::Word(base),
                } if indexed_form_may_overlap(base) => {
                    indexed_potential_overlaps.insert(AccessSite {
                        bank: actual_bank,
                        address,
                        access: direction,
                        form: AccessForm::AbsoluteX,
                        operand: base,
                    });
                }
                MemoryAddress::Effective {
                    mode: AddressingMode::AbsoluteY,
                    operand: Operand::Word(base),
                } if indexed_form_may_overlap(base) => {
                    indexed_potential_overlaps.insert(AccessSite {
                        bank: actual_bank,
                        address,
                        access: direction,
                        form: AccessForm::AbsoluteY,
                        operand: base,
                    });
                }
                MemoryAddress::Effective {
                    mode: AddressingMode::ZeroPageIndexedIndirectX,
                    operand: Operand::Byte(pointer),
                } => {
                    indirect_sites.insert(AccessSite {
                        bank: actual_bank,
                        address,
                        access: direction,
                        form: AccessForm::IndexedIndirectX,
                        operand: u16::from(pointer),
                    });
                }
                MemoryAddress::Effective {
                    mode: AddressingMode::ZeroPageIndirectIndexedY,
                    operand: Operand::Byte(pointer),
                } => {
                    indirect_sites.insert(AccessSite {
                        bank: actual_bank,
                        address,
                        access: direction,
                        form: AccessForm::IndirectIndexedY,
                        operand: u16::from(pointer),
                    });
                }
                _ => {}
            }
        }

        match rp2a03_direct_control_flow(&instruction, address)? {
            Rp2a03DirectControlFlow::FallThrough { next } => {
                pending.push((switchable_bank, next));
            }
            Rp2a03DirectControlFlow::Branch {
                target,
                fallthrough,
            } => {
                pending.push((switchable_bank, target));
                pending.extend(fallthrough.map(|next| (switchable_bank, next)));
            }
            Rp2a03DirectControlFlow::Jump { target } => {
                pending.extend(target.map(|target| (switchable_bank, target)));
            }
            Rp2a03DirectControlFlow::Call {
                target,
                return_address,
            } => {
                pending.push((switchable_bank, return_address));
                if target != INLINE_POINTER_DISPATCH_ADDRESS {
                    pending.push((switchable_bank, target));
                }
            }
            Rp2a03DirectControlFlow::Return
            | Rp2a03DirectControlFlow::Interrupt
            | Rp2a03DirectControlFlow::Stop => {}
        }
    }

    Ok(RuntimeAccessTrace {
        visited,
        direct_overlaps,
        indexed_potential_overlaps,
        indirect_sites,
        switchable_boundaries,
    })
}

pub(super) fn indexed_form_may_overlap(base: u16) -> bool {
    base <= CANDIDATE_END && base.saturating_add(u16::from(u8::MAX)) >= CANDIDATE_START
}

fn source_instruction_bytes(source: &Rom, bank: u8, address: u16) -> Result<&[u8]> {
    let relative = if address >= 0xC000 {
        ensure!(
            bank == FIXED_BANK,
            "fixed source instruction uses a non-fixed bank"
        );
        usize::from(address - 0xC000)
    } else {
        ensure!(
            address >= 0x8000,
            "switchable source instruction is below 0x8000"
        );
        usize::from(address - 0x8000)
    };
    let offset = HEADER_SIZE + usize::from(bank) * PRG_BANK_SIZE + relative;
    source
        .data()
        .get(offset..offset + 3)
        .context("runtime access instruction is outside source ROM")
}
