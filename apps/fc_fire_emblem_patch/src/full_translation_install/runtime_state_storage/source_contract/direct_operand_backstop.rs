use std::collections::BTreeSet;

use anyhow::{Context, Result, ensure};
use retro_rp2a03::{Location, MemoryAddress, Operand, Rp2A03, decode_bytes};
use serde::Serialize;
use typed_isa_core::StaticSemantics;

use crate::{
    dialogue_inventory::inspect_main_dialogue_storage,
    rom::{HEADER_SIZE, Rom},
    sha1_hex,
    typed_source::decode_rp2a03_sequence,
};

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

const BANK_ZERO_GRAPHIC_RECORD_FALSE_POSITIVES: [GraphicRecordSpec; 4] = [
    GraphicRecordSpec {
        candidate: RawDirectOperandCandidate {
            bank: 0x00,
            address: 0x911E,
            opcode: 0xFE,
            operand: 0x07FF,
        },
        pointer_cell_address: 0x8FA7,
        record_start: 0x9115,
        record_end_exclusive: 0x913C,
    },
    GraphicRecordSpec {
        candidate: RawDirectOperandCandidate {
            bank: 0x00,
            address: 0x998B,
            opcode: 0xB9,
            operand: 0x07FF,
        },
        pointer_cell_address: 0x902D,
        record_start: 0x9985,
        record_end_exclusive: 0x99B2,
    },
    GraphicRecordSpec {
        candidate: RawDirectOperandCandidate {
            bank: 0x00,
            address: 0x99EC,
            opcode: 0xB9,
            operand: 0x07FF,
        },
        pointer_cell_address: 0x9031,
        record_start: 0x99D4,
        record_end_exclusive: 0x99F6,
    },
    GraphicRecordSpec {
        candidate: RawDirectOperandCandidate {
            bank: 0x00,
            address: 0x9A3B,
            opcode: 0x59,
            operand: 0x07FF,
        },
        pointer_cell_address: 0x9035,
        record_start: 0x9A22,
        record_end_exclusive: 0x9A57,
    },
];

