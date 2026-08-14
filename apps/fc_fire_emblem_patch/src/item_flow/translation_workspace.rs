use std::path::Path;

use anyhow::{Result, ensure};

use crate::{
    rom::Rom,
    semantic_translation::{
        ExpectedSemanticEntry, SemanticTranslationPlan, plan_semantic_translation,
    },
};

use super::{inspect_item_action_label_count, source_contract::ITEM_ACTION_LABELS};

pub(crate) fn plan_item_action_labels(
    rom: &Rom,
    workspace_path: &Path,
) -> Result<SemanticTranslationPlan> {
    let expected_count = inspect_item_action_label_count(rom)?;
    let expected_entries = ITEM_ACTION_LABELS
        .iter()
        .filter(|spec| spec.translation_scope == "japanese_only")
        .map(|spec| ExpectedSemanticEntry {
            id: format!("item-action-label:{:02X}", spec.index),
            japanese_markup: spec.source_text.to_owned(),
            max_visible_cells: (spec.expected.len() - 1).max(6),
        })
        .collect::<Vec<_>>();
    ensure!(
        expected_entries.len() == expected_count,
        "item action semantic population changed"
    );
    plan_semantic_translation(workspace_path, &expected_entries)
}
