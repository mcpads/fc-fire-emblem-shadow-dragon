use std::collections::BTreeSet;

use anyhow::{Context, Result, ensure};
use retro_rp2a03::{AddressingMode, Mnemonic, Operand, decode_bytes};
use serde::Serialize;

use crate::{
    battle_runtime_state::{BATTLE_ACTIVE_ONE_WRITES, BATTLE_RUNTIME_STATE, BattleSourceWrite},
    mapper165::inline_pointer_dispatch::INLINE_POINTER_DISPATCH_ADDRESS,
    rom::Rom,
    runtime_storage_layout::{BATTLE_DIALOGUE_CACHE_KEY_ADDRESS, BATTLE_REMAP_PAIR_TABLE_START},
    sha1_hex,
    typed_source::{Rp2a03DirectControlFlow, decode_rp2a03_sequence, rp2a03_direct_control_flow},
};

use super::super::{
    background_payloads::BATTLE_BANK_PUBLISH_SITES,
    phase_cooccurrence::battle_phase_roots,
    source_window::source_bytes,
    text_consumer_topology::source_contract::{
        BATTLE_DIALOGUE_STATE_HANDLERS, BATTLE_TERRAIN_BANK_HANDLER_POINTER,
        DIALOGUE_BOX_INNER_STATE_POINTERS,
    },
};

mod indirect_destinations;

#[cfg(test)]
use indirect_destinations::DESTINATION_CLASS_COUNT;
use indirect_destinations::{
    IndirectStoreDestinationClass, battle_lifetime_reachable_indirect_store_sites,
    bind_indirect_store_destination_classes,
};
pub(in crate::mapper165) use indirect_destinations::{
    IndirectWriteDestinationBounds, bind_indirect_write_destination_bounds,
};

const COMMON_TEXT_QUEUE_READY_ADDRESS: u16 = 0xE5C7;
const QUEUE_READY_ZERO_PAGE_ADDRESS: u8 = 0x21;
// The battle-owned pair table begins here. The enclosing source-access proof remains
// conservative through the battle metadata at $07FF, including the intervening dialogue state.
const REMAP_STORAGE_START: u16 = BATTLE_REMAP_PAIR_TABLE_START;
const REMAP_STORAGE_END: u16 = BATTLE_DIALOGUE_CACHE_KEY_ADDRESS;

#[derive(Clone, Debug, Serialize)]
pub(super) struct BattleStorageSourceContract {
    battle_root_count: usize,
    traced_instruction_count: usize,
    queue_ready_publisher_count: usize,
    queue_ready_publisher_catalog_sha1: String,
    indexed_remap_storage_overlap_instruction_count: usize,
    indexed_remap_storage_overlap_catalog_sha1: String,
    direct_unindexed_remap_storage_access_count: usize,
    indirect_store_instruction_count: usize,
    indirect_store_catalog_sha1: String,
    indirect_store_destination_class_count: usize,
    indirect_store_destination_classes: Vec<IndirectStoreDestinationClass>,
    bounded_queue_copy_indirect_store_count: usize,
    non_queue_indirect_store_count: usize,
    battle_active_direct_read_count: usize,
    battle_active_direct_write_count: usize,
    battle_active_nonzero_reader_address_hex: String,
    battle_active_full_byte_writer_addresses_hex: Vec<String>,
    every_battle_queue_publisher_reached: bool,
    every_indexed_remap_storage_overlap_is_a_bounded_queue_access: bool,
    every_indirect_store_classified: bool,
    every_indirect_store_destination_outside_remap_storage: bool,
    original_battle_active_reader_is_zero_nonzero_only: bool,
    original_battle_active_writers_are_full_byte_zero_or_one: bool,
}

