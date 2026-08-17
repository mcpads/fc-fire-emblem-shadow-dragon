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

use super::fixed_vectors::{InlineDispatchSelectorBounds, trace_fixed_scheduler_contexts};
use super::shared_menu_request::SharedMenuExecutionSource;

const SOURCE_PRG_BANK_BYTE_COUNT: usize = 16 * 1024;
const FIXED_PRG_BANK: u8 = 0x0F;
const SWITCHABLE_CPU_START: u16 = 0x8000;
const FIXED_CPU_START: u16 = 0xC000;

pub(super) const FIXED_SCHEDULER_ENTRY: u16 = 0xF28F;
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
const MAP_INITIALIZATION_STATE_COUNT: u8 = 8;
const MAP_INITIALIZATION_TARGETS: [u16; MAP_INITIALIZATION_STATE_COUNT as usize] = [
    0xA67B, 0xA696, 0xA6EF, 0xA718, 0xA75D, 0xA7EF, 0xA77A, 0xA8EC,
];
const MAP_INITIALIZATION_TRANSITION_SELECTORS: [u8; 3] = [0x02, 0x03, 0x04];
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
    table_selector_domain: BTreeSet<u8>,
    positive_selector_domain: BTreeSet<u8>,
    selector_targets: BTreeMap<u8, u16>,
    reset_entry_contexts: BTreeSet<(u8, u8)>,
    positive_entry_contexts: BTreeSet<(u8, u8)>,
    known_produced_states: BTreeSet<u8>,
    source_bound_producer_instruction_starts: BTreeSet<(u8, u16)>,
    reachable_instruction_starts: BTreeSet<(u8, u16)>,
    bound_switchable_roots: BTreeSet<(u8, u16)>,
    indirect_write_sites_below_mapper_space: BTreeSet<(u8, u16, u8)>,
    open_control_facts: Vec<String>,
}

impl FixedSchedulerExecution {
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

    pub(super) fn indirect_write_sites_below_mapper_space(&self) -> &BTreeSet<(u8, u16, u8)> {
        &self.indirect_write_sites_below_mapper_space
    }

    pub(super) fn open_control_fact_descriptions(&self) -> &[String] {
        &self.open_control_facts
    }
}

