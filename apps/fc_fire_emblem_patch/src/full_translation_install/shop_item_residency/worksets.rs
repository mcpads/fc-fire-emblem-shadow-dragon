//! 시설 재고가 대사 레코드 밖에서 합성하는 품목 글리프를 페이지 작업집합에 결속한다.

use std::collections::BTreeSet;

use anyhow::{Result, ensure};

use super::{ShopItemWorksetResidencyInputs, ShopItemWorksetResidencyPlan};
use crate::{
    full_translation_install::dialogue_item_worksets::{
        DialogueItemWorksetInputs, augment_dialogue_item_worksets,
    },
    shop_flow::{SHOP_ITEM_ENTRY_COUNT, bind_shop_item_composition_source},
};

pub(in crate::full_translation_install) fn plan_shop_item_workset_residency(
    inputs: ShopItemWorksetResidencyInputs<'_>,
) -> Result<ShopItemWorksetResidencyPlan> {
    let source = bind_shop_item_composition_source(inputs.source)?;
    ensure!(
        inputs
            .fixed
            .entries
            .iter()
            .filter(|entry| entry.table_id == "item-names")
            .count()
            == SHOP_ITEM_ENTRY_COUNT,
        "shop item worksets lost the 91-entry translated item-name population"
    );
    let target_record_ids = source
        .dialogue_lifetime_record_indices()
        .iter()
        .map(|index| format!("shop-and-item-dialogue:{index:03}"))
        .collect::<BTreeSet<_>>();
    let augmentation = augment_dialogue_item_worksets(DialogueItemWorksetInputs {
        role: "shop item residency",
        display: inputs.display,
        fixed: inputs.fixed,
        dialogue_worksets: inputs.dialogue_worksets,
        canonical_item_codes: inputs.canonical_dynamic_codes,
        item_name_appender_display_codes: inputs.item_name_appender_display_codes,
        item_source_indices: source.item_source_indices(),
        target_record_ids: &target_record_ids,
    })?;

    Ok(ShopItemWorksetResidencyPlan {
        augmented_worksets: augmentation.augmented_worksets,
        outer_state_address: source.outer_state_address(),
        composition_state: source.composition_state(),
        composite_state: source.composite_state(),
        selected_facility_address: source.selected_facility_address(),
        dialogue_directory_address: source.dialogue_directory_address(),
        dialogue_directory_selector: source.dialogue_directory_selector(),
        selling_facilities: source.selling_facilities(),
        non_selling_facilities: source.non_selling_facilities(),
        stock_group_count: source.stock_group_ids().len(),
        stocked_item_entry_count: source.item_source_indices().len(),
        target_record_count: augmentation.target_record_count,
        target_record_ids,
        target_workset_count: augmentation.target_workset_count,
        stocked_item_glyph_count: augmentation.item_glyph_count,
        preserved_item_code_count: augmentation.preserved_item_code_count,
        maximum_augmented_workset_slot_demand: augmentation.maximum_augmented_workset_slot_demand,
        every_stocked_item_uses_canonical_code: true,
        every_augmented_workset_fits: true,
    })
}
