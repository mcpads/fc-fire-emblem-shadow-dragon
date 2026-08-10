use std::path::Path;

use anyhow::{Context, Result, ensure};

use crate::{
    dialogue_assets::MainDialogueBundlePlan,
    font_slots::FONT_PAGE_SIZE,
    mmc5_prg::{count_direct_transfers_to_range, fixed_bank_file_offset},
    rom::Rom,
    sha1_hex,
    tracked::TrackedImage,
};

use super::super::{
    OUTPUT_MAPPER, front_end_page,
    shop_dialogue_page::{
        PAGE_ROUTINE_ADDRESS, PAGE_ROUTINE_CAVE_END, PAGE_ROUTINE_END, ShopDialoguePagePlan,
        build_page_selector, plan_shop_dialogue_page,
    },
    unit_name_page,
};
pub(super) struct ShopDialogueStageOutput {
    pub(super) output: Vec<u8>,
    pub(super) output_sha1: String,
    pub(super) page: ShopDialoguePagePlan,
    pub(super) tracked_write_count: usize,
}

pub(super) fn install_shop_dialogue_stage(
    class_profile_output: &[u8],
    source_rom: &Rom,
    plan: &MainDialogueBundlePlan,
    unit_ui_mapper_register: u8,
    evidence_path: &Path,
) -> Result<ShopDialogueStageOutput> {
    ensure!(
        !plan.record_ids.is_empty(),
        "weapon-shop dialogue plan is empty"
    );
    let class_profile_rom = Rom::parse(class_profile_output.to_vec())
        .context("parse class-profile cumulative stage")?;
    let physical_chr_page = u8::try_from(class_profile_rom.chr().len() / FONT_PAGE_SIZE)
        .context("weapon-shop physical CHR page does not fit u8")?;
    ensure!(
        physical_chr_page == 48 && physical_chr_page.is_multiple_of(2),
        "weapon-shop font page no longer begins at physical CHR page 48"
    );
    let page = plan_shop_dialogue_page(
        &class_profile_rom,
        evidence_path,
        &plan.unique_glyphs(),
        &plan.preserved_source_codes,
        physical_chr_page,
    )?;
    let encoded_bundle = plan.encoded(&page.assignments)?;

    let selector = build_page_selector(page.mapper_register, front_end_page::PAGE_ROUTINE_ADDRESS)?;
    let prior_unit_selector = unit_name_page::build_page_selector(
        unit_ui_mapper_register,
        front_end_page::PAGE_ROUTINE_ADDRESS,
    )?;
    let chained_unit_selector =
        unit_name_page::build_page_selector(unit_ui_mapper_register, PAGE_ROUTINE_ADDRESS)?;
    ensure!(
        prior_unit_selector.len() == chained_unit_selector.len(),
        "weapon-shop stage changed unit-UI selector size"
    );
    validate_selector_cave(source_rom)?;

    let mut expanded_base = class_profile_output.to_vec();
    expanded_base.extend_from_slice(&page.page_pack);
    ensure!(
        expanded_base.len() == class_profile_output.len() + 2 * FONT_PAGE_SIZE,
        "weapon-shop stage must append one 8 KiB CHR bank"
    );
    let mut image = TrackedImage::new(expanded_base.clone());
    image.write_expected(
        "expand cumulative mapper 165 CHR from 24 to 25 banks",
        5,
        &[24],
        &[25],
    )?;
    for region in &encoded_bundle.regions {
        image.write_expected(
            "repack weapon-shop dialogue source region",
            region.file_offset,
            &region.source_storage,
            &region.encoded_storage,
        )?;
    }
    for pointer in &encoded_bundle.pointer_writes {
        if pointer.source_pointer == pointer.planned_pointer {
            continue;
        }
        image.write_expected(
            format!(
                "repoint cumulative main dialogue record {}",
                pointer.record_id
            ),
            pointer.file_offset,
            &pointer.source_pointer.to_le_bytes(),
            &pointer.planned_pointer.to_le_bytes(),
        )?;
    }
    image.write_expected(
        "chain unit-UI selector through weapon-shop dialogue selector",
        fixed_bank_file_offset(unit_name_page::PAGE_ROUTINE_ADDRESS)?,
        &prior_unit_selector,
        &chained_unit_selector,
    )?;
    image.write_expected(
        "weapon-shop dialogue font-page selector",
        fixed_bank_file_offset(PAGE_ROUTINE_ADDRESS)?,
        &vec![0xFF; selector.len()],
        &selector,
    )?;
    image.verify_all_changes_tracked(&expanded_base)?;
    let tracked_write_count = image.writes().len();
    let output = image.into_data();
    let output_rom =
        Rom::parse(output.clone()).context("parse weapon-shop dialogue cumulative stage")?;
    ensure!(
        output_rom.mapper() == OUTPUT_MAPPER,
        "weapon-shop output mapper changed"
    );
    ensure!(
        output_rom.chr().len() == class_profile_rom.chr().len() + page.page_pack.len(),
        "weapon-shop output CHR size changed"
    );
    ensure!(
        output_rom.chr()[..class_profile_rom.chr().len()] == *class_profile_rom.chr(),
        "weapon-shop stage changed an earlier CHR page"
    );
    ensure!(
        output_rom.chr()[class_profile_rom.chr().len()..] == *page.page_pack,
        "weapon-shop output appended different page bytes"
    );
    for region in &encoded_bundle.regions {
        ensure!(
            output[region.file_offset..region.file_offset + region.encoded_storage.len()]
                == *region.encoded_storage,
            "weapon-shop output repacked region changed"
        );
    }
    for pointer in &encoded_bundle.pointer_writes {
        ensure!(
            output[pointer.file_offset..pointer.file_offset + 2]
                == pointer.planned_pointer.to_le_bytes(),
            "weapon-shop output pointer changed for {}",
            pointer.record_id
        );
    }
    let unit_selector_offset = fixed_bank_file_offset(unit_name_page::PAGE_ROUTINE_ADDRESS)?;
    ensure!(
        output[unit_selector_offset..unit_selector_offset + chained_unit_selector.len()]
            == *chained_unit_selector,
        "weapon-shop output bypasses its selector from the unit-UI chain"
    );
    let selector_offset = fixed_bank_file_offset(PAGE_ROUTINE_ADDRESS)?;
    ensure!(
        output[selector_offset..selector_offset + selector.len()] == *selector,
        "weapon-shop output selector changed"
    );

    Ok(ShopDialogueStageOutput {
        output_sha1: sha1_hex(&output),
        output,
        page,
        tracked_write_count,
    })
}

fn validate_selector_cave(source_rom: &Rom) -> Result<()> {
    let start = fixed_bank_file_offset(PAGE_ROUTINE_ADDRESS)?;
    let end = fixed_bank_file_offset(PAGE_ROUTINE_CAVE_END)?;
    ensure!(
        source_rom.data()[start..end]
            .iter()
            .all(|byte| *byte == 0xFF),
        "weapon-shop selector cave is no longer all FF"
    );
    ensure!(
        count_direct_transfers_to_range(
            source_rom.prg(),
            PAGE_ROUTINE_ADDRESS,
            PAGE_ROUTINE_CAVE_END,
        )? == 0,
        "weapon-shop selector cave gained a pre-existing direct transfer"
    );
    ensure!(
        PAGE_ROUTINE_END <= PAGE_ROUTINE_CAVE_END,
        "weapon-shop selector exceeds its checked cave"
    );
    Ok(())
}
