use std::{collections::BTreeSet, path::Path};

use anyhow::{Context, Result, ensure};

use crate::{
    font_slots::FONT_PAGE_SIZE,
    front_end_menu::FrontEndMenuPlan,
    mmc5_prg::{count_direct_transfers_to_range, fixed_bank_file_offset},
    rom::Rom,
    sha1_hex,
    tracked::TrackedImage,
};

use super::{
    DIALOGUE_FONT_PAGE_SELECTOR_ADDRESS, OUTPUT_MAPPER, ROSTER_PAGE_REGISTERS,
    ROSTER_SELECTOR_ADDRESS, build_chained_roster_selector,
};
use crate::mapper165::SELECT_RIGHT_FD_CHR_BANK_FOR_PAIR_ADDRESS;
use crate::mapper165::font_pair_projection::{
    WRITE_TRANSLATED_CHR_PAGE_ADDRESS, build_translated_chr_page_writer,
};
use crate::mapper165::front_end_page::{
    FrontEndPagePlan, PAGE_ROUTINE_ADDRESS, PAGE_ROUTINE_END, build_page_selector,
    plan_front_end_page,
};
use crate::mapper165::roster_page::{
    CENTRAL_RIGHT_FE_COMPANION_FD_REFRESH_CALL_ADDRESS, central_right_fe_companion_fd_refresh_call,
};

pub(super) struct FrontEndStageOutput {
    pub(super) output: Vec<u8>,
    pub(super) output_sha1: String,
    pub(super) page: FrontEndPagePlan,
    pub(super) encoded_entries: Vec<Vec<u8>>,
    pub(super) tracked_write_count: usize,
}

