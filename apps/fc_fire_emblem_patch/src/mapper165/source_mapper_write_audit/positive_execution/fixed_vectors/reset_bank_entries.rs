use std::collections::{BTreeMap, BTreeSet, VecDeque};

use anyhow::{Context, Result, ensure};
use retro_rp2a03::{AddressingMode, Mnemonic, Operand, decode_bytes};

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

use super::{FIXED_CPU_START, FIXED_PRG_BANK, RESET_RAM_CLEAR_CODE, RESET_RAM_CLEAR_START};

mod call_effects;
mod trace_state;

use call_effects::{StateTransparentCallSummary, inspect_state_transparent_call};
use trace_state::{
    ActivationId, ByteValueSet, ResetTraceIdentity, ResetTraceState, ReturnContinuation,
    ReturnFrame,
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
}

impl InlineDispatchSelectorBounds {
    pub(in super::super) fn from_source_producers(selectors: BTreeSet<u8>) -> Self {
        Self {
            admitted_selectors: selectors.clone(),
            source_bound_produced_selectors: Some(selectors),
        }
    }

    pub(in super::super) fn from_handler_table(selectors: BTreeSet<u8>) -> Self {
        Self {
            admitted_selectors: selectors,
            source_bound_produced_selectors: None,
        }
    }

    fn admitted_selectors(&self) -> &BTreeSet<u8> {
        &self.admitted_selectors
    }

    fn source_bound_produced_selectors(&self) -> Option<&BTreeSet<u8>> {
        self.source_bound_produced_selectors.as_ref()
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

#[derive(Debug)]
pub(in super::super) struct StatefulBankExecution {
    switchable_roots: BTreeSet<(u8, u16)>,
    reachable_instruction_starts: BTreeSet<(u8, u16)>,
    open_facts: BTreeSet<String>,
    inline_dispatch_selectors: BTreeMap<(u8, u16), BTreeSet<u8>>,
    inline_dispatch_entry_banks: BTreeMap<(u8, u16, u8), BTreeSet<u8>>,
    terminal_entry_contexts: BTreeMap<(u8, u16), BTreeSet<(u8, u8)>>,
    indirect_write_sites_below_mapper_space: BTreeSet<(u8, u16, u8)>,
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
        &BTreeMap::new(),
        indirect_write_destination_bounds,
    )
    .context("trace reset-rooted source execution")
}

pub(in super::super) fn trace_fixed_scheduler_contexts(
    source: &Rom,
    state_load_address: u16,
    dispatch_call_address: u16,
    return_address: u16,
    entry_contexts: impl IntoIterator<Item = (u8, u8)>,
    inline_dispatch_selector_bounds: &BTreeMap<(u8, u16), InlineDispatchSelectorBounds>,
    indirect_write_destination_bounds: &BTreeMap<(u8, u16, u8), IndirectWriteDestinationBounds>,
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
        selectors.iter().copied(),
        "fixed scheduler positive state dispatch",
    )?;
    let targets = selectors
        .iter()
        .copied()
        .zip(dispatch.targets_in_selector_order())
        .collect::<BTreeMap<_, _>>();

