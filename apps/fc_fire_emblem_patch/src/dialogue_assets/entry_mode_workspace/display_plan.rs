use std::{collections::BTreeSet, fs, ops::Range, path::Path};

use anyhow::{Context, Result, ensure};

use crate::font_slots::active_hangul_codes;

use super::*;

const DIRECT_MODE: &str = "direct";
const TRANSITION_MODE: &str = "transition";
const DIALOGUE_PREFIX_OUTPUT_CODES: [u8; 2] = [0x9E, 0xAB];

pub(crate) struct MainDialogueDisplayPlan {
    pub(crate) canonical_record_count: usize,
    pub(crate) display_path_count: usize,
    pub(crate) ordinary_record_count: usize,
    pub(crate) dual_entry_record_count: usize,
    pub(crate) direct_display_path_count: usize,
    pub(crate) transition_display_path_count: usize,
    pub(crate) page_worksets: Vec<MainDialoguePageWorkset>,
}

impl MainDialogueDisplayPlan {
    pub(crate) fn from_canonical_bundle(dialogue: &MainDialogueBundlePlan) -> Self {
        Self {
            canonical_record_count: dialogue.record_ids.len(),
            display_path_count: dialogue.record_ids.len(),
            ordinary_record_count: dialogue.record_ids.len(),
            dual_entry_record_count: 0,
            direct_display_path_count: 0,
            transition_display_path_count: 0,
            page_worksets: dialogue.page_worksets.clone(),
        }
    }

    pub(crate) fn unique_glyphs(&self) -> BTreeSet<char> {
        self.page_worksets
            .iter()
            .flat_map(|workset| workset.target_glyphs.iter().copied())
            .collect()
    }
}

pub(crate) fn plan_normalized_main_dialogue_display(
    source: &[u8],
    workspace_path: &Path,
    dialogue: &MainDialogueBundlePlan,
) -> Result<MainDialogueDisplayPlan> {
    let workspace_bytes = fs::read(workspace_path)
        .with_context(|| format!("read entry-mode workspace {}", workspace_path.display()))?;
    let workspace: EntryModeWorkspace = serde_json::from_slice(&workspace_bytes)
        .with_context(|| format!("parse entry-mode workspace {}", workspace_path.display()))?;
    let expected = build_entry_mode_workspace_without_seed(source)?;
    validate_workspace_binding(&workspace, &expected)?;
    let translation_counts = validate_workspace_translations(&workspace)?;
    ensure!(
        translation_counts.untranslated_japanese_part_count == 0,
        "normalized main-dialogue display still has untranslated Japanese"
    );

    let normalized_ids = workspace
        .records
        .iter()
        .map(|record| record.id.as_str())
        .collect::<BTreeSet<_>>();
    ensure!(
        normalized_ids.len() == workspace.records.len(),
        "normalized main-dialogue display contains duplicate record IDs"
    );
    ensure!(
        normalized_ids
            .iter()
            .all(|record_id| dialogue.record_ids.iter().any(|id| id == record_id)),
        "normalized main-dialogue display contains a record outside the canonical bundle"
    );

    let mut page_worksets = dialogue
        .page_worksets
        .iter()
        .filter(|workset| !normalized_ids.contains(workset.record_id.as_str()))
        .cloned()
        .collect::<Vec<_>>();
    for record in &workspace.records {
        let inherited_prefix_codes = dialogue
            .page_worksets
            .iter()
            .find(|workset| workset.record_id == record.id && workset.page_index == 0)
            .context("dual-entry record has no canonical first-page workset")?
            .preserved_target_active_codes
            .intersection(&DIALOGUE_PREFIX_OUTPUT_CODES.into_iter().collect())
            .copied()
            .collect::<BTreeSet<_>>();
        let source_reclaimable_active_codes = dialogue
            .page_worksets
            .iter()
            .filter(|workset| workset.record_id == record.id)
            .flat_map(|workset| workset.source_reclaimable_active_codes.iter().copied())
            .collect::<BTreeSet<_>>();
        page_worksets.extend(display_path_worksets(
            record,
            DIRECT_MODE,
            &record.direct_leading,
            &record.common_body,
            &inherited_prefix_codes,
            &source_reclaimable_active_codes,
        )?);
        page_worksets.extend(display_path_worksets(
            record,
            TRANSITION_MODE,
            &record.transition_leading,
            &record.common_body,
            &inherited_prefix_codes,
            &source_reclaimable_active_codes,
        )?);
    }

    let ordinary_record_count = dialogue.record_ids.len() - workspace.records.len();
    let display_path_count = ordinary_record_count + workspace.records.len() * 2;
    ensure!(
        page_worksets
            .iter()
            .map(|workset| workset.display_path_id.as_str())
            .collect::<BTreeSet<_>>()
            .len()
            == display_path_count,
        "normalized main-dialogue display path population changed"
    );
    Ok(MainDialogueDisplayPlan {
        canonical_record_count: dialogue.record_ids.len(),
        display_path_count,
        ordinary_record_count,
        dual_entry_record_count: workspace.records.len(),
        direct_display_path_count: workspace.records.len(),
        transition_display_path_count: workspace.records.len(),
        page_worksets,
    })
}

