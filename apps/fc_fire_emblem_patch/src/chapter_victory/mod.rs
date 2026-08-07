mod chapter_map;
mod screen_flow;
mod source_flow;

use std::{fs, path::Path};

use anyhow::{Context, Result};
use serde::Serialize;

use crate::{
    rom::{EXPECTED_SOURCE_SHA1, Rom},
    sha1_hex,
};

use self::{
    chapter_map::ChapterMapBinding,
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
            proof_boundary: "source-bound chapter-eleven victory tiles and unit-command-to-staged-victory control flow plus one runtime map sample; no dialogue source, translation, ROM mutation, coordinate teleport, or direct action-state write",
        },
        chapter_map,
        command_route,
        source_regions,
        route_steps: screen_flow::victory_route_steps(),
        runtime_map_sample: screen_flow::chapter_eleven_runtime_map_sample(),
        observation_plan: screen_flow::observation_plan(),
        unresolved: vec![
            "The two source victory tiles are initially covered by unit-occupancy tile 0x1B in the runtime map buffer; the report does not claim that しろ is visible yet.",
            "The 0x0C outer-screen victory stages are statically bound but have not been executed in chapter eleven.",
            "Chapter-eleven epilogue page count, portraits, flashing phases, CHR pairs, and the continuous chapter-eleven-to-twelve transition remain runtime gates.",
            "Defeat and unfavorable-state checks remain separate later validation gates with progression cheats disabled or intentionally adverse.",
        ],
        release_eligible: false,
    })
}
