use std::collections::{BTreeMap, BTreeSet};

use anyhow::{Result, ensure};

use crate::{
    mapper165::executable_mapper_writes::MappedPrgLocation,
    rom::Rom,
    title_graphics::{TitleStateExecution, bind_title_state_execution},
};

use super::{FIXED_PRG_BANK, source_mapped_location};

mod chapter_map_loader;
mod fixed_scheduler;
mod fixed_vectors;
mod shared_menu_request;
mod state_accesses;

use chapter_map_loader::{CHAPTER_MAP_INDIRECT_WRITE_SITE, bind_chapter_map_loader_destination};
use fixed_scheduler::bind_fixed_scheduler_execution;
use fixed_vectors::bind_fixed_vector_execution;
use shared_menu_request::bind_shared_menu_execution_source;
pub(super) use state_accesses::PositiveStateAccess;
use state_accesses::bind_positive_state_accesses;

const BATTLE_PHASE_GRAPH: &str = "battle_phase_catalog";
const DIALOGUE_INTERRUPT_AUDIO_GRAPH: &str = "main_dialogue_nmi_and_audio_positive_graph";
const FIXED_HARDWARE_VECTOR_GRAPH: &str = "fixed_hardware_vector_direct_graph";
const FIXED_SCHEDULER_EXECUTION_GRAPH: &str = "fixed_scheduler_state_execution_graph";
const RESET_STATEFUL_EXECUTION_GRAPH: &str = "reset_stateful_execution_graph";
const TITLE_STATE_EXECUTION_GRAPH: &str = "title_state_execution_graph";

/// Positive source execution slices already bound by their owning battle and dialogue contracts.
/// This is deliberately not a complete executable-root ledger.
pub(super) struct SourcePositiveExecutionGraph {
    instruction_roles: BTreeMap<(u8, u16), BTreeSet<&'static str>>,
    indirect_write_sites_below_mapper_space: BTreeSet<(u8, u16, u8)>,
    hardware_vector_slot_count: usize,
    hardware_vector_root_count: usize,
    fixed_vector_instruction_count: usize,
    reset_stateful_execution_instruction_count: usize,
    fixed_scheduler_source_bound_producer_instruction_count: usize,
    fixed_scheduler_positive_execution_instruction_count: usize,
    fixed_scheduler_table_selector_count: usize,
    fixed_scheduler_table_handler_count: usize,
    fixed_scheduler_positive_handler_root_count: usize,
    fixed_scheduler_reset_entry_context_count: usize,
    fixed_scheduler_positive_entry_context_count: usize,
    fixed_scheduler_known_produced_states: Vec<String>,
    fixed_scheduler_positive_states: Vec<String>,
    fixed_scheduler_bound_switchable_roots: Vec<String>,
    fixed_scheduler_open_control_facts: Vec<String>,
    fixed_vector_bound_switchable_roots: Vec<String>,
    fixed_vector_open_control_edges: Vec<String>,
    reset_bound_switchable_roots: Vec<String>,
    reset_open_control_facts: Vec<String>,
    title_state_selector_count: usize,
    title_state_handler_root_count: usize,
    title_state_open_control_facts: Vec<String>,
    state_accesses: Vec<PositiveStateAccess>,
}

