use std::collections::{BTreeMap, BTreeSet, VecDeque};

use anyhow::{Context, Result, ensure};

use crate::{
    chapter_transition::{
        bind_ending_character_animation_dispatch_source, bind_ending_dialogue_progress_boundaries,
        bind_ending_sequence_phase_dispatch_source, bind_ending_sequence_phase_seed,
    },
    mapper165::battle_codebook_plan::IndirectWriteDestinationBounds,
    rom::Rom,
};

use super::{
    control_state::{
        ObservedControlStateWrites, known_control_state_write_values,
        merge_observed_control_state_writes,
    },
    fixed_vectors::{
        InlineDispatchSelectorBounds, StatefulBankExecution,
        trace_source_bound_inline_state_continuation, trace_source_bound_inline_state_handler,
    },
    indexed_write_destinations::AbsoluteIndexedWriteDestinationBounds,
};

const PHASE_STATE_LOAD: u16 = 0x9F15;
const PHASE_RETURN_ADDRESS: u16 = 0x9ECF;

#[derive(Debug)]
pub(super) struct EndingSequencePositiveExecution {
    produced_selectors: BTreeSet<u8>,
    reachable_instruction_starts: BTreeSet<(u8, u16)>,
    indirect_write_sites_below_mapper_space: BTreeSet<(u8, u16, u8)>,
    control_state_write_values: ObservedControlStateWrites,
    open_control_facts: Vec<String>,
}

impl EndingSequencePositiveExecution {
    pub(super) fn produced_selectors(&self) -> &BTreeSet<u8> {
        &self.produced_selectors
    }

    pub(super) fn reachable_instruction_starts(&self) -> &BTreeSet<(u8, u16)> {
        &self.reachable_instruction_starts
    }

    pub(super) fn indirect_write_sites_below_mapper_space(&self) -> &BTreeSet<(u8, u16, u8)> {
        &self.indirect_write_sites_below_mapper_space
    }

    pub(super) fn control_state_write_values(&self) -> &ObservedControlStateWrites {
        &self.control_state_write_values
    }

    pub(super) fn open_control_fact_descriptions(&self) -> &[String] {
        &self.open_control_facts
    }
}

