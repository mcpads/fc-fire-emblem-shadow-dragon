use super::*;

pub(super) fn build_main_dialogue_graph(
    tables: &[DialogueTableReport],
) -> Result<MainDialogueGraphReport> {
    let mut table_index_by_id = BTreeMap::new();
    for (table_index, table) in tables.iter().enumerate() {
        if table.directory_binding.is_some() {
            ensure!(
                table_index_by_id.insert(table.id, table_index).is_none(),
                "duplicate main dialogue table id {}",
                table.id
            );
        }
    }

    let mut nodes = BTreeMap::new();
    for (table_index, table) in tables.iter().enumerate() {
        if table.directory_binding.is_none() {
            continue;
        }
        for entry in table.entries.iter().filter(|entry| {
            entry.target_kind == "script_entry_start" && is_canonical_dialogue_entry(entry)
        }) {
            let key = MainDialogueGraphNodeKey {
                table_index,
                pointer_cpu_address: entry.pointer_cpu_address,
            };
            ensure!(
                nodes.insert(key, (table, entry)).is_none(),
                "{} canonical entry {} duplicates a graph node",
                table.id,
                entry.index
            );
        }
    }

    let mut states = BTreeMap::new();
    let mut transition_edges = Vec::new();
    for (source_key, (source_table, source_entry)) in &nodes {
        let segment = source_entry.main_linear_segment.as_ref().with_context(|| {
            format!(
                "{} canonical entry {} has no main linear segment",
                source_table.id, source_entry.index
            )
        })?;
        let transition_target = if matches!(segment.boundary_control, 0xE4 | 0xE6) {
            let transition = segment.transition_target.as_ref().with_context(|| {
                format!(
                    "{} canonical entry {} has a transition boundary without a target",
                    source_table.id, source_entry.index
                )
            })?;
            let target_table_index = *table_index_by_id
                .get(transition.target_table_id)
                .with_context(|| {
                    format!(
                        "{} canonical entry {} targets undeclared table {}",
                        source_table.id, source_entry.index, transition.target_table_id
                    )
                })?;
            let target_table = &tables[target_table_index];
            let target_entry = target_table
                .entries
                .get(transition.target_entry_index)
                .with_context(|| {
                    format!(
                        "{} canonical entry {} targets missing entry {}:{}",
                        source_table.id,
                        source_entry.index,
                        transition.target_table_id,
                        transition.target_entry_index
                    )
                })?;
            ensure!(
                target_entry.target_kind == "script_entry_start",
                "{} canonical entry {} transition targets non-dialogue handler {}:{}",
                source_table.id,
                source_entry.index,
                transition.target_table_id,
                transition.target_entry_index
            );
            let target_key = MainDialogueGraphNodeKey {
                table_index: target_table_index,
                pointer_cpu_address: target_entry.pointer_cpu_address,
            };
            ensure!(
                nodes.contains_key(&target_key),
                "{} canonical entry {} transition target has no canonical graph node",
                source_table.id,
                source_entry.index
            );
            transition_edges.push(MainDialogueTransitionEdgeReport {
                source_table_id: source_table.id,
                source_canonical_entry_index: source_entry.index,
                source_entry_indices: dialogue_entry_indices(source_entry),
                source_pointer_cpu_address: source_entry.pointer_cpu_address,
                source_pointer_cpu_address_hex: format!(
                    "0x{:04X}",
                    source_entry.pointer_cpu_address
                ),
                source_file_offset: source_entry.file_offset,
                source_file_offset_hex: format!("0x{:05X}", source_entry.file_offset),
                control: segment.boundary_control,
                control_hex: format!("{:02X}", segment.boundary_control),
                target_table_id: target_table.id,
                target_entry_index: target_entry.index,
                target_canonical_entry_index: canonical_dialogue_entry_index(target_entry),
                target_pointer_cpu_address: target_entry.pointer_cpu_address,
                target_pointer_cpu_address_hex: format!(
                    "0x{:04X}",
                    target_entry.pointer_cpu_address
                ),
                target_file_offset: target_entry.file_offset,
                target_file_offset_hex: format!("0x{:05X}", target_entry.file_offset),
            });
            Some(target_key)
        } else {
            ensure!(
                matches!(segment.boundary_control, 0xEF | 0xE7),
                "{} canonical entry {} has unsupported graph boundary {:02X}",
                source_table.id,
                source_entry.index,
                segment.boundary_control
            );
            ensure!(
                segment.transition_target.is_none(),
                "{} canonical entry {} has a target on non-transition boundary {:02X}",
                source_table.id,
                source_entry.index,
                segment.boundary_control
            );
            None
        };
        states.insert(
            *source_key,
            MainDialogueGraphNodeState {
                boundary_control: segment.boundary_control,
                transition_target,
            },
        );
    }

    let closure = classify_main_dialogue_graph(&states)?;
    ensure!(
        transition_edges.len()
            == states
                .values()
                .filter(|state| state.transition_target.is_some())
                .count(),
        "main dialogue graph edge report coverage mismatch"
    );

    Ok(MainDialogueGraphReport {
        node_count: states.len(),
        transition_edge_count: transition_edges.len(),
        terminal_reachable_node_count: closure.terminal_reachable_node_count,
        caller_handoff_boundary_reachable_node_count: closure
            .caller_handoff_boundary_reachable_node_count,
        max_transition_edge_count_to_boundary: closure.max_transition_edge_count_to_boundary,
        cycle_count: 0,
        unresolved_node_count: 0,
        transition_edges,
    })
}

