use std::{collections::BTreeSet, fs, path::Path};

use anyhow::{Context, Result, ensure};

use crate::{
    rom::{EXPECTED_SOURCE_SHA1, Rom},
    sha1_hex,
};

mod capture;
mod report;
mod run_analysis;
#[cfg(test)]
mod tests;

use capture::{evidence_tree_sha1, validate_capture_binding, validate_evidence_root};
use report::{
    EpilogueVariantEvidenceReport, EvidenceScope, EvidenceSummary, RoutingNoOpReport,
    VariantUnionReport,
};
use run_analysis::{RunAnalysis, analyze_routing_no_op, analyze_run, expected_run_specs};

const REPORT_SCHEMA: u8 = 1;
pub(super) const SAMPLE_OFFSETS: [u16; 5] = [7, 19, 43, 82, 171];

pub struct EpilogueVariantEvidenceSummary {
    pub report_sha1: String,
    pub visible_entry_count: usize,
    pub sample_count: usize,
    pub chr_pair_count: usize,
    pub evidence_complete: bool,
}

pub fn analyze_epilogue_variants(
    source_path: &Path,
    capture_rom_path: &Path,
    mapper_report_path: &Path,
    evidence_root: &Path,
    report_path: &Path,
) -> Result<EpilogueVariantEvidenceSummary> {
    let source = Rom::from_path(source_path)?;
    source.verify_supported_japanese()?;
    let binding = validate_capture_binding(capture_rom_path, mapper_report_path)?;
    validate_evidence_root(evidence_root)?;

    let analyses = expected_run_specs()
        .into_iter()
        .map(|spec| analyze_run(evidence_root, &spec))
        .collect::<Result<Vec<_>>>()?;
    let routing_no_op = analyze_routing_no_op(evidence_root)?;
    let evidence_sha1 = evidence_tree_sha1(evidence_root)?;
    let report = build_report(binding, evidence_sha1, analyses, routing_no_op);
    ensure!(
        report.summary.every_capture_complete && report.summary.every_entry_irregularly_sampled,
        "epilogue evidence did not close its capture contract"
    );

    let mut bytes =
        serde_json::to_vec_pretty(&report).context("serialize epilogue-variant report")?;
    bytes.push(b'\n');
    let report_sha1 = sha1_hex(&bytes);
    if let Some(parent) = report_path.parent() {
        fs::create_dir_all(parent)
            .with_context(|| format!("create output directory {}", parent.display()))?;
    }
    fs::write(report_path, bytes).with_context(|| format!("write {}", report_path.display()))?;

    Ok(EpilogueVariantEvidenceSummary {
        report_sha1,
        visible_entry_count: report.summary.visible_entry_count,
        sample_count: report.summary.sample_count,
        chr_pair_count: report.summary.chr_pair_count,
        evidence_complete: true,
    })
}

fn build_report(
    binding: capture::CaptureBinding,
    evidence_sha1: String,
    analyses: Vec<RunAnalysis>,
    routing_no_op: RoutingNoOpReport,
) -> EpilogueVariantEvidenceReport {
    let mut chr_pairs = BTreeSet::new();
    let mut screenshot_sha1s = BTreeSet::new();
    let mut nametable_sha1s = BTreeSet::new();
    let mut oam_sha1s = BTreeSet::new();
    let mut palette_sha1s = BTreeSet::new();
    let mut selector_entry_pairs = BTreeSet::new();
    let mut nametable_tile_codes = BTreeSet::new();
    let mut visible_sprite_tile_codes = BTreeSet::new();
    let mut visible_entry_count = 0;
    let mut sample_count = 0;

    for analysis in &analyses {
        visible_entry_count += analysis.report.entry_count;
        sample_count += analysis.report.sample_count;
        chr_pairs.extend(analysis.chr_pairs.iter().cloned());
        screenshot_sha1s.extend(analysis.screenshot_sha1s.iter().cloned());
        nametable_sha1s.extend(analysis.nametable_sha1s.iter().cloned());
        oam_sha1s.extend(analysis.oam_sha1s.iter().cloned());
        palette_sha1s.extend(analysis.palette_sha1s.iter().cloned());
        selector_entry_pairs.extend(analysis.selector_entry_pairs.iter().cloned());
        nametable_tile_codes.extend(analysis.nametable_tile_codes.iter().copied());
        visible_sprite_tile_codes.extend(analysis.visible_sprite_tile_codes.iter().copied());
    }

    let summary = EvidenceSummary {
        run_count: analyses.len(),
        visible_entry_count,
        sample_count,
        samples_per_visible_entry: SAMPLE_OFFSETS.len(),
        every_capture_complete: sample_count == visible_entry_count * SAMPLE_OFFSETS.len(),
        every_entry_irregularly_sampled: analyses
            .iter()
            .all(|analysis| analysis.report.sample_offsets_frames == SAMPLE_OFFSETS),
        chr_pair_count: chr_pairs.len(),
        distinct_screenshot_count: screenshot_sha1s.len(),
        distinct_nametable_count: nametable_sha1s.len(),
        distinct_oam_count: oam_sha1s.len(),
        distinct_palette_count: palette_sha1s.len(),
    };

    EpilogueVariantEvidenceReport {
        schema: REPORT_SCHEMA,
        source_sha1: EXPECTED_SOURCE_SHA1,
        capture_rom_sha1: binding.capture_rom_sha1,
        mapper_probe_report_sha1: binding.mapper_report_sha1,
        evidence_sha1,
        scope: evidence_scope(),
        summary,
        runs: analyses
            .into_iter()
            .map(|analysis| analysis.report)
            .collect(),
        routing_no_op,
        union: VariantUnionReport {
            chr_pairs: chr_pairs.into_iter().collect(),
            screenshot_sha1_count: screenshot_sha1s.len(),
            nametable_sha1_count: nametable_sha1s.len(),
            oam_sha1_count: oam_sha1s.len(),
            palette_sha1_count: palette_sha1s.len(),
            selector_entry_pairs: selector_entry_pairs.into_iter().collect(),
            nametable_tile_codes_hex: hex_codes(nametable_tile_codes),
            visible_sprite_tile_codes_hex: hex_codes(visible_sprite_tile_codes),
        },
        unresolved: vec![
            "the exact gameplay condition assigning direct versus routing action remains unresolved",
        ],
        release_eligible: false,
    }
}

fn evidence_scope() -> EvidenceScope {
    EvidenceScope {
        translation_direction: "Japanese to Korean only",
        preserve_existing_english_and_digits: true,
        dialogue_content_emitted: false,
        evidence_paths_emitted: false,
        intervention_scope: "roster action and location fields only; flow state protected",
        proof_boundary: "natural roster plus synthetic direct and routing unions with five irregular temporal samples per visible entry",
    }
}

fn hex_codes(codes: BTreeSet<u8>) -> Vec<String> {
    codes
        .into_iter()
        .map(|code| format!("{code:02X}"))
        .collect()
}
