use std::collections::BTreeSet;

use anyhow::{Context, Result, ensure};
use retro_rp2a03::{
    AddressingMode, Location, MemoryAddress, Mnemonic, Operand, Rp2A03, decode_bytes,
};
use typed_isa_core::StaticSemantics;

use crate::{
    rom::{PRG_SIZE, Rom},
    sha1_hex,
};

use super::{
    AbsoluteOperandCandidate, PRG_BANK_SIZE, RecordPointerCandidate, TERRAIN_NAME_COUNT,
    false_positive_regions, prg_source_bytes,
};

pub(super) const TERRAIN_POINTER_TABLE_START: u16 = 0xE5F1;
const TERRAIN_POINTER_TABLE_END: u16 = 0xE610;
const TERRAIN_POINTER_TABLE_BYTE_COUNT: usize = TERRAIN_NAME_COUNT * 2;
const TERRAIN_POINTER_TABLE_SHA1: &str = "41ea0bc691d5111689ecf935ac684ad82ba33451";
pub(super) const TERRAIN_POINTER_TABLE: [u8; TERRAIN_POINTER_TABLE_BYTE_COUNT] = [
    0x11, 0xE6, 0x17, 0xE6, 0x1B, 0xE6, 0x1F, 0xE6, 0x24, 0xE6, 0x28, 0xE6, 0x2C, 0xE6, 0x30, 0xE6,
    0x35, 0xE6, 0x39, 0xE6, 0x3D, 0xE6, 0x41, 0xE6, 0x45, 0xE6, 0x49, 0xE6, 0x4F, 0xE6, 0x56, 0xE6,
];

pub(super) const TERRAIN_POINTER_REFERENCES: [AbsoluteOperandCandidate; 2] = [
    AbsoluteOperandCandidate {
        target: 0xE5F1,
        prg_bank: 0x07,
        cpu_address: 0x8487,
        opcode: 0xB9,
    },
    AbsoluteOperandCandidate {
        target: 0xE5F2,
        prg_bank: 0x07,
        cpu_address: 0x848C,
        opcode: 0xB9,
    },
];

pub(super) fn bind_known_terrain_source_references(rom: &Rom) -> Result<()> {
    let prg = rom.prg();
    ensure!(
        prg.len() == PRG_SIZE,
        "terrain reference census PRG size changed"
    );

    let actual_references = scan_terrain_pointer_reference_candidates(prg)?;
    bind_terrain_pointer_reference_candidates(&actual_references)?;
    for reference in TERRAIN_POINTER_REFERENCES {
        let instruction = decode_candidate(reference)?;
        ensure!(
            instruction.opcode_is_documented()
                && instruction.mnemonic() == Mnemonic::Lda
                && instruction.addressing_mode() == AddressingMode::AbsoluteY
                && instruction.operand() == Operand::Word(reference.target),
            "terrain-name pointer reference is not a documented typed LDA absolute,Y at {:02X}:${:04X}",
            reference.prg_bank,
            reference.cpu_address
        );
    }

    bind_unique_terrain_pointer_table(prg)?;
    bind_terrain_record_pointer_pair_census(prg)?;
    false_positive_regions::bind_false_positive_regions(rom)?;
    Ok(())
}

pub(super) fn bind_terrain_pointer_reference_candidates(
    actual: &BTreeSet<AbsoluteOperandCandidate>,
) -> Result<()> {
    let expected_references = expected_raw_candidates();
    ensure!(
        *actual == expected_references,
        "terrain-name pointer reference census changed: expected {expected_references:?}, found {actual:?}"
    );
    Ok(())
}

pub(super) fn expected_raw_candidates() -> BTreeSet<AbsoluteOperandCandidate> {
    TERRAIN_POINTER_REFERENCES
        .into_iter()
        .chain(false_positive_regions::absolute_operand_candidates())
        .collect()
}

