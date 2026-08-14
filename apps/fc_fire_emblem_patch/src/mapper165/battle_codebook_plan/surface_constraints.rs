use std::{
    collections::{BTreeMap, BTreeSet},
    fs,
    path::Path,
};

use anyhow::{Context, Result, ensure};
use serde::Serialize;

use crate::{
    dialogue_assets::plan_battle_dialogue_records,
    font_slots::ACTIVE_HANGUL_SLOT_COUNT,
    rom::{EXPECTED_SOURCE_SHA1, Rom},
    sha1_hex,
    temporal_surface::{ObservedBattleRuntimeInput, load_observed_battle_temporal_evidence},
    text_inventory::plan_fixed_text,
};

use super::{
    ScreenCodeConstraint,
    background_ownership::bind_battle_background_code_ownership,
    composition::{BattleRuntimeRecipeInput, BattleRuntimeRecipeSelection},
    plan_battle_codebook_model,
    protected_color_placement::plan_protected_color_placement,
    selected_physical_assignment::prove_selected_assignment_capacity,
};

mod dynamic_assignment_catalog;

use dynamic_assignment_catalog::{
    RouteDynamicAssignmentCoverage, catalog_observed_dynamic_assignments,
};

#[derive(Debug, Serialize)]
struct BattleSurfaceConstraintReport {
    schema: u8,
    source_sha1: &'static str,
    fixed_workspace_sha1: String,
    dialogue_workspace_sha1: String,
    temporal_manifest_sha1: String,
    observed_battle_sample_count: usize,
    observed_route_sample_counts: BTreeMap<String, usize>,
    observed_runtime_tuple_count: usize,
    observed_dialogue_selector_count: usize,
    selector_62_predicate_sample_count: usize,
    selector_projection_changed_sample_count: usize,
    screen_constraint_count: usize,
    source_font_page_sha1: &'static str,
    source_page_japanese_text_active_code_count: usize,
    source_page_preserved_non_japanese_active_code_count: usize,
    background_producer_topology: super::background_ownership::BattleBackgroundProducerTopology,
    background_payload_model: super::background_payloads::BattleBackgroundPayloadModel,
    phase_publisher_reachability: super::phase_cooccurrence::BattlePhasePublisherReachability,
    text_consumer_topology: super::text_consumer_topology::BattleTextConsumerTopology,
    remap_storage: super::remap_storage::BattleRemapStorageContract,
    protected_color_placement: super::protected_color_placement::ProtectedColorPlacementReport,
    physical_assignment_architecture: PhysicalAssignmentArchitecture,
    route_dynamic_assignment_coverage: Vec<RouteDynamicAssignmentCoverage>,
    minimum_observed_active_code_count: usize,
    maximum_observed_active_code_count: usize,
    minimum_observed_japanese_text_code_count: usize,
    maximum_observed_japanese_text_code_count: usize,
    minimum_preserved_non_japanese_active_code_count: usize,
    maximum_preserved_non_japanese_active_code_count: usize,
    maximum_selected_recipe_count: usize,
    maximum_selected_glyph_count: usize,
    maximum_selected_overlay_count: usize,
    nametable_constrained_sample_count: usize,
    visible_oam_constrained_sample_count: usize,
    abstract_assignment_sha1: String,
    observed_dynamic_assignment_catalog_sha1: String,
    stable_color_count: usize,
    observed_maximum_selected_color_count: usize,
    observed_maximum_remap_pair_count: usize,
    conservative_text_overlay_count: usize,
    exact_modeled_text_overlay_count: usize,
    observed_maximum_combined_slot_demand: usize,
    observed_minimum_slot_headroom: usize,
    conservative_global_preserved_active_code_count: usize,
    conservative_global_combined_slot_demand: usize,
    conservative_global_minimum_slot_headroom: usize,
    exact_modeled_global_combined_slot_demand: usize,
    exact_modeled_global_minimum_slot_headroom: usize,
    temporal_sampling_is_irregular: bool,
    pattern_table_consumers_filtered: bool,
    source_page_code_ownership_applied: bool,
    observed_japanese_codes_reclaimed_for_translation: bool,
    every_observed_recipe_resolved: bool,
    runtime_selection_uses_source_bound_dialogue_projection: bool,
    every_observed_sample_has_dynamic_assignment: bool,
    observed_route_catalog_complete: bool,
    global_background_capacity_bound_complete: bool,
    observed_dynamic_assignment_catalog_complete: bool,
    glyph_characters_emitted: bool,
    translation_text_emitted: bool,
    runtime_verified: bool,
    release_eligible: bool,
    next_gate: &'static str,
}

