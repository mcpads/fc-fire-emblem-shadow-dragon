use std::path::Path;

use anyhow::{Context, Result, ensure};

use crate::{
    dialogue_assets::MainDialogueSlicePlan,
    font_slots::FONT_PAGE_SIZE,
    mmc5_chr::switchable_bank_file_offset,
    mmc5_prg::{count_direct_transfers_to_range, fixed_bank_file_offset},
    rom::{HEADER_SIZE, Rom},
    sha1_hex,
    tracked::TrackedImage,
};

use super::super::{
    OUTPUT_MAPPER,
    battle_composition_loader_probe::CUMULATIVE_RUNTIME_LAYOUT,
    maximum_dialogue_page::{MaximumDialoguePagePlan, plan_maximum_dialogue_pages},
    maximum_dialogue_runtime::{
        COMPLETED_PAGE_CONTINUE_ADDRESS, COMPLETED_PAGE_CONTINUE_SOURCE,
        FONT_GROUP_SELECTOR_ADDRESS, FONT_GROUP_SELECTOR_END, INITIAL_FONT_SUPPLY_POINTER_ADVANCE,
        INITIAL_PAGE_SELECTOR_ADDRESS, INITIAL_PAGE_SELECTOR_CAVE_END, MAIN_DIALOGUE_PRG_BANK,
        MAXIMUM_DIALOGUE_PAGE_RELOAD_ADDRESS, MAXIMUM_DIALOGUE_PAGE_RELOAD_END,
        build_completed_page_continue_hook, build_font_group_selector, build_initial_page_selector,
        build_maximum_dialogue_page_reload,
    },
};
use super::ROSTER_SELECTOR_ADDRESS;

const FIXED_BANK_SIZE: usize = 16 * 1024;
const EXPECTED_INPUT_CHR_BANK_COUNT: u8 = 25;
const OUTPUT_CHR_BANK_COUNT: u8 = 27;

pub(super) struct MaximumDialogueStageInputs<'a> {
    pub(super) prior_output: &'a [u8],
    pub(super) source_rom: &'a Rom,
    pub(super) record: &'a MainDialogueSlicePlan,
    pub(super) evidence_path: &'a Path,
    pub(super) page_boundary_path: &'a Path,
}

pub(super) struct MaximumDialogueStageOutput {
    pub(super) output: Vec<u8>,
    pub(super) output_sha1: String,
    pub(super) page: MaximumDialoguePagePlan,
    pub(super) tracked_write_count: usize,
    pub(super) initial_selector_byte_count: usize,
    pub(super) font_group_selector_byte_count: usize,
    pub(super) completed_page_transition_byte_count: usize,
}

