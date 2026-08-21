use std::collections::{BTreeMap, BTreeSet};

use anyhow::{Context, Result, ensure};

use crate::dialogue_inventory::{
    MainDialogueGraphReport, MainDialogueStorageLine, MainDialogueStorageRecord,
    inspect_main_dialogue_fixed_text_width,
};
use crate::text_inventory::dialogue_literal_display_cell_count;

use super::{
    DIALOGUE_CONTROL_SPECS, LogicalDialogueByte, MainDialogueWorkspace, TranslationStatus,
    encode_korean_markup,
};

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct MainDialogueLineLayout {
    pub(crate) record_id: String,
    pub(crate) line_id: String,
    pub(crate) line_index: usize,
    pub(crate) maximum_visible_cell_count: Option<usize>,
    pub(crate) source_static_visible_cell_count: usize,
    pub(crate) static_visible_cell_count: usize,
    pub(crate) dynamic_string_selectors: Vec<u8>,
}

#[derive(Clone, Debug)]
pub(crate) struct MainDialogueLineLayoutPlan {
    pub(crate) lines: Vec<MainDialogueLineLayout>,
    pub(crate) fixed_width_seed_record_count: usize,
    pub(crate) resolved_width_record_count: usize,
    pub(crate) inherited_width_record_count: usize,
    pub(crate) unresolved_width_record_ids: Vec<String>,
}

pub(super) fn build_main_dialogue_line_layout_plan(
    source: &[u8],
    source_records: &[MainDialogueStorageRecord],
    workspace: &MainDialogueWorkspace,
    graph: &MainDialogueGraphReport,
    requested: &BTreeSet<&str>,
) -> Result<MainDialogueLineLayoutPlan> {
    ensure!(
        source_records.len() == workspace.records.len(),
        "main-dialogue line layout lost workspace records"
    );

    let mut seed_widths = BTreeMap::new();
    let mut record_ids = BTreeSet::new();
    for (source_record, workspace_record) in source_records.iter().zip(&workspace.records) {
        let expected_id = format!(
            "{}:{:03}",
            source_record.table_id, source_record.canonical_entry_index
        );
        ensure!(
            workspace_record.id == expected_id && record_ids.insert(expected_id.clone()),
            "main-dialogue line-layout record binding changed at {expected_id}"
        );
        if let Some(width) = inspect_main_dialogue_fixed_text_width(source, source_record)? {
            ensure!(
                seed_widths.insert(expected_id, width).is_none(),
                "main-dialogue line-layout width seed is duplicated"
            );
        }
    }
    let resolved_widths = propagate_window_widths(&record_ids, &seed_widths, graph)?;
    let requested_record_ids = record_ids
        .iter()
        .filter(|record_id| requested.contains(record_id.as_str()))
        .cloned()
        .collect::<BTreeSet<_>>();
    ensure!(
        requested_record_ids.len() == requested.len(),
        "main-dialogue line-layout request names an unknown record"
    );
    let resolved_record_ids = resolved_widths.keys().cloned().collect::<BTreeSet<_>>();
    let unresolved_width_record_ids = requested_record_ids
        .difference(&resolved_record_ids)
        .cloned()
        .collect::<Vec<_>>();

    let mut lines = Vec::new();
    let mut line_ids = BTreeSet::new();
    for (source_record, workspace_record) in source_records.iter().zip(&workspace.records) {
        if !requested.contains(workspace_record.id.as_str()) {
            continue;
        }
        ensure!(
            source_record.lines.len() == workspace_record.lines.len(),
            "{} line-layout source and workspace line counts differ",
            workspace_record.id
        );
        for (source_line, workspace_line) in source_record.lines.iter().zip(&workspace_record.lines)
        {
            ensure!(
                line_ids.insert(workspace_line.id.clone()),
                "duplicate main-dialogue line-layout ID {}",
                workspace_line.id
            );
            let source_logical_line = source_logical_line(source, source_line)?;
            let logical_line = if workspace_line.status == TranslationStatus::Untranslated {
                source_logical_line.clone()
            } else {
                encode_korean_markup(&workspace_line.korean)
                    .with_context(|| format!("encode line layout at {}", workspace_line.id))?
            };
            let (source_static_visible_cell_count, source_dynamic_string_selectors) =
                inspect_logical_line_layout(&source_logical_line).with_context(|| {
                    format!("inspect source line layout at {}", workspace_line.id)
                })?;
            let (static_visible_cell_count, dynamic_string_selectors) =
                inspect_logical_line_layout(&logical_line)
                    .with_context(|| format!("inspect line layout at {}", workspace_line.id))?;
            ensure!(
                dynamic_string_selectors == source_dynamic_string_selectors,
                "{} changed its dynamic string selector sequence",
                workspace_line.id
            );
            lines.push(MainDialogueLineLayout {
                record_id: workspace_record.id.clone(),
                line_id: workspace_line.id.clone(),
                line_index: workspace_line.index,
                maximum_visible_cell_count: resolved_widths.get(&workspace_record.id).copied(),
                source_static_visible_cell_count,
                static_visible_cell_count,
                dynamic_string_selectors,
            });
        }
    }

    let fixed_width_seed_record_count = seed_widths
        .keys()
        .filter(|record_id| requested.contains(record_id.as_str()))
        .count();
    let resolved_width_record_count = resolved_widths
        .keys()
        .filter(|record_id| requested.contains(record_id.as_str()))
        .count();
    Ok(MainDialogueLineLayoutPlan {
        lines,
        fixed_width_seed_record_count,
        resolved_width_record_count,
        inherited_width_record_count: resolved_width_record_count
            .saturating_sub(fixed_width_seed_record_count),
        unresolved_width_record_ids,
    })
}

