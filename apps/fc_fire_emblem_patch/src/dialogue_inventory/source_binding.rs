use super::*;

pub(super) fn extract_dialogue_table(
    source: &[u8],
    spec: &DialogueTableSpec,
) -> Result<DialogueTableReport> {
    ensure!(
        source.len() >= HEADER_SIZE + PRG_SIZE,
        "source is shorter than the PRG region"
    );
    ensure!(
        spec.source_prg_bank < 0x0F,
        "{} uses fixed or unavailable PRG bank {:02X}",
        spec.id,
        spec.source_prg_bank
    );

    let bank_start = switchable_bank_file_start(spec.source_prg_bank);
    let bank_end = bank_start + PRG_BANK_SIZE;
    let pointer_table_byte_count = spec
        .pointer_count
        .checked_mul(2)
        .context("dialogue pointer table length overflow")?;
    let pointer_table_end = spec
        .pointer_table_file_offset
        .checked_add(pointer_table_byte_count)
        .context("dialogue pointer table range overflow")?;
    ensure!(
        spec.pointer_count != 0,
        "{} declares an empty pointer table",
        spec.id
    );
    ensure!(
        spec.pointer_table_file_offset >= bank_start && pointer_table_end <= bank_end,
        "{} pointer table is outside source PRG bank {:02X}",
        spec.id,
        spec.source_prg_bank
    );
    ensure!(
        spec.data_file_start >= pointer_table_end && spec.data_file_start < bank_end,
        "{} data start is outside the post-table source-bank range",
        spec.id
    );

    let pointer_table_cpu_address =
        switchable_file_to_cpu(spec.source_prg_bank, spec.pointer_table_file_offset)?;
    let directory_binding = spec
        .directory_group
        .map(|group| {
            validate_directory_binding(
                source,
                spec.source_prg_bank,
                group,
                pointer_table_cpu_address,
                spec.directory_selector_use,
                spec.id,
            )
        })
        .transpose()?;
    ensure!(
        spec.directory_group.is_some() || spec.directory_selector_use.is_none(),
        "{} declares a directory selector use without a directory root",
        spec.id
    );
    ensure!(
        !(spec.directory_group.is_some() && spec.separate_consumer.is_some()),
        "{} declares two consumer bindings",
        spec.id
    );
    let separate_consumer_binding = spec
        .separate_consumer
        .map(|consumer| {
            validate_separate_consumer(source, consumer, pointer_table_cpu_address, spec.id)
        })
        .transpose()?;

    let pointer_table_bytes = &source[spec.pointer_table_file_offset..pointer_table_end];
    let pointers = pointer_table_bytes
        .chunks_exact(2)
        .map(|bytes| u16::from_le_bytes([bytes[0], bytes[1]]))
        .collect::<Vec<_>>();
    let mut indices_by_pointer: BTreeMap<u16, Vec<usize>> = BTreeMap::new();
    for (index, pointer) in pointers.iter().copied().enumerate() {
        indices_by_pointer.entry(pointer).or_default().push(index);
    }

    let mut entries = Vec::with_capacity(pointers.len());
    let mut ordinary_target_file_offsets = Vec::new();
    for (index, pointer) in pointers.iter().copied().enumerate() {
        let alias_entry_indices = indices_by_pointer[&pointer]
            .iter()
            .copied()
            .filter(|other| *other != index)
            .collect();
        if let Some(handler) = spec
            .allowed_handler_targets
            .iter()
            .find(|handler| handler.cpu_address == pointer)
        {
            let file_offset =
                validate_handler_target(source, spec.source_prg_bank, handler, spec.id, index)?;
            entries.push(DialogueEntryReport {
                index,
                pointer_cpu_address: pointer,
                pointer_cpu_address_hex: format!("0x{pointer:04X}"),
                target_kind: "code_handler",
                file_offset,
                file_offset_hex: format!("0x{file_offset:05X}"),
                handler_role: Some(handler.role),
                alias_entry_indices,
                main_record_prefix: None,
                main_first_line: None,
                main_linear_segment: None,
                main_record_storage: None,
                battle_record_storage: None,
            });
            continue;
        }

        let file_offset = switchable_cpu_to_file_offset(spec.source_prg_bank, pointer)
            .with_context(|| {
                format!(
                    "{} entry {index} pointer {pointer:04X} is outside its switchable PRG window",
                    spec.id
                )
            })?;
        ensure!(
            file_offset >= spec.data_file_start && file_offset < bank_end,
            "{} entry {index} points outside its declared data region",
            spec.id
        );
        ordinary_target_file_offsets.push(file_offset);
        let main_record_prefix = spec
            .directory_group
            .map(|_| inspect_main_record_prefix(source, file_offset, bank_end, spec.id, index))
            .transpose()?;
        let main_linear_segment = main_record_prefix
            .as_ref()
            .map(|prefix| {
                scan_main_linear_segment(
                    source,
                    prefix.first_line_file_offset,
                    bank_end,
                    spec.id,
                    index,
                )
            })
            .transpose()?;
        let main_first_line = main_linear_segment
            .as_ref()
            .and_then(|segment| segment.lines.first().cloned());
        let main_record_storage = match (&main_record_prefix, &main_linear_segment) {
            (Some(prefix), Some(segment)) => Some(build_main_record_storage(
                source,
                file_offset,
                bank_end,
                prefix,
                segment,
                spec.id,
                index,
            )?),
            (None, None) => None,
            _ => anyhow::bail!(
                "{} entry {index} has incomplete main record storage evidence",
                spec.id
            ),
        };
        let battle_record_storage = (spec.id == BATTLE_DIALOGUE_TABLE_ID)
            .then(|| {
                scan_battle_dialogue_record(
                    source,
                    file_offset,
                    bank_end,
                    &BATTLE_DIALOGUE_REFERENCED_HEADERS,
                    spec.id,
                    index,
                )
            })
            .transpose()?;
        entries.push(DialogueEntryReport {
            index,
            pointer_cpu_address: pointer,
            pointer_cpu_address_hex: format!("0x{pointer:04X}"),
            target_kind: "script_entry_start",
            file_offset,
            file_offset_hex: format!("0x{file_offset:05X}"),
            handler_role: None,
            alias_entry_indices,
            main_record_prefix,
            main_first_line,
            main_linear_segment,
            main_record_storage,
            battle_record_storage,
        });
    }
    ensure!(
        ordinary_target_file_offsets.iter().min().copied() == Some(spec.data_file_start),
        "{} first declared data byte is not referenced by its pointer table",
        spec.id
    );

    let alias_groups = indices_by_pointer
        .values()
        .filter(|indices| indices.len() > 1)
        .collect::<Vec<_>>();
    let alias_group_count = alias_groups.len();
    let aliased_entry_count = alias_groups.iter().map(|indices| indices.len()).sum();
    let handler_target_entry_count = entries
        .iter()
        .filter(|entry| entry.target_kind == "code_handler")
        .count();
    let unique_script_entry_count = entries
        .iter()
        .filter(|entry| {
            entry.target_kind == "script_entry_start"
                && indices_by_pointer[&entry.pointer_cpu_address][0] == entry.index
        })
        .count();
    let main_record_prefix_summary = if spec.directory_group.is_some() {
        let unique_prefixes = entries
            .iter()
            .filter(|entry| indices_by_pointer[&entry.pointer_cpu_address][0] == entry.index)
            .filter_map(|entry| entry.main_record_prefix.as_ref())
            .collect::<Vec<_>>();
        ensure!(
            unique_prefixes.len() == unique_script_entry_count,
            "{} main record prefix coverage does not match its unique targets",
            spec.id
        );
        Some(MainRecordPrefixSummary {
            unique_target_count: unique_prefixes.len(),
            e5_prefix_unique_target_count: unique_prefixes
                .iter()
                .filter(|prefix| prefix.e5_prefix_present)
                .count(),
            e8_prefix_unique_target_count: unique_prefixes
                .iter()
                .filter(|prefix| prefix.e8_prefix_present)
                .count(),
            both_optional_prefixes_unique_target_count: unique_prefixes
                .iter()
                .filter(|prefix| prefix.e5_prefix_present && prefix.e8_prefix_present)
                .count(),
            no_optional_prefix_unique_target_count: unique_prefixes
                .iter()
                .filter(|prefix| !prefix.e5_prefix_present && !prefix.e8_prefix_present)
                .count(),
        })
    } else {
        None
    };
    let main_first_line_summary = if spec.directory_group.is_some() {
        let unique_lines = entries
            .iter()
            .filter(|entry| indices_by_pointer[&entry.pointer_cpu_address][0] == entry.index)
            .filter_map(|entry| entry.main_first_line.as_ref())
            .collect::<Vec<_>>();
        ensure!(
            unique_lines.len() == unique_script_entry_count,
            "{} first-line coverage does not match its unique script entries",
            spec.id
        );
        let mut line_end_control_count_map = BTreeMap::new();
        for line in &unique_lines {
            *line_end_control_count_map
                .entry(line.line_end_control)
                .or_insert(0) += 1;
        }
        Some(MainFirstLineSummary {
            unique_line_count: unique_lines.len(),
            max_storage_byte_count: unique_lines
                .iter()
                .map(|line| line.storage_byte_count)
                .max()
                .unwrap_or(0),
            japanese_literal_byte_count: unique_lines
                .iter()
                .map(|line| line.japanese_literal_byte_count)
                .sum(),
            non_japanese_literal_byte_count: unique_lines
                .iter()
                .map(|line| line.non_japanese_literal_byte_count)
                .sum(),
            protected_original_alphanumeric_literal_byte_count: unique_lines
                .iter()
                .map(|line| line.protected_original_alphanumeric_literal_byte_count)
                .sum(),
            line_end_control_counts: control_usage_reports(
                line_end_control_count_map,
                &MAIN_LINE_END_CODES,
            ),
        })
    } else {
        None
    };
    let main_linear_segment_summary = if spec.directory_group.is_some() {
        let unique_segments = entries
            .iter()
            .filter(|entry| indices_by_pointer[&entry.pointer_cpu_address][0] == entry.index)
            .filter_map(|entry| entry.main_linear_segment.as_ref())
            .collect::<Vec<_>>();
        ensure!(
            unique_segments.len() == unique_script_entry_count,
            "{} linear-segment coverage does not match its unique script entries",
            spec.id
        );
        let mut boundary_control_count_map = BTreeMap::new();
        for segment in &unique_segments {
            *boundary_control_count_map
                .entry(segment.boundary_control)
                .or_insert(0) += 1;
        }
        Some(MainLinearSegmentSummary {
            unique_segment_count: unique_segments.len(),
            total_line_count: unique_segments
                .iter()
                .map(|segment| segment.line_count)
                .sum(),
            max_line_count: unique_segments
                .iter()
                .map(|segment| segment.line_count)
                .max()
                .unwrap_or(0),
            japanese_literal_byte_count: unique_segments
                .iter()
                .map(|segment| segment.japanese_literal_byte_count)
                .sum(),
            non_japanese_literal_byte_count: unique_segments
                .iter()
                .map(|segment| segment.non_japanese_literal_byte_count)
                .sum(),
            protected_original_alphanumeric_literal_byte_count: unique_segments
                .iter()
                .map(|segment| segment.protected_original_alphanumeric_literal_byte_count)
                .sum(),
            boundary_control_counts: control_usage_reports(
                boundary_control_count_map,
                &MAIN_LINEAR_SEGMENT_BOUNDARY_CODES,
            ),
            transition_count: unique_segments
                .iter()
                .filter(|segment| segment.transition_target.is_some())
                .count(),
        })
    } else {
        None
    };
    let main_record_storage_summary = if spec.directory_group.is_some() {
        let unique_records = entries
            .iter()
            .filter(|entry| indices_by_pointer[&entry.pointer_cpu_address][0] == entry.index)
            .filter_map(|entry| {
                entry
                    .main_record_storage
                    .as_ref()
                    .map(|record| (entry.index, record))
            })
            .collect::<Vec<_>>();
        ensure!(
            unique_records.len() == unique_script_entry_count,
            "{} record-storage coverage does not match its unique script entries",
            spec.id
        );
        let ranges = unique_records
            .iter()
            .map(|(_, record)| MainRecordStorageRange {
                start: record.file_offset,
                end_exclusive: record.end_file_offset_exclusive,
            })
            .collect::<Vec<_>>();
        Some(summarize_main_record_storage(&ranges)?)
    } else {
        None
    };
    let battle_record_storage_summary = if spec.id == BATTLE_DIALOGUE_TABLE_ID {
        Some(summarize_battle_dialogue_storage(
            source,
            &entries,
            &indices_by_pointer,
            spec.data_file_start,
        )?)
    } else {
        None
    };

    Ok(DialogueTableReport {
        id: spec.id,
        role: spec.role,
        source_prg_bank: spec.source_prg_bank,
        source_prg_bank_hex: format!("0x{:02X}", spec.source_prg_bank),
        pointer_table_cpu_address,
        pointer_table_cpu_address_hex: format!("0x{pointer_table_cpu_address:04X}"),
        pointer_table_file_offset: spec.pointer_table_file_offset,
        pointer_table_file_offset_hex: format!("0x{:05X}", spec.pointer_table_file_offset),
        pointer_table_file_end_exclusive: pointer_table_end,
        pointer_table_file_end_exclusive_hex: format!("0x{pointer_table_end:05X}"),
        pointer_table_byte_count,
        pointer_table_sha1: sha1_hex(pointer_table_bytes),
        pointer_count: pointers.len(),
        unique_target_count: indices_by_pointer.len(),
        unique_script_entry_count,
        handler_target_entry_count,
        alias_group_count,
        aliased_entry_count,
        main_record_prefix_summary,
        main_first_line_summary,
        main_linear_segment_summary,
        main_record_storage_summary,
        battle_record_storage_summary,
        data_file_start: spec.data_file_start,
        data_file_start_hex: format!("0x{:05X}", spec.data_file_start),
        directory_binding,
        separate_consumer_binding,
        consumer_binding_status: if spec.directory_group.is_some() {
            "main_dialogue_directory_root_confirmed"
        } else if spec.separate_consumer.is_some() {
            "separate_pointer_loader_confirmed"
        } else {
            "unresolved"
        },
        entries,
    })
}

