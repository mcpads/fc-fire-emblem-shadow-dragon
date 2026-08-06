use std::{fs, path::Path};

use anyhow::{Context, Result, ensure};
use serde::Serialize;

use crate::{
    rom::{EXPECTED_CHR_SHA1, EXPECTED_SOURCE_SHA1, HEADER_SIZE, PRG_SIZE, Rom},
    rp2a03::{Instruction, assemble_at},
    sha1_hex,
    tracked::TrackedImage,
};

pub(crate) const RESET_INITIALIZER_ADDRESS: u16 = 0xFA00;
const SELECT_PRG_BANK_AND_SAVE_ADDRESS: u16 = 0xFA20;
const SELECT_PRG_BANK_ADDRESS: u16 = 0xFA30;
const SELECT_HORIZONTAL_MIRRORING_ADDRESS: u16 = 0xFA40;
const SELECT_VERTICAL_MIRRORING_ADDRESS: u16 = 0xFA50;

pub(crate) const SOURCE_RESET_ADDRESS: u16 = 0xC075;
const SOURCE_SELECT_PRG_BANK_AND_SAVE_ADDRESS: u16 = 0xC9A6;
const SOURCE_SELECT_HORIZONTAL_MIRRORING_ADDRESS: u16 = 0xC9CE;
const SOURCE_SELECT_VERTICAL_MIRRORING_ADDRESS: u16 = 0xC9D6;
const SOURCE_BOOT_SELECT_TEMPORARY_BANK_ADDRESS: u16 = 0xC1FD;
const SOURCE_BOOT_RESTORE_BANK_ADDRESS: u16 = 0xC205;
const SOURCE_INDEXED_POINTER_TABLE_SELECT_BANK_ADDRESS: u16 = 0xE6BA;
const SOURCE_INDEXED_POINTER_TABLE_RESTORE_BANK_ADDRESS: u16 = 0xE6F1;

const CODE_CAVE_START_ADDRESS: u16 = 0xFA00;
const CODE_CAVE_LEN: usize = 0x80;

#[derive(Debug, Serialize)]
struct Mmc5PrgProbeReport {
    schema: u32,
    source_sha1: &'static str,
    output_sha1: String,
    source_mapper: u16,
    output_mapper: u16,
    prg_size: usize,
    chr_size: usize,
    chr_sha1: &'static str,
    battery_flag_preserved: bool,
    prg_mode: u8,
    prg_ram_bank: u8,
    code_cave: CodeCaveEvidence,
    direct_code_cave_transfer_count: usize,
    routines: Vec<RoutinePlacement>,
    tracked_writes: Vec<TrackedWrite>,
    unresolved_boundaries: Vec<&'static str>,
    release_eligible: bool,
}

