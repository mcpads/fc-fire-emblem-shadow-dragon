use std::{collections::BTreeSet, fs, path::Path};

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
        build_roster_page_pair,
    },
    sha1_hex,
    tracked::TrackedImage,
};

use super::{
    FIRST_EXTENSION_CHR_PAGE, SELECT_RIGHT_FD_CHR_BANK_FOR_PAIR_ADDRESS, encode_chr_page_register,
    install_mapper165_parity_bytes,
    options_lifetime::inspect_options_lifetime,
    options_page::{
        OPTIONS_COMPOSITE_STATE, OPTIONS_COMPOSITE_STATE_ADDRESS,
        PAGE_A_REGISTER as OPTIONS_PAGE_A_REGISTER, PAGE_B_REGISTER as OPTIONS_PAGE_B_REGISTER,
        PAGE_ROUTINE_ADDRESS as OPTIONS_PAGE_ROUTINE_ADDRESS,
        PAGE_ROUTINE_END as OPTIONS_PAGE_ROUTINE_END, ROW_HOOK_ADDRESS as OPTIONS_ROW_HOOK_ADDRESS,
        ROW_HOOK_LEN as OPTIONS_ROW_HOOK_LEN,
        ROW_OWNER_GATE_ADDRESS as OPTIONS_ROW_OWNER_GATE_ADDRESS,
        ROW_OWNER_GATE_END as OPTIONS_ROW_OWNER_GATE_END, ROW_PRG_BANK as OPTIONS_ROW_PRG_BANK,
        build_page_routine_with_fallback as build_options_page_routine_with_fallback,
        build_row_owner_gate as build_options_row_owner_gate,
        row_calculation as options_row_calculation, row_hook as options_row_hook,
    },
    roster_page::{
        CENTRAL_RIGHT_FD_SELECTOR_CALL_ADDRESS, HEADER_CALL_ADDRESS as ROSTER_HEADER_CALL_ADDRESS,
        HEADER_RESOURCE_ID as ROSTER_HEADER_RESOURCE_ID,
        OWNER_CONSTRUCTOR_ADDRESS as ROSTER_OWNER_CONSTRUCTOR_ADDRESS,
        OWNER_CONSTRUCTOR_PRG_BANK as ROSTER_OWNER_CONSTRUCTOR_PRG_BANK,
        OWNER_CONSTRUCTOR_SIGNATURE as ROSTER_OWNER_CONSTRUCTOR_SIGNATURE,
        PAGE_REGISTERS as ROSTER_PAGE_REGISTERS,
        PAGE_ROUTINE_ADDRESS as ROSTER_PAGE_ROUTINE_ADDRESS,
        PAGE_ROUTINE_END as ROSTER_PAGE_ROUTINE_END,
        PHYSICAL_CHR_PAGES as ROSTER_PHYSICAL_CHR_PAGES,
        bind_header_composite_route as bind_roster_header_composite_route,
        build_page_routine as build_roster_page_routine, central_right_fd_selector_call,
    },
};

const CHR_PAGE_SIZE: usize = 0x1000;
const OPTIONS_PAGE_COUNT: usize = 2;
const EXTENSION_PAGE_COUNT: usize = 4;
const OUTPUT_CHR_BANK_COUNT: u8 = 19;

#[derive(Debug, Serialize)]
struct UiPageInstallReport {
    schema: u32,
    source_sha1: &'static str,
    parity_base_sha1: String,
    output_sha1: String,
    output_mapper: u16,
    prg_size: usize,
    chr_size: usize,
    page_pack_sha1: String,
    roster_page_pack_sha1: String,
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
    screen_evidence_manifest_sha1: String,
    temporal_sample_count: usize,
    unique_nametable_count: usize,
    observed_row_states: Vec<u8>,
    target_glyph_count: usize,
    visible_active_code_count: usize,
    preserved_active_code_count: usize,
    total_slot_demand: usize,
    fallback: &'static str,
}

