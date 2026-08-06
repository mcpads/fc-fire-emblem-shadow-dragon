use std::{fs, path::Path};

use anyhow::{Context, Result, ensure};
use serde::Serialize;

use crate::{
    mmc5_chr::switchable_bank_file_offset,
    mmc5_prg::{SOURCE_RESET_ADDRESS, count_direct_transfers_to_range, fixed_bank_file_offset},
    rom::{CHR_FILE_OFFSET, EXPECTED_CHR_SHA1, EXPECTED_SOURCE_SHA1, PRG_SIZE, Rom},
    rp2a03::{Instruction, assemble_at},
    sha1_hex,
    static_analysis::find_absolute_write_candidates,
    tracked::TrackedImage,
};

const OUTPUT_MAPPER: u16 = 165;
const OUTPUT_CHR_PADDING_SIZE: usize = 8 * 1024;
const OUTPUT_CHR_BANK_COUNT: u8 = 17;
const RESET_INITIALIZER_ADDRESS: u16 = 0xFA00;
const SELECT_PRG_BANK_ADDRESS: u16 = 0xFA20;
const SELECT_LEFT_FD_CHR_BANK_ADDRESS: u16 = 0xFA40;
const SELECT_LEFT_FE_CHR_BANK_ADDRESS: u16 = 0xFA60;
const SELECT_RIGHT_FD_CHR_BANK_ADDRESS: u16 = 0xFA80;
const SELECT_RIGHT_FE_CHR_BANK_ADDRESS: u16 = 0xFAA0;
const CODE_CAVE_START_ADDRESS: u16 = RESET_INITIALIZER_ADDRESS;
const CODE_CAVE_LEN: usize = 0xC0;

const SOURCE_SELECT_PRG_BANK_AND_SAVE_ADDRESS: u16 = 0xC9A6;
const SOURCE_SELECT_HORIZONTAL_MIRRORING_ADDRESS: u16 = 0xC9CE;
const SOURCE_SELECT_VERTICAL_MIRRORING_ADDRESS: u16 = 0xC9D6;

const SOURCE_PRG_BANK_WRITERS: &[DirectWriter] = &[
    DirectWriter::fixed(
        "boot temporary PRG bank selection",
        0xC1FD,
        0xA000,
        SELECT_PRG_BANK_ADDRESS,
    ),
    DirectWriter::fixed(
        "boot PRG bank restoration",
        0xC205,
        0xA000,
        SELECT_PRG_BANK_ADDRESS,
    ),
    DirectWriter::fixed(
        "indirect copy PRG bank selection",
        0xC99F,
        0xA000,
        SELECT_PRG_BANK_ADDRESS,
    ),
    DirectWriter::fixed(
        "unit data PRG bank selection",
        0xE422,
        0xA000,
        SELECT_PRG_BANK_ADDRESS,
    ),
    DirectWriter::fixed(
        "unit data PRG bank restoration",
        0xE43E,
        0xA000,
        SELECT_PRG_BANK_ADDRESS,
    ),
    DirectWriter::fixed(
        "dialogue byte PRG bank selection",
        0xE6A1,
        0xA000,
        SELECT_PRG_BANK_ADDRESS,
    ),
    DirectWriter::fixed(
        "dialogue byte PRG bank restoration",
        0xE6AB,
        0xA000,
        SELECT_PRG_BANK_ADDRESS,
    ),
    DirectWriter::fixed(
        "indexed pointer table PRG bank selection",
        0xE6BA,
        0xA000,
        SELECT_PRG_BANK_ADDRESS,
    ),
    DirectWriter::fixed(
        "indexed pointer table PRG bank restoration",
        0xE6F1,
        0xA000,
        SELECT_PRG_BANK_ADDRESS,
    ),
    DirectWriter::fixed(
        "PPU queue source PRG bank selection",
        0xE71D,
        0xA000,
        SELECT_PRG_BANK_ADDRESS,
    ),
    DirectWriter::fixed(
        "PPU queue source PRG bank restoration",
        0xE736,
        0xA000,
        SELECT_PRG_BANK_ADDRESS,
    ),
];

