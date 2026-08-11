use std::collections::BTreeSet;

use anyhow::{Result, ensure};
use serde::Serialize;

use crate::{
    font_slots::{ACTIVE_HANGUL_SLOT_COUNT, reserved_font_codes},
    japanese_encoding::is_japanese_text_code,
    text_inventory::{TextTableBudget, scoped_text_table_budgets},
};

use super::target_glyphs::TargetGlyphBudget;

const UNIT_UI_TEXT_TABLE_IDS: [&str; 4] =
    ["class-names", "item-names", "unit-names", "enemy-names"];

#[derive(Debug, Serialize)]
pub(super) struct GlyphBudgetReport {
    dynamic_tables: Vec<DynamicTableBudgetReport>,
    dynamic_pointer_count: usize,
    dynamic_unique_string_count: usize,
    dynamic_referenced_text_byte_count: usize,
    dynamic_unique_text_storage_byte_count: usize,
    dynamic_distinct_source_code_count: usize,
    dynamic_japanese_source_code_count: usize,
    fixed_japanese_label_count: usize,
    globally_active_hangul_slot_count: usize,
    additional_preserved_unresolved_codes: Vec<u8>,
    additional_preserved_unresolved_codes_hex: Vec<String>,
    provisional_unit_ui_family_hangul_slot_ceiling: usize,
    fixed_text_workspace_sha1: String,
    unit_name_workspace_sha1: String,
    unit_ui_label_workspace_sha1: String,
    translation_inputs_review_complete: bool,
    target_korean_glyph_count: usize,
    summary_status_family_target_glyph_count: usize,
    command_family_target_glyph_count: usize,
    maximum_unit_or_enemy_name_glyph_count: usize,
    maximum_class_name_glyph_count: usize,
    maximum_item_name_glyph_count: usize,
    level_label_unique_glyph_count: usize,
    summary_status_label_unique_glyph_count: usize,
    single_family_page_fit: bool,
    summary_status_family_page_fit: bool,
    command_family_page_fit: bool,
    source_repertoire_is_target_glyph_budget: bool,
    screen_lifetimes: Vec<ScreenLifetimeBudgetReport>,
    mutable_content_boundary: &'static str,
}

impl GlyphBudgetReport {
    pub(super) fn dynamic_pointer_count(&self) -> usize {
        self.dynamic_pointer_count
    }

    pub(super) fn dynamic_unique_string_count(&self) -> usize {
        self.dynamic_unique_string_count
    }

    pub(super) fn fixed_japanese_label_count(&self) -> usize {
        self.fixed_japanese_label_count
    }

    pub(super) fn provisional_hangul_slot_ceiling(&self) -> usize {
        self.provisional_unit_ui_family_hangul_slot_ceiling
    }

    pub(super) fn single_family_page_fit(&self) -> bool {
        self.single_family_page_fit
    }
}

#[derive(Debug, Serialize)]
struct DynamicTableBudgetReport {
    id: &'static str,
    pointer_count: usize,
    unique_string_count: usize,
    referenced_text_byte_count: usize,
    unique_text_storage_byte_count: usize,
    max_entry_byte_count: usize,
    distinct_source_code_count: usize,
    source_codes: Vec<u8>,
    source_codes_hex: Vec<String>,
}

#[derive(Debug, Serialize)]
struct ScreenLifetimeBudgetReport {
    screen_role: &'static str,
    target_glyph_upper_bound: usize,
    preserved_active_source_code_upper_bound: usize,
    total_slot_upper_bound: usize,
    active_slot_count: usize,
    upper_bound_fits_active_page: bool,
    page_behavior: &'static str,
    visible_dynamic_content: &'static str,
    fixed_content: &'static str,
}

pub(super) fn analyze(
    source: &[u8],
    fixed_japanese_label_count: usize,
    target: TargetGlyphBudget,
) -> Result<GlyphBudgetReport> {
    let table_budgets = scoped_text_table_budgets(source, &UNIT_UI_TEXT_TABLE_IDS)?;
    build_glyph_budget(table_budgets, fixed_japanese_label_count, target)
}

