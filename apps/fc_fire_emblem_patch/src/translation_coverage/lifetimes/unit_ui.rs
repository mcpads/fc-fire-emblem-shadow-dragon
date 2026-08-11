use std::{fs, path::Path};

use anyhow::{Context, Result, ensure};
use serde::Deserialize;

use crate::{font_slots::ACTIVE_HANGUL_SLOT_COUNT, rom::EXPECTED_SOURCE_SHA1, sha1_hex};

use super::super::report::TranslationLifetimeDemandReport;

const REPORT_SCHEMA: u8 = 1;

pub(super) struct InputBindings<'a> {
    pub(super) fixed_text_workspace_sha1: &'a str,
    pub(super) unit_name_workspace_sha1: &'a str,
    pub(super) unit_ui_label_workspace_sha1: &'a str,
}

#[derive(Debug, Deserialize)]
struct Report {
    schema: u8,
    source_sha1: String,
    glyph_budget: GlyphBudget,
}

#[derive(Debug, Deserialize)]
struct GlyphBudget {
    fixed_text_workspace_sha1: String,
    unit_name_workspace_sha1: String,
    unit_ui_label_workspace_sha1: String,
    target_korean_glyph_count: usize,
    summary_status_family_target_glyph_count: usize,
    command_family_target_glyph_count: usize,
    single_family_page_fit: bool,
    summary_status_family_page_fit: bool,
    command_family_page_fit: bool,
    additional_preserved_unresolved_codes: Vec<u8>,
    screen_lifetimes: Vec<ScreenLifetime>,
}

#[derive(Debug, Deserialize)]
struct ScreenLifetime {
    screen_role: String,
    target_glyph_upper_bound: usize,
    preserved_active_source_code_upper_bound: usize,
    total_slot_upper_bound: usize,
    active_slot_count: usize,
    upper_bound_fits_active_page: bool,
}

pub(super) fn inspect(
    report_path: &Path,
    bindings: InputBindings<'_>,
) -> Result<Vec<TranslationLifetimeDemandReport>> {
    let bytes = fs::read(report_path)
        .with_context(|| format!("read unit-UI text report {}", report_path.display()))?;
    let report: Report = serde_json::from_slice(&bytes)
        .with_context(|| format!("parse unit-UI text report {}", report_path.display()))?;
    validate(&report, bindings)?;
    build_demands(report, sha1_hex(&bytes))
}

fn validate(report: &Report, bindings: InputBindings<'_>) -> Result<()> {
    let budget = &report.glyph_budget;
    ensure!(
        report.schema == REPORT_SCHEMA
            && report.source_sha1 == EXPECTED_SOURCE_SHA1
            && budget.fixed_text_workspace_sha1 == bindings.fixed_text_workspace_sha1
            && budget.unit_name_workspace_sha1 == bindings.unit_name_workspace_sha1
            && budget.unit_ui_label_workspace_sha1 == bindings.unit_ui_label_workspace_sha1,
        "unit-UI lifetime report is stale or bound to different translation inputs"
    );
    ensure!(
        budget.target_korean_glyph_count > ACTIVE_HANGUL_SLOT_COUNT
            && budget.summary_status_family_target_glyph_count > ACTIVE_HANGUL_SLOT_COUNT
            && budget.command_family_target_glyph_count <= ACTIVE_HANGUL_SLOT_COUNT
            && !budget.single_family_page_fit
            && !budget.summary_status_family_page_fit
            && budget.command_family_page_fit,
        "unit-UI family-page result no longer matches the target glyph populations"
    );
    ensure!(
        budget.screen_lifetimes.len() == 3,
        "unit-UI lifetime report must contain summary, status, and command screens"
    );
    let expected_preserved_count = budget.additional_preserved_unresolved_codes.len();
    for lifetime in &budget.screen_lifetimes {
        let component_total = lifetime
            .target_glyph_upper_bound
            .checked_add(lifetime.preserved_active_source_code_upper_bound)
            .context("unit-UI lifetime component overflow")?;
        ensure!(
            lifetime.preserved_active_source_code_upper_bound == expected_preserved_count
                && lifetime.active_slot_count == ACTIVE_HANGUL_SLOT_COUNT
                && component_total == lifetime.total_slot_upper_bound
                && lifetime.upper_bound_fits_active_page
                    == (lifetime.total_slot_upper_bound <= ACTIVE_HANGUL_SLOT_COUNT),
            "unit-UI {} lifetime components or fit result changed",
            lifetime.screen_role
        );
    }
    Ok(())
}

fn build_demands(
    report: Report,
    report_sha1: String,
) -> Result<Vec<TranslationLifetimeDemandReport>> {
    report
        .glyph_budget
        .screen_lifetimes
        .into_iter()
        .map(|lifetime| {
            let (screen_role, measurement_basis) = match lifetime.screen_role.as_str() {
                "unit_summary" => (
                    "unit_summary",
                    "conservative independent maxima for one name, one class, four items, and the level label",
                ),
                "unit_status" => (
                    "unit_status",
                    "conservative independent maxima for one name, one class, and the complete status-label union",
                ),
                "unit_command_menu" => (
                    "unit_command_menu",
                    "complete union of all fifteen condition-selected command labels",
                ),
                other => anyhow::bail!("unknown unit-UI lifetime {other}"),
            };
            Ok(TranslationLifetimeDemandReport {
                screen_role,
                measurement_basis,
                target_glyph_count: lifetime.target_glyph_upper_bound,
                preserved_active_source_code_count: lifetime
                    .preserved_active_source_code_upper_bound,
                additional_target_glyph_reservation_count: 0,
                total_slot_demand: lifetime.total_slot_upper_bound,
                active_slot_count: lifetime.active_slot_count,
                fits_active_page: lifetime.upper_bound_fits_active_page,
                evidence_report_sha1: report_sha1.clone(),
            })
        })
        .collect()
}
