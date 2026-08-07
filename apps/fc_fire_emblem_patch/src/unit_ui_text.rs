use std::{fs, path::Path};

use anyhow::{Context, Result, ensure};
use serde::Serialize;

use crate::{
    rom::{EXPECTED_SOURCE_SHA1, HEADER_SIZE, PRG_SIZE, Rom},
    sha1_hex,
};

mod command_menu;
mod glyph_budget;

const PRG_BANK_SIZE: usize = 16 * 1024;
pub(super) const UNIT_UI_BANK: usize = 0x0B;
const SWITCHABLE_CPU_BASE: u16 = 0x8000;
const FIXED_STRING_POINTER_TABLE_ADDRESS: u16 = 0x8FC2;

struct CodeRegionSpec {
    role: &'static str,
    cpu_address: u16,
    expected: &'static [u8],
}

const CODE_REGION_SPECS: &[CodeRegionSpec] = &[
    CodeRegionSpec {
        role: "dispatch_composite_text_role_from_05e8",
        cpu_address: 0x8000,
        expected: &[
            0xAD, 0xE8, 0x05, 0x20, 0x4C, 0xC3, 0x54, 0x80, 0x88, 0x80, 0xF6, 0x80, 0x87, 0x81,
            0x6C, 0x82, 0xE3, 0x82, 0xF4, 0x84, 0xBE, 0x85, 0xE5, 0x85, 0x13, 0x86, 0xC1, 0x86,
            0x7D, 0x86, 0x85, 0x87, 0xE6, 0x8B, 0x8F, 0x8C, 0xF2, 0x87, 0x6A, 0x88, 0x91, 0x88,
            0xC4, 0x88, 0xD5, 0x88, 0x23, 0x89, 0x65, 0x89, 0xDB, 0x89, 0xFD, 0x89, 0x25, 0x8A,
            0xC4, 0x87, 0x47, 0x8A, 0xA1, 0x8A, 0xE6, 0x8A, 0x08, 0x8B, 0x3A, 0x8B, 0x80, 0x8B,
            0xB9, 0x8B, 0xE8, 0x8C, 0x4B, 0x8D, 0x98, 0x8D, 0xC6, 0x8D, 0xDB, 0x81, 0x0F, 0x8E,
        ],
    },
    CodeRegionSpec {
        role: "compose_unit_summary_header",
        cpu_address: 0x826C,
        expected: &[
            0xA9, 0x0A, 0x8D, 0xD0, 0x05, 0xA9, 0x0C, 0x8D, 0xCF, 0x05, 0x20, 0xC8, 0x97, 0x20,
            0x3C, 0x8E, 0xA9, 0xF4, 0x85, 0x74, 0xA9, 0x76, 0x85, 0x75, 0x20, 0x88, 0x8E, 0xA0,
            0x00, 0xB1, 0x74, 0xC9, 0x01, 0xD0, 0x18, 0xAD, 0x7E, 0x76, 0xF0, 0x13, 0xCA, 0xA9,
            0xFF, 0x9D, 0x51, 0x04, 0xE8, 0xA9, 0xBF, 0x9D, 0x51, 0x04, 0xE8, 0xA9, 0xED, 0x9D,
            0x51, 0x04, 0xE8, 0x20, 0xBA, 0x8E, 0xA9, 0x08, 0x20, 0xEE, 0x8E, 0xA0, 0x02, 0xB1,
            0x74, 0x85, 0x00, 0x20, 0x0E, 0x8F, 0xA9, 0xED, 0x9D, 0x51, 0x04, 0xE8, 0xA9, 0x09,
            0x20, 0xEE, 0x8E, 0x20, 0xD7, 0x8E, 0xA9, 0xAD, 0x9D, 0x51, 0x04, 0xE8, 0xA0, 0x04,
            0xB1, 0x74, 0x85, 0x00, 0x20, 0x0E, 0x8F, 0xA9, 0xED, 0x9D, 0x51, 0x04, 0xE8, 0xA9,
            0xEF, 0x9D, 0x51, 0x04, 0x4C, 0x39, 0x8F,
        ],
    },
    CodeRegionSpec {
        role: "compose_unit_summary_items",
        cpu_address: 0x85BE,
        expected: &[
            0xA9, 0x0E, 0x8D, 0xCF, 0x05, 0x20, 0xC8, 0x97, 0x20, 0x3C, 0x8E, 0xA0, 0x13, 0xB1,
            0x74, 0x84, 0x12, 0xF0, 0x06, 0x20, 0x5F, 0x87, 0x38, 0xB0, 0x01, 0x18, 0x6E, 0xEB,
            0x05, 0xA4, 0x12, 0xC8, 0xC0, 0x17, 0xD0, 0xE9, 0x4C, 0x95, 0x85,
        ],
    },
    CodeRegionSpec {
        role: "compose_unit_status_stats",
        cpu_address: 0x87F2,
        expected: &[
            0xA9, 0x0E, 0x8D, 0xCF, 0x05, 0xA9, 0x14, 0x8D, 0xD0, 0x05, 0xA9, 0x10, 0x85, 0x70,
            0xAA, 0xA5, 0x8F, 0x38, 0xE5, 0x64, 0xC9, 0x08, 0x90, 0x02, 0xA2, 0x80, 0x86, 0x71,
            0x20, 0x3C, 0x8E, 0xA0, 0x07, 0x84, 0x12, 0xC0, 0x0E, 0xF0, 0x18, 0x98, 0x38, 0xE9,
            0x07, 0x20, 0xEE, 0x8E, 0xA4, 0x12, 0xB1, 0x74, 0x29, 0x7F, 0x85, 0x00, 0x20, 0x0E,
            0x8F, 0xA9, 0xED, 0x9D, 0x51, 0x04, 0xE8, 0xA4, 0x12, 0xC8, 0xC0, 0x0F, 0xD0, 0xDB,
            0xA9, 0x27, 0x20, 0xEE, 0x8E, 0xA0, 0x0F, 0xB1, 0x74, 0x29, 0x7F, 0x85, 0x00, 0x20,
            0x0E, 0x8F, 0xA9, 0xED, 0x9D, 0x51, 0x04, 0xE8, 0xA9, 0x28, 0x20, 0xEE, 0x8E, 0xA0,
            0x05, 0xB1, 0x74, 0x85, 0x00, 0x20, 0x0E, 0x8F, 0xA9, 0xED, 0x9D, 0x51, 0x04, 0xE8,
            0xA9, 0xEF, 0x9D, 0x51, 0x04, 0x4C, 0x39, 0x8F,
        ],
    },
    CodeRegionSpec {
        role: "append_item_name_and_uses",
        cpu_address: 0x875F,
        expected: &[
            0x20, 0x6B, 0x8E, 0xA0, 0x00, 0xB1, 0x74, 0x30, 0x1C, 0xA4, 0x12, 0xC8, 0xC8, 0xC8,
            0xC8, 0xB1, 0x74, 0x30, 0x12, 0x85, 0x00, 0xCA, 0xA9, 0xAD, 0x9D, 0x51, 0x04, 0xE8,
            0x20, 0x0E, 0x8F, 0xA9, 0xED, 0x9D, 0x51, 0x04, 0xE8, 0x60,
        ],
    },
    CodeRegionSpec {
        role: "select_item_name",
        cpu_address: 0x8E6B,
        expected: &[
            0xA4, 0x12, 0xB1, 0x74, 0x38, 0xE9, 0x01, 0x0A, 0xA8, 0xB9, 0xD5, 0xDA, 0x85, 0x00,
            0xB9, 0xD6, 0xDA, 0x85, 0x01, 0x20, 0xFA, 0x8E, 0xA9, 0xED, 0x9D, 0x51, 0x04, 0xE8,
            0x60,
        ],
    },
    CodeRegionSpec {
        role: "select_unit_name",
        cpu_address: 0x8E88,
        expected: &[
            0xA0, 0x00, 0xB1, 0x74, 0x29, 0x7F, 0xA8, 0x88, 0x98, 0x0A, 0xA8, 0xAD, 0xF4, 0x76,
            0x29, 0x80, 0xF0, 0x0C, 0xB9, 0xA4, 0xDF, 0x85, 0x00, 0xB9, 0xA5, 0xDF, 0x85, 0x01,
            0xD0, 0x0A, 0xB9, 0x2B, 0xDE, 0x85, 0x00, 0xB9, 0x2C, 0xDE, 0x85, 0x01, 0x20, 0xFA,
            0x8E, 0xA9, 0xED, 0x9D, 0x51, 0x04, 0xE8, 0x60,
        ],
    },
    CodeRegionSpec {
        role: "select_class_name",
        cpu_address: 0x8EBA,
        expected: &[
            0xA0, 0x01, 0xB1, 0x74, 0xA8, 0x88, 0x98, 0x0A, 0xA8, 0xB9, 0x1F, 0xDA, 0x85, 0x00,
            0xB9, 0x20, 0xDA, 0x85, 0x01, 0x20, 0xFA, 0x8E, 0xA9, 0xED, 0x9D, 0x51, 0x04, 0xE8,
            0x60,
        ],
    },
    CodeRegionSpec {
        role: "append_fixed_string",
        cpu_address: 0x8EEE,
        expected: &[
            0x0A, 0xA8, 0xB9, 0xC2, 0x8F, 0x85, 0x00, 0xB9, 0xC3, 0x8F, 0x85, 0x01, 0xA0, 0x00,
            0xB1, 0x00, 0xC9, 0xEF, 0xF0, 0x0B, 0x9D, 0x51, 0x04, 0xE8, 0xC9, 0xED, 0xF0, 0x03,
            0xC8, 0xD0, 0xEF, 0x60,
        ],
    },
    CodeRegionSpec {
        role: "append_number",
        cpu_address: 0x8F0E,
        expected: &[
            0x18, 0x8A, 0x69, 0x51, 0x85, 0x08, 0xA9, 0x04, 0x69, 0x00, 0x85, 0x09, 0xA9, 0x02,
            0x85, 0x01, 0xE8, 0xE8, 0x4C, 0xEA, 0xC7, 0x18, 0x8A, 0x69, 0x51, 0x85, 0x08, 0xA9,
            0x04, 0x69, 0x00, 0x85, 0x09, 0xA9, 0x03, 0x85, 0x01, 0xE8, 0xE8, 0xE8, 0x4C, 0xEA,
            0xC7,
        ],
    },
    CodeRegionSpec {
        role: "parse_composite_text",
        cpu_address: 0x8F39,
        expected: &[
            0xA9, 0x51, 0x85, 0x76, 0xA9, 0x04, 0x85, 0x77, 0xA2, 0x00, 0x8E, 0x10, 0x03, 0x8E,
            0x50, 0x04,
        ],
    },
    CodeRegionSpec {
        role: "dispatch_unit_window_phase_from_05de",
        cpu_address: 0x9251,
        expected: &[
            0xAD, 0xDE, 0x05, 0x20, 0x4C, 0xC3, 0x3D, 0xC7, 0x65, 0x92, 0xA2, 0x92, 0xC9, 0x92,
            0xFB, 0x92, 0x33, 0x93, 0xE0, 0x93,
        ],
    },
    CodeRegionSpec {
        role: "open_unit_ui_right_fd_page_00",
        cpu_address: 0x927B,
        expected: &[
            0xA9, 0x06, 0x85, 0x44, 0x20, 0xFA, 0xC9, 0x20, 0xF5, 0xE6, 0x20, 0x0D, 0xC7, 0xA9,
            0x00, 0x20, 0xBE, 0xC9,
        ],
    },
];

