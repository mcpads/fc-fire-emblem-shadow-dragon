use std::{collections::BTreeSet, fs, ops::Range, path::Path};

use anyhow::{Context, Result, ensure};

use crate::font_slots::active_hangul_codes;

use super::*;

const DIRECT_MODE: &str = "direct";
const TRANSITION_MODE: &str = "transition";
const DIALOGUE_PREFIX_OUTPUT_CODES: [u8; 2] = [0x9E, 0xAB];

struct DisplayPathSpec<'a> {
    mode_label: &'static str,
    mode: MainDialogueDisplayMode,
    source_prg_bank: u8,
    prefix: &'a [u8],
    leading: &'a EntryModePart,
    common: &'a EntryModePart,
}

pub(crate) struct MainDialogueDisplayPlan {
    pub(crate) canonical_record_count: usize,
    pub(crate) display_path_count: usize,
    pub(crate) ordinary_record_count: usize,
    pub(crate) dual_entry_record_count: usize,
    pub(crate) direct_display_path_count: usize,
    pub(crate) transition_display_path_count: usize,
    pub(crate) page_worksets: Vec<MainDialoguePageWorkset>,
    pub(crate) display_paths: Vec<MainDialogueDisplayPath>,
    pub(crate) normalized_record_storage: Vec<NormalizedDisplayRecordStorage>,
}

pub(crate) struct NormalizedDisplayRecordStorage {
    pub(crate) record_id: String,
    pub(crate) direct_storage_byte_count: usize,
    pub(crate) transition_storage_byte_count: usize,
    pub(crate) direct_leading_target_byte_count: usize,
    pub(crate) transition_leading_target_byte_count: usize,
    pub(crate) common_body_target_byte_count: usize,
    pub(crate) direct_leading_line_count: usize,
    pub(crate) transition_leading_line_count: usize,
}