pub(super) fn scan_terrain_pointer_reference_candidates(
    prg: &[u8],
) -> Result<BTreeSet<AbsoluteOperandCandidate>> {
    let mut candidates = BTreeSet::new();
    for bank_index in 0..PRG_SIZE / PRG_BANK_SIZE {
        let prg_bank = u8::try_from(bank_index).context("terrain reference PRG bank overflow")?;
        let bank = &prg[bank_index * PRG_BANK_SIZE..(bank_index + 1) * PRG_BANK_SIZE];
        let cpu_base: u16 = if prg_bank == 0x0F { 0xC000 } else { 0x8000 };
        for (relative, bytes) in bank.windows(3).enumerate() {
            let target = u16::from_le_bytes([bytes[1], bytes[2]]);
            if !(TERRAIN_POINTER_TABLE_START..=TERRAIN_POINTER_TABLE_END).contains(&target)
                || !documented_absolute_memory_operand(bytes[0], target)
            {
                continue;
            }
            let cpu_address = cpu_base
                .checked_add(
                    u16::try_from(relative).context("terrain reference bank offset overflow")?,
                )
                .context("terrain reference CPU address overflow")?;
            candidates.insert(AbsoluteOperandCandidate {
                target,
                prg_bank,
                cpu_address,
                opcode: bytes[0],
            });
        }
    }
    Ok(candidates)
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

fn decode_candidate(candidate: AbsoluteOperandCandidate) -> Result<retro_rp2a03::Instruction> {
    let [low, high] = candidate.target.to_le_bytes();
    decode_bytes(&[candidate.opcode, low, high])
        .context("decode terrain-table raw reference through typed RP2A03 ISA")
}

pub(super) fn bind_unique_terrain_pointer_table(prg: &[u8]) -> Result<()> {
    let table = prg_source_bytes(
        prg,
        0x0F,
        TERRAIN_POINTER_TABLE_START,
        TERRAIN_POINTER_TABLE_BYTE_COUNT,
    )?;
    ensure!(
        table == TERRAIN_POINTER_TABLE && sha1_hex(table) == TERRAIN_POINTER_TABLE_SHA1,
        "terrain-name pointer table changed"
    );
    ensure!(
        prg.windows(table.len())
            .filter(|window| *window == table)
            .count()
            == 1,
        "terrain-name pointer table storage is not unique in PRG"
    );
    Ok(())
}

pub(super) fn bind_terrain_record_pointer_pair_census(prg: &[u8]) -> Result<()> {
    let record_targets = terrain_record_targets();
    ensure!(
        record_targets.len() == TERRAIN_NAME_COUNT,
        "terrain-name record target population changed"
    );
    let actual = scan_terrain_record_pointer_candidates(prg, &record_targets)?;
    bind_terrain_record_pointer_candidates(&actual)?;
    Ok(())
}

pub(super) fn bind_terrain_record_pointer_candidates(
    actual: &BTreeSet<RecordPointerCandidate>,
) -> Result<()> {
    let canonical = TERRAIN_POINTER_TABLE
        .chunks_exact(2)
        .enumerate()
        .map(|(index, bytes)| RecordPointerCandidate {
            target: u16::from_le_bytes([bytes[0], bytes[1]]),
            prg_bank: 0x0F,
            cpu_address: TERRAIN_POINTER_TABLE_START + u16::try_from(index * 2).unwrap(),
        });
    let expected = canonical
        .chain(false_positive_regions::record_pointer_candidates())
        .collect::<BTreeSet<_>>();
    ensure!(
        *actual == expected,
        "terrain-name record-pointer pair census changed: expected {expected:?}, found {actual:?}"
    );
    Ok(())
}

pub(super) fn terrain_record_targets() -> BTreeSet<u16> {
    TERRAIN_POINTER_TABLE
        .chunks_exact(2)
        .map(|bytes| u16::from_le_bytes([bytes[0], bytes[1]]))
        .collect()
}

pub(super) fn scan_terrain_record_pointer_candidates(
    prg: &[u8],
    record_targets: &BTreeSet<u16>,
) -> Result<BTreeSet<RecordPointerCandidate>> {
    let mut candidates = BTreeSet::new();
    for bank_index in 0..PRG_SIZE / PRG_BANK_SIZE {
        let prg_bank =
            u8::try_from(bank_index).context("terrain record-pointer PRG bank overflow")?;
        let bank = &prg[bank_index * PRG_BANK_SIZE..(bank_index + 1) * PRG_BANK_SIZE];
        let cpu_base: u16 = if prg_bank == 0x0F { 0xC000 } else { 0x8000 };
        for (relative, bytes) in bank.windows(2).enumerate() {
            let target = u16::from_le_bytes([bytes[0], bytes[1]]);
            if !record_targets.contains(&target) {
                continue;
            }
            let cpu_address = cpu_base
                .checked_add(
                    u16::try_from(relative)
                        .context("terrain record-pointer bank offset overflow")?,
                )
                .context("terrain record-pointer CPU address overflow")?;
            candidates.insert(RecordPointerCandidate {
                target,
                prg_bank,
                cpu_address,
            });
        }
    }
    Ok(candidates)
}
