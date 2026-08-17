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

const MAXIMUM_SUMMARIZED_INSTRUCTION_COUNT: usize = 4_096;

#[derive(Clone, Debug)]
pub(super) struct StateTransparentCallSummary {
    instruction_starts: BTreeSet<(u8, u16)>,
}

impl StateTransparentCallSummary {
    pub(super) fn instruction_starts(&self) -> &BTreeSet<(u8, u16)> {
        &self.instruction_starts
    }
}

/// Finds a normal-returning direct call graph whose instructions cannot change any state used to
/// resolve PRG mappings or scheduler control. Its register result is deliberately not summarized;
/// the caller must invalidate A/X/Y and flags when applying this summary.
pub(super) fn inspect_state_transparent_call(
    source: &Rom,
    entry_bank: u8,
    entry_address: u16,
) -> Result<Option<StateTransparentCallSummary>> {
    let mut pending = vec![entry_address];
    let mut instruction_starts = BTreeSet::new();
    let mut saw_return = false;

    while let Some(address) = pending.pop() {
        if address < 0x8000 {
            return Ok(None);
        }
        let physical_bank = if address >= FIXED_CPU_START {
            FIXED_PRG_BANK
        } else {
            entry_bank
        };
        if !instruction_starts.insert((physical_bank, address)) {
            continue;
        }
        if instruction_starts.len() > MAXIMUM_SUMMARIZED_INSTRUCTION_COUNT {
            return Ok(None);
        }

        let instruction = decode_bytes(&source_instruction_bytes(
            source,
            physical_bank,
            address,
            3,
        )?)
        .with_context(|| {
            format!("decode state-transparent call at {physical_bank:02X}:${address:04X}")
        })?;
        if !instruction.opcode_is_documented() || manipulates_software_stack(instruction.mnemonic())
        {
            return Ok(None);
        }
        if writes_mapper_control_state(&instruction, address) {
            return Ok(None);
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
            Rp2a03DirectControlFlow::Jump {
                target: Some(target),
            } => pending.push(target),
            Rp2a03DirectControlFlow::Call {
                target,
                return_address,
            } => {
                pending.push(target);
                pending.push(return_address);
            }
            Rp2a03DirectControlFlow::Return => saw_return = true,
            Rp2a03DirectControlFlow::Jump { target: None }
            | Rp2a03DirectControlFlow::Interrupt
            | Rp2a03DirectControlFlow::Stop => return Ok(None),
        }
    }

    Ok(saw_return.then_some(StateTransparentCallSummary { instruction_starts }))
}

fn manipulates_software_stack(mnemonic: Mnemonic) -> bool {
    matches!(
        mnemonic,
        Mnemonic::Pha
            | Mnemonic::Php
            | Mnemonic::Pla
            | Mnemonic::Plp
            | Mnemonic::Tsx
            | Mnemonic::Txs
    )
}

fn writes_mapper_control_state(instruction: &retro_rp2a03::Instruction, address: u16) -> bool {
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
            MemoryAddress::Direct(target) => address_changes_mapper_control(target),
            MemoryAddress::Effective { mode, operand } => {
                effective_write_may_change_mapper_control(mode, operand)
            }
            MemoryAddress::Stack => instruction.mnemonic() != Mnemonic::Jsr,
            MemoryAddress::Pointer { .. } | MemoryAddress::InterruptVector => true,
        }
    })
}

fn effective_write_may_change_mapper_control(mode: AddressingMode, operand: Operand) -> bool {
    match (mode, operand) {
        (AddressingMode::AbsoluteX | AddressingMode::AbsoluteY, Operand::Word(base)) => (0
            ..=u8::MAX)
            .map(|index| base.wrapping_add(u16::from(index)))
            .any(address_changes_mapper_control),
        (AddressingMode::ZeroPageX | AddressingMode::ZeroPageY, Operand::Byte(base)) => (0
            ..=u8::MAX)
            .map(|index| u16::from(base.wrapping_add(index)))
            .any(address_changes_mapper_control),
        (
            AddressingMode::ZeroPageIndexedIndirectX | AddressingMode::ZeroPageIndirectIndexedY,
            _,
        ) => true,
        _ => true,
    }
}

fn address_changes_mapper_control(address: u16) -> bool {
    ResetTraceState::tracks_memory_address(address)
        || decode_source_mmc4_write(address) == Some(SourceMmc4Register::PrgBank)
}
