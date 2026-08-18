use std::collections::BTreeSet;

use anyhow::{Context, Result};
use retro_rp2a03::{
    AddressingMode, Location, MemoryAddress, Mnemonic, Operand, Rp2A03, decode_bytes,
};
use typed_isa_core::{AccessKind, StaticSemantics};

use crate::{
    mapper165::executable_mapper_writes::{SourceMmc4Register, decode_source_mmc4_write},
    rom::Rom,
    typed_source::{Rp2a03DirectControlFlow, rp2a03_direct_control_flow},
};

use super::{FIXED_CPU_START, FIXED_PRG_BANK, ResetTraceState, source_instruction_bytes};

const MAXIMUM_SUMMARIZED_STATE_COUNT: usize = 4_096;

#[derive(Clone, Debug, Eq, Ord, PartialEq, PartialOrd)]
struct CallSummaryState {
    address: u16,
    stack: Vec<CallStackByte>,
}

#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
enum CallStackByte {
    ParentReturnHigh,
    ParentReturnLow,
    EntryReturnHigh,
    EntryReturnLow,
    DirectReturnHigh(u16),
    DirectReturnLow(u16),
    Scratch,
}

#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub(super) enum CallReturnEffect {
    Normal,
    EscapeOneCaller,
}

#[derive(Clone, Debug)]
pub(super) struct TrackedStateCallSummary {
    instruction_starts: BTreeSet<(u8, u16)>,
    return_effects: BTreeSet<CallReturnEffect>,
}

impl TrackedStateCallSummary {
    pub(super) fn instruction_starts(&self) -> &BTreeSet<(u8, u16)> {
        &self.instruction_starts
    }

    pub(super) fn return_effects(&self) -> &BTreeSet<CallReturnEffect> {
        &self.return_effects
    }
}

/// Finds a direct call graph whose instructions cannot change any state used to resolve PRG
/// mappings or scheduler control. The byte stack is followed across nested calls, so a callee that
/// discards its own return address and returns through its caller composes into that caller's
/// ordinary return. A graph may also consume the entry frame and one parent return address, which
/// represents the source queue append's intentional early escape. Register results are deliberately
/// not summarized; the caller must invalidate A/X/Y and flags when applying this summary.
pub(super) fn inspect_tracked_state_call(
    source: &Rom,
    entry_bank: u8,
    entry_address: u16,
) -> Result<Option<TrackedStateCallSummary>> {
    inspect_state_preserving_call(
        source,
        entry_bank,
        entry_address,
        ResetTraceState::tracks_memory_address,
    )
}

pub(super) fn inspect_selected_state_call(
    source: &Rom,
    entry_bank: u8,
    entry_address: u16,
    preserved_addresses: &BTreeSet<u16>,
) -> Result<Option<TrackedStateCallSummary>> {
    inspect_state_preserving_call(source, entry_bank, entry_address, |address| {
        preserved_addresses.contains(&address)
    })
}

