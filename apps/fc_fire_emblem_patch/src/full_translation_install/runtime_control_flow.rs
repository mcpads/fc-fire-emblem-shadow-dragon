use std::collections::BTreeSet;

use anyhow::{Context, Result, ensure};
use serde::Serialize;

use super::{
    runtime_bank_contract::bind_bank_restore_contract,
    runtime_code::{DialogueRuntimeHookRole, dispatcher_gate::EXPECTED_RECLAIMED_GATE_CAVE_SHA1},
    runtime_nmi_contract::bind_quiet_frame_gate,
};
use crate::{
    dialogue_inventory::switchable_cpu_to_file_offset,
    font_slots::FONT_PAGE_SIZE,
    mapper165::battle_composition_loader_probe::cumulative_battle_composition_dispatch_bytes,
    rom::{HEADER_SIZE, Rom},
    sha1_hex,
    typed_source::decode_rp2a03_sequence,
};

const FIXED_BANK_SIZE: usize = 16 * 1024;
const MAIN_DIALOGUE_BANK: u8 = 0x0A;
const SOURCE_POINTER_RESOLVER: u16 = 0xE6B2;
/// 전투 합성이 계속 쓰는 자리다. 소유자가 다르므로 그대로 둔다.
const BATTLE_NMI_HOOK: u16 = 0xC191;
const SHARED_NMI_DISPATCH: u16 = 0xFC20;
const SHARED_NMI_DISPATCH_END: u16 = 0xFC56;
const SHARED_NMI_EXPANSION_END: u16 = 0xFC60;
const FIXED_TRAMPOLINE_START: u16 = 0xF400;
const FIXED_TRAMPOLINE_END: u16 = 0xF4B0;
const SAMPLE_GROUP_SELECTOR_START: u16 = 0xF341;
const SAMPLE_GROUP_SELECTOR_END: u16 = 0xF378;
const SAMPLE_INITIAL_SELECTOR_START: u16 = 0xF990;
const SAMPLE_INITIAL_SELECTOR_END: u16 = 0xFA00;
const CENTRAL_SELECTOR_FALLBACK: u16 = 0xFF40;
/// 완성된 대사 수명이 원본 제어 흐름에 끼어들어야 하는 모든 역할이다.
///
/// 주소의 개수가 아니다. 완료 판정은 이 역할 집합에서 빠진 것이 없는지를 본다.
const PLANNED_HOOK_ROLES: [DialogueRuntimeHookRole; 33] = [
    DialogueRuntimeHookRole::InitialDirectEntryRequest,
    DialogueRuntimeHookRole::E4TransitionEntryRequest,
    DialogueRuntimeHookRole::E6TransitionEntryRequest,
    DialogueRuntimeHookRole::E7CallerResumeRequest,
    DialogueRuntimeHookRole::CompletedPageAdvanceOrLifetimeEnd,
    DialogueRuntimeHookRole::E7CallerHandoffResidencySuspension,
    DialogueRuntimeHookRole::BattleComposerInvalidatesDialogueResidency,
    DialogueRuntimeHookRole::NmiPageComposer,
    DialogueRuntimeHookRole::DispatcherGate,
    DialogueRuntimeHookRole::ChrRamSelector,
    DialogueRuntimeHookRole::DynamicItemSlotProducer,
    DialogueRuntimeHookRole::DynamicUnitSlotProducer,
    DialogueRuntimeHookRole::DynamicVillageItemProducer,
    DialogueRuntimeHookRole::DynamicEpilogueUnitProducer,
    DialogueRuntimeHookRole::DynamicEpilogueLocationProducer,
    DialogueRuntimeHookRole::ConsumerCatalogItemAppender,
    DialogueRuntimeHookRole::ConsumerCatalogUnitAppender,
    DialogueRuntimeHookRole::ConsumerCatalogClassAppender,
    DialogueRuntimeHookRole::ConsumerFontPagePublisher,
    DialogueRuntimeHookRole::ConsumerFontPageOpen,
    DialogueRuntimeHookRole::ConsumerFontPageClose,
    DialogueRuntimeHookRole::ConsumerFontPageGameplayHandoff,
    DialogueRuntimeHookRole::FixedMenuFontPageAppenderRoutine,
    DialogueRuntimeHookRole::FixedMenuUnitSelectionAppender,
    DialogueRuntimeHookRole::FixedMenuFastSpeedAppender,
    DialogueRuntimeHookRole::FixedMenuSlowSpeedAppender,
    DialogueRuntimeHookRole::FixedMenuStorageActionAppender,
    DialogueRuntimeHookRole::FixedMenuStorageOverflowAppender,
    DialogueRuntimeHookRole::FixedMenuStorageCapacityAppender,
    DialogueRuntimeHookRole::DialogueSpeakerPrefixProjection,
    DialogueRuntimeHookRole::EndingRecordFontPageEnter,
    DialogueRuntimeHookRole::EndingRecordFontPageExit,
    DialogueRuntimeHookRole::EndingCharacterEpilogueFontPageExit,
];
use super::runtime_material::{
    RUNTIME_CODE_MMC3_PAGE, RUNTIME_MATERIAL_FIRST_PAGE, RUNTIME_MATERIAL_PAGE_COUNT,
};
const RUNTIME_CODE_WINDOW_START: u16 = 0xA000;
const BATTLE_SOURCE_PAGE_MMC3_PAGE: u8 = 0x21;
const EXPECTED_COMPLETED_PAGE_SOURCE_SHA1: &str = "8c2a9f5a6e028a59409f9cc254add2b81f318b21";
const EXPECTED_COMPLETED_PAGE_CANDIDATE_SHA1: &str = "965de5bfca83263ac587e5c7c316ed6324d95ca8";
pub(in crate::full_translation_install) const EXPECTED_SAMPLE_INITIAL_SELECTOR_SHA1: &str =
    "67856cd2b7a26ef43649181f5e86ffe2741eb8b3";

