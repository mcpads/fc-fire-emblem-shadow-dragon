use serde::Serialize;

use super::TranslationStatus;

#[derive(Debug, Serialize)]
pub(super) struct MainDialogueGlyphWorksetReport {
    pub(super) schema: u8,
    pub(super) source_sha1: &'static str,
    pub(super) workspace_sha1: String,
    pub(super) scope: GlyphWorksetScope,
    pub(super) record_count: usize,
    pub(super) line_count: usize,
    pub(super) status_counts: GlyphWorksetStatusCounts,
    pub(super) target_glyph_occurrence_count: usize,
    pub(super) filled_glyphs: GlyphSetReport,
    pub(super) approved_glyphs: GlyphSetReport,
    pub(super) max_line_unique_glyph_count: usize,
    pub(super) max_record_unique_glyph_count: usize,
    pub(super) max_transition_chain_unique_glyph_count: usize,
    pub(super) capacity: GlyphCapacityReport,
    pub(super) unresolved: Vec<&'static str>,
    pub(super) release_eligible: bool,
}

#[derive(Debug, Serialize)]
pub(super) struct GlyphWorksetScope {
    pub(super) translation_direction: &'static str,
    pub(super) preserve_existing_english_and_digits: bool,
    pub(super) dialogue_content_emitted: bool,
    pub(super) glyph_characters_emitted: bool,
    pub(super) workspace_paths_emitted: bool,
    pub(super) approved_status: &'static str,
}

#[derive(Default, Debug, Serialize)]
pub(super) struct GlyphWorksetStatusCounts {
    pub(super) untranslated: usize,
    pub(super) in_progress: usize,
    pub(super) needs_review: usize,
    pub(super) needs_human_review: usize,
    pub(super) complete: usize,
    pub(super) filled: usize,
}

impl GlyphWorksetStatusCounts {
    pub(super) fn add(&mut self, status: TranslationStatus) {
        match status {
            TranslationStatus::Untranslated => self.untranslated += 1,
            TranslationStatus::InProgress => self.in_progress += 1,
            TranslationStatus::NeedsReview => self.needs_review += 1,
            TranslationStatus::NeedsHumanReview => self.needs_human_review += 1,
            TranslationStatus::Complete => self.complete += 1,
        }
        if status != TranslationStatus::Untranslated {
            self.filled += 1;
        }
    }

    pub(super) fn total(&self) -> usize {
        self.untranslated
            + self.in_progress
            + self.needs_review
            + self.needs_human_review
            + self.complete
    }
}

#[derive(Debug, Serialize)]
pub(super) struct GlyphSetReport {
    pub(super) unique_count: usize,
    pub(super) sorted_set_sha1: String,
}

#[derive(Debug, Serialize)]
pub(super) struct GlyphCapacityReport {
    pub(super) active_slot_count: usize,
    pub(super) translation_input_complete: bool,
    pub(super) working_set_ready: bool,
    pub(super) filled_set_fits_one_page_so_far: bool,
    pub(super) filled_transition_chains_fit_one_page_so_far: bool,
    pub(super) approved_single_page_fit: Option<bool>,
    pub(super) approved_transition_chains_fit_one_page: Option<bool>,
    pub(super) final_page_plan_eligible: bool,
}
