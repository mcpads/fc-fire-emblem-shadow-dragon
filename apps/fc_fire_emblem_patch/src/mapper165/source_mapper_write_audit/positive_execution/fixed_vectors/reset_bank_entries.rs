use std::collections::{BTreeMap, BTreeSet, VecDeque};

use anyhow::{Context, Result, ensure};
use retro_rp2a03::{
    AddressingMode, Location, MemoryAddress, Mnemonic, Operand, Rp2A03, decode_bytes,
};
use typed_isa_core::{AccessKind, StaticSemantics};

use crate::{
    mapper165::banked_call_dispatch::{
        BANKED_CALL_DISPATCH_ADDRESS, BankedCallTransfer, bind_banked_call_dispatch,
    },
    mapper165::battle_codebook_plan::IndirectWriteDestinationBounds,
    mapper165::inline_pointer_dispatch::{
        INLINE_POINTER_DISPATCH_ADDRESS, bind_inline_pointer_dispatch,
    },
    rom::Rom,
    sha1_hex,
    typed_source::{Rp2a03DirectControlFlow, rp2a03_direct_control_flow},
};

use super::super::control_state::{
    FIXED_SCHEDULER_DISPATCH_GATE, ObservedControlStateWrites, PENDING_SHARED_MENU_REQUEST_STATE,
    PRG_BANK_SHADOW, merge_observed_control_state_writes, positive_control_state,
};
#[cfg(test)]
use super::super::control_state::{MAIN_STATE, MAP_DIALOGUE_OUTER_STATE, OUTER_SCREEN_STATE};
use super::super::indexed_write_destinations::AbsoluteIndexedWriteDestinationBounds;
use super::{FIXED_CPU_START, FIXED_PRG_BANK, RESET_RAM_CLEAR_CODE, RESET_RAM_CLEAR_START};

mod call_effects;
mod trace_state;

use call_effects::{CallReturnEffect, TrackedStateCallSummary, inspect_tracked_state_call};
use trace_state::{
    ActivationId, ByteValueSet, ResetTraceIdentity, ResetTraceState, ReturnContinuation,
    ReturnFrame, TrackedByteLocation,
};

const MAXIMUM_RESET_TRACE_STATES: usize = 50_000;
const SOURCE_PRG_BANK_COUNT: u8 = 16;
const SELECT_PRG_BANK_AND_SAVE_ENTRY: u16 = 0xC9A6;
const SELECT_PRG_BANK_AND_SAVE_CODE: [u8; 8] = [
    0x85, 0x29, // STA $29
    0x85, 0x51, // STA $51
    0x8D, 0x00, 0xA0, // STA $A000
    0x60, // RTS
];
const SELECT_PRG_BANK_AND_SAVE_INSTRUCTION_OFFSETS: [u16; 4] = [0, 2, 4, 7];
const TEMPORARY_BANK_SPRITE_COMPOSITION_ENTRY: u16 = 0xE759;
const TEMPORARY_BANK_SPRITE_COMPOSITION_END: u16 = 0xE828;
const TEMPORARY_BANK_SPRITE_COMPOSITION_SHA1: &str = "c8fdbb5fbc620720974dbcb06a47780dde0159c7";
const PENDING_STATE_ESCAPE_ENTRY: u16 = 0xE65C;
const PENDING_STATE_ESCAPE_NORMAL_RETURN: u16 = 0xE66D;
const PENDING_STATE_ESCAPE_TARGET: u16 = 0xE684;
const PENDING_STATE_ESCAPE_CODE: [u8; 18] = [
    0xAD, 0xCC, 0x05, 0xF0, 0x0C, 0xA9, 0x01, 0x85, 0x97, 0x68, 0x68, 0xAD, 0xCC, 0x05, 0x4C, 0x84,
    0xE6, 0x60,
];
const PENDING_STATE_ESCAPE_COMMON_INSTRUCTION_OFFSETS: [u16; 2] = [0, 3];
const PENDING_STATE_ESCAPE_ACTIVE_INSTRUCTION_OFFSETS: [u16; 6] = [5, 7, 9, 10, 11, 14];

#[derive(Clone, Debug, Eq, PartialEq)]
pub(in super::super) struct InlineDispatchSelectorBounds {
    admitted_selectors: BTreeSet<u8>,
    source_bound_produced_selectors: Option<BTreeSet<u8>>,
    selector_memory_addresses: BTreeSet<u16>,
}

impl InlineDispatchSelectorBounds {
    pub(in super::super) fn from_source_producers(selectors: BTreeSet<u8>) -> Self {
        Self {
            admitted_selectors: selectors.clone(),
            source_bound_produced_selectors: Some(selectors),
            selector_memory_addresses: BTreeSet::new(),
        }
    }

    pub(in super::super) fn from_handler_table(selectors: BTreeSet<u8>) -> Self {
        Self {
            admitted_selectors: selectors,
            source_bound_produced_selectors: None,
            selector_memory_addresses: BTreeSet::new(),
        }
    }

    pub(in super::super) fn with_selector_memory_address(mut self, address: u16) -> Self {
        self.selector_memory_addresses.insert(address);
        self
    }

    pub(in super::super) fn with_selector_memory_addresses(
        mut self,
        addresses: impl IntoIterator<Item = u16>,
    ) -> Self {
        self.selector_memory_addresses.extend(addresses);
        self
    }

    pub(in super::super) fn merge_handler_table_owner(
        &mut self,
        selectors: &BTreeSet<u8>,
        selector_memory_address: Option<u16>,
    ) -> Result<()> {
        ensure!(
            !selectors.is_empty(),
            "inline dispatch handler-table owner has an empty selector domain"
        );
        if let Some(produced) = &self.source_bound_produced_selectors {
            ensure!(
                produced.is_subset(selectors),
                "source-produced inline dispatch selector escapes a second owner-bound handler table"
            );
        } else {
            ensure!(
                self.admitted_selectors == *selectors,
                "two owners disagree about one inline dispatch handler-table domain"
            );
        }
        self.admitted_selectors = selectors.clone();

        if let Some(address) = selector_memory_address {
            ensure!(
                self.selector_memory_addresses.is_empty()
                    || self.selector_memory_addresses.contains(&address),
                "two owners disagree about one inline dispatch selector memory address"
            );
            self.selector_memory_addresses.insert(address);
        }
        Ok(())
    }

    pub(in super::super) fn merge_source_producer_owner(
        &mut self,
        selectors: &BTreeSet<u8>,
        selector_memory_address: Option<u16>,
    ) -> Result<()> {
        ensure!(
            !selectors.is_empty() && selectors.is_subset(&self.admitted_selectors),
            "inline dispatch source producer is empty or escapes its owner-bound handler table"
        );
        if let Some(previous) = &self.source_bound_produced_selectors {
            ensure!(
                previous == selectors,
                "two owners disagree about one inline dispatch source-producer domain"
            );
        } else {
            self.source_bound_produced_selectors = Some(selectors.clone());
        }
        if let Some(address) = selector_memory_address {
            ensure!(
                self.selector_memory_addresses.is_empty()
                    || self.selector_memory_addresses.contains(&address),
                "two source-producer owners disagree about one inline dispatch selector memory address"
            );
            self.selector_memory_addresses.insert(address);
        }
        Ok(())
    }

    fn admitted_selectors(&self) -> &BTreeSet<u8> {
        &self.admitted_selectors
    }

    fn source_bound_produced_selectors(&self) -> Option<&BTreeSet<u8>> {
        self.source_bound_produced_selectors.as_ref()
    }

    fn selector_memory_addresses(&self) -> &BTreeSet<u16> {
        &self.selector_memory_addresses
    }
}

#[derive(Default)]
struct ReturnFlow {
    continuations: BTreeMap<ActivationId, BTreeSet<ReturnContinuation>>,
    completed: BTreeMap<ActivationId, BTreeMap<ResetTraceIdentity, ResetTraceState>>,
}

#[derive(Clone, Debug, Eq, Ord, PartialEq, PartialOrd)]
struct ActivationNode {
    entry_bank: u8,
    entry_address: u16,
    call_site: Option<(u8, u16)>,
    parent: Option<ActivationId>,
}

#[derive(Default)]
struct ActivationArena {
    nodes: Vec<ActivationNode>,
    interned: BTreeMap<ActivationNode, ActivationId>,
}

impl ActivationArena {
    fn root(&mut self, entry_bank: u8, entry_address: u16) -> ActivationId {
        self.intern(ActivationNode {
            entry_bank,
            entry_address,
            call_site: None,
            parent: None,
        })
    }

    fn called(
        &mut self,
        entry_bank: u8,
        entry_address: u16,
        call_site_bank: u8,
        call_site_address: u16,
        parent: ActivationId,
    ) -> ActivationId {
        let call_site = Some((call_site_bank, call_site_address));
        let mut ancestor = Some(parent);
        while let Some(activation) = ancestor {
            let node = &self.nodes[activation.0];
            if node.entry_bank == entry_bank
                && node.entry_address == entry_address
                && node.call_site == call_site
            {
                return activation;
            }
            ancestor = node.parent;
        }
        self.intern(ActivationNode {
            entry_bank,
            entry_address,
            call_site,
            parent: Some(parent),
        })
    }

    fn node(&self, activation: ActivationId) -> &ActivationNode {
        &self.nodes[activation.0]
    }

    fn intern(&mut self, node: ActivationNode) -> ActivationId {
        if let Some(activation) = self.interned.get(&node) {
            return *activation;
        }
        let activation = ActivationId(self.nodes.len());
        self.nodes.push(node.clone());
        self.interned.insert(node, activation);
        activation
    }
}

#[derive(Clone, Debug, Eq, Ord, PartialEq, PartialOrd)]
struct FixedSchedulerEntryState {
    selector: u8,
    mapped_prg_bank: u8,
    state: ResetTraceState,
}

#[derive(Debug, Default)]
pub(in super::super) struct StatefulBankExecution {
    switchable_roots: BTreeSet<(u8, u16)>,
    reachable_instruction_starts: BTreeSet<(u8, u16)>,
    open_facts: BTreeSet<String>,
    inline_dispatch_selectors: BTreeMap<(u8, u16), BTreeSet<u8>>,
    inline_dispatch_entry_banks: BTreeMap<(u8, u16, u8), BTreeSet<u8>>,
    terminal_entry_contexts: BTreeMap<(u8, u16), BTreeSet<(u8, u8)>>,
    fixed_scheduler_entry_states: BTreeSet<FixedSchedulerEntryState>,
    indirect_write_sites_below_mapper_space: BTreeSet<(u8, u16, u8)>,
    control_state_write_values: ObservedControlStateWrites,
}

impl StatefulBankExecution {
    pub(in super::super) fn switchable_roots(&self) -> &BTreeSet<(u8, u16)> {
        &self.switchable_roots
    }

    pub(in super::super) fn reachable_instruction_starts(&self) -> &BTreeSet<(u8, u16)> {
        &self.reachable_instruction_starts
    }

    pub(in super::super) fn open_fact_descriptions(&self) -> Vec<String> {
        self.open_facts.iter().cloned().collect()
    }

    pub(in super::super) fn inline_dispatch_selectors(&self) -> &BTreeMap<(u8, u16), BTreeSet<u8>> {
        &self.inline_dispatch_selectors
    }

    pub(in super::super) fn inline_dispatch_contexts(
        &self,
        bank: u8,
        address: u16,
    ) -> BTreeSet<(u8, u8)> {
        self.inline_dispatch_entry_banks
            .iter()
            .filter_map(|(&(actual_bank, actual_address, selector), entry_banks)| {
                (actual_bank == bank && actual_address == address).then_some(
                    entry_banks
                        .iter()
                        .map(move |entry_bank| (selector, *entry_bank)),
                )
            })
            .flatten()
            .collect()
    }

    pub(in super::super) fn terminal_entry_contexts(
        &self,
        bank: u8,
        address: u16,
    ) -> BTreeSet<(u8, u8)> {
        self.terminal_entry_contexts
            .get(&(bank, address))
            .cloned()
            .unwrap_or_default()
    }

    pub(in super::super) fn indirect_write_sites_below_mapper_space(
        &self,
    ) -> &BTreeSet<(u8, u16, u8)> {
        &self.indirect_write_sites_below_mapper_space
    }

    pub(in super::super) fn control_state_write_values(&self) -> &ObservedControlStateWrites {
        &self.control_state_write_values
    }

    pub(in super::super) fn merge(&mut self, other: Self) {
        self.switchable_roots.extend(other.switchable_roots);
        self.reachable_instruction_starts
            .extend(other.reachable_instruction_starts);
        self.open_facts.extend(other.open_facts);
        merge_set_map(
            &mut self.inline_dispatch_selectors,
            other.inline_dispatch_selectors,
        );
        merge_set_map(
            &mut self.inline_dispatch_entry_banks,
            other.inline_dispatch_entry_banks,
        );
        merge_set_map(
            &mut self.terminal_entry_contexts,
            other.terminal_entry_contexts,
        );
        self.fixed_scheduler_entry_states
            .extend(other.fixed_scheduler_entry_states);
        self.indirect_write_sites_below_mapper_space
            .extend(other.indirect_write_sites_below_mapper_space);
        merge_observed_control_state_writes(
            &mut self.control_state_write_values,
            &other.control_state_write_values,
        );
    }

    pub(in super::super) fn close_inline_dispatch_producer_fact(
        &mut self,
        bank: u8,
        address: u16,
        handler_table_count: usize,
        produced_selectors: &BTreeSet<u8>,
    ) -> Result<()> {
        let observed = self
            .inline_dispatch_selectors
            .get(&(bank, address))
            .cloned()
            .unwrap_or_default();
        ensure!(
            observed == *produced_selectors,
            "cannot close inline-dispatch producer fact at {bank:02X}:${address:04X}: observed {observed:02X?}, expected {produced_selectors:02X?}"
        );
        self.open_facts
            .remove(&inline_dispatch_producer_unknown_description(
                bank,
                address,
                handler_table_count,
            ));
        Ok(())
    }
}

fn inline_dispatch_producer_unknown_description(
    bank: u8,
    address: u16,
    handler_table_count: usize,
) -> String {
    format!(
        "inline_dispatch@{bank:02X}:{address:04X}:selector_producer_unknown[handler_table_count={handler_table_count}]"
    )
}

fn merge_set_map<K: Ord, V: Ord>(
    merged: &mut BTreeMap<K, BTreeSet<V>>,
    additions: BTreeMap<K, BTreeSet<V>>,
) {
    for (key, values) in additions {
        merged.entry(key).or_default().extend(values);
    }
}

pub(super) fn bind_reset_bank_entries(
    source: &Rom,
    reset_root: u16,
    terminal_entries: &BTreeSet<(u8, u16)>,
    indirect_write_destination_bounds: &BTreeMap<(u8, u16, u8), IndirectWriteDestinationBounds>,
) -> Result<StatefulBankExecution> {
    ensure!(
        reset_root >= FIXED_CPU_START,
        "source reset vector does not enter the fixed PRG window"
    );
    let mut activations = ActivationArena::default();
    let reset_activation = activations.root(FIXED_PRG_BANK, reset_root);
    trace_bank_state_entries(
        source,
        VecDeque::from([ResetTraceState::at(reset_root, reset_activation)]),
        activations,
        ReturnFlow::default(),
        terminal_entries,
        &BTreeSet::new(),
        None,
        &BTreeMap::new(),
        indirect_write_destination_bounds,
        &BTreeMap::new(),
    )
    .context("trace reset-rooted source execution")
}

