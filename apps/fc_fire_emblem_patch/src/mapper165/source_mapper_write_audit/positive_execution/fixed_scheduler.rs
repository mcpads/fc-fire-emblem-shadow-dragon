use std::collections::{BTreeMap, BTreeSet};

use anyhow::{Context, Result, ensure};
use retro_rp2a03::decode_bytes;

use crate::{
    mapper165::{
        battle_codebook_plan::IndirectWriteDestinationBounds,
        inline_pointer_dispatch::bind_inline_pointer_dispatch,
    },
    rom::Rom,
    sha1_hex,
    title_graphics::TitleStateExecution,
    typed_source::decode_rp2a03_sequence,
};

use super::fixed_vectors::trace_fixed_scheduler_contexts;

const SOURCE_PRG_BANK_BYTE_COUNT: usize = 16 * 1024;
const FIXED_PRG_BANK: u8 = 0x0F;
const SWITCHABLE_CPU_START: u16 = 0x8000;
const FIXED_CPU_START: u16 = 0xC000;

pub(super) const FIXED_SCHEDULER_STATE_LOAD: u16 = 0xF2A0;
pub(super) const FIXED_SCHEDULER_DISPATCH_CALL: u16 = 0xF2A2;
const FIXED_SCHEDULER_POINTER_TABLE: u16 = 0xF2A5;
const FIXED_SCHEDULER_CALL_SITE: u16 = 0xC148;
const FIXED_SCHEDULER_CALL: [u8; 3] = [0x20, 0x8F, 0xF2];
const FIXED_SCHEDULER_RETURN_ADDRESS: u16 = 0xC14B;
const FIXED_SCHEDULER_STATE_COUNT: u8 = 6;
const FIXED_SCHEDULER_TARGETS: [u16; FIXED_SCHEDULER_STATE_COUNT as usize] =
    [0xC034, 0xF2CB, 0xF2D8, 0xC73D, 0xF323, 0xF32A];

const MAP_INITIALIZATION_ENTRY: u16 = 0xF2CB;
const MAP_INITIALIZATION_ENTRY_BYTES: [u8; 13] = [
    0xA9, 0x02, 0x20, 0xA6, 0xC9, 0x20, 0xFA, 0xBF, 0xA9, 0x06, 0x4C, 0xA6, 0xC9,
];
const MAP_INITIALIZATION_BANK: u8 = 0x02;
const MAP_INITIALIZATION_TRAMPOLINE: u16 = 0xBFFA;
const MAP_INITIALIZATION_TRAMPOLINE_BYTES: [u8; 3] = [0x4C, 0x4E, 0xA6];
const MAP_INITIALIZATION_DISPATCH_CALL: u16 = 0xA653;
const MAP_INITIALIZATION_SELECTOR_DOMAIN: [u8; 3] = [0x02, 0x03, 0x04];
const MAP_INITIALIZATION_TRANSITION_ROOTS: [u16; 3] = [0xA6EF, 0xA718, 0xA75D];
const MAP_INITIALIZATION_TRANSITION_REGIONS: [(u16, usize, &str, u16); 3] = [
    (
        0xA6EF,
        0x29,
        "6707552d4fc7db13bf13575c20ffdf04f75de180",
        0xA715,
    ),
    (
        0xA718,
        0x32,
        "49f96cba8df455cc0707670f82288682570502d3",
        0xA747,
    ),
    (
        0xA75D,
        0x1D,
        "8fcc40362418399361149f4c2f06a19c531d0ea7",
        0xA777,
    ),
];
const INCREMENT_SCHEDULER_AND_RETURN: [u8; 3] = [0xE6, 0x25, 0x60];

const SOUND_TEST_ENTRY_BANK: u8 = 0x06;
const SOUND_TEST_ENTRY: u16 = 0xB748;
const SOUND_TEST_ENTRY_BYTES: [u8; 15] = [
    0xA9, 0x01, 0x8D, 0xF0, 0x06, 0xAD, 0xE0, 0x05, 0xF0, 0x05, 0xE6, 0x23, 0x4C, 0xC0, 0xFF,
];
const SOUND_TEST_FIXED_TRAMPOLINE: u16 = 0xFFC0;
const SOUND_TEST_FIXED_TRAMPOLINE_BYTES: [u8; 3] = [0x4C, 0xB4, 0xF2];
const SOUND_TEST_SCHEDULER_WRITER: u16 = 0xF2B4;
const SOUND_TEST_SCHEDULER_WRITES: [u8; 4] = [0xA9, 0x04, 0x85, 0x25];

