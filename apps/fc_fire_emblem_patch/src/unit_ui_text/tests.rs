use super::*;

fn contract_fixture() -> Vec<u8> {
    let mut prg = vec![0; PRG_SIZE];
    for spec in CODE_REGION_SPECS {
        let offset = banked_prg_offset(UNIT_UI_BANK, spec.cpu_address).unwrap();
        prg[offset..offset + spec.expected.len()].copy_from_slice(spec.expected);
    }
    command_menu::install_fixture(&mut prg);
    for spec in SUMMARY_AND_STATUS_LABEL_SPECS
        .iter()
        .chain(command_menu::COMMAND_LABEL_SPECS)
    {
        let pointer_address = FIXED_STRING_POINTER_TABLE_ADDRESS + u16::from(spec.index) * 2;
        let pointer_offset = banked_prg_offset(UNIT_UI_BANK, pointer_address).unwrap();
        prg[pointer_offset..pointer_offset + 2].copy_from_slice(&spec.pointer.to_le_bytes());
        let source_offset = banked_prg_offset(UNIT_UI_BANK, spec.pointer).unwrap();
        prg[source_offset..source_offset + spec.expected.len()].copy_from_slice(spec.expected);
    }
    prg
}

fn build_fixture_report(prg: &[u8]) -> Result<UnitUiTextReport> {
    build_report(prg, glyph_budget::fixture_report(25))
}

#[test]
fn binds_unit_ui_page_supply_and_inheritance_roles() {
    let report = build_fixture_report(&contract_fixture()).unwrap();

    let states = report
        .composition_dispatch
        .relevant_states
        .iter()
        .map(|entry| (entry.state, entry.role, entry.handler_address))
        .collect::<Vec<_>>();
    assert_eq!(
        states,
        vec![
            (0x04, "unit_summary_header", 0x826C),
            (0x05, "unit_command_menu", 0x82E3),
            (0x07, "unit_summary_items", 0x85BE),
            (0x0F, "unit_status_stats", 0x87F2),
        ]
    );
    assert_eq!(
        report.page_lifetime.right_fd_page_supplied_by_screen_roles,
        vec!["unit_summary", "unit_command_menu"]
    );
    assert_eq!(
        report.page_lifetime.proven_inherited_by_screen_roles,
        vec!["unit_status"]
    );
    assert_eq!(
        report.screen_roles[2].inherited_content,
        vec!["unit_summary_header"]
    );
    assert_eq!(report.command_menu.static_label_count, 15);
    assert_eq!(report.command_menu.runtime_observed_label_count, 6);
}

#[test]
fn preserves_original_hp_label_while_targeting_japanese_labels() {
    let report = build_fixture_report(&contract_fixture()).unwrap();
    let hp = report
        .fixed_labels
        .iter()
        .find(|label| label.index == 0x09)
        .unwrap();

    assert_eq!(hp.source_text, "HP");
    assert_eq!(hp.translation_scope, "preserve_original_latin");
    assert_eq!(
        report
            .fixed_labels
            .iter()
            .filter(|label| label.translation_scope == "japanese_only")
            .count(),
        25
    );
}

#[test]
fn rejects_a_changed_summary_item_composer() {
    let mut prg = contract_fixture();
    let offset = banked_prg_offset(UNIT_UI_BANK, 0x85BE).unwrap();
    prg[offset + 19] ^= 0x01;

    let error = build_fixture_report(&prg).unwrap_err().to_string();
    assert!(error.contains("compose_unit_summary_items"));
}

#[test]
fn rejects_a_changed_fixed_label_pointer() {
    let mut prg = contract_fixture();
    let pointer_address = FIXED_STRING_POINTER_TABLE_ADDRESS + 0x27 * 2;
    let offset = banked_prg_offset(UNIT_UI_BANK, pointer_address).unwrap();
    prg[offset] ^= 0x01;

    let error = build_fixture_report(&prg).unwrap_err().to_string();
    assert!(error.contains("index 0x27"));
}

#[test]
fn rejects_a_changed_command_menu_composer() {
    let mut prg = contract_fixture();
    let offset = banked_prg_offset(UNIT_UI_BANK, command_menu::composer_address()).unwrap();
    prg[offset + 0x20] ^= 0x01;

    let error = build_fixture_report(&prg).unwrap_err().to_string();
    assert!(error.contains("unit-command-menu composer"));
}
