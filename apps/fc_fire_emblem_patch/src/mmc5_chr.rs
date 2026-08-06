use std::{fs, path::Path};

use anyhow::{Context, Result, ensure};
use serde::Serialize;

use crate::{
    mmc5_prg::{
        RESET_INITIALIZER_ADDRESS, SOURCE_RESET_ADDRESS, create_mmc5_prg_probe_image,
        fixed_bank_file_offset,
    },
    rom::{EXPECTED_CHR_SHA1, EXPECTED_SOURCE_SHA1, PRG_SIZE, Rom},
    rp2a03::{Instruction, assemble_at},
    sha1_hex,
    tracked::TrackedImage,
};

const RESET_INITIALIZER_TAIL_ADDRESS: u16 = RESET_INITIALIZER_ADDRESS + 0x19;
const CHR_MODE_INITIALIZER_ADDRESS: u16 = 0xFA60;

const CHR_WRITERS: &[ChrWriter] = &[
    ChrWriter {
        role: "PPU $0000 FD source",
        source_address: 0xC9AE,
        shadow_address: 0x59,
        source_register: 0xB000,
        target_register: 0x5123,
    },
    ChrWriter {
        role: "PPU $0000 FE source",
        source_address: 0xC9B6,
        shadow_address: 0x5A,
        source_register: 0xC000,
        target_register: 0x5123,
    },
    ChrWriter {
        role: "PPU $1000 FD source",
        source_address: 0xC9BE,
        shadow_address: 0x5B,
        source_register: 0xD000,
        target_register: 0x5127,
    },
    ChrWriter {
        role: "PPU $1000 FE source",
        source_address: 0xC9C6,
        shadow_address: 0x5C,
        source_register: 0xE000,
        target_register: 0x5127,
    },
];

#[derive(Debug, Clone, Copy)]
struct ChrWriter {
    role: &'static str,
    source_address: u16,
    shadow_address: u8,
    source_register: u16,
    target_register: u16,
}

#[derive(Debug, Serialize)]
struct Mmc5ChrWriterProbeReport {
    schema: u32,
    source_sha1: &'static str,
    base_prg_probe_sha1: String,
    output_sha1: String,
    output_mapper: u16,
    prg_size: usize,
    chr_size: usize,
    chr_sha1: &'static str,
    chr_mode: u8,
    projection: &'static str,
    fd_fe_latch_equivalent: bool,
    writer_mappings: Vec<ChrWriterMapping>,
    tracked_delta_writes: Vec<TrackedWrite>,
    unresolved_boundaries: Vec<&'static str>,
    release_eligible: bool,
}

#[derive(Debug, Serialize)]
struct ChrWriterMapping {
    role: &'static str,
    source_cpu_address: String,
    shadow_address: String,
    source_register: String,
    target_register: String,
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
    pub tracked_delta_write_count: usize,
}

