use std::{collections::BTreeSet, fs, path::Path};

use anyhow::{Context, Result, ensure};
use serde::{Deserialize, Serialize};

use crate::{
    font_slots::FONT_PAGE_SIZE,
    mmc5_prg::count_direct_transfers_to_range,
    rom::{EXPECTED_SOURCE_SHA1, HEADER_SIZE, Rom},
    rp2a03::{Instruction, assemble_at},
    sha1_hex,
    temporal_surface::load_observed_battle_temporal_evidence,
    tracked::TrackedImage,
};

use super::{
    OUTPUT_MAPPER,
    battle_codebook_plan::{BattleRuntimeRecipeInput, inspect_runtime_recipe_input},
    battle_text_cache_probe::{
        GLYPH_ATLAS_MMC3_PAGE, PHYSICAL_CODE_TABLE_CPU_ADDRESS, RECIPE_BLOB_MMC3_PAGE,
        RECIPE_BLOB_PRG_OFFSET, SOURCE_PAGE_MMC3_PAGE,
    },
};

mod runtime;

use runtime::{RuntimeRoutine, build_runtime_routines, parse_recipe_directories};

const EXPANDED_PRG_SIZE: usize = 512 * 1024;
const FIXED_BANK_SIZE: usize = 16 * 1024;
const MATERIAL_RECIPE_CPU_ADDRESS: u16 = 0xB000;
const MATERIAL_SOURCE_PAGE_CPU_ADDRESS: u16 = 0xA000;
const MATERIAL_ATLAS_CPU_ADDRESS: u16 = 0x8000;

const SOURCE_NMI_UPLOAD_HOOK: u16 = 0xC191;
const SOURCE_NMI_INPUT_SCAN: u16 = 0xC2D9;
const SOURCE_NMI_SCROLL_RESTORE: u16 = 0xC36A;
const SOURCE_RIGHT_FD_SELECTOR: u16 = 0xFA80;
const SOURCE_RIGHT_FE_SELECTOR: u16 = 0xFAA0;
const SOURCE_PAIR_AWARE_RIGHT_FD_SELECTOR: u16 = 0xFAC0;
const SOURCE_CENTRAL_RIGHT_FD_CALL: u16 = 0xC9C2;
const SOURCE_CENTRAL_FE_FD_REFRESH_CALL: u16 = 0xFABB;
const SOURCE_PRG_BANK_SELECTOR: u16 = 0xFA20;

const DISPATCH_ADDRESS: u16 = 0xFAF3;
const COMPOSE_PAGE_ADDRESS: u16 = 0xFB20;
const APPLY_RECIPE_ADDRESS: u16 = 0xFC50;
const APPLY_DIRECTORY_ADDRESS: u16 = 0xFCD0;
const APPLY_PARTICIPANT_ADDRESS: u16 = 0xFCF0;
const PROJECT_DIALOGUE_SELECTOR_ADDRESS: u16 = 0xFD20;
const ADMITTED_TUPLE_PREDICATE_ADDRESS: u16 = 0xFD40;
const BATTLE_RIGHT_FD_SELECTOR_ADDRESS: u16 = 0xFEA0;
const BATTLE_CENTRAL_RIGHT_FD_SELECTOR_ADDRESS: u16 = 0xFEE0;
const BATTLE_RIGHT_FE_SELECTOR_ADDRESS: u16 = 0xFF20;
const FIXED_CAVE_END_ADDRESS: u16 = 0xFFA0;

const PPU_MASK_SHADOW: u8 = 0xCC;
const PPU_CONTROL_SHADOW: u16 = 0x00CD;
const PRG_BANK_SHADOW: u8 = 0x51;
const RIGHT_FE_SHADOW: u8 = 0x5C;
const CHR_HIGH_BITS_SHADOW: u8 = 0x52;
const MAIN_STATE_ADDRESS: u16 = 0x0084;
const PLAYER_INITIATED_BATTLE_STATE: u8 = 0x16;
const ENEMY_INITIATED_BATTLE_STATE: u8 = 0x32;
const BATTLE_ACTIVE_FLAG: u16 = 0x047D;
const CACHE_UPLOADED_MARKER: u8 = 0x80;
const UPLOAD_RENDER_MASK: u8 = 0x06;

