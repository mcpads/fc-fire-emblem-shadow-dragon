use super::*;

#[derive(Debug)]
pub struct FontSupplySummary {
    pub report_sha1: String,
    pub page_count: usize,
    pub protected_code_count: usize,
    pub unresolved_code_count: usize,
}

#[derive(Debug, Serialize)]
pub(super) struct FontSupplyReport {
    pub(super) schema_version: u8,
    pub(super) scope: ReportScope,
    pub(super) tile_format: TileFormat,
    pub(super) summary: ReportSummary,
    pub(super) mmc4_control_routines: Vec<Mmc4ControlRoutineReport>,
    pub(super) mmc4_chr_bank_writers: Vec<Mmc4ChrWriterReport>,
    pub(super) mmc4_register_write_candidates: Vec<Mmc4RegisterWriteInventory>,
    pub(super) mmc4_adjacent_chr_write_candidate_groups: Vec<Mmc4ChrWriteCandidateGroup>,
    pub(super) known_references: Vec<ReferenceReport>,
    pub(super) pages: Vec<PageReport>,
    pub(super) font_page: FontPageReport,
    pub(super) active_slot_ceiling: ActiveSlotCeiling,
    pub(super) unknowns: Vec<&'static str>,
}

#[derive(Debug, Serialize)]
pub(super) struct ReportScope {
    pub(super) source_sha1: &'static str,
    pub(super) chr_sha1: &'static str,
    pub(super) mapper: u16,
    pub(super) font_page_index: usize,
}

#[derive(Debug, Serialize)]
pub(super) struct TileFormat {
    pub(super) width: u8,
    pub(super) height: u8,
    pub(super) bits_per_pixel: u8,
    pub(super) bytes_per_tile: usize,
    pub(super) chr_page_size: usize,
    pub(super) tiles_per_page: usize,
}

#[derive(Debug, Serialize)]
pub(super) struct ReportSummary {
    pub(super) page_count: usize,
    pub(super) tile_count: usize,
    pub(super) nonblank_tile_count: usize,
    pub(super) blank_pattern_count: usize,
    pub(super) protected_font_code_count: usize,
    pub(super) unresolved_font_code_count: usize,
    pub(super) available_font_code_count: usize,
}

#[derive(Debug, Serialize)]
pub(super) struct ActiveSlotCeiling {
    pub(super) total_font_code_count: usize,
    pub(super) confirmed_protected_code_count: usize,
    pub(super) provisional_layout_reserved_codes: Vec<u8>,
    pub(super) provisional_layout_reserved_codes_hex: Vec<String>,
    pub(super) current_reserved_code_count: usize,
    pub(super) current_hangul_slot_ceiling: usize,
    pub(super) proof_boundary: &'static str,
}

#[derive(Debug, Serialize)]
pub(super) struct Mmc4ChrWriterReport {
    pub(super) cpu_address: u16,
    pub(super) cpu_address_hex: String,
    pub(super) shadow_address: u8,
    pub(super) shadow_address_hex: String,
    pub(super) page_group_shadow_address: u8,
    pub(super) page_group_shadow_address_hex: String,
    pub(super) hardware_register: u16,
    pub(super) hardware_register_hex: String,
    pub(super) latch_domain: &'static str,
    pub(super) routine_bytes_hex: String,
    pub(super) direct_jsr_candidates: Vec<AbsoluteTransferCandidate>,
    pub(super) direct_jmp_candidates: Vec<AbsoluteTransferCandidate>,
}

#[derive(Debug, Serialize)]
pub(super) struct Mmc4ControlRoutineReport {
    pub(super) role: &'static str,
    pub(super) cpu_address: u16,
    pub(super) cpu_address_hex: String,
    pub(super) routine_bytes_hex: String,
}

#[derive(Debug, Serialize)]
pub(super) struct Mmc4RegisterWriteInventory {
    pub(super) register_address: u16,
    pub(super) register_address_hex: String,
    pub(super) role: &'static str,
    pub(super) candidates: Vec<AbsoluteWriteCandidate>,
}

#[derive(Debug, Serialize)]
pub(super) struct Mmc4ChrWriteCandidateGroup {
    pub(super) prg_bank: usize,
    pub(super) prg_bank_hex: String,
    pub(super) start_cpu_address: u16,
    pub(super) start_cpu_address_hex: String,
    pub(super) last_cpu_address: u16,
    pub(super) last_cpu_address_hex: String,
    pub(super) instruction_count: usize,
    pub(super) largest_gap_byte_count: usize,
    pub(super) evidence: &'static str,
    pub(super) disposition: &'static str,
    pub(super) writes: Vec<Mmc4ChrWriteCandidateSite>,
}

#[derive(Debug, Serialize)]
pub(super) struct Mmc4ChrWriteCandidateSite {
    pub(super) cpu_address: u16,
    pub(super) cpu_address_hex: String,
    pub(super) prg_offset: usize,
    pub(super) prg_offset_hex: String,
    pub(super) opcode_hex: String,
    pub(super) mnemonic: &'static str,
    pub(super) register_address: u16,
    pub(super) register_address_hex: String,
}

#[derive(Debug, Serialize)]
pub(super) struct ReferenceReport {
    pub(super) id: &'static str,
    pub(super) file_offset: usize,
    pub(super) file_offset_hex: String,
    pub(super) byte_length: usize,
    pub(super) bytes_hex: String,
    pub(super) displayed_text: &'static str,
    pub(super) consumer: &'static str,
    pub(super) scope: ReferenceScope,
    pub(super) evidence: &'static str,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub(super) enum ReferenceScope {
    TranslatedJapanese,
    PreservedOriginal,
}

#[derive(Debug, Serialize)]
pub(super) struct PageReport {
    pub(super) page_index: usize,
    pub(super) chr_offset: usize,
    pub(super) chr_offset_hex: String,
    pub(super) sha1: String,
    pub(super) nonblank_tile_count: usize,
    pub(super) blank_pattern_count: usize,
    pub(super) low_plane_only_count: usize,
    pub(super) high_plane_only_count: usize,
    pub(super) dual_plane_count: usize,
    pub(super) distinct_pattern_count: usize,
    pub(super) blank_pattern_codes: Vec<String>,
}

#[derive(Debug, Serialize)]
pub(super) struct FontPageReport {
    pub(super) page_index: usize,
    pub(super) chr_offset: usize,
    pub(super) chr_offset_hex: String,
    pub(super) slots: Vec<SlotReport>,
}

#[derive(Debug, Serialize)]
pub(super) struct SlotReport {
    pub(super) code: u8,
    pub(super) code_hex: String,
    pub(super) chr_offset: usize,
    pub(super) chr_offset_hex: String,
    pub(super) tile_sha1: String,
    pub(super) plane_usage: PlaneUsage,
    pub(super) nonzero_pixel_count: u32,
    pub(super) declared_glyph: Option<String>,
    pub(super) reference_occurrences: Vec<ReferenceOccurrence>,
    pub(super) matching_codes: Vec<String>,
    pub(super) code_assignment: Decision,
    pub(super) code_assignment_reasons: Vec<&'static str>,
    pub(super) tile_reuse: Decision,
    pub(super) tile_reuse_reasons: Vec<&'static str>,
}

#[derive(Debug, Serialize)]
pub(super) struct ReferenceOccurrence {
    pub(super) reference_id: &'static str,
    pub(super) count: usize,
    pub(super) scope: ReferenceScope,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub(super) enum PlaneUsage {
    Blank,
    LowOnly,
    HighOnly,
    Dual,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub(super) enum Decision {
    Protected,
    Unresolved,
}