    let mut pending = VecDeque::new();
    let mut activations = ActivationArena::default();
    let parent_activation = activations.root(FIXED_PRG_BANK, return_address);
    let mut return_flow = ReturnFlow::default();
    for (selector, mapped_prg_bank) in entry_contexts.iter().copied() {
        let target = targets[&selector];
        let target_bank = if target >= FIXED_CPU_START {
            FIXED_PRG_BANK
        } else {
            mapped_prg_bank
        };
        let handler_activation = activations.called(
            target_bank,
            target,
            FIXED_PRG_BANK,
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
        let mut state = ResetTraceState::at(target, handler_activation);
        state.write_memory(0x0025, Some(selector));
        state.write_prg_bank_shadows(Some(mapped_prg_bank));
        state.mapped_prg_bank = Some(mapped_prg_bank);
        state.set_accumulator(Some(selector.wrapping_mul(2)));
        pending.push_back(state);
    }
    let mut execution = trace_bank_state_entries(
        source,
        pending,
        activations,
        return_flow,
        &BTreeSet::new(),
        &BTreeSet::from([(FIXED_PRG_BANK, dispatch_call_address)]),
        inline_dispatch_selector_bounds,
        indirect_write_destination_bounds,
    )
    .context("trace one fixed-scheduler source epoch")?;
    execution.reachable_instruction_starts.extend([
        (FIXED_PRG_BANK, state_load_address),
        (FIXED_PRG_BANK, dispatch_call_address),
    ]);
    for (selector, mapped_prg_bank) in entry_contexts {
        execution
            .inline_dispatch_selectors
            .entry((FIXED_PRG_BANK, dispatch_call_address))
            .or_default()
            .insert(selector);
        execution
            .inline_dispatch_entry_banks
            .entry((FIXED_PRG_BANK, dispatch_call_address, selector))
            .or_default()
            .insert(mapped_prg_bank);
        if targets[&selector] < FIXED_CPU_START {
            execution
                .switchable_roots
                .insert((mapped_prg_bank, targets[&selector]));
        }
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
    inline_dispatch_selector_bounds: &BTreeMap<(u8, u16), InlineDispatchSelectorBounds>,
    indirect_write_destination_bounds: &BTreeMap<(u8, u16, u8), IndirectWriteDestinationBounds>,
) -> Result<StatefulBankExecution> {
    let mut visited = BTreeMap::<ResetTraceIdentity, ResetTraceState>::new();
    let mut switchable_roots = BTreeSet::new();
    let mut reachable_instruction_starts = BTreeSet::new();
    let mut open_facts = BTreeSet::new();
    let mut inline_dispatch_selectors = BTreeMap::<_, BTreeSet<_>>::new();
    let mut inline_dispatch_entry_banks = BTreeMap::<_, BTreeSet<_>>::new();
    let mut terminal_entry_contexts = BTreeMap::<_, BTreeSet<_>>::new();
    let mut indirect_write_sites_below_mapper_space = BTreeMap::<_, bool>::new();
    let mut transparent_call_summaries =
        BTreeMap::<(u8, u16), Option<StateTransparentCallSummary>>::new();

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
            &mut open_facts,
        )? {
            record_indirect_write_observation(
                &mut indirect_write_sites_below_mapper_space,
                observation,
            );
        }

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
                    taken.address = target;
                    pending.push_back(taken);
                }
                if condition != Some(true) {
                    if let Some(fallthrough) = fallthrough {
                        state.address = fallthrough;
                        pending.push_back(state);
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
                            open_facts.insert(format!(
                                "inline_dispatch@{physical_bank:02X}:{:04X}:selector_producer_unknown[handler_table_count={}]",
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
                let transparent_summary = match target_bank {
                    Some(target_bank) => {
                        let key = (target_bank, target);
                        if !transparent_call_summaries.contains_key(&key) {
                            let summary =
                                inspect_state_transparent_call(source, target_bank, target)?;
                            transparent_call_summaries.insert(key, summary);
                        }
                        transparent_call_summaries.get(&key).cloned().flatten()
                    }
                    None => None,
                };
                if let Some(summary) = transparent_summary {
                    record_fixed_to_switchable_entry(
                        &state,
                        target,
                        &mut switchable_roots,
                        &mut open_facts,
                    );
                    reachable_instruction_starts
                        .extend(summary.instruction_starts().iter().copied());
                    state.invalidate_registers_and_flags();
                    state.address = return_address;
                    pending.push_back(state);
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
        indirect_write_sites_below_mapper_space: indirect_write_sites_below_mapper_space
            .into_iter()
            .filter_map(|(site, is_below_mapper_space)| is_below_mapper_space.then_some(site))
            .collect(),
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

    let request_values = state.read_memory_values(0x05CC);
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

fn apply_data_effect(
    instruction: &retro_rp2a03::Instruction,
    state: &mut ResetTraceState,
    physical_bank: u8,
    indirect_write_destination_bounds: &BTreeMap<(u8, u16, u8), IndirectWriteDestinationBounds>,
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
            state.set_index_x(state.read_memory(u16::from(address)));
        }
        (Mnemonic::Ldy, AddressingMode::ZeroPage, Operand::Byte(address)) => {
            state.set_index_y(state.read_memory(u16::from(address)));
        }
        (Mnemonic::Lda, AddressingMode::Absolute, Operand::Word(address)) => {
            state.set_accumulator_values(state.read_memory_values(address));
        }
        (Mnemonic::Ldx, AddressingMode::Absolute, Operand::Word(address)) => {
            state.set_index_x(state.read_memory(address));
        }
        (Mnemonic::Ldy, AddressingMode::Absolute, Operand::Word(address)) => {
            state.set_index_y(state.read_memory(address));
        }
        (Mnemonic::Sta, AddressingMode::ZeroPage, Operand::Byte(address)) => {
            state.write_memory_values(u16::from(address), state.accumulator.clone());
        }
        (Mnemonic::Stx, AddressingMode::ZeroPage, Operand::Byte(address)) => {
            state.write_memory(u16::from(address), state.index_x);
        }
        (Mnemonic::Sty, AddressingMode::ZeroPage, Operand::Byte(address)) => {
            state.write_memory(u16::from(address), state.index_y);
        }
        (Mnemonic::Sta, AddressingMode::Absolute, Operand::Word(0xA000..=0xAFFF)) => {
            state.mapped_prg_bank = state.accumulator.singleton().map(|value| value & 0x0F);
        }
        (Mnemonic::Stx, AddressingMode::Absolute, Operand::Word(0xA000..=0xAFFF)) => {
            state.mapped_prg_bank = state.index_x.map(|value| value & 0x0F);
        }
        (Mnemonic::Sty, AddressingMode::Absolute, Operand::Word(0xA000..=0xAFFF)) => {
            state.mapped_prg_bank = state.index_y.map(|value| value & 0x0F);
        }
        (Mnemonic::Sta, AddressingMode::Absolute, Operand::Word(address)) => {
            state.write_memory_values(address, state.accumulator.clone());
        }
        (Mnemonic::Stx, AddressingMode::Absolute, Operand::Word(address)) => {
            state.write_memory(address, state.index_x);
        }
        (Mnemonic::Sty, AddressingMode::Absolute, Operand::Word(address)) => {
            state.write_memory(address, state.index_y);
        }
        (Mnemonic::Sta, AddressingMode::ZeroPageIndirectIndexedY, Operand::Byte(pointer)) => {
            let site = (physical_bank, state.address, pointer);
            if let (Some(low), Some(high), Some(index_y)) = (
                state.read_memory(u16::from(pointer)),
                state.read_memory(u16::from(pointer.wrapping_add(1))),
                state.index_y,
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
            state.set_index_x(state.accumulator.singleton());
        }
        (Mnemonic::Tay, AddressingMode::Implied, Operand::None) => {
            state.set_index_y(state.accumulator.singleton());
        }
        (Mnemonic::Txa, AddressingMode::Implied, Operand::None) => {
            state.set_accumulator(state.index_x);
        }
        (Mnemonic::Tya, AddressingMode::Implied, Operand::None) => {
            state.set_accumulator(state.index_y);
        }
        (Mnemonic::Inx, AddressingMode::Implied, Operand::None) => {
            state.set_index_x(state.index_x.map(|value| value.wrapping_add(1)));
        }
        (Mnemonic::Dex, AddressingMode::Implied, Operand::None) => {
            state.set_index_x(state.index_x.map(|value| value.wrapping_sub(1)));
        }
        (Mnemonic::Iny, AddressingMode::Implied, Operand::None) => {
            state.set_index_y(state.index_y.map(|value| value.wrapping_add(1)));
        }
        (Mnemonic::Dey, AddressingMode::Implied, Operand::None) => {
            state.set_index_y(state.index_y.map(|value| value.wrapping_sub(1)));
        }
        (Mnemonic::Inc, AddressingMode::ZeroPage, Operand::Byte(address)) => {
            let values = state
                .read_memory_values(u16::from(address))
                .map(|value| value.wrapping_add(1));
            state.write_memory_values(u16::from(address), values.clone());
            state.zero = values.uniform(|value| value == 0);
            state.negative = values.uniform(|value| value & 0x80 != 0);
        }
        (Mnemonic::Dec, AddressingMode::ZeroPage, Operand::Byte(address)) => {
            let values = state
                .read_memory_values(u16::from(address))
                .map(|value| value.wrapping_sub(1));
            state.write_memory_values(u16::from(address), values.clone());
            state.zero = values.uniform(|value| value == 0);
            state.negative = values.uniform(|value| value & 0x80 != 0);
        }
        (Mnemonic::Inc, AddressingMode::Absolute, Operand::Word(address)) => {
            let values = state
                .read_memory_values(address)
                .map(|value| value.wrapping_add(1));
            state.write_memory_values(address, values.clone());
            state.zero = values.uniform(|value| value == 0);
            state.negative = values.uniform(|value| value & 0x80 != 0);
        }
        (Mnemonic::Dec, AddressingMode::Absolute, Operand::Word(address)) => {
            let values = state
                .read_memory_values(address)
                .map(|value| value.wrapping_sub(1));
            state.write_memory_values(address, values.clone());
            state.zero = values.uniform(|value| value == 0);
            state.negative = values.uniform(|value| value & 0x80 != 0);
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
            compare_accumulator(state.accumulator.clone(), value, state);
        }
        (Mnemonic::Cpx, AddressingMode::Immediate, Operand::Byte(value)) => {
            compare(state.index_x, value, state);
        }
        (Mnemonic::Cpy, AddressingMode::Immediate, Operand::Byte(value)) => {
            compare(state.index_y, value, state);
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
        }
        _ => {}
    }
    Ok(indirect_write_observation)
}

fn compare(register: Option<u8>, operand: u8, state: &mut ResetTraceState) {
    state.zero = register.map(|value| value == operand);
    state.carry = register.map(|value| value >= operand);
    state.negative = register.map(|value| value.wrapping_sub(operand) & 0x80 != 0);
}

fn compare_accumulator(register: ByteValueSet, operand: u8, state: &mut ResetTraceState) {
    state.zero = register.uniform(|value| value == operand);
    state.carry = register.uniform(|value| value >= operand);
    state.negative = register.uniform(|value| value.wrapping_sub(operand) & 0x80 != 0);
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
            &bounds,
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
    fn one_scheduler_epoch_records_reentry_without_traversing_the_next_handler() {
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
                (0xC110, &[0xA9, 0x01, 0x85, 0x25, 0x60]),
                (0xC120, &[0xA9, 0x02, 0x85, 0x25, 0x60]),
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
            &selector_bounds,
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
            !trace
                .reachable_instruction_starts()
                .contains(&(FIXED_PRG_BANK, 0xC120))
        );
    }
}
