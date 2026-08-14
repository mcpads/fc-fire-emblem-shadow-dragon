use std::{
    collections::{BTreeMap, BTreeSet},
    fs,
    path::{Component, Path},
};

use anyhow::{Context, Result, ensure};
use serde::Deserialize;

use crate::{
    font_slots::{FONT_PAGE_SIZE, active_hangul_codes},
    rom::{EXPECTED_SOURCE_SHA1, Rom},
    rp2a03::{Instruction, assemble_at},
    sha1_hex,
};

use super::{
    FIRST_EXTENSION_CHR_PAGE, SELECT_RIGHT_FD_CHR_BANK_FOR_PAIR_ADDRESS,
    dialogue_probe_font::{assign_glyph_codes_excluding, build_font_page},
    encode_chr_page_register,
};

pub(super) const SCREEN_ROLE: &str = "chapter_1_intro_dialogue";
pub(super) const PHYSICAL_CHR_PAGE: u8 = FIRST_EXTENSION_CHR_PAGE;
pub(super) const OUTPUT_CHR_BANK_COUNT: u8 = 18;
pub(super) const CENTRAL_RIGHT_FD_SELECTOR_CALL_ADDRESS: u16 = 0xC9C2;
pub(super) const PAGE_ROUTINE_ADDRESS: u16 = 0xFB20;
pub(super) const PAGE_ROUTINE_END: u16 = 0xFB68;

const SOURCE_FONT_PHYSICAL_PAGE: usize = 2;
const EXTENSION_PAGE_COUNT: usize = 2;
const NAMETABLE_MEMORY_SIZE: usize = 2 * 1024;
const PHYSICAL_NAMETABLE_SIZE: usize = 1024;
const TILE_BYTES_PER_NAMETABLE: usize = 30 * 32;
const TARGET_NAMETABLE: usize = 1;
const DIALOGUE_INTERIOR_ROW_START: usize = 15;
const DIALOGUE_INTERIOR_ROW_END_EXCLUSIVE: usize = 25;
const DIALOGUE_INTERIOR_COLUMN_START: usize = 7;
const DIALOGUE_INTERIOR_COLUMN_END_EXCLUSIVE: usize = 25;
const MINIMUM_TEMPORAL_SAMPLE_COUNT: usize = 3;

#[derive(Debug, Deserialize)]
struct ScreenEvidenceManifest {
    format_version: u8,
    screen_role: String,
    target_record_id: String,
    source_sha1: String,
    samples: Vec<ScreenEvidenceSample>,
}

#[derive(Debug, Deserialize)]
struct ScreenEvidenceSample {
    label: String,
    directory: String,
    nametable_sha1: String,
    state_sha1: String,
}

pub(super) struct DialogueLifetimePagePlan {
    pub(super) assignments: BTreeMap<char, u8>,
    pub(super) page_pack: Vec<u8>,
    pub(super) manifest_sha1: String,
    pub(super) temporal_sample_count: usize,
    pub(super) unique_nametable_count: usize,
    pub(super) preserved_screen_active_code_count: usize,
    pub(super) preserved_source_active_code_count: usize,
    pub(super) preserved_active_code_count: usize,
    pub(super) page_sha1: String,
    pub(super) physical_chr_page: u8,
    pub(super) mapper_register: u8,
}

pub(super) fn plan_dialogue_lifetime_page(
    parity_rom: &Rom,
    manifest_path: &Path,
    screen_role: &str,
    target_record_id: &str,
    glyphs: &BTreeSet<char>,
    preserved_source_codes: &BTreeSet<u8>,
    physical_chr_page: u8,
) -> Result<DialogueLifetimePagePlan> {
    let (manifest_sha1, temporal_sample_count, unique_nametable_count, screen_codes) =
        load_screen_codes(manifest_path, screen_role, target_record_id)?;
    let active_codes = active_hangul_codes().into_iter().collect::<BTreeSet<_>>();
    let preserved_screen_active_codes = screen_codes
        .intersection(&active_codes)
        .copied()
        .collect::<BTreeSet<_>>();
    let preserved_source_active_codes = preserved_source_codes
        .intersection(&active_codes)
        .copied()
        .collect::<BTreeSet<_>>();
    let preserved_active_codes = preserved_screen_active_codes
        .union(&preserved_source_active_codes)
        .copied()
        .collect::<BTreeSet<_>>();
    let assignments = assign_glyph_codes_excluding(glyphs, &preserved_active_codes)?;

    ensure!(
        parity_rom.chr().len().is_multiple_of(FONT_PAGE_SIZE),
        "mapper 165 parity CHR is not 4 KiB page aligned"
    );
    ensure!(
        parity_rom.chr().len() / FONT_PAGE_SIZE == usize::from(physical_chr_page),
        "dialogue extension physical CHR page does not follow the cumulative base"
    );
    let source_start = SOURCE_FONT_PHYSICAL_PAGE * FONT_PAGE_SIZE;
    let source_end = source_start + EXTENSION_PAGE_COUNT * FONT_PAGE_SIZE;
    let source_pair = parity_rom
        .chr()
        .get(source_start..source_end)
        .context("dialogue source font page pair is outside parity CHR")?;
    let mut page_pack = build_font_page(&source_pair[..FONT_PAGE_SIZE], &assignments)?;
    page_pack.extend_from_slice(&source_pair[FONT_PAGE_SIZE..]);
    let mapper_register = encode_chr_page_register(physical_chr_page)?;

    Ok(DialogueLifetimePagePlan {
        assignments,
        page_sha1: sha1_hex(&page_pack[..FONT_PAGE_SIZE]),
        page_pack,
        manifest_sha1,
        temporal_sample_count,
        unique_nametable_count,
        preserved_screen_active_code_count: preserved_screen_active_codes.len(),
        preserved_source_active_code_count: preserved_source_active_codes.len(),
        preserved_active_code_count: preserved_active_codes.len(),
        physical_chr_page,
        mapper_register,
    })
}