fn inspect_state_preserving_call(
    source: &Rom,
    entry_bank: u8,
    entry_address: u16,
    preserves_address: impl Fn(u16) -> bool,
) -> Result<Option<TrackedStateCallSummary>> {
    let mut pending = vec![CallSummaryState {
        address: entry_address,
        stack: vec![
            CallStackByte::ParentReturnHigh,
            CallStackByte::ParentReturnLow,
            CallStackByte::EntryReturnHigh,
            CallStackByte::EntryReturnLow,
        ],
    }];
    let mut visited = BTreeSet::new();
    let mut instruction_starts = BTreeSet::new();
    let mut return_effects = BTreeSet::new();
    let mut reads_tracked_control_state = false;

    while let Some(summary_state) = pending.pop() {
        if !visited.insert(summary_state.clone()) {
            continue;
        }
        if visited.len() > MAXIMUM_SUMMARIZED_STATE_COUNT {
            return Ok(None);
        }
        let address = summary_state.address;
        if address < 0x8000 {
            return Ok(None);
        }
        let physical_bank = if address >= FIXED_CPU_START {
            FIXED_PRG_BANK
        } else {
            entry_bank
        };
        instruction_starts.insert((physical_bank, address));

        let instruction = decode_bytes(&source_instruction_bytes(
            source,
            physical_bank,
            address,
            3,
        )?)
        .with_context(|| {
            format!("decode state-transparent call at {physical_bank:02X}:${address:04X}")
        })?;
        if !instruction.opcode_is_documented()
            || matches!(instruction.mnemonic(), Mnemonic::Tsx | Mnemonic::Txs)
        {
            return Ok(None);
        }
        if writes_preserved_or_mapper_state(&instruction, address, &preserves_address) {
            return Ok(None);
        }
        reads_tracked_control_state |=
            reads_preserved_state(&instruction, address, &preserves_address);
        let mut stack = summary_state.stack;
        match instruction.mnemonic() {
            Mnemonic::Pha | Mnemonic::Php => {
                if stack.len() >= 0x100 {
                    return Ok(None);
                }
                stack.push(CallStackByte::Scratch);
            }
            Mnemonic::Pla | Mnemonic::Plp => {
                let Some(_) = stack.pop() else {
                    return Ok(None);
                };
            }
            _ => {}
        }
        let next_state = |address| CallSummaryState {
            address,
            stack: stack.clone(),
        };

        match rp2a03_direct_control_flow(&instruction, address)? {
            Rp2a03DirectControlFlow::FallThrough { next } => pending.push(next_state(next)),
            Rp2a03DirectControlFlow::Branch {
                target,
                fallthrough,
            } => {
                pending.push(next_state(target));
                if let Some(fallthrough) = fallthrough {
                    pending.push(next_state(fallthrough));
                }
            }
            Rp2a03DirectControlFlow::Jump {
                target: Some(target),
            } => pending.push(next_state(target)),
            Rp2a03DirectControlFlow::Call {
                target,
                return_address,
            } => {
                if stack.len() > 0xFE {
                    return Ok(None);
                }
                stack.push(CallStackByte::DirectReturnHigh(return_address));
                stack.push(CallStackByte::DirectReturnLow(return_address));
                pending.push(CallSummaryState {
                    address: target,
                    stack,
                });
            }
            Rp2a03DirectControlFlow::Return => {
                let (Some(low), Some(high)) = (stack.pop(), stack.pop()) else {
                    return Ok(None);
                };
                match (high, low) {
                    (
                        CallStackByte::DirectReturnHigh(high_return),
                        CallStackByte::DirectReturnLow(low_return),
                    ) if high_return == low_return => pending.push(CallSummaryState {
                        address: high_return,
                        stack,
                    }),
                    (CallStackByte::EntryReturnHigh, CallStackByte::EntryReturnLow)
                        if stack
                            == [
                                CallStackByte::ParentReturnHigh,
                                CallStackByte::ParentReturnLow,
                            ] =>
                    {
                        return_effects.insert(CallReturnEffect::Normal);
                    }
                    (CallStackByte::ParentReturnHigh, CallStackByte::ParentReturnLow)
                        if stack.is_empty() =>
                    {
                        return_effects.insert(CallReturnEffect::EscapeOneCaller);
                    }
                    _ => return Ok(None),
                }
            }
            Rp2a03DirectControlFlow::Jump { target: None }
            | Rp2a03DirectControlFlow::Interrupt
            | Rp2a03DirectControlFlow::Stop => return Ok(None),
        }
    }

    if return_effects.is_empty()
        || reads_tracked_control_state
            && return_effects.contains(&CallReturnEffect::EscapeOneCaller)
    {
        return Ok(None);
    }
    Ok(Some(TrackedStateCallSummary {
        instruction_starts,
        return_effects,
    }))
}

fn reads_preserved_state(
    instruction: &retro_rp2a03::Instruction,
    address: u16,
    preserves_address: &impl Fn(u16) -> bool,
) -> bool {
    let semantics =
        Rp2A03::semantics(instruction, &address).expect("RP2A03 static semantics are infallible");
    semantics.location_accesses.into_iter().any(|access| {
        if access.kind != AccessKind::Read {
            return false;
        }
        let Location::Memory(memory) = access.location else {
            return false;
        };
        match memory {
            MemoryAddress::Direct(target) => preserves_address(target),
            MemoryAddress::Effective { mode, operand } => {
                effective_access_may_touch_preserved_state(mode, operand, preserves_address)
            }
            MemoryAddress::Pointer { .. } | MemoryAddress::InterruptVector => true,
            MemoryAddress::Stack => false,
        }
    })
}

