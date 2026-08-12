use std::collections::BTreeSet;

use anyhow::Result;
use serde::Serialize;

use crate::{
    dialogue_assets::EncodedMainDialogueDisplayStorage,
    dialogue_inventory::main_dialogue_runtime_handler_roots,
    mapper165::battle_codebook_plan::BATTLE_RUNTIME_STORAGE_END, rom::Rom, sha1_hex,
};

mod access_trace;
mod concurrent_access;
mod source_contract;

use access_trace::{AccessDirection, AccessForm, AccessSite, trace_main_dialogue_accesses};
use concurrent_access::{ConcurrentRuntimeAccessContract, bind_concurrent_runtime_accesses};
use source_contract::{RuntimeStateSourceAccessContract, bind_runtime_state_source_accesses};

const CANDIDATE_START: u16 = 0x07F0;
const CANDIDATE_END: u16 = 0x07F4;

#[derive(Serialize)]
pub(super) struct DialogueRuntimeStateStoragePlan {
    strategy: &'static str,
    candidate_cpu_range_hex: &'static str,
    required_byte_count: usize,
    ownership_lifetime: &'static str,
    main_dialogue_handler_root_count: usize,
    main_dialogue_reachable_instruction_count: usize,
    main_dialogue_reachable_instruction_catalog_sha1: String,
    concurrent_access_contract: ConcurrentRuntimeAccessContract,
    direct_access_overlap_count: usize,
    indexed_access_potential_overlap_count: usize,
    indirect_access_site_count: usize,
    direct_access_overlaps: Vec<MemoryAccessSite>,
    indexed_access_potential_overlaps: Vec<MemoryAccessSite>,
    indirect_access_sites: Vec<MemoryAccessSite>,
    source_access_contract: RuntimeStateSourceAccessContract,
    main_dialogue_direct_accesses_exclude_candidate: bool,
    main_dialogue_indexed_access_bounds_proven: bool,
    main_dialogue_indirect_access_ranges_proven: bool,
    main_dialogue_queue_bound_proven: bool,
    battle_reservation_excludes_candidate: bool,
    inactive_lifetime_may_clobber_candidate: bool,
    future_runtime_lifecycle_contract: RuntimeLifecycleContract,
    every_entry_cold_initializes_all_bytes: bool,
    runtime_initializer_emitted: bool,
    selected_cpu_range_hex: Option<&'static str>,
    selection_complete: bool,
    complete: bool,
}

#[derive(Serialize)]
struct RuntimeLifecycleContract {
    ownership_begin: &'static str,
    ownership_continue: &'static str,
    ownership_invalidate: &'static str,
    cold_entry_writes_all_five_bytes_before_any_selector_read: bool,
    inactive_selector_ignores_all_five_bytes: bool,
    implementation_required_before_rom_emission: bool,
}

#[derive(Clone, Debug, Serialize)]
struct MemoryAccessSite {
    prg_bank_hex: String,
    cpu_address_hex: String,
    access: &'static str,
    address_form: &'static str,
    operand_hex: String,
}

