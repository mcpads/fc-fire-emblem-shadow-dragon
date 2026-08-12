use std::{
    collections::{BTreeMap, BTreeSet},
    fs,
    path::Path,
};

use anyhow::{Context, Result, ensure};
use serde::Serialize;

use crate::{
    battle_text_workset::FORECAST_LABEL_GLYPHS,
    dialogue_assets::{BattleDialogueReinsertionPlan, plan_battle_dialogue_records},
    font_slots::{ACTIVE_HANGUL_SLOT_COUNT, active_hangul_codes},
    rom::{EXPECTED_SOURCE_SHA1, Rom},
    sha1_hex,
    text_inventory::{FixedTextPlan, plan_fixed_text},
};

use super::battle_combination_probe::GAMEPLAY_BATTLE_PRESERVED_ACTIVE_CODES;

mod background_ownership;
mod background_payloads;
mod composition;
mod conflict_graph;
mod enemy_domain;
mod item_domain;
mod phase_cooccurrence;
mod physical_assignment;
mod protected_color_placement;
mod remap_storage;
mod runtime_demand;
mod runtime_inputs;
mod selected_physical_assignment;
mod source_window;
pub(crate) mod surface_constraints;
mod text_consumer_topology;

pub(in crate::mapper165) use composition::{
    BattleCacheCompositionMaterial, BattleRuntimeRecipeInput, compose_runtime_font_page,
    inspect_runtime_recipe_input,
};
use composition::{BattleCacheCompositionPlan, plan_cache_composition};
use conflict_graph::{BattleGlyphFamilies, plan_stable_coloring};
use enemy_domain::{EnemyBattleDomain, EnemyBattleDomainBinding, bind_enemy_battle_domain};
use item_domain::{BattleItemDomain, BattleItemDomainBinding, bind_battle_item_domain};
pub(super) use physical_assignment::ScreenCodeConstraint;
use physical_assignment::assign_physical_codes;
use runtime_demand::{BattleRuntimeDemandPlan, ExactModeledRuntimeInput, plan_runtime_demand};
use runtime_inputs::{BattleRuntimeInputBinding, bind_battle_runtime_inputs};

struct BattleCodebookModel {
    coloring: conflict_graph::StableColoringPlan,
    glyph_families: BattleGlyphFamilies,
    message_template_entry_count: usize,
    unit_name_entry_count: usize,
    enemy_name_entry_count: usize,
    class_entry_count: usize,
    item_entry_count: usize,
    terrain_entry_count: usize,
    dialogue_record_count: usize,
    player_participant_candidate_count: usize,
    enemy_participant_candidate_count: usize,
    runtime_demand: BattleRuntimeDemandPlan,
    composition: BattleCacheCompositionMaterial,
    item_domain: BattleItemDomainBinding,
    enemy_domain: EnemyBattleDomainBinding,
}

pub(super) struct ConstrainedBattleCodebook {
    pub(super) glyph_codes: BTreeMap<char, u8>,
    pub(super) abstract_assignment_sha1: String,
    pub(super) physical_assignment_sha1: String,
    pub(super) stable_color_count: usize,
    pub(super) constrained_screen_count: usize,
    pub(super) constrained_color_count: usize,
}

pub(super) struct CanonicalBattleCodebook {
    pub(super) glyph_codes: BTreeMap<char, u8>,
    pub(super) color_codes: Vec<u8>,
    pub(super) protected_abstract_colors: Vec<u8>,
    pub(super) safe_abstract_colors: Vec<u8>,
    pub(super) abstract_assignment_sha1: String,
    pub(super) canonical_assignment_sha1: String,
    pub(super) stable_color_count: usize,
    pub(super) protected_physical_code_count: usize,
    pub(super) maximum_remap_pair_count: usize,
}

pub(crate) struct GlyphWorkset {
    pub(crate) target_glyphs: BTreeSet<char>,
    pub(crate) preserved_active_codes: BTreeSet<u8>,
}