const CENTRAL_CHR_WRITERS: &[CentralChrWriter] = &[
    CentralChrWriter {
        role: "PPU $0000 FD source",
        source_address: 0xC9AE,
        shadow_address: 0x59,
        source_register: 0xB000,
        target_routine: SELECT_LEFT_FD_CHR_BANK_ADDRESS,
    },
    CentralChrWriter {
        role: "PPU $0000 FE source",
        source_address: 0xC9B6,
        shadow_address: 0x5A,
        source_register: 0xC000,
        target_routine: SELECT_LEFT_FE_CHR_BANK_ADDRESS,
    },
    CentralChrWriter {
        role: "PPU $1000 FD source",
        source_address: 0xC9BE,
        shadow_address: 0x5B,
        source_register: 0xD000,
        target_routine: SELECT_RIGHT_FD_CHR_BANK_ADDRESS,
    },
    CentralChrWriter {
        role: "PPU $1000 FE source",
        source_address: 0xC9C6,
        shadow_address: 0x5C,
        source_register: 0xE000,
        target_routine: SELECT_RIGHT_FE_CHR_BANK_ADDRESS,
    },
];

const DIRECT_CHR_WRITERS: &[DirectWriter] = &[
    DirectWriter::switchable(
        "bank 05 left FD initialization",
        0x05,
        0x810E,
        0xB000,
        SELECT_LEFT_FD_CHR_BANK_ADDRESS,
    ),
    DirectWriter::switchable(
        "bank 05 left FE initialization",
        0x05,
        0x8113,
        0xC000,
        SELECT_LEFT_FE_CHR_BANK_ADDRESS,
    ),
    DirectWriter::switchable(
        "bank 05 left FD reset",
        0x05,
        0x85E9,
        0xB000,
        SELECT_LEFT_FD_CHR_BANK_ADDRESS,
    ),
    DirectWriter::switchable(
        "bank 05 left FE reset",
        0x05,
        0x85EC,
        0xC000,
        SELECT_LEFT_FE_CHR_BANK_ADDRESS,
    ),
    DirectWriter::switchable(
        "bank 05 right FD reset",
        0x05,
        0x880E,
        0xD000,
        SELECT_RIGHT_FD_CHR_BANK_ADDRESS,
    ),
    DirectWriter::switchable(
        "bank 05 right FE reset",
        0x05,
        0x8811,
        0xE000,
        SELECT_RIGHT_FE_CHR_BANK_ADDRESS,
    ),
    DirectWriter::switchable(
        "bank 05 alternate left FD reset",
        0x05,
        0x8D25,
        0xB000,
        SELECT_LEFT_FD_CHR_BANK_ADDRESS,
    ),
    DirectWriter::switchable(
        "bank 05 alternate left FE reset",
        0x05,
        0x8D28,
        0xC000,
        SELECT_LEFT_FE_CHR_BANK_ADDRESS,
    ),
    DirectWriter::switchable(
        "bank 07 left FD reset",
        0x07,
        0xAC35,
        0xB000,
        SELECT_LEFT_FD_CHR_BANK_ADDRESS,
    ),
    DirectWriter::switchable(
        "bank 07 left FE reset",
        0x07,
        0xAC38,
        0xC000,
        SELECT_LEFT_FE_CHR_BANK_ADDRESS,
    ),
    DirectWriter::switchable(
        "bank 07 right FD reset",
        0x07,
        0xAC3B,
        0xD000,
        SELECT_RIGHT_FD_CHR_BANK_ADDRESS,
    ),
    DirectWriter::switchable(
        "bank 07 right FE reset",
        0x07,
        0xAC3E,
        0xE000,
        SELECT_RIGHT_FE_CHR_BANK_ADDRESS,
    ),
    DirectWriter::switchable(
        "bank 0B first left FD reset",
        0x0B,
        0x9BF2,
        0xB000,
        SELECT_LEFT_FD_CHR_BANK_ADDRESS,
    ),
    DirectWriter::switchable(
        "bank 0B first left FE reset",
        0x0B,
        0x9BF5,
        0xC000,
        SELECT_LEFT_FE_CHR_BANK_ADDRESS,
    ),
    DirectWriter::switchable(
        "bank 0B first right FD reset",
        0x0B,
        0x9BF8,
        0xD000,
        SELECT_RIGHT_FD_CHR_BANK_ADDRESS,
    ),
    DirectWriter::switchable(
        "bank 0B first right FE reset",
        0x0B,
        0x9BFB,
        0xE000,
        SELECT_RIGHT_FE_CHR_BANK_ADDRESS,
    ),
    DirectWriter::switchable(
        "bank 0B second left FD reset",
        0x0B,
        0x9EAE,
        0xB000,
        SELECT_LEFT_FD_CHR_BANK_ADDRESS,
    ),
    DirectWriter::switchable(
        "bank 0B second left FE reset",
        0x0B,
        0x9EB1,
        0xC000,
        SELECT_LEFT_FE_CHR_BANK_ADDRESS,
    ),
    DirectWriter::switchable(
        "bank 0B second right FD reset",
        0x0B,
        0x9EB4,
        0xD000,
        SELECT_RIGHT_FD_CHR_BANK_ADDRESS,
    ),
    DirectWriter::switchable(
        "bank 0B second right FE reset",
        0x0B,
        0x9EB7,
        0xE000,
        SELECT_RIGHT_FE_CHR_BANK_ADDRESS,
    ),
    DirectWriter::switchable(
        "automatic status right FD source",
        0x0D,
        0x8036,
        0xD000,
        SELECT_RIGHT_FD_CHR_BANK_ADDRESS,
    ),
    DirectWriter::switchable(
        "automatic status right FE source",
        0x0D,
        0x8039,
        0xE000,
        SELECT_RIGHT_FE_CHR_BANK_ADDRESS,
    ),
    DirectWriter::switchable(
        "automatic status left FD source",
        0x0D,
        0x83AB,
        0xB000,
        SELECT_LEFT_FD_CHR_BANK_ADDRESS,
    ),
    DirectWriter::switchable(
        "automatic status left FE source",
        0x0D,
        0x83AE,
        0xC000,
        SELECT_LEFT_FE_CHR_BANK_ADDRESS,
    ),
    DirectWriter::fixed(
        "reset right FD source",
        0xC1B7,
        0xD000,
        SELECT_RIGHT_FD_CHR_BANK_ADDRESS,
    ),
    DirectWriter::fixed(
        "reset right FE source",
        0xC1BA,
        0xE000,
        SELECT_RIGHT_FE_CHR_BANK_ADDRESS,
    ),
    DirectWriter::fixed(
        "NMI right FD source",
        0xC1F2,
        0xD000,
        SELECT_RIGHT_FD_CHR_BANK_ADDRESS,
    ),
    DirectWriter::fixed(
        "NMI right FE source",
        0xC1F7,
        0xE000,
        SELECT_RIGHT_FE_CHR_BANK_ADDRESS,
    ),
    DirectWriter::fixed(
        "screen clear right FD source",
        0xCF28,
        0xD000,
        SELECT_RIGHT_FD_CHR_BANK_ADDRESS,
    ),
    DirectWriter::fixed(
        "screen clear right FE source",
        0xCF2B,
        0xE000,
        SELECT_RIGHT_FE_CHR_BANK_ADDRESS,
    ),
    DirectWriter::fixed(
        "unit data left FD source",
        0xE414,
        0xB000,
        SELECT_LEFT_FD_CHR_BANK_ADDRESS,
    ),
];