pub(super) fn plan_dialogue_runtime_state_storage(
    source: &Rom,
    encoded_display: &EncodedMainDialogueDisplayStorage,
) -> Result<DialogueRuntimeStateStoragePlan> {
    let roots = main_dialogue_runtime_handler_roots();
    let trace = trace_main_dialogue_accesses(source, &roots)?;
    let catalog = trace
        .visited
        .iter()
        .flat_map(|(bank, address)| [*bank].into_iter().chain(address.to_le_bytes()))
        .collect::<Vec<_>>();
    let direct_accesses_exclude_candidate = trace.direct_overlaps.is_empty();
    let source_access_contract = bind_runtime_state_source_accesses(source, &trace)?;
    let source_lifetime_accesses_exclude_candidate =
        source_access_contract.source_lifetime_accesses_exclude_candidate();
    let main_dialogue_queue_bound_proven = source_access_contract.queue_bound_proven();
    let main_dialogue_indirect_access_ranges_proven =
        source_access_contract.indirect_access_ranges_proven();
    let concurrent_access_contract = bind_concurrent_runtime_accesses(
        source,
        &encoded_display.transition_mirrors,
        main_dialogue_queue_bound_proven,
    )?;
    let battle_reservation_excludes_candidate = CANDIDATE_START > BATTLE_RUNTIME_STORAGE_END;
    let selection_complete = direct_accesses_exclude_candidate
        && source_lifetime_accesses_exclude_candidate
        && concurrent_access_contract.every_concurrent_writer_excludes_candidate()
        && main_dialogue_queue_bound_proven
        && battle_reservation_excludes_candidate;

    Ok(DialogueRuntimeStateStoragePlan {
        strategy: "own one five-byte scratch range only from a cold-initialized main-dialogue entry through its terminal or external-caller invalidation; inactive screens may clobber it",
        candidate_cpu_range_hex: "0x07F0..0x07F4",
        required_byte_count: usize::from(CANDIDATE_END - CANDIDATE_START + 1),
        ownership_lifetime: "main dialogue active, including page transitions; battle and every inactive or external-caller lifetime are excluded",
        main_dialogue_handler_root_count: roots.len(),
        main_dialogue_reachable_instruction_count: trace.visited.len(),
        main_dialogue_reachable_instruction_catalog_sha1: sha1_hex(&catalog),
        concurrent_access_contract,
        direct_access_overlap_count: trace.direct_overlaps.len(),
        indexed_access_potential_overlap_count: trace.indexed_potential_overlaps.len(),
        indirect_access_site_count: trace.indirect_sites.len(),
        direct_access_overlaps: report_sites(&trace.direct_overlaps),
        indexed_access_potential_overlaps: report_sites(&trace.indexed_potential_overlaps),
        indirect_access_sites: report_sites(&trace.indirect_sites),
        source_access_contract,
        main_dialogue_direct_accesses_exclude_candidate: direct_accesses_exclude_candidate,
        main_dialogue_indexed_access_bounds_proven: main_dialogue_queue_bound_proven,
        main_dialogue_indirect_access_ranges_proven,
        main_dialogue_queue_bound_proven,
        battle_reservation_excludes_candidate,
        inactive_lifetime_may_clobber_candidate: true,
        future_runtime_lifecycle_contract: RuntimeLifecycleContract {
            ownership_begin: "every direct entry and every E7 resume performs one cold initialization before publishing a request",
            ownership_continue: "E4, E6, and visible-page transitions retain ownership only while the original main-dialogue active state remains true",
            ownership_invalidate: "E7 handoff, every terminal path, reset, save/load boundary, and every inactive selector path invalidate ownership",
            cold_entry_writes_all_five_bytes_before_any_selector_read: true,
            inactive_selector_ignores_all_five_bytes: true,
            implementation_required_before_rom_emission: true,
        },
        every_entry_cold_initializes_all_bytes: false,
        runtime_initializer_emitted: false,
        selected_cpu_range_hex: selection_complete.then_some("0x07F0..0x07F4"),
        selection_complete,
        complete: false,
    })
}

impl DialogueRuntimeStateStoragePlan {
    pub(super) fn selected_cpu_range_hex(&self) -> Option<&'static str> {
        self.selected_cpu_range_hex
    }

    pub(super) fn selection_complete(&self) -> bool {
        self.selection_complete
    }
}

fn report_sites(sites: &BTreeSet<AccessSite>) -> Vec<MemoryAccessSite> {
    sites
        .iter()
        .map(|site| MemoryAccessSite {
            prg_bank_hex: format!("0x{:02X}", site.bank),
            cpu_address_hex: format!("0x{:04X}", site.address),
            access: match site.access {
                AccessDirection::Read => "read",
                AccessDirection::Write => "write",
            },
            address_form: match site.form {
                AccessForm::Direct => "direct",
                AccessForm::AbsoluteX => "absolute_x",
                AccessForm::AbsoluteY => "absolute_y",
                AccessForm::IndexedIndirectX => "indexed_indirect_x",
                AccessForm::IndirectIndexedY => "indirect_indexed_y",
            },
            operand_hex: format!("0x{:04X}", site.operand),
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn candidate_is_exactly_five_bytes_after_the_battle_reservation() {
        assert_eq!(CANDIDATE_END - CANDIDATE_START + 1, 5);
        assert_eq!(CANDIDATE_START, BATTLE_RUNTIME_STORAGE_END + 1);
    }

    #[test]
    fn indexed_overlap_is_conservative_over_the_full_index_domain() {
        assert!(access_trace::indexed_form_may_overlap(0x0781));
        assert!(access_trace::indexed_form_may_overlap(0x07F4));
        assert!(!access_trace::indexed_form_may_overlap(0x07F5));
        assert!(!access_trace::indexed_form_may_overlap(0x0600));
    }
}