pub(in super::super) fn trace_fixed_scheduler_contexts(
    source: &Rom,
    state_load_address: u16,
    dispatch_call_address: u16,
    return_address: u16,
    entry_contexts: impl IntoIterator<Item = (u8, u8)>,
    initial_memory_values: &BTreeMap<u16, u8>,
    terminal_inline_dispatches: &BTreeSet<(u8, u16)>,
    inline_dispatch_selector_bounds: &BTreeMap<(u8, u16), InlineDispatchSelectorBounds>,
    indirect_write_destination_bounds: &BTreeMap<(u8, u16, u8), IndirectWriteDestinationBounds>,
    absolute_indexed_write_bounds: &BTreeMap<(u8, u16), AbsoluteIndexedWriteDestinationBounds>,
) -> Result<StatefulBankExecution> {
    let entry_contexts = entry_contexts.into_iter().collect::<BTreeSet<_>>();
    ensure!(
        !entry_contexts.is_empty(),
        "fixed scheduler trace has no selector and entry-bank contexts"
    );
    ensure!(
        entry_contexts
            .iter()
            .all(|(_, bank)| *bank <= FIXED_PRG_BANK),
        "fixed scheduler selector trace has an entry bank outside the MMC4 selector domain"
    );
    ensure!(
        dispatch_call_address == state_load_address.wrapping_add(2),
        "fixed scheduler state load and inline dispatch are no longer adjacent"
    );
    ensure!(
        initial_memory_values
            .keys()
            .all(|address| ResetTraceState::tracks_memory_address(*address)),
        "fixed scheduler trace received an initial value for an untracked memory address"
    );
    ensure!(
        source_instruction_bytes(source, FIXED_PRG_BANK, state_load_address, 2)? == [0xA5, 0x25],
        "fixed scheduler no longer loads its selector from $25"
    );
    let selectors = entry_contexts
        .iter()
        .map(|(selector, _)| *selector)
        .collect::<BTreeSet<_>>();
    let scheduler_bounds = inline_dispatch_selector_bounds
        .get(&(FIXED_PRG_BANK, dispatch_call_address))
        .context("fixed scheduler inline dispatch has no owner-bound selector domain")?;
    ensure!(
        selectors.is_subset(scheduler_bounds.admitted_selectors()),
        "fixed scheduler positive contexts exceed the owner-bound selector domain"
    );
    let dispatch = bind_inline_pointer_dispatch(
        source,
        FIXED_PRG_BANK,
        dispatch_call_address,
        scheduler_bounds.admitted_selectors().iter().copied(),
        "fixed scheduler stateful dispatch closure",
    )?;
    let targets = scheduler_bounds
        .admitted_selectors()
        .iter()
        .copied()
        .zip(dispatch.targets_in_selector_order())
        .collect::<BTreeMap<_, _>>();

    let mut entry_states = BTreeMap::new();
    let mut pending_contexts = VecDeque::new();
    for (selector, mapped_prg_bank) in entry_contexts.iter().copied() {
        let mut state = ResetTraceState::at(dispatch_call_address, ActivationId(0));
        state.write_memory(0x0025, Some(selector));
        state.write_memory_values(FIXED_SCHEDULER_DISPATCH_GATE, ByteValueSet::nonzero());
        for (&address, &value) in initial_memory_values {
            state.write_memory(address, Some(value));
        }
        state.write_prg_bank_shadows(Some(mapped_prg_bank));
        state.mapped_prg_bank = Some(mapped_prg_bank);
        state.invalidate_registers_and_flags();
        ensure!(
            entry_states
                .insert((selector, mapped_prg_bank), state)
                .is_none(),
            "fixed scheduler received a duplicate initial entry context"
        );
        pending_contexts.push_back((selector, mapped_prg_bank));
    }
    let mut execution = StatefulBankExecution::default();
    let mut trace_pass_count = 0_usize;
    while let Some((selector, mapped_prg_bank)) = pending_contexts.pop_front() {
        trace_pass_count += 1;
        ensure!(
            trace_pass_count <= MAXIMUM_RESET_TRACE_STATES,
            "fixed scheduler stateful closure exceeded {MAXIMUM_RESET_TRACE_STATES} widening passes"
        );
        let state = entry_states
            .get(&(selector, mapped_prg_bank))
            .cloned()
            .context("fixed scheduler worklist lost an entry state")?;
        let mut epoch = trace_fixed_scheduler_entry_state(
            source,
            FIXED_PRG_BANK,
            dispatch_call_address,
            return_address,
            0x0025,
            FixedSchedulerEntryState {
                selector,
                mapped_prg_bank,
                state,
            },
            &targets,
            terminal_inline_dispatches,
            inline_dispatch_selector_bounds,
            indirect_write_destination_bounds,
            absolute_indexed_write_bounds,
        )?;
        epoch
            .reachable_instruction_starts
            .insert((FIXED_PRG_BANK, state_load_address));
        for successor in &epoch.fixed_scheduler_entry_states {
            let key = (successor.selector, successor.mapped_prg_bank);
            if let Some(previous) = entry_states.get(&key) {
                let joined = previous.join_data_state(&successor.state);
                if joined != *previous {
                    entry_states.insert(key, joined);
                    pending_contexts.push_back(key);
                }
            } else {
                entry_states.insert(key, successor.state.clone());
                pending_contexts.push_back(key);
            }
        }
        execution.merge(epoch);
    }
    Ok(execution)
}

#[allow(clippy::too_many_arguments)]
pub(in super::super) fn trace_source_bound_inline_state_handler(
    source: &Rom,
    dispatch_bank: u8,
    state_load_address: u16,
    dispatch_call_address: u16,
    return_address: u16,
    selector_memory_address: u16,
    selector: u8,
    mapped_prg_bank: u8,
    initial_memory_values: &BTreeMap<u16, u8>,
    inline_dispatch_selector_bounds: &BTreeMap<(u8, u16), InlineDispatchSelectorBounds>,
    indirect_write_destination_bounds: &BTreeMap<(u8, u16, u8), IndirectWriteDestinationBounds>,
    absolute_indexed_write_bounds: &BTreeMap<(u8, u16), AbsoluteIndexedWriteDestinationBounds>,
) -> Result<StatefulBankExecution> {
    ensure!(
        dispatch_bank <= FIXED_PRG_BANK && mapped_prg_bank <= FIXED_PRG_BANK,
        "source-bound inline state handler has a bank outside the MMC4 domain"
    );
    ensure!(
        dispatch_bank == FIXED_PRG_BANK || dispatch_bank == mapped_prg_bank,
        "switchable inline state handler is not mapped in its owning physical bank"
    );
    ensure!(
        initial_memory_values
            .keys()
            .all(|address| ResetTraceState::tracks_memory_address(*address)),
        "source-bound inline state handler received an untracked initial memory address"
    );
    let bounds = inline_dispatch_selector_bounds
        .get(&(dispatch_bank, dispatch_call_address))
        .context("source-bound inline state handler has no owner-bound selector domain")?;
    ensure!(
        bounds.admitted_selectors().contains(&selector),
        "source-bound inline state selector left its owner-bound handler table"
    );
    let dispatch = bind_inline_pointer_dispatch(
        source,
        dispatch_bank,
        dispatch_call_address,
        [selector],
        "source-bound inline state handler",
    )?;
    let target = *dispatch
        .targets_in_selector_order()
        .first()
        .context("source-bound inline state selector has no target")?;
    let mut state = ResetTraceState::at(dispatch_call_address, ActivationId(0));
    for (&address, &value) in initial_memory_values {
        state.write_memory(address, Some(value));
    }
    state.write_memory(selector_memory_address, Some(selector));
    state.write_prg_bank_shadows(Some(mapped_prg_bank));
    state.mapped_prg_bank = Some(mapped_prg_bank);
    state.invalidate_registers_and_flags();
    let mut execution = trace_fixed_scheduler_entry_state(
        source,
        dispatch_bank,
        dispatch_call_address,
        return_address,
        selector_memory_address,
        FixedSchedulerEntryState {
            selector,
            mapped_prg_bank,
            state,
        },
        &BTreeMap::from([(selector, target)]),
        &BTreeSet::new(),
        inline_dispatch_selector_bounds,
        indirect_write_destination_bounds,
        absolute_indexed_write_bounds,
    )?;
    execution
        .reachable_instruction_starts
        .insert((dispatch_bank, state_load_address));
    Ok(execution)
}

#[allow(clippy::too_many_arguments)]
pub(in super::super) fn trace_source_bound_inline_state_continuation(
    source: &Rom,
    dispatch_bank: u8,
    state_load_address: u16,
    dispatch_call_address: u16,
    return_address: u16,
    selector_memory_address: u16,
    selector: u8,
    mapped_prg_bank: u8,
    handler_address: u16,
    continuation_address: u16,
    prefix_instruction_starts: &BTreeSet<u16>,
    initial_memory_values: &BTreeMap<u16, u8>,
    inline_dispatch_selector_bounds: &BTreeMap<(u8, u16), InlineDispatchSelectorBounds>,
    indirect_write_destination_bounds: &BTreeMap<(u8, u16, u8), IndirectWriteDestinationBounds>,
    absolute_indexed_write_bounds: &BTreeMap<(u8, u16), AbsoluteIndexedWriteDestinationBounds>,
) -> Result<StatefulBankExecution> {
    ensure!(
        dispatch_bank <= FIXED_PRG_BANK && mapped_prg_bank <= FIXED_PRG_BANK,
        "source-bound continuation has a bank outside the MMC4 domain"
    );
    ensure!(
        dispatch_bank == FIXED_PRG_BANK || dispatch_bank == mapped_prg_bank,
        "switchable source-bound continuation is not mapped in its owning physical bank"
    );
    ensure!(
        initial_memory_values
            .keys()
            .all(|address| ResetTraceState::tracks_memory_address(*address)),
        "source-bound continuation received an untracked initial memory address"
    );
    ensure!(
        !prefix_instruction_starts.is_empty()
            && prefix_instruction_starts.contains(&handler_address)
            && prefix_instruction_starts
                .iter()
                .all(|address| *address >= handler_address && *address < continuation_address),
        "source-bound continuation prefix does not cover exactly the pre-continuation handler"
    );
    let bounds = inline_dispatch_selector_bounds
        .get(&(dispatch_bank, dispatch_call_address))
        .context("source-bound continuation has no owner-bound selector domain")?;
    ensure!(
        bounds.admitted_selectors().contains(&selector),
        "source-bound continuation selector left its owner-bound handler table"
    );
    let dispatch = bind_inline_pointer_dispatch(
        source,
        dispatch_bank,
        dispatch_call_address,
        [selector],
        "source-bound asynchronous handler continuation",
    )?;
    ensure!(
        dispatch.targets_in_selector_order() == [handler_address],
        "source-bound continuation no longer belongs to the selected handler"
    );

    let return_bank = if return_address >= FIXED_CPU_START {
        FIXED_PRG_BANK
    } else {
        dispatch_bank
    };
    let target_bank = if handler_address >= FIXED_CPU_START {
        FIXED_PRG_BANK
    } else {
        dispatch_bank
    };
    let mut activations = ActivationArena::default();
    let parent_activation = activations.root(return_bank, return_address);
    let handler_activation = activations.called(
        target_bank,
        handler_address,
        dispatch_bank,
        dispatch_call_address,
        parent_activation,
    );
    let mut return_flow = ReturnFlow::default();
    return_flow
        .continuations
        .entry(handler_activation)
        .or_default()
        .insert(ReturnContinuation {
            parent: parent_activation,
            frame: ReturnFrame::Direct(return_address),
        });
    let mut state = ResetTraceState::at(continuation_address, handler_activation);
    for (&address, &value) in initial_memory_values {
        state.write_memory(address, Some(value));
    }
    state.write_memory(selector_memory_address, Some(selector));
    state.write_prg_bank_shadows(Some(mapped_prg_bank));
    state.mapped_prg_bank = Some(mapped_prg_bank);
    state.invalidate_registers_and_flags();

    let mut execution = trace_bank_state_entries(
        source,
        VecDeque::from([state]),
        activations,
        return_flow,
        &BTreeSet::new(),
        &BTreeSet::new(),
        None,
        inline_dispatch_selector_bounds,
        indirect_write_destination_bounds,
        absolute_indexed_write_bounds,
    )
    .context("trace source-bound asynchronous handler continuation")?;
    execution
        .reachable_instruction_starts
        .insert((dispatch_bank, state_load_address));
    execution
        .reachable_instruction_starts
        .insert((dispatch_bank, dispatch_call_address));
    execution.reachable_instruction_starts.extend(
        prefix_instruction_starts
            .iter()
            .copied()
            .map(|address| (dispatch_bank, address)),
    );
    execution
        .inline_dispatch_selectors
        .entry((dispatch_bank, dispatch_call_address))
        .or_default()
        .insert(selector);
    execution
        .inline_dispatch_entry_banks
        .entry((dispatch_bank, dispatch_call_address, selector))
        .or_default()
        .insert(mapped_prg_bank);
    if handler_address < FIXED_CPU_START {
        execution
            .switchable_roots
            .insert((dispatch_bank, handler_address));
    }
    Ok(execution)
}

#[allow(clippy::too_many_arguments)]
fn trace_fixed_scheduler_entry_state(
    source: &Rom,
    dispatch_bank: u8,
    dispatch_call_address: u16,
    return_address: u16,
    selector_memory_address: u16,
    entry: FixedSchedulerEntryState,
    targets: &BTreeMap<u8, u16>,
    additional_terminal_inline_dispatches: &BTreeSet<(u8, u16)>,
    inline_dispatch_selector_bounds: &BTreeMap<(u8, u16), InlineDispatchSelectorBounds>,
    indirect_write_destination_bounds: &BTreeMap<(u8, u16, u8), IndirectWriteDestinationBounds>,
    absolute_indexed_write_bounds: &BTreeMap<(u8, u16), AbsoluteIndexedWriteDestinationBounds>,
) -> Result<StatefulBankExecution> {
    let FixedSchedulerEntryState {
        selector,
        mapped_prg_bank,
        mut state,
    } = entry;
    let target = *targets
        .get(&selector)
        .context("fixed scheduler successor state left its owner-bound handler table")?;
    let mut activations = ActivationArena::default();
    let return_bank = if return_address >= FIXED_CPU_START {
        FIXED_PRG_BANK
    } else {
        dispatch_bank
    };
    let parent_activation = activations.root(return_bank, return_address);
    let mut return_flow = ReturnFlow::default();
    let target_bank = if target >= FIXED_CPU_START {
        FIXED_PRG_BANK
    } else if dispatch_bank < FIXED_PRG_BANK {
        dispatch_bank
    } else {
        mapped_prg_bank
    };
    let handler_activation = activations.called(
        target_bank,
        target,
        dispatch_bank,
        dispatch_call_address,
        parent_activation,
    );
    return_flow
        .continuations
        .entry(handler_activation)
        .or_default()
        .insert(ReturnContinuation {
            parent: parent_activation,
            frame: ReturnFrame::Direct(return_address),
        });
    state.address = target;
    state.activation = handler_activation;
    state.invalidate_registers_and_flags();
    state.write_memory(selector_memory_address, Some(selector));
    state.write_prg_bank_shadows(Some(mapped_prg_bank));
    state.mapped_prg_bank = Some(mapped_prg_bank);
    state.set_accumulator(Some(selector.wrapping_mul(2)));
    let terminal_inline_dispatches = additional_terminal_inline_dispatches
        .iter()
        .copied()
        .chain([(dispatch_bank, dispatch_call_address)])
        .collect::<BTreeSet<_>>();
    let mut execution = trace_bank_state_entries(
        source,
        VecDeque::from([state]),
        activations,
        return_flow,
        &BTreeSet::new(),
        &terminal_inline_dispatches,
        Some((dispatch_bank, dispatch_call_address)),
        inline_dispatch_selector_bounds,
        indirect_write_destination_bounds,
        absolute_indexed_write_bounds,
    )
    .context("trace one fixed-scheduler source epoch")?;
    execution
        .reachable_instruction_starts
        .insert((dispatch_bank, dispatch_call_address));
    execution
        .inline_dispatch_selectors
        .entry((dispatch_bank, dispatch_call_address))
        .or_default()
        .insert(selector);
    execution
        .inline_dispatch_entry_banks
        .entry((dispatch_bank, dispatch_call_address, selector))
        .or_default()
        .insert(mapped_prg_bank);
    if target < FIXED_CPU_START {
        execution.switchable_roots.insert((mapped_prg_bank, target));
    }
    Ok(execution)
}

