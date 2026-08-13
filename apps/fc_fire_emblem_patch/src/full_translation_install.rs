use std::{fs, path::Path};

use anyhow::{Context, Result, ensure};
use serde::Serialize;

use crate::{
    chapter_transition::{plan_chapter_titles, plan_transition_labels},
    choice_labels::plan_choice_labels,
    dialogue_assets::{plan_all_main_dialogue_records, validate_main_dialogue_workspace},
    font_slots::{FONT_PAGE_SIZE, FONT_TILE_SIZE},
    item_flow::plan_item_action_labels,
    map_menu::plan_map_menu,
    mapper165::battle_codebook_plan::{
        build_glyph_workset_font_page_pack, plan_glyph_workset_page_upper_bound,
    },
    rom::{EXPECTED_SOURCE_SHA1, Rom},
    sha1_hex,
    text_inventory::{plan_fixed_text, plan_location_name_text},
    unit_names::plan_unit_names,
    unit_ui_text::plan_unit_ui_labels,
};

mod chapter_intro_residency;
mod current_candidate;
mod dialogue_bank_layout;
mod dynamic_composition;
mod dynamic_input_producers;
mod dynamic_inputs;
mod installation_layout;
mod integrated_write_set;
mod runtime_bank_contract;
mod runtime_code;
mod runtime_control_flow;
mod runtime_cursor_storage;
mod runtime_identity;
mod runtime_material;
mod runtime_nmi_contract;
mod runtime_state_storage;

use chapter_intro_residency::plan_chapter_intro_residency;
use current_candidate::{CurrentCandidateInputs, inspect_dialogue_page_pool_capacity};
use dynamic_composition::plan_dialogue_runtime_composition;
use dynamic_input_producers::{DynamicInputProducerPlan, inspect_dynamic_input_producers};
use dynamic_inputs::{plan_dynamic_dialogue_inputs, plan_dynamic_string_remap};
use installation_layout::{InstallationLayoutPlan, plan_installation_layout};
use integrated_write_set::{
    IntegratedWriteSetInputs, IntegratedWriteSetPlan, plan_integrated_write_set,
};
use runtime_code::{plan_dialogue_runtime_code, resolve_request::MaterialLayout};
use runtime_control_flow::{
    DialogueRuntimeControlFlowPlan, RuntimeControlFlowInputs, plan_dialogue_runtime_control_flow,
};
use runtime_identity::{DialogueRuntimeIdentityPlan, plan_dialogue_runtime_identity};
use runtime_material::{
    DialogueRuntimeMaterialPlan, RuntimeMaterialInputs, plan_dialogue_runtime_material,
};
use runtime_state_storage::{DialogueRuntimeStateStoragePlan, plan_dialogue_runtime_state_storage};

/// 대사 런타임 재료 용기가 시작하는 MMC3 8 KiB 페이지다.
const MAIN_DIALOGUE_MATERIAL_FIRST_PAGE: u8 = 0x2C;
/// MMC3 페이지 하나다.
const MMC3_PAGE_BYTE_COUNT: usize = 8 * 1024;
/// 런타임 식별표 헤더의 길이다.
const IDENTITY_HEADER_BYTE_COUNT: usize = 16;
/// 그 뒤 selector 디렉터리의 길이다.
const IDENTITY_SELECTOR_DIRECTORY_BYTE_COUNT: usize = 256;

const REQUIRED_DOMAIN_COUNT: usize = 13;
const REQUIRED_DOMAINS: [&str; REQUIRED_DOMAIN_COUNT] = [
    "chapter_save_offer_label",
    "chapter_titles",
    "choice_labels",
    "class_names",
    "ending_record_labels",
    "enemy_names",
    "item_action_labels",
    "item_names",
    "location_names",
    "main_dialogue",
    "map_menu_labels",
    "unit_names",
    "unit_ui_labels",
];

pub(crate) struct FullTranslationInstallInputs<'a> {
    pub(crate) source_path: &'a Path,
    pub(crate) main_dialogue_workspace_path: &'a Path,
    pub(crate) fixed_text_workspace_path: &'a Path,
    pub(crate) unit_name_localization_path: &'a Path,
    pub(crate) chapter_title_localization_path: &'a Path,
    pub(crate) choice_label_localization_path: &'a Path,
    pub(crate) map_menu_localization_path: &'a Path,
    pub(crate) unit_ui_label_localization_path: &'a Path,
    pub(crate) item_action_label_localization_path: &'a Path,
    pub(crate) transition_label_localization_path: &'a Path,
    pub(crate) location_name_localization_path: &'a Path,
    pub(crate) current_candidate_path: &'a Path,
    pub(crate) current_build_report_path: &'a Path,
    pub(crate) report_path: &'a Path,
    /// 대사 런타임만 실은 확인용 이미지를 쓸 자리다. 배포본이 아니라 탐침이다.
    pub(crate) transport_probe_path: Option<&'a Path>,
}

