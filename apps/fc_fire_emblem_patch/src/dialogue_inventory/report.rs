use super::*;

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct MainDialogueStorageRecord {
    pub table_id: &'static str,
    pub source_prg_bank: u8,
    pub canonical_entry_index: usize,
    pub entry_indices: Vec<usize>,
    pub pointer_file_offsets: Vec<usize>,
    pub pointer_cpu_address: u16,
    pub file_offset: usize,
    pub end_file_offset_exclusive: usize,
    pub storage_byte_count: usize,
    pub storage_sha1: String,
    pub prefix_byte_count: usize,
    pub boundary_control: u8,
    pub literal_file_offsets: Vec<usize>,
    pub lines: Vec<MainDialogueStorageLine>,
}

#[derive(Debug)]
pub(crate) struct MainDialogueStorageInspection {
    pub(crate) records: Vec<MainDialogueStorageRecord>,
    pub(crate) safe_japanese_translation_source_byte_count: usize,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct MainDialogueStorageLine {
    pub file_offset: usize,
    pub storage_byte_count: usize,
    pub storage_sha1: String,
    pub line_end_control: u8,
    pub literal_file_offsets: Vec<usize>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct ChapterIntroContextBinding {
    pub(crate) entry_indices: Vec<usize>,
    pub(crate) file_offset: usize,
    pub(crate) prefix_payload: [u8; OPTIONAL_PREFIX_BYTE_COUNT - 1],
    pub(crate) chapter_index: u8,
}

#[derive(Debug, Serialize)]
pub(crate) struct ShopDialogueTableBinding {
    pub(crate) table_id: &'static str,
    pub(crate) source_prg_bank: u8,
    pub(crate) source_prg_bank_hex: String,
    pub(crate) directory_selector: u8,
    pub(crate) directory_selector_hex: String,
    pub(crate) directory_entry_cpu_address: u16,
    pub(crate) directory_entry_cpu_address_hex: String,
    pub(crate) pointer_table_cpu_address: u16,
    pub(crate) pointer_table_cpu_address_hex: String,
    pub(crate) pointer_table_sha1: String,
    pub(crate) pointer_count: usize,
    pub(crate) unique_target_count: usize,
    pub(crate) first_entry_pointer_cpu_address: u16,
    pub(crate) first_entry_pointer_cpu_address_hex: String,
}

#[derive(Debug, Serialize)]
pub(crate) struct TranslationSurfaceDialogueTableBinding {
    pub(crate) table_id: &'static str,
    pub(crate) source_prg_bank: u8,
    pub(crate) source_prg_bank_hex: String,
    pub(crate) pointer_table_cpu_address: u16,
    pub(crate) pointer_table_cpu_address_hex: String,
    pub(crate) pointer_table_sha1: String,
    pub(crate) pointer_count: usize,
    pub(crate) unique_target_count: usize,
    pub(crate) consumer_binding_status: &'static str,
    pub(crate) directory_selector: Option<u8>,
    pub(crate) directory_selector_hex: Option<String>,
    pub(crate) separate_loader_cpu_address: Option<u16>,
    pub(crate) separate_loader_cpu_address_hex: Option<String>,
    pub(crate) proven_record_count: Option<usize>,
    pub(crate) unique_record_storage_byte_count: Option<usize>,
    pub(crate) unreferenced_record_count: Option<usize>,
    pub(crate) literal_inventory: TranslationSurfaceLiteralInventory,
    #[serde(skip)]
    pub(crate) literal_file_offsets: BTreeSet<usize>,
}

#[derive(Debug, Serialize)]
pub(crate) struct DialogueStructureReport {
    pub(crate) schema_version: u8,
    pub(crate) scope: ReportScope,
    pub(crate) summary: ReportSummary,
    pub(crate) main_dialogue_state_machine: MainDialogueStateMachineReport,
    pub(crate) battle_dialogue_state_machine: BattleDialogueStateMachineReport,
    pub(crate) main_dialogue_graph: MainDialogueGraphReport,
    pub(crate) tables: Vec<DialogueTableReport>,
    pub(crate) unknowns: Vec<&'static str>,
}

#[derive(Debug, Serialize)]
pub(crate) struct ReportScope {
    pub(crate) source_sha1: &'static str,
    pub(crate) translation_direction: &'static str,
    pub(crate) preserve_existing_english: bool,
    pub(crate) proof_boundary: &'static str,
}

#[derive(Debug, Serialize)]
pub(crate) struct ReportSummary {
    pub(crate) table_count: usize,
    pub(crate) directory_bound_table_count: usize,
    pub(crate) separate_consumer_bound_table_count: usize,
    pub(crate) consumer_bound_table_count: usize,
    pub(crate) unresolved_consumer_table_count: usize,
    pub(crate) pointer_count: usize,
    pub(crate) unique_target_count: usize,
    pub(crate) unique_script_entry_count: usize,
    pub(crate) handler_target_entry_count: usize,
    pub(crate) main_first_line_count: usize,
    pub(crate) max_main_first_line_storage_byte_count: usize,
    pub(crate) main_first_line_japanese_literal_byte_count: usize,
    pub(crate) main_first_line_non_japanese_literal_byte_count: usize,
    pub(crate) main_first_line_protected_original_alphanumeric_literal_byte_count: usize,
    pub(crate) main_first_line_end_control_counts: Vec<ControlUsageReport>,
    pub(crate) main_linear_segment_count: usize,
    pub(crate) main_linear_line_count: usize,
    pub(crate) max_main_linear_segment_line_count: usize,
    pub(crate) main_linear_segment_japanese_literal_byte_count: usize,
    pub(crate) main_linear_segment_non_japanese_literal_byte_count: usize,
    pub(crate) main_linear_segment_protected_original_alphanumeric_literal_byte_count: usize,
    pub(crate) main_unique_japanese_literal_storage_byte_count: usize,
    pub(crate) main_unique_non_japanese_literal_storage_byte_count: usize,
    pub(crate) main_literal_kind_conflict_storage_byte_count: usize,
    pub(crate) main_literal_structural_conflict_storage_byte_count: usize,
    pub(crate) main_safe_japanese_translation_source_byte_count: usize,
    pub(crate) main_translation_view_line_count: usize,
    pub(crate) main_translation_view_safe_japanese_source_byte_count: usize,
    pub(crate) main_transition_target_record_count: usize,
    pub(crate) main_linear_segment_boundary_control_counts: Vec<ControlUsageReport>,
    pub(crate) main_linear_segment_transition_count: usize,
    pub(crate) main_record_count: usize,
    pub(crate) main_record_consumed_storage_byte_count: usize,
    pub(crate) main_record_unique_storage_byte_count: usize,
    pub(crate) main_record_shared_storage_byte_count: usize,
    pub(crate) main_record_overlapping_pair_count: usize,
    pub(crate) max_main_record_overlap_depth: usize,
    pub(crate) max_main_record_storage_byte_count: usize,
    pub(crate) battle_pointer_referenced_record_count: usize,
    pub(crate) battle_unreferenced_record_count: usize,
    pub(crate) battle_pointer_referenced_storage_byte_count: usize,
    pub(crate) battle_physical_record_storage_byte_count: usize,
    pub(crate) alias_group_count: usize,
    pub(crate) aliased_entry_count: usize,
}

#[derive(Debug, Serialize)]
pub(crate) struct DialogueTableReport {
    pub(crate) id: &'static str,
    pub(crate) role: &'static str,
    pub(crate) source_prg_bank: u8,
    pub(crate) source_prg_bank_hex: String,
    pub(crate) pointer_table_cpu_address: u16,
    pub(crate) pointer_table_cpu_address_hex: String,
    pub(crate) pointer_table_file_offset: usize,
    pub(crate) pointer_table_file_offset_hex: String,
    pub(crate) pointer_table_file_end_exclusive: usize,
    pub(crate) pointer_table_file_end_exclusive_hex: String,
    pub(crate) pointer_table_byte_count: usize,
    pub(crate) pointer_table_sha1: String,
    pub(crate) pointer_count: usize,
    pub(crate) unique_target_count: usize,
    pub(crate) unique_script_entry_count: usize,
    pub(crate) handler_target_entry_count: usize,
    pub(crate) alias_group_count: usize,
    pub(crate) aliased_entry_count: usize,
    pub(crate) main_record_prefix_summary: Option<MainRecordPrefixSummary>,
    pub(crate) main_first_line_summary: Option<MainFirstLineSummary>,
    pub(crate) main_linear_segment_summary: Option<MainLinearSegmentSummary>,
    pub(crate) main_record_storage_summary: Option<MainRecordStorageSummary>,
    pub(crate) battle_record_storage_summary: Option<BattleDialogueRecordStorageSummary>,
    pub(crate) data_file_start: usize,
    pub(crate) data_file_start_hex: String,
    pub(crate) directory_binding: Option<DirectoryBindingReport>,
    pub(crate) separate_consumer_binding: Option<SeparateConsumerBindingReport>,
    pub(crate) consumer_binding_status: &'static str,
    pub(crate) entries: Vec<DialogueEntryReport>,
}

#[derive(Debug, Serialize)]
pub(crate) struct DirectoryBindingReport {
    pub(crate) selector: u8,
    pub(crate) selector_hex: String,
    pub(crate) directory_group: u8,
    pub(crate) directory_entry_cpu_address: u16,
    pub(crate) directory_entry_cpu_address_hex: String,
    pub(crate) directory_entry_file_offset: usize,
    pub(crate) directory_entry_file_offset_hex: String,
    pub(crate) resolved_pointer_table_cpu_address: u16,
    pub(crate) resolved_pointer_table_cpu_address_hex: String,
    pub(crate) selector_use: Option<DirectorySelectorUseReport>,
}

#[derive(Debug, Serialize)]
pub(crate) struct DirectorySelectorUseReport {
    pub(crate) role: &'static str,
    pub(crate) prg_bank: u8,
    pub(crate) prg_bank_hex: String,
    pub(crate) cpu_address: u16,
    pub(crate) cpu_address_hex: String,
    pub(crate) file_offset: usize,
    pub(crate) file_offset_hex: String,
    pub(crate) code_byte_count: usize,
    pub(crate) code_sha1: String,
}

#[derive(Debug, Serialize)]
pub(crate) struct SeparateConsumerBindingReport {
    pub(crate) prg_bank: u8,
    pub(crate) prg_bank_hex: String,
    pub(crate) loader_cpu_address: u16,
    pub(crate) loader_cpu_address_hex: String,
    pub(crate) loader_file_offset: usize,
    pub(crate) loader_file_offset_hex: String,
    pub(crate) loader_code_sha1: String,
    pub(crate) table_set_selector: &'static str,
    pub(crate) table_set_index: u8,
    pub(crate) entry_index_selector: &'static str,
    pub(crate) destination_pointer: &'static str,
    pub(crate) table_root_cell_cpu_address: u16,
    pub(crate) table_root_cell_cpu_address_hex: String,
    pub(crate) table_root_cell_file_offset: usize,
    pub(crate) table_root_cell_file_offset_hex: String,
    pub(crate) resolved_pointer_table_cpu_address: u16,
    pub(crate) resolved_pointer_table_cpu_address_hex: String,
}

#[derive(Debug, Serialize)]
pub(crate) struct DialogueEntryReport {
    pub(crate) index: usize,
    pub(crate) pointer_cpu_address: u16,
    pub(crate) pointer_cpu_address_hex: String,
    pub(crate) target_kind: &'static str,
    pub(crate) file_offset: usize,
    pub(crate) file_offset_hex: String,
    pub(crate) handler_role: Option<&'static str>,
    pub(crate) alias_entry_indices: Vec<usize>,
    pub(crate) main_record_prefix: Option<MainRecordPrefixReport>,
    pub(crate) main_first_line: Option<MainLineReport>,
    pub(crate) main_linear_segment: Option<MainLinearSegmentReport>,
    pub(crate) main_record_storage: Option<MainRecordStorageReport>,
    pub(crate) battle_record_storage: Option<BattleDialogueRecordStorageReport>,
}

#[derive(Debug, Serialize)]
pub(crate) struct BattleDialogueStateMachineReport {
    pub(crate) prg_bank: u8,
    pub(crate) prg_bank_hex: String,
    pub(crate) state_address: u16,
    pub(crate) state_address_hex: String,
    pub(crate) dispatcher_cpu_address: u16,
    pub(crate) dispatcher_cpu_address_hex: String,
    pub(crate) handler_table_cpu_address: u16,
    pub(crate) handler_table_cpu_address_hex: String,
    pub(crate) handler_table_sha1: String,
    pub(crate) handler_count: usize,
    pub(crate) handlers: Vec<DialogueStateHandlerReport>,
    pub(crate) fixed_record_header_byte_count: usize,
    pub(crate) record_end_control: u8,
    pub(crate) record_end_control_hex: String,
    pub(crate) dynamic_value_control: u8,
    pub(crate) dynamic_value_control_hex: String,
    pub(crate) dynamic_selector_operand_byte_count: usize,
    pub(crate) dynamic_selector_max: u8,
    pub(crate) code_regions: Vec<BattleDialogueCodeRegionReport>,
}

#[derive(Debug, Serialize)]
pub(crate) struct BattleDialogueCodeRegionReport {
    pub(crate) role: &'static str,
    pub(crate) cpu_address: u16,
    pub(crate) cpu_address_hex: String,
    pub(crate) file_offset: usize,
    pub(crate) file_offset_hex: String,
    pub(crate) byte_count: usize,
    pub(crate) code_sha1: String,
    pub(crate) typed_instruction_count: usize,
}

#[derive(Debug, Clone, Serialize)]
pub(crate) struct BattleDialogueRecordStorageReport {
    pub(crate) file_offset: usize,
    pub(crate) file_offset_hex: String,
    pub(crate) end_file_offset_exclusive: usize,
    pub(crate) end_file_offset_exclusive_hex: String,
    pub(crate) storage_byte_count: usize,
    pub(crate) storage_sha1: String,
    pub(crate) header_hex: String,
    pub(crate) dynamic_selector_values: Vec<u8>,
    pub(crate) control_counts: Vec<ControlUsageReport>,
    #[serde(skip)]
    pub(crate) literal_file_offsets: Vec<usize>,
}

#[derive(Debug, Serialize)]
pub(crate) struct BattleDialogueRecordStorageSummary {
    pub(crate) pointer_referenced_record_count: usize,
    pub(crate) unreferenced_record_count: usize,
    pub(crate) consumed_storage_byte_count: usize,
    pub(crate) unique_storage_byte_count: usize,
    pub(crate) shared_storage_byte_count: usize,
    pub(crate) overlapping_record_pair_count: usize,
    pub(crate) max_overlap_depth: usize,
    pub(crate) max_storage_byte_count: usize,
    pub(crate) physical_record_count: usize,
    pub(crate) physical_record_storage_byte_count: usize,
    pub(crate) physical_data_file_end_exclusive: usize,
    pub(crate) physical_data_file_end_exclusive_hex: String,
    pub(crate) header_counts: Vec<BattleDialogueHeaderCount>,
    pub(crate) physical_control_counts: Vec<ControlUsageReport>,
    pub(crate) unreferenced_records: Vec<BattleDialogueRecordStorageReport>,
}

#[derive(Debug, Serialize)]
pub(crate) struct BattleDialogueHeaderCount {
    pub(crate) header_hex: String,
    pub(crate) count: usize,
}

#[derive(Debug, Serialize)]
pub(crate) struct MainDialogueStateMachineReport {
    pub(crate) prg_bank: u8,
    pub(crate) prg_bank_hex: String,
    pub(crate) state_address: u16,
    pub(crate) state_address_hex: String,
    pub(crate) dispatcher_cpu_address: u16,
    pub(crate) dispatcher_cpu_address_hex: String,
    pub(crate) dispatcher_file_offset: usize,
    pub(crate) dispatcher_file_offset_hex: String,
    pub(crate) dispatcher_code_sha1: String,
    pub(crate) dispatch_helper_cpu_address: u16,
    pub(crate) dispatch_helper_cpu_address_hex: String,
    pub(crate) handler_table_cpu_address: u16,
    pub(crate) handler_table_cpu_address_hex: String,
    pub(crate) handler_table_file_offset: usize,
    pub(crate) handler_table_file_offset_hex: String,
    pub(crate) handler_table_sha1: String,
    pub(crate) handler_count: usize,
    pub(crate) handlers: Vec<DialogueStateHandlerReport>,
    pub(crate) record_prefix_contract: MainRecordPrefixContract,
    pub(crate) caller_handoff_contract: CallerHandoffContract,
    pub(crate) code_regions: Vec<CodeRegionReport>,
}

#[derive(Debug, Serialize)]
pub(crate) struct DialogueStateHandlerReport {
    pub(crate) state: usize,
    pub(crate) cpu_address: u16,
    pub(crate) cpu_address_hex: String,
    pub(crate) file_offset: usize,
    pub(crate) file_offset_hex: String,
    pub(crate) structural_role: &'static str,
    pub(crate) alias_state_indices: Vec<usize>,
}

#[derive(Debug, Serialize)]
pub(crate) struct MainRecordPrefixContract {
    pub(crate) optional_e5_prefix_code: u8,
    pub(crate) optional_e5_prefix_code_hex: String,
    pub(crate) optional_e5_prefix_byte_count: usize,
    pub(crate) fixed_record_header_byte_count: usize,
    pub(crate) optional_e8_prefix_code: u8,
    pub(crate) optional_e8_prefix_code_hex: String,
    pub(crate) optional_e8_prefix_byte_count: usize,
}

#[derive(Debug, Serialize)]
pub(crate) struct CallerHandoffContract {
    pub(crate) control_code: u8,
    pub(crate) control_code_hex: String,
    pub(crate) decoder_flag_address: u16,
    pub(crate) decoder_flag_address_hex: String,
    pub(crate) caller_flag_address: u16,
    pub(crate) caller_flag_address_hex: String,
    pub(crate) handoff_state: u8,
    pub(crate) resume_state: u8,
    pub(crate) pointer_resolver_cpu_address: u16,
    pub(crate) pointer_resolver_cpu_address_hex: String,
    pub(crate) pointer_resolver_file_offset: usize,
    pub(crate) pointer_resolver_file_offset_hex: String,
    pub(crate) pointer_resolver_code_sha1: String,
    pub(crate) caller_flag_load_candidate_count: usize,
    pub(crate) direct_dispatch_bound_observer_count: usize,
    pub(crate) direct_dispatch_unbound_observer_count: usize,
    pub(crate) confirmed_direct_dispatch_binding_count: usize,
    pub(crate) confirmed_direct_handler_slot_count: usize,
    pub(crate) caller_flag_load_candidates: Vec<CallerHandoffObserverReport>,
}

#[derive(Debug, Serialize)]
pub(crate) struct CallerHandoffObserverReport {
    pub(crate) prg_bank: u8,
    pub(crate) prg_bank_hex: String,
    pub(crate) cpu_address: u16,
    pub(crate) cpu_address_hex: String,
    pub(crate) file_offset: usize,
    pub(crate) file_offset_hex: String,
    pub(crate) instruction: &'static str,
    pub(crate) handler_cpu_address: u16,
    pub(crate) handler_cpu_address_hex: String,
    pub(crate) direct_dispatch_bindings: Vec<CallerHandoffDispatchBindingReport>,
}

#[derive(Debug, Serialize)]
pub(crate) struct CallerHandoffDispatchBindingReport {
    pub(crate) state_address: u16,
    pub(crate) state_address_hex: String,
    pub(crate) dispatcher_cpu_address: u16,
    pub(crate) dispatcher_cpu_address_hex: String,
    pub(crate) dispatcher_file_offset: usize,
    pub(crate) dispatcher_file_offset_hex: String,
    pub(crate) handler_table_cpu_address: u16,
    pub(crate) handler_table_cpu_address_hex: String,
    pub(crate) handler_table_file_offset: usize,
    pub(crate) handler_table_file_offset_hex: String,
    pub(crate) handler_table_sha1: String,
    pub(crate) handler_count: usize,
    pub(crate) handler_cpu_address: u16,
    pub(crate) handler_cpu_address_hex: String,
    pub(crate) handler_state_indices: Vec<usize>,
}

#[derive(Debug, Serialize)]
pub(crate) struct CodeRegionReport {
    pub(crate) role: &'static str,
    pub(crate) cpu_address: u16,
    pub(crate) cpu_address_hex: String,
    pub(crate) file_offset: usize,
    pub(crate) file_offset_hex: String,
    pub(crate) byte_count: usize,
    pub(crate) code_sha1: String,
}

#[derive(Debug, Serialize)]
pub(crate) struct MainRecordPrefixSummary {
    pub(crate) unique_target_count: usize,
    pub(crate) e5_prefix_unique_target_count: usize,
    pub(crate) e8_prefix_unique_target_count: usize,
    pub(crate) both_optional_prefixes_unique_target_count: usize,
    pub(crate) no_optional_prefix_unique_target_count: usize,
}

#[derive(Debug, Clone, Serialize)]
pub(crate) struct MainRecordPrefixReport {
    pub(crate) e5_prefix_present: bool,
    pub(crate) e5_prefix_byte_count: usize,
    pub(crate) fixed_record_header_file_offset: usize,
    pub(crate) fixed_record_header_file_offset_hex: String,
    pub(crate) fixed_record_header_byte_count: usize,
    pub(crate) e8_prefix_present: bool,
    pub(crate) e8_prefix_byte_count: usize,
    pub(crate) first_line_file_offset: usize,
    pub(crate) first_line_file_offset_hex: String,
    pub(crate) total_prefix_byte_count: usize,
}

#[derive(Debug, Clone, Serialize)]
pub(crate) struct MainLineReport {
    pub(crate) file_offset: usize,
    pub(crate) file_offset_hex: String,
    pub(crate) storage_byte_count: usize,
    pub(crate) storage_sha1: String,
    pub(crate) current_pointer_advance_bytes: usize,
    pub(crate) literal_byte_count: usize,
    pub(crate) japanese_literal_byte_count: usize,
    pub(crate) non_japanese_literal_byte_count: usize,
    #[serde(skip)]
    pub(crate) literal_file_offsets: Vec<usize>,
    pub(crate) protected_original_alphanumeric_literal_byte_count: usize,
    pub(crate) control_token_count: usize,
    pub(crate) inline_operand_byte_count: usize,
    pub(crate) transition_target_byte_count: usize,
    pub(crate) control_counts: Vec<ControlUsageReport>,
    pub(crate) line_end_control: u8,
    pub(crate) line_end_control_hex: String,
    pub(crate) transition_target: Option<TransitionTargetReport>,
}

#[derive(Debug, Clone, Serialize)]
pub(crate) struct MainLinearSegmentReport {
    pub(crate) start_file_offset: usize,
    pub(crate) start_file_offset_hex: String,
    pub(crate) line_count: usize,
    pub(crate) storage_byte_count: usize,
    pub(crate) storage_sha1: String,
    pub(crate) japanese_literal_byte_count: usize,
    pub(crate) non_japanese_literal_byte_count: usize,
    pub(crate) protected_original_alphanumeric_literal_byte_count: usize,
    pub(crate) boundary_control: u8,
    pub(crate) boundary_control_hex: String,
    pub(crate) boundary_kind: &'static str,
    pub(crate) transition_target: Option<TransitionTargetReport>,
    pub(crate) lines: Vec<MainLineReport>,
}

#[derive(Debug, Clone, Serialize)]
pub(crate) struct TransitionTargetReport {
    pub(crate) selector: u8,
    pub(crate) selector_hex: String,
    pub(crate) target_table_id: &'static str,
    pub(crate) target_entry_index: usize,
}

#[derive(Debug, Clone, Serialize)]
pub(crate) struct ControlUsageReport {
    pub(crate) code: u8,
    pub(crate) code_hex: String,
    pub(crate) count: usize,
}

#[derive(Debug, Serialize)]
pub(crate) struct MainFirstLineSummary {
    pub(crate) unique_line_count: usize,
    pub(crate) max_storage_byte_count: usize,
    pub(crate) japanese_literal_byte_count: usize,
    pub(crate) non_japanese_literal_byte_count: usize,
    pub(crate) protected_original_alphanumeric_literal_byte_count: usize,
    pub(crate) line_end_control_counts: Vec<ControlUsageReport>,
}

#[derive(Debug, Serialize)]
pub(crate) struct MainLinearSegmentSummary {
    pub(crate) unique_segment_count: usize,
    pub(crate) total_line_count: usize,
    pub(crate) max_line_count: usize,
    pub(crate) japanese_literal_byte_count: usize,
    pub(crate) non_japanese_literal_byte_count: usize,
    pub(crate) protected_original_alphanumeric_literal_byte_count: usize,
    pub(crate) boundary_control_counts: Vec<ControlUsageReport>,
    pub(crate) transition_count: usize,
}

#[derive(Debug, Serialize)]
pub(crate) struct MainRecordStorageSummary {
    pub(crate) unique_record_count: usize,
    pub(crate) consumed_storage_byte_count: usize,
    pub(crate) unique_storage_byte_count: usize,
    pub(crate) shared_storage_byte_count: usize,
    pub(crate) overlapping_record_pair_count: usize,
    pub(crate) max_overlap_depth: usize,
    pub(crate) max_storage_byte_count: usize,
}

#[derive(Debug, Clone, Copy)]
pub(crate) struct MainRecordStorageRange {
    pub(crate) start: usize,
    pub(crate) end_exclusive: usize,
}

#[derive(Debug, Default)]
pub(crate) struct MainLiteralStorageFlags {
    pub(crate) japanese_literal: bool,
    pub(crate) non_japanese_literal: bool,
    pub(crate) structural: bool,
}

#[derive(Debug)]
pub(crate) struct MainLiteralStorageSummary {
    pub(crate) unique_japanese_literal_storage_byte_count: usize,
    pub(crate) unique_non_japanese_literal_storage_byte_count: usize,
    pub(crate) literal_kind_conflict_storage_byte_count: usize,
    pub(crate) literal_structural_conflict_storage_byte_count: usize,
    pub(crate) safe_japanese_translation_source_byte_count: usize,
}

#[derive(Debug, Clone, Serialize)]
pub(crate) struct MainRecordStorageReport {
    pub(crate) file_offset: usize,
    pub(crate) file_offset_hex: String,
    pub(crate) end_file_offset_exclusive: usize,
    pub(crate) end_file_offset_exclusive_hex: String,
    pub(crate) storage_byte_count: usize,
    pub(crate) storage_sha1: String,
    pub(crate) prefix_byte_count: usize,
    pub(crate) linear_segment_storage_byte_count: usize,
    pub(crate) boundary_control: u8,
    pub(crate) boundary_control_hex: String,
    pub(crate) boundary_kind: &'static str,
}

#[derive(Debug, Serialize)]
pub(crate) struct MainDialogueGraphReport {
    pub(crate) node_count: usize,
    pub(crate) transition_edge_count: usize,
    pub(crate) terminal_reachable_node_count: usize,
    pub(crate) caller_handoff_boundary_reachable_node_count: usize,
    pub(crate) max_transition_edge_count_to_boundary: usize,
    pub(crate) cycle_count: usize,
    pub(crate) unresolved_node_count: usize,
    pub(crate) transition_edges: Vec<MainDialogueTransitionEdgeReport>,
}

#[derive(Debug, Serialize)]
pub(crate) struct MainDialogueTransitionEdgeReport {
    pub(crate) source_table_id: &'static str,
    pub(crate) source_canonical_entry_index: usize,
    pub(crate) source_entry_indices: Vec<usize>,
    pub(crate) source_pointer_cpu_address: u16,
    pub(crate) source_pointer_cpu_address_hex: String,
    pub(crate) source_file_offset: usize,
    pub(crate) source_file_offset_hex: String,
    pub(crate) control: u8,
    pub(crate) control_hex: String,
    pub(crate) target_table_id: &'static str,
    pub(crate) target_entry_index: usize,
    pub(crate) target_canonical_entry_index: usize,
    pub(crate) target_pointer_cpu_address: u16,
    pub(crate) target_pointer_cpu_address_hex: String,
    pub(crate) target_file_offset: usize,
    pub(crate) target_file_offset_hex: String,
}

#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub(crate) struct MainDialogueGraphNodeKey {
    pub(crate) table_index: usize,
    pub(crate) pointer_cpu_address: u16,
}

#[derive(Clone, Copy, Debug)]
pub(crate) struct MainDialogueGraphNodeState {
    pub(crate) boundary_control: u8,
    pub(crate) transition_target: Option<MainDialogueGraphNodeKey>,
}

#[derive(Debug, PartialEq)]
pub(crate) struct MainDialogueGraphClosure {
    pub(crate) terminal_reachable_node_count: usize,
    pub(crate) caller_handoff_boundary_reachable_node_count: usize,
    pub(crate) max_transition_edge_count_to_boundary: usize,
}
