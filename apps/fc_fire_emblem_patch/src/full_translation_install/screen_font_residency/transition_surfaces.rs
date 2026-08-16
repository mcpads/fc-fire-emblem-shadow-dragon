//! 장 저장과 엔딩 전적의 고정 글꼴 수명을 중앙 페이지 계획에 결속한다.

use std::collections::BTreeSet;

use anyhow::{Result, ensure};
use serde::Serialize;

use crate::{
    chapter_transition::{ChapterTitlePlan, TransitionTranslationPlans},
    choice_labels::ChoiceLabelPlan,
};

use super::{
    CHAPTER_SAVE_OFFER_COMPOSITE_STATE, COMPOSITE_FONT_RESIDENCY_POLICIES, ScreenFontPageRole,
    ScreenFontResidencyPolicy,
};
use crate::full_translation_install::consumer_codebook::ConsumerCodebookPlan;

const CHAPTER_SAVE_PAGE_ID: &str = "chapter_save_offer";
const ENDING_RECORD_PAGE_ID: &str = "ending_chapter_record";

pub(super) struct TransitionSurfaceInputs<'a> {
    pub(super) consumer_codebook: &'a ConsumerCodebookPlan,
    pub(super) chapter_titles: &'a ChapterTitlePlan,
    pub(super) choices: &'a ChoiceLabelPlan,
    pub(super) transitions: &'a TransitionTranslationPlans,
}

#[derive(Serialize)]
pub(super) struct TransitionSurfacePlan {
    schema: u8,
    strategy: &'static str,
    chapter_save_choice_count: usize,
    chapter_save_required_glyph_count: usize,
    ending_chapter_title_count: usize,
    ending_required_title_glyph_count: usize,
    ending_required_label_glyph_count: usize,
    chapter_save_policy_matches_static_page: bool,
    chapter_save_page_contains_the_offer_and_both_choices: bool,
    ending_page_contains_every_chapter_title_and_record_label: bool,
    #[serde(skip)]
    chapter_save_route: u8,
    #[serde(skip)]
    ending_record_route: u8,
}

impl TransitionSurfacePlan {
    pub(super) fn chapter_save_route(&self) -> u8 {
        self.chapter_save_route
    }

    pub(super) fn ending_record_route(&self) -> u8 {
        self.ending_record_route
    }
}

pub(super) fn plan_transition_surfaces(
    inputs: TransitionSurfaceInputs<'_>,
) -> Result<TransitionSurfacePlan> {
    ensure!(
        COMPOSITE_FONT_RESIDENCY_POLICIES
            .iter()
            .find_map(|(state, policy)| {
                (*state == CHAPTER_SAVE_OFFER_COMPOSITE_STATE).then_some(*policy)
            })
            == Some(ScreenFontResidencyPolicy::Static(
                ScreenFontPageRole::ChapterSaveOffer
            )),
        "chapter-save screen disagrees with the central font residency policy"
    );
    ensure!(
        inputs.choices.entries.len() == 2
            && inputs.transitions.save_offer.entry_count == 1
            && inputs.chapter_titles.entries.len() == 25
            && inputs.transitions.ending_record.entry_count == 1,
        "chapter-save or ending screen font population changed"
    );

    let chapter_save_glyphs = inputs
        .transitions
        .save_offer
        .target_glyphs
        .union(&inputs.choices.unique_glyphs())
        .copied()
        .collect::<BTreeSet<_>>();
    inputs.consumer_codebook.validate_static_page_residency(
        CHAPTER_SAVE_PAGE_ID,
        &chapter_save_glyphs,
        &BTreeSet::new(),
    )?;

    let ending_title_glyphs = inputs.chapter_titles.unique_glyphs();
    let ending_label_glyphs = inputs.transitions.ending_record.target_glyphs.clone();
    inputs.consumer_codebook.validate_static_page_residency(
        ENDING_RECORD_PAGE_ID,
        &ending_label_glyphs,
        &ending_title_glyphs,
    )?;

    Ok(TransitionSurfacePlan {
        schema: 1,
        strategy: "bind chapter-save and ending-record simultaneous text surfaces to their central static consumer pages",
        chapter_save_choice_count: inputs.choices.entries.len(),
        chapter_save_required_glyph_count: chapter_save_glyphs.len(),
        ending_chapter_title_count: inputs.chapter_titles.entries.len(),
        ending_required_title_glyph_count: ending_title_glyphs.len(),
        ending_required_label_glyph_count: ending_label_glyphs.len(),
        chapter_save_policy_matches_static_page: true,
        chapter_save_page_contains_the_offer_and_both_choices: true,
        ending_page_contains_every_chapter_title_and_record_label: true,
        chapter_save_route: inputs
            .consumer_codebook
            .mapper_route_for(CHAPTER_SAVE_PAGE_ID)?,
        ending_record_route: inputs
            .consumer_codebook
            .mapper_route_for(ENDING_RECORD_PAGE_ID)?,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn chapter_save_state_uses_the_declared_static_page_role() {
        assert_eq!(
            COMPOSITE_FONT_RESIDENCY_POLICIES
                .iter()
                .find_map(|(state, policy)| {
                    (*state == CHAPTER_SAVE_OFFER_COMPOSITE_STATE).then_some(*policy)
                }),
            Some(ScreenFontResidencyPolicy::Static(
                ScreenFontPageRole::ChapterSaveOffer
            ))
        );
    }
}
