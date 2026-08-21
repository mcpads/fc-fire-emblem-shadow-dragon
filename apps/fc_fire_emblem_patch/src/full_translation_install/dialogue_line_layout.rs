use std::collections::{BTreeMap, BTreeSet};

use anyhow::{Result, ensure};
use serde::Serialize;

use crate::dialogue_assets::MainDialogueLineLayoutPlan;

use super::dynamic_inputs::{DynamicDialogueInputPlan, DynamicDisplayWidth};

#[derive(Debug, Serialize)]
pub(super) struct DialogueLineLayoutAudit {
    strategy: &'static str,
    line_count: usize,
    fixed_width_seed_record_count: usize,
    inherited_width_record_count: usize,
    resolved_width_record_count: usize,
    unknown_absolute_width_record_count: usize,
    unknown_absolute_width_record_ids: Vec<String>,
    fully_resolved_line_count: usize,
    absolute_window_bound_line_count: usize,
    source_relative_bound_line_count: usize,
    unresolved_line_count: usize,
    unresolved_record_count: usize,
    unresolved_line_count_by_table: BTreeMap<String, usize>,
    unresolved_lines: Vec<UnresolvedDialogueLineLayout>,
    unknown_window_width_line_count: usize,
    unknown_absolute_dynamic_width_line_count: usize,
    resolved_dynamic_string_control_count: usize,
    unresolved_dynamic_string_control_count: usize,
    maximum_absolute_bound_rendered_cell_count: usize,
    smallest_absolute_bound_remaining_cell_count: usize,
    overflowing_line_count: usize,
    every_proven_line_fits: bool,
    whole_program_line_width_complete: bool,
}

#[derive(Debug, Serialize)]
struct UnresolvedDialogueLineLayout {
    line_id: String,
    maximum_visible_cell_count: Option<usize>,
    source_static_visible_cell_count: usize,
    target_nonpreserved_maximum_cell_count: usize,
    additional_source_bound_cell_count_needed: usize,
    dynamic_string_selectors_hex: Vec<String>,
}

pub(super) fn audit_main_dialogue_line_layout(
    layout: &MainDialogueLineLayoutPlan,
    dynamic: &DynamicDialogueInputPlan,
) -> Result<DialogueLineLayoutAudit> {
    audit_with_dynamic_widths(layout, |record_id, selector| {
        dynamic.display_width(record_id, selector)
    })
}

