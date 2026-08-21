use std::collections::{BTreeMap, BTreeSet};

use anyhow::{Context, Result, ensure};

use crate::{
    dialogue_inventory::{
        bind_caller_handoff_state_dispatch_sources, switchable_cpu_to_file_offset,
    },
    rom::Rom,
    sha1_hex,
    typed_source::decode_rp2a03_sequence,
    unit_ui_text::bind_map_facility_dispatch_source,
};

const SOURCE_BANK: u8 = 0x06;
const DIALOGUE_RECORD_ADDRESS: u16 = 0x77F1;
const CALLER_STATE_ADDRESS: u16 = 0x05DB;
const STORAGE_FACILITY_INDEX: u8 = 0x04;

const FACILITY_DISPATCH_CALL: u16 = 0x9DC1;
const FACILITY_STATE_MACHINE_START: u16 = 0x9DBE;
const FACILITY_STATE_MACHINE_END: u16 = 0xA0C2;
const FACILITY_HANDLER_TARGETS: [u16; 17] = [
    0x99CC, 0xA13E, 0x99F1, 0x9E07, 0x9E15, 0xA13E, 0x9EAC, 0x9EC1, 0xA13E, 0x9DE6, 0x9F16, 0x9F99,
    0x9C02, 0xA13E, 0x9B7A, 0xA07D, 0xA122,
];
const FACILITY_IMMEDIATE_RECORD_WRITES: [(u16, u8); 11] = [
    (0x9E31, 0x3D),
    (0x9E44, 0x2E),
    (0x9E5B, 0x3F),
    (0x9E75, 0x2A),
    (0x9E84, 0x2B),
    (0x9E90, 0x2C),
    (0x9ECC, 0x2D),
    (0x9F0B, 0x3B),
    (0x9FB4, 0x2D),
    (0x9FEC, 0x3E),
    (0xA031, 0x3C),
];
const FACILITY_INITIALIZER: [u8; 31] = [
    0xA9, 0x00, 0x8D, 0xDC, 0x77, 0x8D, 0xF0, 0x77, 0xAE, 0xD0, 0x77, 0xBD, 0xEB, 0x99, 0x8D, 0xF1,
    0x77, 0xA9, 0xB1, 0x8D, 0xF4, 0x77, 0xA9, 0x01, 0x8D, 0xF7, 0x77, 0xEE, 0xDB, 0x05, 0x60,
];
const FACILITY_INITIAL_RECORDS: [u8; 6] = [0x00, 0x00, 0x07, 0x0E, 0x29, 0x47];
const FACILITY_DIALOGUE_ADVANCE: [u8; 10] =
    [0x20, 0x5A, 0x9C, 0x20, 0xBC, 0xA0, 0xEE, 0xDB, 0x05, 0x60];
