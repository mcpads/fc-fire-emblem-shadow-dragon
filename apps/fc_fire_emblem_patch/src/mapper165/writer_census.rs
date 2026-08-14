use std::collections::BTreeSet;

use anyhow::{Context, Result, ensure};
use serde::Serialize;

use crate::{
    mmc5_prg::fixed_bank_file_offset,
    rom::{HEADER_SIZE, PRG_SIZE, Rom},
    rp2a03::{Instruction, assemble_at},
};

use super::writer_sites::{CENTRAL_CHR_WRITERS, DIRECT_CHR_WRITERS, WriterLocation};

mod audio_record;

use audio_record::{
    AUDIO_BANK, AudioRecordCandidateBinding, RECORD_ADDRESS, RECORD_BYTES,
    bind_audio_record_candidate,
};

const PRG_BANK_SIZE: usize = 16 * 1024;
const FIXED_PRG_BANK: u8 = 0x0F;
const MMC4_CHR_REGISTERS: [u16; 4] = [0xB000, 0xC000, 0xD000, 0xE000];
const SOURCE_BOUND_AUDIO_RECORD_CANDIDATE: ChrWriteCandidate = ChrWriteCandidate {
    register: 0xC000,
    prg_bank: 0x0E,
    cpu_address: 0x9A7A,
    opcode: 0x8C,
};

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
struct ChrWriteCandidate {
    register: u16,
    prg_bank: u8,
    cpu_address: u16,
    opcode: u8,
}

#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub(super) struct LegacyCanonicalChrWriteCandidate {
    pub(super) prg_bank: u8,
    pub(super) cpu_address: u16,
    pub(super) register: u16,
    pub(super) opcode: u8,
}

#[derive(Debug, Clone, Serialize)]
pub(super) struct AbsoluteChrWriterCensus {
    candidate_scope: &'static str,
    out_of_scope_write_forms: &'static str,
    documented_non_indexed_absolute_candidate_count: usize,
    converted_writer_count: usize,
    central_writer_count: usize,
    direct_writer_count: usize,
    source_bound_audio_record_candidate_count: usize,
    source_bound_audio_record_candidate: AudioRecordCandidateBinding,
    register_candidate_counts: Vec<RegisterCandidateCount>,
    battle_immediate_left_fd_writer_count: usize,
    every_battle_immediate_left_fd_writer_reachable: bool,
    every_documented_non_indexed_absolute_candidate_classified: bool,
    every_declared_writer_source_bound_and_converted: bool,
}

#[derive(Debug, Clone, Serialize)]
struct RegisterCandidateCount {
    register_hex: String,
    documented_non_indexed_absolute_candidate_count: usize,
    converted_writer_count: usize,
    source_bound_audio_record_candidate_count: usize,
}

#[derive(Clone, Copy, Debug)]
pub(super) struct SourceBoundPrgDataRegion {
    pub(super) role: &'static str,
    pub(super) prg_bank: u8,
    pub(super) cpu_address: u16,
    pub(super) expected_bytes: &'static [u8],
}

pub(super) fn bind_audio_record_data_region(source: &Rom) -> Result<SourceBoundPrgDataRegion> {
    bind_audio_record_candidate(source, RECORD_ADDRESS, RECORD_BYTES.len())?;
    Ok(SourceBoundPrgDataRegion {
        role: "source-bound C0/C1 audio record",
        prg_bank: AUDIO_BANK,
        cpu_address: RECORD_ADDRESS,
        expected_bytes: &RECORD_BYTES,
    })
}

