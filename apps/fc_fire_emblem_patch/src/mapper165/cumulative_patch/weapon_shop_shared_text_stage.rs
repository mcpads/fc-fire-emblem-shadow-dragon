use anyhow::{Context, Result, ensure};

use crate::{
    choice_labels::ChoiceLabelPlan,
    font_slots::FONT_PAGE_SIZE,
    mmc5_chr::switchable_bank_file_offset,
    mmc5_prg::{count_direct_transfers_to_range, fixed_bank_file_offset},
    rom::{CHR_FILE_OFFSET, Rom},
    sha1_hex,
    text_inventory::FixedTextPlan,
    tracked::TrackedImage,
};

use super::super::{
    OUTPUT_MAPPER,
    shop_dialogue_page::ShopDialoguePagePlan,
    weapon_shop_shared_text::{
        CHOICE_POINTER_LOAD_ADDRESS, CHOICE_POINTER_LOAD_BYTES, CHOICE_POINTER_LOAD_PRG_BANK,
        CHOICE_SELECTOR_ADDRESS, CODE_RANGES, ITEM_LIST_POINTER_LOAD_ADDRESS,
        ITEM_LIST_POINTER_LOAD_BYTES, ITEM_LIST_POINTER_LOAD_PRG_BANK, ITEM_LIST_SELECTOR_ADDRESS,
        ITEM_POINTER_TABLE_ADDRESS, SELECTED_ITEM_POINTER_LOAD_ADDRESS,
        SELECTED_ITEM_POINTER_LOAD_BYTES, SELECTED_ITEM_POINTER_LOAD_PRG_BANK,
        SELECTED_ITEM_SELECTOR_ADDRESS, WeaponShopSharedTextPlan, build_choice_pointer_load_call,
        build_choice_pointer_selector, build_item_list_pointer_load_call,
        build_item_list_pointer_selector, build_selected_item_pointer_load_call,
        build_selected_item_pointer_selector, build_weapon_shop_lifetime_identity_predicate,
        plan_weapon_shop_shared_text,
    },
};

pub(super) struct WeaponShopSharedTextStageOutput {
    pub(super) output: Vec<u8>,
    pub(super) output_sha1: String,
    pub(super) plan: WeaponShopSharedTextPlan,
    pub(super) tracked_write_count: usize,
}

