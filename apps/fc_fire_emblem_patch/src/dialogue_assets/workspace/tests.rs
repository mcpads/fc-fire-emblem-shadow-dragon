use super::*;

#[test]
fn preserves_a_translation_when_its_stable_line_and_source_match() {
    let mut fresh = workspace("あ{EF}", "", TranslationStatus::Untranslated);
    let existing = workspace("あ{EF}", "한{EF}", TranslationStatus::Complete);

    let preserved = preserve_workspace_translations(&mut fresh, &existing).unwrap();

    assert_eq!(preserved, 1);
    assert_eq!(fresh.records[0].lines[0].korean, "한{EF}");
    assert_eq!(
        fresh.records[0].lines[0].status,
        TranslationStatus::Complete
    );
}

#[test]
fn refuses_to_replace_a_workspace_when_a_translated_source_changed() {
    let mut fresh = workspace("い{EF}", "", TranslationStatus::Untranslated);
    let before = fresh.clone();
    let existing = workspace("あ{EF}", "한{EF}", TranslationStatus::NeedsHumanReview);

    let error = preserve_workspace_translations(&mut fresh, &existing).unwrap_err();

    assert!(error.to_string().contains("translated source changed"));
    assert_eq!(fresh, before);
}

#[test]
fn refuses_to_drop_a_translated_line_that_no_longer_exists() {
    let mut fresh = workspace("あ{EF}", "", TranslationStatus::Untranslated);
    fresh.records[0].lines[0].id = "replacement:line:00".to_owned();
    let before = fresh.clone();
    let existing = workspace("あ{EF}", "한{EF}", TranslationStatus::InProgress);

    let error = preserve_workspace_translations(&mut fresh, &existing).unwrap_err();

    assert!(error.to_string().contains("no longer exists"));
    assert_eq!(fresh, before);
}

fn workspace(
    source_markup: &str,
    korean: &str,
    status: TranslationStatus,
) -> MainDialogueWorkspace {
    MainDialogueWorkspace {
        format_version: WORKSPACE_FORMAT_VERSION,
        source_sha1: EXPECTED_SOURCE_SHA1.to_owned(),
        translate_from: "ja".to_owned(),
        translate_to: "ko".to_owned(),
        preserve_existing_english: true,
        purpose: "private_translation_workspace".to_owned(),
        safe_japanese_source_byte_count: 1,
        source_preservation_line_ids: Vec::new(),
        records: vec![WorkspaceRecord {
            id: "chapter-intro-dialogue:000".to_owned(),
            table_id: "chapter-intro-dialogue".to_owned(),
            source_prg_bank: 0,
            canonical_entry_index: 0,
            entry_indices: vec![0],
            pointer_cpu_address_hex: "0x8000".to_owned(),
            prefix_byte_count: 0,
            boundary_control_hex: "EF".to_owned(),
            lines: vec![WorkspaceLine {
                id: "chapter-intro-dialogue:000:line:00".to_owned(),
                index: 0,
                file_offset_hex: "0x00000".to_owned(),
                source_storage_sha1: "source".to_owned(),
                source_markup: source_markup.to_owned(),
                korean: korean.to_owned(),
                status,
                japanese_source_byte_count: 1,
                safe_japanese_source_byte_count: 1,
                requires_relocation: false,
                conflicting_file_offsets_hex: Vec::new(),
            }],
        }],
    }
}