pub(super) fn bind_absolute_chr_writer_census(source: &Rom) -> Result<AbsoluteChrWriterCensus> {
    super::direct_chr_pairs::bind_direct_chr_writer_source_contracts(source)?;
    bind_central_chr_writer_source_contracts(source)?;
    let actual = scan_documented_non_indexed_absolute_chr_write_candidates(source.prg())?;
    let converted = declared_converted_candidates()?;
    let source_bound_audio_record = BTreeSet::from([SOURCE_BOUND_AUDIO_RECORD_CANDIDATE]);
    bind_candidate_partition(&actual, &converted, &source_bound_audio_record)?;
    let audio_record_candidate =
        bind_audio_record_candidate(source, SOURCE_BOUND_AUDIO_RECORD_CANDIDATE.cpu_address, 3)?;
    let battle_reachable =
        super::battle_codebook_plan::phase_cooccurrence::battle_phase_reachable_instruction_starts(
            source,
        )?;
    let battle_immediate_writer_addresses =
        super::direct_chr_pairs::immediate_left_fd_writer_addresses();
    let unreachable_battle_writers = battle_immediate_writer_addresses
        .iter()
        .filter(|address| !battle_reachable.contains(&(0x05, **address)))
        .copied()
        .collect::<Vec<_>>();
    ensure!(
        unreachable_battle_writers.is_empty(),
        "immediate left-FD writers are unreachable from the source-bound battle phase graph: {unreachable_battle_writers:04X?}"
    );
    let register_candidate_counts = MMC4_CHR_REGISTERS
        .into_iter()
        .map(|register| RegisterCandidateCount {
            register_hex: format!("0x{register:04X}"),
            documented_non_indexed_absolute_candidate_count: actual
                .iter()
                .filter(|candidate| candidate.register == register)
                .count(),
            converted_writer_count: converted
                .iter()
                .filter(|candidate| candidate.register == register)
                .count(),
            source_bound_audio_record_candidate_count: source_bound_audio_record
                .iter()
                .filter(|candidate| candidate.register == register)
                .count(),
        })
        .collect();

    Ok(AbsoluteChrWriterCensus {
        candidate_scope: "documented non-indexed absolute STA/STX/STY opcode windows (8C/8D/8E) whose encoded operand is an MMC4 CHR register; switchable $BFFE/$BFFF fetches continue into fixed $C000 bytes",
        out_of_scope_write_forms: "documented read-modify-write instructions, absolute-indexed stores, computed or indirect writes, self-modified effective addresses, and undocumented opcodes are separate negative-space work",
        documented_non_indexed_absolute_candidate_count: actual.len(),
        converted_writer_count: converted.len(),
        central_writer_count: CENTRAL_CHR_WRITERS.len(),
        direct_writer_count: DIRECT_CHR_WRITERS.len(),
        source_bound_audio_record_candidate_count: source_bound_audio_record.len(),
        source_bound_audio_record_candidate: audio_record_candidate,
        register_candidate_counts,
        battle_immediate_left_fd_writer_count: battle_immediate_writer_addresses.len(),
        every_battle_immediate_left_fd_writer_reachable: true,
        every_documented_non_indexed_absolute_candidate_classified: true,
        every_declared_writer_source_bound_and_converted: true,
    })
}

pub(super) fn legacy_canonical_chr_write_candidates(
    source: &Rom,
) -> Result<BTreeSet<LegacyCanonicalChrWriteCandidate>> {
    Ok(
        scan_documented_non_indexed_absolute_chr_write_candidates(source.prg())?
            .into_iter()
            .map(|candidate| LegacyCanonicalChrWriteCandidate {
                prg_bank: candidate.prg_bank,
                cpu_address: candidate.cpu_address,
                register: candidate.register,
                opcode: candidate.opcode,
            })
            .collect(),
    )
}

fn bind_central_chr_writer_source_contracts(source: &Rom) -> Result<()> {
    for writer in CENTRAL_CHR_WRITERS {
        let expected = assemble_at(
            writer.source_address,
            &[
                Instruction::StaZeroPage(writer.shadow_address),
                Instruction::OraZeroPage(0x52),
                Instruction::StaAbsolute(writer.source_register),
                Instruction::Rts,
            ],
        )?;
        let start = fixed_bank_file_offset(writer.source_address)?;
        let end = start
            .checked_add(expected.len())
            .context("central CHR writer source range overflow")?;
        ensure!(
            source.data().get(start..end) == Some(expected.as_slice()),
            "source instructions changed for {}",
            writer.role
        );
    }
    Ok(())
}

fn scan_documented_non_indexed_absolute_chr_write_candidates(
    prg: &[u8],
) -> Result<BTreeSet<ChrWriteCandidate>> {
    ensure!(
        prg.len() == PRG_SIZE,
        "source MMC4 CHR writer census PRG size changed"
    );
    let mut candidates = BTreeSet::new();
    for (bank_index, bank) in prg.chunks_exact(PRG_BANK_SIZE).enumerate() {
        let prg_bank = u8::try_from(bank_index).context("CHR writer PRG bank overflow")?;
        let cpu_base: u16 = if prg_bank == FIXED_PRG_BANK {
            0xC000
        } else {
            0x8000
        };
        for (relative, bytes) in bank.windows(3).enumerate() {
            let cpu_address = cpu_base
                .checked_add(u16::try_from(relative).context("CHR writer bank offset overflow")?)
                .context("CHR writer CPU address overflow")?;
            insert_candidate(&mut candidates, prg_bank, cpu_address, bytes);
        }
    }

    let fixed_bank = &prg[(usize::from(FIXED_PRG_BANK) * PRG_BANK_SIZE)..];
    for prg_bank in 0..FIXED_PRG_BANK {
        let bank_start = usize::from(prg_bank) * PRG_BANK_SIZE;
        let bank = &prg[bank_start..bank_start + PRG_BANK_SIZE];
        insert_candidate(
            &mut candidates,
            prg_bank,
            0xBFFE,
            &[
                bank[PRG_BANK_SIZE - 2],
                bank[PRG_BANK_SIZE - 1],
                fixed_bank[0],
            ],
        );
        insert_candidate(
            &mut candidates,
            prg_bank,
            0xBFFF,
            &[bank[PRG_BANK_SIZE - 1], fixed_bank[0], fixed_bank[1]],
        );
    }
    Ok(candidates)
}