pub(super) fn install_weapon_shop_shared_text_stage(
    dialogue_output: &[u8],
    source_rom: &Rom,
    dialogue_page: &ShopDialoguePagePlan,
    fixed_text: &FixedTextPlan,
    choice_labels: &ChoiceLabelPlan,
) -> Result<WeaponShopSharedTextStageOutput> {
    let dialogue_rom =
        Rom::parse(dialogue_output.to_vec()).context("parse weapon-shop dialogue stage")?;
    ensure!(
        dialogue_page.physical_chr_page == 48 && dialogue_page.mapper_register == 0xC0,
        "weapon-shop shared text lost its dialogue-page identity"
    );
    let page_offset = CHR_FILE_OFFSET
        + usize::from(dialogue_page.physical_chr_page)
            .checked_mul(FONT_PAGE_SIZE)
            .context("weapon-shop font page offset overflow")?;
    ensure!(
        dialogue_output[page_offset..page_offset + dialogue_page.page_pack.len()]
            == *dialogue_page.page_pack,
        "weapon-shop dialogue stage page changed before shared-text projection"
    );
    let plan = plan_weapon_shop_shared_text(source_rom, dialogue_page, fixed_text, choice_labels)?;
    let identity = build_weapon_shop_lifetime_identity_predicate()?;
    let item_list_selector = build_item_list_pointer_selector()?;
    let yes_index = choice_labels.entry("choice-label:yes")?.fixed_string_index;
    let no_index = choice_labels.entry("choice-label:no")?.fixed_string_index;
    let choice_selector = build_choice_pointer_selector(
        yes_index,
        plan.projection.yes_pointer,
        no_index,
        plan.projection.no_pointer,
    )?;
    let selected_item_selector = build_selected_item_pointer_selector()?;
    let item_list_call = build_item_list_pointer_load_call()?;
    let choice_call = build_choice_pointer_load_call()?;
    let selected_item_call = build_selected_item_pointer_load_call()?;
    validate_cave(
        source_rom,
        &identity,
        &item_list_selector,
        &choice_selector,
        &selected_item_selector,
        &plan,
    )?;

    let mut image = TrackedImage::new(dialogue_output.to_vec());
    image.write_expected(
        "extend weapon-shop dialogue page with shared item and choice glyphs",
        page_offset,
        &dialogue_page.page_pack,
        &plan.page.page_pack,
    )?;
    image.write_expected(
        "weapon-shop lifetime identity predicate",
        fixed_bank_file_offset(CODE_RANGES[0].0)?,
        &vec![0xFF; identity.len()],
        &identity,
    )?;
    image.write_expected(
        "weapon-shop item-list pointer selector",
        fixed_bank_file_offset(ITEM_LIST_SELECTOR_ADDRESS)?,
        &vec![0xFF; item_list_selector.len()],
        &item_list_selector,
    )?;
    image.write_expected(
        "weapon-shop choice-label pointer selector",
        fixed_bank_file_offset(CHOICE_SELECTOR_ADDRESS)?,
        &vec![0xFF; choice_selector.len()],
        &choice_selector,
    )?;
    image.write_expected(
        "weapon-shop selected-item pointer selector",
        fixed_bank_file_offset(SELECTED_ITEM_SELECTOR_ADDRESS)?,
        &vec![0xFF; selected_item_selector.len()],
        &selected_item_selector,
    )?;
    image.write_expected(
        "weapon-shop projected item pointer table",
        fixed_bank_file_offset(ITEM_POINTER_TABLE_ADDRESS)?,
        &vec![0xFF; plan.projection.item_pointer_table.len()],
        &plan.projection.item_pointer_table,
    )?;
    image.write_expected(
        "weapon-shop projected item and choice strings",
        fixed_bank_file_offset(super::super::weapon_shop_shared_text::STRING_DATA_ADDRESS)?,
        &vec![0xFF; plan.projection.strings.len()],
        &plan.projection.strings,
    )?;
    image.write_expected(
        "route weapon-shop item-list pointer load",
        switchable_bank_file_offset(
            ITEM_LIST_POINTER_LOAD_PRG_BANK,
            ITEM_LIST_POINTER_LOAD_ADDRESS,
        )?,
        &ITEM_LIST_POINTER_LOAD_BYTES,
        &item_list_call,
    )?;
    image.write_expected(
        "route weapon-shop choice-label pointer load",
        switchable_bank_file_offset(CHOICE_POINTER_LOAD_PRG_BANK, CHOICE_POINTER_LOAD_ADDRESS)?,
        &CHOICE_POINTER_LOAD_BYTES,
        &choice_call,
    )?;
    image.write_expected(
        "route weapon-shop selected-item pointer load",
        switchable_bank_file_offset(
            SELECTED_ITEM_POINTER_LOAD_PRG_BANK,
            SELECTED_ITEM_POINTER_LOAD_ADDRESS,
        )?,
        &SELECTED_ITEM_POINTER_LOAD_BYTES,
        &selected_item_call,
    )?;
    image.verify_all_changes_tracked(dialogue_output)?;
    let tracked_write_count = image.writes().len();
    let output = image.into_data();
    let installed_selectors = [
        (CODE_RANGES[0].0, identity.as_slice(), "identity predicate"),
        (
            ITEM_LIST_SELECTOR_ADDRESS,
            item_list_selector.as_slice(),
            "item-list selector",
        ),
        (
            CHOICE_SELECTOR_ADDRESS,
            choice_selector.as_slice(),
            "choice selector",
        ),
        (
            SELECTED_ITEM_SELECTOR_ADDRESS,
            selected_item_selector.as_slice(),
            "selected-item selector",
        ),
    ];
    let installed_hooks = [
        (
            ITEM_LIST_POINTER_LOAD_PRG_BANK,
            ITEM_LIST_POINTER_LOAD_ADDRESS,
            item_list_call.as_slice(),
            "item-list pointer hook",
        ),
        (
            CHOICE_POINTER_LOAD_PRG_BANK,
            CHOICE_POINTER_LOAD_ADDRESS,
            choice_call.as_slice(),
            "choice-label pointer hook",
        ),
        (
            SELECTED_ITEM_POINTER_LOAD_PRG_BANK,
            SELECTED_ITEM_POINTER_LOAD_ADDRESS,
            selected_item_call.as_slice(),
            "selected-item pointer hook",
        ),
    ];
    verify_output(
        &dialogue_rom,
        &output,
        &plan,
        &installed_selectors,
        &installed_hooks,
    )?;

    Ok(WeaponShopSharedTextStageOutput {
        output_sha1: sha1_hex(&output),
        output,
        plan,
        tracked_write_count,
    })
}

