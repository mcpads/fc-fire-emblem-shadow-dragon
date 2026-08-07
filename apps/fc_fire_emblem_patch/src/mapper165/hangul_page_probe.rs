use std::{fs, path::Path};

use anyhow::{Context, Result, ensure};
use serde::Serialize;

use crate::{
    hangul_page_plan::assemble_hangul_page_pack,
    localization::OptionsLocalization,
    mmc5_chr::switchable_bank_file_offset,
    mmc5_prg::{count_direct_transfers_to_range, fixed_bank_file_offset},
    options::{OPTIONS_TABLE_OFFSET, SOURCE_OPTIONS_TABLE},
    rom::{EXPECTED_SOURCE_SHA1, PRG_SIZE, Rom},
    rp2a03::{Instruction, assemble_at},
    sha1_hex,
    tracked::TrackedImage,
};

use super::{
    FIRST_EXTENSION_CHR_PAGE, SELECT_RIGHT_FD_CHR_BANK_FOR_PAIR_ADDRESS,
    assemble_mapper165_parity_bytes, encode_chr_page_register,
};

const CHR_PAGE_SIZE: usize = 0x1000;
const PAGE_COUNT: usize = 2;
const OUTPUT_CHR_BANK_COUNT: u8 = 18;
const OPTIONS_ROW_PRG_BANK: u8 = 0x0B;
const OPTIONS_ROW_HOOK_ADDRESS: u16 = 0x93B7;
const OPTIONS_ROW_HOOK_LEN: usize = 11;
const OPTIONS_PAGE_ROUTINE_ADDRESS: u16 = 0xFB20;
const OPTIONS_PAGE_ROUTINE_END: u16 = 0xFB68;
const OPTIONS_PAGE_A_REGISTER: u8 = 0x88;
const OPTIONS_PAGE_B_REGISTER: u8 = 0x8C;

#[derive(Debug, Serialize)]
struct HangulPageProbeReport {
    schema: u32,
    source_sha1: &'static str,
    parity_base_sha1: String,
    output_sha1: String,
    output_mapper: u16,
    prg_size: usize,
    chr_size: usize,
    page_pack_sha1: String,
    pages: Vec<PageBindingReport>,
    screen_contract: ScreenContractReport,
    hook: HookReport,
    direct_code_cave_transfer_count: usize,
    tracked_writes: Vec<TrackedWriteReport>,
    runtime_hook_installed: bool,
    runtime_verified: bool,
    unresolved_boundaries: Vec<&'static str>,
    release_eligible: bool,
}

#[derive(Debug, Serialize)]
struct PageBindingReport {
    role: &'static str,
    physical_4k_page: u8,
    mapper_register_value: u8,
    page_sha1: String,
}

#[derive(Debug, Serialize)]
struct ScreenContractReport {
    left_fd: u8,
    left_fe: u8,
    right_fd: u8,
    right_fe: u8,
    row_state_address: String,
    page_a_rows: Vec<u8>,
    page_b_rows: Vec<u8>,
    fallback: &'static str,
}

#[derive(Debug, Serialize)]
struct HookReport {
    source_prg_bank: u8,
    source_cpu_address: String,
    source_len: usize,
    routine_cpu_start: String,
    routine_cpu_end_exclusive: String,
    routine_len: usize,
    preserves_original_row_calculation: bool,
    preserves_y_and_status_result: bool,
}

#[derive(Debug, Serialize)]
struct TrackedWriteReport {
    label: String,
    file_offset: String,
    len: usize,
}

pub(crate) struct HangulPageProbeSummary {
    pub(crate) output_sha1: String,
    pub(crate) report_sha1: String,
    pub(crate) page_pack_sha1: String,
    pub(crate) tracked_write_count: usize,
}