pub(super) fn bind_ending_sequence_positive_execution(
    source: &Rom,
    indirect_write_destination_bounds: &BTreeMap<(u8, u16, u8), IndirectWriteDestinationBounds>,
    absolute_indexed_write_bounds: &BTreeMap<(u8, u16), AbsoluteIndexedWriteDestinationBounds>,
) -> Result<EndingSequencePositiveExecution> {
    let seed = bind_ending_sequence_phase_seed(source)?;
    let phase = bind_ending_sequence_phase_dispatch_source(source)?;
    let character_animation = bind_ending_character_animation_dispatch_source(source)?;
    let progress_boundaries = bind_ending_dialogue_progress_boundaries(source)?
        .into_iter()
        .map(|boundary| (boundary.phase(), boundary))
        .collect::<BTreeMap<_, _>>();
    ensure!(
        progress_boundaries.len() == 2
            && progress_boundaries
                .keys()
                .all(|selector| phase.admitted_selectors().contains(selector)),
        "ending dialogue progress boundaries left the source phase table"
    );
    ensure!(
        seed.selector_address() == phase.selector_address()
            && phase.dispatch_call() == PHASE_STATE_LOAD + 3,
        "ending phase seed and dispatcher no longer share one selector"
    );
    ensure!(
        character_animation.prg_bank() == phase.prg_bank()
            && progress_boundaries.get(&0x15).is_some_and(|boundary| {
                boundary.continuation_address() < character_animation.dispatch_call()
                    && character_animation.dispatch_call() < 0xA384
            }),
        "ending character-animation dispatch left the completion-phase continuation"
    );

    let selector_bounds = BTreeMap::from([
        (
            (phase.prg_bank(), phase.dispatch_call()),
            InlineDispatchSelectorBounds::from_handler_table(phase.admitted_selectors().clone())
                .with_selector_memory_address(phase.selector_address()),
        ),
        (
            (phase.prg_bank(), phase.inner_dispatch_call()),
            InlineDispatchSelectorBounds::from_source_producers(
                phase.inner_produced_selectors().clone(),
            )
            .with_selector_memory_address(phase.inner_selector_address()),
        ),
        (
            (
                character_animation.prg_bank(),
                character_animation.dispatch_call(),
            ),
            InlineDispatchSelectorBounds::from_source_producers(
                character_animation.produced_selectors().clone(),
            )
            .with_selector_memory_address(character_animation.selector_address()),
        ),
    ]);

    let initial_selector = seed.selector();
    ensure!(
        phase.admitted_selectors().contains(&initial_selector),
        "ending phase seed left the source handler table"
    );
    let mut produced_selectors = BTreeSet::from([initial_selector]);
    let mut traced_selectors = BTreeSet::new();
    let mut pending = VecDeque::from([initial_selector]);
    let mut aggregate: Option<StatefulBankExecution> = None;

    while let Some(selector) = pending.pop_front() {
        if !traced_selectors.insert(selector) {
            continue;
        }
        let trace = if let Some(boundary) = progress_boundaries.get(&selector) {
            ensure!(
                boundary.prg_bank() == phase.prg_bank(),
                "ending dialogue progress boundary moved outside its phase bank"
            );
            let mut boundary_trace: Option<StatefulBankExecution> = None;
            for progress_value in [boundary.pending_value(), boundary.asserted_value()] {
                let trace = trace_source_bound_inline_state_continuation(
                    source,
                    phase.prg_bank(),
                    PHASE_STATE_LOAD,
                    phase.dispatch_call(),
                    PHASE_RETURN_ADDRESS,
                    phase.selector_address(),
                    selector,
                    phase.prg_bank(),
                    boundary.handler_address(),
                    boundary.continuation_address(),
                    boundary.prefix_instruction_starts(),
                    &BTreeMap::from([
                        (phase.selector_address(), selector),
                        (boundary.progress_flag_address(), progress_value),
                    ]),
                    &selector_bounds,
                    indirect_write_destination_bounds,
                    absolute_indexed_write_bounds,
                )?;
                match &mut boundary_trace {
                    Some(boundary_trace) => boundary_trace.merge(trace),
                    None => boundary_trace = Some(trace),
                }
            }
            boundary_trace.context("ending dialogue progress boundary produced no trace")?
        } else {
            trace_source_bound_inline_state_handler(
                source,
                phase.prg_bank(),
                PHASE_STATE_LOAD,
                phase.dispatch_call(),
                PHASE_RETURN_ADDRESS,
                phase.selector_address(),
                selector,
                phase.prg_bank(),
                &BTreeMap::from([(phase.selector_address(), selector)]),
                &selector_bounds,
                indirect_write_destination_bounds,
                absolute_indexed_write_bounds,
            )?
        };
        let observed = known_control_state_write_values(
            trace.control_state_write_values(),
            phase.selector_address(),
        );
        ensure!(
            observed.is_subset(phase.admitted_selectors()),
            "ending phase execution produced a selector beyond its source handler table: {observed:02X?}"
        );
        for produced in observed {
            if produced_selectors.insert(produced) {
                pending.push_back(produced);
            }
        }
        match &mut aggregate {
            Some(aggregate) => aggregate.merge(trace),
            None => aggregate = Some(trace),
        }
    }
    ensure!(
        traced_selectors == produced_selectors,
        "ending phase producer worklist did not trace every discovered selector"
    );
    let aggregate = aggregate.context("ending phase producer worklist traced no handler")?;
    ensure!(
        character_animation
            .producer_instruction_starts()
            .iter()
            .all(|address| aggregate
                .reachable_instruction_starts()
                .contains(&(character_animation.prg_bank(), *address))),
        "ending character-animation selector producer left the positive phase graph"
    );
    let mut open_control_facts = aggregate.open_fact_descriptions();
    let unproduced = phase
        .admitted_selectors()
        .difference(&produced_selectors)
        .copied()
        .collect::<Vec<_>>();
    if !unproduced.is_empty() {
        open_control_facts.push(format!(
            "ending_phase_handler_table:selectors_not_source_produced={unproduced:02X?}"
        ));
    }
    open_control_facts.sort();
    open_control_facts.dedup();

    let mut control_state_write_values = ObservedControlStateWrites::new();
    merge_observed_control_state_writes(
        &mut control_state_write_values,
        aggregate.control_state_write_values(),
    );
    Ok(EndingSequencePositiveExecution {
        produced_selectors,
        reachable_instruction_starts: aggregate.reachable_instruction_starts().clone(),
        indirect_write_sites_below_mapper_space: aggregate
            .indirect_write_sites_below_mapper_space()
            .clone(),
        control_state_write_values,
        open_control_facts,
    })
}
