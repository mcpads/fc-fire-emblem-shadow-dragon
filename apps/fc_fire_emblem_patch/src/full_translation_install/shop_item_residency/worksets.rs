//! 시설 재고가 대사 레코드 밖에서 합성하는 품목 글리프를 페이지 작업집합에 결속한다.

use std::collections::{BTreeMap, BTreeSet};

use anyhow::{Context, Result, ensure};

use super::{ShopItemWorksetResidencyInputs, ShopItemWorksetResidencyPlan};
use crate::{
    dialogue_assets::MainDialogueDisplayPlan,
    font_slots::{ACTIVE_HANGUL_SLOT_COUNT, active_hangul_codes},
    mapper165::battle_codebook_plan::GlyphWorkset,
    shop_flow::{
        SHOP_DIALOGUE_LIFETIME_RECORD_IDS, SHOP_ITEM_ENTRY_COUNT, bind_shop_item_composition_source,
    },
    text_inventory::{FixedTextLogicalByte, FixedTextPlan},
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
    let augmentation = augment_shop_item_worksets(
        inputs.display,
        inputs.fixed,
        inputs.dialogue_worksets,
        inputs.canonical_dynamic_codes,
        source.item_source_indices(),
    )?;

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
        target_workset_count: augmentation.target_workset_count,
        stocked_item_glyph_count: augmentation.stocked_item_glyph_count,
        preserved_item_code_count: augmentation.preserved_item_code_count,
        maximum_augmented_workset_slot_demand: augmentation.maximum_augmented_workset_slot_demand,
        every_stocked_item_uses_canonical_code: true,
        every_augmented_workset_fits: true,
    })
}

struct ShopItemWorksetAugmentation {
    augmented_worksets: Vec<GlyphWorkset>,
    target_record_count: usize,
    target_workset_count: usize,
    stocked_item_glyph_count: usize,
    preserved_item_code_count: usize,
    maximum_augmented_workset_slot_demand: usize,
}

