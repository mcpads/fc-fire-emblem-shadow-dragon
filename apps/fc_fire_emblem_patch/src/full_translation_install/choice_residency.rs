//! 저장 완료 대사 위에 겹치는 선택 라벨의 글리프 코드를 대사 페이지마다 고정한다.

use std::collections::{BTreeMap, BTreeSet};

use anyhow::{Result, ensure};
use serde::Serialize;

use crate::{
    chapter_transition::bind_save_complete_dialogue_records,
    choice_labels::ChoiceLabelPlan,
    dialogue_assets::MainDialogueDisplayPlan,
    font_slots::{ACTIVE_HANGUL_SLOT_COUNT, active_hangul_codes},
    mapper165::battle_codebook_plan::GlyphWorkset,
    rom::Rom,
};

use super::resident_glyph_assignment::{assign_resident_glyph_codes, assignment_sha1};

#[derive(Serialize)]
pub(super) struct ChoiceResidencyPlan {
    strategy: &'static str,
    continue_prompt_record_id: &'static str,
    resident_workset_count: usize,
    choice_glyph_count: usize,
    fixed_code_count: usize,
    maximum_augmented_workset_slot_demand: usize,
    fixed_assignment_sha1: String,
    every_choice_glyph_has_one_stable_code: bool,
    every_continue_prompt_page_contains_every_choice_glyph: bool,
    #[serde(skip)]
    pub(super) augmented_worksets: Vec<GlyphWorkset>,
    #[serde(skip)]
    pub(super) choice_glyph_codes: BTreeMap<char, u8>,
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

    let resident_workset_indices = display
        .page_worksets
        .iter()
        .enumerate()
        .filter_map(|(index, workset)| {
            (workset.record_id == records.continue_prompt).then_some(index)
        })
        .collect::<Vec<_>>();
    ensure!(
        !resident_workset_indices.is_empty(),
        "save-complete continue prompt has no visible dialogue workset"
    );

    let mut forbidden_codes_by_glyph = choice_glyphs
        .iter()
        .copied()
        .map(|glyph| (glyph, BTreeSet::new()))
        .collect::<BTreeMap<_, _>>();
    let mut preassigned_codes_by_glyph = BTreeMap::<char, BTreeSet<u8>>::new();
    for workset_index in &resident_workset_indices {
        let workset = &dialogue_worksets[*workset_index];
        for glyph in &choice_glyphs {
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

    let resident_workset_indices = resident_workset_indices
        .into_iter()
        .collect::<BTreeSet<_>>();
    let mut augmented_worksets = dialogue_worksets.to_vec();
    let mut maximum_augmented_workset_slot_demand = 0;
    for (index, workset) in augmented_worksets.iter_mut().enumerate() {
        if resident_workset_indices.contains(&index) {
            for glyph in &choice_glyphs {
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

    Ok(ChoiceResidencyPlan {
        strategy: "assign one injective choice-label codebook across every visible page of the source-bound save-complete continue prompt",
        continue_prompt_record_id: records.continue_prompt,
        resident_workset_count: resident_workset_indices.len(),
        choice_glyph_count: choice_glyphs.len(),
        fixed_code_count: choice_glyph_codes.len(),
        maximum_augmented_workset_slot_demand,
        fixed_assignment_sha1: assignment_sha1(&choice_glyph_codes),
        every_choice_glyph_has_one_stable_code: true,
        every_continue_prompt_page_contains_every_choice_glyph: true,
        augmented_worksets,
        choice_glyph_codes,
    })
}
