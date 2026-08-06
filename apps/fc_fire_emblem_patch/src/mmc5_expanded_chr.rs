use std::{fs, path::Path};

use anyhow::{Context, Result, ensure};
use serde::Serialize;

use crate::{
    localization::OptionsLocalization,
    mmc5_chr::{create_mmc5_chr_writer_probe_image, switchable_bank_file_offset},
    mmc5_prg::fixed_bank_file_offset,
    options::{OPTIONS_TABLE_OFFSET, SOURCE_OPTIONS_TABLE},
    rom::{CHR_FILE_OFFSET, CHR_SIZE, EXPECTED_CHR_SHA1, EXPECTED_SOURCE_SHA1, PRG_SIZE, Rom},
    rp2a03::{Instruction, assemble_at},
    sha1_hex,
    tracked::TrackedImage,
};

const EXPANDED_CHR_SIZE: usize = CHR_SIZE * 2;
const EXPANDED_CHR_BANK_OFFSET: u8 = 0x20;
const PPU_0000_UPPER_CHR_WRAPPER_ADDRESS: u16 = 0xFA68;
const PPU_1000_UPPER_CHR_WRAPPER_ADDRESS: u16 = 0xFA74;

const CHR_STORE_REDIRECTS: &[ChrStoreRedirect] = &[
    ChrStoreRedirect::fixed(
        "central PPU $0000 FD store",
        0xC9B2,
        0x5123,
        PPU_0000_UPPER_CHR_WRAPPER_ADDRESS,
    ),
    ChrStoreRedirect::fixed(
        "central PPU $0000 FE store",
        0xC9BA,
        0x5123,
        PPU_0000_UPPER_CHR_WRAPPER_ADDRESS,
    ),
    ChrStoreRedirect::fixed(
        "central PPU $1000 FD store",
        0xC9C2,
        0x5127,
        PPU_1000_UPPER_CHR_WRAPPER_ADDRESS,
    ),
    ChrStoreRedirect::fixed(
        "central PPU $1000 FE store",
        0xC9CA,
        0x5127,
        PPU_1000_UPPER_CHR_WRAPPER_ADDRESS,
    ),
    ChrStoreRedirect::switchable(
        "automatic status PPU $1000 FD store",
        0x0D,
        0x8036,
        0x5127,
        PPU_1000_UPPER_CHR_WRAPPER_ADDRESS,
    ),
    ChrStoreRedirect::switchable(
        "automatic status PPU $1000 FE store",
        0x0D,
        0x8039,
        0x5127,
        PPU_1000_UPPER_CHR_WRAPPER_ADDRESS,
    ),
    ChrStoreRedirect::switchable(
        "automatic status PPU $0000 FD store",
        0x0D,
        0x83AB,
        0x5123,
        PPU_0000_UPPER_CHR_WRAPPER_ADDRESS,
    ),
    ChrStoreRedirect::switchable(
        "automatic status PPU $0000 FE store",
        0x0D,
        0x83AE,
        0x5123,
        PPU_0000_UPPER_CHR_WRAPPER_ADDRESS,
    ),
];

#[derive(Debug, Clone, Copy)]
enum PrgLocation {
    Fixed,
    Switchable { prg_bank: u8 },
}

#[derive(Debug, Clone, Copy)]
struct ChrStoreRedirect {
    role: &'static str,
    location: PrgLocation,
    cpu_address: u16,
    target_register: u16,
    wrapper_address: u16,
}

impl ChrStoreRedirect {
    const fn fixed(
        role: &'static str,
        cpu_address: u16,
        target_register: u16,
        wrapper_address: u16,
    ) -> Self {
        Self {
            role,
            location: PrgLocation::Fixed,
            cpu_address,
            target_register,
            wrapper_address,
        }
    }

    const fn switchable(
        role: &'static str,
        prg_bank: u8,
        cpu_address: u16,
        target_register: u16,
        wrapper_address: u16,
    ) -> Self {
        Self {
            role,
            location: PrgLocation::Switchable { prg_bank },
            cpu_address,
            target_register,
            wrapper_address,
        }
    }