fn trace_bank_state_entries(
    source: &Rom,
    mut pending: VecDeque<ResetTraceState>,
    mut activations: ActivationArena,
    mut return_flow: ReturnFlow,
    terminal_entries: &BTreeSet<(u8, u16)>,
    terminal_inline_dispatches: &BTreeSet<(u8, u16)>,
    scheduler_reentry_inline_dispatch: Option<(u8, u16)>,
    inline_dispatch_selector_bounds: &BTreeMap<(u8, u16), InlineDispatchSelectorBounds>,
    indirect_write_destination_bounds: &BTreeMap<(u8, u16, u8), IndirectWriteDestinationBounds>,
    absolute_indexed_write_bounds: &BTreeMap<(u8, u16), AbsoluteIndexedWriteDestinationBounds>,
) -> Result<StatefulBankExecution> {
    let mut visited = BTreeMap::<ResetTraceIdentity, ResetTraceState>::new();
    let mut switchable_roots = BTreeSet::new();
    let mut reachable_instruction_starts = BTreeSet::new();
    let mut open_facts = BTreeSet::new();
    let mut inline_dispatch_selectors = BTreeMap::<_, BTreeSet<_>>::new();
    let mut inline_dispatch_entry_banks = BTreeMap::<_, BTreeSet<_>>::new();
    let mut terminal_entry_contexts = BTreeMap::<_, BTreeSet<_>>::new();
    let mut fixed_scheduler_entry_states = BTreeSet::new();
    let mut indirect_write_sites_below_mapper_space = BTreeMap::<_, bool>::new();
    let mut control_state_write_values = BTreeMap::new();
    let mut tracked_state_call_summaries =
        BTreeMap::<(u8, u16), Option<TrackedStateCallSummary>>::new();

    while let Some(mut state) = pending.pop_front() {
        let identity = state.identity();
        if let Some(previous) = visited.get(&identity) {
            let joined = previous.join_data_state(&state);
            if joined == *previous {
                continue;
            }
            state = joined.clone();
            visited.insert(identity, joined);
        } else {
            ensure!(
                visited.len() < MAXIMUM_RESET_TRACE_STATES,
                "source reset bank-state trace exceeded {MAXIMUM_RESET_TRACE_STATES} distinct control states at ${:04X} in activation {:?}; busiest addresses: {}",
                identity.address(),
                activations.node(identity.activation()),
                busiest_trace_addresses(&visited),
            );
            visited.insert(identity, state.clone());
        }

        if state.address == RESET_RAM_CLEAR_START {
            summarize_reset_ram_clear(source, &mut state, &mut reachable_instruction_starts)?;
            pending.push_back(state);
            continue;
        }
        let physical_bank = physical_bank_for_state(&state, &mut open_facts)?;
        let Some(physical_bank) = physical_bank else {
            continue;
        };
        if terminal_entries.contains(&(physical_bank, state.address)) {
            reachable_instruction_starts.insert((physical_bank, state.address));
            match (state.read_memory(0x0025), state.mapped_prg_bank) {
                (Some(selector), Some(mapped_prg_bank)) => {
                    terminal_entry_contexts
                        .entry((physical_bank, state.address))
                        .or_default()
                        .insert((selector, mapped_prg_bank));
                }
                (selector, mapped_prg_bank) => {
                    open_facts.insert(format!(
                        "terminal_entry@{physical_bank:02X}:{:04X}:scheduler_context_unknown[selector={selector:02X?},bank={mapped_prg_bank:02X?}]",
                        state.address,
                    ));
                }
            }
            continue;
        }
        if state.address == SELECT_PRG_BANK_AND_SAVE_ENTRY {
            summarize_prg_bank_selection(
                source,
                state,
                &mut pending,
                &mut return_flow,
                &mut reachable_instruction_starts,
                &mut switchable_roots,
                &mut open_facts,
                &mut control_state_write_values,
            )?;
            continue;
        }
        if state.address == TEMPORARY_BANK_SPRITE_COMPOSITION_ENTRY {
            summarize_temporary_bank_sprite_composition(
                source,
                state,
                &mut pending,
                &mut return_flow,
                &mut reachable_instruction_starts,
                &mut switchable_roots,
                &mut open_facts,
            )?;
            continue;
        }
        if state.address == PENDING_STATE_ESCAPE_ENTRY {
            summarize_pending_state_escape(
                source,
                state,
                &mut pending,
                &return_flow,
                &mut reachable_instruction_starts,
                &mut open_facts,
            )?;
            continue;
        }
        let instruction = decode_bytes(&source_instruction_bytes(
            source,
            physical_bank,
            state.address,
            3,
        )?)
        .with_context(|| {
            format!(
                "decode reset bank-state instruction at {physical_bank:02X}:${:04X}",
                state.address,
            )
        })?;
        if !instruction.opcode_is_documented() {
            open_facts.insert(format!(
                "undocumented_opcode@{physical_bank:02X}:{:04X}",
                state.address,
            ));
            continue;
        }
        reachable_instruction_starts.insert((physical_bank, state.address));
        if let Some(observation) = apply_data_effect(
            &instruction,
            &mut state,
            physical_bank,
            indirect_write_destination_bounds,
            absolute_indexed_write_bounds,
            &mut open_facts,
        )? {
            record_indirect_write_observation(
                &mut indirect_write_sites_below_mapper_space,
                observation,
            );
        }
        record_control_state_write_values(
            &mut control_state_write_values,
            physical_bank,
            state.address,
            &instruction,
            &state,
        );

        match rp2a03_direct_control_flow(&instruction, state.address)? {
            Rp2a03DirectControlFlow::FallThrough { next } => {
                state.address = next;
                pending.push_back(state);
            }
            Rp2a03DirectControlFlow::Branch {
                target,
                fallthrough,
            } => {
                let condition = branch_condition(instruction.mnemonic(), &state);
                if condition != Some(false) {
                    let mut taken = state.clone();
                    if refine_branch_state(instruction.mnemonic(), true, &mut taken) {
                        taken.address = target;
                        pending.push_back(taken);
                    }
                }
                if condition != Some(true) {
                    if let Some(fallthrough) = fallthrough {
                        if refine_branch_state(instruction.mnemonic(), false, &mut state) {
                            state.address = fallthrough;
                            pending.push_back(state);
                        }
                    }
                }
            }
            Rp2a03DirectControlFlow::Call {
                target,
                return_address,
            } if target == BANKED_CALL_DISPATCH_ADDRESS => {
                let continuation = ReturnContinuation {
                    parent: state.activation.clone(),
                    frame: ReturnFrame::Direct(return_address),
                };
                route_banked_call(
                    source,
                    physical_bank,
                    state,
                    BankedCallTransfer::Call,
                    continuation,
                    &mut pending,
                    &mut activations,
                    &mut return_flow,
                    &mut switchable_roots,
                    &mut open_facts,
                )?;
            }
            Rp2a03DirectControlFlow::Call {
                target,
                return_address: _,
            } if target == INLINE_POINTER_DISPATCH_ADDRESS => {
                let mut selectors = match state.accumulator.known_values() {
                    Some(selectors) => selectors,
                    None => {
                        let Some(bounds) =
                            inline_dispatch_selector_bounds.get(&(physical_bank, state.address))
                        else {
                            open_facts.insert(format!(
                                "inline_dispatch@{physical_bank:02X}:{:04X}:selector_unknown",
                                state.address,
                            ));
                            continue;
                        };
                        let Some(selectors) = bounds.source_bound_produced_selectors() else {
                            open_facts.insert(inline_dispatch_producer_unknown_description(
                                physical_bank,
                                state.address,
                                bounds.admitted_selectors().len(),
                            ));
                            continue;
                        };
                        ensure!(
                            !selectors.is_empty(),
                            "source-produced inline dispatch at {physical_bank:02X}:${:04X} has an empty selector domain",
                            state.address,
                        );
                        selectors.iter().copied().collect()
                    }
                };
                if let Some(bounds) =
                    inline_dispatch_selector_bounds.get(&(physical_bank, state.address))
                {
                    if let Some(produced) = bounds.source_bound_produced_selectors() {
                        selectors.retain(|selector| produced.contains(selector));
                        if selectors.is_empty() {
                            open_facts.insert(format!(
                                "inline_dispatch@{physical_bank:02X}:{:04X}:observed_selector_domain_disjoint_from_source_producers",
                                state.address,
                            ));
                            continue;
                        }
                    }
                    let outside_owned = selectors
                        .iter()
                        .copied()
                        .filter(|selector| !bounds.admitted_selectors().contains(selector))
                        .collect::<Vec<_>>();
                    if !outside_owned.is_empty() {
                        open_facts.insert(format!(
                            "inline_dispatch@{physical_bank:02X}:{:04X}:selectors_outside_owned_domain={outside_owned:02X?}",
                            state.address,
                        ));
                        selectors.retain(|selector| bounds.admitted_selectors().contains(selector));
                    }
                } else {
                    open_facts.insert(format!(
                        "inline_dispatch@{physical_bank:02X}:{:04X}:known_selector_domain_unowned[count={}]",
                        state.address,
                        selectors.len(),
                    ));
                    continue;
                }
                if selectors.is_empty() {
                    continue;
                }
                let Some(mapped_prg_bank) = state.mapped_prg_bank else {
                    open_facts.insert(format!(
                        "inline_dispatch@{physical_bank:02X}:{:04X}:entry_bank_unknown",
                        state.address,
                    ));
                    continue;
                };
                for selector in selectors {
                    inline_dispatch_selectors
                        .entry((physical_bank, state.address))
                        .or_default()
                        .insert(selector);
                    inline_dispatch_entry_banks
                        .entry((physical_bank, state.address, selector))
                        .or_default()
                        .insert(mapped_prg_bank);
                    if terminal_inline_dispatches.contains(&(physical_bank, state.address)) {
                        if scheduler_reentry_inline_dispatch == Some((physical_bank, state.address))
                        {
                            let mut next_entry = state.clone();
                            next_entry.address = state.address;
                            next_entry.activation = ActivationId(0);
                            next_entry.invalidate_registers_and_flags();
                            next_entry.write_memory(0x0025, Some(selector));
                            next_entry.write_prg_bank_shadows(Some(mapped_prg_bank));
                            next_entry.mapped_prg_bank = Some(mapped_prg_bank);
                            fixed_scheduler_entry_states.insert(FixedSchedulerEntryState {
                                selector,
                                mapped_prg_bank,
                                state: next_entry,
                            });
                        }
                        continue;
                    }
                    let binding = bind_inline_pointer_dispatch(
                        source,
                        physical_bank,
                        state.address,
                        [selector],
                        "stateful bank execution inline dispatch",
                    )?;
                    let target = *binding
                        .targets_in_selector_order()
                        .first()
                        .context("single stateful inline selector did not bind a target")?;
                    let mut selected = state.clone();
                    if let Some(selector_addresses) = inline_dispatch_selector_bounds
                        .get(&(physical_bank, state.address))
                        .map(InlineDispatchSelectorBounds::selector_memory_addresses)
                    {
                        for &selector_address in selector_addresses {
                            ensure!(
                                ResetTraceState::tracks_memory_address(selector_address),
                                "inline dispatch at {physical_bank:02X}:${:04X} refines an untracked selector-memory address ${selector_address:04X}",
                                state.address,
                            );
                            selected.write_memory(selector_address, Some(selector));
                        }
                    }
                    selected.set_accumulator(Some(selector.wrapping_mul(2)));
                    route_direct_target(
                        selected,
                        target,
                        &mut pending,
                        &mut switchable_roots,
                        &mut open_facts,
                    );
                }
            }
            Rp2a03DirectControlFlow::Call {
                target,
                return_address,
            } => {
                let target_bank = if target >= FIXED_CPU_START {
                    Some(FIXED_PRG_BANK)
                } else {
                    state.mapped_prg_bank
                };
                let tracked_state_summary = match target_bank {
                    Some(target_bank) => {
                        let key = (target_bank, target);
                        if !tracked_state_call_summaries.contains_key(&key) {
                            let summary = inspect_tracked_state_call(source, target_bank, target)?;
                            tracked_state_call_summaries.insert(key, summary);
                        }
                        tracked_state_call_summaries.get(&key).cloned().flatten()
                    }
                    None => None,
                };
                if let Some(summary) = tracked_state_summary {
                    record_fixed_to_switchable_entry(
                        &state,
                        target,
                        &mut switchable_roots,
                        &mut open_facts,
                    );
                    reachable_instruction_starts
                        .extend(summary.instruction_starts().iter().copied());
                    if summary.return_effects().contains(&CallReturnEffect::Normal) {
                        let mut normal_return = state.clone();
                        normal_return.invalidate_registers_and_flags();
                        normal_return.address = return_address;
                        pending.push_back(normal_return);
                    }
                    if summary
                        .return_effects()
                        .contains(&CallReturnEffect::EscapeOneCaller)
                    {
                        state.invalidate_registers_and_flags();
                        record_completed_return(
                            state,
                            &mut return_flow,
                            &mut pending,
                            &mut switchable_roots,
                            &mut open_facts,
                        );
                    }
                } else {
                    route_call_target(
                        state,
                        physical_bank,
                        target,
                        return_address,
                        &mut pending,
                        &mut activations,
                        &mut return_flow,
                        &mut switchable_roots,
                        &mut open_facts,
                    );
                }
            }
            Rp2a03DirectControlFlow::Jump {
                target: Some(target),
            } if target == BANKED_CALL_DISPATCH_ADDRESS => {
                let continuations = return_flow
                    .continuations
                    .get(&state.activation)
                    .cloned()
                    .unwrap_or_default();
                if continuations.is_empty() {
                    open_facts.insert(format!(
                        "banked_tail_jump@{physical_bank:02X}:{:04X}:return_stack_empty",
                        state.address,
                    ));
                }
                for continuation in continuations {
                    route_banked_call(
                        source,
                        physical_bank,
                        state.clone(),
                        BankedCallTransfer::TailJump,
                        continuation,
                        &mut pending,
                        &mut activations,
                        &mut return_flow,
                        &mut switchable_roots,
                        &mut open_facts,
                    )?;
                }
            }
            Rp2a03DirectControlFlow::Jump {
                target: Some(target),
            } => route_direct_target(
                state,
                target,
                &mut pending,
                &mut switchable_roots,
                &mut open_facts,
            ),
            Rp2a03DirectControlFlow::Jump { target: None } => {
                open_facts.insert(format!(
                    "indirect_jump@{physical_bank:02X}:{:04X}",
                    state.address,
                ));
            }
            Rp2a03DirectControlFlow::Return => {
                record_completed_return(
                    state,
                    &mut return_flow,
                    &mut pending,
                    &mut switchable_roots,
                    &mut open_facts,
                );
            }
            Rp2a03DirectControlFlow::Interrupt | Rp2a03DirectControlFlow::Stop => {}
        }
    }

    Ok(StatefulBankExecution {
        switchable_roots,
        reachable_instruction_starts,
        open_facts,
        inline_dispatch_selectors,
        inline_dispatch_entry_banks,
        terminal_entry_contexts,
        fixed_scheduler_entry_states,
        indirect_write_sites_below_mapper_space: indirect_write_sites_below_mapper_space
            .into_iter()
            .filter_map(|(site, is_below_mapper_space)| is_below_mapper_space.then_some(site))
            .collect(),
        control_state_write_values,
    })
}

fn summarize_temporary_bank_sprite_composition(
    source: &Rom,
    state: ResetTraceState,
    pending: &mut VecDeque<ResetTraceState>,
    return_flow: &mut ReturnFlow,
    reachable_instruction_starts: &mut BTreeSet<(u8, u16)>,
    switchable_roots: &mut BTreeSet<(u8, u16)>,
    open_facts: &mut BTreeSet<String>,
) -> Result<()> {
    let byte_count = usize::from(
        TEMPORARY_BANK_SPRITE_COMPOSITION_END - TEMPORARY_BANK_SPRITE_COMPOSITION_ENTRY,
    );
    let bytes = source_instruction_bytes(
        source,
        FIXED_PRG_BANK,
        TEMPORARY_BANK_SPRITE_COMPOSITION_ENTRY,
        byte_count,
    )?;
    ensure!(
        sha1_hex(&bytes) == TEMPORARY_BANK_SPRITE_COMPOSITION_SHA1,
        "source temporary-bank sprite composition changed before semantic summarization"
    );
    let mut offset = 0_usize;
    let mut last_flow = None;
    while offset < bytes.len() {
        let address = TEMPORARY_BANK_SPRITE_COMPOSITION_ENTRY
            .checked_add(u16::try_from(offset)?)
            .context("temporary-bank sprite composition address overflow")?;
        let instruction = decode_bytes(&bytes[offset..]).with_context(|| {
            format!("decode temporary-bank sprite composition at 0F:${address:04X}")
        })?;
        ensure!(
            instruction.opcode_is_documented(),
            "temporary-bank sprite composition contains an undocumented opcode at 0F:${address:04X}"
        );
        reachable_instruction_starts.insert((FIXED_PRG_BANK, address));
        last_flow = Some(rp2a03_direct_control_flow(&instruction, address)?);
        offset += instruction.encoded_len();
    }
    ensure!(
        offset == bytes.len() && matches!(last_flow, Some(Rp2a03DirectControlFlow::Return)),
        "temporary-bank sprite composition no longer ends at its source-bound RTS"
    );

    let temporary_bank_values = state.read_memory_values(0x05C6);
    if temporary_bank_values
        .restrict(|value| value & 0x80 != 0)
        .is_some()
    {
        resume_sprite_composition_state(
            state.clone(),
            None,
            pending,
            return_flow,
            switchable_roots,
            open_facts,
        );
    }
    if temporary_bank_values
        .restrict(|value| value & 0x80 == 0)
        .is_some()
    {
        let saved_bank_values = state.read_memory_values(0x0029);
        match saved_bank_values.known_values() {
            Some(values) => {
                for value in values {
                    resume_sprite_composition_state(
                        state.clone(),
                        Some(value),
                        pending,
                        return_flow,
                        switchable_roots,
                        open_facts,
                    );
                }
            }
            None => {
                open_facts.insert(
                    "temporary_bank_sprite_composition@0F:E759:restore_bank_unknown".to_owned(),
                );
                let mut unknown_restore = state;
                unknown_restore.mapped_prg_bank = None;
                unknown_restore.write_memory(0x05C7, None);
                resume_sprite_composition_state(
                    unknown_restore,
                    None,
                    pending,
                    return_flow,
                    switchable_roots,
                    open_facts,
                );
            }
        }
    }
    Ok(())
}

fn resume_sprite_composition_state(
    mut state: ResetTraceState,
    restored_bank: Option<u8>,
    pending: &mut VecDeque<ResetTraceState>,
    return_flow: &mut ReturnFlow,
    switchable_roots: &mut BTreeSet<(u8, u16)>,
    open_facts: &mut BTreeSet<String>,
) {
    if let Some(restored_bank) = restored_bank {
        state.mapped_prg_bank = Some(restored_bank & 0x0F);
        state.write_memory(0x05C7, Some(restored_bank));
    }
    state.write_memory(0x0008, None);
    state.write_memory(0x05C6, Some(0xFF));
    state.set_accumulator(Some(0));
    state.set_index_x(None);
    state.set_index_y(None);
    state.carry = None;
    if !return_flow.continuations.contains_key(&state.activation) {
        open_facts
            .insert("temporary_bank_sprite_composition@0F:E759:return_stack_empty".to_owned());
    }
    record_completed_return(state, return_flow, pending, switchable_roots, open_facts);
}