const RECIPE_POINTER_LOW: u8 = 0x00;
const RECIPE_POINTER_HIGH: u8 = 0x01;
const DIRECTORY_POINTER_LOW: u8 = 0x02;
const DIRECTORY_POINTER_HIGH: u8 = 0x03;
const ATLAS_POINTER_LOW: u8 = 0x04;
const ATLAS_POINTER_HIGH: u8 = 0x05;
const RECIPE_PAIR_COUNT: u16 = 0x0006;
const PHYSICAL_TILE_CODE: u8 = 0x07;
const BORROWED_SCRATCH: [u8; 8] = [0, 1, 2, 3, 4, 5, 6, 7];

const RUNTIME_FIELD_ADDRESSES: [u16; 8] = [
    0x0304, 0x0305, 0x0306, 0x0307, 0x0320, 0x0321, 0x0322, 0x0323,
];

#[derive(Debug, Deserialize)]
struct BattleTextRuntimeBaseContract {
    schema: u8,
    source_sha1: String,
    output_sha1: String,
    output_mapper: u16,
    prg_size: usize,
    observed_runtime_tuple_count: usize,
    maximum_observed_overlay_count: usize,
    stable_color_count: usize,
    physical_code_table_byte_count: usize,
    physical_code_table_cpu_address_hex: String,
    glyph_atlas_mmc3_page: u8,
    source_page_mmc3_page: u8,
    recipe_blob_byte_count: usize,
    recipe_blob_sha1: String,
    recipe_blob_mmc3_page: u8,
    physical_assignment_catalog_complete: bool,
    runtime_loader_installed: bool,
    release_eligible: bool,
}

#[derive(Debug, Serialize)]
struct BattleCompositionLoaderProbeReport {
    schema: u8,
    source_sha1: &'static str,
    base_report_sha1: String,
    base_output_sha1: String,
    temporal_manifest_sha1: String,
    output_sha1: String,
    output_mapper: u16,
    prg_size: usize,
    chr_size: usize,
    admitted_runtime_tuple_count: usize,
    runtime_field_count: usize,
    maximum_observed_unique_overlay_count: usize,
    maximum_observed_raw_glyph_reference_count: usize,
    source_page_ppu_write_count: usize,
    maximum_observed_overlay_ppu_write_count: usize,
    maximum_observed_total_ppu_write_count: usize,
    glyph_atlas_mmc3_page: u8,
    source_and_recipe_mmc3_page: u8,
    atlas_cpu_address_hex: String,
    physical_code_table_cpu_address_hex: String,
    source_page_cpu_address_hex: String,
    recipe_blob_cpu_address_hex: String,
    fixed_cave_start_cpu_address_hex: String,
    fixed_cave_end_cpu_address_exclusive_hex: String,
    fixed_cave_byte_count: usize,
    runtime_routine_count: usize,
    runtime_routine_byte_count: usize,
    runtime_tracked_write_count: usize,
    source_raw_direct_cave_transfer_pattern_count: usize,
    raw_direct_transfer_patterns_are_code_proof: bool,
    borrowed_scratch_byte_count: usize,
    borrowed_scratch_restored: bool,
    ppu_address_latch_reset_before_composition: bool,
    sequential_ppu_increment_during_composition: bool,
    rendering_disabled_during_composition: bool,
    nmi_disabled_during_composition: bool,
    pending_vblank_cleared_before_nmi_restore: bool,
    source_prg_bank_restored_from_shadow: bool,
    runtime_recipe_duplicates_replayed: bool,
    source_bound_dialogue_projection_installed: bool,
    admitted_tuple_gate_installed: bool,
    battle_zero_right_page_uses_chr_ram_after_success: bool,
    non_battle_right_pages_use_natural_selection: bool,
    physical_assignment_catalog_complete: bool,
    runtime_cycle_budget_measured: bool,
    runtime_verified: bool,
    release_eligible: bool,
    translation_text_emitted: bool,
    glyph_characters_emitted: bool,
    next_gate: &'static str,
}

pub(crate) struct BattleCompositionLoaderProbeSummary {
    pub(crate) output_sha1: String,
    pub(crate) report_sha1: String,
    pub(crate) admitted_runtime_tuple_count: usize,
    pub(crate) maximum_observed_ppu_write_count: usize,
    pub(crate) runtime_routine_byte_count: usize,
}

