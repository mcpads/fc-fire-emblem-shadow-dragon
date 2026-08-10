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
    composition::{BattleRuntimeRecipeInput, BattleRuntimeRecipeSelection},
    physical_assignment::assign_physical_codes,
    plan_battle_codebook_model,
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
    route_assignment_feasibility: Vec<RouteAssignmentFeasibility>,
    gameplay_routes_combined_assignment: RouteAssignmentFeasibility,
    minimum_preserved_active_code_count: usize,
    maximum_preserved_active_code_count: usize,
    maximum_selected_recipe_count: usize,
    maximum_selected_glyph_count: usize,
    maximum_selected_overlay_count: usize,
    nametable_constrained_sample_count: usize,
    visible_oam_constrained_sample_count: usize,
    abstract_assignment_sha1: String,
    physical_assignment_sha1: Option<String>,
    stable_color_count: usize,
    constrained_color_count: Option<usize>,
    temporal_sampling_is_irregular: bool,
    pattern_table_consumers_filtered: bool,
    every_observed_recipe_resolved: bool,
    runtime_selection_uses_source_bound_dialogue_projection: bool,
    combined_physical_assignment_found: bool,
    observed_route_catalog_complete: bool,
    physical_assignment_catalog_complete: bool,
    glyph_characters_emitted: bool,
    translation_text_emitted: bool,
    runtime_verified: bool,
    release_eligible: bool,
    next_gate: &'static str,
}

#[derive(Debug, Serialize)]
struct RouteAssignmentFeasibility {
    route_role: String,
    sample_count: usize,
    physical_assignment_found: bool,
    physical_assignment_sha1: Option<String>,
    constrained_color_count: Option<usize>,
}

pub(crate) struct BattleSurfaceConstraintSummary {
    pub(crate) report_sha1: String,
    pub(crate) sample_count: usize,
    pub(crate) runtime_tuple_count: usize,
    pub(crate) physical_assignment_sha1: Option<String>,
    pub(crate) constrained_color_count: Option<usize>,
}

pub(in crate::mapper165) struct ObservedBattleSurfaceSelection {
    pub(in crate::mapper165) constraints: Vec<(String, ScreenCodeConstraint)>,
    route_sample_counts: BTreeMap<String, usize>,
    pub(in crate::mapper165) runtime_input_count: usize,
    observed_dialogue_selector_count: usize,
    selector_62_predicate_sample_count: usize,
    selector_projection_changed_sample_count: usize,
    minimum_preserved_active_code_count: usize,
    maximum_preserved_active_code_count: usize,
    maximum_selected_recipe_count: usize,
    maximum_selected_glyph_count: usize,
    pub(in crate::mapper165) maximum_selected_overlay_count: usize,
    nametable_constrained_sample_count: usize,
    visible_oam_constrained_sample_count: usize,
}

