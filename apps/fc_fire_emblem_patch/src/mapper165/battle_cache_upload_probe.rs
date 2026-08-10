use std::{fs, path::Path};

use anyhow::{Context, Result, ensure};
use serde::Serialize;

use crate::{
    font_slots::FONT_PAGE_SIZE,
    mmc5_chr::switchable_bank_file_offset,
    mmc5_prg::count_direct_transfers_to_range,
    rom::{EXPECTED_SOURCE_SHA1, HEADER_SIZE, Rom},
    rp2a03::{Instruction, assemble_at},
    sha1_hex,
    tracked::TrackedImage,
};

use super::{
    OUTPUT_MAPPER,
    battle_combination_probe::{
        GameplayBattleCombinationImage, assemble_gameplay_battle_combination,
    },
    dialogue_probe_font::SOURCE_FONT_PHYSICAL_PAGE,
};

const EXPANDED_PRG_SIZE: usize = 512 * 1024;
const FIXED_BANK_SIZE: usize = 16 * 1024;
const CACHE_PRG_OFFSET: usize = 256 * 1024;
const CACHE_MMC3_PAGE: u8 = 0x20;
const BATTLE_ENGINE_PRG_BANK: u8 = 0x05;
const SOURCE_NMI_UPLOAD_HOOK: u16 = 0xC191;
const SOURCE_NMI_INPUT_SCAN: u16 = 0xC2D9;
const SOURCE_NMI_SCROLL_RESTORE: u16 = 0xC36A;
const BATTLE_TRANSITION_HOOK: u16 = 0xFAF3;
const UPLOAD_FONT_PAGE: u16 = 0xFB20;
const GAMEPLAY_BATTLE_CACHE_MATCH_PREDICATE: u16 = 0xFC21;
const BATTLE_RIGHT_FD_SELECTOR: u16 = 0xFC50;
const BATTLE_CENTRAL_RIGHT_FD_SELECTOR: u16 = 0xFC80;
const BATTLE_RIGHT_FE_SELECTOR: u16 = 0xFCC4;
const SOURCE_RIGHT_FD_SELECTOR: u16 = 0xFA80;
const SOURCE_RIGHT_FE_SELECTOR: u16 = 0xFAA0;
const SOURCE_PAIR_AWARE_RIGHT_FD_SELECTOR: u16 = 0xFAC0;
const SOURCE_CENTRAL_RIGHT_FD_CALL: u16 = 0xC9C2;
const SOURCE_CENTRAL_FE_FD_REFRESH_CALL: u16 = 0xFABB;
const BATTLE_ACTIVE_FLAG: u16 = 0x047D;
const CACHE_UPLOADED_MARKER: u8 = 0x80;
const MAIN_STATE: u8 = 0x84;
const BATTLE_MAIN_STATE: u8 = 0x16;
const ENEMY_INITIATED_BATTLE_MAIN_STATE: u8 = 0x32;
const BATTLE_RECORD_ONE: u16 = 0x76F4;
const BATTLE_RECORD_TWO: u16 = 0x7715;
const CAIN_RECORD_IDENTITY: u8 = 0x04;
const GARUDA_SOLDIER_RECORD_IDENTITY: u8 = 0x85;
const PPU_MASK_SHADOW: u8 = 0xCC;
const UPLOAD_RENDER_MASK: u8 = 0x06;
const PPU_CONTROL_SHADOW: u16 = 0x00CD;
const RIGHT_FE_SHADOW: u8 = 0x5C;
const CHR_HIGH_BITS: u8 = 0x52;

#[derive(Debug, Serialize)]
struct BattleCacheUploadProbeReport {
    schema: u8,
    source_sha1: &'static str,
    fixed_workspace_sha1: String,
    dialogue_workspace_sha1: String,
    output_sha1: String,
    output_mapper: u16,
    prg_size: usize,
    chr_size: usize,
    combination_role: &'static str,
    gameplay_battle_main_states: [u8; 2],
    cache_participant_record_identities: [u8; 2],
    participant_pair_gated: bool,
    preserved_active_code_count: usize,
    codebook_glyph_count: usize,
    codebook_assignment_sha1: String,
    cache_mmc3_page: u8,
    cache_page_byte_count: usize,
    cache_page_sha1: String,
    transition_hook_bank: &'static str,
    transition_hook_cpu_address: String,
    render_disabled_mask: u8,
    render_disabled_phase: u8,
    upload_after_ppu_mask_write: bool,
    nmi_disabled_during_upload: bool,
    sequential_ppu_increment_during_upload: bool,
    ppu_address_latch_reset_before_upload: bool,
    pending_vblank_cleared_before_nmi_restore: bool,
    battle_active_flag_address: String,
    cache_uploaded_marker: u8,
    battle_active_nonzero_semantics_preserved: bool,
    battle_initializers_clear_cache_marker: bool,
    original_prg_bank_restored: bool,
    battle_zero_right_page_uses_chr_ram: bool,
    non_battle_right_pages_use_natural_selection: bool,
    original_chr_preserved: bool,
    runtime_routine_count: usize,
    runtime_tracked_write_count: usize,
    translation_text_emitted: bool,
    glyph_characters_emitted: bool,
    runtime_verified: bool,
    release_eligible: bool,
    next_gate: &'static str,
}

