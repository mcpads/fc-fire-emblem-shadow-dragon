use std::{
    collections::{BTreeMap, BTreeSet},
    fs,
    path::{Path, PathBuf},
};

use anyhow::{Context, Result, bail, ensure};
use serde::{Deserialize, Serialize};

use crate::{
    font_slots::active_hangul_codes,
    rom::{EXPECTED_SOURCE_SHA1, Rom},
    sha1_hex,
};

pub(crate) mod capture_state;
pub(crate) mod route_analysis;
#[cfg(test)]
mod tests;

use capture_state::{ChrPairReport, parse_capture_state};
use route_analysis::*;

const MANIFEST_SCHEMA: u8 = 1;
const REPORT_SCHEMA: u8 = 3;
const NAMETABLE_BYTE_COUNT: usize = 0x800;
const NAMETABLE_PAGE_BYTE_COUNT: usize = 0x400;
const NAMETABLE_TILE_BYTE_COUNT: usize = 0x3C0;
const OAM_BYTE_COUNT: usize = 0x100;
const PALETTE_BYTE_COUNT: usize = 0x20;
const INTERNAL_RAM_BYTE_COUNT: usize = 0x800;
const PRG_RAM_BYTE_COUNT: usize = 0x2000;
const VISIBLE_SPRITE_Y_MAX: u8 = 0xEE;
const MIN_IRREGULAR_SAMPLE_COUNT: usize = 4;
const BATTLE_CACHE_PATTERN_ADDRESS: u16 = 0x1000;

const REQUIRED_ROUTE_ROLES: [&str; 5] = [
    "sound_test_shared_battle",
    "gameplay_battle_favorable",
    "gameplay_battle_unfavorable",
    "gameplay_battle_defeat",
    "sound_test_automatic_ending",
];

const ENDING_SCREEN_ROLES: [&str; 5] = [
    "ending_opening_and_cast_scroll",
    "ending_chapter_record_scroll",
    "ending_staff_credits",
    "ending_character_epilogue",
    "ending_final_signature",
];

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct TemporalSurfaceManifest {
    schema: u8,
    source_sha1: String,
    routes: Vec<RouteInput>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct RouteInput {
    route_role: String,
    entry_action: String,
    source_bound_effect: String,
    negative_case: bool,
    samples: Vec<SampleInput>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct SampleInput {
    frame_offset: u64,
    screen_role: String,
    capture_dir: PathBuf,
    expected_memory: Vec<MemoryExpectation>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct MemoryExpectation {
    region: MemoryRegion,
    address: usize,
    bytes_hex: String,
    reason: String,
}

#[derive(Clone, Copy, Debug, Deserialize)]
#[serde(rename_all = "snake_case")]
enum MemoryRegion {
    InternalRam,
    PrgRam,
}

impl MemoryRegion {
    fn file_name(self) -> &'static str {
        match self {
            Self::InternalRam => "iram.bin",
            Self::PrgRam => "prgram.bin",
        }
    }

    fn base_address(self) -> usize {
        match self {
            Self::InternalRam => 0x0000,
            Self::PrgRam => 0x6000,
        }
    }

    fn byte_count(self) -> usize {
        match self {
            Self::InternalRam => INTERNAL_RAM_BYTE_COUNT,
            Self::PrgRam => PRG_RAM_BYTE_COUNT,
        }
    }
}

#[derive(Debug, Serialize)]
struct TemporalSurfaceReport {
    schema: u8,
    source_sha1: &'static str,
    manifest_sha1: String,
    scope: ReportScope,
    summary: ReportSummary,
    required_route_roles: &'static [&'static str],
    missing_route_roles: Vec<&'static str>,
    routes: Vec<RouteReport>,
    union: TemporalUnionReport,
    unresolved: Vec<&'static str>,
    release_eligible: bool,
}

#[derive(Debug, Serialize)]
struct ReportScope {
    translation_direction: &'static str,
    preserve_existing_english_and_digits: bool,
    dialogue_content_emitted: bool,
    evidence_paths_emitted: bool,
    proof_boundary: &'static str,
}

#[derive(Debug, Serialize)]
struct ReportSummary {
    route_count: usize,
    sample_count: usize,
    screen_role_count: usize,
    chr_pair_count: usize,
    nametable_tile_code_count: usize,
    visible_sprite_tile_code_count: usize,
    distinct_screenshot_count: usize,
    negative_route_count: usize,
    required_route_coverage_complete: bool,
    every_route_irregularly_sampled: bool,
    every_sample_memory_checked: bool,
}

#[derive(Debug, Serialize)]
struct RouteReport {
    route_role: String,
    entry_action: String,
    source_bound_effect: String,
    negative_case: bool,
    sample_count: usize,
    frame_offsets: Vec<u64>,
    irregular_temporal_sampling: bool,
    screen_roles: Vec<String>,
    distinct_screenshot_count: usize,
    distinct_nametable_count: usize,
    distinct_oam_count: usize,
    distinct_palette_count: usize,
    memory_expectation_count: usize,
    game_over_dialogue_selector_hex: Option<String>,
    game_over_dialogue_selector_sample_count: usize,
    screen_role_variants: Vec<ScreenRoleVariantReport>,
    chr_pairs: Vec<ChrPairReport>,
    nametable_tile_codes_hex: Vec<String>,
    visible_sprite_tile_codes_hex: Vec<String>,
    samples: Vec<SampleReport>,
}

#[derive(Debug, Serialize)]
struct ScreenRoleVariantReport {
    screen_role: String,
    sample_count: usize,
    distinct_screenshot_count: usize,
    distinct_nametable_count: usize,
    distinct_oam_count: usize,
    distinct_palette_count: usize,
}

#[derive(Debug, Serialize)]
struct SampleReport {
    frame_offset: u64,
    screen_role: String,
    producer_frame_count: u64,
    screenshot_sha1: String,
    state_sha1: String,
    internal_ram_sha1: String,
    prg_ram_sha1: String,
    nametable_sha1: String,
    oam_sha1: String,
    palette_sha1: String,
    chr_pair: ChrPairReport,
    left_latch: u8,
    right_latch: u8,
    background_enabled: bool,
    sprites_enabled: bool,
    background_pattern_address_hex: String,
    sprite_pattern_address_hex: String,
    visible_sprite_count: usize,
    memory_expectation_count: usize,
}

#[derive(Debug, Serialize)]
struct TemporalUnionReport {
    screen_roles: Vec<String>,
    chr_pairs: Vec<ChrPairReport>,
    nametable_tile_codes_hex: Vec<String>,
    visible_sprite_tile_codes_hex: Vec<String>,
    screenshot_sha1s: Vec<String>,
}

pub struct TemporalSurfaceSummary {
    pub report_sha1: String,
    pub route_count: usize,
    pub sample_count: usize,
    pub chr_pair_count: usize,
    pub required_route_coverage_complete: bool,
}

#[derive(Clone, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub(crate) struct ObservedBattleRuntimeInput {
    pub(crate) participant_record_identities: [u8; 2],
    pub(crate) class_record_identities: [u8; 2],
    pub(crate) item_source_indices: [u8; 2],
    pub(crate) terrain_source_indices: [u8; 2],
    pub(crate) observed_dialogue_selector: u8,
    pub(crate) projected_dialogue_selector: u8,
    pub(crate) selector_62_predicate_matched: bool,
}

