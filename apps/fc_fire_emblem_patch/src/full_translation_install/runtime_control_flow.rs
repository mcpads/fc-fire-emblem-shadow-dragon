use anyhow::{Context, Result, ensure};
use serde::Serialize;

use crate::{
    dialogue_inventory::switchable_cpu_to_file_offset,
    font_slots::FONT_PAGE_SIZE,
    rom::{HEADER_SIZE, Rom},
    sha1_hex,
    typed_source::decode_rp2a03_sequence,
};

const FIXED_BANK_SIZE: usize = 16 * 1024;
const MAIN_DIALOGUE_BANK: u8 = 0x0A;
const SOURCE_POINTER_RESOLVER: u16 = 0xE6B2;
const NMI_HOOK: u16 = 0xC191;
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
const RUNTIME_CODE_MMC3_PAGE: u8 = 0x2E;
const RUNTIME_CODE_WINDOW_START: u16 = 0xA000;
const BATTLE_SOURCE_PAGE_MMC3_PAGE: u8 = 0x21;
const EXPECTED_COMPLETED_PAGE_SOURCE_SHA1: &str = "8c2a9f5a6e028a59409f9cc254add2b81f318b21";
const EXPECTED_COMPLETED_PAGE_CANDIDATE_SHA1: &str = "965de5bfca83263ac587e5c7c316ed6324d95ca8";
const EXPECTED_SHARED_NMI_DISPATCH_SHA1: &str = "9f0090bd11866f7a4786db24a30e6660588b7758";
const EXPECTED_SAMPLE_GROUP_SELECTOR_SHA1: &str = "cea25e67f4399e422e8747046c13a959f5669ac1";
const EXPECTED_SAMPLE_INITIAL_SELECTOR_SHA1: &str = "67856cd2b7a26ef43649181f5e86ffe2741eb8b3";

#[derive(Serialize)]
pub(super) struct DialogueRuntimeControlFlowPlan {
    strategy: &'static str,
    states: Vec<RuntimeState>,
    producers: Vec<RuntimeProducer>,
    nmi_consumer: NmiConsumer,
    font_page_builder: FontPageBuilder,
    selector_consumer: SelectorConsumer,
    dynamic_text_projection: DynamicTextProjection,
    runtime_state: RuntimeStateStorage,
    superseded_sample_runtime: SupersededSampleRuntime,
    source_entry_points_bound: bool,
    existing_nmi_owner_preserved: bool,
    runtime_material_execution_address_bound: bool,
    runtime_state_storage_bound: bool,
    runtime_code_emitted: bool,
    runtime_hooks_contributed: bool,
    complete: bool,
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
}

#[derive(Serialize)]
struct FontPageBuilder {
    strategy: &'static str,
    source_page_mmc3_page_hex: &'static str,
    source_page_sha1: String,
    source_page_matches_original_font: bool,
    runtime_code_mmc3_page_hex: &'static str,
    runtime_code_cpu_start_hex: String,
    runtime_code_cpu_end_exclusive_hex: &'static str,
    runtime_code_capacity_byte_count: usize,
    cold_request_action: &'static str,
    continuous_request_action: &'static str,
    dynamic_values_covered_by_page_group: bool,
}

#[derive(Serialize)]
struct SelectorConsumer {
    chain_owner_cpu_address_hex: &'static str,
    current_fallback_cpu_address_hex: &'static str,
    replacement_role: &'static str,
    selects_chr_ram_only_when_ready: bool,
    inactive_falls_through_to_existing_consumers: bool,
}

#[derive(Serialize)]
struct DynamicTextProjection {
    shared_glyph_reader_cpu_address_hex: &'static str,
    page_group_remap_required: bool,
    applies_only_to_ready_main_dialogue: bool,
    original_english_latin_and_digits_use_identity_mapping: bool,
    hook_bound: bool,
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
}

