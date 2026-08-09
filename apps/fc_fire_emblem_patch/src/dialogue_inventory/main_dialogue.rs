use super::*;

pub(super) fn build_main_dialogue_state_machine(
    source: &[u8],
) -> Result<MainDialogueStateMachineReport> {
    let dispatcher_file_offset = switchable_cpu_to_file_offset(
        MAIN_DIALOGUE_PRG_BANK,
        MAIN_DIALOGUE_DISPATCHER_CPU_ADDRESS,
    )?;
    let dispatcher_end = dispatcher_file_offset + MAIN_DIALOGUE_DISPATCHER_CODE.len();
    ensure!(
        source.get(dispatcher_file_offset..dispatcher_end) == Some(MAIN_DIALOGUE_DISPATCHER_CODE),
        "main dialogue state dispatcher changed"
    );

    let handler_table_file_offset = switchable_cpu_to_file_offset(
        MAIN_DIALOGUE_PRG_BANK,
        MAIN_DIALOGUE_HANDLER_TABLE_CPU_ADDRESS,
    )?;
    let handler_table_byte_count = MAIN_DIALOGUE_STATE_HANDLERS.len() * 2;
    let handler_table_end = handler_table_file_offset + handler_table_byte_count;
    let handler_table_bytes = source
        .get(handler_table_file_offset..handler_table_end)
        .context("main dialogue handler table is outside the source")?;
    let actual_handlers = handler_table_bytes
        .chunks_exact(2)
        .map(|bytes| u16::from_le_bytes([bytes[0], bytes[1]]))
        .collect::<Vec<_>>();
    ensure!(
        actual_handlers == MAIN_DIALOGUE_STATE_HANDLERS,
        "main dialogue state handler table changed"
    );

    let mut indices_by_handler: BTreeMap<u16, Vec<usize>> = BTreeMap::new();
    for (state, handler) in MAIN_DIALOGUE_STATE_HANDLERS.iter().copied().enumerate() {
        indices_by_handler.entry(handler).or_default().push(state);
    }
    let handlers = MAIN_DIALOGUE_STATE_HANDLERS
        .iter()
        .copied()
        .enumerate()
        .map(|(state, cpu_address)| {
            let file_offset = if cpu_address >= FIXED_CPU_START {
                fixed_cpu_to_file_offset(cpu_address)?
            } else {
                switchable_cpu_to_file_offset(MAIN_DIALOGUE_PRG_BANK, cpu_address)?
            };
            Ok(DialogueStateHandlerReport {
                state,
                cpu_address,
                cpu_address_hex: format!("0x{cpu_address:04X}"),
                file_offset,
                file_offset_hex: format!("0x{file_offset:05X}"),
                structural_role: MAIN_DIALOGUE_HANDLER_ROLES[state],
                alias_state_indices: indices_by_handler[&cpu_address]
                    .iter()
                    .copied()
                    .filter(|other| *other != state)
                    .collect(),
            })
        })
        .collect::<Result<Vec<_>>>()?;
    let no_op_file_offset = fixed_cpu_to_file_offset(MAIN_DIALOGUE_STATE_HANDLERS[0])?;
    ensure!(
        source.get(no_op_file_offset) == Some(&0x60),
        "main dialogue no-op state handler changed"
    );

    let code_regions = MAIN_DIALOGUE_STATE_CODE_REGIONS
        .iter()
        .map(|region| {
            let file_offset =
                switchable_cpu_to_file_offset(MAIN_DIALOGUE_PRG_BANK, region.cpu_address)?;
            let end = file_offset
                .checked_add(region.bytes.len())
                .context("main dialogue state code range overflow")?;
            ensure!(
                source.get(file_offset..end) == Some(region.bytes),
                "main dialogue state code {} changed",
                region.role
            );
            Ok(CodeRegionReport {
                role: region.role,
                cpu_address: region.cpu_address,
                cpu_address_hex: format!("0x{:04X}", region.cpu_address),
                file_offset,
                file_offset_hex: format!("0x{file_offset:05X}"),
                byte_count: region.bytes.len(),
                code_sha1: sha1_hex(region.bytes),
            })
        })
        .collect::<Result<Vec<_>>>()?;
    let pointer_resolver_file_offset =
        fixed_cpu_to_file_offset(MAIN_DIALOGUE_POINTER_RESOLVER_CPU_ADDRESS)?;
    let pointer_resolver_end = pointer_resolver_file_offset
        .checked_add(MAIN_DIALOGUE_POINTER_RESOLVER_CODE.len())
        .context("main dialogue pointer resolver range overflow")?;
    ensure!(
        source.get(pointer_resolver_file_offset..pointer_resolver_end)
            == Some(MAIN_DIALOGUE_POINTER_RESOLVER_CODE),
        "main dialogue pointer resolver changed"
    );
    let caller_flag_load_candidates = CALLER_HANDOFF_OBSERVER_SPECS
        .iter()
        .map(|observer| {
            let file_offset =
                switchable_cpu_to_file_offset(observer.prg_bank, observer.cpu_address)?;
            let end = file_offset
                .checked_add(CALLER_HANDOFF_FLAG_LOAD.len())
                .context("caller handoff flag load range overflow")?;
            ensure!(
                source.get(file_offset..end) == Some(&CALLER_HANDOFF_FLAG_LOAD),
                "caller handoff flag load changed at bank {:02X}:{:04X}",
                observer.prg_bank,
                observer.cpu_address
            );
            let direct_dispatch_bindings =
                build_caller_handoff_dispatch_bindings(source, *observer)?;
            Ok(CallerHandoffObserverReport {
                prg_bank: observer.prg_bank,
                prg_bank_hex: format!("0x{:02X}", observer.prg_bank),
                cpu_address: observer.cpu_address,
                cpu_address_hex: format!("0x{:04X}", observer.cpu_address),
                file_offset,
                file_offset_hex: format!("0x{file_offset:05X}"),
                instruction: "LDA $7809",
                handler_cpu_address: observer.handler_cpu_address,
                handler_cpu_address_hex: format!("0x{:04X}", observer.handler_cpu_address),
                direct_dispatch_bindings,
            })
        })
        .collect::<Result<Vec<_>>>()?;
    let actual_caller_flag_load_offsets = source[HEADER_SIZE..HEADER_SIZE + PRG_SIZE]
        .windows(CALLER_HANDOFF_FLAG_LOAD.len())
        .enumerate()
        .filter_map(|(relative_offset, bytes)| {
            (bytes == CALLER_HANDOFF_FLAG_LOAD).then_some(HEADER_SIZE + relative_offset)
        })
        .collect::<Vec<_>>();
    let expected_caller_flag_load_offsets = caller_flag_load_candidates
        .iter()
        .map(|candidate| candidate.file_offset)
        .collect::<Vec<_>>();
    ensure!(
        actual_caller_flag_load_offsets == expected_caller_flag_load_offsets,
        "caller handoff flag load candidate set changed"
    );
    let direct_dispatch_bound_observer_count = caller_flag_load_candidates
        .iter()
        .filter(|candidate| !candidate.direct_dispatch_bindings.is_empty())
        .count();
    let direct_dispatch_unbound_observer_count =
        caller_flag_load_candidates.len() - direct_dispatch_bound_observer_count;
    let confirmed_direct_dispatch_binding_count = caller_flag_load_candidates
        .iter()
        .map(|candidate| candidate.direct_dispatch_bindings.len())
        .sum();
    let confirmed_direct_handler_slot_count = caller_flag_load_candidates
        .iter()
        .flat_map(|candidate| &candidate.direct_dispatch_bindings)
        .map(|binding| binding.handler_state_indices.len())
        .sum();
    ensure!(
        confirmed_direct_dispatch_binding_count == CALLER_HANDOFF_DISPATCH_SPECS.len(),
        "caller handoff dispatch specification is not bound to exactly one observer"
    );

    Ok(MainDialogueStateMachineReport {
        prg_bank: MAIN_DIALOGUE_PRG_BANK,
        prg_bank_hex: format!("0x{MAIN_DIALOGUE_PRG_BANK:02X}"),
        state_address: MAIN_DIALOGUE_STATE_ADDRESS,
        state_address_hex: format!("0x{MAIN_DIALOGUE_STATE_ADDRESS:04X}"),
        dispatcher_cpu_address: MAIN_DIALOGUE_DISPATCHER_CPU_ADDRESS,
        dispatcher_cpu_address_hex: format!("0x{MAIN_DIALOGUE_DISPATCHER_CPU_ADDRESS:04X}"),
        dispatcher_file_offset,
        dispatcher_file_offset_hex: format!("0x{dispatcher_file_offset:05X}"),
        dispatcher_code_sha1: sha1_hex(MAIN_DIALOGUE_DISPATCHER_CODE),
        dispatch_helper_cpu_address: 0xC34C,
        dispatch_helper_cpu_address_hex: "0xC34C".to_owned(),
        handler_table_cpu_address: MAIN_DIALOGUE_HANDLER_TABLE_CPU_ADDRESS,
        handler_table_cpu_address_hex: format!("0x{MAIN_DIALOGUE_HANDLER_TABLE_CPU_ADDRESS:04X}"),
        handler_table_file_offset,
        handler_table_file_offset_hex: format!("0x{handler_table_file_offset:05X}"),
        handler_table_sha1: sha1_hex(handler_table_bytes),
        handler_count: handlers.len(),
        handlers,
        record_prefix_contract: MainRecordPrefixContract {
            optional_e5_prefix_code: OPTIONAL_E5_PREFIX_CODE,
            optional_e5_prefix_code_hex: format!("{OPTIONAL_E5_PREFIX_CODE:02X}"),
            optional_e5_prefix_byte_count: OPTIONAL_PREFIX_BYTE_COUNT,
            fixed_record_header_byte_count: FIXED_RECORD_HEADER_BYTE_COUNT,
            optional_e8_prefix_code: OPTIONAL_E8_PREFIX_CODE,
            optional_e8_prefix_code_hex: format!("{OPTIONAL_E8_PREFIX_CODE:02X}"),
            optional_e8_prefix_byte_count: OPTIONAL_PREFIX_BYTE_COUNT,
        },
        caller_handoff_contract: CallerHandoffContract {
            control_code: 0xE7,
            control_code_hex: "E7".to_owned(),
            decoder_flag_address: 0x7808,
            decoder_flag_address_hex: "0x7808".to_owned(),
            caller_flag_address: 0x7809,
            caller_flag_address_hex: "0x7809".to_owned(),
            handoff_state: 17,
            resume_state: 9,
            pointer_resolver_cpu_address: MAIN_DIALOGUE_POINTER_RESOLVER_CPU_ADDRESS,
            pointer_resolver_cpu_address_hex: format!(
                "0x{MAIN_DIALOGUE_POINTER_RESOLVER_CPU_ADDRESS:04X}"
            ),
            pointer_resolver_file_offset,
            pointer_resolver_file_offset_hex: format!("0x{pointer_resolver_file_offset:05X}"),
            pointer_resolver_code_sha1: sha1_hex(MAIN_DIALOGUE_POINTER_RESOLVER_CODE),
            caller_flag_load_candidate_count: caller_flag_load_candidates.len(),
            direct_dispatch_bound_observer_count,
            direct_dispatch_unbound_observer_count,
            confirmed_direct_dispatch_binding_count,
            confirmed_direct_handler_slot_count,
            caller_flag_load_candidates,
        },
        code_regions,
    })
}