pub(crate) fn build_battle_composition_loader_probe(
    source_path: &Path,
    temporal_manifest_path: &Path,
    base_path: &Path,
    base_report_path: &Path,
    output_path: &Path,
    report_path: &Path,
) -> Result<BattleCompositionLoaderProbeSummary> {
    let source_rom = Rom::from_path(source_path)?;
    source_rom.verify_supported_japanese()?;
    let base = fs::read(base_path).with_context(|| format!("read {}", base_path.display()))?;
    let base_sha1 = sha1_hex(&base);
    let base_report_bytes = fs::read(base_report_path)
        .with_context(|| format!("read {}", base_report_path.display()))?;
    let base_contract: BattleTextRuntimeBaseContract =
        serde_json::from_slice(&base_report_bytes)
            .with_context(|| format!("parse {}", base_report_path.display()))?;
    validate_base_contract(&base_contract, &base_sha1)?;
    let base_rom = Rom::parse(base.clone()).context("parse battle text runtime base")?;
    ensure!(
        base_rom.mapper() == OUTPUT_MAPPER && base_rom.prg().len() == EXPANDED_PRG_SIZE,
        "battle composition loader base layout changed"
    );

    let recipe_blob = base_rom
        .prg()
        .get(RECIPE_BLOB_PRG_OFFSET..RECIPE_BLOB_PRG_OFFSET + base_contract.recipe_blob_byte_count)
        .context("battle composition recipe blob is outside expanded PRG")?;
    ensure!(
        sha1_hex(recipe_blob) == base_contract.recipe_blob_sha1,
        "battle composition recipe blob hash changed"
    );
    let directories = parse_recipe_directories(recipe_blob)?;

    let evidence = load_observed_battle_temporal_evidence(source_path, temporal_manifest_path)?;
    let runtime_inputs = evidence
        .samples
        .iter()
        .map(|sample| BattleRuntimeRecipeInput {
            participant_record_identities: sample.runtime_input.participant_record_identities,
            class_record_identities: sample.runtime_input.class_record_identities,
            item_source_indices: sample.runtime_input.item_source_indices,
            terrain_source_indices: sample.runtime_input.terrain_source_indices,
            dialogue_selector: sample.runtime_input.projected_dialogue_selector,
        })
        .collect::<BTreeSet<_>>();
    ensure!(
        !runtime_inputs.is_empty(),
        "battle composition loader has no admitted runtime tuples"
    );
    ensure!(
        runtime_inputs.len() == base_contract.observed_runtime_tuple_count,
        "battle composition loader tuple count disagrees with its base"
    );
    let stats = runtime_inputs
        .iter()
        .map(|input| inspect_runtime_recipe_input(recipe_blob, *input))
        .collect::<Result<Vec<_>>>()?;
    ensure!(
        stats.iter().all(|stats| stats.recipe_count == 10),
        "battle composition loader lost a recipe family"
    );
    let maximum_observed_unique_overlay_count = stats
        .iter()
        .map(|stats| stats.unique_overlay_count)
        .max()
        .unwrap_or(0);
    let maximum_observed_raw_glyph_reference_count = stats
        .iter()
        .map(|stats| stats.glyph_reference_count)
        .max()
        .unwrap_or(0);
    ensure!(
        maximum_observed_unique_overlay_count == base_contract.maximum_observed_overlay_count,
        "battle composition loader overlay count disagrees with its base"
    );

    let routines = build_runtime_routines(&runtime_inputs, directories)?;
    let source_raw_direct_cave_transfer_pattern_count =
        verify_runtime_cave(&base_rom, source_rom.prg(), &routines)?;
    let mut image = TrackedImage::new(base.clone());
    for routine in &routines {
        image.write_expected(
            format!("battle composition {} routine", routine.role),
            expanded_fixed_bank_file_offset(routine.address)?,
            &vec![0xFF; routine.bytes.len()],
            &routine.bytes,
        )?;
    }
    image.write_expected(
        "battle composition NMI post-mask hook",
        expanded_fixed_bank_file_offset(SOURCE_NMI_UPLOAD_HOOK)?,
        &assemble_at(
            SOURCE_NMI_UPLOAD_HOOK,
            &[Instruction::JsrAbsolute(SOURCE_NMI_INPUT_SCAN)],
        )?,
        &assemble_at(
            SOURCE_NMI_UPLOAD_HOOK,
            &[Instruction::JsrAbsolute(DISPATCH_ADDRESS)],
        )?,
    )?;
    redirect_right_selector(
        &mut image,
        "battle composition direct right FD selector",
        SOURCE_RIGHT_FD_SELECTOR,
        BATTLE_RIGHT_FD_SELECTOR_ADDRESS,
        2,
    )?;
    redirect_right_selector(
        &mut image,
        "battle composition right FE selector",
        SOURCE_RIGHT_FE_SELECTOR,
        BATTLE_RIGHT_FE_SELECTOR_ADDRESS,
        4,
    )?;
    redirect_call(
        &mut image,
        "battle composition central right FD selector",
        SOURCE_CENTRAL_RIGHT_FD_CALL,
        SOURCE_PAIR_AWARE_RIGHT_FD_SELECTOR,
        BATTLE_CENTRAL_RIGHT_FD_SELECTOR_ADDRESS,
    )?;
    redirect_call(
        &mut image,
        "battle composition central FE right FD refresh",
        SOURCE_CENTRAL_FE_FD_REFRESH_CALL,
        SOURCE_PAIR_AWARE_RIGHT_FD_SELECTOR,
        BATTLE_CENTRAL_RIGHT_FD_SELECTOR_ADDRESS,
    )?;
    image.verify_all_changes_tracked(&base)?;
    let runtime_tracked_write_count = image.writes().len();
    let output = image.into_data();
    let output_rom = Rom::parse(output.clone()).context("parse battle composition loader probe")?;
    ensure!(
        output_rom.mapper() == OUTPUT_MAPPER
            && output_rom.prg().len() == EXPANDED_PRG_SIZE
            && output_rom.chr() == base_rom.chr(),
        "battle composition loader changed the base media layout"
    );

    let source_page_ppu_write_count = FONT_PAGE_SIZE;
    let maximum_observed_overlay_ppu_write_count = maximum_observed_raw_glyph_reference_count * 16;
    let maximum_observed_total_ppu_write_count =
        source_page_ppu_write_count + maximum_observed_overlay_ppu_write_count;
    let runtime_routine_byte_count = routines.iter().map(|routine| routine.bytes.len()).sum();
    let output_sha1 = sha1_hex(&output);
    let report = BattleCompositionLoaderProbeReport {
        schema: 1,
        source_sha1: EXPECTED_SOURCE_SHA1,
        base_report_sha1: sha1_hex(&base_report_bytes),
        base_output_sha1: base_sha1,
        temporal_manifest_sha1: evidence.manifest_sha1,
        output_sha1: output_sha1.clone(),
        output_mapper: output_rom.mapper(),
        prg_size: output_rom.prg().len(),
        chr_size: output_rom.chr().len(),
        admitted_runtime_tuple_count: runtime_inputs.len(),
        runtime_field_count: RUNTIME_FIELD_ADDRESSES.len() + 1,
        maximum_observed_unique_overlay_count,
        maximum_observed_raw_glyph_reference_count,
        source_page_ppu_write_count,
        maximum_observed_overlay_ppu_write_count,
        maximum_observed_total_ppu_write_count,
        glyph_atlas_mmc3_page: GLYPH_ATLAS_MMC3_PAGE,
        source_and_recipe_mmc3_page: SOURCE_PAGE_MMC3_PAGE,
        atlas_cpu_address_hex: format!("0x{MATERIAL_ATLAS_CPU_ADDRESS:04X}"),
        physical_code_table_cpu_address_hex: format!("0x{PHYSICAL_CODE_TABLE_CPU_ADDRESS:04X}"),
        source_page_cpu_address_hex: format!("0x{MATERIAL_SOURCE_PAGE_CPU_ADDRESS:04X}"),
        recipe_blob_cpu_address_hex: format!("0x{MATERIAL_RECIPE_CPU_ADDRESS:04X}"),
        fixed_cave_start_cpu_address_hex: format!("0x{DISPATCH_ADDRESS:04X}"),
        fixed_cave_end_cpu_address_exclusive_hex: format!("0x{FIXED_CAVE_END_ADDRESS:04X}"),
        fixed_cave_byte_count: usize::from(FIXED_CAVE_END_ADDRESS - DISPATCH_ADDRESS),
        runtime_routine_count: routines.len(),
        runtime_routine_byte_count,
        runtime_tracked_write_count,
        source_raw_direct_cave_transfer_pattern_count,
        raw_direct_transfer_patterns_are_code_proof: false,
        borrowed_scratch_byte_count: BORROWED_SCRATCH.len(),
        borrowed_scratch_restored: true,
        ppu_address_latch_reset_before_composition: true,
        sequential_ppu_increment_during_composition: true,
        rendering_disabled_during_composition: true,
        nmi_disabled_during_composition: true,
        pending_vblank_cleared_before_nmi_restore: true,
        source_prg_bank_restored_from_shadow: true,
        runtime_recipe_duplicates_replayed: true,
        source_bound_dialogue_projection_installed: true,
        admitted_tuple_gate_installed: true,
        battle_zero_right_page_uses_chr_ram_after_success: true,
        non_battle_right_pages_use_natural_selection: true,
        physical_assignment_catalog_complete: false,
        runtime_cycle_budget_measured: false,
        runtime_verified: false,
        release_eligible: false,
        translation_text_emitted: false,
        glyph_characters_emitted: false,
        next_gate: "cold-run every admitted sound-test and gameplay tuple through irregular temporal captures, measure the blank transition, and verify automatic exit restoration",
    };
    let mut report_bytes = serde_json::to_vec_pretty(&report)
        .context("serialize battle composition loader probe report")?;
    report_bytes.push(b'\n');
    write_file(output_path, &output)?;
    write_file(report_path, &report_bytes)?;
    Ok(BattleCompositionLoaderProbeSummary {
        output_sha1,
        report_sha1: sha1_hex(&report_bytes),
        admitted_runtime_tuple_count: runtime_inputs.len(),
        maximum_observed_ppu_write_count: maximum_observed_total_ppu_write_count,
        runtime_routine_byte_count,
    })
}

