use std::{
    collections::{BTreeMap, BTreeSet},
    fs,
    path::Path,
};

use anyhow::{Context, Result, ensure};
use serde::Deserialize;

use crate::{
    rom::EXPECTED_SOURCE_SHA1,
    sha1_hex,
    text_inventory::{FixedTextLogicalByte, encode_target_markup, is_japanese_character},
};

pub(crate) struct ExpectedSemanticEntry {
    pub(crate) id: String,
    pub(crate) japanese_markup: String,
    pub(crate) max_visible_cells: usize,
}

#[derive(Debug)]
pub(crate) struct SemanticTranslationPlan {
    pub(crate) workspace_sha1: String,
    pub(crate) entry_count: usize,
    pub(crate) review_complete: bool,
    reviewed_entry_ids: BTreeSet<String>,
    ordered_entry_ids: Vec<String>,
    logical_bytes_by_entry_id: BTreeMap<String, Vec<FixedTextLogicalByte>>,
    target_glyphs_by_entry_id: BTreeMap<String, BTreeSet<char>>,
}

impl SemanticTranslationPlan {
    pub(crate) fn entry_review_complete(&self, id: &str) -> bool {
        self.reviewed_entry_ids.contains(id)
    }

    pub(crate) fn entry_target_glyphs(&self, id: &str) -> Option<&BTreeSet<char>> {
        self.target_glyphs_by_entry_id.get(id)
    }

    pub(crate) fn entry_logical_bytes(&self, id: &str) -> Option<&[FixedTextLogicalByte]> {
        self.logical_bytes_by_entry_id.get(id).map(Vec::as_slice)
    }

    pub(crate) fn entry_ids(&self) -> impl Iterator<Item = &str> {
        self.ordered_entry_ids.iter().map(String::as_str)
    }

    pub(crate) fn unique_target_glyphs(&self) -> BTreeSet<char> {
        self.target_glyphs_by_entry_id
            .values()
            .flatten()
            .copied()
            .collect()
    }
}

#[derive(Debug, Deserialize)]
struct Workspace {
    format_version: u8,
    source_sha1: String,
    translate_from: String,
    translate_to: String,
    preserve_existing_english_and_digits: bool,
    entries: Vec<WorkspaceEntry>,
}

#[derive(Debug, Deserialize)]
struct WorkspaceEntry {
    id: String,
    japanese_markup: String,
    korean_markup: String,
    status: String,
}