pub(in crate::mapper165) fn select_observed_battle_surfaces(
    material: &super::composition::BattleCacheCompositionMaterial,
    evidence: &crate::temporal_surface::ObservedBattleTemporalEvidence,
) -> Result<ObservedBattleSurfaceSelection> {
    let mut route_sample_counts = BTreeMap::new();
    let mut runtime_inputs = BTreeSet::new();
    let mut observed_dialogue_selectors = BTreeSet::new();
    let mut constraints = Vec::with_capacity(evidence.samples.len());
    let mut minimum_preserved_active_code_count = usize::MAX;
    let mut maximum_preserved_active_code_count = 0;
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
        minimum_preserved_active_code_count =
            minimum_preserved_active_code_count.min(sample.active_tile_codes.len());
        maximum_preserved_active_code_count =
            maximum_preserved_active_code_count.max(sample.active_tile_codes.len());
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
                preserved_active_codes: sample.active_tile_codes.clone(),
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
        minimum_preserved_active_code_count,
        maximum_preserved_active_code_count,
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

    let selection = select_observed_battle_surfaces(&model.composition, &evidence)?;
    let constraints = selection.constraints;
    let route_sample_counts = selection.route_sample_counts;
    let minimum_preserved_active_code_count = selection.minimum_preserved_active_code_count;
    let maximum_preserved_active_code_count = selection.maximum_preserved_active_code_count;
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
    let route_assignment_feasibility = route_sample_counts
        .iter()
        .map(|(route_role, sample_count)| {
            let route_constraints = constraints
                .iter()
                .filter(|(candidate, _)| candidate == route_role)
                .map(|(_, constraint)| ScreenCodeConstraint {
                    glyphs: constraint.glyphs.clone(),
                    preserved_active_codes: constraint.preserved_active_codes.clone(),
                })
                .collect::<Vec<_>>();
            let physical = assign_physical_codes(&model.coloring, &route_constraints).ok();
            RouteAssignmentFeasibility {
                route_role: route_role.clone(),
                sample_count: *sample_count,
                physical_assignment_found: physical.is_some(),
                physical_assignment_sha1: physical
                    .as_ref()
                    .map(|assignment| assignment.assignment_sha1.clone()),
                constrained_color_count: physical
                    .as_ref()
                    .map(|assignment| assignment.constrained_color_count),
            }
        })
        .collect::<Vec<_>>();
    let gameplay_constraints = constraints
        .iter()
        .filter(|(route_role, _)| route_role.starts_with("gameplay_battle_"))
        .map(|(_, constraint)| ScreenCodeConstraint {
            glyphs: constraint.glyphs.clone(),
            preserved_active_codes: constraint.preserved_active_codes.clone(),
        })
        .collect::<Vec<_>>();
    let gameplay_physical = assign_physical_codes(&model.coloring, &gameplay_constraints).ok();
    let gameplay_routes_combined_assignment = RouteAssignmentFeasibility {
        route_role: "gameplay_battle_polarities_combined".to_owned(),
        sample_count: gameplay_constraints.len(),
        physical_assignment_found: gameplay_physical.is_some(),
        physical_assignment_sha1: gameplay_physical
            .as_ref()
            .map(|assignment| assignment.assignment_sha1.clone()),
        constrained_color_count: gameplay_physical
            .as_ref()
            .map(|assignment| assignment.constrained_color_count),
    };
    let all_constraints = constraints
        .iter()
        .map(|(_, constraint)| ScreenCodeConstraint {
            glyphs: constraint.glyphs.clone(),
            preserved_active_codes: constraint.preserved_active_codes.clone(),
        })
        .collect::<Vec<_>>();
    let physical = assign_physical_codes(&model.coloring, &all_constraints).ok();
    let physical_assignment_sha1 = physical
        .as_ref()
        .map(|assignment| assignment.assignment_sha1.clone());
    let constrained_color_count = physical
        .as_ref()
        .map(|assignment| assignment.constrained_color_count);
    let report = BattleSurfaceConstraintReport {
        schema: 1,
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
        route_assignment_feasibility,
        gameplay_routes_combined_assignment,
        minimum_preserved_active_code_count,
        maximum_preserved_active_code_count,
        maximum_selected_recipe_count,
        maximum_selected_glyph_count,
        maximum_selected_overlay_count,
        nametable_constrained_sample_count,
        visible_oam_constrained_sample_count,
        abstract_assignment_sha1: model.coloring.assignment_sha1,
        physical_assignment_sha1: physical_assignment_sha1.clone(),
        stable_color_count: model.coloring.color_count,
        constrained_color_count,
        temporal_sampling_is_irregular: true,
        pattern_table_consumers_filtered: true,
        every_observed_recipe_resolved: true,
        runtime_selection_uses_source_bound_dialogue_projection: true,
        combined_physical_assignment_found: physical.is_some(),
        observed_route_catalog_complete: true,
        physical_assignment_catalog_complete: false,
        glyph_characters_emitted: false,
        translation_text_emitted: false,
        runtime_verified: false,
        release_eligible: false,
        next_gate: if physical.is_some() {
            "extend protection constraints from the admitted chapter-one and sound-test samples to the remaining battle visual variants, then install the runtime composition loader"
        } else {
            "separate translated text producers from preserved graphics in the admitted temporal samples before extending the visual-variant catalog or installing the runtime loader"
        },
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
        physical_assignment_sha1,
        constrained_color_count,
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
            schema: 1,
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
            route_assignment_feasibility: vec![RouteAssignmentFeasibility {
                route_role: "battle".to_owned(),
                sample_count: 1,
                physical_assignment_found: true,
                physical_assignment_sha1: Some("physical".to_owned()),
                constrained_color_count: Some(3),
            }],
            gameplay_routes_combined_assignment: RouteAssignmentFeasibility {
                route_role: "gameplay_battle_polarities_combined".to_owned(),
                sample_count: 1,
                physical_assignment_found: true,
                physical_assignment_sha1: Some("physical".to_owned()),
                constrained_color_count: Some(3),
            },
            minimum_preserved_active_code_count: 1,
            maximum_preserved_active_code_count: 2,
            maximum_selected_recipe_count: 10,
            maximum_selected_glyph_count: 3,
            maximum_selected_overlay_count: 3,
            nametable_constrained_sample_count: 1,
            visible_oam_constrained_sample_count: 0,
            abstract_assignment_sha1: "abstract".to_owned(),
            physical_assignment_sha1: Some("physical".to_owned()),
            stable_color_count: 3,
            constrained_color_count: Some(3),
            temporal_sampling_is_irregular: true,
            pattern_table_consumers_filtered: true,
            every_observed_recipe_resolved: true,
            runtime_selection_uses_source_bound_dialogue_projection: true,
            combined_physical_assignment_found: true,
            observed_route_catalog_complete: true,
            physical_assignment_catalog_complete: false,
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
