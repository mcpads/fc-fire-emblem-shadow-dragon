use std::{collections::BTreeSet, fs, path::Path};

use anyhow::{Context, Result, ensure};
use serde::{Deserialize, Serialize};

use crate::{
    battle_runtime_state::{
        BATTLE_COMPOSITION_LIFETIME_START_WRITES,
        SOUND_TEST_BATTLE_COMPOSITION_LIFETIME_START_WRITE,
    },
    font_slots::FONT_PAGE_SIZE,
    mmc5_chr::switchable_bank_file_offset,
    mmc5_prg::count_direct_transfers_to_range,
    rom::{EXPECTED_SOURCE_SHA1, HEADER_SIZE, Rom},
    rp2a03::{Instruction, assemble_at},
    runtime_storage_layout::{
        BATTLE_DIALOGUE_CACHE_KEY_ADDRESS, BATTLE_REMAP_PAIR_TABLE_START,
        BATTLE_REMAP_STATE_ADDRESS, bind_integrated_runtime_storage_layout,
    },
    sha1_hex,
    temporal_surface::load_observed_battle_temporal_evidence,
    tracked::TrackedImage,
};

use super::{
    OUTPUT_MAPPER,
    battle_codebook_plan::{BattleRuntimeRecipeInput, inspect_runtime_recipe_input},
    battle_text_material::{
        COLOR_BIT_MASKS_CPU_ADDRESS, DYNAMIC_ASSIGNMENT_CODE_CPU_ADDRESS,
        DYNAMIC_ASSIGNMENT_CODE_PRG_OFFSET, GLYPH_ATLAS_MMC3_PAGE, PHYSICAL_CODE_TABLE_CPU_ADDRESS,
        PROTECTED_ABSTRACT_COLOR_COUNT, PROTECTED_ABSTRACT_COLORS_CPU_ADDRESS,
        RECIPE_BLOB_MMC3_PAGE, RECIPE_BLOB_PRG_OFFSET, SAFE_ABSTRACT_COLOR_COUNT,
        SAFE_ABSTRACT_COLORS_CPU_ADDRESS, SOURCE_PAGE_MMC3_PAGE,
    },
};

mod dialogue_cache_refresh;
mod dynamic_assignment;
mod runtime;
mod runtime_recipe_fields;

pub(crate) use dialogue_cache_refresh::{
    InstalledDialogueCacheRefresh, match_installed_final_dialogue_cache_refresh,
};
use dialogue_cache_refresh::{
    bind_final_dialogue_cache_refresh_base, bind_final_dialogue_cache_refresh_source,
    install_final_dialogue_cache_refresh,
};
use dynamic_assignment::{
    build_dynamic_assignment_routines, build_dynamic_assignment_routines_for_layout,
};
pub(crate) use runtime::composition_dispatch_for_layout;
use runtime::{
    RuntimeRoutine, battle_central_right_fd_selector_for_layout, build_runtime_routines,
    build_runtime_routines_for_layout, parse_recipe_directories,
    shared_battle_phase_active_for_layout,
};
use runtime_recipe_fields::runtime_recipe_fields;

pub(crate) fn cumulative_battle_composition_dispatch_bytes() -> Result<Vec<u8>> {
    composition_dispatch_for_layout(CUMULATIVE_RUNTIME_LAYOUT)
}

pub(crate) fn cumulative_shared_battle_phase_active_bytes() -> Result<Vec<u8>> {
    shared_battle_phase_active_for_layout(CUMULATIVE_RUNTIME_LAYOUT)
}

