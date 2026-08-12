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

mod consumer_visible_prefixes;
mod current_candidate;
mod dynamic_composition;
mod dynamic_input_producers;
mod dynamic_inputs;

use consumer_visible_prefixes::{ConsumerVisiblePrefixPlan, plan_consumer_visible_prefixes};
use current_candidate::{CurrentCandidateInputs, inspect_dialogue_page_pool_capacity};
use dynamic_composition::plan_dialogue_runtime_composition;
use dynamic_input_producers::{DynamicInputProducerPlan, inspect_dynamic_input_producers};
use dynamic_inputs::{plan_dynamic_dialogue_inputs, plan_dynamic_string_remap};

const REQUIRED_DOMAIN_COUNT: usize = 13;

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
    dialogue_page_pool: DialoguePagePool,
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
    mode_specific_visible_prefix_japanese_source_byte_count: usize,
    mode_specific_visible_prefix_translation_input_complete: bool,
    translation_input_complete: bool,
    review_complete: bool,
}

#[derive(Serialize)]
struct DialogueCodebook {
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
    page_local_bundle_encoding_connected: bool,
    glyph_characters_emitted: bool,
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
    consumer_visible_prefixes: ConsumerVisiblePrefixPlan,
    dense_group_lookup_byte_count: usize,
    record_page_group_selector_byte_count: usize,
    record_selector_directory_byte_count: usize,
    scan_material_byte_count: usize,
    atlas_and_scan_material_byte_count: usize,
    atlas_scan_and_dynamic_remap_byte_count: usize,
    runtime_page_scan_bound_to_control_flow: bool,
    current_battle_glyph_atlas_tile_count: usize,
    current_battle_maximum_ppu_write_count: usize,
    current_battle_runtime_routine_byte_count: usize,
    current_battle_runtime_bound_to_build: bool,
    battle_compositor_is_directly_reusable: bool,
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

