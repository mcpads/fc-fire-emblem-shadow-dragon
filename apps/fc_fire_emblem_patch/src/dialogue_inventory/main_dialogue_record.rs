use super::*;

pub(super) fn inspect_main_record_prefix(
    source: &[u8],
    entry_file_offset: usize,
    bank_end: usize,
    table_id: &str,
    entry_index: usize,
) -> Result<MainRecordPrefixReport> {
    ensure!(
        entry_file_offset < bank_end,
        "{table_id} entry {entry_index} begins outside its source bank"
    );
    let e5_prefix_present = source[entry_file_offset] == OPTIONAL_E5_PREFIX_CODE;
    let e5_prefix_byte_count = if e5_prefix_present {
        OPTIONAL_PREFIX_BYTE_COUNT
    } else {
        0
    };
    let fixed_record_header_file_offset = entry_file_offset
        .checked_add(e5_prefix_byte_count)
        .context("main record E5 prefix range overflow")?;
    let after_fixed_record_header = fixed_record_header_file_offset
        .checked_add(FIXED_RECORD_HEADER_BYTE_COUNT)
        .context("main fixed record header range overflow")?;
    ensure!(
        after_fixed_record_header < bank_end,
        "{table_id} entry {entry_index} record prefix crosses its source bank"
    );
    let e8_prefix_present = source[after_fixed_record_header] == OPTIONAL_E8_PREFIX_CODE;
    let e8_prefix_byte_count = if e8_prefix_present {
        OPTIONAL_PREFIX_BYTE_COUNT
    } else {
        0
    };
    let first_line_file_offset = after_fixed_record_header
        .checked_add(e8_prefix_byte_count)
        .context("main record E8 prefix range overflow")?;
    ensure!(
        first_line_file_offset < bank_end,
        "{table_id} entry {entry_index} first line begins outside its source bank"
    );

    Ok(MainRecordPrefixReport {
        e5_prefix_present,
        e5_prefix_byte_count,
        fixed_record_header_file_offset,
        fixed_record_header_file_offset_hex: format!("0x{fixed_record_header_file_offset:05X}"),
        fixed_record_header_byte_count: FIXED_RECORD_HEADER_BYTE_COUNT,
        e8_prefix_present,
        e8_prefix_byte_count,
        first_line_file_offset,
        first_line_file_offset_hex: format!("0x{first_line_file_offset:05X}"),
        total_prefix_byte_count: first_line_file_offset - entry_file_offset,
    })
}

