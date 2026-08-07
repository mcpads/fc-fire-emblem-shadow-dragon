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
    roster_localization::{
        ROSTER_HEADER_CPU_ADDRESS, ROSTER_TEXT_PRG_BANK, RosterLocalization, SOURCE_ROSTER_HEADER,
        build_roster_font_page,
    },
    sha1_hex,
    tracked::TrackedImage,
};

use super::{
    FIRST_EXTENSION_CHR_PAGE, SELECT_RIGHT_FD_CHR_BANK_FOR_PAIR_ADDRESS,
    assemble_mapper165_parity_bytes, encode_chr_page_register,
    options_page::{
        PAGE_A_REGISTER as OPTIONS_PAGE_A_REGISTER, PAGE_B_REGISTER as OPTIONS_PAGE_B_REGISTER,
        PAGE_ROUTINE_ADDRESS as OPTIONS_PAGE_ROUTINE_ADDRESS,
        PAGE_ROUTINE_END as OPTIONS_PAGE_ROUTINE_END, ROW_HOOK_ADDRESS as OPTIONS_ROW_HOOK_ADDRESS,
        ROW_HOOK_LEN as OPTIONS_ROW_HOOK_LEN, ROW_PRG_BANK as OPTIONS_ROW_PRG_BANK,
        build_page_routine as build_options_page_routine,
        row_calculation as options_row_calculation, row_hook as options_row_hook,
    },
    roster_page::{
        ALIGNMENT_PADDING_PHYSICAL_CHR_PAGE, CENTRAL_RIGHT_FD_SELECTOR_CALL_ADDRESS,
        PAGE_REGISTER as ROSTER_PAGE_REGISTER, PAGE_ROUTINE_ADDRESS as ROSTER_PAGE_ROUTINE_ADDRESS,
        PAGE_ROUTINE_END as ROSTER_PAGE_ROUTINE_END, PHYSICAL_CHR_PAGE as ROSTER_PHYSICAL_CHR_PAGE,
        build_page_routine as build_roster_page_routine, central_right_fd_selector_call,
    },
};

