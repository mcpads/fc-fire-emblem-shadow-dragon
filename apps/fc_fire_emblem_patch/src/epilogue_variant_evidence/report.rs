use serde::Serialize;

use crate::temporal_surface::capture_state::ChrPairReport;

#[derive(Clone, Debug, Serialize)]
pub(super) struct EpilogueVariantEvidenceReport {
    pub(super) schema: u8,
    pub(super) source_sha1: &'static str,
    pub(super) capture_rom_sha1: String,
    pub(super) mapper_probe_report_sha1: String,
    pub(super) evidence_sha1: String,
    pub(super) scope: EvidenceScope,
    pub(super) summary: EvidenceSummary,
    pub(super) runs: Vec<VariantRunReport>,
    pub(super) routing_no_op: RoutingNoOpReport,
    pub(super) union: VariantUnionReport,
    pub(super) unresolved: Vec<&'static str>,
    pub(super) release_eligible: bool,
}

#[derive(Clone, Debug, Serialize)]
pub(super) struct EvidenceScope {
    pub(super) translation_direction: &'static str,
    pub(super) preserve_existing_english_and_digits: bool,
    pub(super) dialogue_content_emitted: bool,
    pub(super) evidence_paths_emitted: bool,
    pub(super) intervention_scope: &'static str,
    pub(super) proof_boundary: &'static str,
}

#[derive(Clone, Debug, Serialize)]
pub(super) struct EvidenceSummary {
    pub(super) run_count: usize,
    pub(super) visible_entry_count: usize,
    pub(super) sample_count: usize,
    pub(super) samples_per_visible_entry: usize,
    pub(super) every_capture_complete: bool,
    pub(super) every_entry_irregularly_sampled: bool,
    pub(super) chr_pair_count: usize,
    pub(super) distinct_screenshot_count: usize,
    pub(super) distinct_nametable_count: usize,
    pub(super) distinct_oam_count: usize,
    pub(super) distinct_palette_count: usize,
}

#[derive(Clone, Debug, Serialize)]
pub(super) struct VariantRunReport {
    pub(super) run_role: &'static str,
    pub(super) expected_root_selector: &'static str,
    pub(super) entry_count: usize,
    pub(super) first_entry_hex: String,
    pub(super) last_entry_hex: String,
    pub(super) sample_offsets_frames: Vec<u16>,
    pub(super) sample_count: usize,
    pub(super) distinct_screenshot_count: usize,
    pub(super) distinct_settled_screenshot_count: usize,
    pub(super) distinct_nametable_count: usize,
    pub(super) distinct_oam_count: usize,
    pub(super) distinct_palette_count: usize,
    pub(super) chr_pairs: Vec<ChrPairReport>,
    pub(super) selector_entry_pairs: Vec<String>,
    pub(super) nametable_tile_codes_hex: Vec<String>,
    pub(super) visible_sprite_tile_codes_hex: Vec<String>,
}

#[derive(Clone, Debug, Serialize)]
pub(super) struct RoutingNoOpReport {
    pub(super) entry_hex: &'static str,
    pub(super) selector_hex: &'static str,
    pub(super) phase_hex: &'static str,
    pub(super) no_input_frames_before_capture: u16,
    pub(super) background_enabled: bool,
    pub(super) sprites_enabled: bool,
    pub(super) screenshot_sha1: String,
    pub(super) interpretation: &'static str,
}

#[derive(Clone, Debug, Serialize)]
pub(super) struct VariantUnionReport {
    pub(super) chr_pairs: Vec<ChrPairReport>,
    pub(super) screenshot_sha1_count: usize,
    pub(super) nametable_sha1_count: usize,
    pub(super) oam_sha1_count: usize,
    pub(super) palette_sha1_count: usize,
    pub(super) selector_entry_pairs: Vec<String>,
    pub(super) nametable_tile_codes_hex: Vec<String>,
    pub(super) visible_sprite_tile_codes_hex: Vec<String>,
}
