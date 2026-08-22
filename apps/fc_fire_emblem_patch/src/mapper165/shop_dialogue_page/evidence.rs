use std::{
    collections::BTreeSet,
    fs,
    path::{Component, Path},
};

use anyhow::{Context, Result, ensure};
use serde::Deserialize;

use crate::{
    dialogue_runtime_state::MAIN_DIALOGUE_RUNTIME_STATE, rom::EXPECTED_SOURCE_SHA1, sha1_hex,
    shop_flow::WEAPON_SHOP_DIALOGUE_LIFETIME_RECORD_IDS,
};

use super::SCREEN_ROLE;

const NAMETABLE_MEMORY_SIZE: usize = 2 * 1024;
const INTERNAL_RAM_SIZE: usize = 2 * 1024;
const PRG_RAM_SIZE: usize = 8 * 1024;
const PHYSICAL_NAMETABLE_SIZE: usize = 1024;
const TILE_BYTES_PER_NAMETABLE: usize = 30 * 32;
const TARGET_NAMETABLE: usize = 1;
const DIALOGUE_INTERIOR_ROW_START: usize = 15;
const DIALOGUE_INTERIOR_ROW_END_EXCLUSIVE: usize = 25;
const DIALOGUE_INTERIOR_COLUMN_START: usize = 7;
const DIALOGUE_INTERIOR_COLUMN_END_EXCLUSIVE: usize = 25;
const SHOP_OUTER_STATE_ADDRESS: usize =
    MAIN_DIALOGUE_RUNTIME_STATE.map_dialogue_outer_state_address as usize;
const SELECTED_FACILITY_PRG_RAM_OFFSET: usize = 0x77D0 - 0x6000;
const DIALOGUE_BANK_PRG_RAM_OFFSET: usize = 0x77F2 - 0x6000;
const DIALOGUE_DIRECTORY_PRG_RAM_OFFSET: usize =
    MAIN_DIALOGUE_RUNTIME_STATE.directory_selector_address as usize - 0x6000;

#[derive(Debug, Deserialize)]
struct ShopScreenEvidenceManifest {
    format_version: u8,
    screen_role: String,
    target_record_ids: Vec<String>,
    source_sha1: String,
    samples: Vec<ShopScreenEvidenceSample>,
}

#[derive(Debug, Deserialize)]
struct ShopScreenEvidenceSample {
    label: String,
    screen_role: String,
    directory: String,
    iram_sha1: String,
    prgram_sha1: String,
    nametable_sha1: String,
    state_sha1: String,
}

pub(super) struct ShopScreenEvidence {
    pub(super) manifest_sha1: String,
    pub(super) sample_count: usize,
    pub(super) unique_nametable_count: usize,
    pub(super) screen_codes: BTreeSet<u8>,
}

pub(super) fn load_shop_screen_codes(manifest_path: &Path) -> Result<ShopScreenEvidence> {
    let manifest_bytes = fs::read(manifest_path).with_context(|| {
        format!(
            "read weapon-shop screen evidence {}",
            manifest_path.display()
        )
    })?;
    let manifest: ShopScreenEvidenceManifest = serde_json::from_slice(&manifest_bytes)
        .with_context(|| {
            format!(
                "parse weapon-shop screen evidence {}",
                manifest_path.display()
            )
        })?;
    ensure!(
        manifest.format_version == 1,
        "unsupported weapon-shop screen evidence format"
    );
    ensure!(
        manifest.screen_role == SCREEN_ROLE,
        "weapon-shop screen evidence role changed"
    );
    ensure!(
        manifest.target_record_ids
            == WEAPON_SHOP_DIALOGUE_LIFETIME_RECORD_IDS
                .map(str::to_owned)
                .to_vec(),
        "weapon-shop screen evidence record bundle changed"
    );
    ensure!(
        manifest.source_sha1 == EXPECTED_SOURCE_SHA1,
        "weapon-shop screen evidence source binding changed"
    );
    ensure!(
        manifest.samples.len() >= 3,
        "weapon-shop screen evidence needs at least three distinct surfaces"
    );

    let parent = manifest_path
        .parent()
        .context("weapon-shop screen evidence has no parent directory")?;
    let mut labels = BTreeSet::new();
    let mut roles = BTreeSet::new();
    let mut frame_counts = BTreeSet::new();
    let mut nametable_hashes = BTreeSet::new();
    let mut screen_codes = BTreeSet::new();
    for sample in &manifest.samples {
        ensure!(
            labels.insert(&sample.label),
            "duplicate weapon-shop sample label"
        );
        ensure!(
            roles.insert(sample.screen_role.as_str()),
            "duplicate weapon-shop sample role"
        );
        let relative = Path::new(&sample.directory);
        ensure!(
            !relative.is_absolute()
                && relative
                    .components()
                    .all(|component| !matches!(component, Component::ParentDir)),
            "weapon-shop sample paths must stay below the manifest"
        );
        let directory = parent.join(relative);
        let iram = read_bound_file(&directory.join("iram.bin"), &sample.iram_sha1, "IRAM")?;
        let prgram = read_bound_file(
            &directory.join("prgram.bin"),
            &sample.prgram_sha1,
            "PRG RAM",
        )?;
        let nametable = read_bound_file(
            &directory.join("nametable.bin"),
            &sample.nametable_sha1,
            "nametable",
        )?;
        let state = read_bound_file(&directory.join("state.json"), &sample.state_sha1, "state")?;
        ensure!(
            iram.len() == INTERNAL_RAM_SIZE,
            "weapon-shop IRAM size changed"
        );
        ensure!(
            prgram.len() == PRG_RAM_SIZE,
            "weapon-shop PRG RAM size changed"
        );
        ensure!(
            nametable.len() == NAMETABLE_MEMORY_SIZE,
            "weapon-shop nametable size changed"
        );
        validate_shop_state(sample, &iram, &prgram)?;
        let state: serde_json::Value =
            serde_json::from_slice(&state).context("parse weapon-shop screen state")?;
        validate_video_state(&state)?;
        let frame_count = state
            .get("frameCount")
            .and_then(|value| value.as_u64())
            .context("weapon-shop sample has no frame count")?;
        ensure!(
            frame_counts.insert(frame_count),
            "weapon-shop evidence repeats one emulated frame"
        );
        nametable_hashes.insert(sample.nametable_sha1.clone());
        collect_preserved_screen_codes(&nametable, &mut screen_codes);
    }
    ensure!(
        roles
            == BTreeSet::from([
                "weapon_shop_item_list",
                "weapon_shop_purchase_question_handoff",
                "weapon_shop_purchase_confirmation",
            ]),
        "weapon-shop evidence lost a critical surface"
    );
    Ok(ShopScreenEvidence {
        manifest_sha1: sha1_hex(&manifest_bytes),
        sample_count: manifest.samples.len(),
        unique_nametable_count: nametable_hashes.len(),
        screen_codes,
    })
}