    fn file_offset(self) -> Result<usize> {
        match self.location {
            PrgLocation::Fixed => fixed_bank_file_offset(self.cpu_address),
            PrgLocation::Switchable { prg_bank } => {
                switchable_bank_file_offset(prg_bank, self.cpu_address)
            }
        }
    }
}

#[derive(Debug, Serialize)]
struct ExpandedChrProbeReport {
    schema: u32,
    source_sha1: &'static str,
    base_chr_writer_probe_sha1: String,
    output_sha1: String,
    output_mapper: u16,
    prg_size: usize,
    source_chr_size: usize,
    expanded_chr_size: usize,
    lower_chr_sha1: String,
    upper_chr_sha1: String,
    original_lower_chr_unchanged: bool,
    upper_chr_tail_matches_source: bool,
    chr_bank_offset: String,
    localization_scope: &'static str,
    localized_glyph_codes: Vec<String>,
    wrappers: Vec<WrapperReport>,
    store_redirects: Vec<StoreRedirectReport>,
    tracked_writes: Vec<TrackedWrite>,
    unresolved_boundaries: Vec<&'static str>,
    release_eligible: bool,
}

#[derive(Debug, Serialize)]
struct WrapperReport {
    role: &'static str,
    cpu_address: String,
    target_register: String,
    len: usize,
    preserves_accumulator: bool,
    preserves_status_flags: bool,
}