pub(super) fn scan_main_line(
    source: &[u8],
    line_file_offset: usize,
    bank_end: usize,
    table_id: &str,
    entry_index: usize,
) -> Result<MainLineReport> {
    let mut cursor = line_file_offset;
    let mut literal_byte_count = 0;
    let mut japanese_literal_byte_count = 0;
    let mut non_japanese_literal_byte_count = 0;
    let mut literal_file_offsets = Vec::new();
    let mut protected_original_alphanumeric_literal_byte_count = 0;
    let mut control_token_count = 0;
    let mut inline_operand_byte_count = 0;
    let mut control_count_map = BTreeMap::new();

    while cursor < bank_end && cursor - line_file_offset < MAX_MAIN_LINE_SCAN_BYTES {
        let code = source[cursor];
        let Some(control) = DIALOGUE_CONTROL_SPECS
            .iter()
            .find(|control| control.code == code)
        else {
            literal_byte_count += 1;
            literal_file_offsets.push(cursor);
            if is_japanese_text_code(code) {
                japanese_literal_byte_count += 1;
            } else {
                non_japanese_literal_byte_count += 1;
            }
            protected_original_alphanumeric_literal_byte_count +=
                usize::from((0x60..=0x83).contains(&code));
            cursor += 1;
            continue;
        };

        control_token_count += 1;
        inline_operand_byte_count += control.inline_operand_byte_count;
        *control_count_map.entry(code).or_insert(0) += 1;
        let storage_byte_count =
            1 + control.inline_operand_byte_count + control.transition_target_byte_count;
        let storage_end = cursor
            .checked_add(storage_byte_count)
            .context("main line control storage range overflow")?;
        ensure!(
            storage_end <= bank_end,
            "{table_id} entry {entry_index} line control {code:02X} crosses its source bank"
        );
        if code == 0xEC {
            ensure!(
                source[cursor + 1] <= 3,
                "{table_id} entry {entry_index} line EC operand is outside 0..3"
            );
        }
        if code == 0xDF {
            ensure!(
                source[cursor + 1] & 0x0F < 8,
                "{table_id} entry {entry_index} line DF low nibble is outside 0..7"
            );
        }

        if MAIN_LINE_END_CODES.contains(&code) {
            let transition_target = if control.transition_target_byte_count == 2 {
                let selector = source[cursor + 1];
                let target_entry_index = usize::from(source[cursor + 2]);
                let target_table = DIALOGUE_TABLE_SPECS
                    .iter()
                    .find(|candidate| {
                        candidate
                            .directory_group
                            .map(|group| (candidate.source_prg_bank << 4) | group)
                            == Some(selector)
                    })
                    .with_context(|| {
                        format!(
                            "{table_id} entry {entry_index} line transition selector {selector:02X} is not a declared main dialogue table"
                        )
                    })?;
                ensure!(
                    target_entry_index < target_table.pointer_count,
                    "{table_id} entry {entry_index} line transition target {selector:02X}:{target_entry_index:02X} is outside {}",
                    target_table.id
                );
                Some(TransitionTargetReport {
                    selector,
                    selector_hex: format!("0x{selector:02X}"),
                    target_table_id: target_table.id,
                    target_entry_index,
                })
            } else {
                ensure!(
                    control.transition_target_byte_count == 0,
                    "{table_id} entry {entry_index} line control {code:02X} has an unsupported transition width"
                );
                None
            };
            let storage = &source[line_file_offset..storage_end];
            return Ok(MainLineReport {
                file_offset: line_file_offset,
                file_offset_hex: format!("0x{line_file_offset:05X}"),
                storage_byte_count: storage.len(),
                storage_sha1: sha1_hex(storage),
                current_pointer_advance_bytes: cursor - line_file_offset
                    + control.current_pointer_advance_bytes,
                literal_byte_count,
                japanese_literal_byte_count,
                non_japanese_literal_byte_count,
                literal_file_offsets,
                protected_original_alphanumeric_literal_byte_count,
                control_token_count,
                inline_operand_byte_count,
                transition_target_byte_count: control.transition_target_byte_count,
                control_counts: control_usage_reports(
                    control_count_map,
                    &DIALOGUE_SCRIPT_CONTROL_CODES,
                ),
                line_end_control: code,
                line_end_control_hex: format!("{code:02X}"),
                transition_target,
            });
        }

        ensure!(
            control.transition_target_byte_count == 0,
            "{table_id} entry {entry_index} non-ending line control {code:02X} has transition bytes"
        );
        cursor = cursor
            .checked_add(control.current_pointer_advance_bytes)
            .context("main line current pointer overflow")?;
    }

    anyhow::bail!(
        "{table_id} entry {entry_index} has no recognized line end within {MAX_MAIN_LINE_SCAN_BYTES} bytes"
    )
}