fn display_path_worksets(
    record: &EntryModeRecord,
    mode: &str,
    leading: &EntryModePart,
    common: &EntryModePart,
    inherited_prefix_codes: &BTreeSet<u8>,
    source_reclaimable_active_codes: &BTreeSet<u8>,
) -> Result<Vec<MainDialoguePageWorkset>> {
    let mut logical = target_logical_bytes(leading)?;
    logical.extend(target_logical_bytes(common)?);
    let line_ranges = visible_line_ranges(&record.id, mode, &logical)?;
    let display_path_id = format!("{}@{mode}", record.id);
    line_ranges
        .chunks(MAIN_DIALOGUE_VISIBLE_LINES_PER_PAGE)
        .enumerate()
        .map(|(page_index, page_lines)| {
            let start = page_lines.first().expect("line chunks are nonempty").start;
            let end = page_lines.last().expect("line chunks are nonempty").end;
            let page_bytes = &logical[start..end];
            let dynamic_string_selector_counts =
                super::super::bundle::dynamic_string_controls(page_bytes)?;
            let dynamic_string_control_count =
                dynamic_string_selector_counts.values().sum::<usize>();
            let active_codes = active_hangul_codes().into_iter().collect::<BTreeSet<_>>();
            let mut target_glyphs = BTreeSet::new();
            let mut preserved_target_active_codes = BTreeSet::new();
            for byte in page_bytes {
                match byte {
                    LogicalDialogueByte::TargetGlyph(glyph) => {
                        target_glyphs.insert(*glyph);
                    }
                    LogicalDialogueByte::Encoded(code) if active_codes.contains(code) => {
                        preserved_target_active_codes.insert(*code);
                    }
                    LogicalDialogueByte::Encoded(_) => {}
                }
            }
            if page_bytes.contains(&LogicalDialogueByte::Encoded(0xEA)) {
                preserved_target_active_codes.extend(DIALOGUE_PREFIX_OUTPUT_CODES);
            }
            if page_index == 0 {
                preserved_target_active_codes.extend(inherited_prefix_codes);
            }
            let mut reclaimable = source_reclaimable_active_codes.clone();
            reclaimable.retain(|code| !preserved_target_active_codes.contains(code));
            Ok(MainDialoguePageWorkset {
                record_id: record.id.clone(),
                display_path_id: display_path_id.clone(),
                page_index,
                target_glyphs,
                dynamic_string_selectors: dynamic_string_selector_counts.keys().copied().collect(),
                dynamic_string_selector_counts,
                dynamic_string_control_count,
                source_reclaimable_active_codes: reclaimable,
                preserved_target_active_codes,
            })
        })
        .collect()
}

