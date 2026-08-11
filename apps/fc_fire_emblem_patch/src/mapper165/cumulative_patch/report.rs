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
    pub(super) front_end_menu: CumulativeFrontEndMenuReport,
    pub(super) playable_unit_names: CumulativeUnitNameReport,
    pub(super) automatic_class_profiles: CumulativeClassProfileReport,
    pub(super) weapon_shop_shared_text: CumulativeWeaponShopSharedTextReport,
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
pub(super) struct CumulativeWeaponShopSharedTextReport {
    pub(super) screen_role: &'static str,
    pub(super) fixed_text_workspace_sha1: String,
    pub(super) choice_label_workspace_sha1: String,
    pub(super) installed_item_name_count: usize,
    pub(super) installed_choice_label_count: usize,
    pub(super) projected_item_pointer_count: usize,
    pub(super) item_string_byte_count: usize,
    pub(super) choice_string_byte_count: usize,
    pub(super) shared_page_unique_glyph_count: usize,
    pub(super) added_glyph_count: usize,
    pub(super) glyph_assignment_sha1: String,
    pub(super) font_physical_page: u8,
    pub(super) font_mapper_register: u8,
    pub(super) font_page_sha1: String,
    pub(super) font_page_pack_sha1: String,
    pub(super) item_list_pointer_selector_installed: bool,
    pub(super) selected_item_pointer_selector_installed: bool,
    pub(super) choice_pointer_selector_installed: bool,
    pub(super) unconverted_consumers_fallback_to_source_tables: bool,
    pub(super) runtime_evidence_manifest_sha1: String,
    pub(super) runtime_sample_count: usize,
    pub(super) runtime_unique_image_count: usize,
    pub(super) runtime_bound_to_build: bool,
    pub(super) review_complete: bool,
}

#[derive(Debug, Serialize)]
pub(super) struct CumulativeClassProfileReport {
    pub(super) workspace_sha1: String,
    pub(super) workspace_entry_count: usize,
    pub(super) installed_entry_count: usize,
    pub(super) installed_description_line_count: usize,
    pub(super) installed_source_storage_byte_count: usize,
    pub(super) installed_output_storage_byte_count: usize,
    pub(super) total_unique_glyph_count: usize,
    pub(super) page_unique_glyph_counts: [usize; 2],
    pub(super) glyph_assignment_sha1s: [String; 2],
    pub(super) font_physical_pages: [u8; 2],
    pub(super) font_mapper_registers: [u8; 2],
    pub(super) font_page_sha1s: [String; 2],
    pub(super) font_page_pack_sha1: String,
    pub(super) screen_evidence_manifest_sha1: String,
    pub(super) temporal_sample_count: usize,
    pub(super) unique_image_count: usize,
    pub(super) runtime_evidence_manifest_sha1: String,
    pub(super) runtime_sample_count: usize,
    pub(super) runtime_unique_image_count: usize,
    pub(super) visible_code_count: usize,
    pub(super) preserved_active_code_count: usize,
    pub(super) original_english_digits_and_ui_preserved: bool,
    pub(super) profile_index_page_selector_installed: bool,
    pub(super) runtime_bound_to_build: bool,
    pub(super) review_complete: bool,
}

#[derive(Debug, Serialize)]
pub(super) struct CumulativeUnitNameReport {
    pub(super) workspace_sha1: String,
    pub(super) workspace_entry_count: usize,
    pub(super) unique_glyph_count: usize,
    pub(super) roster_projection_byte_count: usize,
    pub(super) unit_ui_projection_byte_count: usize,
    pub(super) roster_assignment_sha1: String,
    pub(super) unit_ui_assignment_sha1: String,
    pub(super) roster_page_pack_sha1: String,
    pub(super) unit_ui_page_pack_sha1: String,
    pub(super) unit_ui_font_physical_page: u8,
    pub(super) unit_ui_font_mapper_register: u8,
    pub(super) screen_evidence_manifest_sha1: String,
    pub(super) temporal_sample_count: usize,
    pub(super) unique_nametable_count: usize,
    pub(super) preserved_unit_ui_code_count: usize,
    pub(super) roster_projection_installed: bool,
    pub(super) unit_summary_projection_installed: bool,
    pub(super) source_battle_and_ending_table_preserved: bool,
    pub(super) runtime_bound_to_build: bool,
    pub(super) review_complete: bool,
}

#[derive(Debug, Serialize)]
pub(super) struct CumulativeFrontEndMenuReport {
    pub(super) workspace_sha1: String,
    pub(super) workspace_entry_count: usize,
    pub(super) installed_entry_count: usize,
    pub(super) installed_source_storage_byte_count: usize,
    pub(super) installed_output_storage_byte_count: usize,
    pub(super) original_english_and_digits_preserved: bool,
    pub(super) screen_evidence_manifest_sha1: String,
    pub(super) temporal_sample_count: usize,
    pub(super) unique_nametable_count: usize,
    pub(super) unique_glyph_count: usize,
    pub(super) glyph_assignment_sha1: String,
    pub(super) preserved_screen_active_code_count: usize,
    pub(super) preserved_source_active_code_count: usize,
    pub(super) preserved_active_code_count: usize,
    pub(super) font_physical_page: u8,
    pub(super) font_mapper_register: u8,
    pub(super) font_page_sha1: String,
    pub(super) font_page_pack_sha1: String,
    pub(super) central_fe_companion_refresh_routed: bool,
    pub(super) no_save_source_lifetime_bound: bool,
    pub(super) runtime_variants_bound_to_build: bool,
    pub(super) review_complete: bool,
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
    pub(super) runtime_evidence_manifest_sha1: Option<String>,
    pub(super) runtime_sample_count: usize,
    pub(super) runtime_unique_image_count: usize,
    pub(super) runtime_bound_to_build: bool,
}

#[derive(Debug, Serialize)]
pub(super) struct SelectorChainReport {
    pub(super) role: &'static str,
    pub(super) cpu_address: String,
    pub(super) fallback_role: &'static str,
    pub(super) admitted_chapter_indices: Vec<u8>,
}
