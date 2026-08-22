use std::collections::{BTreeMap, BTreeSet};

use anyhow::{Context, Result, ensure};

use crate::{
    mapper165::inline_pointer_dispatch::bind_inline_pointer_dispatch, rom::Rom,
    typed_source::decode_rp2a03_sequence,
};

use super::super::super::super::control_state::{MAP_EVENT_STATE, VICTORY_STAGE};
use super::super::ScreenSubstateDispatch;
use super::source_bytes;

mod computed_selectors;
mod state_lifecycles;

use computed_selectors::{
    bind_computed_selector_sources, event_code_selector_domain, map_direction_selector_domain,
    unit_kind_selector_domain,
};
use state_lifecycles::{
    bind_selector_writer_census, bind_state_transition_sources, map_event_state_domain,
    two_phase_map_event_state_domain, victory_stage_domain,
};

const MAP_PREPARATION_BANK: u8 = 0x03;
const INLINE_POINTER_DISPATCH_CALL: [u8; 3] = [0x20, 0x4C, 0xC3];
const PRG_BANK_BYTE_COUNT: usize = 16 * 1024;
const FIXED_PRG_BANK: usize = 15;

const UNIT_KIND_TARGETS: [u16; 4] = [0x8415, 0x8524, 0x8547, 0x8548];
const MAP_DIRECTION_TARGETS: [u16; 7] = [0x89CE, 0x89EA, 0x8A07, 0x8A2C, 0x8A43, 0x8A5D, 0x8A6E];
const MAP_EVENT_TARGETS: [u16; 6] = [0x8CE0, 0x8E48, 0x8DC0, 0x8DF3, 0x8ECC, 0xC73D];
const FIRST_VICTORY_TARGETS: [u16; 4] = [0x9A91, 0x9A0C, 0x9A4B, 0xC73D];
const SECOND_VICTORY_TARGETS: [u16; 4] = [0x9ACC, 0x9B46, 0x9D8C, 0xC73D];
const EVENT_CODE_TARGETS: [u16; 8] = [
    0x9C6E, 0x9C7C, 0x9C8A, 0x9C98, 0x9CA6, 0x9CB4, 0x9CC2, 0x9CD0,
];
const THIRD_VICTORY_TARGETS: [u16; 4] = [0x9EFC, 0x9D78, 0x9F1B, 0xC73D];
const SHORT_MAP_EVENT_TARGETS: [u16; 3] = [0x9F4E, 0x9FA1, 0xC73D];
const TWO_PHASE_MAP_EVENT_TARGETS: [u16; 2] = [0xA042, 0xA07E];

const BANK_THREE_INLINE_DISPATCH_CALLS: [u16; 10] = [
    0x800F, 0x840A, 0x89BD, 0x8CD1, 0x9A01, 0x9ABD, 0x9BB7, 0x9EF1, 0x9F45, 0xA03B,
];

#[derive(Clone, Copy)]
struct NestedDispatchSpec {
    call_address: u16,
    targets: &'static [u16],
    selector_memory_address: Option<u16>,
    role: &'static str,
}

pub(super) fn bind_nested_map_preparation_dispatches(
    source: &Rom,
) -> Result<Vec<ScreenSubstateDispatch>> {
    source.verify_supported_japanese()?;
    bind_complete_inline_dispatch_call_set(source)?;
    bind_selector_writer_census(source)?;
    bind_computed_selector_sources(source)?;
    bind_state_transition_sources(source)?;

    let unit_kind_domain = unit_kind_selector_domain();
    let map_direction_domain = map_direction_selector_domain();
    let event_code_domain = event_code_selector_domain();
    let victory_domain = victory_stage_domain()?;
    let map_event_domain = map_event_state_domain()?;
    let two_phase_map_event_domain = two_phase_map_event_state_domain()?;

    let specs = [
        (
            NestedDispatchSpec {
                call_address: 0x840A,
                targets: &UNIT_KIND_TARGETS,
                selector_memory_address: None,
                role: "map-preparation unit-kind dispatch",
            },
            unit_kind_domain,
        ),
        (
            NestedDispatchSpec {
                call_address: 0x89BD,
                targets: &MAP_DIRECTION_TARGETS,
                selector_memory_address: None,
                role: "map-preparation guarded direction dispatch",
            },
            map_direction_domain,
        ),
        (
            NestedDispatchSpec {
                call_address: 0x8CD1,
                targets: &MAP_EVENT_TARGETS,
                selector_memory_address: Some(MAP_EVENT_STATE),
                role: "map-preparation map-event state dispatch",
            },
            map_event_domain,
        ),
        (
            NestedDispatchSpec {
                call_address: 0x9A01,
                targets: &FIRST_VICTORY_TARGETS,
                selector_memory_address: Some(VICTORY_STAGE),
                role: "map-preparation first victory-stage dispatch",
            },
            victory_domain.clone(),
        ),
        (
            NestedDispatchSpec {
                call_address: 0x9ABD,
                targets: &SECOND_VICTORY_TARGETS,
                selector_memory_address: Some(VICTORY_STAGE),
                role: "map-preparation second victory-stage dispatch",
            },
            victory_domain.clone(),
        ),
        (
            NestedDispatchSpec {
                call_address: 0x9BB7,
                targets: &EVENT_CODE_TARGETS,
                selector_memory_address: None,
                role: "map-preparation bounded event-code dispatch",
            },
            event_code_domain,
        ),
        (
            NestedDispatchSpec {
                call_address: 0x9EF1,
                targets: &THIRD_VICTORY_TARGETS,
                selector_memory_address: Some(VICTORY_STAGE),
                role: "map-preparation third victory-stage dispatch",
            },
            victory_domain,
        ),
        (
            NestedDispatchSpec {
                call_address: 0x9F45,
                targets: &SHORT_MAP_EVENT_TARGETS,
                selector_memory_address: Some(MAP_EVENT_STATE),
                role: "map-preparation short map-event dispatch",
            },
            two_phase_map_event_domain.clone(),
        ),
        (
            NestedDispatchSpec {
                call_address: 0xA03B,
                targets: &TWO_PHASE_MAP_EVENT_TARGETS,
                selector_memory_address: Some(MAP_EVENT_STATE),
                role: "map-preparation two-phase map-event dispatch",
            },
            two_phase_map_event_domain,
        ),
    ];

    specs
        .into_iter()
        .map(|(spec, produced_selectors)| bind_dispatch(source, spec, produced_selectors))
        .collect()
}

