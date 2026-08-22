use std::collections::BTreeSet;

use anyhow::{Context, Result, ensure};
use serde::Serialize;

use crate::{
    battle_runtime_state::BATTLE_RUNTIME_STATE, mmc5_prg::fixed_bank_file_offset, rom::Rom,
    sha1_hex, typed_source::decode_rp2a03_sequence,
};

use super::{RoutineSpec, SourceRoutineBinding, bind_routine};

const NORMAL_BATTLE_INPUT_CALLER: RoutineSpec = RoutineSpec {
    role: "compose_normal_battle_inputs_from_live_records",
    prg_bank: 0x06,
    cpu_address: 0x91F1,
    byte_count: 0x62,
    expected_sha1: "10b2e6eaff6a4e7b5d4abc221938e910263ffeb2",
};
const BATTLE_FIELD_COMPOSER: RoutineSpec = RoutineSpec {
    role: "project_live_records_into_battle_fields",
    prg_bank: 0x06,
    cpu_address: 0xAD46,
    byte_count: 0x14F,
    expected_sha1: "b331668ccf78fde80f515fc91f92dd9f64d06a41",
};
const BATTLE_CORE_CALLER: RoutineSpec = RoutineSpec {
    role: "run_normal_battle_core_after_field_projection",
    prg_bank: 0x06,
    cpu_address: 0x92E1,
    byte_count: 0x09,
    expected_sha1: "16dffc3ba097958a175b860307ec469e35422c4d",
};
const DRAGON_BONUS_REMOVER: RoutineSpec = RoutineSpec {
    role: "remove_temporary_dragon_battle_stat_bonuses",
    prg_bank: 0x06,
    cpu_address: 0xAE95,
    byte_count: 0x33,
    expected_sha1: "5447bd2b184e964bbfbc759e280af210b12b6856",
};
const DRAGON_BONUS_REMOVER_CALL: RoutineSpec = RoutineSpec {
    role: "call_post_battle_dragon_bonus_remover",
    prg_bank: 0x06,
    cpu_address: 0x931C,
    byte_count: 0x03,
    expected_sha1: "4833e3f4cd188a49d118f676d2b1e0e70cbc5cfd",
};
const SPECIAL_TERRAIN_OVERRIDE_ADDRESS: u16 = 0xCF2F;
const SPECIAL_TERRAIN_OVERRIDE_BYTE_COUNT: usize = 0x16;
const SPECIAL_TERRAIN_OVERRIDE_SHA1: &str = "bdf8d01de06cd88b116eb497779a26f450e0d150";
const SPECIAL_TERRAIN_REAPPLY_ADDRESS: u16 = 0xD038;
const SPECIAL_TERRAIN_REAPPLY_BYTE_COUNT: usize = 0x16;
const SPECIAL_TERRAIN_REAPPLY_SHA1: &str = "b1eee0f654ac41fb50f93c61972e83007033aecd";

const CELL_TO_TERRAIN_ADDRESS: u16 = 0xE828;
const CELL_TO_TERRAIN_BYTE_COUNT: usize = 0xD0;
const CELL_TO_TERRAIN_SHA1: &str = "cee21a5c758fae492b334d5a5ea799e6446fed88";
const TERRAIN_TO_NAME_SOURCE_INDEX_ADDRESS: u16 = 0xEBEE;
const TERRAIN_TO_NAME_SOURCE_INDEX_BYTE_COUNT: usize = 0x16;
const TERRAIN_TO_NAME_SOURCE_INDEX_SHA1: &str = "110ab93fa94e5eec38905d2f35acd39a556bc306";
const CLASS_NAME_POINTER_TABLE_ADDRESS: u16 = 0xDA1F;
const CLASS_NAME_POINTER_COUNT: usize = 0x18;
const CLASS_NAME_POINTER_TABLE_SHA1: &str = "f0775f1f970aca5b8b40e3e280b0e464754ca945";