pub(crate) struct FullTranslationInstallSummary {
    pub(crate) report_sha1: String,
    pub(crate) required_domain_count: usize,
    pub(crate) dialogue_record_count: usize,
    pub(crate) dialogue_page_workset_count: usize,
    pub(crate) dialogue_glyph_count: usize,
    pub(crate) dialogue_maximum_page_slot_demand: usize,
    pub(crate) dialogue_static_page_upper_bound_count: usize,
    pub(crate) dialogue_pointer_write_count: usize,
    pub(crate) dialogue_planned_storage_byte_count: usize,
}

#[derive(Serialize)]
struct FullTranslationInstallReport {
    schema: u8,
    source_sha1: &'static str,
    strategy: &'static str,
    required_domain_count: usize,
    required_domains: [&'static str; REQUIRED_DOMAIN_COUNT],
    translation_inputs: TranslationInputs,
    dialogue_codebook: DialogueCodebook,
    chapter_intro_residency: ChapterIntroResidency,
    dialogue_page_pool: DialoguePagePool,
    installation_layout: InstallationLayoutPlan,
    integrated_write_set: IntegratedWriteSetPlan,
    dialogue_runtime_control_flow: DialogueRuntimeControlFlowPlan,
    dialogue_runtime_state_storage: DialogueRuntimeStateStoragePlan,
    dialogue_runtime_composition: DialogueRuntimeComposition,
    dialogue_storage: DialogueStorage,
    installation_gates: InstallationGates,
    rom_emitted: bool,
    dynamic_verification_started: bool,
    next_gate: &'static str,
}

#[derive(Serialize)]
struct TranslationInputs {
    main_dialogue_record_count: usize,
    fixed_text_physical_entry_count: usize,
    playable_unit_name_count: usize,
    chapter_title_count: usize,
    choice_label_count: usize,
    map_menu_label_count: usize,
    unit_ui_label_count: usize,
    item_action_label_count: usize,
    transition_label_count: usize,
    location_name_count: usize,
    translation_input_complete: bool,
    review_complete: bool,
}

#[derive(Serialize)]
struct DialogueCodebook {
    canonical_record_count: usize,
    page_workset_count: usize,
    unique_workset_count: usize,
    literal_glyph_count: usize,
    unique_glyph_count: usize,
    active_slot_count: usize,
    maximum_workset_slot_demand: usize,
    maximum_page_slot_demand: usize,
    greedy_page_count: usize,
    packing_strategy: &'static str,
    constraint_solver_version: Option<String>,
    constraint_solver_timeout_seconds: Option<u64>,
    packing_sha1: String,
    page_assignment_sha1: String,
    static_page_upper_bound_count: usize,
    static_page_pack_sha1: String,
    canonical_records_connected: bool,
    page_local_bundle_encoding_connected: bool,
    glyph_characters_emitted: bool,
}

#[derive(Serialize)]
struct ChapterIntroResidency {
    chapter_context_count: usize,
    resident_workset_count: usize,
    title_glyph_count: usize,
    fixed_code_count: usize,
    encoded_title_count: usize,
    maximum_augmented_workset_slot_demand: usize,
    fixed_assignment_sha1: String,
    every_title_glyph_has_one_stable_code: bool,
    title_storage_connected: bool,
}

#[derive(Serialize)]
struct DialoguePagePool {
    current_candidate_sha1: String,
    current_chr_page_count: usize,
    first_installable_physical_page: u8,
    superseded_maximum_dialogue_page_count: usize,
    appendable_page_count: usize,
    available_page_count: usize,
    prebuilt_font_page_upper_bound: usize,
    prebuilt_upper_bound_fits_available_pages: bool,
    exact_available_page_fit_decided: bool,
    mapper_capacity_bound: bool,
    current_candidate_bound: bool,
}

#[derive(Serialize)]
struct DialogueRuntimeComposition {
    strategy_selected: bool,
    glyph_atlas_tile_count: usize,
    stored_bytes_per_glyph: usize,
    composed_bytes_per_glyph: usize,
    glyph_atlas_byte_count: usize,
    glyph_atlas_prg_8k_page_count: usize,
    glyph_atlas_sha1: String,
    generated_high_bitplane_is_zero: bool,
    four_by_four_block_count: usize,
    four_by_four_block_index_bit_count: usize,
    four_by_four_block_atlas_byte_count: usize,
    static_page_group_count: usize,
    group_block_directory_byte_count: usize,
    group_block_byte_count: usize,
    group_block_directory_offset: usize,
    static_page_group_overlay_reference_count: usize,
    maximum_static_page_group_overlay_tile_count: usize,
    visible_page_recipe_count: usize,
    visible_page_recipe_reference_count: usize,
    visible_page_overlay_reference_count: usize,
    maximum_visible_page_overlay_tile_count: usize,
    maximum_visible_page_rebuild_ppu_write_count: usize,
    sequential_page_transition_count: usize,
    distinct_visible_page_recipe_transition_count: usize,
    unchanged_visible_page_recipe_transition_count: usize,
    maximum_delta_tile_count: usize,
    maximum_delta_ppu_write_count: usize,
    total_delta_ppu_write_count: usize,
    rebuild_every_visible_page_ppu_write_count: usize,
    initial_rebuild_then_delta_ppu_write_count: usize,
    direct_visible_page_recipe_byte_count: usize,
    bitpacked_visible_page_recipe_byte_count: usize,
    bitmap_and_atlas_index_visible_page_recipe_byte_count: usize,
    direct_delta_recipe_byte_count: usize,
    bitpacked_delta_recipe_byte_count: usize,
    encoded_page_scan_strategy_selected: bool,
    script_scan_covers_dynamic_strings: bool,
    dynamic_string_control_count: usize,
    dynamic_string_page_count: usize,
    dynamic_string_selector_count: usize,
    dynamic_string_domain_count: usize,
    translated_dynamic_page_count: usize,
    preserved_numeric_page_count: usize,
    translated_dynamic_glyph_count: usize,
    combined_dialogue_glyph_count: usize,
    maximum_possible_domain_glyph_count: usize,
    maximum_augmented_workset_slot_demand: usize,
    maximum_rendered_target_glyph_upper_bound: usize,
    mixed_dynamic_domain_page_count: usize,
    dynamic_string_domains_classified: bool,
    dynamic_augmented_worksets_fit: bool,
    canonical_dynamic_code_count: usize,
    remapped_page_group_count: usize,
    dynamic_remap_entry_count: usize,
    non_identity_dynamic_remap_entry_count: usize,
    dense_dynamic_remap_byte_count: usize,
    sparse_dynamic_remap_byte_count: usize,
    sparse_non_identity_dynamic_remap_byte_count: usize,
    selected_dynamic_remap_byte_count: usize,
    selected_dynamic_remap_strategy: &'static str,
    dynamic_remap_material_sha1: String,
    page_selector_remap_flag_sufficient: bool,
    every_translated_dynamic_page_remappable: bool,
    dynamic_string_producers_bound: bool,
    dynamic_string_producers: DynamicInputProducerPlan,
    dense_group_lookup_byte_count: usize,
    record_page_group_selector_byte_count: usize,
    record_selector_directory_byte_count: usize,
    scan_material_byte_count: usize,
    scan_material_sha1: String,
    scan_material_serialized: bool,
    atlas_and_scan_material_byte_count: usize,
    atlas_scan_and_dynamic_remap_byte_count: usize,
    dialogue_runtime_identity: DialogueRuntimeIdentityPlan,
    atlas_scan_remap_and_identity_byte_count: usize,
    runtime_material: DialogueRuntimeMaterialPlan,
    runtime_page_scan_bound_to_control_flow: bool,
    current_battle_glyph_atlas_tile_count: usize,
    current_battle_maximum_ppu_write_count: usize,
    current_battle_runtime_routine_byte_count: usize,
    current_battle_runtime_bound_to_build: bool,
    battle_compositor_is_directly_reusable: bool,
    main_dialogue_page_identity_material_serialized: bool,
    main_dialogue_page_identity_bound: bool,
    main_dialogue_transition_hook_planned: bool,
}

#[derive(Serialize)]
struct DialogueStorage {
    region_count: usize,
    record_count: usize,
    pointer_write_count: usize,
    source_owned_storage_byte_count: usize,
    planned_storage_byte_count: usize,
    remaining_storage_byte_count: usize,
    every_pointer_within_source_owned_regions: bool,
}

#[derive(Serialize)]
struct InstallationGates {
    all_translation_inputs_loaded: bool,
    all_dialogue_records_encoded: bool,
    all_visible_dialogue_text_encoded: bool,
    all_dialogue_pointers_planned: bool,
    all_dialogue_page_code_assignments_found: bool,
    all_dialogue_page_worksets_packed: bool,
    all_chapter_titles_encoded_with_resident_codes: bool,
    all_chapter_title_storage_writes_planned: bool,
    static_prebuilt_dialogue_page_pool_fits: bool,
    dialogue_runtime_composition_planned: bool,
    cross_domain_consumer_writes_planned: bool,
    integrated_candidate_ready: bool,
}

pub(crate) fn plan_full_translation_installation(
    inputs: FullTranslationInstallInputs<'_>,
) -> Result<FullTranslationInstallSummary> {
    let rom = Rom::from_path(inputs.source_path)?;
    rom.verify_supported_japanese()?;
    let page_capacity = inspect_dialogue_page_pool_capacity(CurrentCandidateInputs {
        source_rom: &rom,
        candidate_path: inputs.current_candidate_path,
        build_report_path: inputs.current_build_report_path,
    })?;

    let dialogue_validation =
        validate_main_dialogue_workspace(inputs.source_path, inputs.main_dialogue_workspace_path)?;
    ensure!(
        dialogue_validation.translation_input_complete,
        "all-record main dialogue installation still has untranslated Japanese"
    );
    let dialogue = plan_all_main_dialogue_records(&rom, inputs.main_dialogue_workspace_path)?;
    // 표시 계획은 정규 레코드에서 바로 만든다. 직접 진입과 전이 진입을 나누던 구조는
    // 프리픽스 파서 결함이 만든 것이어서 폐기했다. 의사결정 59번을 따른다.
    let display = crate::dialogue_assets::MainDialogueDisplayPlan::from_canonical_bundle(&dialogue);
    let fixed = plan_fixed_text(&rom, inputs.fixed_text_workspace_path)?;
    let unit_names = plan_unit_names(&rom, inputs.unit_name_localization_path)?;
    let chapter_titles = plan_chapter_titles(&rom, inputs.chapter_title_localization_path)?;
    let choices = plan_choice_labels(&rom, inputs.choice_label_localization_path)?;
    let map_menu = plan_map_menu(&rom, inputs.map_menu_localization_path)?;
    let unit_ui = plan_unit_ui_labels(&rom, inputs.unit_ui_label_localization_path)?;
    let item_actions = plan_item_action_labels(&rom, inputs.item_action_label_localization_path)?;
    let transitions = plan_transition_labels(&rom, inputs.transition_label_localization_path)?;
    let locations = plan_location_name_text(&rom, inputs.location_name_localization_path)?;

    ensure!(
        dialogue.record_ids.len() == 504
            && fixed.entries.len() == 272
            && unit_names.entries.len() == 52
            && chapter_titles.entry_count == 25
            && chapter_titles.translated_entry_count == 25
            && choices.entries.len() == 2
            && map_menu.entry_count == 6
            && map_menu.translated_entry_count == 6
            && unit_ui.entry_count == 25
            && item_actions.entry_count == 4
            && transitions.save_offer.entry_count == 1
            && transitions.ending_record.entry_count == 1
            && locations.entries.len() == 24,
        "full translation installation input population changed"
    );

    let baseline_dynamic_inputs = plan_dynamic_dialogue_inputs(
        &display,
        &fixed.entries,
        &unit_names.entries,
        &locations.entries,
    )?;
    let baseline_codebook =
        plan_glyph_workset_page_upper_bound(&baseline_dynamic_inputs.augmented_worksets)?;
    let baseline_encoded = dialogue.encoded_by_page_groups(
        &baseline_codebook.workset_page_indices,
        &baseline_codebook.page_assignments,
    )?;
    ensure!(
        baseline_encoded.regions.len() == 11 && baseline_encoded.pointer_writes.len() == 517,
        "baseline dialogue encoded layout changed"
    );

    let dynamic_inputs = plan_dynamic_dialogue_inputs(
        &display,
        &fixed.entries,
        &unit_names.entries,
        &locations.entries,
    )?;
    let dynamic_input_producers = inspect_dynamic_input_producers(&rom)?;
    let dynamic_string_producers_bound =
        dynamic_input_producers.every_record_selector_route_bound();
    let chapter_intro_residency = plan_chapter_intro_residency(
        &rom,
        &display,
        &chapter_titles,
        &dynamic_inputs.augmented_worksets,
    )?;
    let codebook =
        plan_glyph_workset_page_upper_bound(&chapter_intro_residency.augmented_worksets)?;
    let dynamic_remap = plan_dynamic_string_remap(&dynamic_inputs, &codebook)?;
    ensure!(
        codebook.workset_count == display.page_worksets.len()
            && codebook.workset_page_indices.len() == display.page_worksets.len(),
        "dialogue codebook lost visible page worksets"
    );
    let source_font_page = rom
        .chr()
        .get(..FONT_PAGE_SIZE)
        .context("source dialogue font page is outside CHR")?;
    let font_page_pack = build_glyph_workset_font_page_pack(source_font_page, &codebook)?;
    ensure!(
        font_page_pack.len() == codebook.page_assignments.len() * FONT_PAGE_SIZE,
        "dialogue font page pack length changed after rasterization"
    );
    let composition = plan_dialogue_runtime_composition(
        &display,
        &codebook,
        &dynamic_remap,
        source_font_page,
        &font_page_pack,
    )?;
    // 전이 미러를 함께 내던 인코딩은 이중 진입과 함께 폐기했다. 정규 레코드의
    // 원천 소유 구간과 포인터만 낸다. 의사결정 59번을 따른다.
    let encoded_display = dialogue
        .encoded_by_page_groups(&codebook.workset_page_indices, &codebook.page_assignments)?;
    let source_owned_storage_byte_count = baseline_encoded
        .regions
        .iter()
        .map(|region| region.source_storage.len())
        .sum::<usize>();
    let planned_storage_byte_count = encoded_display
        .regions
        .iter()
        .map(|region| region.used_storage_byte_count)
        .sum::<usize>();
    let current_candidate = Rom::from_path(inputs.current_candidate_path)?;
    ensure!(
        sha1_hex(current_candidate.data()) == page_capacity.current_candidate_sha1,
        "relocated dialogue bank plan and page-pool plan use different current candidates"
    );
    ensure!(
        planned_storage_byte_count <= source_owned_storage_byte_count,
        "complete dialogue encoded storage exceeds its source-owned regions"
    );
    let atlas_scan_and_dynamic_remap_byte_count = composition.glyph_atlas.len()
        + composition.scan_material.len()
        + dynamic_remap.selected_dense_material.len();
    let runtime_identity = plan_dialogue_runtime_identity(rom.data(), &display)?;
    let atlas_scan_remap_and_identity_byte_count = atlas_scan_and_dynamic_remap_byte_count
        .checked_add(runtime_identity.material.len())
        .context("dialogue runtime material length overflow")?;
    let mut runtime_material = plan_dialogue_runtime_material(RuntimeMaterialInputs {
        glyph_atlas: &composition.glyph_atlas,
        page_scan: &composition.scan_material,
        dynamic_remap: &dynamic_remap.selected_dense_material,
        runtime_identity: &runtime_identity.material,
    })?;
    // 실행 코드는 자료 배치가 끝난 뒤에 조립한다. 놓일 주소가 자료 길이에서 나오기
    // 때문이다. 코드 길이는 그 주소에 영향을 주지 않으므로 한 번에 정해진다.
    // 실행 코드는 자료 배치가 끝난 뒤에 조립한다. 읽을 표들이 어느 페이지의 어느
    // 주소에 놓이는지가 배치에서 나오기 때문이다.
    let atlas_offset = runtime_material.glyph_atlas_offset()?;
    let scan_offset = runtime_material.section_offset("page_scan")?;
    let identity_offset = runtime_material.section_offset("runtime_identity")?;
    let page = |offset: usize| -> Result<u8> {
        Ok(MAIN_DIALOGUE_MATERIAL_FIRST_PAGE
            + u8::try_from(offset / MMC3_PAGE_BYTE_COUNT)
                .context("material page index overflow")?)
    };
    let window = |offset: usize| -> Result<u16> {
        u16::try_from(0x8000 + offset % MMC3_PAGE_BYTE_COUNT)
            .context("material CPU address does not fit the 8000 window")
    };
    let layout = MaterialLayout {
        identity_page: page(identity_offset)?,
        identity_selector_directory: window(identity_offset + IDENTITY_HEADER_BYTE_COUNT)?,
        identity_table_descriptors: window(
            identity_offset + IDENTITY_HEADER_BYTE_COUNT + IDENTITY_SELECTOR_DIRECTORY_BYTE_COUNT,
        )?,
        scan_page: page(scan_offset)?,
        page_selectors: window(scan_offset)?,
        record_directory: window(scan_offset + composition.record_page_group_selector_byte_count)?,
        group_directory: window(scan_offset + composition.group_block_directory_offset)?,
        group_block_container_base: u16::try_from(
            scan_offset
                + composition.group_block_directory_offset
                + composition.group_block_directory_byte_count,
        )
        .context("page group block base does not fit a 16-bit container offset")?,
        container_first_page: MAIN_DIALOGUE_MATERIAL_FIRST_PAGE,
    };
    let dialogue_runtime_code = plan_dialogue_runtime_code(
        &rom,
        &current_candidate,
        runtime_material.runtime_code_cpu_start()?,
        page(atlas_offset)?,
        runtime_material.runtime_code_mmc3_page(),
        layout,
    )?;
    for routine in &dialogue_runtime_code.code_routines {
        runtime_material.place_runtime_code(routine.address, &routine.bytes)?;
    }

    let runtime_state_storage = plan_dialogue_runtime_state_storage(&rom)?;
    ensure!(
        runtime_state_storage.selection_complete(),
        "dialogue runtime-state storage selection is incomplete"
    );
    let selected_runtime_state_cpu_range = runtime_state_storage
        .selected_cpu_range_hex()
        .context("dialogue runtime-state selection has no CPU range")?;
    let emitted_hook_roles = dialogue_runtime_code.hook_roles();
    let runtime_control_flow = plan_dialogue_runtime_control_flow(RuntimeControlFlowInputs {
        source: &rom,
        candidate: &current_candidate,
        runtime_code_offset: runtime_material.runtime_code_offset,
        runtime_code_byte_count: runtime_material.material.len()
            - runtime_material.runtime_code_offset,
        selected_runtime_state_cpu_range,
        runtime_code_emitted: true,
        emitted_hook_roles: &emitted_hook_roles,
        chr_restore_callee_cycles: dialogue_runtime_code.chr_restore_callee_cycles,
    })?;
    let installation_layout = plan_installation_layout(
        &current_candidate,
        &page_capacity,
        runtime_material.material.len(),
    )?;
    let (installed_image, integrated_write_set) =
        plan_integrated_write_set(IntegratedWriteSetInputs {
            candidate: &current_candidate,
            encoded_dialogue: &encoded_display,
            dialogue_runtime_material: &runtime_material.material,
            dialogue_runtime_code: &dialogue_runtime_code,
            encoded_chapter_titles: &chapter_intro_residency.encoded_titles,
            required_domains: &REQUIRED_DOMAINS,
        })?;
    let translation_input_complete = dialogue_validation.translation_input_complete;
    let review_complete = dialogue_validation.review_complete
        && fixed.review_complete
        && unit_names.review_complete
        && chapter_titles.review_complete
        && choices.review_complete
        && map_menu.review_complete
        && unit_ui.review_complete
        && item_actions.review_complete
        && transitions.save_offer.review_complete
        && transitions.ending_record.review_complete
        && locations.review_complete
        && translation_input_complete;
    let next_gate = if translation_input_complete && runtime_state_storage.selection_complete() {
        "cold-boot the exact emitted probe through chapter one and verify the resident Korean title, original EA marker, readable dialogue body, map, window, and portrait on the first presented ready frame before advancing to the chapter-seven multi-page route"
    } else if translation_input_complete {
        "close the exact volatile runtime-state storage selection against source access, queue, save/load, and battle lifetimes; do not emit or run a partial ROM"
    } else {
        "author Korean for every untranslated Japanese line before recalculating glyph lifetimes; do not emit or run a partial ROM"
    };

    let report = FullTranslationInstallReport {
        schema: 10,
        source_sha1: EXPECTED_SOURCE_SHA1,
        strategy: "install all remaining translation domains in one cumulative candidate, run complete static gates, then run consumer-path dynamic regression on that same ROM",
        required_domain_count: REQUIRED_DOMAIN_COUNT,
        required_domains: REQUIRED_DOMAINS,
        translation_inputs: TranslationInputs {
            main_dialogue_record_count: dialogue.record_ids.len(),
            fixed_text_physical_entry_count: fixed.entries.len(),
            playable_unit_name_count: unit_names.entries.len(),
            chapter_title_count: chapter_titles.entry_count,
            choice_label_count: choices.entries.len(),
            map_menu_label_count: map_menu.entry_count,
            unit_ui_label_count: unit_ui.entry_count,
            item_action_label_count: item_actions.entry_count,
            transition_label_count: transitions.save_offer.entry_count
                + transitions.ending_record.entry_count,
            location_name_count: locations.entries.len(),
            translation_input_complete,
            review_complete,
        },
        dialogue_codebook: DialogueCodebook {
            canonical_record_count: display.canonical_record_count,
            page_workset_count: display.page_worksets.len(),
            unique_workset_count: codebook.unique_workset_count,
            literal_glyph_count: display.unique_glyphs().len(),
            unique_glyph_count: codebook.glyph_count,
            active_slot_count: crate::font_slots::ACTIVE_HANGUL_SLOT_COUNT,
            maximum_workset_slot_demand: codebook.maximum_workset_slot_demand,
            maximum_page_slot_demand: codebook.maximum_page_slot_demand,
            greedy_page_count: codebook.greedy_page_count,
            packing_strategy: codebook.packing_strategy,
            constraint_solver_version: codebook.constraint_solver_version,
            constraint_solver_timeout_seconds: codebook.constraint_solver_timeout_seconds,
            packing_sha1: codebook.packing_sha1,
            page_assignment_sha1: codebook.page_assignment_sha1,
            static_page_upper_bound_count: codebook.page_assignments.len(),
            static_page_pack_sha1: sha1_hex(&font_page_pack),
            canonical_records_connected: true,
            page_local_bundle_encoding_connected: true,
            glyph_characters_emitted: false,
        },
        chapter_intro_residency: ChapterIntroResidency {
            chapter_context_count: chapter_intro_residency.chapter_context_count,
            resident_workset_count: chapter_intro_residency.resident_workset_count,
            title_glyph_count: chapter_intro_residency.title_glyph_count,
            fixed_code_count: chapter_intro_residency.fixed_code_count,
            encoded_title_count: chapter_intro_residency.encoded_titles.len(),
            maximum_augmented_workset_slot_demand: chapter_intro_residency
                .maximum_augmented_workset_slot_demand,
            fixed_assignment_sha1: chapter_intro_residency.fixed_assignment_sha1.clone(),
            every_title_glyph_has_one_stable_code: true,
            title_storage_connected: true,
        },
        dialogue_page_pool: DialoguePagePool {
            current_candidate_sha1: page_capacity.current_candidate_sha1,
            current_chr_page_count: page_capacity.current_chr_page_count,
            first_installable_physical_page: page_capacity.first_installable_physical_page,
            superseded_maximum_dialogue_page_count: page_capacity
                .superseded_maximum_dialogue_page_count,
            appendable_page_count: page_capacity.appendable_page_count,
            available_page_count: page_capacity.available_page_count,
            prebuilt_font_page_upper_bound: codebook.page_assignments.len(),
            prebuilt_upper_bound_fits_available_pages: codebook.page_assignments.len()
                <= page_capacity.available_page_count,
            exact_available_page_fit_decided: false,
            mapper_capacity_bound: true,
            current_candidate_bound: true,
        },
        installation_layout,
        integrated_write_set,
        dialogue_runtime_control_flow: runtime_control_flow,
        dialogue_runtime_state_storage: runtime_state_storage,
        dialogue_runtime_composition: DialogueRuntimeComposition {
            strategy_selected: true,
            glyph_atlas_tile_count: composition.glyph_atlas_tile_count,
            stored_bytes_per_glyph: 8,
            composed_bytes_per_glyph: FONT_TILE_SIZE,
            glyph_atlas_byte_count: composition.glyph_atlas.len(),
            glyph_atlas_prg_8k_page_count: composition.glyph_atlas.len().div_ceil(8 * 1024),
            glyph_atlas_sha1: sha1_hex(&composition.glyph_atlas),
            generated_high_bitplane_is_zero: true,
            group_block_directory_byte_count: composition.group_block_directory_byte_count,
            group_block_byte_count: composition.group_block_byte_count,
            group_block_directory_offset: composition.group_block_directory_offset,
            four_by_four_block_count: composition.four_by_four_block_count,
            four_by_four_block_index_bit_count: composition.four_by_four_block_index_bit_count,
            four_by_four_block_atlas_byte_count: composition.four_by_four_block_atlas_byte_count,
            static_page_group_count: composition.static_page_group_count,
            static_page_group_overlay_reference_count: composition
                .static_page_group_overlay_reference_count,
            maximum_static_page_group_overlay_tile_count: composition
                .maximum_static_page_group_overlay_tile_count,
            visible_page_recipe_count: composition.visible_page_recipe_count,
            visible_page_recipe_reference_count: composition.visible_page_recipe_reference_count,
            visible_page_overlay_reference_count: composition.visible_page_overlay_reference_count,
            maximum_visible_page_overlay_tile_count: composition
                .maximum_visible_page_overlay_tile_count,
            maximum_visible_page_rebuild_ppu_write_count: FONT_PAGE_SIZE
                + composition.maximum_visible_page_overlay_tile_count * FONT_TILE_SIZE,
            sequential_page_transition_count: composition.sequential_page_transition_count,
            distinct_visible_page_recipe_transition_count: composition
                .distinct_visible_page_recipe_transition_count,
            unchanged_visible_page_recipe_transition_count: composition
                .unchanged_visible_page_recipe_transition_count,
            maximum_delta_tile_count: composition.maximum_delta_tile_count,
            maximum_delta_ppu_write_count: composition.maximum_delta_ppu_write_count,
            total_delta_ppu_write_count: composition.total_delta_ppu_write_count,
            rebuild_every_visible_page_ppu_write_count: composition
                .rebuild_every_visible_page_ppu_write_count,
            initial_rebuild_then_delta_ppu_write_count: composition
                .initial_rebuild_then_delta_ppu_write_count,
            direct_visible_page_recipe_byte_count: composition
                .direct_visible_page_recipe_byte_count,
            bitpacked_visible_page_recipe_byte_count: composition
                .bitpacked_visible_page_recipe_byte_count,
            bitmap_and_atlas_index_visible_page_recipe_byte_count: composition
                .bitmap_and_atlas_index_visible_page_recipe_byte_count,
            direct_delta_recipe_byte_count: composition.direct_delta_recipe_byte_count,
            bitpacked_delta_recipe_byte_count: composition.bitpacked_delta_recipe_byte_count,
            encoded_page_scan_strategy_selected: false,
            script_scan_covers_dynamic_strings: false,
            dynamic_string_control_count: composition.dynamic_string_control_count,
            dynamic_string_page_count: composition.dynamic_string_page_count,
            dynamic_string_selector_count: composition.dynamic_string_selector_count,
            dynamic_string_domain_count: dynamic_inputs.declared_domain_count,
            translated_dynamic_page_count: dynamic_inputs.translated_dynamic_page_count,
            preserved_numeric_page_count: dynamic_inputs.preserved_numeric_page_count,
            translated_dynamic_glyph_count: dynamic_inputs.translated_dynamic_glyph_count,
            combined_dialogue_glyph_count: dynamic_inputs.combined_dialogue_glyph_count,
            maximum_possible_domain_glyph_count: dynamic_inputs.maximum_possible_domain_glyph_count,
            maximum_augmented_workset_slot_demand: dynamic_inputs
                .maximum_augmented_workset_slot_demand,
            maximum_rendered_target_glyph_upper_bound: dynamic_inputs
                .maximum_rendered_target_glyph_upper_bound,
            mixed_dynamic_domain_page_count: dynamic_inputs.mixed_dynamic_domain_page_count,
            dynamic_string_domains_classified: dynamic_inputs.every_dynamic_control_classified,
            dynamic_augmented_worksets_fit: dynamic_inputs.every_augmented_workset_fits,
            canonical_dynamic_code_count: dynamic_remap.canonical_code_count,
            remapped_page_group_count: dynamic_remap.remapped_page_group_count,
            dynamic_remap_entry_count: dynamic_remap.remap_entry_count,
            non_identity_dynamic_remap_entry_count: dynamic_remap.non_identity_remap_entry_count,
            dense_dynamic_remap_byte_count: dynamic_remap.dense_remap_byte_count,
            sparse_dynamic_remap_byte_count: dynamic_remap.sparse_remap_byte_count,
            sparse_non_identity_dynamic_remap_byte_count: dynamic_remap
                .sparse_non_identity_remap_byte_count,
            selected_dynamic_remap_byte_count: dynamic_remap.selected_dense_remap_byte_count,
            selected_dynamic_remap_strategy: dynamic_remap.selected_strategy,
            dynamic_remap_material_sha1: dynamic_remap.remap_material_sha1,
            page_selector_remap_flag_sufficient: dynamic_remap.page_selector_remap_flag_sufficient,
            every_translated_dynamic_page_remappable: dynamic_remap
                .every_translated_dynamic_page_remappable,
            dynamic_string_producers_bound,
            dynamic_string_producers: dynamic_input_producers,
            dense_group_lookup_byte_count: composition.dense_group_lookup_byte_count,
            record_page_group_selector_byte_count: composition
                .record_page_group_selector_byte_count,
            record_selector_directory_byte_count: composition.record_selector_directory_byte_count,
            scan_material_byte_count: composition.scan_material_byte_count,
            scan_material_sha1: composition.scan_material_sha1,
            scan_material_serialized: true,
            atlas_and_scan_material_byte_count: composition.glyph_atlas.len()
                + composition.scan_material_byte_count,
            atlas_scan_and_dynamic_remap_byte_count,
            dialogue_runtime_identity: runtime_identity,
            atlas_scan_remap_and_identity_byte_count,
            runtime_material,
            runtime_page_scan_bound_to_control_flow: false,
            current_battle_glyph_atlas_tile_count: page_capacity.battle_glyph_atlas_tile_count,
            current_battle_maximum_ppu_write_count: page_capacity.battle_maximum_ppu_write_count,
            current_battle_runtime_routine_byte_count: page_capacity
                .battle_runtime_routine_byte_count,
            current_battle_runtime_bound_to_build: page_capacity.battle_runtime_bound_to_build,
            battle_compositor_is_directly_reusable: false,
            main_dialogue_page_identity_material_serialized: true,
            main_dialogue_page_identity_bound: false,
            main_dialogue_transition_hook_planned: true,
        },
        dialogue_storage: DialogueStorage {
            region_count: encoded_display.regions.len(),
            record_count: dialogue.record_ids.len(),
            pointer_write_count: encoded_display.pointer_writes.len(),
            source_owned_storage_byte_count,
            planned_storage_byte_count,
            remaining_storage_byte_count: source_owned_storage_byte_count
                - planned_storage_byte_count,
            every_pointer_within_source_owned_regions: true,
        },
        installation_gates: InstallationGates {
            all_translation_inputs_loaded: translation_input_complete,
            all_dialogue_records_encoded: true,
            all_visible_dialogue_text_encoded: true,
            all_dialogue_pointers_planned: true,
            all_dialogue_page_code_assignments_found: true,
            all_dialogue_page_worksets_packed: true,
            all_chapter_titles_encoded_with_resident_codes: true,
            all_chapter_title_storage_writes_planned: true,
            static_prebuilt_dialogue_page_pool_fits: codebook.page_assignments.len()
                <= page_capacity.available_page_count,
            dialogue_runtime_composition_planned: true,
            cross_domain_consumer_writes_planned: false,
            integrated_candidate_ready: false,
        },
        rom_emitted: false,
        dynamic_verification_started: false,
        next_gate,
    };
    let mut report_bytes =
        serde_json::to_vec_pretty(&report).context("serialize full translation install plan")?;
    report_bytes.push(b'\n');
    if let Some(parent) = inputs.report_path.parent() {
        fs::create_dir_all(parent).with_context(|| format!("create {}", parent.display()))?;
    }
    fs::write(inputs.report_path, &report_bytes)
        .with_context(|| format!("write {}", inputs.report_path.display()))?;

    // 대사 런타임과 그 재료만 실은 확인용 이미지다. 나머지 도메인이 아직 없으므로
    // 배포본이 아니고, 이름과 별도 요청으로 그 구분을 남긴다. vblank 불변 조건과
    // 합성 결과는 실행해 봐야만 확인되고, 그 확인이 나머지 도메인을 기다릴 이유가 없다.
    if let Some(path) = inputs.transport_probe_path {
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).with_context(|| format!("create {}", parent.display()))?;
        }
        fs::write(path, &installed_image).with_context(|| format!("write {}", path.display()))?;
    }

    Ok(FullTranslationInstallSummary {
        report_sha1: sha1_hex(&report_bytes),
        required_domain_count: REQUIRED_DOMAIN_COUNT,
        dialogue_record_count: dialogue.record_ids.len(),
        dialogue_page_workset_count: display.page_worksets.len(),
        dialogue_glyph_count: codebook.glyph_count,
        dialogue_maximum_page_slot_demand: codebook.maximum_page_slot_demand,
        dialogue_static_page_upper_bound_count: codebook.page_assignments.len(),
        dialogue_pointer_write_count: encoded_display.pointer_writes.len(),
        dialogue_planned_storage_byte_count: planned_storage_byte_count,
    })
}
