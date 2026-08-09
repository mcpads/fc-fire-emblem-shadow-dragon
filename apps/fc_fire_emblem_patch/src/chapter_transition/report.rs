use super::{unit_record_history::UnitRecordHistoryContract, *};

#[derive(Debug, Serialize)]
pub(super) struct ChapterTransitionReport {
    pub(super) schema: u8,
    pub(super) source_sha1: &'static str,
    pub(super) scope: Scope,
    pub(super) observed_screens: Vec<TransitionScreen>,
    pub(super) chapter_intro_contexts: ChapterIntroContextSummary,
    pub(super) chapter_titles: ChapterTitleSummary,
    pub(super) regular_save_reachability: RegularSaveReachability,
    pub(super) save_offer_no_branch: SaveOfferNoBranchContract,
    pub(super) save_complete_no_branch: SaveCompleteNoBranchContract,
    pub(super) sound_test_controls: SoundTestControlContract,
    pub(super) unit_record_history: UnitRecordHistoryContract,
    pub(super) translation_surfaces: TranslationSurfaceContracts,
    pub(super) chapter_intro_runtime_samples: Vec<ChapterIntroRuntimeSample>,
    pub(super) fixed_labels: Vec<FixedLabelBinding>,
    pub(super) source_regions: Vec<SourceRegionBinding>,
    pub(super) next_universalization_gate: &'static str,
    pub(super) unresolved: Vec<&'static str>,
    pub(super) release_eligible: bool,
}

#[derive(Debug, Serialize)]
pub(super) struct Scope {
    pub(super) translation_direction: &'static str,
    pub(super) preserve_existing_english_and_digits: bool,
    pub(super) dialogue_content_emitted: bool,
    pub(super) proof_boundary: &'static str,
}

#[derive(Debug, Serialize)]
pub(super) struct TransitionScreen {
    pub(super) route_stage: u8,
    pub(super) route_membership: &'static [&'static str],
    pub(super) screen_role: &'static str,
    pub(super) entry_condition: &'static str,
    pub(super) runtime_observed: bool,
    pub(super) input_behavior: &'static str,
    pub(super) visible_components: &'static [&'static str],
    pub(super) translation_target: &'static str,
    pub(super) preserved_original: &'static [&'static str],
    pub(super) runtime_state: RuntimeScreenState,
    pub(super) observed_chr_pair: ChrPair,
    pub(super) temporal_behavior: &'static str,
    pub(super) input_actions: &'static [InputAction],
    pub(super) focus_elements: &'static [&'static str],
    pub(super) unresolved_focus: &'static [&'static str],
}

#[derive(Debug, Serialize)]
pub(super) struct RuntimeScreenState {
    pub(super) outer_screen_state: u8,
    pub(super) outer_screen_state_hex: &'static str,
    pub(super) main_state: u8,
    pub(super) main_state_hex: &'static str,
    pub(super) victory_stage: Option<u8>,
    pub(super) dialogue_state: Option<u8>,
}

#[derive(Debug, Serialize)]
pub(super) struct ChrPair {
    pub(super) left_fd: u8,
    pub(super) left_fe: u8,
    pub(super) right_fd: u8,
    pub(super) right_fe: u8,
}

#[derive(Debug, Serialize)]
pub(super) struct InputAction {
    pub(super) input: &'static str,
    pub(super) immediate_effect: &'static str,
    pub(super) may_cause_persistent_gameplay_mutation: bool,
    pub(super) next_role: &'static str,
}

