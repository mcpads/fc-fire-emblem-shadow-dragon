use super::*;

#[test]
fn approved_set_uses_only_complete_lines() {
    let workspace = workspace_with_lines(vec![
        line("line-complete", "한{EF}", TranslationStatus::Complete),
        line("line-review", "글{EF}", TranslationStatus::NeedsHumanReview),
    ]);

    let report =
        build_glyph_workset_report(&workspace, &empty_graph(), "workspace-sha1".to_owned())
            .unwrap();

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

    let report =
        build_glyph_workset_report(&workspace, &empty_graph(), "workspace-sha1".to_owned())
            .unwrap();

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
    let report =
        build_glyph_workset_report(&workspace, &empty_graph(), "workspace-sha1".to_owned())
            .unwrap();
    let json = serde_json::to_string(&report).unwrap();

    assert!(!json.contains('한'));
    assert!(!json.contains("line-complete"));
    assert!(!json.contains("private/"));
    assert!(json.contains("\"glyph_characters_emitted\":false"));
}

#[test]
fn transition_chain_capacity_uses_the_union_of_every_record() {
    let first_glyphs = (0..110)
        .map(|index| char::from_u32(0xAC00 + index).unwrap())
        .collect::<String>();
    let second_glyphs = (110..220)
        .map(|index| char::from_u32(0xAC00 + index).unwrap())
        .collect::<String>();
    let workspace = workspace_with_records(vec![
        record("main-dialogue", 0, &format!("{first_glyphs}{{E6:80:01}}")),
        record("main-dialogue", 1, &format!("{second_glyphs}{{EF}}")),
    ]);
    let graph = graph_with_edge("main-dialogue", 0, "main-dialogue", 1);

    let report =
        build_glyph_workset_report(&workspace, &graph, "workspace-sha1".to_owned()).unwrap();

    assert_eq!(report.max_record_unique_glyph_count, 110);
    assert_eq!(report.max_transition_chain_unique_glyph_count, 220);
    assert!(!report.capacity.filled_transition_chains_fit_one_page_so_far);
}

#[test]
fn observed_shop_lifetime_counts_retained_source_slots_and_both_dialogue_records() {
    let first_glyphs = (0..97)
        .map(|index| char::from_u32(0xAC00 + index).unwrap())
        .collect::<String>();
    let second_glyphs = (97..194)
        .map(|index| char::from_u32(0xAC00 + index).unwrap())
        .collect::<String>();
    let workspace = workspace_with_records(vec![
        record(
            "shop-and-item-dialogue",
            0,
            &format!("{first_glyphs}{{EF}}"),
        ),
        record(
            "shop-and-item-dialogue",
            1,
            &format!("{second_glyphs}{{E7}}"),
        ),
    ]);

    let report =
        build_glyph_workset_report(&workspace, &empty_graph(), "workspace-sha1".to_owned())
            .unwrap();

    let lifetime = &report.observed_screen_lifetimes[0];
    assert_eq!(lifetime.source_record_count, 2);
    assert_eq!(lifetime.filled_unique_glyph_count, 194);
    assert_eq!(lifetime.preserved_active_source_code_count, 17);
    assert_eq!(lifetime.filled_slot_demand, 211);
    assert!(!lifetime.filled_set_fits_one_page_so_far);
    assert_eq!(lifetime.approved_slot_demand, Some(211));
    assert_eq!(lifetime.approved_set_fits_one_page, Some(false));
    assert!(
        !report
            .capacity
            .filled_observed_screen_lifetimes_fit_one_page_so_far
    );
    assert_eq!(
        report
            .capacity
            .approved_observed_screen_lifetimes_fit_one_page,
        Some(false)
    );
}

#[test]
fn observed_epilogue_family_reserves_names_locations_and_the_dialogue_chain() {
    let glyphs = (0..94)
        .map(|index| char::from_u32(0xAC00 + index).unwrap())
        .collect::<String>();
    let workspace = workspace_with_records(vec![record(
        "epilogue-dialogue",
        0,
        &format!("{glyphs}{{E7}}"),
    )]);

    let report =
        build_glyph_workset_report(&workspace, &empty_graph(), "workspace-sha1".to_owned())
            .unwrap();

    let lifetime = &report.observed_screen_lifetimes[0];
    assert_eq!(lifetime.source_record_count, 1);
    assert_eq!(lifetime.filled_unique_glyph_count, 94);
    assert_eq!(lifetime.preserved_active_source_code_count, 99);
    assert_eq!(lifetime.additional_target_glyph_reservation_count, 18);
    assert_eq!(lifetime.filled_slot_demand, 211);
    assert!(!lifetime.filled_set_fits_one_page_so_far);
}