pub(crate) fn plan_semantic_translation(
    workspace_path: &Path,
    expected_entries: &[ExpectedSemanticEntry],
) -> Result<SemanticTranslationPlan> {
    let bytes = fs::read(workspace_path)
        .with_context(|| format!("read semantic translation {}", workspace_path.display()))?;
    let workspace: Workspace = serde_json::from_slice(&bytes)
        .with_context(|| format!("parse semantic translation {}", workspace_path.display()))?;
    ensure!(
        workspace.format_version == 1,
        "unsupported semantic translation format"
    );
    ensure!(
        workspace.source_sha1 == EXPECTED_SOURCE_SHA1
            && workspace.translate_from == "ja"
            && workspace.translate_to == "ko"
            && workspace.preserve_existing_english_and_digits,
        "semantic translation scope changed"
    );
    ensure!(
        workspace.entries.len() == expected_entries.len(),
        "semantic translation entry count changed"
    );

    let mut reviewed_entry_ids = BTreeSet::new();
    let mut ordered_entry_ids = Vec::with_capacity(workspace.entries.len());
    let mut logical_bytes_by_entry_id = BTreeMap::new();
    let mut target_glyphs_by_entry_id = BTreeMap::new();
    for (entry, expected) in workspace.entries.iter().zip(expected_entries) {
        ensure!(
            entry.id == expected.id && entry.japanese_markup == expected.japanese_markup,
            "semantic translation source binding changed for {}",
            expected.id
        );
        ensure!(
            matches!(entry.status.as_str(), "needs_human_review" | "complete"),
            "invalid semantic translation status for {}",
            expected.id
        );
        ensure!(
            !entry.korean_markup.is_empty(),
            "semantic translation is empty for {}",
            expected.id
        );
        ensure!(
            !entry.korean_markup.chars().any(is_japanese_character),
            "semantic translation still contains Japanese for {}",
            expected.id
        );
        ensure!(
            protected_source_sequence(&entry.korean_markup, is_target_hangul)
                == protected_source_sequence(&expected.japanese_markup, is_japanese_character),
            "semantic translation changed protected original text for {}",
            expected.id
        );
        ensure!(
            visible_cell_count(&entry.korean_markup) <= expected.max_visible_cells,
            "semantic translation for {} needs more than {} visible cells",
            expected.id,
            expected.max_visible_cells
        );
        if entry.status == "complete" {
            reviewed_entry_ids.insert(entry.id.clone());
        }
        ordered_entry_ids.push(entry.id.clone());
        let logical_bytes = encode_target_markup(&entry.korean_markup)
            .with_context(|| format!("encode semantic translation {}", entry.id))?;
        let target_glyphs = logical_bytes
            .iter()
            .filter_map(|byte| match byte {
                FixedTextLogicalByte::TargetGlyph(glyph) => Some(*glyph),
                FixedTextLogicalByte::Encoded(_) => None,
            })
            .collect();
        ensure!(
            logical_bytes_by_entry_id
                .insert(entry.id.clone(), logical_bytes)
                .is_none(),
            "semantic translation repeats logical entry {}",
            entry.id
        );
        ensure!(
            target_glyphs_by_entry_id
                .insert(entry.id.clone(), target_glyphs)
                .is_none(),
            "semantic translation repeats entry {}",
            entry.id
        );
    }

    Ok(SemanticTranslationPlan {
        workspace_sha1: sha1_hex(&bytes),
        entry_count: workspace.entries.len(),
        review_complete: reviewed_entry_ids.len() == workspace.entries.len(),
        reviewed_entry_ids,
        ordered_entry_ids,
        logical_bytes_by_entry_id,
        target_glyphs_by_entry_id,
    })
}

fn protected_source_sequence(markup: &str, translated_character: fn(char) -> bool) -> String {
    markup
        .chars()
        .filter(|character| !translated_character(*character))
        .collect()
}

fn is_target_hangul(character: char) -> bool {
    ('가'..='힣').contains(&character)
}

fn visible_cell_count(markup: &str) -> usize {
    let characters = markup.chars().collect::<Vec<_>>();
    let mut index = 0;
    let mut count = 0;
    while index < characters.len() {
        if characters[index] == '{' && index + 3 < characters.len() && characters[index + 3] == '}'
        {
            index += 4;
        } else {
            count += 1;
            index += 1;
        }
    }
    count
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn layout_tokens_do_not_consume_visible_cells() {
        assert_eq!(visible_cell_count("합계{FF}턴"), 3);
    }

    #[test]
    fn original_ascii_punctuation_is_protected() {
        assert_eq!(
            protected_source_sequence("저장할까요?", is_target_hangul),
            "?"
        );
    }

    #[test]
    fn planned_semantic_text_retains_ordered_logical_cells_without_reloading() {
        let plan = SemanticTranslationPlan {
            workspace_sha1: "workspace".to_owned(),
            entry_count: 1,
            review_complete: false,
            reviewed_entry_ids: BTreeSet::new(),
            ordered_entry_ids: vec!["label".to_owned()],
            logical_bytes_by_entry_id: BTreeMap::from([(
                "label".to_owned(),
                vec![
                    FixedTextLogicalByte::TargetGlyph('가'),
                    FixedTextLogicalByte::Encoded(0xFF),
                    FixedTextLogicalByte::TargetGlyph('나'),
                ],
            )]),
            target_glyphs_by_entry_id: BTreeMap::from([(
                "label".to_owned(),
                BTreeSet::from(['가', '나']),
            )]),
        };

        assert_eq!(plan.entry_ids().collect::<Vec<_>>(), ["label"]);
        assert_eq!(
            plan.entry_logical_bytes("label").unwrap(),
            [
                FixedTextLogicalByte::TargetGlyph('가'),
                FixedTextLogicalByte::Encoded(0xFF),
                FixedTextLogicalByte::TargetGlyph('나'),
            ]
        );
    }
}