pub(crate) fn build_mapper165_hangul_page_probe(
    source_path: &Path,
    localization_path: &Path,
    output_path: &Path,
    report_path: &Path,
) -> Result<HangulPageProbeSummary> {
    let source_rom = Rom::from_path(source_path)?;
    source_rom.verify_supported_japanese()?;
    let localization = OptionsLocalization::from_path(localization_path)?;
    let validated_localization = localization.validate()?;
    let page_pack = assemble_hangul_page_pack(&source_rom, &localization)?;
    ensure!(
        page_pack.len() == PAGE_COUNT * CHR_PAGE_SIZE,
        "Hangul page probe needs exactly two 4 KiB pages"
    );
    let page_a_register = encode_chr_page_register(FIRST_EXTENSION_CHR_PAGE)?;
    let page_b_register = encode_chr_page_register(FIRST_EXTENSION_CHR_PAGE + 1)?;
    ensure!(
        page_a_register == OPTIONS_PAGE_A_REGISTER && page_b_register == OPTIONS_PAGE_B_REGISTER,
        "Hangul page register contract changed"
    );

    let parity_base = assemble_mapper165_parity_bytes(&source_rom)?;
    let parity_base_sha1 = sha1_hex(&parity_base);
    let parity_rom = Rom::parse(parity_base.clone()).context("parse mapper 165 parity base")?;
    ensure!(
        parity_rom.chr().len() == 17 * 8 * 1024,
        "mapper 165 parity base CHR size changed"
    );

    let routine = build_options_page_routine(page_a_register, page_b_register)?;
    ensure!(
        OPTIONS_PAGE_ROUTINE_ADDRESS as usize + routine.len() == OPTIONS_PAGE_ROUTINE_END as usize,
        "options page routine size changed"
    );
    let cave_start = fixed_bank_file_offset(OPTIONS_PAGE_ROUTINE_ADDRESS)?;
    let cave_end = cave_start + routine.len();
    ensure!(
        parity_base[cave_start..cave_end]
            .iter()
            .all(|byte| *byte == 0xFF),
        "options page routine cave is no longer all FF"
    );
    let direct_code_cave_transfer_count = count_direct_transfers_to_range(
        source_rom.prg(),
        OPTIONS_PAGE_ROUTINE_ADDRESS,
        OPTIONS_PAGE_ROUTINE_END,
    )?;
    ensure!(
        direct_code_cave_transfer_count == 0,
        "options page routine cave has {direct_code_cave_transfer_count} pre-existing direct transfers"
    );

    let mut expanded_base = parity_base.clone();
    expanded_base.extend_from_slice(&page_pack);
    let mut image = TrackedImage::new(expanded_base.clone());
    image.write_expected(
        "expand mapper 165 CHR from 17 to 18 banks",
        5,
        &[17],
        &[OUTPUT_CHR_BANK_COUNT],
    )?;
    image.write_expected(
        "Korean options text table",
        OPTIONS_TABLE_OFFSET,
        &SOURCE_OPTIONS_TABLE,
        &validated_localization.replacement_table,
    )?;
    image.write_expected(
        "options screen Hangul page routine",
        cave_start,
        &vec![0xFF; routine.len()],
        &routine,
    )?;

    let hook_offset = switchable_bank_file_offset(OPTIONS_ROW_PRG_BANK, OPTIONS_ROW_HOOK_ADDRESS)?;
    let expected_hook = options_row_calculation()?;
    ensure!(
        expected_hook.len() == OPTIONS_ROW_HOOK_LEN,
        "options row calculation length changed"
    );
    let replacement_hook = options_row_hook()?;
    ensure!(
        replacement_hook.len() == OPTIONS_ROW_HOOK_LEN,
        "options row hook length changed"
    );
    image.write_expected(
        "options row calculation to Hangul page selector",
        hook_offset,
        &expected_hook,
        &replacement_hook,
    )?;
    image.verify_all_changes_tracked(&expanded_base)?;

    let tracked_writes = image
        .writes()
        .iter()
        .map(|write| TrackedWriteReport {
            label: write.label.clone(),
            file_offset: format!("0x{:06X}", write.offset),
            len: write.len,
        })
        .collect::<Vec<_>>();
    let output = image.into_data();
    let output_rom = Rom::parse(output.clone()).context("parse mapper 165 Hangul page probe")?;
    verify_output(
        &parity_rom,
        &output_rom,
        &page_pack,
        &validated_localization.replacement_table,
    )?;

    let output_sha1 = sha1_hex(&output);
    let page_pack_sha1 = sha1_hex(&page_pack);
    let report = HangulPageProbeReport {
        schema: 1,
        source_sha1: EXPECTED_SOURCE_SHA1,
        parity_base_sha1,
        output_sha1: output_sha1.clone(),
        output_mapper: output_rom.mapper(),
        prg_size: output_rom.prg().len(),
        chr_size: output_rom.chr().len(),
        page_pack_sha1: page_pack_sha1.clone(),
        pages: [page_a_register, page_b_register]
            .into_iter()
            .enumerate()
            .map(|(index, mapper_register_value)| {
                let start = index * CHR_PAGE_SIZE;
                PageBindingReport {
                    role: if index == 0 { "page_a" } else { "page_b" },
                    physical_4k_page: FIRST_EXTENSION_CHR_PAGE + index as u8,
                    mapper_register_value,
                    page_sha1: sha1_hex(&page_pack[start..start + CHR_PAGE_SIZE]),
                }
            })
            .collect(),
        screen_contract: ScreenContractReport {
            left_fd: 0x1A,
            left_fe: 0x1A,
            right_fd: 0x00,
            right_fe: 0x15,
            row_state_address: "0x0034".to_owned(),
            page_a_rows: vec![0x20, 0x40],
            page_b_rows: vec![0x30],
            fallback: "call the existing pair-aware right FD selector",
        },
        hook: HookReport {
            source_prg_bank: OPTIONS_ROW_PRG_BANK,
            source_cpu_address: format!("0x{OPTIONS_ROW_HOOK_ADDRESS:04X}"),
            source_len: OPTIONS_ROW_HOOK_LEN,
            routine_cpu_start: format!("0x{OPTIONS_PAGE_ROUTINE_ADDRESS:04X}"),
            routine_cpu_end_exclusive: format!("0x{OPTIONS_PAGE_ROUTINE_END:04X}"),
            routine_len: routine.len(),
            preserves_original_row_calculation: true,
            preserves_y_and_status_result: true,
        },
        direct_code_cave_transfer_count,
        tracked_writes,
        runtime_hook_installed: true,
        runtime_verified: false,
        unresolved_boundaries: vec![
            "The installed A-B-A selector still needs cold runtime proof on the options screen.",
            "The options screen has no existing English; English and Hangul coexistence remains a separate screen contract.",
            "Leaving the options screen must visibly restore the natural FD page before this probe can pass G3.",
            "The two pages prove capacity and switching, not final corpus packing or whole-game coverage.",
        ],
        release_eligible: false,
    };
    let report_bytes = serde_json::to_vec_pretty(&report)
        .context("serialize mapper 165 Hangul page probe report")?;
    let report_sha1 = sha1_hex(&report_bytes);
    let tracked_write_count = report.tracked_writes.len();

    write_file(output_path, &output)?;
    write_file(report_path, &report_bytes)?;
    Ok(HangulPageProbeSummary {
        output_sha1,
        report_sha1,
        page_pack_sha1,
        tracked_write_count,
    })
}

