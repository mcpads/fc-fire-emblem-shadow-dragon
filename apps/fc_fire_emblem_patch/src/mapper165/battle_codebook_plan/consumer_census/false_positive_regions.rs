use anyhow::{Result, ensure};

use crate::{rom::Rom, sha1_hex, typed_source::decode_rp2a03_sequence};

#[cfg(test)]
use super::PRG_BANK_SIZE;
use super::{
    AbsoluteOperandCandidate, RecordPointerCandidate, dialogue_false_positives, prg_source_bytes,
};

#[derive(Clone, Copy)]
struct InstructionInteriorSpec {
    candidate: AbsoluteOperandCandidate,
    containing_address: u16,
    containing_sequence: &'static [u8],
}

#[derive(Clone, Copy)]
struct RecordPointerInstructionInteriorSpec {
    candidate: RecordPointerCandidate,
    containing_address: u16,
    containing_sequence: &'static [u8],
}

const INSTRUCTION_INTERIOR_FALSE_POSITIVES: [InstructionInteriorSpec; 3] = [
    InstructionInteriorSpec {
        candidate: AbsoluteOperandCandidate {
            target: 0xE605,
            prg_bank: 0x03,
            cpu_address: 0x9A7C,
            opcode: 0x3E,
        },
        containing_address: 0x9A79,
        containing_sequence: &[0xA9, 0x00, 0x8D, 0x3E, 0x05, 0xE6, 0x24, 0x60],
    },
    InstructionInteriorSpec {
        candidate: AbsoluteOperandCandidate {
            target: 0xE605,
            prg_bank: 0x06,
            cpu_address: 0xB003,
            opcode: 0x0D,
        },
        containing_address: 0xB000,
        containing_sequence: &[0xA9, 0x06, 0x8D, 0x0D, 0x05, 0xE6, 0x84, 0x60],
    },
    InstructionInteriorSpec {
        candidate: AbsoluteOperandCandidate {
            target: 0xE605,
            prg_bank: 0x06,
            cpu_address: 0xB6C4,
            opcode: 0xEE,
        },
        containing_address: 0xB6C1,
        containing_sequence: &[0xA9, 0x00, 0x8D, 0xEE, 0x05, 0xE6, 0x84, 0x60],
    },
];

const RECORD_POINTER_INSTRUCTION_INTERIOR_FALSE_POSITIVE: RecordPointerInstructionInteriorSpec =
    RecordPointerInstructionInteriorSpec {
        candidate: RecordPointerCandidate {
            target: 0xE639,
            prg_bank: 0x04,
            cpu_address: 0x9EDA,
        },
        containing_address: 0x9ED6,
        containing_sequence: &[0xAD, 0x32, 0x77, 0xF0, 0x39, 0xE6, 0xCA],
    };

pub(super) const TITLE_STREAM_RAW_CANDIDATE: AbsoluteOperandCandidate = AbsoluteOperandCandidate {
    target: 0xE5FF,
    prg_bank: 0x0D,
    cpu_address: 0xB3A3,
    opcode: 0xEC,
};
pub(super) const TITLE_FOLLOWUP_STREAM_ADDRESS: u16 = 0xB39B;
pub(super) const TITLE_FOLLOWUP_STREAM: [u8; 25] = [
    0x22, 0xA2, 0x0E, 0xFD, 0xEB, 0xED, 0xEE, 0xEE, 0xEC, 0xFF, 0xE5, 0xE6, 0xE7, 0xE8, 0xE9, 0xEA,
    0xFE, 0x23, 0xE8, 0x04, 0xFF, 0xFF, 0xFF, 0xFF, 0x00,
];
const TITLE_FOLLOWUP_STREAM_SHA1: &str = "2e7d2a0d10e30a67b9cfb38bd4bd1118cd92f674";

pub(super) fn absolute_operand_candidates() -> impl Iterator<Item = AbsoluteOperandCandidate> {
    INSTRUCTION_INTERIOR_FALSE_POSITIVES
        .iter()
        .map(|spec| spec.candidate)
        .chain(dialogue_false_positives::absolute_operand_candidates())
        .chain([TITLE_STREAM_RAW_CANDIDATE])
}

pub(super) fn record_pointer_candidates() -> impl Iterator<Item = RecordPointerCandidate> {
    [RECORD_POINTER_INSTRUCTION_INTERIOR_FALSE_POSITIVE.candidate]
        .into_iter()
        .chain(dialogue_false_positives::record_pointer_candidates())
}

