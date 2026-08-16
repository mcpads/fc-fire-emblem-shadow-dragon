use std::{
    collections::BTreeSet,
    fs,
    path::{Component, Path},
};

use anyhow::{Context, Result, ensure};
use serde::Deserialize;

use crate::{font_slots::active_hangul_codes, rom::EXPECTED_SOURCE_SHA1, sha1_hex};

use super::options_page::{
    OPTIONS_COMPOSITE_STATE, OPTIONS_COMPOSITE_STATE_ADDRESS, OPTIONS_MAIN_STATE,
    OPTIONS_MAIN_STATE_ADDRESS,
};

const INTERNAL_RAM_SIZE: usize = 2 * 1024;
const NAMETABLE_MEMORY_SIZE: usize = 2 * 1024;
const PHYSICAL_NAMETABLE_SIZE: usize = 1024;
const TILE_BYTES_PER_NAMETABLE: usize = 30 * 32;
const ROW_STATE_ADDRESS: usize = 0x0034;
const SCREEN_KIND_ADDRESS: usize = 0x0052;
const CHR_PAIR_ADDRESS: usize = 0x0059;
const EXPECTED_CHR_PAIR: [u8; 4] = [0x1A, 0x1A, 0x00, 0x15];

#[derive(Debug, Deserialize)]
struct OptionsLifetimeManifest {
    format_version: u8,
    screen_role: String,
    source_sha1: String,
    samples: Vec<OptionsLifetimeSample>,
}

#[derive(Debug, Deserialize)]
struct OptionsLifetimeSample {
    label: String,
    directory: String,
    expected_row_state: u8,
    iram_sha1: String,
    nametable_sha1: String,
    state_sha1: String,
}

pub(super) struct OptionsLifetimeEvidence {
    pub(super) manifest_sha1: String,
    pub(super) sample_count: usize,
    pub(super) unique_nametable_count: usize,
    pub(super) observed_row_states: Vec<u8>,
    pub(super) target_glyph_count: usize,
    pub(super) visible_active_code_count: usize,
    pub(super) preserved_active_code_count: usize,
    pub(super) total_slot_demand: usize,
}