impl SourcePositiveExecutionGraph {
    pub(super) fn instruction_starts(&self) -> impl Iterator<Item = (u8, u16)> + '_ {
        self.instruction_roles.keys().copied()
    }

    pub(super) fn instruction_count(&self) -> usize {
        self.instruction_roles.len()
    }

    pub(super) fn roles_at(&self, bank: u8, address: u16) -> Option<&BTreeSet<&'static str>> {
        self.instruction_roles.get(&(bank, address))
    }

    pub(super) fn indirect_write_sites_below_mapper_space(&self) -> &BTreeSet<(u8, u16, u8)> {
        &self.indirect_write_sites_below_mapper_space
    }

    pub(super) fn hardware_vector_slot_count(&self) -> usize {
        self.hardware_vector_slot_count
    }

    pub(super) fn hardware_vector_root_count(&self) -> usize {
        self.hardware_vector_root_count
    }

    pub(super) fn fixed_vector_instruction_count(&self) -> usize {
        self.fixed_vector_instruction_count
    }

    pub(super) fn reset_stateful_execution_instruction_count(&self) -> usize {
        self.reset_stateful_execution_instruction_count
    }

    pub(super) fn fixed_scheduler_source_bound_producer_instruction_count(&self) -> usize {
        self.fixed_scheduler_source_bound_producer_instruction_count
    }

    pub(super) fn fixed_scheduler_positive_execution_instruction_count(&self) -> usize {
        self.fixed_scheduler_positive_execution_instruction_count
    }

    pub(super) fn fixed_scheduler_table_selector_count(&self) -> usize {
        self.fixed_scheduler_table_selector_count
    }

    pub(super) fn fixed_scheduler_table_handler_count(&self) -> usize {
        self.fixed_scheduler_table_handler_count
    }

    pub(super) fn fixed_scheduler_positive_handler_root_count(&self) -> usize {
        self.fixed_scheduler_positive_handler_root_count
    }

    pub(super) fn fixed_scheduler_reset_entry_context_count(&self) -> usize {
        self.fixed_scheduler_reset_entry_context_count
    }

    pub(super) fn fixed_scheduler_positive_entry_context_count(&self) -> usize {
        self.fixed_scheduler_positive_entry_context_count
    }

    pub(super) fn fixed_scheduler_known_produced_states(&self) -> &[String] {
        &self.fixed_scheduler_known_produced_states
    }

    pub(super) fn fixed_scheduler_positive_states(&self) -> &[String] {
        &self.fixed_scheduler_positive_states
    }

    pub(super) fn fixed_scheduler_bound_switchable_roots(&self) -> &[String] {
        &self.fixed_scheduler_bound_switchable_roots
    }

    pub(super) fn fixed_scheduler_open_control_facts(&self) -> &[String] {
        &self.fixed_scheduler_open_control_facts
    }

    pub(super) fn fixed_vector_bound_switchable_roots(&self) -> &[String] {
        &self.fixed_vector_bound_switchable_roots
    }

    pub(super) fn fixed_vector_open_control_edges(&self) -> &[String] {
        &self.fixed_vector_open_control_edges
    }

    pub(super) fn reset_bound_switchable_roots(&self) -> &[String] {
        &self.reset_bound_switchable_roots
    }

    pub(super) fn reset_open_control_facts(&self) -> &[String] {
        &self.reset_open_control_facts
    }

    pub(super) fn title_state_selector_count(&self) -> usize {
        self.title_state_selector_count
    }

    pub(super) fn title_state_handler_root_count(&self) -> usize {
        self.title_state_handler_root_count
    }

    pub(super) fn title_state_open_control_facts(&self) -> &[String] {
        &self.title_state_open_control_facts
    }

    pub(super) fn state_accesses(&self) -> &[PositiveStateAccess] {
        &self.state_accesses
    }

    pub(super) fn mapped_instruction_starts(&self) -> Result<BTreeSet<MappedPrgLocation>> {
        self.instruction_starts()
            .map(|(bank, address)| source_mapped_location(bank, address))
            .collect()
    }
}

