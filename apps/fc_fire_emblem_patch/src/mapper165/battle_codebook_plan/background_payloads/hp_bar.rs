use anyhow::{Result, ensure};
use serde::Serialize;

use crate::{rom::Rom, sha1_hex, typed_source::decode_rp2a03_sequence};

use super::super::{
    enemy_domain::{EnemyGeneratedHpBound, bind_enemy_generated_hp_bound},
    source_window::source_bytes,
};

const PLAYER_STAT_CAP_ADDRESS: u16 = 0xD0F0;
const PLAYER_STAT_CAP_BYTES: [u8; 38] = [
    0xC5, 0x48, 0xF0, 0x1E, 0x90, 0x1C, 0xA0, 0x00, 0xB1, 0x04, 0x18, 0x6D, 0x5E, 0x03, 0xE0, 0x06,
    0xF0, 0x08, 0xC9, 0x14, 0x90, 0x0A, 0xA9, 0x14, 0xD0, 0x06, 0xC9, 0x34, 0x90, 0x02, 0xA9, 0x34,
    0x91, 0x08, 0xCA, 0x10, 0xAC, 0x60,
];
const PLAYER_STAT_CAP_SHA1: &str = "983a28c15e1fd1ecb64972bc1523c66fa8efec18";
const PLAYER_HP_CAP: u8 = 52;
const HP_PER_BAR_TILE: usize = 2;
const BAR_TILE_COUNT_PER_ROW: usize = 13;
const BAR_ROW_COUNT: usize = 2;
const QUEUE_COMMAND_HEADER_BYTE_COUNT: usize = 3;
const QUEUE_TERMINATOR_BYTE_COUNT: usize = 1;

#[derive(Clone, Debug, Serialize)]
pub(super) struct BattleHpBarQueueBound {
    player_stat_cap_address_hex: String,
    player_stat_cap_source_sha1: String,
    player_stat_cap_typed_instruction_count: usize,
    player_hp_cap: u8,
    enemy_generation: EnemyGeneratedHpBound,
    maximum_generated_enemy_hp: u8,
    maximum_supported_battle_hp: u8,
    hp_per_bar_tile: usize,
    bar_tile_count_per_row: usize,
    bar_row_count: usize,
    maximum_published_queue_byte_count: usize,
    player_and_enemy_hp_fit_two_rows: bool,
}

pub(super) fn bind_hp_bar_queue_bound(rom: &Rom) -> Result<BattleHpBarQueueBound> {
    let player_cap = source_bytes(
        rom,
        0x0F,
        PLAYER_STAT_CAP_ADDRESS,
        PLAYER_STAT_CAP_BYTES.len(),
    )?;
    ensure!(
        player_cap == PLAYER_STAT_CAP_BYTES,
        "player stat-cap routine changed"
    );
    ensure!(
        sha1_hex(player_cap) == PLAYER_STAT_CAP_SHA1,
        "player stat-cap routine hash changed"
    );
    let typed = decode_rp2a03_sequence(
        player_cap,
        PLAYER_STAT_CAP_ADDRESS,
        "player HP and ordinary-stat cap",
    )?;
    let enemy_generation = bind_enemy_generated_hp_bound(rom)?;
    let maximum_generated_enemy_hp = enemy_generation.maximum_generated_hp();
    let maximum_supported_battle_hp = PLAYER_HP_CAP.max(maximum_generated_enemy_hp);
    let bar_hp_capacity = HP_PER_BAR_TILE * BAR_TILE_COUNT_PER_ROW * BAR_ROW_COUNT;
    ensure!(
        usize::from(maximum_supported_battle_hp) <= bar_hp_capacity,
        "supported battle HP exceeds the two-row HP bar"
    );
    let maximum_published_queue_byte_count = BAR_ROW_COUNT
        .checked_mul(QUEUE_COMMAND_HEADER_BYTE_COUNT + BAR_TILE_COUNT_PER_ROW)
        .and_then(|bytes| bytes.checked_add(QUEUE_TERMINATOR_BYTE_COUNT))
        .expect("HP-bar queue bound fits usize");
    ensure!(
        maximum_published_queue_byte_count == 33,
        "HP-bar queue bound changed"
    );

    Ok(BattleHpBarQueueBound {
        player_stat_cap_address_hex: format!("0x{PLAYER_STAT_CAP_ADDRESS:04X}"),
        player_stat_cap_source_sha1: sha1_hex(player_cap),
        player_stat_cap_typed_instruction_count: typed.len(),
        player_hp_cap: PLAYER_HP_CAP,
        enemy_generation,
        maximum_generated_enemy_hp,
        maximum_supported_battle_hp,
        hp_per_bar_tile: HP_PER_BAR_TILE,
        bar_tile_count_per_row: BAR_TILE_COUNT_PER_ROW,
        bar_row_count: BAR_ROW_COUNT,
        maximum_published_queue_byte_count,
        player_and_enemy_hp_fit_two_rows: true,
    })
}

#[cfg(test)]
pub(super) fn test_model() -> BattleHpBarQueueBound {
    BattleHpBarQueueBound {
        player_stat_cap_address_hex: "0xD0F0".to_owned(),
        player_stat_cap_source_sha1: "cap".to_owned(),
        player_stat_cap_typed_instruction_count: 1,
        player_hp_cap: PLAYER_HP_CAP,
        enemy_generation: super::super::enemy_domain::test_hp_bound(),
        maximum_generated_enemy_hp: 45,
        maximum_supported_battle_hp: PLAYER_HP_CAP,
        hp_per_bar_tile: HP_PER_BAR_TILE,
        bar_tile_count_per_row: BAR_TILE_COUNT_PER_ROW,
        bar_row_count: BAR_ROW_COUNT,
        maximum_published_queue_byte_count: 33,
        player_and_enemy_hp_fit_two_rows: true,
    }
}