const GAMEPLAY_SCHEDULER_BANK: u8 = 0x06;
const GAMEPLAY_SCHEDULER_REGION: u16 = 0xB9F1;
const GAMEPLAY_SCHEDULER_REGION_BYTE_COUNT: usize = 0x26;
const GAMEPLAY_SCHEDULER_REGION_SHA1: &str = "8cacd7ef1beec90afae19a73298366bb9a39d56b";
const GAMEPLAY_SCHEDULER_WRITER: u16 = 0xBA07;
const SELECT_GAMEPLAY_SCHEDULER: [u8; 4] = [0xA9, 0x05, 0x85, 0x25];

#[derive(Debug)]
pub(super) struct FixedSchedulerExecution {
    inline_dispatch: (u16, u16),
    table_selector_domain: BTreeSet<u8>,
    positive_selector_domain: BTreeSet<u8>,
    selector_targets: BTreeMap<u8, u16>,
    reset_entry_contexts: BTreeSet<(u8, u8)>,
    positive_entry_contexts: BTreeSet<(u8, u8)>,
    known_produced_states: BTreeSet<u8>,
    source_bound_producer_instruction_starts: BTreeSet<(u8, u16)>,
    reachable_instruction_starts: BTreeSet<(u8, u16)>,
    bound_switchable_roots: BTreeSet<(u8, u16)>,
    open_control_facts: Vec<String>,
}

impl FixedSchedulerExecution {
    pub(super) fn inline_dispatch(&self) -> (u16, u16) {
        self.inline_dispatch
    }

    pub(super) fn table_selector_domain(&self) -> &BTreeSet<u8> {
        &self.table_selector_domain
    }

    pub(super) fn positive_selector_domain(&self) -> &BTreeSet<u8> {
        &self.positive_selector_domain
    }

    pub(super) fn selector_targets(&self) -> &BTreeMap<u8, u16> {
        &self.selector_targets
    }

    pub(super) fn reset_entry_contexts(&self) -> &BTreeSet<(u8, u8)> {
        &self.reset_entry_contexts
    }

    pub(super) fn positive_entry_contexts(&self) -> &BTreeSet<(u8, u8)> {
        &self.positive_entry_contexts
    }

    pub(super) fn known_produced_states(&self) -> &BTreeSet<u8> {
        &self.known_produced_states
    }

    pub(super) fn source_bound_producer_instruction_starts(&self) -> &BTreeSet<(u8, u16)> {
        &self.source_bound_producer_instruction_starts
    }

    pub(super) fn reachable_instruction_starts(&self) -> &BTreeSet<(u8, u16)> {
        &self.reachable_instruction_starts
    }

    pub(super) fn bound_switchable_roots(&self) -> &BTreeSet<(u8, u16)> {
        &self.bound_switchable_roots
    }

    pub(super) fn open_control_fact_descriptions(&self) -> &[String] {
        &self.open_control_facts
    }
}

