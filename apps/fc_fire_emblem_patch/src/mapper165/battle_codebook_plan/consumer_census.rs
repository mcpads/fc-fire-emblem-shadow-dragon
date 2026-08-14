use anyhow::{Context, Result, ensure};

use crate::{
    rom::{PRG_SIZE, Rom},
    text_inventory::scoped_text_table_budgets,
    translation_consumer::{
        ScreenConsumerSourceBinding, TranslationConsumerSourceEvidence, source_binding_id,
    },
};

use super::{
    background_ownership::bind_battle_background_code_ownership,
    runtime_inputs::bind_battle_runtime_inputs,
    text_consumer_topology::bind_battle_text_consumer_topology,
};

mod dialogue_false_positives;
mod false_positive_regions;
mod pointer_reference_census;
#[cfg(test)]
mod tests;

const PRG_BANK_SIZE: usize = 16 * 1024;
const TERRAIN_TABLE_ID: &str = "terrain-names";
const TERRAIN_NAME_COUNT: usize = 16;

#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
struct AbsoluteOperandCandidate {
    target: u16,
    prg_bank: u8,
    cpu_address: u16,
    opcode: u8,
}

#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
struct RecordPointerCandidate {
    target: u16,
    prg_bank: u8,
    cpu_address: u16,
}

pub(crate) fn inspect_known_terrain_name_translation_routes(
    rom: &Rom,
) -> Result<TranslationConsumerSourceEvidence> {
    rom.verify_supported_japanese()?;

    let budgets = scoped_text_table_budgets(rom.data(), &[TERRAIN_TABLE_ID])?;
    let terrain = budgets
        .into_iter()
        .next()
        .context("terrain-name source table is absent")?;
    ensure!(
        terrain.pointer_count == TERRAIN_NAME_COUNT
            && terrain.unique_string_count == TERRAIN_NAME_COUNT,
        "terrain-name source population changed"
    );

    pointer_reference_census::bind_known_terrain_source_references(rom)?;
    let _runtime_inputs = bind_battle_runtime_inputs(rom)?;
    let _text_topology = bind_battle_text_consumer_topology(rom)?;
    let _background_topology = bind_battle_background_code_ownership(rom)?;

    let population_ids = terrain_population_ids(terrain.pointer_count);
    Ok(TranslationConsumerSourceEvidence {
        population_ids: population_ids.clone(),
        screen_bindings: vec![ScreenConsumerSourceBinding {
            screen_role: "battle_animation",
            population_ids,
            source_binding_ids: vec![
                source_binding_id(0x06, 0xAD46, "project_battle_terrain_source_indices"),
                source_binding_id(0x0F, 0xE828, "map_cell_to_terrain_identity"),
                source_binding_id(0x0F, 0xEBEE, "map_normal_terrain_to_name_source_index"),
                source_binding_id(0x0F, 0xCF2F, "override_special_battle_terrain_name"),
                source_binding_id(0x0F, 0xD038, "reapply_special_battle_terrain_name"),
                source_binding_id(0x0F, 0xE5F1, "terrain_name_pointer_table"),
                source_binding_id(0x07, 0x8487, "load_terrain_name_pointer"),
                source_binding_id(0x07, 0x82CC, "compose_battle_terrain_name"),
                source_binding_id(0x07, 0x84A2, "publish_battle_terrain_name"),
                source_binding_id(0x0F, 0xE56C, "render_common_text_rows"),
                source_binding_id(0x0F, 0xC3A5, "consume_published_ppu_queue"),
            ],
        }],
    })
}

fn terrain_population_ids(count: usize) -> Vec<String> {
    (0..count)
        .map(|index| format!("{TERRAIN_TABLE_ID}:{index:03}"))
        .collect()
}

fn prg_source_bytes(prg: &[u8], bank: u8, address: u16, len: usize) -> Result<&[u8]> {
    ensure!(prg.len() == PRG_SIZE, "terrain route PRG size changed");
    let cpu_base: u16 = if bank == 0x0F { 0xC000 } else { 0x8000 };
    ensure!(
        address >= cpu_base && usize::from(address - cpu_base) + len <= PRG_BANK_SIZE,
        "terrain route source range is outside its PRG bank"
    );
    let start = usize::from(bank) * PRG_BANK_SIZE + usize::from(address - cpu_base);
    prg.get(start..start + len)
        .context("terrain route source range is outside PRG")
}