const FACILITY_ACTION_MENU_ENTRY: [u8; 14] = [
    0x20,
    0x5A,
    0x9C,
    0x20,
    0x5C,
    0xE6,
    0xEE,
    0xDB,
    0x05,
    0xA9,
    super::STORAGE_ACTION_MENU_COMPOSITE_STATE,
    0x4C,
    0x90,
    0xE6,
];
const FACILITY_EXIT: [u8; 27] = [
    0xAE, 0xD0, 0x77, 0xBD, 0xB0, 0xA0, 0x8D, 0xF1, 0x77, 0xBD, 0xB6, 0xA0, 0x8D, 0xDB, 0x05, 0xA2,
    0x19, 0xAD, 0xDC, 0x77, 0xD0, 0x02, 0xA2, 0x0E, 0x86, 0x26, 0x60,
];
const FACILITY_EXIT_RECORDS: [u8; 6] = [0x06, 0x06, 0x0D, 0x06, 0x06, 0x4D];
const FACILITY_EXIT_STATES: [u8; 6] = [0x00, 0x08, 0x08, 0x00, 0x10, 0x08];
const FACILITY_ACTION_MENU_ENTRY_STATE: u8 = 0x03;
const FACILITY_ITEM_LIST_COMPOSITION_STATE: u8 = 0x06;
const FACILITY_ITEM_LIST_SETTLED_STATE: u8 = 0x07;
const FACILITY_RESULT_DIALOGUE_STATE: u8 = 0x0C;
const FACILITY_CHOICE_COMPOSITION_STATE: u8 = 0x0E;
const FACILITY_CHOICE_INPUT_STATE: u8 = 0x0F;
/// Both branches of the storage action menu feed the same item-list composer.
/// The store branch publishes record 0x2A, while the withdraw branch publishes
/// record 0x2C before entering the shared composition state.
const FACILITY_ITEM_LIST_DIALOGUE_RECORD_WRITES: [(u16, u8); 2] = [(0x9E75, 0x2A), (0x9E90, 0x2C)];
const FACILITY_CHOICE_DIALOGUE_RECORD: u8 = 0x2D;
const FACILITY_CHOICE_COMPOSER: u16 = 0x9B7A;
const FACILITY_CHOICE_COMPOSER_BYTES: [u8; 12] = [
    0x20,
    0x5A,
    0x9C,
    0xA9,
    crate::choice_labels::CHOICE_LABEL_COMPOSITE_STATE,
    0x20,
    0x90,
    0xE6,
    0xEE,
    0xDB,
    0x05,
    0x60,
];
const FACILITY_CHOICE_INPUT: u16 = 0xA07D;
const FACILITY_CHOICE_INPUT_BYTES: [u8; 24] = [
    0x20,
    0x5A,
    0x9C,
    0x20,
    0x5C,
    0xE6,
    0xAD,
    0xEB,
    0x05,
    0xC9,
    0x01,
    0xD0,
    0x08,
    0xA9,
    FACILITY_ACTION_MENU_ENTRY_STATE,
    0x8D,
    0xDB,
    0x05,
    0x4C,
    0x6E,
    0xE6,
    0x4C,
    0x95,
    0xA0,
];
const FACILITY_ITEM_LIST_COMPOSITE_STATE: u8 = 0x07;
const FACILITY_ITEM_LIST_COMPOSER: u16 = 0x9EAC;
const FACILITY_ITEM_LIST_COMPOSER_BYTES: [u8; 21] = [
    0x20,
    0x5A,
    0x9C,
    0x20,
    0x5C,
    0xE6,
    0xA9,
    FACILITY_ITEM_LIST_COMPOSITE_STATE,
    0x20,
    0x90,
    0xE6,
    0xA9,
    0x20,
    0x85,
    0x70,
    0x85,
    0x71,
    0xEE,
    0xDB,
    0x05,
    0x60,
];
const OVERFLOW_ITEM_LIST_DIALOGUE_RECORD: u8 = 0x44;
const OVERFLOW_ITEM_LIST_COMPOSITE_STATE: u8 = 0x24;
const OVERFLOW_ITEM_LIST_COMPOSER: u16 = 0xB1F7;
const OVERFLOW_ITEM_LIST_COMPOSER_BYTES: [u8; 25] = [
    0xAD,
    0xF4,
    0x76,
    0x10,
    0x08,
    0xA9,
    0x15,
    0x85,
    0x74,
    0xA9,
    0x77,
    0x85,
    0x75,
    0xEE,
    0xE2,
    0x05,
    0xA9,
    OVERFLOW_ITEM_LIST_COMPOSITE_STATE,
    0x20,
    0x90,
    0xE6,
    0xEE,
    0xDB,
    0x05,
    0x60,
];

const OVERFLOW_DISPATCH_CALL: u16 = 0xB110;
const OVERFLOW_STATE_MACHINE_START: u16 = 0xB10D;
const OVERFLOW_STATE_MACHINE_END: u16 = 0xB2A8;
const OVERFLOW_HANDLER_TARGETS: [u16; 9] = [
    0xB125, 0xA13E, 0xB17A, 0xB182, 0xB19C, 0xA13E, 0xB1F7, 0xB210, 0xA122,
];
const OVERFLOW_IMMEDIATE_RECORD_WRITES: [(u16, u8); 5] = [
    (0xB140, 0x40),
    (0xB1AB, 0x41),
    (0xB1BE, 0x42),
    (0xB1E9, 0x43),
    (0xB259, 0x45),
];
const OVERFLOW_HELD_ITEM_DISCARD: [u8; 9] = [0xA9, 0x00, 0x8D, 0xB1, 0x77, 0xA9, 0x46, 0xD0, 0x31];
const OVERFLOW_ACTION_MENU_ENTRY: [u8; 8] = [
    0xEE,
    0xDB,
    0x05,
    0xA9,
    super::STORAGE_OVERFLOW_ACTION_COMPOSITE_STATE,
    0x4C,
    0x90,
    0xE6,
];