fn validate_base_contract(
    contract: &BattleTextRuntimeBaseContract,
    actual_sha1: &str,
) -> Result<()> {
    ensure!(
        contract.schema == 1
            && contract.source_sha1 == EXPECTED_SOURCE_SHA1
            && contract.output_sha1 == actual_sha1
            && contract.output_mapper == OUTPUT_MAPPER
            && contract.prg_size == EXPANDED_PRG_SIZE,
        "battle composition loader base report binding changed"
    );
    ensure!(
        contract.stable_color_count == contract.physical_code_table_byte_count
            && contract.physical_code_table_cpu_address_hex
                == format!("0x{PHYSICAL_CODE_TABLE_CPU_ADDRESS:04X}"),
        "battle composition loader physical table contract changed"
    );
    ensure!(
        contract.glyph_atlas_mmc3_page == GLYPH_ATLAS_MMC3_PAGE
            && contract.source_page_mmc3_page == SOURCE_PAGE_MMC3_PAGE
            && contract.recipe_blob_mmc3_page == RECIPE_BLOB_MMC3_PAGE,
        "battle composition loader material page contract changed"
    );
    ensure!(
        !contract.physical_assignment_catalog_complete
            && !contract.runtime_loader_installed
            && !contract.release_eligible,
        "battle composition loader expected a gated development base"
    );
    Ok(())
}