pub(crate) struct BattleCacheUploadProbeSummary {
    pub(crate) output_sha1: String,
    pub(crate) report_sha1: String,
    pub(crate) glyph_count: usize,
    pub(crate) runtime_tracked_write_count: usize,
}

struct RuntimeRoutine {
    role: &'static str,
    address: u16,
    bytes: Vec<u8>,
}

pub(crate) fn build_battle_cache_upload_probe(
    source_path: &Path,
    fixed_workspace_path: &Path,
    dialogue_workspace_path: &Path,
    output_path: &Path,
    report_path: &Path,
) -> Result<BattleCacheUploadProbeSummary> {
    let combination = assemble_gameplay_battle_combination(
        source_path,
        fixed_workspace_path,
        dialogue_workspace_path,
    )?;
    let cache_page = combination_cache_page(&combination)?;
    let base = expand_combination_with_cache(&combination, &cache_page)?;
    let base_rom = Rom::parse(base.clone()).context("parse battle cache upload base")?;
    let routines = build_runtime_routines()?;
    verify_runtime_caves(&combination.parity, &routines)?;
    verify_battle_active_flag_contract(&combination.parity)?;
    let mut image = TrackedImage::new(base.clone());
    for routine in &routines {
        image.write_expected(
            format!("battle cache {} routine", routine.role),
            expanded_fixed_bank_file_offset(routine.address)?,
            &vec![0xFF; routine.bytes.len()],
            &routine.bytes,
        )?;
    }

    image.write_expected(
        "battle NMI post-mask cache hook",
        expanded_fixed_bank_file_offset(SOURCE_NMI_UPLOAD_HOOK)?,
        &assemble_at(
            SOURCE_NMI_UPLOAD_HOOK,
            &[Instruction::JsrAbsolute(SOURCE_NMI_INPUT_SCAN)],
        )?,
        &assemble_at(
            SOURCE_NMI_UPLOAD_HOOK,
            &[Instruction::JsrAbsolute(BATTLE_TRANSITION_HOOK)],
        )?,
    )?;

    let natural_right_fd = natural_right_fd_selector()?;
    let mut right_fd_redirect = assemble_at(
        SOURCE_RIGHT_FD_SELECTOR,
        &[Instruction::JmpAbsolute(BATTLE_RIGHT_FD_SELECTOR)],
    )?;
    right_fd_redirect.resize(natural_right_fd.len(), 0xEA);
    image.write_expected(
        "battle-aware direct right FD selector",
        expanded_fixed_bank_file_offset(SOURCE_RIGHT_FD_SELECTOR)?,
        &natural_right_fd,
        &right_fd_redirect,
    )?;

    let natural_right_fe = natural_right_chr_selector(SOURCE_RIGHT_FE_SELECTOR, 4)?;
    let mut right_fe_redirect = assemble_at(
        SOURCE_RIGHT_FE_SELECTOR,
        &[Instruction::JmpAbsolute(BATTLE_RIGHT_FE_SELECTOR)],
    )?;
    right_fe_redirect.resize(natural_right_fe.len(), 0xEA);
    image.write_expected(
        "battle-aware right FE selector",
        expanded_fixed_bank_file_offset(SOURCE_RIGHT_FE_SELECTOR)?,
        &natural_right_fe,
        &right_fe_redirect,
    )?;

    redirect_call(
        &mut image,
        "battle-aware central right FD selector",
        SOURCE_CENTRAL_RIGHT_FD_CALL,
        SOURCE_PAIR_AWARE_RIGHT_FD_SELECTOR,
        BATTLE_CENTRAL_RIGHT_FD_SELECTOR,
    )?;
    redirect_call(
        &mut image,
        "battle-aware central FE right FD refresh",
        SOURCE_CENTRAL_FE_FD_REFRESH_CALL,
        SOURCE_PAIR_AWARE_RIGHT_FD_SELECTOR,
        BATTLE_CENTRAL_RIGHT_FD_SELECTOR,
    )?;

    image.verify_all_changes_tracked(&base)?;
    let runtime_tracked_write_count = image.writes().len();
    let output = image.into_data();
    let output_rom = Rom::parse(output.clone()).context("parse battle cache upload probe")?;
    ensure!(
        output_rom.mapper() == OUTPUT_MAPPER,
        "output mapper changed"
    );
    ensure!(
        output_rom.prg().len() == EXPANDED_PRG_SIZE,
        "expanded PRG size changed"
    );
    ensure!(
        output_rom.chr() == base_rom.chr(),
        "runtime installation changed CHR"
    );
    ensure!(
        output_rom.chr() == Rom::parse(combination.parity.clone())?.chr(),
        "battle cache probe changed the parity CHR"
    );

    let output_sha1 = sha1_hex(&output);
    let report = BattleCacheUploadProbeReport {
        schema: 1,
        source_sha1: EXPECTED_SOURCE_SHA1,
        fixed_workspace_sha1: combination.fixed_workspace_sha1,
        dialogue_workspace_sha1: combination.dialogue_workspace_sha1,
        output_sha1: output_sha1.clone(),
        output_mapper: output_rom.mapper(),
        prg_size: output_rom.prg().len(),
        chr_size: output_rom.chr().len(),
        combination_role: "chapter-one Cain and Garuda soldier gameplay battle pair",
        gameplay_battle_main_states: [BATTLE_MAIN_STATE, ENEMY_INITIATED_BATTLE_MAIN_STATE],
        cache_participant_record_identities: [CAIN_RECORD_IDENTITY, GARUDA_SOLDIER_RECORD_IDENTITY],
        participant_pair_gated: true,
        preserved_active_code_count: combination.preserved_active_code_count,
        codebook_glyph_count: combination.glyph_count,
        codebook_assignment_sha1: combination.codebook_assignment_sha1,
        cache_mmc3_page: CACHE_MMC3_PAGE,
        cache_page_byte_count: cache_page.len(),
        cache_page_sha1: sha1_hex(&cache_page),
        transition_hook_bank: "fixed",
        transition_hook_cpu_address: format!("0x{SOURCE_NMI_UPLOAD_HOOK:04X}"),
        render_disabled_mask: UPLOAD_RENDER_MASK,
        render_disabled_phase: 0,
        upload_after_ppu_mask_write: true,
        nmi_disabled_during_upload: true,
        sequential_ppu_increment_during_upload: true,
        ppu_address_latch_reset_before_upload: true,
        pending_vblank_cleared_before_nmi_restore: true,
        battle_active_flag_address: format!("0x{BATTLE_ACTIVE_FLAG:04X}"),
        cache_uploaded_marker: CACHE_UPLOADED_MARKER,
        battle_active_nonzero_semantics_preserved: true,
        battle_initializers_clear_cache_marker: true,
        original_prg_bank_restored: true,
        battle_zero_right_page_uses_chr_ram: true,
        non_battle_right_pages_use_natural_selection: true,
        original_chr_preserved: true,
        runtime_routine_count: routines.len(),
        runtime_tracked_write_count,
        translation_text_emitted: false,
        glyph_characters_emitted: false,
        runtime_verified: false,
        release_eligible: false,
        next_gate: "replace the participant-pair probe key with a generated full battle signature before broadening cache coverage; keep defeat dialogue on its separate screen path",
    };
    let mut report_bytes =
        serde_json::to_vec_pretty(&report).context("serialize battle cache upload report")?;
    report_bytes.push(b'\n');
    write_file(output_path, &output)?;
    write_file(report_path, &report_bytes)?;
    Ok(BattleCacheUploadProbeSummary {
        output_sha1,
        report_sha1: sha1_hex(&report_bytes),
        glyph_count: combination.glyph_count,
        runtime_tracked_write_count,
    })
}

