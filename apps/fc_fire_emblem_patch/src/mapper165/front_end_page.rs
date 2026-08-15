use std::{
    collections::{BTreeMap, BTreeSet},
    fs,
    path::{Component, Path},
};

use anyhow::{Context, Result, ensure};
use serde::Deserialize;

use crate::{
    font_slots::{FONT_PAGE_SIZE, active_hangul_codes},
    front_end_menu::SAVE_SLOT_SELECTION_COMPOSITE_STATE,
    rom::{EXPECTED_SOURCE_SHA1, Rom},
    rp2a03::{Instruction, assemble_at},
    sha1_hex,
    typed_source::decode_rp2a03_sequence,
};

use super::{
    dialogue_probe_font::{assign_glyph_codes_excluding, build_font_page},
    encode_chr_page_register,
    font_pair_projection::{
        RightFontPageProjection, TranslatedFePageSelection, WRITE_TRANSLATED_CHR_PAGE_ADDRESS,
    },
};

pub(super) const PAGE_ROUTINE_ADDRESS: u16 = 0xFC60;
pub(super) const PAGE_ROUTINE_END: u16 = 0xFC99;
const CHECK_COMPOSITE_PAIR_ADDRESS: u16 = 0xFC70;
const FALLBACK_ADDRESS: u16 = 0xFC94;
const PAGE_REGISTER_OPERAND_ADDRESS: u16 = 0xFC85;
const FALLBACK_TARGET_OPERAND_ADDRESS: u16 = 0xFC97;
const SOURCE_FONT_PHYSICAL_PAGE: usize = 2;
const EXTENSION_PAGE_COUNT: usize = 2;
const NAMETABLE_MEMORY_SIZE: usize = 2 * 1024;
const INTERNAL_RAM_SIZE: usize = 2 * 1024;
const PHYSICAL_NAMETABLE_SIZE: usize = 1024;
const TILE_BYTES_PER_NAMETABLE: usize = 30 * 32;
const MINIMUM_TEMPORAL_SAMPLE_COUNT: usize = 4;

#[derive(Debug, Deserialize)]
struct ScreenEvidenceManifest {
    format_version: u8,
    screen_role: String,
    variant: String,
    source_sha1: String,
    samples: Vec<ScreenEvidenceSample>,
}

#[derive(Debug, Deserialize)]
struct ScreenEvidenceSample {
    label: String,
    directory: String,
    iram_sha1: String,
    nametable_sha1: String,
    state_sha1: String,
}

pub(super) struct FrontEndPagePlan {
    pub(super) assignments: BTreeMap<char, u8>,
    pub(super) page_pack: Vec<u8>,
    pub(super) manifest_sha1: String,
    pub(super) temporal_sample_count: usize,
    pub(super) unique_nametable_count: usize,
    pub(super) preserved_screen_active_code_count: usize,
    pub(super) preserved_source_active_code_count: usize,
    pub(super) preserved_result_dialogue_active_code_count: usize,
    pub(super) preserved_active_code_count: usize,
    pub(super) page_sha1: String,
    pub(super) physical_chr_page: u8,
    pub(super) mapper_register: u8,
}