fn verify_runtime_cave(
    base_rom: &Rom,
    source_prg: &[u8],
    routines: &[RuntimeRoutine],
) -> Result<usize> {
    let mut direct_transfer_count = 0;
    for routine in routines {
        let end = routine
            .address
            .checked_add(u16::try_from(routine.bytes.len())?)
            .context("battle composition routine range overflow")?;
        let start_offset = expanded_fixed_bank_file_offset(routine.address)? - HEADER_SIZE;
        let end_offset = expanded_fixed_bank_file_offset(end)? - HEADER_SIZE;
        ensure!(
            base_rom.prg()[start_offset..end_offset]
                .iter()
                .all(|byte| *byte == 0xFF),
            "battle composition {} cave is no longer all FF",
            routine.role
        );
        direct_transfer_count += count_direct_transfers_to_range(source_prg, routine.address, end)?;
    }
    Ok(direct_transfer_count)
}

fn redirect_right_selector(
    image: &mut TrackedImage,
    label: &str,
    source_address: u16,
    replacement_address: u16,
    mapper_register: u8,
) -> Result<()> {
    let expected = natural_right_selector(source_address, mapper_register)?;
    let mut replacement = assemble_at(
        source_address,
        &[Instruction::JmpAbsolute(replacement_address)],
    )?;
    replacement.resize(expected.len(), 0xEA);
    image.write_expected(
        label,
        expanded_fixed_bank_file_offset(source_address)?,
        &expected,
        &replacement,
    )
}

fn natural_right_selector(address: u16, mapper_register: u8) -> Result<Vec<u8>> {
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

fn expanded_fixed_bank_file_offset(cpu_address: u16) -> Result<usize> {
    ensure!(cpu_address >= 0xC000, "address is outside the fixed bank");
    Ok(HEADER_SIZE + EXPANDED_PRG_SIZE - FIXED_BANK_SIZE + usize::from(cpu_address - 0xC000))
}

fn read_u16(bytes: &[u8], offset: usize) -> u16 {
    u16::from_le_bytes([bytes[offset], bytes[offset + 1]])
}

fn write_file(path: &Path, bytes: &[u8]) -> Result<()> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).with_context(|| format!("create {}", parent.display()))?;
    }
    fs::write(path, bytes).with_context(|| format!("write {}", path.display()))
}

#[cfg(test)]
mod tests;