pub(crate) struct GlyphWorksetCodebook {
    pub(crate) glyph_codes: BTreeMap<char, u8>,
    pub(crate) glyph_count: usize,
    pub(crate) conflict_edge_count: usize,
    pub(crate) constructed_clique_glyph_count: usize,
    pub(crate) stable_color_count: usize,
    pub(crate) abstract_assignment_sha1: String,
    pub(crate) physical_assignment_sha1: String,
    pub(crate) workset_count: usize,
    pub(crate) constrained_color_count: usize,
    pub(crate) active_ceiling_assignment_found: bool,
}

pub(crate) fn plan_glyph_workset_codebook(
    worksets: &[GlyphWorkset],
) -> Result<GlyphWorksetCodebook> {
    ensure!(!worksets.is_empty(), "glyph codebook has no worksets");
    let families = BattleGlyphFamilies {
        base: BTreeSet::new(),
        player_participants: Vec::new(),
        enemy_participants: Vec::new(),
        terrains: Vec::new(),
        dialogue_records: worksets
            .iter()
            .map(|workset| workset.target_glyphs.clone())
            .collect(),
    };
    let coloring = plan_stable_coloring(&families, ACTIVE_HANGUL_SLOT_COUNT)?;
    ensure!(
        coloring.active_ceiling_assignment_found
            && coloring.color_count <= ACTIVE_HANGUL_SLOT_COUNT,
        "complete glyph worksets need {} stable colors but only {} active slots exist",
        coloring.color_count,
        ACTIVE_HANGUL_SLOT_COUNT
    );
    let constraints = worksets
        .iter()
        .map(|workset| ScreenCodeConstraint {
            glyphs: workset.target_glyphs.clone(),
            preserved_active_codes: workset.preserved_active_codes.clone(),
        })
        .collect::<Vec<_>>();
    let physical = assign_physical_codes(&coloring, &constraints)?;
    Ok(GlyphWorksetCodebook {
        glyph_codes: physical.glyph_codes,
        glyph_count: coloring.glyph_count,
        conflict_edge_count: coloring.conflict_edge_count,
        constructed_clique_glyph_count: coloring.constructed_clique_glyph_count,
        stable_color_count: coloring.color_count,
        abstract_assignment_sha1: coloring.assignment_sha1,
        physical_assignment_sha1: physical.assignment_sha1,
        workset_count: physical.constrained_screen_count,
        constrained_color_count: physical.constrained_color_count,
        active_ceiling_assignment_found: coloring.active_ceiling_assignment_found,
    })
}

#[derive(Debug, Serialize)]
struct BattleCodebookPlanReport {
    schema: u8,
    source_sha1: &'static str,
    fixed_workspace_sha1: String,
    dialogue_workspace_sha1: String,
    cooccurrence_model: &'static str,
    message_template_entry_count: usize,
    unit_name_entry_count: usize,
    enemy_name_entry_count: usize,
    class_entry_count: usize,
    item_entry_count: usize,
    terrain_entry_count: usize,
    dialogue_record_count: usize,
    player_participant_candidate_count: usize,
    enemy_participant_candidate_count: usize,
    player_names_per_cache: usize,
    enemy_names_per_cache: usize,
    classes_per_cache: usize,
    items_per_cache: usize,
    terrains_per_cache: usize,
    dialogue_records_per_cache: usize,
    all_message_templates_per_cache: bool,
    forecast_label_per_cache: bool,
    glyph_vertex_count: usize,
    conflict_edge_count: usize,
    constructed_clique_glyph_count: usize,
    stable_color_count: usize,
    stable_assignment_sha1: String,
    coloring_strategy: &'static str,
    active_ceiling_search_node_count: usize,
    active_ceiling_search_limit_reached: bool,
    active_ceiling_assignment_found: bool,
    model_chromatic_number_proven: bool,
    active_slot_count: usize,
    chapter_one_preserved_active_code_count: usize,
    chapter_one_safe_target_code_count: usize,
    item_domain: BattleItemDomainBinding,
    enemy_domain: EnemyBattleDomainBinding,
    runtime_inputs: BattleRuntimeInputBinding,
    runtime_demand: BattleRuntimeDemandPlan,
    composition: BattleCacheCompositionPlan,
    stable_assignment_fits_active_slot_ceiling: bool,
    stable_assignment_fits_chapter_one_safe_codes: bool,
    model_active_slot_infeasibility_proven: bool,
    actual_battle_combination_graph_bound: bool,
    chapter_one_protected_set_generalized: bool,
    runtime_catalog_bound: bool,
    glyph_characters_emitted: bool,
    translation_text_emitted: bool,
    release_eligible: bool,
    next_gate: &'static str,
}