pub(crate) struct ObservedBattleTemporalSample {
    pub(crate) route_role: String,
    pub(crate) active_tile_codes: BTreeSet<u8>,
    pub(crate) nametable_constrains_cache: bool,
    pub(crate) visible_oam_constrains_cache: bool,
    pub(crate) runtime_input: ObservedBattleRuntimeInput,
}

pub(crate) struct ObservedBattleTemporalEvidence {
    pub(crate) manifest_sha1: String,
    pub(crate) samples: Vec<ObservedBattleTemporalSample>,
}

struct CaptureFiles {
    screenshot: Vec<u8>,
    state: Vec<u8>,
    internal_ram: Vec<u8>,
    prg_ram: Vec<u8>,
    nametable: Vec<u8>,
    oam: Vec<u8>,
    palette: Vec<u8>,
}

#[derive(Default)]
struct ScreenRoleVariantAccumulator {
    sample_count: usize,
    frame_offsets: Vec<u64>,
    screenshot_sha1s: BTreeSet<String>,
    nametable_sha1s: BTreeSet<String>,
    oam_sha1s: BTreeSet<String>,
    palette_sha1s: BTreeSet<String>,
}

pub fn analyze_temporal_surfaces(
    source_path: &Path,
    manifest_path: &Path,
    report_path: &Path,
) -> Result<TemporalSurfaceSummary> {
    let rom = Rom::from_path(source_path)?;
    rom.verify_supported_japanese()?;
    let manifest_bytes =
        fs::read(manifest_path).with_context(|| format!("read {}", manifest_path.display()))?;
    let manifest: TemporalSurfaceManifest = serde_json::from_slice(&manifest_bytes)
        .with_context(|| format!("parse {}", manifest_path.display()))?;
    let manifest_root = manifest_path.parent().unwrap_or_else(|| Path::new("."));
    let report = build_report(&manifest, &sha1_hex(&manifest_bytes), manifest_root)?;
    let mut report_bytes =
        serde_json::to_vec_pretty(&report).context("serialize temporal-surface report")?;
    report_bytes.push(b'\n');
    let report_sha1 = sha1_hex(&report_bytes);

    if let Some(parent) = report_path.parent() {
        fs::create_dir_all(parent)
            .with_context(|| format!("create output directory {}", parent.display()))?;
    }
    fs::write(report_path, report_bytes)
        .with_context(|| format!("write {}", report_path.display()))?;

    Ok(TemporalSurfaceSummary {
        report_sha1,
        route_count: report.summary.route_count,
        sample_count: report.summary.sample_count,
        chr_pair_count: report.summary.chr_pair_count,
        required_route_coverage_complete: report.summary.required_route_coverage_complete,
    })
}

