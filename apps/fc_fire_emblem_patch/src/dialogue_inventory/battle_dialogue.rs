use super::*;

pub(super) fn build_battle_dialogue_state_machine(
    source: &[u8],
) -> Result<BattleDialogueStateMachineReport> {
    let dispatcher_file_offset = switchable_cpu_to_file_offset(
        BATTLE_DIALOGUE_PRG_BANK,
        BATTLE_DIALOGUE_DISPATCHER_CPU_ADDRESS,
    )?;
    let dispatcher_code = [0xAD, 0x37, 0x79, 0x20, 0x4C, 0xC3];
    ensure!(
        source.get(dispatcher_file_offset..dispatcher_file_offset + dispatcher_code.len())
            == Some(dispatcher_code.as_slice()),
        "battle-dialogue state dispatcher changed"
    );

    let handler_table_file_offset = switchable_cpu_to_file_offset(
        BATTLE_DIALOGUE_PRG_BANK,
        BATTLE_DIALOGUE_HANDLER_TABLE_CPU_ADDRESS,
    )?;
    let handler_table_byte_count = BATTLE_DIALOGUE_STATE_HANDLERS.len() * 2;
    let handler_table_bytes = source
        .get(handler_table_file_offset..handler_table_file_offset + handler_table_byte_count)
        .context("battle-dialogue handler table is outside the source")?;
    let actual_handlers = handler_table_bytes
        .chunks_exact(2)
        .map(|bytes| u16::from_le_bytes([bytes[0], bytes[1]]))
        .collect::<Vec<_>>();
    ensure!(
        actual_handlers == BATTLE_DIALOGUE_STATE_HANDLERS,
        "battle-dialogue state handler table changed"
    );
    ensure!(
        sha1_hex(handler_table_bytes) == "b8ed5c6682275d2f8adae45bc0e6375979e48ef2",
        "battle-dialogue state handler table SHA-1 changed"
    );

    let mut indices_by_handler: BTreeMap<u16, Vec<usize>> = BTreeMap::new();
    for (state, handler) in BATTLE_DIALOGUE_STATE_HANDLERS.iter().copied().enumerate() {
        indices_by_handler.entry(handler).or_default().push(state);
    }
    let handlers = BATTLE_DIALOGUE_STATE_HANDLERS
        .iter()
        .copied()
        .enumerate()
        .map(|(state, cpu_address)| {
            let file_offset = if cpu_address >= FIXED_CPU_START {
                fixed_cpu_to_file_offset(cpu_address)?
            } else {
                switchable_cpu_to_file_offset(BATTLE_DIALOGUE_PRG_BANK, cpu_address)?
            };
            Ok(DialogueStateHandlerReport {
                state,
                cpu_address,
                cpu_address_hex: format!("0x{cpu_address:04X}"),
                file_offset,
                file_offset_hex: format!("0x{file_offset:05X}"),
                structural_role: BATTLE_DIALOGUE_STATE_ROLES[state],
                alias_state_indices: indices_by_handler[&cpu_address]
                    .iter()
                    .copied()
                    .filter(|other| *other != state)
                    .collect(),
            })
        })
        .collect::<Result<Vec<_>>>()?;
    let no_op_file_offset = fixed_cpu_to_file_offset(BATTLE_DIALOGUE_STATE_HANDLERS[0])?;
    ensure!(
        source.get(no_op_file_offset) == Some(&0x60),
        "battle-dialogue no-op handler changed"
    );

    let code_regions = BATTLE_DIALOGUE_CODE_REGIONS
        .iter()
        .map(|region| {
            let file_offset =
                switchable_cpu_to_file_offset(BATTLE_DIALOGUE_PRG_BANK, region.cpu_address)?;
            let end = file_offset
                .checked_add(region.byte_count)
                .context("battle-dialogue code range overflow")?;
            let bytes = source.get(file_offset..end).with_context(|| {
                format!("battle-dialogue code {} is outside source", region.role)
            })?;
            ensure!(
                sha1_hex(bytes) == region.expected_sha1,
                "battle-dialogue code {} changed",
                region.role
            );
            let typed_instructions =
                decode_rp2a03_sequence(bytes, region.cpu_address, region.role)?;
            ensure!(
                !typed_instructions.is_empty(),
                "battle-dialogue code {} has no typed instructions",
                region.role
            );
            Ok(BattleDialogueCodeRegionReport {
                role: region.role,
                cpu_address: region.cpu_address,
                cpu_address_hex: format!("0x{:04X}", region.cpu_address),
                file_offset,
                file_offset_hex: format!("0x{file_offset:05X}"),
                byte_count: bytes.len(),
                code_sha1: sha1_hex(bytes),
                typed_instruction_count: typed_instructions.len(),
            })
        })
        .collect::<Result<Vec<_>>>()?;

    Ok(BattleDialogueStateMachineReport {
        prg_bank: BATTLE_DIALOGUE_PRG_BANK,
        prg_bank_hex: format!("0x{BATTLE_DIALOGUE_PRG_BANK:02X}"),
        state_address: BATTLE_DIALOGUE_STATE_ADDRESS,
        state_address_hex: format!("0x{BATTLE_DIALOGUE_STATE_ADDRESS:04X}"),
        dispatcher_cpu_address: BATTLE_DIALOGUE_DISPATCHER_CPU_ADDRESS,
        dispatcher_cpu_address_hex: format!("0x{BATTLE_DIALOGUE_DISPATCHER_CPU_ADDRESS:04X}"),
        handler_table_cpu_address: BATTLE_DIALOGUE_HANDLER_TABLE_CPU_ADDRESS,
        handler_table_cpu_address_hex: format!("0x{BATTLE_DIALOGUE_HANDLER_TABLE_CPU_ADDRESS:04X}"),
        handler_table_sha1: sha1_hex(handler_table_bytes),
        handler_count: handlers.len(),
        handlers,
        fixed_record_header_byte_count: BATTLE_DIALOGUE_FIXED_HEADER_BYTE_COUNT,
        record_end_control: BATTLE_DIALOGUE_END_CONTROL,
        record_end_control_hex: format!("{BATTLE_DIALOGUE_END_CONTROL:02X}"),
        dynamic_value_control: BATTLE_DIALOGUE_DYNAMIC_CONTROL,
        dynamic_value_control_hex: format!("{BATTLE_DIALOGUE_DYNAMIC_CONTROL:02X}"),
        dynamic_selector_operand_byte_count: 1,
        dynamic_selector_max: BATTLE_DIALOGUE_DYNAMIC_SELECTOR_MAX,
        code_regions,
    })
}