pub(crate) struct BattleCodebookPlanSummary {
    pub(crate) report_sha1: String,
    pub(crate) glyph_count: usize,
    pub(crate) conflict_edge_count: usize,
    pub(crate) constructed_clique_glyph_count: usize,
    pub(crate) stable_color_count: usize,
    pub(crate) chapter_one_safe_code_count: usize,
}

pub(crate) fn analyze_battle_codebook_plan(
    source_path: &Path,
    fixed_workspace_path: &Path,
    dialogue_workspace_path: &Path,
    report_path: &Path,
) -> Result<BattleCodebookPlanSummary> {
    let rom = Rom::from_path(source_path)?;
    rom.verify_supported_japanese()?;
    let fixed = plan_fixed_text(&rom, fixed_workspace_path)?;
    let dialogue = plan_battle_dialogue_records(&rom, dialogue_workspace_path)?;
    let model = plan_battle_codebook_model(&rom, &fixed, &dialogue)?;
    let BattleCodebookModel {
        coloring,
        glyph_families: _,
        message_template_entry_count,
        unit_name_entry_count,
        enemy_name_entry_count,
        class_entry_count,
        item_entry_count,
        terrain_entry_count,
        dialogue_record_count,
        player_participant_candidate_count,
        enemy_participant_candidate_count,
        runtime_demand,
        composition,
        item_domain,
        enemy_domain,
    } = model;
    ensure!(
        coloring.color_count <= ACTIVE_HANGUL_SLOT_COUNT,
        "renderer-complete battle codebook needs {} colors but only {} active slots exist",
        coloring.color_count,
        ACTIVE_HANGUL_SLOT_COUNT
    );
    let protected = GAMEPLAY_BATTLE_PRESERVED_ACTIVE_CODES
        .into_iter()
        .collect::<BTreeSet<_>>();
    let active = active_hangul_codes().into_iter().collect::<BTreeSet<_>>();
    ensure!(
        protected.is_subset(&active),
        "chapter-one battle protection includes a reserved font code"
    );
    let chapter_one_safe_code_count = active.difference(&protected).count();
    let runtime_inputs = bind_battle_runtime_inputs(&rom)?;
    let fixed_workspace_sha1 = sha1_hex(&fs::read(fixed_workspace_path)?);
    let dialogue_workspace_sha1 = sha1_hex(&fs::read(dialogue_workspace_path)?);
    let report = BattleCodebookPlanReport {
        schema: 3,
        source_sha1: EXPECTED_SOURCE_SHA1,
        fixed_workspace_sha1,
        dialogue_workspace_sha1,
        cooccurrence_model: "side-aware gameplay battle upper bound with source-bound item eligibility, enemy identities and items, plus every renderer-defined enemy class",
        message_template_entry_count,
        unit_name_entry_count,
        enemy_name_entry_count,
        class_entry_count,
        item_entry_count,
        terrain_entry_count,
        dialogue_record_count,
        player_participant_candidate_count,
        enemy_participant_candidate_count,
        player_names_per_cache: 1,
        enemy_names_per_cache: 1,
        classes_per_cache: 2,
        items_per_cache: 2,
        terrains_per_cache: 2,
        dialogue_records_per_cache: 1,
        all_message_templates_per_cache: true,
        forecast_label_per_cache: true,
        glyph_vertex_count: coloring.glyph_count,
        conflict_edge_count: coloring.conflict_edge_count,
        constructed_clique_glyph_count: coloring.constructed_clique_glyph_count,
        stable_color_count: coloring.color_count,
        stable_assignment_sha1: coloring.assignment_sha1.clone(),
        coloring_strategy: coloring.coloring_strategy,
        active_ceiling_search_node_count: coloring.active_ceiling_search_node_count,
        active_ceiling_search_limit_reached: coloring.active_ceiling_search_limit_reached,
        active_ceiling_assignment_found: coloring.active_ceiling_assignment_found,
        model_chromatic_number_proven: coloring.model_chromatic_number_proven,
        active_slot_count: ACTIVE_HANGUL_SLOT_COUNT,
        chapter_one_preserved_active_code_count: protected.len(),
        chapter_one_safe_target_code_count: chapter_one_safe_code_count,
        item_domain,
        enemy_domain,
        runtime_inputs,
        runtime_demand,
        composition: composition.plan,
        stable_assignment_fits_active_slot_ceiling: coloring.color_count
            <= ACTIVE_HANGUL_SLOT_COUNT,
        stable_assignment_fits_chapter_one_safe_codes: coloring.color_count
            <= chapter_one_safe_code_count,
        model_active_slot_infeasibility_proven: coloring.constructed_clique_glyph_count
            > ACTIVE_HANGUL_SLOT_COUNT,
        actual_battle_combination_graph_bound: true,
        chapter_one_protected_set_generalized: false,
        runtime_catalog_bound: true,
        glyph_characters_emitted: false,
        translation_text_emitted: false,
        release_eligible: false,
        next_gate: "bind the abstract colors to physical codes under every cache page protection constraint; selector 62 natural reachability remains a runtime proof gate",
    };
    let mut report_bytes =
        serde_json::to_vec_pretty(&report).context("serialize battle codebook plan")?;
    report_bytes.push(b'\n');
    if let Some(parent) = report_path.parent() {
        fs::create_dir_all(parent).with_context(|| format!("create {}", parent.display()))?;
    }
    fs::write(report_path, &report_bytes)
        .with_context(|| format!("write {}", report_path.display()))?;
    Ok(BattleCodebookPlanSummary {
        report_sha1: sha1_hex(&report_bytes),
        glyph_count: coloring.glyph_count,
        conflict_edge_count: coloring.conflict_edge_count,
        constructed_clique_glyph_count: coloring.constructed_clique_glyph_count,
        stable_color_count: coloring.color_count,
        chapter_one_safe_code_count,
    })
}

