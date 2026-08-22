use std::collections::BTreeSet;

use anyhow::{Context, Result, ensure};
use serde::Serialize;

use crate::{
    battle_runtime_state::BATTLE_RUNTIME_STATE,
    dialogue_inventory::{
        TranslationSurfaceDialogueTableBinding,
        aggregate_translation_surface_dialogue_literal_inventory,
    },
    rom::Rom,
    source_literals::{
        SourceLiteralCodeClass, TranslationSurfaceLiteralInventory, classify_source_literal_code,
    },
    text_inventory::scoped_text_table_budgets,
};

use super::{CodeLocation, location};

#[derive(Debug, Serialize)]
pub(super) struct BattleAnimationTranslationSurface {
    screen_role: &'static str,
    sound_test_outer_phase_address: u16,
    sound_test_outer_phase_address_hex: &'static str,
    shared_engine_outer_phase: u8,
    shared_engine_outer_phase_hex: &'static str,
    shared_engine_entry: CodeLocation,
    shared_phase_address: u16,
    shared_phase_address_hex: String,
    shared_phase_count: usize,
    terminal_shared_phase: u8,
    terminal_shared_phase_hex: String,
    repeated_outer_phase: u8,
    repeated_outer_phase_hex: &'static str,
    fixed_text_tables: Vec<BattleTextTableBinding>,
    fixed_text_code_union: SourceCodePartition,
    dialogue_table_id: &'static str,
    dialogue_literal_inventory: TranslationSurfaceLiteralInventory,
    dialogue_selector_address: u16,
    dialogue_selector_address_hex: String,
    dialogue_table_set_address: u16,
    dialogue_table_set_address_hex: String,
    message_template_pointer_count: usize,
    message_template_data_byte_count: usize,
    message_template_loader: CodeLocation,
    forecast_label: CodeLocation,
    writer_roles: &'static [&'static str],
    translation_handling: &'static str,
    unresolved: &'static [&'static str],
}

#[derive(Debug, Serialize)]
struct BattleTextTableBinding {
    table_id: &'static str,
    table_cpu_address: u16,
    table_cpu_address_hex: &'static str,
    pointer_count: usize,
    unique_string_count: usize,
    referenced_text_byte_count: usize,
    unique_text_storage_byte_count: usize,
    max_entry_byte_count: usize,
    distinct_source_code_count: usize,
    source_code_partition: SourceCodePartition,
    writer_role: &'static str,
    translation_handling: &'static str,
}

#[derive(Debug, Serialize)]
struct SourceCodePartition {
    distinct_source_code_count: usize,
    japanese_codes_hex: Vec<String>,
    preserved_original_codes_hex: Vec<String>,
    layout_codes_hex: Vec<String>,
    unresolved_codes_hex: Vec<String>,
}