fn load_screen_codes(
    manifest_path: &Path,
    screen_role: &str,
    target_record_id: &str,
) -> Result<(String, usize, usize, BTreeSet<u8>)> {
    let manifest_bytes = fs::read(manifest_path)
        .with_context(|| format!("read dialogue screen evidence {}", manifest_path.display()))?;
    let manifest: ScreenEvidenceManifest = serde_json::from_slice(&manifest_bytes)
        .with_context(|| format!("parse dialogue screen evidence {}", manifest_path.display()))?;
    ensure!(
        manifest.format_version == 1,
        "unsupported dialogue screen evidence format"
    );
    ensure!(
        manifest.screen_role == screen_role,
        "dialogue screen evidence role changed"
    );
    ensure!(
        manifest.target_record_id == target_record_id,
        "dialogue screen evidence targets a different record"
    );
    ensure!(
        manifest.source_sha1 == EXPECTED_SOURCE_SHA1,
        "dialogue screen evidence source binding changed"
    );
    ensure!(
        manifest.samples.len() >= MINIMUM_TEMPORAL_SAMPLE_COUNT,
        "dialogue screen evidence needs at least {MINIMUM_TEMPORAL_SAMPLE_COUNT} temporal samples"
    );

    let parent = manifest_path
        .parent()
        .context("dialogue screen evidence has no parent directory")?;
    let mut labels = BTreeSet::new();
    let mut frame_counts = BTreeSet::new();
    let mut nametable_hashes = BTreeSet::new();
    let mut screen_codes = BTreeSet::new();
    for sample in &manifest.samples {
        ensure!(
            labels.insert(&sample.label),
            "duplicate dialogue screen evidence label"
        );
        let relative = Path::new(&sample.directory);
        ensure!(
            !relative.is_absolute()
                && relative
                    .components()
                    .all(|component| !matches!(component, Component::ParentDir)),
            "dialogue screen evidence sample paths must stay below the manifest"
        );
        let directory = parent.join(relative);
        let nametable = read_bound_file(
            &directory.join("nametable.bin"),
            &sample.nametable_sha1,
            "nametable",
        )?;
        ensure!(
            nametable.len() == NAMETABLE_MEMORY_SIZE,
            "dialogue nametable sample must contain exactly 2 KiB"
        );
        let state = read_bound_file(&directory.join("state.json"), &sample.state_sha1, "state")?;
        let state: serde_json::Value =
            serde_json::from_slice(&state).context("parse dialogue screen state sample")?;
        ensure!(
            state
                .get("ppu.control.backgroundPatternAddr")
                .and_then(|value| value.as_u64())
                == Some(0x1000),
            "dialogue screen sample does not use the right background pattern table"
        );
        ensure!(
            state
                .get("ppu.control.spritePatternAddr")
                .and_then(|value| value.as_u64())
                == Some(0),
            "dialogue screen sample does not keep sprites on the left pattern table"
        );
        let frame_count = state
            .get("frameCount")
            .and_then(|value| value.as_u64())
            .context("dialogue screen sample has no frame count")?;
        ensure!(
            frame_counts.insert(frame_count),
            "dialogue screen evidence repeats one emulated frame"
        );
        nametable_hashes.insert(sample.nametable_sha1.clone());
        collect_preserved_screen_codes(&nametable, &mut screen_codes);
    }
    Ok((
        sha1_hex(&manifest_bytes),
        manifest.samples.len(),
        nametable_hashes.len(),
        screen_codes,
    ))
}

