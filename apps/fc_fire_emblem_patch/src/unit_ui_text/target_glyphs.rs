use std::{collections::BTreeSet, path::Path};

use anyhow::{Context, Result, ensure};

use crate::{
    rom::Rom,
    semantic_translation::SemanticTranslationPlan,
    text_inventory::{FixedTextPlan, FixedTextPlannedEntry, plan_fixed_text},
    unit_names::plan_unit_names,
};

use super::{SUMMARY_AND_STATUS_LABEL_SPECS, command_menu, plan_unit_ui_labels};

const MAXIMUM_VISIBLE_ITEM_COUNT: usize = 4;

pub(super) struct TargetGlyphBudget {
    pub(super) fixed_text_workspace_sha1: String,
    pub(super) unit_name_workspace_sha1: String,
    pub(super) unit_ui_label_workspace_sha1: String,
    pub(super) translation_inputs_review_complete: bool,
    pub(super) all_family_unique_glyph_count: usize,
    pub(super) summary_status_family_unique_glyph_count: usize,
    pub(super) command_family_unique_glyph_count: usize,
    pub(super) maximum_unit_or_enemy_name_glyph_count: usize,
    pub(super) maximum_class_name_glyph_count: usize,
    pub(super) maximum_item_name_glyph_count: usize,
    pub(super) level_label_unique_glyph_count: usize,
    pub(super) summary_status_label_unique_glyph_count: usize,
    pub(super) summary_target_glyph_upper_bound: usize,
    pub(super) status_target_glyph_upper_bound: usize,
    pub(super) command_target_glyph_upper_bound: usize,
}