pub(super) fn bind_fixed_scheduler_execution(
    source: &Rom,
    title_state: &TitleStateExecution,
    entry_contexts: &BTreeSet<(u8, u8)>,
    indirect_write_destination_bounds: &BTreeMap<(u8, u16, u8), IndirectWriteDestinationBounds>,
) -> Result<FixedSchedulerExecution> {
    source.verify_supported_japanese()?;
    let table_selector_domain = (0..FIXED_SCHEDULER_STATE_COUNT).collect::<BTreeSet<_>>();
    let dispatch = bind_inline_pointer_dispatch(
        source,
        FIXED_PRG_BANK,
        FIXED_SCHEDULER_DISPATCH_CALL,
        table_selector_domain.iter().copied(),
        "fixed scheduler state dispatch",
    )?;
    ensure!(
        dispatch.table_start() == FIXED_SCHEDULER_POINTER_TABLE,
        "fixed scheduler pointer-table boundary changed"
    );
    ensure!(
        dispatch.targets_in_selector_order() == FIXED_SCHEDULER_TARGETS,
        "fixed scheduler state-to-handler mapping changed"
    );
    let selector_targets = table_selector_domain
        .iter()
        .copied()
        .zip(dispatch.targets_in_selector_order())
        .collect::<BTreeMap<_, _>>();

    ensure!(
        title_state.scheduler_produced_values() == &BTreeSet::from([0x00, 0x01, 0x05]),
        "title scheduler producers no longer select reset, map initialization, and gameplay states"
    );
    let mut source_bound_producer_instruction_starts = BTreeSet::new();
    bind_exact_sequence(
        source,
        FIXED_PRG_BANK,
        FIXED_SCHEDULER_CALL_SITE,
        &FIXED_SCHEDULER_CALL,
        "fixed scheduler NMI call",
        &mut source_bound_producer_instruction_starts,
    )?;
    bind_map_initialization_transition(source, &mut source_bound_producer_instruction_starts)?;
    bind_sound_test_transition(source, &mut source_bound_producer_instruction_starts)?;
    bind_gameplay_transition(source, &mut source_bound_producer_instruction_starts)?;

    let known_produced_states = title_state
        .scheduler_produced_values()
        .iter()
        .copied()
        .chain([0x02, 0x04, 0x05])
        .collect::<BTreeSet<_>>();
    ensure!(
        known_produced_states.is_subset(&table_selector_domain),
        "a fixed scheduler producer selects beyond the six-entry handler table"
    );
    ensure!(
        entry_contexts
            .iter()
            .all(|(selector, _)| known_produced_states.contains(selector)),
        "reset-rooted scheduler entry contexts contain a selector outside the source-bound producer states: {entry_contexts:?}"
    );
    let owned_inline_selector_domains = BTreeMap::from([
        (
            (0x0D, title_state.dispatch_call()),
            title_state.selector_domain().clone(),
        ),
        (
            (MAP_INITIALIZATION_BANK, MAP_INITIALIZATION_DISPATCH_CALL),
            MAP_INITIALIZATION_SELECTOR_DOMAIN.into_iter().collect(),
        ),
    ]);

    let handler_trace = trace_fixed_scheduler_contexts(
        source,
        FIXED_SCHEDULER_STATE_LOAD,
        FIXED_SCHEDULER_RETURN_ADDRESS,
        entry_contexts.iter().copied(),
        &owned_inline_selector_domains,
        indirect_write_destination_bounds,
    )?;
    let positive_selector_domain = handler_trace
        .inline_dispatch_selectors()
        .get(&(FIXED_PRG_BANK, FIXED_SCHEDULER_DISPATCH_CALL))
        .cloned()
        .unwrap_or_default();
    let expected_positive_states = BTreeSet::from([0x00, 0x01, 0x02, 0x05]);
    ensure!(
        positive_selector_domain == expected_positive_states,
        "stateful scheduler trace no longer derives the reset/title/map positive states: expected {expected_positive_states:?}, found {positive_selector_domain:?}"
    );
    let positive_entry_contexts =
        handler_trace.inline_dispatch_contexts(FIXED_PRG_BANK, FIXED_SCHEDULER_DISPATCH_CALL);
    ensure!(
        positive_entry_contexts
            == expected_positive_states
                .iter()
                .copied()
                .map(|state| (state, 0x06))
                .collect(),
        "fixed scheduler positive states no longer enter with the restored gameplay PRG bank: {positive_entry_contexts:?}"
    );
    ensure!(
        handler_trace
            .inline_dispatch_selectors()
            .get(&(0x0D, title_state.dispatch_call()))
            == Some(title_state.selector_domain()),
        "fixed scheduler trace no longer consumes the owner-bound title selector domain"
    );
    ensure!(
        handler_trace
            .inline_dispatch_selectors()
            .get(&(MAP_INITIALIZATION_BANK, MAP_INITIALIZATION_DISPATCH_CALL))
            == Some(&MAP_INITIALIZATION_SELECTOR_DOMAIN.into_iter().collect()),
        "fixed scheduler trace no longer consumes the owner-bound map-initialization selector domain"
    );
    let mut open_control_facts = handler_trace.open_fact_descriptions();
    open_control_facts
        .push("fixed_scheduler_state_04@0F:F2B6:outer_screen_route_unrooted".to_owned());
    open_control_facts.sort();
    open_control_facts.dedup();
    Ok(FixedSchedulerExecution {
        inline_dispatch: (FIXED_SCHEDULER_DISPATCH_CALL, FIXED_SCHEDULER_POINTER_TABLE),
        table_selector_domain,
        positive_selector_domain,
        selector_targets,
        reset_entry_contexts: entry_contexts.clone(),
        positive_entry_contexts,
        known_produced_states,
        source_bound_producer_instruction_starts,
        reachable_instruction_starts: handler_trace.reachable_instruction_starts().clone(),
        bound_switchable_roots: handler_trace.switchable_roots().clone(),
        open_control_facts,
    })
}

