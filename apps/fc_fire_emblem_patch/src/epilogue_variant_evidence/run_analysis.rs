use std::{collections::BTreeSet, path::Path};

use anyhow::{Result, ensure};

use crate::{
    sha1_hex,
    temporal_surface::{
        capture_state::ChrPairReport,
        route_analysis::{nametable_tile_codes_for, visible_sprite_tile_codes_for},
    },
};

use super::{
    SAMPLE_OFFSETS,
    capture::{CaptureFiles, directory_names, read_capture},
    report::{RoutingNoOpReport, VariantRunReport},
};

pub(super) struct RunSpec {
    pub(super) role: &'static str,
    pub(super) directory: &'static str,
    pub(super) expected_root_selector: &'static str,
    pub(super) entries: Vec<(u8, u8)>,
    pub(super) allowed_extra_directories: &'static [&'static str],
}

pub(super) struct RunAnalysis {
    pub(super) report: VariantRunReport,
    pub(super) chr_pairs: BTreeSet<ChrPairReport>,
    pub(super) screenshot_sha1s: BTreeSet<String>,
    pub(super) nametable_sha1s: BTreeSet<String>,
    pub(super) oam_sha1s: BTreeSet<String>,
    pub(super) palette_sha1s: BTreeSet<String>,
    pub(super) selector_entry_pairs: BTreeSet<String>,
    pub(super) nametable_tile_codes: BTreeSet<u8>,
    pub(super) visible_sprite_tile_codes: BTreeSet<u8>,
}

pub(super) fn expected_run_specs() -> Vec<RunSpec> {
    vec![
        RunSpec {
            role: "natural_roster",
            directory: "natural",
            expected_root_selector: "40 direct or 41 routing from natural action state",
            entries: vec![
                (0x07, 0x40),
                (0x06, 0x40),
                (0x05, 0x41),
                (0x04, 0x41),
                (0x03, 0x40),
                (0x02, 0x40),
                (0x01, 0x40),
            ],
            allowed_extra_directories: &[],
        },
        RunSpec {
            role: "synthetic_all_direct",
            directory: "all-direct",
            expected_root_selector: "40",
            entries: (1..=0x35).rev().map(|entry| (entry, 0x40)).collect(),
            allowed_extra_directories: &[],
        },
        RunSpec {
            role: "synthetic_all_routing_visible",
            directory: "all-routing",
            expected_root_selector: "41",
            entries: (2..=0x35).rev().map(|entry| (entry, 0x41)).collect(),
            allowed_extra_directories: &["no-op-entry-01"],
        },
    ]
}

pub(super) fn analyze_run(root: &Path, spec: &RunSpec) -> Result<RunAnalysis> {
    let run_dir = root.join(spec.directory);
    validate_entry_directories(&run_dir, spec)?;
    let mut analysis = RunAnalysis {
        report: VariantRunReport {
            run_role: spec.role,
            expected_root_selector: spec.expected_root_selector,
            entry_count: spec.entries.len(),
            first_entry_hex: format!("{:02X}", spec.entries[0].0),
            last_entry_hex: format!("{:02X}", spec.entries[spec.entries.len() - 1].0),
            sample_offsets_frames: SAMPLE_OFFSETS.to_vec(),
            sample_count: 0,
            distinct_screenshot_count: 0,
            distinct_settled_screenshot_count: 0,
            distinct_nametable_count: 0,
            distinct_oam_count: 0,
            distinct_palette_count: 0,
            chr_pairs: Vec::new(),
            selector_entry_pairs: Vec::new(),
            nametable_tile_codes_hex: Vec::new(),
            visible_sprite_tile_codes_hex: Vec::new(),
        },
        chr_pairs: BTreeSet::new(),
        screenshot_sha1s: BTreeSet::new(),
        nametable_sha1s: BTreeSet::new(),
        oam_sha1s: BTreeSet::new(),
        palette_sha1s: BTreeSet::new(),
        selector_entry_pairs: BTreeSet::new(),
        nametable_tile_codes: BTreeSet::new(),
        visible_sprite_tile_codes: BTreeSet::new(),
    };
    let mut settled_screenshot_sha1s = BTreeSet::new();

    for &(entry, expected_root) in &spec.entries {
        let entry_dir = run_dir.join(format!("entry-{entry:02x}"));
        let expected_offset_dirs = SAMPLE_OFFSETS
            .iter()
            .map(|offset| format!("offset-{offset:04}"))
            .collect::<BTreeSet<_>>();
        ensure!(
            directory_names(&entry_dir)? == expected_offset_dirs,
            "{} entry {:02X} does not have the exact irregular sample set",
            spec.role,
            entry
        );
        let mut frames = Vec::new();
        for &offset in &SAMPLE_OFFSETS {
            let capture = read_capture(&entry_dir.join(format!("offset-{offset:04}")))?;
            if offset == SAMPLE_OFFSETS[0] {
                ensure!(
                    capture.selector_entry() == (expected_root, entry),
                    "{} entry {:02X} starts at selector/entry {:02X}:{:02X}, expected {:02X}:{:02X}",
                    spec.role,
                    entry,
                    capture.selector_entry().0,
                    capture.selector_entry().1,
                    expected_root,
                    entry
                );
            }
            frames.push(capture.state.producer_frame_count);
            accumulate_capture(&mut analysis, &capture);
            if offset == *SAMPLE_OFFSETS.last().expect("sample offsets are nonempty") {
                settled_screenshot_sha1s.insert(sha1_hex(&capture.screenshot));
            }
        }
        validate_frame_sequence(&frames)?;
    }

    analysis.report.sample_count = spec.entries.len() * SAMPLE_OFFSETS.len();
    analysis.report.distinct_screenshot_count = analysis.screenshot_sha1s.len();
    analysis.report.distinct_settled_screenshot_count = settled_screenshot_sha1s.len();
    analysis.report.distinct_nametable_count = analysis.nametable_sha1s.len();
    analysis.report.distinct_oam_count = analysis.oam_sha1s.len();
    analysis.report.distinct_palette_count = analysis.palette_sha1s.len();
    analysis.report.chr_pairs = analysis.chr_pairs.iter().cloned().collect();
    analysis.report.selector_entry_pairs = analysis.selector_entry_pairs.iter().cloned().collect();
    analysis.report.nametable_tile_codes_hex = hex_codes(&analysis.nametable_tile_codes);
    analysis.report.visible_sprite_tile_codes_hex = hex_codes(&analysis.visible_sprite_tile_codes);
    Ok(analysis)
}

