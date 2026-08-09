use serde::Serialize;

const DIRECT_EXTENSION_ENTRY_WRITES: &[u8] =
    &[0x36, 0x37, 0x38, 0x39, 0x3B, 0x3C, 0x3D, 0x3F, 0x40, 0x41];
const SOURCE_POSSIBLE_BUT_UNOBSERVED_DIRECT_EXTENSIONS: &[u8] = &[0x3A, 0x3E];
const OBSERVED_LEFT_CHR_PAGES: &[u8] = &[
    0x04, 0x07, 0x0A, 0x0B, 0x0F, 0x10, 0x11, 0x13, 0x17, 0x1A, 0x1C, 0x1D, 0x1F,
];

#[derive(Debug, Serialize)]
pub(super) struct EndingEpilogueVariantObservation {
    capture_rom_sha1: &'static str,
    structural_report_sha1: &'static str,
    evidence_tree_sha1: &'static str,
    run_count: usize,
    visible_entry_count: usize,
    sample_count: usize,
    samples_per_visible_entry: usize,
    natural_visible_entry_count: usize,
    direct_visible_entry_count: usize,
    routing_visible_entry_count: usize,
    chr_pair_count: usize,
    observed_left_chr_pages: &'static [u8],
    observed_right_chr_pair: &'static str,
    distinct_screenshot_count: usize,
    distinct_nametable_count: usize,
    distinct_oam_count: usize,
    distinct_palette_count: usize,
    direct_event_write_count: usize,
    direct_candidate_entry_write_range: &'static str,
    direct_extension_entry_writes: &'static [u8],
    source_possible_but_unobserved_direct_extensions: &'static [u8],
    direct_root_selector_writes: &'static [u8],
    routing_entry_01_observation: &'static str,
    coverage_status: &'static str,
    proof_boundary: &'static str,
}

pub(super) fn ending_epilogue_variant_observation() -> EndingEpilogueVariantObservation {
    EndingEpilogueVariantObservation {
        capture_rom_sha1: "c513c36b46d2de80ae6047694f65d1475c2b8a0a",
        structural_report_sha1: "e201ef14639e12f3d1bd3c9d61f9baefaa7c569e",
        evidence_tree_sha1: "71546fe01803a13a5340c68334111bfa9f13b443",
        run_count: 3,
        visible_entry_count: 112,
        sample_count: 560,
        samples_per_visible_entry: 5,
        natural_visible_entry_count: 7,
        direct_visible_entry_count: 53,
        routing_visible_entry_count: 52,
        chr_pair_count: OBSERVED_LEFT_CHR_PAGES.len(),
        observed_left_chr_pages: OBSERVED_LEFT_CHR_PAGES,
        observed_right_chr_pair: "0x00/0x00",
        distinct_screenshot_count: 258,
        distinct_nametable_count: 307,
        distinct_oam_count: 102,
        distinct_palette_count: 168,
        direct_event_write_count: 125,
        direct_candidate_entry_write_range: "0x01..0x35",
        direct_extension_entry_writes: DIRECT_EXTENSION_ENTRY_WRITES,
        source_possible_but_unobserved_direct_extensions:
            SOURCE_POSSIBLE_BUT_UNOBSERVED_DIRECT_EXTENSIONS,
        direct_root_selector_writes: &[0x40],
        routing_entry_01_observation: "selector 0x41 entry 0x01 advanced phase 0x0F to 0x10 in one frame, then remained blank with background and sprites disabled for 600 more no-input frames; it is not a visible entry",
        coverage_status: "natural, all-direct, visible all-routing, and direct event-write coverage complete",
        proof_boundary: "the controlled roster writes prove render and control-flow coverage only; natural gameplay causes inside action 0xFF remain semantically unresolved",
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn direct_event_trace_keeps_observed_and_unobserved_extensions_distinct() {
        assert_eq!(DIRECT_EXTENSION_ENTRY_WRITES.len(), 10);
        assert!(DIRECT_EXTENSION_ENTRY_WRITES.contains(&0x3F));
        assert!(!DIRECT_EXTENSION_ENTRY_WRITES.contains(&0x3A));
        assert!(!DIRECT_EXTENSION_ENTRY_WRITES.contains(&0x3E));
        assert_eq!(
            SOURCE_POSSIBLE_BUT_UNOBSERVED_DIRECT_EXTENSIONS,
            &[0x3A, 0x3E]
        );
    }

    #[test]
    fn observation_closes_every_visible_entry_with_irregular_samples() {
        let observation = ending_epilogue_variant_observation();
        assert_eq!(
            observation.natural_visible_entry_count
                + observation.direct_visible_entry_count
                + observation.routing_visible_entry_count,
            observation.visible_entry_count
        );
        assert_eq!(
            observation.visible_entry_count * observation.samples_per_visible_entry,
            observation.sample_count
        );
        assert_eq!(observation.chr_pair_count, OBSERVED_LEFT_CHR_PAGES.len());
    }
}