pub(super) fn plan_front_end_page(
    parity_rom: &Rom,
    source_rom: &Rom,
    manifest_path: &Path,
    glyphs: &BTreeSet<char>,
    preserved_source_codes: &BTreeSet<u8>,
    result_dialogue_preserved_codes: &BTreeSet<u8>,
    physical_chr_page: u8,
) -> Result<FrontEndPagePlan> {
    source_rom.verify_supported_japanese()?;
    let (manifest_sha1, temporal_sample_count, unique_nametable_count, screen_codes) =
        load_screen_codes(manifest_path)?;
    let active_codes = active_hangul_codes().into_iter().collect::<BTreeSet<_>>();
    let preserved_screen_active_codes = screen_codes
        .intersection(&active_codes)
        .copied()
        .collect::<BTreeSet<_>>();
    let preserved_source_active_codes = preserved_source_codes
        .intersection(&active_codes)
        .copied()
        .collect::<BTreeSet<_>>();
    let preserved_result_dialogue_active_codes = result_dialogue_preserved_codes
        .intersection(&active_codes)
        .copied()
        .collect::<BTreeSet<_>>();
    let preserved_active_codes = preserved_screen_active_codes
        .union(&preserved_source_active_codes)
        .copied()
        .chain(preserved_result_dialogue_active_codes.iter().copied())
        .collect::<BTreeSet<_>>();
    let assignments = assign_glyph_codes_excluding(glyphs, &preserved_active_codes)?;

    ensure!(
        parity_rom.chr().len().is_multiple_of(FONT_PAGE_SIZE),
        "front-end parity CHR is not 4 KiB page aligned"
    );
    ensure!(
        parity_rom.chr().len() / FONT_PAGE_SIZE == usize::from(physical_chr_page),
        "front-end extension physical CHR page does not follow the cumulative base"
    );
    let source_start = SOURCE_FONT_PHYSICAL_PAGE * FONT_PAGE_SIZE;
    let source_end = source_start + EXTENSION_PAGE_COUNT * FONT_PAGE_SIZE;
    let source_pair = parity_rom
        .chr()
        .get(source_start..source_end)
        .context("front-end source font page pair is outside parity CHR")?;
    let mut translated_page = build_font_page(&source_pair[..FONT_PAGE_SIZE], &assignments)?;
    let projection = RightFontPageProjection::for_screen_roles(
        source_rom.chr(),
        &["new_game_choice", "save_slot_selection"],
        0,
    )?;
    ensure!(
        projection.fe_selection() == TranslatedFePageSelection::UseTranslatedPage,
        "front-end screen roles no longer require one translated FD/FE page"
    );
    projection.apply_to_page(&mut translated_page)?;
    let mut page_pack = translated_page;
    page_pack.extend_from_slice(&source_pair[FONT_PAGE_SIZE..]);
    let mapper_register = encode_chr_page_register(physical_chr_page)?;

    Ok(FrontEndPagePlan {
        assignments,
        page_sha1: sha1_hex(&page_pack[..FONT_PAGE_SIZE]),
        page_pack,
        manifest_sha1,
        temporal_sample_count,
        unique_nametable_count,
        preserved_screen_active_code_count: preserved_screen_active_codes.len(),
        preserved_source_active_code_count: preserved_source_active_codes.len(),
        preserved_result_dialogue_active_code_count: preserved_result_dialogue_active_codes.len(),
        preserved_active_code_count: preserved_active_codes.len(),
        physical_chr_page,
        mapper_register,
    })
}

pub(super) fn build_page_selector(mapper_register: u8, fallback_target: u16) -> Result<Vec<u8>> {
    ensure!(
        mapper_register != 0,
        "front-end page register cannot be zero"
    );
    assemble_at(
        PAGE_ROUTINE_ADDRESS,
        &[
            Instruction::Php,
            Instruction::Pha,
            Instruction::LdaAbsolute(0x05E8),
            Instruction::Sec,
            Instruction::SbcImmediate(0x0D),
            Instruction::CmpImmediate(0x02),
            Instruction::BccAbsolute(CHECK_COMPOSITE_PAIR_ADDRESS),
            Instruction::CmpImmediate(SAVE_SLOT_SELECTION_COMPOSITE_STATE - 0x0D),
            Instruction::BneAbsolute(FALLBACK_ADDRESS),
            Instruction::LdaZeroPage(0x59),
            Instruction::CmpImmediate(0x1A),
            Instruction::BneAbsolute(FALLBACK_ADDRESS),
            Instruction::LdaZeroPage(0x5A),
            Instruction::CmpImmediate(0x1A),
            Instruction::BneAbsolute(FALLBACK_ADDRESS),
            Instruction::LdaZeroPage(0x5B),
            Instruction::OraZeroPage(0x52),
            Instruction::AndImmediate(0x1F),
            Instruction::BneAbsolute(FALLBACK_ADDRESS),
            Instruction::LdaImmediate(mapper_register),
            Instruction::LdxImmediate(2),
            Instruction::JsrAbsolute(WRITE_TRANSLATED_CHR_PAGE_ADDRESS),
            Instruction::LdxImmediate(4),
            Instruction::JsrAbsolute(WRITE_TRANSLATED_CHR_PAGE_ADDRESS),
            Instruction::Pla,
            Instruction::Plp,
            Instruction::Rts,
            Instruction::Nop,
            Instruction::Pla,
            Instruction::Plp,
            Instruction::JmpAbsolute(fallback_target),
        ],
    )
}