fn propagate_window_widths(
    record_ids: &BTreeSet<String>,
    seed_widths: &BTreeMap<String, usize>,
    graph: &MainDialogueGraphReport,
) -> Result<BTreeMap<String, usize>> {
    ensure!(
        seed_widths
            .keys()
            .all(|record_id| record_ids.contains(record_id)),
        "main-dialogue line-layout width seed names an unknown record"
    );
    let mut candidates = seed_widths
        .iter()
        .map(|(record_id, width)| (record_id.clone(), BTreeSet::from([*width])))
        .collect::<BTreeMap<_, _>>();

    loop {
        let mut changed = false;
        for edge in &graph.transition_edges {
            let source_id = format!(
                "{}:{:03}",
                edge.source_table_id, edge.source_canonical_entry_index
            );
            let target_id = format!(
                "{}:{:03}",
                edge.target_table_id, edge.target_canonical_entry_index
            );
            ensure!(
                record_ids.contains(&source_id) && record_ids.contains(&target_id),
                "main-dialogue line-layout graph names an unknown record"
            );
            // A directly consumed fixed header establishes a fresh window. Only records
            // without such a header inherit the caller's already-open text width.
            if seed_widths.contains_key(&target_id) {
                continue;
            }
            let inherited = candidates.get(&source_id).cloned().unwrap_or_default();
            let target = candidates.entry(target_id).or_default();
            let before = target.len();
            target.extend(inherited);
            changed |= target.len() != before;
        }
        if !changed {
            break;
        }
    }

    Ok(candidates
        .into_iter()
        .filter_map(|(record_id, widths)| {
            (widths.len() == 1).then(|| (record_id, *widths.first().expect("one width")))
        })
        .collect())
}

fn source_logical_line(
    source: &[u8],
    line: &MainDialogueStorageLine,
) -> Result<Vec<LogicalDialogueByte>> {
    let end = line
        .file_offset
        .checked_add(line.storage_byte_count)
        .context("main-dialogue layout source-line range overflow")?;
    Ok(source
        .get(line.file_offset..end)
        .context("main-dialogue layout source line is outside the ROM")?
        .iter()
        .copied()
        .map(LogicalDialogueByte::Encoded)
        .collect())
}

