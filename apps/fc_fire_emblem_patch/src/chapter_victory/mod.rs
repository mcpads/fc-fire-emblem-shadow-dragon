mod chapter_map;
mod runtime_evidence;
mod screen_flow;
mod source_flow;

use std::{fs, path::Path};

use anyhow::{Context, Result};
use serde::Serialize;

use crate::{
    rom::{Rom, EXPECTED_SOURCE_SHA1},
    sha1_hex,
};

use self::{
    chapter_map::ChapterMapBinding,
    runtime_evidence::ContinuousVictoryRuntimeEvidence,
    screen_flow::{ObservationPlan, RuntimeMapSample, VictoryRouteStep},
    source_flow::{CommandRouteBinding, SourceRegionBinding},
};

#[derive(Debug, Serialize)]
struct ChapterVictoryReport {
    schema: u8,
    source_sha1: &'static str,
    scope: Scope,
    chapter_map: ChapterMapBinding,
    command_route: CommandRouteBinding,
    source_regions: Vec<SourceRegionBinding>,
    route_steps: Vec<VictoryRouteStep>,
    runtime_map_sample: RuntimeMapSample,
    continuous_runtime_evidence: ContinuousVictoryRuntimeEvidence,
    observation_plan: ObservationPlan,
    unresolved: Vec<&'static str>,
    release_eligible: bool,
}

#[derive(Debug, Serialize)]
struct Scope {
    translation_direction: &'static str,
    preserve_existing_english_and_digits: bool,
    dialogue_content_emitted: bool,
    proof_boundary: &'static str,
}

pub struct ChapterVictorySummary {
    pub report_sha1: String,
    pub victory_tile_count: usize,
    pub source_region_count: usize,
    pub route_step_count: usize,
    pub runtime_screen_count: usize,
    pub continuous_gate_closed: bool,
    pub next_observation_gate: &'static str,
}

pub fn analyze_chapter_victory(
    source_path: &Path,
    report_path: &Path,
) -> Result<ChapterVictorySummary> {
    let rom = Rom::from_path(source_path)?;
    rom.verify_supported_japanese()?;
    let report = build_report(&rom)?;
    let mut bytes =
        serde_json::to_vec_pretty(&report).context("serialize chapter-victory report")?;
    bytes.push(b'\n');
    let report_sha1 = sha1_hex(&bytes);

    if let Some(parent) = report_path.parent() {
        fs::create_dir_all(parent)
            .with_context(|| format!("create output directory {}", parent.display()))?;
    }
    fs::write(report_path, bytes).with_context(|| format!("write {}", report_path.display()))?;

    Ok(ChapterVictorySummary {
        report_sha1,
        victory_tile_count: report.chapter_map.victory_tiles.len(),
        source_region_count: report.source_regions.len(),
        route_step_count: report.route_steps.len(),
        runtime_screen_count: report.continuous_runtime_evidence.screen_count(),
        continuous_gate_closed: report.continuous_runtime_evidence.continuous_gate_closed(),
        next_observation_gate: report.observation_plan.next_gate,
    })
}

fn build_report(rom: &Rom) -> Result<ChapterVictoryReport> {
    let chapter_map = chapter_map::bind_chapter_eleven_map(rom.prg())?;
    let (command_route, source_regions) = source_flow::bind_command_route(rom.prg())?;

    Ok(ChapterVictoryReport {
        schema: 1,
        source_sha1: EXPECTED_SOURCE_SHA1,
        scope: Scope {
            translation_direction: "Japanese to Korean",
            preserve_existing_english_and_digits: true,
            dialogue_content_emitted: false,
            proof_boundary: "source-bound chapter-eleven victory tiles and unit-command-to-staged-victory control flow plus the continuous ordinary-control route from the castle command through the chapter-twelve intro with declared progression accelerations; no dialogue source, translation, ROM mutation, coordinate teleport, direct action-state write, or direct victory-stage write",
        },
        chapter_map,
        command_route,
        source_regions,
        route_steps: screen_flow::victory_route_steps(),
        runtime_map_sample: screen_flow::chapter_eleven_runtime_map_sample(),
        continuous_runtime_evidence: runtime_evidence::continuous_chapter_eleven_victory_evidence(),
        observation_plan: screen_flow::observation_plan(),
        unresolved: vec![
            "The save-offer and save-complete no choices are source-bound and runtime-observed; the latter opens a terminal power-off notice with a hidden sound-test unlock.",
            "The accelerated route establishes reachability rather than baseline difficulty or unaccelerated combat equivalence.",
            "Defeat and unfavorable-state checks remain separate validation gates with progression cheats disabled or intentionally adverse.",
            "Other chapter-specific victory, epilogue, portrait, CHR, and transition variants remain unobserved.",
        ],
        release_eligible: false,
    })
}