pub(crate) fn bind_installed_front_end_mapper_register(candidate: &Rom) -> Result<u8> {
    let selector_len = usize::from(PAGE_ROUTINE_END - PAGE_ROUTINE_ADDRESS);
    let fixed_bank = candidate
        .prg()
        .get(candidate.prg().len().saturating_sub(16 * 1024)..)
        .context("candidate has no active fixed PRG bank")?;
    let start = usize::from(PAGE_ROUTINE_ADDRESS - 0xC000);
    let actual = fixed_bank
        .get(start..start + selector_len)
        .context("candidate front-end page selector is outside the active fixed bank")?;
    decode_rp2a03_sequence(
        actual,
        PAGE_ROUTINE_ADDRESS,
        "select the installed front-end font page",
    )?;

    let register_offset = usize::from(PAGE_REGISTER_OPERAND_ADDRESS - PAGE_ROUTINE_ADDRESS);
    let fallback_offset = usize::from(FALLBACK_TARGET_OPERAND_ADDRESS - PAGE_ROUTINE_ADDRESS);
    let mapper_register = actual[register_offset];
    let fallback_target =
        u16::from_le_bytes([actual[fallback_offset], actual[fallback_offset + 1]]);
    ensure!(
        build_page_selector(mapper_register, fallback_target)? == actual,
        "candidate front-end page selector no longer matches its generated structure"
    );
    let physical_page = mapper_register / 4;
    ensure!(
        encode_chr_page_register(physical_page)? == mapper_register
            && usize::from(physical_page) < candidate.chr().len() / FONT_PAGE_SIZE,
        "candidate front-end selector names an invalid physical CHR page"
    );
    Ok(mapper_register)
}

fn load_screen_codes(manifest_path: &Path) -> Result<(String, usize, usize, BTreeSet<u8>)> {
    let manifest_bytes = fs::read(manifest_path)
        .with_context(|| format!("read front-end screen evidence {}", manifest_path.display()))?;
    let manifest: ScreenEvidenceManifest =
        serde_json::from_slice(&manifest_bytes).with_context(|| {
            format!(
                "parse front-end screen evidence {}",
                manifest_path.display()
            )
        })?;
    ensure!(
        manifest.format_version == 1,
        "unsupported front-end evidence format"
    );
    ensure!(
        manifest.screen_role == "front_end_menu",
        "front-end evidence role changed"
    );
    ensure!(
        manifest.variant == "no_valid_save",
        "front-end evidence variant changed"
    );
    ensure!(
        manifest.source_sha1 == EXPECTED_SOURCE_SHA1,
        "front-end evidence source changed"
    );
    ensure!(
        manifest.samples.len() >= MINIMUM_TEMPORAL_SAMPLE_COUNT,
        "front-end evidence needs at least {MINIMUM_TEMPORAL_SAMPLE_COUNT} temporal samples"
    );

    let parent = manifest_path
        .parent()
        .context("front-end screen evidence has no parent directory")?;
    let mut labels = BTreeSet::new();
    let mut frame_counts = BTreeSet::new();
    let mut nametable_hashes = BTreeSet::new();
    let mut screen_codes = BTreeSet::new();
    for sample in &manifest.samples {
        ensure!(
            labels.insert(&sample.label),
            "duplicate front-end evidence label"
        );
        let relative = Path::new(&sample.directory);
        ensure!(
            !relative.is_absolute()
                && relative
                    .components()
                    .all(|component| !matches!(component, Component::ParentDir)),
            "front-end evidence sample paths must stay below the manifest"
        );
        let directory = parent.join(relative);
        let iram = read_bound_file(&directory.join("iram.bin"), &sample.iram_sha1, "IRAM")?;
        ensure!(
            iram.len() == INTERNAL_RAM_SIZE,
            "front-end IRAM sample must be 2 KiB"
        );
        ensure!(
            iram[0x05E8] == 0x0D,
            "front-end no-save sample is not in composite state 0x0D"
        );
        let nametable = read_bound_file(
            &directory.join("nametable.bin"),
            &sample.nametable_sha1,
            "nametable",
        )?;
        ensure!(
            nametable.len() == NAMETABLE_MEMORY_SIZE,
            "front-end nametable sample must contain exactly 2 KiB"
        );
        let state = read_bound_file(&directory.join("state.json"), &sample.state_sha1, "state")?;
        let state: serde_json::Value =
            serde_json::from_slice(&state).context("parse front-end screen state sample")?;
        ensure!(
            state
                .get("ppu.control.backgroundPatternAddr")
                .and_then(serde_json::Value::as_u64)
                == Some(0x1000),
            "front-end sample does not use the right background pattern table"
        );
        ensure!(
            state
                .get("ppu.control.spritePatternAddr")
                .and_then(serde_json::Value::as_u64)
                == Some(0),
            "front-end sample does not keep sprites on the left pattern table"
        );
        ensure!(
            state
                .get("mapper.registers2")
                .and_then(serde_json::Value::as_u64)
                == Some(8),
            "front-end sample does not use the original right-FD font page"
        );
        let frame_count = state
            .get("frameCount")
            .and_then(serde_json::Value::as_u64)
            .context("front-end state sample has no frame count")?;
        ensure!(
            frame_counts.insert(frame_count),
            "front-end evidence repeats one frame"
        );
        nametable_hashes.insert(sample.nametable_sha1.clone());
        collect_screen_codes(&nametable, &mut screen_codes);
    }
    Ok((
        sha1_hex(&manifest_bytes),
        manifest.samples.len(),
        nametable_hashes.len(),
        screen_codes,
    ))
}