#[derive(Debug, Serialize)]
struct RosterScreenContractReport {
    screen_role: &'static str,
    owner_prg_bank: u8,
    owner_constructor_cpu_address: String,
    owner_descriptor_signature_occurrences: usize,
    owner_resource_id: u8,
    owner_resource_call_cpu_address: String,
    owner_header_cpu_address: String,
    lifetime_state: Vec<LifetimeStateByteReport>,
    left_fd: u8,
    left_fe: u8,
    right_fd: u8,
    observed_right_fe: Vec<u8>,
    unobserved_right_fe: Vec<u8>,
    page_a_right_fe: Vec<u8>,
    page_b_right_fe: Vec<u8>,
    page_assignment_count: usize,
    page_local_proof_glyph_count: usize,
    page_union_glyph_count: usize,
    translated_header_codes: Vec<u8>,
    cleared_source_codes: Vec<u8>,
    preserved_original_codes: Vec<u8>,
    preserved_original_labels: Vec<&'static str>,
    fallback: &'static str,
}

#[derive(Debug, Serialize)]
struct LifetimeStateByteReport {
    address: String,
    value: u8,
    role: &'static str,
}

#[derive(Debug, Serialize)]
struct HookReport {
    source_prg_bank: u8,
    source_cpu_address: String,
    source_len: usize,
    owner_state_address: String,
    owner_state_value: u8,
    owner_gate_cpu_start: String,
    owner_gate_cpu_end_exclusive: String,
    owner_gate_len: usize,
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
    original_target_cpu_address: String,
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

pub(crate) struct UiPageInstallSummary {
    pub(crate) output_sha1: String,
    pub(crate) report_sha1: String,
    pub(crate) page_pack_sha1: String,
    pub(crate) roster_page_pack_sha1: String,
    pub(crate) options_screen_evidence_manifest_sha1: String,
    pub(crate) options_temporal_sample_count: usize,
    pub(crate) options_unique_nametable_count: usize,
    pub(crate) options_observed_row_states: Vec<u8>,
    pub(crate) options_target_glyph_count: usize,
    pub(crate) options_visible_active_code_count: usize,
    pub(crate) options_preserved_active_code_count: usize,
    pub(crate) options_total_slot_demand: usize,
    pub(crate) tracked_write_count: usize,
}

pub(crate) fn build_mapper165_hangul_page_probe(
    source_path: &Path,
    localization_path: &Path,
    roster_localization_path: &Path,
    options_screen_evidence_path: &Path,
    output_path: &Path,
    report_path: &Path,
) -> Result<UiPageInstallSummary> {
    let source_rom = Rom::from_path(source_path)?;
    let parity_base = install_mapper165_parity_bytes(&source_rom)?;
    install_mapper165_ui_pages_from_parity(
        &source_rom,
        &parity_base,
        localization_path,
        roster_localization_path,
        options_screen_evidence_path,
        output_path,
        report_path,
    )
}

pub(crate) fn install_mapper165_ui_pages_from_parity(
    source_rom: &Rom,
    parity_base: &[u8],
    localization_path: &Path,
    roster_localization_path: &Path,
    options_screen_evidence_path: &Path,
    output_path: &Path,
    report_path: &Path,
) -> Result<UiPageInstallSummary> {
    source_rom.verify_supported_japanese()?;
    bind_roster_header_composite_route(source_rom)?;
    let roster_owner_constructor_offset = switchable_bank_file_offset(
        ROSTER_OWNER_CONSTRUCTOR_PRG_BANK,
        ROSTER_OWNER_CONSTRUCTOR_ADDRESS,
    )?;
    ensure!(
        source_rom.data()[roster_owner_constructor_offset
            ..roster_owner_constructor_offset + ROSTER_OWNER_CONSTRUCTOR_SIGNATURE.len()]
            == ROSTER_OWNER_CONSTRUCTOR_SIGNATURE,
        "roster owner constructor signature changed"
    );
    let roster_owner_descriptor_signature_occurrences = source_rom
        .prg()
        .windows(ROSTER_OWNER_CONSTRUCTOR_SIGNATURE.len())
        .filter(|window| *window == ROSTER_OWNER_CONSTRUCTOR_SIGNATURE)
        .count();
    ensure!(
        roster_owner_descriptor_signature_occurrences == 1,
        "roster owner descriptor signature must occur exactly once"
    );
    let localization = OptionsLocalization::from_path(localization_path)?;
    let validated_localization = localization.validate()?;
    let option_target_codes = localization
        .glyphs
        .iter()
        .map(|glyph| glyph.code)
        .collect::<BTreeSet<_>>();
    ensure!(
        option_target_codes.len() == validated_localization.tiles.len(),
        "options glyph assignments repeat a target code"
    );
    let options_lifetime =
        inspect_options_lifetime(options_screen_evidence_path, &option_target_codes)?;
    let page_pack = assemble_hangul_page_pack(source_rom, &localization)?;
    ensure!(
        page_pack.len() == OPTIONS_PAGE_COUNT * CHR_PAGE_SIZE,
        "UI page installation needs exactly two 4 KiB pages"
    );
    let roster_localization = RosterLocalization::from_path(roster_localization_path)?;
    let validated_roster_localization = roster_localization.validate()?;
    let roster_pages = build_roster_page_pair(
        &source_rom.chr()[..CHR_PAGE_SIZE],
        &validated_roster_localization,
        ROSTER_PHYSICAL_CHR_PAGES,
    )?;
    let page_a_register = encode_chr_page_register(FIRST_EXTENSION_CHR_PAGE)?;
    let page_b_register = encode_chr_page_register(FIRST_EXTENSION_CHR_PAGE + 1)?;
    ensure!(
        page_a_register == OPTIONS_PAGE_A_REGISTER && page_b_register == OPTIONS_PAGE_B_REGISTER,
        "Hangul page register contract changed"
    );
    let roster_page_registers = [
        encode_chr_page_register(ROSTER_PHYSICAL_CHR_PAGES[0])?,
        encode_chr_page_register(ROSTER_PHYSICAL_CHR_PAGES[1])?,
    ];
    ensure!(
        roster_page_registers == ROSTER_PAGE_REGISTERS,
        "roster Hangul page registers changed"
    );

    let parity_base_sha1 = sha1_hex(parity_base);
    let parity_rom = Rom::parse(parity_base.to_vec()).context("parse mapper 165 parity base")?;
    ensure!(
        parity_rom.chr().len() == 17 * 8 * 1024,
        "mapper 165 parity base CHR size changed"
    );

    let options_routine = build_options_page_routine_with_fallback(
        page_a_register,
        page_b_register,
        ROSTER_PAGE_ROUTINE_ADDRESS,
    )?;
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

    let options_row_owner_gate = build_options_row_owner_gate()?;
    ensure!(
        OPTIONS_ROW_OWNER_GATE_ADDRESS as usize + options_row_owner_gate.len()
            <= OPTIONS_ROW_OWNER_GATE_END as usize,
        "options row owner gate exceeds its fixed-bank cave"
    );
    let options_row_owner_gate_start = fixed_bank_file_offset(OPTIONS_ROW_OWNER_GATE_ADDRESS)?;
    let options_row_owner_gate_end = options_row_owner_gate_start + options_row_owner_gate.len();
    ensure!(
        parity_base[options_row_owner_gate_start..options_row_owner_gate_end]
            .iter()
            .all(|byte| *byte == 0xFF),
        "options row owner gate cave is no longer all FF"
    );
    let options_row_owner_gate_direct_transfer_count = count_direct_transfers_to_range(
        source_rom.prg(),
        OPTIONS_ROW_OWNER_GATE_ADDRESS,
        OPTIONS_ROW_OWNER_GATE_END,
    )?;
    ensure!(
        options_row_owner_gate_direct_transfer_count == 0,
        "options row owner gate cave has {options_row_owner_gate_direct_transfer_count} pre-existing direct transfers"
    );

    let roster_routine =
        build_roster_page_routine(roster_page_registers[0], roster_page_registers[1])?;
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
    let direct_code_cave_transfer_count = options_direct_transfer_count
        + options_row_owner_gate_direct_transfer_count
        + roster_direct_transfer_count;

    let mut expanded_base = parity_base.to_vec();
    expanded_base.extend_from_slice(&page_pack);
    expanded_base.extend_from_slice(&roster_pages.page_pack);
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
        "options row owner gate",
        options_row_owner_gate_start,
        &vec![0xFF; options_row_owner_gate.len()],
        &options_row_owner_gate,
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
        "central right FD selector to roster-lifetime selector",
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
    let output_rom = Rom::parse(output.clone()).context("parse mapper 165 UI page installation")?;
    verify_output(
        &parity_rom,
        &output_rom,
        &page_pack,
        &roster_pages.page_pack,
        &validated_localization.replacement_table,
        &validated_roster_localization.replacement_header,
    )?;

    let output_sha1 = sha1_hex(&output);
    let page_pack_sha1 = sha1_hex(&page_pack);
    let roster_page_pack_sha1 = sha1_hex(&roster_pages.page_pack);
    let report = UiPageInstallReport {
        schema: 1,
        source_sha1: EXPECTED_SOURCE_SHA1,
        parity_base_sha1,
        output_sha1: output_sha1.clone(),
        output_mapper: output_rom.mapper(),
        prg_size: output_rom.prg().len(),
        chr_size: output_rom.chr().len(),
        page_pack_sha1: page_pack_sha1.clone(),
        roster_page_pack_sha1: roster_page_pack_sha1.clone(),
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
                role: "unit_roster_page_a",
                physical_4k_page: ROSTER_PHYSICAL_CHR_PAGES[0],
                mapper_register_value: roster_page_registers[0],
                page_sha1: roster_pages.page_sha1s[0].clone(),
            },
            PageBindingReport {
                role: "unit_roster_page_b",
                physical_4k_page: ROSTER_PHYSICAL_CHR_PAGES[1],
                mapper_register_value: roster_page_registers[1],
                page_sha1: roster_pages.page_sha1s[1].clone(),
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
            screen_evidence_manifest_sha1: options_lifetime.manifest_sha1.clone(),
            temporal_sample_count: options_lifetime.sample_count,
            unique_nametable_count: options_lifetime.unique_nametable_count,
            observed_row_states: options_lifetime.observed_row_states.clone(),
            target_glyph_count: options_lifetime.target_glyph_count,
            visible_active_code_count: options_lifetime.visible_active_code_count,
            preserved_active_code_count: options_lifetime.preserved_active_code_count,
            total_slot_demand: options_lifetime.total_slot_demand,
            fallback: "call the existing pair-aware right FD selector",
        },
        roster_screen_contract: RosterScreenContractReport {
            screen_role: "unit_roster",
            owner_prg_bank: ROSTER_OWNER_CONSTRUCTOR_PRG_BANK,
            owner_constructor_cpu_address: format!("0x{ROSTER_OWNER_CONSTRUCTOR_ADDRESS:04X}"),
            owner_descriptor_signature_occurrences: roster_owner_descriptor_signature_occurrences,
            owner_resource_id: ROSTER_HEADER_RESOURCE_ID,
            owner_resource_call_cpu_address: format!("0x{ROSTER_HEADER_CALL_ADDRESS:04X}"),
            owner_header_cpu_address: format!("0x{ROSTER_HEADER_CPU_ADDRESS:04X}"),
            lifetime_state: vec![
                LifetimeStateByteReport {
                    address: "0x05CF".to_owned(),
                    value: 0x12,
                    role: "roster window descriptor byte zero",
                },
                LifetimeStateByteReport {
                    address: "0x05D0".to_owned(),
                    value: 0x04,
                    role: "roster window descriptor byte one",
                },
                LifetimeStateByteReport {
                    address: "0x0070".to_owned(),
                    value: 0x30,
                    role: "roster window horizontal placement",
                },
                LifetimeStateByteReport {
                    address: "0x0071".to_owned(),
                    value: 0x40,
                    role: "roster window vertical placement",
                },
            ],
            left_fd: 0x18,
            left_fe: 0x18,
            right_fd: 0x00,
            observed_right_fe: vec![0x15, 0x18, 0x19],
            unobserved_right_fe: vec![],
            page_a_right_fe: vec![0x18, 0x19],
            page_b_right_fe: vec![0x15],
            page_assignment_count: roster_pages.assignment_count_per_page,
            page_local_proof_glyph_count: roster_pages.page_local_proof_glyph_count,
            page_union_glyph_count: roster_pages.page_union_glyph_count,
            translated_header_codes: vec![0x15, 0x20],
            cleared_source_codes: vec![0x03],
            preserved_original_codes: vec![0x60, 0x61, 0x62, 0x66, 0x68, 0x71, 0x75, 0x79, 0x7F],
            preserved_original_labels: vec!["0", "1", "2", "6", "8", "H", "L", "P", "V"],
            fallback: "call the existing pair-aware selector unless the exact roster descriptor and an observed right FE phase both match",
        },
        hook: HookReport {
            source_prg_bank: OPTIONS_ROW_PRG_BANK,
            source_cpu_address: format!("0x{OPTIONS_ROW_HOOK_ADDRESS:04X}"),
            source_len: OPTIONS_ROW_HOOK_LEN,
            owner_state_address: format!("0x{OPTIONS_COMPOSITE_STATE_ADDRESS:04X}"),
            owner_state_value: OPTIONS_COMPOSITE_STATE,
            owner_gate_cpu_start: format!("0x{OPTIONS_ROW_OWNER_GATE_ADDRESS:04X}"),
            owner_gate_cpu_end_exclusive: format!("0x{OPTIONS_ROW_OWNER_GATE_END:04X}"),
            owner_gate_len: options_row_owner_gate.len(),
            routine_cpu_start: format!("0x{OPTIONS_PAGE_ROUTINE_ADDRESS:04X}"),
            routine_cpu_end_exclusive: format!("0x{OPTIONS_PAGE_ROUTINE_END:04X}"),
            routine_len: options_routine.len(),
            preserves_original_row_calculation: true,
            preserves_y_and_status_result: true,
        },
        roster_hook: RosterHookReport {
            source_cpu_address: format!("0x{CENTRAL_RIGHT_FD_SELECTOR_CALL_ADDRESS:04X}"),
            source_len: 3,
            original_target_cpu_address: format!(
                "0x{SELECT_RIGHT_FD_CHR_BANK_FOR_PAIR_ADDRESS:04X}"
            ),
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
            "Cold visible roster A-B-A evidence stays external to this static build report, so runtime_verified remains false by construction.",
            "The options A-B-A selector, natural-page restoration, and non-options owner-gate fallthrough need a regression check on this expanded output.",
            "The mixed roster contract covers the three observed 00/15, 00/18, and 00/19 backing-page variants; other pairs still fall back to the natural page.",
            "The pages prove screen-bound selection, not final corpus packing or whole-game coverage.",
        ],
        release_eligible: false,
    };
    let report_bytes = serde_json::to_vec_pretty(&report)
        .context("serialize mapper 165 UI page installation report")?;
    let report_sha1 = sha1_hex(&report_bytes);
    let tracked_write_count = report.tracked_writes.len();