pub(super) fn plan_constrained_battle_codebook(
    rom: &Rom,
    fixed: &FixedTextPlan,
    dialogue: &BattleDialogueReinsertionPlan,
    constraints: &[ScreenCodeConstraint],
) -> Result<ConstrainedBattleCodebook> {
    let model = plan_battle_codebook_model(rom, fixed, dialogue)?;
    let physical = assign_physical_codes(&model.coloring, constraints)?;
    Ok(ConstrainedBattleCodebook {
        glyph_codes: physical.glyph_codes,
        abstract_assignment_sha1: model.coloring.assignment_sha1,
        physical_assignment_sha1: physical.assignment_sha1,
        stable_color_count: model.coloring.color_count,
        constrained_screen_count: physical.constrained_screen_count,
        constrained_color_count: physical.constrained_color_count,
    })
}

pub(super) fn plan_canonical_battle_codebook(
    rom: &Rom,
    fixed: &FixedTextPlan,
    dialogue: &BattleDialogueReinsertionPlan,
) -> Result<CanonicalBattleCodebook> {
    let model = plan_battle_codebook_model(rom, fixed, dialogue)?;
    let protected_physical_codes =
        background_ownership::bind_battle_background_code_ownership(rom)?
            .conservative_global_preserved_active_codes();
    let placement = protected_color_placement::plan_protected_color_placement(
        &model.glyph_families,
        &model.coloring,
        &protected_physical_codes,
    )?;
    let glyph_codes = model
        .coloring
        .glyph_colors()
        .iter()
        .map(|(glyph, color)| {
            Ok((
                *glyph,
                *placement
                    .canonical_color_codes
                    .get(*color)
                    .with_context(|| format!("canonical battle color {color} is absent"))?,
            ))
        })
        .collect::<Result<BTreeMap<_, _>>>()?;
    let protected_abstract_colors = placement
        .canonical_color_codes
        .iter()
        .enumerate()
        .filter(|(_, code)| protected_physical_codes.contains(code))
        .map(|(color, _)| {
            u8::try_from(color).context("protected battle abstract color exceeds one byte")
        })
        .collect::<Result<Vec<_>>>()?;
    let safe_abstract_colors = placement
        .canonical_color_codes
        .iter()
        .enumerate()
        .filter(|(_, code)| !protected_physical_codes.contains(code))
        .map(|(color, _)| {
            u8::try_from(color).context("safe battle abstract color exceeds one byte")
        })
        .collect::<Result<Vec<_>>>()?;
    ensure!(
        protected_abstract_colors.len() == protected_physical_codes.len()
            && safe_abstract_colors.len() + protected_abstract_colors.len()
                == placement.canonical_color_codes.len(),
        "canonical battle color partitions changed"
    );
    ensure!(
        placement.conservative_collision_count <= 8,
        "canonical battle placement exceeds the proven eight remap pairs"
    );

    Ok(CanonicalBattleCodebook {
        glyph_codes,
        canonical_assignment_sha1: sha1_hex(&placement.canonical_color_codes),
        color_codes: placement.canonical_color_codes,
        protected_abstract_colors,
        safe_abstract_colors,
        abstract_assignment_sha1: model.coloring.assignment_sha1,
        stable_color_count: model.coloring.color_count,
        protected_physical_code_count: protected_physical_codes.len(),
        maximum_remap_pair_count: placement.conservative_collision_count,
    })
}