pub(super) struct StorageSourceBinding {
    pub(super) facility_root_record_indices: BTreeSet<usize>,
    pub(super) overflow_root_record_indices: BTreeSet<usize>,
    pub(super) facility_overlay_root_record_index: usize,
    pub(super) overflow_overlay_root_record_index: usize,
    pub(super) facility_item_list_root_record_indices: BTreeSet<usize>,
    pub(super) facility_action_menu_return_root_record_indices: BTreeSet<usize>,
    pub(super) facility_choice_record_index: usize,
    pub(super) overflow_item_list_overlay_root_record_index: usize,
    pub(super) item_list_settled_state: u8,
    pub(super) item_list_route: super::StorageItemListRuntimeRoute,
    pub(super) source_dispatch_count: usize,
    pub(super) source_direct_record_store_count: usize,
    pub(super) source_binding_sha1: String,
}

pub(super) fn bind_storage_dialogue_sources(source: &Rom) -> Result<StorageSourceBinding> {
    source.verify_supported_japanese()?;

    let dispatches = bind_caller_handoff_state_dispatch_sources(source)?;
    bind_dispatch(
        &dispatches,
        FACILITY_DISPATCH_CALL,
        &FACILITY_HANDLER_TARGETS,
        "storage facility",
    )?;
    bind_dispatch(
        &dispatches,
        OVERFLOW_DISPATCH_CALL,
        &OVERFLOW_HANDLER_TARGETS,
        "storage overflow",
    )?;

    let map_facilities = bind_map_facility_dispatch_source(source)?;
    ensure!(
        map_facilities
            .produced_selectors()
            .contains(&STORAGE_FACILITY_INDEX),
        "map facility source no longer produces the storage facility index"
    );

    bind_exact_code(
        source,
        0x99CC,
        &FACILITY_INITIALIZER,
        "storage facility dialogue initializer",
    )?;
    bind_exact_bytes(
        source,
        0x99EB,
        &FACILITY_INITIAL_RECORDS,
        "storage facility initial record table",
    )?;
    bind_exact_code(
        source,
        0x99F1,
        &FACILITY_DIALOGUE_ADVANCE,
        "storage facility dialogue advance",
    )?;
    bind_exact_code(
        source,
        0x9E07,
        &FACILITY_ACTION_MENU_ENTRY,
        "storage facility action-menu entry",
    )?;
    bind_exact_code(
        source,
        0xA095,
        &FACILITY_EXIT,
        "storage facility exit selector",
    )?;
    bind_exact_bytes(
        source,
        0xA0B0,
        &FACILITY_EXIT_RECORDS,
        "storage facility exit record table",
    )?;
    bind_exact_bytes(
        source,
        0xA0B6,
        &FACILITY_EXIT_STATES,
        "storage facility exit state table",
    )?;
    bind_exact_code(
        source,
        FACILITY_ITEM_LIST_COMPOSER,
        &FACILITY_ITEM_LIST_COMPOSER_BYTES,
        "storage facility dialogue-retaining item-list composer",
    )?;
    bind_exact_code(
        source,
        FACILITY_CHOICE_COMPOSER,
        &FACILITY_CHOICE_COMPOSER_BYTES,
        "storage facility shared yes-no composer",
    )?;
    bind_exact_code(
        source,
        FACILITY_CHOICE_INPUT,
        &FACILITY_CHOICE_INPUT_BYTES,
        "storage facility yes-no return to action menu",
    )?;
    bind_exact_code(
        source,
        OVERFLOW_ITEM_LIST_COMPOSER,
        &OVERFLOW_ITEM_LIST_COMPOSER_BYTES,
        "storage overflow dialogue-retaining item-list composer",
    )?;
    ensure!(
        FACILITY_INITIAL_RECORDS[usize::from(STORAGE_FACILITY_INDEX)] == 0x29
            && FACILITY_EXIT_RECORDS[usize::from(STORAGE_FACILITY_INDEX)] == 0x06
            && FACILITY_EXIT_STATES[usize::from(STORAGE_FACILITY_INDEX)] == 0x10,
        "storage facility index no longer selects its entry dialogue and return state"
    );
    ensure!(
        FACILITY_HANDLER_TARGETS[usize::from(FACILITY_ITEM_LIST_COMPOSITION_STATE)]
            == FACILITY_ITEM_LIST_COMPOSER
            && FACILITY_ITEM_LIST_DIALOGUE_RECORD_WRITES
                .iter()
                .all(|write| FACILITY_IMMEDIATE_RECORD_WRITES.contains(write))
            && FACILITY_ITEM_LIST_SETTLED_STATE
                == FACILITY_ITEM_LIST_COMPOSITION_STATE.wrapping_add(1),
        "storage item-list state no longer follows both store record 0x2A and withdraw record 0x2C before advancing into state 0x07"
    );
    ensure!(
        FACILITY_HANDLER_TARGETS[usize::from(FACILITY_ACTION_MENU_ENTRY_STATE)] == 0x9E07
            && FACILITY_HANDLER_TARGETS[usize::from(FACILITY_RESULT_DIALOGUE_STATE)] == 0x9C02
            && FACILITY_HANDLER_TARGETS[usize::from(FACILITY_CHOICE_COMPOSITION_STATE)]
                == FACILITY_CHOICE_COMPOSER
            && FACILITY_HANDLER_TARGETS[usize::from(FACILITY_CHOICE_INPUT_STATE)]
                == FACILITY_CHOICE_INPUT
            && FACILITY_IMMEDIATE_RECORD_WRITES
                .iter()
                .filter(|(_, record)| *record == FACILITY_CHOICE_DIALOGUE_RECORD)
                .map(|(address, _)| *address)
                .collect::<BTreeSet<_>>()
                == BTreeSet::from([0x9ECC, 0x9FB4]),
        "storage result dialogue no longer reaches the shared yes-no choice and action-menu return"
    );
    ensure!(
        OVERFLOW_HANDLER_TARGETS[usize::from(FACILITY_ITEM_LIST_COMPOSITION_STATE)]
            == OVERFLOW_ITEM_LIST_COMPOSER
            && OVERFLOW_IMMEDIATE_RECORD_WRITES.contains(&(0xB1BE, 0x42)),
        "storage overflow item-list state no longer follows the full-storage dialogue into state 0x24"
    );

    let facility_region = source_region(
        source,
        FACILITY_STATE_MACHINE_START,
        FACILITY_STATE_MACHINE_END,
    )?;
    let facility_immediate = scan_immediate_record_writes(
        facility_region,
        FACILITY_STATE_MACHINE_START,
        DIALOGUE_RECORD_ADDRESS,
    )?;
    ensure!(
        facility_immediate == FACILITY_IMMEDIATE_RECORD_WRITES.into_iter().collect(),
        "storage facility immediate dialogue-record writers changed: {facility_immediate:04X?}"
    );
    let facility_state_writes = scan_immediate_byte_writes(
        facility_region,
        FACILITY_STATE_MACHINE_START,
        CALLER_STATE_ADDRESS,
        "storage facility immediate state writer",
    )?;
    ensure!(
        facility_state_writes
            .values()
            .all(|state| usize::from(*state) < FACILITY_HANDLER_TARGETS.len()),
        "storage facility writes a caller state outside its dispatch domain"
    );
    let facility_action_menu_return_root_record_indices = records_immediately_entering_state(
        &facility_immediate,
        &facility_state_writes,
        FACILITY_RESULT_DIALOGUE_STATE,
    )?
    .into_iter()
    .map(usize::from)
    .collect::<BTreeSet<_>>();
    ensure!(
        !facility_action_menu_return_root_record_indices.is_empty(),
        "storage facility has no result dialogue capable of returning to the action menu"
    );
    bind_direct_store_denominator(
        facility_region,
        FACILITY_STATE_MACHINE_START,
        &FACILITY_IMMEDIATE_RECORD_WRITES,
        &[0xA09B],
        "storage facility",
    )?;

    let overflow_region = source_region(
        source,
        OVERFLOW_STATE_MACHINE_START,
        OVERFLOW_STATE_MACHINE_END,
    )?;
    let overflow_immediate = scan_immediate_record_writes(
        overflow_region,
        OVERFLOW_STATE_MACHINE_START,
        DIALOGUE_RECORD_ADDRESS,
    )?;
    ensure!(
        overflow_immediate == OVERFLOW_IMMEDIATE_RECORD_WRITES.into_iter().collect(),
        "storage overflow immediate dialogue-record writers changed: {overflow_immediate:04X?}"
    );
    bind_direct_store_denominator(
        overflow_region,
        OVERFLOW_STATE_MACHINE_START,
        &OVERFLOW_IMMEDIATE_RECORD_WRITES,
        &[],
        "storage overflow",
    )?;
    bind_exact_code(
        source,
        0xB17A,
        &OVERFLOW_ACTION_MENU_ENTRY,
        "storage overflow action-menu entry",
    )?;
    bind_exact_code(
        source,
        0xB221,
        &OVERFLOW_HELD_ITEM_DISCARD,
        "storage overflow held-item discard branch",
    )?;
    let branch_target = 0xB221u16
        .checked_add(OVERFLOW_HELD_ITEM_DISCARD.len() as u16)
        .and_then(|next| next.checked_add(u16::from(OVERFLOW_HELD_ITEM_DISCARD[8])))
        .context("storage overflow discard branch target overflow")?;
    ensure!(
        branch_target == 0xB25B,
        "storage overflow record 0x46 no longer joins the common record store"
    );

    let mut facility_root_record_indices = FACILITY_IMMEDIATE_RECORD_WRITES
        .iter()
        .map(|(_, record)| usize::from(*record))
        .collect::<BTreeSet<_>>();
    facility_root_record_indices.insert(usize::from(
        FACILITY_INITIAL_RECORDS[usize::from(STORAGE_FACILITY_INDEX)],
    ));
    facility_root_record_indices.insert(usize::from(
        FACILITY_EXIT_RECORDS[usize::from(STORAGE_FACILITY_INDEX)],
    ));
    ensure!(
        facility_root_record_indices
            == BTreeSet::from([6, 41, 42, 43, 44, 45, 46, 59, 60, 61, 62, 63]),
        "storage facility dialogue root population changed"
    );

    let mut overflow_root_record_indices = OVERFLOW_IMMEDIATE_RECORD_WRITES
        .iter()
        .map(|(_, record)| usize::from(*record))
        .collect::<BTreeSet<_>>();
    overflow_root_record_indices.insert(usize::from(OVERFLOW_HELD_ITEM_DISCARD[6]));
    ensure!(
        overflow_root_record_indices == BTreeSet::from([64, 65, 66, 67, 69, 70]),
        "storage overflow dialogue root population changed"
    );

    let mut identity = Vec::new();
    identity.extend_from_slice(&FACILITY_INITIALIZER);
    identity.extend_from_slice(&FACILITY_INITIAL_RECORDS);
    identity.extend_from_slice(facility_region);
    identity.extend_from_slice(overflow_region);

    Ok(StorageSourceBinding {
        facility_root_record_indices,
        overflow_root_record_indices,
        facility_overlay_root_record_index: usize::from(
            FACILITY_INITIAL_RECORDS[usize::from(STORAGE_FACILITY_INDEX)],
        ),
        overflow_overlay_root_record_index: 0x40,
        facility_item_list_root_record_indices: FACILITY_ITEM_LIST_DIALOGUE_RECORD_WRITES
            .iter()
            .map(|(_, record)| usize::from(*record))
            .collect(),
        facility_action_menu_return_root_record_indices,
        facility_choice_record_index: usize::from(FACILITY_CHOICE_DIALOGUE_RECORD),
        overflow_item_list_overlay_root_record_index: usize::from(
            OVERFLOW_ITEM_LIST_DIALOGUE_RECORD,
        ),
        item_list_settled_state: FACILITY_ITEM_LIST_SETTLED_STATE,
        item_list_route: super::StorageItemListRuntimeRoute {
            caller_state_address: CALLER_STATE_ADDRESS,
            composition_state: FACILITY_ITEM_LIST_COMPOSITION_STATE,
            facility_composite_state: FACILITY_ITEM_LIST_COMPOSITE_STATE,
            overflow_composite_state: OVERFLOW_ITEM_LIST_COMPOSITE_STATE,
        },
        source_dispatch_count: 2,
        source_direct_record_store_count: FACILITY_IMMEDIATE_RECORD_WRITES.len()
            + 1
            + OVERFLOW_IMMEDIATE_RECORD_WRITES.len(),
        source_binding_sha1: sha1_hex(&identity),
    })
}

