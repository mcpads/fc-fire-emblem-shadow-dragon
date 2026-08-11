use std::path::Path;

use anyhow::{Result, ensure};

use crate::{
    rom::Rom,
    semantic_translation::{
        ExpectedSemanticEntry, SemanticTranslationPlan, plan_semantic_translation,
    },
};

use super::{SUMMARY_AND_STATUS_LABEL_SPECS, command_menu, inspect_unit_ui_japanese_label_count};

pub(crate) fn plan_unit_ui_labels(
    rom: &Rom,
    workspace_path: &Path,
) -> Result<SemanticTranslationPlan> {
    let expected_count = inspect_unit_ui_japanese_label_count(rom.data())?;
    let expected_entries = SUMMARY_AND_STATUS_LABEL_SPECS
        .iter()
        .chain(command_menu::COMMAND_LABEL_SPECS)
        .filter(|spec| spec.translation_scope == "japanese_only")
        .map(|spec| ExpectedSemanticEntry {
            id: format!("unit-ui-label:{:02X}", spec.index),
            japanese_markup: spec.source_text.to_owned(),
            max_visible_cells: (spec.expected.len() - 1).max(6),
        })
        .collect::<Vec<_>>();
    ensure!(
        expected_entries.len() == expected_count,
        "unit UI semantic population changed"
    );
    plan_semantic_translation(workspace_path, &expected_entries)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn public_workspace_covers_every_japanese_unit_ui_label() {
        let source = Path::new(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../../roms/Fire Emblem - Ankoku Ryuu to Hikari no Tsurugi (Japan).nes"
        ));
        if !source.exists() {
            return;
        }
        let workspace = Path::new(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../../assets/translation/unit-ui-labels.ko.json"
        ));
        let rom = Rom::from_path(source).unwrap();
        let plan = plan_unit_ui_labels(&rom, workspace).unwrap();
        assert_eq!(plan.entry_count, 25);
        assert!(!plan.review_complete);
    }
}
