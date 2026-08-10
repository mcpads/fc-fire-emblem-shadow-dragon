use std::collections::BTreeSet;
use std::{fs, path::Path};

use anyhow::{Context, Result, ensure};
use serde::Serialize;

use crate::{
    rom::{EXPECTED_SOURCE_SHA1, HEADER_SIZE, PRG_SIZE, Rom},
    sha1_hex,
};

mod command_menu;
mod glyph_budget;
mod source_spec;
#[cfg(test)]
mod tests;

use source_spec::*;

const PRG_BANK_SIZE: usize = 16 * 1024;
pub(super) const UNIT_UI_BANK: usize = 0x0B;
const SWITCHABLE_CPU_BASE: u16 = 0x8000;
const FIXED_STRING_POINTER_TABLE_ADDRESS: u16 = 0x8FC2;

#[derive(Debug, Serialize)]
struct UnitUiTextReport {
    schema: u8,
    source_sha1: &'static str,
    screen_roles: Vec<ScreenRoleReport>,
    composition_dispatch: CompositionDispatchReport,
    shared_appenders: Vec<CodeRegionReport>,
    fixed_labels: Vec<FixedLabelReport>,
    dynamic_sources: Vec<DynamicSourceReport>,
    glyph_budget: glyph_budget::GlyphBudgetReport,
    command_menu: command_menu::CommandMenuReport,
    page_lifetime: PageLifetimeReport,
    implementation_boundary: ImplementationBoundary,
}

#[derive(Debug, Serialize)]
struct ScreenRoleReport {
    screen_role: &'static str,
    composers: Vec<&'static str>,
    inherited_content: Vec<&'static str>,
    translation_scope: &'static str,
}

#[derive(Debug, Serialize)]
struct CompositionDispatchReport {
    state_address: &'static str,
    dispatcher: CodeRegionReport,
    relevant_states: Vec<CompositionStateReport>,
}

#[derive(Debug, Serialize)]
struct CompositionStateReport {
    state: u8,
    state_hex: String,
    role: &'static str,
    handler_address: u16,
    handler_address_hex: String,
    handler: CodeRegionReport,
}

#[derive(Debug, Serialize, Clone)]
pub(super) struct CodeRegionReport {
    pub(super) role: &'static str,
    pub(super) prg_bank: usize,
    pub(super) prg_bank_hex: String,
    pub(super) cpu_address: u16,
    pub(super) cpu_address_hex: String,
    pub(super) file_offset: usize,
    pub(super) file_offset_hex: String,
    pub(super) byte_count: usize,
    pub(super) bytes_hex: String,
}

#[derive(Debug, Serialize)]
struct FixedLabelReport {
    index: u8,
    index_hex: String,
    source_text: &'static str,
    translation_scope: &'static str,
    pointer_table_address: u16,
    pointer_table_address_hex: String,
    source_address: u16,
    source_address_hex: String,
    source_codes: Vec<u8>,
    source_codes_hex: Vec<String>,
}

#[derive(Debug, Serialize)]
struct DynamicSourceReport {
    role: &'static str,
    text_inventory_table_id: &'static str,
    selected_by: &'static str,
    destination: &'static str,
}

#[derive(Debug, Serialize)]
struct PageLifetimeReport {
    right_fd_page_supplied_by_screen_roles: Vec<&'static str>,
    proven_inherited_by_screen_roles: Vec<&'static str>,
    phase_state_address: &'static str,
    phase_dispatcher: CodeRegionReport,
    right_fd_page_supply: CodeRegionReport,
    runtime_evidence: &'static str,
    runtime_variant_coverage: &'static str,
}

