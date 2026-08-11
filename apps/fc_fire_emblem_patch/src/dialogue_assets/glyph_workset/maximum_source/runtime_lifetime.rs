use std::{collections::BTreeSet, path::Path};

use anyhow::{Result, ensure};
use serde::Serialize;

use crate::{dialogue_assets::WorkspaceRecord, font_slots::active_hangul_codes};

use super::MaximumDialogueSourceBinding;
use crate::dialogue_assets::glyph_workset::report::ObservedScreenLifetimeReport;

mod evidence;
mod page_budget;

pub(crate) use evidence::{RuntimeEvidence, load_runtime_evidence};
use page_budget::page_glyph_sets;

const REPORT_SCREEN_ROLE: &str = "chapter-seven maximum dialogue page";
const TARGET_RECORD_ID: &str = "village-and-outro-dialogue:024";
const DISPLAY_LINES_PER_PAGE: usize = 4;
const OBSERVED_PAGE_COUNT: usize = 15;
const SAMPLING_FRAME_OFFSETS: [usize; 6] = [0, 7, 19, 43, 82, 171];

#[derive(Debug, Serialize)]
pub(super) struct MaximumDialogueRuntimeLifetimeBinding {
    evidence_manifest_sha1: String,
    completed_page_count: usize,
    samples_per_page: usize,
    temporal_sample_count: usize,
    unique_nametable_count: usize,
    display_lines_per_page: usize,
    workspace_line_count: usize,
    preserved_screen_active_code_count: usize,
    preserved_source_active_code_count: usize,
    pub(super) preserved_active_source_code_count: usize,
    whole_record_filled_unique_glyph_count: usize,
    whole_record_slot_demand: usize,
    whole_record_fits_one_page: bool,
    pub(super) maximum_filled_page_unique_glyph_count: usize,
    pub(super) maximum_approved_page_unique_glyph_count: usize,
    pub(super) maximum_filled_page_slot_demand: usize,
    maximum_filled_page_fits_one_page: bool,
    page_granular_reload_required: bool,
    dialogue_content_emitted: bool,
    glyph_characters_emitted: bool,
    evidence_paths_emitted: bool,
}

impl MaximumDialogueRuntimeLifetimeBinding {
    pub(super) fn observed_screen_lifetime_report(
        &self,
        active_slot_count: usize,
        review_complete: bool,
    ) -> ObservedScreenLifetimeReport {
        let approved_slot_demand = review_complete.then_some(
            self.preserved_active_source_code_count + self.maximum_approved_page_unique_glyph_count,
        );
        ObservedScreenLifetimeReport {
            screen_role: REPORT_SCREEN_ROLE,
            budget_basis: "conservative union of all 90 completed-page nametables outside the dialogue interior, every source-preserved code in the record, and the largest four-line Korean page",
            evidence_digest: format!("sha1:{}", self.evidence_manifest_sha1),
            source_record_count: 1,
            filled_unique_glyph_count: self.maximum_filled_page_unique_glyph_count,
            preserved_active_source_code_count: self.preserved_active_source_code_count,
            additional_target_glyph_reservation_count: 0,
            filled_slot_demand: self.maximum_filled_page_slot_demand,
            filled_set_fits_one_page_so_far: self.maximum_filled_page_slot_demand
                <= active_slot_count,
            approved_unique_glyph_count: self.maximum_approved_page_unique_glyph_count,
            approved_slot_demand,
            approved_set_fits_one_page: approved_slot_demand
                .map(|slot_demand| slot_demand <= active_slot_count),
        }
    }
}

pub(in crate::dialogue_assets::glyph_workset) fn bind_runtime_lifetime(
    binding: &mut MaximumDialogueSourceBinding,
    manifest_path: &Path,
    workspace_record: &WorkspaceRecord,
    preserved_source_codes: &BTreeSet<u8>,
    whole_record_filled_unique_glyph_count: usize,
) -> Result<()> {
    ensure!(
        workspace_record.id == TARGET_RECORD_ID,
        "maximum runtime lifetime workspace record changed"
    );
    let evidence = load_runtime_evidence(manifest_path, workspace_record.lines.len())?;
    let (filled_pages, approved_pages) = page_glyph_sets(workspace_record)?;
    ensure!(
        filled_pages.len() == OBSERVED_PAGE_COUNT,
        "maximum runtime lifetime page partition changed"
    );

    let active_codes = active_hangul_codes().into_iter().collect::<BTreeSet<_>>();
    let preserved_screen_active_codes = evidence
        .screen_codes
        .intersection(&active_codes)
        .copied()
        .collect::<BTreeSet<_>>();
    let preserved_source_active_codes = preserved_source_codes
        .intersection(&active_codes)
        .copied()
        .collect::<BTreeSet<_>>();
    let preserved_active_codes = preserved_screen_active_codes
        .union(&preserved_source_active_codes)
        .copied()
        .collect::<BTreeSet<_>>();
    let maximum_filled_page_unique_glyph_count =
        filled_pages.iter().map(BTreeSet::len).max().unwrap_or(0);
    let maximum_approved_page_unique_glyph_count =
        approved_pages.iter().map(BTreeSet::len).max().unwrap_or(0);
    let active_slot_count = active_codes.len();
    let whole_record_slot_demand =
        preserved_active_codes.len() + whole_record_filled_unique_glyph_count;
    let maximum_filled_page_slot_demand =
        preserved_active_codes.len() + maximum_filled_page_unique_glyph_count;
    let whole_record_fits_one_page = whole_record_slot_demand <= active_slot_count;
    let maximum_filled_page_fits_one_page = maximum_filled_page_slot_demand <= active_slot_count;
    let page_granular_reload_required =
        !whole_record_fits_one_page && maximum_filled_page_fits_one_page;

    binding.binding_status = if page_granular_reload_required {
        "source_and_runtime_bound_page_reload_required"
    } else if maximum_filled_page_fits_one_page {
        "source_and_runtime_bound_single_page_fit"
    } else {
        "source_and_runtime_bound_page_overflow"
    };
    binding.screen_lifetime_bound = true;
    binding.next_gate = if page_granular_reload_required {
        "bind a font reload to the fifteen observed completed-page boundaries"
    } else if maximum_filled_page_fits_one_page {
        "install and verify the source-bound maximum dialogue font page"
    } else {
        "split the maximum dialogue below the observed four-line page lifetime"
    };
    binding.runtime_screen_lifetime = Some(MaximumDialogueRuntimeLifetimeBinding {
        evidence_manifest_sha1: evidence.manifest_sha1,
        completed_page_count: evidence.completed_page_count,
        samples_per_page: evidence.samples_per_page,
        temporal_sample_count: evidence.temporal_sample_count,
        unique_nametable_count: evidence.unique_nametable_count,
        display_lines_per_page: DISPLAY_LINES_PER_PAGE,
        workspace_line_count: workspace_record.lines.len(),
        preserved_screen_active_code_count: preserved_screen_active_codes.len(),
        preserved_source_active_code_count: preserved_source_active_codes.len(),
        preserved_active_source_code_count: preserved_active_codes.len(),
        whole_record_filled_unique_glyph_count,
        whole_record_slot_demand,
        whole_record_fits_one_page,
        maximum_filled_page_unique_glyph_count,
        maximum_approved_page_unique_glyph_count,
        maximum_filled_page_slot_demand,
        maximum_filled_page_fits_one_page,
        page_granular_reload_required,
        dialogue_content_emitted: false,
        glyph_characters_emitted: false,
        evidence_paths_emitted: false,
    });
    Ok(())
}
