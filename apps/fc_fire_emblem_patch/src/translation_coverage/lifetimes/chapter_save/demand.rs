use std::collections::BTreeSet;

use anyhow::{Context, Result, ensure};
use serde::Serialize;

use crate::{
    font_slots::ACTIVE_HANGUL_SLOT_COUNT, rom::EXPECTED_SOURCE_SHA1, sha1_hex,
    translation_coverage::report::TranslationLifetimeDemandReport,
};

use super::super::full_page_bound;

#[derive(Serialize)]
struct EvidenceDigest<'a> {
    schema: u8,
    source_sha1: &'static str,
    screen_role: &'static str,
    main_dialogue_workspace_sha1: &'a str,
    choice_label_workspace_sha1: Option<&'a str>,
    transition_label_workspace_sha1: Option<&'a str>,
    runtime_manifest_sha1: Option<&'a str>,
    main_dialogue_record_id: Option<&'a str>,
    target_glyph_count: usize,
    source_reclaimable_active_code_count: Option<usize>,
    preserved_active_source_code_count: usize,
    preservation_policy: &'static str,
    source_binding: &'static str,
}

pub(super) struct EvidenceBindings<'a> {
    pub(super) main_dialogue_workspace_sha1: &'a str,
    pub(super) choice_label_workspace_sha1: Option<&'a str>,
    pub(super) transition_label_workspace_sha1: Option<&'a str>,
    pub(super) runtime_manifest_sha1: Option<&'a str>,
    pub(super) main_dialogue_record_id: Option<&'a str>,
    pub(super) source_binding: &'static str,
}

pub(super) fn full_page(
    screen_role: &'static str,
    measurement_basis: &'static str,
    target_glyphs: &BTreeSet<char>,
    source_reclaimable_active_codes: &BTreeSet<u8>,
    evidence_bindings: EvidenceBindings<'_>,
) -> Result<TranslationLifetimeDemandReport> {
    let bound =
        full_page_bound::measure(target_glyphs, source_reclaimable_active_codes, screen_role)?;
    let evidence = EvidenceDigest {
        schema: 1,
        source_sha1: EXPECTED_SOURCE_SHA1,
        screen_role,
        main_dialogue_workspace_sha1: evidence_bindings.main_dialogue_workspace_sha1,
        choice_label_workspace_sha1: evidence_bindings.choice_label_workspace_sha1,
        transition_label_workspace_sha1: evidence_bindings.transition_label_workspace_sha1,
        runtime_manifest_sha1: evidence_bindings.runtime_manifest_sha1,
        main_dialogue_record_id: evidence_bindings.main_dialogue_record_id,
        target_glyph_count: bound.target_glyph_count,
        source_reclaimable_active_code_count: Some(bound.source_reclaimable_active_code_count),
        preserved_active_source_code_count: bound.preserved_active_source_code_count,
        preservation_policy: "preserve every active code except exact Japanese codes removed from the selected consumers",
        source_binding: evidence_bindings.source_binding,
    };
    let evidence_bytes =
        serde_json::to_vec(&evidence).context("serialize chapter-save lifetime evidence")?;
    Ok(TranslationLifetimeDemandReport {
        screen_role,
        measurement_basis,
        target_glyph_count: bound.target_glyph_count,
        preserved_active_source_code_count: bound.preserved_active_source_code_count,
        additional_target_glyph_reservation_count: 0,
        total_slot_demand: bound.total_slot_demand,
        active_slot_count: ACTIVE_HANGUL_SLOT_COUNT,
        fits_active_page: true,
        evidence_report_sha1: sha1_hex(&evidence_bytes),
    })
}

pub(super) fn observed_screen(
    screen_role: &'static str,
    measurement_basis: &'static str,
    target_glyphs: &BTreeSet<char>,
    preserved_active_source_codes: &BTreeSet<u8>,
    evidence_bindings: EvidenceBindings<'_>,
) -> Result<TranslationLifetimeDemandReport> {
    let active_codes = crate::font_slots::active_hangul_codes()
        .into_iter()
        .collect::<BTreeSet<_>>();
    ensure!(
        preserved_active_source_codes.is_subset(&active_codes),
        "{screen_role} observed lifetime contains a reserved source code"
    );
    let total_slot_demand = target_glyphs
        .len()
        .checked_add(preserved_active_source_codes.len())
        .context("chapter-save observed lifetime slot demand overflow")?;
    ensure!(
        total_slot_demand <= ACTIVE_HANGUL_SLOT_COUNT,
        "{screen_role} observed lifetime needs {total_slot_demand} active slots"
    );
    let evidence = EvidenceDigest {
        schema: 1,
        source_sha1: EXPECTED_SOURCE_SHA1,
        screen_role,
        main_dialogue_workspace_sha1: evidence_bindings.main_dialogue_workspace_sha1,
        choice_label_workspace_sha1: evidence_bindings.choice_label_workspace_sha1,
        transition_label_workspace_sha1: evidence_bindings.transition_label_workspace_sha1,
        runtime_manifest_sha1: evidence_bindings.runtime_manifest_sha1,
        main_dialogue_record_id: evidence_bindings.main_dialogue_record_id,
        target_glyph_count: target_glyphs.len(),
        source_reclaimable_active_code_count: None,
        preserved_active_source_code_count: preserved_active_source_codes.len(),
        preservation_policy: "preserve the irregular frozen-frame screen union plus exact protected outputs from the selected consumers",
        source_binding: evidence_bindings.source_binding,
    };
    let evidence_bytes =
        serde_json::to_vec(&evidence).context("serialize chapter-save runtime evidence")?;
    Ok(TranslationLifetimeDemandReport {
        screen_role,
        measurement_basis,
        target_glyph_count: target_glyphs.len(),
        preserved_active_source_code_count: preserved_active_source_codes.len(),
        additional_target_glyph_reservation_count: 0,
        total_slot_demand,
        active_slot_count: ACTIVE_HANGUL_SLOT_COUNT,
        fits_active_page: true,
        evidence_report_sha1: sha1_hex(&evidence_bytes),
    })
}