fn validate_shop_state(
    sample: &ShopScreenEvidenceSample,
    iram: &[u8],
    prgram: &[u8],
) -> Result<()> {
    let expected_outer_state = match sample.screen_role.as_str() {
        "weapon_shop_item_list" => 4,
        "weapon_shop_purchase_question_handoff" => 5,
        "weapon_shop_purchase_confirmation" => 7,
        _ => anyhow::bail!("unknown weapon-shop evidence surface"),
    };
    ensure!(
        iram[SHOP_OUTER_STATE_ADDRESS] == expected_outer_state,
        "weapon-shop sample outer state changed"
    );
    ensure!(
        iram[0x59..=0x5C] == [0x1E, 0x1E, 0x00, 0x18],
        "weapon-shop sample CHR state changed"
    );
    ensure!(
        prgram[SELECTED_FACILITY_PRG_RAM_OFFSET] == 1,
        "weapon-shop sample facility changed"
    );
    ensure!(
        prgram[DIALOGUE_BANK_PRG_RAM_OFFSET] == 0x0B,
        "weapon-shop sample dialogue bank changed"
    );
    ensure!(
        prgram[DIALOGUE_DIRECTORY_PRG_RAM_OFFSET] == 0xB1,
        "weapon-shop sample dialogue directory changed"
    );
    Ok(())
}

fn validate_video_state(state: &serde_json::Value) -> Result<()> {
    ensure!(
        state
            .get("ppu.control.backgroundPatternAddr")
            .and_then(|value| value.as_u64())
            == Some(0x1000),
        "weapon-shop sample does not use the right background table"
    );
    ensure!(
        state
            .get("ppu.control.spritePatternAddr")
            .and_then(|value| value.as_u64())
            == Some(0),
        "weapon-shop sample does not keep sprites on the left table"
    );
    for (key, expected) in [
        ("mapper.leftChrPage[0]", 0x1E),
        ("mapper.leftChrPage[1]", 0x1E),
        ("mapper.rightChrPage[0]", 0x00),
        ("mapper.rightChrPage[1]", 0x18),
    ] {
        ensure!(
            state.get(key).and_then(|value| value.as_u64()) == Some(expected),
            "weapon-shop sample mapper state changed at {key}"
        );
    }
    Ok(())
}

fn read_bound_file(path: &Path, expected_sha1: &str, role: &str) -> Result<Vec<u8>> {
    let bytes = fs::read(path)
        .with_context(|| format!("read weapon-shop {role} sample {}", path.display()))?;
    ensure!(
        sha1_hex(&bytes) == expected_sha1,
        "weapon-shop {role} sample SHA-1 changed"
    );
    Ok(bytes)
}

fn collect_preserved_screen_codes(nametable: &[u8], codes: &mut BTreeSet<u8>) {
    for physical_table in 0..2 {
        let table_start = physical_table * PHYSICAL_NAMETABLE_SIZE;
        for tile_index in 0..TILE_BYTES_PER_NAMETABLE {
            let row = tile_index / 32;
            let column = tile_index % 32;
            let is_dialogue_cell = physical_table == TARGET_NAMETABLE
                && (DIALOGUE_INTERIOR_ROW_START..DIALOGUE_INTERIOR_ROW_END_EXCLUSIVE)
                    .contains(&row)
                && (DIALOGUE_INTERIOR_COLUMN_START..DIALOGUE_INTERIOR_COLUMN_END_EXCLUSIVE)
                    .contains(&column);
            if !is_dialogue_cell {
                codes.insert(nametable[table_start + tile_index]);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn dialogue_window_is_the_only_unpreserved_region() {
        let mut nametable = vec![0x44; NAMETABLE_MEMORY_SIZE];
        nametable[TARGET_NAMETABLE * PHYSICAL_NAMETABLE_SIZE + 15 * 32 + 7] = 0x33;
        nametable[TARGET_NAMETABLE * PHYSICAL_NAMETABLE_SIZE + 14 * 32 + 7] = 0x22;
        let mut codes = BTreeSet::new();

        collect_preserved_screen_codes(&nametable, &mut codes);

        assert_eq!(codes, BTreeSet::from([0x22, 0x44]));
    }
}