pub(super) fn install_front_end_stage(
    chapter_two_output: &[u8],
    source_rom: &Rom,
    menu_plan: &FrontEndMenuPlan,
    result_dialogue_preserved_codes: &BTreeSet<u8>,
    evidence_path: &Path,
    prior_roster_selector: &[u8],
    dialogue_selector: &[u8],
) -> Result<FrontEndStageOutput> {
    let chapter_two_rom =
        Rom::parse(chapter_two_output.to_vec()).context("parse Chapter 2 cumulative stage")?;
    let physical_chr_page = u8::try_from(chapter_two_rom.chr().len() / FONT_PAGE_SIZE)
        .context("front-end physical CHR page does not fit u8")?;
    ensure!(
        physical_chr_page == 42 && physical_chr_page.is_multiple_of(2),
        "front-end font page no longer begins at physical CHR page 42"
    );
    let page = plan_front_end_page(
        &chapter_two_rom,
        source_rom,
        evidence_path,
        &menu_plan.unique_glyphs(),
        &menu_plan.preserved_source_codes(),
        result_dialogue_preserved_codes,
        physical_chr_page,
    )?;
    let encoded_entries = menu_plan
        .entries
        .iter()
        .map(|entry| entry.encoded_storage_bytes(&page.assignments))
        .collect::<Result<Vec<_>>>()?;
    let selector = build_page_selector(page.mapper_register, DIALOGUE_FONT_PAGE_SELECTOR_ADDRESS)?;
    let translated_page_writer = build_translated_chr_page_writer()?;
    let roster_selector = build_chained_roster_selector(
        ROSTER_PAGE_REGISTERS[0],
        ROSTER_PAGE_REGISTERS[1],
        PAGE_ROUTINE_ADDRESS,
    )?;
    ensure!(
        roster_selector.len() == prior_roster_selector.len(),
        "front-end selector chaining changed roster routine size"
    );

    let selector_offset = fixed_bank_file_offset(PAGE_ROUTINE_ADDRESS)?;
    ensure!(
        source_rom.data()[selector_offset..selector_offset + selector.len()]
            .iter()
            .all(|byte| *byte == 0xFF),
        "front-end selector cave is no longer all FF"
    );
    ensure!(
        PAGE_ROUTINE_ADDRESS as usize + selector.len() == PAGE_ROUTINE_END as usize,
        "front-end selector no longer owns its exact cave span"
    );
    ensure!(
        count_direct_transfers_to_range(source_rom.prg(), PAGE_ROUTINE_ADDRESS, PAGE_ROUTINE_END,)?
            == 0,
        "front-end selector cave gained a pre-existing direct transfer"
    );

    let mut expanded_base = chapter_two_output.to_vec();
    expanded_base.extend_from_slice(&page.page_pack);
    ensure!(
        expanded_base.len() == chapter_two_output.len() + 2 * FONT_PAGE_SIZE,
        "front-end stage must append one 8 KiB CHR bank"
    );
    let mut image = TrackedImage::new(expanded_base.clone());
    image.write_expected(
        "expand cumulative mapper 165 CHR from 21 to 22 banks",
        5,
        &[21],
        &[22],
    )?;
    for (entry, encoded) in menu_plan.entries.iter().zip(&encoded_entries) {
        ensure!(
            encoded.len() == entry.source_storage_byte_count,
            "front-end menu entry changed its owned storage size"
        );
        image.write_expected(
            format!("cumulative {}", entry.id),
            entry.file_offset,
            &chapter_two_output[entry.file_offset..entry.file_offset + encoded.len()],
            encoded,
        )?;
    }
    image.write_expected(
        "chain roster selector through front-end menu selector",
        fixed_bank_file_offset(ROSTER_SELECTOR_ADDRESS)?,
        prior_roster_selector,
        &roster_selector,
    )?;
    image.write_expected(
        "shared translated FD/FE CHR page writer",
        fixed_bank_file_offset(WRITE_TRANSLATED_CHR_PAGE_ADDRESS)?,
        &vec![0xFF; translated_page_writer.len()],
        &translated_page_writer,
    )?;
    image.write_expected(
        "front-end menu font-page selector",
        selector_offset,
        &vec![0xFF; selector.len()],
        &selector,
    )?;
    image.write_expected(
        "route central FE companion FD refresh through the screen-lifetime selector chain",
        fixed_bank_file_offset(CENTRAL_RIGHT_FE_COMPANION_FD_REFRESH_CALL_ADDRESS)?,
        &central_right_fe_companion_fd_refresh_call(SELECT_RIGHT_FD_CHR_BANK_FOR_PAIR_ADDRESS)?,
        &central_right_fe_companion_fd_refresh_call(ROSTER_SELECTOR_ADDRESS)?,
    )?;
    image.verify_all_changes_tracked(&expanded_base)?;
    let tracked_write_count = image.writes().len();
    let output = image.into_data();
    let output_rom = Rom::parse(output.clone()).context("parse front-end cumulative stage")?;
    ensure!(
        output_rom.mapper() == OUTPUT_MAPPER,
        "front-end output mapper changed"
    );
    ensure!(
        output_rom.chr().len() == chapter_two_rom.chr().len() + page.page_pack.len(),
        "front-end output CHR size changed"
    );
    ensure!(
        output_rom.chr()[..chapter_two_rom.chr().len()] == *chapter_two_rom.chr(),
        "front-end stage changed an earlier CHR page"
    );
    ensure!(
        output_rom.chr()[chapter_two_rom.chr().len()..] == *page.page_pack,
        "front-end stage appended different page bytes"
    );
    for (entry, encoded) in menu_plan.entries.iter().zip(&encoded_entries) {
        ensure!(
            output[entry.file_offset..entry.file_offset + encoded.len()] == *encoded,
            "front-end output entry changed for {}",
            entry.id
        );
    }
    let roster_offset = fixed_bank_file_offset(ROSTER_SELECTOR_ADDRESS)?;
    ensure!(
        output[roster_offset..roster_offset + roster_selector.len()] == *roster_selector,
        "front-end output roster selector changed"
    );
    ensure!(
        output[selector_offset..selector_offset + selector.len()] == *selector,
        "front-end output page selector changed"
    );
    let companion_refresh_offset =
        fixed_bank_file_offset(CENTRAL_RIGHT_FE_COMPANION_FD_REFRESH_CALL_ADDRESS)?;
    let companion_refresh = central_right_fe_companion_fd_refresh_call(ROSTER_SELECTOR_ADDRESS)?;
    ensure!(
        output[companion_refresh_offset..companion_refresh_offset + companion_refresh.len()]
            == *companion_refresh,
        "front-end output central FE companion FD refresh bypasses the lifetime selector chain"
    );
    let translated_page_writer_offset = fixed_bank_file_offset(WRITE_TRANSLATED_CHR_PAGE_ADDRESS)?;
    ensure!(
        output[translated_page_writer_offset
            ..translated_page_writer_offset + translated_page_writer.len()]
            == *translated_page_writer,
        "front-end output changed the shared translated CHR page writer"
    );
    let dialogue_offset = fixed_bank_file_offset(DIALOGUE_FONT_PAGE_SELECTOR_ADDRESS)?;
    ensure!(
        output[dialogue_offset..dialogue_offset + dialogue_selector.len()] == *dialogue_selector,
        "front-end stage changed the chapter dialogue selector"
    );

    Ok(FrontEndStageOutput {
        output_sha1: sha1_hex(&output),
        output,
        page,
        encoded_entries,
        tracked_write_count,
    })
}
