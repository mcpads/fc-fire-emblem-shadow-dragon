use super::*;

#[derive(Debug)]
pub(crate) struct MainDialogueEntryModeInspection {
    pub(crate) canonical_record_count: usize,
    pub(crate) transition_edge_count: usize,
    pub(crate) transition_targets: Vec<MainDialogueTransitionTargetMode>,
}

#[derive(Debug)]
pub(crate) struct MainDialogueTransitionTargetMode {
    pub(crate) record_id: String,
    pub(crate) record_file_offset: usize,
    pub(crate) leading_source_bytes: [u8; 6],
    pub(crate) incoming_transition_edge_count: usize,
    pub(crate) direct_prefix_byte_count: usize,
    pub(crate) transition_prefix_byte_count: usize,
    pub(crate) transition_to_direct_body_delta: isize,
    pub(crate) record_end_file_offset_exclusive: usize,
    pub(crate) direct_lines: Vec<MainDialogueStorageLine>,
    pub(crate) transition_lines: Vec<MainDialogueStorageLine>,
}

pub(crate) fn inspect_main_dialogue_entry_modes(
    source: &[u8],
) -> Result<MainDialogueEntryModeInspection> {
    let report = build_report(source)?;
    let mut incoming_edges = BTreeMap::<(&str, usize), usize>::new();
    for edge in &report.main_dialogue_graph.transition_edges {
        *incoming_edges
            .entry((edge.target_table_id, edge.target_canonical_entry_index))
            .or_default() += 1;
    }

    let mut transition_targets = Vec::with_capacity(incoming_edges.len());
    for ((table_id, canonical_entry_index), incoming_transition_edge_count) in incoming_edges {
        let table = report
            .tables
            .iter()
            .find(|table| table.id == table_id)
            .with_context(|| format!("transition target table {table_id} is absent"))?;
        let entry = table
            .entries
            .iter()
            .find(|entry| {
                entry.target_kind == "script_entry_start"
                    && canonical_dialogue_entry_index(entry) == canonical_entry_index
            })
            .with_context(|| {
                format!("transition target {table_id}:{canonical_entry_index:03} is absent")
            })?;
        let direct_prefix_byte_count = entry
            .main_record_prefix
            .as_ref()
            .context("main dialogue transition target has no direct-entry prefix")?
            .total_prefix_byte_count;
        let direct_segment = entry
            .main_linear_segment
            .as_ref()
            .context("main dialogue transition target has no direct-entry segment")?;
        let bank_end = switchable_bank_file_start(table.source_prg_bank)
            .checked_add(PRG_BANK_SIZE)
            .context("main dialogue transition-target bank range overflow")?;
        let transition_prefix_byte_count = inspect_main_transition_prefix_byte_count(
            source,
            entry.file_offset,
            bank_end,
            table.id,
            entry.index,
        )?;
        let transition_segment = scan_main_linear_segment(
            source,
            entry
                .file_offset
                .checked_add(transition_prefix_byte_count)
                .context("main dialogue transition first-line range overflow")?,
            bank_end,
            table.id,
            entry.index,
        )?;
        let direct_end = direct_segment
            .start_file_offset
            .checked_add(direct_segment.storage_byte_count)
            .context("main dialogue direct segment range overflow")?;
        let transition_end = transition_segment
            .start_file_offset
            .checked_add(transition_segment.storage_byte_count)
            .context("main dialogue transition segment range overflow")?;
        ensure!(
            direct_end == transition_end,
            "transition target {table_id}:{canonical_entry_index:03} entry modes do not converge at the record end"
        );
        ensure!(
            direct_segment.boundary_control == transition_segment.boundary_control,
            "transition target {table_id}:{canonical_entry_index:03} entry modes change the record boundary"
        );
        let transition_to_direct_body_delta =
            signed_body_delta(direct_prefix_byte_count, transition_prefix_byte_count)?;
        ensure!(
            transition_to_direct_body_delta != 0,
            "transition target {table_id}:{canonical_entry_index:03} has no consumer-entry delta"
        );
        let leading_source_bytes = source
            .get(entry.file_offset..entry.file_offset + 6)
            .with_context(|| {
                format!(
                    "transition target {table_id}:{canonical_entry_index:03} leading bytes are outside the source"
                )
            })?
            .try_into()
            .expect("six-byte source range has exact array length");
        transition_targets.push(MainDialogueTransitionTargetMode {
            record_id: format!("{table_id}:{canonical_entry_index:03}"),
            record_file_offset: entry.file_offset,
            leading_source_bytes,
            incoming_transition_edge_count,
            direct_prefix_byte_count,
            transition_prefix_byte_count,
            transition_to_direct_body_delta,
            record_end_file_offset_exclusive: direct_end,
            direct_lines: direct_segment.lines.iter().map(storage_line).collect(),
            transition_lines: transition_segment.lines.iter().map(storage_line).collect(),
        });
    }
    transition_targets.sort_unstable_by(|left, right| left.record_id.cmp(&right.record_id));
    ensure!(
        transition_targets
            .iter()
            .map(|target| target.incoming_transition_edge_count)
            .sum::<usize>()
            == report.main_dialogue_graph.transition_edge_count,
        "main dialogue entry-mode inspection lost transition edges"
    );

    Ok(MainDialogueEntryModeInspection {
        canonical_record_count: report.main_dialogue_graph.node_count,
        transition_edge_count: report.main_dialogue_graph.transition_edge_count,
        transition_targets,
    })
}

fn storage_line(line: &MainLineReport) -> MainDialogueStorageLine {
    MainDialogueStorageLine {
        file_offset: line.file_offset,
        storage_byte_count: line.storage_byte_count,
        storage_sha1: line.storage_sha1.clone(),
        line_end_control: line.line_end_control,
        literal_file_offsets: line.literal_file_offsets.clone(),
    }
}

fn signed_body_delta(direct: usize, transition: usize) -> Result<isize> {
    let direct = isize::try_from(direct).context("direct prefix size exceeds isize")?;
    let transition = isize::try_from(transition).context("transition prefix size exceeds isize")?;
    Ok(direct - transition)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn body_delta_keeps_both_consumer_directions() {
        assert_eq!(signed_body_delta(4, 0).unwrap(), 4);
        assert_eq!(signed_body_delta(4, 6).unwrap(), -2);
    }
}