pub(super) fn validate_separate_consumer(
    source: &[u8],
    consumer: SeparateConsumerSpec,
    expected_table_cpu_address: u16,
    table_id: &str,
) -> Result<SeparateConsumerBindingReport> {
    let loader_file_offset =
        switchable_cpu_to_file_offset(consumer.prg_bank, consumer.loader_cpu_address)?;
    let loader_end = loader_file_offset
        .checked_add(consumer.loader_code.len())
        .context("separate dialogue consumer range overflow")?;
    ensure!(
        source.get(loader_file_offset..loader_end) == Some(consumer.loader_code),
        "{table_id} separate pointer loader changed"
    );
    let table_root_cell_cpu_address = consumer
        .table_root_cell_cpu_address
        .checked_add(u16::from(consumer.table_set_index) * 2)
        .context("separate dialogue table-root cell overflow")?;
    let table_root_cell_file_offset =
        switchable_cpu_to_file_offset(consumer.prg_bank, table_root_cell_cpu_address)?;
    let resolved_pointer_table_cpu_address = u16::from_le_bytes([
        source[table_root_cell_file_offset],
        source[table_root_cell_file_offset + 1],
    ]);
    ensure!(
        resolved_pointer_table_cpu_address == expected_table_cpu_address,
        "{table_id} separate pointer-table root changed: expected {expected_table_cpu_address:04X}, found {resolved_pointer_table_cpu_address:04X}"
    );

    Ok(SeparateConsumerBindingReport {
        prg_bank: consumer.prg_bank,
        prg_bank_hex: format!("0x{:02X}", consumer.prg_bank),
        loader_cpu_address: consumer.loader_cpu_address,
        loader_cpu_address_hex: format!("0x{:04X}", consumer.loader_cpu_address),
        loader_file_offset,
        loader_file_offset_hex: format!("0x{loader_file_offset:05X}"),
        loader_code_sha1: sha1_hex(consumer.loader_code),
        table_set_selector: consumer.table_set_selector,
        table_set_index: consumer.table_set_index,
        entry_index_selector: consumer.entry_index_selector,
        destination_pointer: consumer.destination_pointer,
        table_root_cell_cpu_address,
        table_root_cell_cpu_address_hex: format!("0x{table_root_cell_cpu_address:04X}"),
        table_root_cell_file_offset,
        table_root_cell_file_offset_hex: format!("0x{table_root_cell_file_offset:05X}"),
        resolved_pointer_table_cpu_address,
        resolved_pointer_table_cpu_address_hex: format!(
            "0x{resolved_pointer_table_cpu_address:04X}"
        ),
    })
}

