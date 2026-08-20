//! 상점 재고 생산자와 대사 글꼴 작업집합의 공동 수명 계약이다.
//!
//! 상점 품목명은 대사 `{EC}` 레코드 안에 있지 않고 시설 재고표에서 별도로 합성된다.
//! 따라서 동적 대사만 스캔하면 저장 바이트는 올바른 한글이어도 현재 대사 페이지에
//! 그 글리프가 없어 가블이 난다. 이 모듈은 시설 레코드에서 실제 판매 재고를 유도해
//! 모든 상점 대사 수명에 같은 canonical 아이템 코드를 추가하고, 그 결과를 런타임
//! 카탈로그·E7 페이지 선택 계약까지 이어 준다.

use std::collections::BTreeMap;

use anyhow::{Result, ensure};
use serde::Serialize;

use super::{
    consumer_catalog::{ConsumerCatalogRuntimeLayout, ConsumerCatalogRuntimeMaterialPlan},
    dynamic_inputs::DynamicProducerEncodingPlan,
    runtime_code::DialogueRuntimeCodePlan,
};
use crate::{
    dialogue_assets::MainDialogueDisplayPlan, mapper165::battle_codebook_plan::GlyphWorkset,
    rom::Rom, shop_flow::SHOP_ITEM_ENTRY_COUNT, text_inventory::FixedTextPlan,
};

mod worksets;

pub(super) use worksets::plan_shop_item_workset_residency;

pub(super) struct ShopItemWorksetResidencyInputs<'a> {
    pub(super) source: &'a Rom,
    pub(super) display: &'a MainDialogueDisplayPlan,
    pub(super) fixed: &'a FixedTextPlan,
    pub(super) dialogue_worksets: &'a [GlyphWorkset],
    pub(super) canonical_dynamic_codes: &'a BTreeMap<char, u8>,
}

pub(super) struct ShopItemWorksetResidencyPlan {
    pub(super) augmented_worksets: Vec<GlyphWorkset>,
    outer_state_address: u16,
    composition_state: u8,
    composite_state: u8,
    selected_facility_address: u16,
    dialogue_directory_address: u16,
    dialogue_directory_selector: u8,
    selling_facilities: [u8; 3],
    non_selling_facilities: [u8; 2],
    stock_group_count: usize,
    stocked_item_entry_count: usize,
    target_record_count: usize,
    target_workset_count: usize,
    stocked_item_glyph_count: usize,
    preserved_item_code_count: usize,
    maximum_augmented_workset_slot_demand: usize,
    every_stocked_item_uses_canonical_code: bool,
    every_augmented_workset_fits: bool,
}

pub(super) struct ShopItemResidencyInputs<'a> {
    pub(super) workset_residency: &'a ShopItemWorksetResidencyPlan,
    pub(super) dynamic_producer_encoding: &'a DynamicProducerEncodingPlan,
    pub(super) consumer_catalog_runtime: &'a ConsumerCatalogRuntimeMaterialPlan,
    pub(super) producer_material_page: u8,
    pub(super) producer_material_base: u16,
    pub(super) producer_item_directory: u16,
    pub(super) consumer_catalog_layout: ConsumerCatalogRuntimeLayout,
    pub(super) e7_caller_resume_flag_address: u16,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(in crate::full_translation_install) struct ShopItemResidencyRuntimeContract {
    pub(in crate::full_translation_install) outer_state_address: u16,
    pub(in crate::full_translation_install) composition_state: u8,
    pub(in crate::full_translation_install) composite_state: u8,
    pub(in crate::full_translation_install) selected_facility_address: u16,
    pub(in crate::full_translation_install) dialogue_directory_address: u16,
    pub(in crate::full_translation_install) dialogue_directory_selector: u8,
    pub(in crate::full_translation_install) e7_caller_resume_flag_address: u16,
    pub(in crate::full_translation_install) selling_facilities: [u8; 3],
    pub(in crate::full_translation_install) non_selling_facilities: [u8; 2],
    pub(in crate::full_translation_install) dialogue_material_page: u8,
    pub(in crate::full_translation_install) dialogue_material_base: u16,
    pub(in crate::full_translation_install) dialogue_item_directory: u16,
    pub(in crate::full_translation_install) catalog_material_page: u8,
    pub(in crate::full_translation_install) catalog_material_base: u16,
    pub(in crate::full_translation_install) catalog_item_directory: u16,
}

#[derive(Serialize)]
pub(super) struct ShopItemResidencyPlan {
    schema: u8,
    strategy: &'static str,
    source_composition_state: u8,
    source_composite_state: u8,
    item_selling_facilities: [u8; 3],
    non_item_selling_facilities: [u8; 2],
    item_entry_count: usize,
    source_stock_group_count: usize,
    stocked_item_entry_count: usize,
    target_dialogue_record_count: usize,
    target_dialogue_workset_count: usize,
    stocked_item_glyph_count: usize,
    preserved_item_code_count: usize,
    maximum_augmented_workset_slot_demand: usize,
    source_lifetime_bound: bool,
    stocked_item_worksets_bound: bool,
    canonical_dialogue_item_material_bound: bool,
    catalog_fallback_item_material_bound: bool,
    producer_normalization_hooks_bound: bool,
    runtime_material_selector_bound: bool,
    e7_dialogue_page_residency_bound: bool,
    static_contract_complete: bool,
    runtime_observation_complete: bool,
    #[serde(skip)]
    runtime: ShopItemResidencyRuntimeContract,
}