pub(crate) fn load_observed_battle_temporal_evidence(
    source_path: &Path,
    manifest_path: &Path,
) -> Result<ObservedBattleTemporalEvidence> {
    let rom = Rom::from_path(source_path)?;
    rom.verify_supported_japanese()?;
    let manifest_bytes =
        fs::read(manifest_path).with_context(|| format!("read {}", manifest_path.display()))?;
    let manifest: TemporalSurfaceManifest = serde_json::from_slice(&manifest_bytes)
        .with_context(|| format!("parse {}", manifest_path.display()))?;
    let manifest_root = manifest_path.parent().unwrap_or_else(|| Path::new("."));
    build_report(&manifest, &sha1_hex(&manifest_bytes), manifest_root)?;

    let active_codes = active_hangul_codes().into_iter().collect::<BTreeSet<_>>();
    let mut samples = Vec::new();
    for route in &manifest.routes {
        for sample in &route.samples {
            if sample.screen_role != "battle_animation" {
                continue;
            }
            let capture_dir = resolve_capture_dir(manifest_root, &sample.capture_dir)?;
            let files = read_capture_files(&capture_dir)?;
            validate_memory_expectations(sample, &files)?;
            let state = parse_capture_state(&files.state)?;
            let nametable_constrains_cache = state.background_enabled
                && state.background_pattern_address == BATTLE_CACHE_PATTERN_ADDRESS;
            let visible_oam_constrains_cache = state.sprites_enabled
                && state.sprite_pattern_address == BATTLE_CACHE_PATTERN_ADDRESS;
            let nametable_codes = nametable_constrains_cache
                .then(|| nametable_tile_codes_for(&files.nametable))
                .into_iter()
                .flatten();
            let visible_oam_codes = visible_oam_constrains_cache
                .then(|| visible_sprite_tile_codes_for(&files.oam).0)
                .into_iter()
                .flatten();
            let active_tile_codes = nametable_codes
                .chain(visible_oam_codes)
                .filter(|code| active_codes.contains(code))
                .collect::<BTreeSet<_>>();
            samples.push(ObservedBattleTemporalSample {
                route_role: route.route_role.clone(),
                active_tile_codes,
                nametable_constrains_cache,
                visible_oam_constrains_cache,
                runtime_input: observed_battle_runtime_input(&files)?,
            });
        }
    }
    ensure!(
        !samples.is_empty(),
        "temporal evidence contains no battle-animation samples"
    );
    Ok(ObservedBattleTemporalEvidence {
        manifest_sha1: sha1_hex(&manifest_bytes),
        samples,
    })
}

fn observed_battle_runtime_input(files: &CaptureFiles) -> Result<ObservedBattleRuntimeInput> {
    let pair = |address: usize, role: &str| {
        files
            .internal_ram
            .get(address..address + 2)
            .map(|bytes| [bytes[0], bytes[1]])
            .with_context(|| format!("observed battle {role} is outside internal RAM"))
    };
    let observed_dialogue_selector = files
        .prg_ram
        .get(0x7936 - MemoryRegion::PrgRam.base_address())
        .copied()
        .context("observed battle dialogue selector is outside PRG RAM")?;
    let selector_62_predicate_matched = [0x0334, 0x0479, 0x0335]
        .into_iter()
        .all(|address| files.internal_ram[address] != 0)
        && files.internal_ram[0x05DF] == 0;
    let projected_dialogue_selector = if selector_62_predicate_matched {
        0x3E
    } else {
        observed_dialogue_selector
    };
    Ok(ObservedBattleRuntimeInput {
        participant_record_identities: pair(0x0304, "participant identities")?,
        class_record_identities: pair(0x0306, "class identities")?,
        item_source_indices: pair(0x0320, "item source indices")?,
        terrain_source_indices: pair(0x0322, "terrain source indices")?,
        observed_dialogue_selector,
        projected_dialogue_selector,
        selector_62_predicate_matched,
    })
}