pub(crate) fn cumulative_battle_central_right_fd_selector(fallback_target: u16) -> Result<Vec<u8>> {
    battle_central_right_fd_selector_for_layout(CUMULATIVE_RUNTIME_LAYOUT, fallback_target)
}

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
const SOURCE_COMMON_GLYPH_READ: u16 = 0xE57F;
const DISPATCH_ADDRESS: u16 = 0xFAF3;
const COMPOSE_PAGE_ADDRESS: u16 = 0xFB30;
const APPLY_RECIPE_ADDRESS: u16 = 0xFC60;
const APPLY_DIRECTORY_ADDRESS: u16 = 0xFCE0;
const APPLY_PARTICIPANT_ADDRESS: u16 = 0xFD00;
const PROJECT_DIALOGUE_SELECTOR_ADDRESS: u16 = 0xFD30;
const SHARED_BATTLE_PHASE_ACTIVE_ADDRESS: u16 = 0xFD50;
const INITIALIZE_BATTLE_REMAP_ADDRESS: u16 = 0xFD80;
const CLEAR_REMAP_STATE_OUTSIDE_SHARED_BATTLE_ADDRESS: u16 = 0xFE50;
const BATTLE_RIGHT_FD_SELECTOR_ADDRESS: u16 = 0xFEA0;
const BATTLE_CENTRAL_RIGHT_FD_SELECTOR_ADDRESS: u16 = 0xFEE0;
const BATTLE_RIGHT_FE_SELECTOR_ADDRESS: u16 = 0xFF20;
const TEXT_PROJECTION_WRAPPER_ADDRESS: u16 = 0xFE60;
const PROJECT_COLOR_ADDRESS: u16 = 0xFF78;
const FIXED_CAVE_END_ADDRESS: u16 = 0xFFA0;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct BattleCompositionRuntimeLayout {
    pub(crate) dispatch: u16,
    pub(crate) compose_page: u16,
    pub(crate) apply_recipe: u16,
    pub(crate) apply_directory: u16,
    pub(crate) apply_participant: u16,
    pub(crate) project_dialogue_selector: u16,
    pub(crate) shared_battle_phase_active: u16,
    pub(crate) initialize_battle_remap: u16,
    pub(crate) clear_remap_state_outside_shared_battle: u16,
    pub(crate) text_projection_wrapper: u16,
    pub(crate) battle_right_fd_selector: u16,
    pub(crate) battle_central_right_fd_selector: u16,
    pub(crate) battle_right_fe_selector: u16,
    pub(crate) project_color: u16,
    pub(crate) fixed_cave_end: u16,
}

pub(crate) const PROBE_RUNTIME_LAYOUT: BattleCompositionRuntimeLayout =
    BattleCompositionRuntimeLayout {
        dispatch: DISPATCH_ADDRESS,
        compose_page: COMPOSE_PAGE_ADDRESS,
        apply_recipe: APPLY_RECIPE_ADDRESS,
        apply_directory: APPLY_DIRECTORY_ADDRESS,
        apply_participant: APPLY_PARTICIPANT_ADDRESS,
        project_dialogue_selector: PROJECT_DIALOGUE_SELECTOR_ADDRESS,
        shared_battle_phase_active: SHARED_BATTLE_PHASE_ACTIVE_ADDRESS,
        initialize_battle_remap: INITIALIZE_BATTLE_REMAP_ADDRESS,
        clear_remap_state_outside_shared_battle: CLEAR_REMAP_STATE_OUTSIDE_SHARED_BATTLE_ADDRESS,
        text_projection_wrapper: TEXT_PROJECTION_WRAPPER_ADDRESS,
        battle_right_fd_selector: BATTLE_RIGHT_FD_SELECTOR_ADDRESS,
        battle_central_right_fd_selector: BATTLE_CENTRAL_RIGHT_FD_SELECTOR_ADDRESS,
        battle_right_fe_selector: BATTLE_RIGHT_FE_SELECTOR_ADDRESS,
        project_color: PROJECT_COLOR_ADDRESS,
        fixed_cave_end: FIXED_CAVE_END_ADDRESS,
    };

pub(crate) const CUMULATIVE_RUNTIME_LAYOUT: BattleCompositionRuntimeLayout =
    BattleCompositionRuntimeLayout {
        dispatch: 0xFC20,
        compose_page: 0xFC99,
        apply_recipe: 0xFDC2,
        apply_directory: 0xFE3C,
        apply_participant: 0xFE4C,
        project_dialogue_selector: 0xFE75,
        shared_battle_phase_active: 0xFE90,
        initialize_battle_remap: 0xFEB3,
        clear_remap_state_outside_shared_battle: 0xFEC0,
        text_projection_wrapper: 0xFECE,
        battle_right_fd_selector: 0xFEEE,
        battle_central_right_fd_selector: 0xFF1D,
        battle_right_fe_selector: 0xFF43,
        project_color: 0xFF72,
        fixed_cave_end: FIXED_CAVE_END_ADDRESS,
    };

