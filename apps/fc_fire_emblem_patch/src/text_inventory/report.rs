use super::*;

#[derive(Debug)]
pub struct TextInventorySummary {
    pub report_sha1: String,
    pub table_count: usize,
    pub pointer_count: usize,
    pub unique_string_count: usize,
    pub referenced_protected_original_byte_count: usize,
}

#[derive(Debug, Serialize)]
pub(super) struct TextInventoryReport {
    pub(super) schema_version: u8,
    pub(super) scope: ReportScope,
    pub(super) summary: ReportSummary,
    pub(super) source_code_usage: Vec<SourceCodeUsage>,
    pub(super) layout_controls: Vec<LayoutControlEvidence>,
    pub(super) dialogue_text_path: DialogueTextPathEvidence,
    pub(super) tables: Vec<TextTableReport>,
    pub(super) unknowns: Vec<&'static str>,
}

#[derive(Debug, Serialize)]
pub(super) struct ReportScope {
    pub(super) source_sha1: &'static str,
    pub(super) translation_direction: &'static str,
    pub(super) preserve_existing_english: bool,
    pub(super) proof_boundary: &'static str,
}

#[derive(Debug, Serialize)]
pub(super) struct ReportSummary {
    pub(super) table_count: usize,
    pub(super) pointer_count: usize,
    pub(super) unique_string_count: usize,
    pub(super) referenced_text_byte_count: usize,
    pub(super) unique_text_storage_byte_count: usize,
    pub(super) referenced_protected_original_byte_count: usize,
    pub(super) unique_protected_original_byte_count: usize,
    pub(super) referenced_unresolved_byte_count: usize,
    pub(super) unique_unresolved_byte_count: usize,
    pub(super) referenced_unresolved_nonblank_font_tile_byte_count: usize,
    pub(super) unique_unresolved_nonblank_font_tile_byte_count: usize,
    pub(super) referenced_unresolved_blank_font_tile_byte_count: usize,
    pub(super) unique_unresolved_blank_font_tile_byte_count: usize,
    pub(super) distinct_source_code_count: usize,
    pub(super) distinct_unresolved_nonblank_font_code_count: usize,
    pub(super) distinct_unresolved_blank_font_code_count: usize,
}

#[derive(Debug, Serialize)]
pub(super) struct TextTableReport {
    pub(super) id: &'static str,
    pub(super) role: &'static str,
    pub(super) table_file_offset: usize,
    pub(super) table_file_offset_hex: String,
    pub(super) table_cpu_address: u16,
    pub(super) table_cpu_address_hex: String,
    pub(super) pointer_count: usize,
    pub(super) unique_string_count: usize,
    pub(super) pointer_table_sha1: String,
    pub(super) terminator: u8,
    pub(super) terminator_hex: String,
    pub(super) consumer: ConsumerEvidence,
    pub(super) transfer: TextTransferEvidence,
    pub(super) data_file_start: usize,
    pub(super) data_file_start_hex: String,
    pub(super) data_file_end_exclusive: usize,
    pub(super) data_file_end_exclusive_hex: String,
    pub(super) referenced_text_byte_count: usize,
    pub(super) unique_text_storage_byte_count: usize,
    pub(super) referenced_protected_original_byte_count: usize,
    pub(super) unique_protected_original_byte_count: usize,
    pub(super) referenced_unresolved_byte_count: usize,
    pub(super) unique_unresolved_byte_count: usize,
    pub(super) referenced_unresolved_nonblank_font_tile_byte_count: usize,
    pub(super) unique_unresolved_nonblank_font_tile_byte_count: usize,
    pub(super) referenced_unresolved_blank_font_tile_byte_count: usize,
    pub(super) unique_unresolved_blank_font_tile_byte_count: usize,
    pub(super) source_code_usage: Vec<SourceCodeUsage>,
    pub(super) entries: Vec<TextEntryReport>,
}

#[derive(Clone, Debug, Serialize)]
pub(super) struct SourceCodeUsage {
    pub(super) code: u8,
    pub(super) code_hex: String,
    pub(super) font_tile_sha1: String,
    pub(super) font_tile_all_zero: bool,
    pub(super) referenced_byte_count: usize,
    pub(super) unique_storage_byte_count: usize,
    pub(super) referenced_protected_original_byte_count: usize,
    pub(super) unique_protected_original_byte_count: usize,
    pub(super) referenced_unresolved_nonblank_font_tile_byte_count: usize,
    pub(super) unique_unresolved_nonblank_font_tile_byte_count: usize,
    pub(super) referenced_unresolved_blank_font_tile_byte_count: usize,
    pub(super) unique_unresolved_blank_font_tile_byte_count: usize,
}

