use super::*;

/// 줄을 여는 제어 코드다. 창 기술자가 있는 레코드는 기술자 뒤가 이런 코드로 이어진다.
const LINE_START_CONTROL_CODE: u8 = 0xE9;

const SYNTHETIC_BANK: u8 = 0x02;
const SYNTHETIC_TABLE_OFFSET: usize = HEADER_SIZE + 2 * PRG_BANK_SIZE + 0x0100;
const SYNTHETIC_DATA_START: usize = HEADER_SIZE + 2 * PRG_BANK_SIZE + 0x0200;
const SYNTHETIC_TRANSITION_BANK: u8 = 0x03;
const SYNTHETIC_TRANSITION_TARGET_ENTRY_INDEX: usize = 1;
const SYNTHETIC_TRANSITION_POINTER_COUNT: usize = SYNTHETIC_TRANSITION_TARGET_ENTRY_INDEX + 1;
const SYNTHETIC_TRANSITION_TABLE_OFFSET: usize =
    HEADER_SIZE + SYNTHETIC_TRANSITION_BANK as usize * PRG_BANK_SIZE + 0x0100;
const SYNTHETIC_TRANSITION_DATA_START: usize =
    SYNTHETIC_TRANSITION_TABLE_OFFSET + SYNTHETIC_TRANSITION_POINTER_COUNT * 2;
const SYNTHETIC_TRANSITION_LEADING_TEXT: [u8; FIXED_RECORD_HEADER_BYTE_COUNT] =
    [0x00, 0x01, 0x02, 0x03];
const TEST_FIXED_HANDLER: HandlerTargetSpec = HandlerTargetSpec {
    cpu_address: 0xC73D,
    role: "empty_dialogue_handler",
    expected_code: &[0x60],
};

fn synthetic_source() -> Vec<u8> {
    vec![0; HEADER_SIZE + PRG_SIZE]
}

fn synthetic_spec(pointer_count: usize) -> DialogueTableSpec {
    DialogueTableSpec {
        id: "synthetic-dialogue",
        role: "synthetic_dialogue",
        source_prg_bank: SYNTHETIC_BANK,
        pointer_table_file_offset: SYNTHETIC_TABLE_OFFSET,
        pointer_count,
        data_file_start: SYNTHETIC_DATA_START,
        directory_group: None,
        directory_selector_use: None,
        separate_consumer: None,
        allowed_handler_targets: NO_HANDLER_TARGETS,
    }
}

fn synthetic_transition_only_spec() -> DialogueTableSpec {
    DialogueTableSpec {
        id: "epilogue-dialogue",
        role: "synthetic_transition_dialogue",
        source_prg_bank: SYNTHETIC_TRANSITION_BANK,
        pointer_table_file_offset: SYNTHETIC_TRANSITION_TABLE_OFFSET,
        pointer_count: SYNTHETIC_TRANSITION_POINTER_COUNT,
        data_file_start: SYNTHETIC_TRANSITION_DATA_START,
        directory_group: Some(0),
        directory_selector_use: None,
        separate_consumer: None,
        allowed_handler_targets: NO_HANDLER_TARGETS,
    }
}

fn synthetic_transition_only_source() -> Vec<u8> {
    let mut source = synthetic_source();
    let pointer_table_cpu_address =
        switchable_file_to_cpu(SYNTHETIC_TRANSITION_BANK, SYNTHETIC_TRANSITION_TABLE_OFFSET)
            .unwrap();
    let directory_entry_file_offset =
        switchable_cpu_to_file_offset(SYNTHETIC_TRANSITION_BANK, DIALOGUE_DIRECTORY_CPU_ADDRESS)
            .unwrap();
    source[directory_entry_file_offset..directory_entry_file_offset + 2]
        .copy_from_slice(&pointer_table_cpu_address.to_le_bytes());

    let transition_source_cpu_address =
        switchable_file_to_cpu(SYNTHETIC_TRANSITION_BANK, SYNTHETIC_TRANSITION_DATA_START).unwrap();
    let transition_target_file_offset = SYNTHETIC_TRANSITION_DATA_START + 3;
    let transition_target_cpu_address =
        switchable_file_to_cpu(SYNTHETIC_TRANSITION_BANK, transition_target_file_offset).unwrap();
    for index in 0..SYNTHETIC_TRANSITION_TARGET_ENTRY_INDEX {
        let pointer_file_offset = SYNTHETIC_TRANSITION_TABLE_OFFSET + index * 2;
        source[pointer_file_offset..pointer_file_offset + 2]
            .copy_from_slice(&transition_source_cpu_address.to_le_bytes());
    }
    let target_pointer_file_offset =
        SYNTHETIC_TRANSITION_TABLE_OFFSET + SYNTHETIC_TRANSITION_TARGET_ENTRY_INDEX * 2;
    source[target_pointer_file_offset..target_pointer_file_offset + 2]
        .copy_from_slice(&transition_target_cpu_address.to_le_bytes());

    source[SYNTHETIC_TRANSITION_DATA_START..SYNTHETIC_TRANSITION_DATA_START + 3]
        .copy_from_slice(&[0xE4, 0x40, SYNTHETIC_TRANSITION_TARGET_ENTRY_INDEX as u8]);
    source[transition_target_file_offset
        ..transition_target_file_offset + SYNTHETIC_TRANSITION_LEADING_TEXT.len()]
        .copy_from_slice(&SYNTHETIC_TRANSITION_LEADING_TEXT);
    source[transition_target_file_offset + SYNTHETIC_TRANSITION_LEADING_TEXT.len()] = 0xEF;
    source
}