#[derive(Debug, Clone, Copy)]
enum WriterLocation {
    Fixed,
    Switchable { prg_bank: u8 },
}

#[derive(Debug, Clone, Copy)]
struct DirectWriter {
    role: &'static str,
    location: WriterLocation,
    source_address: u16,
    source_register: u16,
    target_routine: u16,
}

impl DirectWriter {
    const fn fixed(
        role: &'static str,
        source_address: u16,
        source_register: u16,
        target_routine: u16,
    ) -> Self {
        Self {
            role,
            location: WriterLocation::Fixed,
            source_address,
            source_register,
            target_routine,
        }
    }

    const fn switchable(
        role: &'static str,
        prg_bank: u8,
        source_address: u16,
        source_register: u16,
        target_routine: u16,
    ) -> Self {
        Self {
            role,
            location: WriterLocation::Switchable { prg_bank },
            source_address,
            source_register,
            target_routine,
        }
    }
}

#[derive(Debug, Clone, Copy)]
struct CentralChrWriter {
    role: &'static str,
    source_address: u16,
    shadow_address: u8,
    source_register: u16,
    target_routine: u16,
}

#[derive(Debug)]
struct AssembledRoutine {
    role: &'static str,
    cpu_address: u16,
    bytes: Vec<u8>,
}

#[derive(Debug, Serialize)]
struct Mapper165ParityReport {
    schema: u32,
    source_sha1: &'static str,
    output_sha1: String,
    source_mapper: u16,
    output_mapper: u16,
    prg_size: usize,
    chr_size: usize,
    source_chr_sha1: &'static str,
    relocated_source_chr_sha1: String,
    output_chr_sha1: String,
    battery_flag_preserved: bool,
    chr_layout: ChrLayoutEvidence,
    code_cave: CodeCaveEvidence,
    direct_code_cave_transfer_count: usize,
    routines: Vec<RoutinePlacement>,
    prg_writer_count: usize,
    central_chr_writer_count: usize,
    direct_chr_writer_count: usize,
    tracked_writes: Vec<TrackedWrite>,
    unresolved_boundaries: Vec<&'static str>,
    release_eligible: bool,
}

