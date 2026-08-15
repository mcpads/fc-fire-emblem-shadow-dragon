use std::collections::BTreeSet;

use anyhow::Result;
use serde::Serialize;

use crate::{
    dialogue_inventory::main_dialogue_runtime_handler_roots,
    mapper165::battle_codebook_plan::BATTLE_RUNTIME_STORAGE_END, rom::Rom, sha1_hex,
};

mod access_trace;
mod concurrent_access;
mod source_contract;

use access_trace::{AccessDirection, AccessForm, AccessSite, trace_main_dialogue_accesses};
use concurrent_access::{ConcurrentRuntimeAccessContract, bind_concurrent_runtime_accesses};
use source_contract::{RuntimeStateSourceAccessContract, bind_runtime_state_source_accesses};

pub(super) const CANDIDATE_START: u16 = 0x07F0;
/// 다섯 바이트는 생산자와 소비자의 공유 계약이고, 뒤의 여섯 바이트는 소비자만
/// 쓰는 전송 커서다. 마지막 두 바이트는 생산자 시점의 원문 선행 조회값을 전송
/// 완료까지 붙잡는다. 마지막 한 바이트는 비대사 복합 UI가 자기 수명 동안 게시하는
/// 현재 CHR 페이지다. 서로 동시에 활성화되지 않으며 같은
/// 원본·NMI·전투 접근 배제 증명을 쓰므로 한 연속 범위로 묶는다. 증명을 둘로 나누면
/// 약한 쪽이 생긴다.
pub(super) const CANDIDATE_END: u16 = 0x07FD;

/// 비대사 복합 UI가 현재 사용하는 CHR mapper register다. 0은 소유한 페이지 없음이다.
/// 합성기와 이름 appender가 게시·즉시 적용하고, 화면 열기가 재적용 뒤 소비하며,
/// 화면 닫기가 재합성 뒤 남은 값을 지운다.
pub(in crate::full_translation_install) const CONSUMER_FONT_PAGE: u16 = CANDIDATE_END;

/// 생산자와 소비자가 공유하는 런타임 정체성이다. 앞의 네 바이트가 모두 세워진
/// 뒤에만 `REQUEST_STATE`를 요청 또는 준비 상태로 올린다.
pub(super) const RECORD_INDEX_LOW: u16 = CANDIDATE_START;
pub(super) const RECORD_INDEX_HIGH: u16 = RECORD_INDEX_LOW + 1;
pub(super) const VISIBLE_PAGE_INDEX: u16 = RECORD_INDEX_HIGH + 1;
pub(super) const CURRENT_PAGE_GROUP: u16 = VISIBLE_PAGE_INDEX + 1;
pub(super) const REQUEST_STATE: u16 = CURRENT_PAGE_GROUP + 1;

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
    runtime_lifecycle_contract: RuntimeLifecycleContract,
    selected_cpu_range_hex: Option<&'static str>,
    source_reservation_selection_complete: bool,
}

#[derive(Serialize)]
struct RuntimeLifecycleContract {
    ownership_begin: &'static str,
    ownership_continue: &'static str,
    ownership_invalidate: &'static str,
    cold_entry_writes_all_five_bytes_before_any_selector_read: bool,
    inactive_selector_ignores_all_five_bytes: bool,
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
    // 전이 미러 뱅크는 이중 진입과 함께 폐기했으므로 동시 접근을 만들 미러가 없다.
    // 의사결정 59번을 따른다.
    let concurrent_access_contract =
        bind_concurrent_runtime_accesses(source, main_dialogue_queue_bound_proven)?;
    let battle_reservation_excludes_candidate = CANDIDATE_START > BATTLE_RUNTIME_STORAGE_END;
    let selection_complete = direct_accesses_exclude_candidate
        && source_lifetime_accesses_exclude_candidate
        && concurrent_access_contract.every_concurrent_writer_excludes_candidate()
        && main_dialogue_queue_bound_proven
        && battle_reservation_excludes_candidate;