fn validate_cave(
    source_rom: &Rom,
    identity: &[u8],
    item_list_selector: &[u8],
    choice_selector: &[u8],
    selected_item_selector: &[u8],
    plan: &WeaponShopSharedTextPlan,
) -> Result<()> {
    for (start_address, end_address) in CODE_RANGES {
        let start = fixed_bank_file_offset(start_address)?;
        let end = fixed_bank_file_offset(end_address)?;
        ensure!(
            source_rom.data()[start..end]
                .iter()
                .all(|byte| *byte == 0xFF),
            "weapon-shop shared-text cave {start_address:04X}..{end_address:04X} is no longer all FF"
        );
        ensure!(
            count_direct_transfers_to_range(source_rom.prg(), start_address, end_address)? == 0,
            "weapon-shop shared-text cave {start_address:04X}..{end_address:04X} gained a pre-existing direct transfer"
        );
    }
    ensure!(
        usize::from(CODE_RANGES[0].0) + identity.len() <= usize::from(ITEM_LIST_SELECTOR_ADDRESS)
            && usize::from(ITEM_LIST_SELECTOR_ADDRESS) + item_list_selector.len()
                <= usize::from(CHOICE_SELECTOR_ADDRESS)
            && usize::from(CHOICE_SELECTOR_ADDRESS) + choice_selector.len()
                <= usize::from(SELECTED_ITEM_SELECTOR_ADDRESS)
            && usize::from(SELECTED_ITEM_SELECTOR_ADDRESS) + selected_item_selector.len()
                <= usize::from(ITEM_POINTER_TABLE_ADDRESS)
            && usize::from(ITEM_POINTER_TABLE_ADDRESS) + plan.projection.item_pointer_table.len()
                <= usize::from(super::super::weapon_shop_shared_text::STRING_DATA_ADDRESS)
            && usize::from(super::super::weapon_shop_shared_text::STRING_DATA_ADDRESS)
                + plan.projection.strings.len()
                <= usize::from(CODE_RANGES[1].1),
        "weapon-shop shared-text cave placements overlap"
    );
    Ok(())
}

fn verify_output(
    input_rom: &Rom,
    output: &[u8],
    plan: &WeaponShopSharedTextPlan,
    installed_selectors: &[(u16, &[u8], &str)],
    installed_hooks: &[(u8, u16, &[u8], &str)],
) -> Result<()> {
    let output_rom = Rom::parse(output.to_vec()).context("parse weapon-shop shared-text stage")?;
    ensure!(
        output_rom.mapper() == OUTPUT_MAPPER
            && output_rom.prg().len() == input_rom.prg().len()
            && output_rom.chr().len() == input_rom.chr().len(),
        "weapon-shop shared-text stage changed the ROM layout"
    );
    let page_offset = CHR_FILE_OFFSET + usize::from(plan.page.physical_chr_page) * FONT_PAGE_SIZE;
    ensure!(
        output[page_offset..page_offset + plan.page.page_pack.len()] == *plan.page.page_pack,
        "weapon-shop shared-text output page changed"
    );
    for &(address, expected, role) in installed_selectors {
        let offset = fixed_bank_file_offset(address)?;
        ensure!(
            output[offset..offset + expected.len()] == *expected,
            "weapon-shop shared-text {role} changed"
        );
    }
    for &(prg_bank, address, expected, role) in installed_hooks {
        let offset = switchable_bank_file_offset(prg_bank, address)?;
        ensure!(
            output[offset..offset + expected.len()] == *expected,
            "weapon-shop shared-text {role} changed"
        );
    }
    Ok(())
}
