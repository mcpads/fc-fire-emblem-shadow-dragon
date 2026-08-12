use std::collections::BTreeSet;

use anyhow::{Context, Result, ensure};
use serde::Serialize;

use crate::{
    mapper165::battle_codebook_plan::{BATTLE_RUNTIME_STORAGE_END, BATTLE_RUNTIME_STORAGE_START},
    rom::{HEADER_SIZE, Rom},
    sha1_hex,
    typed_source::decode_rp2a03_sequence,
};

mod source_contract;

use source_contract::{ConcurrentComputedAccessContract, bind_concurrent_computed_accesses};

use super::{
    CANDIDATE_START,
    access_trace::{
        AccessDirection, RuntimeAccessTrace, trace_fixed_interrupt_accesses,
        trace_switchable_accesses,
    },
};

const PRG_BANK_SIZE: usize = 16 * 1024;
const FIXED_BANK: u8 = 0x0F;
const SOURCE_NMI_VECTOR_ADDRESS: u16 = 0xFFFA;
const SOURCE_NMI_ENTRY: u16 = 0xC163;
const SOURCE_AUDIO_DISPATCH: u16 = 0xC1FB;
const SOURCE_AUDIO_ENTRY: u16 = 0x8000;
const SOURCE_AUDIO_BANK: u8 = 0x0E;
const SOURCE_AUDIO_DISPATCH_CODE: [u8; 8] = [0xA9, 0x0E, 0x8D, 0x00, 0xA0, 0x20, 0x00, 0x80];

#[derive(Serialize)]
pub(super) struct ConcurrentRuntimeAccessContract {
    strategy: &'static str,
    source_nmi_vector_hex: String,
    source_nmi_entry_hex: String,
    source_nmi: ConcurrentTraceReport,
    source_nmi_switchable_boundary_count: usize,
    source_nmi_switchable_boundaries_hex: Vec<String>,
    source_audio_dispatch_cpu_range_hex: String,
    source_audio_dispatch_sha1: String,
    source_audio_bank_hex: String,
    source_audio_entry_hex: String,
    source_audio: ConcurrentTraceReport,
    computed_access_contract: ConcurrentComputedAccessContract,
    existing_mapper165_battle_reservation_hex: String,
    candidate_begins_after_battle_reservation: bool,
    every_concurrent_writer_excludes_candidate: bool,
}

impl ConcurrentRuntimeAccessContract {
    pub(super) fn every_concurrent_writer_excludes_candidate(&self) -> bool {
        self.every_concurrent_writer_excludes_candidate
    }
}

#[derive(Serialize)]
struct ConcurrentTraceReport {
    reachable_instruction_count: usize,
    reachable_instruction_catalog_sha1: String,
    direct_overlap_count: usize,
    indexed_potential_overlap_count: usize,
    indirect_read_site_count: usize,
    indirect_write_site_count: usize,
    direct_and_indexed_accesses_exclude_candidate: bool,
    indirect_accesses_are_read_only: bool,
}