#[derive(Default)]
pub(super) struct CodeUsageCounts {
    pub(super) referenced_byte_count: usize,
    pub(super) unique_storage_byte_count: usize,
    pub(super) referenced_protected_original_byte_count: usize,
    pub(super) unique_protected_original_byte_count: usize,
    pub(super) referenced_unresolved_nonblank_font_tile_byte_count: usize,
    pub(super) unique_unresolved_nonblank_font_tile_byte_count: usize,
    pub(super) referenced_unresolved_blank_font_tile_byte_count: usize,
    pub(super) unique_unresolved_blank_font_tile_byte_count: usize,
}

#[derive(Debug, Serialize)]
pub(super) struct ConsumerEvidence {
    pub(super) file_offset: usize,
    pub(super) file_offset_hex: String,
    pub(super) prg_bank: usize,
    pub(super) prg_bank_hex: String,
    pub(super) cpu_address: u16,
    pub(super) cpu_address_hex: String,
    pub(super) instruction_bytes_hex: String,
    pub(super) pointer_load_mode: &'static str,
    pub(super) destination_pointer: String,
}

#[derive(Debug, Serialize)]
pub(super) struct TextTransferEvidence {
    pub(super) source_pointer: &'static str,
    pub(super) destination: &'static str,
    pub(super) recognized_stop_codes: Vec<u8>,
    pub(super) recognized_stop_codes_hex: Vec<String>,
    pub(super) declared_source_terminator: u8,
    pub(super) declared_source_terminator_hex: String,
    pub(super) destination_end_code: u8,
    pub(super) destination_end_code_hex: String,
    pub(super) destination_end_origin: &'static str,
    pub(super) explicit_copy_byte_limit: Option<usize>,
    pub(super) code_regions: Vec<TransferCodeEvidence>,
}

#[derive(Debug, Serialize)]
pub(super) struct TransferCodeEvidence {
    pub(super) role: &'static str,
    pub(super) file_offset: usize,
    pub(super) file_offset_hex: String,
    pub(super) prg_bank: usize,
    pub(super) prg_bank_hex: String,
    pub(super) cpu_address: u16,
    pub(super) cpu_address_hex: String,
    pub(super) instruction_bytes_hex: String,
}

#[derive(Debug, Serialize)]
pub(super) struct LayoutControlEvidence {
    pub(super) scope: &'static str,
    pub(super) entry_cpu_address: u16,
    pub(super) entry_cpu_address_hex: String,
    pub(super) source_buffer: &'static str,
    pub(super) output_buffer: &'static str,
    pub(super) segment_separator_code: u8,
    pub(super) segment_separator_code_hex: String,
    pub(super) end_code: u8,
    pub(super) end_code_hex: String,
    pub(super) overlay_blank_code: u8,
    pub(super) overlay_blank_code_hex: String,
    pub(super) first_pass_behavior: &'static str,
    pub(super) second_pass_behavior: &'static str,
    pub(super) segment_output_order: &'static str,
    pub(super) codes: Vec<u8>,
    pub(super) codes_hex: Vec<String>,
    pub(super) observed_behavior: &'static str,
    pub(super) inventory_referenced_byte_count: usize,
    pub(super) inventory_unique_storage_byte_count: usize,
    pub(super) code_regions: Vec<TransferCodeEvidence>,
    pub(super) downstream_consumer: CompositeTextConsumerEvidence,
    pub(super) plane_packing: CompositePlanePackingEvidence,
    pub(super) direct_jsr_candidates: Vec<AbsoluteTransferCandidate>,
    pub(super) direct_jmp_candidates: Vec<AbsoluteTransferCandidate>,
}

#[derive(Debug, Serialize)]
pub(super) struct CompositeTextConsumerEvidence {
    pub(super) entry_cpu_address: u16,
    pub(super) entry_cpu_address_hex: String,
    pub(super) source_buffer_pointer: &'static str,
    pub(super) source_cursor: &'static str,
    pub(super) stage_output_buffer: &'static str,
    pub(super) output_stage_call_count: usize,
    pub(super) segment_separator_replacement_code: u8,
    pub(super) segment_separator_replacement_code_hex: String,
    pub(super) observed_behavior: &'static str,
    pub(super) code_regions: Vec<TransferCodeEvidence>,
    pub(super) ppu_transfer: PpuTransferEvidence,
}

