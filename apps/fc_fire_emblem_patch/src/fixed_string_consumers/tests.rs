use super::*;

fn synthetic_bank() -> Vec<u8> {
    let mut bank = vec![0xEA; PRG_BANK_BYTE_COUNT];
    let table = bank_offset(FIXED_STRING_POINTER_TABLE).unwrap();
    let mut pointer = FIXED_STRING_STORAGE_START;
    for index in 0..FIXED_STRING_ENTRY_COUNT {
        let record_len = if index + 1 == FIXED_STRING_ENTRY_COUNT {
            14
        } else {
            7
        };
        bank[table + index * 2..table + index * 2 + 2].copy_from_slice(&pointer.to_le_bytes());
        let start = bank_offset(pointer).unwrap();
        bank[start..start + record_len - 1].fill(u8::try_from(index).unwrap());
        bank[start + record_len - 1] = 0xED;
        pointer += u16::try_from(record_len).unwrap();
    }
    assert_eq!(pointer, FIXED_STRING_STORAGE_END_EXCLUSIVE);
    bank
}

#[test]
fn contiguous_pointer_table_is_the_record_denominator() {
    let bank = synthetic_bank();
    let records = parse_fixed_string_records(&bank).unwrap();

    assert_eq!(records.len(), FIXED_STRING_ENTRY_COUNT);
    assert_eq!(records[0].pointer, FIXED_STRING_STORAGE_START);
    assert_eq!(
        records.last().unwrap().pointer
            + u16::try_from(records.last().unwrap().source_bytes.len()).unwrap(),
        FIXED_STRING_STORAGE_END_EXCLUSIVE
    );
}

#[test]
fn duplicate_gap_and_unterminated_records_fail_closed() {
    let mut bank = synthetic_bank();
    let table = bank_offset(FIXED_STRING_POINTER_TABLE).unwrap();
    bank[table + 2..table + 4].copy_from_slice(&FIXED_STRING_STORAGE_START.to_le_bytes());
    assert!(
        parse_fixed_string_records(&bank)
            .unwrap_err()
            .to_string()
            .contains("repeats")
    );

    let mut bank = synthetic_bank();
    let second = FIXED_STRING_STORAGE_START + 3;
    bank[table + 2..table + 4].copy_from_slice(&second.to_le_bytes());
    assert!(
        parse_fixed_string_records(&bank)
            .unwrap_err()
            .to_string()
            .contains("not contiguous")
    );

    let mut bank = synthetic_bank();
    let last = bank_offset(u16::from_le_bytes([
        bank[table + (FIXED_STRING_ENTRY_COUNT - 1) * 2],
        bank[table + (FIXED_STRING_ENTRY_COUNT - 1) * 2 + 1],
    ]))
    .unwrap();
    let last_len = usize::from(FIXED_STRING_STORAGE_END_EXCLUSIVE) - usize::from(0x8000_u16) - last;
    bank[last + last_len - 1] = 0x40;
    assert!(parse_fixed_string_records(&bank).is_err());
}

#[test]
fn immediate_call_index_is_read_from_the_source_instruction() {
    let mut bank = vec![0xEA; PRG_BANK_BYTE_COUNT];
    let start = bank_offset(0x9000).unwrap();
    bank[start..start + 5].copy_from_slice(&[0xA9, 0x2C, 0x20, 0xEE, 0x8E]);

    assert_eq!(classify_call_indices(&bank, 0x9002).unwrap(), [0x2C]);

    bank[start] = 0xA2;
    assert!(classify_call_indices(&bank, 0x9002).is_err());
}

#[test]
fn direct_producer_filter_does_not_promote_unowned_handlers() {
    let calls = [
        FixedStringCallSite {
            cpu_address: 0x8000,
            composite_state: 0x00,
            possible_indices: vec![0x1E, 0x1F],
        },
        FixedStringCallSite {
            cpu_address: 0x8100,
            composite_state: 0x18,
            possible_indices: vec![0x2C],
        },
    ];
    let produced_states = BTreeSet::from([0x18]);
    let indices = calls
        .iter()
        .filter(|call| produced_states.contains(&call.composite_state))
        .flat_map(|call| call.possible_indices.iter().copied())
        .collect::<BTreeSet<_>>();

    assert_eq!(indices, BTreeSet::from([0x2C]));
}
