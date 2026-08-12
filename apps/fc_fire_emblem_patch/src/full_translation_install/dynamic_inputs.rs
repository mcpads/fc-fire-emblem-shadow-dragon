use std::collections::{BTreeMap, BTreeSet};

use anyhow::{Result, ensure};

use crate::{
    dialogue_assets::MainDialogueBundlePlan, mapper165::battle_codebook_plan::GlyphWorkset,
    text_inventory::FixedTextPlannedEntry,
};

#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
enum DynamicStringDomain {
    PreservedNumeric,
    ItemName,
    PlayableUnitName,
    LocationName,
}

pub(super) struct DynamicDialogueInputPlan {
    pub(super) augmented_worksets: Vec<GlyphWorkset>,
    pub(super) declared_domain_count: usize,
    pub(super) translated_dynamic_page_count: usize,
    pub(super) preserved_numeric_page_count: usize,
    pub(super) translated_dynamic_glyph_count: usize,
    pub(super) combined_dialogue_glyph_count: usize,
    pub(super) maximum_possible_domain_glyph_count: usize,
    pub(super) maximum_augmented_workset_slot_demand: usize,
    pub(super) maximum_rendered_target_glyph_upper_bound: usize,
    pub(super) every_dynamic_control_classified: bool,
    pub(super) every_augmented_workset_fits: bool,
}

pub(super) fn plan_dynamic_dialogue_inputs(
    dialogue: &MainDialogueBundlePlan,
    fixed_text: &[FixedTextPlannedEntry],
    unit_names: &[FixedTextPlannedEntry],
    location_names: &[FixedTextPlannedEntry],
) -> Result<DynamicDialogueInputPlan> {
    let item_name_domain = domain_glyphs(fixed_text, "item-names")?;
    let unit_name_domain = domain_glyphs(unit_names, "unit-names")?;
    let location_name_domain = domain_glyphs(location_names, "location-names")?;
    let domains = BTreeMap::from([
        (DynamicStringDomain::ItemName, item_name_domain),
        (DynamicStringDomain::PlayableUnitName, unit_name_domain),
        (DynamicStringDomain::LocationName, location_name_domain),
    ]);
    let translated_dynamic_glyphs = domains
        .values()
        .flat_map(|domain| domain.glyphs.iter().copied())
        .collect::<BTreeSet<_>>();
    let literal_dialogue_glyphs = dialogue.unique_glyphs();
    let combined_dialogue_glyph_count = literal_dialogue_glyphs
        .union(&translated_dynamic_glyphs)
        .count();

    let mut augmented_worksets = Vec::with_capacity(dialogue.page_worksets.len());
    let mut translated_dynamic_page_count = 0;
    let mut preserved_numeric_page_count = 0;
    let mut maximum_possible_domain_glyph_count = 0;
    let mut maximum_augmented_workset_slot_demand = 0;
    let mut maximum_rendered_target_glyph_upper_bound = 0;
    let mut classified_control_count = 0;

    for workset in &dialogue.page_worksets {
        let mut target_glyphs = workset.target_glyphs.clone();
        let mut possible_domain_glyphs = BTreeSet::new();
        let mut rendered_dynamic_glyph_upper_bound = 0;
        let mut has_translated_domain = false;
        let mut has_preserved_numeric = false;

        for (selector, control_count) in &workset.dynamic_string_selector_counts {
            let domain = dynamic_string_domain(&workset.record_id, *selector).ok_or_else(|| {
                anyhow::anyhow!(
                    "{} page {} has an unclassified EC selector {selector:02X}",
                    workset.record_id,
                    workset.page_index
                )
            })?;
            classified_control_count += *control_count;
            match domain {
                DynamicStringDomain::PreservedNumeric => has_preserved_numeric = true,
                translated => {
                    has_translated_domain = true;
                    let domain = &domains[&translated];
                    possible_domain_glyphs.extend(domain.glyphs.iter().copied());
                    rendered_dynamic_glyph_upper_bound +=
                        *control_count * domain.maximum_entry_glyph_count;
                }
            }
        }
        if has_translated_domain {
            translated_dynamic_page_count += 1;
        }
        if has_preserved_numeric {
            preserved_numeric_page_count += 1;
        }
        maximum_possible_domain_glyph_count =
            maximum_possible_domain_glyph_count.max(possible_domain_glyphs.len());
        let rendered_target_glyph_upper_bound =
            target_glyphs.len() + rendered_dynamic_glyph_upper_bound;
        maximum_rendered_target_glyph_upper_bound =
            maximum_rendered_target_glyph_upper_bound.max(rendered_target_glyph_upper_bound);
        target_glyphs.extend(possible_domain_glyphs);
        let slot_demand = target_glyphs.len() + workset.preserved_target_active_codes.len();
        maximum_augmented_workset_slot_demand =
            maximum_augmented_workset_slot_demand.max(slot_demand);
        augmented_worksets.push(GlyphWorkset {
            target_glyphs,
            preserved_active_codes: workset.preserved_target_active_codes.clone(),
        });
    }

    let dynamic_control_count = dialogue
        .page_worksets
        .iter()
        .map(|workset| workset.dynamic_string_control_count)
        .sum::<usize>();
    ensure!(
        classified_control_count == dynamic_control_count,
        "dynamic dialogue classification lost EC controls"
    );
    let every_augmented_workset_fits =
        maximum_augmented_workset_slot_demand <= crate::font_slots::ACTIVE_HANGUL_SLOT_COUNT;
    ensure!(
        every_augmented_workset_fits,
        "dynamic dialogue workset needs {maximum_augmented_workset_slot_demand} active slots but only {} exist",
        crate::font_slots::ACTIVE_HANGUL_SLOT_COUNT
    );

    Ok(DynamicDialogueInputPlan {
        augmented_worksets,
        declared_domain_count: DynamicStringDomain::ALL.len(),
        translated_dynamic_page_count,
        preserved_numeric_page_count,
        translated_dynamic_glyph_count: translated_dynamic_glyphs.len(),
        combined_dialogue_glyph_count,
        maximum_possible_domain_glyph_count,
        maximum_augmented_workset_slot_demand,
        maximum_rendered_target_glyph_upper_bound,
        every_dynamic_control_classified: true,
        every_augmented_workset_fits,
    })
}