#[derive(Debug, Serialize)]
struct CodeCaveEvidence {
    cpu_start: String,
    file_start: String,
    len: usize,
    expected_fill: String,
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

pub(crate) struct PrgProbeImage {
    data: Vec<u8>,
    cave_file_start: usize,
    direct_code_cave_transfer_count: usize,
    routines: Vec<AssembledRoutine>,
    tracked_writes: Vec<TrackedWrite>,
}

impl PrgProbeImage {
    pub(crate) fn data(&self) -> &[u8] {
        &self.data
    }
}

pub(crate) fn create_mmc5_prg_probe_image(source_rom: &Rom) -> Result<PrgProbeImage> {
    source_rom.verify_supported_japanese()?;
    let source = source_rom.data().to_vec();
    let cave_file_start = fixed_bank_file_offset(CODE_CAVE_START_ADDRESS)?;
    let cave_file_end = cave_file_start
        .checked_add(CODE_CAVE_LEN)
        .ok_or_else(|| anyhow::anyhow!("MMC5 probe code cave range overflow"))?;
    ensure!(
        source[cave_file_start..cave_file_end]
            .iter()
            .all(|byte| *byte == 0xFF),
        "MMC5 probe code cave is no longer all FF"
    );
    let direct_code_cave_transfer_count = count_direct_transfers_to_range(
        source_rom.prg(),
        CODE_CAVE_START_ADDRESS,
        cave_end_address(),
    )?;
    ensure!(
        direct_code_cave_transfer_count == 0,
        "MMC5 probe code cave has {direct_code_cave_transfer_count} direct JSR or JMP references"
    );

    let routines = build_routines()?;
    validate_routine_placements(&routines)?;

    let mut image = TrackedImage::new(source.clone());
    image.write_expected("iNES mapper 10 to mapper 5", 6, &[0xA2], &[0x52])?;

    for routine in &routines {
        let file_offset = fixed_bank_file_offset(routine.cpu_address)?;
        image.write_expected(
            format!("MMC5 {} routine", routine.role),
            file_offset,
            &vec![0xFF; routine.bytes.len()],
            &routine.bytes,
        )?;
    }

    replace_source_routine(
        &mut image,
        "central PRG bank selector",
        SOURCE_SELECT_PRG_BANK_AND_SAVE_ADDRESS,
        &[
            Instruction::StaZeroPage(0x29),
            Instruction::StaZeroPage(0x51),
            Instruction::StaAbsolute(0xA000),
            Instruction::Rts,
        ],
        SELECT_PRG_BANK_AND_SAVE_ADDRESS,
    )?;
    replace_source_routine(
        &mut image,
        "horizontal mirroring selector",
        SOURCE_SELECT_HORIZONTAL_MIRRORING_ADDRESS,
        &[
            Instruction::LdaImmediate(1),
            Instruction::StaZeroPage(0xC8),
            Instruction::StaAbsolute(0xF000),
            Instruction::Rts,
        ],
        SELECT_HORIZONTAL_MIRRORING_ADDRESS,
    )?;
    replace_source_routine(
        &mut image,
        "vertical mirroring selector",
        SOURCE_SELECT_VERTICAL_MIRRORING_ADDRESS,
        &[
            Instruction::LdaImmediate(0),
            Instruction::StaZeroPage(0xC8),
            Instruction::StaAbsolute(0xF000),
            Instruction::Rts,
        ],
        SELECT_VERTICAL_MIRRORING_ADDRESS,
    )?;
    replace_absolute_store_with_subroutine_call(
        &mut image,
        "boot temporary PRG bank selection",
        SOURCE_BOOT_SELECT_TEMPORARY_BANK_ADDRESS,
    )?;
    replace_absolute_store_with_subroutine_call(
        &mut image,
        "boot PRG bank restoration",
        SOURCE_BOOT_RESTORE_BANK_ADDRESS,
    )?;
    replace_absolute_store_with_subroutine_call(
        &mut image,
        "indexed pointer table PRG bank selection",
        SOURCE_INDEXED_POINTER_TABLE_SELECT_BANK_ADDRESS,
    )?;
    replace_absolute_store_with_subroutine_call(
        &mut image,
        "indexed pointer table PRG bank restoration",
        SOURCE_INDEXED_POINTER_TABLE_RESTORE_BANK_ADDRESS,
    )?;
    image.write_expected(
        "reset vector to MMC5 initializer",
        fixed_bank_file_offset(0xFFFC)?,
        &SOURCE_RESET_ADDRESS.to_le_bytes(),
        &RESET_INITIALIZER_ADDRESS.to_le_bytes(),
    )?;

    image.verify_all_changes_tracked(&source)?;
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
    let output_rom = Rom::parse(output.clone()).context("parse MMC5 PRG probe output")?;
    ensure!(
        output_rom.mapper() == 5,
        "MMC5 probe output mapper is not 5"
    );
    ensure!(
        output_rom.prg().len() == PRG_SIZE,
        "MMC5 probe changed PRG size"
    );
    ensure!(
        sha1_hex(output_rom.chr()) == EXPECTED_CHR_SHA1,
        "MMC5 probe changed source CHR"
    );
    ensure!(
        output[6] & 0x02 == source[6] & 0x02,
        "MMC5 probe changed the iNES battery flag"
    );

    Ok(PrgProbeImage {
        data: output,
        cave_file_start,
        direct_code_cave_transfer_count,
        routines,
        tracked_writes,
    })
}

pub fn build_mmc5_prg_probe(
    source_path: &Path,
    output_path: &Path,
    report_path: &Path,
) -> Result<BuildSummary> {
    let source_rom = Rom::from_path(source_path)?;
    let probe = create_mmc5_prg_probe_image(&source_rom)?;
    let output_rom = Rom::parse(probe.data.clone()).context("parse MMC5 PRG probe output")?;
    let output_sha1 = sha1_hex(&probe.data);
    let report = Mmc5PrgProbeReport {
        schema: 1,
        source_sha1: EXPECTED_SOURCE_SHA1,
        output_sha1: output_sha1.clone(),
        source_mapper: source_rom.mapper(),
        output_mapper: output_rom.mapper(),
        prg_size: output_rom.prg().len(),
        chr_size: output_rom.chr().len(),
        chr_sha1: EXPECTED_CHR_SHA1,
        battery_flag_preserved: true,
        prg_mode: 1,
        prg_ram_bank: 0,
        code_cave: CodeCaveEvidence {
            cpu_start: format!("0x{CODE_CAVE_START_ADDRESS:04X}"),
            file_start: format!("0x{:06X}", probe.cave_file_start),
            len: CODE_CAVE_LEN,
            expected_fill: "0xFF".to_owned(),
        },
        direct_code_cave_transfer_count: probe.direct_code_cave_transfer_count,
        routines: probe
            .routines
            .iter()
            .map(|routine| RoutinePlacement {
                role: routine.role,
                cpu_address: format!("0x{:04X}", routine.cpu_address),
                len: routine.bytes.len(),
            })
            .collect(),
        tracked_writes: probe.tracked_writes,
        unresolved_boundaries: vec![
            "MMC4 CHR latch behavior has not been converted to MMC5.",
            "Only the four direct $A000 stores proven on the observed boot and indexed-pointer-table paths are redirected; other byte-pattern candidates remain unclassified.",
            "The all-FF cave has no direct JSR or JMP reference, but indirect and data references are not fully disproven.",
            "No runtime boot, graphics, save/load, or progression equivalence is claimed by this static probe.",
        ],
        release_eligible: false,
    };
    let report_bytes = serde_json::to_vec_pretty(&report).context("serialize MMC5 PRG report")?;
    let report_sha1 = sha1_hex(&report_bytes);

    let tracked_write_count = report.tracked_writes.len();
    write_file(output_path, &probe.data)?;
    write_file(report_path, &report_bytes)?;
    Ok(BuildSummary {
        output_sha1,
        report_sha1,
        tracked_write_count,
    })
}

#[derive(Debug)]
struct AssembledRoutine {
    role: &'static str,
    cpu_address: u16,
    bytes: Vec<u8>,
}

fn build_routines() -> Result<Vec<AssembledRoutine>> {
    Ok(vec![
        assemble_routine(
            "reset initialization",
            RESET_INITIALIZER_ADDRESS,
            &[
                Instruction::LdaImmediate(1),
                Instruction::StaAbsolute(0x5100),
                Instruction::LdaImmediate(0x9F),
                Instruction::StaAbsolute(0x5117),
                Instruction::LdaImmediate(0),
                Instruction::StaAbsolute(0x5113),
                Instruction::LdaImmediate(2),
                Instruction::StaAbsolute(0x5102),
                Instruction::LdaImmediate(1),
                Instruction::StaAbsolute(0x5103),
                Instruction::JmpAbsolute(SOURCE_RESET_ADDRESS),
            ],
        )?,
        assemble_routine(
            "bank selection with source shadows",
            SELECT_PRG_BANK_AND_SAVE_ADDRESS,
            &[
                Instruction::StaZeroPage(0x29),
                Instruction::StaZeroPage(0x51),
                Instruction::JmpAbsolute(SELECT_PRG_BANK_ADDRESS),
            ],
        )?,
        assemble_routine(
            "raw 16 KiB PRG bank selection",
            SELECT_PRG_BANK_ADDRESS,
            &[
                Instruction::Php,
                Instruction::Pha,
                Instruction::AslAccumulator,
                Instruction::OraImmediate(0x80),
                Instruction::StaAbsolute(0x5115),
                Instruction::Pla,
                Instruction::Plp,
                Instruction::Rts,
            ],
        )?,
        assemble_routine(
            "horizontal mirroring selection",
            SELECT_HORIZONTAL_MIRRORING_ADDRESS,
            &[
                Instruction::LdaImmediate(1),
                Instruction::StaZeroPage(0xC8),
                Instruction::Pha,
                Instruction::LdaImmediate(0x50),
                Instruction::StaAbsolute(0x5105),
                Instruction::Pla,
                Instruction::Rts,
            ],
        )?,
        assemble_routine(
            "vertical mirroring selection",
            SELECT_VERTICAL_MIRRORING_ADDRESS,
            &[
                Instruction::LdaImmediate(0),
                Instruction::StaZeroPage(0xC8),
                Instruction::Pha,
                Instruction::LdaImmediate(0x44),
                Instruction::StaAbsolute(0x5105),
                Instruction::Pla,
                Instruction::Rts,
            ],
        )?,
    ])
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
            "MMC5 {} routine is outside the proven code cave",
            routine.role
        );
        if let Some(next) = routines.get(index + 1) {
            ensure!(
                routine_end <= next.cpu_address as usize,
                "MMC5 {} routine overlaps {}",
                routine.role,
                next.role
            );
        }
    }
    Ok(())
}