#[derive(Debug, Serialize)]
pub(super) struct ChapterIntroContextSummary {
    pub(super) prefix_code: u8,
    pub(super) prefix_code_hex: &'static str,
    pub(super) payload_destinations: [u16; 5],
    pub(super) payload_destination_hex: [&'static str; 5],
    pub(super) unique_context_count: usize,
    pub(super) first_chapter_index: u8,
    pub(super) last_chapter_index: u8,
    pub(super) chapter_index_address: u16,
    pub(super) chapter_index_address_hex: &'static str,
    pub(super) shared_non_index_payload_sha1: String,
    pub(super) source_entry_indices: Vec<Vec<usize>>,
}

#[derive(Debug, Serialize)]
pub(super) struct ChapterTitleSummary {
    pub(super) pointer_table: CodeLocation,
    pub(super) pointer_count: usize,
    pub(super) data_file_start: usize,
    pub(super) data_file_start_hex: String,
    pub(super) data_file_end_exclusive: usize,
    pub(super) data_file_end_exclusive_hex: String,
    pub(super) source_terminator: u8,
    pub(super) source_terminator_hex: &'static str,
    pub(super) protected_original_digit_count: usize,
    pub(super) composer: CodeLocation,
    pub(super) selector_address: u16,
    pub(super) selector_address_hex: &'static str,
    pub(super) translation_target: &'static str,
}

#[derive(Debug, Serialize)]
pub(super) struct RegularSaveReachability {
    pub(super) file_one_data_start_address: u16,
    pub(super) file_one_data_start_address_hex: &'static str,
    pub(super) file_one_data_end_exclusive_address: u16,
    pub(super) file_one_data_end_exclusive_address_hex: &'static str,
    pub(super) file_one_chapter_address: u16,
    pub(super) file_one_chapter_address_hex: &'static str,
    pub(super) file_one_checksum_address: u16,
    pub(super) file_one_checksum_address_hex: &'static str,
    pub(super) checksum_byte_order: &'static str,
    pub(super) checksum_algorithm: &'static str,
    pub(super) chapter_number_basis: &'static str,
    pub(super) runtime_use: &'static str,
    pub(super) natural_progression_claimed: bool,
}

#[derive(Debug, Serialize)]
pub(super) struct SaveOfferNoBranchContract {
    pub(super) screen_role: &'static str,
    pub(super) outer_screen_state_address: u16,
    pub(super) outer_screen_state_address_hex: &'static str,
    pub(super) offer_outer_screen_state: u8,
    pub(super) offer_outer_screen_state_hex: &'static str,
    pub(super) main_state_address: u16,
    pub(super) main_state_address_hex: &'static str,
    pub(super) owned_main_state_sequence: [u8; 4],
    pub(super) owned_main_state_sequence_hex: [&'static str; 4],
    pub(super) menu_depth_address: u16,
    pub(super) menu_depth_address_hex: &'static str,
    pub(super) observed_menu_depth: u8,
    pub(super) active_selection_address: u16,
    pub(super) active_selection_address_hex: &'static str,
    pub(super) default_yes_selection: u8,
    pub(super) no_selection: u8,
    pub(super) committed_result_address: u16,
    pub(super) committed_result_address_hex: &'static str,
    pub(super) no_committed_result: u8,
    pub(super) no_branch_exit_outer_state: u8,
    pub(super) no_branch_exit_outer_state_hex: &'static str,
    pub(super) no_branch_blackout_chr_pair: ChrPair,
    pub(super) persistent_save_route_entered: bool,
    pub(super) next_role: &'static str,
    pub(super) stable_sample_offsets_frames: [u16; 8],
    pub(super) stable_sample_screenshot_sha256: &'static str,
    pub(super) runtime_evidence: &'static str,
}

#[derive(Debug, Serialize)]
pub(super) struct SaveCompleteNoBranchContract {
    pub(super) screen_role: &'static str,
    pub(super) outer_screen_state_address: u16,
    pub(super) outer_screen_state_address_hex: &'static str,
    pub(super) outer_screen_state: u8,
    pub(super) outer_screen_state_hex: &'static str,
    pub(super) main_state_address: u16,
    pub(super) main_state_address_hex: &'static str,
    pub(super) main_state: u8,
    pub(super) main_state_hex: &'static str,
    pub(super) dialogue_substate_address: u16,
    pub(super) dialogue_substate_address_hex: &'static str,
    pub(super) owned_dialogue_substate_sequence: [u8; 4],
    pub(super) owned_dialogue_substate_sequence_hex: [&'static str; 4],
    pub(super) menu_depth_address: u16,
    pub(super) menu_depth_address_hex: &'static str,
    pub(super) observed_menu_depth: u8,
    pub(super) active_selection_address: u16,
    pub(super) active_selection_address_hex: &'static str,
    pub(super) default_yes_selection: u8,
    pub(super) no_selection: u8,
    pub(super) committed_result_address: u16,
    pub(super) committed_result_address_hex: &'static str,
    pub(super) no_committed_result: u8,
    pub(super) next_role: &'static str,
    pub(super) notice_chr_pair: ChrPair,
    pub(super) notice_draw_sample_offsets_frames: [u16; 8],
    pub(super) settled_notice_sample_offsets_frames: [u16; 4],
    pub(super) settled_notice_screenshot_sha256: &'static str,
    pub(super) hidden_unlock_progress_address: u16,
    pub(super) hidden_unlock_progress_address_hex: &'static str,
    pub(super) hidden_unlock_input_bytes: [u8; 6],
    pub(super) hidden_unlock_inputs: [&'static str; 6],
    pub(super) hidden_unlock_next_role: &'static str,
    pub(super) sound_test_chr_pair: ChrPair,
    pub(super) sound_test_translation_handling: &'static str,
    pub(super) runtime_evidence: &'static str,
}

#[derive(Debug, Serialize)]
pub(super) struct SoundTestControlContract {
    pub(super) screen_role: &'static str,
    pub(super) input_address: u16,
    pub(super) input_address_hex: &'static str,
    pub(super) sound_number_address: u16,
    pub(super) sound_number_address_hex: &'static str,
    pub(super) initial_sound_number: u8,
    pub(super) upper_boundary: u8,
    pub(super) upper_boundary_hex: &'static str,
    pub(super) sound_event_base_address: u16,
    pub(super) sound_event_base_address_hex: &'static str,
    pub(super) sound_event_slot_count: u8,
    pub(super) controls: Vec<SoundTestControl>,
    pub(super) downstream_families: Vec<DownstreamFamilyContract>,
    pub(super) controls_runtime_observed: bool,
    pub(super) translation_handling: &'static str,
    pub(super) proof_boundary: &'static str,
}

#[derive(Debug, Serialize)]
pub(super) struct SoundTestControl {
    pub(super) input: &'static str,
    pub(super) input_mask: u8,
    pub(super) input_mask_hex: &'static str,
    pub(super) source_effect: &'static str,
    pub(super) next_dialogue_substate: Option<u8>,
    pub(super) downstream_family_role: Option<&'static str>,
}

#[derive(Debug, Serialize)]
pub(super) struct DownstreamFamilyContract {
    pub(super) family_role: &'static str,
    pub(super) entry_dialogue_substate: u8,
    pub(super) prg_bank: u8,
    pub(super) prg_bank_hex: &'static str,
    pub(super) bank_handler_index: u8,
    pub(super) bank_handler_index_hex: &'static str,
    pub(super) entry_point: u16,
    pub(super) entry_point_hex: &'static str,
    pub(super) phase_state_address: u16,
    pub(super) phase_state_address_hex: &'static str,
    pub(super) phase_pointer_count: usize,
    pub(super) static_flow: &'static str,
    pub(super) runtime_observed: bool,
    pub(super) screen_partition_status: &'static str,
    pub(super) visible_screen_roles: &'static [&'static str],
    pub(super) translation_scope_status: &'static str,
}

#[derive(Debug, Serialize)]
pub(super) struct ChapterIntroRuntimeSample {
    pub(super) sample_role: &'static str,
    pub(super) chapter_number_one_based: u8,
    pub(super) chapter_index_zero_based: u8,
    pub(super) entry_method: &'static str,
    pub(super) left_fd_chr_page: u8,
    pub(super) left_fe_chr_page: u8,
    pub(super) right_fd_chr_page: u8,
    pub(super) right_fe_chr_page: u8,
    pub(super) portrait_visible_in_sample: bool,
    pub(super) completion_marker_phase_union_observed: bool,
    pub(super) proof_limit: &'static str,
}

#[derive(Debug, Serialize)]
pub(super) struct FixedLabelBinding {
    pub(super) screen_role: &'static str,
    pub(super) index: u8,
    pub(super) index_hex: String,
    pub(super) source_text: &'static str,
    pub(super) translation_handling: &'static str,
    pub(super) pointer: u16,
    pub(super) pointer_hex: String,
    pub(super) composer: CodeLocation,
}

#[derive(Debug, Serialize)]
pub(super) struct SourceRegionBinding {
    pub(super) role: &'static str,
    pub(super) region_kind: &'static str,
    pub(super) prg_bank: u8,
    pub(super) prg_bank_hex: String,
    pub(super) cpu_address: u16,
    pub(super) cpu_address_hex: String,
    pub(super) file_offset: usize,
    pub(super) file_offset_hex: String,
    pub(super) byte_count: usize,
    pub(super) source_sha1: String,
    pub(super) typed_instructions: Vec<TypedInstructionBinding>,
}

#[derive(Debug, Serialize)]
pub(super) struct CodeLocation {
    pub(super) prg_bank: u8,
    pub(super) prg_bank_hex: String,
    pub(super) cpu_address: u16,
    pub(super) cpu_address_hex: String,
}