const LIVE_RECORD_COPY_FRAGMENT: [u8; 22] = [
    0xA0, 0x00, 0x84, 0x12, 0x84, 0x13, 0xB1, 0x00, 0xA4, 0x13, 0x91, 0x02, 0xE6, 0x13, 0xE6, 0x13,
    0xA4, 0x12, 0xC8, 0x84, 0x12, 0xC0,
];
const EQUIPPED_ITEM_PROJECTION_FRAGMENT: [u8; 25] = [
    0xA0, 0x13, 0xB1, 0x00, 0xF0, 0x04, 0xC9, 0x40, 0x90, 0x02, 0xA9, 0x45, 0xA8, 0x88, 0x98, 0xA0,
    0x1C, 0x91, 0x02, 0xA0, 0x17, 0xB1, 0x00, 0xA0, 0x20,
];
const TERRAIN_NAME_PROJECTION_FRAGMENT: [u8; 25] = [
    0xA0, 0x06, 0xB1, 0x00, 0xA8, 0xB9, 0x28, 0xE8, 0xA8, 0xB9, 0xD8, 0xEB, 0x84, 0x12, 0xA0, 0x0C,
    0x91, 0x02, 0xA4, 0x12, 0xB9, 0xEE, 0xEB, 0xA0, 0x1E,
];
const DRAGON_CLASS_PROJECTION_FRAGMENT: [u8; 24] = [
    0xB9, 0x06, 0x03, 0xC9, 0x11, 0xD0, 0x45, 0xB9, 0x20, 0x03, 0xC9, 0x44, 0xF0, 0x3E, 0xC9, 0x21,
    0xF0, 0x0C, 0xC9, 0x22, 0xF0, 0x08, 0xC9, 0x23,
];
const NORMAL_CALLER_FRAGMENT: [u8; 28] = [
    0xA9, 0xF4, 0x85, 0x00, 0xA9, 0x76, 0x85, 0x01, 0x20, 0x46, 0xAD, 0xA9, 0x15, 0x85, 0x00, 0xA9,
    0x77, 0x85, 0x01, 0xA5, 0x92, 0x29, 0x10, 0xD0, 0x06, 0x20, 0x46, 0xAD,
];

#[derive(Debug, Serialize)]
pub(super) struct BattleFieldProducerBinding {
    normal_battle_input_caller: SourceRoutineBinding,
    battle_field_composer: SourceRoutineBinding,
    battle_core_caller: SourceRoutineBinding,
    dragon_bonus_remover: SourceRoutineBinding,
    dragon_bonus_remover_call: SourceRoutineBinding,
    special_terrain_override: SourceRoutineBinding,
    special_terrain_reapply: SourceRoutineBinding,
    live_record_identity_offset: u8,
    live_record_class_offset: u8,
    live_record_equipped_item_offset: u8,
    battle_identity_addresses: [u16; 2],
    battle_class_addresses: [u16; 2],
    battle_item_source_index_addresses: [u16; 2],
    battle_terrain_source_index_addresses: [u16; 2],
    equipped_item_to_name_source_index: &'static str,
    transformed_class_ids: [u8; 2],
    transformed_class_name_source_indices: [u8; 2],
    class_name_pointers: FixedTableBinding,
    transformed_class_pointer_alias_bound: bool,
    cell_to_terrain: FixedTableBinding,
    terrain_to_name_source_index: FixedTableBinding,
    terrain_name_source_index_count: usize,
    normal_terrain_name_source_index_count: usize,
    special_terrain_name_source_index: u8,
    all_projected_terrain_names_bound: bool,
    terrain_pair_model: &'static str,
}

#[derive(Debug, Serialize)]
struct FixedTableBinding {
    role: &'static str,
    cpu_address: u16,
    byte_count: usize,
    source_sha1: String,
}

