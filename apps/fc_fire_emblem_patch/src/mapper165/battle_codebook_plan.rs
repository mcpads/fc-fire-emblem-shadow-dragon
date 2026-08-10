use std::{collections::BTreeSet, fs, path::Path};

use anyhow::{Context, Result, ensure};
use serde::Serialize;

use crate::{
    battle_text_workset::FORECAST_LABEL_GLYPHS,
    dialogue_assets::plan_battle_dialogue_records,
    font_slots::{ACTIVE_HANGUL_SLOT_COUNT, active_hangul_codes},
    rom::{EXPECTED_SOURCE_SHA1, Rom},
    sha1_hex,
    text_inventory::{FixedTextPlan, plan_fixed_text},
};

use super::battle_combination_probe::GAMEPLAY_BATTLE_PRESERVED_ACTIVE_CODES;

mod conflict_graph;

use conflict_graph::{BattleGlyphFamilies, plan_stable_coloring};

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
    active_slot_count: usize,
    chapter_one_preserved_active_code_count: usize,
    chapter_one_safe_target_code_count: usize,
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
    let message_templates = entry_glyph_sets(&fixed, "battle-message-templates");
    let unit_names = entry_glyph_sets(&fixed, "unit-names");
    let enemy_names = entry_glyph_sets(&fixed, "enemy-names");
    let classes = entry_glyph_sets(&fixed, "class-names");
    let items = entry_glyph_sets(&fixed, "item-names");
    let terrains = entry_glyph_sets(&fixed, "terrain-names");
    for (role, entries) in [
        ("battle message", &message_templates),
        ("unit name", &unit_names),
        ("enemy name", &enemy_names),
        ("class", &classes),
        ("item", &items),
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
    let coloring = plan_stable_coloring(&BattleGlyphFamilies {
        base,
        unit_names,
        enemy_names,
        classes,
        items,
        terrains,
        dialogue_records,
    })?;
    let protected = GAMEPLAY_BATTLE_PRESERVED_ACTIVE_CODES
        .into_iter()
        .collect::<BTreeSet<_>>();
    let active = active_hangul_codes().into_iter().collect::<BTreeSet<_>>();
    ensure!(
        protected.is_subset(&active),
        "chapter-one battle protection includes a reserved font code"
    );
    let chapter_one_safe_code_count = active.difference(&protected).count();
    let fixed_workspace_sha1 = sha1_hex(&fs::read(fixed_workspace_path)?);
    let dialogue_workspace_sha1 = sha1_hex(&fs::read(dialogue_workspace_path)?);
    let report = BattleCodebookPlanReport {
        schema: 1,
        source_sha1: EXPECTED_SOURCE_SHA1,
        fixed_workspace_sha1,
        dialogue_workspace_sha1,
        cooccurrence_model: "conservative battle-cache family coverage",
        message_template_entry_count: message_templates.len(),
        unit_name_entry_count: coloring.family_entry_counts.unit_names,
        enemy_name_entry_count: coloring.family_entry_counts.enemy_names,
        class_entry_count: coloring.family_entry_counts.classes,
        item_entry_count: coloring.family_entry_counts.items,
        terrain_entry_count: coloring.family_entry_counts.terrains,
        dialogue_record_count: coloring.family_entry_counts.dialogue_records,
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
        stable_assignment_sha1: coloring.assignment_sha1,
        active_slot_count: ACTIVE_HANGUL_SLOT_COUNT,
        chapter_one_preserved_active_code_count: protected.len(),
        chapter_one_safe_target_code_count: chapter_one_safe_code_count,
        stable_assignment_fits_active_slot_ceiling: coloring.color_count
            <= ACTIVE_HANGUL_SLOT_COUNT,
        stable_assignment_fits_chapter_one_safe_codes: coloring.color_count
            <= chapter_one_safe_code_count,
        model_active_slot_infeasibility_proven: coloring.constructed_clique_glyph_count
            > ACTIVE_HANGUL_SLOT_COUNT,
        actual_battle_combination_graph_bound: false,
        chapter_one_protected_set_generalized: false,
        runtime_catalog_bound: false,
        glyph_characters_emitted: false,
        translation_text_emitted: false,
        release_eligible: false,
        next_gate: "bind the actual battle-combination graph; if its clique lower bound still exceeds active slots, store cache-owned encoded text with each glyph page",
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
            schema: 1,
            source_sha1: EXPECTED_SOURCE_SHA1,
            fixed_workspace_sha1: "fixed".to_owned(),
            dialogue_workspace_sha1: "dialogue".to_owned(),
            cooccurrence_model: "family coverage",
            message_template_entry_count: 22,
            unit_name_entry_count: 52,
            enemy_name_entry_count: 68,
            class_entry_count: 22,
            item_entry_count: 91,
            terrain_entry_count: 15,
            dialogue_record_count: 28,
            player_names_per_cache: 1,
            enemy_names_per_cache: 1,
            classes_per_cache: 2,
            items_per_cache: 2,
            terrains_per_cache: 2,
            dialogue_records_per_cache: 1,
            all_message_templates_per_cache: true,
            forecast_label_per_cache: true,
            glyph_vertex_count: 319,
            conflict_edge_count: 1,
            constructed_clique_glyph_count: 1,
            stable_color_count: 1,
            stable_assignment_sha1: "assignment".to_owned(),
            active_slot_count: ACTIVE_HANGUL_SLOT_COUNT,
            chapter_one_preserved_active_code_count: 119,
            chapter_one_safe_target_code_count: 91,
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
