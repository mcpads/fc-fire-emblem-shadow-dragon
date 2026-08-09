use super::*;

pub(super) const SOURCE_ASSET_FORMAT_VERSION: u8 = 2;
pub(super) const WORKSPACE_FORMAT_VERSION: u8 = 2;

#[derive(Debug)]
pub struct DialogueSourceAssetSummary {
    pub asset_sha1: String,
    pub storage_region_count: usize,
    pub record_count: usize,
    pub unique_storage_byte_count: usize,
}

#[derive(Debug)]
pub struct DialogueSourceRoundtripSummary {
    pub output_sha1: String,
    pub storage_region_count: usize,
    pub record_count: usize,
}

#[derive(Debug)]
pub struct DialogueWorkspaceSummary {
    pub workspace_sha1: String,
    pub record_count: usize,
    pub line_count: usize,
    pub safe_japanese_source_byte_count: usize,
    pub blocked_line_count: usize,
    pub preserved_translation_line_count: usize,
}

#[derive(Debug)]
pub struct DialogueWorkspaceValidationSummary {
    pub workspace_sha1: String,
    pub record_count: usize,
    pub line_count: usize,
    pub filled_line_count: usize,
    pub complete_line_count: usize,
    pub target_glyph_count: usize,
}

#[derive(Debug)]
pub struct DialogueLayoutPlanSummary {
    pub report_sha1: String,
    pub region_count: usize,
    pub record_count: usize,
    pub pointer_write_count: usize,
    pub planned_storage_byte_count: usize,
    pub remaining_storage_byte_count: usize,
    pub changed_record_count: usize,
    pub translation_input_complete: bool,
    pub release_eligible: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub(super) struct MainDialogueWorkspace {
    pub(super) format_version: u8,
    pub(super) source_sha1: String,
    pub(super) translate_from: String,
    pub(super) translate_to: String,
    pub(super) preserve_existing_english: bool,
    pub(super) purpose: String,
    pub(super) safe_japanese_source_byte_count: usize,
    pub(super) records: Vec<WorkspaceRecord>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub(super) struct WorkspaceRecord {
    pub(super) id: String,
    pub(super) table_id: String,
    pub(super) source_prg_bank: u8,
    pub(super) canonical_entry_index: usize,
    pub(super) entry_indices: Vec<usize>,
    pub(super) pointer_cpu_address_hex: String,
    pub(super) prefix_byte_count: usize,
    pub(super) boundary_control_hex: String,
    pub(super) lines: Vec<WorkspaceLine>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub(super) struct WorkspaceLine {
    pub(super) id: String,
    pub(super) index: usize,
    pub(super) file_offset_hex: String,
    pub(super) source_storage_sha1: String,
    pub(super) source_markup: String,
    pub(super) korean: String,
    pub(super) status: TranslationStatus,
    pub(super) japanese_source_byte_count: usize,
    pub(super) safe_japanese_source_byte_count: usize,
    pub(super) requires_relocation: bool,
    pub(super) conflicting_file_offsets_hex: Vec<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(super) enum TranslationStatus {
    Untranslated,
    InProgress,
    NeedsReview,
    NeedsHumanReview,
    Complete,
}

#[derive(Debug, Serialize)]
pub(super) struct MainDialogueLayoutReport {
    pub(super) schema_version: u8,
    pub(super) scope: LayoutReportScope,
    pub(super) summary: LayoutReportSummary,
    pub(super) regions: Vec<LayoutRegionReport>,
    pub(super) records: Vec<LayoutRecordReport>,
    pub(super) unknowns: Vec<&'static str>,
}

#[derive(Debug, Serialize)]
pub(super) struct LayoutReportScope {
    pub(super) source_sha1: &'static str,
    pub(super) workspace_sha1: String,
    pub(super) translation_direction: &'static str,
    pub(super) preserve_existing_english: bool,
    pub(super) layout_mode: &'static str,
    pub(super) output_boundary: &'static str,
}

#[derive(Debug, Serialize)]
pub(super) struct LayoutReportSummary {
    pub(super) storage_region_count: usize,
    pub(super) record_count: usize,
    pub(super) pointer_write_count: usize,
    pub(super) source_owned_storage_byte_count: usize,
    pub(super) planned_storage_byte_count: usize,
    pub(super) remaining_storage_byte_count: usize,
    pub(super) changed_record_count: usize,
    pub(super) filled_line_count: usize,
    pub(super) complete_line_count: usize,
    pub(super) translation_input_complete: bool,
    pub(super) release_eligible: bool,
}

#[derive(Debug, Serialize)]
pub(super) struct LayoutRegionReport {
    pub(super) index: usize,
    pub(super) source_prg_bank: u8,
    pub(super) source_prg_bank_hex: String,
    pub(super) file_offset: usize,
    pub(super) file_offset_hex: String,
    pub(super) end_file_offset_exclusive: usize,
    pub(super) end_file_offset_exclusive_hex: String,
    pub(super) capacity_byte_count: usize,
    pub(super) planned_storage_byte_count: usize,
    pub(super) remaining_storage_byte_count: usize,
    pub(super) record_count: usize,
    pub(super) source_equivalent_layout: bool,
}

#[derive(Debug, Serialize)]
pub(super) struct LayoutRecordReport {
    pub(super) id: String,
    pub(super) source_prg_bank: u8,
    pub(super) source_prg_bank_hex: String,
    pub(super) source_pointer_cpu_address: u16,
    pub(super) source_pointer_cpu_address_hex: String,
    pub(super) planned_pointer_cpu_address: u16,
    pub(super) planned_pointer_cpu_address_hex: String,
    pub(super) pointer_file_offsets: Vec<usize>,
    pub(super) pointer_file_offsets_hex: Vec<String>,
    pub(super) source_storage_byte_count: usize,
    pub(super) planned_storage_byte_count: usize,
    pub(super) translated_line_count: usize,
    pub(super) changed: bool,
    pub(super) storage_region_index: usize,
    pub(super) region_relative_offset: usize,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) enum LogicalDialogueByte {
    Encoded(u8),
    TargetGlyph(char),
}

#[derive(Debug)]
pub(super) struct LogicalDialogueRecord {
    pub(super) id: String,
    pub(super) source_prg_bank: u8,
    pub(super) source_pointer_cpu_address: u16,
    pub(super) pointer_file_offsets: Vec<usize>,
    pub(super) source_file_offset: usize,
    pub(super) source_storage_byte_count: usize,
    pub(super) translated_line_count: usize,
    pub(super) bytes: Vec<LogicalDialogueByte>,
}

#[derive(Debug)]
pub(super) struct WorkspaceTranslationCounts {
    pub(super) filled_line_count: usize,
    pub(super) complete_line_count: usize,
    pub(super) target_glyph_count: usize,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub(super) struct MainDialogueSourceAsset {
    pub(super) format_version: u8,
    pub(super) source_sha1: String,
    pub(super) translate_from: String,
    pub(super) translate_to: String,
    pub(super) preserve_existing_english: bool,
    pub(super) purpose: String,
    pub(super) storage_regions: Vec<SourceStorageRegion>,
    pub(super) records: Vec<SourceRecordReference>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub(super) struct SourceStorageRegion {
    pub(super) index: usize,
    pub(super) source_prg_bank: u8,
    pub(super) file_offset: usize,
    pub(super) file_offset_hex: String,
    pub(super) end_file_offset_exclusive: usize,
    pub(super) end_file_offset_exclusive_hex: String,
    pub(super) storage_byte_count: usize,
    pub(super) storage_sha1: String,
    pub(super) storage_hex: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub(super) struct SourceRecordReference {
    pub(super) table_id: String,
    pub(super) source_prg_bank: u8,
    pub(super) canonical_entry_index: usize,
    pub(super) entry_indices: Vec<usize>,
    pub(super) pointer_file_offsets: Vec<usize>,
    pub(super) pointer_file_offsets_hex: Vec<String>,
    pub(super) pointer_cpu_address: u16,
    pub(super) pointer_cpu_address_hex: String,
    pub(super) storage_region_index: usize,
    pub(super) region_relative_offset: usize,
    pub(super) storage_byte_count: usize,
    pub(super) storage_sha1: String,
    pub(super) prefix_byte_count: usize,
    pub(super) boundary_control: u8,
    pub(super) boundary_control_hex: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) struct OwnedStorageRange {
    pub(super) source_prg_bank: u8,
    pub(super) start: usize,
    pub(super) end_exclusive: usize,
}