fn augment_shop_item_worksets(
    display: &MainDialogueDisplayPlan,
    fixed: &FixedTextPlan,
    dialogue_worksets: &[GlyphWorkset],
    canonical_dynamic_codes: &BTreeMap<char, u8>,
    item_source_indices: &BTreeSet<usize>,
) -> Result<ShopItemWorksetAugmentation> {
    ensure!(
        display.page_worksets.len() == dialogue_worksets.len(),
        "shop item residency lost the main-dialogue workset identity"
    );
    let active_codes = active_hangul_codes().into_iter().collect::<BTreeSet<_>>();
    let mut stocked_item_glyphs = BTreeSet::new();
    let mut preserved_item_codes = BTreeSet::new();
    for source_index in item_source_indices {
        let entry = fixed
            .entry_for_source_index("item-names", *source_index)
            .with_context(|| format!("shop stock item {source_index} has no translated name"))?;
        stocked_item_glyphs.extend(entry.unique_glyphs());
        preserved_item_codes.extend(entry.logical_bytes.iter().filter_map(|byte| match byte {
            FixedTextLogicalByte::Encoded(code) if active_codes.contains(code) => Some(*code),
            FixedTextLogicalByte::TargetGlyph(_) | FixedTextLogicalByte::Encoded(_) => None,
        }));
    }
    ensure!(
        !stocked_item_glyphs.is_empty()
            && stocked_item_glyphs
                .iter()
                .all(|glyph| canonical_dynamic_codes.contains_key(glyph)),
        "shop stock item glyphs are not a subset of the canonical item encoding"
    );

    let target_record_ids = SHOP_DIALOGUE_LIFETIME_RECORD_IDS
        .into_iter()
        .collect::<BTreeSet<_>>();
    let present_target_record_ids = display
        .page_worksets
        .iter()
        .filter(|page| target_record_ids.contains(page.record_id.as_str()))
        .map(|page| page.record_id.as_str())
        .collect::<BTreeSet<_>>();
    ensure!(
        present_target_record_ids == target_record_ids,
        "shop item residency lost one of its eight dialogue records"
    );

    let mut augmented_worksets = dialogue_worksets.to_vec();
    let mut target_workset_count = 0_usize;
    for (page, workset) in display.page_worksets.iter().zip(&mut augmented_worksets) {
        if !target_record_ids.contains(page.record_id.as_str()) {
            continue;
        }
        target_workset_count += 1;
        workset
            .target_glyphs
            .extend(stocked_item_glyphs.iter().copied());
        workset
            .preserved_active_codes
            .extend(preserved_item_codes.iter().copied());
        for glyph in &stocked_item_glyphs {
            let code = canonical_dynamic_codes[glyph];
            if let Some(existing) = workset.fixed_glyph_codes.insert(*glyph, code) {
                ensure!(
                    existing == code,
                    "shop item glyph {glyph:?} changed its canonical code"
                );
            }
        }
        ensure!(
            workset
                .fixed_glyph_codes
                .values()
                .all(|code| !workset.preserved_active_codes.contains(code)),
            "shop item canonical code collides with a preserved dialogue-page code"
        );
    }
    let maximum_augmented_workset_slot_demand = augmented_worksets
        .iter()
        .map(|workset| workset.target_glyphs.len() + workset.preserved_active_codes.len())
        .max()
        .unwrap_or(0);
    ensure!(
        target_workset_count >= SHOP_DIALOGUE_LIFETIME_RECORD_IDS.len()
            && maximum_augmented_workset_slot_demand <= ACTIVE_HANGUL_SLOT_COUNT,
        "shop item workset needs {maximum_augmented_workset_slot_demand} active slots but only {ACTIVE_HANGUL_SLOT_COUNT} exist"
    );

    Ok(ShopItemWorksetAugmentation {
        augmented_worksets,
        target_record_count: target_record_ids.len(),
        target_workset_count,
        stocked_item_glyph_count: stocked_item_glyphs.len(),
        preserved_item_code_count: preserved_item_codes.len(),
        maximum_augmented_workset_slot_demand,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{dialogue_assets::MainDialoguePageWorkset, text_inventory::FixedTextPlannedEntry};

    fn fixed_entry(source_index: usize, glyph: char) -> FixedTextPlannedEntry {
        FixedTextPlannedEntry {
            id: format!("item-names:{source_index:03}"),
            table_id: "item-names".to_owned(),
            source_index,
            alias_indices: Vec::new(),
            file_offset: source_index,
            source_storage_byte_count: 1,
            review_complete: true,
            logical_bytes: vec![FixedTextLogicalByte::TargetGlyph(glyph)],
        }
    }

    fn display_and_worksets() -> (MainDialogueDisplayPlan, Vec<GlyphWorkset>) {
        let mut record_ids = SHOP_DIALOGUE_LIFETIME_RECORD_IDS
            .into_iter()
            .map(str::to_owned)
            .collect::<Vec<_>>();
        record_ids.push("unrelated-dialogue:000".to_owned());
        let page_worksets = record_ids
            .iter()
            .map(|record_id| MainDialoguePageWorkset {
                record_id: record_id.clone(),
                page_index: 0,
                target_glyphs: BTreeSet::from(['기']),
                dynamic_string_selectors: BTreeSet::new(),
                dynamic_string_selector_counts: BTreeMap::new(),
                dynamic_string_control_count: 0,
                source_reclaimable_active_codes: BTreeSet::new(),
                preserved_target_active_codes: BTreeSet::from([0xA0]),
            })
            .collect::<Vec<_>>();
        let worksets = page_worksets
            .iter()
            .map(|_| GlyphWorkset {
                target_glyphs: BTreeSet::from(['기']),
                preserved_active_codes: BTreeSet::from([0xA0]),
                fixed_glyph_codes: BTreeMap::new(),
            })
            .collect::<Vec<_>>();
        (
            MainDialogueDisplayPlan {
                canonical_record_count: record_ids.len(),
                page_worksets,
                record_ids,
            },
            worksets,
        )
    }

    #[test]
    fn stocked_item_glyphs_extend_every_shop_lifetime_but_not_unrelated_dialogue() {
        let (display, worksets) = display_and_worksets();
        let fixed = FixedTextPlan {
            workspace_sha1: "fixed".to_owned(),
            review_complete: true,
            entries: vec![fixed_entry(0, '검'), fixed_entry(1, '창')],
        };
        let canonical = BTreeMap::from([('검', 0xA1), ('창', 0xA2)]);
        let augmentation = augment_shop_item_worksets(
            &display,
            &fixed,
            &worksets,
            &canonical,
            &BTreeSet::from([0, 1]),
        )
        .unwrap();

        assert_eq!(augmentation.target_record_count, 8);
        assert_eq!(augmentation.target_workset_count, 8);
        for workset in &augmentation.augmented_worksets[..8] {
            assert!(
                workset
                    .target_glyphs
                    .is_superset(&BTreeSet::from(['검', '창']))
            );
            assert_eq!(workset.fixed_glyph_codes, canonical);
        }
        assert_eq!(
            augmentation.augmented_worksets[8].target_glyphs,
            worksets[8].target_glyphs
        );
        assert_eq!(
            augmentation.augmented_worksets[8].preserved_active_codes,
            worksets[8].preserved_active_codes
        );
        assert_eq!(
            augmentation.augmented_worksets[8].fixed_glyph_codes,
            worksets[8].fixed_glyph_codes
        );
    }

    #[test]
    fn stocked_item_code_cannot_overwrite_a_preserved_page_code() {
        let (display, mut worksets) = display_and_worksets();
        worksets[0].preserved_active_codes.insert(0xA1);
        let fixed = FixedTextPlan {
            workspace_sha1: "fixed".to_owned(),
            review_complete: true,
            entries: vec![fixed_entry(0, '검')],
        };
        let error = match augment_shop_item_worksets(
            &display,
            &fixed,
            &worksets,
            &BTreeMap::from([('검', 0xA1)]),
            &BTreeSet::from([0]),
        ) {
            Ok(_) => panic!("preserved code collision was accepted"),
            Err(error) => error,
        };

        assert!(error.to_string().contains("collides with a preserved"));
    }
}