#[derive(Debug, Serialize)]
struct ChrLayoutEvidence {
    reserved_prefix_size: usize,
    source_chr_offset: usize,
    source_4k_page_bias: u8,
    maximum_4k_chr_rom_pages: usize,
    remaining_4k_pages_at_maximum_size: usize,
}

#[derive(Debug, Serialize)]
struct CodeCaveEvidence {
    cpu_start: String,
    file_start: String,
    len: usize,
    expected_fill: &'static str,
}

#[derive(Debug, Serialize)]
struct RoutinePlacement {
    role: &'static str,
    cpu_address: String,
    len: usize,
}

#[derive(Debug, Serialize)]
struct TrackedWrite {
    label: String,
    file_offset: String,
    len: usize,
}

pub struct BuildSummary {
    pub output_sha1: String,
    pub report_sha1: String,
    pub tracked_write_count: usize,
}

pub fn build_mapper165_parity_probe(
    source_path: &Path,
    output_path: &Path,
    report_path: &Path,
) -> Result<BuildSummary> {
    let source_rom = Rom::from_path(source_path)?;
    source_rom.verify_supported_japanese()?;
    verify_complete_prg_writer_inventory(&source_rom)?;

    let base = create_chr_relocated_image(&source_rom)?;
    let cave_file_start = fixed_bank_file_offset(CODE_CAVE_START_ADDRESS)?;
    let cave_file_end = cave_file_start
        .checked_add(CODE_CAVE_LEN)
        .ok_or_else(|| anyhow::anyhow!("mapper 165 code cave range overflow"))?;
    ensure!(
        base[cave_file_start..cave_file_end]
            .iter()
            .all(|byte| *byte == 0xFF),
        "mapper 165 code cave is no longer all FF"
    );
    let direct_code_cave_transfer_count = count_direct_transfers_to_range(
        source_rom.prg(),
        CODE_CAVE_START_ADDRESS,
        CODE_CAVE_START_ADDRESS + CODE_CAVE_LEN as u16,
    )?;
    ensure!(
        direct_code_cave_transfer_count == 0,
        "mapper 165 code cave has {direct_code_cave_transfer_count} direct JSR or JMP references"
    );

    let routines = build_routines()?;
    validate_routine_placements(&routines)?;
    let mut image = TrackedImage::new(base.clone());
    image.write_expected("iNES mapper low nibble 10 to 165", 6, &[0xA2], &[0x52])?;
    image.write_expected("iNES mapper high nibble 10 to 165", 7, &[0x00], &[0xA0])?;
    image.write_expected(
        "reserve two CHR pages before the source CHR",
        5,
        &[0x10],
        &[OUTPUT_CHR_BANK_COUNT],
    )?;

    for routine in &routines {
        image.write_expected(
            format!("mapper 165 {} routine", routine.role),
            fixed_bank_file_offset(routine.cpu_address)?,
            &vec![0xFF; routine.bytes.len()],
            &routine.bytes,
        )?;
    }

    replace_central_prg_writer(&mut image)?;
    for writer in SOURCE_PRG_BANK_WRITERS {
        replace_direct_writer(&mut image, *writer)?;
    }
    for writer in CENTRAL_CHR_WRITERS {
        replace_central_chr_writer(&mut image, *writer)?;
    }
    for writer in DIRECT_CHR_WRITERS {
        replace_direct_writer(&mut image, *writer)?;
    }
    replace_mirroring_writer(
        &mut image,
        "horizontal mirroring selector",
        SOURCE_SELECT_HORIZONTAL_MIRRORING_ADDRESS,
        1,
    )?;
    replace_mirroring_writer(
        &mut image,
        "vertical mirroring selector",
        SOURCE_SELECT_VERTICAL_MIRRORING_ADDRESS,
        0,
    )?;
    image.write_expected(
        "reset vector to mapper 165 initializer",
        fixed_bank_file_offset(0xFFFC)?,
        &SOURCE_RESET_ADDRESS.to_le_bytes(),
        &RESET_INITIALIZER_ADDRESS.to_le_bytes(),
    )?;

    image.verify_all_changes_tracked(&base)?;
    let tracked_writes = image
        .writes()
        .iter()
        .map(|write| TrackedWrite {
            label: write.label.clone(),
            file_offset: format!("0x{:06X}", write.offset),
            len: write.len,
        })
        .collect::<Vec<_>>();
    let output = image.into_data();
    let output_rom = Rom::parse(output.clone()).context("parse mapper 165 parity probe")?;
    verify_output(&source_rom, &output_rom, &output)?;

    let output_sha1 = sha1_hex(&output);
    let relocated_source_chr_sha1 = sha1_hex(&output_rom.chr()[OUTPUT_CHR_PADDING_SIZE..]);
    let output_chr_sha1 = sha1_hex(output_rom.chr());
    let report = Mapper165ParityReport {
        schema: 1,
        source_sha1: EXPECTED_SOURCE_SHA1,
        output_sha1: output_sha1.clone(),
        source_mapper: source_rom.mapper(),
        output_mapper: output_rom.mapper(),
        prg_size: output_rom.prg().len(),
        chr_size: output_rom.chr().len(),
        source_chr_sha1: EXPECTED_CHR_SHA1,
        relocated_source_chr_sha1,
        output_chr_sha1,
        battery_flag_preserved: true,
        chr_layout: ChrLayoutEvidence {
            reserved_prefix_size: OUTPUT_CHR_PADDING_SIZE,
            source_chr_offset: OUTPUT_CHR_PADDING_SIZE,
            source_4k_page_bias: 2,
            maximum_4k_chr_rom_pages: 64,
            remaining_4k_pages_at_maximum_size: 30,
        },
        code_cave: CodeCaveEvidence {
            cpu_start: format!("0x{CODE_CAVE_START_ADDRESS:04X}"),
            file_start: format!("0x{cave_file_start:06X}"),
            len: CODE_CAVE_LEN,
            expected_fill: "0xFF",
        },
        direct_code_cave_transfer_count,
        routines: routines
            .iter()
            .map(|routine| RoutinePlacement {
                role: routine.role,
                cpu_address: format!("0x{:04X}", routine.cpu_address),
                len: routine.bytes.len(),
            })
            .collect(),
        prg_writer_count: SOURCE_PRG_BANK_WRITERS.len() + 1,
        central_chr_writer_count: CENTRAL_CHR_WRITERS.len(),
        direct_chr_writer_count: DIRECT_CHR_WRITERS.len(),
        tracked_writes,
        unresolved_boundaries: vec![
            "Mapper 165 changes the FD latch trigger timing from MMC4; visible parity must be measured on trigger-bearing screens.",
            "Direct CHR writers are limited to instruction-boundary sites proven by fixed-bank disassembly, adjacent register groups, or prior runtime traces; isolated byte-pattern candidates remain unclassified.",
            "The probe preserves and relocates the source CHR but does not add Korean glyphs or translation assets.",
            "Save RAM is enabled statically, but save/load persistence and adverse gameplay paths still require runtime verification.",
        ],
        release_eligible: false,
    };
    let report_bytes =
        serde_json::to_vec_pretty(&report).context("serialize mapper 165 parity report")?;
    let report_sha1 = sha1_hex(&report_bytes);
    let tracked_write_count = report.tracked_writes.len();

    write_file(output_path, &output)?;
    write_file(report_path, &report_bytes)?;
    Ok(BuildSummary {
        output_sha1,
        report_sha1,
        tracked_write_count,
    })
}