fn bind_dispatch(
    dispatches: &[crate::dialogue_inventory::CallerHandoffStateDispatchSource],
    call_address: u16,
    expected_targets: &[u16],
    role: &str,
) -> Result<()> {
    let dispatch = dispatches
        .iter()
        .find(|dispatch| {
            dispatch.prg_bank() == SOURCE_BANK && dispatch.call_address() == call_address
        })
        .with_context(|| format!("{role} caller-handoff dispatch is absent"))?;
    ensure!(
        dispatch.selector_address() == CALLER_STATE_ADDRESS
            && dispatch.selector_domain()
                == &(0..expected_targets.len() as u8).collect::<BTreeSet<_>>()
            && dispatch
                .selector_domain()
                .iter()
                .all(|selector| dispatch.handler_target(*selector)
                    == Some(expected_targets[usize::from(*selector)])),
        "{role} caller-handoff state domain changed"
    );
    Ok(())
}

fn bind_direct_store_denominator(
    region: &[u8],
    origin: u16,
    immediate_writes: &[(u16, u8)],
    additional_stores: &[u16],
    role: &str,
) -> Result<()> {
    let actual = region
        .windows(3)
        .enumerate()
        .filter(|(_, window)| *window == [0x8D, 0xF1, 0x77])
        .map(|(offset, _)| origin + offset as u16)
        .collect::<BTreeSet<_>>();
    let expected = immediate_writes
        .iter()
        .map(|(address, _)| address + 2)
        .chain(additional_stores.iter().copied())
        .collect::<BTreeSet<_>>();
    ensure!(
        actual == expected,
        "{role} direct dialogue-record store denominator changed: {actual:04X?}"
    );
    Ok(())
}