pub(super) fn analyze_routing_no_op(root: &Path) -> Result<RoutingNoOpReport> {
    let no_op_dir = root.join("all-routing/no-op-entry-01");
    ensure!(
        directory_names(&no_op_dir)? == BTreeSet::from(["phase-10-after-0601".to_owned()]),
        "routing entry 01 no-op evidence has an unexpected topology"
    );
    let capture = read_capture(&no_op_dir.join("phase-10-after-0601"))?;
    ensure!(
        capture.selector_entry() == (0x41, 0x01),
        "routing no-op selector drifted"
    );
    ensure!(
        capture.phase() == 0x10,
        "routing no-op did not remain in phase 10"
    );
    ensure!(
        !capture.state.background_enabled && !capture.state.sprites_enabled,
        "routing no-op unexpectedly renders a background or sprites"
    );
    Ok(RoutingNoOpReport {
        entry_hex: "01",
        selector_hex: "41",
        phase_hex: "10",
        no_input_frames_before_capture: 601,
        background_enabled: capture.state.background_enabled,
        sprites_enabled: capture.state.sprites_enabled,
        screenshot_sha1: sha1_hex(&capture.screenshot),
        interpretation: "phase 0F advanced to 10 after one frame, then stayed blank for 600 additional no-input frames; this is not a visible epilogue entry",
    })
}

pub(super) fn validate_frame_sequence(frames: &[u64]) -> Result<()> {
    ensure!(
        frames.len() == SAMPLE_OFFSETS.len(),
        "wrong temporal sample count"
    );
    let first = frames[0];
    let actual = frames
        .iter()
        .map(|frame| {
            frame
                .checked_sub(first)
                .ok_or_else(|| anyhow::anyhow!("producer frames are not monotonic"))
        })
        .collect::<Result<Vec<_>>>()?;
    let expected = SAMPLE_OFFSETS
        .iter()
        .map(|offset| u64::from(offset - SAMPLE_OFFSETS[0]))
        .collect::<Vec<_>>();
    ensure!(
        actual == expected,
        "producer frames do not match the irregular sample offsets"
    );
    Ok(())
}

fn validate_entry_directories(run_dir: &Path, spec: &RunSpec) -> Result<()> {
    let mut expected = spec
        .entries
        .iter()
        .map(|(entry, _)| format!("entry-{entry:02x}"))
        .collect::<BTreeSet<_>>();
    expected.extend(
        spec.allowed_extra_directories
            .iter()
            .map(|name| (*name).to_owned()),
    );
    ensure!(
        directory_names(run_dir)? == expected,
        "{} evidence has an unexpected entry set",
        spec.role
    );
    Ok(())
}

fn accumulate_capture(analysis: &mut RunAnalysis, capture: &CaptureFiles) {
    analysis.chr_pairs.insert(capture.state.chr_pair.clone());
    analysis
        .screenshot_sha1s
        .insert(sha1_hex(&capture.screenshot));
    analysis
        .nametable_sha1s
        .insert(sha1_hex(&capture.nametable));
    analysis.oam_sha1s.insert(sha1_hex(&capture.oam));
    analysis.palette_sha1s.insert(sha1_hex(&capture.palette));
    let (selector, entry) = capture.selector_entry();
    analysis
        .selector_entry_pairs
        .insert(format!("{selector:02X}:{entry:02X}"));
    analysis
        .nametable_tile_codes
        .extend(nametable_tile_codes_for(&capture.nametable));
    analysis
        .visible_sprite_tile_codes
        .extend(visible_sprite_tile_codes_for(&capture.oam).0);
}

fn hex_codes(codes: &BTreeSet<u8>) -> Vec<String> {
    codes.iter().map(|code| format!("{code:02X}")).collect()
}