#[derive(Debug, Serialize)]
struct PhysicalAssignmentArchitecture {
    static_abstract_color_count: usize,
    active_physical_code_count: usize,
    borrowed_logical_code_count: usize,
    protected_abstract_color_count: usize,
    protected_physical_code_count: usize,
    safe_physical_code_count: usize,
    static_full_model_assignment_infeasible: bool,
    conservative_per_battle_text_code_count: usize,
    conservative_per_battle_combined_code_count: usize,
    dynamic_per_battle_headroom: usize,
    exact_modeled_per_battle_text_code_count: usize,
    exact_modeled_per_battle_combined_code_count: usize,
    exact_modeled_per_battle_headroom: usize,
    arbitrary_selection_maximum_collision_count: usize,
    arbitrary_selection_direct_table_byte_count: usize,
    modeled_maximum_collision_count: usize,
    modeled_pair_table_byte_count: usize,
    identity_code_count_at_maximum_collision: usize,
    strongest_dynamic_assignment_sha1: String,
    dynamic_per_battle_assignment_capacity_proven: bool,
    selected_assignment_algorithm_complete: bool,
    selected_strategy: &'static str,
    per_battle_dynamic_assignment_is_release_architecture: bool,
}

pub(crate) struct BattleSurfaceConstraintSummary {
    pub(crate) report_sha1: String,
    pub(crate) sample_count: usize,
    pub(crate) runtime_tuple_count: usize,
    pub(crate) dynamic_assignment_catalog_sha1: String,
    pub(crate) maximum_selected_color_count: usize,
    pub(crate) maximum_remap_pair_count: usize,
}

pub(in crate::mapper165) struct ObservedBattleSurfaceSelection {
    pub(in crate::mapper165) constraints: Vec<(String, ScreenCodeConstraint)>,
    route_sample_counts: BTreeMap<String, usize>,
    pub(in crate::mapper165) runtime_input_count: usize,
    observed_dialogue_selector_count: usize,
    selector_62_predicate_sample_count: usize,
    selector_projection_changed_sample_count: usize,
    minimum_observed_active_code_count: usize,
    maximum_observed_active_code_count: usize,
    minimum_observed_japanese_text_code_count: usize,
    maximum_observed_japanese_text_code_count: usize,
    minimum_preserved_non_japanese_active_code_count: usize,
    maximum_preserved_non_japanese_active_code_count: usize,
    maximum_selected_recipe_count: usize,
    maximum_selected_glyph_count: usize,
    pub(in crate::mapper165) maximum_selected_overlay_count: usize,
    nametable_constrained_sample_count: usize,
    visible_oam_constrained_sample_count: usize,
}