pub(super) fn scan_battle_dialogue_record(
    source: &[u8],
    record_file_offset: usize,
    scan_end_exclusive: usize,
    allowed_headers: &[[u8; BATTLE_DIALOGUE_FIXED_HEADER_BYTE_COUNT]],
    table_id: &str,
    entry_index: usize,
) -> Result<BattleDialogueRecordStorageReport> {
    ensure!(
        scan_end_exclusive <= source.len(),
        "{table_id} entry {entry_index} battle scan end is outside source"
    );
    let header_end = record_file_offset
        .checked_add(BATTLE_DIALOGUE_FIXED_HEADER_BYTE_COUNT)
        .context("battle-dialogue header range overflow")?;
    ensure!(
        header_end <= scan_end_exclusive,
        "{table_id} entry {entry_index} has a truncated battle record header"
    );
    let header: [u8; BATTLE_DIALOGUE_FIXED_HEADER_BYTE_COUNT] = source
        .get(record_file_offset..header_end)
        .context("battle-dialogue header is outside source")?
        .try_into()
        .expect("battle-dialogue header slice length is fixed");
    ensure!(
        allowed_headers.contains(&header),
        "{table_id} entry {entry_index} has an unrecognized battle record header"
    );

    let mut cursor = header_end;
    let mut dynamic_selector_values = Vec::new();
    let mut control_count_map = BTreeMap::new();
    let mut literal_file_offsets = Vec::new();
    loop {
        ensure!(
            cursor < scan_end_exclusive,
            "{table_id} entry {entry_index} has no EF battle record terminator"
        );
        let byte = source[cursor];
        if BATTLE_DIALOGUE_CONTROL_CODES.contains(&byte) {
            *control_count_map.entry(byte).or_insert(0) += 1;
        }
        if byte == BATTLE_DIALOGUE_DYNAMIC_CONTROL {
            let selector_offset = cursor
                .checked_add(1)
                .context("battle-dialogue dynamic selector offset overflow")?;
            ensure!(
                selector_offset < scan_end_exclusive,
                "{table_id} entry {entry_index} has a truncated EC selector"
            );
            let selector = source[selector_offset];
            ensure!(
                selector <= BATTLE_DIALOGUE_DYNAMIC_SELECTOR_MAX,
                "{table_id} entry {entry_index} has out-of-range EC selector {selector:02X}"
            );
            dynamic_selector_values.push(selector);
            cursor = selector_offset + 1;
            continue;
        }
        if !BATTLE_DIALOGUE_CONTROL_CODES.contains(&byte) {
            literal_file_offsets.push(cursor);
        }
        cursor += 1;
        if byte == BATTLE_DIALOGUE_END_CONTROL {
            break;
        }
    }

    let storage = source
        .get(record_file_offset..cursor)
        .context("battle-dialogue record storage is outside source")?;
    Ok(BattleDialogueRecordStorageReport {
        file_offset: record_file_offset,
        file_offset_hex: format!("0x{record_file_offset:05X}"),
        end_file_offset_exclusive: cursor,
        end_file_offset_exclusive_hex: format!("0x{cursor:05X}"),
        storage_byte_count: storage.len(),
        storage_sha1: sha1_hex(storage),
        header_hex: header.iter().map(|byte| format!("{byte:02X}")).collect(),
        dynamic_selector_values,
        control_counts: control_usage_reports(control_count_map, &BATTLE_DIALOGUE_CONTROL_CODES),
        literal_file_offsets,
    })
}