pub(super) fn plan_battle_cache_composition_material(
    rom: &Rom,
    fixed: &FixedTextPlan,
    dialogue: &BattleDialogueReinsertionPlan,
) -> Result<BattleCacheCompositionMaterial> {
    Ok(plan_battle_codebook_model(rom, fixed, dialogue)?.composition)
}

fn plan_battle_codebook_model(
    rom: &Rom,
    fixed: &FixedTextPlan,
    dialogue: &BattleDialogueReinsertionPlan,
) -> Result<BattleCodebookModel> {
    let message_templates = entry_glyph_sets(fixed, "battle-message-templates");
    let player_names = entry_glyph_sets(fixed, "unit-names");
    let enemy_names = entry_glyph_sets(fixed, "enemy-names");
    let player_classes = entry_glyph_sets(fixed, "class-names");
    let BattleItemDomain {
        equip_candidate_item_glyph_sets: player_items,
        equip_candidate_item_source_indices,
        enemy_class_item_pairs,
        player_participant_glyph_sets: player_participants,
        player_participant_inputs,
        binding: item_domain,
    } = bind_battle_item_domain(rom, fixed)?;
    let EnemyBattleDomain {
        participant_glyph_sets: enemy_participants,
        participant_inputs: enemy_participant_inputs,
        enemy_name_source_indices,
        binding: enemy_domain,
    } = bind_enemy_battle_domain(rom, fixed, &enemy_class_item_pairs)?;
    let terrains = entry_glyph_sets(fixed, "terrain-names");
    for (role, entries) in [
        ("battle message", &message_templates),
        ("unit name", &player_names),
        ("enemy name", &enemy_names),
        ("class", &player_classes),
        ("item", &player_items),
        ("enemy participant", &enemy_participants),
        ("terrain", &terrains),
    ] {
        ensure!(!entries.is_empty(), "battle codebook has no {role} entries");
    }
    let base = always_selected_battle_glyphs(fixed);
    let dialogue_records = dialogue
        .records
        .iter()
        .map(|record| record.unique_glyphs())
        .collect::<Vec<_>>();
    ensure!(
        !dialogue_records.is_empty(),
        "battle codebook has no dialogue records"
    );
    let terrain_entry_count = terrains.len();
    let families = BattleGlyphFamilies {
        base,
        player_participants: player_participants.clone(),
        enemy_participants: enemy_participants.clone(),
        terrains,
        dialogue_records,
    };
    let mut coloring = plan_stable_coloring(&families, ACTIVE_HANGUL_SLOT_COUNT)?;
    coloring.expand_to_color_count(ACTIVE_HANGUL_SLOT_COUNT)?;
    let mut runtime_demand = plan_runtime_demand(&families, &coloring)?;
    let [
        player_index,
        enemy_index,
        terrain_left,
        terrain_right,
        dialogue_index,
    ] = runtime_demand.exact_witness_indices();
    let player_input = player_participant_inputs
        .get(player_index)
        .context("exact demand player witness is outside the participant domain")?;
    let enemy_input = enemy_participant_inputs
        .get(enemy_index)
        .context("exact demand enemy witness is outside the participant domain")?;
    let dialogue_selector = u8::try_from(
        dialogue
            .records
            .get(dialogue_index)
            .context("exact demand dialogue witness is outside the dialogue domain")?
            .canonical_entry_index,
    )
    .context("exact demand dialogue selector exceeds one byte")?;
    let exact_runtime_input = ExactModeledRuntimeInput {
        participant_record_identities: [player_input.identity, enemy_input.identity],
        class_record_identities: [player_input.class_id, enemy_input.class_id],
        item_source_indices: [
            player_input.item_source_index,
            enemy_input.item_source_index,
        ],
        terrain_source_indices: [
            u8::try_from(terrain_left).context("left terrain witness exceeds one byte")?,
            u8::try_from(terrain_right).context("right terrain witness exceeds one byte")?,
        ],
        dialogue_selector,
    };
    let composition = plan_cache_composition(
        fixed,
        dialogue,
        &coloring,
        &equip_candidate_item_source_indices,
        &enemy_name_source_indices,
        player_participants.len(),
        enemy_participants.len(),
        terrain_entry_count,
        runtime_demand.maximum_overlay_glyph_count(),
    )?;
    let exact_selection = composition.select_runtime_recipes(BattleRuntimeRecipeInput {
        participant_record_identities: exact_runtime_input.participant_record_identities,
        class_record_identities: exact_runtime_input.class_record_identities,
        item_source_indices: exact_runtime_input.item_source_indices,
        terrain_source_indices: exact_runtime_input.terrain_source_indices,
        dialogue_selector: exact_runtime_input.dialogue_selector,
    })?;
    ensure!(
        exact_selection.overlays.len() == runtime_demand.exact_maximum_overlay_glyph_count(),
        "exact demand witness selects {} runtime overlays instead of {}",
        exact_selection.overlays.len(),
        runtime_demand.exact_maximum_overlay_glyph_count()
    );
    runtime_demand.bind_exact_runtime_input(exact_runtime_input)?;
    Ok(BattleCodebookModel {
        message_template_entry_count: message_templates.len(),
        unit_name_entry_count: player_names.len(),
        enemy_name_entry_count: enemy_names.len(),
        class_entry_count: player_classes.len(),
        item_entry_count: player_items.len(),
        terrain_entry_count,
        dialogue_record_count: dialogue.records.len(),
        player_participant_candidate_count: player_participants.len(),
        enemy_participant_candidate_count: enemy_participants.len(),
        coloring,
        glyph_families: families,
        runtime_demand,
        composition,
        item_domain,
        enemy_domain,
    })
}