    let dynamic_inputs = plan_dynamic_dialogue_inputs(
        &dialogue,
        &fixed.entries,
        &unit_names.entries,
        &locations.entries,
    )?;
    let dynamic_input_producers = inspect_dynamic_input_producers(&rom)?;
    let dynamic_string_producers_bound =
        dynamic_input_producers.every_record_selector_route_bound();
    let codebook = plan_glyph_workset_page_upper_bound(&dynamic_inputs.augmented_worksets)?;
    let dynamic_remap = plan_dynamic_string_remap(&dynamic_inputs, &codebook)?;
    ensure!(
        codebook.workset_count == dialogue.page_worksets.len()
            && codebook.workset_page_indices.len() == dialogue.page_worksets.len(),
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
    let composition =
        plan_dialogue_runtime_composition(&dialogue, &codebook, source_font_page, &font_page_pack)?;
    let encoded = dialogue
        .encoded_by_page_groups(&codebook.workset_page_indices, &codebook.page_assignments)?;
    ensure!(
        encoded.regions.len() == 11 && encoded.pointer_writes.len() == 517,
        "complete dialogue encoded layout changed"
    );
    let source_owned_storage_byte_count = encoded
        .regions
        .iter()
        .map(|region| region.source_storage.len())
        .sum::<usize>();
    let planned_storage_byte_count = encoded
        .regions
        .iter()
        .map(|region| region.used_storage_byte_count)
        .sum::<usize>();
    ensure!(
        planned_storage_byte_count <= source_owned_storage_byte_count,
        "complete dialogue encoded storage exceeds its source-owned regions"
    );
    let atlas_scan_and_dynamic_remap_byte_count = composition.glyph_atlas.len()
        + composition.scan_material_byte_count
        + dynamic_remap.selected_dense_remap_byte_count;
    let consumer_visible_prefixes = plan_consumer_visible_prefixes(
        rom.data(),
        &dialogue,
        source_owned_storage_byte_count,
        planned_storage_byte_count,
        atlas_scan_and_dynamic_remap_byte_count,
    )?;
    let translation_input_complete = consumer_visible_prefixes.translation_input_complete();
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

    let report = FullTranslationInstallReport {
        schema: 2,
        source_sha1: EXPECTED_SOURCE_SHA1,
        strategy: "install all remaining translation domains in one cumulative candidate, run complete static gates, then run consumer-path dynamic regression on that same ROM",
        required_domain_count: REQUIRED_DOMAIN_COUNT,
        required_domains: [
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
        ],
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
            mode_specific_visible_prefix_japanese_source_byte_count: consumer_visible_prefixes
                .japanese_source_byte_count(),
            mode_specific_visible_prefix_translation_input_complete: translation_input_complete,
            translation_input_complete,
            review_complete,
        },
        dialogue_codebook: DialogueCodebook {
            page_workset_count: dialogue.page_worksets.len(),
            unique_workset_count: codebook.unique_workset_count,
            literal_glyph_count: dialogue.unique_glyphs().len(),
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
            page_local_bundle_encoding_connected: true,
            glyph_characters_emitted: false,
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
        dialogue_runtime_composition: DialogueRuntimeComposition {
            strategy_selected: true,
            glyph_atlas_tile_count: composition.glyph_atlas_tile_count,
            stored_bytes_per_glyph: 8,
            composed_bytes_per_glyph: FONT_TILE_SIZE,
            glyph_atlas_byte_count: composition.glyph_atlas.len(),
            glyph_atlas_prg_8k_page_count: composition.glyph_atlas.len().div_ceil(8 * 1024),
            glyph_atlas_sha1: sha1_hex(&composition.glyph_atlas),
            generated_high_bitplane_is_zero: true,
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
            consumer_visible_prefixes,
            dense_group_lookup_byte_count: composition.dense_group_lookup_byte_count,
            record_page_group_selector_byte_count: composition
                .record_page_group_selector_byte_count,
            record_selector_directory_byte_count: composition.record_selector_directory_byte_count,
            scan_material_byte_count: composition.scan_material_byte_count,
            atlas_and_scan_material_byte_count: composition.glyph_atlas.len()
                + composition.scan_material_byte_count,
            atlas_scan_and_dynamic_remap_byte_count,
            runtime_page_scan_bound_to_control_flow: false,
            current_battle_glyph_atlas_tile_count: page_capacity.battle_glyph_atlas_tile_count,
            current_battle_maximum_ppu_write_count: page_capacity.battle_maximum_ppu_write_count,
            current_battle_runtime_routine_byte_count: page_capacity
                .battle_runtime_routine_byte_count,
            current_battle_runtime_bound_to_build: page_capacity.battle_runtime_bound_to_build,
            battle_compositor_is_directly_reusable: false,
            main_dialogue_page_identity_bound: false,
            main_dialogue_transition_hook_planned: false,
        },
        dialogue_storage: DialogueStorage {
            region_count: encoded.regions.len(),
            record_count: dialogue.record_ids.len(),
            pointer_write_count: encoded.pointer_writes.len(),
            source_owned_storage_byte_count,
            planned_storage_byte_count,
            remaining_storage_byte_count: source_owned_storage_byte_count
                - planned_storage_byte_count,
            every_pointer_within_source_owned_regions: true,
        },
        installation_gates: InstallationGates {
            all_translation_inputs_loaded: translation_input_complete,
            all_dialogue_records_encoded: true,
            all_visible_dialogue_text_encoded: false,
            all_dialogue_pointers_planned: true,
            all_dialogue_page_code_assignments_found: true,
            all_dialogue_page_worksets_packed: true,
            static_prebuilt_dialogue_page_pool_fits: codebook.page_assignments.len()
                <= page_capacity.available_page_count,
            dialogue_runtime_composition_planned: false,
            cross_domain_consumer_writes_planned: false,
            integrated_candidate_ready: false,
        },
        rom_emitted: false,
        dynamic_verification_started: false,
        next_gate: "prove direct and transition reachability for every differing entry mode and translate every reachable mode-specific Japanese fragment before binding normalized bodies or shims; do not emit or run a partial ROM",
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
        required_domain_count: REQUIRED_DOMAIN_COUNT,
        dialogue_record_count: dialogue.record_ids.len(),
        dialogue_page_workset_count: dialogue.page_worksets.len(),
        dialogue_glyph_count: codebook.glyph_count,
        dialogue_maximum_page_slot_demand: codebook.maximum_page_slot_demand,
        dialogue_static_page_upper_bound_count: codebook.page_assignments.len(),
        dialogue_pointer_write_count: encoded.pointer_writes.len(),
        dialogue_planned_storage_byte_count: planned_storage_byte_count,
    })
}
