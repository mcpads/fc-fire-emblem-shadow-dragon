use std::collections::BTreeSet;

use anyhow::{Context, Result, ensure};

use crate::{
    choice_labels::ChoiceLabelPlan,
    rom::Rom,
    text_inventory::{FixedTextPlan, FixedTextPlannedEntry},
};

use super::shop_dialogue_page::{ShopDialoguePagePlan, extend_shop_dialogue_page};

mod projection;
mod selectors;

pub(super) use selectors::{
    CHOICE_POINTER_LOAD_ADDRESS, CHOICE_POINTER_LOAD_BYTES, CHOICE_POINTER_LOAD_PRG_BANK,
    CHOICE_SELECTOR_ADDRESS, CODE_RANGES, ITEM_LIST_POINTER_LOAD_ADDRESS,
    ITEM_LIST_POINTER_LOAD_BYTES, ITEM_LIST_POINTER_LOAD_PRG_BANK, ITEM_LIST_SELECTOR_ADDRESS,
    SELECTED_ITEM_POINTER_LOAD_ADDRESS, SELECTED_ITEM_POINTER_LOAD_BYTES,
    SELECTED_ITEM_POINTER_LOAD_PRG_BANK, SELECTED_ITEM_SELECTOR_ADDRESS,
    build_choice_pointer_load_call, build_choice_pointer_selector,
    build_item_list_pointer_load_call, build_item_list_pointer_selector,
    build_selected_item_pointer_load_call, build_selected_item_pointer_selector,
    build_weapon_shop_lifetime_identity_predicate,
};

pub(super) use projection::{
    ITEM_POINTER_TABLE_ADDRESS, STRING_DATA_ADDRESS, WeaponShopTextProjection,
    build_weapon_shop_text_projection,
};

pub(super) const SCREEN_ROLE: &str = "weapon_shop_shared_text";
pub(super) const ITEM_NAME_SOURCE_INDICES: [usize; 6] = [1, 11, 14, 16, 18, 26];

pub(super) struct WeaponShopSharedTextPlan {
    pub(super) page: ShopDialoguePagePlan,
    pub(super) projection: WeaponShopTextProjection,
    pub(super) fixed_text_workspace_sha1: String,
    pub(super) choice_label_workspace_sha1: String,
    pub(super) review_complete: bool,
}

pub(super) fn plan_weapon_shop_shared_text(
    source_rom: &Rom,
    dialogue_page: &ShopDialoguePagePlan,
    fixed_text: &FixedTextPlan,
    choice_labels: &ChoiceLabelPlan,
) -> Result<WeaponShopSharedTextPlan> {
    let item_entries = selected_item_entries(fixed_text)?;
    let mut requested_glyphs = item_entries
        .iter()
        .flat_map(|entry| entry.unique_glyphs())
        .collect::<BTreeSet<_>>();
    requested_glyphs.extend(choice_labels.unique_glyphs());
    let page = extend_shop_dialogue_page(dialogue_page, &requested_glyphs)?;
    ensure!(
        requested_glyphs
            .iter()
            .all(|glyph| page.assignments.contains_key(glyph)),
        "weapon-shop shared-text page lost a requested glyph"
    );
    let projection = build_weapon_shop_text_projection(
        source_rom,
        &item_entries,
        choice_labels,
        &page.assignments,
    )?;

    Ok(WeaponShopSharedTextPlan {
        page,
        projection,
        fixed_text_workspace_sha1: fixed_text.workspace_sha1.clone(),
        choice_label_workspace_sha1: choice_labels.workspace_sha1.clone(),
        review_complete: fixed_text.review_complete && choice_labels.review_complete,
    })
}

fn selected_item_entries(fixed_text: &FixedTextPlan) -> Result<Vec<FixedTextPlannedEntry>> {
    ITEM_NAME_SOURCE_INDICES
        .iter()
        .map(|source_index| {
            fixed_text
                .entry_for_source_index("item-names", *source_index)
                .cloned()
                .with_context(|| {
                    format!("fixed-text plan lost weapon-shop item index {source_index}")
                })
        })
        .collect()
}
