use std::{collections::BTreeSet, fs, path::Path};

use anyhow::{Context, Result, ensure};
use serde::Serialize;

use crate::{
    mmc5_chr::switchable_bank_file_offset,
    mmc5_prg::fixed_bank_file_offset,
    rom::{EXPECTED_SOURCE_SHA1, Rom},
    rp2a03::{Instruction, assemble_at},
    sha1_hex,
};

use super::writer_sites::{DIRECT_CHR_WRITERS, WriterLocation};

mod immediate_left_fd;

use immediate_left_fd::{
    LOCATION as IMMEDIATE_LEFT_FD_LOCATION, REGISTER as IMMEDIATE_LEFT_FD_REGISTER,
    WRITERS as IMMEDIATE_LEFT_FD_WRITERS,
};

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
enum PairLocation {
    Fixed,
    Switchable { prg_bank: u8 },
}

impl PairLocation {
    fn file_offset(self, cpu_address: u16) -> Result<usize> {
        match self {
            Self::Fixed => fixed_bank_file_offset(cpu_address),
            Self::Switchable { prg_bank } => switchable_bank_file_offset(prg_bank, cpu_address),
        }
    }

    fn label(self) -> String {
        match self {
            Self::Fixed => "fixed_bank_0F".to_owned(),
            Self::Switchable { prg_bank } => format!("switchable_bank_{prg_bank:02X}"),
        }
    }
}

#[derive(Debug, Clone, Copy)]
struct SameAccumulatorGroup {
    role: &'static str,
    location: PairLocation,
    first_writer_address: u16,
    registers: &'static [u16],
}

#[derive(Debug, Clone, Copy)]
struct SeparateValuePair {
    role: &'static str,
    location: PairLocation,
    fd_writer_address: u16,
    fd_register: u16,
    fd_value_address: u8,
    fe_writer_address: u16,
    fe_register: u16,
    fe_value_address: u8,
}

const SAME_ACCUMULATOR_GROUPS: &[SameAccumulatorGroup] = &[
    same_group(
        "bank 05 left reset",
        switchable(0x05),
        0x85E9,
        &[0xB000, 0xC000],
    ),
    same_group(
        "bank 05 right reset",
        switchable(0x05),
        0x880E,
        &[0xD000, 0xE000],
    ),
    same_group(
        "bank 05 alternate left reset",
        switchable(0x05),
        0x8D25,
        &[0xB000, 0xC000],
    ),
    same_group(
        "bank 07 all-window reset",
        switchable(0x07),
        0xAC35,
        &[0xB000, 0xC000, 0xD000, 0xE000],
    ),
    same_group(
        "bank 0B first all-window reset",
        switchable(0x0B),
        0x9BF2,
        &[0xB000, 0xC000, 0xD000, 0xE000],
    ),
    same_group(
        "bank 0B second all-window reset",
        switchable(0x0B),
        0x9EAE,
        &[0xB000, 0xC000, 0xD000, 0xE000],
    ),
    same_group(
        "automatic status right pair",
        switchable(0x0D),
        0x8036,
        &[0xD000, 0xE000],
    ),
    same_group(
        "automatic status left pair",
        switchable(0x0D),
        0x83AB,
        &[0xB000, 0xC000],
    ),
    same_group(
        "fixed reset right pair",
        PairLocation::Fixed,
        0xC1B7,
        &[0xD000, 0xE000],
    ),
    same_group(
        "screen clear right pair",
        PairLocation::Fixed,
        0xCF28,
        &[0xD000, 0xE000],
    ),
];

const SEPARATE_VALUE_PAIRS: &[SeparateValuePair] = &[
    SeparateValuePair {
        role: "bank 05 variable left pair",
        location: switchable(0x05),
        fd_writer_address: 0x810E,
        fd_register: 0xB000,
        fd_value_address: 0x5E,
        fe_writer_address: 0x8113,
        fe_register: 0xC000,
        fe_value_address: 0x5F,
    },
    SeparateValuePair {
        role: "NMI variable right pair",
        location: PairLocation::Fixed,
        fd_writer_address: 0xC1F2,
        fd_register: 0xD000,
        fd_value_address: 0x5E,
        fe_writer_address: 0xC1F7,
        fe_register: 0xE000,
        fe_value_address: 0x5F,
    },
];