pub fn build_mmc5_chr_writer_probe(
    source_path: &Path,
    output_path: &Path,
    report_path: &Path,
) -> Result<BuildSummary> {
    let source_rom = Rom::from_path(source_path)?;
    source_rom.verify_supported_japanese()?;
    let prg_probe = create_mmc5_prg_probe_image(&source_rom)?;
    let base = prg_probe.data().to_vec();
    let base_prg_probe_sha1 = sha1_hex(&base);
    let mut image = TrackedImage::new(base.clone());

    image.write_expected(
        "redirect reset tail to CHR mode initializer",
        fixed_bank_file_offset(RESET_INITIALIZER_TAIL_ADDRESS)?,
        &assemble_at(
            RESET_INITIALIZER_TAIL_ADDRESS,
            &[Instruction::JmpAbsolute(SOURCE_RESET_ADDRESS)],
        )?,
        &assemble_at(
            RESET_INITIALIZER_TAIL_ADDRESS,
            &[Instruction::JmpAbsolute(CHR_MODE_INITIALIZER_ADDRESS)],
        )?,
    )?;
    let chr_mode_initializer = assemble_at(
        CHR_MODE_INITIALIZER_ADDRESS,
        &[
            Instruction::LdaImmediate(1),
            Instruction::StaAbsolute(0x5101),
            Instruction::JmpAbsolute(SOURCE_RESET_ADDRESS),
        ],
    )?;
    image.write_expected(
        "MMC5 4 KiB CHR mode initializer",
        fixed_bank_file_offset(CHR_MODE_INITIALIZER_ADDRESS)?,
        &vec![0xFF; chr_mode_initializer.len()],
        &chr_mode_initializer,
    )?;

    for writer in CHR_WRITERS {
        replace_chr_writer(&mut image, *writer)?;
    }

    image.verify_all_changes_tracked(&base)?;
    let tracked_delta_writes = image
        .writes()
        .iter()
        .map(|write| TrackedWrite {
            label: write.label.clone(),
            file_offset: format!("0x{:06X}", write.offset),
            len: write.len,
        })
        .collect::<Vec<_>>();
    let tracked_delta_write_count = tracked_delta_writes.len();
    let output = image.into_data();
    let output_rom = Rom::parse(output.clone()).context("parse MMC5 CHR writer probe")?;
    ensure!(
        output_rom.mapper() == 5,
        "MMC5 CHR writer probe mapper is not 5"
    );
    ensure!(
        output_rom.prg().len() == PRG_SIZE,
        "MMC5 CHR writer probe changed PRG size"
    );
    ensure!(
        sha1_hex(output_rom.chr()) == EXPECTED_CHR_SHA1,
        "MMC5 CHR writer probe changed source CHR"
    );

    let output_sha1 = sha1_hex(&output);
    let report = Mmc5ChrWriterProbeReport {
        schema: 1,
        source_sha1: EXPECTED_SOURCE_SHA1,
        base_prg_probe_sha1,
        output_sha1: output_sha1.clone(),
        output_mapper: output_rom.mapper(),
        prg_size: output_rom.prg().len(),
        chr_size: output_rom.chr().len(),
        chr_sha1: EXPECTED_CHR_SHA1,
        chr_mode: 1,
        projection: "last MMC4 writer per 4 KiB PPU window",
        fd_fe_latch_equivalent: false,
        writer_mappings: CHR_WRITERS
            .iter()
            .map(|writer| ChrWriterMapping {
                role: writer.role,
                source_cpu_address: format!("0x{:04X}", writer.source_address),
                shadow_address: format!("0x{:02X}", writer.shadow_address),
                source_register: format!("0x{:04X}", writer.source_register),
                target_register: format!("0x{:04X}", writer.target_register),
            })
            .collect(),
        tracked_delta_writes,
        unresolved_boundaries: vec![
            "MMC4 FD and FE latch banks collapse to one last-writer bank per 4 KiB PPU window.",
            "Screens whose paired shadows differ cannot be declared visually equivalent.",
            "The projection relies on the original 8x8 sprite mode selecting MMC5 CHR register set A.",
            "Unclassified direct writes outside the four central MMC4 CHR routines remain unconverted.",
            "No runtime graphics, progression, or save/load equivalence is claimed by this static probe.",
        ],
        release_eligible: false,
    };
    let report_bytes =
        serde_json::to_vec_pretty(&report).context("serialize MMC5 CHR writer report")?;
    let report_sha1 = sha1_hex(&report_bytes);

    write_file(output_path, &output)?;
    write_file(report_path, &report_bytes)?;
    Ok(BuildSummary {
        output_sha1,
        report_sha1,
        tracked_delta_write_count,
    })
}

fn replace_chr_writer(image: &mut TrackedImage, writer: ChrWriter) -> Result<()> {
    let source_instructions = [
        Instruction::StaZeroPage(writer.shadow_address),
        Instruction::OraZeroPage(0x52),
        Instruction::StaAbsolute(writer.source_register),
        Instruction::Rts,
    ];
    let target_instructions = [
        Instruction::StaZeroPage(writer.shadow_address),
        Instruction::OraZeroPage(0x52),
        Instruction::StaAbsolute(writer.target_register),
        Instruction::Rts,
    ];
    let expected = assemble_at(writer.source_address, &source_instructions)?;
    let replacement = assemble_at(writer.source_address, &target_instructions)?;
    ensure!(
        expected.len() == replacement.len(),
        "MMC5 CHR writer replacement changed routine length"
    );
    image.write_expected(
        format!("project {} to MMC5", writer.role),
        fixed_bank_file_offset(writer.source_address)?,
        &expected,
        &replacement,
    )
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
    fn chr_mode_initializer_preserves_the_source_reset_entry_value() {
        let bytes = assemble_at(
            CHR_MODE_INITIALIZER_ADDRESS,
            &[
                Instruction::LdaImmediate(1),
                Instruction::StaAbsolute(0x5101),
                Instruction::JmpAbsolute(SOURCE_RESET_ADDRESS),
            ],
        )
        .unwrap();

        assert_eq!(bytes, [0xA9, 0x01, 0x8D, 0x01, 0x51, 0x4C, 0x75, 0xC0]);
    }

    #[test]
    fn paired_latch_writers_project_to_one_register_per_ppu_window() {
        assert_eq!(CHR_WRITERS[0].target_register, 0x5123);
        assert_eq!(CHR_WRITERS[1].target_register, 0x5123);
        assert_eq!(CHR_WRITERS[2].target_register, 0x5127);
        assert_eq!(CHR_WRITERS[3].target_register, 0x5127);

        for writer in CHR_WRITERS {
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
            let target = assemble_at(
                writer.source_address,
                &[
                    Instruction::StaZeroPage(writer.shadow_address),
                    Instruction::OraZeroPage(0x52),
                    Instruction::StaAbsolute(writer.target_register),
                    Instruction::Rts,
                ],
            )
            .unwrap();
            assert_eq!(source.len(), 8);
            assert_eq!(target.len(), source.len());
        }
    }
}