fn build_report(
    manifest: &TemporalSurfaceManifest,
    manifest_sha1: &str,
    manifest_root: &Path,
) -> Result<TemporalSurfaceReport> {
    ensure!(
        manifest.schema == MANIFEST_SCHEMA,
        "temporal-surface manifest schema changed"
    );
    ensure!(
        manifest
            .source_sha1
            .eq_ignore_ascii_case(EXPECTED_SOURCE_SHA1),
        "temporal-surface manifest source SHA-1 is not the supported Japanese ROM"
    );
    ensure!(
        !manifest.routes.is_empty(),
        "temporal-surface manifest has no routes"
    );

    let mut route_roles = BTreeSet::new();
    let mut routes = Vec::new();
    for route in &manifest.routes {
        ensure!(
            route_roles.insert(route.route_role.as_str()),
            "duplicate temporal route role {}",
            route.route_role
        );
        validate_route_contract(route)?;
        routes.push(analyze_route(route, manifest_root)?);
    }

    let missing_route_roles = REQUIRED_ROUTE_ROLES
        .iter()
        .copied()
        .filter(|role| !route_roles.contains(role))
        .collect::<Vec<_>>();
    let required_route_coverage_complete = missing_route_roles.is_empty();
    let every_route_irregularly_sampled =
        routes.iter().all(|route| route.irregular_temporal_sampling);
    let every_sample_memory_checked = routes
        .iter()
        .flat_map(|route| &route.samples)
        .all(|sample| sample.memory_expectation_count > 0);

    let screen_roles = routes
        .iter()
        .flat_map(|route| route.screen_roles.iter().cloned())
        .collect::<BTreeSet<_>>();
    let chr_pairs = routes
        .iter()
        .flat_map(|route| route.chr_pairs.iter().cloned())
        .collect::<BTreeSet<_>>();
    let nametable_tile_codes = routes
        .iter()
        .flat_map(|route| route.nametable_tile_codes_hex.iter().cloned())
        .collect::<BTreeSet<_>>();
    let visible_sprite_tile_codes = routes
        .iter()
        .flat_map(|route| route.visible_sprite_tile_codes_hex.iter().cloned())
        .collect::<BTreeSet<_>>();
    let screenshot_sha1s = routes
        .iter()
        .flat_map(|route| {
            route
                .samples
                .iter()
                .map(|sample| sample.screenshot_sha1.clone())
        })
        .collect::<BTreeSet<_>>();
    let sample_count = routes.iter().map(|route| route.sample_count).sum();
    let negative_route_count = routes.iter().filter(|route| route.negative_case).count();

    Ok(TemporalSurfaceReport {
        schema: REPORT_SCHEMA,
        source_sha1: EXPECTED_SOURCE_SHA1,
        manifest_sha1: manifest_sha1.to_owned(),
        scope: ReportScope {
            translation_direction: "Japanese to Korean",
            preserve_existing_english_and_digits: true,
            dialogue_content_emitted: false,
            evidence_paths_emitted: false,
            proof_boundary: "consumer-side aggregation of explicit frozen-frame screenshots, producer state, internal RAM, PRG RAM, nametable RAM, OAM, and palette dumps; route labels and expected bytes are admitted by the manifest and verified against each dump, while the report emits hashes and unions but no evidence paths or dialogue content",
        },
        summary: ReportSummary {
            route_count: routes.len(),
            sample_count,
            screen_role_count: screen_roles.len(),
            chr_pair_count: chr_pairs.len(),
            nametable_tile_code_count: nametable_tile_codes.len(),
            visible_sprite_tile_code_count: visible_sprite_tile_codes.len(),
            distinct_screenshot_count: screenshot_sha1s.len(),
            negative_route_count,
            required_route_coverage_complete,
            every_route_irregularly_sampled,
            every_sample_memory_checked,
        },
        required_route_roles: &REQUIRED_ROUTE_ROLES,
        missing_route_roles,
        routes,
        union: TemporalUnionReport {
            screen_roles: screen_roles.into_iter().collect(),
            chr_pairs: chr_pairs.into_iter().collect(),
            nametable_tile_codes_hex: nametable_tile_codes.into_iter().collect(),
            visible_sprite_tile_codes_hex: visible_sprite_tile_codes.into_iter().collect(),
            screenshot_sha1s: screenshot_sha1s.into_iter().collect(),
        },
        unresolved: vec![
            "A complete byte union proves only what the admitted frozen-frame samples contain; route completeness still depends on source-bound entry effects and an explicit variant census.",
            "Nametable and OAM codes are producer facts, not automatic classifications of text, portraits, cursors, bars, or other graphics.",
            "Translation glyph budgeting remains separate and must preserve existing English, digits, portraits, sprites, and the source-bound static final signature.",
        ],
        release_eligible: false,
    })
}
