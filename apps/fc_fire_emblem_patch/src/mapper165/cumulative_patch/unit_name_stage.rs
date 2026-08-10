use std::path::Path;

use anyhow::{Context, Result, ensure};

use crate::{
    font_slots::FONT_PAGE_SIZE,
    mmc5_chr::switchable_bank_file_offset,
    mmc5_prg::{count_direct_transfers_to_range, fixed_bank_file_offset},
    rom::{CHR_FILE_OFFSET, Rom},
    roster_localization::RosterLocalization,
    sha1_hex,
    tracked::TrackedImage,
    unit_names::UnitNamePlan,
};

use super::super::{
    front_end_page,
    roster_page::PHYSICAL_CHR_PAGES as ROSTER_PHYSICAL_CHR_PAGES,
    unit_name_page::{
        PAGE_ROUTINE_ADDRESS, PAGE_ROUTINE_END, UnitNamePagePlan, build_page_selector,
        plan_unit_name_pages,
    },
    unit_name_table::{
        CAVE_END_ADDRESS as TABLE_CAVE_END_ADDRESS, CAVE_START_ADDRESS as TABLE_CAVE_START_ADDRESS,
        PLAYER_POINTER_LOAD_ADDRESS, ROSTER_POINTER_TABLE_ADDRESS, ROSTER_STRING_DATA_ADDRESS,
        SELECTOR_ADDRESS, SOURCE_PLAYER_POINTER_LOAD, SOURCE_PRG_BANK,
        UNIT_UI_POINTER_TABLE_ADDRESS, UNIT_UI_STRING_DATA_ADDRESS, UnitNameTablePlan,
        plan_unit_name_tables,
    },
};
use super::{ROSTER_PAGE_REGISTERS, ROSTER_SELECTOR_ADDRESS, build_chained_roster_selector};

pub(super) struct UnitNameStageOutput {
    pub(super) output: Vec<u8>,
    pub(super) output_sha1: String,
    pub(super) page: UnitNamePagePlan,
    pub(super) tables: UnitNameTablePlan,
    pub(super) tracked_write_count: usize,
}