fn create_chr_relocated_image(source_rom: &Rom) -> Result<Vec<u8>> {
    let output_len = source_rom
        .data()
        .len()
        .checked_add(OUTPUT_CHR_PADDING_SIZE)
        .ok_or_else(|| anyhow::anyhow!("mapper 165 output size overflow"))?;
    let mut output = Vec::with_capacity(output_len);
    output.extend_from_slice(&source_rom.data()[..CHR_FILE_OFFSET]);
    output.resize(output.len() + OUTPUT_CHR_PADDING_SIZE, 0);
    output.extend_from_slice(source_rom.chr());
    ensure!(
        output.len() == output_len,
        "mapper 165 CHR relocation size mismatch"
    );
    Ok(output)
}

fn verify_complete_prg_writer_inventory(source_rom: &Rom) -> Result<()> {
    let candidates = find_absolute_write_candidates(source_rom.prg(), 0xA000);
    ensure!(
        candidates.len() == SOURCE_PRG_BANK_WRITERS.len() + 1,
        "source $A000 write inventory changed: expected {}, found {}",
        SOURCE_PRG_BANK_WRITERS.len() + 1,
        candidates.len()
    );
    ensure!(
        candidates.iter().all(|candidate| candidate.opcode == 0x8D),
        "source $A000 inventory contains a non-STA writer"
    );
    let mut actual = candidates
        .iter()
        .map(|candidate| candidate.cpu_address)
        .collect::<Vec<_>>();
    actual.sort_unstable();
    let mut expected = SOURCE_PRG_BANK_WRITERS
        .iter()
        .map(|writer| writer.source_address)
        .chain(std::iter::once(0xC9AA))
        .collect::<Vec<_>>();
    expected.sort_unstable();
    ensure!(actual == expected, "source $A000 writer addresses changed");
    Ok(())
}