pub(super) fn bind_battle_field_producers(rom: &Rom) -> Result<BattleFieldProducerBinding> {
    let runtime = BATTLE_RUNTIME_STATE;
    let normal_battle_input_caller = bind_routine(rom, NORMAL_BATTLE_INPUT_CALLER)?;
    let battle_field_composer = bind_routine(rom, BATTLE_FIELD_COMPOSER)?;
    let battle_core_caller = bind_routine(rom, BATTLE_CORE_CALLER)?;
    let dragon_bonus_remover = bind_routine(rom, DRAGON_BONUS_REMOVER)?;
    let dragon_bonus_remover_call = bind_routine(rom, DRAGON_BONUS_REMOVER_CALL)?;
    let special_terrain_override = bind_fixed_routine(
        rom,
        "override_special_battle_terrain_name",
        SPECIAL_TERRAIN_OVERRIDE_ADDRESS,
        SPECIAL_TERRAIN_OVERRIDE_BYTE_COUNT,
        SPECIAL_TERRAIN_OVERRIDE_SHA1,
    )?;
    let special_terrain_reapply = bind_fixed_routine(
        rom,
        "reapply_special_battle_terrain_name",
        SPECIAL_TERRAIN_REAPPLY_ADDRESS,
        SPECIAL_TERRAIN_REAPPLY_BYTE_COUNT,
        SPECIAL_TERRAIN_REAPPLY_SHA1,
    )?;

    let caller = super::source_slice(rom, NORMAL_BATTLE_INPUT_CALLER)?;
    ensure!(
        caller
            .windows(NORMAL_CALLER_FRAGMENT.len())
            .any(|window| window == NORMAL_CALLER_FRAGMENT),
        "normal battle caller no longer supplies both live battle records"
    );
    let composer = super::source_slice(rom, BATTLE_FIELD_COMPOSER)?;
    for (role, fragment) in [
        (
            "live record field copy",
            LIVE_RECORD_COPY_FRAGMENT.as_slice(),
        ),
        (
            "equipped item projection",
            EQUIPPED_ITEM_PROJECTION_FRAGMENT.as_slice(),
        ),
        (
            "terrain name projection",
            TERRAIN_NAME_PROJECTION_FRAGMENT.as_slice(),
        ),
        (
            "dragon class projection",
            DRAGON_CLASS_PROJECTION_FRAGMENT.as_slice(),
        ),
    ] {
        ensure!(
            composer
                .windows(fragment.len())
                .any(|window| window == fragment),
            "battle field composer lost its {role}"
        );
    }

    let cell_to_terrain = fixed_table(
        rom,
        "map_cell_to_terrain_identity",
        CELL_TO_TERRAIN_ADDRESS,
        CELL_TO_TERRAIN_BYTE_COUNT,
        CELL_TO_TERRAIN_SHA1,
    )?;
    let terrain_source_indices = fixed_table_bytes(
        rom,
        TERRAIN_TO_NAME_SOURCE_INDEX_ADDRESS,
        TERRAIN_TO_NAME_SOURCE_INDEX_BYTE_COUNT,
        "terrain identity to name source index",
    )?;
    let terrain_source_sha1 = sha1_hex(terrain_source_indices);
    ensure!(
        terrain_source_sha1 == TERRAIN_TO_NAME_SOURCE_INDEX_SHA1,
        "terrain name projection table changed: expected {TERRAIN_TO_NAME_SOURCE_INDEX_SHA1}, found {terrain_source_sha1}"
    );
    let projected_terrains = terrain_source_indices
        .iter()
        .copied()
        .collect::<BTreeSet<_>>();
    let mut expected_normal_terrains = (0_u8..=0x0F).collect::<BTreeSet<_>>();
    expected_normal_terrains.remove(&0x0E);
    ensure!(
        projected_terrains == expected_normal_terrains,
        "normal terrain projection no longer covers the expected 15 source indices"
    );
    let all_projected_terrains = projected_terrains
        .iter()
        .copied()
        .chain([0x0E])
        .collect::<BTreeSet<_>>();
    ensure!(
        all_projected_terrains == (0_u8..=0x0F).collect(),
        "normal and special terrain producers no longer cover source indices 0 through 15"
    );
    let class_name_pointer_bytes = fixed_table_bytes(
        rom,
        CLASS_NAME_POINTER_TABLE_ADDRESS,
        CLASS_NAME_POINTER_COUNT * 2,
        "class name pointers",
    )?;
    let class_name_pointer_sha1 = sha1_hex(class_name_pointer_bytes);
    ensure!(
        class_name_pointer_sha1 == CLASS_NAME_POINTER_TABLE_SHA1,
        "class name pointers changed: expected {CLASS_NAME_POINTER_TABLE_SHA1}, found {class_name_pointer_sha1}"
    );
    let class_name_pointers = class_name_pointer_bytes
        .chunks_exact(2)
        .map(|bytes| u16::from_le_bytes([bytes[0], bytes[1]]))
        .collect::<Vec<_>>();
    ensure!(
        class_name_pointers[0x10] == class_name_pointers[0x16]
            && class_name_pointers[0x10] == class_name_pointers[0x17],
        "transformed dragon classes no longer alias the base dragon class name"
    );

    Ok(BattleFieldProducerBinding {
        normal_battle_input_caller,
        battle_field_composer,
        battle_core_caller,
        dragon_bonus_remover,
        dragon_bonus_remover_call,
        special_terrain_override,
        special_terrain_reapply,
        live_record_identity_offset: runtime.live_record_identity_offset,
        live_record_class_offset: runtime.live_record_class_offset,
        live_record_equipped_item_offset: runtime.live_record_equipped_item_offset,
        battle_identity_addresses: runtime.staged_participant_identity_addresses,
        battle_class_addresses: runtime.staged_class_identity_addresses,
        battle_item_source_index_addresses: runtime.staged_item_source_index_addresses,
        battle_terrain_source_index_addresses: runtime.staged_terrain_source_index_addresses,
        equipped_item_to_name_source_index: "item_id == 0 is unreachable for combat; item_id < 0x40 maps to item_id - 1; item_id >= 0x40 maps to 0x44",
        transformed_class_ids: [0x17, 0x18],
        transformed_class_name_source_indices: [0x16, 0x17],
        class_name_pointers: FixedTableBinding {
            role: "class_name_pointers_including_transformed_dragon_aliases",
            cpu_address: CLASS_NAME_POINTER_TABLE_ADDRESS,
            byte_count: CLASS_NAME_POINTER_COUNT * 2,
            source_sha1: class_name_pointer_sha1,
        },
        transformed_class_pointer_alias_bound: true,
        cell_to_terrain,
        terrain_to_name_source_index: FixedTableBinding {
            role: "terrain_identity_to_name_source_index",
            cpu_address: TERRAIN_TO_NAME_SOURCE_INDEX_ADDRESS,
            byte_count: TERRAIN_TO_NAME_SOURCE_INDEX_BYTE_COUNT,
            source_sha1: terrain_source_sha1.clone(),
        },
        terrain_name_source_index_count: all_projected_terrains.len(),
        normal_terrain_name_source_index_count: projected_terrains.len(),
        special_terrain_name_source_index: 0x0E,
        all_projected_terrain_names_bound: true,
        terrain_pair_model: "both sides independently project through the same 16-name table; the graph keeps the full pairwise terrain-name upper bound",
    })
}