pub(super) fn scan_main_linear_segment(
    source: &[u8],
    first_line_file_offset: usize,
    bank_end: usize,
    table_id: &str,
    entry_index: usize,
) -> Result<MainLinearSegmentReport> {
    let mut line_file_offset = first_line_file_offset;
    let mut lines = Vec::new();

    for _ in 0..MAX_MAIN_LINEAR_SEGMENT_LINES {
        let line = scan_main_line(source, line_file_offset, bank_end, table_id, entry_index)?;
        let boundary_control = line.line_end_control;
        let next_line_file_offset = line_file_offset
            .checked_add(line.current_pointer_advance_bytes)
            .context("main linear segment current pointer overflow")?;
        lines.push(line);

        if MAIN_LINEAR_SEGMENT_BOUNDARY_CODES.contains(&boundary_control) {
            let final_line = lines.last().context("main linear segment has no lines")?;
            let storage_end = final_line
                .file_offset
                .checked_add(final_line.storage_byte_count)
                .context("main linear segment storage range overflow")?;
            ensure!(
                storage_end <= bank_end,
                "{table_id} entry {entry_index} linear segment crosses its source bank"
            );
            let storage = &source[first_line_file_offset..storage_end];
            return Ok(MainLinearSegmentReport {
                start_file_offset: first_line_file_offset,
                start_file_offset_hex: format!("0x{first_line_file_offset:05X}"),
                line_count: lines.len(),
                storage_byte_count: storage.len(),
                storage_sha1: sha1_hex(storage),
                japanese_literal_byte_count: lines
                    .iter()
                    .map(|line| line.japanese_literal_byte_count)
                    .sum(),
                non_japanese_literal_byte_count: lines
                    .iter()
                    .map(|line| line.non_japanese_literal_byte_count)
                    .sum(),
                protected_original_alphanumeric_literal_byte_count: lines
                    .iter()
                    .map(|line| line.protected_original_alphanumeric_literal_byte_count)
                    .sum(),
                boundary_control,
                boundary_control_hex: format!("{boundary_control:02X}"),
                boundary_kind: match boundary_control {
                    0xEF => "terminal",
                    0xE7 => "caller_handoff",
                    0xE4 | 0xE6 => "transition_target",
                    _ => unreachable!("declared boundary code is not classified"),
                },
                transition_target: final_line.transition_target.clone(),
                lines,
            });
        }

        ensure!(
            MAIN_LINEAR_CONTINUATION_CODES.contains(&boundary_control),
            "{table_id} entry {entry_index} line ends with unclassified control {boundary_control:02X}"
        );
        ensure!(
            next_line_file_offset < bank_end,
            "{table_id} entry {entry_index} next linear line begins outside its source bank"
        );
        line_file_offset = next_line_file_offset;
    }

    anyhow::bail!(
        "{table_id} entry {entry_index} exceeds {MAX_MAIN_LINEAR_SEGMENT_LINES} linear lines without a terminal, caller-handoff, or transition boundary"
    )
}

pub(super) fn build_main_record_storage(
    source: &[u8],
    record_file_offset: usize,
    bank_end: usize,
    prefix: &MainRecordPrefixReport,
    segment: &MainLinearSegmentReport,
    table_id: &str,
    entry_index: usize,
) -> Result<MainRecordStorageReport> {
    ensure!(
        prefix.first_line_file_offset == segment.start_file_offset,
        "{table_id} entry {entry_index} record prefix and linear segment are disconnected"
    );
    ensure!(
        prefix.first_line_file_offset
            == record_file_offset
                .checked_add(prefix.total_prefix_byte_count)
                .context("main record prefix end overflow")?,
        "{table_id} entry {entry_index} record prefix length is inconsistent"
    );
    let end_file_offset_exclusive = segment
        .start_file_offset
        .checked_add(segment.storage_byte_count)
        .context("main record storage end overflow")?;
    ensure!(
        end_file_offset_exclusive <= bank_end,
        "{table_id} entry {entry_index} record storage crosses its source bank"
    );
    ensure!(
        record_file_offset < end_file_offset_exclusive,
        "{table_id} entry {entry_index} record storage is empty"
    );
    let storage = source
        .get(record_file_offset..end_file_offset_exclusive)
        .context("main record storage is outside the source")?;
    ensure!(
        storage.len() == prefix.total_prefix_byte_count + segment.storage_byte_count,
        "{table_id} entry {entry_index} record storage length is inconsistent"
    );

    Ok(MainRecordStorageReport {
        file_offset: record_file_offset,
        file_offset_hex: format!("0x{record_file_offset:05X}"),
        end_file_offset_exclusive,
        end_file_offset_exclusive_hex: format!("0x{end_file_offset_exclusive:05X}"),
        storage_byte_count: storage.len(),
        storage_sha1: sha1_hex(storage),
        prefix_byte_count: prefix.total_prefix_byte_count,
        linear_segment_storage_byte_count: segment.storage_byte_count,
        boundary_control: segment.boundary_control,
        boundary_control_hex: format!("{:02X}", segment.boundary_control),
        boundary_kind: segment.boundary_kind,
    })
}