pub(super) fn validate_directory_binding(
    source: &[u8],
    source_prg_bank: u8,
    group: u8,
    expected_table_cpu_address: u16,
    selector_use: Option<DirectorySelectorUseSpec>,
    table_id: &str,
) -> Result<DirectoryBindingReport> {
    ensure!(
        group < 0x10,
        "{table_id} dialogue directory group is outside one selector nibble"
    );
    let directory_entry_cpu_address = DIALOGUE_DIRECTORY_CPU_ADDRESS
        .checked_add(u16::from(group) * 2)
        .context("dialogue directory CPU address overflow")?;
    ensure!(
        directory_entry_cpu_address + 1 < SWITCHABLE_CPU_END_EXCLUSIVE,
        "{table_id} dialogue directory entry is outside the source bank"
    );
    let directory_entry_file_offset =
        switchable_cpu_to_file_offset(source_prg_bank, directory_entry_cpu_address)?;
    let resolved_pointer_table_cpu_address = u16::from_le_bytes([
        source[directory_entry_file_offset],
        source[directory_entry_file_offset + 1],
    ]);
    ensure!(
        resolved_pointer_table_cpu_address == expected_table_cpu_address,
        "{table_id} dialogue directory root changed: expected {expected_table_cpu_address:04X}, found {resolved_pointer_table_cpu_address:04X}"
    );
    let selector = (source_prg_bank << 4) | group;
    let selector_use = selector_use
        .map(|selector_use| {
            ensure!(
                !selector_use.code.is_empty(),
                "{table_id} declares an empty directory selector-use signature"
            );
            let selector_write = [0xA9, selector, 0x8D, 0xF4, 0x77];
            ensure!(
                selector_use
                    .code
                    .windows(selector_write.len())
                    .any(|bytes| bytes == selector_write),
                "{table_id} selector-use signature does not write selector {selector:02X}"
            );
            let file_offset =
                switchable_cpu_to_file_offset(selector_use.prg_bank, selector_use.cpu_address)?;
            let end = file_offset
                .checked_add(selector_use.code.len())
                .context("dialogue directory selector-use range overflow")?;
            ensure!(
                source.get(file_offset..end) == Some(selector_use.code),
                "{table_id} directory selector-use code changed"
            );
            Ok(DirectorySelectorUseReport {
                role: selector_use.role,
                prg_bank: selector_use.prg_bank,
                prg_bank_hex: format!("0x{:02X}", selector_use.prg_bank),
                cpu_address: selector_use.cpu_address,
                cpu_address_hex: format!("0x{:04X}", selector_use.cpu_address),
                file_offset,
                file_offset_hex: format!("0x{file_offset:05X}"),
                code_byte_count: selector_use.code.len(),
                code_sha1: sha1_hex(selector_use.code),
            })
        })
        .transpose()?;

    Ok(DirectoryBindingReport {
        selector,
        selector_hex: format!("0x{selector:02X}"),
        directory_group: group,
        directory_entry_cpu_address,
        directory_entry_cpu_address_hex: format!("0x{directory_entry_cpu_address:04X}"),
        directory_entry_file_offset,
        directory_entry_file_offset_hex: format!("0x{directory_entry_file_offset:05X}"),
        resolved_pointer_table_cpu_address,
        resolved_pointer_table_cpu_address_hex: format!(
            "0x{resolved_pointer_table_cpu_address:04X}"
        ),
        selector_use,
    })
}