fn combination_cache_page(combination: &GameplayBattleCombinationImage) -> Result<Vec<u8>> {
    let combination_rom = Rom::parse(combination.data.clone())?;
    let start = SOURCE_FONT_PHYSICAL_PAGE
        .checked_mul(FONT_PAGE_SIZE)
        .context("combination font-page offset overflow")?;
    Ok(combination_rom
        .chr()
        .get(start..start + FONT_PAGE_SIZE)
        .context("combination font page is outside CHR")?
        .to_vec())
}

fn expand_combination_with_cache(
    combination: &GameplayBattleCombinationImage,
    cache_page: &[u8],
) -> Result<Vec<u8>> {
    ensure!(
        cache_page.len() == FONT_PAGE_SIZE,
        "cache page is not 4 KiB"
    );
    let combination_rom = Rom::parse(combination.data.clone())?;
    let parity_rom = Rom::parse(combination.parity.clone())?;
    ensure!(
        combination_rom.prg().len() == 256 * 1024,
        "combination PRG size changed"
    );
    ensure!(
        combination_rom.chr().len() == parity_rom.chr().len(),
        "combination CHR size changed"
    );
    let mut header = parity_rom.data()[..HEADER_SIZE].to_vec();
    header[4] = u8::try_from(EXPANDED_PRG_SIZE / FIXED_BANK_SIZE)?;
    let mut prg = vec![0xFF; EXPANDED_PRG_SIZE];
    prg[..combination_rom.prg().len()].copy_from_slice(combination_rom.prg());
    prg[CACHE_PRG_OFFSET..CACHE_PRG_OFFSET + cache_page.len()].copy_from_slice(cache_page);
    let fixed_start = combination_rom.prg().len() - FIXED_BANK_SIZE;
    prg[EXPANDED_PRG_SIZE - FIXED_BANK_SIZE..]
        .copy_from_slice(&combination_rom.prg()[fixed_start..]);
    let mut output = Vec::with_capacity(HEADER_SIZE + prg.len() + parity_rom.chr().len());
    output.extend_from_slice(&header);
    output.extend_from_slice(&prg);
    output.extend_from_slice(parity_rom.chr());
    Ok(output)
}