fn read_bound_file(path: &Path, expected_sha1: &str, role: &str) -> Result<Vec<u8>> {
    let bytes =
        fs::read(path).with_context(|| format!("read front-end {role} {}", path.display()))?;
    ensure!(
        sha1_hex(&bytes) == expected_sha1,
        "front-end {role} SHA-1 changed"
    );
    Ok(bytes)
}

fn collect_screen_codes(nametable: &[u8], codes: &mut BTreeSet<u8>) {
    for physical_table in 0..2 {
        let table_start = physical_table * PHYSICAL_NAMETABLE_SIZE;
        codes.extend(
            nametable[table_start..table_start + TILE_BYTES_PER_NAMETABLE]
                .iter()
                .copied(),
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn selector_admits_both_front_end_menu_lifetimes_and_original_page_request() {
        let routine = build_page_selector(
            0xA8,
            super::super::SELECT_RIGHT_FD_CHR_BANK_FOR_PAIR_ADDRESS,
        )
        .unwrap();

        assert_eq!(
            routine.len(),
            usize::from(PAGE_ROUTINE_END - PAGE_ROUTINE_ADDRESS)
        );
        assert!(routine.windows(12).any(|bytes| {
            bytes
                == [
                    0xAD,
                    0xE8,
                    0x05,
                    0x38,
                    0xE9,
                    0x0D,
                    0xC9,
                    0x02,
                    0x90,
                    0x04,
                    0xC9,
                    SAVE_SLOT_SELECTION_COMPOSITE_STATE - 0x0D,
                ]
        }));
        assert!(
            routine
                .windows(8)
                .any(|bytes| { bytes == [0xA5, 0x5B, 0x05, 0x52, 0x29, 0x1F, 0xD0, 0x10] })
        );
        assert!(
            !routine.windows(2).any(|bytes| bytes == [0xA5, 0x5C]),
            "front-end FD codebook selection must not depend on the unrelated FE backdrop"
        );
        assert!(
            routine
                .windows(4)
                .any(|bytes| bytes == [0xA9, 0xA8, 0xA2, 0x02])
        );
        assert!(routine.windows(5).any(|bytes| {
            bytes
                == [
                    0x20,
                    WRITE_TRANSLATED_CHR_PAGE_ADDRESS as u8,
                    (WRITE_TRANSLATED_CHR_PAGE_ADDRESS >> 8) as u8,
                    0xA2,
                    0x04,
                ]
        }));
        assert_eq!(&routine[routine.len() - 3..], &[0x4C, 0xC0, 0xFA]);
    }
}