pub(super) fn validate_handler_target(
    source: &[u8],
    source_prg_bank: u8,
    handler: &HandlerTargetSpec,
    table_id: &str,
    entry_index: usize,
) -> Result<usize> {
    ensure!(
        !handler.expected_code.is_empty(),
        "{table_id} entry {entry_index} declares an empty fixed-handler signature"
    );
    let file_offset = if handler.cpu_address >= FIXED_CPU_START {
        fixed_cpu_to_file_offset(handler.cpu_address)
    } else {
        switchable_cpu_to_file_offset(source_prg_bank, handler.cpu_address)
    }
    .with_context(|| format!("{table_id} entry {entry_index}"))?;
    let end = file_offset
        .checked_add(handler.expected_code.len())
        .context("fixed-handler signature range overflow")?;
    ensure!(
        source.get(file_offset..end) == Some(handler.expected_code),
        "{table_id} entry {entry_index} code handler {} changed",
        handler.role
    );
    Ok(file_offset)
}

pub(super) fn switchable_bank_file_start(bank: u8) -> usize {
    HEADER_SIZE + usize::from(bank) * PRG_BANK_SIZE
}

pub(crate) fn switchable_file_to_cpu(bank: u8, file_offset: usize) -> Result<u16> {
    let bank_start = switchable_bank_file_start(bank);
    let relative = file_offset
        .checked_sub(bank_start)
        .with_context(|| format!("file offset {file_offset:05X} is before PRG bank {bank:02X}"))?;
    ensure!(
        relative < PRG_BANK_SIZE,
        "file offset {file_offset:05X} is outside PRG bank {bank:02X}"
    );
    Ok(SWITCHABLE_CPU_START + relative as u16)
}

pub(crate) fn switchable_cpu_to_file_offset(bank: u8, cpu_address: u16) -> Result<usize> {
    ensure!(
        (SWITCHABLE_CPU_START..SWITCHABLE_CPU_END_EXCLUSIVE).contains(&cpu_address),
        "CPU address {cpu_address:04X} is outside the switchable PRG window"
    );
    Ok(switchable_bank_file_start(bank) + usize::from(cpu_address - SWITCHABLE_CPU_START))
}

pub(super) fn fixed_cpu_to_file_offset(cpu_address: u16) -> Result<usize> {
    ensure!(
        cpu_address >= FIXED_CPU_START,
        "CPU address {cpu_address:04X} is outside the fixed PRG window"
    );
    Ok(HEADER_SIZE + PRG_SIZE - PRG_BANK_SIZE + usize::from(cpu_address - FIXED_CPU_START))
}