#[derive(Debug, Serialize)]
struct ImplementationBoundary {
    required_design: &'static str,
    rejected_shortcut: &'static str,
    preserved_original: [&'static str; 3],
    separate_screen_contract: &'static str,
}

pub struct UnitUiTextSummary {
    pub report_sha1: String,
    pub screen_role_count: usize,
    pub composer_count: usize,
    pub fixed_label_count: usize,
    pub translated_japanese_label_count: usize,
    pub command_label_count: usize,
    pub dynamic_pointer_count: usize,
    pub dynamic_unique_string_count: usize,
    pub provisional_hangul_slot_ceiling: usize,
    pub single_family_page_fit: &'static str,
}

pub fn analyze_unit_ui_text(source_path: &Path, report_path: &Path) -> Result<UnitUiTextSummary> {
    let source_rom = Rom::from_path(source_path)?;
    source_rom.verify_supported_japanese()?;
    let fixed_japanese_label_count = SUMMARY_AND_STATUS_LABEL_SPECS
        .iter()
        .chain(command_menu::COMMAND_LABEL_SPECS)
        .filter(|label| label.translation_scope == "japanese_only")
        .count();
    let glyph_budget = glyph_budget::analyze(source_rom.data(), fixed_japanese_label_count)?;
    let report = build_report(source_rom.prg(), glyph_budget)?;
    let report_bytes =
        serde_json::to_vec_pretty(&report).context("serialize unit-UI text report")?;
    let report_sha1 = sha1_hex(&report_bytes);

    if let Some(parent) = report_path.parent() {
        fs::create_dir_all(parent)
            .with_context(|| format!("create output directory {}", parent.display()))?;
    }
    fs::write(report_path, report_bytes)
        .with_context(|| format!("write {}", report_path.display()))?;

    Ok(UnitUiTextSummary {
        report_sha1,
        screen_role_count: report.screen_roles.len(),
        composer_count: report.composition_dispatch.relevant_states.len(),
        fixed_label_count: report.fixed_labels.len(),
        translated_japanese_label_count: report
            .fixed_labels
            .iter()
            .filter(|label| label.translation_scope == "japanese_only")
            .count(),
        command_label_count: report.command_menu.static_label_count,
        dynamic_pointer_count: report.glyph_budget.dynamic_pointer_count(),
        dynamic_unique_string_count: report.glyph_budget.dynamic_unique_string_count(),
        provisional_hangul_slot_ceiling: report.glyph_budget.provisional_hangul_slot_ceiling(),
        single_family_page_fit: report.glyph_budget.single_family_page_fit(),
    })
}

pub(crate) fn preserved_codes_for_unit_name_projection(source: &[u8]) -> Result<BTreeSet<u8>> {
    validate_code_regions(&source[HEADER_SIZE..HEADER_SIZE + PRG_SIZE])?;
    validate_fixed_labels(
        &source[HEADER_SIZE..HEADER_SIZE + PRG_SIZE],
        SUMMARY_AND_STATUS_LABEL_SPECS
            .iter()
            .chain(command_menu::COMMAND_LABEL_SPECS),
    )?;
    let mut preserved = crate::text_inventory::scoped_text_table_budgets(
        source,
        &["class-names", "item-names", "enemy-names"],
    )?
    .into_iter()
    .flat_map(|table| table.source_codes)
    .collect::<BTreeSet<_>>();
    preserved.extend(
        SUMMARY_AND_STATUS_LABEL_SPECS
            .iter()
            .chain(command_menu::COMMAND_LABEL_SPECS)
            .flat_map(|label| label.expected.iter().copied()),
    );
    Ok(preserved)
}

fn build_report(
    prg: &[u8],
    glyph_budget: glyph_budget::GlyphBudgetReport,
) -> Result<UnitUiTextReport> {
    ensure!(prg.len() == PRG_SIZE, "unexpected PRG size");
    validate_code_regions(prg)?;
    let command_menu = command_menu::analyze(prg)?;
    let fixed_labels = validate_fixed_labels(
        prg,
        SUMMARY_AND_STATUS_LABEL_SPECS
            .iter()
            .chain(command_menu::COMMAND_LABEL_SPECS),
    )?;
    ensure!(
        fixed_labels
            .iter()
            .filter(|label| label.translation_scope == "japanese_only")
            .count()
            == glyph_budget.fixed_japanese_label_count(),
        "unit-UI fixed-label count disagrees with the glyph budget"
    );

    Ok(UnitUiTextReport {
        schema: 1,
        source_sha1: EXPECTED_SOURCE_SHA1,
        screen_roles: vec![
            ScreenRoleReport {
                screen_role: "unit_summary",
                composers: vec!["unit_summary_header", "unit_summary_items"],
                inherited_content: vec![],
                translation_scope: "translate Japanese unit, class, level, and item text; preserve original Latin, digits, slash, and punctuation",
            },
            ScreenRoleReport {
                screen_role: "unit_command_menu",
                composers: vec!["unit_command_menu"],
                inherited_content: vec![],
                translation_scope: "translate all fifteen conditional Japanese command labels; do not infer one command set from the observed inventory-and-wait variant",
            },
            ScreenRoleReport {
                screen_role: "unit_status",
                composers: vec!["unit_status_stats"],
                inherited_content: vec!["unit_summary_header"],
                translation_scope: "translate nine Japanese stat labels; preserve inherited original Latin, digits, slash, and punctuation",
            },
        ],
        composition_dispatch: CompositionDispatchReport {
            state_address: "0x05E8",
            dispatcher: region_report(region("dispatch_composite_text_role_from_05e8")),
            relevant_states: vec![
                composition_state(0x04, "unit_summary_header", 0x826C),
                composition_state_with_report(
                    0x05,
                    "unit_command_menu",
                    command_menu.composer.clone(),
                ),
                composition_state(0x07, "unit_summary_items", 0x85BE),
                composition_state(0x0F, "unit_status_stats", 0x87F2),
            ],
        },
        shared_appenders: [
            "append_item_name_and_uses",
            "select_item_name",
            "select_unit_name",
            "select_class_name",
            "append_fixed_string",
            "append_number",
            "parse_composite_text",
        ]
        .into_iter()
        .map(|role| region_report(region(role)))
        .collect(),
        fixed_labels,
        dynamic_sources: vec![
            DynamicSourceReport {
                role: "unit_name",
                text_inventory_table_id: "unit-names or enemy-names",
                selected_by: "0B:8E88",
                destination: "composite buffer 0x0451,X",
            },
            DynamicSourceReport {
                role: "class_name",
                text_inventory_table_id: "class-names",
                selected_by: "0B:8EBA",
                destination: "composite buffer 0x0451,X",
            },
            DynamicSourceReport {
                role: "item_name",
                text_inventory_table_id: "item-names",
                selected_by: "0B:8E6B through 0B:875F",
                destination: "composite buffer 0x0451,X",
            },
        ],
        glyph_budget,
        command_menu,
        page_lifetime: PageLifetimeReport {
            right_fd_page_supplied_by_screen_roles: vec!["unit_summary", "unit_command_menu"],
            proven_inherited_by_screen_roles: vec!["unit_status"],
            phase_state_address: "0x05DE",
            phase_dispatcher: region_report(region("dispatch_unit_window_phase_from_05de")),
            right_fd_page_supply: region_report(region("open_unit_ui_right_fd_page_00")),
            runtime_evidence: "unit_summary and unit_command_menu entry each execute the right-FD supply; unit_summary-to-unit_status changes the left CHR pair without another right-FD supply",
            runtime_variant_coverage: "unit_summary, unit_command_menu, and unit_status each have runtime right 00/15, 00/18, and 00/19 evidence",
        },
        implementation_boundary: ImplementationBoundary {
            required_design: "budget one unit-UI family across four composition roles, fifteen command labels, and shared dynamic source tables while preserving every observed backing-page variant",
            rejected_shortcut: "per-unit or per-visible-string byte patches",
            preserved_original: ["Latin letters", "digits", "punctuation and slash"],
            separate_screen_contract: "automatic class_profile",
        },
    })
}

fn validate_code_regions(prg: &[u8]) -> Result<()> {
    for spec in CODE_REGION_SPECS {
        let offset = banked_prg_offset(UNIT_UI_BANK, spec.cpu_address)?;
        let end = offset + spec.expected.len();
        ensure!(
            end <= prg.len() && &prg[offset..end] == spec.expected,
            "unit-UI code contract mismatch for {} at bank 0B:{:04X}",
            spec.role,
            spec.cpu_address
        );
    }
    Ok(())
}

fn validate_fixed_labels<'a>(
    prg: &[u8],
    specs: impl IntoIterator<Item = &'a FixedLabelSpec>,
) -> Result<Vec<FixedLabelReport>> {
    specs
        .into_iter()
        .map(|spec| {
            let pointer_address = FIXED_STRING_POINTER_TABLE_ADDRESS + u16::from(spec.index) * 2;
            let pointer_offset = banked_prg_offset(UNIT_UI_BANK, pointer_address)?;
            let actual_pointer = u16::from_le_bytes([prg[pointer_offset], prg[pointer_offset + 1]]);
            ensure!(
                actual_pointer == spec.pointer,
                "fixed-label pointer mismatch for index 0x{:02X}: expected 0x{:04X}, found 0x{:04X}",
                spec.index,
                spec.pointer,
                actual_pointer
            );

            let source_offset = banked_prg_offset(UNIT_UI_BANK, spec.pointer)?;
            let source_end = source_offset + spec.expected.len();
            ensure!(
                source_end <= prg.len() && &prg[source_offset..source_end] == spec.expected,
                "fixed-label bytes mismatch for index 0x{:02X} at bank 0B:{:04X}",
                spec.index,
                spec.pointer
            );

            Ok(FixedLabelReport {
                index: spec.index,
                index_hex: format!("0x{:02X}", spec.index),
                source_text: spec.source_text,
                translation_scope: spec.translation_scope,
                pointer_table_address: pointer_address,
                pointer_table_address_hex: format!("0x{pointer_address:04X}"),
                source_address: spec.pointer,
                source_address_hex: format!("0x{:04X}", spec.pointer),
                source_codes: spec.expected.to_vec(),
                source_codes_hex: spec
                    .expected
                    .iter()
                    .map(|code| format!("0x{code:02X}"))
                    .collect(),
            })
        })
        .collect()
}

