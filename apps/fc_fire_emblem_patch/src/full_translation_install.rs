use std::{
    collections::{BTreeMap, BTreeSet},
    fs,
    path::Path,
};

use anyhow::{Context, Result, ensure};
use serde::Serialize;

use crate::{
    chapter_transition::{plan_chapter_titles, plan_transition_labels},
    choice_labels::plan_choice_labels,
    dialogue_assets::{plan_all_main_dialogue_records, validate_main_dialogue_workspace},
    dialogue_inventory::inspect_main_dialogue_graph,
    fixed_menu_labels::plan_fixed_menu_labels,
    fixed_string_consumers::{FixedStringConsumerCensus, inspect_fixed_string_consumers},
    fixed_string_ownership::{FixedStringOwnershipReport, inspect_fixed_string_ownership},
    font_slots::{FONT_PAGE_SIZE, FONT_TILE_SIZE},
    front_end_menu::plan_front_end_menu,
    item_flow::plan_item_action_labels,
    localization::OptionsLocalization,
    map_menu::plan_map_menu,
    mapper165::{
        CarriedBattleDomainInputs, CarriedBattleDomainPreservation, CarriedUiDomainInputs,
        CarriedUiDomainPreservation,
        battle_codebook_plan::{
            build_glyph_workset_font_page_pack, plan_glyph_workset_page_upper_bound,
            verify_glyph_workset_font_page_pack,
        },
        bind_installed_front_end_mapper_register,
        font_pair_projection::RightFontPageProjection,
        inspect_carried_battle_domains, inspect_carried_ui_domains,
    },
    rom::{EXPECTED_SOURCE_SHA1, Rom},
    sha1_hex,
    text_inventory::{plan_fixed_text, plan_location_name_text},
    unit_names::plan_unit_names,
    unit_ui_text::{plan_unit_ui_labels, preserved_unit_ui_display_codes},
};

mod chapter_intro_residency;
mod chapter_save_projection;
mod choice_residency;
mod cold_request_presentation;
mod consumer_catalog;
mod consumer_codebook;
mod consumer_installation;
mod cross_domain_material;
mod current_candidate;
mod dialogue_bank_layout;
mod dynamic_composition;
mod dynamic_input_producers;
mod dynamic_inputs;
mod ending_record_projection;
mod final_runtime_evidence;
mod fixed_ui_projection;
mod front_end_result_residency;
mod installation_layout;
mod integrated_write_set;
mod main_dialogue_route_population;
mod resident_glyph_assignment;
mod runtime_bank_contract;
mod runtime_code;
mod runtime_control_flow;
mod runtime_cursor_storage;
mod runtime_identity;
mod runtime_material;
mod runtime_nmi_contract;
mod runtime_state_storage;
mod transition_residency;