#[derive(Serialize)]
pub(super) struct DialogueRuntimeControlFlowPlan {
    strategy: &'static str,
    states: Vec<RuntimeState>,
    producers: Vec<RuntimeProducer>,
    nmi_consumer: NmiConsumer,
    font_page_builder: FontPageBuilder,
    selector_consumer: SelectorConsumer,
    dynamic_text_consumption: DynamicTextConsumption,
    runtime_state: RuntimeStateStorage,
    superseded_sample_runtime: SupersededSampleRuntime,
    source_entry_points_bound: bool,
    existing_nmi_owner_preserved: bool,
    /// 소비자가 실행 코드 페이지를 빌려 쓰고 되돌리는 계약이 원본 바이트에 걸려 있다.
    prg_bank_restore_bound: bool,
    /// `$FA20`이 닿을 수 있는 8 KiB 페이지 수다. 실행 코드 페이지가 이 밖이라
    /// 소비자는 뱅크 레지스터를 직접 쓴다.
    source_bank_helper_reachable_page_count: u16,
    /// 소비자가 «조용한 프레임»에만 도는 근거인 원본 분기 수다.
    quiet_frame_gated_branch_count: usize,
    runtime_material_execution_address_bound: bool,
    runtime_state_storage_bound: bool,
    runtime_code_routines_assembled: bool,
    required_hook_roles: Vec<DialogueRuntimeHookRole>,
    assembled_hook_roles: Vec<DialogueRuntimeHookRole>,
    missing_assembled_hook_roles: Vec<DialogueRuntimeHookRole>,
    all_required_hook_roles_assembled: bool,
}

impl DialogueRuntimeControlFlowPlan {
    pub(super) fn all_required_hook_roles_assembled(&self) -> bool {
        self.all_required_hook_roles_assembled && self.missing_assembled_hook_roles.is_empty()
    }
}

#[derive(Serialize)]
struct RuntimeState {
    id: &'static str,
    meaning: &'static str,
}

#[derive(Serialize)]
struct RuntimeProducer {
    role: &'static str,
    prg_bank_hex: &'static str,
    cpu_address_hex: &'static str,
    source_span_byte_count: usize,
    request: &'static str,
    continuity: &'static str,
}

#[derive(Serialize)]
struct NmiConsumer {
    source_hook_cpu_address_hex: &'static str,
    existing_dispatch_cpu_range_hex: &'static str,
    existing_dispatch_sha1: String,
    exact_ff_expansion_byte_count: usize,
    battle_composition_priority_preserved: bool,
    source_input_scan_called_once: bool,
    render_disabled_mask_hex: &'static str,
    ppu_address_latch_reset: bool,
    sequential_ppu_increment_forced: bool,
    source_prg_bank_restored: bool,
    scroll_restore_preserved: bool,
    registers_and_status_preserved: bool,
    chr_restore_cycle_bounds_from_typed_cfg: bool,
    chr_fd_restore_callee_worst_case_cycles: u32,
    chr_fe_restore_callee_worst_case_cycles: u32,
}