fn read_bound_file(path: &Path, expected_sha1: &str, role: &str) -> Result<Vec<u8>> {
    let bytes = fs::read(path)
        .with_context(|| format!("read dialogue {role} sample {}", path.display()))?;
    ensure!(
        sha1_hex(&bytes) == expected_sha1,
        "dialogue {role} sample SHA-1 changed"
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

pub(super) fn central_right_fd_selector_call(target: u16) -> Result<Vec<u8>> {
    assemble_at(
        CENTRAL_RIGHT_FD_SELECTOR_CALL_ADDRESS,
        &[Instruction::JsrAbsolute(target)],
    )
}

pub(super) fn build_page_routine(mapper_register: u8) -> Result<Vec<u8>> {
    build_page_routine_at(
        PAGE_ROUTINE_ADDRESS,
        mapper_register,
        SELECT_RIGHT_FD_CHR_BANK_FOR_PAIR_ADDRESS,
    )
}

pub(super) fn build_page_routine_at(
    routine_address: u16,
    mapper_register: u8,
    fallback_target: u16,
) -> Result<Vec<u8>> {
    let fallback_address = routine_address
        .checked_add(0x43)
        .context("dialogue page selector fallback address overflow")?;

    assemble_at(
        routine_address,
        &[
            Instruction::Php,
            Instruction::Pha,
            Instruction::LdaZeroPage(0x24),
            Instruction::CmpImmediate(0x0B),
            Instruction::BneAbsolute(fallback_address),
            Instruction::LdaZeroPage(0x84),
            Instruction::CmpImmediate(0x00),
            Instruction::BneAbsolute(fallback_address),
            Instruction::LdaZeroPage(0x59),
            Instruction::CmpImmediate(0x1A),
            Instruction::BneAbsolute(fallback_address),
            Instruction::LdaZeroPage(0x5A),
            Instruction::CmpImmediate(0x1A),
            Instruction::BneAbsolute(fallback_address),
            Instruction::LdaZeroPage(0x5B),
            Instruction::CmpImmediate(0x00),
            Instruction::BneAbsolute(fallback_address),
            Instruction::LdaZeroPage(0x5C),
            Instruction::CmpImmediate(0x18),
            Instruction::BneAbsolute(fallback_address),
            Instruction::LdaAbsolute(0x77F7),
            Instruction::CmpImmediate(0x03),
            Instruction::BneAbsolute(fallback_address),
            Instruction::LdaAbsolute(0x781D),
            Instruction::CmpImmediate(0x00),
            Instruction::BneAbsolute(fallback_address),
            Instruction::LdaImmediate(mapper_register),
            Instruction::Pha,
            Instruction::LdaImmediate(2),
            crate::mapper165::selector_safety::select_register_instruction(),
            Instruction::Pla,
            Instruction::StaAbsolute(0x8001),
            Instruction::Pla,
            Instruction::Plp,
            Instruction::Rts,
            Instruction::Pla,
            Instruction::Plp,
            Instruction::JmpAbsolute(fallback_target),
        ],
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn selector_uses_only_the_exact_observed_chapter_one_supplier_contract() {
        let routine = build_page_routine(0x88).unwrap();

        assert_eq!(
            routine.len(),
            usize::from(PAGE_ROUTINE_END - PAGE_ROUTINE_ADDRESS)
        );
        assert_eq!(&routine[..2], &[0x08, 0x48]);
        assert_eq!(
            &routine[routine.len() - 5..],
            &[0x68, 0x28, 0x4C, 0xC0, 0xFA]
        );
        assert!(
            routine
                .windows(5)
                .any(|bytes| bytes == [0xAD, 0xF7, 0x77, 0xC9, 0x03])
        );
        assert!(
            routine
                .windows(5)
                .any(|bytes| bytes == [0xAD, 0x1D, 0x78, 0xC9, 0x00])
        );
    }

    #[test]
    fn selector_can_move_without_changing_its_size_or_fallback_contract() {
        let routine = build_page_routine_at(0xFBD8, 0x98, 0xFAC0).unwrap();

        assert_eq!(
            routine.len(),
            usize::from(PAGE_ROUTINE_END - PAGE_ROUTINE_ADDRESS)
        );
        assert_eq!(&routine[routine.len() - 3..], &[0x4C, 0xC0, 0xFA]);
        assert!(routine.windows(2).any(|bytes| bytes == [0xA9, 0x98]));
    }

    #[test]
    fn dialogue_window_interior_is_the_only_unpreserved_screen_region() {
        let mut nametable = vec![0x44; NAMETABLE_MEMORY_SIZE];
        nametable[TARGET_NAMETABLE * PHYSICAL_NAMETABLE_SIZE + 15 * 32 + 7] = 0x33;
        nametable[TARGET_NAMETABLE * PHYSICAL_NAMETABLE_SIZE + 14 * 32 + 7] = 0x22;
        let mut codes = BTreeSet::new();

        collect_preserved_screen_codes(&nametable, &mut codes);

        assert_eq!(codes, BTreeSet::from([0x22, 0x44]));
    }
}