pub(super) fn summarize_main_record_storage(
    ranges: &[MainRecordStorageRange],
) -> Result<MainRecordStorageSummary> {
    let mut events = BTreeMap::<usize, isize>::new();
    let mut consumed_storage_byte_count = 0;
    let mut max_storage_byte_count = 0;
    for range in ranges {
        ensure!(
            range.start < range.end_exclusive,
            "main dialogue record-storage range is empty or reversed"
        );
        let storage_byte_count = range.end_exclusive - range.start;
        consumed_storage_byte_count += storage_byte_count;
        max_storage_byte_count = max_storage_byte_count.max(storage_byte_count);
        *events.entry(range.start).or_insert(0) += 1;
        *events.entry(range.end_exclusive).or_insert(0) -= 1;
    }

    let overlapping_record_pair_count = ranges
        .iter()
        .enumerate()
        .map(|(index, range)| {
            ranges[index + 1..]
                .iter()
                .filter(|other| {
                    range.start < other.end_exclusive && other.start < range.end_exclusive
                })
                .count()
        })
        .sum();

    let mut unique_storage_byte_count = 0;
    let mut shared_storage_byte_count = 0;
    let mut max_overlap_depth = 0;
    let mut active_range_count = 0_isize;
    let mut previous_offset = None;
    for (offset, delta) in events {
        if let Some(previous_offset) = previous_offset {
            let span_byte_count = offset - previous_offset;
            if active_range_count > 0 {
                unique_storage_byte_count += span_byte_count;
            }
            if active_range_count > 1 {
                shared_storage_byte_count += span_byte_count;
            }
        }
        active_range_count += delta;
        ensure!(
            active_range_count >= 0,
            "main dialogue record-storage coverage became negative"
        );
        max_overlap_depth = max_overlap_depth.max(active_range_count as usize);
        previous_offset = Some(offset);
    }
    ensure!(
        active_range_count == 0,
        "main dialogue record-storage coverage did not close"
    );

    Ok(MainRecordStorageSummary {
        unique_record_count: ranges.len(),
        consumed_storage_byte_count,
        unique_storage_byte_count,
        shared_storage_byte_count,
        overlapping_record_pair_count,
        max_overlap_depth,
        max_storage_byte_count,
    })
}

pub(super) fn summarize_main_literal_storage(
    source: &[u8],
    tables: &[DialogueTableReport],
) -> Result<MainLiteralStorageSummary> {
    let mut flags_by_offset = BTreeMap::<usize, MainLiteralStorageFlags>::new();
    for table in tables
        .iter()
        .filter(|table| table.directory_binding.is_some())
    {
        for entry in table.entries.iter().filter(|entry| {
            entry.target_kind == "script_entry_start" && is_canonical_dialogue_entry(entry)
        }) {
            let storage = entry.main_record_storage.as_ref().with_context(|| {
                format!(
                    "{} canonical entry {} has no record-storage range",
                    table.id, entry.index
                )
            })?;
            let segment = entry.main_linear_segment.as_ref().with_context(|| {
                format!(
                    "{} canonical entry {} has no linear segment",
                    table.id, entry.index
                )
            })?;
            let literal_offsets = segment
                .lines
                .iter()
                .flat_map(|line| line.literal_file_offsets.iter().copied())
                .collect::<BTreeSet<_>>();
            ensure!(
                literal_offsets.iter().all(|offset| {
                    (storage.file_offset..storage.end_file_offset_exclusive).contains(offset)
                }),
                "{} canonical entry {} has a literal outside its record storage",
                table.id,
                entry.index
            );

            for offset in storage.file_offset..storage.end_file_offset_exclusive {
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
    }

    Ok(MainLiteralStorageSummary {
        unique_japanese_literal_storage_byte_count: flags_by_offset
            .values()
            .filter(|flags| flags.japanese_literal)
            .count(),
        unique_non_japanese_literal_storage_byte_count: flags_by_offset
            .values()
            .filter(|flags| flags.non_japanese_literal)
            .count(),
        literal_kind_conflict_storage_byte_count: flags_by_offset
            .values()
            .filter(|flags| flags.japanese_literal && flags.non_japanese_literal)
            .count(),
        literal_structural_conflict_storage_byte_count: flags_by_offset
            .values()
            .filter(|flags| {
                (flags.japanese_literal || flags.non_japanese_literal) && flags.structural
            })
            .count(),
        safe_japanese_translation_source_byte_count: flags_by_offset
            .values()
            .filter(|flags| {
                flags.japanese_literal && !flags.non_japanese_literal && !flags.structural
            })
            .count(),
    })
}
