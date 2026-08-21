//! 대사 위에 별도 합성되는 아이템명이 현재 대사 페이지와 같은 코드 배정을 쓰게 한다.

use std::collections::{BTreeMap, BTreeSet};

use anyhow::{Context, Result, ensure};

use crate::{
    dialogue_assets::MainDialogueDisplayPlan,
    font_slots::{ACTIVE_HANGUL_SLOT_COUNT, active_hangul_codes},
    mapper165::battle_codebook_plan::GlyphWorkset,
    text_inventory::{FixedTextLogicalByte, FixedTextPlan},
};

pub(super) struct DialogueItemWorksetInputs<'a> {
    pub(super) role: &'static str,
    pub(super) display: &'a MainDialogueDisplayPlan,
    pub(super) fixed: &'a FixedTextPlan,
    pub(super) dialogue_worksets: &'a [GlyphWorkset],
    pub(super) canonical_item_codes: &'a BTreeMap<char, u8>,
    pub(super) item_source_indices: &'a BTreeSet<usize>,
    pub(super) target_record_ids: &'a BTreeSet<String>,
}

pub(super) struct DialogueItemWorksetAugmentation {
    pub(super) augmented_worksets: Vec<GlyphWorkset>,
    pub(super) target_record_count: usize,
    pub(super) target_workset_count: usize,
    pub(super) item_glyph_count: usize,
    pub(super) preserved_item_code_count: usize,
    pub(super) maximum_augmented_workset_slot_demand: usize,
}

pub(super) fn collect_item_name_glyphs(
    role: &str,
    fixed: &FixedTextPlan,
    item_source_indices: &BTreeSet<usize>,
) -> Result<(BTreeSet<char>, BTreeSet<u8>)> {
    let active_codes = active_hangul_codes().into_iter().collect::<BTreeSet<_>>();
    let mut item_glyphs = BTreeSet::new();
    let mut preserved_item_codes = BTreeSet::new();
    for source_index in item_source_indices {
        let entry = fixed
            .entry_for_source_index("item-names", *source_index)
            .with_context(|| format!("{role} item {source_index} has no translated name"))?;
        item_glyphs.extend(entry.unique_glyphs());
        preserved_item_codes.extend(entry.logical_bytes.iter().filter_map(|byte| match byte {
            FixedTextLogicalByte::Encoded(code) if active_codes.contains(code) => Some(*code),
            FixedTextLogicalByte::TargetGlyph(_) | FixedTextLogicalByte::Encoded(_) => None,
        }));
    }
    Ok((item_glyphs, preserved_item_codes))
}