pub(super) fn install_maximum_dialogue_stage(
    inputs: MaximumDialogueStageInputs<'_>,
) -> Result<MaximumDialogueStageOutput> {
    let prior_rom =
        Rom::parse(inputs.prior_output.to_vec()).context("parse pre-maximum-dialogue output")?;
    ensure!(
        prior_rom.mapper() == OUTPUT_MAPPER
            && prior_rom.data()[5] == EXPECTED_INPUT_CHR_BANK_COUNT
            && prior_rom.chr().len()
                == usize::from(EXPECTED_INPUT_CHR_BANK_COUNT) * 2 * FONT_PAGE_SIZE,
        "maximum dialogue input layout changed"
    );
    let page = plan_maximum_dialogue_pages(
        &prior_rom,
        inputs.record,
        inputs.evidence_path,
        inputs.page_boundary_path,
    )?;
    let mapper_registers: [u8; 3] = page
        .mapper_registers
        .clone()
        .try_into()
        .map_err(|_| anyhow::anyhow!("maximum dialogue mapper register count changed"))?;
    let font_group_selector =
        build_font_group_selector(mapper_registers, page.group_transition_pointers)?;
    let completed_page_reload = build_maximum_dialogue_page_reload()?;
    let completed_page_hook = build_completed_page_continue_hook()?;
    let initial_supply_pointer = inputs
        .record
        .source_pointer_cpu_address()
        .checked_add(INITIAL_FONT_SUPPLY_POINTER_ADVANCE)
        .context("maximum dialogue initial font-supply pointer overflow")?;
    let initial_selector =
        build_initial_page_selector(ROSTER_SELECTOR_ADDRESS, initial_supply_pointer)?;
    validate_caves(inputs.source_rom, &prior_rom)?;

    let central_selector_start = CUMULATIVE_RUNTIME_LAYOUT.battle_central_right_fd_selector;
    let central_selector_end = CUMULATIVE_RUNTIME_LAYOUT.battle_right_fe_selector;
    let central_selector =
        active_fixed_bank_slice(&prior_rom, central_selector_start, central_selector_end)?;
    ensure!(
        central_selector.ends_with(&[0x68, 0x28, 0x4C, 0x80, 0xFB]),
        "battle-aware central selector no longer falls back to the roster selector"
    );
    let central_fallback_address = central_selector_end - 3;

    let mut expanded_base = inputs.prior_output.to_vec();
    expanded_base.extend_from_slice(&page.page_pack);
    ensure!(
        expanded_base.len() == inputs.prior_output.len() + 4 * FONT_PAGE_SIZE,
        "maximum dialogue stage must append two CHR banks"
    );
    let mut image = TrackedImage::new(expanded_base.clone());
    image.write_expected(
        "expand cumulative mapper 165 CHR from 25 to 27 banks",
        5,
        &[EXPECTED_INPUT_CHR_BANK_COUNT],
        &[OUTPUT_CHR_BANK_COUNT],
    )?;
    let record_end = inputs
        .record
        .source_file_offset
        .checked_add(page.encoded_record.len())
        .context("maximum dialogue record write range overflow")?;
    image.write_expected(
        "install page-group encoded maximum dialogue record",
        inputs.record.source_file_offset,
        inputs
            .prior_output
            .get(inputs.record.source_file_offset..record_end)
            .context("maximum dialogue record is outside cumulative output")?,
        &page.encoded_record,
    )?;
    image.write_expected(
        "maximum dialogue font-group selector",
        active_fixed_bank_file_offset(&prior_rom, FONT_GROUP_SELECTOR_ADDRESS)?,
        &vec![0xFF; font_group_selector.len()],
        &font_group_selector,
    )?;
    image.write_expected(
        "maximum dialogue initial page selector",
        active_fixed_bank_file_offset(&prior_rom, INITIAL_PAGE_SELECTOR_ADDRESS)?,
        &vec![0xFF; initial_selector.len()],
        &initial_selector,
    )?;
    image.write_expected(
        "scope completed-page font reload to the maximum dialogue lifetime",
        switchable_bank_file_offset(MAIN_DIALOGUE_PRG_BANK, MAXIMUM_DIALOGUE_PAGE_RELOAD_ADDRESS)?,
        &vec![0xFF; completed_page_reload.len()],
        &completed_page_reload,
    )?;
    image.write_expected(
        "route non-battle central font supply through maximum dialogue selector",
        active_fixed_bank_file_offset(&prior_rom, central_fallback_address)?,
        &[
            0x4C,
            ROSTER_SELECTOR_ADDRESS as u8,
            (ROSTER_SELECTOR_ADDRESS >> 8) as u8,
        ],
        &[
            0x4C,
            INITIAL_PAGE_SELECTOR_ADDRESS as u8,
            (INITIAL_PAGE_SELECTOR_ADDRESS >> 8) as u8,
        ],
    )?;
    image.write_expected(
        "reload maximum dialogue font on completed-page continuation",
        switchable_bank_file_offset(MAIN_DIALOGUE_PRG_BANK, COMPLETED_PAGE_CONTINUE_ADDRESS)?,
        &COMPLETED_PAGE_CONTINUE_SOURCE,
        &completed_page_hook,
    )?;
    image.verify_all_changes_tracked(&expanded_base)?;
    let tracked_write_count = image.writes().len();
    let output = image.into_data();
    verify_output(
        &prior_rom,
        &output,
        inputs.record,
        &page,
        &font_group_selector,
        &initial_selector,
        &completed_page_reload,
        &completed_page_hook,
        central_fallback_address,
    )?;

    Ok(MaximumDialogueStageOutput {
        output_sha1: sha1_hex(&output),
        output,
        page,
        tracked_write_count,
        initial_selector_byte_count: initial_selector.len(),
        font_group_selector_byte_count: font_group_selector.len(),
        completed_page_transition_byte_count: completed_page_hook.len(),
    })
}

fn validate_caves(source_rom: &Rom, prior_rom: &Rom) -> Result<()> {
    for (start, end, role) in [
        (
            FONT_GROUP_SELECTOR_ADDRESS,
            FONT_GROUP_SELECTOR_END,
            "font-group selector",
        ),
        (
            INITIAL_PAGE_SELECTOR_ADDRESS,
            INITIAL_PAGE_SELECTOR_CAVE_END,
            "initial page selector",
        ),
    ] {
        let source_start = fixed_bank_file_offset(start)?;
        let source_end = fixed_bank_file_offset(end)?;
        ensure!(
            source_rom.data()[source_start..source_end]
                .iter()
                .all(|byte| *byte == 0xFF),
            "maximum dialogue {role} source cave is no longer all FF"
        );
        ensure!(
            count_direct_transfers_to_range(source_rom.prg(), start, end)? == 0,
            "maximum dialogue {role} source cave gained a pre-existing direct transfer"
        );
        ensure!(
            active_fixed_bank_slice(prior_rom, start, end)?
                .iter()
                .all(|byte| *byte == 0xFF),
            "maximum dialogue {role} cumulative cave is already occupied"
        );
    }
    let reload_start =
        switchable_bank_file_offset(MAIN_DIALOGUE_PRG_BANK, MAXIMUM_DIALOGUE_PAGE_RELOAD_ADDRESS)?;
    let reload_end =
        switchable_bank_file_offset(MAIN_DIALOGUE_PRG_BANK, MAXIMUM_DIALOGUE_PAGE_RELOAD_END)?;
    ensure!(
        source_rom.data()[reload_start..reload_end]
            .iter()
            .all(|byte| *byte == 0xFF),
        "maximum dialogue page-reload source cave is no longer all FF"
    );
    ensure!(
        count_direct_transfers_to_range(
            source_rom.prg(),
            MAXIMUM_DIALOGUE_PAGE_RELOAD_ADDRESS,
            MAXIMUM_DIALOGUE_PAGE_RELOAD_END,
        )? == 0,
        "maximum dialogue page-reload source cave gained a pre-existing direct transfer"
    );
    ensure!(
        prior_rom.data()[reload_start..reload_end]
            .iter()
            .all(|byte| *byte == 0xFF),
        "maximum dialogue page-reload cumulative cave is already occupied"
    );
    Ok(())
}