pub(super) fn install_unit_name_stage(
    front_end_output: &[u8],
    source_rom: &Rom,
    names: &UnitNamePlan,
    roster_localization_path: &Path,
    evidence_path: &Path,
) -> Result<UnitNameStageOutput> {
    let front_rom = Rom::parse(front_end_output.to_vec()).context("parse front-end stage")?;
    let unit_ui_physical_page = u8::try_from(front_rom.chr().len() / FONT_PAGE_SIZE)
        .context("unit-UI physical CHR page does not fit u8")?;
    ensure!(
        unit_ui_physical_page == 44 && unit_ui_physical_page.is_multiple_of(2),
        "unit-UI name page no longer begins at physical CHR page 44"
    );
    let roster_localization =
        RosterLocalization::from_path(roster_localization_path)?.validate()?;
    let page = plan_unit_name_pages(
        source_rom,
        evidence_path,
        &roster_localization,
        names,
        unit_ui_physical_page,
    )?;
    let tables = plan_unit_name_tables(names, &page.roster_assignments, &page.unit_ui_assignments)?;
    let page_selector = build_page_selector(
        page.unit_ui_mapper_register,
        front_end_page::PAGE_ROUTINE_ADDRESS,
    )?;
    ensure!(
        PAGE_ROUTINE_ADDRESS as usize + page_selector.len() == PAGE_ROUTINE_END as usize,
        "unit-UI selector no longer owns its exact cave span"
    );
    let prior_roster_selector = build_chained_roster_selector(
        ROSTER_PAGE_REGISTERS[0],
        ROSTER_PAGE_REGISTERS[1],
        front_end_page::PAGE_ROUTINE_ADDRESS,
    )?;
    let roster_selector = build_chained_roster_selector(
        ROSTER_PAGE_REGISTERS[0],
        ROSTER_PAGE_REGISTERS[1],
        PAGE_ROUTINE_ADDRESS,
    )?;
    ensure!(
        roster_selector.len() == prior_roster_selector.len(),
        "unit-name stage changed roster selector size"
    );

    validate_fixed_cave(source_rom, PAGE_ROUTINE_ADDRESS, PAGE_ROUTINE_END)?;
    validate_switchable_cave(
        source_rom,
        SOURCE_PRG_BANK,
        TABLE_CAVE_START_ADDRESS,
        TABLE_CAVE_END_ADDRESS,
    )?;

    let mut expanded_base = front_end_output.to_vec();
    expanded_base.extend_from_slice(&page.unit_ui_page_pack);
    ensure!(
        expanded_base.len() == front_end_output.len() + 2 * FONT_PAGE_SIZE,
        "unit-name stage must append one 8 KiB CHR bank"
    );
    let mut image = TrackedImage::new(expanded_base.clone());
    image.write_expected(
        "expand cumulative mapper 165 CHR from 22 to 23 banks",
        5,
        &[22],
        &[23],
    )?;
    let roster_page_offset =
        CHR_FILE_OFFSET + usize::from(ROSTER_PHYSICAL_CHR_PAGES[0]) * FONT_PAGE_SIZE;
    image.write_expected(
        "replace roster proof pages with translated unit-name pages",
        roster_page_offset,
        &front_end_output[roster_page_offset..roster_page_offset + 2 * FONT_PAGE_SIZE],
        &page.roster_page_pack,
    )?;
    image.write_expected(
        "chain roster selector through unit-UI name selector",
        fixed_bank_file_offset(ROSTER_SELECTOR_ADDRESS)?,
        &prior_roster_selector,
        &roster_selector,
    )?;
    image.write_expected(
        "unit-UI translated-name page selector",
        fixed_bank_file_offset(PAGE_ROUTINE_ADDRESS)?,
        &vec![0xFF; page_selector.len()],
        &page_selector,
    )?;
    image.write_expected(
        "select consumer-specific playable-unit name table",
        switchable_bank_file_offset(SOURCE_PRG_BANK, SELECTOR_ADDRESS)?,
        &vec![0xFF; tables.selector.len()],
        &tables.selector,
    )?;
    image.write_expected(
        "route playable-unit pointer load through the consumer selector",
        switchable_bank_file_offset(SOURCE_PRG_BANK, PLAYER_POINTER_LOAD_ADDRESS)?,
        &SOURCE_PLAYER_POINTER_LOAD,
        &tables.selector_call,
    )?;
    write_projection(
        &mut image,
        "roster playable-unit name pointers",
        SOURCE_PRG_BANK,
        ROSTER_POINTER_TABLE_ADDRESS,
        &tables.roster.pointer_table,
    )?;
    write_projection(
        &mut image,
        "roster playable-unit name strings",
        SOURCE_PRG_BANK,
        ROSTER_STRING_DATA_ADDRESS,
        &tables.roster.strings,
    )?;
    write_projection(
        &mut image,
        "unit-UI playable-unit name pointers",
        SOURCE_PRG_BANK,
        UNIT_UI_POINTER_TABLE_ADDRESS,
        &tables.unit_ui.pointer_table,
    )?;
    write_projection(
        &mut image,
        "unit-UI playable-unit name strings",
        SOURCE_PRG_BANK,
        UNIT_UI_STRING_DATA_ADDRESS,
        &tables.unit_ui.strings,
    )?;
    image.verify_all_changes_tracked(&expanded_base)?;
    let tracked_write_count = image.writes().len();
    let output = image.into_data();
    let output_rom = Rom::parse(output.clone()).context("parse unit-name cumulative stage")?;
    ensure!(
        output_rom.chr().len() == front_rom.chr().len() + page.unit_ui_page_pack.len(),
        "unit-name output CHR size changed"
    );
    ensure!(
        output_rom.chr()[..usize::from(ROSTER_PHYSICAL_CHR_PAGES[0]) * FONT_PAGE_SIZE]
            == front_rom.chr()[..usize::from(ROSTER_PHYSICAL_CHR_PAGES[0]) * FONT_PAGE_SIZE],
        "unit-name stage changed CHR before the roster pages"
    );
    ensure!(
        output_rom.chr()[usize::from(ROSTER_PHYSICAL_CHR_PAGES[0]) * FONT_PAGE_SIZE
            ..usize::from(ROSTER_PHYSICAL_CHR_PAGES[0]) * FONT_PAGE_SIZE + 2 * FONT_PAGE_SIZE]
            == *page.roster_page_pack,
        "unit-name output roster pages changed"
    );
    ensure!(
        output_rom.chr()[front_rom.chr().len()..] == *page.unit_ui_page_pack,
        "unit-name output appended page pack changed"
    );

    Ok(UnitNameStageOutput {
        output_sha1: sha1_hex(&output),
        output,
        page,
        tables,
        tracked_write_count,
    })
}

fn validate_fixed_cave(source_rom: &Rom, start: u16, end: u16) -> Result<()> {
    let start_offset = fixed_bank_file_offset(start)?;
    let end_offset = fixed_bank_file_offset(end)?;
    ensure!(
        source_rom.data()[start_offset..end_offset]
            .iter()
            .all(|byte| *byte == 0xFF),
        "unit-UI selector cave is no longer all FF"
    );
    ensure!(
        count_direct_transfers_to_range(source_rom.prg(), start, end)? == 0,
        "unit-UI selector cave gained a pre-existing direct transfer"
    );
    Ok(())
}

fn validate_switchable_cave(rom: &Rom, bank: u8, start: u16, end: u16) -> Result<()> {
    let start_offset = switchable_bank_file_offset(bank, start)?;
    let end_offset = switchable_bank_file_offset(bank, end)?;
    ensure!(
        rom.data()[start_offset..end_offset]
            .iter()
            .all(|byte| *byte == 0xFF),
        "unit-name table cave is no longer all FF"
    );
    Ok(())
}

fn write_projection(
    image: &mut TrackedImage,
    label: &str,
    bank: u8,
    address: u16,
    replacement: &[u8],
) -> Result<()> {
    image.write_expected(
        label,
        switchable_bank_file_offset(bank, address)?,
        &vec![0xFF; replacement.len()],
        replacement,
    )?;
    Ok(())
}