    write_file(output_path, &output)?;
    write_file(report_path, &report_bytes)?;
    Ok(UiPageInstallSummary {
        output_sha1,
        report_sha1,
        page_pack_sha1,
        roster_page_pack_sha1,
        options_screen_evidence_manifest_sha1: options_lifetime.manifest_sha1,
        options_temporal_sample_count: options_lifetime.sample_count,
        options_unique_nametable_count: options_lifetime.unique_nametable_count,
        options_observed_row_states: options_lifetime.observed_row_states,
        options_target_glyph_count: options_lifetime.target_glyph_count,
        options_visible_active_code_count: options_lifetime.visible_active_code_count,
        options_preserved_active_code_count: options_lifetime.preserved_active_code_count,
        options_total_slot_demand: options_lifetime.total_slot_demand,
        tracked_write_count,
    })
}

fn verify_output(
    parity_rom: &Rom,
    output_rom: &Rom,
    page_pack: &[u8],
    roster_page_pack: &[u8],
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
        output_rom.chr().len() == parity_rom.chr().len() + page_pack.len() + roster_page_pack.len(),
        "Hangul page output CHR size is incorrect"
    );
    ensure!(
        output_rom.chr()[..parity_rom.chr().len()] == *parity_rom.chr(),
        "UI page installation changed the mapper 165 parity CHR base"
    );
    ensure!(
        output_rom.chr()[parity_rom.chr().len()..parity_rom.chr().len() + page_pack.len()]
            == *page_pack,
        "UI page installation appended different options page bytes"
    );
    let roster_page_pack_start = parity_rom.chr().len() + page_pack.len();
    ensure!(
        output_rom.chr()[roster_page_pack_start..] == *roster_page_pack,
        "UI page installation appended different roster page-pair bytes"
    );
    ensure!(
        output_rom.data()[OPTIONS_TABLE_OFFSET..OPTIONS_TABLE_OFFSET + replacement_table.len()]
            == *replacement_table,
        "UI page installation changed the options table"
    );
    let roster_header_offset =
        switchable_bank_file_offset(ROSTER_TEXT_PRG_BANK, ROSTER_HEADER_CPU_ADDRESS)?;
    ensure!(
        output_rom.data()
            [roster_header_offset..roster_header_offset + replacement_roster_header.len()]
            == *replacement_roster_header,
        "UI page installation changed the roster header"
    );
    ensure!(
        output_rom.data()[6] & 0x02 == parity_rom.data()[6] & 0x02,
        "UI page installation changed the battery flag"
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