fn build_routines() -> Result<Vec<AssembledRoutine>> {
    Ok(vec![
        assemble_routine(
            "reset initialization",
            RESET_INITIALIZER_ADDRESS,
            &[
                Instruction::LdaImmediate(0),
                Instruction::StaAbsolute(0xE000),
                Instruction::LdaImmediate(0x80),
                Instruction::StaAbsolute(0xA001),
                Instruction::LdaImmediate(0),
                Instruction::JsrAbsolute(SELECT_PRG_BANK_ADDRESS),
                Instruction::JsrAbsolute(SELECT_LEFT_FD_CHR_BANK_ADDRESS),
                Instruction::JsrAbsolute(SELECT_LEFT_FE_CHR_BANK_ADDRESS),
                Instruction::JsrAbsolute(SELECT_RIGHT_FD_CHR_BANK_ADDRESS),
                Instruction::JsrAbsolute(SELECT_RIGHT_FE_CHR_BANK_ADDRESS),
                Instruction::JmpAbsolute(SOURCE_RESET_ADDRESS),
            ],
        )?,
        assemble_routine(
            "16 KiB PRG bank selection",
            SELECT_PRG_BANK_ADDRESS,
            &[
                Instruction::Php,
                Instruction::Pha,
                Instruction::AndImmediate(0x0F),
                Instruction::AslAccumulator,
                Instruction::Pha,
                Instruction::LdaImmediate(6),
                Instruction::StaAbsolute(0x8000),
                Instruction::Pla,
                Instruction::StaAbsolute(0x8001),
                Instruction::Pha,
                Instruction::LdaImmediate(7),
                Instruction::StaAbsolute(0x8000),
                Instruction::Pla,
                Instruction::OraImmediate(1),
                Instruction::StaAbsolute(0x8001),
                Instruction::Pla,
                Instruction::Plp,
                Instruction::Rts,
            ],
        )?,
        build_chr_routine(
            "PPU $0000 FD CHR bank selection",
            SELECT_LEFT_FD_CHR_BANK_ADDRESS,
            0,
        )?,
        build_chr_routine(
            "PPU $0000 FE CHR bank selection",
            SELECT_LEFT_FE_CHR_BANK_ADDRESS,
            1,
        )?,
        build_chr_routine(
            "PPU $1000 FD CHR bank selection",
            SELECT_RIGHT_FD_CHR_BANK_ADDRESS,
            2,
        )?,
        build_chr_routine(
            "PPU $1000 FE CHR bank selection",
            SELECT_RIGHT_FE_CHR_BANK_ADDRESS,
            4,
        )?,
    ])
}

fn build_chr_routine(
    role: &'static str,
    cpu_address: u16,
    mapper_register: u8,
) -> Result<AssembledRoutine> {
    assemble_routine(
        role,
        cpu_address,
        &[
            Instruction::Php,
            Instruction::Pha,
            Instruction::AndImmediate(0x1F),
            Instruction::AslAccumulator,
            Instruction::AslAccumulator,
            Instruction::Clc,
            Instruction::AdcImmediate(8),
            Instruction::Pha,
            Instruction::LdaImmediate(mapper_register),
            Instruction::StaAbsolute(0x8000),
            Instruction::Pla,
            Instruction::StaAbsolute(0x8001),
            Instruction::Pla,
            Instruction::Plp,
            Instruction::Rts,
        ],
    )
}

fn assemble_routine(
    role: &'static str,
    cpu_address: u16,
    instructions: &[Instruction],
) -> Result<AssembledRoutine> {
    Ok(AssembledRoutine {
        role,
        cpu_address,
        bytes: assemble_at(cpu_address, instructions)?,
    })
}

