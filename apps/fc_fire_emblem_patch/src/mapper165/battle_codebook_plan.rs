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

mod composition;
mod conflict_graph;
mod enemy_domain;
mod item_domain;
mod physical_assignment;
mod runtime_inputs;

use composition::{
    BattleCacheCompositionMaterial, BattleCacheCompositionPlan, plan_cache_composition,
};
use conflict_graph::{BattleGlyphFamilies, plan_stable_coloring};
use enemy_domain::{EnemyBattleDomain, EnemyBattleDomainBinding, bind_enemy_battle_domain};
use item_domain::{BattleItemDomain, BattleItemDomainBinding, bind_battle_item_domain};
pub(super) use physical_assignment::ScreenCodeConstraint;
use physical_assignment::assign_physical_codes;
use runtime_inputs::{BattleRuntimeInputBinding, bind_battle_runtime_inputs};

struct BattleCodebookModel {
    coloring: conflict_graph::StableColoringPlan,
    message_template_entry_count: usize,
    unit_name_entry_count: usize,
    enemy_name_entry_count: usize,
    class_entry_count: usize,
    item_entry_count: usize,
    terrain_entry_count: usize,
    dialogue_record_count: usize,
    player_participant_candidate_count: usize,
    enemy_participant_candidate_count: usize,
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
        message_template_entry_count,
        unit_name_entry_count,
        enemy_name_entry_count,
        class_entry_count,
        item_entry_count,
        terrain_entry_count,
        dialogue_record_count,
        player_participant_candidate_count,
        enemy_participant_candidate_count,
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
        schema: 2,
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
        binding: item_domain,
    } = bind_battle_item_domain(rom, fixed)?;
    let EnemyBattleDomain {
        participant_glyph_sets: enemy_participants,
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
    let mut base = message_templates
        .iter()
        .flatten()
        .copied()
        .collect::<BTreeSet<_>>();
    base.extend(FORECAST_LABEL_GLYPHS);
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
    let coloring = plan_stable_coloring(
        &BattleGlyphFamilies {
            base,
            player_participants: player_participants.clone(),
            enemy_participants: enemy_participants.clone(),
            terrains,
            dialogue_records,
        },
        ACTIVE_HANGUL_SLOT_COUNT,
    )?;
    let composition = plan_cache_composition(
        fixed,
        dialogue,
        &coloring,
        &equip_candidate_item_source_indices,
        &enemy_name_source_indices,
        player_participants.len(),
        enemy_participants.len(),
        terrain_entry_count,
    )?;
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
        composition,
        item_domain,
        enemy_domain,
    })
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

    #[test]
    fn report_does_not_emit_translation_content_or_private_paths() {
        let report = BattleCodebookPlanReport {
            schema: 2,
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