const MAIN_DIALOGUE_DATA_FALSE_POSITIVES: [RawDirectOperandCandidate; 3] = [
    RawDirectOperandCandidate {
        bank: 0x07,
        address: 0xA265,
        opcode: 0x2E,
        operand: 0x07FF,
    },
    RawDirectOperandCandidate {
        bank: 0x07,
        address: 0xA34D,
        opcode: 0x2C,
        operand: 0x07FF,
    },
    RawDirectOperandCandidate {
        bank: 0x0C,
        address: 0x8B43,
        opcode: 0x19,
        operand: 0x07FF,
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
    exact_data_false_positive_count: usize,
    exact_data_false_positives: Vec<ExactDataFalsePositive>,
    every_instruction_candidate_bound_as_instruction_interior: bool,
    every_data_candidate_bound_to_an_exact_owner: bool,
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

#[derive(Clone, Copy)]
struct GraphicRecordSpec {
    candidate: RawDirectOperandCandidate,
    pointer_cell_address: u16,
    record_start: u16,
    record_end_exclusive: u16,
}

#[derive(Serialize)]
struct ExactDataFalsePositive {
    role: String,
    prg_bank_hex: String,
    raw_cpu_address_hex: String,
    raw_opcode_hex: String,
    raw_operand_hex: String,
    owner_cpu_or_file_range_hex: String,
    owner_sha1: String,
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
        .chain(
            BANK_ZERO_GRAPHIC_RECORD_FALSE_POSITIVES
                .iter()
                .map(|spec| spec.candidate),
        )
        .chain(MAIN_DIALOGUE_DATA_FALSE_POSITIVES)
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

    let mut exact_data_false_positives = bind_bank_zero_graphic_records(source)?;
    exact_data_false_positives.extend(bind_main_dialogue_data_candidates(source)?);
    ensure!(
        exact_data_false_positives.len()
            == BANK_ZERO_GRAPHIC_RECORD_FALSE_POSITIVES.len()
                + MAIN_DIALOGUE_DATA_FALSE_POSITIVES.len(),
        "whole-PRG exact-data candidate binding lost coverage"
    );

    Ok(WholePrgDirectOperandBackstop {
        strategy: "scan every three-byte window for documented absolute memory opcodes targeting the candidate, then partition every raw hit as either a typed instruction interior or exact source-owned data",
        scanned_prg_bank_count: PRG_BANK_COUNT,
        scanned_prg_byte_count: PRG_BANK_COUNT * PRG_BANK_SIZE,
        raw_candidate_count: raw_candidates.len(),
        instruction_interior_false_positive_count: instruction_interior_false_positives.len(),
        instruction_interior_false_positives,
        exact_data_false_positive_count: exact_data_false_positives.len(),
        exact_data_false_positives,
        every_instruction_candidate_bound_as_instruction_interior: true,
        every_data_candidate_bound_to_an_exact_owner: true,
        whole_prg_direct_accesses_exclude_candidate: true,
    })
}

fn bind_bank_zero_graphic_records(source: &Rom) -> Result<Vec<ExactDataFalsePositive>> {
    BANK_ZERO_GRAPHIC_RECORD_FALSE_POSITIVES
        .iter()
        .map(|spec| {
            let pointer_cell = source_bytes(source, 0x00, spec.pointer_cell_address, 4)?;
            let expected_pointer_cell = [
                spec.record_start as u8,
                (spec.record_start >> 8) as u8,
                spec.record_end_exclusive as u8,
                (spec.record_end_exclusive >> 8) as u8,
            ];
            ensure!(
                pointer_cell == expected_pointer_cell,
                "bank-00 graphic record pointer boundary changed at {:04X}",
                spec.pointer_cell_address
            );
            let record_byte_count = usize::from(
                spec.record_end_exclusive
                    .checked_sub(spec.record_start)
                    .context("bank-00 graphic record range reversed")?,
            );
            let record = source_bytes(source, 0x00, spec.record_start, record_byte_count)?;
            ensure!(
                spec.candidate.address >= spec.record_start
                    && spec
                        .candidate
                        .address
                        .checked_add(3)
                        .is_some_and(|end| end <= spec.record_end_exclusive),
                "bank-00 raw candidate is outside its pointer-delimited graphic record"
            );
            bind_candidate_window(source, spec.candidate)?;
            Ok(ExactDataFalsePositive {
                role: format!(
                    "bank-00 pointer-delimited graphic record at 0x{:04X}",
                    spec.pointer_cell_address
                ),
                prg_bank_hex: "0x00".to_owned(),
                raw_cpu_address_hex: format!("0x{:04X}", spec.candidate.address),
                raw_opcode_hex: format!("0x{:02X}", spec.candidate.opcode),
                raw_operand_hex: format!("0x{:04X}", spec.candidate.operand),
                owner_cpu_or_file_range_hex: format!(
                    "0x{:04X}..0x{:04X}",
                    spec.record_start, spec.record_end_exclusive
                ),
                owner_sha1: sha1_hex(record),
            })
        })
        .collect()
}

fn bind_main_dialogue_data_candidates(source: &Rom) -> Result<Vec<ExactDataFalsePositive>> {
    let inspection = inspect_main_dialogue_storage(source.data())?;
    MAIN_DIALOGUE_DATA_FALSE_POSITIVES
        .iter()
        .map(|candidate| {
            bind_candidate_window(source, *candidate)?;
            let file_offset = source_file_offset(candidate.bank, candidate.address)?;
            let owners = inspection
                .records
                .iter()
                .flat_map(|record| {
                    record
                        .lines
                        .iter()
                        .enumerate()
                        .filter_map(move |(line_index, line)| {
                            let end = line.file_offset.checked_add(line.storage_byte_count)?;
                            (file_offset >= line.file_offset && file_offset + 3 <= end)
                                .then_some((record, line_index, line, end))
                        })
                })
                .collect::<Vec<_>>();
            ensure!(
                owners.len() == 1,
                "dialogue raw candidate {:02X}:{:04X} belongs to {} source lines",
                candidate.bank,
                candidate.address,
                owners.len()
            );
            let (record, line_index, line, end) = owners[0];
            Ok(ExactDataFalsePositive {
                role: format!("{} line {line_index}", record.table_id),
                prg_bank_hex: format!("0x{:02X}", candidate.bank),
                raw_cpu_address_hex: format!("0x{:04X}", candidate.address),
                raw_opcode_hex: format!("0x{:02X}", candidate.opcode),
                raw_operand_hex: format!("0x{:04X}", candidate.operand),
                owner_cpu_or_file_range_hex: format!("0x{:05X}..0x{end:05X}", line.file_offset),
                owner_sha1: line.storage_sha1.clone(),
            })
        })
        .collect()
}

fn bind_candidate_window(source: &Rom, candidate: RawDirectOperandCandidate) -> Result<()> {
    let actual = source_bytes(source, candidate.bank, candidate.address, 3)?;
    let [low, high] = candidate.operand.to_le_bytes();
    ensure!(
        actual == [candidate.opcode, low, high],
        "raw direct-operand candidate changed at {:02X}:{:04X}",
        candidate.bank,
        candidate.address
    );
    Ok(())
}

fn source_file_offset(bank: u8, address: u16) -> Result<usize> {
    let base = if bank == FIXED_BANK { 0xC000 } else { 0x8000 };
    ensure!(address >= base, "source address precedes its PRG window");
    HEADER_SIZE
        .checked_add(usize::from(bank) * PRG_BANK_SIZE)
        .and_then(|offset| offset.checked_add(usize::from(address - base)))
        .context("source file offset overflow")
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