pub(super) fn inspect_options_lifetime(
    manifest_path: &Path,
    target_codes: &BTreeSet<u8>,
) -> Result<OptionsLifetimeEvidence> {
    let manifest_bytes = fs::read(manifest_path)
        .with_context(|| format!("read options lifetime evidence {}", manifest_path.display()))?;
    let manifest: OptionsLifetimeManifest =
        serde_json::from_slice(&manifest_bytes).with_context(|| {
            format!(
                "parse options lifetime evidence {}",
                manifest_path.display()
            )
        })?;
    ensure!(
        manifest.format_version == 1
            && manifest.screen_role == "options"
            && manifest.source_sha1 == EXPECTED_SOURCE_SHA1,
        "options lifetime evidence contract changed"
    );
    ensure!(
        manifest.samples.len() >= 2,
        "options lifetime needs samples from both Hangul page rows"
    );

    let parent = manifest_path
        .parent()
        .context("options lifetime evidence has no parent directory")?;
    let mut labels = BTreeSet::new();
    let mut frames = BTreeSet::new();
    let mut nametable_hashes = BTreeSet::new();
    let mut observed_row_states = BTreeSet::new();
    let mut visible_codes = BTreeSet::new();
    for sample in &manifest.samples {
        ensure!(
            labels.insert(&sample.label),
            "duplicate options evidence label"
        );
        let relative = Path::new(&sample.directory);
        ensure!(
            !relative.is_absolute()
                && relative
                    .components()
                    .all(|component| !matches!(component, Component::ParentDir)),
            "options evidence sample paths must stay below the manifest"
        );
        let directory = parent.join(relative);
        let iram = read_bound_file(&directory.join("iram.bin"), &sample.iram_sha1, "IRAM")?;
        ensure!(
            iram.len() == INTERNAL_RAM_SIZE
                && iram[ROW_STATE_ADDRESS] == sample.expected_row_state
                && iram[SCREEN_KIND_ADDRESS] == 0
                && iram[usize::from(OPTIONS_COMPOSITE_STATE_ADDRESS)] == OPTIONS_COMPOSITE_STATE
                && iram[usize::from(OPTIONS_MAIN_STATE_ADDRESS)] == OPTIONS_MAIN_STATE
                && iram[CHR_PAIR_ADDRESS..CHR_PAIR_ADDRESS + EXPECTED_CHR_PAIR.len()]
                    == EXPECTED_CHR_PAIR,
            "options IRAM sample no longer matches its exact screen lifetime"
        );
        observed_row_states.insert(sample.expected_row_state);

        let nametable = read_bound_file(
            &directory.join("nametable.bin"),
            &sample.nametable_sha1,
            "nametable",
        )?;
        ensure!(
            nametable.len() == NAMETABLE_MEMORY_SIZE,
            "options nametable sample must contain exactly 2 KiB"
        );
        nametable_hashes.insert(sample.nametable_sha1.clone());
        for physical_table in 0..2 {
            let start = physical_table * PHYSICAL_NAMETABLE_SIZE;
            visible_codes.extend(
                nametable[start..start + TILE_BYTES_PER_NAMETABLE]
                    .iter()
                    .copied(),
            );
        }

        let state = read_bound_file(&directory.join("state.json"), &sample.state_sha1, "state")?;
        let state: serde_json::Value =
            serde_json::from_slice(&state).context("parse options screen state sample")?;
        ensure!(
            state
                .get("ppu.control.backgroundPatternAddr")
                .and_then(serde_json::Value::as_u64)
                == Some(0x1000)
                && state
                    .get("ppu.control.spritePatternAddr")
                    .and_then(serde_json::Value::as_u64)
                    == Some(0)
                && state
                    .get("mapper.registers2")
                    .and_then(serde_json::Value::as_u64)
                    == Some(8),
            "options state sample no longer uses the original right-FD font page"
        );
        let frame = state
            .get("frameCount")
            .and_then(serde_json::Value::as_u64)
            .context("options state sample has no frame count")?;
        ensure!(frames.insert(frame), "options evidence repeats one frame");
    }
    ensure!(
        observed_row_states.contains(&0x20) && observed_row_states.contains(&0x30),
        "options evidence no longer covers both Hangul page selections"
    );

    let active_codes = active_hangul_codes().into_iter().collect::<BTreeSet<_>>();
    let visible_active_codes = visible_codes
        .intersection(&active_codes)
        .copied()
        .collect::<BTreeSet<_>>();
    ensure!(
        target_codes.is_subset(&visible_active_codes),
        "options target codes are not all present on the frozen screen"
    );
    let preserved_active_codes = visible_active_codes
        .difference(target_codes)
        .copied()
        .collect::<BTreeSet<_>>();
    let target_glyph_count = target_codes.len();
    let total_slot_demand = target_glyph_count
        .checked_add(preserved_active_codes.len())
        .context("options lifetime slot demand overflow")?;
    ensure!(
        total_slot_demand == visible_active_codes.len(),
        "options target and preserved code sets no longer partition the visible active codes"
    );

    Ok(OptionsLifetimeEvidence {
        manifest_sha1: sha1_hex(&manifest_bytes),
        sample_count: manifest.samples.len(),
        unique_nametable_count: nametable_hashes.len(),
        observed_row_states: observed_row_states.into_iter().collect(),
        target_glyph_count,
        visible_active_code_count: visible_active_codes.len(),
        preserved_active_code_count: preserved_active_codes.len(),
        total_slot_demand,
    })
}

fn read_bound_file(path: &Path, expected_sha1: &str, role: &str) -> Result<Vec<u8>> {
    let bytes = fs::read(path)
        .with_context(|| format!("read options lifetime {role} {}", path.display()))?;
    ensure!(
        sha1_hex(&bytes) == expected_sha1,
        "options lifetime {role} SHA-1 changed"
    );
    Ok(bytes)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rejects_parent_directory_sample_paths() {
        let relative = Path::new("../outside");
        assert!(
            relative
                .components()
                .any(|component| matches!(component, Component::ParentDir))
        );
    }
}