fn summarize_prg_bank_selection(
    source: &Rom,
    state: ResetTraceState,
    pending: &mut VecDeque<ResetTraceState>,
    return_flow: &mut ReturnFlow,
    reachable_instruction_starts: &mut BTreeSet<(u8, u16)>,
    switchable_roots: &mut BTreeSet<(u8, u16)>,
    open_facts: &mut BTreeSet<String>,
    control_state_write_values: &mut ObservedControlStateWrites,
) -> Result<()> {
    let bytes = source_instruction_bytes(
        source,
        FIXED_PRG_BANK,
        SELECT_PRG_BANK_AND_SAVE_ENTRY,
        SELECT_PRG_BANK_AND_SAVE_CODE.len(),
    )?;
    ensure!(
        bytes == SELECT_PRG_BANK_AND_SAVE_CODE,
        "source PRG-bank selector changed before semantic summarization"
    );
    for offset in SELECT_PRG_BANK_AND_SAVE_INSTRUCTION_OFFSETS {
        reachable_instruction_starts
            .insert((FIXED_PRG_BANK, SELECT_PRG_BANK_AND_SAVE_ENTRY + offset));
    }

    if return_flow
        .continuations
        .get(&state.activation)
        .is_none_or(BTreeSet::is_empty)
    {
        open_facts.insert("prg_bank_selector@0F:C9A6:return_stack_empty".to_owned());
    }
    let accumulator_values = state.accumulator.known_values();
    merge_observed_control_state_writes(
        control_state_write_values,
        &ObservedControlStateWrites::from([(
            (
                FIXED_PRG_BANK,
                SELECT_PRG_BANK_AND_SAVE_ENTRY,
                PRG_BANK_SHADOW,
            ),
            accumulator_values
                .as_ref()
                .map(|values| values.iter().copied().collect()),
        )]),
    );
    match accumulator_values {
        Some(values) => {
            for value in values {
                let mut selected = state.clone();
                selected.accumulator = ByteValueSet::known(value);
                selected.write_prg_bank_shadows(Some(value));
                selected.mapped_prg_bank = Some(value & 0x0F);
                record_completed_return(
                    selected,
                    return_flow,
                    pending,
                    switchable_roots,
                    open_facts,
                );
            }
        }
        None => {
            open_facts.insert("prg_bank_selector@0F:C9A6:selected_bank_unknown".to_owned());
            let mut selected = state;
            selected.write_prg_bank_shadows(None);
            selected.mapped_prg_bank = None;
            record_completed_return(selected, return_flow, pending, switchable_roots, open_facts);
        }
    }
    Ok(())
}

fn summarize_pending_state_escape(
    source: &Rom,
    state: ResetTraceState,
    pending: &mut VecDeque<ResetTraceState>,
    return_flow: &ReturnFlow,
    reachable_instruction_starts: &mut BTreeSet<(u8, u16)>,
    open_facts: &mut BTreeSet<String>,
) -> Result<()> {
    let bytes = source_instruction_bytes(
        source,
        FIXED_PRG_BANK,
        PENDING_STATE_ESCAPE_ENTRY,
        PENDING_STATE_ESCAPE_CODE.len(),
    )?;
    ensure!(
        bytes == PENDING_STATE_ESCAPE_CODE,
        "source pending-state escape sequence changed before stack-effect summarization"
    );
    for offset in PENDING_STATE_ESCAPE_COMMON_INSTRUCTION_OFFSETS {
        reachable_instruction_starts.insert((FIXED_PRG_BANK, PENDING_STATE_ESCAPE_ENTRY + offset));
    }

    let request_values = state.read_memory_values(PENDING_SHARED_MENU_REQUEST_STATE);
    if request_values.restrict(|value| value == 0).is_some() {
        reachable_instruction_starts.insert((FIXED_PRG_BANK, PENDING_STATE_ESCAPE_NORMAL_RETURN));
        let mut normal_return = state.clone();
        normal_return.set_accumulator(Some(0));
        normal_return.address = PENDING_STATE_ESCAPE_NORMAL_RETURN;
        pending.push_back(normal_return);
    }

    if let Some(active_request_values) = request_values.restrict(|value| value != 0) {
        for offset in PENDING_STATE_ESCAPE_ACTIVE_INSTRUCTION_OFFSETS {
            reachable_instruction_starts
                .insert((FIXED_PRG_BANK, PENDING_STATE_ESCAPE_ENTRY + offset));
        }
        let mut escaped = state;
        escaped.set_accumulator_values(active_request_values);
        let continuations = return_flow
            .continuations
            .get(&escaped.activation)
            .cloned()
            .unwrap_or_default();
        if continuations.is_empty() {
            open_facts.insert("pending_state_escape@0F:E65C:return_stack_empty".to_owned());
        }
        for continuation in continuations {
            match continuation.frame {
                ReturnFrame::Direct(_) => {
                    let mut continued = escaped.clone();
                    continued.activation = continuation.parent;
                    continued.address = PENDING_STATE_ESCAPE_TARGET;
                    pending.push_back(continued);
                }
                ReturnFrame::Banked { .. } => {
                    open_facts.insert(
                        "pending_state_escape@0F:E65C:discarded_frame_is_banked".to_owned(),
                    );
                }
            }
        }
    }
    Ok(())
}

fn busiest_trace_addresses(visited: &BTreeMap<ResetTraceIdentity, ResetTraceState>) -> String {
    let mut counts = BTreeMap::<u16, usize>::new();
    for identity in visited.keys() {
        *counts.entry(identity.address()).or_default() += 1;
    }
    let mut counts = counts.into_iter().collect::<Vec<_>>();
    counts.sort_by_key(|&(address, count)| (std::cmp::Reverse(count), address));
    counts
        .into_iter()
        .take(12)
        .map(|(address, count)| {
            let at_address = visited
                .keys()
                .filter(|identity| identity.address() == address)
                .collect::<Vec<_>>();
            let activation_count = at_address
                .iter()
                .map(|identity| identity.activation().clone())
                .collect::<BTreeSet<_>>()
                .len();
            let bank_count = at_address
                .iter()
                .map(|identity| identity.mapped_prg_bank())
                .collect::<BTreeSet<_>>()
                .len();
            format!("${address:04X}={count}[activations={activation_count},banks={bank_count}]")
        })
        .collect::<Vec<_>>()
        .join(", ")
}

fn summarize_reset_ram_clear(
    source: &Rom,
    state: &mut ResetTraceState,
    reachable_instruction_starts: &mut BTreeSet<(u8, u16)>,
) -> Result<()> {
    let bytes = source_instruction_bytes(
        source,
        FIXED_PRG_BANK,
        RESET_RAM_CLEAR_START,
        RESET_RAM_CLEAR_CODE.len(),
    )?;
    ensure!(
        bytes == RESET_RAM_CLEAR_CODE,
        "source reset RAM-clear loop changed before state summarization"
    );
    let mut offset = 0_usize;
    while offset < bytes.len() {
        let instruction = decode_bytes(&bytes[offset..])
            .context("decode source reset RAM-clear summarized instruction")?;
        ensure!(
            instruction.opcode_is_documented(),
            "source reset RAM-clear summary reached undocumented opcode"
        );
        reachable_instruction_starts.insert((
            FIXED_PRG_BANK,
            RESET_RAM_CLEAR_START + u16::try_from(offset)?,
        ));
        offset += instruction.encoded_len();
    }
    ensure!(
        offset == bytes.len(),
        "source reset RAM-clear summary did not consume the exact loop"
    );

    state.initialize_memory_after_ram_clear();
    state.set_accumulator(Some(0));
    state.set_index_y(Some(0));
    state.zero = Some(false);
    state.negative = Some(true);
    state.clear_zero_source();
    state.address = RESET_RAM_CLEAR_START + u16::try_from(RESET_RAM_CLEAR_CODE.len())?;
    Ok(())
}

fn route_banked_call(
    source: &Rom,
    caller_bank: u8,
    mut state: ResetTraceState,
    transfer: BankedCallTransfer,
    continuation: ReturnContinuation,
    pending: &mut VecDeque<ResetTraceState>,
    activations: &mut ActivationArena,
    return_flow: &mut ReturnFlow,
    switchable_roots: &mut BTreeSet<(u8, u16)>,
    open_facts: &mut BTreeSet<String>,
) -> Result<()> {
    let Some(requested_bank) = state.accumulator.singleton().map(|value| value & 0x0F) else {
        open_facts.insert(format!(
            "banked_call@{caller_bank:02X}:{:04X}:requested_bank_unknown",
            state.address,
        ));
        return Ok(());
    };
    let Some(selector) = state.read_memory(0x0044) else {
        open_facts.insert(format!(
            "banked_call@{caller_bank:02X}:{:04X}:selector_44_unknown[requested_bank={requested_bank:02X}]",
            state.address,
        ));
        return Ok(());
    };
    let binding = bind_banked_call_dispatch(
        source,
        caller_bank,
        state.address,
        transfer,
        requested_bank,
        selector,
        "reset-rooted banked call",
    )?;
    ensure!(
        binding.call_address() == state.address
            && binding.requested_bank() == requested_bank
            && binding.selector() == selector,
        "reset-rooted banked call binding changed its caller-owned inputs"
    );
    let target = binding.target();
    if target < 0x8000 {
        open_facts.insert(format!(
            "banked_call@{caller_bank:02X}:{:04X}->${target:04X}:ram_target",
            state.address,
        ));
        return Ok(());
    }

    let restore_bank = state.mapped_prg_bank;
    state.mapped_prg_bank = Some(requested_bank);
    state.write_prg_bank_shadows(Some(requested_bank));
    state.set_accumulator(Some((target >> 8) as u8));
    state.set_index_x(Some(selector.wrapping_mul(2)));
    let target_bank = if target >= FIXED_CPU_START {
        FIXED_PRG_BANK
    } else {
        requested_bank
    };
    let target_activation = activations.called(
        target_bank,
        target,
        caller_bank,
        state.address,
        state.activation,
    );
    register_return_continuation(
        target_activation,
        ReturnContinuation {
            parent: continuation.parent,
            frame: ReturnFrame::Banked {
                continuation: Box::new(continuation.frame),
                restore_bank,
            },
        },
        return_flow,
        pending,
        switchable_roots,
        open_facts,
    );
    if target < FIXED_CPU_START {
        switchable_roots.insert((requested_bank, target));
    }
    state.activation = target_activation;
    state.address = target;
    pending.push_back(state);
    Ok(())
}

fn register_return_continuation(
    activation: ActivationId,
    continuation: ReturnContinuation,
    return_flow: &mut ReturnFlow,
    pending: &mut VecDeque<ResetTraceState>,
    switchable_roots: &mut BTreeSet<(u8, u16)>,
    open_facts: &mut BTreeSet<String>,
) {
    if !return_flow
        .continuations
        .entry(activation.clone())
        .or_default()
        .insert(continuation.clone())
    {
        return;
    }
    let completed = return_flow
        .completed
        .get(&activation)
        .into_iter()
        .flat_map(BTreeMap::values)
        .cloned()
        .collect::<Vec<_>>();
    for state in completed {
        resume_return_continuation(
            state,
            continuation.clone(),
            pending,
            switchable_roots,
            open_facts,
        );
    }
}

fn record_completed_return(
    state: ResetTraceState,
    return_flow: &mut ReturnFlow,
    pending: &mut VecDeque<ResetTraceState>,
    switchable_roots: &mut BTreeSet<(u8, u16)>,
    open_facts: &mut BTreeSet<String>,
) {
    let activation = state.activation.clone();
    let identity = state.identity();
    let completed = return_flow.completed.entry(activation.clone()).or_default();
    let state = match completed.get(&identity) {
        Some(previous) => {
            let joined = previous.join_data_state(&state);
            if joined == *previous {
                return;
            }
            completed.insert(identity, joined.clone());
            joined
        }
        None => {
            completed.insert(identity, state.clone());
            state
        }
    };
    let continuations = return_flow
        .continuations
        .get(&activation)
        .cloned()
        .unwrap_or_default();
    for continuation in continuations {
        resume_return_continuation(
            state.clone(),
            continuation,
            pending,
            switchable_roots,
            open_facts,
        );
    }
}

fn resume_return_continuation(
    mut state: ResetTraceState,
    continuation: ReturnContinuation,
    pending: &mut VecDeque<ResetTraceState>,
    switchable_roots: &mut BTreeSet<(u8, u16)>,
    open_facts: &mut BTreeSet<String>,
) {
    state.activation = continuation.parent;
    resume_return_frame(
        state,
        continuation.frame,
        false,
        pending,
        switchable_roots,
        open_facts,
    );
}

fn resume_return_frame(
    mut state: ResetTraceState,
    frame: ReturnFrame,
    restored_bank_boundary: bool,
    pending: &mut VecDeque<ResetTraceState>,
    switchable_roots: &mut BTreeSet<(u8, u16)>,
    open_facts: &mut BTreeSet<String>,
) {
    match frame {
        ReturnFrame::Direct(return_address) => {
            if restored_bank_boundary && return_address < FIXED_CPU_START {
                let Some(bank) = state.mapped_prg_bank else {
                    open_facts.insert(format!("banked_return->${return_address:04X}:bank_unknown"));
                    return;
                };
                switchable_roots.insert((bank, return_address));
            } else {
                record_fixed_to_switchable_entry(
                    &state,
                    return_address,
                    switchable_roots,
                    open_facts,
                );
            }
            state.address = return_address;
            pending.push_back(state);
        }
        ReturnFrame::Banked {
            continuation,
            restore_bank,
        } => {
            state.mapped_prg_bank = restore_bank;
            state.write_prg_bank_shadows(restore_bank);
            state.set_accumulator(restore_bank);
            resume_return_frame(
                state,
                *continuation,
                true,
                pending,
                switchable_roots,
                open_facts,
            );
        }
    }
}

fn physical_bank_for_state(
    state: &ResetTraceState,
    open_facts: &mut BTreeSet<String>,
) -> Result<Option<u8>> {
    ensure!(
        state.address >= 0x8000,
        "source reset bank-state trace reached RAM at ${:04X}",
        state.address
    );
    if state.address >= FIXED_CPU_START {
        return Ok(Some(FIXED_PRG_BANK));
    }
    match state.mapped_prg_bank {
        Some(bank) if bank < SOURCE_PRG_BANK_COUNT => Ok(Some(bank)),
        _ => {
            open_facts.insert(format!(
                "instruction_fetch@${:04X}:bank_unknown",
                state.address
            ));
            Ok(None)
        }
    }
}

fn source_instruction_bytes(
    source: &Rom,
    physical_bank: u8,
    address: u16,
    byte_count: usize,
) -> Result<Vec<u8>> {
    ensure!(
        physical_bank < SOURCE_PRG_BANK_COUNT,
        "source reset physical bank is outside the MMC4 selector range"
    );
    (0..byte_count)
        .map(|offset| {
            let cpu_address = address.wrapping_add(u16::try_from(offset)?);
            ensure!(
                cpu_address >= 0x8000,
                "source reset instruction fetch wrapped into RAM"
            );
            let (bank, relative) = if cpu_address >= FIXED_CPU_START {
                (FIXED_PRG_BANK, usize::from(cpu_address - FIXED_CPU_START))
            } else {
                (physical_bank, usize::from(cpu_address - 0x8000))
            };
            let prg_offset = usize::from(bank)
                .checked_mul(16 * 1024)
                .and_then(|base| base.checked_add(relative))
                .context("source reset instruction offset overflow")?;
            source.prg().get(prg_offset).copied().with_context(|| {
                format!(
                    "source reset instruction fetch exceeds bank {bank:02X} at ${cpu_address:04X}"
                )
            })
        })
        .collect()
}

fn route_call_target(
    mut state: ResetTraceState,
    caller_bank: u8,
    target: u16,
    return_address: u16,
    pending: &mut VecDeque<ResetTraceState>,
    activations: &mut ActivationArena,
    return_flow: &mut ReturnFlow,
    switchable_roots: &mut BTreeSet<(u8, u16)>,
    open_facts: &mut BTreeSet<String>,
) {
    if target < 0x8000 {
        open_facts.insert(format!(
            "call@{:04X}->${target:04X}:ram_target",
            state.address
        ));
        return;
    }
    record_fixed_to_switchable_entry(&state, target, switchable_roots, open_facts);
    let target_bank = if target >= FIXED_CPU_START {
        FIXED_PRG_BANK
    } else {
        let Some(target_bank) = state.mapped_prg_bank else {
            return;
        };
        target_bank
    };
    let target_activation = activations.called(
        target_bank,
        target,
        caller_bank,
        state.address,
        state.activation,
    );
    register_return_continuation(
        target_activation,
        ReturnContinuation {
            parent: state.activation,
            frame: ReturnFrame::Direct(return_address),
        },
        return_flow,
        pending,
        switchable_roots,
        open_facts,
    );
    state.activation = target_activation;
    state.address = target;
    pending.push_back(state);
}

