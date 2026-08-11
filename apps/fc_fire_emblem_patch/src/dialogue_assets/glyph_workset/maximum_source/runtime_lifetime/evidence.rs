use std::{
    collections::BTreeSet,
    fs,
    path::{Component, Path},
};

use anyhow::{Context, Result, ensure};
use serde::Deserialize;

use crate::{rom::EXPECTED_SOURCE_SHA1, sha1_hex};

use super::{
    DISPLAY_LINES_PER_PAGE, OBSERVED_PAGE_COUNT, SAMPLING_FRAME_OFFSETS, TARGET_RECORD_ID,
};

const SCREEN_ROLE: &str = "chapter_7_castle_clear_maximum_dialogue";
const NAMETABLE_MEMORY_SIZE: usize = 2 * 1024;
const PHYSICAL_NAMETABLE_SIZE: usize = 1024;
const TILE_BYTES_PER_NAMETABLE: usize = 30 * 32;
const TARGET_NAMETABLE: usize = 1;
const DIALOGUE_INTERIOR_ROW_START: usize = 15;
const DIALOGUE_INTERIOR_ROW_END_EXCLUSIVE: usize = 25;
const DIALOGUE_INTERIOR_COLUMN_START: usize = 7;
const DIALOGUE_INTERIOR_COLUMN_END_EXCLUSIVE: usize = 25;

#[derive(Debug, Deserialize)]
struct RuntimeManifest {
    format_version: u8,
    screen_role: String,
    target_record_id: String,
    source_sha1: String,
    runtime_binding: RuntimeRouteBinding,
    completed_page_count: usize,
    sampling_frame_offsets: Vec<usize>,
    samples: Vec<RuntimeSample>,
}

#[derive(Debug, Deserialize)]
struct RuntimeRouteBinding {
    chapter_number: u8,
    producer_coordinate: ProducerCoordinate,
    dialogue_selector: String,
    outer_screen_state: String,
    main_state: String,
    dialogue_stage: u8,
    completed_page_state: String,
    exit_effect: ExitEffect,
}

#[derive(Debug, Deserialize)]
struct ProducerCoordinate {
    row: u8,
    column: u8,
}

#[derive(Debug, Deserialize)]
struct ExitEffect {
    outer_screen_state: String,
    main_state: String,
    screen_role: String,
}

#[derive(Debug, Deserialize)]
struct RuntimeSample {
    label: String,
    page_index: usize,
    frame_offset: usize,
    directory: String,
    nametable_sha1: String,
    state_sha1: String,
}

#[derive(Debug)]
pub(crate) struct RuntimeEvidence {
    pub(crate) manifest_sha1: String,
    pub(crate) completed_page_count: usize,
    pub(crate) samples_per_page: usize,
    pub(crate) temporal_sample_count: usize,
    pub(crate) unique_nametable_count: usize,
    pub(crate) screen_codes: BTreeSet<u8>,
}