impl ShopItemResidencyPlan {
    pub(super) fn runtime_contract(&self) -> ShopItemResidencyRuntimeContract {
        self.runtime
    }

    pub(super) fn bind_runtime_routes(
        &mut self,
        runtime_code: &DialogueRuntimeCodePlan,
        producer_normalization_hooks_bound: bool,
    ) -> Result<()> {
        runtime_code.verify_shop_item_residency_routes(&self.runtime)?;
        self.producer_normalization_hooks_bound = producer_normalization_hooks_bound;
        self.runtime_material_selector_bound = true;
        self.e7_dialogue_page_residency_bound = true;
        self.static_contract_complete = self.source_lifetime_bound
            && self.stocked_item_worksets_bound
            && self.canonical_dialogue_item_material_bound
            && self.catalog_fallback_item_material_bound
            && self.producer_normalization_hooks_bound
            && self.runtime_material_selector_bound
            && self.e7_dialogue_page_residency_bound;
        ensure!(
            self.static_contract_complete,
            "shop item residency did not bind every source, workset, material, and runtime route"
        );
        Ok(())
    }
}

pub(super) fn plan_shop_item_residency(
    inputs: ShopItemResidencyInputs<'_>,
) -> Result<ShopItemResidencyPlan> {
    let dialogue_item_entry_count = inputs
        .dynamic_producer_encoding
        .item_material_entry_count()?;
    let catalog_item_entry_count = inputs.consumer_catalog_runtime.item_material_entry_count();
    ensure!(
        dialogue_item_entry_count == SHOP_ITEM_ENTRY_COUNT
            && catalog_item_entry_count == SHOP_ITEM_ENTRY_COUNT,
        "shop item material populations diverged: dialogue={dialogue_item_entry_count}, catalog={catalog_item_entry_count}"
    );
    ensure!(
        (
            inputs.producer_material_page,
            inputs.producer_item_directory
        ) != (
            inputs.consumer_catalog_layout.material_page,
            inputs.consumer_catalog_layout.item_directory,
        ),
        "shop dialogue and catalog item routes unexpectedly alias one material identity"
    );
    let worksets = inputs.workset_residency;
    ensure!(
        worksets.every_stocked_item_uses_canonical_code && worksets.every_augmented_workset_fits,
        "shop item workset residency is incomplete"
    );

    let runtime = ShopItemResidencyRuntimeContract {
        outer_state_address: worksets.outer_state_address,
        composition_state: worksets.composition_state,
        composite_state: worksets.composite_state,
        selected_facility_address: worksets.selected_facility_address,
        dialogue_directory_address: worksets.dialogue_directory_address,
        dialogue_directory_selector: worksets.dialogue_directory_selector,
        e7_caller_resume_flag_address: inputs.e7_caller_resume_flag_address,
        selling_facilities: worksets.selling_facilities,
        non_selling_facilities: worksets.non_selling_facilities,
        dialogue_material_page: inputs.producer_material_page,
        dialogue_material_base: inputs.producer_material_base,
        dialogue_item_directory: inputs.producer_item_directory,
        catalog_material_page: inputs.consumer_catalog_layout.material_page,
        catalog_material_base: inputs.consumer_catalog_layout.material_base,
        catalog_item_directory: inputs.consumer_catalog_layout.item_directory,
    };

    Ok(ShopItemResidencyPlan {
        schema: 2,
        strategy: "derive every weapon, tool, and secret-shop stock group from map facility records; reserve every stocked Korean item glyph at its canonical dynamic code across all eight shop dialogue lifetimes; then bind the 91-entry dialogue material, catalog fallback, and E7 page route",
        source_composition_state: runtime.composition_state,
        source_composite_state: runtime.composite_state,
        item_selling_facilities: runtime.selling_facilities,
        non_item_selling_facilities: runtime.non_selling_facilities,
        item_entry_count: SHOP_ITEM_ENTRY_COUNT,
        source_stock_group_count: worksets.stock_group_count,
        stocked_item_entry_count: worksets.stocked_item_entry_count,
        target_dialogue_record_count: worksets.target_record_count,
        target_dialogue_workset_count: worksets.target_workset_count,
        stocked_item_glyph_count: worksets.stocked_item_glyph_count,
        preserved_item_code_count: worksets.preserved_item_code_count,
        maximum_augmented_workset_slot_demand: worksets.maximum_augmented_workset_slot_demand,
        source_lifetime_bound: true,
        stocked_item_worksets_bound: true,
        canonical_dialogue_item_material_bound: true,
        catalog_fallback_item_material_bound: true,
        producer_normalization_hooks_bound: false,
        runtime_material_selector_bound: false,
        e7_dialogue_page_residency_bound: false,
        static_contract_complete: false,
        runtime_observation_complete: false,
        runtime,
    })
}