fn replace_source_routine(
    image: &mut TrackedImage,
    label: &str,
    source_address: u16,
    source_instructions: &[Instruction],
    replacement_address: u16,
) -> Result<()> {
    let expected = assemble_at(source_address, source_instructions)?;
    let mut replacement = assemble_at(
        source_address,
        &[Instruction::JmpAbsolute(replacement_address)],
    )?;
    ensure!(
        replacement.len() <= expected.len(),
        "replacement jump does not fit source routine"
    );
    replacement.extend(std::iter::repeat_n(
        assemble_at(source_address, &[Instruction::Nop])?[0],
        expected.len() - replacement.len(),
    ));
    image.write_expected(
        format!("redirect {label}"),
        fixed_bank_file_offset(source_address)?,
        &expected,
        &replacement,
    )
}

fn replace_absolute_store_with_subroutine_call(
    image: &mut TrackedImage,
    label: &str,
    source_address: u16,
) -> Result<()> {
    image.write_expected(
        format!("redirect {label}"),
        fixed_bank_file_offset(source_address)?,
        &assemble_at(source_address, &[Instruction::StaAbsolute(0xA000)])?,
        &assemble_at(
            source_address,
            &[Instruction::JsrAbsolute(SELECT_PRG_BANK_ADDRESS)],
        )?,
    )
}