    Ok(DialogueRuntimeStateStoragePlan {
        strategy: "own one fourteen-byte scratch range proven free of source, NMI, queue, save, and battle writers; the first thirteen bytes belong to the main-dialogue lifetime and the final byte carries the screen-scoped non-dialogue consumer font page",
        candidate_cpu_range_hex: "0x07F0..0x07FD",
        required_byte_count: usize::from(CANDIDATE_END - CANDIDATE_START + 1),
        ownership_lifetime: "main dialogue active plus an E7-suspended resident page, or from a non-dialogue composite publication through immediate selection, screen open, redraw, and screen close; open consumes its publication, close clears redraw state, and source/concurrent writers exclude the whole range",
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
        runtime_lifecycle_contract: RuntimeLifecycleContract {
            ownership_begin: "every direct entry and E7 resume resolves a request before publishing cold or ready state",
            ownership_continue: "E4, E6, visible-page transitions, and E7 suspension retain ownership while no shared CHR-RAM writer has invalidated it",
            ownership_invalidate: "battle CHR-RAM composition, every terminal path, reset, save/load boundary, and unclassified inactive writers invalidate ownership",
            cold_entry_writes_all_five_bytes_before_any_selector_read: true,
            inactive_selector_ignores_all_five_bytes: true,
        },
        selected_cpu_range_hex: selection_complete.then_some("0x07F0..0x07FD"),
        source_reservation_selection_complete: selection_complete,
    })
}

impl DialogueRuntimeStateStoragePlan {
    pub(super) fn selected_cpu_range_hex(&self) -> Option<&'static str> {
        self.selected_cpu_range_hex
    }

    pub(super) fn source_reservation_selection_complete(&self) -> bool {
        self.source_reservation_selection_complete
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

    /// 예약은 전투 예약 바로 뒤에서 시작해야 두 소유자가 겹치지 않는다.
    /// 길이는 공유 계약 다섯 바이트, 소비자 전용 커서 여섯 바이트, 요청 시점 원문
    /// 정체성 두 바이트, 상호 배타적인 카탈로그 페이지 한 바이트의 합이다.
    #[test]
    fn the_reservation_starts_after_the_battle_reservation_and_covers_both_owners() {
        const SHARED_CONTRACT_BYTES: u16 = 5;
        const TRANSPORT_CURSOR_BYTES: u16 = 6;
        const REQUEST_SOURCE_IDENTITY_BYTES: u16 = 2;
        const CONSUMER_FONT_PAGE_BYTES: u16 = 1;

        assert_eq!(CANDIDATE_START, BATTLE_RUNTIME_STORAGE_END + 1);
        assert_eq!(
            CANDIDATE_END - CANDIDATE_START + 1,
            SHARED_CONTRACT_BYTES
                + TRANSPORT_CURSOR_BYTES
                + REQUEST_SOURCE_IDENTITY_BYTES
                + CONSUMER_FONT_PAGE_BYTES
        );
    }

    #[test]
    fn shared_identity_fields_fill_the_first_five_bytes_in_order() {
        assert_eq!(RECORD_INDEX_LOW, CANDIDATE_START);
        assert_eq!(RECORD_INDEX_HIGH, RECORD_INDEX_LOW + 1);
        assert_eq!(VISIBLE_PAGE_INDEX, RECORD_INDEX_HIGH + 1);
        assert_eq!(CURRENT_PAGE_GROUP, VISIBLE_PAGE_INDEX + 1);
        assert_eq!(REQUEST_STATE, CURRENT_PAGE_GROUP + 1);
    }

    /// 색인 판정은 보수적이어야 한다. 기준 주소가 예약보다 아래여도 색인이
    /// 최대 255까지 더해지므로 «닿을 수 있음»으로 봐야 놓치지 않는다.
    #[test]
    fn indexed_overlap_is_conservative_over_the_full_index_domain() {
        assert!(access_trace::indexed_form_may_overlap(0x0781));
        assert!(access_trace::indexed_form_may_overlap(CANDIDATE_END));
        assert!(!access_trace::indexed_form_may_overlap(CANDIDATE_END + 1));
        assert!(!access_trace::indexed_form_may_overlap(
            CANDIDATE_START - u16::from(u8::MAX) - 1
        ));
    }
}
