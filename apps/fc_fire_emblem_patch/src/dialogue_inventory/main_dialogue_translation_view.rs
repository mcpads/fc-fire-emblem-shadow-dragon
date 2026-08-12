use super::*;

pub(crate) fn inspect_main_dialogue_storage(
    source: &[u8],
) -> Result<MainDialogueStorageInspection> {
    let report = build_report(source)?;
    let records =
        build_main_dialogue_storage_records(source, &report.tables, &report.main_dialogue_graph)?;
    ensure!(
        records.len() == report.summary.main_record_count,
        "main dialogue storage record export lost coverage"
    );
    let safe_japanese_translation_source_byte_count =
        safe_main_dialogue_japanese_literal_offsets(source, &records)?.len();
    Ok(MainDialogueStorageInspection {
        records,
        safe_japanese_translation_source_byte_count,
    })
}

pub(crate) fn inspect_main_dialogue_runtime_identities(
    source: &[u8],
) -> Result<Vec<MainDialogueRuntimeIdentityBinding>> {
    let report = build_report(source)?;
    let records =
        build_main_dialogue_storage_records(source, &report.tables, &report.main_dialogue_graph)?;
    let tables = report
        .tables
        .iter()
        .filter_map(|table| {
            table
                .directory_binding
                .as_ref()
                .map(|directory| (table.id, (directory.selector, table.pointer_count)))
        })
        .collect::<BTreeMap<_, _>>();
    ensure!(
        tables.len() == 8,
        "main dialogue runtime identity table population changed"
    );
    let bindings = records
        .iter()
        .map(|record| {
            let (directory_selector, pointer_count) =
                tables.get(record.table_id).copied().with_context(|| {
                    format!("{} has no runtime directory selector", record.table_id)
                })?;
            ensure!(
                record
                    .entry_indices
                    .iter()
                    .all(|index| *index < pointer_count),
                "{} runtime entry index exceeds its pointer table",
                record.table_id
            );
            Ok(MainDialogueRuntimeIdentityBinding {
                record_id: format!("{}:{:03}", record.table_id, record.canonical_entry_index),
                directory_selector,
                pointer_count,
                entry_indices: record.entry_indices.clone(),
            })
        })
        .collect::<Result<Vec<_>>>()?;
    ensure!(
        bindings.len() == 504
            && bindings
                .iter()
                .map(|binding| binding.entry_indices.len())
                .sum::<usize>()
                == 517,
        "main dialogue runtime identity binding population changed"
    );
    Ok(bindings)
}

