use std::{
    collections::{BTreeMap, BTreeSet},
    fs,
    path::{Path, PathBuf},
};

use anyhow::{Context, Result, bail, ensure};
use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::{
    rom::{EXPECTED_SOURCE_SHA1, Rom},
    sha1_hex,
};

const MANIFEST_SCHEMA: u8 = 1;
const REPORT_SCHEMA: u8 = 1;
const NAMETABLE_BYTE_COUNT: usize = 0x800;
const NAMETABLE_PAGE_BYTE_COUNT: usize = 0x400;
const NAMETABLE_TILE_BYTE_COUNT: usize = 0x3C0;
const OAM_BYTE_COUNT: usize = 0x100;
const PALETTE_BYTE_COUNT: usize = 0x20;
const INTERNAL_RAM_BYTE_COUNT: usize = 0x800;
const PRG_RAM_BYTE_COUNT: usize = 0x2000;
const VISIBLE_SPRITE_Y_MAX: u8 = 0xEE;
const MIN_IRREGULAR_SAMPLE_COUNT: usize = 4;

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
    visible_sprite_count: usize,
    memory_expectation_count: usize,
}

#[derive(Clone, Debug, Eq, Ord, PartialEq, PartialOrd, Serialize)]
struct ChrPairReport {
    left_fd: u8,
    left_fe: u8,
    right_fd: u8,
    right_fe: u8,
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

struct CaptureFiles {
    screenshot: Vec<u8>,
    state: Vec<u8>,
    internal_ram: Vec<u8>,
    prg_ram: Vec<u8>,
    nametable: Vec<u8>,
    oam: Vec<u8>,
    palette: Vec<u8>,
}

struct CaptureState {
    producer_frame_count: u64,
    chr_pair: ChrPairReport,
    left_latch: u8,
    right_latch: u8,
    background_enabled: bool,
    sprites_enabled: bool,
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

fn validate_route_contract(route: &RouteInput) -> Result<()> {
    ensure!(!route.route_role.is_empty(), "temporal route role is empty");
    ensure!(
        !route.entry_action.is_empty(),
        "{} entry action is empty",
        route.route_role
    );
    ensure!(
        !route.source_bound_effect.is_empty(),
        "{} source-bound effect is empty",
        route.route_role
    );
    ensure!(
        route.samples.len() >= MIN_IRREGULAR_SAMPLE_COUNT,
        "{} needs at least {MIN_IRREGULAR_SAMPLE_COUNT} temporal samples",
        route.route_role
    );
    match route.route_role.as_str() {
        "sound_test_shared_battle" => {
            ensure!(
                route.entry_action == "START",
                "sound-test battle must enter through START"
            );
            ensure!(
                !route.negative_case,
                "sound-test battle is not a negative route"
            );
        }
        "sound_test_automatic_ending" => {
            ensure!(
                route.entry_action == "SELECT",
                "sound-test ending must enter through SELECT"
            );
            ensure!(
                !route.negative_case,
                "sound-test ending is not a negative route"
            );
        }
        "gameplay_battle_favorable" => ensure!(
            !route.negative_case,
            "favorable gameplay battle is not a negative route"
        ),
        "gameplay_battle_unfavorable" | "gameplay_battle_defeat" => ensure!(
            route.negative_case,
            "{} must be marked as a negative route",
            route.route_role
        ),
        other => bail!("unknown temporal route role {other}"),
    }
    Ok(())
}

fn analyze_route(route: &RouteInput, manifest_root: &Path) -> Result<RouteReport> {
    let frame_offsets = route
        .samples
        .iter()
        .map(|sample| sample.frame_offset)
        .collect::<Vec<_>>();
    ensure!(
        frame_offsets.windows(2).all(|window| window[0] < window[1]),
        "{} frame offsets must be strictly increasing",
        route.route_role
    );
    let frame_deltas = frame_offsets
        .windows(2)
        .map(|window| window[1] - window[0])
        .collect::<BTreeSet<_>>();
    let irregular_temporal_sampling = frame_deltas.len() > 1;
    ensure!(
        irregular_temporal_sampling,
        "{} frame offsets must be irregular rather than a fixed-step sample",
        route.route_role
    );

    let mut samples = Vec::new();
    let mut screen_roles = BTreeSet::new();
    let mut screenshot_sha1s = BTreeSet::new();
    let mut nametable_sha1s = BTreeSet::new();
    let mut oam_sha1s = BTreeSet::new();
    let mut palette_sha1s = BTreeSet::new();
    let mut chr_pairs = BTreeSet::new();
    let mut nametable_tile_codes = BTreeSet::new();
    let mut visible_sprite_tile_codes = BTreeSet::new();
    let mut memory_expectation_count = 0;
    let mut screen_role_variants = BTreeMap::<String, ScreenRoleVariantAccumulator>::new();
    let mut producer_frame_counts = Vec::new();
    let mut capture_dirs = BTreeSet::new();

    for sample in &route.samples {
        validate_screen_role(&route.route_role, &sample.screen_role)?;
        ensure!(
            !sample.expected_memory.is_empty(),
            "{} frame {} has no expected state bytes",
            route.route_role,
            sample.frame_offset
        );
        let capture_dir = resolve_capture_dir(manifest_root, &sample.capture_dir)?;
        ensure!(
            capture_dirs.insert(capture_dir.clone()),
            "{} reuses temporal capture directory {}",
            route.route_role,
            capture_dir.display()
        );
        let files = read_capture_files(&capture_dir)?;
        validate_memory_expectations(sample, &files)?;
        let state = parse_capture_state(&files.state)?;
        let screenshot_sha1 = sha1_hex(&files.screenshot);
        let state_sha1 = sha1_hex(&files.state);
        let internal_ram_sha1 = sha1_hex(&files.internal_ram);
        let prg_ram_sha1 = sha1_hex(&files.prg_ram);
        let nametable_sha1 = sha1_hex(&files.nametable);
        let oam_sha1 = sha1_hex(&files.oam);
        let palette_sha1 = sha1_hex(&files.palette);
        let sample_nametable_tiles = nametable_tile_codes_for(&files.nametable);
        let (sample_sprite_tiles, visible_sprite_count) = visible_sprite_tile_codes_for(&files.oam);

        screen_roles.insert(sample.screen_role.clone());
        screenshot_sha1s.insert(screenshot_sha1.clone());
        nametable_sha1s.insert(nametable_sha1.clone());
        oam_sha1s.insert(oam_sha1.clone());
        palette_sha1s.insert(palette_sha1.clone());
        chr_pairs.insert(state.chr_pair.clone());
        nametable_tile_codes.extend(sample_nametable_tiles);
        visible_sprite_tile_codes.extend(sample_sprite_tiles);
        memory_expectation_count += sample.expected_memory.len();
        producer_frame_counts.push(state.producer_frame_count);
        let role_variant = screen_role_variants
            .entry(sample.screen_role.clone())
            .or_default();
        role_variant.sample_count += 1;
        role_variant.frame_offsets.push(sample.frame_offset);
        role_variant
            .screenshot_sha1s
            .insert(screenshot_sha1.clone());
        role_variant.nametable_sha1s.insert(nametable_sha1.clone());
        role_variant.oam_sha1s.insert(oam_sha1.clone());
        role_variant.palette_sha1s.insert(palette_sha1.clone());

        samples.push(SampleReport {
            frame_offset: sample.frame_offset,
            screen_role: sample.screen_role.clone(),
            producer_frame_count: state.producer_frame_count,
            screenshot_sha1,
            state_sha1,
            internal_ram_sha1,
            prg_ram_sha1,
            nametable_sha1,
            oam_sha1,
            palette_sha1,
            chr_pair: state.chr_pair,
            left_latch: state.left_latch,
            right_latch: state.right_latch,
            background_enabled: state.background_enabled,
            sprites_enabled: state.sprites_enabled,
            visible_sprite_count,
            memory_expectation_count: sample.expected_memory.len(),
        });
    }
    validate_producer_frame_deltas(&route.route_role, &frame_offsets, &producer_frame_counts)?;
    if route.route_role == "sound_test_automatic_ending" {
        let observed_roles = screen_role_variants
            .keys()
            .map(String::as_str)
            .collect::<BTreeSet<_>>();
        let expected_roles = ENDING_SCREEN_ROLES.into_iter().collect::<BTreeSet<_>>();
        ensure!(
            observed_roles == expected_roles,
            "automatic ending temporal samples do not cover all five ending screen roles"
        );
        let final_signature = screen_role_variants
            .get("ending_final_signature")
            .context("automatic ending has no final-signature samples")?;
        ensure!(
            final_signature.sample_count >= 4
                && final_signature.frame_offsets.last().unwrap()
                    - final_signature.frame_offsets.first().unwrap()
                    >= 12_000
                && final_signature.screenshot_sha1s.len() == 1
                && final_signature.nametable_sha1s.len() == 1
                && final_signature.oam_sha1s.len() == 1
                && final_signature.palette_sha1s.len() == 1,
            "ending final signature is not stable across the admitted long-span samples"
        );
    }
    let screen_role_variants = screen_role_variants
        .into_iter()
        .map(|(screen_role, variant)| ScreenRoleVariantReport {
            screen_role,
            sample_count: variant.sample_count,
            distinct_screenshot_count: variant.screenshot_sha1s.len(),
            distinct_nametable_count: variant.nametable_sha1s.len(),
            distinct_oam_count: variant.oam_sha1s.len(),
            distinct_palette_count: variant.palette_sha1s.len(),
        })
        .collect();

    Ok(RouteReport {
        route_role: route.route_role.clone(),
        entry_action: route.entry_action.clone(),
        source_bound_effect: route.source_bound_effect.clone(),
        negative_case: route.negative_case,
        sample_count: samples.len(),
        frame_offsets,
        irregular_temporal_sampling,
        screen_roles: screen_roles.into_iter().collect(),
        distinct_screenshot_count: screenshot_sha1s.len(),
        distinct_nametable_count: nametable_sha1s.len(),
        distinct_oam_count: oam_sha1s.len(),
        distinct_palette_count: palette_sha1s.len(),
        memory_expectation_count,
        screen_role_variants,
        chr_pairs: chr_pairs.into_iter().collect(),
        nametable_tile_codes_hex: hex_codes(nametable_tile_codes),
        visible_sprite_tile_codes_hex: hex_codes(visible_sprite_tile_codes),
        samples,
    })
}

fn validate_producer_frame_deltas(
    route_role: &str,
    declared_frame_offsets: &[u64],
    producer_frame_counts: &[u64],
) -> Result<()> {
    ensure!(
        declared_frame_offsets.len() == producer_frame_counts.len(),
        "{route_role} declared and producer frame counts have different lengths"
    );
    for (declared, produced) in declared_frame_offsets
        .windows(2)
        .zip(producer_frame_counts.windows(2))
    {
        ensure!(
            produced[0] < produced[1],
            "{route_role} producer frame counts must be strictly increasing"
        );
        ensure!(
            declared[1] - declared[0] == produced[1] - produced[0],
            "{route_role} producer frame delta does not match the declared exact-step delta"
        );
    }
    Ok(())
}

fn validate_screen_role(route_role: &str, screen_role: &str) -> Result<()> {
    match route_role {
        "sound_test_automatic_ending" => ensure!(
            ENDING_SCREEN_ROLES.contains(&screen_role),
            "ending route sample has unknown screen role {screen_role}"
        ),
        "gameplay_battle_defeat" => ensure!(
            matches!(screen_role, "battle_animation" | "game_over"),
            "defeat route sample has unknown screen role {screen_role}"
        ),
        _ => ensure!(
            screen_role == "battle_animation",
            "battle route sample has unknown screen role {screen_role}"
        ),
    }
    Ok(())
}

fn resolve_capture_dir(manifest_root: &Path, capture_dir: &Path) -> Result<PathBuf> {
    let resolved = if capture_dir.is_absolute() {
        capture_dir.to_path_buf()
    } else {
        manifest_root.join(capture_dir)
    };
    ensure!(
        resolved.is_dir(),
        "temporal capture directory does not exist: {}",
        resolved.display()
    );
    Ok(resolved)
}

fn read_capture_files(capture_dir: &Path) -> Result<CaptureFiles> {
    let read = |name: &str| {
        let path = capture_dir.join(name);
        fs::read(&path).with_context(|| format!("read {}", path.display()))
    };
    let files = CaptureFiles {
        screenshot: read("screenshot.png")?,
        state: read("state.json")?,
        internal_ram: read("iram.bin")?,
        prg_ram: read("prgram.bin")?,
        nametable: read("nametable.bin")?,
        oam: read("oam.bin")?,
        palette: read("palette.bin")?,
    };
    ensure!(
        files.screenshot.starts_with(b"\x89PNG\r\n\x1A\n"),
        "{} screenshot is not PNG",
        capture_dir.display()
    );
    ensure!(
        files.internal_ram.len() == INTERNAL_RAM_BYTE_COUNT,
        "{} internal RAM dump length changed",
        capture_dir.display()
    );
    ensure!(
        files.prg_ram.len() == PRG_RAM_BYTE_COUNT,
        "{} PRG RAM dump length changed",
        capture_dir.display()
    );
    ensure!(
        files.nametable.len() == NAMETABLE_BYTE_COUNT,
        "{} nametable dump length changed",
        capture_dir.display()
    );
    ensure!(
        files.oam.len() == OAM_BYTE_COUNT,
        "{} OAM dump length changed",
        capture_dir.display()
    );
    ensure!(
        files.palette.len() == PALETTE_BYTE_COUNT,
        "{} palette dump length changed",
        capture_dir.display()
    );
    Ok(files)
}

fn validate_memory_expectations(sample: &SampleInput, files: &CaptureFiles) -> Result<()> {
    for expectation in &sample.expected_memory {
        ensure!(
            !expectation.reason.is_empty(),
            "frame {} has an expected byte range without a reason",
            sample.frame_offset
        );
        let expected = decode_hex(&expectation.bytes_hex).with_context(|| {
            format!(
                "decode frame {} expected {} bytes",
                sample.frame_offset,
                expectation.region.file_name()
            )
        })?;
        ensure!(
            !expected.is_empty(),
            "frame {} expected byte range is empty",
            sample.frame_offset
        );
        let region = match expectation.region {
            MemoryRegion::InternalRam => &files.internal_ram,
            MemoryRegion::PrgRam => &files.prg_ram,
        };
        let base = expectation.region.base_address();
        ensure!(
            expectation.address >= base,
            "frame {} expected address 0x{:04X} is below {}",
            sample.frame_offset,
            expectation.address,
            expectation.region.file_name()
        );
        let offset = expectation.address - base;
        let end = offset
            .checked_add(expected.len())
            .context("expected memory range overflow")?;
        ensure!(
            end <= expectation.region.byte_count() && end <= region.len(),
            "frame {} expected range crosses {}",
            sample.frame_offset,
            expectation.region.file_name()
        );
        ensure!(
            region[offset..end] == expected,
            "frame {} expected bytes at 0x{:04X} changed ({})",
            sample.frame_offset,
            expectation.address,
            expectation.reason
        );
    }
    Ok(())
}

fn decode_hex(value: &str) -> Result<Vec<u8>> {
    ensure!(
        value.len().is_multiple_of(2),
        "hex byte string has odd length"
    );
    (0..value.len())
        .step_by(2)
        .map(|index| {
            u8::from_str_radix(&value[index..index + 2], 16)
                .with_context(|| format!("invalid hex byte at character {index}"))
        })
        .collect()
}

fn parse_capture_state(bytes: &[u8]) -> Result<CaptureState> {
    let state: BTreeMap<String, Value> =
        serde_json::from_slice(bytes).context("parse producer state JSON")?;
    let unsigned = |key: &str| -> Result<u64> {
        state
            .get(key)
            .and_then(Value::as_u64)
            .with_context(|| format!("producer state has no unsigned {key}"))
    };
    let boolean = |key: &str| -> Result<bool> {
        state
            .get(key)
            .and_then(Value::as_bool)
            .with_context(|| format!("producer state has no boolean {key}"))
    };
    let byte = |key: &str| -> Result<u8> {
        unsigned(key)?
            .try_into()
            .with_context(|| format!("producer state {key} does not fit a byte"))
    };

    Ok(CaptureState {
        producer_frame_count: unsigned("frameCount")?,
        chr_pair: ChrPairReport {
            left_fd: byte("mapper.leftChrPage[0]")?,
            left_fe: byte("mapper.leftChrPage[1]")?,
            right_fd: byte("mapper.rightChrPage[0]")?,
            right_fe: byte("mapper.rightChrPage[1]")?,
        },
        left_latch: byte("mapper.leftLatch")?,
        right_latch: byte("mapper.rightLatch")?,
        background_enabled: boolean("ppu.mask.backgroundEnabled")?,
        sprites_enabled: boolean("ppu.mask.spritesEnabled")?,
    })
}

fn nametable_tile_codes_for(nametable: &[u8]) -> BTreeSet<u8> {
    (0..2)
        .flat_map(|page| {
            let start = page * NAMETABLE_PAGE_BYTE_COUNT;
            nametable[start..start + NAMETABLE_TILE_BYTE_COUNT]
                .iter()
                .copied()
        })
        .collect()
}

fn visible_sprite_tile_codes_for(oam: &[u8]) -> (BTreeSet<u8>, usize) {
    let visible_sprites = oam
        .chunks_exact(4)
        .filter(|sprite| sprite[0] <= VISIBLE_SPRITE_Y_MAX)
        .collect::<Vec<_>>();
    let tile_codes = visible_sprites.iter().map(|sprite| sprite[1]).collect();
    (tile_codes, visible_sprites.len())
}

fn hex_codes(codes: BTreeSet<u8>) -> Vec<String> {
    codes
        .into_iter()
        .map(|code| format!("{code:02X}"))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn irregular_sampling_rejects_a_fixed_step_series_before_reading_captures() {
        let route = RouteInput {
            route_role: "sound_test_shared_battle".to_owned(),
            entry_action: "START".to_owned(),
            source_bound_effect: "enter shared battle engine".to_owned(),
            negative_case: false,
            samples: [0, 10, 20, 30]
                .into_iter()
                .map(|frame_offset| SampleInput {
                    frame_offset,
                    screen_role: "battle_animation".to_owned(),
                    capture_dir: PathBuf::from("absent"),
                    expected_memory: vec![MemoryExpectation {
                        region: MemoryRegion::PrgRam,
                        address: 0x7730,
                        bytes_hex: "05".to_owned(),
                        reason: "shared battle outer phase".to_owned(),
                    }],
                })
                .collect(),
        };

        let error = analyze_route(&route, Path::new("."))
            .unwrap_err()
            .to_string();
        assert!(error.contains("irregular"));
    }

    #[test]
    fn visible_sprite_union_excludes_hidden_oam_entries() {
        let mut oam = vec![0xFF; OAM_BYTE_COUNT];
        oam[0..4].copy_from_slice(&[0x20, 0x31, 0x00, 0x40]);
        oam[4..8].copy_from_slice(&[0xEF, 0x32, 0x00, 0x40]);
        oam[8..12].copy_from_slice(&[0xEE, 0x33, 0x00, 0x40]);

        let (codes, count) = visible_sprite_tile_codes_for(&oam);

        assert_eq!(count, 2);
        assert_eq!(codes, BTreeSet::from([0x31, 0x33]));
    }

    #[test]
    fn nametable_union_reads_tile_bytes_from_both_physical_pages_only() {
        let mut nametable = vec![0; NAMETABLE_BYTE_COUNT];
        nametable[0] = 0x11;
        nametable[NAMETABLE_PAGE_BYTE_COUNT] = 0x22;
        nametable[NAMETABLE_TILE_BYTE_COUNT] = 0x33;
        nametable[NAMETABLE_PAGE_BYTE_COUNT + NAMETABLE_TILE_BYTE_COUNT] = 0x44;

        let codes = nametable_tile_codes_for(&nametable);

        assert!(codes.contains(&0x11));
        assert!(codes.contains(&0x22));
        assert!(!codes.contains(&0x33));
        assert!(!codes.contains(&0x44));
    }

    #[test]
    fn memory_expectations_use_cpu_addresses_for_each_dump_region() {
        let mut files = CaptureFiles {
            screenshot: b"\x89PNG\r\n\x1A\n".to_vec(),
            state: Vec::new(),
            internal_ram: vec![0; INTERNAL_RAM_BYTE_COUNT],
            prg_ram: vec![0; PRG_RAM_BYTE_COUNT],
            nametable: vec![0; NAMETABLE_BYTE_COUNT],
            oam: vec![0xFF; OAM_BYTE_COUNT],
            palette: vec![0; PALETTE_BYTE_COUNT],
        };
        files.internal_ram[0x47C] = 0x1F;
        files.prg_ram[0x1730] = 0x05;
        let sample = SampleInput {
            frame_offset: 19,
            screen_role: "battle_animation".to_owned(),
            capture_dir: PathBuf::new(),
            expected_memory: vec![
                MemoryExpectation {
                    region: MemoryRegion::InternalRam,
                    address: 0x047C,
                    bytes_hex: "1F".to_owned(),
                    reason: "shared engine terminal phase".to_owned(),
                },
                MemoryExpectation {
                    region: MemoryRegion::PrgRam,
                    address: 0x7730,
                    bytes_hex: "05".to_owned(),
                    reason: "sound-test outer phase".to_owned(),
                },
            ],
        };

        validate_memory_expectations(&sample, &files).unwrap();
    }

    #[test]
    fn required_route_polarity_keeps_favorable_and_negative_cases_distinct() {
        let route = RouteInput {
            route_role: "gameplay_battle_unfavorable".to_owned(),
            entry_action: "source-bound gameplay attack".to_owned(),
            source_bound_effect: "attacker misses and receives damage".to_owned(),
            negative_case: false,
            samples: Vec::new(),
        };

        let error = validate_route_contract(&route).unwrap_err().to_string();
        assert!(error.contains("at least"));

        let mut route = route;
        route.samples = (0..MIN_IRREGULAR_SAMPLE_COUNT)
            .map(|index| SampleInput {
                frame_offset: u64::try_from(index * index + index).unwrap(),
                screen_role: "battle_animation".to_owned(),
                capture_dir: PathBuf::new(),
                expected_memory: Vec::new(),
            })
            .collect();
        let error = validate_route_contract(&route).unwrap_err().to_string();
        assert!(error.contains("negative route"));
    }

    #[test]
    fn producer_frame_deltas_must_match_declared_exact_steps() {
        validate_producer_frame_deltas(
            "sound_test_shared_battle",
            &[43, 82, 171],
            &[10_043, 10_082, 10_171],
        )
        .unwrap();

        let error = validate_producer_frame_deltas(
            "sound_test_shared_battle",
            &[43, 82, 171],
            &[10_043, 10_083, 10_171],
        )
        .unwrap_err()
        .to_string();
        assert!(error.contains("exact-step delta"));
    }
}