fn options_row_calculation() -> Result<Vec<u8>> {
    assemble_at(
        OPTIONS_ROW_HOOK_ADDRESS,
        &[
            Instruction::LdyImmediate(4),
            Instruction::LdaIndirectY(0x6E),
            Instruction::Clc,
            Instruction::AdcAbsoluteX(0x93D8),
            Instruction::StaZeroPage(0x34),
            Instruction::Iny,
        ],
    )
}

fn options_row_hook() -> Result<Vec<u8>> {
    let mut instructions = vec![Instruction::JsrAbsolute(OPTIONS_PAGE_ROUTINE_ADDRESS)];
    instructions.extend(std::iter::repeat_n(
        Instruction::Nop,
        OPTIONS_ROW_HOOK_LEN - 3,
    ));
    assemble_at(OPTIONS_ROW_HOOK_ADDRESS, &instructions)
}

fn build_options_page_routine(page_a_register: u8, page_b_register: u8) -> Result<Vec<u8>> {
    const PAGE_B_ADDRESS: u16 = 0xFB55;
    const WRITE_MAPPER_ADDRESS: u16 = 0xFB57;
    const FALLBACK_ADDRESS: u16 = 0xFB63;

    assemble_at(
        OPTIONS_PAGE_ROUTINE_ADDRESS,
        &[
            Instruction::LdyImmediate(4),
            Instruction::LdaIndirectY(0x6E),
            Instruction::Clc,
            Instruction::AdcAbsoluteX(0x93D8),
            Instruction::StaZeroPage(0x34),
            Instruction::Iny,
            Instruction::Php,
            Instruction::LdaZeroPage(0x52),
            Instruction::CmpImmediate(0x00),
            Instruction::BneAbsolute(FALLBACK_ADDRESS),
            Instruction::LdaZeroPage(0x59),
            Instruction::CmpImmediate(0x1A),
            Instruction::BneAbsolute(FALLBACK_ADDRESS),
            Instruction::LdaZeroPage(0x5A),
            Instruction::CmpImmediate(0x1A),
            Instruction::BneAbsolute(FALLBACK_ADDRESS),
            Instruction::LdaZeroPage(0x5B),
            Instruction::CmpImmediate(0x00),
            Instruction::BneAbsolute(FALLBACK_ADDRESS),
            Instruction::LdaZeroPage(0x5C),
            Instruction::CmpImmediate(0x15),
            Instruction::BneAbsolute(FALLBACK_ADDRESS),
            Instruction::LdaZeroPage(0x34),
            Instruction::CmpImmediate(0x30),
            Instruction::BeqAbsolute(PAGE_B_ADDRESS),
            Instruction::LdaImmediate(page_a_register),
            Instruction::JmpAbsolute(WRITE_MAPPER_ADDRESS),
            Instruction::LdaImmediate(page_b_register),
            Instruction::Pha,
            Instruction::LdaImmediate(2),
            Instruction::StaAbsolute(0x8000),
            Instruction::Pla,
            Instruction::StaAbsolute(0x8001),
            Instruction::Plp,
            Instruction::Rts,
            Instruction::JsrAbsolute(SELECT_RIGHT_FD_CHR_BANK_FOR_PAIR_ADDRESS),
            Instruction::Plp,
            Instruction::Rts,
        ],
    )
}