#[derive(Debug, Serialize)]
struct StoreRedirectReport {
    role: &'static str,
    source_prg_bank: Option<String>,
    cpu_address: String,
    target_register: String,
    wrapper_address: String,
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

pub fn build_mmc5_expanded_chr_options_probe(
    source_path: &Path,
    localization_path: &Path,
    output_path: &Path,
    report_path: &Path,
) -> Result<BuildSummary> {
    let source_rom = Rom::from_path(source_path)?;
    source_rom.verify_supported_japanese()?;
    let localization = OptionsLocalization::from_path(localization_path)?.validate()?;
    let base_probe = create_mmc5_chr_writer_probe_image(&source_rom)?;
    let base_chr_writer_probe_sha1 = sha1_hex(base_probe.data());

    let mut expanded_base = base_probe.data().to_vec();
    expanded_base.extend_from_slice(source_rom.chr());
    ensure!(
        expanded_base.len() == CHR_FILE_OFFSET + EXPANDED_CHR_SIZE,
        "expanded CHR base has an unexpected size"
    );
    let mut image = TrackedImage::new(expanded_base.clone());
    image.write_expected(
        "expand iNES CHR from 128 KiB to 256 KiB",
        5,
        &[0x10],
        &[0x20],
    )?;

    install_upper_chr_wrapper(
        &mut image,
        "PPU $0000 upper CHR bank",
        PPU_0000_UPPER_CHR_WRAPPER_ADDRESS,
        0x5123,
    )?;
    install_upper_chr_wrapper(
        &mut image,
        "PPU $1000 upper CHR bank",
        PPU_1000_UPPER_CHR_WRAPPER_ADDRESS,
        0x5127,
    )?;
    for redirect in CHR_STORE_REDIRECTS {
        redirect_chr_store(&mut image, *redirect)?;
    }

    image.write_expected(
        "Japanese options text table",
        OPTIONS_TABLE_OFFSET,
        &SOURCE_OPTIONS_TABLE,
        &localization.replacement_table,
    )?;
    let upper_chr_file_offset = CHR_FILE_OFFSET + CHR_SIZE;
    for (code, replacement) in &localization.tiles {
        let tile_offset = usize::from(*code) * 16;
        image.write_expected(
            format!("upper CHR Korean options glyph {code:02X}"),
            upper_chr_file_offset + tile_offset,
            &source_rom.chr()[tile_offset..tile_offset + 16],
            replacement,
        )?;
    }

    image.verify_all_changes_tracked(&expanded_base)?;
    let tracked_writes = image
        .writes()
        .iter()
        .map(|write| TrackedWrite {
            label: write.label.clone(),
            file_offset: format!("0x{:06X}", write.offset),
            len: write.len,
        })
        .collect::<Vec<_>>();
    let tracked_write_count = tracked_writes.len();
    let output = image.into_data();
    let output_rom = Rom::parse(output.clone()).context("parse expanded CHR probe")?;
    ensure!(
        output_rom.mapper() == 5,
        "expanded CHR probe mapper is not 5"
    );
    ensure!(
        output_rom.prg().len() == PRG_SIZE,
        "expanded CHR probe changed PRG size"
    );
    ensure!(
        output_rom.chr().len() == EXPANDED_CHR_SIZE,
        "expanded CHR probe has the wrong CHR size"
    );
    let lower_chr = &output_rom.chr()[..CHR_SIZE];
    let upper_chr = &output_rom.chr()[CHR_SIZE..];
    ensure!(
        sha1_hex(lower_chr) == EXPECTED_CHR_SHA1,
        "expanded CHR probe changed original lower CHR"
    );
    ensure!(
        upper_chr[0x1000..] == source_rom.chr()[0x1000..],
        "expanded CHR probe changed the copied CHR tail"
    );

    let output_sha1 = sha1_hex(&output);
    let report = ExpandedChrProbeReport {
        schema: 1,
        source_sha1: EXPECTED_SOURCE_SHA1,
        base_chr_writer_probe_sha1,
        output_sha1: output_sha1.clone(),
        output_mapper: output_rom.mapper(),
        prg_size: output_rom.prg().len(),
        source_chr_size: CHR_SIZE,
        expanded_chr_size: output_rom.chr().len(),
        lower_chr_sha1: sha1_hex(lower_chr),
        upper_chr_sha1: sha1_hex(upper_chr),
        original_lower_chr_unchanged: true,
        upper_chr_tail_matches_source: true,
        chr_bank_offset: format!("0x{EXPANDED_CHR_BANK_OFFSET:02X}"),
        localization_scope: "Japanese options labels only; existing English remains unchanged",
        localized_glyph_codes: localization
            .tiles
            .keys()
            .map(|code| format!("0x{code:02X}"))
            .collect(),
        wrappers: vec![
            WrapperReport {
                role: "PPU $0000 upper CHR bank",
                cpu_address: format!("0x{PPU_0000_UPPER_CHR_WRAPPER_ADDRESS:04X}"),
                target_register: "0x5123".to_owned(),
                len: 10,
                preserves_accumulator: true,
                preserves_status_flags: true,
            },
            WrapperReport {
                role: "PPU $1000 upper CHR bank",
                cpu_address: format!("0x{PPU_1000_UPPER_CHR_WRAPPER_ADDRESS:04X}"),
                target_register: "0x5127".to_owned(),
                len: 10,
                preserves_accumulator: true,
                preserves_status_flags: true,
            },
        ],
        store_redirects: CHR_STORE_REDIRECTS
            .iter()
            .map(|redirect| StoreRedirectReport {
                role: redirect.role,
                source_prg_bank: match redirect.location {
                    PrgLocation::Fixed => None,
                    PrgLocation::Switchable { prg_bank } => Some(format!("0x{prg_bank:02X}")),
                },
                cpu_address: format!("0x{:04X}", redirect.cpu_address),
                target_register: format!("0x{:04X}", redirect.target_register),
                wrapper_address: format!("0x{:04X}", redirect.wrapper_address),
            })
            .collect(),
        tracked_writes,
        unresolved_boundaries: vec![
            "Only runtime-proven CHR writers are redirected to the upper copy; other direct-write candidates remain unclassified.",
            "The options localization still reuses Japanese tile codes and is not the final dynamic Hangul layout.",
            "No full-game graphics, progression, save/load, or adverse-path equivalence is claimed by this probe.",
        ],
        release_eligible: false,
    };
    let report_bytes =
        serde_json::to_vec_pretty(&report).context("serialize expanded CHR probe report")?;
    let report_sha1 = sha1_hex(&report_bytes);

    write_file(output_path, &output)?;
    write_file(report_path, &report_bytes)?;
    Ok(BuildSummary {
        output_sha1,
        report_sha1,
        tracked_write_count,
    })
}

fn install_upper_chr_wrapper(
    image: &mut TrackedImage,
    role: &str,
    cpu_address: u16,
    target_register: u16,
) -> Result<()> {
    let wrapper = upper_chr_wrapper(cpu_address, target_register)?;
    image.write_expected(
        format!("MMC5 {role} wrapper"),
        fixed_bank_file_offset(cpu_address)?,
        &vec![0xFF; wrapper.len()],
        &wrapper,
    )
}

fn upper_chr_wrapper(cpu_address: u16, target_register: u16) -> Result<Vec<u8>> {
    assemble_at(
        cpu_address,
        &[
            Instruction::Php,
            Instruction::Pha,
            Instruction::OraImmediate(EXPANDED_CHR_BANK_OFFSET),
            Instruction::StaAbsolute(target_register),
            Instruction::Pla,
            Instruction::Plp,
            Instruction::Rts,
        ],
    )
}

fn redirect_chr_store(image: &mut TrackedImage, redirect: ChrStoreRedirect) -> Result<()> {
    image.write_expected(
        format!("redirect {} through upper CHR wrapper", redirect.role),
        redirect.file_offset()?,
        &assemble_at(
            redirect.cpu_address,
            &[Instruction::StaAbsolute(redirect.target_register)],
        )?,
        &assemble_at(
            redirect.cpu_address,
            &[Instruction::JsrAbsolute(redirect.wrapper_address)],
        )?,
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
    fn upper_chr_wrappers_preserve_a_and_add_the_copy_offset() {
        assert_eq!(
            upper_chr_wrapper(PPU_0000_UPPER_CHR_WRAPPER_ADDRESS, 0x5123).unwrap(),
            [0x08, 0x48, 0x09, 0x20, 0x8D, 0x23, 0x51, 0x68, 0x28, 0x60]
        );
        assert_eq!(
            upper_chr_wrapper(PPU_1000_UPPER_CHR_WRAPPER_ADDRESS, 0x5127).unwrap(),
            [0x08, 0x48, 0x09, 0x20, 0x8D, 0x27, 0x51, 0x68, 0x28, 0x60]
        );
    }

    #[test]
    fn store_redirects_keep_the_original_three_byte_instruction_size() {
        for redirect in CHR_STORE_REDIRECTS {
            let source = assemble_at(
                redirect.cpu_address,
                &[Instruction::StaAbsolute(redirect.target_register)],
            )
            .unwrap();
            let replacement = assemble_at(
                redirect.cpu_address,
                &[Instruction::JsrAbsolute(redirect.wrapper_address)],
            )
            .unwrap();
            assert_eq!(source.len(), 3);
            assert_eq!(replacement.len(), source.len());
        }
    }

    #[test]
    fn wrapper_placements_are_disjoint_and_inside_the_proven_cave() {
        assert_eq!(
            fixed_bank_file_offset(PPU_0000_UPPER_CHR_WRAPPER_ADDRESS).unwrap(),
            0x3FA78
        );
        assert_eq!(
            fixed_bank_file_offset(PPU_1000_UPPER_CHR_WRAPPER_ADDRESS).unwrap(),
            0x3FA84
        );
        assert_eq!(PPU_1000_UPPER_CHR_WRAPPER_ADDRESS + 10, 0xFA7E);
    }

    #[test]
    fn upper_chr_bank_offset_selects_only_the_copied_128_kib_half() {
        assert_eq!(EXPANDED_CHR_BANK_OFFSET, (CHR_SIZE / 0x1000) as u8);
        for source_bank in 0x00..EXPANDED_CHR_BANK_OFFSET {
            let copied_bank = source_bank | EXPANDED_CHR_BANK_OFFSET;
            assert_eq!(copied_bank, source_bank + EXPANDED_CHR_BANK_OFFSET);
            assert!(copied_bank < (EXPANDED_CHR_SIZE / 0x1000) as u8);
        }
    }
}