#[test]
fn observed_game_over_budget_uses_every_victory_and_defeat_glyph() {
    let glyphs = (0..121)
        .map(|index| char::from_u32(0xAC00 + index).unwrap())
        .collect::<String>();
    let workspace = workspace_with_records(vec![record(
        "victory-and-defeat-dialogue",
        0,
        &format!("{glyphs}{{E7}}"),
    )]);

    let report =
        build_glyph_workset_report(&workspace, &empty_graph(), "workspace-sha1".to_owned())
            .unwrap();

    let lifetime = &report.observed_screen_lifetimes[0];
    assert_eq!(lifetime.source_record_count, 1);
    assert_eq!(lifetime.filled_unique_glyph_count, 121);
    assert_eq!(lifetime.preserved_active_source_code_count, 90);
    assert_eq!(lifetime.filled_slot_demand, 211);
    assert!(!lifetime.filled_set_fits_one_page_so_far);
}

fn workspace_with_lines(lines: Vec<WorkspaceLine>) -> MainDialogueWorkspace {
    workspace_with_records(vec![WorkspaceRecord {
        id: "record".to_owned(),
        table_id: "main-dialogue".to_owned(),
        source_prg_bank: 0,
        canonical_entry_index: 0,
        entry_indices: vec![0],
        pointer_cpu_address_hex: "0x8000".to_owned(),
        prefix_byte_count: 0,
        boundary_control_hex: "EF".to_owned(),
        lines,
    }])
}

fn workspace_with_records(records: Vec<WorkspaceRecord>) -> MainDialogueWorkspace {
    MainDialogueWorkspace {
        format_version: 2,
        source_sha1: EXPECTED_SOURCE_SHA1.to_owned(),
        translate_from: "ja".to_owned(),
        translate_to: "ko".to_owned(),
        preserve_existing_english: true,
        purpose: "private_translation_workspace".to_owned(),
        safe_japanese_source_byte_count: 2,
        records,
    }
}

fn record(table_id: &str, index: usize, korean: &str) -> WorkspaceRecord {
    WorkspaceRecord {
        id: format!("{table_id}:{index:03}"),
        table_id: table_id.to_owned(),
        source_prg_bank: 0,
        canonical_entry_index: index,
        entry_indices: vec![index],
        pointer_cpu_address_hex: "0x8000".to_owned(),
        prefix_byte_count: 0,
        boundary_control_hex: "EF".to_owned(),
        lines: vec![line(
            &format!("line-{index}"),
            korean,
            TranslationStatus::Complete,
        )],
    }
}

fn empty_graph() -> MainDialogueGraphReport {
    MainDialogueGraphReport {
        node_count: 1,
        transition_edge_count: 0,
        terminal_reachable_node_count: 1,
        caller_handoff_boundary_reachable_node_count: 0,
        max_transition_edge_count_to_boundary: 0,
        cycle_count: 0,
        unresolved_node_count: 0,
        transition_edges: Vec::new(),
    }
}

fn graph_with_edge(
    source_table_id: &'static str,
    source_index: usize,
    target_table_id: &'static str,
    target_index: usize,
) -> MainDialogueGraphReport {
    MainDialogueGraphReport {
        node_count: 2,
        transition_edge_count: 1,
        terminal_reachable_node_count: 2,
        caller_handoff_boundary_reachable_node_count: 0,
        max_transition_edge_count_to_boundary: 1,
        cycle_count: 0,
        unresolved_node_count: 0,
        transition_edges: vec![MainDialogueTransitionEdgeReport {
            source_table_id,
            source_canonical_entry_index: source_index,
            source_entry_indices: vec![source_index],
            source_pointer_cpu_address: 0x8000,
            source_pointer_cpu_address_hex: "0x8000".to_owned(),
            source_file_offset: 0,
            source_file_offset_hex: "0x00000".to_owned(),
            control: 0xE6,
            control_hex: "E6".to_owned(),
            target_table_id,
            target_entry_index: target_index,
            target_canonical_entry_index: target_index,
            target_pointer_cpu_address: 0x8001,
            target_pointer_cpu_address_hex: "0x8001".to_owned(),
            target_file_offset: 1,
            target_file_offset_hex: "0x00001".to_owned(),
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