fn validate_routine_placements(routines: &[AssembledRoutine]) -> Result<()> {
    let cave_end = CODE_CAVE_START_ADDRESS as usize + CODE_CAVE_LEN;
    for (index, routine) in routines.iter().enumerate() {
        let routine_end = routine.cpu_address as usize + routine.bytes.len();
        ensure!(
            routine.cpu_address >= CODE_CAVE_START_ADDRESS && routine_end <= cave_end,
            "mapper 165 {} routine is outside the proven code cave",
            routine.role
        );
        if let Some(next) = routines.get(index + 1) {
            ensure!(
                routine_end <= next.cpu_address as usize,
                "mapper 165 {} routine overlaps {}",
                routine.role,
                next.role
            );
        }
    }
    Ok(())
}

fn replace_central_prg_writer(image: &mut TrackedImage) -> Result<()> {
    let source = [
        Instruction::StaZeroPage(0x29),
        Instruction::StaZeroPage(0x51),
        Instruction::StaAbsolute(0xA000),
        Instruction::Rts,
    ];
    let replacement = [
        Instruction::StaZeroPage(0x29),
        Instruction::StaZeroPage(0x51),
        Instruction::JsrAbsolute(SELECT_PRG_BANK_ADDRESS),
        Instruction::Rts,
    ];
    replace_same_length_routine(
        image,
        "central PRG bank selector",
        SOURCE_SELECT_PRG_BANK_AND_SAVE_ADDRESS,
        &source,
        &replacement,
    )
}

fn replace_central_chr_writer(image: &mut TrackedImage, writer: CentralChrWriter) -> Result<()> {
    let source = [
        Instruction::StaZeroPage(writer.shadow_address),
        Instruction::OraZeroPage(0x52),
        Instruction::StaAbsolute(writer.source_register),
        Instruction::Rts,
    ];
    let replacement = [
        Instruction::StaZeroPage(writer.shadow_address),
        Instruction::OraZeroPage(0x52),
        Instruction::JsrAbsolute(writer.target_routine),
        Instruction::Rts,
    ];
    replace_same_length_routine(
        image,
        writer.role,
        writer.source_address,
        &source,
        &replacement,
    )
}

fn replace_same_length_routine(
    image: &mut TrackedImage,
    label: &str,
    source_address: u16,
    source: &[Instruction],
    replacement: &[Instruction],
) -> Result<()> {
    let expected = assemble_at(source_address, source)?;
    let replacement = assemble_at(source_address, replacement)?;
    ensure!(
        expected.len() == replacement.len(),
        "mapper 165 {label} replacement changed routine length"
    );
    image.write_expected(
        format!("redirect {label} to mapper 165"),
        fixed_bank_file_offset(source_address)?,
        &expected,
        &replacement,
    )
}

fn replace_direct_writer(image: &mut TrackedImage, writer: DirectWriter) -> Result<()> {
    let file_offset = match writer.location {
        WriterLocation::Fixed => fixed_bank_file_offset(writer.source_address)?,
        WriterLocation::Switchable { prg_bank } => {
            switchable_bank_file_offset(prg_bank, writer.source_address)?
        }
    };
    image.write_expected(
        format!("redirect {} to mapper 165", writer.role),
        file_offset,
        &assemble_at(
            writer.source_address,
            &[Instruction::StaAbsolute(writer.source_register)],
        )?,
        &assemble_at(
            writer.source_address,
            &[Instruction::JsrAbsolute(writer.target_routine)],
        )?,
    )
}

fn replace_mirroring_writer(
    image: &mut TrackedImage,
    role: &str,
    source_address: u16,
    value: u8,
) -> Result<()> {
    let source = [
        Instruction::LdaImmediate(value),
        Instruction::StaZeroPage(0xC8),
        Instruction::StaAbsolute(0xF000),
        Instruction::Rts,
    ];
    let replacement = [
        Instruction::LdaImmediate(value),
        Instruction::StaZeroPage(0xC8),
        Instruction::StaAbsolute(0xA000),
        Instruction::Rts,
    ];
    replace_same_length_routine(image, role, source_address, &source, &replacement)
}

fn verify_output(source_rom: &Rom, output_rom: &Rom, output: &[u8]) -> Result<()> {
    ensure!(
        output_rom.mapper() == OUTPUT_MAPPER,
        "output mapper is not 165"
    );
    ensure!(
        output_rom.prg().len() == PRG_SIZE,
        "mapper 165 changed PRG size"
    );
    ensure!(
        output_rom.chr().len() == source_rom.chr().len() + OUTPUT_CHR_PADDING_SIZE,
        "mapper 165 output CHR size is incorrect"
    );
    ensure!(
        output_rom.chr()[..OUTPUT_CHR_PADDING_SIZE]
            .iter()
            .all(|byte| *byte == 0),
        "mapper 165 reserved CHR prefix is not blank"
    );
    ensure!(
        output_rom.chr()[OUTPUT_CHR_PADDING_SIZE..] == *source_rom.chr(),
        "mapper 165 relocated source CHR changed"
    );
    ensure!(
        output[6] & 0x02 == source_rom.data()[6] & 0x02,
        "mapper 165 changed the iNES battery flag"
    );
    Ok(())
}