pub(super) fn bind_battle_storage_source_contract(
    rom: &Rom,
) -> Result<BattleStorageSourceContract> {
    bind_battle_active_flag(rom)?;
    let roots = battle_lifetime_roots();
    let trace = trace_battle_lifetime(rom, &roots)?;
    let expected_publishers = BATTLE_BANK_PUBLISH_SITES
        .iter()
        .map(|(bank, address, _)| (*bank, *address))
        .chain([(0x0F, COMMON_TEXT_QUEUE_READY_ADDRESS)])
        .collect::<BTreeSet<_>>();
    ensure!(
        trace.queue_ready_publishers == expected_publishers,
        "battle queue-ready publisher reachability changed: expected {expected_publishers:?}, found {:?}",
        trace.queue_ready_publishers
    );
    ensure!(
        trace.direct_remap_storage_accesses.is_empty(),
        "original battle lifetime directly accesses remap storage: {:?}",
        trace.direct_remap_storage_accesses
    );
    ensure!(
        trace
            .indexed_remap_storage_overlaps
            .iter()
            .all(|(_, _, base)| (0x0780..=0x0794).contains(base)),
        "battle lifetime gained an indexed pair-table overlap outside the bounded queue writers"
    );
    let expected_indirect_stores = battle_lifetime_reachable_indirect_store_sites();
    ensure!(
        trace.indirect_stores == expected_indirect_stores,
        "battle lifetime indirect-store catalog changed: expected {expected_indirect_stores:?}, found {:?}",
        trace.indirect_stores
    );
    ensure!(
        trace.bounded_copy_callers
            == [(0x04, 0x8337), (0x05, 0x952A), (0x07, 0x8038)]
                .into_iter()
                .collect(),
        "bounded battle-copy caller set changed"
    );
    ensure!(
        trace.battle_zero_fill_callers == [(0x05, 0x8288)].into_iter().collect(),
        "battle zero-fill caller set changed"
    );
    ensure!(
        trace.fixed_glyph_flag_callers
            == [
                (0x05, 0x86BB),
                (0x05, 0x89E8),
                (0x05, 0x965C),
                (0x07, 0x814A),
            ]
            .into_iter()
            .collect(),
        "fixed glyph-flag caller set changed"
    );
    let indirect_store_destination_classes = bind_indirect_store_destination_classes(rom)?;
    ensure!(
        indirect_store_destination_classes
            .iter()
            .all(|class| class.every_destination_range_outside_remap_storage),
        "an indirect-store destination class overlaps remap storage"
    );

    let publisher_catalog = catalog_pairs(&trace.queue_ready_publishers);
    let indexed_catalog = trace
        .indexed_remap_storage_overlaps
        .iter()
        .flat_map(|(bank, address, base)| {
            [*bank]
                .into_iter()
                .chain(address.to_le_bytes())
                .chain(base.to_le_bytes())
        })
        .collect::<Vec<_>>();
    let indirect_catalog = trace
        .indirect_stores
        .iter()
        .flat_map(|(bank, address, pointer)| {
            [*bank]
                .into_iter()
                .chain(address.to_le_bytes())
                .chain([*pointer])
        })
        .collect::<Vec<_>>();

    Ok(BattleStorageSourceContract {
        battle_root_count: roots.len(),
        traced_instruction_count: trace.visited_instruction_count,
        queue_ready_publisher_count: trace.queue_ready_publishers.len(),
        queue_ready_publisher_catalog_sha1: sha1_hex(&publisher_catalog),
        indexed_remap_storage_overlap_instruction_count: trace.indexed_remap_storage_overlaps.len(),
        indexed_remap_storage_overlap_catalog_sha1: sha1_hex(&indexed_catalog),
        direct_unindexed_remap_storage_access_count: trace.direct_remap_storage_accesses.len(),
        indirect_store_instruction_count: trace.indirect_stores.len(),
        indirect_store_catalog_sha1: sha1_hex(&indirect_catalog),
        indirect_store_destination_class_count: indirect_store_destination_classes.len(),
        indirect_store_destination_classes,
        bounded_queue_copy_indirect_store_count: 1,
        non_queue_indirect_store_count: trace.indirect_stores.len() - 1,
        battle_active_direct_read_count: 1,
        battle_active_direct_write_count: BATTLE_ACTIVE_ONE_WRITES.len() + 1,
        battle_active_nonzero_reader_address_hex: "0x05:0x8000".to_owned(),
        battle_active_full_byte_writer_addresses_hex: std::iter::once(BattleSourceWrite {
            prg_bank: 0x05,
            cpu_address: 0x8100,
        })
        .chain(BATTLE_ACTIVE_ONE_WRITES)
        .map(|writer| format!("0x{:02X}:0x{:04X}", writer.prg_bank, writer.cpu_address))
        .collect(),
        every_battle_queue_publisher_reached: true,
        every_indexed_remap_storage_overlap_is_a_bounded_queue_access: true,
        every_indirect_store_classified: true,
        every_indirect_store_destination_outside_remap_storage: true,
        original_battle_active_reader_is_zero_nonzero_only: true,
        original_battle_active_writers_are_full_byte_zero_or_one: true,
    })
}