pub(super) fn bind_false_positive_regions(rom: &Rom) -> Result<()> {
    bind_instruction_interior_false_positives(rom.prg())?;
    dialogue_false_positives::bind_dialogue_data_false_positives(rom.data())?;
    bind_title_followup_stream_false_positive(rom.prg())?;
    bind_record_pointer_instruction_interior_false_positive(rom.prg())?;
    Ok(())
}

fn bind_instruction_interior_false_positives(prg: &[u8]) -> Result<()> {
    for spec in INSTRUCTION_INTERIOR_FALSE_POSITIVES {
        let bytes = prg_source_bytes(
            prg,
            spec.candidate.prg_bank,
            spec.containing_address,
            spec.containing_sequence.len(),
        )?;
        ensure!(
            bytes == spec.containing_sequence,
            "terrain-table raw candidate containing instruction changed at {:02X}:${:04X}",
            spec.candidate.prg_bank,
            spec.containing_address
        );
        decode_rp2a03_sequence(
            bytes,
            spec.containing_address,
            "terrain-table instruction-interior false positive",
        )?;
    }
    Ok(())
}

fn bind_record_pointer_instruction_interior_false_positive(prg: &[u8]) -> Result<()> {
    let spec = RECORD_POINTER_INSTRUCTION_INTERIOR_FALSE_POSITIVE;
    let bytes = prg_source_bytes(
        prg,
        spec.candidate.prg_bank,
        spec.containing_address,
        spec.containing_sequence.len(),
    )?;
    ensure!(
        bytes == spec.containing_sequence,
        "terrain record-pointer raw candidate containing instruction changed"
    );
    decode_rp2a03_sequence(
        bytes,
        spec.containing_address,
        "terrain record-pointer instruction-interior false positive",
    )?;
    Ok(())
}

pub(super) fn bind_title_followup_stream_false_positive(prg: &[u8]) -> Result<()> {
    let stream = prg_source_bytes(
        prg,
        TITLE_STREAM_RAW_CANDIDATE.prg_bank,
        TITLE_FOLLOWUP_STREAM_ADDRESS,
        TITLE_FOLLOWUP_STREAM.len(),
    )?;
    ensure!(
        stream == TITLE_FOLLOWUP_STREAM && sha1_hex(stream) == TITLE_FOLLOWUP_STREAM_SHA1,
        "title follow-up PPU stream changed"
    );
    let mut cursor = 0_usize;
    for (address, length) in [(0x22A2_u16, 0x0E_usize), (0x23E8_u16, 0x04_usize)] {
        ensure!(
            stream.get(cursor..cursor + 3)
                == Some(&[
                    address.to_be_bytes()[0],
                    address.to_be_bytes()[1],
                    length as u8,
                ]),
            "title follow-up PPU command header changed"
        );
        cursor += 3 + length;
    }
    ensure!(
        cursor + 1 == stream.len() && stream[cursor] == 0,
        "title follow-up PPU stream terminator changed"
    );
    Ok(())
}

#[cfg(test)]
pub(super) fn populate_synthetic_false_positive_regions(prg: &mut [u8]) {
    for spec in INSTRUCTION_INTERIOR_FALSE_POSITIVES {
        put(
            prg,
            spec.candidate.prg_bank,
            spec.containing_address,
            spec.containing_sequence,
        );
    }
    let record_spec = RECORD_POINTER_INSTRUCTION_INTERIOR_FALSE_POSITIVE;
    put(
        prg,
        record_spec.candidate.prg_bank,
        record_spec.containing_address,
        record_spec.containing_sequence,
    );
    dialogue_false_positives::populate_synthetic_dialogue_regions(prg);
    put(
        prg,
        TITLE_STREAM_RAW_CANDIDATE.prg_bank,
        TITLE_FOLLOWUP_STREAM_ADDRESS,
        &TITLE_FOLLOWUP_STREAM,
    );
}

#[cfg(test)]
fn put(prg: &mut [u8], bank: u8, address: u16, bytes: &[u8]) {
    let cpu_base: u16 = if bank == 0x0F { 0xC000 } else { 0x8000 };
    let offset = usize::from(bank) * PRG_BANK_SIZE + usize::from(address - cpu_base);
    prg[offset..offset + bytes.len()].copy_from_slice(bytes);
}