pub(super) fn plan_target_glyph_budget(
    rom: &Rom,
    fixed_text_workspace_path: &Path,
    unit_name_workspace_path: &Path,
    unit_ui_label_workspace_path: &Path,
) -> Result<TargetGlyphBudget> {
    let fixed_text = plan_fixed_text(rom, fixed_text_workspace_path)?;
    let unit_names = plan_unit_names(rom, unit_name_workspace_path)?;
    let unit_ui_labels = plan_unit_ui_labels(rom, unit_ui_label_workspace_path)?;

    let enemy_names = table_entries(&fixed_text, "enemy-names")?;
    let class_names = table_entries(&fixed_text, "class-names")?;
    let item_names = table_entries(&fixed_text, "item-names")?;
    let maximum_unit_or_enemy_name_glyph_count = unit_names
        .entries
        .iter()
        .map(FixedTextPlannedEntry::unique_glyphs)
        .chain(enemy_names.iter().map(|entry| entry.unique_glyphs()))
        .map(|glyphs| glyphs.len())
        .max()
        .context("unit UI has no unit or enemy names")?;
    let maximum_class_name_glyph_count = maximum_entry_glyph_count(&class_names)?;
    let maximum_item_name_glyph_count = maximum_entry_glyph_count(&item_names)?;

    let level_label_glyphs = label_glyphs(&unit_ui_labels, &[0x08])?;
    let summary_status_label_indices = SUMMARY_AND_STATUS_LABEL_SPECS
        .iter()
        .filter(|spec| spec.translation_scope == "japanese_only")
        .map(|spec| spec.index)
        .collect::<Vec<_>>();
    let summary_status_label_glyphs = label_glyphs(&unit_ui_labels, &summary_status_label_indices)?;
    let command_label_indices = command_menu::COMMAND_LABEL_SPECS
        .iter()
        .map(|spec| spec.index)
        .collect::<Vec<_>>();
    let command_label_glyphs = label_glyphs(&unit_ui_labels, &command_label_indices)?;

    let unit_name_glyphs = unit_names.unique_glyphs();
    let enemy_name_glyphs = union_entry_glyphs(&enemy_names);
    let class_name_glyphs = union_entry_glyphs(&class_names);
    let item_name_glyphs = union_entry_glyphs(&item_names);
    let summary_status_family_glyphs = union_sets([
        &unit_name_glyphs,
        &enemy_name_glyphs,
        &class_name_glyphs,
        &item_name_glyphs,
        &summary_status_label_glyphs,
    ]);
    let all_family_glyphs = union_sets([&summary_status_family_glyphs, &command_label_glyphs]);
    let all_label_glyphs = unit_ui_labels.unique_target_glyphs();
    ensure!(
        all_label_glyphs.is_subset(&all_family_glyphs),
        "unit UI label glyphs escaped the complete family union"
    );

    let summary_target_glyph_upper_bound = maximum_unit_or_enemy_name_glyph_count
        .checked_add(maximum_class_name_glyph_count)
        .and_then(|count| {
            maximum_item_name_glyph_count
                .checked_mul(MAXIMUM_VISIBLE_ITEM_COUNT)
                .and_then(|items| count.checked_add(items))
        })
        .and_then(|count| count.checked_add(level_label_glyphs.len()))
        .context("unit-summary target glyph upper bound overflow")?;
    let status_target_glyph_upper_bound = maximum_unit_or_enemy_name_glyph_count
        .checked_add(maximum_class_name_glyph_count)
        .and_then(|count| count.checked_add(summary_status_label_glyphs.len()))
        .context("unit-status target glyph upper bound overflow")?;

    Ok(TargetGlyphBudget {
        fixed_text_workspace_sha1: fixed_text.workspace_sha1,
        unit_name_workspace_sha1: unit_names.workspace_sha1,
        unit_ui_label_workspace_sha1: unit_ui_labels.workspace_sha1,
        translation_inputs_review_complete: fixed_text.review_complete
            && unit_names.review_complete
            && unit_ui_labels.review_complete,
        all_family_unique_glyph_count: all_family_glyphs.len(),
        summary_status_family_unique_glyph_count: summary_status_family_glyphs.len(),
        command_family_unique_glyph_count: command_label_glyphs.len(),
        maximum_unit_or_enemy_name_glyph_count,
        maximum_class_name_glyph_count,
        maximum_item_name_glyph_count,
        level_label_unique_glyph_count: level_label_glyphs.len(),
        summary_status_label_unique_glyph_count: summary_status_label_glyphs.len(),
        summary_target_glyph_upper_bound,
        status_target_glyph_upper_bound,
        command_target_glyph_upper_bound: command_label_glyphs.len(),
    })
}

fn table_entries<'a>(
    fixed_text: &'a FixedTextPlan,
    table_id: &str,
) -> Result<Vec<&'a FixedTextPlannedEntry>> {
    let entries = fixed_text
        .entries
        .iter()
        .filter(|entry| entry.table_id == table_id)
        .collect::<Vec<_>>();
    ensure!(!entries.is_empty(), "unit UI table {table_id} is empty");
    Ok(entries)
}

fn maximum_entry_glyph_count(entries: &[&FixedTextPlannedEntry]) -> Result<usize> {
    entries
        .iter()
        .map(|entry| entry.unique_glyphs().len())
        .max()
        .context("unit UI target table is empty")
}

fn union_entry_glyphs(entries: &[&FixedTextPlannedEntry]) -> BTreeSet<char> {
    entries
        .iter()
        .flat_map(|entry| entry.unique_glyphs())
        .collect()
}

fn label_glyphs(plan: &SemanticTranslationPlan, indices: &[u8]) -> Result<BTreeSet<char>> {
    let mut glyphs = BTreeSet::new();
    for index in indices {
        let id = format!("unit-ui-label:{index:02X}");
        glyphs.extend(
            plan.entry_target_glyphs(&id)
                .with_context(|| format!("unit UI translation lost {id}"))?,
        );
    }
    Ok(glyphs)
}

fn union_sets<const N: usize>(sets: [&BTreeSet<char>; N]) -> BTreeSet<char> {
    sets.into_iter()
        .flat_map(|set| set.iter().copied())
        .collect()
}