fn target_logical_bytes(part: &EntryModePart) -> Result<Vec<LogicalDialogueByte>> {
    let markup = if part.status == TranslationStatus::Untranslated {
        ensure!(
            part.japanese_source_byte_count == 0,
            "{} has untranslated Japanese in a required display path",
            part.id
        );
        &part.source_markup
    } else {
        &part.korean
    };
    encode_korean_markup(markup).with_context(|| format!("encode normalized part {}", part.id))
}

fn visible_line_ranges(
    record_id: &str,
    mode: &str,
    logical: &[LogicalDialogueByte],
) -> Result<Vec<Range<usize>>> {
    let mut lines = Vec::new();
    let mut line_start = 0;
    let mut cursor = 0;
    while cursor < logical.len() {
        let LogicalDialogueByte::Encoded(code) = logical[cursor] else {
            cursor += 1;
            continue;
        };
        let Some(control) = DIALOGUE_CONTROL_SPECS
            .iter()
            .find(|control| control.code == code)
        else {
            cursor += 1;
            continue;
        };
        let control_end = cursor
            .checked_add(
                1 + control.inline_operand_byte_count + control.transition_target_byte_count,
            )
            .context("normalized dialogue control range overflow")?;
        ensure!(
            control_end <= logical.len()
                && logical[cursor + 1..control_end]
                    .iter()
                    .all(|byte| matches!(byte, LogicalDialogueByte::Encoded(_))),
            "{record_id} {mode} display path has a truncated control"
        );
        cursor = control_end;
        if control.finishes_line() {
            lines.push(line_start..cursor);
            line_start = cursor;
        }
    }
    ensure!(
        line_start == logical.len() && !lines.is_empty(),
        "{record_id} {mode} display path does not end at a line boundary"
    );
    Ok(lines)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn visible_lines_follow_control_semantics_and_skip_transition_operands() {
        let logical = encode_korean_markup("하나{E4:ED:EF}둘{EF}").unwrap();

        let lines = visible_line_ranges("record", DIRECT_MODE, &logical).unwrap();

        assert_eq!(lines, vec![0..5, 5..7]);
    }

    #[test]
    fn dual_entry_paths_share_common_glyphs_without_merging_distinct_leading_glyphs() {
        let record = EntryModeRecord {
            id: "record".to_owned(),
            incoming_transition_edge_count: 1,
            direct_prefix_byte_count: 4,
            transition_prefix_byte_count: 0,
            common_body_source_file_offset_hex: "0x00001".to_owned(),
            divergent_segment_source_sha1: "source".to_owned(),
            direct_leading: translated_part(
                "record:direct-leading",
                EntryModePartRole::DirectLeading,
                "가{ED}",
            ),
            common_body: translated_part(
                "record:common-body",
                EntryModePartRole::CommonBody,
                "나{EF}",
            ),
            transition_leading: translated_part(
                "record:transition-leading",
                EntryModePartRole::TransitionLeading,
                "다{ED}",
            ),
        };

        let direct = display_path_worksets(
            &record,
            DIRECT_MODE,
            &record.direct_leading,
            &record.common_body,
            &BTreeSet::new(),
            &BTreeSet::new(),
        )
        .unwrap();
        let transition = display_path_worksets(
            &record,
            TRANSITION_MODE,
            &record.transition_leading,
            &record.common_body,
            &BTreeSet::new(),
            &BTreeSet::new(),
        )
        .unwrap();

        assert_eq!(direct[0].display_path_id, "record@direct");
        assert_eq!(direct[0].target_glyphs, BTreeSet::from(['가', '나']));
        assert_eq!(transition[0].display_path_id, "record@transition");
        assert_eq!(transition[0].target_glyphs, BTreeSet::from(['나', '다']));
    }

    fn translated_part(id: &str, role: EntryModePartRole, korean: &str) -> EntryModePart {
        EntryModePart {
            id: id.to_owned(),
            role,
            source_file_offset_hex: "0x00000".to_owned(),
            source_storage_byte_count: 1,
            source_storage_sha1: "source".to_owned(),
            source_markup: "あ{EF}".to_owned(),
            japanese_source_byte_count: 1,
            korean: korean.to_owned(),
            status: TranslationStatus::NeedsHumanReview,
        }
    }
}