pub(super) fn augment_dialogue_item_worksets(
    inputs: DialogueItemWorksetInputs<'_>,
) -> Result<DialogueItemWorksetAugmentation> {
    ensure!(
        inputs.display.page_worksets.len() == inputs.dialogue_worksets.len(),
        "{} lost the main-dialogue workset identity",
        inputs.role
    );
    ensure!(
        !inputs.item_source_indices.is_empty() && !inputs.target_record_ids.is_empty(),
        "{} has an empty item or dialogue population",
        inputs.role
    );

    let (item_glyphs, preserved_item_codes) =
        collect_item_name_glyphs(inputs.role, inputs.fixed, inputs.item_source_indices)?;
    ensure!(
        !item_glyphs.is_empty()
            && item_glyphs
                .iter()
                .all(|glyph| inputs.canonical_item_codes.contains_key(glyph)),
        "{} item glyphs are not a subset of the canonical item encoding",
        inputs.role
    );

    let present_target_record_ids = inputs
        .display
        .page_worksets
        .iter()
        .filter(|page| inputs.target_record_ids.contains(&page.record_id))
        .map(|page| page.record_id.clone())
        .collect::<BTreeSet<_>>();
    ensure!(
        present_target_record_ids == *inputs.target_record_ids,
        "{} lost one or more target dialogue records",
        inputs.role
    );

    let mut augmented_worksets = inputs.dialogue_worksets.to_vec();
    let mut target_workset_count = 0_usize;
    for (page, workset) in inputs
        .display
        .page_worksets
        .iter()
        .zip(&mut augmented_worksets)
    {
        if !inputs.target_record_ids.contains(&page.record_id) {
            continue;
        }
        target_workset_count += 1;
        workset.target_glyphs.extend(item_glyphs.iter().copied());
        workset
            .preserved_active_codes
            .extend(preserved_item_codes.iter().copied());
        for glyph in &item_glyphs {
            let code = inputs.canonical_item_codes[glyph];
            let conflicting_glyph =
                workset
                    .fixed_glyph_codes
                    .iter()
                    .find_map(|(existing_glyph, existing_code)| {
                        (*existing_glyph != *glyph && *existing_code == code)
                            .then_some(*existing_glyph)
                    });
            ensure!(
                conflicting_glyph.is_none(),
                "{} dialogue {} page {} cannot assign item glyph {glyph:?} to canonical code 0x{code:02X}; that code already belongs to {conflicting_glyph:?}",
                inputs.role,
                page.record_id,
                page.page_index,
            );
            if let Some(existing) = workset.fixed_glyph_codes.insert(*glyph, code) {
                ensure!(
                    existing == code,
                    "{} item glyph {glyph:?} changed its canonical code",
                    inputs.role
                );
            }
        }
        ensure!(
            workset
                .fixed_glyph_codes
                .values()
                .all(|code| !workset.preserved_active_codes.contains(code)),
            "{} canonical item code collides with a preserved dialogue-page code",
            inputs.role
        );
    }

    let maximum_augmented_workset_slot_demand = augmented_worksets
        .iter()
        .map(|workset| workset.target_glyphs.len() + workset.preserved_active_codes.len())
        .max()
        .unwrap_or(0);
    ensure!(
        target_workset_count >= inputs.target_record_ids.len()
            && maximum_augmented_workset_slot_demand <= ACTIVE_HANGUL_SLOT_COUNT,
        "{} workset needs {maximum_augmented_workset_slot_demand} active slots but only {ACTIVE_HANGUL_SLOT_COUNT} exist",
        inputs.role
    );

    Ok(DialogueItemWorksetAugmentation {
        augmented_worksets,
        target_record_count: inputs.target_record_ids.len(),
        target_workset_count,
        item_glyph_count: item_glyphs.len(),
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
            source_display_cell_count: 1,
            review_complete: true,
            logical_bytes: vec![FixedTextLogicalByte::TargetGlyph(glyph)],
        }
    }

    fn page(record_id: &str) -> MainDialoguePageWorkset {
        MainDialoguePageWorkset {
            record_id: record_id.to_owned(),
            page_index: 0,
            target_glyphs: BTreeSet::from(['기']),
            dynamic_string_selectors: BTreeSet::new(),
            dynamic_string_selector_counts: BTreeMap::new(),
            dynamic_string_control_count: 0,
            source_reclaimable_active_codes: BTreeSet::new(),
            preserved_target_active_codes: BTreeSet::from([0xA0]),
        }
    }

    fn workset() -> GlyphWorkset {
        GlyphWorkset {
            target_glyphs: BTreeSet::from(['기']),
            preserved_active_codes: BTreeSet::from([0xA0]),
            fixed_glyph_codes: BTreeMap::new(),
        }
    }

    #[test]
    fn only_source_selected_dialogue_lifetimes_receive_canonical_item_glyphs() {
        let display = MainDialogueDisplayPlan {
            canonical_record_count: 2,
            page_worksets: vec![page("target:000"), page("unrelated:000")],
            record_ids: vec!["target:000".to_owned(), "unrelated:000".to_owned()],
        };
        let fixed = FixedTextPlan {
            workspace_sha1: "fixed".to_owned(),
            review_complete: true,
            entries: vec![fixed_entry(0, '검'), fixed_entry(1, '창')],
        };
        let original = vec![workset(), workset()];
        let augmented = augment_dialogue_item_worksets(DialogueItemWorksetInputs {
            role: "test item lifetime",
            display: &display,
            fixed: &fixed,
            dialogue_worksets: &original,
            canonical_item_codes: &BTreeMap::from([('검', 0xA1), ('창', 0xA2)]),
            item_source_indices: &BTreeSet::from([0, 1]),
            target_record_ids: &BTreeSet::from(["target:000".to_owned()]),
        })
        .unwrap();

        assert_eq!(augmented.target_record_count, 1);
        assert_eq!(augmented.target_workset_count, 1);
        assert!(
            augmented.augmented_worksets[0]
                .target_glyphs
                .is_superset(&BTreeSet::from(['검', '창']))
        );
        assert_eq!(
            augmented.augmented_worksets[0].fixed_glyph_codes,
            BTreeMap::from([('검', 0xA1), ('창', 0xA2)])
        );
        assert_eq!(
            augmented.augmented_worksets[1].target_glyphs,
            original[1].target_glyphs
        );
        assert_eq!(
            augmented.augmented_worksets[1].preserved_active_codes,
            original[1].preserved_active_codes
        );
        assert_eq!(
            augmented.augmented_worksets[1].fixed_glyph_codes,
            original[1].fixed_glyph_codes
        );
    }

    #[test]
    fn canonical_item_code_cannot_overwrite_preserved_or_fixed_page_ownership() {
        let display = MainDialogueDisplayPlan {
            canonical_record_count: 1,
            page_worksets: vec![page("target:000")],
            record_ids: vec!["target:000".to_owned()],
        };
        let fixed = FixedTextPlan {
            workspace_sha1: "fixed".to_owned(),
            review_complete: true,
            entries: vec![fixed_entry(0, '검')],
        };
        let targets = BTreeSet::from(["target:000".to_owned()]);
        let indices = BTreeSet::from([0]);
        let canonical = BTreeMap::from([('검', 0xA1)]);

        let mut preserved = workset();
        preserved.preserved_active_codes.insert(0xA1);
        let error = augment_dialogue_item_worksets(DialogueItemWorksetInputs {
            role: "test item lifetime",
            display: &display,
            fixed: &fixed,
            dialogue_worksets: &[preserved],
            canonical_item_codes: &canonical,
            item_source_indices: &indices,
            target_record_ids: &targets,
        })
        .err()
        .expect("preserved collision unexpectedly passed");
        assert!(error.to_string().contains("preserved dialogue-page code"));

        let mut assigned = workset();
        assigned.fixed_glyph_codes.insert('창', 0xA1);
        let error = augment_dialogue_item_worksets(DialogueItemWorksetInputs {
            role: "test item lifetime",
            display: &display,
            fixed: &fixed,
            dialogue_worksets: &[assigned],
            canonical_item_codes: &canonical,
            item_source_indices: &indices,
            target_record_ids: &targets,
        })
        .err()
        .expect("fixed-code collision unexpectedly passed");
        assert!(error.to_string().contains("already belongs to"));
    }
}