fn fixed_table(
    rom: &Rom,
    role: &'static str,
    cpu_address: u16,
    byte_count: usize,
    expected_sha1: &str,
) -> Result<FixedTableBinding> {
    let bytes = fixed_table_bytes(rom, cpu_address, byte_count, role)?;
    let source_sha1 = sha1_hex(bytes);
    ensure!(
        source_sha1 == expected_sha1,
        "{role} changed: expected {expected_sha1}, found {source_sha1}"
    );
    Ok(FixedTableBinding {
        role,
        cpu_address,
        byte_count,
        source_sha1,
    })
}

fn bind_fixed_routine(
    rom: &Rom,
    role: &'static str,
    cpu_address: u16,
    byte_count: usize,
    expected_sha1: &str,
) -> Result<SourceRoutineBinding> {
    let bytes = fixed_table_bytes(rom, cpu_address, byte_count, role)?;
    let source_sha1 = sha1_hex(bytes);
    ensure!(
        source_sha1 == expected_sha1,
        "{role} changed: expected {expected_sha1}, found {source_sha1}"
    );
    let typed = decode_rp2a03_sequence(bytes, cpu_address, role)?;
    Ok(SourceRoutineBinding {
        role,
        prg_bank: 0x0F,
        cpu_address,
        byte_count,
        source_sha1,
        typed_instruction_count: typed.len(),
    })
}