pub(super) fn summarize_battle_dialogue_storage(
    source: &[u8],
    entries: &[DialogueEntryReport],
    indices_by_pointer: &BTreeMap<u16, Vec<usize>>,
    data_file_start: usize,
) -> Result<BattleDialogueRecordStorageSummary> {
    let referenced_records = entries
        .iter()
        .filter(|entry| indices_by_pointer[&entry.pointer_cpu_address][0] == entry.index)
        .map(|entry| {
            entry
                .battle_record_storage
                .as_ref()
                .context("canonical battle-dialogue entry has no record-storage range")
        })
        .collect::<Result<Vec<_>>>()?;
    ensure!(
        referenced_records.len() == 28,
        "battle-dialogue pointer-referenced record count changed"
    );
    let referenced_ranges = referenced_records
        .iter()
        .map(|record| MainRecordStorageRange {
            start: record.file_offset,
            end_exclusive: record.end_file_offset_exclusive,
        })
        .collect::<Vec<_>>();
    let referenced_summary = summarize_main_record_storage(&referenced_ranges)?;
    ensure!(
        referenced_summary.consumed_storage_byte_count == 1152
            && referenced_summary.unique_storage_byte_count == 1152
            && referenced_summary.shared_storage_byte_count == 0
            && referenced_summary.overlapping_record_pair_count == 0
            && referenced_summary.max_overlap_depth == 1
            && referenced_summary.max_storage_byte_count == 210,
        "battle-dialogue referenced storage topology changed"
    );

    let physical_data_file_end_exclusive = switchable_cpu_to_file_offset(
        BATTLE_DIALOGUE_PRG_BANK,
        BATTLE_DIALOGUE_DATA_END_EXCLUSIVE_CPU_ADDRESS,
    )?;
    ensure!(
        data_file_start < physical_data_file_end_exclusive,
        "battle-dialogue physical data region is empty"
    );
    let mut physical_records = Vec::new();
    let mut cursor = data_file_start;
    while cursor < physical_data_file_end_exclusive {
        let record = scan_battle_dialogue_record(
            source,
            cursor,
            physical_data_file_end_exclusive,
            &BATTLE_DIALOGUE_PHYSICAL_HEADERS,
            BATTLE_DIALOGUE_TABLE_ID,
            physical_records.len(),
        )?;
        ensure!(
            record.end_file_offset_exclusive > cursor,
            "battle-dialogue physical scanner did not advance"
        );
        cursor = record.end_file_offset_exclusive;
        physical_records.push(record);
    }
    ensure!(
        cursor == physical_data_file_end_exclusive,
        "battle-dialogue physical records do not end at the code boundary"
    );

    let referenced_start_offsets = referenced_records
        .iter()
        .map(|record| record.file_offset)
        .collect::<BTreeSet<_>>();
    let physical_start_offsets = physical_records
        .iter()
        .map(|record| record.file_offset)
        .collect::<BTreeSet<_>>();
    ensure!(
        referenced_start_offsets.is_subset(&physical_start_offsets),
        "battle-dialogue pointer targets do not all begin physical records"
    );
    let unreferenced_records = physical_records
        .iter()
        .filter(|record| !referenced_start_offsets.contains(&record.file_offset))
        .cloned()
        .collect::<Vec<_>>();
    ensure!(
        physical_records.len() == 29 && unreferenced_records.len() == 1,
        "battle-dialogue physical or unreferenced record count changed"
    );

    let mut header_count_map = BTreeMap::new();
    let mut control_count_map = BTreeMap::new();
    for record in &physical_records {
        *header_count_map
            .entry(record.header_hex.clone())
            .or_insert(0) += 1;
        for usage in &record.control_counts {
            *control_count_map.entry(usage.code).or_insert(0) += usage.count;
        }
    }
    let physical_record_storage_byte_count = physical_records
        .iter()
        .map(|record| record.storage_byte_count)
        .sum();
    ensure!(
        physical_record_storage_byte_count == 1168,
        "battle-dialogue physical storage byte count changed"
    );

    Ok(BattleDialogueRecordStorageSummary {
        pointer_referenced_record_count: referenced_summary.unique_record_count,
        unreferenced_record_count: unreferenced_records.len(),
        consumed_storage_byte_count: referenced_summary.consumed_storage_byte_count,
        unique_storage_byte_count: referenced_summary.unique_storage_byte_count,
        shared_storage_byte_count: referenced_summary.shared_storage_byte_count,
        overlapping_record_pair_count: referenced_summary.overlapping_record_pair_count,
        max_overlap_depth: referenced_summary.max_overlap_depth,
        max_storage_byte_count: referenced_summary.max_storage_byte_count,
        physical_record_count: physical_records.len(),
        physical_record_storage_byte_count,
        physical_data_file_end_exclusive,
        physical_data_file_end_exclusive_hex: format!("0x{physical_data_file_end_exclusive:05X}"),
        header_counts: header_count_map
            .into_iter()
            .map(|(header_hex, count)| BattleDialogueHeaderCount { header_hex, count })
            .collect(),
        physical_control_counts: control_usage_reports(
            control_count_map,
            &BATTLE_DIALOGUE_CONTROL_CODES,
        ),
        unreferenced_records,
    })
}