const PPU_MASK_SHADOW: u8 = 0xCC;
const PPU_CONTROL_SHADOW: u16 = 0x00CD;
const PRG_BANK_SHADOW: u8 = 0x29;
const RIGHT_FE_SHADOW: u8 = 0x5C;
const CHR_HIGH_BITS_SHADOW: u8 = 0x52;
const CACHE_UPLOADED_MARKER: u8 = 0x80;
const UPLOAD_RENDER_MASK: u8 = 0x06;
const SELECTED_COLOR_BITMAP_ADDRESS: u16 = 0x07C4;
const SELECTED_COLOR_BITMAP_BYTE_COUNT: u8 = 27;
const CACHED_DIALOGUE_SELECTOR_ADDRESS: u16 = BATTLE_DIALOGUE_CACHE_KEY_ADDRESS;
const REMAP_STATE_ADDRESS: u16 = BATTLE_REMAP_STATE_ADDRESS;
const REMAP_PAIR_COUNT_MASK: u8 = 0x1E;
const REMAP_PAIR_TABLE_ADDRESS: u16 = BATTLE_REMAP_PAIR_TABLE_START;
const MAXIMUM_REMAP_PAIR_COUNT: u8 = 8;

const RECIPE_POINTER_LOW: u8 = 0x00;
const RECIPE_POINTER_HIGH: u8 = 0x01;
const DIRECTORY_POINTER_LOW: u8 = 0x02;
const DIRECTORY_POINTER_HIGH: u8 = 0x03;
const ATLAS_POINTER_LOW: u8 = 0x04;
const ATLAS_POINTER_HIGH: u8 = 0x05;
const RECIPE_PAIR_COUNT: u16 = 0x0006;
const PHYSICAL_TILE_CODE: u8 = 0x07;
const BORROWED_SCRATCH: [u8; 8] = [0, 1, 2, 3, 4, 5, 6, 7];

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
    borrowed_logical_code_count: usize,
    canonical_code_table_byte_count: usize,
    canonical_code_table_cpu_address_hex: String,
    protected_abstract_color_count: usize,
    protected_abstract_colors_cpu_address_hex: String,
    safe_abstract_color_count: usize,
    safe_abstract_colors_cpu_address_hex: String,
    color_bit_mask_byte_count: usize,
    color_bit_masks_cpu_address_hex: String,
    maximum_remap_pair_count: usize,
    glyph_atlas_mmc3_page: u8,
    source_page_mmc3_page: u8,
    recipe_blob_byte_count: usize,
    recipe_blob_sha1: String,
    recipe_blob_mmc3_page: u8,
    dynamic_assignment_source_contract_complete: bool,
    runtime_loader_installed: bool,
    release_eligible: bool,
}

#[derive(Debug, Serialize)]
struct BattleCompositionRuntimeReport {
    schema: u8,
    source_sha1: &'static str,
    base_report_sha1: String,
    base_output_sha1: String,
    temporal_manifest_sha1: String,
    output_sha1: String,
    output_mapper: u16,
    prg_size: usize,
    chr_size: usize,
    observed_runtime_tuple_count: usize,
    runtime_field_count: usize,
    maximum_observed_unique_overlay_count: usize,
    maximum_observed_raw_glyph_reference_count: usize,
    source_page_ppu_write_count: usize,
    maximum_observed_overlay_ppu_write_count: usize,
    maximum_observed_total_ppu_write_count: usize,
    glyph_atlas_mmc3_page: u8,
    source_and_recipe_mmc3_page: u8,
    atlas_cpu_address_hex: String,
    canonical_code_table_cpu_address_hex: String,
    source_page_cpu_address_hex: String,
    recipe_blob_cpu_address_hex: String,
    fixed_cave_start_cpu_address_hex: String,
    fixed_cave_end_cpu_address_exclusive_hex: String,
    fixed_cave_byte_count: usize,
    fixed_runtime_routine_count: usize,
    fixed_runtime_routine_byte_count: usize,
    material_runtime_start_cpu_address_hex: String,
    material_runtime_end_cpu_address_exclusive_hex: String,
    material_runtime_routine_count: usize,
    material_runtime_routine_byte_count: usize,
    total_runtime_routine_byte_count: usize,
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
    observed_tuple_gate_installed: bool,
    modeled_runtime_inputs_enabled: bool,
    selected_color_bitmap_address_hex: String,
    selected_color_bitmap_byte_count: usize,
    remap_state_address_hex: String,
    remap_pair_table_address_hex: String,
    maximum_remap_pair_count: usize,
    remap_overflow_aborts_composition: bool,
    shared_text_projection_hook_address_hex: String,
    shared_text_projection_installed: bool,
    battle_initializer_hook_count: usize,
    battle_initializers_reopen_composition: bool,
    sound_test_battle_initializer_hook_address_hex: String,
    sound_test_shared_battle_activation_installed: bool,
    sound_test_battle_recomposition_boundary_installed: bool,
    battle_zero_right_page_uses_chr_ram_after_success: bool,
    non_battle_right_pages_use_natural_selection: bool,
    dynamic_assignment_source_contract_complete: bool,
    runtime_cycle_budget_measured: bool,
    runtime_verified: bool,
    release_eligible: bool,
    translation_text_emitted: bool,
    glyph_characters_emitted: bool,
    next_gate: &'static str,
}

