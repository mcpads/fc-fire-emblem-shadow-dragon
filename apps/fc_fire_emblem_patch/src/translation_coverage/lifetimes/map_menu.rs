use std::path::Path;

use anyhow::{Context, Result, ensure};
use serde::Serialize;

use crate::{
    font_slots::ACTIVE_HANGUL_SLOT_COUNT,
    map_menu::plan_map_menu,
    rom::{EXPECTED_SOURCE_SHA1, Rom},
    sha1_hex,
};

use super::super::report::TranslationLifetimeDemandReport;
use super::full_page_bound;

pub(super) struct InputBindings<'a> {
    pub(super) source_path: &'a Path,
    pub(super) localization_path: &'a Path,
    pub(super) localization_sha1: &'a str,
}

#[derive(Serialize)]
struct EvidenceDigest<'a> {
    schema: u8,
    source_sha1: &'static str,
    localization_sha1: &'a str,
    screen_roles: [&'static str; 2],
    source_reclaimable_active_code_count: usize,
    target_glyph_count: usize,
    preservation_policy: &'static str,
}

pub(super) fn inspect(bindings: InputBindings<'_>) -> Result<Vec<TranslationLifetimeDemandReport>> {
    let rom = Rom::from_path(bindings.source_path)?;
    rom.verify_supported_japanese()?;
    let plan = plan_map_menu(&rom, bindings.localization_path)?;
    ensure!(
        plan.workspace_sha1 == bindings.localization_sha1
            && plan.entry_count == 8
            && plan.translated_entry_count == 8,
        "map-menu lifetime translation input changed"
    );
    let bound = full_page_bound::measure(
        &plan.target_glyphs,
        &plan.source_reclaimable_active_codes,
        "map_menu",
    )?;
    let evidence = EvidenceDigest {
        schema: 1,
        source_sha1: EXPECTED_SOURCE_SHA1,
        localization_sha1: bindings.localization_sha1,
        screen_roles: ["map_menu", "map_funds_summary"],
        source_reclaimable_active_code_count: bound.source_reclaimable_active_code_count,
        target_glyph_count: bound.target_glyph_count,
        preservation_policy: "preserve every active code except the exact eight map-menu and funds-summary source-label code union",
    };
    let evidence_bytes = serde_json::to_vec(&evidence).context("serialize map-menu evidence")?;

    let evidence_report_sha1 = sha1_hex(&evidence_bytes);
    Ok(["map_menu", "map_funds_summary"]
        .into_iter()
        .map(|screen_role| TranslationLifetimeDemandReport {
            screen_role,
            measurement_basis: "shared full-page upper bound preserving every active code except the exact eight map-menu and funds-summary source-label code union",
            target_glyph_count: bound.target_glyph_count,
            preserved_active_source_code_count: bound.preserved_active_source_code_count,
            additional_target_glyph_reservation_count: 0,
            total_slot_demand: bound.total_slot_demand,
            active_slot_count: ACTIVE_HANGUL_SLOT_COUNT,
            fits_active_page: true,
            evidence_report_sha1: evidence_report_sha1.clone(),
        })
        .collect())
}