pub(super) fn build_main_dialogue_storage_records(
    source: &[u8],
    tables: &[DialogueTableReport],
    graph: &MainDialogueGraphReport,
) -> Result<Vec<MainDialogueStorageRecord>> {
    let transition_targets = graph
        .transition_edges
        .iter()
        .map(|edge| (edge.target_table_id, edge.target_canonical_entry_index))
        .collect::<BTreeSet<_>>();
    tables
        .iter()
        .filter(|table| table.directory_binding.is_some())
        .flat_map(|table| {
            table
                .entries
                .iter()
                .filter(|entry| {
                    entry.target_kind == "script_entry_start" && is_canonical_dialogue_entry(entry)
                })
                .map(move |entry| (table, entry))
        })
        .map(|(table, entry)| {
            let direct_storage = entry.main_record_storage.as_ref().with_context(|| {
                format!(
                    "{} canonical entry {} has no record-storage range",
                    table.id, entry.index
                )
            })?;
            let direct_segment = entry
                .main_linear_segment
                .as_ref()
                .context("canonical main dialogue entry has no linear segment")?;
            let (prefix_byte_count, segment) = if transition_targets
                .contains(&(table.id, canonical_dialogue_entry_index(entry)))
            {
                let bank_end = switchable_bank_file_start(table.source_prg_bank)
                    .checked_add(PRG_BANK_SIZE)
                    .context("main transition target source-bank range overflow")?;
                let prefix_byte_count = inspect_main_transition_prefix_byte_count(
                    source,
                    entry.file_offset,
                    bank_end,
                    table.id,
                    entry.index,
                )?;
                let first_line_file_offset = entry
                    .file_offset
                    .checked_add(prefix_byte_count)
                    .context("main transition target first-line range overflow")?;
                let segment = scan_main_linear_segment(
                    source,
                    first_line_file_offset,
                    bank_end,
                    table.id,
                    entry.index,
                )?;
                let end_file_offset_exclusive = segment
                    .start_file_offset
                    .checked_add(segment.storage_byte_count)
                    .context("main transition target storage range overflow")?;
                ensure!(
                    end_file_offset_exclusive == direct_storage.end_file_offset_exclusive,
                    "{} canonical transition target {} changes its proven storage end",
                    table.id,
                    entry.index
                );
                ensure!(
                    segment.boundary_control == direct_segment.boundary_control,
                    "{} canonical transition target {} changes its graph boundary",
                    table.id,
                    entry.index
                );
                ensure!(
                    transition_targets_match(
                        segment.transition_target.as_ref(),
                        direct_segment.transition_target.as_ref()
                    ),
                    "{} canonical transition target {} changes its graph destination",
                    table.id,
                    entry.index
                );
                (prefix_byte_count, segment)
            } else {
                (direct_storage.prefix_byte_count, direct_segment.clone())
            };
            let literal_file_offsets = segment
                .lines
                .iter()
                .flat_map(|line| line.literal_file_offsets.iter().copied())
                .collect();
            let lines = segment
                .lines
                .iter()
                .map(|line| MainDialogueStorageLine {
                    file_offset: line.file_offset,
                    storage_byte_count: line.storage_byte_count,
                    storage_sha1: line.storage_sha1.clone(),
                    line_end_control: line.line_end_control,
                    literal_file_offsets: line.literal_file_offsets.clone(),
                })
                .collect();
            Ok(MainDialogueStorageRecord {
                table_id: table.id,
                source_prg_bank: table.source_prg_bank,
                canonical_entry_index: canonical_dialogue_entry_index(entry),
                entry_indices: dialogue_entry_indices(entry),
                pointer_file_offsets: dialogue_entry_indices(entry)
                    .iter()
                    .map(|index| table.pointer_table_file_offset + index * 2)
                    .collect(),
                pointer_cpu_address: entry.pointer_cpu_address,
                file_offset: direct_storage.file_offset,
                end_file_offset_exclusive: direct_storage.end_file_offset_exclusive,
                storage_byte_count: direct_storage.storage_byte_count,
                storage_sha1: direct_storage.storage_sha1.clone(),
                prefix_byte_count,
                boundary_control: segment.boundary_control,
                literal_file_offsets,
                lines,
            })
        })
        .collect()
}

fn transition_targets_match(
    left: Option<&TransitionTargetReport>,
    right: Option<&TransitionTargetReport>,
) -> bool {
    match (left, right) {
        (Some(left), Some(right)) => {
            left.selector == right.selector
                && left.target_table_id == right.target_table_id
                && left.target_entry_index == right.target_entry_index
        }
        (None, None) => true,
        _ => false,
    }
}

pub(super) fn safe_main_dialogue_japanese_literal_offsets(
    source: &[u8],
    records: &[MainDialogueStorageRecord],
) -> Result<BTreeSet<usize>> {
    let mut flags_by_offset = BTreeMap::<usize, MainLiteralStorageFlags>::new();
    for record in records {
        let literal_offsets = record
            .literal_file_offsets
            .iter()
            .copied()
            .collect::<BTreeSet<_>>();
        ensure!(
            literal_offsets.iter().all(|offset| {
                (record.file_offset..record.end_file_offset_exclusive).contains(offset)
            }),
            "{} canonical entry {} has a literal outside its record storage",
            record.table_id,
            record.canonical_entry_index
        );
        for offset in record.file_offset..record.end_file_offset_exclusive {
            let flags = flags_by_offset.entry(offset).or_default();
            if literal_offsets.contains(&offset) {
                let code = *source
                    .get(offset)
                    .context("main dialogue literal offset is outside the source")?;
                if is_japanese_text_code(code) {
                    flags.japanese_literal = true;
                } else {
                    flags.non_japanese_literal = true;
                }
            } else {
                flags.structural = true;
            }
        }
    }
    Ok(flags_by_offset
        .into_iter()
        .filter_map(|(offset, flags)| {
            (flags.japanese_literal && !flags.non_japanese_literal && !flags.structural)
                .then_some(offset)
        })
        .collect())
}