pub(crate) struct BattleCompositionBuildSummary {
    pub(crate) output_sha1: String,
    pub(crate) report_sha1: String,
    pub(crate) observed_runtime_tuple_count: usize,
    pub(crate) maximum_observed_ppu_write_count: usize,
    pub(crate) runtime_routine_byte_count: usize,
    pub(crate) runtime_tracked_write_count: usize,
}

pub(crate) struct BattleCompositionBuild<'a> {
    pub(crate) source_path: &'a Path,
    pub(crate) temporal_manifest_path: &'a Path,
    pub(crate) base_path: &'a Path,
    pub(crate) base_report_path: &'a Path,
    pub(crate) output_path: &'a Path,
    pub(crate) report_path: &'a Path,
    pub(crate) layout: BattleCompositionRuntimeLayout,
    pub(crate) central_fallback_target: u16,
}

pub(crate) fn build_battle_composition_loader_probe(
    source_path: &Path,
    temporal_manifest_path: &Path,
    base_path: &Path,
    base_report_path: &Path,
    output_path: &Path,
    report_path: &Path,
) -> Result<BattleCompositionBuildSummary> {
    build_battle_composition_runtime(BattleCompositionBuild {
        source_path,
        temporal_manifest_path,
        base_path,
        base_report_path,
        output_path,
        report_path,
        layout: PROBE_RUNTIME_LAYOUT,
        central_fallback_target: SOURCE_PAIR_AWARE_RIGHT_FD_SELECTOR,
    })
}