fn bind_dispatch(
    source: &Rom,
    spec: NestedDispatchSpec,
    produced_selectors: BTreeSet<u8>,
) -> Result<ScreenSubstateDispatch> {
    let handler_domain = (0..u8::try_from(spec.targets.len())?).collect::<BTreeSet<_>>();
    ensure!(
        !produced_selectors.is_empty() && produced_selectors.is_subset(&handler_domain),
        "{} producer domain escapes its handler table",
        spec.role
    );
    let binding = bind_inline_pointer_dispatch(
        source,
        MAP_PREPARATION_BANK,
        spec.call_address,
        handler_domain.iter().copied(),
        spec.role,
    )?;
    ensure!(
        binding.targets_in_selector_order() == spec.targets,
        "{} handlers changed",
        spec.role
    );
    Ok(ScreenSubstateDispatch {
        prg_bank: MAP_PREPARATION_BANK,
        call_address: spec.call_address,
        handler_domain,
        selector_memory_address: spec.selector_memory_address,
        source_bound_produced_selectors: Some(produced_selectors),
        indirect_write_destinations: BTreeMap::new(),
        role: spec.role,
    })
}

fn bind_complete_inline_dispatch_call_set(source: &Rom) -> Result<()> {
    let bank_start = usize::from(MAP_PREPARATION_BANK) * PRG_BANK_BYTE_COUNT;
    let bank = source
        .prg()
        .get(bank_start..bank_start + PRG_BANK_BYTE_COUNT)
        .context("map-preparation bank exceeds source PRG")?;
    let fixed = source
        .prg()
        .get(FIXED_PRG_BANK * PRG_BANK_BYTE_COUNT..(FIXED_PRG_BANK + 1) * PRG_BANK_BYTE_COUNT)
        .context("fixed bank exceeds source PRG")?;
    let mut calls = bank
        .windows(INLINE_POINTER_DISPATCH_CALL.len())
        .enumerate()
        .filter(|(_, bytes)| *bytes == INLINE_POINTER_DISPATCH_CALL)
        .map(|(offset, _)| 0x8000 + u16::try_from(offset).expect("bank offset fits u16"))
        .collect::<BTreeSet<_>>();
    for (address, bytes) in [
        (0xBFFE, [bank[0x3FFE], bank[0x3FFF], fixed[0]]),
        (0xBFFF, [bank[0x3FFF], fixed[0], fixed[1]]),
    ] {
        if bytes == INLINE_POINTER_DISPATCH_CALL {
            calls.insert(address);
        }
    }
    ensure!(
        calls == BTreeSet::from(BANK_THREE_INLINE_DISPATCH_CALLS),
        "bank03 inline pointer-dispatch call census changed: expected {:?}, found {calls:?}",
        BTreeSet::from(BANK_THREE_INLINE_DISPATCH_CALLS),
    );
    Ok(())
}

fn bind_exact_code(source: &Rom, address: u16, expected: &[u8], role: &str) -> Result<()> {
    let actual = source_bytes(source, address, expected.len())?;
    ensure!(actual == expected, "{role} source bytes changed");
    decode_rp2a03_sequence(actual, address, role)?;
    Ok(())
}