fn battle_lifetime_roots() -> Vec<(u8, u16)> {
    battle_phase_roots()
        .into_iter()
        .chain(
            BATTLE_DIALOGUE_STATE_HANDLERS
                .into_iter()
                .map(|address| (0x04, address)),
        )
        .chain(
            DIALOGUE_BOX_INNER_STATE_POINTERS
                .into_iter()
                .map(|address| (0x07, address)),
        )
        .chain(
            BATTLE_TERRAIN_BANK_HANDLER_POINTER
                .into_iter()
                .map(|address| (0x07, address)),
        )
        .collect::<BTreeSet<_>>()
        .into_iter()
        .collect()
}

struct BattleLifetimeTrace {
    visited_instruction_count: usize,
    queue_ready_publishers: BTreeSet<(u8, u16)>,
    direct_remap_storage_accesses: BTreeSet<(u8, u16, u16)>,
    indexed_remap_storage_overlaps: BTreeSet<(u8, u16, u16)>,
    indirect_stores: BTreeSet<(u8, u16, u8)>,
    bounded_copy_callers: BTreeSet<(u8, u16)>,
    battle_zero_fill_callers: BTreeSet<(u8, u16)>,
    fixed_glyph_flag_callers: BTreeSet<(u8, u16)>,
}

fn trace_battle_lifetime(rom: &Rom, roots: &[(u8, u16)]) -> Result<BattleLifetimeTrace> {
    let mut pending = roots.to_vec();
    let mut visited = BTreeSet::new();
    let mut queue_ready_publishers = BTreeSet::new();
    let mut direct_remap_storage_accesses = BTreeSet::new();
    let mut indexed_remap_storage_overlaps = BTreeSet::new();
    let mut indirect_stores = BTreeSet::new();
    let mut bounded_copy_callers = BTreeSet::new();
    let mut battle_zero_fill_callers = BTreeSet::new();
    let mut fixed_glyph_flag_callers = BTreeSet::new();
    while let Some((switchable_bank, address)) = pending.pop() {
        if !visited.insert((switchable_bank, address)) {
            continue;
        }
        let actual_bank = if address >= 0xC000 {
            0x0F
        } else {
            switchable_bank
        };
        let bytes = source_bytes(rom, actual_bank, address, 3)
            .with_context(|| format!("read battle lifetime {actual_bank:02X}:${address:04X}"))?;
        let instruction = decode_bytes(bytes)
            .with_context(|| format!("decode battle lifetime {actual_bank:02X}:${address:04X}"))?;
        ensure!(
            instruction.opcode_is_documented(),
            "battle lifetime reached undocumented selector at {actual_bank:02X}:${address:04X}"
        );
        if matches!(
            instruction.mnemonic(),
            Mnemonic::Sta | Mnemonic::Stx | Mnemonic::Sty
        ) && instruction.addressing_mode() == AddressingMode::ZeroPage
            && instruction.operand() == Operand::Byte(QUEUE_READY_ZERO_PAGE_ADDRESS)
        {
            queue_ready_publishers.insert((actual_bank, address));
        }
        if let Operand::Word(base) = instruction.operand() {
            match instruction.addressing_mode() {
                AddressingMode::Absolute
                    if (REMAP_STORAGE_START..=REMAP_STORAGE_END).contains(&base) =>
                {
                    direct_remap_storage_accesses.insert((actual_bank, address, base));
                }
                AddressingMode::AbsoluteX | AddressingMode::AbsoluteY
                    if base <= REMAP_STORAGE_END
                        && base.saturating_add(0x00FF) >= REMAP_STORAGE_START =>
                {
                    indexed_remap_storage_overlaps.insert((actual_bank, address, base));
                }
                _ => {}
            }
        }
        if matches!(
            instruction.mnemonic(),
            Mnemonic::Sta | Mnemonic::Stx | Mnemonic::Sty
        ) && matches!(
            instruction.addressing_mode(),
            AddressingMode::ZeroPageIndexedIndirectX | AddressingMode::ZeroPageIndirectIndexedY
        ) {
            let Operand::Byte(pointer) = instruction.operand() else {
                unreachable!("typed indirect store has a byte operand")
            };
            indirect_stores.insert((actual_bank, address, pointer));
        }

        match rp2a03_direct_control_flow(&instruction, address)? {
            Rp2a03DirectControlFlow::FallThrough { next } => {
                pending.push((switchable_bank, next));
            }
            Rp2a03DirectControlFlow::Branch {
                target,
                fallthrough,
            } => {
                pending.push((switchable_bank, target));
                pending.extend(fallthrough.map(|next| (switchable_bank, next)));
            }
            Rp2a03DirectControlFlow::Jump { target } => {
                pending.extend(target.map(|target| (switchable_bank, target)));
            }
            Rp2a03DirectControlFlow::Call {
                target,
                return_address,
            } => {
                match target {
                    0xC209 => {
                        bounded_copy_callers.insert((actual_bank, address));
                    }
                    0xC225 => {
                        battle_zero_fill_callers.insert((actual_bank, address));
                    }
                    0xC7BA => {
                        fixed_glyph_flag_callers.insert((actual_bank, address));
                    }
                    _ => {}
                }
                if target != INLINE_POINTER_DISPATCH_ADDRESS {
                    pending.push((switchable_bank, return_address));
                    pending.push((switchable_bank, target));
                }
            }
            Rp2a03DirectControlFlow::Return
            | Rp2a03DirectControlFlow::Interrupt
            | Rp2a03DirectControlFlow::Stop => {}
        }
    }
    Ok(BattleLifetimeTrace {
        visited_instruction_count: visited.len(),
        queue_ready_publishers,
        direct_remap_storage_accesses,
        indexed_remap_storage_overlaps,
        indirect_stores,
        bounded_copy_callers,
        battle_zero_fill_callers,
        fixed_glyph_flag_callers,
    })
}