impl MainDialogueDisplayPlan {
    pub(crate) fn from_canonical_bundle(dialogue: &MainDialogueBundlePlan) -> Result<Self> {
        Ok(Self {
            canonical_record_count: dialogue.record_ids.len(),
            display_path_count: dialogue.record_ids.len(),
            ordinary_record_count: dialogue.record_ids.len(),
            dual_entry_record_count: 0,
            direct_display_path_count: 0,
            transition_display_path_count: 0,
            page_worksets: dialogue.page_worksets.clone(),
            display_paths: dialogue.canonical_display_paths()?,
            normalized_record_storage: Vec::new(),
        })
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
    let entry_modes = crate::dialogue_inventory::inspect_main_dialogue_entry_modes(source)?
        .transition_targets
        .into_iter()
        .map(|target| (target.record_id.clone(), target))
        .collect::<std::collections::BTreeMap<_, _>>();
    ensure!(
        entry_modes.len() == workspace.records.len(),
        "normalized main-dialogue display lost entry-mode source bindings"
    );

    let canonical_paths = dialogue.canonical_display_paths()?;
    let source_banks = canonical_paths
        .iter()
        .map(|path| (path.record_id.clone(), path.source_prg_bank))
        .collect::<std::collections::BTreeMap<_, _>>();
    let mut page_worksets = dialogue
        .page_worksets
        .iter()
        .filter(|workset| !normalized_ids.contains(workset.record_id.as_str()))
        .cloned()
        .collect::<Vec<_>>();
    let mut display_paths = canonical_paths
        .into_iter()
        .filter(|path| !normalized_ids.contains(path.record_id.as_str()))
        .collect::<Vec<_>>();
    let mut normalized_record_storage = Vec::with_capacity(workspace.records.len());
    for record in &workspace.records {
        let entry_mode = entry_modes
            .get(&record.id)
            .with_context(|| format!("{} has no entry-mode source binding", record.id))?;
        let source_prg_bank = source_banks
            .get(&record.id)
            .copied()
            .with_context(|| format!("{} has no canonical source bank", record.id))?;
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
        let direct_leading = target_logical_bytes(&record.direct_leading)?;
        let transition_leading = target_logical_bytes(&record.transition_leading)?;
        let common_body = target_logical_bytes(&record.common_body)?;
        normalized_record_storage.push(NormalizedDisplayRecordStorage {
            record_id: record.id.clone(),
            direct_storage_byte_count: record.direct_prefix_byte_count
                + direct_leading.len()
                + common_body.len(),
            transition_storage_byte_count: record.transition_prefix_byte_count
                + transition_leading.len()
                + common_body.len(),
            direct_leading_target_byte_count: direct_leading.len(),
            transition_leading_target_byte_count: transition_leading.len(),
            common_body_target_byte_count: common_body.len(),
            direct_leading_line_count: visible_line_ranges(
                &record.id,
                DIRECT_MODE,
                &direct_leading,
            )?
            .len(),
            transition_leading_line_count: visible_line_ranges(
                &record.id,
                TRANSITION_MODE,
                &transition_leading,
            )?
            .len(),
        });
        let (direct_path, direct_worksets) = display_path(
            record,
            DisplayPathSpec {
                mode_label: DIRECT_MODE,
                mode: MainDialogueDisplayMode::Direct,
                source_prg_bank,
                prefix: &entry_mode.leading_source_bytes[..record.direct_prefix_byte_count],
                leading: &record.direct_leading,
                common: &record.common_body,
            },
            &inherited_prefix_codes,
            &source_reclaimable_active_codes,
        )?;
        display_paths.push(direct_path);
        page_worksets.extend(direct_worksets);
        let (transition_path, transition_worksets) = display_path(
            record,
            DisplayPathSpec {
                mode_label: TRANSITION_MODE,
                mode: MainDialogueDisplayMode::Transition,
                source_prg_bank,
                prefix: &entry_mode.leading_source_bytes[..record.transition_prefix_byte_count],
                leading: &record.transition_leading,
                common: &record.common_body,
            },
            &inherited_prefix_codes,
            &source_reclaimable_active_codes,
        )?;
        display_paths.push(transition_path);
        page_worksets.extend(transition_worksets);
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
    ensure!(
        display_paths.len() == display_path_count
            && display_paths
                .iter()
                .map(|path| path.display_path_id.as_str())
                .collect::<BTreeSet<_>>()
                .len()
                == display_path_count,
        "normalized main-dialogue display path storage population changed"
    );
    Ok(MainDialogueDisplayPlan {
        canonical_record_count: dialogue.record_ids.len(),
        display_path_count,
        ordinary_record_count,
        dual_entry_record_count: workspace.records.len(),
        direct_display_path_count: workspace.records.len(),
        transition_display_path_count: workspace.records.len(),
        page_worksets,
        display_paths,
        normalized_record_storage,
    })
}

fn display_path(
    record: &EntryModeRecord,
    spec: DisplayPathSpec<'_>,
    inherited_prefix_codes: &BTreeSet<u8>,
    source_reclaimable_active_codes: &BTreeSet<u8>,
) -> Result<(MainDialogueDisplayPath, Vec<MainDialoguePageWorkset>)> {
    let mut visible_logical = target_logical_bytes(spec.leading)?;
    visible_logical.extend(target_logical_bytes(spec.common)?);
    let line_ranges = visible_line_ranges(&record.id, spec.mode_label, &visible_logical)?;
    let display_path_id = format!("{}@{}", record.id, spec.mode_label);
    let worksets = line_ranges
        .chunks(MAIN_DIALOGUE_VISIBLE_LINES_PER_PAGE)
        .enumerate()
        .map(|(page_index, page_lines)| {
            let start = page_lines.first().expect("line chunks are nonempty").start;
            let end = page_lines.last().expect("line chunks are nonempty").end;
            let page_bytes = &visible_logical[start..end];
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
        .collect::<Result<Vec<_>>>()?;
    let prefix_byte_count = spec.prefix.len();
    let mut logical_bytes = spec
        .prefix
        .iter()
        .copied()
        .map(LogicalDialogueByte::Encoded)
        .collect::<Vec<_>>();
    logical_bytes.extend(visible_logical);
    let visible_page_ranges = line_ranges
        .chunks(MAIN_DIALOGUE_VISIBLE_LINES_PER_PAGE)
        .enumerate()
        .map(|(page_index, page_lines)| {
            let first = page_lines.first().expect("line chunks are nonempty");
            let last = page_lines.last().expect("line chunks are nonempty");
            if page_index == 0 {
                0..prefix_byte_count + last.end
            } else {
                prefix_byte_count + first.start..prefix_byte_count + last.end
            }
        })
        .collect::<Vec<_>>();
    ensure!(
        visible_page_ranges
            .first()
            .is_some_and(|range| range.start == 0)
            && visible_page_ranges
                .last()
                .is_some_and(|range| range.end == logical_bytes.len())
            && visible_page_ranges
                .windows(2)
                .all(|pair| pair[0].end == pair[1].start),
        "{} {} visible pages do not partition its stored display path",
        record.id,
        spec.mode_label,
    );
    Ok((
        MainDialogueDisplayPath {
            record_id: record.id.clone(),
            display_path_id,
            source_prg_bank: spec.source_prg_bank,
            mode: spec.mode,
            logical_bytes,
            visible_page_ranges,
        },
        worksets,
    ))
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

        let (_, direct) = display_path(
            &record,
            DisplayPathSpec {
                mode_label: DIRECT_MODE,
                mode: MainDialogueDisplayMode::Direct,
                source_prg_bank: 0x07,
                prefix: &[0x10, 0x11, 0x12, 0x13],
                leading: &record.direct_leading,
                common: &record.common_body,
            },
            &BTreeSet::new(),
            &BTreeSet::new(),
        )
        .unwrap();
        let (_, transition) = display_path(
            &record,
            DisplayPathSpec {
                mode_label: TRANSITION_MODE,
                mode: MainDialogueDisplayMode::Transition,
                source_prg_bank: 0x07,
                prefix: &[],
                leading: &record.transition_leading,
                common: &record.common_body,
            },
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
