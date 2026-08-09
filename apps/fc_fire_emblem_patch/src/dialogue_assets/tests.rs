use super::*;

fn record(
    source_prg_bank: u8,
    file_offset: usize,
    end_file_offset_exclusive: usize,
) -> MainDialogueStorageRecord {
    MainDialogueStorageRecord {
        table_id: "synthetic-dialogue",
        source_prg_bank,
        canonical_entry_index: 0,
        entry_indices: vec![0],
        pointer_file_offsets: vec![0],
        pointer_cpu_address: 0x8000,
        file_offset,
        end_file_offset_exclusive,
        storage_byte_count: end_file_offset_exclusive - file_offset,
        storage_sha1: String::new(),
        prefix_byte_count: 4,
        boundary_control: 0xEF,
        literal_file_offsets: Vec::new(),
        lines: Vec::new(),
    }
}

fn workspace_line(source_markup: &str, korean: &str) -> WorkspaceLine {
    WorkspaceLine {
        id: "synthetic-dialogue:000:line:00".to_owned(),
        index: 0,
        file_offset_hex: "0x00000".to_owned(),
        source_storage_sha1: "source-line".to_owned(),
        source_markup: source_markup.to_owned(),
        korean: korean.to_owned(),
        status: TranslationStatus::Complete,
        japanese_source_byte_count: 3,
        safe_japanese_source_byte_count: 3,
        requires_relocation: false,
        conflicting_file_offsets_hex: Vec::new(),
    }
}

fn logical_record(id: &str, bytes: &[u8]) -> LogicalDialogueRecord {
    LogicalDialogueRecord {
        id: id.to_owned(),
        source_prg_bank: 2,
        source_pointer_cpu_address: 0x8000,
        pointer_file_offsets: vec![0],
        source_file_offset: 0,
        source_storage_byte_count: bytes.len(),
        translated_line_count: 0,
        bytes: bytes
            .iter()
            .copied()
            .map(LogicalDialogueByte::Encoded)
            .collect(),
    }
}

#[test]
fn normalizes_shared_and_adjacent_records_into_disjoint_owned_regions() {
    let records = [
        record(2, 10, 20),
        record(2, 15, 25),
        record(2, 25, 30),
        record(2, 40, 45),
        record(3, 10, 15),
    ];

    let ranges = normalize_storage_ranges(&records).unwrap();

    assert_eq!(
        ranges,
        vec![
            OwnedStorageRange {
                source_prg_bank: 2,
                start: 10,
                end_exclusive: 30,
            },
            OwnedStorageRange {
                source_prg_bank: 2,
                start: 40,
                end_exclusive: 45,
            },
            OwnedStorageRange {
                source_prg_bank: 3,
                start: 10,
                end_exclusive: 15,
            },
        ]
    );
}

#[test]
fn hex_storage_roundtrips_and_rejects_non_hex_input() {
    let bytes = [0x00, 0x7F, 0x80, 0xFF];

    assert_eq!(decode_hex(&encode_hex(&bytes)).unwrap(), bytes);
    assert!(decode_hex("0").is_err());
    assert!(decode_hex("gg").is_err());
}

#[test]
fn decodes_japanese_latin_unknown_literals_and_controls_without_mixing_them() {
    let source = [0x3A, 0x32, 0x5F, 0x44, 0x0F, 0xED];
    let line = MainDialogueStorageLine {
        file_offset: 0,
        storage_byte_count: source.len(),
        storage_sha1: String::new(),
        line_end_control: 0xED,
        literal_file_offsets: (0..5).collect(),
    };
    assert_eq!(
        decode_line_markup(&source, &line).unwrap(),
        "サウント゛{ED}"
    );

    let source = [0x7C, 0x7D, 0x7B, 0xFF, 0x9D, 0xE9, 0x03, 0xEF];
    let line = MainDialogueStorageLine {
        file_offset: 0,
        storage_byte_count: source.len(),
        storage_sha1: String::new(),
        line_end_control: 0xEF,
        literal_file_offsets: (0..5).collect(),
    };
    assert_eq!(
        decode_line_markup(&source, &line).unwrap(),
        "STR{SP}{LIT:9D}{E9:03}{EF}"
    );
}

