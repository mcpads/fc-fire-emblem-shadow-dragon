use anyhow::{Context, Result, ensure};
use serde::Serialize;

use crate::{
    battle_runtime_state::BATTLE_RUNTIME_STATE,
    rom::Rom,
    runtime_storage_layout::{
        BATTLE_REMAP_PAIR_TABLE_END, BATTLE_REMAP_PAIR_TABLE_START, BATTLE_REMAP_STATE_ADDRESS,
        SOURCE_PPU_QUEUE_START, bind_integrated_runtime_storage_layout,
    },
};

use super::{
    background_payloads::BattleBackgroundPayloadModel,
    text_consumer_topology::BattleTextConsumerTopology,
};

mod source_contract;

pub(in crate::mapper165) use source_contract::{
    IndirectWriteDestinationBounds, bind_indirect_write_destination_bounds,
};

const PAIR_TABLE_BYTE_COUNT: usize = 16;
const MAXIMUM_REMAP_PAIR_COUNT: usize = 8;
const REMAP_COUNT_MASK: u8 = 0x1E;
const REMAP_COUNT_SHIFT: u8 = 1;
const CACHE_READY_MASK: u8 = 0x80;

#[derive(Clone, Debug, Serialize)]
pub(super) struct BattleRemapStorageContract {
    logical_remap_payload_byte_count: usize,
    maximum_remap_pair_count: usize,
    pair_table_start_hex: String,
    pair_table_end_hex: String,
    pair_table_byte_count: usize,
    remap_state_address_hex: String,
    original_active_address_hex: String,
    remap_count_mask_hex: String,
    remap_count_shift: u8,
    cache_ready_mask_hex: String,
    source_queue_start_hex: String,
    maximum_background_queue_byte_count: usize,
    maximum_text_queue_byte_count: usize,
    maximum_battle_queue_byte_count: usize,
    remap_state_offset_from_queue_start: usize,
    pair_table_offset_from_queue_start: usize,
    unused_queue_tail_byte_count_before_pair_table: usize,
    source_contract: source_contract::BattleStorageSourceContract,
    count_and_pair_capacity_match: bool,
    remap_state_masks_are_disjoint: bool,
    original_active_boolean_preserved: bool,
    original_active_writers_leave_remap_state_untouched: bool,
    remap_storage_outside_every_bounded_battle_queue: bool,
    battle_lifetime_home_proven: bool,
    projection_hook_installed: bool,
    runtime_verified: bool,
}

impl BattleRemapStorageContract {
    pub(super) fn maximum_remap_pair_count(&self) -> usize {
        self.maximum_remap_pair_count
    }
}

