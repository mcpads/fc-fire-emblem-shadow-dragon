//! 대사 위에 겹치는 공용 예/아니오 라벨의 글리프 코드를 모든 소비 페이지에 고정한다.

use std::collections::{BTreeMap, BTreeSet};

use anyhow::{Result, ensure};
use serde::Serialize;

use crate::{
    chapter_transition::bind_save_complete_dialogue_records,
    choice_labels::{CHOICE_LABEL_COMPOSITE_STATE, ChoiceLabelPlan},
    dialogue_assets::MainDialogueDisplayPlan,
    fixed_string_consumers::scan_direct_composite_state_producers,
    front_end_menu::FRONT_END_RESULT_DIALOGUE_RECORD_IDS,
    mapper165::battle_codebook_plan::GlyphWorkset,
    rom::Rom,
};

use super::{
    resident_glyph_assignment::{
        WorksetDemandComponents, assign_and_augment_resident_worksets, assignment_sha1,
        maximum_workset_demand_components,
    },
    storage_residency::bind_storage_choice_dialogue_record_id,
};

#[derive(Serialize)]
pub(super) struct ChoiceResidencyPlan {
    strategy: &'static str,
    composite_state: u8,
    continue_prompt_record_id: &'static str,
    front_end_result_record_ids: [&'static str; 4],
    storage_choice_record_id: String,
    direct_choice_composite_producer_count: usize,
    resident_record_ids: Vec<String>,
    resident_workset_count: usize,
    choice_glyph_count: usize,
    fixed_code_count: usize,
    maximum_augmented_workset_slot_demand: usize,
    storage_follow_up_workset_count: usize,
    storage_follow_up_target_glyph_count: usize,
    storage_follow_up_preserved_active_code_count: usize,
    storage_follow_up_total_slot_demand: usize,
    fixed_assignment_sha1: String,
    every_choice_glyph_has_one_stable_code: bool,
    every_resident_page_contains_every_choice_glyph: bool,
    #[serde(skip)]
    pub(super) augmented_worksets: Vec<GlyphWorkset>,
    #[serde(skip)]
    pub(super) choice_glyph_codes: BTreeMap<char, u8>,
}

impl ChoiceResidencyPlan {
    pub(super) fn composite_state(&self) -> u8 {
        self.composite_state
    }
}

pub(super) fn plan_choice_residency(
    rom: &Rom,
    display: &MainDialogueDisplayPlan,
    choices: &ChoiceLabelPlan,
    dialogue_worksets: &[GlyphWorkset],
) -> Result<ChoiceResidencyPlan> {
    ensure!(
        display.page_worksets.len() == dialogue_worksets.len(),
        "choice residency lost dialogue page worksets"
    );
    let records = bind_save_complete_dialogue_records(rom)?;
    let storage_choice_record_id = bind_storage_choice_dialogue_record_id(rom)?;
    let direct_choice_composite_producers = scan_direct_composite_state_producers(rom)?
        .into_iter()
        .filter(|producer| producer.state == CHOICE_LABEL_COMPOSITE_STATE)
        .collect::<Vec<_>>();
    ensure!(
        direct_choice_composite_producers.len() == 3,
        "shared yes-no composite producer population changed"
    );
    let choice_glyphs = choices.unique_glyphs();
    ensure!(
        !choice_glyphs.is_empty(),
        "choice residency has no translated glyphs"
    );

    let resident_record_ids = FRONT_END_RESULT_DIALOGUE_RECORD_IDS
        .into_iter()
        .map(str::to_owned)
        .chain([
            records.continue_prompt.to_owned(),
            storage_choice_record_id.clone(),
        ])
        .collect::<BTreeSet<_>>();
    let resident_workset_indices = collect_resident_workset_indices(display, &resident_record_ids)?;
    let storage_follow_up_workset_indices = collect_resident_workset_indices(
        display,
        &BTreeSet::from([storage_choice_record_id.clone()]),
    )?;
    ensure!(
        storage_follow_up_workset_indices.is_subset(&resident_workset_indices),
        "storage follow-up choice worksets escaped shared choice residency"
    );

    let (choice_glyph_codes, augmented_worksets, maximum_augmented_workset_slot_demand) =
        assign_and_augment_choice_worksets(
            &choice_glyphs,
            &resident_workset_indices,
            dialogue_worksets,
        )?;
    let storage_follow_up_demand = maximum_workset_demand_for_indices(
        "storage follow-up choice",
        &augmented_worksets,
        &storage_follow_up_workset_indices,
    )?;
    ensure!(
        storage_follow_up_demand.total_slot_demand <= maximum_augmented_workset_slot_demand,
        "storage follow-up choice demand exceeds shared choice residency"
    );

    Ok(ChoiceResidencyPlan {
        strategy: "assign one injective fallback choice-label codebook across chapter save completion, storage follow-up, and front-end copy/delete/error results; keep shop questions on their separately routed weapon-shop string cave and codebook",
        composite_state: CHOICE_LABEL_COMPOSITE_STATE,
        continue_prompt_record_id: records.continue_prompt,
        front_end_result_record_ids: FRONT_END_RESULT_DIALOGUE_RECORD_IDS,
        storage_choice_record_id,
        direct_choice_composite_producer_count: direct_choice_composite_producers.len(),
        resident_record_ids: resident_record_ids.into_iter().collect(),
        resident_workset_count: resident_workset_indices.len(),
        choice_glyph_count: choice_glyphs.len(),
        fixed_code_count: choice_glyph_codes.len(),
        maximum_augmented_workset_slot_demand,
        storage_follow_up_workset_count: storage_follow_up_workset_indices.len(),
        storage_follow_up_target_glyph_count: storage_follow_up_demand.target_glyph_count,
        storage_follow_up_preserved_active_code_count: storage_follow_up_demand
            .preserved_active_code_count,
        storage_follow_up_total_slot_demand: storage_follow_up_demand.total_slot_demand,
        fixed_assignment_sha1: assignment_sha1(&choice_glyph_codes),
        every_choice_glyph_has_one_stable_code: true,
        every_resident_page_contains_every_choice_glyph: true,
        augmented_worksets,
        choice_glyph_codes,
    })
}

fn maximum_workset_demand_for_indices(
    role: &str,
    worksets: &[GlyphWorkset],
    workset_indices: &BTreeSet<usize>,
) -> Result<WorksetDemandComponents> {
    ensure!(
        workset_indices.iter().all(|index| *index < worksets.len()),
        "{role} references a workset outside the dialogue population"
    );
    let selected_worksets = workset_indices
        .iter()
        .map(|index| worksets[*index].clone())
        .collect::<Vec<_>>();
    maximum_workset_demand_components(role, &selected_worksets)
}

fn collect_resident_workset_indices(
    display: &MainDialogueDisplayPlan,
    resident_record_ids: &BTreeSet<String>,
) -> Result<BTreeSet<usize>> {
    ensure!(
        !resident_record_ids.is_empty(),
        "choice residency record identities overlap"
    );
    let mut found_record_ids = BTreeSet::<String>::new();
    let resident_workset_indices = display
        .page_worksets
        .iter()
        .enumerate()
        .filter_map(|(index, workset)| {
            if resident_record_ids.contains(&workset.record_id) {
                found_record_ids.insert(workset.record_id.clone());
                Some(index)
            } else {
                None
            }
        })
        .collect::<BTreeSet<_>>();
    ensure!(
        found_record_ids == *resident_record_ids,
        "choice residency is missing a source-bound dialogue record"
    );
    ensure!(
        !resident_workset_indices.is_empty(),
        "choice residency has no visible dialogue workset"
    );
    Ok(resident_workset_indices)
}

fn assign_and_augment_choice_worksets(
    choice_glyphs: &BTreeSet<char>,
    resident_workset_indices: &BTreeSet<usize>,
    dialogue_worksets: &[GlyphWorkset],
) -> Result<(BTreeMap<char, u8>, Vec<GlyphWorkset>, usize)> {
    let plan = assign_and_augment_resident_worksets(
        "shared yes-no choice residency",
        choice_glyphs,
        resident_workset_indices,
        dialogue_worksets,
        &BTreeMap::new(),
    )?;
    Ok((
        plan.glyph_codes,
        plan.augmented_worksets,
        plan.maximum_augmented_workset_slot_demand,
    ))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::dialogue_assets::MainDialoguePageWorkset;

    fn page(record_id: &str, page_index: usize) -> MainDialoguePageWorkset {
        MainDialoguePageWorkset {
            record_id: record_id.to_owned(),
            page_index,
            target_glyphs: BTreeSet::new(),
            dynamic_string_selectors: BTreeSet::new(),
            dynamic_string_selector_counts: BTreeMap::new(),
            dynamic_string_control_count: 0,
            source_reclaimable_active_codes: BTreeSet::new(),
            preserved_target_active_codes: BTreeSet::new(),
        }
    }

    fn resident_record_ids(storage_record_id: &str) -> BTreeSet<String> {
        FRONT_END_RESULT_DIALOGUE_RECORD_IDS
            .into_iter()
            .chain(["victory-and-defeat-dialogue:000", storage_record_id])
            .map(str::to_owned)
            .collect()
    }

    #[test]
    fn every_dialogue_that_retains_the_shared_choice_window_uses_one_codebook() {
        let resident_record_ids = resident_record_ids("shop-and-item-dialogue:045");
        let mut pages = resident_record_ids
            .iter()
            .enumerate()
            .map(|(index, id)| page(id, index))
            .collect::<Vec<_>>();
        pages.push(page("unrelated:000", 0));
        let unrelated_index = pages.len() - 1;
        let display = MainDialogueDisplayPlan {
            canonical_record_count: pages.len(),
            record_ids: pages.iter().map(|page| page.record_id.clone()).collect(),
            page_worksets: pages,
        };
        let mut worksets = vec![
            GlyphWorkset {
                target_glyphs: BTreeSet::new(),
                preserved_active_codes: BTreeSet::new(),
                fixed_glyph_codes: BTreeMap::new(),
            };
            display.page_worksets.len()
        ];
        for workset in worksets.iter_mut().take(resident_record_ids.len()) {
            workset.target_glyphs.extend(['기', '오']);
            workset
                .fixed_glyph_codes
                .extend([('기', 0x84), ('오', 0x85)]);
        }
        let choice_glyphs = BTreeSet::from(['예', '아', '니', '오']);
        let resident = collect_resident_workset_indices(&display, &resident_record_ids).unwrap();

        let (assignments, augmented, _) =
            assign_and_augment_choice_worksets(&choice_glyphs, &resident, &worksets).unwrap();

        assert_eq!(resident.len(), resident_record_ids.len());
        assert_eq!(assignments[&'오'], 0x85);
        assert!(assignments.values().all(|code| *code != 0x84));
        for index in resident {
            let workset = &augmented[index];
            assert!(
                choice_glyphs.iter().all(|glyph| {
                    workset.fixed_glyph_codes.get(glyph) == assignments.get(glyph)
                })
            );
        }
        assert!(augmented[unrelated_index].fixed_glyph_codes.is_empty());
    }

    #[test]
    fn a_missing_choice_window_dialogue_record_fails_closed() {
        let resident_record_ids = resident_record_ids("shop-and-item-dialogue:045");
        let omitted_record_id = FRONT_END_RESULT_DIALOGUE_RECORD_IDS[0];
        let pages = resident_record_ids
            .iter()
            .filter(|record_id| record_id.as_str() != omitted_record_id)
            .enumerate()
            .map(|(index, record_id)| page(record_id, index))
            .collect::<Vec<_>>();
        let display = MainDialogueDisplayPlan {
            canonical_record_count: pages.len(),
            record_ids: pages.iter().map(|page| page.record_id.clone()).collect(),
            page_worksets: pages,
        };

        let error = collect_resident_workset_indices(&display, &resident_record_ids).unwrap_err();

        assert!(error.to_string().contains("missing"));
    }

    #[test]
    fn storage_follow_up_demand_comes_from_its_record_pages_not_the_global_maximum() {
        let storage_record_id = "shop-and-item-dialogue:045";
        let front_end_record_id = FRONT_END_RESULT_DIALOGUE_RECORD_IDS[0];
        let display = MainDialogueDisplayPlan {
            canonical_record_count: 2,
            record_ids: vec![storage_record_id.to_owned(), front_end_record_id.to_owned()],
            page_worksets: vec![page(storage_record_id, 0), page(front_end_record_id, 0)],
        };
        let worksets = vec![
            GlyphWorkset {
                target_glyphs: BTreeSet::from(['보']),
                preserved_active_codes: BTreeSet::new(),
                fixed_glyph_codes: BTreeMap::new(),
            },
            GlyphWorkset {
                target_glyphs: BTreeSet::from(['가', '나', '다', '라', '마', '바']),
                preserved_active_codes: BTreeSet::new(),
                fixed_glyph_codes: BTreeMap::new(),
            },
        ];
        let resident_record_ids =
            BTreeSet::from([storage_record_id.to_owned(), front_end_record_id.to_owned()]);
        let resident_indices =
            collect_resident_workset_indices(&display, &resident_record_ids).unwrap();
        let storage_indices = collect_resident_workset_indices(
            &display,
            &BTreeSet::from([storage_record_id.to_owned()]),
        )
        .unwrap();
        let choice_glyphs = BTreeSet::from(['예', '아', '니', '오']);

        let (_, augmented, global_maximum) =
            assign_and_augment_choice_worksets(&choice_glyphs, &resident_indices, &worksets)
                .unwrap();
        let storage = maximum_workset_demand_for_indices(
            "storage follow-up choice",
            &augmented,
            &storage_indices,
        )
        .unwrap();

        assert_eq!(storage.target_glyph_count, 5);
        assert_eq!(storage.preserved_active_code_count, 0);
        assert_eq!(storage.total_slot_demand, 5);
        assert!(storage.total_slot_demand < global_maximum);
    }
}