#[test]
fn excludes_a_japanese_literal_that_an_overlapping_record_reads_as_structure() {
    let source = vec![0x00; 8];
    let mut first = record(2, 0, 4);
    first.literal_file_offsets = vec![0, 1];
    let mut second = record(2, 1, 4);
    second.literal_file_offsets = vec![2];

    let safe = safe_japanese_literal_offsets(&source, &[first, second]).unwrap();

    assert_eq!(safe, BTreeSet::from([0]));
}

#[test]
fn accepts_hangul_while_preserving_existing_english_and_control_tokens() {
    let line = workspace_line("マルスSTR{SP}{E9:03}{EF}", "마르스STR{SP}{E9:03}{EF}");

    assert_eq!(validate_translation_markup(&line).unwrap(), 3);
}

#[test]
fn rejects_changed_existing_english_in_a_korean_target() {
    let line = workspace_line("マルスSTR{SP}{E9:03}{EF}", "마르스SKI{SP}{E9:03}{EF}");

    let error = validate_translation_markup(&line).unwrap_err().to_string();
    assert!(error.contains("existing English"));
}

#[test]
fn rejects_changed_or_moved_control_tokens() {
    let changed = workspace_line("マルス{E9:03}{EF}", "마르스{E9:04}{EF}");
    assert!(
        validate_translation_markup(&changed)
            .unwrap_err()
            .to_string()
            .contains("protected control token")
    );

    let moved = workspace_line("マルス{EF}", "마르스{EF}님");
    assert!(
        validate_translation_markup(&moved)
            .unwrap_err()
            .to_string()
            .contains("line-end control token")
    );
}

#[test]
fn rejects_japanese_remaining_in_a_korean_target() {
    let line = workspace_line("マルス{EF}", "마르ス{EF}");

    let error = validate_translation_markup(&line).unwrap_err().to_string();
    assert!(error.contains("inspect korean markup"));
}

#[test]
fn encodes_target_glyphs_as_logical_bytes_and_preserves_source_codes() {
    assert_eq!(
        encode_korean_markup("한STR{SP}{E9:03}{EF}").unwrap(),
        vec![
            LogicalDialogueByte::TargetGlyph('한'),
            LogicalDialogueByte::Encoded(0x7C),
            LogicalDialogueByte::Encoded(0x7D),
            LogicalDialogueByte::Encoded(0x7B),
            LogicalDialogueByte::Encoded(0xFF),
            LogicalDialogueByte::Encoded(0xE9),
            LogicalDialogueByte::Encoded(0x03),
            LogicalDialogueByte::Encoded(0xEF),
        ]
    );
    assert!(decode_protected_token("{E9}").is_err());
    assert!(decode_protected_token("{E9:03:04}").is_err());
}

#[test]
fn packs_shared_suffixes_once_and_splits_changed_records() {
    let whole = logical_record("whole", &[0x10, 0x20, 0x30]);
    let shared_tail = logical_record("tail", &[0x20, 0x30]);
    let (storage, placements) = pack_logical_records(&[&whole, &shared_tail]);
    assert_eq!(
        storage,
        vec![
            LogicalDialogueByte::Encoded(0x10),
            LogicalDialogueByte::Encoded(0x20),
            LogicalDialogueByte::Encoded(0x30),
        ]
    );
    assert_eq!(placements, vec![0, 1]);

    let changed_tail = LogicalDialogueRecord {
        bytes: vec![
            LogicalDialogueByte::TargetGlyph('한'),
            LogicalDialogueByte::Encoded(0x30),
        ],
        translated_line_count: 1,
        ..logical_record("changed-tail", &[0x20, 0x30])
    };
    let (storage, placements) = pack_logical_records(&[&whole, &changed_tail]);
    assert_eq!(storage.len(), 5);
    assert_eq!(placements, vec![0, 3]);
}