fn build_glyph_budget(
    table_budgets: Vec<TextTableBudget>,
    fixed_japanese_label_count: usize,
    target: TargetGlyphBudget,
) -> Result<GlyphBudgetReport> {
    ensure!(
        !table_budgets.is_empty(),
        "unit-UI glyph budget needs at least one dynamic text table"
    );

    let dynamic_pointer_count = table_budgets.iter().map(|table| table.pointer_count).sum();
    let dynamic_unique_string_count = table_budgets
        .iter()
        .map(|table| table.unique_string_count)
        .sum();
    let dynamic_referenced_text_byte_count = table_budgets
        .iter()
        .map(|table| table.referenced_text_byte_count)
        .sum();
    let dynamic_unique_text_storage_byte_count = table_budgets
        .iter()
        .map(|table| table.unique_text_storage_byte_count)
        .sum();
    let source_codes = table_budgets
        .iter()
        .flat_map(|table| table.source_codes.iter().copied())
        .collect::<BTreeSet<_>>();
    let dynamic_japanese_source_code_count = source_codes
        .iter()
        .filter(|code| is_japanese_text_code(**code))
        .count();
    let reserved_codes = reserved_font_codes();
    let additional_preserved_unresolved_codes = source_codes
        .iter()
        .filter(|code| !is_japanese_text_code(**code) && !reserved_codes.contains(code))
        .copied()
        .collect::<Vec<_>>();
    let provisional_unit_ui_family_hangul_slot_ceiling = ACTIVE_HANGUL_SLOT_COUNT
        .checked_sub(additional_preserved_unresolved_codes.len())
        .ok_or_else(|| anyhow::anyhow!("unit-UI preserved codes exceed active Hangul slots"))?;
    let preserved_active_source_code_upper_bound = additional_preserved_unresolved_codes.len();
    let screen_upper_bound = |target_glyph_upper_bound: usize| -> Result<usize> {
        target_glyph_upper_bound
            .checked_add(preserved_active_source_code_upper_bound)
            .ok_or_else(|| anyhow::anyhow!("unit-UI screen slot upper bound overflow"))
    };
    let summary_slot_upper_bound = screen_upper_bound(target.summary_target_glyph_upper_bound)?;
    let status_slot_upper_bound = screen_upper_bound(target.status_target_glyph_upper_bound)?;
    let command_slot_upper_bound = screen_upper_bound(target.command_target_glyph_upper_bound)?;
    let all_family_slot_count = screen_upper_bound(target.all_family_unique_glyph_count)?;
    let summary_status_family_slot_count =
        screen_upper_bound(target.summary_status_family_unique_glyph_count)?;
    let command_family_slot_count = screen_upper_bound(target.command_family_unique_glyph_count)?;

    let dynamic_tables = table_budgets
        .into_iter()
        .map(|table| {
            let source_codes = table.source_codes.into_iter().collect::<Vec<_>>();
            DynamicTableBudgetReport {
                id: table.id,
                pointer_count: table.pointer_count,
                unique_string_count: table.unique_string_count,
                referenced_text_byte_count: table.referenced_text_byte_count,
                unique_text_storage_byte_count: table.unique_text_storage_byte_count,
                max_entry_byte_count: table.max_entry_byte_count,
                distinct_source_code_count: source_codes.len(),
                source_codes_hex: source_codes
                    .iter()
                    .map(|code| format!("0x{code:02X}"))
                    .collect(),
                source_codes,
            }
        })
        .collect();

    Ok(GlyphBudgetReport {
        dynamic_tables,
        dynamic_pointer_count,
        dynamic_unique_string_count,
        dynamic_referenced_text_byte_count,
        dynamic_unique_text_storage_byte_count,
        dynamic_distinct_source_code_count: source_codes.len(),
        dynamic_japanese_source_code_count,
        fixed_japanese_label_count,
        globally_active_hangul_slot_count: ACTIVE_HANGUL_SLOT_COUNT,
        additional_preserved_unresolved_codes_hex: additional_preserved_unresolved_codes
            .iter()
            .map(|code| format!("0x{code:02X}"))
            .collect(),
        additional_preserved_unresolved_codes,
        provisional_unit_ui_family_hangul_slot_ceiling,
        fixed_text_workspace_sha1: target.fixed_text_workspace_sha1,
        unit_name_workspace_sha1: target.unit_name_workspace_sha1,
        unit_ui_label_workspace_sha1: target.unit_ui_label_workspace_sha1,
        translation_inputs_review_complete: target.translation_inputs_review_complete,
        target_korean_glyph_count: target.all_family_unique_glyph_count,
        summary_status_family_target_glyph_count: target.summary_status_family_unique_glyph_count,
        command_family_target_glyph_count: target.command_family_unique_glyph_count,
        maximum_unit_or_enemy_name_glyph_count: target.maximum_unit_or_enemy_name_glyph_count,
        maximum_class_name_glyph_count: target.maximum_class_name_glyph_count,
        maximum_item_name_glyph_count: target.maximum_item_name_glyph_count,
        level_label_unique_glyph_count: target.level_label_unique_glyph_count,
        summary_status_label_unique_glyph_count: target.summary_status_label_unique_glyph_count,
        single_family_page_fit: all_family_slot_count <= ACTIVE_HANGUL_SLOT_COUNT,
        summary_status_family_page_fit: summary_status_family_slot_count
            <= ACTIVE_HANGUL_SLOT_COUNT,
        command_family_page_fit: command_family_slot_count <= ACTIVE_HANGUL_SLOT_COUNT,
        source_repertoire_is_target_glyph_budget: false,
        screen_lifetimes: vec![
            ScreenLifetimeBudgetReport {
                screen_role: "unit_summary",
                target_glyph_upper_bound: target.summary_target_glyph_upper_bound,
                preserved_active_source_code_upper_bound,
                total_slot_upper_bound: summary_slot_upper_bound,
                active_slot_count: ACTIVE_HANGUL_SLOT_COUNT,
                upper_bound_fits_active_page: summary_slot_upper_bound <= ACTIVE_HANGUL_SLOT_COUNT,
                page_behavior: "unit_summary supplies the right FD page and unit_status inherits it",
                visible_dynamic_content: "one playable-or-enemy unit name, one class name, and up to four current item names",
                fixed_content: "level label; original Latin, digits, slash, and punctuation stay preserved",
            },
            ScreenLifetimeBudgetReport {
                screen_role: "unit_status",
                target_glyph_upper_bound: target.status_target_glyph_upper_bound,
                preserved_active_source_code_upper_bound,
                total_slot_upper_bound: status_slot_upper_bound,
                active_slot_count: ACTIVE_HANGUL_SLOT_COUNT,
                upper_bound_fits_active_page: status_slot_upper_bound <= ACTIVE_HANGUL_SLOT_COUNT,
                page_behavior: "unit_status inherits the page selected by unit_summary",
                visible_dynamic_content: "one playable-or-enemy unit name and one class name",
                fixed_content: "level and nine stat labels; original Latin, digits, slash, and punctuation stay preserved",
            },
            ScreenLifetimeBudgetReport {
                screen_role: "unit_command_menu",
                target_glyph_upper_bound: target.command_target_glyph_upper_bound,
                preserved_active_source_code_upper_bound,
                total_slot_upper_bound: command_slot_upper_bound,
                active_slot_count: ACTIVE_HANGUL_SLOT_COUNT,
                upper_bound_fits_active_page: command_slot_upper_bound <= ACTIVE_HANGUL_SLOT_COUNT,
                page_behavior: "entry supplies a new right FD page independently of unit_status inheritance",
                visible_dynamic_content: "a condition-selected subset of fifteen fixed command labels",
                fixed_content: "all fifteen command labels belong to the safe upper bound even though only six have runtime display evidence",
            },
        ],
        mutable_content_boundary: "the complete and summary-status family unions exceed one page even though each screen upper bound fits; unit_summary therefore needs a content-keyed page or runtime glyph upload, unit_status must inherit that page, and unit_command_menu can use an independent static page",
    })
}

