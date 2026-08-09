use super::*;

#[test]
fn approved_set_uses_only_complete_lines() {
    let workspace = workspace_with_lines(vec![
        line("line-complete", "한{EF}", TranslationStatus::Complete),
        line("line-review", "글{EF}", TranslationStatus::NeedsHumanReview),
    ]);

    let report = build_glyph_workset_report(&workspace, "workspace-sha1".to_owned()).unwrap();

    assert_eq!(report.status_counts.complete, 1);
    assert_eq!(report.status_counts.needs_human_review, 1);
    assert_eq!(report.filled_glyphs.unique_count, 2);
    assert_eq!(report.approved_glyphs.unique_count, 1);
    assert!(!report.capacity.working_set_ready);
    assert_eq!(report.capacity.approved_single_page_fit, None);
}

#[test]
fn all_complete_input_reports_final_capacity() {
    let glyphs = (0..211)
        .map(|index| char::from_u32(0xAC00 + index).unwrap())
        .collect::<String>();
    let workspace = workspace_with_lines(vec![line(
        "line-complete",
        &format!("{glyphs}{{EF}}"),
        TranslationStatus::Complete,
    )]);

    let report = build_glyph_workset_report(&workspace, "workspace-sha1".to_owned()).unwrap();

    assert!(report.capacity.translation_input_complete);
    assert!(report.capacity.working_set_ready);
    assert_eq!(report.approved_glyphs.unique_count, 211);
    assert_eq!(report.capacity.approved_single_page_fit, Some(false));
    assert!(report.capacity.final_page_plan_eligible);
}

#[test]
fn serialized_report_omits_dialogue_glyphs_and_paths() {
    let workspace = workspace_with_lines(vec![line(
        "line-complete",
        "한{EF}",
        TranslationStatus::Complete,
    )]);
    let report = build_glyph_workset_report(&workspace, "workspace-sha1".to_owned()).unwrap();
    let json = serde_json::to_string(&report).unwrap();

    assert!(!json.contains('한'));
    assert!(!json.contains("line-complete"));
    assert!(!json.contains("private/"));
    assert!(json.contains("\"glyph_characters_emitted\":false"));
}

fn workspace_with_lines(lines: Vec<WorkspaceLine>) -> MainDialogueWorkspace {
    MainDialogueWorkspace {
        format_version: 2,
        source_sha1: EXPECTED_SOURCE_SHA1.to_owned(),
        translate_from: "ja".to_owned(),
        translate_to: "ko".to_owned(),
        preserve_existing_english: true,
        purpose: "private_translation_workspace".to_owned(),
        safe_japanese_source_byte_count: 2,
        records: vec![WorkspaceRecord {
            id: "record".to_owned(),
            table_id: "main-dialogue".to_owned(),
            source_prg_bank: 0,
            canonical_entry_index: 0,
            entry_indices: vec![0],
            pointer_cpu_address_hex: "0x8000".to_owned(),
            prefix_byte_count: 0,
            boundary_control_hex: "EF".to_owned(),
            lines,
        }],
    }
}

fn line(id: &str, korean: &str, status: TranslationStatus) -> WorkspaceLine {
    WorkspaceLine {
        id: id.to_owned(),
        index: 0,
        file_offset_hex: "0x00000".to_owned(),
        source_storage_sha1: "source".to_owned(),
        source_markup: "あ{EF}".to_owned(),
        korean: korean.to_owned(),
        status,
        japanese_source_byte_count: 1,
        safe_japanese_source_byte_count: 1,
        requires_relocation: false,
        conflicting_file_offsets_hex: Vec::new(),
    }
}
