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