pub(in crate::mapper165) fn select_observed_battle_surfaces(
    rom: &Rom,
    material: &super::composition::BattleCacheCompositionMaterial,
    evidence: &crate::temporal_surface::ObservedBattleTemporalEvidence,
) -> Result<ObservedBattleSurfaceSelection> {
    let ownership = bind_battle_background_code_ownership(rom)?;
    let mut route_sample_counts = BTreeMap::new();
    let mut runtime_inputs = BTreeSet::new();
    let mut observed_dialogue_selectors = BTreeSet::new();
    let mut constraints = Vec::with_capacity(evidence.samples.len());
    let mut minimum_observed_active_code_count = usize::MAX;
    let mut maximum_observed_active_code_count = 0;
    let mut minimum_observed_japanese_text_code_count = usize::MAX;
    let mut maximum_observed_japanese_text_code_count = 0;
    let mut minimum_preserved_non_japanese_active_code_count = usize::MAX;
    let mut maximum_preserved_non_japanese_active_code_count = 0;
    let mut maximum_selected_recipe_count = 0;
    let mut maximum_selected_glyph_count = 0;
    let mut maximum_selected_overlay_count = 0;
    let mut nametable_constrained_sample_count = 0;
    let mut visible_oam_constrained_sample_count = 0;
    let mut selector_62_predicate_sample_count = 0;
    let mut selector_projection_changed_sample_count = 0;
    for sample in &evidence.samples {
        *route_sample_counts
            .entry(sample.route_role.clone())
            .or_insert(0) += 1;
        let input = runtime_recipe_input(&sample.runtime_input);
        runtime_inputs.insert(input);
        observed_dialogue_selectors.insert(sample.runtime_input.observed_dialogue_selector);
        selector_62_predicate_sample_count +=
            usize::from(sample.runtime_input.selector_62_predicate_matched);
        selector_projection_changed_sample_count += usize::from(
            sample.runtime_input.observed_dialogue_selector
                != sample.runtime_input.projected_dialogue_selector,
        );
        let selection = material.select_runtime_recipes(input)?;
        validate_selection(&selection)?;
        let background = ownership.partition_observed(&sample.active_tile_codes)?;
        minimum_observed_active_code_count =
            minimum_observed_active_code_count.min(sample.active_tile_codes.len());
        maximum_observed_active_code_count =
            maximum_observed_active_code_count.max(sample.active_tile_codes.len());
        minimum_observed_japanese_text_code_count =
            minimum_observed_japanese_text_code_count.min(background.japanese_text_codes.len());
        maximum_observed_japanese_text_code_count =
            maximum_observed_japanese_text_code_count.max(background.japanese_text_codes.len());
        minimum_preserved_non_japanese_active_code_count =
            minimum_preserved_non_japanese_active_code_count
                .min(background.preserved_non_japanese_codes.len());
        maximum_preserved_non_japanese_active_code_count =
            maximum_preserved_non_japanese_active_code_count
                .max(background.preserved_non_japanese_codes.len());
        maximum_selected_recipe_count =
            maximum_selected_recipe_count.max(selection.recipe_offsets.len());
        maximum_selected_glyph_count = maximum_selected_glyph_count.max(selection.glyphs.len());
        maximum_selected_overlay_count =
            maximum_selected_overlay_count.max(selection.overlays.len());
        nametable_constrained_sample_count += usize::from(sample.nametable_constrains_cache);
        visible_oam_constrained_sample_count += usize::from(sample.visible_oam_constrains_cache);
        constraints.push((
            sample.route_role.clone(),
            ScreenCodeConstraint {
                glyphs: selection.glyphs,
                preserved_active_codes: background.preserved_non_japanese_codes,
            },
        ));
    }
    ensure!(
        !constraints.is_empty(),
        "observed battle surface selection is empty"
    );
    Ok(ObservedBattleSurfaceSelection {
        constraints,
        route_sample_counts,
        runtime_input_count: runtime_inputs.len(),
        observed_dialogue_selector_count: observed_dialogue_selectors.len(),
        selector_62_predicate_sample_count,
        selector_projection_changed_sample_count,
        minimum_observed_active_code_count,
        maximum_observed_active_code_count,
        minimum_observed_japanese_text_code_count,
        maximum_observed_japanese_text_code_count,
        minimum_preserved_non_japanese_active_code_count,
        maximum_preserved_non_japanese_active_code_count,
        maximum_selected_recipe_count,
        maximum_selected_glyph_count,
        maximum_selected_overlay_count,
        nametable_constrained_sample_count,
        visible_oam_constrained_sample_count,
    })
}