#[derive(Debug, Serialize)]
pub(super) struct PpuTransferEvidence {
    pub(super) stage_descriptor_buffer: &'static str,
    pub(super) queued_command_buffer: &'static str,
    pub(super) queued_command_length: &'static str,
    pub(super) ready_flag: &'static str,
    pub(super) serializer_cpu_address: u16,
    pub(super) serializer_cpu_address_hex: String,
    pub(super) flush_cpu_address: u16,
    pub(super) flush_cpu_address_hex: String,
    pub(super) queue_consumer_cpu_address: u16,
    pub(super) queue_consumer_cpu_address_hex: String,
    pub(super) command_pointer: &'static str,
    pub(super) command_terminator: u8,
    pub(super) command_address_byte_order: &'static str,
    pub(super) descriptor_byte_offset: usize,
    pub(super) descriptor_length_mask: u8,
    pub(super) descriptor_length_mask_hex: String,
    pub(super) descriptor_vertical_increment_mask: u8,
    pub(super) descriptor_vertical_increment_mask_hex: String,
    pub(super) descriptor_bit_6_behavior: &'static str,
    pub(super) data_byte_offset: usize,
    pub(super) ppu_address_register: &'static str,
    pub(super) ppu_data_register: &'static str,
    pub(super) ppu_data_write_cpu_address: u16,
    pub(super) ppu_data_write_cpu_address_hex: String,
    pub(super) observed_behavior: &'static str,
    pub(super) code_regions: Vec<TransferCodeEvidence>,
}

#[derive(Debug, Serialize)]
pub(super) struct CompositePlanePackingEvidence {
    pub(super) entry_cpu_address: u16,
    pub(super) entry_cpu_address_hex: String,
    pub(super) caller_cpu_addresses: Vec<u16>,
    pub(super) caller_cpu_addresses_hex: Vec<String>,
    pub(super) input_buffer: &'static str,
    pub(super) separator_scan_start_index: usize,
    pub(super) separator_code: u8,
    pub(super) separator_code_hex: String,
    pub(super) copy_source: &'static str,
    pub(super) copy_destination: &'static str,
    pub(super) copy_byte_count: &'static str,
    pub(super) copy_routine_cpu_address: u16,
    pub(super) copy_routine_cpu_address_hex: String,
    pub(super) output_layout: &'static str,
    pub(super) observed_behavior: &'static str,
    pub(super) code_regions: Vec<TransferCodeEvidence>,
}

#[derive(Debug, Serialize)]
pub(super) struct DialogueTextPathEvidence {
    pub(super) script: DialogueScriptEvidence,
    pub(super) renderer: DialogueRendererEvidence,
    pub(super) runtime_observation: DialogueRuntimeObservation,
}

#[derive(Debug, Serialize)]
pub(super) struct DialogueScriptEvidence {
    pub(super) reader_entry_cpu_address: u16,
    pub(super) reader_entry_cpu_address_hex: String,
    pub(super) source_bank_state: &'static str,
    pub(super) source_pointer: &'static str,
    pub(super) source_index: &'static str,
    pub(super) readback_byte: &'static str,
    pub(super) restored_dialogue_prg_bank: u8,
    pub(super) restored_dialogue_prg_bank_hex: String,
    pub(super) line_destination_pointer: &'static str,
    pub(super) destination_index: &'static str,
    pub(super) line_buffer_addresses: Vec<u16>,
    pub(super) line_buffer_addresses_hex: Vec<String>,
    pub(super) line_buffer_stride_bytes: usize,
    pub(super) line_end_code: u8,
    pub(super) line_end_code_hex: String,
    pub(super) recognized_control_codes: Vec<u8>,
    pub(super) recognized_control_codes_hex: Vec<String>,
    pub(super) controls: Vec<DialogueControlEvidence>,
    pub(super) synthesized_pair_control_code: u8,
    pub(super) synthesized_pair_control_code_hex: String,
    pub(super) synthesized_pair_codes: Vec<u8>,
    pub(super) synthesized_pair_codes_hex: Vec<String>,
    pub(super) code_regions: Vec<TransferCodeEvidence>,
    pub(super) packed_state_bit_code_regions: Vec<TransferCodeEvidence>,
}

