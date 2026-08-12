use serde::{Deserialize, Serialize};

use super::*;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub(super) struct EntryModeWorkspace {
    pub(super) format_version: u8,
    pub(super) source_sha1: String,
    pub(super) translate_from: String,
    pub(super) translate_to: String,
    pub(super) preserve_existing_english: bool,
    pub(super) purpose: String,
    pub(super) reachability_policy: String,
    pub(super) required_entry_modes: [String; 2],
    pub(super) differing_entry_start_japanese_source_byte_count: usize,
    pub(super) records: Vec<EntryModeRecord>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub(super) struct EntryModeRecord {
    pub(super) id: String,
    pub(super) incoming_transition_edge_count: usize,
    pub(super) direct_prefix_byte_count: usize,
    pub(super) transition_prefix_byte_count: usize,
    pub(super) common_body_source_file_offset_hex: String,
    pub(super) divergent_segment_source_sha1: String,
    pub(super) direct_leading: EntryModePart,
    pub(super) common_body: EntryModePart,
    pub(super) transition_leading: EntryModePart,
}

impl EntryModeRecord {
    pub(super) fn parts(&self) -> [&EntryModePart; 3] {
        [
            &self.direct_leading,
            &self.common_body,
            &self.transition_leading,
        ]
    }

    pub(super) fn parts_mut(&mut self) -> [&mut EntryModePart; 3] {
        [
            &mut self.direct_leading,
            &mut self.common_body,
            &mut self.transition_leading,
        ]
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub(super) struct EntryModePart {
    pub(super) id: String,
    pub(super) role: EntryModePartRole,
    pub(super) source_file_offset_hex: String,
    pub(super) source_storage_byte_count: usize,
    pub(super) source_storage_sha1: String,
    pub(super) source_markup: String,
    pub(super) japanese_source_byte_count: usize,
    pub(super) korean: String,
    pub(super) status: TranslationStatus,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(super) enum EntryModePartRole {
    DirectLeading,
    CommonBody,
    TransitionLeading,
}

#[derive(Debug, Default)]
pub(super) struct TranslationCounts {
    pub(super) filled_part_count: usize,
    pub(super) complete_part_count: usize,
    pub(super) untranslated_japanese_part_count: usize,
    pub(super) target_glyph_count: usize,
}