fn verify_runtime_caves(parity: &[u8], routines: &[RuntimeRoutine]) -> Result<()> {
    let parity_rom = Rom::parse(parity.to_vec())?;
    for routine in routines {
        let end_address = routine
            .address
            .checked_add(u16::try_from(routine.bytes.len())?)
            .context("battle cache runtime routine range overflow")?;
        let start = original_fixed_bank_file_offset(routine.address)?;
        let end = original_fixed_bank_file_offset(end_address)?;
        ensure!(
            parity[start..end].iter().all(|byte| *byte == 0xFF),
            "battle cache {} cave is no longer all FF",
            routine.role
        );
        let transfer_count =
            count_direct_transfers_to_range(parity_rom.prg(), routine.address, end_address)?;
        ensure!(
            transfer_count == 0,
            "battle cache {} cave has {transfer_count} pre-existing direct transfers",
            routine.role
        );
    }
    Ok(())
}

fn verify_battle_active_flag_contract(parity: &[u8]) -> Result<()> {
    let prg = Rom::parse(parity.to_vec())?.prg().to_vec();
    let read_pattern = [0xAD, 0x7D, 0x04];
    let write_pattern = [0x8D, 0x7D, 0x04];
    ensure!(
        prg.windows(read_pattern.len())
            .filter(|bytes| *bytes == read_pattern)
            .count()
            == 1,
        "battle-active flag direct read count changed"
    );
    ensure!(
        prg.windows(write_pattern.len())
            .filter(|bytes| *bytes == write_pattern)
            .count()
            == 5,
        "battle-active flag direct write count changed"
    );

    verify_switchable_bytes(
        parity,
        0x05,
        0x8000,
        &[0xAD, 0x7D, 0x04, 0xD0, 0x01, 0x60],
        "battle-active nonzero reader",
    )?;
    verify_switchable_bytes(
        parity,
        0x05,
        0x80DE,
        &[
            0xA9, 0x00, 0xA2, 0x02, 0x9D, 0xAD, 0x03, 0x9D, 0x89, 0x03, 0x9D, 0xA7, 0x03, 0x9D,
            0xAA, 0x03, 0xCA, 0x10, 0xF1, 0x8D, 0x78, 0x04, 0x8D, 0xCF, 0x03, 0x8D, 0xD0, 0x03,
            0x8D, 0x7C, 0x04, 0x8D, 0xBF, 0x03, 0x8D, 0x7D, 0x04,
        ],
        "battle-active zeroing writer",
    )?;
    for (bank, address) in [(0x05, 0x82B9), (0x06, 0x92FE), (0x06, 0x9D50)] {
        verify_switchable_bytes(
            parity,
            bank,
            address,
            &[0xA9, 0x01, 0x8D, 0x7D, 0x04],
            "battle-active initializer",
        )?;
    }
    verify_switchable_bytes(
        parity,
        0x07,
        0xAC12,
        &[0xA9, 0x01, 0x8D, 0xED, 0x05, 0x8D, 0x7D, 0x04],
        "sound-test battle-active initializer",
    )?;
    for (bank, address) in [
        (0x05, 0x8100),
        (0x05, 0x82BB),
        (0x06, 0x9300),
        (0x06, 0x9D52),
        (0x07, 0xAC17),
    ] {
        verify_switchable_bytes(
            parity,
            bank,
            address,
            &write_pattern,
            "battle-active full-byte writer",
        )?;
    }
    Ok(())
}