pub(crate) fn load_runtime_evidence(
    manifest_path: &Path,
    workspace_line_count: usize,
) -> Result<RuntimeEvidence> {
    let manifest_bytes = fs::read(manifest_path).with_context(|| {
        format!(
            "read maximum dialogue runtime evidence {}",
            manifest_path.display()
        )
    })?;
    let manifest: RuntimeManifest = serde_json::from_slice(&manifest_bytes).with_context(|| {
        format!(
            "parse maximum dialogue runtime evidence {}",
            manifest_path.display()
        )
    })?;
    ensure!(
        manifest.format_version == 1,
        "unsupported maximum dialogue runtime evidence format"
    );
    ensure!(
        manifest.screen_role == SCREEN_ROLE,
        "maximum dialogue runtime screen role changed"
    );
    ensure!(
        manifest.target_record_id == TARGET_RECORD_ID,
        "maximum dialogue runtime target changed"
    );
    ensure!(
        manifest.source_sha1 == EXPECTED_SOURCE_SHA1,
        "maximum dialogue runtime source changed"
    );
    ensure!(
        manifest.runtime_binding.chapter_number == 7
            && manifest.runtime_binding.producer_coordinate.row == 27
            && manifest.runtime_binding.producer_coordinate.column == 10
            && manifest.runtime_binding.dialogue_selector == "C0:18"
            && manifest.runtime_binding.outer_screen_state == "0x0C"
            && manifest.runtime_binding.main_state == "0x3C"
            && manifest.runtime_binding.dialogue_stage == 2
            && manifest.runtime_binding.completed_page_state == "0x0E",
        "maximum dialogue runtime producer binding changed"
    );
    ensure!(
        manifest.runtime_binding.exit_effect.outer_screen_state == "0x0D"
            && manifest.runtime_binding.exit_effect.main_state == "0x03"
            && manifest.runtime_binding.exit_effect.screen_role == "next_story_banner",
        "maximum dialogue runtime exit effect changed"
    );
    let expected_page_count = workspace_line_count.div_ceil(DISPLAY_LINES_PER_PAGE);
    ensure!(
        expected_page_count == OBSERVED_PAGE_COUNT
            && manifest.completed_page_count == expected_page_count,
        "maximum dialogue runtime completed-page count changed"
    );
    ensure!(
        manifest.sampling_frame_offsets == SAMPLING_FRAME_OFFSETS,
        "maximum dialogue runtime sampling offsets changed"
    );
    ensure!(
        manifest.samples.len() == expected_page_count * SAMPLING_FRAME_OFFSETS.len(),
        "maximum dialogue runtime sample coverage is incomplete"
    );

    let parent = manifest_path
        .parent()
        .context("maximum dialogue runtime manifest has no parent")?;
    let mut labels = BTreeSet::new();
    let mut sample_keys = BTreeSet::new();
    let mut frame_counts = BTreeSet::new();
    let mut nametable_hashes = BTreeSet::new();
    let mut screen_codes = BTreeSet::new();
    for sample in &manifest.samples {
        ensure!(
            labels.insert(&sample.label),
            "duplicate maximum dialogue runtime sample label"
        );
        ensure!(
            (1..=expected_page_count).contains(&sample.page_index)
                && SAMPLING_FRAME_OFFSETS.contains(&sample.frame_offset),
            "maximum dialogue runtime sample is outside the page/time grid"
        );
        ensure!(
            sample_keys.insert((sample.page_index, sample.frame_offset)),
            "duplicate maximum dialogue runtime page/time sample"
        );
        let expected_directory = format!(
            "page-{:02}/frame-{:04}",
            sample.page_index, sample.frame_offset
        );
        ensure!(
            sample.directory == expected_directory,
            "maximum dialogue runtime sample directory changed"
        );
        let relative = Path::new(&sample.directory);
        ensure!(
            !relative.is_absolute()
                && relative
                    .components()
                    .all(|component| !matches!(component, Component::ParentDir)),
            "maximum dialogue runtime sample path escapes the manifest"
        );
        let directory = parent.join(relative);
        let nametable = read_bound_file(
            &directory.join("nametable.bin"),
            &sample.nametable_sha1,
            "nametable",
        )?;
        ensure!(
            nametable.len() == NAMETABLE_MEMORY_SIZE,
            "maximum dialogue runtime nametable must contain exactly 2 KiB"
        );
        let state = read_bound_file(&directory.join("state.json"), &sample.state_sha1, "state")?;
        let state: serde_json::Value = serde_json::from_slice(&state)
            .context("parse maximum dialogue runtime state sample")?;
        ensure!(
            state
                .get("ppu.control.backgroundPatternAddr")
                .and_then(serde_json::Value::as_u64)
                == Some(0x1000)
                && state
                    .get("ppu.control.spritePatternAddr")
                    .and_then(serde_json::Value::as_u64)
                    == Some(0),
            "maximum dialogue runtime pattern-table split changed"
        );
        let frame_count = state
            .get("frameCount")
            .and_then(serde_json::Value::as_u64)
            .context("maximum dialogue runtime state has no frame count")?;
        ensure!(
            frame_counts.insert(frame_count),
            "maximum dialogue runtime evidence repeats one emulated frame"
        );
        nametable_hashes.insert(sample.nametable_sha1.clone());
        collect_preserved_screen_codes(&nametable, &mut screen_codes);
    }
    ensure!(
        sample_keys.len() == expected_page_count * SAMPLING_FRAME_OFFSETS.len(),
        "maximum dialogue runtime page/time grid is incomplete"
    );

    Ok(RuntimeEvidence {
        manifest_sha1: sha1_hex(&manifest_bytes),
        completed_page_count: expected_page_count,
        samples_per_page: SAMPLING_FRAME_OFFSETS.len(),
        temporal_sample_count: manifest.samples.len(),
        unique_nametable_count: nametable_hashes.len(),
        screen_codes,
    })
}

fn read_bound_file(path: &Path, expected_sha1: &str, role: &str) -> Result<Vec<u8>> {
    let bytes = fs::read(path).with_context(|| {
        format!(
            "read maximum dialogue runtime {role} sample {}",
            path.display()
        )
    })?;
    ensure!(
        sha1_hex(&bytes) == expected_sha1,
        "maximum dialogue runtime {role} sample SHA-1 changed"
    );
    Ok(bytes)
}

fn collect_preserved_screen_codes(nametable: &[u8], codes: &mut BTreeSet<u8>) {
    for physical_table in 0..2 {
        let table_start = physical_table * PHYSICAL_NAMETABLE_SIZE;
        for tile_index in 0..TILE_BYTES_PER_NAMETABLE {
            let row = tile_index / 32;
            let column = tile_index % 32;
            let is_target_text_cell = physical_table == TARGET_NAMETABLE
                && (DIALOGUE_INTERIOR_ROW_START..DIALOGUE_INTERIOR_ROW_END_EXCLUSIVE)
                    .contains(&row)
                && (DIALOGUE_INTERIOR_COLUMN_START..DIALOGUE_INTERIOR_COLUMN_END_EXCLUSIVE)
                    .contains(&column);
            if !is_target_text_cell {
                codes.insert(nametable[table_start + tile_index]);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn dialogue_interior_is_the_only_unpreserved_nametable_region() {
        let mut nametable = vec![0x44; NAMETABLE_MEMORY_SIZE];
        nametable[TARGET_NAMETABLE * PHYSICAL_NAMETABLE_SIZE + 15 * 32 + 7] = 0x33;
        nametable[TARGET_NAMETABLE * PHYSICAL_NAMETABLE_SIZE + 14 * 32 + 7] = 0x22;
        let mut codes = BTreeSet::new();

        collect_preserved_screen_codes(&nametable, &mut codes);

        assert_eq!(codes, BTreeSet::from([0x22, 0x44]));
    }
}
