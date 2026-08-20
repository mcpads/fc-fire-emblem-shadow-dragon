//! 저장 완료 대사 위에 겹치는 선택 라벨의 글리프 코드를 대사 페이지마다 고정한다.

use std::collections::{BTreeMap, BTreeSet};

use anyhow::{Result, ensure};
use serde::Serialize;

use crate::{
    chapter_transition::bind_save_complete_dialogue_records,
    choice_labels::{CHOICE_LABEL_COMPOSITE_STATE, ChoiceLabelPlan},
    dialogue_assets::MainDialogueDisplayPlan,
    font_slots::{ACTIVE_HANGUL_SLOT_COUNT, active_hangul_codes},
    front_end_menu::FRONT_END_RESULT_DIALOGUE_RECORD_IDS,
    mapper165::battle_codebook_plan::GlyphWorkset,
    rom::Rom,
};

use super::resident_glyph_assignment::{assign_resident_glyph_codes, assignment_sha1};

#[derive(Serialize)]
pub(super) struct ChoiceResidencyPlan {
    strategy: &'static str,
    composite_state: u8,
    continue_prompt_record_id: &'static str,
    front_end_result_record_ids: [&'static str; 4],
    resident_workset_count: usize,
    choice_glyph_count: usize,
    fixed_code_count: usize,
    maximum_augmented_workset_slot_demand: usize,
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
    let choice_glyphs = choices.unique_glyphs();
    ensure!(
        !choice_glyphs.is_empty(),
        "choice residency has no translated glyphs"
    );

    let resident_workset_indices =
        collect_resident_workset_indices(display, records.continue_prompt)?;

    let (choice_glyph_codes, augmented_worksets, maximum_augmented_workset_slot_demand) =
        assign_and_augment_choice_worksets(
            &choice_glyphs,
            &resident_workset_indices,
            dialogue_worksets,
        )?;

    Ok(ChoiceResidencyPlan {
        strategy: "assign one injective choice-label codebook across the save-complete prompt and every retained front-end copy/delete/error result surface",
        composite_state: CHOICE_LABEL_COMPOSITE_STATE,
        continue_prompt_record_id: records.continue_prompt,
        front_end_result_record_ids: FRONT_END_RESULT_DIALOGUE_RECORD_IDS,
        resident_workset_count: resident_workset_indices.len(),
        choice_glyph_count: choice_glyphs.len(),
        fixed_code_count: choice_glyph_codes.len(),
        maximum_augmented_workset_slot_demand,
        fixed_assignment_sha1: assignment_sha1(&choice_glyph_codes),
        every_choice_glyph_has_one_stable_code: true,
        every_resident_page_contains_every_choice_glyph: true,
        augmented_worksets,
        choice_glyph_codes,
    })
}

fn collect_resident_workset_indices(
    display: &MainDialogueDisplayPlan,
    continue_prompt_record_id: &'static str,
) -> Result<BTreeSet<usize>> {
    let resident_record_ids = FRONT_END_RESULT_DIALOGUE_RECORD_IDS
        .into_iter()
        .chain([continue_prompt_record_id])
        .collect::<BTreeSet<_>>();
    ensure!(
        resident_record_ids.len() == FRONT_END_RESULT_DIALOGUE_RECORD_IDS.len() + 1,
        "choice residency record identities overlap"
    );
    let mut found_record_ids = BTreeSet::new();
    let resident_workset_indices = display
        .page_worksets
        .iter()
        .enumerate()
        .filter_map(|(index, workset)| {
            resident_record_ids
                .contains(workset.record_id.as_str())
                .then(|| {
                    found_record_ids.insert(workset.record_id.as_str());
                    index
                })
        })
        .collect::<BTreeSet<_>>();
    ensure!(
        found_record_ids == resident_record_ids,
        "choice residency is missing a save-complete or front-end result record"
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
    let mut forbidden_codes_by_glyph = choice_glyphs
        .iter()
        .copied()
        .map(|glyph| (glyph, BTreeSet::new()))
        .collect::<BTreeMap<_, _>>();
    let mut preassigned_codes_by_glyph = BTreeMap::<char, BTreeSet<u8>>::new();
    for workset_index in resident_workset_indices {
        let workset = &dialogue_worksets[*workset_index];
        for glyph in choice_glyphs {
            forbidden_codes_by_glyph
                .get_mut(glyph)
                .expect("choice glyph was initialized")
                .extend(workset.preserved_active_codes.iter().copied());
            for (fixed_glyph, fixed_code) in &workset.fixed_glyph_codes {
                if fixed_glyph == glyph {
                    preassigned_codes_by_glyph
                        .entry(*glyph)
                        .or_default()
                        .insert(*fixed_code);
                } else {
                    forbidden_codes_by_glyph
                        .get_mut(glyph)
                        .expect("choice glyph was initialized")
                        .insert(*fixed_code);
                }
            }
        }
    }
    let choice_glyph_codes = assign_resident_glyph_codes(
        "save-complete choice residency",
        &forbidden_codes_by_glyph,
        &preassigned_codes_by_glyph,
        &active_hangul_codes().into_iter().collect(),
    )?;

    let mut augmented_worksets = dialogue_worksets.to_vec();
    let mut maximum_augmented_workset_slot_demand = 0;
    for (index, workset) in augmented_worksets.iter_mut().enumerate() {
        if resident_workset_indices.contains(&index) {
            for glyph in choice_glyphs {
                let code = choice_glyph_codes[glyph];
                ensure!(
                    !workset.preserved_active_codes.contains(&code),
                    "choice glyph {glyph:?} uses a code preserved by the continue prompt"
                );
                workset.target_glyphs.insert(*glyph);
                if let Some(existing) = workset.fixed_glyph_codes.insert(*glyph, code) {
                    ensure!(
                        existing == code,
                        "choice glyph {glyph:?} changes its preassigned fixed code"
                    );
                }
            }
        }
        maximum_augmented_workset_slot_demand = maximum_augmented_workset_slot_demand
            .max(workset.target_glyphs.len() + workset.preserved_active_codes.len());
    }
    ensure!(
        maximum_augmented_workset_slot_demand <= ACTIVE_HANGUL_SLOT_COUNT,
        "choice-augmented dialogue page needs {maximum_augmented_workset_slot_demand} active slots but only {ACTIVE_HANGUL_SLOT_COUNT} exist"
    );

    Ok((
        choice_glyph_codes,
        augmented_worksets,
        maximum_augmented_workset_slot_demand,
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

    #[test]
    fn save_prompt_and_front_end_results_share_one_choice_codebook() {
        let continue_prompt = "victory-and-defeat-dialogue:000";
        let mut pages = FRONT_END_RESULT_DIALOGUE_RECORD_IDS
            .iter()
            .enumerate()
            .map(|(index, id)| page(id, index))
            .collect::<Vec<_>>();
        pages.push(page(continue_prompt, 0));
        pages.push(page("unrelated:000", 0));
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
            6
        ];
        for workset in &mut worksets[..4] {
            workset.target_glyphs.extend(['기', '오']);
            workset
                .fixed_glyph_codes
                .extend([('기', 0x84), ('오', 0x85)]);
        }
        let choice_glyphs = BTreeSet::from(['예', '아', '니', '오']);
        let resident = collect_resident_workset_indices(&display, continue_prompt).unwrap();

        let (assignments, augmented, _) =
            assign_and_augment_choice_worksets(&choice_glyphs, &resident, &worksets).unwrap();

        assert_eq!(resident, BTreeSet::from([0, 1, 2, 3, 4]));
        assert_eq!(assignments[&'오'], 0x85);
        assert!(assignments.values().all(|code| *code != 0x84));
        for workset in &augmented[..5] {
            assert!(
                choice_glyphs.iter().all(|glyph| {
                    workset.fixed_glyph_codes.get(glyph) == assignments.get(glyph)
                })
            );
        }
        assert!(augmented[5].fixed_glyph_codes.is_empty());
    }

    #[test]
    fn missing_front_end_result_record_fails_closed() {
        let continue_prompt = "victory-and-defeat-dialogue:000";
        let display = MainDialogueDisplayPlan {
            canonical_record_count: 2,
            record_ids: vec![
                continue_prompt.to_owned(),
                FRONT_END_RESULT_DIALOGUE_RECORD_IDS[0].to_owned(),
            ],
            page_worksets: vec![
                page(continue_prompt, 0),
                page(FRONT_END_RESULT_DIALOGUE_RECORD_IDS[0], 0),
            ],
        };

        let error = collect_resident_workset_indices(&display, continue_prompt).unwrap_err();

        assert!(error.to_string().contains("missing"));
    }
}