#[allow(clippy::too_many_arguments)]
fn verify_output(
    input_rom: &Rom,
    output: &[u8],
    record: &MainDialogueSlicePlan,
    page: &MaximumDialoguePagePlan,
    font_group_selector: &[u8],
    initial_selector: &[u8],
    completed_page_reload: &[u8],
    completed_page_hook: &[u8],
    central_fallback_address: u16,
) -> Result<()> {
    let output_rom = Rom::parse(output.to_vec()).context("parse maximum dialogue stage")?;
    ensure!(
        output_rom.mapper() == OUTPUT_MAPPER
            && output_rom.prg().len() == input_rom.prg().len()
            && output_rom.data()[5] == OUTPUT_CHR_BANK_COUNT
            && output_rom.chr().len() == input_rom.chr().len() + page.page_pack.len(),
        "maximum dialogue output layout changed"
    );
    ensure!(
        output_rom.chr()[..input_rom.chr().len()] == *input_rom.chr()
            && output_rom.chr()[input_rom.chr().len()..] == *page.page_pack,
        "maximum dialogue output changed an earlier CHR page or its appended pages"
    );
    ensure!(
        output[record.source_file_offset..record.source_file_offset + page.encoded_record.len()]
            == *page.encoded_record,
        "maximum dialogue encoded record changed after installation"
    );
    for (address, expected, role) in [
        (
            FONT_GROUP_SELECTOR_ADDRESS,
            font_group_selector,
            "font-group selector",
        ),
        (
            INITIAL_PAGE_SELECTOR_ADDRESS,
            initial_selector,
            "initial page selector",
        ),
    ] {
        let offset = active_fixed_bank_file_offset(&output_rom, address)?;
        ensure!(
            output[offset..offset + expected.len()] == *expected,
            "maximum dialogue {role} changed after installation"
        );
    }
    let reload_offset =
        switchable_bank_file_offset(MAIN_DIALOGUE_PRG_BANK, MAXIMUM_DIALOGUE_PAGE_RELOAD_ADDRESS)?;
    ensure!(
        output[reload_offset..reload_offset + completed_page_reload.len()]
            == *completed_page_reload,
        "maximum dialogue completed-page ownership selector changed after installation"
    );
    let hook_offset =
        switchable_bank_file_offset(MAIN_DIALOGUE_PRG_BANK, COMPLETED_PAGE_CONTINUE_ADDRESS)?;
    ensure!(
        output[hook_offset..hook_offset + completed_page_hook.len()] == *completed_page_hook,
        "maximum dialogue completed-page hook changed after installation"
    );
    let fallback_offset = active_fixed_bank_file_offset(&output_rom, central_fallback_address)?;
    ensure!(
        output[fallback_offset..fallback_offset + 3]
            == [
                0x4C,
                INITIAL_PAGE_SELECTOR_ADDRESS as u8,
                (INITIAL_PAGE_SELECTOR_ADDRESS >> 8) as u8,
            ],
        "maximum dialogue initial selector is not in the central fallback chain"
    );
    Ok(())
}

fn active_fixed_bank_slice(rom: &Rom, start: u16, end: u16) -> Result<&[u8]> {
    ensure!(
        start >= 0xC000 && start <= end,
        "invalid active fixed-bank range"
    );
    let start_offset = active_fixed_bank_file_offset(rom, start)?;
    let end_offset = active_fixed_bank_file_offset(rom, end)?;
    rom.data()
        .get(start_offset..end_offset)
        .context("active fixed-bank range is outside cumulative ROM")
}

fn active_fixed_bank_file_offset(rom: &Rom, address: u16) -> Result<usize> {
    ensure!(
        address >= 0xC000,
        "active fixed-bank address is below 0xC000"
    );
    let fixed_start = rom
        .prg()
        .len()
        .checked_sub(FIXED_BANK_SIZE)
        .context("cumulative PRG is smaller than one fixed bank")?;
    Ok(HEADER_SIZE + fixed_start + usize::from(address - 0xC000))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn output_chr_growth_uses_two_whole_ines_banks() {
        assert_eq!(OUTPUT_CHR_BANK_COUNT - EXPECTED_INPUT_CHR_BANK_COUNT, 2);
        assert_eq!(4 * FONT_PAGE_SIZE, 2 * 8 * 1024);
    }
}