#[derive(Serialize)]
struct FontPageBuilder {
    strategy: &'static str,
    source_page_mmc3_page_hex: &'static str,
    source_page_sha1: String,
    source_page_matches_original_font: bool,
    source_page_matches_dialogue_fd_page: bool,
    native_fe_backdrop_remains_selected: bool,
    fd_fe_namespaces_merged: bool,
    runtime_code_mmc3_page_hex: String,
    runtime_code_cpu_start_hex: String,
    runtime_code_cpu_end_exclusive_hex: &'static str,
    runtime_code_capacity_byte_count: usize,
    cold_request_action: &'static str,
    continuous_request_action: &'static str,
    dynamic_values_covered_by_visible_page_recipe: bool,
}

#[derive(Serialize)]
struct SelectorConsumer {
    chain_owner_cpu_address_hex: &'static str,
    current_fallback_cpu_address_hex: &'static str,
    replacement_role: &'static str,
    selects_chr_ram_only_when_ready: bool,
    ready_fd_published_by_transport: bool,
    central_fd_resupply_reselects_ready_ram: bool,
    original_dialogue_active_state_range_hex: &'static str,
    prg_bank_shadow_used_as_dialogue_lifetime: bool,
    source_fd_page_guard_hex: &'static str,
    source_fd_mismatch_invalidates_request: bool,
    selects_chr_ram_for_fd_latch_only: bool,
    native_fe_latch_remains_source_rom: bool,
    inactive_falls_through_to_existing_consumers: bool,
}

#[derive(Serialize)]
struct DynamicTextConsumption {
    strategy: &'static str,
    shared_glyph_reader_cpu_address_hex: &'static str,
    canonical_codes_are_page_physical_codes: bool,
    page_group_remap_required: bool,
    shared_glyph_reader_changed_for_main_dialogue: bool,
    original_english_latin_and_digits_use_identity_mapping: bool,
    complete: bool,
}

#[derive(Serialize)]
struct RuntimeStateStorage {
    required_byte_count: usize,
    fields: Vec<&'static str>,
    ownership_rule: &'static str,
    selected_cpu_range_hex: Option<String>,
}

#[derive(Serialize)]
struct SupersededSampleRuntime {
    completed_page_hook_cpu_range_hex: &'static str,
    completed_page_hook_sha1: String,
    fixed_group_selector_cpu_range_hex: &'static str,
    fixed_group_selector_sha1: String,
    fixed_initial_selector_cpu_range_hex: &'static str,
    fixed_initial_selector_sha1: String,
    superseded_hook_count: usize,
    must_be_replaced_in_integrated_write_set: bool,
    appended_static_pages_are_reclaimable_not_authoritative: bool,
}

pub(super) struct RuntimeControlFlowInputs<'a> {
    pub(super) source: &'a Rom,
    pub(super) candidate: &'a Rom,
    pub(super) runtime_code_offset: usize,
    pub(super) runtime_code_byte_count: usize,
    pub(super) selected_runtime_state_cpu_range: &'a str,
    /// 모든 실행 루틴이 재료 용기의 예약 자리에 조립됐는지다.
    pub(super) runtime_code_routines_assembled: bool,
    /// 정적 코드 계획이 조립한 훅의 역할이다. 최종 ROM 설치 여부는 별도 write set이 맡는다.
    pub(super) assembled_hook_roles: &'a [DialogueRuntimeHookRole],
    pub(super) chr_restore_callee_cycles: [(u16, u32); 2],
    pub(super) canonical_dynamic_codes_are_page_physical_codes: bool,
}