pub(crate) fn build_battle_composition_runtime(
    build: BattleCompositionBuild<'_>,
) -> Result<BattleCompositionBuildSummary> {
    let BattleCompositionBuild {
        source_path,
        temporal_manifest_path,
        base_path,
        base_report_path,
        output_path,
        report_path,
        layout,
        central_fallback_target,
    } = build;
    bind_integrated_runtime_storage_layout()?;
    let source_rom = Rom::from_path(source_path)?;
    source_rom.verify_supported_japanese()?;
    bind_final_dialogue_cache_refresh_source(&source_rom)?;
    let base = fs::read(base_path).with_context(|| format!("read {}", base_path.display()))?;
    let base_sha1 = sha1_hex(&base);
    let base_report_bytes = fs::read(base_report_path)
        .with_context(|| format!("read {}", base_report_path.display()))?;
    let base_contract: BattleTextRuntimeBaseContract =
        serde_json::from_slice(&base_report_bytes)
            .with_context(|| format!("parse {}", base_report_path.display()))?;
    validate_base_contract(&base_contract, &base_sha1)?;
    let base_rom = Rom::parse(base.clone()).context("parse battle text runtime base")?;
    bind_final_dialogue_cache_refresh_base(&base_rom)?;
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
            staged_participant_identities: sample.runtime_input.staged_participant_identities,
            class_record_identities: sample.runtime_input.class_record_identities,
            item_source_indices: sample.runtime_input.item_source_indices,
            terrain_source_indices: sample.runtime_input.terrain_source_indices,
            dialogue_selector: sample.runtime_input.projected_dialogue_selector,
        })
        .collect::<BTreeSet<_>>();
    ensure!(
        !runtime_inputs.is_empty(),
        "battle composition loader has no observed verification tuples"
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

    let routines = if layout == PROBE_RUNTIME_LAYOUT
        && central_fallback_target == SOURCE_PAIR_AWARE_RIGHT_FD_SELECTOR
    {
        build_runtime_routines(directories)?
    } else {
        build_runtime_routines_for_layout(directories, layout, central_fallback_target)?
    };
    let material_routines = if layout == PROBE_RUNTIME_LAYOUT {
        build_dynamic_assignment_routines(directories)?
    } else {
        build_dynamic_assignment_routines_for_layout(directories, layout)?
    };
    let source_raw_direct_cave_transfer_pattern_count =
        verify_runtime_cave(&base_rom, source_rom.prg(), &routines)?;
    verify_material_runtime_region(&base_rom, &material_routines)?;
    let mut image = TrackedImage::new(base.clone());
    for routine in &routines {
        image.write_expected(
            format!("battle composition {} routine", routine.role),
            expanded_fixed_bank_file_offset(routine.address)?,
            &vec![0xFF; routine.bytes.len()],
            &routine.bytes,
        )?;
    }
    for routine in &material_routines {
        image.write_expected(
            format!("battle composition {} routine", routine.role),
            material_runtime_file_offset(routine.address)?,
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
            &[Instruction::JsrAbsolute(layout.dispatch)],
        )?,
    )?;
    image.write_expected(
        "battle composition shared text projection hook",
        expanded_fixed_bank_file_offset(SOURCE_COMMON_GLYPH_READ)?,
        &assemble_at(
            SOURCE_COMMON_GLYPH_READ,
            &[
                Instruction::LdaIndirectY(RECIPE_POINTER_LOW),
                Instruction::CmpImmediate(0xEF),
            ],
        )?,
        &assemble_at(
            SOURCE_COMMON_GLYPH_READ,
            &[
                Instruction::JsrAbsolute(layout.text_projection_wrapper),
                Instruction::Nop,
            ],
        )?,
    )?;
    redirect_right_selector(
        &mut image,
        "battle composition direct right FD selector",
        SOURCE_RIGHT_FD_SELECTOR,
        layout.battle_right_fd_selector,
        2,
    )?;
    redirect_right_selector(
        &mut image,
        "battle composition right FE selector",
        SOURCE_RIGHT_FE_SELECTOR,
        layout.battle_right_fe_selector,
        4,
    )?;
    redirect_call(
        &mut image,
        "battle composition central right FD selector",
        SOURCE_CENTRAL_RIGHT_FD_CALL,
        central_fallback_target,
        layout.battle_central_right_fd_selector,
    )?;
    redirect_call(
        &mut image,
        "battle composition central FE right FD refresh",
        SOURCE_CENTRAL_FE_FD_REFRESH_CALL,
        central_fallback_target,
        layout.battle_central_right_fd_selector,
    )?;
    install_battle_lifetime_remap_initializers(&mut image, layout)?;
    install_final_dialogue_cache_refresh(&mut image, layout)?;
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
    let fixed_runtime_routine_byte_count = routines
        .iter()
        .map(|routine| routine.bytes.len())
        .sum::<usize>();
    let material_runtime_routine_byte_count = material_routines
        .iter()
        .map(|routine| routine.bytes.len())
        .sum::<usize>();
    let runtime_routine_byte_count =
        fixed_runtime_routine_byte_count + material_runtime_routine_byte_count;
    let output_sha1 = sha1_hex(&output);
    let report = BattleCompositionRuntimeReport {
        schema: 4,
        source_sha1: EXPECTED_SOURCE_SHA1,
        base_report_sha1: sha1_hex(&base_report_bytes),
        base_output_sha1: base_sha1,
        temporal_manifest_sha1: evidence.manifest_sha1,
        output_sha1: output_sha1.clone(),
        output_mapper: output_rom.mapper(),
        prg_size: output_rom.prg().len(),
        chr_size: output_rom.chr().len(),
        observed_runtime_tuple_count: runtime_inputs.len(),
        runtime_field_count: runtime_recipe_fields(directories).len(),
        maximum_observed_unique_overlay_count,
        maximum_observed_raw_glyph_reference_count,
        source_page_ppu_write_count,
        maximum_observed_overlay_ppu_write_count,
        maximum_observed_total_ppu_write_count,
        glyph_atlas_mmc3_page: GLYPH_ATLAS_MMC3_PAGE,
        source_and_recipe_mmc3_page: SOURCE_PAGE_MMC3_PAGE,
        atlas_cpu_address_hex: format!("0x{MATERIAL_ATLAS_CPU_ADDRESS:04X}"),
        canonical_code_table_cpu_address_hex: format!("0x{PHYSICAL_CODE_TABLE_CPU_ADDRESS:04X}"),
        source_page_cpu_address_hex: format!("0x{MATERIAL_SOURCE_PAGE_CPU_ADDRESS:04X}"),
        recipe_blob_cpu_address_hex: format!("0x{MATERIAL_RECIPE_CPU_ADDRESS:04X}"),
        fixed_cave_start_cpu_address_hex: format!("0x{:04X}", layout.dispatch),
        fixed_cave_end_cpu_address_exclusive_hex: format!("0x{:04X}", layout.fixed_cave_end),
        fixed_cave_byte_count: usize::from(layout.fixed_cave_end - layout.dispatch),
        fixed_runtime_routine_count: routines.len(),
        fixed_runtime_routine_byte_count,
        material_runtime_start_cpu_address_hex: format!(
            "0x{DYNAMIC_ASSIGNMENT_CODE_CPU_ADDRESS:04X}"
        ),
        material_runtime_end_cpu_address_exclusive_hex: material_routines
            .last()
            .map(|routine| {
                format!(
                    "0x{:04X}",
                    usize::from(routine.address) + routine.bytes.len()
                )
            })
            .context("battle composition has no material runtime routines")?,
        material_runtime_routine_count: material_routines.len(),
        material_runtime_routine_byte_count,
        total_runtime_routine_byte_count: runtime_routine_byte_count,
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
        observed_tuple_gate_installed: false,
        modeled_runtime_inputs_enabled: true,
        selected_color_bitmap_address_hex: format!("0x{SELECTED_COLOR_BITMAP_ADDRESS:04X}"),
        selected_color_bitmap_byte_count: usize::from(SELECTED_COLOR_BITMAP_BYTE_COUNT),
        remap_state_address_hex: format!("0x{REMAP_STATE_ADDRESS:04X}"),
        remap_pair_table_address_hex: format!("0x{REMAP_PAIR_TABLE_ADDRESS:04X}"),
        maximum_remap_pair_count: usize::from(MAXIMUM_REMAP_PAIR_COUNT),
        remap_overflow_aborts_composition: true,
        shared_text_projection_hook_address_hex: format!("0x{SOURCE_COMMON_GLYPH_READ:04X}"),
        shared_text_projection_installed: true,
        battle_initializer_hook_count: BATTLE_COMPOSITION_LIFETIME_START_WRITES.len(),
        battle_initializers_reopen_composition: true,
        sound_test_battle_initializer_hook_address_hex: format!(
            "0x{:02X}:0x{:04X}",
            SOUND_TEST_BATTLE_COMPOSITION_LIFETIME_START_WRITE.prg_bank,
            SOUND_TEST_BATTLE_COMPOSITION_LIFETIME_START_WRITE.cpu_address
        ),
        sound_test_shared_battle_activation_installed: true,
        sound_test_battle_recomposition_boundary_installed: true,
        battle_zero_right_page_uses_chr_ram_after_success: true,
        non_battle_right_pages_use_natural_selection: true,
        dynamic_assignment_source_contract_complete: true,
        runtime_cycle_budget_measured: false,
        runtime_verified: false,
        release_eligible: false,
        translation_text_emitted: false,
        glyph_characters_emitted: false,
        next_gate: "run the automatic sound-test battle through repeated compositions, then verify ending nonintervention and both caller-specific exit lifetimes",
    };
    let mut report_bytes = serde_json::to_vec_pretty(&report)
        .context("serialize battle composition loader probe report")?;
    report_bytes.push(b'\n');
    write_file(output_path, &output)?;
    write_file(report_path, &report_bytes)?;
    Ok(BattleCompositionBuildSummary {
        output_sha1,
        report_sha1: sha1_hex(&report_bytes),
        observed_runtime_tuple_count: runtime_inputs.len(),
        maximum_observed_ppu_write_count: maximum_observed_total_ppu_write_count,
        runtime_routine_byte_count,
        runtime_tracked_write_count,
    })
}

