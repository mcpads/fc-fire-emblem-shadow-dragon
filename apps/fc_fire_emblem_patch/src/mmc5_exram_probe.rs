use std::{fs, path::Path};

use anyhow::{Context, Result, ensure};
use serde::Serialize;

use crate::{
    mmc5_chr::create_mmc5_chr_writer_probe_image,
    mmc5_prg::{count_direct_transfers_to_range, fixed_bank_file_offset},
    rom::{EXPECTED_CHR_SHA1, EXPECTED_SOURCE_SHA1, PRG_SIZE, Rom},
    rp2a03::{Instruction, assemble_at},
    sha1_hex,
    tracked::TrackedImage,
};

const SOURCE_RIGHT_FD_WRITER_ADDRESS: u16 = 0xC9BE;
const SOURCE_RIGHT_FE_WRITER_ADDRESS: u16 = 0xC9C6;
const RIGHT_FD_SHADOW_ADDRESS: u8 = 0x5B;
const RIGHT_FE_SHADOW_ADDRESS: u8 = 0x5C;
const MMC5_RIGHT_CHR_REGISTER: u16 = 0x5127;

const UPDATE_EXRAM_FOR_DIALOGUE_PAIR_ADDRESS: u16 = 0xFA90;
const UPDATE_EXRAM_CLEANUP_ADDRESS: u16 = 0xFAA4;
const COPY_EXRAM_ATTRIBUTES_ADDRESS: u16 = 0xFAC0;
const COPY_EXRAM_LOOP_ADDRESS: u16 = 0xFAC9;
const EXRAM_ATTRIBUTE_DATA_ADDRESS: u16 = 0xFB00;

const EXRAM_MODE_REGISTER: u16 = 0x5104;
const EXRAM_CPU_ADDRESS: u16 = 0x5C00;
const EXRAM_ATTRIBUTE_LEN: usize = 0x400;
const VISIBLE_TILE_ATTRIBUTE_LEN: usize = 32 * 30;
const DIALOGUE_RIGHT_FD_BANK: u8 = 0x00;
const DIALOGUE_RIGHT_FE_BANK: u8 = 0x18;

#[derive(Debug, Serialize)]
struct DialogueExramProbeReport {
    schema: u32,
    source_sha1: &'static str,
    base_chr_writer_probe_sha1: String,
    attributes_sha1: String,
    output_sha1: String,
    output_mapper: u16,
    prg_size: usize,
    chr_size: usize,
    chr_sha1: &'static str,
    trigger: TriggerReport,
    exram_copy: ExramCopyReport,
    direct_source_code_transfer_count: usize,
    direct_source_data_transfer_candidate_count: usize,
    tracked_delta_writes: Vec<TrackedWrite>,
    unresolved_boundaries: Vec<&'static str>,
    release_eligible: bool,
}

#[derive(Debug, Serialize)]
struct TriggerReport {
    fd_hooked_cpu_address: String,
    fd_shadow_address: String,
    fd_bank: String,
    fe_hooked_cpu_address: String,
    fe_shadow_address: String,
    fe_bank: String,
    repeated_match_behavior: &'static str,
}