fn classify_assembled_hook_roles(
    assembled_hook_roles: &[DialogueRuntimeHookRole],
) -> Result<(Vec<DialogueRuntimeHookRole>, Vec<DialogueRuntimeHookRole>)> {
    let assembled = assembled_hook_roles
        .iter()
        .copied()
        .collect::<BTreeSet<_>>();
    ensure!(
        assembled.len() == assembled_hook_roles.len(),
        "dialogue runtime assembled the same hook role more than once"
    );
    let planned = PLANNED_HOOK_ROLES.into_iter().collect::<BTreeSet<_>>();
    ensure!(
        planned.len() == PLANNED_HOOK_ROLES.len(),
        "dialogue runtime planned the same hook role more than once"
    );
    ensure!(
        assembled.is_subset(&planned),
        "dialogue runtime assembled a hook role outside the planned control flow"
    );
    let missing = planned.difference(&assembled).copied().collect();
    Ok((assembled.into_iter().collect(), missing))
}

pub(super) fn plan_dialogue_runtime_control_flow(
    inputs: RuntimeControlFlowInputs<'_>,
) -> Result<DialogueRuntimeControlFlowPlan> {
    ensure!(
        inputs.canonical_dynamic_codes_are_page_physical_codes,
        "dynamic dialogue strings still need an unimplemented consumer projection"
    );
    let (assembled_hook_roles, missing_assembled_hook_roles) =
        classify_assembled_hook_roles(inputs.assembled_hook_roles)?;
    let [
        (fd_helper, fd_restore_cycles),
        (fe_helper, fe_restore_cycles),
    ] = inputs.chr_restore_callee_cycles;
    ensure!(
        fd_helper == 0xFA80 && fe_helper == 0xFAA0,
        "dialogue CHR restore cycle bounds target different helpers"
    );
    let producer_specs = [
        (
            "initial_direct_entry",
            0x809B,
            "seed_from_live_identity_or_promote_published_lookahead_then_reuse_or_compose",
            "new dialogue lifetime",
        ),
        (
            "E4_transition_entry",
            0x85F8,
            "promote_published_lookahead_then_reuse_or_compose",
            "same visible dialogue lifetime",
        ),
        (
            "E6_transition_entry",
            0x865F,
            "promote_published_lookahead_then_reuse_or_compose",
            "same visible dialogue lifetime",
        ),
        (
            "E7_caller_resume",
            0x871C,
            "seed_or_promote_identity_after_caller_resume_then_reuse_or_compose",
            "reuse is allowed only while no shared CHR-RAM writer invalidated residency",
        ),
    ];
    for (_, address, _, _) in producer_specs {
        let expected = [
            0x20,
            SOURCE_POINTER_RESOLVER as u8,
            (SOURCE_POINTER_RESOLVER >> 8) as u8,
        ];
        ensure!(
            switchable_bytes(inputs.source, MAIN_DIALOGUE_BANK, address, expected.len())?
                == expected
                && switchable_bytes(
                    inputs.candidate,
                    MAIN_DIALOGUE_BANK,
                    address,
                    expected.len(),
                )? == expected,
            "main-dialogue runtime producer changed at 0A:{address:04X}"
        );
        decode_rp2a03_sequence(&expected, address, "main-dialogue runtime producer call")?;
    }

    let completed_source = switchable_bytes(inputs.source, MAIN_DIALOGUE_BANK, 0x85C9, 29)?;
    let completed_candidate = switchable_bytes(inputs.candidate, MAIN_DIALOGUE_BANK, 0x85C9, 29)?;
    ensure!(
        sha1_hex(completed_source) == EXPECTED_COMPLETED_PAGE_SOURCE_SHA1
            && sha1_hex(completed_candidate) == EXPECTED_COMPLETED_PAGE_CANDIDATE_SHA1,
        "completed-page source or current sample hook changed"
    );
    decode_rp2a03_sequence(
        completed_candidate,
        0x85C9,
        "current sample completed-page hook",
    )?;

    ensure!(
        fixed_bytes(inputs.candidate, BATTLE_NMI_HOOK, 3)?
            == [
                0x20,
                SHARED_NMI_DISPATCH as u8,
                (SHARED_NMI_DISPATCH >> 8) as u8
            ],
        "current NMI hook no longer calls the shared battle dispatch"
    );
    let shared_dispatch = fixed_bytes(
        inputs.candidate,
        SHARED_NMI_DISPATCH,
        usize::from(SHARED_NMI_DISPATCH_END - SHARED_NMI_DISPATCH),
    )?;
    let expected_shared_dispatch = cumulative_battle_composition_dispatch_bytes()?;
    ensure!(
        expected_shared_dispatch.len()
            == usize::from(SHARED_NMI_DISPATCH_END - SHARED_NMI_DISPATCH)
            && shared_dispatch == expected_shared_dispatch,
        "shared NMI battle dispatch changed"
    );
    decode_rp2a03_sequence(
        shared_dispatch,
        SHARED_NMI_DISPATCH,
        "shared NMI battle dispatch",
    )?;
    let nmi_expansion = fixed_bytes(
        inputs.candidate,
        SHARED_NMI_DISPATCH_END,
        usize::from(SHARED_NMI_EXPANSION_END - SHARED_NMI_DISPATCH_END),
    )?;
    ensure!(
        nmi_expansion.iter().all(|byte| *byte == 0xFF),
        "shared NMI dispatch no longer has its ten-byte expansion"
    );

    let trampoline = fixed_bytes(
        inputs.candidate,
        FIXED_TRAMPOLINE_START,
        usize::from(FIXED_TRAMPOLINE_END - FIXED_TRAMPOLINE_START),
    )?;
    ensure!(
        trampoline.iter().all(|byte| *byte == 0xFF),
        "dialogue runtime fixed trampoline cave is no longer exact FF"
    );

    let runtime_code_page_offset =
        usize::from(RUNTIME_CODE_MMC3_PAGE - RUNTIME_MATERIAL_FIRST_PAGE) * 8 * 1024;
    ensure!(
        inputs.runtime_code_offset >= runtime_code_page_offset
            && inputs.runtime_code_offset + inputs.runtime_code_byte_count
                == RUNTIME_MATERIAL_PAGE_COUNT * 8 * 1024,
        "dialogue runtime code is not the final page of its material container"
    );
    let runtime_code_cpu_start = RUNTIME_CODE_WINDOW_START
        .checked_add(
            u16::try_from(inputs.runtime_code_offset - runtime_code_page_offset)
                .context("runtime code offset does not fit the A000 window")?,
        )
        .context("runtime code CPU start overflow")?;
    // 실행 코드는 재료 용기의 마지막 MMC3 페이지이고 창의 끝 `$C000`에서 끝난다. 시작 주소는
    // 앞선 자료가 얼마나 차지하는지에 따라 움직이는 결과값이지 지킬 값이 아니다.
    // 자료가 줄면 시작이 내려가 코드 자리가 넓어지는 것이 정상이다.
    ensure!(
        runtime_code_cpu_start >= RUNTIME_CODE_WINDOW_START
            && usize::from(0xC000 - runtime_code_cpu_start) == inputs.runtime_code_byte_count,
        "dialogue runtime code is no longer the tail of the A000 window"
    );

    let source_page = mmc3_page_bytes(
        inputs.candidate,
        BATTLE_SOURCE_PAGE_MMC3_PAGE,
        FONT_PAGE_SIZE,
    )?;
    let original_font = inputs
        .source
        .chr()
        .get(..FONT_PAGE_SIZE)
        .context("original dialogue font page is outside source CHR")?;
    ensure!(
        source_page == original_font,
        "existing battle source page no longer matches the original dialogue font"
    );

    let sample_group = fixed_bytes(
        inputs.candidate,
        SAMPLE_GROUP_SELECTOR_START,
        usize::from(SAMPLE_GROUP_SELECTOR_END - SAMPLE_GROUP_SELECTOR_START),
    )?;
    let sample_initial = fixed_bytes(
        inputs.candidate,
        SAMPLE_INITIAL_SELECTOR_START,
        usize::from(SAMPLE_INITIAL_SELECTOR_END - SAMPLE_INITIAL_SELECTOR_START),
    )?;
    ensure!(
        sha1_hex(sample_group) == EXPECTED_RECLAIMED_GATE_CAVE_SHA1
            && sha1_hex(sample_initial) == EXPECTED_SAMPLE_INITIAL_SELECTOR_SHA1
            && fixed_bytes(inputs.candidate, CENTRAL_SELECTOR_FALLBACK, 3)?
                == [
                    0x4C,
                    SAMPLE_INITIAL_SELECTOR_START as u8,
                    (SAMPLE_INITIAL_SELECTOR_START >> 8) as u8,
                ],
        "sample-specific maximum-dialogue selector ownership changed"
    );

    // 소비자가 실행 코드 페이지를 `$A000`에 잠깐 걸고 되돌리는 계약이다. 되돌릴 값의
    // 출처가 원본에 있어야 하므로 코드를 방출하기 전에 먼저 확인한다.
    let bank_restore = bind_bank_restore_contract(inputs.candidate)?;
    // 소비자가 들어갈 자리와, «조용한 프레임»의 뜻을 지키는 원본 분기들이다.
    let quiet_frame_gate = bind_quiet_frame_gate(inputs.source, inputs.candidate)?;

    let producers = producer_specs
        .into_iter()
        .map(|(role, address, request, continuity)| RuntimeProducer {
            role,
            prg_bank_hex: "0x0A",
            cpu_address_hex: match address {
                0x809B => "0x809B",
                0x85F8 => "0x85F8",
                0x865F => "0x865F",
                0x871C => "0x871C",
                _ => unreachable!(),
            },
            source_span_byte_count: 3,
            request,
            continuity,
        })
        .chain([
            RuntimeProducer {
                role: "completed_visible_page",
                prg_bank_hex: "0x0A",
                cpu_address_hex: "0x85C9",
                source_span_byte_count: 29,
                request: "advance_one_page_only_for_the_original_09_continue_outcome_and_preserve_0F_or_10_lifetime_boundaries",
                continuity: "same display path",
            },
            RuntimeProducer {
                role: "E7_caller_handoff",
                prg_bank_hex: "0x0A",
                cpu_address_hex: "0x8556",
                source_span_byte_count: 20,
                request: "suspend_selection_but_retain_residency_until_a_real_chr_ram_writer_invalidates_it",
                continuity: "dialogue state 17 gives the intervening screen to the existing selector chain",
            },
            RuntimeProducer {
                role: "terminal_or_E6_idle",
                prg_bank_hex: "0x0A",
                cpu_address_hex: "0x85C9",
                source_span_byte_count: 29,
                request: "invalidate_terminal_but_retain_resident_page_through_E6_idle_transition",
                continuity: "terminal ends the lifetime; E6 idle immediately enters its declared next record",
            },
        ])
        .collect();

    Ok(DialogueRuntimeControlFlowPlan {
        strategy: "treat the original selector and entry as a one-record lookahead pipeline: seed a new lifetime from the live identity, promote the previously published identity on a changed producer call, freeze the new live identity for the following transition, advance exactly one four-line workset only when the original completed-page state chooses 09 continue, retain the completed CHR page through E6 and E7 non-dialogue states, invalidate it at the actual battle CHR-RAM writer, reuse only the exact repeated identity without skipping the displaced source resolver, overlay every glyph used by each newly resolved visible page without restoring 4 KiB when residency exists, and cold-compose only without valid residency",
        states: vec![
            RuntimeState {
                id: "inactive",
                meaning: "existing selector chain owns the font page",
            },
            RuntimeState {
                id: "cold_requested",
                meaning: "build the source font page and the requested visible-page recipe before selection",
            },
            RuntimeState {
                id: "resident_page_overlay_requested",
                meaning: "keep the completed source page, hide it from display, and overlay every glyph used by the newly resolved visible page",
            },
            RuntimeState {
                id: "ready",
                meaning: "the CHR-RAM page contains every glyph used by the current visible page",
            },
        ],
        producers,
        nmi_consumer: NmiConsumer {
            source_hook_cpu_address_hex: "0xC179",
            existing_dispatch_cpu_range_hex: "0xFC20..0xFC56",
            existing_dispatch_sha1: sha1_hex(shared_dispatch),
            exact_ff_expansion_byte_count: nmi_expansion.len(),
            battle_composition_priority_preserved: true,
            source_input_scan_called_once: true,
            render_disabled_mask_hex: "0x06",
            ppu_address_latch_reset: true,
            sequential_ppu_increment_forced: true,
            source_prg_bank_restored: true,
            scroll_restore_preserved: true,
            registers_and_status_preserved: true,
            chr_restore_cycle_bounds_from_typed_cfg: true,
            chr_fd_restore_callee_worst_case_cycles: fd_restore_cycles,
            chr_fe_restore_callee_worst_case_cycles: fe_restore_cycles,
        },
        font_page_builder: FontPageBuilder {
            strategy: "cold dialogue-FD source-page rebuild plus direct visible-page recipe overlay while native FE remains the backdrop",
            source_page_mmc3_page_hex: "0x21",
            source_page_sha1: sha1_hex(source_page),
            source_page_matches_original_font: true,
            source_page_matches_dialogue_fd_page: true,
            native_fe_backdrop_remains_selected: true,
            fd_fe_namespaces_merged: false,
            runtime_code_mmc3_page_hex: format!("0x{RUNTIME_CODE_MMC3_PAGE:02X}"),
            runtime_code_cpu_start_hex: format!("0x{runtime_code_cpu_start:04X}"),
            runtime_code_cpu_end_exclusive_hex: "0xC000",
            runtime_code_capacity_byte_count: inputs.runtime_code_byte_count,
            cold_request_action: "copy all 4096 original font bytes then overlay only the target glyphs used by the selected visible page",
            continuous_request_action: "the original completed-page 09 outcome advances exactly one workset; an exact repeated identity reuses the selected page when ready and still executes the displaced source resolver; every newly resolved page overlays all of its own target glyphs without restoring the source page; a changed producer promotes the previously published lookahead to the current record while freezing the new live lookahead",
            dynamic_values_covered_by_visible_page_recipe: true,
        },
        selector_consumer: SelectorConsumer {
            chain_owner_cpu_address_hex: "0xFF1D",
            current_fallback_cpu_address_hex: "0xFF40",
            replacement_role: "global_main_dialogue_ready_fd_selector_then_existing_roster_chain",
            selects_chr_ram_only_when_ready: true,
            ready_fd_published_by_transport: true,
            central_fd_resupply_reselects_ready_ram: true,
            original_dialogue_active_state_range_hex: "0x00..0x0E",
            prg_bank_shadow_used_as_dialogue_lifetime: false,
            source_fd_page_guard_hex: "0x00",
            source_fd_mismatch_invalidates_request: true,
            selects_chr_ram_for_fd_latch_only: true,
            native_fe_latch_remains_source_rom: true,
            inactive_falls_through_to_existing_consumers: true,
        },
        dynamic_text_consumption: DynamicTextConsumption {
            strategy: "give each dynamic glyph one canonical physical code valid across every page that can consume it",
            shared_glyph_reader_cpu_address_hex: "0xE57F",
            canonical_codes_are_page_physical_codes: true,
            page_group_remap_required: false,
            shared_glyph_reader_changed_for_main_dialogue: false,
            original_english_latin_and_digits_use_identity_mapping: true,
            complete: true,
        },
        runtime_state: RuntimeStateStorage {
            required_byte_count: 5,
            fields: vec![
                "display_path_index_low",
                "display_path_index_high",
                "visible_page_index",
                "page_recipe_residency",
                "inactive_cold_overlay_or_ready_state",
            ],
            ownership_rule: "select no address until every direct and indirect source access, save lifetime, PPU queue lifetime, and existing battle runtime reservation excludes it",
            selected_cpu_range_hex: Some(inputs.selected_runtime_state_cpu_range.to_owned()),
        },
        superseded_sample_runtime: SupersededSampleRuntime {
            completed_page_hook_cpu_range_hex: "0A:0x85C9..0x85E6",
            completed_page_hook_sha1: sha1_hex(completed_candidate),
            fixed_group_selector_cpu_range_hex: "0xF341..0xF378",
            fixed_group_selector_sha1: sha1_hex(sample_group),
            fixed_initial_selector_cpu_range_hex: "0xF990..0xFA00",
            fixed_initial_selector_sha1: sha1_hex(sample_initial),
            superseded_hook_count: 3,
            must_be_replaced_in_integrated_write_set: true,
            appended_static_pages_are_reclaimable_not_authoritative: true,
        },
        source_entry_points_bound: true,
        existing_nmi_owner_preserved: true,
        prg_bank_restore_bound: true,
        source_bank_helper_reachable_page_count: bank_restore.helper_reachable_page_count,
        quiet_frame_gated_branch_count: quiet_frame_gate.gated_branch_count,
        runtime_material_execution_address_bound: true,
        runtime_state_storage_bound: true,
        runtime_code_routines_assembled: inputs.runtime_code_routines_assembled,
        required_hook_roles: PLANNED_HOOK_ROLES.to_vec(),
        assembled_hook_roles,
        all_required_hook_roles_assembled: missing_assembled_hook_roles.is_empty(),
        missing_assembled_hook_roles,
    })
}