fn composition_state(
    state: u8,
    role: &'static str,
    handler_address: u16,
) -> CompositionStateReport {
    let handler = region(match role {
        "unit_summary_header" => "compose_unit_summary_header",
        "unit_summary_items" => "compose_unit_summary_items",
        "unit_status_stats" => "compose_unit_status_stats",
        _ => unreachable!("unknown unit-UI composition role"),
    });
    assert_eq!(handler.cpu_address, handler_address);
    CompositionStateReport {
        state,
        state_hex: format!("0x{state:02X}"),
        role,
        handler_address,
        handler_address_hex: format!("0x{handler_address:04X}"),
        handler: region_report(handler),
    }
}

fn composition_state_with_report(
    state: u8,
    role: &'static str,
    handler: CodeRegionReport,
) -> CompositionStateReport {
    CompositionStateReport {
        state,
        state_hex: format!("0x{state:02X}"),
        role,
        handler_address: handler.cpu_address,
        handler_address_hex: handler.cpu_address_hex.clone(),
        handler,
    }
}

fn region(role: &str) -> &'static CodeRegionSpec {
    CODE_REGION_SPECS
        .iter()
        .find(|spec| spec.role == role)
        .unwrap_or_else(|| panic!("missing code-region spec for {role}"))
}

fn region_report(spec: &CodeRegionSpec) -> CodeRegionReport {
    let prg_offset = banked_prg_offset(UNIT_UI_BANK, spec.cpu_address)
        .expect("static unit-UI region address must be valid");
    let file_offset = HEADER_SIZE + prg_offset;
    CodeRegionReport {
        role: spec.role,
        prg_bank: UNIT_UI_BANK,
        prg_bank_hex: format!("0x{UNIT_UI_BANK:02X}"),
        cpu_address: spec.cpu_address,
        cpu_address_hex: format!("0x{:04X}", spec.cpu_address),
        file_offset,
        file_offset_hex: format!("0x{file_offset:05X}"),
        byte_count: spec.expected.len(),
        bytes_hex: hex(spec.expected),
    }
}

pub(super) fn banked_prg_offset(bank: usize, cpu_address: u16) -> Result<usize> {
    ensure!(bank < PRG_SIZE / PRG_BANK_SIZE, "PRG bank out of range");
    ensure!(
        (SWITCHABLE_CPU_BASE..0xC000).contains(&cpu_address),
        "banked CPU address must be in 0x8000..0xBFFF"
    );
    Ok(bank * PRG_BANK_SIZE + usize::from(cpu_address - SWITCHABLE_CPU_BASE))
}

pub(super) fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|byte| format!("{byte:02X}")).collect()
}