#[cfg(test)]
pub(super) fn fixture_report(fixed_japanese_label_count: usize) -> GlyphBudgetReport {
    build_glyph_budget(
        vec![TextTableBudget {
            id: "fixture-names",
            pointer_count: 1,
            unique_string_count: 1,
            referenced_text_byte_count: 1,
            unique_text_storage_byte_count: 1,
            max_entry_byte_count: 1,
            source_codes: [0x00].into_iter().collect(),
        }],
        fixed_japanese_label_count,
        TargetGlyphBudget {
            fixed_text_workspace_sha1: "fixed".to_owned(),
            unit_name_workspace_sha1: "units".to_owned(),
            unit_ui_label_workspace_sha1: "labels".to_owned(),
            translation_inputs_review_complete: false,
            all_family_unique_glyph_count: 4,
            summary_status_family_unique_glyph_count: 3,
            command_family_unique_glyph_count: 1,
            maximum_unit_or_enemy_name_glyph_count: 1,
            maximum_class_name_glyph_count: 1,
            maximum_item_name_glyph_count: 1,
            level_label_unique_glyph_count: 1,
            summary_status_label_unique_glyph_count: 1,
            summary_target_glyph_upper_bound: 7,
            status_target_glyph_upper_bound: 3,
            command_target_glyph_upper_bound: 1,
        },
    )
    .expect("static unit-UI glyph-budget fixture must be valid")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn table(id: &'static str, source_codes: &[u8]) -> TextTableBudget {
        TextTableBudget {
            id,
            pointer_count: 2,
            unique_string_count: 1,
            referenced_text_byte_count: 6,
            unique_text_storage_byte_count: 3,
            max_entry_byte_count: 3,
            source_codes: source_codes.iter().copied().collect(),
        }
    }

    #[test]
    fn keeps_source_repertoire_separate_from_target_glyph_count() {
        let report = build_glyph_budget(
            vec![
                table("unit-names", &[0x00, 0x6A, 0xA5]),
                table("item-names", &[0x00, 0x84, 0xAB]),
            ],
            25,
            TargetGlyphBudget {
                fixed_text_workspace_sha1: "fixed".to_owned(),
                unit_name_workspace_sha1: "units".to_owned(),
                unit_ui_label_workspace_sha1: "labels".to_owned(),
                translation_inputs_review_complete: false,
                all_family_unique_glyph_count: 229,
                summary_status_family_unique_glyph_count: 218,
                command_family_unique_glyph_count: 30,
                maximum_unit_or_enemy_name_glyph_count: 6,
                maximum_class_name_glyph_count: 4,
                maximum_item_name_glyph_count: 6,
                level_label_unique_glyph_count: 2,
                summary_status_label_unique_glyph_count: 20,
                summary_target_glyph_upper_bound: 36,
                status_target_glyph_upper_bound: 30,
                command_target_glyph_upper_bound: 30,
            },
        )
        .unwrap();

        assert_eq!(report.dynamic_pointer_count, 4);
        assert_eq!(report.dynamic_unique_string_count, 2);
        assert_eq!(report.dynamic_distinct_source_code_count, 5);
        assert_eq!(report.dynamic_japanese_source_code_count, 3);
        assert_eq!(report.additional_preserved_unresolved_codes, [0xA5]);
        assert_eq!(
            report.provisional_unit_ui_family_hangul_slot_ceiling,
            ACTIVE_HANGUL_SLOT_COUNT - 1
        );
        assert_eq!(report.target_korean_glyph_count, 229);
        assert!(!report.single_family_page_fit);
        assert!(!report.summary_status_family_page_fit);
        assert!(report.command_family_page_fit);
        assert_eq!(report.screen_lifetimes[0].total_slot_upper_bound, 37);
        assert_eq!(report.screen_lifetimes[1].total_slot_upper_bound, 31);
        assert_eq!(report.screen_lifetimes[2].total_slot_upper_bound, 31);
        assert!(!report.source_repertoire_is_target_glyph_budget);
    }

    #[test]
    fn rejects_an_empty_dynamic_family() {
        let error = build_glyph_budget(
            Vec::new(),
            25,
            TargetGlyphBudget {
                fixed_text_workspace_sha1: "fixed".to_owned(),
                unit_name_workspace_sha1: "units".to_owned(),
                unit_ui_label_workspace_sha1: "labels".to_owned(),
                translation_inputs_review_complete: false,
                all_family_unique_glyph_count: 0,
                summary_status_family_unique_glyph_count: 0,
                command_family_unique_glyph_count: 0,
                maximum_unit_or_enemy_name_glyph_count: 0,
                maximum_class_name_glyph_count: 0,
                maximum_item_name_glyph_count: 0,
                level_label_unique_glyph_count: 0,
                summary_status_label_unique_glyph_count: 0,
                summary_target_glyph_upper_bound: 0,
                status_target_glyph_upper_bound: 0,
                command_target_glyph_upper_bound: 0,
            },
        )
        .unwrap_err()
        .to_string();
        assert!(error.contains("at least one dynamic text table"));
    }
}
