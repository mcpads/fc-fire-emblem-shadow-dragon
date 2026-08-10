use std::path::Path;

use anyhow::{Context, Result, ensure};

use crate::{
    class_profile::ClassProfilePlan, font_slots::FONT_PAGE_SIZE,
    mmc5_chr::switchable_bank_file_offset, rom::Rom, sha1_hex, tracked::TrackedImage,
};

use super::super::{
    OUTPUT_MAPPER,
    class_profile_page::{
        ClassProfilePagePlan, PROFILE_PAGE_SELECTOR_ADDRESS, PROFILE_PAGE_SELECTOR_CAVE_END,
        SOURCE_TITLE_COMPOSER_PREFIX, TITLE_COMPOSER_HOOK_ADDRESS, build_profile_page_selector,
        build_title_composer_hook, plan_class_profile_pages,
    },
};

const SOURCE_PRG_BANK: u8 = 0x0D;

pub(super) struct ClassProfileStageOutput {
    pub(super) output: Vec<u8>,
    pub(super) output_sha1: String,
    pub(super) page: ClassProfilePagePlan,
    pub(super) encoded_titles: Vec<Vec<u8>>,
    pub(super) encoded_descriptions: Vec<Vec<u8>>,
    pub(super) tracked_write_count: usize,
}

pub(super) fn install_class_profile_stage(
    unit_name_output: &[u8],
    source_rom: &Rom,
    profiles: &ClassProfilePlan,
    evidence_path: &Path,
) -> Result<ClassProfileStageOutput> {
    let unit_name_rom =
        Rom::parse(unit_name_output.to_vec()).context("parse unit-name cumulative stage")?;
    let first_physical_page = u8::try_from(unit_name_rom.chr().len() / FONT_PAGE_SIZE)
        .context("class-profile physical CHR page does not fit u8")?;
    ensure!(
        first_physical_page == 46 && first_physical_page.is_multiple_of(2),
        "class-profile pages no longer begin at physical CHR page 46"
    );
    let page = plan_class_profile_pages(
        &unit_name_rom,
        source_rom,
        profiles,
        evidence_path,
        first_physical_page,
    )?;
    let encoded_titles = profiles
        .entries
        .iter()
        .map(|entry| entry.encoded_title_storage(page.assignments_for_profile(entry.profile_index)))
        .collect::<Result<Vec<_>>>()?;
    let encoded_descriptions = profiles
        .entries
        .iter()
        .map(|entry| {
            entry.encoded_description_storage(page.assignments_for_profile(entry.profile_index))
        })
        .collect::<Result<Vec<_>>>()?;
    let selector = build_profile_page_selector(page.mapper_registers)?;
    let hook = build_title_composer_hook()?;

    let selector_offset =
        switchable_bank_file_offset(SOURCE_PRG_BANK, PROFILE_PAGE_SELECTOR_ADDRESS)?;
    let cave_end_offset =
        switchable_bank_file_offset(SOURCE_PRG_BANK, PROFILE_PAGE_SELECTOR_CAVE_END)?;
    ensure!(
        source_rom.data()[selector_offset..cave_end_offset]
            .iter()
            .all(|byte| *byte == 0xFF),
        "class-profile page selector cave is no longer all FF"
    );

    let mut expanded_base = unit_name_output.to_vec();
    expanded_base.extend_from_slice(&page.page_pack);
    ensure!(
        expanded_base.len() == unit_name_output.len() + 2 * FONT_PAGE_SIZE,
        "class-profile stage must append one 8 KiB CHR bank"
    );
    let mut image = TrackedImage::new(expanded_base.clone());
    image.write_expected(
        "expand cumulative mapper 165 CHR from 23 to 24 banks",
        5,
        &[23],
        &[24],
    )?;
    for ((entry, encoded_title), encoded_description) in profiles
        .entries
        .iter()
        .zip(&encoded_titles)
        .zip(&encoded_descriptions)
    {
        image.write_expected(
            format!("cumulative {} title", entry.id),
            entry.title_file_offset,
            &unit_name_output[entry.title_file_offset
                ..entry.title_file_offset + entry.title_source_storage_byte_count],
            encoded_title,
        )?;
        image.write_expected(
            format!("cumulative {} description", entry.id),
            entry.description_file_offset,
            &unit_name_output[entry.description_file_offset
                ..entry.description_file_offset + entry.description_source_storage_byte_count],
            encoded_description,
        )?;
    }
    image.write_expected(
        "automatic class-profile page selector",
        selector_offset,
        &vec![0xFF; selector.len()],
        &selector,
    )?;
    image.write_expected(
        "route automatic class-profile title composition through its page selector",
        switchable_bank_file_offset(SOURCE_PRG_BANK, TITLE_COMPOSER_HOOK_ADDRESS)?,
        &SOURCE_TITLE_COMPOSER_PREFIX,
        &hook,
    )?;
    image.verify_all_changes_tracked(&expanded_base)?;
    let tracked_write_count = image.writes().len();
    let output = image.into_data();
    let output_rom = Rom::parse(output.clone()).context("parse class-profile cumulative stage")?;

    ensure!(
        output_rom.mapper() == OUTPUT_MAPPER,
        "class-profile output mapper changed"
    );
    ensure!(
        output_rom.chr().len() == unit_name_rom.chr().len() + page.page_pack.len(),
        "class-profile output CHR size changed"
    );
    ensure!(
        output_rom.chr()[..unit_name_rom.chr().len()] == *unit_name_rom.chr(),
        "class-profile stage changed an earlier CHR page"
    );
    ensure!(
        output_rom.chr()[unit_name_rom.chr().len()..] == *page.page_pack,
        "class-profile output appended different page bytes"
    );
    for ((entry, encoded_title), encoded_description) in profiles
        .entries
        .iter()
        .zip(&encoded_titles)
        .zip(&encoded_descriptions)
    {
        ensure!(
            output[entry.title_file_offset..entry.title_file_offset + encoded_title.len()]
                == *encoded_title,
            "class-profile output title changed for {}",
            entry.id
        );
        ensure!(
            output[entry.description_file_offset
                ..entry.description_file_offset + encoded_description.len()]
                == *encoded_description,
            "class-profile output description changed for {}",
            entry.id
        );
    }
    ensure!(
        output[selector_offset..selector_offset + selector.len()] == *selector,
        "class-profile output selector changed"
    );
    let hook_offset = switchable_bank_file_offset(SOURCE_PRG_BANK, TITLE_COMPOSER_HOOK_ADDRESS)?;
    ensure!(
        output[hook_offset..hook_offset + hook.len()] == *hook,
        "class-profile output title hook changed"
    );

    Ok(ClassProfileStageOutput {
        output_sha1: sha1_hex(&output),
        output,
        page,
        encoded_titles,
        encoded_descriptions,
        tracked_write_count,
    })
}
