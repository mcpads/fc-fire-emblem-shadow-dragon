use std::{
    collections::{BTreeMap, BTreeSet},
    fs,
    path::{Component, Path},
};

use anyhow::{Context, Result, ensure};
use serde::Deserialize;

use crate::{rom::EXPECTED_SOURCE_SHA1, sha1_hex};

const INTERNAL_RAM_SIZE: usize = 2 * 1024;
const NAMETABLE_MEMORY_SIZE: usize = 2 * 1024;
const PHYSICAL_NAMETABLE_SIZE: usize = 1024;
const TILE_BYTES_PER_NAMETABLE: usize = 30 * 32;
const MINIMUM_TEMPORAL_SAMPLE_COUNT: usize = 3;

#[derive(Debug, Deserialize)]
struct Manifest {
    format_version: u8,
    source_sha1: String,
    samples: Vec<Sample>,
}

#[derive(Debug, Deserialize)]
struct Sample {
    label: String,
    screen_role: String,
    directory: String,
    iram_sha1: String,
    nametable_sha1: String,
    state_sha1: String,
}

pub(super) struct UnitNameScreenEvidence {
    pub(super) manifest_sha1: String,
    pub(super) temporal_sample_count: usize,
    pub(super) unique_nametable_count: usize,
    pub(super) visible_codes: BTreeSet<u8>,
}

pub(super) fn load_unit_name_screen_evidence(
    manifest_path: &Path,
) -> Result<UnitNameScreenEvidence> {
    let manifest_bytes = fs::read(manifest_path)
        .with_context(|| format!("read unit-name screen evidence {}", manifest_path.display()))?;
    let manifest: Manifest = serde_json::from_slice(&manifest_bytes).with_context(|| {
        format!(
            "parse unit-name screen evidence {}",
            manifest_path.display()
        )
    })?;
    ensure!(
        manifest.format_version == 1,
        "unsupported unit-name evidence format"
    );
    ensure!(
        manifest.source_sha1 == EXPECTED_SOURCE_SHA1,
        "unit-name evidence source changed"
    );
    ensure!(
        manifest.samples.len() >= MINIMUM_TEMPORAL_SAMPLE_COUNT,
        "unit-name evidence needs at least {MINIMUM_TEMPORAL_SAMPLE_COUNT} temporal samples"
    );

    let parent = manifest_path
        .parent()
        .context("unit-name evidence has no parent directory")?;
    let mut labels = BTreeSet::new();
    let mut roles = BTreeMap::<String, usize>::new();
    let mut frames = BTreeSet::new();
    let mut nametable_hashes = BTreeSet::new();
    let mut visible_codes = BTreeSet::new();
    for sample in &manifest.samples {
        ensure!(
            labels.insert(&sample.label),
            "duplicate unit-name evidence label"
        );
        ensure!(
            ["unit_summary", "unit_status"].contains(&sample.screen_role.as_str()),
            "unsupported unit-name evidence screen role"
        );
        *roles.entry(sample.screen_role.clone()).or_default() += 1;
        let relative = Path::new(&sample.directory);
        ensure!(
            !relative.is_absolute()
                && relative
                    .components()
                    .all(|component| !matches!(component, Component::ParentDir)),
            "unit-name evidence sample paths must stay below the manifest"
        );
        let directory = parent.join(relative);
        let iram = read_bound_file(&directory.join("iram.bin"), &sample.iram_sha1, "IRAM")?;
        ensure!(
            iram.len() == INTERNAL_RAM_SIZE,
            "unit-name IRAM sample must be 2 KiB"
        );
        match sample.screen_role.as_str() {
            "unit_summary" => ensure!(
                iram[0x59..0x5D] == [0x1A, 0x1A, 0x00, 0x18],
                "unit-summary evidence CHR pair changed"
            ),
            "unit_status" => ensure!(
                iram[0x59..0x5D] == [0x13, 0x13, 0x00, 0x18],
                "unit-status evidence CHR pair changed"
            ),
            _ => unreachable!(),
        }
        let nametable = read_bound_file(
            &directory.join("nametable.bin"),
            &sample.nametable_sha1,
            "nametable",
        )?;
        ensure!(
            nametable.len() == NAMETABLE_MEMORY_SIZE,
            "unit-name nametable sample must be 2 KiB"
        );
        collect_visible_codes(&nametable, &mut visible_codes);
        nametable_hashes.insert(sample.nametable_sha1.clone());

        let state = read_bound_file(&directory.join("state.json"), &sample.state_sha1, "state")?;
        let state: serde_json::Value =
            serde_json::from_slice(&state).context("parse unit-name state sample")?;
        ensure!(
            state
                .get("ppu.control.backgroundPatternAddr")
                .and_then(serde_json::Value::as_u64)
                == Some(0x1000),
            "unit-name sample does not use the right background pattern table"
        );
        ensure!(
            state
                .get("mapper.registers2")
                .and_then(serde_json::Value::as_u64)
                == Some(8),
            "unit-name source sample does not use the original right-FD font page"
        );
        let frame = state
            .get("frameCount")
            .and_then(serde_json::Value::as_u64)
            .context("unit-name state sample has no frame count")?;
        ensure!(frames.insert(frame), "unit-name evidence repeats one frame");
    }
    ensure!(
        roles.contains_key("unit_summary") && roles.contains_key("unit_status"),
        "unit-name evidence must cover summary and status screens"
    );
    let sorted_frames = frames.into_iter().collect::<Vec<_>>();
    let deltas = sorted_frames
        .windows(2)
        .map(|pair| pair[1] - pair[0])
        .collect::<BTreeSet<_>>();
    ensure!(
        deltas.len() > 1,
        "unit-name temporal samples use one fixed frame step"
    );

    Ok(UnitNameScreenEvidence {
        manifest_sha1: sha1_hex(&manifest_bytes),
        temporal_sample_count: manifest.samples.len(),
        unique_nametable_count: nametable_hashes.len(),
        visible_codes,
    })
}

fn read_bound_file(path: &Path, expected_sha1: &str, role: &str) -> Result<Vec<u8>> {
    let bytes =
        fs::read(path).with_context(|| format!("read unit-name {role} {}", path.display()))?;
    ensure!(
        sha1_hex(&bytes) == expected_sha1,
        "unit-name {role} SHA-1 changed"
    );
    Ok(bytes)
}

fn collect_visible_codes(nametable: &[u8], codes: &mut BTreeSet<u8>) {
    for physical_table in 0..2 {
        let start = physical_table * PHYSICAL_NAMETABLE_SIZE;
        codes.extend(
            nametable[start..start + TILE_BYTES_PER_NAMETABLE]
                .iter()
                .copied(),
        );
    }
}