fn bind_map_initialization_transition(
    source: &Rom,
    instruction_starts: &mut BTreeSet<(u8, u16)>,
) -> Result<()> {
    bind_exact_sequence(
        source,
        FIXED_PRG_BANK,
        MAP_INITIALIZATION_ENTRY,
        &MAP_INITIALIZATION_ENTRY_BYTES,
        "map-initialization scheduler handler",
        instruction_starts,
    )?;
    bind_exact_sequence(
        source,
        MAP_INITIALIZATION_BANK,
        MAP_INITIALIZATION_TRAMPOLINE,
        &MAP_INITIALIZATION_TRAMPOLINE_BYTES,
        "map-initialization bank entry",
        instruction_starts,
    )?;
    let dispatch = bind_inline_pointer_dispatch(
        source,
        MAP_INITIALIZATION_BANK,
        MAP_INITIALIZATION_DISPATCH_CALL,
        MAP_INITIALIZATION_SELECTOR_DOMAIN,
        "map-initialization phase dispatch",
    )?;
    ensure!(
        dispatch.targets_in_selector_order() == MAP_INITIALIZATION_TRANSITION_ROOTS,
        "map-initialization phases no longer route to the scheduler transitions"
    );
    instruction_starts.insert((MAP_INITIALIZATION_BANK, MAP_INITIALIZATION_DISPATCH_CALL));

    for &(start, byte_count, expected_sha1, writer) in &MAP_INITIALIZATION_TRANSITION_REGIONS {
        let bytes = bind_hashed_region(
            source,
            MAP_INITIALIZATION_BANK,
            start,
            byte_count,
            expected_sha1,
            "map-initialization phase",
            instruction_starts,
        )?;
        let writer_offset = usize::from(writer - start);
        ensure!(
            bytes.get(writer_offset..writer_offset + INCREMENT_SCHEDULER_AND_RETURN.len())
                == Some(INCREMENT_SCHEDULER_AND_RETURN.as_slice()),
            "map-initialization phase no longer advances scheduler state one to state two"
        );
        ensure!(
            instruction_starts.contains(&(MAP_INITIALIZATION_BANK, writer)),
            "map-initialization scheduler writer is not an instruction boundary"
        );
    }
    Ok(())
}

fn bind_sound_test_transition(
    source: &Rom,
    instruction_starts: &mut BTreeSet<(u8, u16)>,
) -> Result<()> {
    bind_exact_sequence(
        source,
        SOUND_TEST_ENTRY_BANK,
        SOUND_TEST_ENTRY,
        &SOUND_TEST_ENTRY_BYTES,
        "sound-test scheduler entry",
        instruction_starts,
    )?;
    ensure!(
        SOUND_TEST_ENTRY_BYTES.ends_with(&[0x4C, 0xC0, 0xFF]),
        "sound-test scheduler entry no longer jumps through the fixed trampoline"
    );
    bind_exact_sequence(
        source,
        FIXED_PRG_BANK,
        SOUND_TEST_FIXED_TRAMPOLINE,
        &SOUND_TEST_FIXED_TRAMPOLINE_BYTES,
        "sound-test fixed scheduler trampoline",
        instruction_starts,
    )?;
    bind_exact_sequence(
        source,
        FIXED_PRG_BANK,
        SOUND_TEST_SCHEDULER_WRITER,
        &SOUND_TEST_SCHEDULER_WRITES,
        "sound-test scheduler writer",
        instruction_starts,
    )?;
    Ok(())
}