fn write_file(path: &Path, data: &[u8]) -> Result<()> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)
            .with_context(|| format!("create output directory {}", parent.display()))?;
    }
    fs::write(path, data).with_context(|| format!("write {}", path.display()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn routines_fit_disjoint_ranges_inside_the_proven_cave() {
        let routines = build_routines().unwrap();
        validate_routine_placements(&routines).unwrap();

        assert_eq!(routines.len(), 6);
        for routine in &routines {
            assert!(routine.bytes.len() <= 0x20);
        }
    }

    #[test]
    fn prg_selector_maps_one_mmc4_bank_to_two_consecutive_mmc3_banks() {
        let bytes = &build_routines().unwrap()[1].bytes;
        assert!(bytes.windows(3).any(|window| window == [0x8D, 0x00, 0x80]));
        assert!(bytes.windows(3).any(|window| window == [0x8D, 0x01, 0x80]));
        assert!(bytes.windows(2).any(|window| window == [0x29, 0x0F]));
        assert!(bytes.windows(2).any(|window| window == [0x09, 0x01]));
    }

    #[test]
    fn chr_selectors_bias_source_pages_away_from_chr_ram() {
        for routine in build_routines().unwrap().iter().skip(2) {
            assert!(
                routine
                    .bytes
                    .windows(2)
                    .any(|window| window == [0x29, 0x1F])
            );
            assert!(
                routine
                    .bytes
                    .windows(2)
                    .any(|window| window == [0x69, 0x08])
            );
        }

        assert_eq!(map_source_chr_page(0), 8);
        assert_eq!(map_source_chr_page(31), 132);
        assert_eq!(map_source_chr_page(0xFF), 132);
    }

    #[test]
    fn central_writer_redirects_preserve_source_lengths() {
        let source_prg = assemble_at(
            SOURCE_SELECT_PRG_BANK_AND_SAVE_ADDRESS,
            &[
                Instruction::StaZeroPage(0x29),
                Instruction::StaZeroPage(0x51),
                Instruction::StaAbsolute(0xA000),
                Instruction::Rts,
            ],
        )
        .unwrap();
        let target_prg = assemble_at(
            SOURCE_SELECT_PRG_BANK_AND_SAVE_ADDRESS,
            &[
                Instruction::StaZeroPage(0x29),
                Instruction::StaZeroPage(0x51),
                Instruction::JsrAbsolute(SELECT_PRG_BANK_ADDRESS),
                Instruction::Rts,
            ],
        )
        .unwrap();
        assert_eq!(source_prg.len(), target_prg.len());

        for writer in CENTRAL_CHR_WRITERS {
            let source = assemble_at(
                writer.source_address,
                &[
                    Instruction::StaZeroPage(writer.shadow_address),
                    Instruction::OraZeroPage(0x52),
                    Instruction::StaAbsolute(writer.source_register),
                    Instruction::Rts,
                ],
            )
            .unwrap();
            let replacement = assemble_at(
                writer.source_address,
                &[
                    Instruction::StaZeroPage(writer.shadow_address),
                    Instruction::OraZeroPage(0x52),
                    Instruction::JsrAbsolute(writer.target_routine),
                    Instruction::Rts,
                ],
            )
            .unwrap();
            assert_eq!(source.len(), replacement.len());
        }
    }

    #[test]
    fn direct_writer_redirects_keep_three_byte_instruction_size() {
        for writer in SOURCE_PRG_BANK_WRITERS.iter().chain(DIRECT_CHR_WRITERS) {
            let source = assemble_at(
                writer.source_address,
                &[Instruction::StaAbsolute(writer.source_register)],
            )
            .unwrap();
            let replacement = assemble_at(
                writer.source_address,
                &[Instruction::JsrAbsolute(writer.target_routine)],
            )
            .unwrap();
            assert_eq!(source.len(), 3);
            assert_eq!(replacement.len(), source.len());
        }
    }

    fn map_source_chr_page(source_page: u8) -> u8 {
        ((source_page & 0x1F) << 2) + 8
    }
}