fn effective_access_may_touch_preserved_state(
    mode: AddressingMode,
    operand: Operand,
    preserves_address: &impl Fn(u16) -> bool,
) -> bool {
    match (mode, operand) {
        (AddressingMode::AbsoluteX | AddressingMode::AbsoluteY, Operand::Word(base)) => (0
            ..=u8::MAX)
            .map(|index| base.wrapping_add(u16::from(index)))
            .any(preserves_address),
        (AddressingMode::ZeroPageX | AddressingMode::ZeroPageY, Operand::Byte(base)) => (0
            ..=u8::MAX)
            .map(|index| u16::from(base.wrapping_add(index)))
            .any(preserves_address),
        (
            AddressingMode::ZeroPageIndexedIndirectX | AddressingMode::ZeroPageIndirectIndexedY,
            _,
        ) => true,
        _ => true,
    }
}

fn writes_preserved_or_mapper_state(
    instruction: &retro_rp2a03::Instruction,
    address: u16,
    preserves_address: &impl Fn(u16) -> bool,
) -> bool {
    let semantics =
        Rp2A03::semantics(instruction, &address).expect("RP2A03 static semantics are infallible");
    semantics.location_accesses.into_iter().any(|access| {
        if access.kind != AccessKind::Write {
            return false;
        }
        let Location::Memory(memory) = access.location else {
            return false;
        };
        match memory {
            MemoryAddress::Direct(target) => {
                address_changes_mapper_control(target, preserves_address)
            }
            MemoryAddress::Effective { mode, operand } => {
                effective_write_may_change_mapper_control(mode, operand, preserves_address)
            }
            MemoryAddress::Stack => !matches!(
                instruction.mnemonic(),
                Mnemonic::Jsr | Mnemonic::Pha | Mnemonic::Php
            ),
            MemoryAddress::Pointer { .. } | MemoryAddress::InterruptVector => true,
        }
    })
}

fn effective_write_may_change_mapper_control(
    mode: AddressingMode,
    operand: Operand,
    preserves_address: &impl Fn(u16) -> bool,
) -> bool {
    match (mode, operand) {
        (AddressingMode::AbsoluteX | AddressingMode::AbsoluteY, Operand::Word(base)) => (0
            ..=u8::MAX)
            .map(|index| base.wrapping_add(u16::from(index)))
            .any(|address| address_changes_mapper_control(address, preserves_address)),
        (AddressingMode::ZeroPageX | AddressingMode::ZeroPageY, Operand::Byte(base)) => (0
            ..=u8::MAX)
            .map(|index| u16::from(base.wrapping_add(index)))
            .any(|address| address_changes_mapper_control(address, preserves_address)),
        (
            AddressingMode::ZeroPageIndexedIndirectX | AddressingMode::ZeroPageIndirectIndexedY,
            _,
        ) => true,
        _ => true,
    }
}