const SINGLETON_LOCATION: PairLocation = PairLocation::Fixed;
const SINGLETON_WRITER_ADDRESS: u16 = 0xE414;
const SINGLETON_REGISTER: u16 = 0xB000;

const fn switchable(prg_bank: u8) -> PairLocation {
    PairLocation::Switchable { prg_bank }
}

const fn same_group(
    role: &'static str,
    location: PairLocation,
    first_writer_address: u16,
    registers: &'static [u16],
) -> SameAccumulatorGroup {
    SameAccumulatorGroup {
        role,
        location,
        first_writer_address,
        registers,
    }
}

#[derive(Debug, Serialize)]
struct DirectChrPairReport {
    schema: u32,
    source_sha1: &'static str,
    direct_writer_count: usize,
    same_value_writer_count: usize,
    runtime_value_pair_writer_count: usize,
    singleton_writer_count: usize,
    immediate_left_fd_writer_count: usize,
    groups: Vec<GroupEvidence>,
    unresolved_boundaries: Vec<&'static str>,
    release_eligible: bool,
}

#[derive(Debug, Serialize)]
struct GroupEvidence {
    role: &'static str,
    location: String,
    writer_addresses: Vec<String>,
    mapper_registers: Vec<String>,
    value_contract: &'static str,
    source_value_addresses: Vec<String>,
    immediate_source_pages: Vec<String>,
    runtime_observation_required: bool,
}

pub struct DirectChrPairSummary {
    pub report_sha1: String,
    pub direct_writer_count: usize,
    pub same_value_writer_count: usize,
    pub immediate_left_fd_writer_count: usize,
    pub runtime_observation_writer_count: usize,
}