pub(super) fn bind_fixed_scheduler_execution(
    source: &Rom,
    title_state: &TitleStateExecution,
    shared_menu: &SharedMenuExecutionSource,
    screen_state_selector_domains: &BTreeMap<(u8, u16), BTreeSet<u8>>,
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
    let positive_selector_domain = title_state
        .scheduler_produced_values()
        .iter()
        .copied()
        .chain([0x02])
        .collect::<BTreeSet<_>>();
    ensure!(
        positive_selector_domain.is_subset(&known_produced_states)
            && !positive_selector_domain.contains(&0x04),
        "normal fixed-scheduler states no longer separate the sound-test route"
    );
    let positive_entry_contexts = positive_selector_domain
        .iter()
        .copied()
        .map(|selector| (selector, GAMEPLAY_SCHEDULER_BANK))
        .collect::<BTreeSet<_>>();
    ensure!(
        entry_contexts.contains(&(0x00, GAMEPLAY_SCHEDULER_BANK)),
        "reset-rooted execution no longer reaches scheduler state zero in source-bound bank six: {entry_contexts:?}"
    );
    let mut inline_dispatch_selector_bounds = BTreeMap::from([
        (
            (FIXED_PRG_BANK, FIXED_SCHEDULER_DISPATCH_CALL),
            InlineDispatchSelectorBounds::from_source_producers(positive_selector_domain.clone()),
        ),
        (
            (0x0D, title_state.dispatch_call()),
            InlineDispatchSelectorBounds::from_source_producers(
                title_state.selector_domain().clone(),
            ),
        ),
        (
            (0x0D, title_state.animation_dispatch_call()),
            InlineDispatchSelectorBounds::from_source_producers(
                title_state.animation_selector_domain().clone(),
            ),
        ),
        (
            (MAP_INITIALIZATION_BANK, MAP_INITIALIZATION_DISPATCH_CALL),
            InlineDispatchSelectorBounds::from_source_producers(
                (0..MAP_INITIALIZATION_STATE_COUNT).collect(),
            ),
        ),
        (
            (0x0B, shared_menu.dispatch_call()),
            InlineDispatchSelectorBounds::from_source_producers(
                shared_menu.active_request_states().clone(),
            ),
        ),
    ]);
    for (&site, selectors) in screen_state_selector_domains {
        ensure!(
            inline_dispatch_selector_bounds
                .insert(
                    site,
                    InlineDispatchSelectorBounds::from_handler_table(selectors.clone()),
                )
                .is_none(),
            "screen-state inline dispatch duplicates existing selector bounds at {:02X}:${:04X}",
            site.0,
            site.1,
        );
    }

    let handler_trace = trace_fixed_scheduler_contexts(
        source,
        FIXED_SCHEDULER_STATE_LOAD,
        FIXED_SCHEDULER_DISPATCH_CALL,
        FIXED_SCHEDULER_RETURN_ADDRESS,
        positive_entry_contexts.iter().copied(),
        &inline_dispatch_selector_bounds,
        indirect_write_destination_bounds,
    )?;
    let observed_scheduler_contexts =
        handler_trace.inline_dispatch_contexts(FIXED_PRG_BANK, FIXED_SCHEDULER_DISPATCH_CALL);
    ensure!(
        positive_entry_contexts.is_subset(&observed_scheduler_contexts),
        "one-epoch scheduler trace lost an explicitly source-bound positive context: expected {positive_entry_contexts:?}, found {observed_scheduler_contexts:?}; open facts {:?}",
        handler_trace.open_fact_descriptions(),
    );
    let title_positive_selectors = handler_trace
        .inline_dispatch_selectors()
        .get(&(0x0D, title_state.dispatch_call()))
        .cloned()
        .unwrap_or_default();
    ensure!(
        !title_positive_selectors.is_empty()
            && title_positive_selectors.is_subset(title_state.selector_domain()),
        "fixed scheduler trace selected outside the owner-bound title selector domain: {title_positive_selectors:?}"
    );
    let title_animation_positive_selectors = handler_trace
        .inline_dispatch_selectors()
        .get(&(0x0D, title_state.animation_dispatch_call()))
        .cloned()
        .unwrap_or_default();
    ensure!(
        !title_animation_positive_selectors.is_empty()
            && title_animation_positive_selectors
                .is_subset(title_state.animation_selector_domain()),
        "fixed scheduler trace selected outside the owner-bound title animation selector domain: {title_animation_positive_selectors:?}"
    );
    let map_owned_selectors = (0..MAP_INITIALIZATION_STATE_COUNT).collect::<BTreeSet<_>>();
    let map_positive_selectors = handler_trace
        .inline_dispatch_selectors()
        .get(&(MAP_INITIALIZATION_BANK, MAP_INITIALIZATION_DISPATCH_CALL))
        .cloned()
        .unwrap_or_default();
    ensure!(
        !map_positive_selectors.is_empty()
            && map_positive_selectors.is_subset(&map_owned_selectors),
        "fixed scheduler trace selected outside the owner-bound map-initialization selector domain: {map_positive_selectors:?}"
    );
    let mut open_control_facts = handler_trace.open_fact_descriptions();
    for context in entry_contexts.difference(&BTreeSet::from([(0x00, GAMEPLAY_SCHEDULER_BANK)])) {
        open_control_facts.push(format!(
            "reset_scheduler_context_not_normal_positive={:02X}:{:02X}",
            context.0, context.1,
        ));
    }
    for context in observed_scheduler_contexts.difference(&positive_entry_contexts) {
        open_control_facts.push(format!(
            "fixed_scheduler_successor_context_outside_positive_epoch={:02X}:{:02X}",
            context.0, context.1,
        ));
    }
    record_untraced_owned_selectors(
        &mut open_control_facts,
        "title_state",
        title_state.selector_domain(),
        &title_positive_selectors,
    );
    record_untraced_owned_selectors(
        &mut open_control_facts,
        "title_animation_state",
        title_state.animation_selector_domain(),
        &title_animation_positive_selectors,
    );
    record_untraced_owned_selectors(
        &mut open_control_facts,
        "map_initialization",
        &map_owned_selectors,
        &map_positive_selectors,
    );
    open_control_facts.push("fixed_scheduler_state_03@0F:C73D:no_source_bound_producer".to_owned());
    open_control_facts
        .push("fixed_scheduler_state_04@0F:F2B6:outer_screen_route_unrooted".to_owned());
    open_control_facts.sort();
    open_control_facts.dedup();
    Ok(FixedSchedulerExecution {
        table_selector_domain,
        positive_selector_domain,
        selector_targets,
        reset_entry_contexts: entry_contexts.clone(),
        positive_entry_contexts,
        known_produced_states,
        source_bound_producer_instruction_starts,
        reachable_instruction_starts: handler_trace.reachable_instruction_starts().clone(),
        bound_switchable_roots: handler_trace.switchable_roots().clone(),
        indirect_write_sites_below_mapper_space: handler_trace
            .indirect_write_sites_below_mapper_space()
            .clone(),
        open_control_facts,
    })
}

fn record_untraced_owned_selectors(
    open_control_facts: &mut Vec<String>,
    role: &str,
    owned: &BTreeSet<u8>,
    positive: &BTreeSet<u8>,
) {
    let untraced = owned.difference(positive).copied().collect::<Vec<_>>();
    if !untraced.is_empty() {
        open_control_facts.push(format!(
            "owned_inline_dispatch@{role}:selectors_not_positive={untraced:02X?}"
        ));
    }
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
        0..MAP_INITIALIZATION_STATE_COUNT,
        "map-initialization phase dispatch",
    )?;
    ensure!(
        dispatch.targets_in_selector_order() == MAP_INITIALIZATION_TARGETS,
        "map-initialization state-to-handler mapping changed"
    );
    ensure!(
        MAP_INITIALIZATION_TRANSITION_SELECTORS
            .into_iter()
            .map(|selector| dispatch.targets_in_selector_order()[usize::from(selector)])
            .collect::<Vec<_>>()
            == MAP_INITIALIZATION_TRANSITION_ROOTS,
        "map-initialization transition phases no longer route to the scheduler writers"
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