fn bind_gameplay_transition(
    source: &Rom,
    instruction_starts: &mut BTreeSet<(u8, u16)>,
) -> Result<()> {
    let region = bind_hashed_region(
        source,
        GAMEPLAY_SCHEDULER_BANK,
        GAMEPLAY_SCHEDULER_REGION,
        GAMEPLAY_SCHEDULER_REGION_BYTE_COUNT,
        GAMEPLAY_SCHEDULER_REGION_SHA1,
        "gameplay scheduler transition",
        instruction_starts,
    )?;
    let writer_offset = usize::from(GAMEPLAY_SCHEDULER_WRITER - GAMEPLAY_SCHEDULER_REGION);
    ensure!(
        region.get(writer_offset - 2..writer_offset + 2)
            == Some(SELECT_GAMEPLAY_SCHEDULER.as_slice()),
        "gameplay transition no longer selects scheduler state five"
    );
    ensure!(
        instruction_starts.contains(&(GAMEPLAY_SCHEDULER_BANK, GAMEPLAY_SCHEDULER_WRITER)),
        "gameplay scheduler writer is not an instruction boundary"
    );
    Ok(())
}

fn bind_hashed_region<'a>(
    source: &'a Rom,
    bank: u8,
    address: u16,
    byte_count: usize,
    expected_sha1: &str,
    role: &str,
    instruction_starts: &mut BTreeSet<(u8, u16)>,
) -> Result<&'a [u8]> {
    let bytes = source_cpu_bytes(source, bank, address, byte_count)?;
    ensure!(sha1_hex(bytes) == expected_sha1, "source {role} changed");
    bind_instruction_starts(bytes, bank, address, role, instruction_starts)?;
    Ok(bytes)
}

fn bind_exact_sequence(
    source: &Rom,
    bank: u8,
    address: u16,
    expected: &[u8],
    role: &str,
    instruction_starts: &mut BTreeSet<(u8, u16)>,
) -> Result<()> {
    let bytes = source_cpu_bytes(source, bank, address, expected.len())?;
    ensure!(bytes == expected, "source {role} changed");
    bind_instruction_starts(bytes, bank, address, role, instruction_starts)
}

fn bind_instruction_starts(
    bytes: &[u8],
    bank: u8,
    origin: u16,
    role: &str,
    instruction_starts: &mut BTreeSet<(u8, u16)>,
) -> Result<()> {
    decode_rp2a03_sequence(bytes, origin, role)?;
    let mut offset = 0_usize;
    while offset < bytes.len() {
        let instruction = decode_bytes(&bytes[offset..])
            .with_context(|| format!("decode {role} instruction at +0x{offset:X}"))?;
        instruction_starts.insert((bank, origin + u16::try_from(offset)?));
        offset += instruction.encoded_len();
    }
    ensure!(
        offset == bytes.len(),
        "{role} instruction layout is truncated"
    );
    Ok(())
}

fn source_cpu_bytes(source: &Rom, bank: u8, address: u16, byte_count: usize) -> Result<&[u8]> {
    ensure!(
        bank <= FIXED_PRG_BANK && address >= SWITCHABLE_CPU_START,
        "fixed scheduler source address is outside PRG space"
    );
    let physical_bank = if address >= FIXED_CPU_START {
        FIXED_PRG_BANK
    } else {
        bank
    };
    let cpu_start = if address >= FIXED_CPU_START {
        FIXED_CPU_START
    } else {
        SWITCHABLE_CPU_START
    };
    let offset = usize::from(physical_bank)
        .checked_mul(SOURCE_PRG_BANK_BYTE_COUNT)
        .and_then(|base| base.checked_add(usize::from(address - cpu_start)))
        .context("fixed scheduler source PRG offset overflow")?;
    source
        .prg()
        .get(offset..offset + byte_count)
        .with_context(|| format!("fixed scheduler source range exceeds bank {physical_bank:02X}"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_produced_state_has_a_scheduler_handler() {
        let handlers = (0..FIXED_SCHEDULER_STATE_COUNT).collect::<BTreeSet<_>>();
        let produced = BTreeSet::from([0x00, 0x01, 0x02, 0x04, 0x05]);

        assert!(produced.is_subset(&handlers));
        assert!(!produced.contains(&0x03));
    }

    #[test]
    fn handler_order_keeps_map_initialization_and_gameplay_distinct() {
        assert_eq!(FIXED_SCHEDULER_TARGETS[1], MAP_INITIALIZATION_ENTRY);
        assert_eq!(FIXED_SCHEDULER_TARGETS[5], 0xF32A);
        assert_ne!(FIXED_SCHEDULER_TARGETS[1], FIXED_SCHEDULER_TARGETS[5]);
    }
}