fn insert_candidate(
    candidates: &mut BTreeSet<ChrWriteCandidate>,
    prg_bank: u8,
    cpu_address: u16,
    bytes: &[u8],
) {
    if !matches!(bytes[0], 0x8C | 0x8D | 0x8E) {
        return;
    }
    let register = u16::from_le_bytes([bytes[1], bytes[2]]);
    if !MMC4_CHR_REGISTERS.contains(&register) {
        return;
    }
    candidates.insert(ChrWriteCandidate {
        register,
        prg_bank,
        cpu_address,
        opcode: bytes[0],
    });
}

fn declared_converted_candidates() -> Result<BTreeSet<ChrWriteCandidate>> {
    let direct = DIRECT_CHR_WRITERS.iter().map(|writer| {
        let prg_bank = match writer.location {
            WriterLocation::Fixed => FIXED_PRG_BANK,
            WriterLocation::Switchable { prg_bank } => prg_bank,
        };
        ChrWriteCandidate {
            register: writer.source_register,
            prg_bank,
            cpu_address: writer.source_address,
            opcode: 0x8D,
        }
    });
    let central = CENTRAL_CHR_WRITERS.iter().map(|writer| {
        Ok(ChrWriteCandidate {
            register: writer.source_register,
            prg_bank: FIXED_PRG_BANK,
            cpu_address: writer
                .source_address
                .checked_add(4)
                .context("central CHR writer address overflow")?,
            opcode: 0x8D,
        })
    });
    direct
        .map(Ok)
        .chain(central)
        .collect::<Result<BTreeSet<_>>>()
}

fn bind_candidate_partition(
    actual: &BTreeSet<ChrWriteCandidate>,
    converted: &BTreeSet<ChrWriteCandidate>,
    proven_non_code: &BTreeSet<ChrWriteCandidate>,
) -> Result<()> {
    ensure!(
        converted.is_disjoint(proven_non_code),
        "an MMC4 CHR candidate is classified as both executable and non-code"
    );
    let classified = converted
        .union(proven_non_code)
        .copied()
        .collect::<BTreeSet<_>>();
    ensure!(
        *actual == classified,
        "source MMC4 CHR writer census changed: expected classified {classified:?}, found {actual:?}"
    );
    Ok(())
}

fn source_bytes(source: &Rom, bank: u8, address: u16, len: usize) -> Result<&[u8]> {
    let relative = if address >= 0xC000 {
        ensure!(
            bank == FIXED_PRG_BANK,
            "fixed source region uses a non-fixed PRG bank"
        );
        usize::from(address - 0xC000)
    } else {
        usize::from(address - 0x8000)
    };
    let offset = HEADER_SIZE + usize::from(bank) * PRG_BANK_SIZE + relative;
    source
        .data()
        .get(offset..offset + len)
        .context("source region is outside the ROM")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn candidate(address: u16) -> ChrWriteCandidate {
        ChrWriteCandidate {
            register: 0xB000,
            prg_bank: 0x05,
            cpu_address: address,
            opcode: 0x8D,
        }
    }

    #[test]
    fn exact_executable_and_non_code_partition_is_accepted() {
        let executable = candidate(0x962F);
        let source_bound_audio_record = ChrWriteCandidate {
            register: 0xC000,
            prg_bank: 0x0E,
            cpu_address: 0x9A7A,
            opcode: 0x8C,
        };
        bind_candidate_partition(
            &BTreeSet::from([executable, source_bound_audio_record]),
            &BTreeSet::from([executable]),
            &BTreeSet::from([source_bound_audio_record]),
        )
        .unwrap();
    }

    #[test]
    fn missing_or_extra_candidate_is_rejected() {
        let declared = candidate(0x962F);
        assert!(
            bind_candidate_partition(
                &BTreeSet::new(),
                &BTreeSet::from([declared]),
                &BTreeSet::new(),
            )
            .is_err()
        );
        assert!(
            bind_candidate_partition(
                &BTreeSet::from([declared, candidate(0xA2F8)]),
                &BTreeSet::from([declared]),
                &BTreeSet::new(),
            )
            .is_err()
        );
    }

    #[test]
    fn overlapping_code_and_data_classification_is_rejected() {
        let ambiguous = candidate(0x962F);
        assert!(
            bind_candidate_partition(
                &BTreeSet::from([ambiguous]),
                &BTreeSet::from([ambiguous]),
                &BTreeSet::from([ambiguous]),
            )
            .is_err()
        );
    }

    #[test]
    fn switchable_bank_end_fetches_continue_into_the_fixed_bank() {
        let mut prg = vec![0_u8; PRG_SIZE];
        let switchable_end = 0x05 * PRG_BANK_SIZE + PRG_BANK_SIZE - 1;
        prg[switchable_end] = 0x8D;
        let fixed_start = usize::from(FIXED_PRG_BANK) * PRG_BANK_SIZE;
        prg[fixed_start..fixed_start + 2].copy_from_slice(&[0x00, 0xB0]);

        let candidates = scan_documented_non_indexed_absolute_chr_write_candidates(&prg).unwrap();

        assert!(candidates.contains(&ChrWriteCandidate {
            register: 0xB000,
            prg_bank: 0x05,
            cpu_address: 0xBFFF,
            opcode: 0x8D,
        }));
    }
}