pub(crate) fn analyze_battle_surface_constraints(
    source_path: &Path,
    fixed_workspace_path: &Path,
    dialogue_workspace_path: &Path,
    temporal_manifest_path: &Path,
    report_path: &Path,
) -> Result<BattleSurfaceConstraintSummary> {
    let rom = Rom::from_path(source_path)?;
    rom.verify_supported_japanese()?;
    let fixed = plan_fixed_text(&rom, fixed_workspace_path)?;
    let dialogue = plan_battle_dialogue_records(&rom, dialogue_workspace_path)?;
    let model = plan_battle_codebook_model(&rom, &fixed, &dialogue)?;
    let evidence = load_observed_battle_temporal_evidence(source_path, temporal_manifest_path)?;
    let ownership = bind_battle_background_code_ownership(&rom)?;
    let phase_publisher_reachability =
        super::phase_cooccurrence::bind_phase_publisher_reachability(&rom)?;
    let text_consumer_topology =
        super::text_consumer_topology::bind_battle_text_consumer_topology(&rom)?;
    let background_payload_model = ownership.payload_model();
    let remap_storage = super::remap_storage::bind_battle_remap_storage(
        &rom,
        &background_payload_model,
        &text_consumer_topology,
    )?;

    let selection = select_observed_battle_surfaces(&rom, &model.composition, &evidence)?;
    let constraints = selection.constraints;
    let route_sample_counts = selection.route_sample_counts;
    let minimum_observed_active_code_count = selection.minimum_observed_active_code_count;
    let maximum_observed_active_code_count = selection.maximum_observed_active_code_count;
    let minimum_observed_japanese_text_code_count =
        selection.minimum_observed_japanese_text_code_count;
    let maximum_observed_japanese_text_code_count =
        selection.maximum_observed_japanese_text_code_count;
    let minimum_preserved_non_japanese_active_code_count =
        selection.minimum_preserved_non_japanese_active_code_count;
    let maximum_preserved_non_japanese_active_code_count =
        selection.maximum_preserved_non_japanese_active_code_count;
    let maximum_selected_recipe_count = selection.maximum_selected_recipe_count;
    let maximum_selected_glyph_count = selection.maximum_selected_glyph_count;
    let maximum_selected_overlay_count = selection.maximum_selected_overlay_count;
    let nametable_constrained_sample_count = selection.nametable_constrained_sample_count;
    let visible_oam_constrained_sample_count = selection.visible_oam_constrained_sample_count;
    let selector_62_predicate_sample_count = selection.selector_62_predicate_sample_count;
    let selector_projection_changed_sample_count =
        selection.selector_projection_changed_sample_count;
    ensure!(
        route_sample_counts.keys().cloned().collect::<BTreeSet<_>>()
            == [
                "gameplay_battle_favorable".to_owned(),
                "gameplay_battle_unfavorable".to_owned(),
                "sound_test_shared_battle".to_owned(),
            ]
            .into_iter()
            .collect(),
        "observed battle constraints do not cover both gameplay polarities and the shared sound-test route"
    );
    let conservative_text_overlay_count = model.runtime_demand.maximum_overlay_glyph_count();
    let exact_modeled_text_overlay_count = model.runtime_demand.exact_maximum_overlay_glyph_count();
    let observed_maximum_combined_slot_demand = conservative_text_overlay_count
        .checked_add(maximum_preserved_non_japanese_active_code_count)
        .context("observed battle combined slot demand overflow")?;
    ensure!(
        observed_maximum_combined_slot_demand <= ACTIVE_HANGUL_SLOT_COUNT,
        "observed battle text and preserved background need {observed_maximum_combined_slot_demand} slots but only {ACTIVE_HANGUL_SLOT_COUNT} exist"
    );
    let conservative_global_preserved_active_code_count =
        ownership.conservative_global_preserved_active_codes().len();
    let conservative_global_combined_slot_demand = conservative_text_overlay_count
        .checked_add(conservative_global_preserved_active_code_count)
        .context("global battle combined slot demand overflow")?;
    ensure!(
        conservative_global_combined_slot_demand <= ACTIVE_HANGUL_SLOT_COUNT,
        "global battle text and preserved background union need {conservative_global_combined_slot_demand} slots but only {ACTIVE_HANGUL_SLOT_COUNT} exist"
    );
    let exact_modeled_global_combined_slot_demand = exact_modeled_text_overlay_count
        .checked_add(conservative_global_preserved_active_code_count)
        .context("exact modeled global battle combined slot demand overflow")?;
    ensure!(
        exact_modeled_global_combined_slot_demand <= conservative_global_combined_slot_demand,
        "exact modeled global battle demand exceeds its conservative upper bound"
    );
    let static_safe_code_count =
        ACTIVE_HANGUL_SLOT_COUNT - conservative_global_preserved_active_code_count;
    ensure!(
        model.coloring.color_count > static_safe_code_count,
        "the global battle coloring unexpectedly fits a static table that excludes every preserved graphics code"
    );
    let selected_assignment_capacity = prove_selected_assignment_capacity(
        conservative_text_overlay_count,
        &ownership.conservative_global_preserved_active_codes(),
    )?;
    let protected_color_placement = plan_protected_color_placement(
        &model.glyph_families,
        &model.coloring,
        &ownership.conservative_global_preserved_active_codes(),
        &super::preserved_battle_literal_codes(&fixed, &dialogue),
    )?;
    ensure!(
        protected_color_placement.canonical_color_codes.len() == model.coloring.color_count,
        "protected battle color placement lost a canonical code"
    );
    ensure!(
        protected_color_placement.conservative_collision_count
            <= selected_assignment_capacity.maximum_collision_count,
        "protected battle color placement exceeds the arbitrary-selection collision bound"
    );
    ensure!(
        selected_assignment_capacity.remaining_safe_code_count
            == ACTIVE_HANGUL_SLOT_COUNT - conservative_global_combined_slot_demand,
        "selected battle assignment proof disagrees with the global capacity bound"
    );
    let dynamic_assignments = catalog_observed_dynamic_assignments(
        &constraints,
        &model.coloring,
        &protected_color_placement.canonical_color_codes,
        &protected_color_placement.protected_abstract_colors,
        remap_storage.maximum_remap_pair_count(),
    )?;
    ensure!(
        dynamic_assignments.maximum_selected_color_count <= conservative_text_overlay_count,
        "observed battle selected-color count exceeds the conservative runtime bound"
    );
    ensure!(
        dynamic_assignments.maximum_remap_pair_count
            <= protected_color_placement.conservative_collision_count,
        "observed battle remap demand exceeds the modeled collision bound"
    );
    let report = BattleSurfaceConstraintReport {
        schema: 13,
        source_sha1: EXPECTED_SOURCE_SHA1,
        fixed_workspace_sha1: sha1_hex(&fs::read(fixed_workspace_path)?),
        dialogue_workspace_sha1: sha1_hex(&fs::read(dialogue_workspace_path)?),
        temporal_manifest_sha1: evidence.manifest_sha1,
        observed_battle_sample_count: evidence.samples.len(),
        observed_route_sample_counts: route_sample_counts,
        observed_runtime_tuple_count: selection.runtime_input_count,
        observed_dialogue_selector_count: selection.observed_dialogue_selector_count,
        selector_62_predicate_sample_count,
        selector_projection_changed_sample_count,
        screen_constraint_count: constraints.len(),
        source_font_page_sha1: ownership.source_font_page_sha1(),
        source_page_japanese_text_active_code_count: ownership.japanese_text_active_code_count(),
        source_page_preserved_non_japanese_active_code_count: ownership
            .preserved_non_japanese_active_code_count(),
        background_producer_topology: ownership.producer_topology(),
        background_payload_model,
        phase_publisher_reachability,
        text_consumer_topology,
        remap_storage,
        protected_color_placement: protected_color_placement.report,
        physical_assignment_architecture: PhysicalAssignmentArchitecture {
            static_abstract_color_count: model.coloring.color_count,
            active_physical_code_count: ACTIVE_HANGUL_SLOT_COUNT,
            borrowed_logical_code_count: model.coloring.color_count - ACTIVE_HANGUL_SLOT_COUNT,
            protected_abstract_color_count: selected_assignment_capacity.protected_code_count
                + model.coloring.color_count
                - ACTIVE_HANGUL_SLOT_COUNT,
            protected_physical_code_count: selected_assignment_capacity.protected_code_count,
            safe_physical_code_count: selected_assignment_capacity.safe_code_count,
            static_full_model_assignment_infeasible: model.coloring.color_count
                > static_safe_code_count,
            conservative_per_battle_text_code_count: selected_assignment_capacity
                .maximum_selected_code_count,
            conservative_per_battle_combined_code_count: conservative_global_combined_slot_demand,
            dynamic_per_battle_headroom: ACTIVE_HANGUL_SLOT_COUNT
                - conservative_global_combined_slot_demand,
            exact_modeled_per_battle_text_code_count: exact_modeled_text_overlay_count,
            exact_modeled_per_battle_combined_code_count: exact_modeled_global_combined_slot_demand,
            exact_modeled_per_battle_headroom: ACTIVE_HANGUL_SLOT_COUNT
                - exact_modeled_global_combined_slot_demand,
            arbitrary_selection_maximum_collision_count: selected_assignment_capacity
                .maximum_collision_count,
            arbitrary_selection_direct_table_byte_count: selected_assignment_capacity
                .remap_table_byte_count,
            modeled_maximum_collision_count: protected_color_placement.conservative_collision_count,
            modeled_pair_table_byte_count: 1 + protected_color_placement
                .conservative_collision_count
                * 2,
            identity_code_count_at_maximum_collision: selected_assignment_capacity
                .identity_code_count_at_maximum_collision,
            strongest_dynamic_assignment_sha1: selected_assignment_capacity
                .strongest_assignment_sha1,
            dynamic_per_battle_assignment_capacity_proven: true,
            selected_assignment_algorithm_complete: true,
            selected_strategy: "place preserved physical codes on abstract colors outside the always-live common set, encode text with that canonical permutation, and keep a counted pair only for selected canonical codes that collide",
            per_battle_dynamic_assignment_is_release_architecture: true,
        },
        route_dynamic_assignment_coverage: dynamic_assignments.route_coverage,
        minimum_observed_active_code_count,
        maximum_observed_active_code_count,
        minimum_observed_japanese_text_code_count,
        maximum_observed_japanese_text_code_count,
        minimum_preserved_non_japanese_active_code_count,
        maximum_preserved_non_japanese_active_code_count,
        maximum_selected_recipe_count,
        maximum_selected_glyph_count,
        maximum_selected_overlay_count,
        nametable_constrained_sample_count,
        visible_oam_constrained_sample_count,
        abstract_assignment_sha1: model.coloring.assignment_sha1,
        observed_dynamic_assignment_catalog_sha1: dynamic_assignments.catalog_sha1.clone(),
        stable_color_count: model.coloring.color_count,
        observed_maximum_selected_color_count: dynamic_assignments.maximum_selected_color_count,
        observed_maximum_remap_pair_count: dynamic_assignments.maximum_remap_pair_count,
        conservative_text_overlay_count,
        exact_modeled_text_overlay_count,
        observed_maximum_combined_slot_demand,
        observed_minimum_slot_headroom: ACTIVE_HANGUL_SLOT_COUNT
            - observed_maximum_combined_slot_demand,
        conservative_global_preserved_active_code_count,
        conservative_global_combined_slot_demand,
        conservative_global_minimum_slot_headroom: ACTIVE_HANGUL_SLOT_COUNT
            - conservative_global_combined_slot_demand,
        exact_modeled_global_combined_slot_demand,
        exact_modeled_global_minimum_slot_headroom: ACTIVE_HANGUL_SLOT_COUNT
            - exact_modeled_global_combined_slot_demand,
        temporal_sampling_is_irregular: true,
        pattern_table_consumers_filtered: true,
        source_page_code_ownership_applied: true,
        observed_japanese_codes_reclaimed_for_translation: true,
        every_observed_recipe_resolved: true,
        runtime_selection_uses_source_bound_dialogue_projection: true,
        every_observed_sample_has_dynamic_assignment: true,
        observed_route_catalog_complete: true,
        global_background_capacity_bound_complete: true,
        observed_dynamic_assignment_catalog_complete: true,
        glyph_characters_emitted: false,
        translation_text_emitted: false,
        runtime_verified: false,
        release_eligible: false,
        next_gate: "replay every admitted battle route on the same cumulative artifact, then install the remaining translation-domain consumers",
    };
    let mut report_bytes =
        serde_json::to_vec_pretty(&report).context("serialize battle surface constraints")?;
    report_bytes.push(b'\n');
    if let Some(parent) = report_path.parent() {
        fs::create_dir_all(parent).with_context(|| format!("create {}", parent.display()))?;
    }
    fs::write(report_path, &report_bytes)
        .with_context(|| format!("write {}", report_path.display()))?;
    Ok(BattleSurfaceConstraintSummary {
        report_sha1: sha1_hex(&report_bytes),
        sample_count: evidence.samples.len(),
        runtime_tuple_count: selection.runtime_input_count,
        dynamic_assignment_catalog_sha1: dynamic_assignments.catalog_sha1,
        maximum_selected_color_count: dynamic_assignments.maximum_selected_color_count,
        maximum_remap_pair_count: dynamic_assignments.maximum_remap_pair_count,
    })
}