fn bind_battle_active_flag(rom: &Rom) -> Result<()> {
    let [active_low, active_high] = BATTLE_RUNTIME_STATE.active_flag_address.to_le_bytes();
    let read_pattern = [0xAD, active_low, active_high];
    let write_pattern = [0x8D, active_low, active_high];
    ensure!(
        rom.prg()
            .windows(read_pattern.len())
            .filter(|bytes| *bytes == read_pattern)
            .count()
            == 1,
        "battle-active direct read count changed"
    );
    ensure!(
        rom.prg()
            .windows(write_pattern.len())
            .filter(|bytes| *bytes == write_pattern)
            .count()
            == BATTLE_ACTIVE_ONE_WRITES.len() + 1,
        "battle-active direct write count changed"
    );
    let reader = source_bytes(rom, 0x05, 0x8000, 6)?;
    ensure!(
        reader == [0xAD, active_low, active_high, 0xD0, 0x01, 0x60],
        "battle-active nonzero reader changed"
    );
    decode_rp2a03_sequence(reader, 0x8000, "battle-active nonzero reader")?;
    let zero_writer = source_bytes(rom, 0x05, 0x80DE, 37)?;
    let [phase_low, phase_high] = BATTLE_RUNTIME_STATE.shared_phase_address.to_le_bytes();
    let expected_zero_writer = vec![
        0xA9,
        0x00,
        0xA2,
        0x02,
        0x9D,
        0xAD,
        0x03,
        0x9D,
        0x89,
        0x03,
        0x9D,
        0xA7,
        0x03,
        0x9D,
        0xAA,
        0x03,
        0xCA,
        0x10,
        0xF1,
        0x8D,
        0x78,
        0x04,
        0x8D,
        0xCF,
        0x03,
        0x8D,
        0xD0,
        0x03,
        0x8D,
        phase_low,
        phase_high,
        0x8D,
        0xBF,
        0x03,
        0x8D,
        active_low,
        active_high,
    ];
    ensure!(
        zero_writer == expected_zero_writer,
        "battle-active zeroing writer changed"
    );
    decode_rp2a03_sequence(zero_writer, 0x80DE, "battle-active zeroing writer")?;
    for writer_site in &BATTLE_ACTIVE_ONE_WRITES[..3] {
        let bank = writer_site.prg_bank;
        let write = writer_site.cpu_address;
        let start = write
            .checked_sub(2)
            .context("battle-active literal-one writer start underflow")?;
        let writer = source_bytes(rom, bank, start, 5)?;
        ensure!(
            writer == [0xA9, 0x01, 0x8D, active_low, active_high],
            "battle-active literal-one writer changed at {bank:02X}:${start:04X}"
        );
        decode_rp2a03_sequence(writer, start, "battle-active literal-one writer")?;
    }
    let sound_bank = BATTLE_ACTIVE_ONE_WRITES[3].prg_bank;
    let sound_write = BATTLE_ACTIVE_ONE_WRITES[3].cpu_address;
    let sound_start = sound_write
        .checked_sub(5)
        .context("sound-test battle-active literal-one writer start underflow")?;
    let sound_writer = source_bytes(rom, sound_bank, sound_start, 8)?;
    ensure!(
        sound_writer == [0xA9, 0x01, 0x8D, 0xED, 0x05, 0x8D, active_low, active_high,],
        "sound-test battle-active literal-one writer changed"
    );
    decode_rp2a03_sequence(
        sound_writer,
        sound_start,
        "sound-test battle-active literal-one writer",
    )?;
    Ok(())
}