pub fn analyze_direct_chr_pairs(
    source_path: &Path,
    report_path: &Path,
) -> Result<DirectChrPairSummary> {
    let source_rom = Rom::from_path(source_path)?;
    source_rom.verify_supported_japanese()?;
    bind_direct_chr_writer_source_contracts(&source_rom)?;

    let mut groups = Vec::new();
    for group in SAME_ACCUMULATOR_GROUPS {
        groups.push(GroupEvidence {
            role: group.role,
            location: group.location.label(),
            writer_addresses: writer_addresses(group.first_writer_address, group.registers.len()),
            mapper_registers: group
                .registers
                .iter()
                .map(|register| format!("0x{register:04X}"))
                .collect(),
            value_contract: "consecutive STA instructions consume the same accumulator value",
            source_value_addresses: Vec::new(),
            immediate_source_pages: Vec::new(),
            runtime_observation_required: false,
        });
    }
    for pair in SEPARATE_VALUE_PAIRS {
        groups.push(GroupEvidence {
            role: pair.role,
            location: pair.location.label(),
            writer_addresses: vec![
                format!("0x{:04X}", pair.fd_writer_address),
                format!("0x{:04X}", pair.fe_writer_address),
            ],
            mapper_registers: vec![
                format!("0x{:04X}", pair.fd_register),
                format!("0x{:04X}", pair.fe_register),
            ],
            value_contract: "FD and FE load separate zero-page values",
            source_value_addresses: vec![
                format!("0x{:02X}", pair.fd_value_address),
                format!("0x{:02X}", pair.fe_value_address),
            ],
            immediate_source_pages: Vec::new(),
            runtime_observation_required: true,
        });
    }
    groups.push(GroupEvidence {
        role: "unit data left FD singleton",
        location: SINGLETON_LOCATION.label(),
        writer_addresses: vec![format!("0x{SINGLETON_WRITER_ADDRESS:04X}")],
        mapper_registers: vec![format!("0x{SINGLETON_REGISTER:04X}")],
        value_contract: "one indexed value updates only the left FD register",
        source_value_addresses: vec!["0x0484+X".to_owned()],
        immediate_source_pages: Vec::new(),
        runtime_observation_required: true,
    });
    for writer in IMMEDIATE_LEFT_FD_WRITERS {
        groups.push(GroupEvidence {
            role: writer.role,
            location: IMMEDIATE_LEFT_FD_LOCATION.label(),
            writer_addresses: vec![format!("0x{:04X}", writer.writer_address)],
            mapper_registers: vec![format!("0x{IMMEDIATE_LEFT_FD_REGISTER:04X}")],
            value_contract: "an immediate source page updates only the left FD register",
            source_value_addresses: Vec::new(),
            immediate_source_pages: vec![format!("0x{:02X}", writer.source_page)],
            runtime_observation_required: true,
        });
    }

    let same_value_writer_count = SAME_ACCUMULATOR_GROUPS
        .iter()
        .map(|group| group.registers.len())
        .sum::<usize>();
    let runtime_value_pair_writer_count = SEPARATE_VALUE_PAIRS.len() * 2;
    let singleton_writer_count = 1;
    let immediate_left_fd_writer_count = IMMEDIATE_LEFT_FD_WRITERS.len();
    let direct_writer_count = same_value_writer_count
        + runtime_value_pair_writer_count
        + singleton_writer_count
        + immediate_left_fd_writer_count;
    let report = DirectChrPairReport {
        schema: 2,
        source_sha1: EXPECTED_SOURCE_SHA1,
        direct_writer_count,
        same_value_writer_count,
        runtime_value_pair_writer_count,
        singleton_writer_count,
        immediate_left_fd_writer_count,
        groups,
        unresolved_boundaries: vec![
            "Separate $5E/$5F pairs require runtime values before trigger-plane compatibility can be classified.",
            "The unit-data left FD singleton requires the active left FE page at each execution site.",
            "Immediate left FD writers bind their source page statically, but their active left FE page and execution co-lifetimes are not complete runtime claims.",
            "Static same-accumulator pairing proves equal FD/FE values, not execution coverage of battle, chapter transition, defeat, or ending paths.",
        ],
        release_eligible: false,
    };
    let report_bytes = serde_json::to_vec_pretty(&report)
        .context("serialize mapper 165 direct CHR pair report")?;
    let report_sha1 = sha1_hex(&report_bytes);
    if let Some(parent) = report_path.parent() {
        fs::create_dir_all(parent)
            .with_context(|| format!("create output directory {}", parent.display()))?;
    }
    fs::write(report_path, report_bytes)
        .with_context(|| format!("write {}", report_path.display()))?;

    Ok(DirectChrPairSummary {
        report_sha1,
        direct_writer_count,
        same_value_writer_count,
        immediate_left_fd_writer_count,
        runtime_observation_writer_count: runtime_value_pair_writer_count
            + singleton_writer_count
            + immediate_left_fd_writer_count,
    })
}

pub(super) fn bind_direct_chr_writer_source_contracts(source_rom: &Rom) -> Result<()> {
    verify_declared_writer_partition()?;
    for group in SAME_ACCUMULATOR_GROUPS {
        verify_same_accumulator_group(source_rom, group)?;
    }
    for pair in SEPARATE_VALUE_PAIRS {
        verify_separate_value_pair(source_rom, pair)?;
    }
    verify_singleton(source_rom)?;
    immediate_left_fd::verify_source_sequences(source_rom)
}

pub(super) fn immediate_left_fd_writer_addresses() -> BTreeSet<u16> {
    IMMEDIATE_LEFT_FD_WRITERS
        .iter()
        .map(|writer| writer.writer_address)
        .collect()
}

fn verify_same_accumulator_group(source_rom: &Rom, group: &SameAccumulatorGroup) -> Result<()> {
    let instructions = group
        .registers
        .iter()
        .map(|register| Instruction::StaAbsolute(*register))
        .collect::<Vec<_>>();
    verify_bytes(
        source_rom,
        group.location,
        group.first_writer_address,
        &assemble_at(group.first_writer_address, &instructions)?,
        group.role,
    )
}