fn audit_with_dynamic_widths(
    layout: &MainDialogueLineLayoutPlan,
    mut dynamic_width: impl FnMut(&str, u8) -> Result<DynamicDisplayWidth>,
) -> Result<DialogueLineLayoutAudit> {
    let mut fully_resolved_line_count = 0;
    let mut absolute_window_bound_line_count = 0;
    let mut source_relative_bound_line_count = 0;
    let mut unresolved_line_count = 0;
    let mut unresolved_record_ids = BTreeSet::new();
    let mut unresolved_line_count_by_table = BTreeMap::new();
    let mut unresolved_lines = Vec::new();
    let mut unknown_window_width_line_count = 0;
    let mut unknown_absolute_dynamic_width_line_count = 0;
    let mut resolved_dynamic_string_control_count = 0;
    let mut unresolved_dynamic_string_control_count = 0;
    let mut maximum_absolute_bound_rendered_cell_count = 0;
    let mut smallest_absolute_bound_remaining_cell_count = usize::MAX;
    let mut overflowing_lines = Vec::new();

    for line in &layout.lines {
        let mut rendered_cell_count = line.static_visible_cell_count;
        // A translated dynamic string contributes its proven maximum. A preserved
        // source producer appears identically on both sides of the comparison and
        // therefore cancels from a source-relative bound.
        let mut source_relative_target_cell_count = line.static_visible_cell_count;
        let mut absolute_dynamic_width_unknown = false;
        for selector in &line.dynamic_string_selectors {
            match dynamic_width(&line.record_id, *selector)? {
                DynamicDisplayWidth::Translated {
                    maximum_target_cell_count,
                    maximum_growth_over_source_cell_count,
                } => {
                    rendered_cell_count = rendered_cell_count
                        .checked_add(maximum_target_cell_count)
                        .ok_or_else(|| anyhow::anyhow!("dialogue rendered-cell count overflow"))?;
                    source_relative_target_cell_count = source_relative_target_cell_count
                        .checked_add(maximum_growth_over_source_cell_count)
                        .ok_or_else(|| {
                            anyhow::anyhow!("dialogue source-relative cell count overflow")
                        })?;
                    resolved_dynamic_string_control_count += 1;
                }
                DynamicDisplayWidth::PreservedSource => {
                    absolute_dynamic_width_unknown = true;
                    unresolved_dynamic_string_control_count += 1;
                }
            }
        }

        let window_width_unknown = line.maximum_visible_cell_count.is_none();
        unknown_window_width_line_count += usize::from(window_width_unknown);
        unknown_absolute_dynamic_width_line_count += usize::from(absolute_dynamic_width_unknown);
        let source_relative_bound =
            source_relative_target_cell_count <= line.source_static_visible_cell_count;

        if let Some(maximum_visible_cell_count) = line.maximum_visible_cell_count
            && !absolute_dynamic_width_unknown
        {
            if rendered_cell_count > maximum_visible_cell_count {
                overflowing_lines.push(format!(
                    "{} uses {rendered_cell_count}/{maximum_visible_cell_count} cells",
                    line.line_id
                ));
                continue;
            }
            absolute_window_bound_line_count += 1;
            fully_resolved_line_count += 1;
            maximum_absolute_bound_rendered_cell_count =
                maximum_absolute_bound_rendered_cell_count.max(rendered_cell_count);
            smallest_absolute_bound_remaining_cell_count =
                smallest_absolute_bound_remaining_cell_count
                    .min(maximum_visible_cell_count - rendered_cell_count);
            continue;
        }

        if source_relative_bound {
            source_relative_bound_line_count += 1;
            fully_resolved_line_count += 1;
            continue;
        }

        unresolved_line_count += 1;
        unresolved_record_ids.insert(line.record_id.clone());
        let table_id = line
            .record_id
            .rsplit_once(':')
            .map(|(table_id, _)| table_id)
            .unwrap_or(&line.record_id);
        *unresolved_line_count_by_table
            .entry(table_id.to_owned())
            .or_insert(0) += 1;
        unresolved_lines.push(UnresolvedDialogueLineLayout {
            line_id: line.line_id.clone(),
            maximum_visible_cell_count: line.maximum_visible_cell_count,
            source_static_visible_cell_count: line.source_static_visible_cell_count,
            target_nonpreserved_maximum_cell_count: source_relative_target_cell_count,
            additional_source_bound_cell_count_needed: source_relative_target_cell_count
                .saturating_sub(line.source_static_visible_cell_count),
            dynamic_string_selectors_hex: line
                .dynamic_string_selectors
                .iter()
                .map(|selector| format!("{selector:02X}"))
                .collect(),
        });
    }

    ensure!(
        overflowing_lines.is_empty(),
        "main-dialogue line layout exceeds its proven window width: {}",
        overflowing_lines
            .into_iter()
            .take(8)
            .collect::<Vec<_>>()
            .join(", ")
    );
    ensure!(
        absolute_window_bound_line_count + source_relative_bound_line_count + unresolved_line_count
            == layout.lines.len(),
        "main-dialogue line-layout audit lost lines"
    );
    if absolute_window_bound_line_count == 0 {
        smallest_absolute_bound_remaining_cell_count = 0;
    }

    Ok(DialogueLineLayoutAudit {
        strategy: "derive fixed-header widths and source-bound transitions; otherwise prove that the translated literal plus translated dynamic maxima is no wider than the source literal while identical preserved producers cancel; reject proven overflow and retain every remaining caller-owned window",
        line_count: layout.lines.len(),
        fixed_width_seed_record_count: layout.fixed_width_seed_record_count,
        inherited_width_record_count: layout.inherited_width_record_count,
        resolved_width_record_count: layout.resolved_width_record_count,
        unknown_absolute_width_record_count: layout.unresolved_width_record_ids.len(),
        unknown_absolute_width_record_ids: layout.unresolved_width_record_ids.clone(),
        fully_resolved_line_count,
        absolute_window_bound_line_count,
        source_relative_bound_line_count,
        unresolved_line_count,
        unresolved_record_count: unresolved_record_ids.len(),
        unresolved_line_count_by_table,
        unresolved_lines,
        unknown_window_width_line_count,
        unknown_absolute_dynamic_width_line_count,
        resolved_dynamic_string_control_count,
        unresolved_dynamic_string_control_count,
        maximum_absolute_bound_rendered_cell_count,
        smallest_absolute_bound_remaining_cell_count,
        overflowing_line_count: 0,
        every_proven_line_fits: true,
        whole_program_line_width_complete: unresolved_line_count == 0,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::dialogue_assets::MainDialogueLineLayout;

    fn plan(
        width: Option<usize>,
        source_static_visible_cell_count: usize,
        static_visible_cell_count: usize,
    ) -> MainDialogueLineLayoutPlan {
        MainDialogueLineLayoutPlan {
            lines: vec![MainDialogueLineLayout {
                record_id: "record:000".to_owned(),
                line_id: "record:000:line:00".to_owned(),
                line_index: 0,
                maximum_visible_cell_count: width,
                source_static_visible_cell_count,
                static_visible_cell_count,
                dynamic_string_selectors: vec![0],
            }],
            fixed_width_seed_record_count: usize::from(width.is_some()),
            resolved_width_record_count: usize::from(width.is_some()),
            inherited_width_record_count: 0,
            unresolved_width_record_ids: if width.is_none() {
                vec!["record:000".to_owned()]
            } else {
                Vec::new()
            },
        }
    }

    #[test]
    fn exact_fit_passes_and_one_cell_overflow_fails() {
        let fit = audit_with_dynamic_widths(&plan(Some(8), 2, 2), |_, _| {
            Ok(DynamicDisplayWidth::Translated {
                maximum_target_cell_count: 6,
                maximum_growth_over_source_cell_count: 6,
            })
        })
        .unwrap();
        assert_eq!(fit.fully_resolved_line_count, 1);
        assert_eq!(fit.absolute_window_bound_line_count, 1);
        assert_eq!(fit.smallest_absolute_bound_remaining_cell_count, 0);

        let error = audit_with_dynamic_widths(&plan(Some(7), 2, 2), |_, _| {
            Ok(DynamicDisplayWidth::Translated {
                maximum_target_cell_count: 6,
                maximum_growth_over_source_cell_count: 6,
            })
        })
        .unwrap_err();
        assert!(error.to_string().contains("uses 8/7 cells"));
    }

    #[test]
    fn source_relative_bound_closes_unknown_window_and_preserved_dynamic_width() {
        let translated = audit_with_dynamic_widths(&plan(None, 8, 2), |_, _| {
            Ok(DynamicDisplayWidth::Translated {
                maximum_target_cell_count: 6,
                maximum_growth_over_source_cell_count: 6,
            })
        })
        .unwrap();
        assert_eq!(translated.source_relative_bound_line_count, 1);
        assert_eq!(translated.unresolved_line_count, 0);

        let preserved = audit_with_dynamic_widths(&plan(None, 2, 2), |_, _| {
            Ok(DynamicDisplayWidth::PreservedSource)
        })
        .unwrap();
        assert_eq!(preserved.source_relative_bound_line_count, 1);
        assert_eq!(preserved.unresolved_line_count, 0);
    }

    #[test]
    fn insufficient_source_slack_remains_visible_instead_of_passing() {
        let unknown_window = audit_with_dynamic_widths(&plan(None, 2, 2), |_, _| {
            Ok(DynamicDisplayWidth::Translated {
                maximum_target_cell_count: 6,
                maximum_growth_over_source_cell_count: 6,
            })
        })
        .unwrap();
        assert_eq!(unknown_window.unresolved_line_count, 1);
        assert!(!unknown_window.whole_program_line_width_complete);

        let unknown_dynamic = audit_with_dynamic_widths(&plan(Some(18), 2, 3), |_, _| {
            Ok(DynamicDisplayWidth::PreservedSource)
        })
        .unwrap();
        assert_eq!(unknown_dynamic.unknown_absolute_dynamic_width_line_count, 1);
        assert_eq!(unknown_dynamic.unresolved_dynamic_string_control_count, 1);
        assert_eq!(unknown_dynamic.unresolved_line_count_by_table["record"], 1);
        assert!(!unknown_dynamic.whole_program_line_width_complete);
    }
}