#[derive(Debug, Serialize)]
pub(super) struct DialogueControlEvidence {
    pub(super) code: u8,
    pub(super) code_hex: String,
    pub(super) stream_storage_byte_count: usize,
    pub(super) current_pointer_advance_bytes: usize,
    pub(super) inline_operand_byte_count: usize,
    pub(super) transition_target_byte_count: usize,
    pub(super) line_effect: &'static str,
    pub(super) output_effect: &'static str,
    pub(super) state_effect: &'static str,
    pub(super) operand_contract: &'static str,
}

#[derive(Debug, Serialize)]
pub(super) struct DialogueRendererEvidence {
    pub(super) entry_cpu_address: u16,
    pub(super) entry_cpu_address_hex: String,
    pub(super) source_pointer: &'static str,
    pub(super) line_end_code: u8,
    pub(super) line_end_code_hex: String,
    pub(super) combining_codes: Vec<u8>,
    pub(super) combining_codes_hex: Vec<String>,
    pub(super) overlay_blank_code: u8,
    pub(super) overlay_blank_code_hex: String,
    pub(super) line_width_state: &'static str,
    pub(super) line_width_mask: u8,
    pub(super) line_width_mask_hex: String,
    pub(super) visible_code_count: &'static str,
    pub(super) processed_code_count: &'static str,
    pub(super) stage_descriptor_buffer: &'static str,
    pub(super) stage_payload_buffer: &'static str,
    pub(super) two_plane_header_flag: u8,
    pub(super) two_plane_header_flag_hex: String,
    pub(super) encoded_stage_count: usize,
    pub(super) stage_serializer_entry_cpu_address: u16,
    pub(super) stage_serializer_entry_cpu_address_hex: String,
    pub(super) queued_command_buffer: &'static str,
    pub(super) output_layout: &'static str,
    pub(super) code_regions: Vec<TransferCodeEvidence>,
}

#[derive(Debug, Serialize)]
pub(super) struct DialogueRuntimeObservation {
    pub(super) screen: &'static str,
    pub(super) source_prg_bank: u8,
    pub(super) source_prg_bank_hex: String,
    pub(super) source_cpu_address: u16,
    pub(super) source_cpu_address_hex: String,
    pub(super) source_file_offset: usize,
    pub(super) source_file_offset_hex: String,
    pub(super) destination_line_buffer_address: u16,
    pub(super) destination_line_buffer_address_hex: String,
    pub(super) observed_control_code: u8,
    pub(super) observed_control_code_hex: String,
    pub(super) observed_written_code: u8,
    pub(super) observed_written_code_hex: String,
    pub(super) source_write_instruction_cpu_address: u16,
    pub(super) source_write_instruction_cpu_address_hex: String,
    pub(super) source_write_event_pc: u16,
    pub(super) source_write_event_pc_hex: String,
    pub(super) source_write_dropped_event_count: usize,
    pub(super) observed_stage_descriptor: u8,
    pub(super) observed_stage_descriptor_hex: String,
    pub(super) observed_line_width: usize,
    pub(super) observed_stage_count: usize,
    pub(super) stage_descriptor_write_instruction_cpu_address: u16,
    pub(super) stage_descriptor_write_instruction_cpu_address_hex: String,
    pub(super) stage_descriptor_write_event_pc: u16,
    pub(super) stage_descriptor_write_event_pc_hex: String,
    pub(super) stage_descriptor_write_dropped_event_count: usize,
}

#[derive(Debug, Serialize)]
pub(super) struct TextEntryReport {
    pub(super) index: usize,
    pub(super) pointer_cpu_address: u16,
    pub(super) pointer_cpu_address_hex: String,
    pub(super) file_offset: usize,
    pub(super) file_offset_hex: String,
    pub(super) byte_length: usize,
    pub(super) raw_bytes_hex: String,
    pub(super) raw_sha1: String,
    pub(super) alias_entry_indices: Vec<usize>,
    pub(super) protected_original: Vec<ProtectedByte>,
    pub(super) unresolved_byte_count: usize,
}

#[derive(Debug, Serialize)]
pub(super) struct ProtectedByte {
    pub(super) byte_offset: usize,
    pub(super) code: u8,
    pub(super) code_hex: String,
    pub(super) glyph: String,
}
