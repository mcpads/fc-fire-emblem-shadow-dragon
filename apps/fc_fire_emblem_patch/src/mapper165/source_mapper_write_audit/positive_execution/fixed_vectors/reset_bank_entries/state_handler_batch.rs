use super::*;

#[allow(clippy::too_many_arguments)]
pub(in super::super::super) fn trace_source_bound_inline_state_handler_batch(
    source: &Rom,
    dispatch_bank: u8,
    state_load_address: u16,
    dispatch_call_address: u16,
    return_address: u16,
    selector_memory_address: u16,
    preserved_call_state_addresses: &BTreeSet<u16>,
    selectors: &BTreeSet<u8>,
    mapped_prg_bank: u8,
    initial_memory_contexts: &[BTreeMap<u16, u8>],
    inline_dispatch_selector_bounds: &BTreeMap<(u8, u16), InlineDispatchSelectorBounds>,
    indirect_write_destination_bounds: &BTreeMap<(u8, u16, u8), IndirectWriteDestinationBounds>,
    absolute_indexed_write_bounds: &BTreeMap<(u8, u16), AbsoluteIndexedWriteDestinationBounds>,
) -> Result<StatefulBankExecution> {
    ensure!(
        dispatch_bank <= FIXED_PRG_BANK && mapped_prg_bank <= FIXED_PRG_BANK,
        "source-bound inline state-handler batch has a bank outside the MMC4 domain"
    );
    ensure!(
        dispatch_bank == FIXED_PRG_BANK || dispatch_bank == mapped_prg_bank,
        "switchable inline state-handler batch is not mapped in its owning physical bank"
    );
    ensure!(
        !selectors.is_empty() && !initial_memory_contexts.is_empty(),
        "source-bound inline state-handler batch has no selector or memory context"
    );
    ensure!(
        preserved_call_state_addresses.contains(&selector_memory_address)
            && preserved_call_state_addresses
                .iter()
                .all(|address| ResetTraceState::tracks_memory_address(*address)),
        "source-bound inline state-handler batch does not preserve its selector or names an untracked state"
    );
    ensure!(
        initial_memory_contexts.iter().all(|context| context
            .keys()
            .all(|address| ResetTraceState::tracks_memory_address(*address))),
        "source-bound inline state-handler batch received an untracked initial memory address"
    );
    ensure!(
        initial_memory_contexts
            .iter()
            .collect::<BTreeSet<_>>()
            .len()
            == initial_memory_contexts.len(),
        "source-bound inline state-handler batch received duplicate memory contexts"
    );
    let bounds = inline_dispatch_selector_bounds
        .get(&(dispatch_bank, dispatch_call_address))
        .context("source-bound inline state-handler batch has no owner-bound selector domain")?;
    ensure!(
        selectors.is_subset(bounds.admitted_selectors()),
        "source-bound inline state-handler batch left its owner-bound handler table"
    );
    let dispatch = bind_inline_pointer_dispatch(
        source,
        dispatch_bank,
        dispatch_call_address,
        selectors.iter().copied(),
        "source-bound inline state-handler batch",
    )?;
    let targets = selectors
        .iter()
        .copied()
        .zip(dispatch.targets_in_selector_order())
        .collect::<Vec<_>>();

    let return_bank = if return_address >= FIXED_CPU_START {
        FIXED_PRG_BANK
    } else {
        dispatch_bank
    };
    let mut activations = ActivationArena::joining_same_call_site_lineages();
    let parent_activation = activations.root(return_bank, return_address);
    let mut return_flow = ReturnFlow::default();
    let mut pending = VecDeque::new();
    let mut switchable_handler_roots = BTreeSet::new();
    let mut state_transition_call_summaries =
        TrackedStateCallSummaries::preserving_only(preserved_call_state_addresses.clone())?;
    for (selector, target) in targets {
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
        for initial_memory in initial_memory_contexts {
            let mut state = ResetTraceState::at(target, handler_activation);
            for (&address, &value) in initial_memory {
                state.write_memory(address, Some(value));
            }
            state.write_memory(selector_memory_address, Some(selector));
            state.write_prg_bank_shadows(Some(mapped_prg_bank));
            state.mapped_prg_bank = Some(mapped_prg_bank);
            state.set_accumulator(Some(selector.wrapping_mul(2)));
            pending.push_back(state);
        }
        if target < FIXED_CPU_START {
            switchable_handler_roots.insert((target_bank, target));
        }
    }

    let mut execution = trace_bank_state_entries(
        source,
        pending,
        activations,
        return_flow,
        &BTreeSet::from([(return_bank, return_address)]),
        &BTreeSet::from([(dispatch_bank, dispatch_call_address)]),
        None,
        inline_dispatch_selector_bounds,
        indirect_write_destination_bounds,
        absolute_indexed_write_bounds,
        &mut state_transition_call_summaries,
    )
    .context("trace source-bound inline state-handler batch")?;
    execution
        .reachable_instruction_starts
        .insert((dispatch_bank, state_load_address));
    execution.switchable_roots.extend(switchable_handler_roots);
    Ok(execution)
}