pub(super) struct FixedLabelSpec {
    pub(super) index: u8,
    source_text: &'static str,
    translation_scope: &'static str,
    pointer: u16,
    expected: &'static [u8],
}

const SUMMARY_AND_STATUS_LABEL_SPECS: &[FixedLabelSpec] = &[
    fixed_label(
        0x00,
        "ちから",
        "japanese_only",
        0x9052,
        &[0xFF, 0xFF, 0xFF, 0xFF, 0x11, 0x05, 0x28, 0x8D, 0xEF],
    ),
    fixed_label(
        0x01,
        "わざ",
        "japanese_only",
        0x905B,
        &[0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0x2D, 0x0A, 0x0F, 0x8D, 0xEF],
    ),
    fixed_label(
        0x02,
        "ぶきレベル",
        "japanese_only",
        0x9065,
        &[
            0xFF, 0xFF, 0x1C, 0x0F, 0x06, 0x5B, 0x4D, 0x0F, 0x5A, 0x8D, 0xEF,
        ],
    ),
    fixed_label(
        0x03,
        "すばやさ",
        "japanese_only",
        0x9070,
        &[0xFF, 0xFF, 0xFF, 0x0C, 0x1A, 0x0F, 0x25, 0x0A, 0x8D, 0xEF],
    ),
    fixed_label(
        0x04,
        "うんのよさ",
        "japanese_only",
        0x907A,
        &[0xFF, 0xFF, 0x02, 0x2F, 0x19, 0x27, 0x0A, 0x8D, 0xEF],
    ),
    fixed_label(
        0x05,
        "しゅびりょく",
        "japanese_only",
        0x9083,
        &[0xFF, 0x0B, 0x86, 0x1B, 0x0F, 0x29, 0x87, 0x07, 0x8D, 0xEF],
    ),
    fixed_label(
        0x06,
        "いどうりょく",
        "japanese_only",
        0x908D,
        &[0xFF, 0x01, 0x14, 0x0F, 0x02, 0x29, 0x87, 0x07, 0x8D, 0xEF],
    ),
    fixed_label(
        0x08,
        "レベル",
        "japanese_only",
        0x90A0,
        &[0x5B, 0x4D, 0x0F, 0x5A, 0x8D, 0xEF],
    ),
    fixed_label(
        0x09,
        "HP",
        "preserve_original_latin",
        0x90A6,
        &[0x71, 0x79, 0x8D, 0xEF],
    ),
    fixed_label(
        0x27,
        "まほうぼうぎょ",
        "japanese_only",
        0x915C,
        &[
            0x20, 0x1E, 0x02, 0x1E, 0x0F, 0x02, 0x06, 0x0F, 0x87, 0x8D, 0xEF,
        ],
    ),
    fixed_label(
        0x28,
        "けいけんち",
        "japanese_only",
        0x9167,
        &[0xFF, 0xFF, 0x08, 0x01, 0x08, 0x2F, 0x11, 0x8D, 0xEF],
    ),
];

pub(super) const fn fixed_label(
    index: u8,
    source_text: &'static str,
    translation_scope: &'static str,
    pointer: u16,
    expected: &'static [u8],
) -> FixedLabelSpec {
    FixedLabelSpec {
        index,
        source_text,
        translation_scope,
        pointer,
        expected,
    }
}

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
    unresolved_runtime_variant: &'static str,
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
            unresolved_runtime_variant: "unit_command_menu has runtime right 00/15 and 00/18 evidence; a possible 00/19 backing-page variant is not yet observed",
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

#[cfg(test)]
mod tests {
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
        assert_eq!(report.command_menu.runtime_observed_label_count, 2);
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
}