fn scan_immediate_record_writes(
    region: &[u8],
    origin: u16,
    target_address: u16,
) -> Result<BTreeMap<u16, u8>> {
    scan_immediate_byte_writes(
        region,
        origin,
        target_address,
        "immediate dialogue-record writer",
    )
}

fn scan_immediate_byte_writes(
    region: &[u8],
    origin: u16,
    target_address: u16,
    role: &str,
) -> Result<BTreeMap<u16, u8>> {
    let [target_low, target_high] = target_address.to_le_bytes();
    let mut writes = BTreeMap::new();
    for (offset, window) in region.windows(5).enumerate() {
        if window[0] != 0xA9 || window[2..] != [0x8D, target_low, target_high] {
            continue;
        }
        let address = origin
            .checked_add(offset as u16)
            .with_context(|| format!("{role} address overflow"))?;
        decode_rp2a03_sequence(window, address, role)?;
        ensure!(
            writes.insert(address, window[1]).is_none(),
            "duplicate {role} at ${address:04X}"
        );
    }
    Ok(writes)
}

/// Returns every dialogue record that is immediately followed by a write of the requested caller
/// state. The source uses this pair to publish a result dialogue and enter the shared yes-no path;
/// choosing yes later returns to the action menu without replacing the record.
fn records_immediately_entering_state(
    record_writes: &BTreeMap<u16, u8>,
    state_writes: &BTreeMap<u16, u8>,
    target_state: u8,
) -> Result<BTreeSet<u8>> {
    let mut records = BTreeSet::new();
    for (state_address, _) in state_writes
        .iter()
        .filter(|(_, state)| **state == target_state)
    {
        let record_address = state_address
            .checked_sub(5)
            .context("immediate state write has no preceding instruction slot")?;
        let record = record_writes.get(&record_address).with_context(|| {
            format!(
                "caller state {target_state:02X} at ${state_address:04X} has no immediately preceding dialogue-record write"
            )
        })?;
        records.insert(*record);
    }
    Ok(records)
}