pub(super) fn plan_dialogue_runtime_control_flow(
    inputs: RuntimeControlFlowInputs<'_>,
) -> Result<DialogueRuntimeControlFlowPlan> {
    let producer_specs = [
        (
            "initial_direct_entry",
            0x809B,
            "cold_rebuild_page_zero",
            "new dialogue lifetime",
        ),
        (
            "E4_transition_entry",
            0x85F8,
            "continuous_page_zero",
            "same visible dialogue lifetime",
        ),
        (
            "E6_transition_entry",
            0x865F,
            "continuous_page_zero",
            "same visible dialogue lifetime",
        ),
        (
            "E7_caller_resume",
            0x871C,
            "cold_rebuild_page_zero",
            "external caller may have changed the font lifetime",
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
        fixed_bytes(inputs.candidate, NMI_HOOK, 3)?
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
    ensure!(
        sha1_hex(shared_dispatch) == EXPECTED_SHARED_NMI_DISPATCH_SHA1,
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

    let runtime_code_page_offset = usize::from(RUNTIME_CODE_MMC3_PAGE - 0x2C) * 8 * 1024;
    ensure!(
        inputs.runtime_code_offset >= runtime_code_page_offset
            && inputs.runtime_code_offset + inputs.runtime_code_byte_count == 3 * 8 * 1024,
        "dialogue runtime code is not the tail of MMC3 page 2E"
    );
    let runtime_code_cpu_start = RUNTIME_CODE_WINDOW_START
        .checked_add(
            u16::try_from(inputs.runtime_code_offset - runtime_code_page_offset)
                .context("runtime code offset does not fit the A000 window")?,
        )
        .context("runtime code CPU start overflow")?;
    // 실행 코드는 페이지 `2E`의 꼬리이고 창의 끝 `$C000`에서 끝난다. 시작 주소는
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
        sha1_hex(sample_group) == EXPECTED_SAMPLE_GROUP_SELECTOR_SHA1
            && sha1_hex(sample_initial) == EXPECTED_SAMPLE_INITIAL_SELECTOR_SHA1
            && fixed_bytes(inputs.candidate, CENTRAL_SELECTOR_FALLBACK, 3)?
                == [
                    0x4C,
                    SAMPLE_INITIAL_SELECTOR_START as u8,
                    (SAMPLE_INITIAL_SELECTOR_START >> 8) as u8,
                ],
        "sample-specific maximum-dialogue selector ownership changed"
    );

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
                request: "continuous_next_page_if_present_otherwise_leave_page_ready_until_boundary",
                continuity: "same display path",
            },
            RuntimeProducer {
                role: "E7_caller_handoff",
                prg_bank_hex: "0x0A",
                cpu_address_hex: "0x8556",
                source_span_byte_count: 20,
                request: "invalidate_before_external_caller",
                continuity: "external caller owns the intervening screen",
            },
            RuntimeProducer {
                role: "terminal_or_idle",
                prg_bank_hex: "0x0A",
                cpu_address_hex: "0x85C9",
                source_span_byte_count: 29,
                request: "invalidate_and_fall_through_to_original_terminal_state",
                continuity: "dialogue lifetime ends",
            },
        ])
        .collect();

    Ok(DialogueRuntimeControlFlowPlan {
        strategy: "derive every request from the original main-dialogue state machine, compose one complete page group in the existing render-disabled NMI owner, and select CHR RAM only after that exact request is ready",
        states: vec![
            RuntimeState {
                id: "inactive",
                meaning: "existing selector chain owns the font page",
            },
            RuntimeState {
                id: "cold_requested",
                meaning: "build the source font page and the requested group before selection",
            },
            RuntimeState {
                id: "continuous_requested",
                meaning: "apply the exact group transition from the currently ready dialogue page",
            },
            RuntimeState {
                id: "ready",
                meaning: "the CHR-RAM page matches the current path and page-group identity",
            },
        ],
        producers,
        nmi_consumer: NmiConsumer {
            source_hook_cpu_address_hex: "0xC191",
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
        },
        font_page_builder: FontPageBuilder {
            strategy: "cold source-page rebuild plus dense page-group atlas overlay; continuous changes may use the already measured exact group delta",
            source_page_mmc3_page_hex: "0x21",
            source_page_sha1: sha1_hex(source_page),
            source_page_matches_original_font: true,
            runtime_code_mmc3_page_hex: "0x2E",
            runtime_code_cpu_start_hex: format!("0x{runtime_code_cpu_start:04X}"),
            runtime_code_cpu_end_exclusive_hex: "0xC000",
            runtime_code_capacity_byte_count: inputs.runtime_code_byte_count,
            cold_request_action: "copy all 4096 original font bytes then overlay every assigned target glyph in the selected page group",
            continuous_request_action: "apply a verified delta only while the prior ready identity is still owned; otherwise use a cold rebuild",
            dynamic_values_covered_by_page_group: true,
        },
        selector_consumer: SelectorConsumer {
            chain_owner_cpu_address_hex: "0xFF1D",
            current_fallback_cpu_address_hex: "0xFF40",
            replacement_role: "global_main_dialogue_ready_selector_then_existing_roster_chain",
            selects_chr_ram_only_when_ready: true,
            inactive_falls_through_to_existing_consumers: true,
        },
        dynamic_text_projection: DynamicTextProjection {
            shared_glyph_reader_cpu_address_hex: "0xE57F",
            page_group_remap_required: true,
            applies_only_to_ready_main_dialogue: true,
            original_english_latin_and_digits_use_identity_mapping: true,
            hook_bound: false,
        },
        runtime_state: RuntimeStateStorage {
            required_byte_count: 5,
            fields: vec![
                "display_path_index_low",
                "display_path_index_high",
                "visible_page_index",
                "page_group_selector",
                "inactive_cold_continuous_or_ready_state",
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
        runtime_material_execution_address_bound: true,
        runtime_state_storage_bound: true,
        runtime_code_emitted: false,
        runtime_hooks_contributed: false,
        complete: false,
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

    /// 자료가 끝나는 자리가 어디든 실행 코드는 창의 끝에서 끝나야 한다.
    /// 자료가 줄면 코드 자리는 넓어지고, 늘면 좁아진다.
    #[test]
    fn runtime_code_occupies_the_window_tail_whatever_the_material_size() {
        let page_offset = usize::from(RUNTIME_CODE_MMC3_PAGE - 0x2C) * 8 * 1024;
        for offset in [22_688usize, 21_642, 16_384] {
            let cpu = RUNTIME_CODE_WINDOW_START + u16::try_from(offset - page_offset).unwrap();

            assert!(cpu >= RUNTIME_CODE_WINDOW_START && cpu < 0xC000);
            assert_eq!(usize::from(0xC000 - cpu), 3 * 8 * 1024 - offset);
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