pub(super) fn bind_source_positive_execution_graph(
    source: &Rom,
) -> Result<SourcePositiveExecutionGraph> {
    let battle =
        crate::mapper165::battle_codebook_plan::phase_cooccurrence::battle_phase_reachable_instruction_starts(
            source,
        )?;
    let mut source_bound_indirect_destinations =
        crate::mapper165::battle_codebook_plan::bind_indirect_write_destination_bounds(source)?;
    let chapter_map_destination = bind_chapter_map_loader_destination(source)?;
    ensure!(
        source_bound_indirect_destinations
            .insert(CHAPTER_MAP_INDIRECT_WRITE_SITE, chapter_map_destination)
            .is_none(),
        "chapter map loader writer overlaps an existing indirect-write destination owner"
    );
    let dialogue_interrupt_audio =
        crate::full_translation_install::bind_dialogue_interrupt_audio_mapper_write_slice(source)?;
    let title_state: TitleStateExecution = bind_title_state_execution(source)?;
    let shared_menu_controller = crate::shop_flow::bind_shared_menu_controller_source(source)?;
    let shared_menu = bind_shared_menu_execution_source(source, &shared_menu_controller)?;
    for (&site, destination) in shared_menu.indirect_write_destinations() {
        ensure!(
            source_bound_indirect_destinations
                .insert(site, destination.clone())
                .is_none(),
            "shared-menu writer overlaps an existing indirect-write destination owner at {:02X}:${:04X}",
            site.0,
            site.1,
        );
    }
    let source_bound_indirect_sites = source_bound_indirect_destinations
        .keys()
        .copied()
        .collect::<BTreeSet<_>>();
    let fixed_vectors = bind_fixed_vector_execution(source, &source_bound_indirect_destinations)?;
    let fixed_scheduler_entry_contexts = fixed_vectors
        .reset_terminal_entry_contexts(FIXED_PRG_BANK, fixed_scheduler::FIXED_SCHEDULER_ENTRY);
    let fixed_scheduler = bind_fixed_scheduler_execution(
        source,
        &title_state,
        &shared_menu,
        &fixed_scheduler_entry_contexts,
        &source_bound_indirect_destinations,
    )?;
    ensure!(
        fixed_vectors
            .bound_switchable_roots()
            .iter()
            .all(|root| dialogue_interrupt_audio
                .reachable_instruction_starts
                .contains(root)),
        "a fixed-vector switchable root is not owned by the existing dialogue, interrupt, and audio execution graph"
    );

    let mut instruction_roles = BTreeMap::<_, BTreeSet<_>>::new();
    for (role, starts) in [
        (BATTLE_PHASE_GRAPH, battle.iter()),
        (
            DIALOGUE_INTERRUPT_AUDIO_GRAPH,
            dialogue_interrupt_audio.reachable_instruction_starts.iter(),
        ),
        (
            FIXED_HARDWARE_VECTOR_GRAPH,
            fixed_vectors.reachable_instruction_starts().iter(),
        ),
        (
            RESET_STATEFUL_EXECUTION_GRAPH,
            fixed_vectors.reset_reachable_instruction_starts().iter(),
        ),
        (
            FIXED_SCHEDULER_EXECUTION_GRAPH,
            fixed_scheduler.reachable_instruction_starts().iter(),
        ),
        (
            TITLE_STATE_EXECUTION_GRAPH,
            title_state.reachable_instruction_starts().iter(),
        ),
    ] {
        for &(bank, address) in starts {
            let location = normalize_source_location(bank, address)?;
            instruction_roles.entry(location).or_default().insert(role);
        }
    }
    ensure!(
        !instruction_roles.is_empty(),
        "source positive execution graph contains no instructions"
    );

    // These source contracts already report the physical PRG bank used to bind each instruction.
    // Preserve that identity instead of reinterpreting it as the caller's switchable-bank context.
    let indirect_write_sites_below_mapper_space = source_bound_indirect_sites
        .iter()
        .chain(
            dialogue_interrupt_audio
                .indirect_write_sites_below_mapper_space
                .iter(),
        )
        .chain(fixed_vectors.indirect_write_sites_below_mapper_space())
        .copied()
        .collect();

    let state_accesses = bind_positive_state_accesses(source, &instruction_roles)?;
    let reset_open_control_facts = fixed_vectors
        .reset_open_control_fact_descriptions()
        .to_vec();
    let mut reset_bound_switchable_roots = fixed_vectors.reset_bound_switchable_roots().clone();
    reset_bound_switchable_roots.extend(
        title_state
            .selector_targets()
            .values()
            .chain(title_state.animation_selector_targets().values())
            .map(|target| (0x0D, *target)),
    );
    let title_state_open_control_facts = title_state.open_control_fact_descriptions();
    Ok(SourcePositiveExecutionGraph {
        instruction_roles,
        indirect_write_sites_below_mapper_space,
        hardware_vector_slot_count: fixed_vectors.vector_slot_count(),
        hardware_vector_root_count: fixed_vectors.unique_vector_root_count(),
        fixed_vector_instruction_count: fixed_vectors.reachable_instruction_starts().len(),
        reset_stateful_execution_instruction_count: fixed_vectors
            .reset_reachable_instruction_starts()
            .len(),
        fixed_scheduler_source_bound_producer_instruction_count: fixed_scheduler
            .source_bound_producer_instruction_starts()
            .len(),
        fixed_scheduler_positive_execution_instruction_count: fixed_scheduler
            .reachable_instruction_starts()
            .len(),
        fixed_scheduler_table_selector_count: fixed_scheduler.table_selector_domain().len(),
        fixed_scheduler_table_handler_count: fixed_scheduler
            .selector_targets()
            .values()
            .collect::<BTreeSet<_>>()
            .len(),
        fixed_scheduler_positive_handler_root_count: fixed_scheduler
            .positive_selector_domain()
            .iter()
            .filter_map(|selector| fixed_scheduler.selector_targets().get(selector))
            .collect::<BTreeSet<_>>()
            .len(),
        fixed_scheduler_reset_entry_context_count: fixed_scheduler.reset_entry_contexts().len(),
        fixed_scheduler_positive_entry_context_count: fixed_scheduler
            .positive_entry_contexts()
            .len(),
        fixed_scheduler_known_produced_states: fixed_scheduler
            .known_produced_states()
            .iter()
            .map(|state| format!("0x{state:02X}"))
            .collect(),
        fixed_scheduler_positive_states: fixed_scheduler
            .positive_selector_domain()
            .iter()
            .map(|state| format!("0x{state:02X}"))
            .collect(),
        fixed_scheduler_bound_switchable_roots: fixed_scheduler
            .bound_switchable_roots()
            .iter()
            .map(|(bank, address)| format!("{bank:02X}:${address:04X}"))
            .collect(),
        fixed_scheduler_open_control_facts: fixed_scheduler
            .open_control_fact_descriptions()
            .to_vec(),
        fixed_vector_bound_switchable_roots: fixed_vectors.bound_switchable_root_descriptions(),
        fixed_vector_open_control_edges: fixed_vectors.open_control_edge_descriptions(),
        reset_bound_switchable_roots: reset_bound_switchable_roots
            .iter()
            .map(|(bank, address)| format!("{bank:02X}:${address:04X}"))
            .collect(),
        reset_open_control_facts,
        title_state_selector_count: title_state.selector_domain().len(),
        title_state_handler_root_count: title_state
            .selector_targets()
            .values()
            .collect::<BTreeSet<_>>()
            .len(),
        title_state_open_control_facts,
        state_accesses,
    })
}

fn normalize_source_location(bank: u8, address: u16) -> Result<(u8, u16)> {
    ensure!(
        address >= 0x8000,
        "source positive execution graph escaped PRG space at {bank:02X}:${address:04X}"
    );
    Ok((
        if address >= 0xC000 {
            FIXED_PRG_BANK
        } else {
            bank
        },
        address,
    ))
}