fn inspect_logical_line_layout(bytes: &[LogicalDialogueByte]) -> Result<(usize, Vec<u8>)> {
    let mut visible_cell_count = 0;
    let mut dynamic_string_selectors = Vec::new();
    let mut cursor = 0;
    while cursor < bytes.len() {
        match bytes[cursor] {
            LogicalDialogueByte::TargetGlyph(_) => {
                visible_cell_count += 1;
                cursor += 1;
            }
            LogicalDialogueByte::Encoded(code) => {
                let Some(control) = DIALOGUE_CONTROL_SPECS
                    .iter()
                    .find(|control| control.code == code)
                else {
                    visible_cell_count += dialogue_literal_display_cell_count(code);
                    cursor += 1;
                    continue;
                };
                let stored_byte_count =
                    1 + control.inline_operand_byte_count + control.transition_target_byte_count;
                let end = cursor
                    .checked_add(stored_byte_count)
                    .context("main-dialogue line-layout control range overflow")?;
                ensure!(
                    end <= bytes.len()
                        && bytes[cursor + 1..end]
                            .iter()
                            .all(|byte| matches!(byte, LogicalDialogueByte::Encoded(_))),
                    "main-dialogue line-layout control {code:02X} lost its encoded operands"
                );
                match code {
                    0xEA => visible_cell_count += 2,
                    0xEC => {
                        let LogicalDialogueByte::Encoded(selector) = bytes[cursor + 1] else {
                            unreachable!("EC operand was checked above")
                        };
                        dynamic_string_selectors.push(selector);
                    }
                    _ => {}
                }
                cursor = end;
            }
        }
    }
    Ok((visible_cell_count, dynamic_string_selectors))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::dialogue_inventory::MainDialogueTransitionEdgeReport;

    fn edge(
        source: (&'static str, usize),
        target: (&'static str, usize),
    ) -> MainDialogueTransitionEdgeReport {
        MainDialogueTransitionEdgeReport {
            source_table_id: source.0,
            source_canonical_entry_index: source.1,
            source_entry_indices: vec![source.1],
            source_pointer_cpu_address: 0,
            source_pointer_cpu_address_hex: "0x0000".to_owned(),
            source_file_offset: 0,
            source_file_offset_hex: "0x00000".to_owned(),
            control: 0xE6,
            control_hex: "E6".to_owned(),
            target_table_id: target.0,
            target_entry_index: target.1,
            target_canonical_entry_index: target.1,
            target_pointer_cpu_address: 0,
            target_pointer_cpu_address_hex: "0x0000".to_owned(),
            target_file_offset: 0,
            target_file_offset_hex: "0x00000".to_owned(),
        }
    }

    fn graph(edges: Vec<MainDialogueTransitionEdgeReport>) -> MainDialogueGraphReport {
        MainDialogueGraphReport {
            node_count: 3,
            transition_edge_count: edges.len(),
            terminal_reachable_node_count: 3,
            caller_handoff_boundary_reachable_node_count: 0,
            max_transition_edge_count_to_boundary: edges.len(),
            cycle_count: 0,
            unresolved_node_count: 0,
            transition_edges: edges,
        }
    }

    #[test]
    fn transition_targets_inherit_width_but_fixed_headers_start_fresh_windows() {
        let records = BTreeSet::from([
            "table:000".to_owned(),
            "table:001".to_owned(),
            "table:002".to_owned(),
        ]);
        let seeds = BTreeMap::from([("table:000".to_owned(), 18), ("table:002".to_owned(), 5)]);
        let widths = propagate_window_widths(
            &records,
            &seeds,
            &graph(vec![
                edge(("table", 0), ("table", 1)),
                edge(("table", 1), ("table", 2)),
            ]),
        )
        .unwrap();

        assert_eq!(widths["table:000"], 18);
        assert_eq!(widths["table:001"], 18);
        assert_eq!(widths["table:002"], 5);
    }

    #[test]
    fn logical_layout_counts_rendered_cells_and_preserves_dynamic_selectors() {
        let bytes = vec![
            LogicalDialogueByte::Encoded(0xEA),
            LogicalDialogueByte::TargetGlyph('가'),
            LogicalDialogueByte::Encoded(0xFF),
            LogicalDialogueByte::Encoded(0xEC),
            LogicalDialogueByte::Encoded(0x02),
            LogicalDialogueByte::Encoded(0xED),
        ];

        assert_eq!(inspect_logical_line_layout(&bytes).unwrap(), (4, vec![2]));
    }

    #[test]
    fn source_combining_marks_do_not_consume_another_display_cell() {
        let bytes = vec![
            LogicalDialogueByte::Encoded(0x42),
            LogicalDialogueByte::Encoded(0x0F),
            LogicalDialogueByte::Encoded(0x43),
            LogicalDialogueByte::Encoded(0x1F),
            LogicalDialogueByte::Encoded(0xED),
        ];

        assert_eq!(
            inspect_logical_line_layout(&bytes).unwrap(),
            (2, Vec::new())
        );
    }

    #[test]
    fn malformed_dynamic_control_fails_closed() {
        assert!(inspect_logical_line_layout(&[LogicalDialogueByte::Encoded(0xEC)]).is_err());
    }
}
