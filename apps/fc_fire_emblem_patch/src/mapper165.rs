use std::{fs, path::Path};

use anyhow::{Context, Result, ensure};
use serde::Serialize;

#[cfg(test)]
use crate::rp2a03::{Instruction, assemble_at};
use crate::{
    mmc5_prg::{SOURCE_RESET_ADDRESS, count_direct_transfers_to_range, fixed_bank_file_offset},
    rom::{CHR_FILE_OFFSET, EXPECTED_CHR_SHA1, EXPECTED_SOURCE_SHA1, PRG_SIZE, Rom},
    sha1_hex,
    static_analysis::find_absolute_write_candidates,
    tracked::TrackedImage,
};
mod runtime;
pub(crate) mod trigger_planes;
mod trigger_variants;
mod writer_sites;

use runtime::{
    build_routines, replace_central_chr_writer, replace_central_prg_writer, replace_direct_writer,
    replace_mirroring_writer, validate_routine_placements,
};

use trigger_variants::{
    TriggerVariantPlan, install_observed_trigger_variants, verify_installed_trigger_variants,
};

use writer_sites::{CENTRAL_CHR_WRITERS, DIRECT_CHR_WRITERS, SOURCE_PRG_BANK_WRITERS};

const OUTPUT_MAPPER: u16 = 165;
const OUTPUT_CHR_PADDING_SIZE: usize = 8 * 1024;
const OUTPUT_CHR_BANK_COUNT: u8 = 17;
const RESET_INITIALIZER_ADDRESS: u16 = 0xFA00;
const SELECT_PRG_BANK_ADDRESS: u16 = 0xFA20;
const SELECT_LEFT_FD_CHR_BANK_ADDRESS: u16 = 0xFA40;
const SELECT_LEFT_FE_CHR_BANK_ADDRESS: u16 = 0xFA60;
const SELECT_RIGHT_FD_CHR_BANK_ADDRESS: u16 = 0xFA80;
const SELECT_RIGHT_FE_CHR_BANK_ADDRESS: u16 = 0xFAA0;
const SELECT_CENTRAL_RIGHT_FE_CHR_BANK_ADDRESS: u16 = 0xFAB8;
const SELECT_RIGHT_FD_CHR_BANK_FOR_PAIR_ADDRESS: u16 = 0xFAC0;
const CODE_CAVE_START_ADDRESS: u16 = RESET_INITIALIZER_ADDRESS;
const CODE_CAVE_LEN: usize = 0x110;

const SOURCE_SELECT_PRG_BANK_AND_SAVE_ADDRESS: u16 = 0xC9A6;
const SOURCE_SELECT_HORIZONTAL_MIRRORING_ADDRESS: u16 = 0xC9CE;
const SOURCE_SELECT_VERTICAL_MIRRORING_ADDRESS: u16 = 0xC9D6;

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
    trigger_plane_correction: TriggerPlaneCorrectionEvidence,
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
struct TriggerPlaneCorrectionEvidence {
    installed_variants: Vec<InstalledTriggerVariantEvidence>,
    selector_entries: Vec<PairSelectorEvidence>,
    central_right_writers_pair_aware: bool,
    direct_writers_pair_aware: bool,
}

#[derive(Debug, Serialize)]
struct InstalledTriggerVariantEvidence {
    physical_4k_page: u8,
    mapper_register_value: u8,
    fd_source_page: u8,
    required_high_plane_sha1: String,
    compatible_fe_source_pages: Vec<u8>,
    pattern_windows: Vec<&'static str>,
}

#[derive(Debug, Serialize)]
struct PairSelectorEvidence {
    pattern_window: &'static str,
    fd_source_page: u8,
    fe_source_page: u8,
    mapper_register_value: u8,
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

    let (base, trigger_variant_plan) = create_chr_relocated_image(&source_rom)?;
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

    let routines = build_routines(&trigger_variant_plan.selector_entries)?;
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
    verify_output(&source_rom, &output_rom, &output, &trigger_variant_plan)?;

