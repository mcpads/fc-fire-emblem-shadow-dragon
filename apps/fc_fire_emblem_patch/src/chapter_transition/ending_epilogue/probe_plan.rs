use serde::Serialize;

use super::{CodeLocation, location};

const CANDIDATE_COUNT: usize = 53;
const ROSTER_RECORD_CAPACITY: usize = 54;
const ROSTER_RECORD_STRIDE: usize = 0x1B;
const ROSTER_SPAN_BYTES: usize = ROSTER_RECORD_CAPACITY * ROSTER_RECORD_STRIDE;
const IRREGULAR_SAMPLE_OFFSETS: [u16; 5] = [7, 19, 43, 82, 171];
const PROTECTED_FLOW_ADDRESSES: [u16; 4] = [0x7731, 0x773B, 0x77F1, 0x77F4];

#[derive(Debug, Serialize)]
pub(super) struct EndingEpilogueVariantObservationPlan {
    entry_action: &'static str,
    source_bound_entry_effect: &'static str,
    selector_breakpoint: CodeLocation,
    intervention_boundary: CodeLocation,
    intervention_timing: &'static str,
    intervention_memory_type: &'static str,
    roster_cpu_address: u16,
    roster_cpu_address_hex: &'static str,
    roster_region_offset: u16,
    roster_region_offset_hex: &'static str,
    roster_span_bytes: usize,
    roster_record_capacity: usize,
    candidate_record_count: usize,
    sentinel_record_index: usize,
    protected_flow_addresses: [u16; 4],
    protected_flow_address_hex: [&'static str; 4],
    runs: [EpilogueVariantObservationRun; 3],
    irregular_sample_offsets_frames: [u16; 5],
    capture_surfaces: &'static [&'static str],
    branch_observation_addresses: [u16; 4],
    branch_observation_address_hex: [&'static str; 4],
    proof_boundary: &'static str,
}

#[derive(Debug, Serialize)]
struct EpilogueVariantObservationRun {
    run_role: &'static str,
    roster_intervention: &'static str,
    expected_branch: &'static str,
    expected_classified_candidate_count: usize,
    expected_visible_entry_count: Option<usize>,
    required_observation: &'static str,
}

pub(super) fn ending_epilogue_variant_observation_plan() -> EndingEpilogueVariantObservationPlan {
    EndingEpilogueVariantObservationPlan {
        entry_action: "SELECT on the source-bound sound-test screen",
        source_bound_entry_effect: "change dialogue substate 0x0C to 0x0E and enter the automatic ending without further input",
        selector_breakpoint: location(0x04, 0xA165),
        intervention_boundary: location(0x04, 0xA168),
        intervention_timing: "break on the first phase-0x0F DEC $773B at 0xA165; the backend completes that instruction and freezes at 0xA168 with cursor 0x34, before name composition and classification of candidate id cursor+1 = 0x35; save this instruction-boundary base and restore it before each controlled roster-only run",
        intervention_memory_type: "nesSaveRam",
        roster_cpu_address: 0x6A90,
        roster_cpu_address_hex: "0x6A90",
        roster_region_offset: 0x0A90,
        roster_region_offset_hex: "0x0A90",
        roster_span_bytes: ROSTER_SPAN_BYTES,
        roster_record_capacity: ROSTER_RECORD_CAPACITY,
        candidate_record_count: CANDIDATE_COUNT,
        sentinel_record_index: CANDIDATE_COUNT,
        protected_flow_addresses: PROTECTED_FLOW_ADDRESSES,
        protected_flow_address_hex: ["0x7731", "0x773B", "0x77F1", "0x77F4"],
        runs: [
            EpilogueVariantObservationRun {
                run_role: "natural_baseline",
                roster_intervention: "none; census the existing roster and retain its authentic absent, inactive-or-defeated, and active classifications",
                expected_branch: "derive each branch from the observed identity and action bytes without changing state",
                expected_classified_candidate_count: CANDIDATE_COUNT,
                expected_visible_entry_count: None,
                required_observation: "bind the initial 0x35 cursor to the first visible candidate and record every naturally visible selector 0x40 or 0x41 entry",
            },
            EpilogueVariantObservationRun {
                run_role: "all_direct_candidates",
                roster_intervention: "copy one valid record template into identities 0x01 through 0x35, set action to a non-0xFF value, and place identity 0x00 in record 53",
                expected_branch: "selector 0x40 entries 0x35 down through 0x01",
                expected_classified_candidate_count: CANDIDATE_COUNT,
                expected_visible_entry_count: Some(CANDIDATE_COUNT),
                required_observation: "collect the direct-entry portrait, CHR, nametable, OAM, palette, and dialogue-control transition union",
            },
            EpilogueVariantObservationRun {
                run_role: "all_routing_candidates",
                roster_intervention: "copy one valid record template into identities 0x01 through 0x35, set action to 0xFF and a valid location index, and place identity 0x00 in record 53",
                expected_branch: "selector 0x41 entries 0x35 down through 0x02 followed by direct table 0x40 entry 0x00; routing entry 0x01 is a source-bound no-op",
                expected_classified_candidate_count: CANDIDATE_COUNT,
                expected_visible_entry_count: Some(CANDIDATE_COUNT - 1),
                required_observation: "collect the routing-entry portrait, CHR, nametable, OAM, palette, location-name, and dialogue-control transition union",
            },
        ],
        irregular_sample_offsets_frames: IRREGULAR_SAMPLE_OFFSETS,
        capture_surfaces: &[
            "screenshot",
            "nametable",
            "OAM",
            "palette",
            "CPU and mapper state",
        ],
        branch_observation_addresses: [0x7731, 0x773B, 0x77F1, 0x77F4],
        branch_observation_address_hex: ["0x7731", "0x773B", "0x77F1", "0x77F4"],
        proof_boundary: "roster-only runs enumerate rendering and control-flow surfaces; they do not prove a natural gameplay outcome, difficulty, character fate, or completion route, and they must not write phase, cursor, entry, or table selectors",
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn one_full_candidate_set_and_sentinel_fit_the_roster_exactly() {
        let plan = ending_epilogue_variant_observation_plan();

        assert_eq!(plan.candidate_record_count + 1, plan.roster_record_capacity);
        assert_eq!(plan.sentinel_record_index, plan.candidate_record_count);
        assert_eq!(plan.roster_span_bytes, 54 * 0x1B);
        assert_eq!(plan.roster_region_offset, 0x6A90 - 0x6000);
        assert_eq!(plan.selector_breakpoint.cpu_address, 0xA165);
        assert_eq!(plan.intervention_boundary.cpu_address, 0xA168);
    }

    #[test]
    fn temporal_samples_are_irregular_and_cover_early_and_settled_frames() {
        let deltas = IRREGULAR_SAMPLE_OFFSETS
            .windows(2)
            .map(|pair| pair[1] - pair[0])
            .collect::<Vec<_>>();

        assert!(IRREGULAR_SAMPLE_OFFSETS.is_sorted());
        assert_eq!(deltas, [12, 24, 39, 89]);
        assert!(IRREGULAR_SAMPLE_OFFSETS[0] < 10);
        assert!(IRREGULAR_SAMPLE_OFFSETS[4] > 150);
    }

    #[test]
    fn controlled_runs_preserve_the_selector_flow_addresses() {
        let plan = ending_epilogue_variant_observation_plan();

        assert_eq!(plan.protected_flow_addresses, PROTECTED_FLOW_ADDRESSES);
        assert_eq!(plan.runs[1].expected_visible_entry_count, Some(53));
        assert_eq!(plan.runs[2].expected_visible_entry_count, Some(52));
        assert!(plan.runs.iter().all(|run| {
            !run.roster_intervention.contains("0x7731")
                && !run.roster_intervention.contains("0x773B")
                && !run.roster_intervention.contains("0x77F1")
                && !run.roster_intervention.contains("0x77F4")
        }));
    }
}