pub(super) fn build_caller_handoff_dispatch_bindings(
    source: &[u8],
    observer: CallerHandoffObserverSpec,
) -> Result<Vec<CallerHandoffDispatchBindingReport>> {
    ensure!(
        observer.cpu_address >= observer.handler_cpu_address,
        "caller handoff observer precedes its declared handler"
    );

    CALLER_HANDOFF_DISPATCH_SPECS
        .iter()
        .filter(|spec| {
            spec.prg_bank == observer.prg_bank
                && spec.handler_cpu_address == observer.handler_cpu_address
        })
        .map(|spec| {
            ensure!(
                !spec.handlers.is_empty(),
                "caller handoff dispatcher declares no handlers"
            );
            ensure!(
                !spec.handler_state_indices.is_empty(),
                "caller handoff dispatcher declares no observer handler states"
            );
            ensure!(
                spec.handler_table_cpu_address
                    == spec
                        .dispatcher_cpu_address
                        .checked_add(6)
                        .context("caller handoff dispatcher address overflow")?,
                "caller handoff handler table does not immediately follow its dispatcher"
            );
            let [state_low, state_high] = spec.state_address.to_le_bytes();
            let dispatcher_code = [0xAD, state_low, state_high, 0x20, 0x4C, 0xC3];
            let dispatcher_file_offset =
                switchable_cpu_to_file_offset(spec.prg_bank, spec.dispatcher_cpu_address)?;
            let dispatcher_end = dispatcher_file_offset
                .checked_add(dispatcher_code.len())
                .context("caller handoff dispatcher range overflow")?;
            ensure!(
                source.get(dispatcher_file_offset..dispatcher_end) == Some(&dispatcher_code),
                "caller handoff dispatcher changed at bank {:02X}:{:04X}",
                spec.prg_bank,
                spec.dispatcher_cpu_address
            );

            let handler_table_file_offset =
                switchable_cpu_to_file_offset(spec.prg_bank, spec.handler_table_cpu_address)?;
            let handler_table_byte_count = spec
                .handlers
                .len()
                .checked_mul(2)
                .context("caller handoff handler table length overflow")?;
            let handler_table_end = handler_table_file_offset
                .checked_add(handler_table_byte_count)
                .context("caller handoff handler table range overflow")?;
            ensure!(
                handler_table_end <= switchable_bank_file_start(spec.prg_bank) + PRG_BANK_SIZE,
                "caller handoff handler table crosses its switchable bank"
            );
            let handler_table_bytes = source
                .get(handler_table_file_offset..handler_table_end)
                .context("caller handoff handler table is outside the source")?;
            let actual_handlers = handler_table_bytes
                .chunks_exact(2)
                .map(|bytes| u16::from_le_bytes([bytes[0], bytes[1]]))
                .collect::<Vec<_>>();
            ensure!(
                actual_handlers == spec.handlers,
                "caller handoff handler table changed at bank {:02X}:{:04X}",
                spec.prg_bank,
                spec.handler_table_cpu_address
            );
            let actual_handler_state_indices = actual_handlers
                .iter()
                .enumerate()
                .filter_map(|(state, handler)| {
                    (*handler == spec.handler_cpu_address).then_some(state)
                })
                .collect::<Vec<_>>();
            ensure!(
                actual_handler_state_indices == spec.handler_state_indices,
                "caller handoff handler-state binding changed at bank {:02X}:{:04X}",
                spec.prg_bank,
                spec.handler_table_cpu_address
            );

            Ok(CallerHandoffDispatchBindingReport {
                state_address: spec.state_address,
                state_address_hex: format!("0x{:04X}", spec.state_address),
                dispatcher_cpu_address: spec.dispatcher_cpu_address,
                dispatcher_cpu_address_hex: format!("0x{:04X}", spec.dispatcher_cpu_address),
                dispatcher_file_offset,
                dispatcher_file_offset_hex: format!("0x{dispatcher_file_offset:05X}"),
                handler_table_cpu_address: spec.handler_table_cpu_address,
                handler_table_cpu_address_hex: format!("0x{:04X}", spec.handler_table_cpu_address),
                handler_table_file_offset,
                handler_table_file_offset_hex: format!("0x{handler_table_file_offset:05X}"),
                handler_table_sha1: sha1_hex(handler_table_bytes),
                handler_count: actual_handlers.len(),
                handler_cpu_address: spec.handler_cpu_address,
                handler_cpu_address_hex: format!("0x{:04X}", spec.handler_cpu_address),
                handler_state_indices: actual_handler_state_indices,
            })
        })
        .collect()
}

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