fn bind_exact_code(source: &Rom, address: u16, expected: &[u8], role: &str) -> Result<()> {
    bind_exact_bytes(source, address, expected, role)?;
    decode_rp2a03_sequence(expected, address, role)?;
    Ok(())
}

fn bind_exact_bytes(source: &Rom, address: u16, expected: &[u8], role: &str) -> Result<()> {
    ensure!(
        source_bytes(source, address, expected.len())? == expected,
        "{role} changed at bank {SOURCE_BANK:02X}:${address:04X}"
    );
    Ok(())
}

fn source_region(source: &Rom, start: u16, end: u16) -> Result<&[u8]> {
    ensure!(start < end, "source region is empty or reversed");
    source_bytes(source, start, usize::from(end - start))
}

fn source_bytes(source: &Rom, address: u16, byte_count: usize) -> Result<&[u8]> {
    let offset = switchable_cpu_to_file_offset(SOURCE_BANK, address)?;
    source
        .data()
        .get(offset..offset + byte_count)
        .with_context(|| format!("bank {SOURCE_BANK:02X}:${address:04X} is outside the ROM"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn immediate_record_writer_scan_rejects_an_added_route() {
        let mut region = vec![0xEA; 16];
        region[2..7].copy_from_slice(&[0xA9, 0x29, 0x8D, 0xF1, 0x77]);
        region[9..14].copy_from_slice(&[0xA9, 0x2A, 0x8D, 0xF1, 0x77]);

        let writes = scan_immediate_record_writes(&region, 0x9000, 0x77F1).unwrap();

        assert_eq!(writes, BTreeMap::from([(0x9002, 0x29), (0x9009, 0x2A)]));
        assert_ne!(writes, BTreeMap::from([(0x9002, 0x29)]));
    }

    #[test]
    fn direct_store_denominator_includes_table_driven_store() {
        let region = [
            0xA9, 0x29, 0x8D, 0xF1, 0x77, 0xBD, 0x00, 0x90, 0x8D, 0xF1, 0x77,
        ];

        bind_direct_store_denominator(
            &region,
            0x9000,
            &[(0x9000, 0x29)],
            &[0x9008],
            "synthetic storage",
        )
        .unwrap();
    }

    #[test]
    fn action_menu_return_population_includes_every_result_dialogue_state_entry() {
        let records = BTreeMap::from([(0x9000, 0x2D), (0x9010, 0x3B), (0x9020, 0x2A)]);
        let states = BTreeMap::from([(0x9005, 0x0C), (0x9015, 0x0C), (0x9025, 0x06)]);

        assert_eq!(
            records_immediately_entering_state(&records, &states, 0x0C).unwrap(),
            BTreeSet::from([0x2D, 0x3B])
        );
    }

    #[test]
    fn action_menu_return_population_rejects_an_unbound_state_entry() {
        let error = records_immediately_entering_state(
            &BTreeMap::from([(0x9000, 0x2D)]),
            &BTreeMap::from([(0x9015, 0x0C)]),
            0x0C,
        )
        .unwrap_err();

        assert!(error.to_string().contains("no immediately preceding"));
    }

    #[test]
    fn store_and_withdraw_records_feed_the_shared_item_list_consumer() {
        let records = FACILITY_ITEM_LIST_DIALOGUE_RECORD_WRITES
            .iter()
            .map(|(_, record)| *record)
            .collect::<BTreeSet<_>>();

        assert_eq!(records, BTreeSet::from([0x2A, 0x2C]));
        assert!(
            FACILITY_ITEM_LIST_DIALOGUE_RECORD_WRITES
                .iter()
                .all(|write| FACILITY_IMMEDIATE_RECORD_WRITES.contains(write))
        );
        assert_eq!(
            FACILITY_HANDLER_TARGETS[usize::from(FACILITY_ITEM_LIST_COMPOSITION_STATE)],
            FACILITY_ITEM_LIST_COMPOSER
        );
    }
}