fn route_direct_target(
    mut state: ResetTraceState,
    target: u16,
    pending: &mut VecDeque<ResetTraceState>,
    switchable_roots: &mut BTreeSet<(u8, u16)>,
    open_facts: &mut BTreeSet<String>,
) {
    if target < 0x8000 {
        open_facts.insert(format!(
            "jump@{:04X}->${target:04X}:ram_target",
            state.address
        ));
        return;
    }
    record_fixed_to_switchable_entry(&state, target, switchable_roots, open_facts);
    if target >= FIXED_CPU_START {
        state.address = target;
        pending.push_back(state);
    } else if state.mapped_prg_bank.is_some() {
        state.address = target;
        pending.push_back(state);
    } else {
        open_facts.insert(format!(
            "switchable_target@{:04X}->${target:04X}:bank_unknown",
            state.address
        ));
    }
}

fn record_fixed_to_switchable_entry(
    state: &ResetTraceState,
    target: u16,
    switchable_roots: &mut BTreeSet<(u8, u16)>,
    open_facts: &mut BTreeSet<String>,
) {
    if state.address < FIXED_CPU_START || target >= FIXED_CPU_START {
        return;
    }
    match state.mapped_prg_bank {
        Some(bank) if bank < SOURCE_PRG_BANK_COUNT => {
            switchable_roots.insert((bank, target));
        }
        _ => {
            open_facts.insert(format!(
                "switchable_target@0F:{:04X}->${target:04X}:bank_unknown",
                state.address
            ));
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct IndirectWriteObservation {
    site: (u8, u16, u8),
    is_below_mapper_space: bool,
}

fn record_indirect_write_observation(
    observations: &mut BTreeMap<(u8, u16, u8), bool>,
    observation: IndirectWriteObservation,
) {
    observations
        .entry(observation.site)
        .and_modify(|all_contexts_are_below_mapper_space| {
            *all_contexts_are_below_mapper_space &= observation.is_below_mapper_space;
        })
        .or_insert(observation.is_below_mapper_space);
}

fn record_control_state_write_values(
    observations: &mut ObservedControlStateWrites,
    physical_bank: u8,
    cpu_address: u16,
    instruction: &retro_rp2a03::Instruction,
    state: &ResetTraceState,
) {
    let semantics =
        Rp2A03::semantics(instruction, &cpu_address).expect("RP2A03 semantics are infallible");
    for access in semantics.location_accesses {
        if access.kind != AccessKind::Write {
            continue;
        }
        let Location::Memory(MemoryAddress::Direct(target)) = access.location else {
            continue;
        };
        if positive_control_state(target).is_none() {
            continue;
        }
        let values = direct_write_value_is_modeled(instruction)
            .then(|| {
                state
                    .read_memory_values(target)
                    .known_values()
                    .map(BTreeSet::from_iter)
            })
            .flatten();
        merge_observed_control_state_writes(
            observations,
            &ObservedControlStateWrites::from([((physical_bank, cpu_address, target), values)]),
        );
    }
}

fn direct_write_value_is_modeled(instruction: &retro_rp2a03::Instruction) -> bool {
    matches!(
        (instruction.mnemonic(), instruction.addressing_mode()),
        (
            Mnemonic::Sta | Mnemonic::Stx | Mnemonic::Sty | Mnemonic::Inc | Mnemonic::Dec,
            AddressingMode::ZeroPage | AddressingMode::Absolute,
        )
    )
}

fn apply_data_effect(
    instruction: &retro_rp2a03::Instruction,
    state: &mut ResetTraceState,
    physical_bank: u8,
    indirect_write_destination_bounds: &BTreeMap<(u8, u16, u8), IndirectWriteDestinationBounds>,
    absolute_indexed_write_bounds: &BTreeMap<(u8, u16), AbsoluteIndexedWriteDestinationBounds>,
    open_facts: &mut BTreeSet<String>,
) -> Result<Option<IndirectWriteObservation>> {
    let mut indirect_write_observation = None;
    let mode = instruction.addressing_mode();
    let operand = instruction.operand();
    match (instruction.mnemonic(), mode, operand) {
        (Mnemonic::Lda, AddressingMode::Immediate, Operand::Byte(value)) => {
            state.set_accumulator(Some(value));
        }
        (Mnemonic::Ldx, AddressingMode::Immediate, Operand::Byte(value)) => {
            state.set_index_x(Some(value));
        }
        (Mnemonic::Ldy, AddressingMode::Immediate, Operand::Byte(value)) => {
            state.set_index_y(Some(value));
        }
        (Mnemonic::Lda, AddressingMode::ZeroPage, Operand::Byte(address)) => {
            state.set_accumulator_values(state.read_memory_values(u16::from(address)));
        }
        (Mnemonic::Ldx, AddressingMode::ZeroPage, Operand::Byte(address)) => {
            state.set_index_x_values(state.read_memory_values(u16::from(address)));
        }
        (Mnemonic::Ldy, AddressingMode::ZeroPage, Operand::Byte(address)) => {
            state.set_index_y_values(state.read_memory_values(u16::from(address)));
        }
        (Mnemonic::Lda, AddressingMode::Absolute, Operand::Word(address)) => {
            state.set_accumulator_values(state.read_memory_values(address));
        }
        (Mnemonic::Ldx, AddressingMode::Absolute, Operand::Word(address)) => {
            state.set_index_x_values(state.read_memory_values(address));
        }
        (Mnemonic::Ldy, AddressingMode::Absolute, Operand::Word(address)) => {
            state.set_index_y_values(state.read_memory_values(address));
        }
        (Mnemonic::Sta, AddressingMode::ZeroPage, Operand::Byte(address)) => {
            state.write_memory_values(u16::from(address), state.accumulator.clone());
        }
        (Mnemonic::Stx, AddressingMode::ZeroPage, Operand::Byte(address)) => {
            state.write_memory_values(u16::from(address), state.index_x.clone());
        }
        (Mnemonic::Sty, AddressingMode::ZeroPage, Operand::Byte(address)) => {
            state.write_memory_values(u16::from(address), state.index_y.clone());
        }
        (Mnemonic::Sta, AddressingMode::Absolute, Operand::Word(0xA000..=0xAFFF)) => {
            state.mapped_prg_bank = state.accumulator.singleton().map(|value| value & 0x0F);
        }
        (Mnemonic::Stx, AddressingMode::Absolute, Operand::Word(0xA000..=0xAFFF)) => {
            state.mapped_prg_bank = state.index_x.singleton().map(|value| value & 0x0F);
        }
        (Mnemonic::Sty, AddressingMode::Absolute, Operand::Word(0xA000..=0xAFFF)) => {
            state.mapped_prg_bank = state.index_y.singleton().map(|value| value & 0x0F);
        }
        (Mnemonic::Sta, AddressingMode::Absolute, Operand::Word(address)) => {
            state.write_memory_values(address, state.accumulator.clone());
        }
        (Mnemonic::Stx, AddressingMode::Absolute, Operand::Word(address)) => {
            state.write_memory_values(address, state.index_x.clone());
        }
        (Mnemonic::Sty, AddressingMode::Absolute, Operand::Word(address)) => {
            state.write_memory_values(address, state.index_y.clone());
        }
        (
            Mnemonic::Sta,
            AddressingMode::AbsoluteX | AddressingMode::AbsoluteY,
            Operand::Word(base),
        ) => {
            apply_absolute_indexed_store(
                state,
                physical_bank,
                base,
                mode,
                state.accumulator.clone(),
                absolute_indexed_write_bounds,
            )?;
        }
        (Mnemonic::Stx, AddressingMode::AbsoluteY, Operand::Word(base)) => {
            apply_absolute_indexed_store(
                state,
                physical_bank,
                base,
                mode,
                state.index_x.clone(),
                absolute_indexed_write_bounds,
            )?;
        }
        (Mnemonic::Sta, AddressingMode::ZeroPageIndirectIndexedY, Operand::Byte(pointer)) => {
            let site = (physical_bank, state.address, pointer);
            if let (Some(low), Some(high), Some(index_y)) = (
                state.read_memory(u16::from(pointer)),
                state.read_memory(u16::from(pointer.wrapping_add(1))),
                state.index_y.singleton(),
            ) {
                let base = u16::from_le_bytes([low, high]);
                let target = base.wrapping_add(u16::from(index_y));
                state.write_memory_values(target, state.accumulator.clone());
                if (0xA000..=0xAFFF).contains(&target) {
                    state.mapped_prg_bank = state.accumulator.singleton().map(|value| value & 0x0F);
                }
                indirect_write_observation = Some(IndirectWriteObservation {
                    site,
                    is_below_mapper_space: target < 0x8000,
                });
            } else {
                if let Some(bounds) =
                    indirect_write_destination_bounds.get(&(physical_bank, state.address, pointer))
                {
                    ensure!(
                        bounds
                            .destination_ranges()
                            .iter()
                            .all(|range| { range.start() <= range.end() && *range.end() < 0x8000 }),
                        "{} indirect-write destination bounds can reach mapper space",
                        bounds.role(),
                    );
                    state.clear_memory_in_ranges(bounds.destination_ranges());
                    indirect_write_observation = Some(IndirectWriteObservation {
                        site,
                        is_below_mapper_space: true,
                    });
                } else {
                    open_facts.insert(format!(
                        "effective_write@{physical_bank:02X}:{:04X}:indirect_target_unknown",
                        state.address,
                    ));
                    state.clear_memory_and_bank();
                    indirect_write_observation = Some(IndirectWriteObservation {
                        site,
                        is_below_mapper_space: false,
                    });
                }
            }
        }
        (Mnemonic::Tax, AddressingMode::Implied, Operand::None) => {
            state.set_index_x_values(state.accumulator.clone());
        }
        (Mnemonic::Tay, AddressingMode::Implied, Operand::None) => {
            state.set_index_y_values(state.accumulator.clone());
        }
        (Mnemonic::Txa, AddressingMode::Implied, Operand::None) => {
            state.set_accumulator_values(state.index_x.clone());
        }
        (Mnemonic::Tya, AddressingMode::Implied, Operand::None) => {
            state.set_accumulator_values(state.index_y.clone());
        }
        (Mnemonic::Inx, AddressingMode::Implied, Operand::None) => {
            state.set_index_x_values(state.index_x.map(|value| value.wrapping_add(1)));
        }
        (Mnemonic::Dex, AddressingMode::Implied, Operand::None) => {
            state.set_index_x_values(state.index_x.map(|value| value.wrapping_sub(1)));
        }
        (Mnemonic::Iny, AddressingMode::Implied, Operand::None) => {
            state.set_index_y_values(state.index_y.map(|value| value.wrapping_add(1)));
        }
        (Mnemonic::Dey, AddressingMode::Implied, Operand::None) => {
            state.set_index_y_values(state.index_y.map(|value| value.wrapping_sub(1)));
        }
        (Mnemonic::Inc, AddressingMode::ZeroPage, Operand::Byte(address)) => {
            let values = state
                .read_memory_values(u16::from(address))
                .map(|value| value.wrapping_add(1));
            state.write_memory_values(u16::from(address), values.clone());
            state.zero = values.uniform(|value| value == 0);
            state.negative = values.uniform(|value| value & 0x80 != 0);
            state.set_zero_source_for_memory(u16::from(address), 0);
        }
        (Mnemonic::Dec, AddressingMode::ZeroPage, Operand::Byte(address)) => {
            let values = state
                .read_memory_values(u16::from(address))
                .map(|value| value.wrapping_sub(1));
            state.write_memory_values(u16::from(address), values.clone());
            state.zero = values.uniform(|value| value == 0);
            state.negative = values.uniform(|value| value & 0x80 != 0);
            state.set_zero_source_for_memory(u16::from(address), 0);
        }
        (Mnemonic::Inc, AddressingMode::Absolute, Operand::Word(address)) => {
            let values = state
                .read_memory_values(address)
                .map(|value| value.wrapping_add(1));
            state.write_memory_values(address, values.clone());
            state.zero = values.uniform(|value| value == 0);
            state.negative = values.uniform(|value| value & 0x80 != 0);
            state.set_zero_source_for_memory(address, 0);
        }
        (Mnemonic::Dec, AddressingMode::Absolute, Operand::Word(address)) => {
            let values = state
                .read_memory_values(address)
                .map(|value| value.wrapping_sub(1));
            state.write_memory_values(address, values.clone());
            state.zero = values.uniform(|value| value == 0);
            state.negative = values.uniform(|value| value & 0x80 != 0);
            state.set_zero_source_for_memory(address, 0);
        }
        (
            Mnemonic::Asl
            | Mnemonic::Lsr
            | Mnemonic::Rol
            | Mnemonic::Ror
            | Mnemonic::Inc
            | Mnemonic::Dec,
            AddressingMode::AbsoluteX,
            Operand::Word(base),
        ) => {
            apply_absolute_indexed_read_modify_write(
                state,
                physical_bank,
                base,
                instruction.mnemonic(),
                absolute_indexed_write_bounds,
            )?;
        }
        (Mnemonic::Asl, AddressingMode::Accumulator, Operand::None) => {
            let values = state.accumulator.clone();
            state.carry = values.uniform(|value| value & 0x80 != 0);
            state.set_accumulator_values(values.map(|value| value.wrapping_mul(2)));
        }
        (Mnemonic::And, AddressingMode::Immediate, Operand::Byte(mask)) => {
            state.set_accumulator_values(state.accumulator.map(|value| value & mask));
        }
        (Mnemonic::Ora, AddressingMode::Immediate, Operand::Byte(mask)) => {
            state.set_accumulator_values(state.accumulator.map(|value| value | mask));
        }
        (Mnemonic::Eor, AddressingMode::Immediate, Operand::Byte(mask)) => {
            state.set_accumulator_values(state.accumulator.map(|value| value ^ mask));
        }
        (Mnemonic::Cmp, AddressingMode::Immediate, Operand::Byte(value)) => {
            compare_register(
                state.accumulator.clone(),
                TrackedByteLocation::Accumulator,
                value,
                state,
            );
        }
        (Mnemonic::Cpx, AddressingMode::Immediate, Operand::Byte(value)) => {
            compare_register(
                state.index_x.clone(),
                TrackedByteLocation::IndexX,
                value,
                state,
            );
        }
        (Mnemonic::Cpy, AddressingMode::Immediate, Operand::Byte(value)) => {
            compare_register(
                state.index_y.clone(),
                TrackedByteLocation::IndexY,
                value,
                state,
            );
        }
        (Mnemonic::Clc, AddressingMode::Implied, Operand::None) => state.carry = Some(false),
        (Mnemonic::Sec, AddressingMode::Implied, Operand::None) => state.carry = Some(true),
        (Mnemonic::Lda, _, _) => state.set_accumulator(None),
        (Mnemonic::Ldx, _, _) => state.set_index_x(None),
        (Mnemonic::Ldy, _, _) => state.set_index_y(None),
        (
            Mnemonic::Adc
            | Mnemonic::Sbc
            | Mnemonic::Ora
            | Mnemonic::And
            | Mnemonic::Eor
            | Mnemonic::Lsr
            | Mnemonic::Rol
            | Mnemonic::Ror
            | Mnemonic::Pla,
            _,
            _,
        ) => {
            state.set_accumulator(None);
            state.carry = None;
        }
        (Mnemonic::Bit | Mnemonic::Cmp | Mnemonic::Cpx | Mnemonic::Cpy, _, _) => {
            state.zero = None;
            state.negative = None;
            state.carry = None;
            state.clear_zero_source();
        }
        _ => {}
    }
    Ok(indirect_write_observation)
}

fn apply_absolute_indexed_store(
    state: &mut ResetTraceState,
    physical_bank: u8,
    base: u16,
    mode: AddressingMode,
    value: ByteValueSet,
    absolute_indexed_write_bounds: &BTreeMap<(u8, u16), AbsoluteIndexedWriteDestinationBounds>,
) -> Result<()> {
    let index = absolute_index_value(state, mode)?;
    match index {
        Some(index) => {
            let target = base.wrapping_add(u16::from(index));
            validate_absolute_indexed_target(
                physical_bank,
                state.address,
                target,
                absolute_indexed_write_bounds,
            )?;
            state.write_memory_values(target, value.clone());
            if (0xA000..=0xAFFF).contains(&target) {
                state.mapped_prg_bank = value.singleton().map(|value| value & 0x0F);
            }
        }
        None => clear_unknown_absolute_indexed_destination(
            state,
            physical_bank,
            base,
            absolute_indexed_write_bounds,
        ),
    }
    Ok(())
}

fn apply_absolute_indexed_read_modify_write(
    state: &mut ResetTraceState,
    physical_bank: u8,
    base: u16,
    mnemonic: Mnemonic,
    absolute_indexed_write_bounds: &BTreeMap<(u8, u16), AbsoluteIndexedWriteDestinationBounds>,
) -> Result<()> {
    match state.index_x.singleton() {
        Some(index) => {
            let target = base.wrapping_add(u16::from(index));
            validate_absolute_indexed_target(
                physical_bank,
                state.address,
                target,
                absolute_indexed_write_bounds,
            )?;
            let previous = state.read_memory_values(target);
            let result = match mnemonic {
                Mnemonic::Asl => {
                    state.carry = previous.uniform(|value| value & 0x80 != 0);
                    previous.map(|value| value.wrapping_shl(1))
                }
                Mnemonic::Lsr => {
                    state.carry = previous.uniform(|value| value & 0x01 != 0);
                    previous.map(|value| value >> 1)
                }
                Mnemonic::Rol => {
                    let carry_in = state.carry;
                    state.carry = previous.uniform(|value| value & 0x80 != 0);
                    carry_in.map_or_else(ByteValueSet::default, |carry| {
                        previous.map(|value| value.wrapping_shl(1) | u8::from(carry))
                    })
                }
                Mnemonic::Ror => {
                    let carry_in = state.carry;
                    state.carry = previous.uniform(|value| value & 0x01 != 0);
                    carry_in.map_or_else(ByteValueSet::default, |carry| {
                        previous.map(|value| (value >> 1) | (u8::from(carry) << 7))
                    })
                }
                Mnemonic::Inc => previous.map(|value| value.wrapping_add(1)),
                Mnemonic::Dec => previous.map(|value| value.wrapping_sub(1)),
                _ => unreachable!("absolute-indexed RMW helper received a non-RMW mnemonic"),
            };
            state.write_memory_values(target, result.clone());
            state.zero = result.uniform(|value| value == 0);
            state.negative = result.uniform(|value| value & 0x80 != 0);
            state.set_zero_source_for_memory(target, 0);
        }
        None => {
            clear_unknown_absolute_indexed_destination(
                state,
                physical_bank,
                base,
                absolute_indexed_write_bounds,
            );
            state.zero = None;
            state.negative = None;
            state.carry = None;
            state.clear_zero_source();
        }
    }
    Ok(())
}

fn absolute_index_value(state: &ResetTraceState, mode: AddressingMode) -> Result<Option<u8>> {
    match mode {
        AddressingMode::AbsoluteX => Ok(state.index_x.singleton()),
        AddressingMode::AbsoluteY => Ok(state.index_y.singleton()),
        _ => anyhow::bail!("absolute-indexed write helper received {mode:?}"),
    }
}

fn clear_unknown_absolute_indexed_destination(
    state: &mut ResetTraceState,
    physical_bank: u8,
    base: u16,
    absolute_indexed_write_bounds: &BTreeMap<(u8, u16), AbsoluteIndexedWriteDestinationBounds>,
) {
    let end = base.wrapping_add(u16::from(u8::MAX));
    let conservative_destination_ranges = if end >= base {
        vec![base..=end]
    } else {
        vec![base..=u16::MAX, 0..=end]
    };
    let source_bound = absolute_indexed_write_bounds.get(&(physical_bank, state.address));
    let destination_ranges = source_bound
        .map(AbsoluteIndexedWriteDestinationBounds::destination_ranges)
        .unwrap_or(&conservative_destination_ranges);
    let affects_tracked_state = (0..=u8::MAX)
        .any(|index| ResetTraceState::tracks_memory_address(base.wrapping_add(u16::from(index))));
    let affects_prg_bank = destination_ranges
        .iter()
        .any(|range| range.start() <= &0xAFFF && range.end() >= &0xA000);
    if !affects_tracked_state && !affects_prg_bank {
        return;
    }
    state.clear_memory_in_ranges(&destination_ranges);
    if affects_prg_bank {
        state.mapped_prg_bank = None;
    }
}

fn validate_absolute_indexed_target(
    physical_bank: u8,
    cpu_address: u16,
    target: u16,
    absolute_indexed_write_bounds: &BTreeMap<(u8, u16), AbsoluteIndexedWriteDestinationBounds>,
) -> Result<()> {
    let Some(bounds) = absolute_indexed_write_bounds.get(&(physical_bank, cpu_address)) else {
        return Ok(());
    };
    ensure!(
        bounds
            .destination_ranges()
            .iter()
            .any(|range| range.contains(&target)),
        "{} at {physical_bank:02X}:${cpu_address:04X} reached ${target:04X} outside its source-bound destination ranges",
        bounds.role(),
    );
    Ok(())
}

fn compare_register(
    register: ByteValueSet,
    location: TrackedByteLocation,
    operand: u8,
    state: &mut ResetTraceState,
) {
    state.zero = register.uniform(|value| value == operand);
    state.carry = register.uniform(|value| value >= operand);
    state.negative = register.uniform(|value| value.wrapping_sub(operand) & 0x80 != 0);
    state.set_zero_source_for_register(location, operand);
}

fn refine_branch_state(
    mnemonic: Mnemonic,
    branch_taken: bool,
    state: &mut ResetTraceState,
) -> bool {
    match mnemonic {
        Mnemonic::Beq => state.refine_zero_flag(branch_taken),
        Mnemonic::Bne => state.refine_zero_flag(!branch_taken),
        _ => true,
    }
}

fn branch_condition(mnemonic: Mnemonic, state: &ResetTraceState) -> Option<bool> {
    match mnemonic {
        Mnemonic::Beq => state.zero,
        Mnemonic::Bne => state.zero.map(|value| !value),
        Mnemonic::Bmi => state.negative,
        Mnemonic::Bpl => state.negative.map(|value| !value),
        Mnemonic::Bcs => state.carry,
        Mnemonic::Bcc => state.carry.map(|value| !value),
        Mnemonic::Bvs | Mnemonic::Bvc => None,
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        chapter_transition::ENDING_RECORD_PHASE_ADDRESS,
        dialogue_inventory::MAIN_DIALOGUE_COMPLETION_FLAG_ADDRESS,
        mapper165::inline_pointer_dispatch::INLINE_POINTER_DISPATCH_CODE, rom::HEADER_SIZE,
    };

    fn synthetic_destination_bounds(
        site: (u8, u16, u8),
        destination_ranges: Vec<std::ops::RangeInclusive<u16>>,
    ) -> BTreeMap<(u8, u16, u8), IndirectWriteDestinationBounds> {
        BTreeMap::from([(
            site,
            IndirectWriteDestinationBounds::for_synthetic_test(
                "synthetic indirect write",
                destination_ranges,
            ),
        )])
    }

    fn synthetic_absolute_indexed_destination_bounds(
        site: (u8, u16),
        destination_ranges: Vec<std::ops::RangeInclusive<u16>>,
    ) -> BTreeMap<(u8, u16), AbsoluteIndexedWriteDestinationBounds> {
        BTreeMap::from([(
            site,
            AbsoluteIndexedWriteDestinationBounds::for_synthetic_test(
                "synthetic absolute-indexed write",
                destination_ranges,
            ),
        )])
    }

    fn synthetic_source(fixed_program: &[(u16, &[u8])], reset_root: u16) -> Rom {
        let mut bytes = vec![0x60; HEADER_SIZE + 16 * 16 * 1024];
        bytes[..HEADER_SIZE].fill(0);
        bytes[..4].copy_from_slice(b"NES\x1A");
        bytes[4] = 16;
        bytes[6] = 0xA0;
        let fixed = HEADER_SIZE + 15 * 16 * 1024;
        for &(address, program) in fixed_program {
            let offset = fixed + usize::from(address - FIXED_CPU_START);
            bytes[offset..offset + program.len()].copy_from_slice(program);
        }
        let reset_vector = fixed + usize::from(0xFFFC - FIXED_CPU_START);
        bytes[reset_vector..reset_vector + 2].copy_from_slice(&reset_root.to_le_bytes());
        Rom::parse(bytes).unwrap()
    }

    #[test]
    fn matching_handler_owner_preserves_the_tighter_source_producer_domain() {
        let mut bounds =
            InlineDispatchSelectorBounds::from_source_producers(BTreeSet::from([0x01, 0x02]))
                .with_selector_memory_address(0x7731);

        bounds
            .merge_handler_table_owner(&BTreeSet::from([0x00, 0x01, 0x02]), Some(0x7731))
            .unwrap();

        assert_eq!(
            bounds.admitted_selectors(),
            &BTreeSet::from([0x00, 0x01, 0x02])
        );
        assert_eq!(
            bounds.source_bound_produced_selectors(),
            Some(&BTreeSet::from([0x01, 0x02]))
        );
        assert_eq!(
            bounds.selector_memory_addresses(),
            &BTreeSet::from([0x7731])
        );
    }

    #[test]
    fn conflicting_handler_owner_cannot_hide_a_source_producer_or_selector_address() {
        let mut escaped =
            InlineDispatchSelectorBounds::from_source_producers(BTreeSet::from([0x02]));
        let error = escaped
            .merge_handler_table_owner(&BTreeSet::from([0x00, 0x01]), None)
            .unwrap_err();
        assert!(error.to_string().contains("escapes"));

        let mut moved =
            InlineDispatchSelectorBounds::from_handler_table(BTreeSet::from([0x00, 0x01]))
                .with_selector_memory_address(0x7731);
        let error = moved
            .merge_handler_table_owner(&BTreeSet::from([0x00, 0x01]), Some(0x05DB))
            .unwrap_err();
        assert!(error.to_string().contains("selector memory address"));
    }

    #[test]
    fn joined_index_register_values_preserve_the_prg_bank_domain() {
        let source = synthetic_source(
            &[
                (
                    0xC100,
                    &[
                        0xAD, 0x00, 0x04, // LDA $0400; unknown branch input
                        0xC9, 0x0E, // CMP #$0E
                        0x90, 0x05, // BCC $C10C
                        0xA0, 0x09, // LDY #$09
                        0x4C, 0x0E, 0xC1, // JMP $C10E
                        0xA0, 0x02, // LDY #$02
                        0x84, 0x01, // STY $01
                        0xA5, 0x01, // LDA $01
                        0x20, 0xA6, 0xC9, // JSR $C9A6
                        0x60, // RTS
                    ],
                ),
                (
                    SELECT_PRG_BANK_AND_SAVE_ENTRY,
                    &SELECT_PRG_BANK_AND_SAVE_CODE,
                ),
            ],
            0xC100,
        );

        let trace = trace_with_inline_selector_bounds(&source, 0xC100, BTreeMap::new());

        assert_eq!(
            trace.control_state_write_values().get(&(
                FIXED_PRG_BANK,
                SELECT_PRG_BANK_AND_SAVE_ENTRY,
                PRG_BANK_SHADOW,
            )),
            Some(&Some(BTreeSet::from([0x02, 0x09])))
        );
        assert!(
            trace
                .open_fact_descriptions()
                .iter()
                .all(|fact| !fact.contains("selected_bank_unknown"))
        );
    }

    #[test]
    fn equality_branch_refines_a_loaded_value_before_a_control_state_write() {
        let source = synthetic_source(
            &[(
                0xC100,
                &[
                    0xAD, 0x00, 0x04, // LDA $0400; unknown byte
                    0xF0, 0x05, // BEQ $C10A
                    0xA9, 0x21, // LDA #$21
                    0x4C, 0x0C, 0xC1, // JMP $C10C
                    0x85, 0x84, // STA $84; equality path writes zero
                    0x00, // BRK
                ],
            )],
            0xC100,
        );

        let trace = trace_with_inline_selector_bounds(&source, 0xC100, BTreeMap::new());

        assert_eq!(
            trace
                .control_state_write_values()
                .get(&(FIXED_PRG_BANK, 0xC10A, MAIN_STATE,)),
            Some(&Some(BTreeSet::from([0x00])))
        );
    }

    #[test]
    fn source_producer_owner_refines_a_handler_domain_without_hiding_handlers() {
        let mut bounds =
            InlineDispatchSelectorBounds::from_handler_table(BTreeSet::from([0x00, 0x01, 0x02]))
                .with_selector_memory_address(0x05DB);

        bounds
            .merge_source_producer_owner(&BTreeSet::from([0x00, 0x01]), Some(0x05DB))
            .unwrap();

        assert_eq!(
            bounds.admitted_selectors(),
            &BTreeSet::from([0x00, 0x01, 0x02])
        );
        assert_eq!(
            bounds.source_bound_produced_selectors(),
            Some(&BTreeSet::from([0x00, 0x01]))
        );
        assert_eq!(
            bounds.selector_memory_addresses(),
            &BTreeSet::from([0x05DB])
        );

        let mut escaped =
            InlineDispatchSelectorBounds::from_handler_table(BTreeSet::from([0x00, 0x01]));
        let error = escaped
            .merge_source_producer_owner(&BTreeSet::from([0x02]), None)
            .unwrap_err();
        assert!(error.to_string().contains("escapes"));

        let mut moved =
            InlineDispatchSelectorBounds::from_handler_table(BTreeSet::from([0x00, 0x01]))
                .with_selector_memory_address(0x05DB);
        let error = moved
            .merge_source_producer_owner(&BTreeSet::from([0x00]), Some(0x7731))
            .unwrap_err();
        assert!(error.to_string().contains("selector memory address"));
    }

    fn trace_with_inline_selector_bounds(
        source: &Rom,
        root: u16,
        bounds: BTreeMap<(u8, u16), InlineDispatchSelectorBounds>,
    ) -> StatefulBankExecution {
        let mut activations = ActivationArena::default();
        let root_activation = activations.root(FIXED_PRG_BANK, root);
        trace_bank_state_entries(
            source,
            VecDeque::from([ResetTraceState::at(root, root_activation)]),
            activations,
            ReturnFlow::default(),
            &BTreeSet::new(),
            &BTreeSet::new(),
            None,
            &bounds,
            &BTreeMap::new(),
            &BTreeMap::new(),
        )
        .unwrap()
    }

    #[test]
    fn immediate_mapper_write_resolves_the_switchable_target_bank() {
        let source = synthetic_source(
            &[(0xC100, &[0xA9, 0x02, 0x8D, 0x00, 0xA0, 0x4C, 0x00, 0x84])],
            0xC100,
        );

        let trace =
            bind_reset_bank_entries(&source, 0xC100, &BTreeSet::new(), &BTreeMap::new()).unwrap();

        assert_eq!(trace.switchable_roots(), &BTreeSet::from([(0x02, 0x8400)]));
        assert!(trace.open_facts.is_empty());
    }

    #[test]
    fn unknown_mapper_value_keeps_the_dynamic_target_open() {
        let source = synthetic_source(
            &[(
                0xC100,
                &[0xAD, 0x00, 0x04, 0x8D, 0x00, 0xA0, 0x4C, 0x00, 0x84],
            )],
            0xC100,
        );

        let trace =
            bind_reset_bank_entries(&source, 0xC100, &BTreeSet::new(), &BTreeMap::new()).unwrap();

        assert!(trace.switchable_roots().is_empty());
        assert!(
            trace
                .open_fact_descriptions()
                .iter()
                .any(|fact| fact.contains("bank_unknown"))
        );
    }

    #[test]
    fn source_bound_ram_destination_preserves_the_selected_prg_bank() {
        let source = synthetic_source(
            &[(
                0xC100,
                &[
                    0xA9, 0x02, 0x8D, 0x00, 0xA0, 0xAD, 0x00, 0x04, 0x91, 0x02, 0x4C, 0x00, 0x84,
                ],
            )],
            0xC100,
        );
        let bounds = synthetic_destination_bounds(
            (FIXED_PRG_BANK, 0xC108, 0x02),
            vec![0x0781..=0x07A5, 0x7953..=0x79F2],
        );

        let trace = bind_reset_bank_entries(&source, 0xC100, &BTreeSet::new(), &bounds).unwrap();

        assert_eq!(trace.switchable_roots(), &BTreeSet::from([(0x02, 0x8400)]));
        assert!(
            trace
                .open_fact_descriptions()
                .iter()
                .all(|fact| !fact.contains("effective_write"))
        );
    }

    #[test]
    fn source_bound_destination_clobbers_only_intersecting_tracked_state() {
        let instruction = decode_bytes(&[0x91, 0x02]).unwrap();
        let mut state = ResetTraceState::at(0xC100, ActivationId(0));
        state.write_memory(0x0000, Some(0x34));
        state.write_memory(0x0024, Some(0x04));
        state.write_memory(0x0025, Some(0x05));
        state.write_memory(0x0029, Some(0x06));
        state.write_memory(0x0044, Some(0x07));
        state.write_memory(0x0051, Some(0x06));
        state.write_memory(0x0084, Some(0x0F));
        state.write_memory(0x057A, Some(0x08));
        state.write_memory(0x05E8, Some(0x0A));
        state.write_memory(0x05EE, Some(0x09));
        state.mapped_prg_bank = Some(0x06);
        let bounds =
            synthetic_destination_bounds((FIXED_PRG_BANK, 0xC100, 0x02), vec![0x0025..=0x0025]);
        let mut open_facts = BTreeSet::new();

        let observation = apply_data_effect(
            &instruction,
            &mut state,
            FIXED_PRG_BANK,
            &bounds,
            &BTreeMap::new(),
            &mut open_facts,
        )
        .unwrap();

        assert_eq!(state.read_memory(0x0000), Some(0x34));
        assert_eq!(state.read_memory(0x0024), Some(0x04));
        assert_eq!(state.read_memory(0x0025), None);
        assert_eq!(state.read_memory(0x0029), Some(0x06));
        assert_eq!(state.read_memory(0x0044), Some(0x07));
        assert_eq!(state.read_memory(0x0051), Some(0x06));
        assert_eq!(state.read_memory(0x0084), Some(0x0F));
        assert_eq!(state.read_memory(0x057A), Some(0x08));
        assert_eq!(state.read_memory(0x05E8), Some(0x0A));
        assert_eq!(state.read_memory(0x05EE), Some(0x09));
        assert_eq!(state.mapped_prg_bank, Some(0x06));
        assert!(open_facts.is_empty());
        assert_eq!(
            observation,
            Some(IndirectWriteObservation {
                site: (FIXED_PRG_BANK, 0xC100, 0x02),
                is_below_mapper_space: true,
            })
        );
    }

    #[test]
    fn unknown_absolute_index_clears_every_tracked_state_in_its_possible_range() {
        let instruction = decode_bytes(&[0x9D, 0x50, 0x05]).unwrap();
        let mut state = ResetTraceState::at(0xC100, ActivationId(0));
        state.set_accumulator(Some(0x7A));
        state.write_memory(PENDING_SHARED_MENU_REQUEST_STATE, Some(0x03));
        let mut open_facts = BTreeSet::new();

        apply_data_effect(
            &instruction,
            &mut state,
            FIXED_PRG_BANK,
            &BTreeMap::new(),
            &BTreeMap::new(),
            &mut open_facts,
        )
        .unwrap();

        assert_eq!(state.read_memory(PENDING_SHARED_MENU_REQUEST_STATE), None);
        assert!(open_facts.is_empty());
    }

    #[test]
    fn source_bound_absolute_index_range_preserves_disjoint_control_state() {
        let instruction = decode_bytes(&[0x9D, 0x50, 0x05]).unwrap();
        let mut state = ResetTraceState::at(0xC100, ActivationId(0));
        state.set_accumulator(Some(0x7A));
        state.write_memory(PENDING_SHARED_MENU_REQUEST_STATE, Some(0x03));
        let bounds = synthetic_absolute_indexed_destination_bounds(
            (FIXED_PRG_BANK, 0xC100),
            vec![0x0550..=0x05B4],
        );
        let mut open_facts = BTreeSet::new();

        apply_data_effect(
            &instruction,
            &mut state,
            FIXED_PRG_BANK,
            &BTreeMap::new(),
            &bounds,
            &mut open_facts,
        )
        .unwrap();

        assert_eq!(
            state.read_memory(PENDING_SHARED_MENU_REQUEST_STATE),
            Some(0x03)
        );
        assert!(open_facts.is_empty());
    }

    #[test]
    fn unknown_absolute_indexed_rmw_clears_every_tracked_state_in_its_possible_range() {
        let instruction = decode_bytes(&[0x1E, 0x50, 0x05]).unwrap();
        let mut state = ResetTraceState::at(0xC100, ActivationId(0));
        state.write_memory(PENDING_SHARED_MENU_REQUEST_STATE, Some(0x03));
        let mut open_facts = BTreeSet::new();

        apply_data_effect(
            &instruction,
            &mut state,
            FIXED_PRG_BANK,
            &BTreeMap::new(),
            &BTreeMap::new(),
            &mut open_facts,
        )
        .unwrap();

        assert_eq!(state.read_memory(PENDING_SHARED_MENU_REQUEST_STATE), None);
        assert!(open_facts.is_empty());
    }

    #[test]
    fn unbound_unknown_indirect_write_remains_open_and_clobbers_bank_state() {
        let instruction = decode_bytes(&[0x91, 0x02]).unwrap();
        let mut state = ResetTraceState::at(0xC100, ActivationId(0));
        state.write_memory(0x0000, Some(0x34));
        state.write_memory(0x0025, Some(0x05));
        state.write_memory(0x0051, Some(0x06));
        state.mapped_prg_bank = Some(0x06);
        let mut open_facts = BTreeSet::new();

        let observation = apply_data_effect(
            &instruction,
            &mut state,
            FIXED_PRG_BANK,
            &BTreeMap::new(),
            &BTreeMap::new(),
            &mut open_facts,
        )
        .unwrap();

        assert_eq!(state.read_memory(0x0000), None);
        assert_eq!(state.read_memory(0x0025), None);
        assert_eq!(state.read_memory(0x0051), None);
        assert_eq!(state.mapped_prg_bank, None);
        assert!(
            open_facts
                .iter()
                .any(|fact| fact.contains("indirect_target_unknown"))
        );
        assert_eq!(
            observation,
            Some(IndirectWriteObservation {
                site: (FIXED_PRG_BANK, 0xC100, 0x02),
                is_below_mapper_space: false,
            })
        );
    }

    #[test]
    fn exact_indirect_ram_destination_is_admitted_by_the_stateful_trace() {
        let source = synthetic_source(
            &[(
                0xC100,
                &[
                    0xA9, 0x51, 0x85, 0x08, // pointer low = $51
                    0xA9, 0x04, 0x85, 0x09, // pointer high = $04
                    0xA0, 0x02, // Y = 2
                    0xA9, 0x00, 0x91, 0x08, // STA ($08),Y -> $0453
                    0x60,
                ],
            )],
            0xC100,
        );

        let trace =
            bind_reset_bank_entries(&source, 0xC100, &BTreeSet::new(), &BTreeMap::new()).unwrap();

        assert_eq!(
            trace.indirect_write_sites_below_mapper_space(),
            &BTreeSet::from([(FIXED_PRG_BANK, 0xC10C, 0x08)])
        );
        assert!(trace.open_fact_descriptions().is_empty());
    }

    #[test]
    fn one_unresolved_context_disqualifies_an_indirect_write_site() {
        let site = (FIXED_PRG_BANK, 0xC100, 0x08);
        let mut observations = BTreeMap::new();
        record_indirect_write_observation(
            &mut observations,
            IndirectWriteObservation {
                site,
                is_below_mapper_space: true,
            },
        );
        record_indirect_write_observation(
            &mut observations,
            IndirectWriteObservation {
                site,
                is_below_mapper_space: false,
            },
        );

        assert_eq!(observations, BTreeMap::from([(site, false)]));
    }

    #[test]
    fn undocumented_target_remains_open_and_is_not_admitted_as_executable() {
        let source = synthetic_source(&[(0xC100, &[0x4C, 0x10, 0xC1]), (0xC110, &[0xFF])], 0xC100);

        let trace =
            bind_reset_bank_entries(&source, 0xC100, &BTreeSet::new(), &BTreeMap::new()).unwrap();

        assert!(
            trace
                .open_fact_descriptions()
                .iter()
                .any(|fact| fact == "undocumented_opcode@0F:C110")
        );
        assert!(
            !trace
                .reachable_instruction_starts()
                .contains(&(FIXED_PRG_BANK, 0xC110))
        );
    }

    #[test]
    fn distinct_scheduler_states_remain_separate_control_states() {
        let mut state_zero = ResetTraceState::at(0xC100, ActivationId(0));
        state_zero.mapped_prg_bank = Some(0x06);
        state_zero.write_memory(0x0025, Some(0x00));
        let mut state_five = state_zero.clone();
        state_five.write_memory(0x0025, Some(0x05));

        assert_ne!(state_zero, state_five);
    }

    #[test]
    fn distinct_outer_screen_states_remain_separate_control_states() {
        let mut state_save_offer = ResetTraceState::at(0x8400, ActivationId(0));
        state_save_offer.mapped_prg_bank = Some(0x06);
        state_save_offer.write_memory(0x0024, Some(0x0D));
        state_save_offer.write_memory(0x0025, Some(0x05));
        let mut state_save_complete = state_save_offer.clone();
        state_save_complete.write_memory(0x0024, Some(0x0E));

        assert_ne!(state_save_offer, state_save_complete);
    }

    #[test]
    fn distinct_main_states_remain_separate_control_states() {
        let mut state_input = ResetTraceState::at(0x849D, ActivationId(0));
        state_input.mapped_prg_bank = Some(0x06);
        state_input.write_memory(0x0024, Some(0x02));
        state_input.write_memory(0x0025, Some(0x05));
        state_input.write_memory(0x0084, Some(0x00));
        let mut state_transition = state_input.clone();
        state_transition.write_memory(0x0084, Some(0x01));

        assert_ne!(state_input, state_transition);
    }

    #[test]
    fn multi_value_inline_selector_stays_open_without_an_owner_domain() {
        let source = synthetic_source(
            &[
                (
                    0xC100,
                    &[
                        0xA9, 0x06, // LDA #$06
                        0x8D, 0x00, 0xA0, // STA $A000
                        0xAD, 0x00, 0x04, // LDA $0400
                        0xF0, 0x05, // BEQ $C10F
                        0xA9, 0x00, // LDA #$00
                        0x4C, 0x11, 0xC1, // JMP $C111
                        0xA9, 0x01, // LDA #$01
                        0x20, 0x4C, 0xC3, // JSR $C34C
                        0x20, 0xC1, 0x30, 0xC1, // inline target table
                    ],
                ),
                (0xC120, &[0x60]),
                (0xC130, &[0x60]),
                (
                    INLINE_POINTER_DISPATCH_ADDRESS,
                    &INLINE_POINTER_DISPATCH_CODE,
                ),
            ],
            0xC100,
        );

        let trace =
            bind_reset_bank_entries(&source, 0xC100, &BTreeSet::new(), &BTreeMap::new()).unwrap();

        assert_eq!(
            trace.inline_dispatch_contexts(FIXED_PRG_BANK, 0xC111),
            BTreeSet::new()
        );
        assert!(
            trace.open_fact_descriptions().iter().any(
                |fact| fact == "inline_dispatch@0F:C111:known_selector_domain_unowned[count=2]"
            )
        );
        assert!(
            !trace
                .reachable_instruction_starts()
                .contains(&(0x0F, 0xC120))
        );
        assert!(
            !trace
                .reachable_instruction_starts()
                .contains(&(0x0F, 0xC130))
        );
    }

    #[test]
    fn handler_table_does_not_invent_an_unknown_selector_producer() {
        let source = synthetic_source(
            &[
                (
                    0xC100,
                    &[
                        0xA9, 0x06, // LDA #$06
                        0x8D, 0x00, 0xA0, // STA $A000
                        0xAD, 0x00, 0x04, // LDA $0400
                        0x20, 0x4C, 0xC3, // JSR $C34C
                        0x20, 0xC1, 0x30, 0xC1, // inline target table
                    ],
                ),
                (0xC120, &[0x60]),
                (0xC130, &[0x60]),
                (
                    INLINE_POINTER_DISPATCH_ADDRESS,
                    &INLINE_POINTER_DISPATCH_CODE,
                ),
            ],
            0xC100,
        );
        let trace = trace_with_inline_selector_bounds(
            &source,
            0xC100,
            BTreeMap::from([(
                (FIXED_PRG_BANK, 0xC108),
                InlineDispatchSelectorBounds::from_handler_table(BTreeSet::from([0x00, 0x01])),
            )]),
        );

        assert_eq!(
            trace.inline_dispatch_contexts(FIXED_PRG_BANK, 0xC108),
            BTreeSet::new()
        );
        assert!(trace.open_fact_descriptions().iter().any(|fact| {
            fact == "inline_dispatch@0F:C108:selector_producer_unknown[handler_table_count=2]"
        }));
        assert!(
            !trace
                .reachable_instruction_starts()
                .contains(&(0x0F, 0xC120))
        );
        assert!(
            !trace
                .reachable_instruction_starts()
                .contains(&(0x0F, 0xC130))
        );
    }

    #[test]
    fn later_source_producer_evidence_closes_the_matching_dispatch_fact() {
        let selectors = BTreeSet::from([0x00, 0x01]);
        let mut trace = StatefulBankExecution::default();
        trace
            .inline_dispatch_selectors
            .insert((FIXED_PRG_BANK, 0xC108), selectors.clone());
        trace
            .open_facts
            .insert(inline_dispatch_producer_unknown_description(
                FIXED_PRG_BANK,
                0xC108,
                selectors.len(),
            ));

        trace
            .close_inline_dispatch_producer_fact(
                FIXED_PRG_BANK,
                0xC108,
                selectors.len(),
                &selectors,
            )
            .unwrap();

        assert!(trace.open_fact_descriptions().is_empty());
    }

    #[test]
    fn known_selector_can_use_a_source_bound_handler_table() {
        let source = synthetic_source(
            &[
                (
                    0xC100,
                    &[
                        0xA9, 0x06, // LDA #$06
                        0x8D, 0x00, 0xA0, // STA $A000
                        0xA9, 0x01, // LDA #$01
                        0x20, 0x4C, 0xC3, // JSR $C34C
                        0x20, 0xC1, 0x30, 0xC1, // inline target table
                    ],
                ),
                (0xC120, &[0x60]),
                (0xC130, &[0x60]),
                (
                    INLINE_POINTER_DISPATCH_ADDRESS,
                    &INLINE_POINTER_DISPATCH_CODE,
                ),
            ],
            0xC100,
        );
        let trace = trace_with_inline_selector_bounds(
            &source,
            0xC100,
            BTreeMap::from([(
                (FIXED_PRG_BANK, 0xC107),
                InlineDispatchSelectorBounds::from_handler_table(BTreeSet::from([0x00, 0x01])),
            )]),
        );

        assert_eq!(
            trace
                .inline_dispatch_selectors()
                .get(&(FIXED_PRG_BANK, 0xC107)),
            Some(&BTreeSet::from([0x01]))
        );
        assert!(
            !trace
                .reachable_instruction_starts()
                .contains(&(0x0F, 0xC120))
        );
        assert!(
            trace
                .reachable_instruction_starts()
                .contains(&(0x0F, 0xC130))
        );
        assert!(trace.open_fact_descriptions().is_empty());
    }

    #[test]
    fn memory_backed_dispatch_refines_each_selected_handler_state() {
        let source = synthetic_source(
            &[
                (
                    0xC100,
                    &[
                        0xA9, 0x06, // LDA #$06
                        0x8D, 0x00, 0xA0, // STA $A000
                        0xAD, 0x00, 0x04, // LDA $0400; unknown branch input
                        0xF0, 0x07, // BEQ $C111
                        0xA9, 0x00, // LDA #$00
                        0x85, 0x24, // STA $24
                        0x4C, 0x15, 0xC1, // JMP $C115
                        0xA9, 0x01, // LDA #$01
                        0x85, 0x24, // STA $24
                        0xA5, 0x24, // LDA $24
                        0x20, 0x4C, 0xC3, // JSR $C34C
                        0x30, 0xC1, 0x40, 0xC1, // inline target table
                    ],
                ),
                (0xC130, &[0xE6, 0x24, 0xEE, 0xCC, 0x05, 0x60]),
                (0xC140, &[0xE6, 0x24, 0xEE, 0xCC, 0x05, 0x60]),
                (
                    INLINE_POINTER_DISPATCH_ADDRESS,
                    &INLINE_POINTER_DISPATCH_CODE,
                ),
            ],
            0xC100,
        );
        let trace = trace_with_inline_selector_bounds(
            &source,
            0xC100,
            BTreeMap::from([(
                (FIXED_PRG_BANK, 0xC117),
                InlineDispatchSelectorBounds::from_handler_table(BTreeSet::from([0x00, 0x01]))
                    .with_selector_memory_addresses([
                        OUTER_SCREEN_STATE,
                        PENDING_SHARED_MENU_REQUEST_STATE,
                    ]),
            )]),
        );

        assert_eq!(
            trace
                .control_state_write_values()
                .get(&(FIXED_PRG_BANK, 0xC130, OUTER_SCREEN_STATE)),
            Some(&Some(BTreeSet::from([0x01])))
        );
        assert_eq!(
            trace
                .control_state_write_values()
                .get(&(FIXED_PRG_BANK, 0xC140, OUTER_SCREEN_STATE)),
            Some(&Some(BTreeSet::from([0x02])))
        );
        assert_eq!(
            trace.control_state_write_values().get(&(
                FIXED_PRG_BANK,
                0xC132,
                PENDING_SHARED_MENU_REQUEST_STATE,
            )),
            Some(&Some(BTreeSet::from([0x01])))
        );
        assert_eq!(
            trace.control_state_write_values().get(&(
                FIXED_PRG_BANK,
                0xC142,
                PENDING_SHARED_MENU_REQUEST_STATE,
            )),
            Some(&Some(BTreeSet::from([0x02])))
        );
        assert!(trace.open_fact_descriptions().is_empty());
    }

    #[test]
    fn source_bound_async_continuation_preserves_wait_and_completion_behavior() {
        const DISPATCH_CALL: u16 = 0xC102;
        const HANDLER: u16 = 0xC120;
        const CONTINUATION: u16 = 0xC125;
        const PROGRESS_FLAG: u16 = MAIN_DIALOGUE_COMPLETION_FLAG_ADDRESS;
        let source = synthetic_source(
            &[
                (
                    0xC100,
                    &[
                        0xA5, 0x25, // LDA $25
                        0x20, 0x4C, 0xC3, // JSR $C34C
                        0x20, 0xC1, // selector-zero target
                    ],
                ),
                (
                    HANDLER,
                    &[
                        0xA9, 0x00, // source-owned async setup
                        0x20, 0x00, 0xC2, // summarized external call
                        0xAD, 0x03, 0x78, // LDA progress flag
                        0xF0, 0x03, // pending skips the phase increment
                        0xEE, 0x31, 0x77, // completed advances the phase
                        0x60,
                    ],
                ),
                (0xC140, &[0x60]),
                (
                    INLINE_POINTER_DISPATCH_ADDRESS,
                    &INLINE_POINTER_DISPATCH_CODE,
                ),
            ],
            0xC140,
        );
        let bounds = BTreeMap::from([(
            (FIXED_PRG_BANK, DISPATCH_CALL),
            InlineDispatchSelectorBounds::from_handler_table(BTreeSet::from([0x00]))
                .with_selector_memory_address(OUTER_SCREEN_STATE),
        )]);
        let trace = |progress_value| {
            trace_source_bound_inline_state_continuation(
                &source,
                FIXED_PRG_BANK,
                0xC100,
                DISPATCH_CALL,
                0xC140,
                OUTER_SCREEN_STATE,
                0x00,
                FIXED_PRG_BANK,
                HANDLER,
                CONTINUATION,
                &BTreeSet::from([HANDLER, HANDLER + 2]),
                &BTreeMap::from([
                    (ENDING_RECORD_PHASE_ADDRESS, 0x15),
                    (PROGRESS_FLAG, progress_value),
                ]),
                &bounds,
                &BTreeMap::new(),
                &BTreeMap::new(),
            )
            .unwrap()
        };

        let pending = trace(0);
        let completed = trace(1);

        assert!(
            pending
                .control_state_write_values()
                .get(&(FIXED_PRG_BANK, 0xC12A, ENDING_RECORD_PHASE_ADDRESS))
                .is_none()
        );
        assert_eq!(
            completed.control_state_write_values().get(&(
                FIXED_PRG_BANK,
                0xC12A,
                ENDING_RECORD_PHASE_ADDRESS
            )),
            Some(&Some(BTreeSet::from([0x16])))
        );
        assert!(
            completed
                .reachable_instruction_starts()
                .is_superset(&BTreeSet::from([
                    (FIXED_PRG_BANK, HANDLER),
                    (FIXED_PRG_BANK, HANDLER + 2),
                    (FIXED_PRG_BANK, CONTINUATION),
                ]))
        );
    }

    #[test]
    fn summarized_callee_preserves_normal_and_one_caller_escape_routes() {
        let source = synthetic_source(
            &[
                (
                    0xC100,
                    &[
                        0x20, 0x40, 0xC1, // JSR $C140
                        0xA9, 0x02, // LDA #$02
                        0x8D, 0x00, 0xA0, // STA $A000
                        0x4C, 0x00, 0x84, // JMP $8400
                    ],
                ),
                (
                    0xC140,
                    &[
                        0x20, 0x80, 0xC1, // JSR $C180
                        0xA9, 0x03, // LDA #$03
                        0x8D, 0x00, 0xA0, // STA $A000
                        0x4C, 0x00, 0x85, // JMP $8500
                    ],
                ),
                (
                    0xC180,
                    &[
                        0xAD, 0x00, 0x04, // LDA $0400
                        0xF0, 0x03, // BEQ $C188
                        0x68, // PLA: discard this callee's return low byte
                        0x68, // PLA: discard this callee's return high byte
                        0x60, // RTS: return from the caller at $C140
                        0x60, // $C188: normal RTS
                    ],
                ),
            ],
            0xC100,
        );

        let trace =
            bind_reset_bank_entries(&source, 0xC100, &BTreeSet::new(), &BTreeMap::new()).unwrap();

        assert_eq!(
            trace.switchable_roots(),
            &BTreeSet::from([(0x02, 0x8400), (0x03, 0x8500)])
        );
    }

    #[test]
    fn shared_callee_returns_only_to_its_call_site_context() {
        let source = synthetic_source(
            &[
                (0xC100, &[0xAD, 0x00, 0x04, 0xF0, 0x1B]),
                (
                    0xC105,
                    &[
                        0xA9, 0x00, 0x85, 0x25, 0x20, 0x80, 0xC1, 0xA5, 0x25, 0xD0, 0x08, 0xA9,
                        0x02, 0x8D, 0x00, 0xA0, 0x4C, 0x00, 0x84, 0xA9, 0x04, 0x8D, 0x00, 0xA0,
                        0x4C, 0x00, 0x86,
                    ],
                ),
                (
                    0xC120,
                    &[
                        0xA9, 0x01, 0x85, 0x25, 0x20, 0x80, 0xC1, 0xA5, 0x25, 0xF0, 0x08, 0xA9,
                        0x03, 0x8D, 0x00, 0xA0, 0x4C, 0x00, 0x85, 0xA9, 0x05, 0x8D, 0x00, 0xA0,
                        0x4C, 0x00, 0x87,
                    ],
                ),
                (0xC180, &[0x60]),
            ],
            0xC100,
        );

        let trace =
            bind_reset_bank_entries(&source, 0xC100, &BTreeSet::new(), &BTreeMap::new()).unwrap();

        assert_eq!(
            trace.switchable_roots(),
            &BTreeSet::from([(0x02, 0x8400), (0x03, 0x8500)])
        );
    }

    #[test]
    fn nested_shared_call_site_keeps_each_outer_activation_lineage() {
        let source = synthetic_source(
            &[
                (
                    0xC100,
                    &[
                        0xAD, 0x00, 0x04, // LDA $0400
                        0xF0, 0x0B, // BEQ $C110
                        0x4C, 0x40, 0xC1, // JMP $C140
                    ],
                ),
                (
                    0xC110,
                    &[
                        0xA9, 0x00, 0x85, 0x25, 0x20, 0x80, 0xC1, 0xA5, 0x25, 0xD0, 0x08, 0xA9,
                        0x02, 0x8D, 0x00, 0xA0, 0x4C, 0x00, 0x84, 0xA9, 0x04, 0x8D, 0x00, 0xA0,
                        0x4C, 0x00, 0x86,
                    ],
                ),
                (
                    0xC140,
                    &[
                        0xA9, 0x01, 0x85, 0x25, 0x20, 0x80, 0xC1, 0xA5, 0x25, 0xF0, 0x08, 0xA9,
                        0x03, 0x8D, 0x00, 0xA0, 0x4C, 0x00, 0x85, 0xA9, 0x05, 0x8D, 0x00, 0xA0,
                        0x4C, 0x00, 0x87,
                    ],
                ),
                (0xC180, &[0x20, 0x90, 0xC1, 0x60]),
                (0xC190, &[0x60]),
            ],
            0xC100,
        );

        let trace =
            bind_reset_bank_entries(&source, 0xC100, &BTreeSet::new(), &BTreeMap::new()).unwrap();

        assert_eq!(
            trace.switchable_roots(),
            &BTreeSet::from([(0x02, 0x8400), (0x03, 0x8500)])
        );
    }

    #[test]
    fn recursive_call_site_converges_without_materializing_recursive_stacks() {
        let source = synthetic_source(
            &[
                (
                    0xC100,
                    &[
                        0x20, 0x80, 0xC1, 0xA9, 0x02, 0x8D, 0x00, 0xA0, 0x4C, 0x00, 0x84,
                    ],
                ),
                (
                    0xC180,
                    &[0xAD, 0x00, 0x04, 0xF0, 0x04, 0x20, 0x80, 0xC1, 0x60, 0x60],
                ),
            ],
            0xC100,
        );

        let trace =
            bind_reset_bank_entries(&source, 0xC100, &BTreeSet::new(), &BTreeMap::new()).unwrap();

        assert_eq!(trace.switchable_roots(), &BTreeSet::from([(0x02, 0x8400)]));
        assert!(trace.open_facts.is_empty());
    }

    #[test]
    fn terminal_scheduler_entry_hands_off_context_without_traversing_main_state() {
        let source = synthetic_source(
            &[
                (
                    0xC100,
                    &[
                        0xA9, 0x06, // LDA #$06
                        0x8D, 0x00, 0xA0, // STA $A000
                        0xA9, 0x05, // LDA #$05
                        0x85, 0x25, // STA $25
                        0x20, 0x20, 0xC1, // JSR $C120
                        0x00, // BRK
                    ],
                ),
                (0xC120, &[0x85, 0x25, 0x4C, 0x00, 0x84]),
            ],
            0xC100,
        );

        let trace = bind_reset_bank_entries(
            &source,
            0xC100,
            &BTreeSet::from([(FIXED_PRG_BANK, 0xC120)]),
            &BTreeMap::new(),
        )
        .unwrap();

        assert_eq!(
            trace.terminal_entry_contexts(FIXED_PRG_BANK, 0xC120),
            BTreeSet::from([(0x05, 0x06)])
        );
        assert!(
            trace
                .reachable_instruction_starts()
                .contains(&(FIXED_PRG_BANK, 0xC120))
        );
        assert!(!trace.switchable_roots().contains(&(0x06, 0x8400)));
    }

    #[test]
    fn saved_prg_bank_survives_a_temporary_mapper_selection() {
        let source = synthetic_source(
            &[
                (
                    0xC100,
                    &[
                        0xA9, 0x06, // LDA #$06
                        0x85, 0x51, // STA $51
                        0x8D, 0x00, 0xA0, // STA $A000
                        0x20, 0x20, 0xC1, // JSR $C120
                        0x4C, 0x00, 0x84, // JMP $8400
                    ],
                ),
                (
                    0xC120,
                    &[
                        0xA5, 0x51, // LDA $51
                        0x85, 0x08, // STA $08
                        0xA9, 0x02, // LDA #$02
                        0x8D, 0x00, 0xA0, // STA $A000
                        0xA5, 0x08, // LDA $08
                        0x8D, 0x00, 0xA0, // STA $A000
                        0x60, // RTS
                    ],
                ),
            ],
            0xC100,
        );

        let trace =
            bind_reset_bank_entries(&source, 0xC100, &BTreeSet::new(), &BTreeMap::new()).unwrap();

        assert_eq!(trace.switchable_roots(), &BTreeSet::from([(0x06, 0x8400)]));
        assert!(
            trace
                .open_fact_descriptions()
                .iter()
                .all(|fact| !fact.contains("bank_unknown"))
        );
    }

    #[test]
    fn pending_state_escape_discards_its_callers_inner_return() {
        let source = synthetic_source(
            &[
                (
                    0xC100,
                    &[
                        0xA9, 0x06, // LDA #$06
                        0x8D, 0x00, 0xA0, // STA $A000
                        0xA9, 0x05, // LDA #$05
                        0x8D, 0xCC, 0x05, // STA $05CC
                        0x20, 0x20, 0xC1, // JSR $C120
                        0x00, // BRK
                    ],
                ),
                (
                    0xC120,
                    &[
                        0x20, 0x5C, 0xE6, // JSR $E65C
                        0x20, 0x30, 0xC1, // JSR $C130; bypassed by the escape
                        0x60, // RTS
                    ],
                ),
                (
                    0xC130,
                    &[
                        0xA9, 0x02, // LDA #$02
                        0x8D, 0x00, 0xA0, // STA $A000
                        0x4C, 0x00, 0x84, // JMP $8400
                    ],
                ),
                (PENDING_STATE_ESCAPE_ENTRY, &PENDING_STATE_ESCAPE_CODE),
                (PENDING_STATE_ESCAPE_TARGET, &[0x60]),
            ],
            0xC100,
        );

        let trace =
            bind_reset_bank_entries(&source, 0xC100, &BTreeSet::new(), &BTreeMap::new()).unwrap();

        assert!(
            !trace
                .reachable_instruction_starts()
                .contains(&(FIXED_PRG_BANK, 0xC130))
        );
        assert!(!trace.switchable_roots().contains(&(0x02, 0x8400)));
        assert!(trace.open_facts.is_empty());
    }

    #[test]
    fn scheduler_closure_preserves_state_into_the_next_handler() {
        let source = synthetic_source(
            &[
                (
                    0xC100,
                    &[
                        0xA5, 0x25, // LDA $25
                        0x20, 0x4C, 0xC3, // JSR $C34C
                        0x10, 0xC1, 0x20, 0xC1, // inline target table
                    ],
                ),
                (
                    0xC110,
                    &[
                        0xA9, 0x03, // LDA #$03
                        0x85, 0x24, // STA $24
                        0xA9, 0x01, // LDA #$01
                        0x85, 0x25, // STA $25
                        0x60, // RTS
                    ],
                ),
                (
                    0xC120,
                    &[
                        0xA5, 0x24, // LDA $24
                        0x8D, 0xDB, 0x05, // STA $05DB
                        0x60, // RTS
                    ],
                ),
                (
                    INLINE_POINTER_DISPATCH_ADDRESS,
                    &INLINE_POINTER_DISPATCH_CODE,
                ),
            ],
            0xC100,
        );
        let selector_bounds = BTreeMap::from([(
            (FIXED_PRG_BANK, 0xC102),
            InlineDispatchSelectorBounds::from_source_producers(BTreeSet::from([0x00, 0x01])),
        )]);

        let trace = trace_fixed_scheduler_contexts(
            &source,
            0xC100,
            0xC102,
            0xC100,
            [(0x00, 0x06)],
            &BTreeMap::new(),
            &BTreeSet::new(),
            &selector_bounds,
            &BTreeMap::new(),
            &BTreeMap::new(),
        )
        .unwrap();

        assert_eq!(
            trace.inline_dispatch_contexts(FIXED_PRG_BANK, 0xC102),
            BTreeSet::from([(0x00, 0x06), (0x01, 0x06)])
        );
        assert!(
            trace
                .reachable_instruction_starts()
                .contains(&(FIXED_PRG_BANK, 0xC110))
        );
        assert!(
            trace
                .reachable_instruction_starts()
                .contains(&(FIXED_PRG_BANK, 0xC120))
        );
        assert_eq!(
            trace
                .control_state_write_values()
                .get(&(FIXED_PRG_BANK, 0xC112, OUTER_SCREEN_STATE,)),
            Some(&Some(BTreeSet::from([0x03])))
        );
        assert_eq!(
            trace.control_state_write_values().get(&(
                FIXED_PRG_BANK,
                0xC122,
                MAP_DIALOGUE_OUTER_STATE,
            )),
            Some(&Some(BTreeSet::from([0x03])))
        );
    }

    #[test]
    fn scheduler_gate_zero_runs_the_mapped_screen_without_dispatching_the_next_state() {
        let source = synthetic_source(
            &[
                (
                    0xC140,
                    &[
                        0x20, 0x80, 0xC1, // JSR $C180
                        0x00, // BRK
                    ],
                ),
                (
                    0xC180,
                    &[
                        0xA5, 0x23, // LDA $23
                        0xF0, 0x03, // BEQ $C187
                        0x4C, 0x90, 0xC1, // JMP $C190
                        0x4C, 0x00, 0x84, // JMP $8400
                    ],
                ),
                (
                    0xC190,
                    &[
                        0xA5, 0x25, // LDA $25
                        0x20, 0x4C, 0xC3, // JSR $C34C
                        0xA0, 0xC1, 0xB0, 0xC1, // inline target table
                    ],
                ),
                (
                    0xC1A0,
                    &[
                        0xA9, 0x00, // LDA #$00
                        0x85, 0x23, // STA $23; enter mapped-screen mode
                        0xA9, 0x01, // LDA #$01
                        0x85, 0x25, // STA $25; deferred next scheduler state
                        0x60, // RTS
                    ],
                ),
                (
                    0xC1B0,
                    &[
                        0xA9, 0x7F, // LDA #$7F
                        0x85, 0x24, // STA $24
                        0x60, // RTS
                    ],
                ),
                (
                    INLINE_POINTER_DISPATCH_ADDRESS,
                    &INLINE_POINTER_DISPATCH_CODE,
                ),
            ],
            0xC190,
        );
        let selector_bounds = BTreeMap::from([(
            (FIXED_PRG_BANK, 0xC192),
            InlineDispatchSelectorBounds::from_source_producers(BTreeSet::from([0x00, 0x01])),
        )]);

        let trace = trace_fixed_scheduler_contexts(
            &source,
            0xC190,
            0xC192,
            0xC140,
            [(0x00, 0x06)],
            &BTreeMap::new(),
            &BTreeSet::new(),
            &selector_bounds,
            &BTreeMap::new(),
            &BTreeMap::new(),
        )
        .unwrap();

        assert_eq!(
            trace.inline_dispatch_contexts(FIXED_PRG_BANK, 0xC192),
            BTreeSet::from([(0x00, 0x06)])
        );
        assert!(trace.switchable_roots().contains(&(0x06, 0x8400)));
        assert!(
            !trace
                .reachable_instruction_starts()
                .contains(&(FIXED_PRG_BANK, 0xC1B0))
        );
    }
}