pub(super) fn bind_concurrent_runtime_accesses(
    source: &Rom,
    main_dialogue_queue_bound_proven: bool,
) -> Result<ConcurrentRuntimeAccessContract> {
    let vector = source_bytes(source, FIXED_BANK, SOURCE_NMI_VECTOR_ADDRESS, 2)?;
    let vector = u16::from_le_bytes([vector[0], vector[1]]);
    ensure!(
        vector == SOURCE_NMI_ENTRY,
        "source NMI vector changed from the concurrency contract"
    );

    let source_nmi = trace_fixed_interrupt_accesses(source, &[SOURCE_NMI_ENTRY])?;
    let expected_boundaries = BTreeSet::from([SOURCE_AUDIO_ENTRY]);
    ensure!(
        source_nmi.switchable_boundaries == expected_boundaries,
        "source NMI switchable-bank boundary census changed"
    );

    let audio_dispatch = source_bytes(
        source,
        FIXED_BANK,
        SOURCE_AUDIO_DISPATCH,
        SOURCE_AUDIO_DISPATCH_CODE.len(),
    )?;
    ensure!(
        audio_dispatch == SOURCE_AUDIO_DISPATCH_CODE,
        "source NMI audio-bank dispatch changed"
    );
    decode_rp2a03_sequence(
        audio_dispatch,
        SOURCE_AUDIO_DISPATCH,
        "source NMI audio-bank dispatch",
    )?;

    let source_audio = trace_switchable_accesses(source, SOURCE_AUDIO_BANK, &[SOURCE_AUDIO_ENTRY])?;
    ensure!(
        source_audio.switchable_boundaries.is_empty(),
        "source audio trace escaped its selected switchable bank"
    );

    let source_nmi_report = report_trace(&source_nmi);
    let source_audio_report = report_trace(&source_audio);
    let computed_access_contract = bind_concurrent_computed_accesses(
        source,
        &source_nmi,
        &source_audio,
        main_dialogue_queue_bound_proven,
    )?;
    let candidate_begins_after_battle_reservation = CANDIDATE_START > BATTLE_RUNTIME_STORAGE_END;
    let every_concurrent_writer_excludes_candidate = source_nmi_report
        .direct_and_indexed_accesses_exclude_candidate
        && source_audio_report.direct_and_indexed_accesses_exclude_candidate
        && computed_access_contract.every_computed_access_excludes_candidate()
        && candidate_begins_after_battle_reservation;
    ensure!(
        every_concurrent_writer_excludes_candidate,
        "a source NMI, source audio, or mapper-165 battle writer can reach the runtime-state candidate"
    );

    Ok(ConcurrentRuntimeAccessContract {
        strategy: "trace the fixed NMI and its source-bound bank-0E audio callee, prove every computed read range by producer role, then compose the existing mapper-165 battle-state reservation",
        source_nmi_vector_hex: format!("0x{SOURCE_NMI_VECTOR_ADDRESS:04X}"),
        source_nmi_entry_hex: format!("0x{SOURCE_NMI_ENTRY:04X}"),
        source_nmi: source_nmi_report,
        source_nmi_switchable_boundary_count: source_nmi.switchable_boundaries.len(),
        source_nmi_switchable_boundaries_hex: source_nmi
            .switchable_boundaries
            .iter()
            .map(|address| format!("0x{address:04X}"))
            .collect(),
        source_audio_dispatch_cpu_range_hex: format!(
            "0x{SOURCE_AUDIO_DISPATCH:04X}..0x{:04X}",
            SOURCE_AUDIO_DISPATCH + SOURCE_AUDIO_DISPATCH_CODE.len() as u16
        ),
        source_audio_dispatch_sha1: sha1_hex(audio_dispatch),
        source_audio_bank_hex: format!("0x{SOURCE_AUDIO_BANK:02X}"),
        source_audio_entry_hex: format!("0x{SOURCE_AUDIO_ENTRY:04X}"),
        source_audio: source_audio_report,
        computed_access_contract,
        existing_mapper165_battle_reservation_hex: format!(
            "0x{BATTLE_RUNTIME_STORAGE_START:04X}..0x{BATTLE_RUNTIME_STORAGE_END:04X}"
        ),
        candidate_begins_after_battle_reservation,
        every_concurrent_writer_excludes_candidate,
    })
}

fn report_trace(trace: &RuntimeAccessTrace) -> ConcurrentTraceReport {
    let catalog = trace
        .visited
        .iter()
        .flat_map(|(bank, address)| [*bank].into_iter().chain(address.to_le_bytes()))
        .collect::<Vec<_>>();
    let indirect_write_site_count = trace
        .indirect_sites
        .iter()
        .filter(|site| site.access == AccessDirection::Write)
        .count();
    ConcurrentTraceReport {
        reachable_instruction_count: trace.visited.len(),
        reachable_instruction_catalog_sha1: sha1_hex(&catalog),
        direct_overlap_count: trace.direct_overlaps.len(),
        indexed_potential_overlap_count: trace.indexed_potential_overlaps.len(),
        indirect_read_site_count: trace.indirect_sites.len() - indirect_write_site_count,
        indirect_write_site_count,
        direct_and_indexed_accesses_exclude_candidate: trace.direct_overlaps.is_empty()
            && trace.indexed_potential_overlaps.is_empty(),
        indirect_accesses_are_read_only: indirect_write_site_count == 0,
    }
}

fn source_bytes(source: &Rom, bank: u8, address: u16, byte_count: usize) -> Result<&[u8]> {
    let relative = if address >= 0xC000 {
        ensure!(
            bank == FIXED_BANK,
            "fixed concurrent source region uses a non-fixed bank"
        );
        usize::from(address - 0xC000)
    } else {
        ensure!(
            address >= 0x8000,
            "switchable concurrent source region is below 0x8000"
        );
        usize::from(address - 0x8000)
    };
    let offset = HEADER_SIZE + usize::from(bank) * PRG_BANK_SIZE + relative;
    source
        .data()
        .get(offset..offset + byte_count)
        .context("concurrent runtime source region is outside the source ROM")
}
