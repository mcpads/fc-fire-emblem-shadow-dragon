use anyhow::{Result, ensure};

use crate::rom::Rom;
use crate::{
    dialogue_inventory::switchable_cpu_to_file_offset, mapper165::FontPageFallbackNodeRole,
};

use super::{
    super::{
        chapter_save_projection::ChapterSaveProjectionPlan,
        ending_record_projection::EndingRecordProjectionPlan,
        fixed_ui_projection::FixedUiProjectionPlan,
        screen_font_residency::FontPageSelectorForwarderPlan,
        shop_text_consumers::ShopTextConsumerPlan,
    },
    FIXED_BANK_SIZE,
    technical_installation::IntegratedImage,
};

pub(super) fn fixed_file_offset(rom: &Rom, address: u16) -> Result<usize> {
    ensure!(address >= 0xC000, "fixed-bank address is below C000");
    let base = rom
        .prg()
        .len()
        .checked_sub(FIXED_BANK_SIZE)
        .ok_or_else(|| anyhow::anyhow!("PRG is smaller than one fixed bank"))?;
    Ok(crate::rom::HEADER_SIZE + base + usize::from(address) - 0xC000)
}

pub(super) fn install_fixed_ui_projection(
    image: &mut IntegratedImage,
    plan: &FixedUiProjectionPlan,
) -> Result<()> {
    ensure!(
        plan.write_count() == 87,
        "fixed UI projection must install thirty-six slots, thirty-six pointers, six unit-selection help lines, six map-menu labels, two map funds-summary labels, and one roster-header codebook projection"
    );
    for write in plan.writes() {
        image.write_expected(
            &write.role,
            write.file_offset,
            &write.expected,
            &write.replacement,
        )?;
    }
    Ok(())
}

pub(super) fn verify_installed_fixed_ui_projection(
    installed: &[u8],
    plan: &FixedUiProjectionPlan,
) -> Result<()> {
    for write in plan.writes() {
        ensure!(
            installed.get(write.file_offset..write.file_offset + write.replacement.len())
                == Some(write.replacement.as_slice()),
            "installed fixed UI projection does not match {}",
            write.role
        );
    }
    Ok(())
}

pub(super) fn install_chapter_save_projection(
    image: &mut IntegratedImage,
    plan: &ChapterSaveProjectionPlan,
) -> Result<()> {
    ensure!(
        plan.write_count() == 3,
        "chapter-save projection must install the save question and both choices"
    );
    for write in plan.writes() {
        image.write_expected(
            &write.role,
            write.file_offset,
            &write.expected,
            &write.replacement,
        )?;
    }
    Ok(())
}

pub(super) fn verify_installed_chapter_save_projection(
    installed: &[u8],
    plan: &ChapterSaveProjectionPlan,
) -> Result<()> {
    for write in plan.writes() {
        ensure!(
            installed.get(write.file_offset..write.file_offset + write.replacement.len())
                == Some(write.replacement.as_slice()),
            "installed chapter-save projection does not match {}",
            write.role
        );
    }
    Ok(())
}

pub(super) fn install_ending_record_projection(
    image: &mut IntegratedImage,
    plan: &EndingRecordProjectionPlan,
) -> Result<()> {
    ensure!(
        plan.write_count() == 52,
        "ending-record projection must install twenty-five title spans, twenty-five turn suffixes, and one aggregate label"
    );
    for write in plan.writes() {
        image.write_expected(
            &write.role,
            write.file_offset,
            &write.expected,
            &write.replacement,
        )?;
    }
    Ok(())
}

pub(super) fn install_font_page_selector_forwarders(
    image: &mut IntegratedImage,
    candidate: &Rom,
    plan: &FontPageSelectorForwarderPlan,
) -> Result<()> {
    ensure!(
        plan.write_count() != 0
            && plan.replaces_source_role(FontPageFallbackNodeRole::WeaponShopDialogue)
            && plan.replaces_source_role(FontPageFallbackNodeRole::ChapterIntroDialogue),
        "screen residency did not replace every obsolete final-codebook selector"
    );
    for write in plan.writes() {
        ensure!(
            write.file_offset == fixed_file_offset(candidate, write.cpu_address)?,
            "font-page selector forwarder file and CPU addresses disagree"
        );
        image.write_expected(
            write.role,
            write.file_offset,
            &write.expected,
            &write.replacement,
        )?;
    }
    Ok(())
}

pub(super) fn install_shop_text_consumers(
    image: &mut IntegratedImage,
    plan: &ShopTextConsumerPlan,
) -> Result<()> {
    ensure!(
        plan.write_count() != 0,
        "shop text consumer ownership has no pointer restorations"
    );
    for write in plan.writes() {
        ensure!(
            write.file_offset == switchable_cpu_to_file_offset(write.prg_bank, write.cpu_address)?,
            "shop text consumer file and CPU addresses disagree"
        );
        image.write_expected(
            write.role,
            write.file_offset,
            &write.expected,
            &write.replacement,
        )?;
    }
    Ok(())
}

pub(super) fn verify_installed_shop_text_consumers(
    installed: &[u8],
    plan: &ShopTextConsumerPlan,
) -> Result<()> {
    for write in plan.writes() {
        ensure!(
            write.file_offset == switchable_cpu_to_file_offset(write.prg_bank, write.cpu_address)?
                && installed.get(write.file_offset..write.file_offset + write.replacement.len())
                    == Some(write.replacement.as_slice()),
            "installed shop text consumer ownership does not match {}",
            write.role
        );
    }
    Ok(())
}

pub(super) fn verify_installed_font_page_selector_forwarders(
    installed: &[u8],
    candidate: &Rom,
    plan: &FontPageSelectorForwarderPlan,
) -> Result<()> {
    for write in plan.writes() {
        ensure!(
            write.file_offset == fixed_file_offset(candidate, write.cpu_address)?
                && installed.get(write.file_offset..write.file_offset + write.replacement.len())
                    == Some(write.replacement.as_slice()),
            "installed font-page selector forwarder does not match {}",
            write.role
        );
    }
    plan.verify_retained_dynamic_selectors(installed, candidate)?;
    Ok(())
}

pub(super) fn verify_installed_ending_record_projection(
    installed: &[u8],
    plan: &EndingRecordProjectionPlan,
) -> Result<()> {
    for write in plan.writes() {
        ensure!(
            installed.get(write.file_offset..write.file_offset + write.replacement.len())
                == Some(write.replacement.as_slice()),
            "installed ending-record projection does not match {}",
            write.role
        );
    }
    Ok(())
}