#[derive(Debug, Serialize)]
struct ExramCopyReport {
    hook_cpu_address: String,
    copy_cpu_address: String,
    data_cpu_start: String,
    data_len: usize,
    cpu_destination: String,
    write_mode: u8,
    display_mode: u8,
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

pub fn build_mmc5_dialogue_exram_probe(
    source_path: &Path,
    attributes_path: &Path,
    output_path: &Path,
    report_path: &Path,
) -> Result<BuildSummary> {
    let source_rom = Rom::from_path(source_path)?;
    source_rom.verify_supported_japanese()?;
    let attributes = fs::read(attributes_path)
        .with_context(|| format!("read ExRAM attributes {}", attributes_path.display()))?;
    validate_dialogue_attributes(&attributes)?;

    let direct_source_code_transfer_count = count_direct_transfers_to_range(
        source_rom.prg(),
        UPDATE_EXRAM_FOR_DIALOGUE_PAIR_ADDRESS,
        UPDATE_EXRAM_FOR_DIALOGUE_PAIR_ADDRESS + dialogue_pair_trigger()?.len() as u16,
    )? + count_direct_transfers_to_range(
        source_rom.prg(),
        COPY_EXRAM_ATTRIBUTES_ADDRESS,
        COPY_EXRAM_ATTRIBUTES_ADDRESS + copy_exram_attributes()?.len() as u16,
    )?;
    ensure!(
        direct_source_code_transfer_count == 0,
        "source has {direct_source_code_transfer_count} direct JSR or JMP references into the ExRAM probe routines"
    );
    let direct_source_data_transfer_candidate_count = count_direct_transfers_to_range(
        source_rom.prg(),
        EXRAM_ATTRIBUTE_DATA_ADDRESS,
        EXRAM_ATTRIBUTE_DATA_ADDRESS + EXRAM_ATTRIBUTE_LEN as u16,
    )?;

    let chr_writer_probe = create_mmc5_chr_writer_probe_image(&source_rom)?;
    let base = chr_writer_probe.data().to_vec();
    let base_chr_writer_probe_sha1 = sha1_hex(&base);
    let mut image = TrackedImage::new(base.clone());

    install_right_writer_hook(
        &mut image,
        "FD",
        SOURCE_RIGHT_FD_WRITER_ADDRESS,
        RIGHT_FD_SHADOW_ADDRESS,
    )?;
    install_right_writer_hook(
        &mut image,
        "FE",
        SOURCE_RIGHT_FE_WRITER_ADDRESS,
        RIGHT_FE_SHADOW_ADDRESS,
    )?;
    install_routine(
        &mut image,
        "dialogue FD/FE pair trigger",
        UPDATE_EXRAM_FOR_DIALOGUE_PAIR_ADDRESS,
        &dialogue_pair_trigger()?,
    )?;
    install_routine(
        &mut image,
        "1 KiB ExRAM attribute copy",
        COPY_EXRAM_ATTRIBUTES_ADDRESS,
        &copy_exram_attributes()?,
    )?;
    image.write_expected(
        "embedded dialogue ExRAM attributes",
        fixed_bank_file_offset(EXRAM_ATTRIBUTE_DATA_ADDRESS)?,
        &vec![0xFF; EXRAM_ATTRIBUTE_LEN],
        &attributes,
    )?;

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
    let output = image.into_data();
    let output_rom = Rom::parse(output.clone()).context("parse MMC5 dialogue ExRAM probe")?;
    ensure!(
        output_rom.mapper() == 5,
        "ExRAM probe output mapper is not 5"
    );
    ensure!(
        output_rom.prg().len() == PRG_SIZE,
        "ExRAM probe changed PRG size"
    );
    ensure!(
        sha1_hex(output_rom.chr()) == EXPECTED_CHR_SHA1,
        "ExRAM probe changed source CHR"
    );

    let output_sha1 = sha1_hex(&output);
    let report = DialogueExramProbeReport {
        schema: 1,
        source_sha1: EXPECTED_SOURCE_SHA1,
        base_chr_writer_probe_sha1,
        attributes_sha1: sha1_hex(&attributes),
        output_sha1: output_sha1.clone(),
        output_mapper: output_rom.mapper(),
        prg_size: output_rom.prg().len(),
        chr_size: output_rom.chr().len(),
        chr_sha1: EXPECTED_CHR_SHA1,
        trigger: TriggerReport {
            fd_hooked_cpu_address: format!("0x{SOURCE_RIGHT_FD_WRITER_ADDRESS:04X}"),
            fd_shadow_address: format!("0x{RIGHT_FD_SHADOW_ADDRESS:02X}"),
            fd_bank: format!("0x{DIALOGUE_RIGHT_FD_BANK:02X}"),
            fe_hooked_cpu_address: format!("0x{SOURCE_RIGHT_FE_WRITER_ADDRESS:04X}"),
            fe_shadow_address: format!("0x{RIGHT_FE_SHADOW_ADDRESS:02X}"),
            fe_bank: format!("0x{DIALOGUE_RIGHT_FE_BANK:02X}"),
            repeated_match_behavior: "reload the complete 1 KiB projection on every matching FE writer call",
        },
        exram_copy: ExramCopyReport {
            hook_cpu_address: format!("0x{UPDATE_EXRAM_FOR_DIALOGUE_PAIR_ADDRESS:04X}"),
            copy_cpu_address: format!("0x{COPY_EXRAM_ATTRIBUTES_ADDRESS:04X}"),
            data_cpu_start: format!("0x{EXRAM_ATTRIBUTE_DATA_ADDRESS:04X}"),
            data_len: EXRAM_ATTRIBUTE_LEN,
            cpu_destination: format!("0x{EXRAM_CPU_ADDRESS:04X}"),
            write_mode: 2,
            display_mode: 1,
        },
        direct_source_code_transfer_count,
        direct_source_data_transfer_candidate_count,
        tracked_delta_writes,
        unresolved_boundaries: vec![
            "The embedded projection is proven only for the observed zero-scroll chapter 1 dialogue screen.",
            "The two right-window writer hooks reload a static projection after the FD/FE pair becomes $00/$18; they do not mirror later nametable writes.",
            "Direct JSR/JMP byte-pattern candidates into the former all-$FF data cave are reported but remain instruction-boundary unclassified.",
            "Fine scroll, PPU prefetch, cross-nametable ownership, other latch pairs, sprites, and repeated-call timing remain unverified.",
            "No progression, save/load, adverse-path, or release equivalence is claimed by this probe.",
        ],
        release_eligible: false,
    };
    let report_bytes =
        serde_json::to_vec_pretty(&report).context("serialize MMC5 dialogue ExRAM report")?;
    let report_sha1 = sha1_hex(&report_bytes);
    let tracked_write_count = report.tracked_delta_writes.len();

    write_file(output_path, &output)?;
    write_file(report_path, &report_bytes)?;
    Ok(BuildSummary {
        output_sha1,
        report_sha1,
        tracked_write_count,
    })
}

fn validate_dialogue_attributes(attributes: &[u8]) -> Result<()> {
    ensure!(
        attributes.len() == EXRAM_ATTRIBUTE_LEN,
        "dialogue ExRAM attributes must be exactly 1024 bytes"
    );
    let visible = &attributes[..VISIBLE_TILE_ATTRIBUTE_LEN];
    ensure!(
        visible
            .iter()
            .all(|byte| matches!(byte & 0x3F, DIALOGUE_RIGHT_FD_BANK | DIALOGUE_RIGHT_FE_BANK)),
        "dialogue ExRAM attributes contain a CHR bank outside $00/$18"
    );
    ensure!(
        visible
            .iter()
            .any(|byte| byte & 0x3F == DIALOGUE_RIGHT_FD_BANK),
        "dialogue ExRAM attributes do not use FD bank $00"
    );
    ensure!(
        visible
            .iter()
            .any(|byte| byte & 0x3F == DIALOGUE_RIGHT_FE_BANK),
        "dialogue ExRAM attributes do not use FE bank $18"
    );
    ensure!(
        attributes[VISIBLE_TILE_ATTRIBUTE_LEN..]
            .iter()
            .all(|byte| *byte == DIALOGUE_RIGHT_FE_BANK),
        "dialogue ExRAM attribute tail must use the initial FE bank $18"
    );
    Ok(())
}

fn install_right_writer_hook(
    image: &mut TrackedImage,
    latch: &str,
    source_address: u16,
    shadow_address: u8,
) -> Result<()> {
    let expected = assemble_at(
        source_address,
        &[
            Instruction::StaZeroPage(shadow_address),
            Instruction::OraZeroPage(0x52),
            Instruction::StaAbsolute(MMC5_RIGHT_CHR_REGISTER),
            Instruction::Rts,
        ],
    )?;
    let replacement = assemble_at(
        source_address,
        &[
            Instruction::StaZeroPage(shadow_address),
            Instruction::JsrAbsolute(UPDATE_EXRAM_FOR_DIALOGUE_PAIR_ADDRESS),
            Instruction::Rts,
            Instruction::Nop,
            Instruction::Nop,
        ],
    )?;
    ensure!(
        replacement.len() == expected.len(),
        "right {latch} writer hook changed the source routine length"
    );
    image.write_expected(
        format!("hook right {latch} writer for dialogue ExRAM projection"),
        fixed_bank_file_offset(source_address)?,
        &expected,
        &replacement,
    )
}

fn dialogue_pair_trigger() -> Result<Vec<u8>> {
    assemble_at(
        UPDATE_EXRAM_FOR_DIALOGUE_PAIR_ADDRESS,
        &[
            Instruction::OraZeroPage(0x52),
            Instruction::StaAbsolute(MMC5_RIGHT_CHR_REGISTER),
            Instruction::Php,
            Instruction::Pha,
            Instruction::LdaZeroPage(RIGHT_FD_SHADOW_ADDRESS),
            Instruction::BneAbsolute(UPDATE_EXRAM_CLEANUP_ADDRESS),
            Instruction::LdaZeroPage(RIGHT_FE_SHADOW_ADDRESS),
            Instruction::CmpImmediate(DIALOGUE_RIGHT_FE_BANK),
            Instruction::BneAbsolute(UPDATE_EXRAM_CLEANUP_ADDRESS),
            Instruction::JsrAbsolute(COPY_EXRAM_ATTRIBUTES_ADDRESS),
            Instruction::Pla,
            Instruction::Plp,
            Instruction::Rts,
        ],
    )
}

fn copy_exram_attributes() -> Result<Vec<u8>> {
    assemble_at(
        COPY_EXRAM_ATTRIBUTES_ADDRESS,
        &[
            Instruction::Txa,
            Instruction::Pha,
            Instruction::LdaImmediate(2),
            Instruction::StaAbsolute(EXRAM_MODE_REGISTER),
            Instruction::LdxImmediate(0),
            Instruction::LdaAbsoluteX(EXRAM_ATTRIBUTE_DATA_ADDRESS),
            Instruction::StaAbsoluteX(EXRAM_CPU_ADDRESS),
            Instruction::LdaAbsoluteX(EXRAM_ATTRIBUTE_DATA_ADDRESS + 0x100),
            Instruction::StaAbsoluteX(EXRAM_CPU_ADDRESS + 0x100),
            Instruction::LdaAbsoluteX(EXRAM_ATTRIBUTE_DATA_ADDRESS + 0x200),
            Instruction::StaAbsoluteX(EXRAM_CPU_ADDRESS + 0x200),
            Instruction::LdaAbsoluteX(EXRAM_ATTRIBUTE_DATA_ADDRESS + 0x300),
            Instruction::StaAbsoluteX(EXRAM_CPU_ADDRESS + 0x300),
            Instruction::Inx,
            Instruction::BneAbsolute(COPY_EXRAM_LOOP_ADDRESS),
            Instruction::LdaImmediate(1),
            Instruction::StaAbsolute(EXRAM_MODE_REGISTER),
            Instruction::Pla,
            Instruction::Tax,
            Instruction::Rts,
        ],
    )
}

fn install_routine(
    image: &mut TrackedImage,
    role: &str,
    cpu_address: u16,
    bytes: &[u8],
) -> Result<()> {
    image.write_expected(
        format!("MMC5 {role} routine"),
        fixed_bank_file_offset(cpu_address)?,
        &vec![0xFF; bytes.len()],
        bytes,
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
    fn dialogue_attribute_input_is_exact_and_pair_bound() {
        let mut attributes = vec![DIALOGUE_RIGHT_FE_BANK; EXRAM_ATTRIBUTE_LEN];
        attributes[0] = DIALOGUE_RIGHT_FD_BANK;
        validate_dialogue_attributes(&attributes).unwrap();

        assert!(validate_dialogue_attributes(&attributes[..1000]).is_err());
        attributes[1] = 0x07;
        assert!(validate_dialogue_attributes(&attributes).is_err());
    }

    #[test]
    fn right_window_writer_hooks_preserve_the_original_routine_size() {
        for (source_address, shadow_address) in [
            (SOURCE_RIGHT_FD_WRITER_ADDRESS, RIGHT_FD_SHADOW_ADDRESS),
            (SOURCE_RIGHT_FE_WRITER_ADDRESS, RIGHT_FE_SHADOW_ADDRESS),
        ] {
            let source = assemble_at(
                source_address,
                &[
                    Instruction::StaZeroPage(shadow_address),
                    Instruction::OraZeroPage(0x52),
                    Instruction::StaAbsolute(MMC5_RIGHT_CHR_REGISTER),
                    Instruction::Rts,
                ],
            )
            .unwrap();
            let replacement = assemble_at(
                source_address,
                &[
                    Instruction::StaZeroPage(shadow_address),
                    Instruction::JsrAbsolute(UPDATE_EXRAM_FOR_DIALOGUE_PAIR_ADDRESS),
                    Instruction::Rts,
                    Instruction::Nop,
                    Instruction::Nop,
                ],
            )
            .unwrap();

            assert_eq!(source.len(), 8);
            assert_eq!(replacement.len(), source.len());
        }
    }

    #[test]
    fn trigger_and_copy_routines_fit_before_the_embedded_attributes() {
        let trigger = dialogue_pair_trigger().unwrap();
        let copy = copy_exram_attributes().unwrap();

        assert_eq!(
            UPDATE_EXRAM_FOR_DIALOGUE_PAIR_ADDRESS + trigger.len() as u16,
            0xFAA7
        );
        assert_eq!(COPY_EXRAM_ATTRIBUTES_ADDRESS + copy.len() as u16, 0xFAEC);
        assert!(COPY_EXRAM_ATTRIBUTES_ADDRESS + copy.len() as u16 <= EXRAM_ATTRIBUTE_DATA_ADDRESS);
        assert_eq!(
            EXRAM_ATTRIBUTE_DATA_ADDRESS + EXRAM_ATTRIBUTE_LEN as u16,
            0xFF00
        );
    }

    #[test]
    fn copy_routine_uses_mode_two_for_writes_and_mode_one_for_display() {
        let copy = copy_exram_attributes().unwrap();

        assert_eq!(&copy[..7], &[0x8A, 0x48, 0xA9, 0x02, 0x8D, 0x04, 0x51]);
        assert_eq!(
            &copy[copy.len() - 8..],
            &[0xA9, 0x01, 0x8D, 0x04, 0x51, 0x68, 0xAA, 0x60]
        );
    }
}