fn address_changes_mapper_control(address: u16, preserves_address: &impl Fn(u16) -> bool) -> bool {
    preserves_address(address)
        || decode_source_mmc4_write(address) == Some(SourceMmc4Register::PrgBank)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::rom::{CHR_SIZE, EXPECTED_HEADER, HEADER_SIZE, PRG_SIZE};

    fn synthetic_source(bank: u8, address: u16, program: &[u8]) -> Rom {
        let mut bytes = vec![0_u8; HEADER_SIZE + PRG_SIZE + CHR_SIZE];
        bytes[..HEADER_SIZE].copy_from_slice(&EXPECTED_HEADER);
        let relative = if address >= FIXED_CPU_START {
            usize::from(address - FIXED_CPU_START)
        } else {
            usize::from(address - 0x8000)
        };
        let physical_bank = if address >= FIXED_CPU_START {
            FIXED_PRG_BANK
        } else {
            bank
        };
        let start = HEADER_SIZE + usize::from(physical_bank) * 0x4000 + relative;
        bytes[start..start + program.len()].copy_from_slice(program);
        Rom::parse(bytes).unwrap()
    }

    #[test]
    fn balanced_scratch_stack_does_not_expand_caller_lineages() {
        let source = synthetic_source(
            0x03,
            0x8250,
            &[
                0x8A, // TXA
                0x48, // PHA
                0xA9, 0x00, // LDA #$00
                0x68, // PLA
                0xAA, // TAX
                0x60, // RTS
            ],
        );

        let summary = inspect_tracked_state_call(&source, 0x03, 0x8250)
            .unwrap()
            .expect("balanced scratch stack should be state-transparent");

        assert_eq!(
            summary.instruction_starts(),
            &BTreeSet::from([
                (0x03, 0x8250),
                (0x03, 0x8251),
                (0x03, 0x8252),
                (0x03, 0x8254),
                (0x03, 0x8255),
                (0x03, 0x8256),
            ])
        );
        assert_eq!(
            summary.return_effects(),
            &BTreeSet::from([CallReturnEffect::Normal])
        );
    }

    #[test]
    fn selected_state_summary_allows_unrelated_control_writes_only() {
        let unrelated = synthetic_source(
            0x03,
            0x8250,
            &[
                0xA9, 0x01, // LDA #$01
                0x85, 0x24, // STA $24
                0x60, // RTS
            ],
        );
        let preserved = BTreeSet::from([0x0026, 0x0084]);
        assert!(
            inspect_tracked_state_call(&unrelated, 0x03, 0x8250)
                .unwrap()
                .is_none()
        );
        assert!(
            inspect_selected_state_call(&unrelated, 0x03, 0x8250, &preserved)
                .unwrap()
                .is_some()
        );

        let protected = synthetic_source(
            0x03,
            0x8250,
            &[
                0xA9, 0x01, // LDA #$01
                0x85, 0x84, // STA $84
                0x60, // RTS
            ],
        );
        assert!(
            inspect_selected_state_call(&protected, 0x03, 0x8250, &preserved)
                .unwrap()
                .is_none()
        );
    }

    #[test]
    fn one_caller_frame_escape_is_an_explicit_call_effect() {
        let source = synthetic_source(
            FIXED_PRG_BANK,
            0xC4A2,
            &[
                0x68, // PLA
                0x68, // PLA
                0x60, // RTS
            ],
        );

        let summary = inspect_tracked_state_call(&source, FIXED_PRG_BANK, 0xC4A2)
            .unwrap()
            .expect("one caller-frame escape should be summarized explicitly");
        assert_eq!(
            summary.return_effects(),
            &BTreeSet::from([CallReturnEffect::EscapeOneCaller])
        );
    }

    #[test]
    fn nested_callee_escape_composes_into_its_callers_normal_return() {
        let source = synthetic_source(
            FIXED_PRG_BANK,
            0xC100,
            &[
                0x20, 0x08, 0xC1, // JSR $C108
                0xA9, 0x55, // unreachable caller continuation
                0x85, 0x00, // unreachable caller continuation
                0x60, // unreachable caller return
                0x68, // PLA: discard nested return low
                0x68, // PLA: discard nested return high
                0x60, // RTS through the outer entry frame
            ],
        );

        let summary = inspect_tracked_state_call(&source, FIXED_PRG_BANK, 0xC100)
            .unwrap()
            .expect("nested early return should compose without expanding the caller");

        assert_eq!(
            summary.instruction_starts(),
            &BTreeSet::from([
                (FIXED_PRG_BANK, 0xC100),
                (FIXED_PRG_BANK, 0xC108),
                (FIXED_PRG_BANK, 0xC109),
                (FIXED_PRG_BANK, 0xC10A),
            ])
        );
        assert_eq!(
            summary.return_effects(),
            &BTreeSet::from([CallReturnEffect::Normal])
        );
    }

    #[test]
    fn tracked_state_dependent_escape_requires_the_stateful_tracer() {
        let source = synthetic_source(
            FIXED_PRG_BANK,
            0xE65C,
            &[
                0xAD, 0xCC, 0x05, // LDA $05CC
                0xF0, 0x03, // BEQ $E664
                0x68, // PLA
                0x68, // PLA
                0x60, // RTS from caller
                0x60, // normal RTS
            ],
        );

        assert!(
            inspect_tracked_state_call(&source, FIXED_PRG_BANK, 0xE65C)
                .unwrap()
                .is_none()
        );
    }
}