fn verify_switchable_bytes(
    image: &[u8],
    bank: u8,
    address: u16,
    expected: &[u8],
    role: &str,
) -> Result<()> {
    let offset = switchable_bank_file_offset(bank, address)?;
    ensure!(
        image.get(offset..offset + expected.len()) == Some(expected),
        "{role} changed at bank {bank:02X}:${address:04X}"
    );
    Ok(())
}

fn build_runtime_routines() -> Result<Vec<RuntimeRoutine>> {
    Ok(vec![
        RuntimeRoutine {
            role: "NMI post-mask dispatch",
            address: BATTLE_TRANSITION_HOOK,
            bytes: assemble_at(
                BATTLE_TRANSITION_HOOK,
                &[
                    Instruction::JsrAbsolute(SOURCE_NMI_INPUT_SCAN),
                    Instruction::JsrAbsolute(GAMEPLAY_BATTLE_CACHE_MATCH_PREDICATE),
                    Instruction::BneAbsolute(BATTLE_TRANSITION_HOOK + 27),
                    Instruction::LdaAbsolute(BATTLE_ACTIVE_FLAG),
                    Instruction::AndImmediate(CACHE_UPLOADED_MARKER),
                    Instruction::BneAbsolute(BATTLE_TRANSITION_HOOK + 27),
                    Instruction::LdaZeroPage(PPU_MASK_SHADOW),
                    Instruction::CmpImmediate(UPLOAD_RENDER_MASK),
                    Instruction::BneAbsolute(BATTLE_TRANSITION_HOOK + 27),
                    Instruction::JsrAbsolute(UPLOAD_FONT_PAGE),
                    Instruction::JsrAbsolute(SOURCE_NMI_SCROLL_RESTORE),
                    Instruction::Rts,
                ],
            )?,
        },
        RuntimeRoutine {
            role: "4 KiB CHR-RAM upload",
            address: UPLOAD_FONT_PAGE,
            bytes: upload_font_page_routine()?,
        },
        RuntimeRoutine {
            role: "gameplay battle-cache match predicate",
            address: GAMEPLAY_BATTLE_CACHE_MATCH_PREDICATE,
            bytes: gameplay_battle_cache_match_predicate()?,
        },
        RuntimeRoutine {
            role: "battle-aware direct right FD selection",
            address: BATTLE_RIGHT_FD_SELECTOR,
            bytes: battle_right_fd_selector()?,
        },
        RuntimeRoutine {
            role: "battle-aware central right FD selection",
            address: BATTLE_CENTRAL_RIGHT_FD_SELECTOR,
            bytes: battle_central_right_fd_selector()?,
        },
        RuntimeRoutine {
            role: "battle-aware right FE selection",
            address: BATTLE_RIGHT_FE_SELECTOR,
            bytes: battle_right_chr_selector(BATTLE_RIGHT_FE_SELECTOR, 4)?,
        },
    ])
}

fn gameplay_battle_cache_match_predicate() -> Result<Vec<u8>> {
    let pair = GAMEPLAY_BATTLE_CACHE_MATCH_PREDICATE + 10;
    let cain_first = GAMEPLAY_BATTLE_CACHE_MATCH_PREDICATE + 27;
    let mismatch = GAMEPLAY_BATTLE_CACHE_MATCH_PREDICATE + 32;
    assemble_at(
        GAMEPLAY_BATTLE_CACHE_MATCH_PREDICATE,
        &[
            Instruction::LdaZeroPage(MAIN_STATE),
            Instruction::CmpImmediate(BATTLE_MAIN_STATE),
            Instruction::BeqAbsolute(pair),
            Instruction::CmpImmediate(ENEMY_INITIATED_BATTLE_MAIN_STATE),
            Instruction::BneAbsolute(mismatch),
            Instruction::LdaAbsolute(BATTLE_RECORD_ONE),
            Instruction::CmpImmediate(CAIN_RECORD_IDENTITY),
            Instruction::BeqAbsolute(cain_first),
            Instruction::CmpImmediate(GARUDA_SOLDIER_RECORD_IDENTITY),
            Instruction::BneAbsolute(mismatch),
            Instruction::LdaAbsolute(BATTLE_RECORD_TWO),
            Instruction::CmpImmediate(CAIN_RECORD_IDENTITY),
            Instruction::Rts,
            Instruction::LdaAbsolute(BATTLE_RECORD_TWO),
            Instruction::CmpImmediate(GARUDA_SOLDIER_RECORD_IDENTITY),
            Instruction::Rts,
        ],
    )
}