struct DomainGlyphs {
    glyphs: BTreeSet<char>,
    maximum_entry_glyph_count: usize,
}

fn domain_glyphs(entries: &[FixedTextPlannedEntry], table_id: &str) -> Result<DomainGlyphs> {
    let selected = entries
        .iter()
        .filter(|entry| entry.table_id == table_id)
        .collect::<Vec<_>>();
    ensure!(
        !selected.is_empty(),
        "dynamic dialogue domain {table_id} has no translated entries"
    );
    Ok(DomainGlyphs {
        glyphs: selected
            .iter()
            .flat_map(|entry| entry.unique_glyphs())
            .collect(),
        maximum_entry_glyph_count: selected
            .iter()
            .map(|entry| entry.unique_glyphs().len())
            .max()
            .unwrap_or(0),
    })
}

impl DynamicStringDomain {
    const ALL: [Self; 4] = [
        Self::PreservedNumeric,
        Self::ItemName,
        Self::PlayableUnitName,
        Self::LocationName,
    ];
}

fn dynamic_string_domain(record_id: &str, selector: u8) -> Option<DynamicStringDomain> {
    let binding = (record_id, selector);
    if ITEM_NAME_BINDINGS.contains(&binding) {
        Some(DynamicStringDomain::ItemName)
    } else if PLAYABLE_UNIT_NAME_BINDINGS.contains(&binding) {
        Some(DynamicStringDomain::PlayableUnitName)
    } else if LOCATION_NAME_BINDINGS.contains(&binding) {
        Some(DynamicStringDomain::LocationName)
    } else if PRESERVED_NUMERIC_BINDINGS.contains(&binding) {
        Some(DynamicStringDomain::PreservedNumeric)
    } else {
        None
    }
}

const ITEM_NAME_BINDINGS: [(&str, u8); 6] = [
    ("village-and-outro-dialogue:014", 0),
    ("village-and-outro-dialogue:021", 0),
    ("shop-and-item-dialogue:025", 1),
    ("shop-and-item-dialogue:026", 1),
    ("shop-and-item-dialogue:027", 1),
    ("shop-and-item-dialogue:028", 1),
];

const PLAYABLE_UNIT_NAME_BINDINGS: [(&str, u8); 5] = [
    ("shop-and-item-dialogue:025", 0),
    ("shop-and-item-dialogue:026", 0),
    ("shop-and-item-dialogue:027", 0),
    ("shop-and-item-dialogue:027", 2),
    ("shop-and-item-dialogue:028", 0),
];

const LOCATION_NAME_BINDINGS: [(&str, u8); 1] = [("epilogue-dialogue:000", 1)];

const PRESERVED_NUMERIC_BINDINGS: [(&str, u8); 20] = [
    ("village-and-outro-dialogue:000", 0),
    ("village-and-outro-dialogue:004", 0),
    ("village-and-outro-dialogue:008", 0),
    ("village-and-outro-dialogue:017", 0),
    ("village-and-outro-dialogue:020", 0),
    ("shop-and-item-dialogue:008", 0),
    ("shop-and-item-dialogue:015", 1),
    ("shop-and-item-dialogue:021", 1),
    ("shop-and-item-dialogue:029", 2),
    ("shop-and-item-dialogue:030", 2),
    ("shop-and-item-dialogue:031", 2),
    ("shop-and-item-dialogue:032", 2),
    ("shop-and-item-dialogue:033", 2),
    ("shop-and-item-dialogue:034", 2),
    ("shop-and-item-dialogue:035", 2),
    ("shop-and-item-dialogue:036", 2),
    ("shop-and-item-dialogue:037", 2),
    ("shop-and-item-dialogue:038", 2),
    ("shop-and-item-dialogue:040", 2),
    ("shop-and-item-dialogue:042", 0),
];

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unknown_dynamic_string_binding_fails_closed() {
        assert_eq!(
            dynamic_string_domain("shop-and-item-dialogue:025", 0),
            Some(DynamicStringDomain::PlayableUnitName)
        );
        assert_eq!(
            dynamic_string_domain("shop-and-item-dialogue:025", 1),
            Some(DynamicStringDomain::ItemName)
        );
        assert_eq!(dynamic_string_domain("unknown", 0), None);
    }
}
