use std::collections::BTreeSet;

use anyhow::{Result, ensure};

use crate::{
    rom::Rom,
    source_direct_memory_writers::{DirectMemoryWriter, scan_direct_memory_writers},
};

use super::super::super::super::super::control_state::{MAP_EVENT_STATE, VICTORY_STAGE};
use super::super::super::super::selector_transition_graph::{StateTransition, reachable_selectors};
use super::bind_exact_code;

const VICTORY_STAGE_WRITERS: [DirectMemoryWriter; 9] = [
    map_preparation_writer(0x9A47, 0xEE, VICTORY_STAGE),
    map_preparation_writer(0x9A7B, 0x8D, VICTORY_STAGE),
    map_preparation_writer(0x9A9A, 0xEE, VICTORY_STAGE),
    map_preparation_writer(0x9AC8, 0xEE, VICTORY_STAGE),
    map_preparation_writer(0x9B3F, 0xEE, VICTORY_STAGE),
    map_preparation_writer(0x9D88, 0xEE, VICTORY_STAGE),
    map_preparation_writer(0x9DFE, 0x8D, VICTORY_STAGE),
    map_preparation_writer(0x9F17, 0xEE, VICTORY_STAGE),
    map_preparation_writer(0x9F3E, 0x8D, VICTORY_STAGE),
];

const MAP_EVENT_STATE_WRITERS: [DirectMemoryWriter; 11] = [
    map_preparation_writer(0x8CEA, 0x8D, MAP_EVENT_STATE),
    map_preparation_writer(0x8D62, 0xEE, MAP_EVENT_STATE),
    map_preparation_writer(0x8D69, 0x8D, MAP_EVENT_STATE),
    map_preparation_writer(0x8DEF, 0x8D, MAP_EVENT_STATE),
    map_preparation_writer(0x8E30, 0x8D, MAP_EVENT_STATE),
    map_preparation_writer(0x8E4E, 0xEE, MAP_EVENT_STATE),
    map_preparation_writer(0x8F50, 0x8D, MAP_EVENT_STATE),
    map_preparation_writer(0x9F95, 0xEE, MAP_EVENT_STATE),
    map_preparation_writer(0x9FC4, 0x8D, MAP_EVENT_STATE),
    map_preparation_writer(0xA072, 0xEE, MAP_EVENT_STATE),
    map_preparation_writer(0xA089, 0x8D, MAP_EVENT_STATE),
];

const fn map_preparation_writer(
    cpu_address: u16,
    opcode: u8,
    target_address: u16,
) -> DirectMemoryWriter {
    DirectMemoryWriter::new(0x03, cpu_address, opcode, target_address)
}

pub(super) fn bind_selector_writer_census(source: &Rom) -> Result<()> {
    let actual = scan_direct_memory_writers(source.prg(), &[VICTORY_STAGE, MAP_EVENT_STATE])?;
    let expected = VICTORY_STAGE_WRITERS
        .into_iter()
        .chain(MAP_EVENT_STATE_WRITERS)
        .collect::<BTreeSet<_>>();
    ensure!(
        actual == expected,
        "map-preparation nested selector writer census changed: expected {expected:?}, found {actual:?}"
    );
    Ok(())
}