    let output_sha1 = sha1_hex(&output);
    let relocated_source_chr_sha1 = sha1_hex(&output_rom.chr()[OUTPUT_CHR_PADDING_SIZE..]);
    let output_chr_sha1 = sha1_hex(output_rom.chr());
    let report = Mapper165ParityReport {
        schema: 2,
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
        trigger_plane_correction: TriggerPlaneCorrectionEvidence {
            installed_variants: trigger_variant_plan
                .installed_variants
                .iter()
                .map(|variant| InstalledTriggerVariantEvidence {
                    physical_4k_page: variant.physical_page,
                    mapper_register_value: variant.mapper_register_value,
                    fd_source_page: variant.fd_source_page,
                    required_high_plane_sha1: sha1_hex(&variant.required_high_plane),
                    compatible_fe_source_pages: variant.compatible_fe_source_pages.clone(),
                    pattern_windows: variant
                        .pattern_windows
                        .iter()
                        .map(|window| window.label())
                        .collect(),
                })
                .collect(),
            selector_entries: trigger_variant_plan
                .selector_entries
                .iter()
                .map(|entry| PairSelectorEvidence {
                    pattern_window: entry.pattern_window.label(),
                    fd_source_page: entry.fd_source_page,
                    fe_source_page: entry.fe_source_page,
                    mapper_register_value: entry.mapper_register_value,
                })
                .collect(),
            central_right_writers_pair_aware: true,
            direct_writers_pair_aware: false,
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
            "Observed central PPU $1000 pairs use generated trigger-plane variants; unobserved pairs still require visible parity measurement.",
            "Direct CHR writers are limited to instruction-boundary sites proven by fixed-bank disassembly, adjacent register groups, or prior runtime traces; isolated byte-pattern candidates remain unclassified.",
            "Direct CHR writers keep source-page mapping without pair-aware FD correction until their paired runtime values are classified.",
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

fn create_chr_relocated_image(source_rom: &Rom) -> Result<(Vec<u8>, TriggerVariantPlan)> {
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
    let trigger_variant_plan = install_observed_trigger_variants(
        source_rom.chr(),
        &mut output[CHR_FILE_OFFSET..CHR_FILE_OFFSET + OUTPUT_CHR_PADDING_SIZE],
    )?;
    Ok((output, trigger_variant_plan))
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

fn verify_output(
    source_rom: &Rom,
    output_rom: &Rom,
    output: &[u8],
    trigger_variant_plan: &TriggerVariantPlan,
) -> Result<()> {
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
    verify_installed_trigger_variants(
        source_rom.chr(),
        &output_rom.chr()[..OUTPUT_CHR_PADDING_SIZE],
        trigger_variant_plan,
    )?;
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
        let routines = build_routines(&[]).unwrap();
        validate_routine_placements(&routines).unwrap();

        assert_eq!(routines.len(), 8);
    }

    #[test]
    fn prg_selector_maps_one_mmc4_bank_to_two_consecutive_mmc3_banks() {
        let bytes = &build_routines(&[]).unwrap()[1].bytes;
        assert!(bytes.windows(3).any(|window| window == [0x8D, 0x00, 0x80]));
        assert!(bytes.windows(3).any(|window| window == [0x8D, 0x01, 0x80]));
        assert!(bytes.windows(2).any(|window| window == [0x29, 0x0F]));
        assert!(bytes.windows(2).any(|window| window == [0x09, 0x01]));
    }

    #[test]
    fn chr_selectors_bias_source_pages_away_from_chr_ram() {
        for routine in build_routines(&[]).unwrap().iter().skip(2).take(4) {
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

    #[test]
    fn pair_aware_right_selector_preserves_a_and_flags_and_selects_the_variant() {
        let entry = trigger_variants::PairSelectorEntry {
            pattern_window: trigger_planes::PatternWindow::Right,
            fd_source_page: 0,
            fe_source_page: 0x14,
            mapper_register_value: 4,
        };
        let routines = build_routines(&[entry]).unwrap();
        let selector = routines
            .iter()
            .find(|routine| routine.cpu_address == SELECT_RIGHT_FD_CHR_BANK_FOR_PAIR_ADDRESS)
            .unwrap();

        assert_eq!(&selector.bytes[..2], &[0x08, 0x48]);
        assert_eq!(
            &selector.bytes[selector.bytes.len() - 3..],
            &[0x68, 0x28, 0x60]
        );
        assert!(
            selector
                .bytes
                .windows(8)
                .any(|bytes| bytes == [0xA5, 0x5B, 0x05, 0x52, 0x29, 0x1F, 0xC9, 0x00])
        );
        assert!(
            selector
                .bytes
                .windows(8)
                .any(|bytes| bytes == [0xA5, 0x5C, 0x05, 0x52, 0x29, 0x1F, 0xC9, 0x14])
        );
        assert!(selector.bytes.windows(2).any(|bytes| bytes == [0xA9, 0x04]));
    }

    #[test]
    fn central_fe_refreshes_pair_selection_while_direct_writers_stay_stateless() {
        let central_right_fe = CENTRAL_CHR_WRITERS
            .iter()
            .find(|writer| writer.source_register == 0xE000)
            .unwrap();
        assert_eq!(
            central_right_fe.target_routine,
            SELECT_CENTRAL_RIGHT_FE_CHR_BANK_ADDRESS
        );
        assert!(
            DIRECT_CHR_WRITERS
                .iter()
                .filter(|writer| writer.source_register == 0xE000)
                .all(|writer| writer.target_routine == SELECT_RIGHT_FE_CHR_BANK_ADDRESS)
        );

        let wrapper = build_routines(&[])
            .unwrap()
            .into_iter()
            .find(|routine| routine.cpu_address == SELECT_CENTRAL_RIGHT_FE_CHR_BANK_ADDRESS)
            .unwrap();
        let expected = assemble_at(
            SELECT_CENTRAL_RIGHT_FE_CHR_BANK_ADDRESS,
            &[
                Instruction::JsrAbsolute(SELECT_RIGHT_FE_CHR_BANK_ADDRESS),
                Instruction::JsrAbsolute(SELECT_RIGHT_FD_CHR_BANK_FOR_PAIR_ADDRESS),
                Instruction::Rts,
            ],
        )
        .unwrap();
        assert_eq!(wrapper.bytes, expected);
    }

    fn map_source_chr_page(source_page: u8) -> u8 {
        ((source_page & 0x1F) << 2) + 8
    }
}