fn fixed_table_bytes<'a>(
    rom: &'a Rom,
    cpu_address: u16,
    byte_count: usize,
    role: &str,
) -> Result<&'a [u8]> {
    let offset = fixed_bank_file_offset(cpu_address)?;
    rom.data()
        .get(offset..offset + byte_count)
        .with_context(|| format!("{role} is outside the fixed PRG bank"))
}

#[cfg(test)]
pub(super) fn test_binding() -> BattleFieldProducerBinding {
    let runtime = BATTLE_RUNTIME_STATE;
    fn routine(role: &'static str) -> SourceRoutineBinding {
        SourceRoutineBinding {
            role,
            prg_bank: 6,
            cpu_address: 0x8000,
            byte_count: 1,
            source_sha1: "source".to_owned(),
            typed_instruction_count: 1,
        }
    }

    BattleFieldProducerBinding {
        normal_battle_input_caller: routine("normal caller"),
        battle_field_composer: routine("field composer"),
        battle_core_caller: routine("core caller"),
        dragon_bonus_remover: routine("bonus remover"),
        dragon_bonus_remover_call: routine("bonus remover call"),
        special_terrain_override: routine("terrain override"),
        special_terrain_reapply: routine("terrain reapply"),
        live_record_identity_offset: runtime.live_record_identity_offset,
        live_record_class_offset: runtime.live_record_class_offset,
        live_record_equipped_item_offset: runtime.live_record_equipped_item_offset,
        battle_identity_addresses: runtime.staged_participant_identity_addresses,
        battle_class_addresses: runtime.staged_class_identity_addresses,
        battle_item_source_index_addresses: runtime.staged_item_source_index_addresses,
        battle_terrain_source_index_addresses: runtime.staged_terrain_source_index_addresses,
        equipped_item_to_name_source_index: "projection",
        transformed_class_ids: [0x17, 0x18],
        transformed_class_name_source_indices: [0x16, 0x17],
        class_name_pointers: FixedTableBinding {
            role: "class pointers",
            cpu_address: CLASS_NAME_POINTER_TABLE_ADDRESS,
            byte_count: CLASS_NAME_POINTER_COUNT * 2,
            source_sha1: "source".to_owned(),
        },
        transformed_class_pointer_alias_bound: true,
        cell_to_terrain: FixedTableBinding {
            role: "cell table",
            cpu_address: CELL_TO_TERRAIN_ADDRESS,
            byte_count: CELL_TO_TERRAIN_BYTE_COUNT,
            source_sha1: "source".to_owned(),
        },
        terrain_to_name_source_index: FixedTableBinding {
            role: "terrain table",
            cpu_address: TERRAIN_TO_NAME_SOURCE_INDEX_ADDRESS,
            byte_count: TERRAIN_TO_NAME_SOURCE_INDEX_BYTE_COUNT,
            source_sha1: "source".to_owned(),
        },
        terrain_name_source_index_count: 16,
        normal_terrain_name_source_index_count: 15,
        special_terrain_name_source_index: 0x0E,
        all_projected_terrain_names_bound: true,
        terrain_pair_model: "pairwise upper bound",
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_binding_records_the_full_terrain_and_projection_bounds() {
        let binding = test_binding();

        assert_eq!(binding.terrain_name_source_index_count, 16);
        assert!(binding.all_projected_terrain_names_bound);
        assert!(binding.transformed_class_pointer_alias_bound);
    }
}