fn upload_font_page_routine() -> Result<Vec<u8>> {
    let mut instructions = vec![
        Instruction::Php,
        Instruction::Pha,
        Instruction::Txa,
        Instruction::Pha,
        Instruction::Tya,
        Instruction::Pha,
        Instruction::LdaAbsolute(PPU_CONTROL_SHADOW),
        Instruction::Pha,
        Instruction::AndImmediate(0x7B),
        Instruction::StaAbsolute(PPU_CONTROL_SHADOW),
        Instruction::StaAbsolute(0x2000),
        Instruction::LdaImmediate(6),
        Instruction::StaAbsolute(0x8000),
        Instruction::LdaImmediate(CACHE_MMC3_PAGE),
        Instruction::StaAbsolute(0x8001),
        Instruction::LdaImmediate(2),
        Instruction::StaAbsolute(0x8000),
        Instruction::LdaImmediate(0),
        Instruction::StaAbsolute(0x8001),
        Instruction::LdaImmediate(4),
        Instruction::StaAbsolute(0x8000),
        Instruction::LdaImmediate(0),
        Instruction::StaAbsolute(0x8001),
        Instruction::LdaAbsolute(0x2002),
        Instruction::LdaImmediate(0x10),
        Instruction::StaAbsolute(0x2006),
        Instruction::LdaImmediate(0),
        Instruction::StaAbsolute(0x2006),
        Instruction::LdxImmediate(0),
    ];
    for page in 0..16_u16 {
        let loop_address =
            UPLOAD_FONT_PAGE + u16::try_from(assemble_at(UPLOAD_FONT_PAGE, &instructions)?.len())?;
        instructions.extend([
            Instruction::LdaAbsoluteX(0x8000 + page * 0x100),
            Instruction::StaAbsolute(0x2007),
            Instruction::Inx,
            Instruction::BneAbsolute(loop_address),
        ]);
    }
    instructions.extend([
        Instruction::LdaAbsolute(BATTLE_ACTIVE_FLAG),
        Instruction::OraImmediate(CACHE_UPLOADED_MARKER),
        Instruction::StaAbsolute(BATTLE_ACTIVE_FLAG),
        Instruction::LdaImmediate(6),
        Instruction::StaAbsolute(0x8000),
        Instruction::LdaImmediate(BATTLE_ENGINE_PRG_BANK * 2),
        Instruction::StaAbsolute(0x8001),
        Instruction::LdaZeroPage(RIGHT_FE_SHADOW),
        Instruction::OraZeroPage(CHR_HIGH_BITS),
        Instruction::JsrAbsolute(0xFAA0),
        Instruction::LdaAbsolute(0x2002),
        Instruction::Pla,
        Instruction::StaAbsolute(PPU_CONTROL_SHADOW),
        Instruction::StaAbsolute(0x2000),
        Instruction::Pla,
        Instruction::Tay,
        Instruction::Pla,
        Instruction::Tax,
        Instruction::Pla,
        Instruction::Plp,
        Instruction::Rts,
    ]);
    let bytes = assemble_at(UPLOAD_FONT_PAGE, &instructions)?;
    ensure!(
        UPLOAD_FONT_PAGE as usize + bytes.len() <= BATTLE_RIGHT_FD_SELECTOR as usize,
        "battle cache upload routine overlaps the selector"
    );
    Ok(bytes)
}

fn natural_right_fd_selector() -> Result<Vec<u8>> {
    natural_right_chr_selector(SOURCE_RIGHT_FD_SELECTOR, 2)
}

fn natural_right_chr_selector(address: u16, mapper_register: u8) -> Result<Vec<u8>> {
    assemble_at(
        address,
        &[
            Instruction::Php,
            Instruction::Pha,
            Instruction::AndImmediate(0x1F),
            Instruction::AslAccumulator,
            Instruction::AslAccumulator,
            Instruction::Clc,
            Instruction::AdcImmediate(8),
            Instruction::Pha,
            Instruction::LdaImmediate(mapper_register),
            Instruction::StaAbsolute(0x8000),
            Instruction::Pla,
            Instruction::StaAbsolute(0x8001),
            Instruction::Pla,
            Instruction::Plp,
            Instruction::Rts,
        ],
    )
}

fn battle_right_fd_selector() -> Result<Vec<u8>> {
    battle_right_chr_selector(BATTLE_RIGHT_FD_SELECTOR, 2)
}