fn inspect_synthetic_transition_only_records(
    source: &[u8],
    transition_only_entries: &BTreeSet<(&'static str, usize)>,
) -> Result<Vec<MainDialogueStorageRecord>> {
    let table = extract_dialogue_table(source, &synthetic_transition_only_spec())?;
    let graph = build_main_dialogue_graph(std::slice::from_ref(&table))?;
    main_dialogue_translation_view::build_main_dialogue_storage_records_with_transition_only_entries(
        source,
        &[table],
        &graph,
        transition_only_entries,
    )
}

fn write_pointer(source: &mut [u8], index: usize, pointer: u16) {
    let offset = SYNTHETIC_TABLE_OFFSET + index * 2;
    source[offset..offset + 2].copy_from_slice(&pointer.to_le_bytes());
}

fn write_main_dialogue_state_machine(source: &mut [u8]) {
    let dispatcher_file_offset =
        switchable_cpu_to_file_offset(MAIN_DIALOGUE_PRG_BANK, MAIN_DIALOGUE_DISPATCHER_CPU_ADDRESS)
            .unwrap();
    source[dispatcher_file_offset..dispatcher_file_offset + MAIN_DIALOGUE_DISPATCHER_CODE.len()]
        .copy_from_slice(MAIN_DIALOGUE_DISPATCHER_CODE);
    let handler_table_file_offset = switchable_cpu_to_file_offset(
        MAIN_DIALOGUE_PRG_BANK,
        MAIN_DIALOGUE_HANDLER_TABLE_CPU_ADDRESS,
    )
    .unwrap();
    for (state, handler) in MAIN_DIALOGUE_STATE_HANDLERS.iter().enumerate() {
        let offset = handler_table_file_offset + state * 2;
        source[offset..offset + 2].copy_from_slice(&handler.to_le_bytes());
    }
    for region in &MAIN_DIALOGUE_STATE_CODE_REGIONS {
        let file_offset =
            switchable_cpu_to_file_offset(MAIN_DIALOGUE_PRG_BANK, region.cpu_address).unwrap();
        source[file_offset..file_offset + region.bytes.len()].copy_from_slice(region.bytes);
    }
    let pointer_resolver_file_offset =
        fixed_cpu_to_file_offset(MAIN_DIALOGUE_POINTER_RESOLVER_CPU_ADDRESS).unwrap();
    source[pointer_resolver_file_offset
        ..pointer_resolver_file_offset + MAIN_DIALOGUE_POINTER_RESOLVER_CODE.len()]
        .copy_from_slice(MAIN_DIALOGUE_POINTER_RESOLVER_CODE);
    for observer in CALLER_HANDOFF_OBSERVER_SPECS {
        let file_offset =
            switchable_cpu_to_file_offset(observer.prg_bank, observer.cpu_address).unwrap();
        source[file_offset..file_offset + CALLER_HANDOFF_FLAG_LOAD.len()]
            .copy_from_slice(&CALLER_HANDOFF_FLAG_LOAD);
    }
    for dispatch in CALLER_HANDOFF_DISPATCH_SPECS {
        let dispatcher_file_offset =
            switchable_cpu_to_file_offset(dispatch.prg_bank, dispatch.dispatcher_cpu_address)
                .unwrap();
        let [state_low, state_high] = dispatch.state_address.to_le_bytes();
        source[dispatcher_file_offset..dispatcher_file_offset + 6]
            .copy_from_slice(&[0xAD, state_low, state_high, 0x20, 0x4C, 0xC3]);
        let handler_table_file_offset =
            switchable_cpu_to_file_offset(dispatch.prg_bank, dispatch.handler_table_cpu_address)
                .unwrap();
        for (state, handler) in dispatch.handlers.iter().enumerate() {
            let offset = handler_table_file_offset + state * 2;
            source[offset..offset + 2].copy_from_slice(&handler.to_le_bytes());
        }
    }
    let no_op_file_offset = fixed_cpu_to_file_offset(MAIN_DIALOGUE_STATE_HANDLERS[0]).unwrap();
    source[no_op_file_offset] = 0x60;
}

#[test]
fn reports_aliases_without_reading_dialogue_bytes() {
    let mut source = synthetic_source();
    let spec = synthetic_spec(3);
    write_pointer(&mut source, 0, 0x8200);
    write_pointer(&mut source, 1, 0x8200);
    write_pointer(&mut source, 2, 0x8210);

    let report = extract_dialogue_table(&source, &spec).unwrap();

    assert_eq!(report.pointer_count, 3);
    assert_eq!(report.unique_target_count, 2);
    assert_eq!(report.alias_group_count, 1);
    assert_eq!(report.aliased_entry_count, 2);
    assert_eq!(report.entries[0].alias_entry_indices, vec![1]);
    assert_eq!(report.entries[1].alias_entry_indices, vec![0]);
    assert_eq!(report.entries[2].alias_entry_indices, Vec::<usize>::new());
}

#[test]
fn validates_main_dialogue_state_dispatch_prefix_and_handoff() {
    let mut source = synthetic_source();
    write_main_dialogue_state_machine(&mut source);

    let report = build_main_dialogue_state_machine(&source).unwrap();

    assert_eq!(report.handler_count, 18);
    assert_eq!(report.handlers[3].alias_state_indices, vec![7]);
    assert_eq!(report.handlers[7].alias_state_indices, vec![3]);
    assert_eq!(
        report.record_prefix_contract.optional_e5_prefix_byte_count,
        6
    );
    assert_eq!(
        report.record_prefix_contract.fixed_record_header_byte_count,
        4
    );
    assert_eq!(
        report.record_prefix_contract.optional_e8_prefix_byte_count,
        6
    );
    assert_eq!(report.caller_handoff_contract.control_code, 0xE7);
    assert_eq!(report.caller_handoff_contract.handoff_state, 17);
    assert_eq!(report.caller_handoff_contract.resume_state, 9);
    assert_eq!(
        report
            .caller_handoff_contract
            .caller_flag_load_candidate_count,
        5
    );
    assert_eq!(
        report
            .caller_handoff_contract
            .direct_dispatch_bound_observer_count,
        4
    );
    assert_eq!(
        report
            .caller_handoff_contract
            .direct_dispatch_unbound_observer_count,
        1
    );
    assert_eq!(
        report
            .caller_handoff_contract
            .confirmed_direct_dispatch_binding_count,
        11
    );
    assert_eq!(
        report
            .caller_handoff_contract
            .confirmed_direct_handler_slot_count,
        22
    );
    assert!(
        report.caller_handoff_contract.caller_flag_load_candidates[1]
            .direct_dispatch_bindings
            .is_empty()
    );

    let e5_file_offset = switchable_cpu_to_file_offset(MAIN_DIALOGUE_PRG_BANK, 0x80A2).unwrap();
    source[e5_file_offset + 9] ^= 0x01;
    let error = build_main_dialogue_state_machine(&source)
        .unwrap_err()
        .to_string();
    assert!(error.contains("state code inspect_and_consume_optional_E5_prefix changed"));
}

#[test]
fn binds_all_main_dialogue_auxiliary_composites_as_one_family() {
    let mut source = synthetic_source();
    write_main_dialogue_state_machine(&mut source);

    let routes = bind_main_dialogue_composite_appenders_in_source(&source).unwrap();

    assert_eq!(
        routes
            .iter()
            .map(|route| route.composite_state)
            .collect::<Vec<_>>(),
        MAIN_DIALOGUE_COMPOSITE_APPENDER_STATES
    );
    assert_eq!(
        routes
            .iter()
            .map(|route| {
                (
                    route.dialogue_state,
                    route.load_address,
                    route.transfer_address,
                )
            })
            .collect::<Vec<_>>(),
        [
            (0x02, 0x80DD, 0x80DF),
            (0x04, 0x8110, 0x8112),
            (0x06, 0x8168, 0x816A),
        ]
    );
    assert!(routes.iter().all(|route| route.prg_bank == 0x0A));
}

#[test]
fn rejects_a_changed_caller_handoff_observer_set() {
    let mut source = synthetic_source();
    write_main_dialogue_state_machine(&mut source);
    let observer = CALLER_HANDOFF_OBSERVER_SPECS[0];
    let observer_file_offset =
        switchable_cpu_to_file_offset(observer.prg_bank, observer.cpu_address).unwrap();
    source[observer_file_offset] = 0xEA;

    let error = build_main_dialogue_state_machine(&source)
        .unwrap_err()
        .to_string();

    assert!(error.contains("caller handoff flag load changed"));
}

#[test]
fn rejects_a_changed_caller_handoff_dispatch_table() {
    let mut source = synthetic_source();
    write_main_dialogue_state_machine(&mut source);
    let dispatch = CALLER_HANDOFF_DISPATCH_SPECS[0];
    let handler_table_file_offset =
        switchable_cpu_to_file_offset(dispatch.prg_bank, dispatch.handler_table_cpu_address)
            .unwrap();
    source[handler_table_file_offset] ^= 0x01;

    let error = build_main_dialogue_state_machine(&source)
        .unwrap_err()
        .to_string();

    assert!(error.contains("caller handoff handler table changed"));
}

#[test]
fn locates_the_first_line_after_only_declared_record_prefixes() {
    let mut source = synthetic_source();
    let bank_end = switchable_bank_file_start(SYNTHETIC_BANK) + PRG_BANK_SIZE;
    let entry_file_offset = SYNTHETIC_DATA_START;
    source[entry_file_offset] = OPTIONAL_E5_PREFIX_CODE;
    source[entry_file_offset + OPTIONAL_PREFIX_BYTE_COUNT + FIXED_RECORD_HEADER_BYTE_COUNT] =
        OPTIONAL_E8_PREFIX_CODE;
    // 창 기술자가 있는 레코드는 기술자 뒤가 제어 코드다. 그 조건으로 기술자 유무를
    // 가르므로 기술자를 두는 픽스처에는 반드시 함께 넣는다.
    let plain_entry_file_offset = entry_file_offset + 0x40;
    source[plain_entry_file_offset + FIXED_RECORD_HEADER_BYTE_COUNT] = LINE_START_CONTROL_CODE;

    let full = inspect_main_record_prefix(
        &source,
        entry_file_offset,
        bank_end,
        "synthetic-dialogue",
        0,
    )
    .unwrap();
    assert!(full.e5_prefix_present);
    assert!(full.e8_prefix_present);
    assert_eq!(full.total_prefix_byte_count, 16);
    assert_eq!(full.first_line_file_offset, entry_file_offset + 16);

    let plain = inspect_main_record_prefix(
        &source,
        plain_entry_file_offset,
        bank_end,
        "synthetic-dialogue",
        1,
    )
    .unwrap();
    assert!(!plain.e5_prefix_present);
    assert!(!plain.e8_prefix_present);
    assert_eq!(plain.total_prefix_byte_count, 4);
    assert_eq!(plain.first_line_file_offset, plain_entry_file_offset + 4);

    // 기술자 없이 표시 내용부터 시작하는 레코드다. `E7` 인계에서 상태 9로 복귀하는
    // 경로가 여기에 해당하며, 네 바이트를 기술자로 먹으면 화면 글자가 잘린다.
    let resumed_entry_file_offset = entry_file_offset + 0x80;
    source[resumed_entry_file_offset..resumed_entry_file_offset + 5]
        .copy_from_slice(&[0x0A, 0x07, 0x13, 0x06, 0x05]);
    let resumed = inspect_main_record_prefix(
        &source,
        resumed_entry_file_offset,
        bank_end,
        "synthetic-dialogue",
        2,
    )
    .unwrap();
    assert_eq!(resumed.total_prefix_byte_count, 0);
    assert_eq!(resumed.first_line_file_offset, resumed_entry_file_offset);

    // 선두가 제어 코드면 기술자 자리가 아니다.
    let control_led_entry_file_offset = entry_file_offset + 0xC0;
    source[control_led_entry_file_offset] = 0xED;
    let control_led = inspect_main_record_prefix(
        &source,
        control_led_entry_file_offset,
        bank_end,
        "synthetic-dialogue",
        3,
    )
    .unwrap();
    assert_eq!(control_led.total_prefix_byte_count, 0);
}

#[test]
fn locates_transition_targets_without_a_direct_record_header() {
    let mut source = synthetic_source();
    let bank_end = switchable_bank_file_start(SYNTHETIC_BANK) + PRG_BANK_SIZE;
    let e8_target_file_offset = SYNTHETIC_DATA_START;
    source[e8_target_file_offset] = OPTIONAL_E8_PREFIX_CODE;

    assert_eq!(
        inspect_main_transition_prefix_byte_count(
            &source,
            e8_target_file_offset,
            bank_end,
            "synthetic-dialogue",
            0,
        )
        .unwrap(),
        OPTIONAL_PREFIX_BYTE_COUNT
    );

    let plain_target_file_offset = e8_target_file_offset + 0x40;
    source[plain_target_file_offset] = 0xED;
    assert_eq!(
        inspect_main_transition_prefix_byte_count(
            &source,
            plain_target_file_offset,
            bank_end,
            "synthetic-dialogue",
            1,
        )
        .unwrap(),
        0
    );
}

/// 직접 엔트리로 해석하면 선두 네 글자를 헤더로 오인하지만, 전이 전용으로 분류된
/// 레코드는 전이 진입 해석을 써야 한다.
#[test]
fn transition_only_policy_keeps_the_leading_text() {
    let source = synthetic_transition_only_source();
    let transition_only_entries =
        BTreeSet::from([("epilogue-dialogue", SYNTHETIC_TRANSITION_TARGET_ENTRY_INDEX)]);
    let record = inspect_synthetic_transition_only_records(&source, &transition_only_entries)
        .unwrap()
        .into_iter()
        .find(|record| {
            record.table_id == "epilogue-dialogue"
                && record.canonical_entry_index == SYNTHETIC_TRANSITION_TARGET_ENTRY_INDEX
        })
        .expect("synthetic transition-only entry");

    assert_eq!(record.prefix_byte_count, 0);
    assert!(
        (record.file_offset..record.file_offset + SYNTHETIC_TRANSITION_LEADING_TEXT.len())
            .all(|offset| record.literal_file_offsets.contains(&offset)),
        "the leading text must stay in the translatable literal set"
    );
}

/// 직접 도달 가능한 레코드까지 전이 해석으로 허용하면 실제 기술자 네 바이트를
/// 글자로 오인한다. 전이 전용 정책이 그 레코드를 승인하지 않으면 닫혀야 한다.
#[test]
fn a_directly_reachable_entry_cannot_use_transition_only_prefix_handling() {
    let source = synthetic_transition_only_source();
    let error = inspect_synthetic_transition_only_records(&source, &BTreeSet::new())
        .unwrap_err()
        .to_string();

    assert!(
        error.contains("is not admitted by the transition-only entry policy"),
        "{error}"
    );
}

#[test]
fn scans_a_first_line_without_emitting_its_source_bytes() {
    let mut source = synthetic_source();
    let line_file_offset = SYNTHETIC_DATA_START;
    source[line_file_offset..line_file_offset + 6]
        .copy_from_slice(&[0x60, 0xE9, 0x03, 0xE4, 0x71, 0x02]);
    let bank_end = switchable_bank_file_start(SYNTHETIC_BANK) + PRG_BANK_SIZE;

    let line =
        scan_main_line(&source, line_file_offset, bank_end, "synthetic-dialogue", 0).unwrap();

    assert_eq!(line.storage_byte_count, 6);
    assert_eq!(line.current_pointer_advance_bytes, 4);
    assert_eq!(line.literal_byte_count, 1);
    assert_eq!(line.japanese_literal_byte_count, 0);
    assert_eq!(line.non_japanese_literal_byte_count, 1);
    assert_eq!(line.literal_file_offsets, vec![line_file_offset]);
    assert_eq!(line.protected_original_alphanumeric_literal_byte_count, 1);
    assert_eq!(line.control_token_count, 2);
    assert_eq!(line.inline_operand_byte_count, 1);
    assert_eq!(line.transition_target_byte_count, 2);
    assert_eq!(line.line_end_control, 0xE4);
    let transition = line.transition_target.unwrap();
    assert_eq!(transition.target_table_id, "recruitment-dialogue");
    assert_eq!(transition.target_entry_index, 2);
}

#[test]
fn rejects_an_out_of_range_first_line_operand() {
    let mut source = synthetic_source();
    let line_file_offset = SYNTHETIC_DATA_START;
    source[line_file_offset..line_file_offset + 3].copy_from_slice(&[0xEC, 0x04, 0xED]);
    let bank_end = switchable_bank_file_start(SYNTHETIC_BANK) + PRG_BANK_SIZE;

    let error = scan_main_line(&source, line_file_offset, bank_end, "synthetic-dialogue", 0)
        .unwrap_err()
        .to_string();

    assert!(error.contains("EC operand is outside 0..3"));
}

#[test]
fn scans_linear_lines_until_the_first_non_linear_boundary() {
    let mut source = synthetic_source();
    let first_line_file_offset = SYNTHETIC_DATA_START;
    source[first_line_file_offset..first_line_file_offset + 6]
        .copy_from_slice(&[0x20, 0xED, 0x21, 0xEE, 0x22, 0xEF]);
    let bank_end = switchable_bank_file_start(SYNTHETIC_BANK) + PRG_BANK_SIZE;

    let segment = scan_main_linear_segment(
        &source,
        first_line_file_offset,
        bank_end,
        "synthetic-dialogue",
        0,
    )
    .unwrap();

    assert_eq!(segment.line_count, 3);
    assert_eq!(segment.storage_byte_count, 6);
    assert_eq!(segment.japanese_literal_byte_count, 3);
    assert_eq!(segment.non_japanese_literal_byte_count, 0);
    assert_eq!(
        segment
            .lines
            .iter()
            .flat_map(|line| line.literal_file_offsets.iter().copied())
            .collect::<Vec<_>>(),
        vec![
            first_line_file_offset,
            first_line_file_offset + 2,
            first_line_file_offset + 4,
        ]
    );
    assert_eq!(segment.boundary_control, 0xEF);
    assert_eq!(segment.boundary_kind, "terminal");
    assert!(segment.transition_target.is_none());
    assert_eq!(
        segment
            .lines
            .iter()
            .map(|line| line.line_end_control)
            .collect::<Vec<_>>(),
        vec![0xED, 0xEE, 0xEF]
    );
}

#[test]
fn bounds_a_main_record_from_prefix_through_transition_target_bytes() {
    let mut source = synthetic_source();
    let record_file_offset = SYNTHETIC_DATA_START;
    // 창 기술자로 인정되려면 네 바이트 뒤가 제어 코드여야 한다. 줄 시작 코드는
    // 피연산자 한 바이트를 가지므로 함께 둔다.
    source[record_file_offset..record_file_offset + 9].copy_from_slice(&[
        0x00,
        0x00,
        0x00,
        0x00,
        LINE_START_CONTROL_CODE,
        0x01,
        0xE4,
        0x71,
        0x02,
    ]);
    let bank_end = switchable_bank_file_start(SYNTHETIC_BANK) + PRG_BANK_SIZE;
    let prefix = inspect_main_record_prefix(
        &source,
        record_file_offset,
        bank_end,
        "synthetic-dialogue",
        0,
    )
    .unwrap();
    let segment = scan_main_linear_segment(
        &source,
        prefix.first_line_file_offset,
        bank_end,
        "synthetic-dialogue",
        0,
    )
    .unwrap();

    let storage = build_main_record_storage(
        &source,
        record_file_offset,
        bank_end,
        &prefix,
        &segment,
        "synthetic-dialogue",
        0,
    )
    .unwrap();

    assert_eq!(storage.file_offset, record_file_offset);
    assert_eq!(storage.end_file_offset_exclusive, record_file_offset + 9);
    assert_eq!(storage.storage_byte_count, 9);
    assert_eq!(storage.prefix_byte_count, 4);
    assert_eq!(storage.linear_segment_storage_byte_count, 5);
    assert_eq!(storage.boundary_control, 0xE4);
}

#[test]
fn distinguishes_consumed_unique_and_shared_record_storage() {
    let ranges = [
        MainRecordStorageRange {
            start: 0,
            end_exclusive: 10,
        },
        MainRecordStorageRange {
            start: 5,
            end_exclusive: 15,
        },
        MainRecordStorageRange {
            start: 12,
            end_exclusive: 20,
        },
        MainRecordStorageRange {
            start: 30,
            end_exclusive: 35,
        },
    ];

    let summary = summarize_main_record_storage(&ranges).unwrap();

    assert_eq!(summary.unique_record_count, 4);
    assert_eq!(summary.consumed_storage_byte_count, 33);
    assert_eq!(summary.unique_storage_byte_count, 25);
    assert_eq!(summary.shared_storage_byte_count, 8);
    assert_eq!(summary.overlapping_record_pair_count, 2);
    assert_eq!(summary.max_overlap_depth, 2);
    assert_eq!(summary.max_storage_byte_count, 10);
}

#[test]
fn rejects_a_linear_segment_beyond_the_declared_line_limit() {
    let mut source = synthetic_source();
    let first_line_file_offset = SYNTHETIC_DATA_START;
    for line_index in 0..=MAX_MAIN_LINEAR_SEGMENT_LINES {
        let line_file_offset = first_line_file_offset + line_index * 2;
        source[line_file_offset..line_file_offset + 2].copy_from_slice(&[0x20, 0xED]);
    }
    let bank_end = switchable_bank_file_start(SYNTHETIC_BANK) + PRG_BANK_SIZE;

    let error = scan_main_linear_segment(
        &source,
        first_line_file_offset,
        bank_end,
        "synthetic-dialogue",
        0,
    )
    .unwrap_err()
    .to_string();

    assert!(error.contains("exceeds 64 linear lines"));
}

#[test]
fn closes_transition_chains_at_terminal_and_caller_handoff_boundaries() {
    let first = MainDialogueGraphNodeKey {
        table_index: 0,
        pointer_cpu_address: 0x8200,
    };
    let second = MainDialogueGraphNodeKey {
        table_index: 0,
        pointer_cpu_address: 0x8300,
    };
    let terminal = MainDialogueGraphNodeKey {
        table_index: 1,
        pointer_cpu_address: 0x8400,
    };
    let caller_handoff = MainDialogueGraphNodeKey {
        table_index: 1,
        pointer_cpu_address: 0x8500,
    };
    let states = BTreeMap::from([
        (
            first,
            MainDialogueGraphNodeState {
                boundary_control: 0xE4,
                transition_target: Some(second),
            },
        ),
        (
            second,
            MainDialogueGraphNodeState {
                boundary_control: 0xE6,
                transition_target: Some(terminal),
            },
        ),
        (
            terminal,
            MainDialogueGraphNodeState {
                boundary_control: 0xEF,
                transition_target: None,
            },
        ),
        (
            caller_handoff,
            MainDialogueGraphNodeState {
                boundary_control: 0xE7,
                transition_target: None,
            },
        ),
    ]);

    let closure = classify_main_dialogue_graph(&states).unwrap();

    assert_eq!(closure.terminal_reachable_node_count, 3);
    assert_eq!(closure.caller_handoff_boundary_reachable_node_count, 1);
    assert_eq!(closure.max_transition_edge_count_to_boundary, 2);
}

#[test]
fn rejects_a_cycle_in_the_explicit_transition_graph() {
    let first = MainDialogueGraphNodeKey {
        table_index: 0,
        pointer_cpu_address: 0x8200,
    };
    let second = MainDialogueGraphNodeKey {
        table_index: 0,
        pointer_cpu_address: 0x8300,
    };
    let states = BTreeMap::from([
        (
            first,
            MainDialogueGraphNodeState {
                boundary_control: 0xE4,
                transition_target: Some(second),
            },
        ),
        (
            second,
            MainDialogueGraphNodeState {
                boundary_control: 0xE6,
                transition_target: Some(first),
            },
        ),
    ]);

    let error = classify_main_dialogue_graph(&states)
        .unwrap_err()
        .to_string();

    assert!(error.contains("graph cycle"));
}

#[test]
fn rejects_a_pointer_outside_the_declared_source_bank_window() {
    let mut source = synthetic_source();
    let spec = synthetic_spec(1);
    write_pointer(&mut source, 0, 0xC000);

    let error = extract_dialogue_table(&source, &spec)
        .unwrap_err()
        .to_string();

    assert!(error.contains("outside its switchable PRG window"));
}

#[test]
fn admits_only_an_exact_declared_code_handler() {
    let mut source = synthetic_source();
    let mut spec = synthetic_spec(2);
    spec.allowed_handler_targets = &[TEST_FIXED_HANDLER];
    write_pointer(&mut source, 0, 0x8200);
    write_pointer(&mut source, 1, TEST_FIXED_HANDLER.cpu_address);
    let handler_file_offset = fixed_cpu_to_file_offset(TEST_FIXED_HANDLER.cpu_address).unwrap();
    source[handler_file_offset] = 0x60;

    let report = extract_dialogue_table(&source, &spec).unwrap();
    assert_eq!(report.entries[1].target_kind, "code_handler");
    assert_eq!(
        report.entries[1].handler_role,
        Some("empty_dialogue_handler")
    );

    source[handler_file_offset] = 0xEA;
    let error = extract_dialogue_table(&source, &spec)
        .unwrap_err()
        .to_string();
    assert!(error.contains("code handler empty_dialogue_handler changed"));
}

#[test]
fn rejects_a_pointer_table_that_crosses_its_prg_bank() {
    let source = synthetic_source();
    let mut spec = synthetic_spec(2);
    spec.pointer_table_file_offset = switchable_bank_file_start(SYNTHETIC_BANK) + PRG_BANK_SIZE - 2;

    let error = extract_dialogue_table(&source, &spec)
        .unwrap_err()
        .to_string();

    assert!(error.contains("pointer table is outside source PRG bank"));
}

#[test]
fn rejects_a_changed_dialogue_directory_root() {
    let mut source = synthetic_source();
    let mut spec = synthetic_spec(1);
    spec.directory_group = Some(0);
    write_pointer(&mut source, 0, 0x8200);
    let directory_file_offset =
        switchable_cpu_to_file_offset(SYNTHETIC_BANK, DIALOGUE_DIRECTORY_CPU_ADDRESS).unwrap();
    source[directory_file_offset..directory_file_offset + 2]
        .copy_from_slice(&0x8300_u16.to_le_bytes());

    let error = extract_dialogue_table(&source, &spec)
        .unwrap_err()
        .to_string();

    assert!(error.contains("dialogue directory root changed"));
}

#[test]
fn rejects_changed_directory_selector_use_code() {
    let mut source = synthetic_source();
    let mut spec = synthetic_spec(1);
    const SELECTOR_USE_CODE: &[u8] = &[0xA9, 0x21, 0x8D, 0xF4, 0x77, 0x60];
    spec.directory_group = Some(1);
    spec.directory_selector_use = Some(DirectorySelectorUseSpec {
        role: "select_synthetic_dialogue",
        prg_bank: SYNTHETIC_BANK,
        cpu_address: 0x8300,
        code: SELECTOR_USE_CODE,
    });
    let pointer_table_cpu_address =
        switchable_file_to_cpu(SYNTHETIC_BANK, SYNTHETIC_TABLE_OFFSET).unwrap();
    let directory_file_offset =
        switchable_cpu_to_file_offset(SYNTHETIC_BANK, DIALOGUE_DIRECTORY_CPU_ADDRESS + 2).unwrap();
    source[directory_file_offset..directory_file_offset + 2]
        .copy_from_slice(&pointer_table_cpu_address.to_le_bytes());
    let selector_use_file_offset = switchable_cpu_to_file_offset(SYNTHETIC_BANK, 0x8300).unwrap();
    source[selector_use_file_offset..selector_use_file_offset + SELECTOR_USE_CODE.len()]
        .copy_from_slice(SELECTOR_USE_CODE);
    source[selector_use_file_offset + SELECTOR_USE_CODE.len() - 1] = 0xEA;

    let error = extract_dialogue_table(&source, &spec)
        .unwrap_err()
        .to_string();

    assert!(error.contains("directory selector-use code changed"));
}

#[test]
fn rejects_a_changed_separate_pointer_loader() {
    let mut source = synthetic_source();
    let mut spec = synthetic_spec(1);
    const CONSUMER_CODE: &[u8] = &[0xA9, 0x00, 0x60];
    spec.separate_consumer = Some(SeparateConsumerSpec {
        prg_bank: SYNTHETIC_BANK,
        loader_cpu_address: 0x8000,
        loader_code: CONSUMER_CODE,
        table_set_index: 0,
        table_root_cell_cpu_address: 0x8010,
        table_set_selector: "synthetic_table_set",
        entry_index_selector: "synthetic_entry_index",
        destination_pointer: "synthetic_destination",
    });
    let loader_file_offset = switchable_bank_file_start(SYNTHETIC_BANK);
    source[loader_file_offset..loader_file_offset + CONSUMER_CODE.len()]
        .copy_from_slice(CONSUMER_CODE);
    source[loader_file_offset + 0x10..loader_file_offset + 0x12]
        .copy_from_slice(&0x8300_u16.to_le_bytes());
    write_pointer(&mut source, 0, 0x8200);

    let error = extract_dialogue_table(&source, &spec)
        .unwrap_err()
        .to_string();

    assert!(error.contains("separate pointer-table root changed"));
}

#[test]
fn battle_record_scanner_uses_ec_operand_width_and_first_ef_boundary() {
    let source = [
        0x08, 0x13, 0x10, 0x04, 0x01, 0xEC, 0x03, 0xED, 0xEF, 0x08, 0x13, 0x10, 0x04,
    ];

    let record = scan_battle_dialogue_record(
        &source,
        0,
        source.len(),
        &BATTLE_DIALOGUE_REFERENCED_HEADERS,
        BATTLE_DIALOGUE_TABLE_ID,
        0,
    )
    .unwrap();

    assert_eq!(record.storage_byte_count, 9);
    assert_eq!(record.end_file_offset_exclusive, 9);
    assert_eq!(record.dynamic_selector_values, [3]);
    assert_eq!(record.literal_file_offsets, [4]);
    assert_eq!(
        record
            .control_counts
            .iter()
            .map(|usage| (usage.code, usage.count))
            .collect::<Vec<_>>(),
        [(0xEC, 1), (0xED, 1), (0xEF, 1)]
    );
}

#[test]
fn battle_record_scanner_rejects_an_out_of_range_dynamic_selector() {
    let source = [0x08, 0x13, 0x10, 0x04, 0xEC, 0x04, 0xEF];

    let error = scan_battle_dialogue_record(
        &source,
        0,
        source.len(),
        &BATTLE_DIALOGUE_REFERENCED_HEADERS,
        BATTLE_DIALOGUE_TABLE_ID,
        0,
    )
    .unwrap_err()
    .to_string();

    assert!(error.contains("out-of-range EC selector 04"));
}