fn verify_output(
    parity_rom: &Rom,
    output_rom: &Rom,
    page_pack: &[u8],
    replacement_table: &[u8],
) -> Result<()> {
    ensure!(
        output_rom.mapper() == 165,
        "Hangul page output mapper changed"
    );
    ensure!(
        output_rom.prg().len() == PRG_SIZE,
        "Hangul page output PRG size changed"
    );
    ensure!(
        output_rom.chr().len() == parity_rom.chr().len() + page_pack.len(),
        "Hangul page output CHR size is incorrect"
    );
    ensure!(
        output_rom.chr()[..parity_rom.chr().len()] == *parity_rom.chr(),
        "Hangul page probe changed the mapper 165 parity CHR base"
    );
    ensure!(
        output_rom.chr()[parity_rom.chr().len()..] == *page_pack,
        "Hangul page probe appended different page bytes"
    );
    ensure!(
        output_rom.data()[OPTIONS_TABLE_OFFSET..OPTIONS_TABLE_OFFSET + replacement_table.len()]
            == *replacement_table,
        "Hangul page probe options table changed"
    );
    ensure!(
        output_rom.data()[6] & 0x02 == parity_rom.data()[6] & 0x02,
        "Hangul page probe changed the battery flag"
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
    fn row_hook_preserves_the_original_span_and_calls_the_fixed_routine() {
        let original = options_row_calculation().unwrap();
        let hook = options_row_hook().unwrap();

        assert_eq!(
            original,
            [
                0xA0, 0x04, 0xB1, 0x6E, 0x18, 0x7D, 0xD8, 0x93, 0x85, 0x34, 0xC8
            ]
        );
        assert_eq!(hook.len(), original.len());
        assert_eq!(&hook[..3], &[0x20, 0x20, 0xFB]);
        assert!(hook[3..].iter().all(|byte| *byte == 0xEA));
    }

    #[test]
    fn options_page_routine_fits_its_proven_cave_and_has_a_pair_aware_fallback() {
        let routine =
            build_options_page_routine(OPTIONS_PAGE_A_REGISTER, OPTIONS_PAGE_B_REGISTER).unwrap();

        assert_eq!(routine.len(), 0x48);
        assert_eq!(
            OPTIONS_PAGE_ROUTINE_ADDRESS as usize + routine.len(),
            OPTIONS_PAGE_ROUTINE_END as usize
        );
        assert!(routine.windows(2).any(|bytes| bytes == [0xA9, 0x88]));
        assert!(routine.windows(2).any(|bytes| bytes == [0xA9, 0x8C]));
        assert!(routine.windows(3).any(|bytes| bytes == [0x20, 0xC0, 0xFA]));
        assert_eq!(&routine[..11], options_row_calculation().unwrap());
    }
}