pub(super) fn bind_battle_animation_translation_surface(
    rom: &Rom,
    dialogue_tables: &[TranslationSurfaceDialogueTableBinding],
) -> Result<BattleAnimationTranslationSurface> {
    let runtime = BATTLE_RUNTIME_STATE;
    let selector = runtime.dialogue_selector_projection;
    let battle_dialogue = dialogue_tables
        .iter()
        .find(|table| table.table_id == "battle-dialogue")
        .context("battle-dialogue surface binding is absent")?;
    ensure!(
        battle_dialogue.pointer_count == 65
            && battle_dialogue.unique_target_count == 28
            && battle_dialogue.separate_loader_cpu_address == Some(0x8000)
            && battle_dialogue.proven_record_count == Some(28)
            && battle_dialogue.unique_record_storage_byte_count == Some(1152)
            && battle_dialogue.unreferenced_record_count == Some(1),
        "battle-dialogue surface structure changed"
    );

    const TABLE_IDS: [&str; 5] = [
        "unit-names",
        "enemy-names",
        "class-names",
        "item-names",
        "terrain-names",
    ];
    let budgets = scoped_text_table_budgets(rom.data(), &TABLE_IDS)?;
    let mut fixed_text_code_union = BTreeSet::new();
    let fixed_text_tables = budgets
        .into_iter()
        .map(|budget| {
            let (table_cpu_address, table_cpu_address_hex, writer_role) = match budget.id {
                "unit-names" => (0xDE2B, "0xDE2B", "compose_battle_unit_name"),
                "enemy-names" => (0xDFA4, "0xDFA4", "compose_battle_unit_name"),
                "class-names" => (0xDA1F, "0xDA1F", "compose_battle_class_name"),
                "item-names" => (0xDAD5, "0xDAD5", "compose_battle_item_name"),
                "terrain-names" => (0xE5F1, "0xE5F1", "compose_battle_terrain_name"),
                other => return Err(anyhow::anyhow!("unexpected battle text table {other}")),
            };
            fixed_text_code_union.extend(budget.source_codes.iter().copied());
            let source_code_partition =
                partition_source_codes(budget.source_codes.iter().copied());
            Ok(BattleTextTableBinding {
                table_id: budget.id,
                table_cpu_address,
                table_cpu_address_hex,
                pointer_count: budget.pointer_count,
                unique_string_count: budget.unique_string_count,
                referenced_text_byte_count: budget.referenced_text_byte_count,
                unique_text_storage_byte_count: budget.unique_text_storage_byte_count,
                max_entry_byte_count: budget.max_entry_byte_count,
                distinct_source_code_count: budget.source_codes.len(),
                source_code_partition,
                writer_role,
                translation_handling: "translate Japanese glyph bytes only; preserve original Latin and digit positions",
            })
        })
        .collect::<Result<Vec<_>>>()?;
    ensure!(
        fixed_text_tables.len() == TABLE_IDS.len(),
        "battle fixed-text table coverage changed"
    );
    let dialogue_literal_inventory = aggregate_translation_surface_dialogue_literal_inventory(
        rom.data(),
        dialogue_tables,
        &["battle-dialogue"],
    )?;

    Ok(BattleAnimationTranslationSurface {
        screen_role: "battle_animation",
        sound_test_outer_phase_address: 0x7730,
        sound_test_outer_phase_address_hex: "0x7730",
        shared_engine_outer_phase: 0x05,
        shared_engine_outer_phase_hex: "0x05",
        shared_engine_entry: location(0x05, 0x8161),
        shared_phase_address: runtime.shared_phase_address,
        shared_phase_address_hex: format!("0x{:04X}", runtime.shared_phase_address),
        shared_phase_count: usize::from(runtime.shared_phase_count),
        terminal_shared_phase: runtime.shared_phase_count - 1,
        terminal_shared_phase_hex: format!("0x{:02X}", runtime.shared_phase_count - 1),
        repeated_outer_phase: 0x03,
        repeated_outer_phase_hex: "0x03",
        fixed_text_tables,
        fixed_text_code_union: partition_source_codes(fixed_text_code_union),
        dialogue_table_id: "battle-dialogue",
        dialogue_literal_inventory,
        dialogue_selector_address: selector.observed_selector_address,
        dialogue_selector_address_hex: format!("0x{:04X}", selector.observed_selector_address),
        dialogue_table_set_address: runtime.dialogue_table_set_address,
        dialogue_table_set_address_hex: format!("0x{:04X}", runtime.dialogue_table_set_address),
        message_template_pointer_count: 22,
        message_template_data_byte_count: 0x10B,
        message_template_loader: location(0x07, 0x82DC),
        forecast_label: location(0x05, 0x96B6),
        writer_roles: &[
            "select_battle_unit_name_source",
            "compose_battle_unit_name",
            "compose_battle_class_name",
            "compose_battle_item_name",
            "compose_battle_item_and_dialogue",
            "override_battle_dialogue_selector",
            "compose_battle_dialogue",
            "compose_battle_dialogue_continuation_one",
            "compose_battle_dialogue_continuation_two",
            "compose_battle_class_and_dialogue",
            "compose_battle_terrain_name",
            "select_battle_message_template",
        ],
        translation_handling: "the debug route reuses the gameplay battle engine and its shared text sources; translate Japanese names, labels, and messages while preserving LV, HIT, EXP, HP bars, percentages, and digits",
        unresolved: &[
            "run the final translated Hangul battle-page regression across actual Korean name, class, item, and message combinations",
        ],
    })
}

fn partition_source_codes(codes: impl IntoIterator<Item = u8>) -> SourceCodePartition {
    let mut japanese_codes = BTreeSet::new();
    let mut preserved_original_codes = BTreeSet::new();
    let mut layout_codes = BTreeSet::new();
    let mut unresolved_codes = BTreeSet::new();
    for code in codes {
        match classify_source_literal_code(code) {
            SourceLiteralCodeClass::Japanese => {
                japanese_codes.insert(code);
            }
            SourceLiteralCodeClass::PreservedOriginal => {
                preserved_original_codes.insert(code);
            }
            SourceLiteralCodeClass::Layout => {
                layout_codes.insert(code);
            }
            SourceLiteralCodeClass::Unresolved => {
                unresolved_codes.insert(code);
            }
        }
    }
    let distinct_source_code_count = japanese_codes.len()
        + preserved_original_codes.len()
        + layout_codes.len()
        + unresolved_codes.len();

    SourceCodePartition {
        distinct_source_code_count,
        japanese_codes_hex: hex_codes(japanese_codes),
        preserved_original_codes_hex: hex_codes(preserved_original_codes),
        layout_codes_hex: hex_codes(layout_codes),
        unresolved_codes_hex: hex_codes(unresolved_codes),
    }
}

fn hex_codes(codes: BTreeSet<u8>) -> Vec<String> {
    codes
        .into_iter()
        .map(|code| format!("{code:02X}"))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn source_code_partition_keeps_translation_and_preservation_distinct() {
        let partition = partition_source_codes([0x01, 0x60, 0x9B, 0xFF, 0x8C]);

        assert_eq!(partition.distinct_source_code_count, 5);
        assert_eq!(partition.japanese_codes_hex, ["01"]);
        assert_eq!(partition.preserved_original_codes_hex, ["60", "9B"]);
        assert_eq!(partition.layout_codes_hex, ["FF"]);
        assert_eq!(partition.unresolved_codes_hex, ["8C"]);
    }
}
