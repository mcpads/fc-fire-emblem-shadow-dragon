use std::collections::BTreeSet;

use anyhow::{Context, Result, ensure};
use retro_rp2a03::{Location, MemoryAddress, Operand, Rp2A03, decode_bytes};
use serde::Serialize;
use typed_isa_core::StaticSemantics;

use crate::{rom::Rom, sha1_hex, typed_source::decode_rp2a03_sequence};

use super::super::{CANDIDATE_END, CANDIDATE_START};
use super::{FIXED_BANK, PRG_BANK_SIZE, source_bytes};

const INSTRUCTION_INTERIOR_FALSE_POSITIVES: [InstructionInteriorSpec; 3] = [
    InstructionInteriorSpec {
        candidate: RawDirectOperandCandidate {
            bank: 0x03,
            address: 0x8D58,
            opcode: 0x8E,
            operand: 0x07F0,
        },
        containing_address: 0x8D56,
        containing_sequence: &[0x20, 0xC4, 0x8E, 0xF0, 0x07],
    },
    InstructionInteriorSpec {
        candidate: RawDirectOperandCandidate {
            bank: 0x04,
            address: 0x83A2,
            opcode: 0x79,
            operand: 0x07F0,
        },
        containing_address: 0x83A0,
        containing_sequence: &[0xAD, 0x38, 0x79, 0xF0, 0x07],
    },
    InstructionInteriorSpec {
        candidate: RawDirectOperandCandidate {
            bank: 0x05,
            address: 0x8ECC,
            opcode: 0x1E,
            operand: 0x07F0,
        },
        containing_address: 0x8ECB,
        containing_sequence: &[0xC9, 0x1E, 0xF0, 0x07],
    },
];

#[derive(Serialize)]
pub(super) struct WholePrgDirectOperandBackstop {
    strategy: &'static str,
    scanned_prg_bank_count: usize,
    scanned_prg_byte_count: usize,
    raw_candidate_count: usize,
    instruction_interior_false_positive_count: usize,
    instruction_interior_false_positives: Vec<InstructionInteriorFalsePositive>,
    every_raw_candidate_bound_as_instruction_interior: bool,
    whole_prg_direct_accesses_exclude_candidate: bool,
}

impl WholePrgDirectOperandBackstop {
    pub(super) fn excludes_candidate(&self) -> bool {
        self.whole_prg_direct_accesses_exclude_candidate
    }
}

#[derive(Serialize)]
struct InstructionInteriorFalsePositive {
    prg_bank_hex: String,
    raw_cpu_address_hex: String,
    raw_opcode_hex: String,
    raw_operand_hex: String,
    containing_instruction_cpu_address_hex: String,
    containing_sequence_sha1: String,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
struct RawDirectOperandCandidate {
    bank: u8,
    address: u16,
    opcode: u8,
    operand: u16,
}

#[derive(Clone, Copy)]
struct InstructionInteriorSpec {
    candidate: RawDirectOperandCandidate,
    containing_address: u16,
    containing_sequence: &'static [u8],
}

pub(super) fn bind_whole_prg_direct_operand_backstop(
    source: &Rom,
) -> Result<WholePrgDirectOperandBackstop> {
    const PRG_BANK_COUNT: usize = 16;

    let mut raw_candidates = BTreeSet::new();
    for bank in 0..PRG_BANK_COUNT {
        let bank = u8::try_from(bank).expect("PRG bank count fits u8");
        let base = if bank == FIXED_BANK { 0xC000 } else { 0x8000 };
        let bytes = source_bytes(source, bank, base, PRG_BANK_SIZE)?;
        for (relative, window) in bytes.windows(3).enumerate() {
            let opcode = window[0];
            let operand = u16::from_le_bytes([window[1], window[2]]);
            if (CANDIDATE_START..=CANDIDATE_END).contains(&operand)
                && documented_absolute_memory_operand(opcode, operand)
            {
                raw_candidates.insert(RawDirectOperandCandidate {
                    bank,
                    address: base
                        .checked_add(
                            u16::try_from(relative).context("raw PRG scan address overflow")?,
                        )
                        .context("raw PRG scan address overflow")?,
                    opcode,
                    operand,
                });
            }
        }
    }

    let expected_candidates = INSTRUCTION_INTERIOR_FALSE_POSITIVES
        .iter()
        .map(|spec| spec.candidate)
        .collect::<BTreeSet<_>>();
    ensure!(
        raw_candidates == expected_candidates,
        "whole-PRG direct candidate census changed: expected {expected_candidates:?}, found {raw_candidates:?}"
    );

    let mut instruction_interior_false_positives = Vec::new();
    for spec in INSTRUCTION_INTERIOR_FALSE_POSITIVES {
        let bytes = source_bytes(
            source,
            spec.candidate.bank,
            spec.containing_address,
            spec.containing_sequence.len(),
        )?;
        ensure!(
            bytes == spec.containing_sequence,
            "whole-PRG raw candidate containing sequence changed at {:02X}:{:04X}",
            spec.candidate.bank,
            spec.containing_address
        );
        decode_rp2a03_sequence(
            bytes,
            spec.containing_address,
            "whole-PRG direct-operand false positive",
        )?;
        instruction_interior_false_positives.push(InstructionInteriorFalsePositive {
            prg_bank_hex: format!("0x{:02X}", spec.candidate.bank),
            raw_cpu_address_hex: format!("0x{:04X}", spec.candidate.address),
            raw_opcode_hex: format!("0x{:02X}", spec.candidate.opcode),
            raw_operand_hex: format!("0x{:04X}", spec.candidate.operand),
            containing_instruction_cpu_address_hex: format!("0x{:04X}", spec.containing_address),
            containing_sequence_sha1: sha1_hex(bytes),
        });
    }

    Ok(WholePrgDirectOperandBackstop {
        strategy: "scan every three-byte window for documented absolute memory opcodes targeting the candidate, then source-bind and typed-decode every raw hit from its real instruction boundary",
        scanned_prg_bank_count: PRG_BANK_COUNT,
        scanned_prg_byte_count: PRG_BANK_COUNT * PRG_BANK_SIZE,
        raw_candidate_count: raw_candidates.len(),
        instruction_interior_false_positive_count: instruction_interior_false_positives.len(),
        instruction_interior_false_positives,
        every_raw_candidate_bound_as_instruction_interior: true,
        whole_prg_direct_accesses_exclude_candidate: true,
    })
}

fn documented_absolute_memory_operand(opcode: u8, operand: u16) -> bool {
    let [low, high] = operand.to_le_bytes();
    let Ok(instruction) = decode_bytes(&[opcode, low, high]) else {
        return false;
    };
    if !instruction.opcode_is_documented() {
        return false;
    }
    let semantics =
        Rp2A03::semantics(&instruction, &0_u16).expect("RP2A03 static semantics are infallible");

    semantics.location_accesses.into_iter().any(|access| {
        matches!(
            access.location,
            Location::Memory(MemoryAddress::Direct(target)) if target == operand
        ) || matches!(
            access.location,
            Location::Memory(MemoryAddress::Effective {
                operand: Operand::Word(target),
                ..
            }) if target == operand
        )
    })
}
