use super::*;

#[test]
fn incomplete_japanese_part_fails_closed_while_protected_english_is_not_translated() {
    let workspace = fixture_workspace(
        part(
            "record:direct-leading",
            "あA",
            "",
            TranslationStatus::Untranslated,
        ),
        part(
            "record:common-body",
            "い{EF}",
            "이{EF}",
            TranslationStatus::Complete,
        ),
        part(
            "record:transition-leading",
            "A",
            "",
            TranslationStatus::Untranslated,
        ),
    );

    let counts = validate_workspace_translations(&workspace).unwrap();

    assert_eq!(counts.untranslated_japanese_part_count, 1);
    assert_eq!(counts.filled_part_count, 1);
}

#[test]
fn translated_segment_must_preserve_existing_english() {
    let workspace = fixture_workspace(
        part(
            "record:direct-leading",
            "あA{ED}",
            "한{ED}",
            TranslationStatus::Complete,
        ),
        part(
            "record:common-body",
            "い{EF}",
            "이{EF}",
            TranslationStatus::Complete,
        ),
        part(
            "record:transition-leading",
            "",
            "",
            TranslationStatus::Untranslated,
        ),
    );

    let error = validate_workspace_translations(&workspace).unwrap_err();

    assert!(
        error
            .to_string()
            .contains("protected control token or existing English")
    );
}

fn part(id: &str, source: &str, korean: &str, status: TranslationStatus) -> EntryModePart {
    EntryModePart {
        id: id.to_owned(),
        role: if id.contains("common") {
            EntryModePartRole::CommonBody
        } else if id.contains("direct") {
            EntryModePartRole::DirectLeading
        } else {
            EntryModePartRole::TransitionLeading
        },
        source_file_offset_hex: "0x00000".to_owned(),
        source_storage_byte_count: 1,
        source_storage_sha1: "source".to_owned(),
        source_markup: source.to_owned(),
        japanese_source_byte_count: usize::from(
            source
                .chars()
                .any(|character| matches!(character, 'あ' | 'い')),
        ),
        korean: korean.to_owned(),
        status,
    }
}

fn fixture_workspace(
    direct_leading: EntryModePart,
    common_body: EntryModePart,
    transition_leading: EntryModePart,
) -> EntryModeWorkspace {
    EntryModeWorkspace {
        format_version: ENTRY_MODE_WORKSPACE_FORMAT_VERSION,
        source_sha1: EXPECTED_SOURCE_SHA1.to_owned(),
        translate_from: "ja".to_owned(),
        translate_to: "ko".to_owned(),
        preserve_existing_english: true,
        purpose: WORKSPACE_PURPOSE.to_owned(),
        reachability_policy: REACHABILITY_POLICY.to_owned(),
        required_entry_modes: REQUIRED_ENTRY_MODES.map(str::to_owned),
        differing_entry_start_japanese_source_byte_count: 1,
        records: vec![EntryModeRecord {
            id: "record".to_owned(),
            incoming_transition_edge_count: 1,
            direct_prefix_byte_count: 4,
            transition_prefix_byte_count: 0,
            common_body_source_file_offset_hex: "0x00004".to_owned(),
            divergent_segment_source_sha1: "leading".to_owned(),
            direct_leading,
            common_body,
            transition_leading,
        }],
    }
}