use chapter_intro_residency::plan_chapter_intro_residency;
use chapter_save_projection::{
    ChapterSaveProjectionInputs, ChapterSaveProjectionPlan, plan_chapter_save_projection,
};
use choice_residency::{ChoiceResidencyPlan, plan_choice_residency};
use cold_request_presentation::plan_cold_request_presentation_page;
use consumer_catalog::{ConsumerCatalogInputs, ConsumerCatalogPlan, plan_consumer_catalog};
use consumer_codebook::{ConsumerCodebookInputs, ConsumerCodebookPlan, plan_consumer_codebook};
use consumer_installation::{
    ConsumerInstallationInputs, ConsumerInstallationPlan, plan_consumer_installation,
};
use cross_domain_material::{CrossDomainMaterialInputs, plan_cross_domain_material};
use current_candidate::{CurrentCandidateInputs, inspect_dialogue_page_pool_capacity};
use dynamic_composition::plan_dialogue_runtime_composition;
use dynamic_input_producers::{DynamicInputProducerPlan, inspect_dynamic_input_producers};
use dynamic_inputs::{
    DynamicProducerEncodingPlan, bind_dynamic_producer_encoding, bind_dynamic_string_page_codes,
    plan_dynamic_dialogue_inputs,
};
use ending_record_projection::{
    EndingRecordProjectionInputs, EndingRecordProjectionPlan, plan_ending_record_projection,
};
use final_runtime_evidence::{FinalArtifactRuntimeEvidence, load_final_artifact_runtime_evidence};
use fixed_ui_projection::{
    FixedUiProjectionInputs, FixedUiProjectionPlan, plan_fixed_ui_projection,
};
use front_end_result_residency::{
    FrontEndResultResidencyPlan, plan_front_end_result_menu_residency,
    plan_front_end_result_residency,
};
use installation_layout::{InstallationLayoutPlan, plan_installation_layout};
use integrated_write_set::{
    IntegratedWriteSetInputs, IntegratedWriteSetPlan, plan_integrated_write_set,
};
use main_dialogue_route_population::{
    MainDialogueRoutePopulationPlan, plan_main_dialogue_route_population,
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
use transition_residency::plan_transition_residency;

/// 대사 런타임 재료 용기가 시작하는 MMC3 8 KiB 페이지다.
const MAIN_DIALOGUE_MATERIAL_FIRST_PAGE: u8 = 0x2C;
/// MMC3 페이지 하나다.
const MMC3_PAGE_BYTE_COUNT: usize = 8 * 1024;
/// 런타임 식별표 헤더의 길이다.
const IDENTITY_HEADER_BYTE_COUNT: usize = 16;
/// 그 뒤 selector 디렉터리의 길이다.
const IDENTITY_SELECTOR_DIRECTORY_BYTE_COUNT: usize = 256;
const FRONT_END_SAVE_SUMMARY_UNIT_SOURCE_INDEX: usize = 0;
const FRONT_END_SAVE_SUMMARY_CLASS_SOURCE_INDEX: usize = 20;

const REQUIRED_DOMAIN_COUNT: usize = 14;
const REQUIRED_DOMAINS: [&str; REQUIRED_DOMAIN_COUNT] = [
    "chapter_save_offer_label",
    "chapter_titles",
    "choice_labels",
    "class_names",
    "ending_record_labels",
    "enemy_names",
    "fixed_menu_labels",
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
    pub(crate) battle_dialogue_workspace_path: &'a Path,
    pub(crate) fixed_text_workspace_path: &'a Path,
    pub(crate) options_localization_path: &'a Path,
    pub(crate) roster_localization_path: &'a Path,
    pub(crate) front_end_menu_localization_path: &'a Path,
    pub(crate) class_profile_localization_path: &'a Path,
    pub(crate) title_graphics_localization_path: &'a Path,
    pub(crate) title_logo_asset_path: &'a Path,
    pub(crate) unit_name_localization_path: &'a Path,
    pub(crate) chapter_title_localization_path: &'a Path,
    pub(crate) choice_label_localization_path: &'a Path,
    pub(crate) map_menu_localization_path: &'a Path,
    pub(crate) unit_ui_label_localization_path: &'a Path,
    pub(crate) item_action_label_localization_path: &'a Path,
    pub(crate) fixed_menu_label_localization_path: &'a Path,
    pub(crate) transition_label_localization_path: &'a Path,
    pub(crate) location_name_localization_path: &'a Path,
    pub(crate) current_candidate_path: &'a Path,
    pub(crate) current_build_report_path: &'a Path,
    pub(crate) final_runtime_evidence_path: Option<&'a Path>,
    pub(crate) report_path: &'a Path,
    /// 선언된 설치 계획의 기술 게이트를 통과한 통합 이미지를 명시적으로 쓸 자리다.
    pub(crate) output_path: Option<&'a Path>,
}

pub(crate) const FULL_TRANSLATION_REPORT_SCHEMA: u8 = 21;

pub(crate) struct FullTranslationInstallSummary {
    pub(crate) report_sha1: String,
    pub(crate) declared_installation_domain_count: usize,
    pub(crate) dialogue_record_count: usize,
    pub(crate) dialogue_page_workset_count: usize,
    pub(crate) dialogue_glyph_count: usize,
    pub(crate) dialogue_maximum_page_slot_demand: usize,
    pub(crate) dialogue_static_page_upper_bound_count: usize,
    pub(crate) dialogue_pointer_write_count: usize,
    pub(crate) dialogue_planned_storage_byte_count: usize,
    pub(crate) integrated_image_sha1: String,
}

#[derive(Serialize)]
struct FullTranslationInstallReport {
    schema: u8,
    source_sha1: &'static str,
    strategy: &'static str,
    declared_installation_domain_count: usize,
    declared_installation_domains: [&'static str; REQUIRED_DOMAIN_COUNT],
    translation_inputs: TranslationInputs,
    fixed_string_consumers: FixedStringConsumerCensus,
    fixed_string_ownership: FixedStringOwnershipReport,
    dialogue_codebook: DialogueCodebook,
    chapter_intro_residency: ChapterIntroResidency,
    choice_residency: ChoiceResidencyPlan,
    front_end_result_residency: FrontEndResultResidencyPlan,
    chapter_save_projection: ChapterSaveProjectionPlan,
    ending_record_projection: EndingRecordProjectionPlan,
    dialogue_page_pool: DialoguePagePool,
    cross_domain_material: cross_domain_material::CrossDomainMaterialPlan,
    consumer_codebook: ConsumerCodebookPlan,
    consumer_catalog: ConsumerCatalogPlan,
    fixed_ui_projection: FixedUiProjectionPlan,
    installation_layout: InstallationLayoutPlan,
    integrated_write_set: IntegratedWriteSetPlan,
    dialogue_runtime_control_flow_static_contract: DialogueRuntimeControlFlowPlan,
    dialogue_runtime_state_storage_source_reservation: DialogueRuntimeStateStoragePlan,
    dialogue_runtime_composition: DialogueRuntimeComposition,
    main_dialogue_route_population: MainDialogueRoutePopulationPlan,
    consumer_installation: ConsumerInstallationPlan,
    carried_ui_domain_preservation: CarriedUiDomainPreservation,
    carried_battle_domain_preservation: CarriedBattleDomainPreservation,
    final_artifact_runtime_evidence: FinalArtifactRuntimeEvidence,
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
    front_end_menu_label_count: usize,
    playable_unit_name_count: usize,
    chapter_title_count: usize,
    choice_label_count: usize,
    map_menu_label_count: usize,
    unit_ui_label_count: usize,
    item_action_label_count: usize,
    fixed_menu_label_count: usize,
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
    static_page_pack_preserves_every_workset_code: bool,
    canonical_records_connected: bool,
    page_local_bundle_encoding_connected: bool,
    glyph_characters_encoded_into_installed_runtime_atlas: bool,
    transition_stable_lifetime_count: usize,
    multi_record_transition_stable_lifetime_count: usize,
    maximum_transition_stable_lifetime_record_count: usize,
    maximum_transition_stable_lifetime_workset_count: usize,
    maximum_transition_stable_lifetime_slot_demand: usize,
    every_resident_transition_uses_one_codebook: bool,
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
    cold_request_presentation_page_count: usize,
    remaining_available_page_count: usize,
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
    dialogue_codebook_glyph_count: usize,
    additional_cross_domain_glyph_count: usize,
    glyph_atlas_covers_every_required_domain_glyph: bool,
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
    resident_group_transition_count: usize,
    resident_group_change_count: usize,
    resident_group_reuse_count: usize,
    maximum_resident_group_overlay_tile_count: usize,
    maximum_resident_group_overlay_frame_count: usize,
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
    translated_dynamic_page_group_count: usize,
    dynamic_page_code_identity_entry_count: usize,
    dynamic_page_code_material_byte_count: usize,
    dynamic_page_code_strategy: &'static str,
    dynamic_page_code_material_sha1: String,
    canonical_dynamic_codes_are_page_physical_codes: bool,
    page_selectors_use_plain_group_indices: bool,
    every_translated_dynamic_page_directly_consumable: bool,
    dynamic_string_producers_bound: bool,
    dynamic_string_producers: DynamicInputProducerPlan,
    dynamic_producer_encoding: DynamicProducerEncodingPlan,
    dense_group_lookup_byte_count: usize,
    record_page_group_selector_byte_count: usize,
    record_selector_directory_byte_count: usize,
    scan_material_byte_count: usize,
    scan_material_sha1: String,
    scan_material_serialized: bool,
    atlas_and_scan_material_byte_count: usize,
    dialogue_runtime_identity: DialogueRuntimeIdentityPlan,
    atlas_scan_and_identity_byte_count: usize,
    runtime_material_layout_and_assembly: DialogueRuntimeMaterialPlan,
    runtime_page_scan_bound_to_assembled_control_flow: bool,
    current_battle_glyph_atlas_tile_count: usize,
    current_battle_maximum_ppu_write_count: usize,
    current_battle_runtime_routine_byte_count: usize,
    current_battle_runtime_bound_to_build: bool,
    battle_compositor_is_directly_reusable: bool,
    main_dialogue_page_identity_material_serialized: bool,
    main_dialogue_page_identity_bound_to_assembled_control_flow: bool,
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
    all_resident_dialogue_transitions_use_one_codebook: bool,
    all_chapter_titles_encoded_with_resident_codes: bool,
    all_chapter_title_storage_writes_planned: bool,
    cold_request_presentation_page_planned: bool,
    cold_request_presentation_write_planned: bool,
    dialogue_runtime_composition_planned: bool,
    all_declared_consumer_writes_planned: bool,
    all_carried_ui_domains_reinspected: bool,
    all_carried_battle_domains_reinspected: bool,
    declared_plan_technical_installation_complete: bool,
    declared_consumer_runtime_observation_complete: bool,
}

pub(crate) fn plan_full_translation_installation(
    inputs: FullTranslationInstallInputs<'_>,
) -> Result<FullTranslationInstallSummary> {
    let rom = Rom::from_path(inputs.source_path)?;
    rom.verify_supported_japanese()?;
    let fixed_string_consumers = inspect_fixed_string_consumers(&rom)?;
    ensure!(
        fixed_string_consumers.records.len() == 72
            && fixed_string_consumers.call_sites.len() == 49
            && fixed_string_consumers.direct_producer_bound_indices.len() == 56,
        "fixed-string source population did not reach its declared consumer boundary"
    );
    let fixed_string_ownership = inspect_fixed_string_ownership(&fixed_string_consumers)?;
    let page_capacity = inspect_dialogue_page_pool_capacity(CurrentCandidateInputs {
        source_rom: &rom,
        candidate_path: inputs.current_candidate_path,
        build_report_path: inputs.current_build_report_path,
    })?;
    let current_candidate = Rom::from_path(inputs.current_candidate_path)?;
    ensure!(
        sha1_hex(current_candidate.data()) == page_capacity.current_candidate_sha1,
        "relocated dialogue bank plan and page-pool plan use different current candidates"
    );
    let front_end_mapper_register = bind_installed_front_end_mapper_register(&current_candidate)?;
    let front_end_mapper_route = RightFontPageProjection::for_screen_roles(
        rom.chr(),
        &["new_game_choice", "save_slot_selection"],
        0,
    )?
    .encode_mapper_route(front_end_mapper_register)?;

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
    let front_end = plan_front_end_menu(&rom, inputs.front_end_menu_localization_path)?;
    let unit_names = plan_unit_names(&rom, inputs.unit_name_localization_path)?;
    let chapter_titles = plan_chapter_titles(&rom, inputs.chapter_title_localization_path)?;
    let choices = plan_choice_labels(&rom, inputs.choice_label_localization_path)?;
    let map_menu = plan_map_menu(&rom, inputs.map_menu_localization_path)?;
    let unit_ui = plan_unit_ui_labels(&rom, inputs.unit_ui_label_localization_path)?;
    let item_actions = plan_item_action_labels(&rom, inputs.item_action_label_localization_path)?;
    let fixed_menu_labels =
        plan_fixed_menu_labels(&rom, inputs.fixed_menu_label_localization_path)?;
    let options_localization = OptionsLocalization::from_path(inputs.options_localization_path)?;
    let validated_options = options_localization.validate()?;
    let options_glyph_codes = options_localization
        .glyphs
        .iter()
        .map(|glyph| (glyph.character, glyph.code))
        .collect::<BTreeMap<_, _>>();
    ensure!(
        validated_options.entries.len() == 3
            && options_glyph_codes.len() == options_localization.glyphs.len(),
        "options localization no longer has three labels with one code per glyph"
    );
    let transitions = plan_transition_labels(&rom, inputs.transition_label_localization_path)?;
    let locations = plan_location_name_text(&rom, inputs.location_name_localization_path)?;

    ensure!(
        dialogue.record_ids.len() == 504
            && fixed.entries.len() == 273
            && front_end.entries.len() == 7
            && unit_names.entries.len() == 53
            && chapter_titles.entry_count == 25
            && chapter_titles.translated_entry_count == 25
            && choices.entries.len() == 2
            && map_menu.entry_count == 8
            && map_menu.translated_entry_count == 8
            && unit_ui.entry_count == 25
            && item_actions.entry_count == 4
            && fixed_menu_labels.entry_count == 7
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
    let mut dynamic_producer_encoding = bind_dynamic_producer_encoding(
        &current_candidate,
        &dynamic_inputs,
        &fixed.entries,
        &unit_names.entries,
        &locations.entries,
    )?;
    let chapter_intro_residency = plan_chapter_intro_residency(
        &rom,
        &display,
        &chapter_titles,
        &dynamic_inputs.augmented_worksets,
    )?;
    let front_end_result_menu_residency = plan_front_end_result_menu_residency(
        &rom,
        &current_candidate,
        &display,
        &front_end,
        &chapter_intro_residency.augmented_worksets,
    )?;
    let choice_residency = plan_choice_residency(
        &rom,
        &display,
        &choices,
        &front_end_result_menu_residency.augmented_worksets,
    )?;
    let source_font_page = rom
        .chr()
        .get(..FONT_PAGE_SIZE)
        .context("source dialogue font page is outside CHR")?;
    let cold_request_presentation = plan_cold_request_presentation_page(
        source_font_page,
        page_capacity.first_installable_physical_page,
    )?;
    let remaining_available_page_count = page_capacity
        .available_page_count
        .checked_sub(1)
        .context("cold-request presentation exhausted the reclaimable CHR page pool")?;
    let consumer_codebook = plan_consumer_codebook(ConsumerCodebookInputs {
        source_font_page,
        source_chr: rom.chr(),
        first_physical_page: cold_request_presentation
            .physical_page
            .checked_add(1)
            .context("consumer font page range overflow")?,
        available_page_count: remaining_available_page_count,
        dynamic_inputs: &dynamic_inputs,
        chapter_intro: &chapter_intro_residency,
        fixed: &fixed,
        unit_names: &unit_names,
        chapter_titles: &chapter_titles,
        choices: &choices,
        choice_glyph_codes: &choice_residency.choice_glyph_codes,
        map_menu: &map_menu,
        unit_ui: &unit_ui,
        item_actions: &item_actions,
        fixed_menu_labels: &fixed_menu_labels,
        options_glyph_codes: &options_glyph_codes,
        transitions: &transitions,
    })?;
    let chapter_save_projection = plan_chapter_save_projection(ChapterSaveProjectionInputs {
        candidate: &current_candidate,
        choices: &choices,
        choice_glyph_codes: &choice_residency.choice_glyph_codes,
        transitions: &transitions,
        consumer_codebook: &consumer_codebook,
    })?;
    let ending_record_projection = plan_ending_record_projection(EndingRecordProjectionInputs {
        source: &rom,
        candidate: &current_candidate,
        chapter_titles: &chapter_titles,
        transitions: &transitions,
        consumer_codebook: &consumer_codebook,
    })?;
    let consumer_catalog = plan_consumer_catalog(ConsumerCatalogInputs {
        source_font_page,
        source_chr: rom.chr(),
        first_physical_page: consumer_codebook.next_physical_page()?,
        available_page_count: consumer_codebook.remaining_page_count()?,
        preserved_unit_ui_display_codes: &preserved_unit_ui_display_codes(rom.data())?,
        resident_front_end_glyph_codes: front_end_result_menu_residency
            .installed_menu_glyph_codes(),
        fixed: &fixed,
        unit_names: &unit_names,
        unit_ui: &unit_ui,
        item_actions: &item_actions,
    })?;
    let front_end_result_residency = plan_front_end_result_residency(
        &display,
        &fixed,
        &unit_names,
        &consumer_catalog,
        front_end_result_menu_residency,
        &choice_residency.augmented_worksets,
    )?;
    let dialogue_graph = inspect_main_dialogue_graph(rom.data())?;
    let transition_residency = plan_transition_residency(
        &display,
        &dialogue_graph,
        &front_end_result_residency.augmented_worksets,
    )?;
    let codebook = plan_glyph_workset_page_upper_bound(&transition_residency.augmented_worksets)?;
    let dynamic_page_codes = bind_dynamic_string_page_codes(&dynamic_inputs, &codebook)?;
    ensure!(
        codebook.workset_count == display.page_worksets.len()
            && codebook.workset_page_indices.len() == display.page_worksets.len(),
        "dialogue codebook lost visible page worksets"
    );
    let fixed_ui_projection = plan_fixed_ui_projection(FixedUiProjectionInputs {
        candidate: &current_candidate,
        unit_ui: &unit_ui,
        item_actions: &item_actions,
        fixed_menu_labels: &fixed_menu_labels,
        map_menu: &map_menu,
        consumer_codebook: &consumer_codebook,
        consumer_catalog: &consumer_catalog,
    })?;
    let font_page_pack = build_glyph_workset_font_page_pack(source_font_page, &codebook)?;
    ensure!(
        font_page_pack.len() == codebook.page_assignments.len() * FONT_PAGE_SIZE,
        "dialogue font page pack length changed after rasterization"
    );
    verify_glyph_workset_font_page_pack(
        source_font_page,
        &transition_residency.augmented_worksets,
        &codebook,
        &font_page_pack,
    )?;
    // 전이 미러를 함께 내던 인코딩은 이중 진입과 함께 폐기했다. 정규 레코드의
    // 원천 소유 구간과 포인터만 낸다. 의사결정 59번을 따른다.
    let encoded_display = dialogue
        .encoded_by_page_groups(&codebook.workset_page_indices, &codebook.page_assignments)?;
    let mut cross_domain_target_glyphs = fixed.unique_glyphs();
    cross_domain_target_glyphs.extend(unit_names.unique_glyphs());
    cross_domain_target_glyphs.extend(chapter_titles.unique_glyphs());
    cross_domain_target_glyphs.extend(choices.unique_glyphs());
    cross_domain_target_glyphs.extend(map_menu.target_glyphs.iter().copied());
    cross_domain_target_glyphs.extend(unit_ui.unique_target_glyphs());
    cross_domain_target_glyphs.extend(item_actions.unique_target_glyphs());
    cross_domain_target_glyphs.extend(fixed_menu_labels.unique_target_glyphs());
    cross_domain_target_glyphs.extend(transitions.save_offer.target_glyphs.iter().copied());
    cross_domain_target_glyphs.extend(transitions.ending_record.target_glyphs.iter().copied());
    cross_domain_target_glyphs.extend(locations.unique_glyphs());
    let composition = plan_dialogue_runtime_composition(
        &display,
        &dialogue_graph,
        &codebook,
        &dynamic_page_codes,
        source_font_page,
        &font_page_pack,
        &cross_domain_target_glyphs,
    )?;
    ensure!(
        cross_domain_target_glyphs.iter().all(|glyph| composition
            .glyph_atlas_characters
            .binary_search(glyph)
            .is_ok()),
        "shared glyph atlas lost a required cross-domain glyph"
    );
    ensure!(
        composition.resident_group_change_count == 0
            && composition.resident_group_reuse_count
                == composition.resident_group_transition_count,
        "a visible dialogue transition changes its resident codebook"
    );
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
    ensure!(
        planned_storage_byte_count <= source_owned_storage_byte_count,
        "complete dialogue encoded storage exceeds its source-owned regions"
    );
    let atlas_and_scan_byte_count = composition.glyph_atlas.len() + composition.scan_material.len();
    let runtime_identity = plan_dialogue_runtime_identity(rom.data(), &display)?;
    let atlas_scan_and_identity_byte_count = atlas_and_scan_byte_count
        .checked_add(runtime_identity.material.len())
        .context("dialogue runtime material length overflow")?;
    let mut runtime_material = plan_dialogue_runtime_material(RuntimeMaterialInputs {
        glyph_atlas: &composition.glyph_atlas,
        page_scan: &composition.scan_material,
        runtime_identity: &runtime_identity.material,
        dynamic_producer_encoding: &dynamic_producer_encoding.material,
    })?;
    let cross_domain_material = plan_cross_domain_material(CrossDomainMaterialInputs {
        main_dialogue_runtime_material_byte_count: runtime_material.material.len(),
        shared_atlas_characters: &composition.glyph_atlas_characters,
        fixed: &fixed,
        unit_names: &unit_names,
        chapter_titles: &chapter_titles,
        choices: &choices,
        map_menu: &map_menu,
        unit_ui: &unit_ui,
        item_actions: &item_actions,
        fixed_menu_labels: &fixed_menu_labels,
        transitions: &transitions,
        locations: &locations,
        consumer_catalog: &consumer_catalog,
    })?;
    // 실행 코드는 자료 배치가 끝난 뒤에 조립한다. 놓일 주소가 자료 길이에서 나오기
    // 때문이다. 코드 길이는 그 주소에 영향을 주지 않으므로 한 번에 정해진다.
    // 실행 코드는 자료 배치가 끝난 뒤에 조립한다. 읽을 표들이 어느 페이지의 어느
    // 주소에 놓이는지가 배치에서 나오기 때문이다.
    let atlas_offset = runtime_material.glyph_atlas_offset()?;
    let scan_offset = runtime_material.section_offset("page_scan")?;
    let identity_offset = runtime_material.section_offset("runtime_identity")?;
    let producer_encoding_offset = runtime_material.section_offset("dynamic_producer_encoding")?;
    ensure!(
        identity_offset / MMC3_PAGE_BYTE_COUNT
            == (identity_offset + runtime_identity.material.len() - 1) / MMC3_PAGE_BYTE_COUNT,
        "runtime identity material crosses its mapped MMC3 page"
    );
    ensure!(
        producer_encoding_offset / MMC3_PAGE_BYTE_COUNT
            == (producer_encoding_offset + dynamic_producer_encoding.material.len() - 1)
                / MMC3_PAGE_BYTE_COUNT,
        "dynamic producer encoding material crosses its mapped MMC3 page"
    );
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
        identity_material_base: window(identity_offset)?,
        identity_selector_directory: window(identity_offset + IDENTITY_HEADER_BYTE_COUNT)?,
        identity_table_descriptors: window(
            identity_offset + IDENTITY_HEADER_BYTE_COUNT + IDENTITY_SELECTOR_DIRECTORY_BYTE_COUNT,
        )?,
        scan_page: page(scan_offset)?,
        page_selectors: window(scan_offset)?,
        record_directory: window(scan_offset + composition.record_page_group_selector_byte_count)?,
        group_directory: window(scan_offset + composition.group_block_directory_offset)?,
        group_block_container_base: u16::try_from(
            scan_offset + composition.group_block_container_offset,
        )
        .context("page group block base does not fit a 16-bit container offset")?,
        container_first_page: MAIN_DIALOGUE_MATERIAL_FIRST_PAGE,
        producer_encoding_page: page(producer_encoding_offset)?,
        producer_item_directory: window(
            producer_encoding_offset + dynamic_producer_encoding.item_directory_offset,
        )?,
        producer_unit_directory: window(
            producer_encoding_offset + dynamic_producer_encoding.unit_directory_offset,
        )?,
        producer_location_directory: window(
            producer_encoding_offset + dynamic_producer_encoding.location_directory_offset,
        )?,
        producer_encoding_base: window(producer_encoding_offset)?,
    };
    let dialogue_runtime_code = plan_dialogue_runtime_code(
        &rom,
        &current_candidate,
        runtime_material.runtime_code_cpu_start()?,
        page(atlas_offset)?,
        runtime_material.runtime_code_mmc3_page(),
        layout,
        cross_domain_material.consumer_catalog_runtime_layout()?,
        cold_request_presentation.mapper_register,
        runtime_code::consumer_font_page::ConsumerFontPageRoutes {
            front_end: front_end_mapper_route,
            unit_command: consumer_codebook.mapper_route_for("unit_command_menu")?,
            map_menu: consumer_codebook.mapper_route_for("map_menu")?,
            ending_record: consumer_codebook.mapper_route_for("ending_chapter_record")?,
            chapter_save_offer: consumer_codebook.mapper_route_for("chapter_save_offer")?,
            catalog: consumer_catalog.mapper_routes()?,
        },
    )?;
    let assembled_hook_roles = dialogue_runtime_code.hook_roles();
    dynamic_producer_encoding.bind_runtime_hooks(&assembled_hook_roles)?;
    let dynamic_string_producers_bound = dynamic_input_producers
        .every_record_selector_route_bound()
        && dynamic_producer_encoding.canonical_outputs_ready();
    let main_dialogue_route_population = plan_main_dialogue_route_population(
        &rom,
        &display,
        &encoded_display,
        &dialogue_graph,
        &dynamic_input_producers,
        &assembled_hook_roles,
    )?;
    for routine in &dialogue_runtime_code.code_routines {
        runtime_material.place_runtime_code(routine.address, &routine.bytes)?;
    }
    let runtime_code_routines_assembled = !dialogue_runtime_code.code_routines.is_empty()
        && runtime_material.runtime_code_routine_placement_count()
            == dialogue_runtime_code.code_routines.len();
    ensure!(
        runtime_code_routines_assembled,
        "dialogue runtime material did not assemble every planned code routine"
    );

    let runtime_state_storage = plan_dialogue_runtime_state_storage(&rom)?;
    ensure!(
        runtime_state_storage.source_reservation_selection_complete(),
        "dialogue runtime-state storage selection is incomplete"
    );
    let selected_runtime_state_cpu_range = runtime_state_storage
        .selected_cpu_range_hex()
        .context("dialogue runtime-state selection has no CPU range")?;
    let runtime_control_flow = plan_dialogue_runtime_control_flow(RuntimeControlFlowInputs {
        source: &rom,
        candidate: &current_candidate,
        runtime_code_offset: runtime_material.runtime_code_offset,
        runtime_code_byte_count: runtime_material.material.len()
            - runtime_material.runtime_code_offset,
        selected_runtime_state_cpu_range,
        runtime_code_routines_assembled,
        assembled_hook_roles: &assembled_hook_roles,
        chr_restore_callee_cycles: dialogue_runtime_code.chr_restore_callee_cycles,
        canonical_dynamic_codes_are_page_physical_codes: dynamic_page_codes
            .canonical_codes_are_page_physical_codes,
    })?;
    let required_target_unit_counts = BTreeMap::from([
        (
            "chapter_save_offer_label",
            transitions.save_offer.entry_count,
        ),
        ("chapter_titles", chapter_titles.entry_count),
        ("choice_labels", choices.entries.len()),
        (
            "class_names",
            fixed
                .entries
                .iter()
                .filter(|entry| entry.table_id == "class-names")
                .count(),
        ),
        (
            "ending_record_labels",
            transitions.ending_record.entry_count,
        ),
        (
            "enemy_names",
            fixed
                .entries
                .iter()
                .filter(|entry| entry.table_id == "enemy-names")
                .count(),
        ),
        ("item_action_labels", item_actions.entry_count),
        ("fixed_menu_labels", fixed_menu_labels.entry_count),
        (
            "item_names",
            fixed
                .entries
                .iter()
                .filter(|entry| entry.table_id == "item-names")
                .count(),
        ),
        ("location_names", locations.entries.len()),
        ("main_dialogue", dialogue.translated_line_count),
        ("map_menu_labels", map_menu.entry_count),
        ("unit_names", unit_names.entries.len()),
        ("unit_ui_labels", unit_ui.entry_count),
    ]);
    ensure!(
        required_target_unit_counts.len() == REQUIRED_DOMAIN_COUNT,
        "full translation consumer target populations do not cover every required domain"
    );
    let mut globally_planned_consumer_roles = BTreeMap::<&'static str, BTreeSet<String>>::new();
    if dialogue_runtime_code.consumer_catalog_paths_planned() {
        let mut add_roles = |domain: &'static str, roles: &[&'static str]| {
            globally_planned_consumer_roles
                .entry(domain)
                .or_default()
                .extend(roles.iter().map(|role| (*role).to_owned()));
        };
        for domain in [
            "unit_ui_labels",
            "item_action_labels",
            "map_menu_labels",
            "fixed_menu_labels",
        ] {
            add_roles(domain, fixed_ui_projection.projected_screen_roles(domain));
        }
        for domain in ["unit_names", "enemy_names", "class_names"] {
            add_roles(domain, &["unit_summary", "unit_status"]);
        }
        add_roles(
            "item_names",
            &[
                "unit_summary",
                "unit_status",
                "item_inventory_list",
                "item_action_menu",
            ],
        );
        for domain in ["chapter_save_offer_label", "choice_labels"] {
            add_roles(
                domain,
                chapter_save_projection.projected_screen_roles(domain),
            );
        }
        for domain in ["chapter_titles", "ending_record_labels"] {
            add_roles(
                domain,
                ending_record_projection.projected_screen_roles(domain),
            );
        }
    }
    let all_required_dialogue_runtime_hook_roles_assembled =
        runtime_control_flow.all_required_hook_roles_assembled();
    let mut consumer_installation = plan_consumer_installation(ConsumerInstallationInputs {
        current_candidate_path: inputs.current_candidate_path,
        current_build_report_path: inputs.current_build_report_path,
        required_domains: &REQUIRED_DOMAINS,
        target_unit_counts: &required_target_unit_counts,
        all_chapter_titles_encoded: chapter_intro_residency.encoded_titles.len()
            == chapter_titles.entry_count,
        all_dialogue_records_encoded: dialogue.record_ids.len() == 504
            && encoded_display.pointer_writes.len() == 517,
        all_dialogue_runtime_hook_roles_assembled:
            all_required_dialogue_runtime_hook_roles_assembled,
        dynamic_dialogue_producers_bound: dynamic_string_producers_bound,
        globally_planned_consumer_roles: &globally_planned_consumer_roles,
    })?;
    let installation_layout = plan_installation_layout(
        &current_candidate,
        &page_capacity,
        runtime_material.material.len(),
        cross_domain_material.material_span_byte_count(),
        &cold_request_presentation,
    )?;
    let (installed_image, integrated_write_set) =
        plan_integrated_write_set(IntegratedWriteSetInputs {
            candidate: &current_candidate,
            encoded_dialogue: &encoded_display,
            dialogue_runtime_material: &runtime_material.material,
            dialogue_runtime_code: &dialogue_runtime_code,
            encoded_chapter_titles: &chapter_intro_residency.encoded_titles,
            cold_request_presentation: &cold_request_presentation,
            consumer_codebook: &consumer_codebook,
            consumer_catalog: &consumer_catalog,
            cross_domain_material: &cross_domain_material,
            fixed_ui_projection: &fixed_ui_projection,
            chapter_save_projection: &chapter_save_projection,
            ending_record_projection: &ending_record_projection,
            consumer_installation: &consumer_installation,
            required_domains: &REQUIRED_DOMAINS,
            all_required_dialogue_runtime_hook_roles_assembled,
            output_will_be_emitted: inputs.output_path.is_some(),
        })?;
    let integrated_rom =
        Rom::parse(installed_image.clone()).context("parse integrated translation image")?;
    let final_roster_consumer_route = dialogue_runtime_code.final_roster_consumer_route()?;
    let carried_ui_domain_preservation = inspect_carried_ui_domains(CarriedUiDomainInputs {
        source: &rom,
        cumulative: &current_candidate,
        integrated: &integrated_rom,
        cumulative_report_path: inputs.current_build_report_path,
        options_localization_path: inputs.options_localization_path,
        roster_localization_path: inputs.roster_localization_path,
        front_end_menu_localization_path: inputs.front_end_menu_localization_path,
        class_profile_localization_path: inputs.class_profile_localization_path,
        title_graphics_localization_path: inputs.title_graphics_localization_path,
        title_logo_asset_path: inputs.title_logo_asset_path,
        final_roster_consumer_route: &final_roster_consumer_route,
    })?;
    let carried_ui_domains_complete = carried_ui_domain_preservation.complete();
    ensure!(
        carried_ui_domains_complete,
        "carried UI domain final-artifact reinspection is incomplete"
    );
    let final_battle_consumer_route = dialogue_runtime_code.final_battle_consumer_route()?;
    let carried_battle_domain_preservation =
        inspect_carried_battle_domains(CarriedBattleDomainInputs {
            source: &rom,
            cumulative: &current_candidate,
            integrated: &integrated_rom,
            cumulative_report_path: inputs.current_build_report_path,
            fixed_workspace_path: inputs.fixed_text_workspace_path,
            dialogue_workspace_path: inputs.battle_dialogue_workspace_path,
            final_consumer_route: &final_battle_consumer_route,
        })?;
    let carried_battle_domains_complete = carried_battle_domain_preservation.complete();
    ensure!(
        carried_battle_domains_complete,
        "carried battle domain final-artifact reinspection is incomplete"
    );
    let translation_input_complete = dialogue_validation.translation_input_complete;
    let review_complete = dialogue_validation.review_complete
        && fixed.review_complete
        && front_end.review_complete
        && unit_names.review_complete
        && chapter_titles.review_complete
        && choices.review_complete
        && map_menu.review_complete
        && unit_ui.review_complete
        && item_actions.review_complete
        && fixed_menu_labels.review_complete
        && transitions.save_offer.review_complete
        && transitions.ending_record.review_complete
        && locations.review_complete
        && carried_ui_domain_preservation.human_review_complete()
        && carried_battle_domain_preservation.human_review_complete()
        && translation_input_complete;
    let all_declared_consumers_statically_accounted =
        consumer_installation.all_declared_consumers_statically_accounted();
    let technical_installation_complete = integrated_write_set.technical_installation_complete();
    let integrated_image_sha1 = sha1_hex(&installed_image);
    let final_artifact_runtime_evidence = load_final_artifact_runtime_evidence(
        inputs.final_runtime_evidence_path,
        &integrated_image_sha1,
    )?;
    let registered_runtime_screen_roles =
        crate::translation_coverage::inspect_domain_screen_targets()?
            .into_iter()
            .flat_map(|domain| domain.screen_roles)
            .collect::<BTreeSet<_>>();
    consumer_installation.bind_declared_consumer_runtime_roles(
        &final_artifact_runtime_evidence.bound_screen_roles(),
        &registered_runtime_screen_roles,
    )?;
    let dynamic_verification_started = final_artifact_runtime_evidence.verification_started();
    let declared_consumer_runtime_observation_complete =
        !consumer_installation.declared_consumer_runtime_replay_required();
    let next_gate = if translation_input_complete
        && runtime_state_storage.source_reservation_selection_complete()
        && all_declared_consumers_statically_accounted
        && declared_consumer_runtime_observation_complete
    {
        "return from the closed declared consumer replay to the separate whole-game consumer census and release regressions for the exact integrated artifact"
    } else if translation_input_complete
        && runtime_state_storage.source_reservation_selection_complete()
        && all_declared_consumers_statically_accounted
        && dynamic_verification_started
    {
        "continue representative and worst-case declared consumer-path replay on the exact integrated artifact"
    } else if translation_input_complete
        && runtime_state_storage.source_reservation_selection_complete()
        && all_declared_consumers_statically_accounted
        && inputs.output_path.is_some()
    {
        "bind representative and worst-case declared consumer paths to the exact emitted artifact before returning to the separate whole-game census"
    } else if translation_input_complete
        && runtime_state_storage.source_reservation_selection_complete()
        && all_declared_consumers_statically_accounted
    {
        "materialize the exact integrated ROM, then bind representative and worst-case declared consumer paths to that artifact before returning to the separate whole-game census"
    } else if translation_input_complete
        && runtime_state_storage.source_reservation_selection_complete()
    {
        "finish every remaining declared consumer storage projection against its already-planned font page; do not treat this declared plan as the whole-game census"
    } else if translation_input_complete {
        "close the exact volatile runtime-state storage selection against source access, queue, save/load, and battle lifetimes; do not emit or run a partial ROM"
    } else {
        "author Korean for every untranslated Japanese line before recalculating glyph lifetimes; do not emit or run a partial ROM"
    };
    let rom_emitted = if let Some(path) = inputs.output_path {
        write_integrated_output(
            path,
            inputs.source_path,
            inputs.current_candidate_path,
            inputs.report_path,
            &installed_image,
        )?;
        true
    } else {
        false
    };
    let report = FullTranslationInstallReport {
        schema: FULL_TRANSLATION_REPORT_SCHEMA,
        source_sha1: EXPECTED_SOURCE_SHA1,
        strategy: "install the declared translation domains in one cumulative candidate, close declared installation gates, then run declared consumer-path dynamic regression on that same ROM; whole-game consumer census remains separate",
        declared_installation_domain_count: REQUIRED_DOMAIN_COUNT,
        declared_installation_domains: REQUIRED_DOMAINS,
        translation_inputs: TranslationInputs {
            main_dialogue_record_count: dialogue.record_ids.len(),
            fixed_text_physical_entry_count: fixed.entries.len(),
            front_end_menu_label_count: front_end.entries.len(),
            playable_unit_name_count: unit_names.entries.len(),
            chapter_title_count: chapter_titles.entry_count,
            choice_label_count: choices.entries.len(),
            map_menu_label_count: map_menu.entry_count,
            unit_ui_label_count: unit_ui.entry_count,
            item_action_label_count: item_actions.entry_count,
            fixed_menu_label_count: fixed_menu_labels.entry_count,
            transition_label_count: transitions.save_offer.entry_count
                + transitions.ending_record.entry_count,
            location_name_count: locations.entries.len(),
            translation_input_complete,
            review_complete,
        },
        fixed_string_consumers: fixed_string_consumers.census,
        fixed_string_ownership,
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
            static_page_pack_preserves_every_workset_code: true,
            canonical_records_connected: true,
            page_local_bundle_encoding_connected: true,
            glyph_characters_encoded_into_installed_runtime_atlas: technical_installation_complete,
            transition_stable_lifetime_count: transition_residency.lifetime_count,
            multi_record_transition_stable_lifetime_count: transition_residency
                .multi_record_lifetime_count,
            maximum_transition_stable_lifetime_record_count: transition_residency
                .maximum_lifetime_record_count,
            maximum_transition_stable_lifetime_workset_count: transition_residency
                .maximum_lifetime_workset_count,
            maximum_transition_stable_lifetime_slot_demand: transition_residency
                .maximum_lifetime_slot_demand,
            every_resident_transition_uses_one_codebook: true,
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
        choice_residency,
        front_end_result_residency,
        chapter_save_projection,
        ending_record_projection,
        dialogue_page_pool: DialoguePagePool {
            current_candidate_sha1: page_capacity.current_candidate_sha1,
            current_chr_page_count: page_capacity.current_chr_page_count,
            first_installable_physical_page: page_capacity.first_installable_physical_page,
            superseded_maximum_dialogue_page_count: page_capacity
                .superseded_maximum_dialogue_page_count,
            appendable_page_count: page_capacity.appendable_page_count,
            available_page_count: page_capacity.available_page_count,
            cold_request_presentation_page_count: 1,
            remaining_available_page_count,
            prebuilt_font_page_upper_bound: codebook.page_assignments.len(),
            prebuilt_upper_bound_fits_available_pages: codebook.page_assignments.len()
                <= remaining_available_page_count,
            exact_available_page_fit_decided: false,
            mapper_capacity_bound: true,
            current_candidate_bound: true,
        },
        cross_domain_material,
        consumer_codebook,
        consumer_catalog,
        fixed_ui_projection,
        installation_layout,
        integrated_write_set,
        dialogue_runtime_control_flow_static_contract: runtime_control_flow,
        dialogue_runtime_state_storage_source_reservation: runtime_state_storage,
        main_dialogue_route_population,
        dialogue_runtime_composition: DialogueRuntimeComposition {
            strategy_selected: true,
            glyph_atlas_tile_count: composition.glyph_atlas_tile_count,
            dialogue_codebook_glyph_count: composition.dialogue_codebook_glyph_count,
            additional_cross_domain_glyph_count: composition.additional_cross_domain_glyph_count,
            glyph_atlas_covers_every_required_domain_glyph: true,
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
            resident_group_transition_count: composition.resident_group_transition_count,
            resident_group_change_count: composition.resident_group_change_count,
            resident_group_reuse_count: composition.resident_group_reuse_count,
            maximum_resident_group_overlay_tile_count: composition
                .maximum_resident_group_overlay_tile_count,
            maximum_resident_group_overlay_frame_count: composition
                .maximum_resident_group_overlay_tile_count
                .div_ceil(usize::from(runtime_code::transport::TILES_PER_FRAME)),
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
            canonical_dynamic_code_count: dynamic_page_codes.canonical_code_count,
            translated_dynamic_page_group_count: dynamic_page_codes
                .translated_dynamic_page_group_count,
            dynamic_page_code_identity_entry_count: dynamic_page_codes.identity_entry_count,
            dynamic_page_code_material_byte_count: dynamic_page_codes.selected_material_byte_count,
            dynamic_page_code_strategy: dynamic_page_codes.selected_strategy,
            dynamic_page_code_material_sha1: dynamic_page_codes.material_sha1,
            canonical_dynamic_codes_are_page_physical_codes: dynamic_page_codes
                .canonical_codes_are_page_physical_codes,
            page_selectors_use_plain_group_indices: dynamic_page_codes
                .page_selectors_use_plain_group_indices,
            every_translated_dynamic_page_directly_consumable: dynamic_page_codes
                .every_translated_dynamic_page_directly_consumable,
            dynamic_string_producers_bound,
            dynamic_string_producers: dynamic_input_producers,
            dynamic_producer_encoding,
            dense_group_lookup_byte_count: composition.dense_group_lookup_byte_count,
            record_page_group_selector_byte_count: composition
                .record_page_group_selector_byte_count,
            record_selector_directory_byte_count: composition.record_selector_directory_byte_count,
            scan_material_byte_count: composition.scan_material_byte_count,
            scan_material_sha1: composition.scan_material_sha1,
            scan_material_serialized: true,
            atlas_and_scan_material_byte_count: composition.glyph_atlas.len()
                + composition.scan_material_byte_count,
            dialogue_runtime_identity: runtime_identity,
            atlas_scan_and_identity_byte_count,
            runtime_material_layout_and_assembly: runtime_material,
            runtime_page_scan_bound_to_assembled_control_flow: technical_installation_complete,
            current_battle_glyph_atlas_tile_count: page_capacity.battle_glyph_atlas_tile_count,
            current_battle_maximum_ppu_write_count: page_capacity.battle_maximum_ppu_write_count,
            current_battle_runtime_routine_byte_count: page_capacity
                .battle_runtime_routine_byte_count,
            current_battle_runtime_bound_to_build: page_capacity.battle_runtime_bound_to_build,
            battle_compositor_is_directly_reusable: false,
            main_dialogue_page_identity_material_serialized: true,
            main_dialogue_page_identity_bound_to_assembled_control_flow:
                technical_installation_complete,
            main_dialogue_transition_hook_planned: true,
        },
        consumer_installation,
        carried_ui_domain_preservation,
        carried_battle_domain_preservation,
        final_artifact_runtime_evidence,
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
            all_resident_dialogue_transitions_use_one_codebook: true,
            all_chapter_titles_encoded_with_resident_codes: true,
            all_chapter_title_storage_writes_planned: true,
            cold_request_presentation_page_planned: true,
            cold_request_presentation_write_planned: true,
            dialogue_runtime_composition_planned: true,
            all_declared_consumer_writes_planned: all_declared_consumers_statically_accounted,
            all_carried_ui_domains_reinspected: carried_ui_domains_complete,
            all_carried_battle_domains_reinspected: carried_battle_domains_complete,
            declared_plan_technical_installation_complete:
                all_declared_consumers_statically_accounted && technical_installation_complete,
            declared_consumer_runtime_observation_complete,
        },
        rom_emitted,
        dynamic_verification_started,
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

    Ok(FullTranslationInstallSummary {
        report_sha1: sha1_hex(&report_bytes),
        declared_installation_domain_count: REQUIRED_DOMAIN_COUNT,
        dialogue_record_count: dialogue.record_ids.len(),
        dialogue_page_workset_count: display.page_worksets.len(),
        dialogue_glyph_count: codebook.glyph_count,
        dialogue_maximum_page_slot_demand: codebook.maximum_page_slot_demand,
        dialogue_static_page_upper_bound_count: codebook.page_assignments.len(),
        dialogue_pointer_write_count: encoded_display.pointer_writes.len(),
        dialogue_planned_storage_byte_count: planned_storage_byte_count,
        integrated_image_sha1,
    })
}

fn write_integrated_output(
    output_path: &Path,
    source_path: &Path,
    current_candidate_path: &Path,
    report_path: &Path,
    installed_image: &[u8],
) -> Result<()> {
    let resolved_output = resolve_output_identity(output_path)?;
    for protected_path in [source_path, current_candidate_path, report_path] {
        let resolved_protected = resolve_output_identity(protected_path)?;
        ensure!(
            resolved_output != resolved_protected,
            "integrated output must not overwrite protected input {}",
            protected_path.display()
        );
    }

    fs::write(output_path, installed_image)
        .with_context(|| format!("write integrated output {}", output_path.display()))?;
    let read_back = fs::read(output_path)
        .with_context(|| format!("read integrated output {}", output_path.display()))?;
    ensure!(
        read_back == installed_image,
        "integrated output read-back differs from the planned image"
    );
    Ok(())
}

fn resolve_output_identity(path: &Path) -> Result<std::path::PathBuf> {
    if path.exists() {
        return fs::canonicalize(path)
            .with_context(|| format!("resolve existing path {}", path.display()));
    }
    let name = path
        .file_name()
        .context("output or report path has no file name")?;
    let parent = path.parent().unwrap_or_else(|| Path::new("."));
    fs::create_dir_all(parent)
        .with_context(|| format!("create output directory {}", parent.display()))?;
    Ok(fs::canonicalize(parent)
        .with_context(|| format!("resolve output directory {}", parent.display()))?
        .join(name))
}