fn validate_base_contract(
    contract: &BattleTextRuntimeBaseContract,
    actual_sha1: &str,
) -> Result<()> {
    ensure!(
        contract.schema == 4
            && contract.source_sha1 == EXPECTED_SOURCE_SHA1
            && contract.output_sha1 == actual_sha1
            && contract.output_mapper == OUTPUT_MAPPER
            && contract.prg_size == EXPANDED_PRG_SIZE,
        "battle composition loader base report binding changed"
    );
    ensure!(
        contract.stable_color_count == contract.canonical_code_table_byte_count
            && contract.borrowed_logical_code_count
                == contract
                    .stable_color_count
                    .saturating_sub(crate::font_slots::ACTIVE_HANGUL_SLOT_COUNT)
            && contract.canonical_code_table_cpu_address_hex
                == format!("0x{PHYSICAL_CODE_TABLE_CPU_ADDRESS:04X}"),
        "battle composition loader canonical table contract changed"
    );
    ensure!(
        contract.protected_abstract_color_count == PROTECTED_ABSTRACT_COLOR_COUNT
            && contract.protected_abstract_colors_cpu_address_hex
                == format!("0x{PROTECTED_ABSTRACT_COLORS_CPU_ADDRESS:04X}")
            && contract.safe_abstract_color_count == SAFE_ABSTRACT_COLOR_COUNT
            && contract.safe_abstract_colors_cpu_address_hex
                == format!("0x{SAFE_ABSTRACT_COLORS_CPU_ADDRESS:04X}")
            && contract.color_bit_mask_byte_count == 8
            && contract.color_bit_masks_cpu_address_hex
                == format!("0x{COLOR_BIT_MASKS_CPU_ADDRESS:04X}")
            // 보고서 값은 코드북이 실제로 요구하는 충돌 쌍 수이고 상수는 `$07E0..$07EF`
            // 16바이트가 담을 수 있는 런타임 용량이다. 번역이 바뀌면 수요는 줄 수도 있으므로
            // 같기를 요구하지 않고 용량 안에 드는지만 본다.
            && contract.maximum_remap_pair_count <= usize::from(MAXIMUM_REMAP_PAIR_COUNT),
        "battle composition loader dynamic-assignment material contract changed"
    );
    ensure!(
        contract.glyph_atlas_mmc3_page == GLYPH_ATLAS_MMC3_PAGE
            && contract.source_page_mmc3_page == SOURCE_PAGE_MMC3_PAGE
            && contract.recipe_blob_mmc3_page == RECIPE_BLOB_MMC3_PAGE,
        "battle composition loader material page contract changed"
    );
    ensure!(
        contract.dynamic_assignment_source_contract_complete
            && !contract.runtime_loader_installed
            && !contract.release_eligible,
        "battle composition loader expected a source-complete development base"
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

fn verify_material_runtime_region(
    base_rom: &Rom,
    routines: &[dynamic_assignment::MaterialRuntimeRoutine],
) -> Result<()> {
    for routine in routines {
        let start = material_runtime_prg_offset(routine.address)?;
        let end = start
            .checked_add(routine.bytes.len())
            .context("battle material runtime range overflow")?;
        ensure!(
            base_rom
                .prg()
                .get(start..end)
                .context("battle material runtime is outside expanded PRG")?
                .iter()
                .all(|byte| *byte == 0xFF),
            "battle composition {} material region is no longer all FF",
            routine.role
        );
    }
    Ok(())
}

fn install_battle_lifetime_remap_initializers(
    image: &mut TrackedImage,
    layout: BattleCompositionRuntimeLayout,
) -> Result<()> {
    for writer in BATTLE_COMPOSITION_LIFETIME_START_WRITES {
        let bank = writer.prg_bank;
        let address = writer.cpu_address;
        image.write_expected(
            format!("battle remap-state initializer at {bank:02X}:${address:04X}"),
            switchable_bank_file_offset(bank, address)?,
            &assemble_at(
                address,
                &[Instruction::StaAbsolute(
                    crate::battle_runtime_state::BATTLE_RUNTIME_STATE.active_flag_address,
                )],
            )?,
            &assemble_at(
                address,
                &[Instruction::JsrAbsolute(layout.initialize_battle_remap)],
            )?,
        )?;
    }
    Ok(())
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
            crate::mapper165::selector_safety::select_register_instruction(),
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

fn material_runtime_prg_offset(cpu_address: u16) -> Result<usize> {
    ensure!(
        (DYNAMIC_ASSIGNMENT_CODE_CPU_ADDRESS..0xA000).contains(&cpu_address),
        "address is outside the battle material runtime page"
    );
    DYNAMIC_ASSIGNMENT_CODE_PRG_OFFSET
        .checked_add(usize::from(
            cpu_address - DYNAMIC_ASSIGNMENT_CODE_CPU_ADDRESS,
        ))
        .context("battle material runtime PRG offset overflow")
}

fn material_runtime_file_offset(cpu_address: u16) -> Result<usize> {
    HEADER_SIZE
        .checked_add(material_runtime_prg_offset(cpu_address)?)
        .context("battle material runtime file offset overflow")
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