pub(super) fn bind_battle_remap_storage(
    rom: &Rom,
    background: &BattleBackgroundPayloadModel,
    text: &BattleTextConsumerTopology,
) -> Result<BattleRemapStorageContract> {
    bind_integrated_runtime_storage_layout()?;
    let source_contract = source_contract::bind_battle_storage_source_contract(rom)?;
    let maximum_background_queue_byte_count = background.maximum_published_queue_byte_count();
    let maximum_text_queue_byte_count = text.maximum_published_queue_byte_count();
    let maximum_battle_queue_byte_count =
        maximum_background_queue_byte_count.max(maximum_text_queue_byte_count);
    let remap_state_offset_from_queue_start = usize::from(
        BATTLE_REMAP_STATE_ADDRESS
            .checked_sub(SOURCE_PPU_QUEUE_START)
            .context("remap state precedes the source queue")?,
    );
    let pair_table_offset_from_queue_start = usize::from(
        BATTLE_REMAP_PAIR_TABLE_START
            .checked_sub(SOURCE_PPU_QUEUE_START)
            .context("remap pair table precedes the source queue")?,
    );
    ensure!(
        maximum_battle_queue_byte_count <= pair_table_offset_from_queue_start,
        "battle queue can overlap the remap pair table"
    );
    let unused_queue_tail_byte_count_before_pair_table = pair_table_offset_from_queue_start
        .checked_sub(maximum_battle_queue_byte_count)
        .context("battle queue tail bound underflow")?;
    let pair_table_end = BATTLE_REMAP_PAIR_TABLE_START
        .checked_add(
            u16::try_from(PAIR_TABLE_BYTE_COUNT - 1)
                .context("remap pair-table byte count exceeds u16")?,
        )
        .context("remap pair-table end overflow")?;
    ensure!(
        pair_table_end == BATTLE_REMAP_PAIR_TABLE_END,
        "remap pair-table end changed"
    );
    ensure!(
        MAXIMUM_REMAP_PAIR_COUNT * 2 == PAIR_TABLE_BYTE_COUNT,
        "remap pair capacity and storage disagree"
    );
    let remap_state_masks_are_disjoint = REMAP_COUNT_MASK & CACHE_READY_MASK == 0;
    ensure!(
        remap_state_masks_are_disjoint,
        "remap-state count and ready masks overlap"
    );
    ensure!(
        ((MAXIMUM_REMAP_PAIR_COUNT as u8) << REMAP_COUNT_SHIFT) & !REMAP_COUNT_MASK == 0,
        "remap count does not fit its status bits"
    );

    Ok(BattleRemapStorageContract {
        logical_remap_payload_byte_count: 1 + PAIR_TABLE_BYTE_COUNT,
        maximum_remap_pair_count: MAXIMUM_REMAP_PAIR_COUNT,
        pair_table_start_hex: format!("0x{BATTLE_REMAP_PAIR_TABLE_START:04X}"),
        pair_table_end_hex: format!("0x{pair_table_end:04X}"),
        pair_table_byte_count: PAIR_TABLE_BYTE_COUNT,
        remap_state_address_hex: format!("0x{BATTLE_REMAP_STATE_ADDRESS:04X}"),
        original_active_address_hex: format!("0x{:04X}", BATTLE_RUNTIME_STATE.active_flag_address),
        remap_count_mask_hex: format!("0x{REMAP_COUNT_MASK:02X}"),
        remap_count_shift: REMAP_COUNT_SHIFT,
        cache_ready_mask_hex: format!("0x{CACHE_READY_MASK:02X}"),
        source_queue_start_hex: format!("0x{SOURCE_PPU_QUEUE_START:04X}"),
        maximum_background_queue_byte_count,
        maximum_text_queue_byte_count,
        maximum_battle_queue_byte_count,
        remap_state_offset_from_queue_start,
        pair_table_offset_from_queue_start,
        unused_queue_tail_byte_count_before_pair_table,
        source_contract,
        count_and_pair_capacity_match: true,
        remap_state_masks_are_disjoint,
        original_active_boolean_preserved: true,
        original_active_writers_leave_remap_state_untouched: true,
        remap_storage_outside_every_bounded_battle_queue: true,
        battle_lifetime_home_proven: true,
        projection_hook_installed: false,
        runtime_verified: false,
    })
}

#[cfg(test)]
pub(super) fn test_model() -> BattleRemapStorageContract {
    BattleRemapStorageContract {
        logical_remap_payload_byte_count: 17,
        maximum_remap_pair_count: 8,
        pair_table_start_hex: "0x07E0".to_owned(),
        pair_table_end_hex: "0x07EF".to_owned(),
        pair_table_byte_count: 16,
        remap_state_address_hex: "0x07FE".to_owned(),
        original_active_address_hex: format!("0x{:04X}", BATTLE_RUNTIME_STATE.active_flag_address),
        remap_count_mask_hex: "0x1E".to_owned(),
        remap_count_shift: 1,
        cache_ready_mask_hex: "0x80".to_owned(),
        source_queue_start_hex: "0x0781".to_owned(),
        maximum_background_queue_byte_count: 45,
        maximum_text_queue_byte_count: 67,
        maximum_battle_queue_byte_count: 67,
        remap_state_offset_from_queue_start: 125,
        pair_table_offset_from_queue_start: 95,
        unused_queue_tail_byte_count_before_pair_table: 28,
        source_contract: source_contract::test_model(),
        count_and_pair_capacity_match: true,
        remap_state_masks_are_disjoint: true,
        original_active_boolean_preserved: true,
        original_active_writers_leave_remap_state_untouched: true,
        remap_storage_outside_every_bounded_battle_queue: true,
        battle_lifetime_home_proven: true,
        projection_hook_installed: false,
        runtime_verified: false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn counted_pairs_fit_in_the_separate_remap_state_byte() {
        for count in 0..=MAXIMUM_REMAP_PAIR_COUNT as u8 {
            let status = CACHE_READY_MASK | (count << REMAP_COUNT_SHIFT);
            assert_eq!(status & CACHE_READY_MASK, CACHE_READY_MASK);
            assert_eq!((status & REMAP_COUNT_MASK) >> REMAP_COUNT_SHIFT, count);
        }
    }
}