fn battle_right_chr_selector(address: u16, mapper_register: u8) -> Result<Vec<u8>> {
    let natural = address + 18;
    let write = address + 27;
    assemble_at(
        address,
        &[
            Instruction::Php,
            Instruction::Pha,
            Instruction::JsrAbsolute(GAMEPLAY_BATTLE_CACHE_MATCH_PREDICATE),
            Instruction::BneAbsolute(natural),
            Instruction::Pla,
            Instruction::Pha,
            Instruction::AndImmediate(0x1F),
            Instruction::BneAbsolute(natural),
            Instruction::LdaImmediate(0),
            Instruction::JmpAbsolute(write),
            Instruction::Pla,
            Instruction::Pha,
            Instruction::AndImmediate(0x1F),
            Instruction::AslAccumulator,
            Instruction::AslAccumulator,
            Instruction::Clc,
            Instruction::AdcImmediate(8),
            Instruction::Pha,
            Instruction::LdaImmediate(mapper_register),
            Instruction::StaAbsolute(0x8000),
            Instruction::Pla,
            Instruction::StaAbsolute(0x8001),
            Instruction::Pla,
            Instruction::Plp,
            Instruction::Rts,
        ],
    )
}

fn battle_central_right_fd_selector() -> Result<Vec<u8>> {
    let natural = BATTLE_CENTRAL_RIGHT_FD_SELECTOR + 26;
    assemble_at(
        BATTLE_CENTRAL_RIGHT_FD_SELECTOR,
        &[
            Instruction::Php,
            Instruction::Pha,
            Instruction::JsrAbsolute(GAMEPLAY_BATTLE_CACHE_MATCH_PREDICATE),
            Instruction::BneAbsolute(natural),
            Instruction::Pla,
            Instruction::Pha,
            Instruction::AndImmediate(0x1F),
            Instruction::BneAbsolute(natural),
            Instruction::LdaImmediate(2),
            Instruction::StaAbsolute(0x8000),
            Instruction::LdaImmediate(0),
            Instruction::StaAbsolute(0x8001),
            Instruction::Pla,
            Instruction::Plp,
            Instruction::Rts,
            Instruction::Pla,
            Instruction::Plp,
            Instruction::JmpAbsolute(SOURCE_PAIR_AWARE_RIGHT_FD_SELECTOR),
        ],
    )
}

fn redirect_call(
    image: &mut TrackedImage,
    label: &str,
    call_address: u16,
    expected_target: u16,
    replacement_target: u16,
) -> Result<()> {
    image.write_expected(
        label,
        expanded_fixed_bank_file_offset(call_address)?,
        &assemble_at(call_address, &[Instruction::JsrAbsolute(expected_target)])?,
        &assemble_at(
            call_address,
            &[Instruction::JsrAbsolute(replacement_target)],
        )?,
    )
}

fn original_fixed_bank_file_offset(cpu_address: u16) -> Result<usize> {
    ensure!(cpu_address >= 0xC000, "address is outside the fixed bank");
    Ok(HEADER_SIZE + 256 * 1024 - FIXED_BANK_SIZE + usize::from(cpu_address - 0xC000))
}

fn expanded_fixed_bank_file_offset(cpu_address: u16) -> Result<usize> {
    ensure!(cpu_address >= 0xC000, "address is outside the fixed bank");
    Ok(HEADER_SIZE + EXPANDED_PRG_SIZE - FIXED_BANK_SIZE + usize::from(cpu_address - 0xC000))
}