pub(super) fn bind_state_transition_sources(source: &Rom) -> Result<()> {
    for (address, bytes, role) in [
        (
            0x9A47,
            &[0xEE, 0x3E, 0x05][..],
            "advance first victory stage",
        ),
        (
            0x9A79,
            &[0xA9, 0x00, 0x8D, 0x3E, 0x05][..],
            "reset first victory stage",
        ),
        (0x9A9A, &[0xEE, 0x3E, 0x05][..], "begin first victory stage"),
        (
            0x9AC8,
            &[0xEE, 0x3E, 0x05][..],
            "advance the victory sentinel stage",
        ),
        (
            0x9B3F,
            &[0xEE, 0x3E, 0x05][..],
            "advance second victory stage",
        ),
        (
            0x9D88,
            &[0xEE, 0x3E, 0x05][..],
            "advance shared victory dialogue stage",
        ),
        (
            0x9DFC,
            &[0xA9, 0x00, 0x8D, 0x3E, 0x05][..],
            "reset second victory stage",
        ),
        (
            0x9F17,
            &[0xEE, 0x3E, 0x05][..],
            "advance third victory stage",
        ),
        (
            0x9F3C,
            &[0xA9, 0x00, 0x8D, 0x3E, 0x05][..],
            "reset third victory stage",
        ),
        (
            0x8CE8,
            &[0xA9, 0x04, 0x8D, 0x42, 0x05][..],
            "select map-event state four",
        ),
        (
            0x8D62,
            &[0xEE, 0x42, 0x05][..],
            "advance map-event state zero",
        ),
        (
            0x8D67,
            &[0xA9, 0x04, 0x8D, 0x42, 0x05][..],
            "fallback to map-event state four",
        ),
        (
            0x8DED,
            &[0xA9, 0x03, 0x8D, 0x42, 0x05][..],
            "select map-event state three",
        ),
        (
            0x8E2E,
            &[0xA9, 0x00, 0x8D, 0x42, 0x05][..],
            "reset map-event state three",
        ),
        (
            0x8E4E,
            &[0xEE, 0x42, 0x05][..],
            "advance map-event state one",
        ),
        (
            0x8F4E,
            &[0xA9, 0x00, 0x8D, 0x42, 0x05][..],
            "reset map-event state four",
        ),
        (
            0x9F95,
            &[0xEE, 0x42, 0x05][..],
            "advance short map-event state",
        ),
        (
            0x9FC2,
            &[0xA9, 0x00, 0x8D, 0x42, 0x05][..],
            "reset short map-event state",
        ),
        (
            0xA072,
            &[0xEE, 0x42, 0x05][..],
            "advance two-phase map-event state",
        ),
        (
            0xA087,
            &[0xA9, 0x00, 0x8D, 0x42, 0x05][..],
            "reset two-phase map-event state",
        ),
    ] {
        bind_exact_code(source, address, bytes, role)?;
    }
    for (address, selector_address, role) in [
        (0x99FE, VICTORY_STAGE, "load first victory stage"),
        (0x9ABA, VICTORY_STAGE, "load second victory stage"),
        (0x9EEE, VICTORY_STAGE, "load third victory stage"),
        (0x8CCE, MAP_EVENT_STATE, "load map-event state"),
        (0x9F42, MAP_EVENT_STATE, "load short map-event state"),
        (0xA038, MAP_EVENT_STATE, "load two-phase map-event state"),
    ] {
        let [low, high] = selector_address.to_le_bytes();
        bind_exact_code(source, address, &[0xAD, low, high, 0x20, 0x4C, 0xC3], role)?;
    }
    Ok(())
}

pub(super) fn victory_stage_domain() -> Result<BTreeSet<u8>> {
    let handlers = (0..4).collect::<BTreeSet<_>>();
    let transitions = [
        StateTransition::new(0, 1),
        StateTransition::new(1, 2),
        StateTransition::new(2, 0),
        StateTransition::new(2, 3),
    ];
    reachable_selectors("victory stage", &handlers, [0], transitions)
}

pub(super) fn map_event_state_domain() -> Result<BTreeSet<u8>> {
    let handlers = (0..6).collect::<BTreeSet<_>>();
    let transitions = [
        StateTransition::new(0, 1),
        StateTransition::new(0, 4),
        StateTransition::new(1, 2),
        StateTransition::new(2, 3),
        StateTransition::new(3, 0),
        StateTransition::new(4, 0),
        StateTransition::new(4, 3),
    ];
    reachable_selectors("map-event state", &handlers, [0], transitions)
}

pub(super) fn two_phase_map_event_state_domain() -> Result<BTreeSet<u8>> {
    reachable_selectors(
        "two-phase map-event state",
        &BTreeSet::from([0, 1]),
        [0],
        [StateTransition::new(0, 1), StateTransition::new(1, 0)],
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn state_lifecycles_exclude_only_their_unproduced_sentinels() {
        assert_eq!(
            victory_stage_domain().unwrap(),
            (0..=3).collect::<BTreeSet<_>>()
        );
        assert_eq!(
            map_event_state_domain().unwrap(),
            (0..=4).collect::<BTreeSet<_>>()
        );
        assert_eq!(
            two_phase_map_event_state_domain().unwrap(),
            BTreeSet::from([0, 1])
        );
    }
}