pub(crate) fn fixed_bank_file_offset(cpu_address: u16) -> Result<usize> {
    ensure!(
        cpu_address >= 0xC000,
        "CPU address {cpu_address:04X} is outside the fixed PRG bank"
    );
    Ok(HEADER_SIZE + (PRG_SIZE - 0x4000) + (cpu_address as usize - 0xC000))
}

fn cave_end_address() -> u16 {
    CODE_CAVE_START_ADDRESS + CODE_CAVE_LEN as u16
}

fn count_direct_transfers_to_range(prg: &[u8], start: u16, end: u16) -> Result<usize> {
    ensure!(start < end, "direct transfer target range is empty");
    Ok(prg
        .windows(3)
        .filter(|window| {
            matches!(window[0], 0x20 | 0x4C) && {
                let target = u16::from_le_bytes([window[1], window[2]]);
                (start..end).contains(&target)
            }
        })
        .count())
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

        assert_eq!(routines[0].cpu_address, 0xFA00);
        assert_eq!(routines[0].bytes.len(), 28);
        assert_eq!(routines[1].cpu_address, 0xFA20);
        assert_eq!(routines[1].bytes.len(), 7);
        assert_eq!(routines[2].cpu_address, 0xFA30);
        assert_eq!(routines[2].bytes.len(), 11);
        assert_eq!(routines[3].cpu_address, 0xFA40);
        assert_eq!(routines[3].bytes.len(), 12);
        assert_eq!(routines[4].cpu_address, 0xFA50);
        assert_eq!(routines[4].bytes.len(), 12);
    }

    #[test]
    fn source_routine_redirects_preserve_their_original_lengths() {
        let central = assemble_at(
            SOURCE_SELECT_PRG_BANK_AND_SAVE_ADDRESS,
            &[
                Instruction::StaZeroPage(0x29),
                Instruction::StaZeroPage(0x51),
                Instruction::StaAbsolute(0xA000),
                Instruction::Rts,
            ],
        )
        .unwrap();
        let mirror = assemble_at(
            SOURCE_SELECT_HORIZONTAL_MIRRORING_ADDRESS,
            &[
                Instruction::LdaImmediate(1),
                Instruction::StaZeroPage(0xC8),
                Instruction::StaAbsolute(0xF000),
                Instruction::Rts,
            ],
        )
        .unwrap();

        assert_eq!(central.len(), 8);
        assert_eq!(mirror.len(), 8);
    }

    #[test]
    fn direct_prg_bank_redirects_keep_the_original_instruction_size() {
        for source_address in [
            SOURCE_BOOT_SELECT_TEMPORARY_BANK_ADDRESS,
            SOURCE_BOOT_RESTORE_BANK_ADDRESS,
            SOURCE_INDEXED_POINTER_TABLE_SELECT_BANK_ADDRESS,
            SOURCE_INDEXED_POINTER_TABLE_RESTORE_BANK_ADDRESS,
        ] {
            let source = assemble_at(source_address, &[Instruction::StaAbsolute(0xA000)]).unwrap();
            let replacement = assemble_at(
                source_address,
                &[Instruction::JsrAbsolute(SELECT_PRG_BANK_ADDRESS)],
            )
            .unwrap();
            assert_eq!(source.len(), 3);
            assert_eq!(replacement.len(), source.len());
        }
    }

    #[test]
    fn fixed_bank_cpu_addresses_map_to_expected_file_offsets() {
        assert_eq!(fixed_bank_file_offset(0xC000).unwrap(), 0x3C010);
        assert_eq!(fixed_bank_file_offset(0xFA00).unwrap(), 0x3FA10);
        assert_eq!(fixed_bank_file_offset(0xFFFC).unwrap(), 0x4000C);
        assert!(fixed_bank_file_offset(0xBFFF).is_err());
    }

    #[test]
    fn direct_transfer_scan_distinguishes_operands_from_unreferenced_values() {
        let prg = [
            0x20, 0x00, 0xFB, 0x4C, 0x7F, 0xFB, 0x20, 0x80, 0xFB, 0x00, 0x00, 0xFB,
        ];

        assert_eq!(
            count_direct_transfers_to_range(&prg, 0xFB00, 0xFB80).unwrap(),
            2
        );
    }
}