fn write_file(path: &Path, data: &[u8]) -> Result<()> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).with_context(|| format!("create {}", parent.display()))?;
    }
    fs::write(path, data).with_context(|| format!("write {}", path.display()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn upload_and_selector_routines_stay_inside_their_declared_cave() {
        let routines = build_runtime_routines().unwrap();
        for pair in routines.windows(2) {
            assert!(pair[0].address as usize + pair[0].bytes.len() <= pair[1].address as usize);
        }
        let last = routines.last().unwrap();
        assert!(last.address as usize + last.bytes.len() <= 0xFCEF);
    }

    #[test]
    fn upload_forces_sequential_ppu_writes_before_restoring_control() {
        let routine = upload_font_page_routine().unwrap();

        assert!(routine.windows(2).any(|bytes| bytes == [0x29, 0x7B]));
        assert!(
            routine
                .windows(5)
                .any(|bytes| bytes == [0xAD, 0x02, 0x20, 0xA9, 0x10])
        );
        assert!(
            routine
                .windows(4)
                .any(|bytes| bytes == [0xAD, 0x02, 0x20, 0x68])
        );
        assert!(routine.windows(3).any(|bytes| bytes == [0x8D, 0x00, 0x20]));
        assert!(routine.windows(8).any(|bytes| {
            bytes
                == [
                    0xAD,
                    0x7D,
                    0x04,
                    0x09,
                    CACHE_UPLOADED_MARKER,
                    0x8D,
                    0x7D,
                    0x04,
                ]
        }));
    }

    #[test]
    fn transition_dispatch_requires_the_observed_render_disabled_window() {
        let dispatch = build_runtime_routines().unwrap().remove(0).bytes;

        assert!(
            dispatch
                .windows(4)
                .any(|bytes| bytes == [0xA5, PPU_MASK_SHADOW, 0xC9, UPLOAD_RENDER_MASK])
        );
        assert!(dispatch.starts_with(&[0x20, 0xD9, 0xC2, 0x20, 0x21, 0xFC, 0xD0, 0x13,]));
        assert!(
            dispatch.windows(7).any(|bytes| {
                bytes == [0xAD, 0x7D, 0x04, 0x29, CACHE_UPLOADED_MARKER, 0xD0, 0x0C]
            })
        );
        assert!(dispatch.ends_with(&[0x20, 0x6A, 0xC3, 0x60]));
    }

    #[test]
    fn gameplay_battle_cache_match_requires_state_and_unordered_participant_pair() {
        assert_eq!(
            gameplay_battle_cache_match_predicate().unwrap(),
            [
                0xA5,
                MAIN_STATE,
                0xC9,
                BATTLE_MAIN_STATE,
                0xF0,
                0x04,
                0xC9,
                ENEMY_INITIATED_BATTLE_MAIN_STATE,
                0xD0,
                0x16,
                0xAD,
                0xF4,
                0x76,
                0xC9,
                CAIN_RECORD_IDENTITY,
                0xF0,
                0x0A,
                0xC9,
                GARUDA_SOLDIER_RECORD_IDENTITY,
                0xD0,
                0x0B,
                0xAD,
                0x15,
                0x77,
                0xC9,
                CAIN_RECORD_IDENTITY,
                0x60,
                0xAD,
                0x15,
                0x77,
                0xC9,
                GARUDA_SOLDIER_RECORD_IDENTITY,
                0x60,
            ]
        );
    }

    #[test]
    fn report_does_not_emit_translation_content_or_private_paths() {
        let report = BattleCacheUploadProbeReport {
            schema: 1,
            source_sha1: EXPECTED_SOURCE_SHA1,
            fixed_workspace_sha1: "fixed".to_owned(),
            dialogue_workspace_sha1: "dialogue".to_owned(),
            output_sha1: "output".to_owned(),
            output_mapper: OUTPUT_MAPPER,
            prg_size: EXPANDED_PRG_SIZE,
            chr_size: 0,
            combination_role: "battle pair",
            gameplay_battle_main_states: [BATTLE_MAIN_STATE, ENEMY_INITIATED_BATTLE_MAIN_STATE],
            cache_participant_record_identities: [
                CAIN_RECORD_IDENTITY,
                GARUDA_SOLDIER_RECORD_IDENTITY,
            ],
            participant_pair_gated: true,
            preserved_active_code_count: 119,
            codebook_glyph_count: 1,
            codebook_assignment_sha1: "assignment".to_owned(),
            cache_mmc3_page: CACHE_MMC3_PAGE,
            cache_page_byte_count: FONT_PAGE_SIZE,
            cache_page_sha1: "page".to_owned(),
            transition_hook_bank: "fixed",
            transition_hook_cpu_address: "0xC191".to_owned(),
            render_disabled_mask: UPLOAD_RENDER_MASK,
            render_disabled_phase: 0,
            upload_after_ppu_mask_write: true,
            nmi_disabled_during_upload: true,
            sequential_ppu_increment_during_upload: true,
            ppu_address_latch_reset_before_upload: true,
            pending_vblank_cleared_before_nmi_restore: true,
            battle_active_flag_address: "0x047D".to_owned(),
            cache_uploaded_marker: CACHE_UPLOADED_MARKER,
            battle_active_nonzero_semantics_preserved: true,
            battle_initializers_clear_cache_marker: true,
            original_prg_bank_restored: true,
            battle_zero_right_page_uses_chr_ram: true,
            non_battle_right_pages_use_natural_selection: true,
            original_chr_preserved: true,
            runtime_routine_count: 6,
            runtime_tracked_write_count: 11,
            translation_text_emitted: false,
            glyph_characters_emitted: false,
            runtime_verified: false,
            release_eligible: false,
            next_gate: "runtime proof",
        };
        let json = serde_json::to_string(&report).unwrap();
        assert!(!json.contains("private/"));
        assert!(!json.contains('한'));
        assert!(!json.contains("korean"));
    }
}
