use serde::Serialize;

#[derive(Debug, Serialize)]
pub(super) struct CumulativePatchReport {
    pub(super) schema: u8,
    pub(super) source_sha1: &'static str,
    pub(super) output_sha1: String,
    pub(super) output_mapper: u16,
    pub(super) prg_size: usize,
    pub(super) chr_size: usize,
    pub(super) stage_count: usize,
    pub(super) stages: Vec<CumulativeStageReport>,
    pub(super) chapter_titles: CumulativeChapterTitleReport,
    pub(super) main_dialogue: CumulativeDialogueReport,
    pub(super) selector_chain: Vec<SelectorChainReport>,
    pub(super) original_chr_preserved: bool,
    pub(super) tracked_write_count: usize,
    pub(super) translation_input_complete: bool,
    pub(super) review_complete: bool,
    pub(super) runtime_verified: bool,
    pub(super) unresolved: Vec<&'static str>,
    pub(super) release_eligible: bool,
}

#[derive(Debug, Serialize)]
pub(super) struct CumulativeChapterTitleReport {
    pub(super) workspace_sha1: String,
    pub(super) workspace_entry_count: usize,
    pub(super) translated_entry_count: usize,
    pub(super) installed_entry_count: usize,
    pub(super) installed_chapter_indices: Vec<u8>,
    pub(super) installed_source_storage_byte_count: usize,
    pub(super) installed_output_storage_byte_count: usize,
    pub(super) original_digits_preserved: bool,
    pub(super) intro_title_table_installed: bool,
    pub(super) ending_scroll_duplicate_installed: bool,
    pub(super) review_complete: bool,
}

#[derive(Debug, Serialize)]
pub(super) struct CumulativeStageReport {
    pub(super) role: &'static str,
    pub(super) output_sha1: String,
    pub(super) report_sha1: Option<String>,
}

#[derive(Debug, Serialize)]
pub(super) struct CumulativeDialogueReport {
    pub(super) workspace_sha1: String,
    pub(super) workspace_record_count: usize,
    pub(super) workspace_filled_line_count: usize,
    pub(super) installed_record_count: usize,
    pub(super) installed_translated_line_count: usize,
    pub(super) installed_shared_page_glyph_slot_count: usize,
    pub(super) source_storage_byte_count: usize,
    pub(super) planned_storage_byte_count: usize,
    pub(super) remaining_storage_byte_count: usize,
    pub(super) lifetimes: Vec<CumulativeDialogueLifetimeReport>,
}

#[derive(Debug, Serialize)]
pub(super) struct CumulativeDialogueLifetimeReport {
    pub(super) screen_role: &'static str,
    pub(super) chapter_index: u8,
    pub(super) screen_evidence_manifest_sha1: String,
    pub(super) installed_record_count: usize,
    pub(super) installed_translated_line_count: usize,
    pub(super) source_storage_byte_count: usize,
    pub(super) planned_storage_byte_count: usize,
    pub(super) remaining_storage_byte_count: usize,
    pub(super) unique_glyph_count: usize,
    pub(super) glyph_assignment_sha1: String,
    pub(super) preserved_screen_active_code_count: usize,
    pub(super) preserved_source_active_code_count: usize,
    pub(super) preserved_active_code_count: usize,
    pub(super) temporal_sample_count: usize,
    pub(super) unique_nametable_count: usize,
    pub(super) font_physical_page: u8,
    pub(super) font_mapper_register: u8,
    pub(super) font_page_sha1: String,
    pub(super) font_page_pack_sha1: String,
}

#[derive(Debug, Serialize)]
pub(super) struct SelectorChainReport {
    pub(super) role: &'static str,
    pub(super) cpu_address: String,
    pub(super) fallback_role: &'static str,
    pub(super) admitted_chapter_indices: Vec<u8>,
}