fn runtime_recipe_input(input: &ObservedBattleRuntimeInput) -> BattleRuntimeRecipeInput {
    BattleRuntimeRecipeInput {
        participant_record_identities: input.participant_record_identities,
        class_record_identities: input.class_record_identities,
        item_source_indices: input.item_source_indices,
        terrain_source_indices: input.terrain_source_indices,
        dialogue_selector: input.projected_dialogue_selector,
    }
}

fn validate_selection(selection: &BattleRuntimeRecipeSelection) -> Result<()> {
    ensure!(
        selection.recipe_offsets.len() == 10,
        "observed battle runtime tuple did not select ten recipe families"
    );
    ensure!(
        selection.overlays.len() <= ACTIVE_HANGUL_SLOT_COUNT,
        "observed battle runtime tuple exceeds the overlay slot ceiling"
    );
    ensure!(
        selection.glyphs.len() == selection.overlays.len(),
        "observed battle runtime tuple has inconsistent glyph and overlay counts"
    );
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn serialized_report_omits_translation_content_and_private_paths() {
        let report = BattleSurfaceConstraintReport {
            schema: 13,
            source_sha1: EXPECTED_SOURCE_SHA1,
            fixed_workspace_sha1: "fixed".to_owned(),
            dialogue_workspace_sha1: "dialogue".to_owned(),
            temporal_manifest_sha1: "temporal".to_owned(),
            observed_battle_sample_count: 1,
            observed_route_sample_counts: BTreeMap::from([("battle".to_owned(), 1)]),
            observed_runtime_tuple_count: 1,
            observed_dialogue_selector_count: 1,
            selector_62_predicate_sample_count: 1,
            selector_projection_changed_sample_count: 1,
            screen_constraint_count: 1,
            source_font_page_sha1: "page",
            source_page_japanese_text_active_code_count: 111,
            source_page_preserved_non_japanese_active_code_count: 99,
            background_producer_topology:
                super::super::background_ownership::BattleBackgroundProducerTopology {
                    primary_phase_count: 32,
                    primary_distinct_handler_count: 27,
                    unit_panel_phase_count: 12,
                    animation_phase_count: 41,
                    battle_switchable_bank_count: 2,
                    queue_publish_site_count: 17,
                    queue_publish_sites_sha1: "publishers".to_owned(),
                    direct_ppu_data_store_count: 0,
                    queue_ready_address_hex: "0x0021",
                    queue_buffer_address_hex: "0x0781",
                    queue_consumer_address_hex: "0xC3A5".to_owned(),
                    every_primary_phase_source_bound: true,
                    every_nested_phase_source_bound: true,
                    every_battle_bank_queue_publisher_classified: true,
                    battle_banks_have_no_direct_ppu_data_stores: true,
                    producer_topology_complete: true,
                    every_publisher_payload_source_bound: true,
                    conservative_global_preserved_code_union_complete: true,
                    simultaneous_preserved_code_demand_complete: false,
                },
            background_payload_model:
                super::super::background_payloads::BattleBackgroundPayloadModel::test_model(),
            phase_publisher_reachability:
                super::super::phase_cooccurrence::BattlePhasePublisherReachability::test_model(),
            text_consumer_topology:
                super::super::text_consumer_topology::BattleTextConsumerTopology::test_model(),
            remap_storage: super::super::remap_storage::test_model(),
            protected_color_placement: super::super::protected_color_placement::test_report(),
            physical_assignment_architecture: PhysicalAssignmentArchitecture {
                static_abstract_color_count: 210,
                active_physical_code_count: 210,
                borrowed_logical_code_count: 0,
                protected_abstract_color_count: 39,
                protected_physical_code_count: 39,
                safe_physical_code_count: 171,
                static_full_model_assignment_infeasible: true,
                conservative_per_battle_text_code_count: 134,
                conservative_per_battle_combined_code_count: 173,
                dynamic_per_battle_headroom: 37,
                exact_modeled_per_battle_text_code_count: 131,
                exact_modeled_per_battle_combined_code_count: 170,
                exact_modeled_per_battle_headroom: 40,
                arbitrary_selection_maximum_collision_count: 39,
                arbitrary_selection_direct_table_byte_count: 39,
                modeled_maximum_collision_count: 8,
                modeled_pair_table_byte_count: 17,
                identity_code_count_at_maximum_collision: 95,
                strongest_dynamic_assignment_sha1: "strongest".to_owned(),
                dynamic_per_battle_assignment_capacity_proven: true,
                selected_assignment_algorithm_complete: true,
                selected_strategy: "dynamic",
                per_battle_dynamic_assignment_is_release_architecture: true,
            },
            route_dynamic_assignment_coverage: vec![RouteDynamicAssignmentCoverage {
                route_role: "battle".to_owned(),
                sample_count: 1,
                every_sample_assignment_found: true,
                assignment_catalog_sha1: "physical".to_owned(),
                maximum_selected_color_count: 3,
                maximum_remap_pair_count: 1,
            }],
            minimum_observed_active_code_count: 2,
            maximum_observed_active_code_count: 3,
            minimum_observed_japanese_text_code_count: 1,
            maximum_observed_japanese_text_code_count: 2,
            minimum_preserved_non_japanese_active_code_count: 1,
            maximum_preserved_non_japanese_active_code_count: 2,
            maximum_selected_recipe_count: 10,
            maximum_selected_glyph_count: 3,
            maximum_selected_overlay_count: 3,
            nametable_constrained_sample_count: 1,
            visible_oam_constrained_sample_count: 0,
            abstract_assignment_sha1: "abstract".to_owned(),
            observed_dynamic_assignment_catalog_sha1: "physical".to_owned(),
            stable_color_count: 3,
            observed_maximum_selected_color_count: 3,
            observed_maximum_remap_pair_count: 1,
            conservative_text_overlay_count: 2,
            exact_modeled_text_overlay_count: 1,
            observed_maximum_combined_slot_demand: 4,
            observed_minimum_slot_headroom: 206,
            conservative_global_preserved_active_code_count: 39,
            conservative_global_combined_slot_demand: 41,
            conservative_global_minimum_slot_headroom: 169,
            exact_modeled_global_combined_slot_demand: 40,
            exact_modeled_global_minimum_slot_headroom: 170,
            temporal_sampling_is_irregular: true,
            pattern_table_consumers_filtered: true,
            source_page_code_ownership_applied: true,
            observed_japanese_codes_reclaimed_for_translation: true,
            every_observed_recipe_resolved: true,
            runtime_selection_uses_source_bound_dialogue_projection: true,
            every_observed_sample_has_dynamic_assignment: true,
            observed_route_catalog_complete: true,
            global_background_capacity_bound_complete: true,
            observed_dynamic_assignment_catalog_complete: true,
            glyph_characters_emitted: false,
            translation_text_emitted: false,
            runtime_verified: false,
            release_eligible: false,
            next_gate: "runtime",
        };

        let json = serde_json::to_string(&report).unwrap();
        assert!(!json.contains("private/"));
        assert!(!json.contains('한'));
        assert!(!json.contains("korean"));
    }
}