fn catalog_pairs(entries: &BTreeSet<(u8, u16)>) -> Vec<u8> {
    entries
        .iter()
        .flat_map(|(bank, address)| [*bank].into_iter().chain(address.to_le_bytes()))
        .collect()
}

#[cfg(test)]
pub(super) fn test_model() -> BattleStorageSourceContract {
    BattleStorageSourceContract {
        battle_root_count: 1,
        traced_instruction_count: 1,
        queue_ready_publisher_count: 18,
        queue_ready_publisher_catalog_sha1: "publishers".to_owned(),
        indexed_remap_storage_overlap_instruction_count: 1,
        indexed_remap_storage_overlap_catalog_sha1: "indexed".to_owned(),
        direct_unindexed_remap_storage_access_count: 0,
        indirect_store_instruction_count: battle_lifetime_reachable_indirect_store_sites().len(),
        indirect_store_catalog_sha1: "indirect".to_owned(),
        indirect_store_destination_class_count: DESTINATION_CLASS_COUNT,
        indirect_store_destination_classes: Vec::new(),
        bounded_queue_copy_indirect_store_count: 1,
        non_queue_indirect_store_count: battle_lifetime_reachable_indirect_store_sites().len() - 1,
        battle_active_direct_read_count: 1,
        battle_active_direct_write_count: 5,
        battle_active_nonzero_reader_address_hex: "0x05:0x8000".to_owned(),
        battle_active_full_byte_writer_addresses_hex: Vec::new(),
        every_battle_queue_publisher_reached: true,
        every_indexed_remap_storage_overlap_is_a_bounded_queue_access: true,
        every_indirect_store_classified: true,
        every_indirect_store_destination_outside_remap_storage: true,
        original_battle_active_reader_is_zero_nonzero_only: true,
        original_battle_active_writers_are_full_byte_zero_or_one: true,
    }
}