pub(super) fn classify_main_dialogue_graph(
    states: &BTreeMap<MainDialogueGraphNodeKey, MainDialogueGraphNodeState>,
) -> Result<MainDialogueGraphClosure> {
    let mut terminal_reachable_node_count = 0;
    let mut caller_handoff_boundary_reachable_node_count = 0;
    let mut max_transition_edge_count_to_boundary = 0;

    for start in states.keys().copied() {
        let mut current = start;
        let mut transition_edge_count = 0;
        let mut visited = BTreeMap::new();
        loop {
            ensure!(
                visited.insert(current, transition_edge_count).is_none(),
                "main dialogue graph cycle reached from table {} pointer {:04X}",
                start.table_index,
                start.pointer_cpu_address
            );
            let state = states.get(&current).with_context(|| {
                format!(
                    "main dialogue graph node is missing for table {} pointer {:04X}",
                    current.table_index, current.pointer_cpu_address
                )
            })?;
            match state.boundary_control {
                0xEF => {
                    ensure!(
                        state.transition_target.is_none(),
                        "terminal graph node has a transition target"
                    );
                    terminal_reachable_node_count += 1;
                    break;
                }
                0xE7 => {
                    ensure!(
                        state.transition_target.is_none(),
                        "caller-handoff graph node has a transition target"
                    );
                    caller_handoff_boundary_reachable_node_count += 1;
                    break;
                }
                0xE4 | 0xE6 => {
                    current = state
                        .transition_target
                        .context("transition graph node has no target")?;
                    transition_edge_count += 1;
                    max_transition_edge_count_to_boundary =
                        max_transition_edge_count_to_boundary.max(transition_edge_count);
                }
                code => anyhow::bail!("unsupported main dialogue graph boundary {code:02X}"),
            }
        }
    }

    ensure!(
        terminal_reachable_node_count + caller_handoff_boundary_reachable_node_count
            == states.len(),
        "main dialogue graph closure does not cover every node"
    );
    Ok(MainDialogueGraphClosure {
        terminal_reachable_node_count,
        caller_handoff_boundary_reachable_node_count,
        max_transition_edge_count_to_boundary,
    })
}

pub(super) fn is_canonical_dialogue_entry(entry: &DialogueEntryReport) -> bool {
    entry
        .alias_entry_indices
        .iter()
        .all(|alias_index| entry.index < *alias_index)
}

pub(super) fn canonical_dialogue_entry_index(entry: &DialogueEntryReport) -> usize {
    entry
        .alias_entry_indices
        .iter()
        .copied()
        .chain(std::iter::once(entry.index))
        .min()
        .expect("dialogue entry index set cannot be empty")
}

pub(super) fn dialogue_entry_indices(entry: &DialogueEntryReport) -> Vec<usize> {
    let mut indices = entry.alias_entry_indices.clone();
    indices.push(entry.index);
    indices.sort_unstable();
    indices
}