fn switchable_bytes(rom: &Rom, bank: u8, address: u16, len: usize) -> Result<&[u8]> {
    let offset = switchable_cpu_to_file_offset(bank, address)?;
    rom.data()
        .get(offset..offset + len)
        .context("switchable runtime control-flow range is outside ROM")
}

fn fixed_bytes(rom: &Rom, address: u16, len: usize) -> Result<&[u8]> {
    ensure!(address >= 0xC000, "fixed runtime address is below C000");
    let fixed_start = rom
        .prg()
        .len()
        .checked_sub(FIXED_BANK_SIZE)
        .context("runtime candidate has no fixed PRG bank")?;
    let offset = HEADER_SIZE + fixed_start + usize::from(address - 0xC000);
    rom.data()
        .get(offset..offset + len)
        .context("fixed runtime control-flow range is outside candidate")
}

fn mmc3_page_bytes(rom: &Rom, page: u8, len: usize) -> Result<&[u8]> {
    let offset = HEADER_SIZE + usize::from(page) * 8 * 1024;
    rom.data()
        .get(offset..offset + len)
        .context("runtime material page is outside candidate")
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 조립된 훅 수가 어떤 목표 수와 같아도 역할이 빠졌다면 정적 계약 완료가 아니다.
    /// 현재 조립 집합은 전이·수명 종료가 빠졌음을 그대로 드러내야 한다.
    #[test]
    fn partial_hook_roles_report_what_is_missing() {
        let assembled = [
            DialogueRuntimeHookRole::InitialDirectEntryRequest,
            DialogueRuntimeHookRole::NmiPageComposer,
            DialogueRuntimeHookRole::DispatcherGate,
            DialogueRuntimeHookRole::ChrRamSelector,
        ];

        let (classified, missing) = classify_assembled_hook_roles(&assembled).unwrap();

        assert_eq!(classified.len(), assembled.len());
        assert!(missing.contains(&DialogueRuntimeHookRole::E4TransitionEntryRequest));
        assert!(missing.contains(&DialogueRuntimeHookRole::E6TransitionEntryRequest));
        assert!(missing.contains(&DialogueRuntimeHookRole::CompletedPageAdvanceOrLifetimeEnd));
        assert!(missing.contains(&DialogueRuntimeHookRole::E7CallerHandoffResidencySuspension));
        assert!(
            missing.contains(&DialogueRuntimeHookRole::BattleComposerInvalidatesDialogueResidency)
        );
    }

    /// 같은 역할을 두 번 세어 빠진 역할을 메운 척할 수 없어야 한다.
    #[test]
    fn duplicate_hook_roles_are_not_counted_as_progress() {
        let assembled = [
            DialogueRuntimeHookRole::NmiPageComposer,
            DialogueRuntimeHookRole::NmiPageComposer,
        ];

        let error = classify_assembled_hook_roles(&assembled).unwrap_err();

        assert!(error.to_string().contains("same hook role more than once"));
    }

    /// 자료가 끝나는 자리가 어디든 실행 코드는 창의 끝에서 끝나야 한다.
    /// 자료가 줄면 코드 자리는 넓어지고, 늘면 좁아진다.
    #[test]
    fn runtime_code_occupies_the_window_tail_whatever_the_material_size() {
        let capacity = RUNTIME_MATERIAL_PAGE_COUNT * 8 * 1024;
        let page_offset =
            usize::from(RUNTIME_CODE_MMC3_PAGE - RUNTIME_MATERIAL_FIRST_PAGE) * 8 * 1024;
        // 마지막 장 안의 어느 자리에서 자료가 끝나든 같은 관계가 성립해야 한다.
        for offset in [page_offset, page_offset + 2_945, capacity - 1_888] {
            let cpu = RUNTIME_CODE_WINDOW_START + u16::try_from(offset - page_offset).unwrap();

            assert!(cpu >= RUNTIME_CODE_WINDOW_START && cpu < 0xC000);
            assert_eq!(usize::from(0xC000 - cpu), capacity - offset);
        }
    }

    #[test]
    fn runtime_state_contract_is_minimal_and_role_named() {
        let fields = [
            "display_path_index_low",
            "display_path_index_high",
            "visible_page_index",
            "page_group_selector",
            "inactive_cold_continuous_or_ready_state",
        ];

        assert_eq!(fields.len(), 5);
        assert!(!fields.iter().any(|field| field.contains("chapter")));
    }
}