const CHR_PAGE_SIZE: usize = 0x1000;
const OPTIONS_PAGE_COUNT: usize = 2;
const EXTENSION_PAGE_COUNT: usize = 4;
const OUTPUT_CHR_BANK_COUNT: u8 = 19;

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
    roster_page_sha1: String,
    alignment_padding_page_sha1: String,
    pages: Vec<PageBindingReport>,
    screen_contract: ScreenContractReport,
    roster_screen_contract: RosterScreenContractReport,
    hook: HookReport,
    roster_hook: RosterHookReport,
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
struct RosterScreenContractReport {
    screen_role: &'static str,
    left_fd: u8,
    left_fe: u8,
    right_fd: u8,
    observed_right_fe: Vec<u8>,
    unobserved_right_fe: Vec<u8>,
    translated_header_codes: Vec<u8>,
    cleared_source_codes: Vec<u8>,
    preserved_original_codes: Vec<u8>,
    preserved_original_labels: Vec<&'static str>,
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
struct RosterHookReport {
    source_cpu_address: String,
    source_len: usize,
    routine_cpu_start: String,
    routine_cpu_end_exclusive: String,
    routine_len: usize,
    exact_contract_only: bool,
    preserves_input_accumulator_and_status: bool,
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
    pub(crate) roster_page_sha1: String,
    pub(crate) tracked_write_count: usize,
}

pub(crate) fn build_mapper165_hangul_page_probe(
    source_path: &Path,
    localization_path: &Path,
    roster_localization_path: &Path,
    output_path: &Path,
    report_path: &Path,
) -> Result<HangulPageProbeSummary> {
    let source_rom = Rom::from_path(source_path)?;
    source_rom.verify_supported_japanese()?;
    let localization = OptionsLocalization::from_path(localization_path)?;
    let validated_localization = localization.validate()?;
    let page_pack = assemble_hangul_page_pack(&source_rom, &localization)?;
    ensure!(
        page_pack.len() == OPTIONS_PAGE_COUNT * CHR_PAGE_SIZE,
        "Hangul page probe needs exactly two 4 KiB pages"
    );
    let roster_localization = RosterLocalization::from_path(roster_localization_path)?;
    let validated_roster_localization = roster_localization.validate()?;
    let roster_page = build_roster_font_page(
        &source_rom.chr()[..CHR_PAGE_SIZE],
        &validated_roster_localization,
    )?;
    let alignment_padding_page = vec![0_u8; CHR_PAGE_SIZE];
    let page_a_register = encode_chr_page_register(FIRST_EXTENSION_CHR_PAGE)?;
    let page_b_register = encode_chr_page_register(FIRST_EXTENSION_CHR_PAGE + 1)?;
    ensure!(
        page_a_register == OPTIONS_PAGE_A_REGISTER && page_b_register == OPTIONS_PAGE_B_REGISTER,
        "Hangul page register contract changed"
    );
    let roster_page_register = encode_chr_page_register(ROSTER_PHYSICAL_CHR_PAGE)?;
    ensure!(
        roster_page_register == ROSTER_PAGE_REGISTER,
        "roster Hangul page register contract changed"
    );
    let alignment_padding_page_register =
        encode_chr_page_register(ALIGNMENT_PADDING_PHYSICAL_CHR_PAGE)?;

    let parity_base = assemble_mapper165_parity_bytes(&source_rom)?;
    let parity_base_sha1 = sha1_hex(&parity_base);
    let parity_rom = Rom::parse(parity_base.clone()).context("parse mapper 165 parity base")?;
    ensure!(
        parity_rom.chr().len() == 17 * 8 * 1024,
        "mapper 165 parity base CHR size changed"
    );

    let options_routine = build_options_page_routine(page_a_register, page_b_register)?;
    ensure!(
        OPTIONS_PAGE_ROUTINE_ADDRESS as usize + options_routine.len()
            == OPTIONS_PAGE_ROUTINE_END as usize,
        "options page routine size changed"
    );
    let options_cave_start = fixed_bank_file_offset(OPTIONS_PAGE_ROUTINE_ADDRESS)?;
    let options_cave_end = options_cave_start + options_routine.len();
    ensure!(
        parity_base[options_cave_start..options_cave_end]
            .iter()
            .all(|byte| *byte == 0xFF),
        "options page routine cave is no longer all FF"
    );
    let options_direct_transfer_count = count_direct_transfers_to_range(
        source_rom.prg(),
        OPTIONS_PAGE_ROUTINE_ADDRESS,
        OPTIONS_PAGE_ROUTINE_END,
    )?;
    ensure!(
        options_direct_transfer_count == 0,
        "options page routine cave has {options_direct_transfer_count} pre-existing direct transfers"
    );

    let roster_routine = build_roster_page_routine(roster_page_register)?;
    ensure!(
        ROSTER_PAGE_ROUTINE_ADDRESS as usize + roster_routine.len()
            == ROSTER_PAGE_ROUTINE_END as usize,
        "roster page routine size changed"
    );
    let roster_cave_start = fixed_bank_file_offset(ROSTER_PAGE_ROUTINE_ADDRESS)?;
    let roster_cave_end = roster_cave_start + roster_routine.len();
    ensure!(
        parity_base[roster_cave_start..roster_cave_end]
            .iter()
            .all(|byte| *byte == 0xFF),
        "roster page routine cave is no longer all FF"
    );
    let roster_direct_transfer_count = count_direct_transfers_to_range(
        source_rom.prg(),
        ROSTER_PAGE_ROUTINE_ADDRESS,
        ROSTER_PAGE_ROUTINE_END,
    )?;
    ensure!(
        roster_direct_transfer_count == 0,
        "roster page routine cave has {roster_direct_transfer_count} pre-existing direct transfers"
    );
    let direct_code_cave_transfer_count =
        options_direct_transfer_count + roster_direct_transfer_count;

    let mut expanded_base = parity_base.clone();
    expanded_base.extend_from_slice(&page_pack);
    expanded_base.extend_from_slice(&roster_page);
    expanded_base.extend_from_slice(&alignment_padding_page);
    ensure!(
        expanded_base.len() == parity_base.len() + EXTENSION_PAGE_COUNT * CHR_PAGE_SIZE,
        "mapper 165 Hangul extension page count changed"
    );
    let mut image = TrackedImage::new(expanded_base.clone());
    image.write_expected(
        "expand mapper 165 CHR from 17 to 19 banks",
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
        options_cave_start,
        &vec![0xFF; options_routine.len()],
        &options_routine,
    )?;
    image.write_expected(
        "Korean roster name header",
        switchable_bank_file_offset(ROSTER_TEXT_PRG_BANK, ROSTER_HEADER_CPU_ADDRESS)?,
        &SOURCE_ROSTER_HEADER,
        &validated_roster_localization.replacement_header,
    )?;
    image.write_expected(
        "roster screen Hangul page routine",
        roster_cave_start,
        &vec![0xFF; roster_routine.len()],
        &roster_routine,
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
    image.write_expected(
        "central right FD selector to roster-aware selector",
        fixed_bank_file_offset(CENTRAL_RIGHT_FD_SELECTOR_CALL_ADDRESS)?,
        &central_right_fd_selector_call(SELECT_RIGHT_FD_CHR_BANK_FOR_PAIR_ADDRESS)?,
        &central_right_fd_selector_call(ROSTER_PAGE_ROUTINE_ADDRESS)?,
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
        &roster_page,
        &alignment_padding_page,
        &validated_localization.replacement_table,
        &validated_roster_localization.replacement_header,
    )?;

    let output_sha1 = sha1_hex(&output);
    let page_pack_sha1 = sha1_hex(&page_pack);
    let roster_page_sha1 = sha1_hex(&roster_page);
    let alignment_padding_page_sha1 = sha1_hex(&alignment_padding_page);
    let report = HangulPageProbeReport {
        schema: 1,
        source_sha1: EXPECTED_SOURCE_SHA1,
        parity_base_sha1,
        output_sha1: output_sha1.clone(),
        output_mapper: output_rom.mapper(),
        prg_size: output_rom.prg().len(),
        chr_size: output_rom.chr().len(),
        page_pack_sha1: page_pack_sha1.clone(),
        roster_page_sha1: roster_page_sha1.clone(),
        alignment_padding_page_sha1: alignment_padding_page_sha1.clone(),
        pages: vec![
            PageBindingReport {
                role: "options_page_a",
                physical_4k_page: FIRST_EXTENSION_CHR_PAGE,
                mapper_register_value: page_a_register,
                page_sha1: sha1_hex(&page_pack[..CHR_PAGE_SIZE]),
            },
            PageBindingReport {
                role: "options_page_b",
                physical_4k_page: FIRST_EXTENSION_CHR_PAGE + 1,
                mapper_register_value: page_b_register,
                page_sha1: sha1_hex(&page_pack[CHR_PAGE_SIZE..]),
            },
            PageBindingReport {
                role: "unit_roster",
                physical_4k_page: ROSTER_PHYSICAL_CHR_PAGE,
                mapper_register_value: roster_page_register,
                page_sha1: roster_page_sha1.clone(),
            },
            PageBindingReport {
                role: "ines_8k_alignment_padding",
                physical_4k_page: ALIGNMENT_PADDING_PHYSICAL_CHR_PAGE,
                mapper_register_value: alignment_padding_page_register,
                page_sha1: alignment_padding_page_sha1,
            },
        ],
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
        roster_screen_contract: RosterScreenContractReport {
            screen_role: "unit_roster",
            left_fd: 0x18,
            left_fe: 0x18,
            right_fd: 0x00,
            observed_right_fe: vec![0x15, 0x18, 0x19],
            unobserved_right_fe: vec![],
            translated_header_codes: vec![0x15, 0x20],
            cleared_source_codes: vec![0x03],
            preserved_original_codes: vec![0x60, 0x61, 0x62, 0x66, 0x68, 0x71, 0x75, 0x79, 0x7F],
            preserved_original_labels: vec!["0", "1", "2", "6", "8", "H", "L", "P", "V"],
            fallback: "call the existing pair-aware right FD selector for every other contract",
        },
        hook: HookReport {
            source_prg_bank: OPTIONS_ROW_PRG_BANK,
            source_cpu_address: format!("0x{OPTIONS_ROW_HOOK_ADDRESS:04X}"),
            source_len: OPTIONS_ROW_HOOK_LEN,
            routine_cpu_start: format!("0x{OPTIONS_PAGE_ROUTINE_ADDRESS:04X}"),
            routine_cpu_end_exclusive: format!("0x{OPTIONS_PAGE_ROUTINE_END:04X}"),
            routine_len: options_routine.len(),
            preserves_original_row_calculation: true,
            preserves_y_and_status_result: true,
        },
        roster_hook: RosterHookReport {
            source_cpu_address: format!("0x{CENTRAL_RIGHT_FD_SELECTOR_CALL_ADDRESS:04X}"),
            source_len: 3,
            routine_cpu_start: format!("0x{ROSTER_PAGE_ROUTINE_ADDRESS:04X}"),
            routine_cpu_end_exclusive: format!("0x{ROSTER_PAGE_ROUTINE_END:04X}"),
            routine_len: roster_routine.len(),
            exact_contract_only: true,
            preserves_input_accumulator_and_status: true,
        },
        direct_code_cave_transfer_count,
        tracked_writes,
        runtime_hook_installed: true,
        runtime_verified: false,
        unresolved_boundaries: vec![
            "The mixed Hangul and original-Latin roster page needs cold visible proof for the observed 00/15, 00/18, and 00/19 backing-page variants.",
            "The options A-B-A selector and natural-page restoration need a regression check on this expanded output.",
            "The mixed roster contract covers the three observed 00/15, 00/18, and 00/19 backing-page variants; other pairs still fall back to the natural page.",
            "The pages prove screen-bound selection, not final corpus packing or whole-game coverage.",
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
        roster_page_sha1,
        tracked_write_count,
    })
}

fn verify_output(
    parity_rom: &Rom,
    output_rom: &Rom,
    page_pack: &[u8],
    roster_page: &[u8],
    alignment_padding_page: &[u8],
    replacement_table: &[u8],
    replacement_roster_header: &[u8],
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
        output_rom.chr().len()
            == parity_rom.chr().len()
                + page_pack.len()
                + roster_page.len()
                + alignment_padding_page.len(),
        "Hangul page output CHR size is incorrect"
    );
    ensure!(
        output_rom.chr()[..parity_rom.chr().len()] == *parity_rom.chr(),
        "Hangul page probe changed the mapper 165 parity CHR base"
    );
    ensure!(
        output_rom.chr()[parity_rom.chr().len()..parity_rom.chr().len() + page_pack.len()]
            == *page_pack,
        "Hangul page probe appended different options page bytes"
    );
    let roster_page_start = parity_rom.chr().len() + page_pack.len();
    ensure!(
        output_rom.chr()[roster_page_start..roster_page_start + roster_page.len()] == *roster_page,
        "Hangul page probe appended different roster page bytes"
    );
    let padding_page_start = roster_page_start + roster_page.len();
    ensure!(
        output_rom.chr()[padding_page_start..] == *alignment_padding_page,
        "Hangul page probe appended different iNES alignment padding bytes"
    );
    ensure!(
        output_rom.data()[OPTIONS_TABLE_OFFSET..OPTIONS_TABLE_OFFSET + replacement_table.len()]
            == *replacement_table,
        "Hangul page probe options table changed"
    );
    let roster_header_offset =
        switchable_bank_file_offset(ROSTER_TEXT_PRG_BANK, ROSTER_HEADER_CPU_ADDRESS)?;
    ensure!(
        output_rom.data()
            [roster_header_offset..roster_header_offset + replacement_roster_header.len()]
            == *replacement_roster_header,
        "Hangul page probe roster header changed"
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