fn always_selected_battle_glyphs(fixed: &FixedTextPlan) -> BTreeSet<char> {
    let mut glyphs = fixed
        .entries
        .iter()
        .filter(|entry| entry.table_id == "battle-message-templates")
        .flat_map(|entry| entry.unique_glyphs())
        .collect::<BTreeSet<_>>();
    glyphs.extend(FORECAST_LABEL_GLYPHS);
    glyphs
}

fn entry_glyph_sets(plan: &FixedTextPlan, table_id: &str) -> Vec<BTreeSet<char>> {
    plan.entries
        .iter()
        .filter(|entry| entry.table_id == table_id)
        .map(|entry| entry.unique_glyphs())
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn glyphs(text: &str) -> BTreeSet<char> {
        text.chars().collect()
    }

    #[test]
    fn workset_codebook_reuses_codes_only_across_noncooccurring_glyphs() {
        let active = active_hangul_codes();
        let worksets = [
            GlyphWorkset {
                target_glyphs: glyphs("가나"),
                preserved_active_codes: BTreeSet::from([active[0]]),
            },
            GlyphWorkset {
                target_glyphs: glyphs("가다"),
                preserved_active_codes: BTreeSet::from([active[1]]),
            },
        ];

        let first = plan_glyph_workset_codebook(&worksets).unwrap();
        let second = plan_glyph_workset_codebook(&worksets).unwrap();

        assert_ne!(first.glyph_codes[&'가'], first.glyph_codes[&'나']);
        assert_ne!(first.glyph_codes[&'가'], first.glyph_codes[&'다']);
        assert_eq!(first.glyph_codes[&'나'], first.glyph_codes[&'다']);
        assert_ne!(first.glyph_codes[&'가'], active[0]);
        assert_ne!(first.glyph_codes[&'가'], active[1]);
        assert_eq!(
            first.abstract_assignment_sha1,
            second.abstract_assignment_sha1
        );
        assert_eq!(
            first.physical_assignment_sha1,
            second.physical_assignment_sha1
        );
        assert_eq!(first.workset_count, worksets.len());
    }

    #[test]
    fn report_does_not_emit_translation_content_or_private_paths() {
        let report = BattleCodebookPlanReport {
            schema: 3,
            source_sha1: EXPECTED_SOURCE_SHA1,
            fixed_workspace_sha1: "fixed".to_owned(),
            dialogue_workspace_sha1: "dialogue".to_owned(),
            cooccurrence_model: "family coverage",
            message_template_entry_count: 22,
            unit_name_entry_count: 52,
            enemy_name_entry_count: 69,
            class_entry_count: 22,
            item_entry_count: 64,
            terrain_entry_count: 15,
            dialogue_record_count: 28,
            player_participant_candidate_count: 100,
            enemy_participant_candidate_count: 100,
            player_names_per_cache: 1,
            enemy_names_per_cache: 1,
            classes_per_cache: 2,
            items_per_cache: 2,
            terrains_per_cache: 2,
            dialogue_records_per_cache: 1,
            all_message_templates_per_cache: true,
            forecast_label_per_cache: true,
            glyph_vertex_count: 299,
            conflict_edge_count: 1,
            constructed_clique_glyph_count: 1,
            stable_color_count: 1,
            stable_assignment_sha1: "assignment".to_owned(),
            coloring_strategy: "test",
            active_ceiling_search_node_count: 1,
            active_ceiling_search_limit_reached: false,
            active_ceiling_assignment_found: true,
            model_chromatic_number_proven: true,
            active_slot_count: ACTIVE_HANGUL_SLOT_COUNT,
            chapter_one_preserved_active_code_count: 119,
            chapter_one_safe_target_code_count: 91,
            item_domain: item_domain::test_binding(),
            enemy_domain: enemy_domain::test_binding(),
            runtime_inputs: runtime_inputs::test_binding(),
            runtime_demand: runtime_demand::test_plan(),
            composition: composition::test_plan(),
            stable_assignment_fits_active_slot_ceiling: true,
            stable_assignment_fits_chapter_one_safe_codes: true,
            model_active_slot_infeasibility_proven: false,
            actual_battle_combination_graph_bound: false,
            chapter_one_protected_set_generalized: false,
            runtime_catalog_bound: false,
            glyph_characters_emitted: false,
            translation_text_emitted: false,
            release_eligible: false,
            next_gate: "runtime binding",
        };

        let json = serde_json::to_string(&report).unwrap();
        assert!(!json.contains("private/"));
        assert!(!json.contains('한'));
        assert!(!json.contains("korean"));
    }
}