fn verify_separate_value_pair(source_rom: &Rom, pair: &SeparateValuePair) -> Result<()> {
    ensure!(
        pair.fe_writer_address == pair.fd_writer_address + 5,
        "{} writer spacing changed",
        pair.role
    );
    let start = pair.fd_writer_address - 2;
    let expected = assemble_at(
        start,
        &[
            Instruction::LdaZeroPage(pair.fd_value_address),
            Instruction::StaAbsolute(pair.fd_register),
            Instruction::LdaZeroPage(pair.fe_value_address),
            Instruction::StaAbsolute(pair.fe_register),
        ],
    )?;
    verify_bytes(source_rom, pair.location, start, &expected, pair.role)
}

fn verify_singleton(source_rom: &Rom) -> Result<()> {
    let start = SINGLETON_WRITER_ADDRESS - 3;
    let expected = assemble_at(
        start,
        &[
            Instruction::LdaAbsoluteX(0x0484),
            Instruction::StaAbsolute(SINGLETON_REGISTER),
        ],
    )?;
    verify_bytes(
        source_rom,
        SINGLETON_LOCATION,
        start,
        &expected,
        "unit data left FD singleton",
    )
}

fn verify_bytes(
    source_rom: &Rom,
    location: PairLocation,
    cpu_address: u16,
    expected: &[u8],
    role: &str,
) -> Result<()> {
    let start = location.file_offset(cpu_address)?;
    let end = start + expected.len();
    ensure!(
        source_rom.data().get(start..end) == Some(expected),
        "source instructions changed for {role}"
    );
    Ok(())
}

fn verify_declared_writer_partition() -> Result<()> {
    immediate_left_fd::verify_inventory_inclusion()?;
    let actual = DIRECT_CHR_WRITERS
        .iter()
        .map(|writer| {
            let location = match writer.location {
                WriterLocation::Fixed => PairLocation::Fixed,
                WriterLocation::Switchable { prg_bank } => PairLocation::Switchable { prg_bank },
            };
            (location, writer.source_address, writer.source_register)
        })
        .collect::<BTreeSet<_>>();
    let mut declared = BTreeSet::new();
    for group in SAME_ACCUMULATOR_GROUPS {
        for (index, register) in group.registers.iter().enumerate() {
            declared.insert((
                group.location,
                group.first_writer_address + u16::try_from(index)? * 3,
                *register,
            ));
        }
    }
    for pair in SEPARATE_VALUE_PAIRS {
        declared.insert((pair.location, pair.fd_writer_address, pair.fd_register));
        declared.insert((pair.location, pair.fe_writer_address, pair.fe_register));
    }
    declared.insert((
        SINGLETON_LOCATION,
        SINGLETON_WRITER_ADDRESS,
        SINGLETON_REGISTER,
    ));
    for writer in IMMEDIATE_LEFT_FD_WRITERS {
        declared.insert((
            IMMEDIATE_LEFT_FD_LOCATION,
            writer.writer_address,
            IMMEDIATE_LEFT_FD_REGISTER,
        ));
    }
    ensure!(
        actual == declared,
        "direct CHR writer pair partition no longer covers the writer inventory exactly"
    );
    Ok(())
}

fn writer_addresses(first: u16, count: usize) -> Vec<String> {
    (0..count)
        .map(|index| format!("0x{:04X}", first + index as u16 * 3))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn declared_groups_partition_all_direct_chr_writers() {
        verify_declared_writer_partition().unwrap();
        let same_value_writer_count = SAME_ACCUMULATOR_GROUPS
            .iter()
            .map(|group| group.registers.len())
            .sum::<usize>();

        assert_eq!(same_value_writer_count, 26);
        assert_eq!(SEPARATE_VALUE_PAIRS.len() * 2, 4);
        assert_eq!(IMMEDIATE_LEFT_FD_WRITERS.len(), 22);
        assert_eq!(
            same_value_writer_count
                + SEPARATE_VALUE_PAIRS.len() * 2
                + 1
                + IMMEDIATE_LEFT_FD_WRITERS.len(),
            53
        );
    }

    #[test]
    fn separate_value_pairs_load_fd_and_fe_from_distinct_sources() {
        for pair in SEPARATE_VALUE_PAIRS {
            assert_ne!(pair.fd_value_address, pair.fe_value_address);
            assert_eq!(pair.fe_writer_address, pair.fd_writer_address + 5);
        }
    }
}
