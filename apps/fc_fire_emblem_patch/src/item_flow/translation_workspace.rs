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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn public_workspace_covers_all_four_japanese_item_actions() {
        let source = Path::new(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../../roms/Fire Emblem - Ankoku Ryuu to Hikari no Tsurugi (Japan).nes"
        ));
        if !source.exists() {
            return;
        }
        let workspace = Path::new(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../../assets/translation/item-action-labels.ko.json"
        ));
        let rom = Rom::from_path(source).unwrap();
        let plan = plan_item_action_labels(&rom, workspace).unwrap();
        assert_eq!(plan.entry_count, 4);
        assert!(!plan.review_complete);
    }
}
