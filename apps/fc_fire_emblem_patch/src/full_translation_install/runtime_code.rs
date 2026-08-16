//! 대사 런타임이 ROM에 넣는 실행 코드다.
//!
//! 갈래를 나눈 기준은 «무엇이 바뀌면 이 파일이 바뀌는가»다. 전송 루프는 프레임
//! 예산이 바뀌면 바뀌고, 트램폴린은 원본 NMI 계약이 바뀌면 바뀐다.

use std::collections::BTreeSet;

use anyhow::{Context, Result, ensure};
use retro_rp2a03::{
    AddressingMode, Location, MemoryAddress, Mnemonic, Operand, Rp2A03, decode_bytes,
};
use serde::Serialize;
use typed_isa_core::{AccessKind, StaticSemantics};

use super::{
    consumer_catalog::ConsumerCatalogRuntimeLayout,
    runtime_bank_contract::bind_bank_restore_contract, runtime_nmi_contract::bind_quiet_frame_gate,
};
use crate::{
    mapper165::{
        FinalBattleConsumerRoute, FinalBattleConsumerRouteRegion, FinalConsumerRouteRegion,
        FinalRosterConsumerRoute,
        executable_mapper_writes::{Mapper165Register, decode_mapper165_write},
    },
    rom::Rom,
    rp2a03::{Instruction, assemble_at},
    typed_source::{Rp2a03DirectControlFlow, decode_rp2a03_sequence, rp2a03_direct_control_flow},
};

mod chr_ram_ownership;
pub(in crate::full_translation_install) mod chr_selector;
pub(in crate::full_translation_install) mod chr_source_state;
mod consumer_catalog;
pub(in crate::full_translation_install) mod consumer_font_page;
pub(in crate::full_translation_install) mod dispatcher_gate;
mod dynamic_producer;
mod fixed_cfg_cycles;
mod font_page_route;
pub(in crate::full_translation_install) mod lifecycle;
pub(in crate::full_translation_install) mod resolve_request;
mod resolved_page_publication;
pub(super) mod trampoline;
pub(in crate::full_translation_install) mod transport;

/// 대사 런타임이 원본 제어 흐름에 끼어드는 각 자리의 의미다.
///
/// 개수만 비교하면 selector나 관측 훅을 생산자 훅으로 잘못 셀 수 있다. 설치와 완료
/// 판정은 이 역할을 따라가며, 주소는 별도의 `DialogueRuntimeHookSite`가 맡는다.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Serialize)]
#[serde(rename_all = "snake_case")]
pub(in crate::full_translation_install) enum DialogueRuntimeHookRole {
    InitialDirectEntryRequest,
    E4TransitionEntryRequest,
    E6TransitionEntryRequest,
    E7CallerResumeRequest,
    CompletedPageAdvanceOrLifetimeEnd,
    E7CallerHandoffResidencySuspension,
    BattleComposerInvalidatesDialogueResidency,
    NmiPageComposer,
    DispatcherGate,
    ChrRamSelector,
    DynamicItemSlotProducer,
    DynamicUnitSlotProducer,
    DynamicVillageItemProducer,
    DynamicEpilogueUnitProducer,
    DynamicEpilogueLocationProducer,
    ConsumerCatalogItemAppender,
    ConsumerCatalogUnitAppender,
    ConsumerCatalogClassAppender,
    ConsumerFontPagePublisher,
    ConsumerFontPageOpen,
    ConsumerFontPageClose,
    ConsumerFontPageGameplayHandoff,
    FixedMenuFontPageAppenderRoutine,
    FixedMenuUnitSelectionAppender,
    FixedMenuFastSpeedAppender,
    FixedMenuSlowSpeedAppender,
    FixedMenuStorageActionAppender,
    FixedMenuStorageOverflowAppender,
    FixedMenuStorageCapacityAppender,
    EndingRecordFontPageEnter,
    EndingRecordFontPageExit,
    EndingCharacterEpilogueFontPageExit,
}

/// 훅이 가져가는 원본 자리다.
pub(in crate::full_translation_install) enum DialogueRuntimeHookSite {
    Fixed(u16),
    Switchable { bank: u8, address: u16 },
}

/// 역할, 원본 자리, 쓸 바이트를 함께 들고 다니는 설치 단위다.
pub(in crate::full_translation_install) struct DialogueRuntimeHook {
    pub(in crate::full_translation_install) role: DialogueRuntimeHookRole,
    pub(in crate::full_translation_install) write_role: &'static str,
    pub(in crate::full_translation_install) site: DialogueRuntimeHookSite,
    pub(in crate::full_translation_install) bytes: Vec<u8>,
}

/// 먼저 설치했던 표본 전용 실행 코드를 전역 런타임이 되찾아 쓰는 고정 뱅크 조각이다.
///
/// 일반 고정 동굴은 `FF`라는 선행 조건만 필요하지만, 여기는 이미 실행 코드가 있다.
/// 전체 원천 구간의 digest를 별도로 고정해야 그중 일부만 우연히 같아도 덮지 않는다.
pub(in crate::full_translation_install) struct ReclaimedFixedRuntimeRoutine {
    pub(in crate::full_translation_install) routine: RuntimeRoutine,
    /// Prefix emitted as executable instructions. The remaining bytes are exact cave padding and
    /// belong to the overwrite contract, not to the executable decoder denominator.
    pub(in crate::full_translation_install) executable_byte_count: usize,
    pub(in crate::full_translation_install) source_end_exclusive: u16,
    pub(in crate::full_translation_install) expected_source_sha1: &'static str,
}

/// 원본 `$C179` 진입 시점에 남아 있는 vblank다. 앞에 NMI 진입 오버헤드와 OAM DMA밖에
/// 없고 둘 다 고정 비용이라 이 값은 표본이 아니라 상수다. 에뮬레이터 실측으로
/// 확인했고 계산값 `2,273 − 566`과 3사이클 차이다. 의사결정 64번을 따른다.
const SOURCE_MEASURED_VBLANK_REMAINDER: u32 = 1_704;
/// mapper165가 `$C173`에서 selector를 스택에 더 저장하고 원래 `$00/$01` 저장으로
/// 돌아오는 고정 오버헤드다. OAM DMA 뒤에 생기므로 DMA parity에는 영향을 주지 않는다.
const SELECTOR_STACK_ENTRY_OVERHEAD: u32 = 12;
/// mapper165 후보가 같은 `$C179` 훅에 도달했을 때 실제로 남는 vblank다.
const MAPPER_VBLANK_REMAINDER: u32 =
    SOURCE_MEASURED_VBLANK_REMAINDER - SELECTOR_STACK_ENTRY_OVERHEAD;
/// 실기 여유다. 남은 vblank를 전부 쓰지 않는다.
const SAFETY_MARGIN_PERCENT: u32 = 20;
/// `$C179`의 `JSR`가 쓰는 몫이다.
const CONSUMER_HOOK_CALL_CYCLES: u32 = 6;
/// 전송 루틴이 한 프레임에 쓸 수 있는 사이클이다.
///
/// `trampoline_reserve`는 훅 호출과 트램폴린이 실제로 쓰는 최악 사이클이고 방출한
/// 명령에서 센 값이다. 임의의 여백을 따로 두지 않는다. 안전 여유는 위의 20% 하나뿐이고,
/// 여백을 두 겹으로 쌓으면 어느 쪽이 실제 근거인지 알 수 없게 된다.
fn budgeted_transport_cycles(trampoline_reserve: u32) -> u32 {
    MAPPER_VBLANK_REMAINDER * (100 - SAFETY_MARGIN_PERCENT) / 100 - trampoline_reserve
}

/// 대사 런타임이 ROM에 넣는 실행 코드와 훅 전체다.
pub(in crate::full_translation_install) struct DialogueRuntimeCodePlan {
    /// 실행 코드 페이지에 놓이는 조각들이다.
    pub(in crate::full_translation_install) code_routines: Vec<RuntimeRoutine>,
    /// 고정 뱅크 동굴에 놓이는 조각들이다.
    pub(in crate::full_translation_install) fixed_routines: Vec<RuntimeRoutine>,
    /// 정확한 기존 실행 코드 전체를 확인한 뒤 되찾아 쓰는 고정 뱅크 조각들이다.
    pub(in crate::full_translation_install) reclaimed_fixed_routines:
        Vec<ReclaimedFixedRuntimeRoutine>,
    /// 원본에 실제로 설치할 훅이다. 역할과 주소와 바이트가 한 단위라 따로 세지 않는다.
    pub(in crate::full_translation_install) hooks: Vec<DialogueRuntimeHook>,
    /// 후보 고정 코드의 typed CFG에서 계산한 FD/FE 복원 helper 상한이다.
    pub(in crate::full_translation_install) chr_restore_callee_cycles: [(u16, u32); 2],
}

impl DialogueRuntimeCodePlan {
    pub(in crate::full_translation_install) fn hook_roles(&self) -> Vec<DialogueRuntimeHookRole> {
        self.hooks.iter().map(|hook| hook.role).collect()
    }

    pub(in crate::full_translation_install) fn consumer_catalog_paths_planned(&self) -> bool {
        let roles = self.hook_roles().into_iter().collect::<BTreeSet<_>>();
        [
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
            DialogueRuntimeHookRole::EndingRecordFontPageEnter,
            DialogueRuntimeHookRole::EndingRecordFontPageExit,
            DialogueRuntimeHookRole::EndingCharacterEpilogueFontPageExit,
        ]
        .iter()
        .all(|role| roles.contains(role))
            && self
                .fixed_routines
                .iter()
                .any(|routine| routine.role == "consumer font page activation")
    }

    pub(in crate::full_translation_install) fn final_roster_consumer_route(
        &self,
    ) -> Result<FinalRosterConsumerRoute> {
        let hook = self
            .hooks
            .iter()
            .find(|hook| hook.role == DialogueRuntimeHookRole::ChrRamSelector)
            .context("dialogue CHR selector hook is missing")?;
        ensure!(
            matches!(hook.site, DialogueRuntimeHookSite::Fixed(address) if address == chr_selector::SELECTOR_CHAIN_SITE)
                && hook.bytes.len() == 3
                && hook.bytes[0] == 0x4C,
            "dialogue CHR selector hook no longer replaces the central fallback with JMP absolute"
        );
        let central_fallback_target = u16::from_le_bytes([hook.bytes[1], hook.bytes[2]]);
        let page_route = self
            .fixed_routines
            .iter()
            .find(|routine| routine.role == "translated font page route selector")
            .context("translated font page route selector is missing")?;
        let dialogue_selector = self
            .fixed_routines
            .iter()
            .find(|routine| routine.role == "dialogue CHR RAM selector")
            .context("dialogue CHR RAM selector is missing")?;
        let route_end = page_route
            .address
            .checked_add(u16::try_from(page_route.bytes.len())?)
            .context("translated font page route address overflow")?;
        ensure!(
            page_route.address <= central_fallback_target
                && central_fallback_target < route_end
                && route_end == dialogue_selector.address,
            "integrated central fallback no longer enters the contiguous font-page route chain"
        );
        let active_offset = usize::from(central_fallback_target - page_route.address);
        decode_rp2a03_sequence(
            &page_route.bytes[active_offset..],
            central_fallback_target,
            "integrated active font-page route",
        )?;
        decode_rp2a03_sequence(
            &dialogue_selector.bytes,
            dialogue_selector.address,
            "integrated dialogue CHR selector",
        )?;
        ensure!(
            dialogue_selector.bytes.ends_with(&[
                0x68,
                0x28,
                0x4C,
                chr_selector::SELECTOR_CHAIN_FALLBACK as u8,
                (chr_selector::SELECTOR_CHAIN_FALLBACK >> 8) as u8,
            ]),
            "integrated dialogue CHR selector no longer falls through to the roster selector"
        );
        Ok(FinalRosterConsumerRoute {
            central_fallback_target,
            regions: vec![
                FinalConsumerRouteRegion {
                    role: "integrated_font_page_route_selector",
                    cpu_address: page_route.address,
                    bytes: page_route.bytes.clone(),
                },
                FinalConsumerRouteRegion {
                    role: "integrated_dialogue_chr_selector",
                    cpu_address: dialogue_selector.address,
                    bytes: dialogue_selector.bytes.clone(),
                },
            ],
        })
    }

    pub(in crate::full_translation_install) fn final_battle_consumer_route(
        &self,
    ) -> Result<FinalBattleConsumerRoute> {
        let roster_route = self.final_roster_consumer_route()?;
        let hook = self
            .hooks
            .iter()
            .find(|hook| {
                hook.role == DialogueRuntimeHookRole::BattleComposerInvalidatesDialogueResidency
            })
            .context("battle composition ownership hook is missing")?;
        ensure!(
            matches!(hook.site, DialogueRuntimeHookSite::Fixed(address) if address == chr_ram_ownership::BATTLE_COMPOSITION_CALL_SITE)
                && hook.bytes.len() == 3
                && hook.bytes[0] == 0x20,
            "battle composition ownership hook is no longer JSR absolute"
        );
        let ownership_target = u16::from_le_bytes([hook.bytes[1], hook.bytes[2]]);
        let ownership = self
            .fixed_routines
            .iter()
            .find(|routine| routine.role == "battle-to-dialogue CHR RAM ownership transfer")
            .context("battle-to-dialogue CHR RAM ownership transfer is missing")?;
        ensure!(
            ownership.address == ownership_target,
            "battle ownership hook no longer targets its generated transfer"
        );
        decode_rp2a03_sequence(
            &ownership.bytes,
            ownership.address,
            "integrated battle-to-dialogue ownership transfer",
        )?;
        Ok(FinalBattleConsumerRoute {
            central_fallback_target: roster_route.central_fallback_target,
            composition_call_address: chr_ram_ownership::BATTLE_COMPOSITION_CALL_SITE,
            composition_call_bytes: hook.bytes.clone(),
            regions: vec![FinalBattleConsumerRouteRegion {
                role: ownership.role,
                cpu_address: ownership.address,
                bytes: ownership.bytes.clone(),
            }],
        })
    }
}

pub(in crate::full_translation_install) fn verify_installed_chr_ram_ownership_gate(
    installed: &Rom,
) -> Result<()> {
    chr_ram_ownership::verify_installed_ownership_gate(installed)
}

/// 실행 코드를 전부 조립한다.
///
/// 고정 뱅크 동굴의 배치는 여기서 한 번에 정한다. 조각마다 시작 주소를 따로 두면
/// 하나가 커졌을 때 다음 조각을 덮는다.
pub(in crate::full_translation_install) fn plan_dialogue_runtime_code(
    source: &Rom,
    candidate: &Rom,
    runtime_code_cpu_start: u16,
    atlas_page: u8,
    code_page: u8,
    layout: resolve_request::MaterialLayout,
    consumer_catalog_layout: ConsumerCatalogRuntimeLayout,
    cold_request_mapper_register: u8,
    consumer_font_pages: consumer_font_page::ConsumerFontPageRoutes,
) -> Result<DialogueRuntimeCodePlan> {
    let bank_restore = bind_bank_restore_contract(candidate)?;
    bind_quiet_frame_gate(source, candidate)?;
    dispatcher_gate::bind_dispatcher_entry(source, candidate)?;
    dispatcher_gate::bind_source_identity_publisher_tail_cave(source, candidate)?;
    lifecycle::bind_lifecycle_sites(source, candidate)?;
    chr_selector::bind_selector_chain_site(candidate)?;
    chr_selector::bind_selector_cave(candidate)?;
    consumer_font_page::bind_consumer_font_page_lifetime(source, candidate)?;
    consumer_font_page::ending_lifetime::bind_ending_font_lifetime(source, candidate)?;
    chr_ram_ownership::bind_shared_chr_ram_ownership_boundary(candidate)?;
    dynamic_producer::bind_hook_sites(source, candidate)?;
    consumer_catalog::bind_consumer_catalog_sites(source, candidate)?;
    let chr_source_state = chr_source_state::bind_chr_source_state(candidate)?;

    let font_page_routes = font_page_route::build_font_page_route_runtime()?;
    let selector = chr_selector::build_chr_selector(
        font_page_routes.dialogue_selector,
        cold_request_mapper_register,
        chr_selector::SELECTOR_CHAIN_FALLBACK,
        font_page_routes.project_dialogue_page,
    )?;
    let cold_presentation_selector_origin = selector.address
        + u16::try_from(selector.bytes.len()).context("dialogue selector length overflow")?;
    let cold_presentation_selector = chr_selector::build_cold_request_presentation_selector(
        cold_presentation_selector_origin,
        cold_request_mapper_register,
        font_page_routes.project_dialogue_page,
    )?;
    let cold_presentation_selector_address = cold_presentation_selector.address;
    let changed_group_request_initializer_origin = cold_presentation_selector.address
        + u16::try_from(cold_presentation_selector.bytes.len())
            .context("cold-request presentation selector length overflow")?;
    let changed_group_request_initializer =
        resolved_page_publication::build_changed_group_request_initializer(
            changed_group_request_initializer_origin,
            cold_presentation_selector_address,
        )?;
    let changed_group_request_initializer_address = changed_group_request_initializer.address;
    let ownership_transfer_origin = changed_group_request_initializer.address
        + u16::try_from(changed_group_request_initializer.bytes.len())
            .context("changed-group request initializer length overflow")?;
    let ownership_transfer =
        chr_ram_ownership::build_battle_composition_ownership_transfer(ownership_transfer_origin)?;
    let ownership_transfer_address = ownership_transfer.address;

    let chr_restore_callee_cycles = chr_source_state.restore_callee_cycles();
    let transport = transport::build_transport_routine(
        runtime_code_cpu_start,
        atlas_page,
        cold_request_mapper_register,
    )?;
    let resolver_origin = transport.address
        + u16::try_from(transport.bytes.len()).context("transport routine length overflow")?;
    let resolver = resolve_request::build_resolve_request(resolver_origin, layout)?;
    let next_page_resolver_origin = resolver.address
        + u16::try_from(resolver.bytes.len()).context("initial resolver length overflow")?;
    let next_page_resolver =
        resolve_request::build_resolve_next_page_request(next_page_resolver_origin, layout)?;
    let trampoline_routine = trampoline::build_trampoline(bank_restore, transport.address)?;

    let gate = dispatcher_gate::build_dispatcher_gate(dispatcher_gate::RECLAIMED_GATE_CAVE_ORIGIN)?;
    let gate_address = gate.address;
    // 게시기는 selector·요청 조각과 같은 역할군이다. remap 비트와 물리 그룹을
    // 분리하면서 커졌으므로 작은 dispatcher 회수 구간에 억지로 두지 않는다.
    let publication_origin = ownership_transfer.address
        + u16::try_from(ownership_transfer.bytes.len())
            .context("ownership-transfer routine length overflow")?;
    let resolved_page_publication = resolved_page_publication::build_resolved_page_publication(
        publication_origin,
        changed_group_request_initializer_address,
    )?;
    let resolved_page_publication_address = resolved_page_publication.address;
    let initial_request_publisher_origin = resolved_page_publication.address
        + u16::try_from(resolved_page_publication.bytes.len())
            .context("resolved-page publication length overflow")?;
    let initial_request_publisher = dispatcher_gate::build_initial_request_publisher(
        initial_request_publisher_origin,
        resolver.address,
        code_page,
        resolved_page_publication_address,
    )?;
    let initial_request_publisher_address = initial_request_publisher.address;
    let consumer_font_page_activation_origin = initial_request_publisher.address
        + u16::try_from(initial_request_publisher.bytes.len())
            .context("initial request publisher length overflow")?;
    let consumer_font_page_activation = consumer_font_page::build_consumer_font_page_activation(
        consumer_font_page_activation_origin,
        font_page_routes.apply_route,
        consumer_font_pages,
    )?;
    let reclaimed_support_origin = gate.address
        + u16::try_from(gate.bytes.len()).context("dispatcher gate length overflow")?;
    let ending_font_lifetime = consumer_font_page::ending_lifetime::build_ending_font_lifetime(
        reclaimed_support_origin,
        consumer_font_page_activation.address,
        consumer_font_pages.ending_record,
    )?;
    let restore_source_pair_address = ending_font_lifetime.restore_source_pair.address;
    let ending_font_lifetime_hooks = ending_font_lifetime.hooks()?;
    let fixed_menu_font_page_appender = consumer_font_page::build_fixed_menu_font_page_appender(
        consumer_font_page::FIXED_MENU_FONT_PAGE_APPENDER_ORIGIN,
        consumer_font_page_activation.address,
        consumer_font_pages,
    )?;
    let composite_font_page_publisher_origin = consumer_font_page_activation.address
        + u16::try_from(consumer_font_page_activation.bytes.len())
            .context("consumer font page activation length overflow")?;
    let composite_font_page_publisher = consumer_font_page::build_composite_font_page_publisher(
        composite_font_page_publisher_origin,
        consumer_font_page_activation.address,
        consumer_font_pages,
    )?;
    let consumer_font_page_open_origin = composite_font_page_publisher.address
        + u16::try_from(composite_font_page_publisher.bytes.len())
            .context("composite font page publisher length overflow")?;
    let consumer_font_page_open = consumer_font_page::build_consumer_font_page_open(
        consumer_font_page_open_origin,
        consumer_font_page_activation.address,
    )?;
    let consumer_font_page_close_origin = consumer_font_page_open.address
        + u16::try_from(consumer_font_page_open.bytes.len())
            .context("consumer font page open length overflow")?;
    let consumer_font_page_close = consumer_font_page::build_consumer_font_page_close(
        consumer_font_page_close_origin,
        restore_source_pair_address,
    )?;
    let consumer_font_page_gameplay_handoff_origin = consumer_font_page_close.address
        + u16::try_from(consumer_font_page_close.bytes.len())
            .context("consumer font page close length overflow")?;
    let consumer_font_page_gameplay_handoff =
        consumer_font_page::build_consumer_font_page_gameplay_handoff(
            consumer_font_page_gameplay_handoff_origin,
        )?;
    let lifecycle = lifecycle::build_lifecycle_suite(
        next_page_resolver.address,
        code_page,
        resolved_page_publication_address,
    )?;
    let dynamic_producer_code_origin = next_page_resolver.address
        + u16::try_from(next_page_resolver.bytes.len())
            .context("next-page resolver length overflow")?;
    let dynamic_producers = dynamic_producer::build_dynamic_producer_runtime(
        dynamic_producer_code_origin,
        code_page,
        layout,
    )?;
    let catalog_code_origin = dynamic_producers
        .code_routines
        .last()
        .map(|routine| usize::from(routine.address) + routine.bytes.len())
        .context("dynamic producer runtime emitted no code routine")?;
    let catalog_code_origin = u16::try_from(catalog_code_origin)
        .context("consumer catalog code origin exceeds the CPU address space")?;
    let catalog_stub_origin = dynamic_producers
        .fixed_routines
        .last()
        .map(|routine| usize::from(routine.address) + routine.bytes.len())
        .context("dynamic producer runtime emitted no fixed bridge")?;
    let catalog_stub_origin = u16::try_from(catalog_stub_origin)
        .context("consumer catalog stub origin exceeds the CPU address space")?;
    let consumer_catalog = consumer_catalog::build_consumer_catalog_runtime(
        catalog_code_origin,
        code_page,
        catalog_stub_origin,
        consumer_font_page_activation.address,
        consumer_catalog_layout,
    )?;
    ensure_disjoint(
        &[
            &gate,
            &ending_font_lifetime.restore_source_pair,
            &ending_font_lifetime.enter_ending_record,
        ],
        dispatcher_gate::RECLAIMED_GATE_CAVE_END,
    )?;
    let mut fixed_support_bytes = gate.bytes;
    for routine in ending_font_lifetime.reclaimed_support_routines() {
        let expected_address = dispatcher_gate::RECLAIMED_GATE_CAVE_ORIGIN
            + u16::try_from(fixed_support_bytes.len())
                .context("reclaimed fixed support length overflow")?;
        ensure!(
            routine.address == expected_address,
            "{} is not contiguous with the reclaimed fixed support",
            routine.role
        );
        fixed_support_bytes.extend_from_slice(&routine.bytes);
    }
    let fixed_support_executable_byte_count = fixed_support_bytes.len();
    let fixed_support_capacity = usize::from(
        dispatcher_gate::RECLAIMED_GATE_CAVE_END - dispatcher_gate::RECLAIMED_GATE_CAVE_ORIGIN,
    );
    ensure!(
        fixed_support_bytes.len() <= fixed_support_capacity,
        "dialogue dispatcher and observer suite exceeds its reclaimed cave"
    );
    fixed_support_bytes.resize(fixed_support_capacity, 0xFF);
    let fixed_support = RuntimeRoutine {
        role: "dialogue dispatcher and ending font support",
        address: dispatcher_gate::RECLAIMED_GATE_CAVE_ORIGIN,
        bytes: fixed_support_bytes,
    };

    let publisher_origin = trampoline_routine.address
        + u16::try_from(trampoline_routine.bytes.len())
            .context("dialogue trampoline length overflow")?;
    let publisher = dispatcher_gate::build_source_identity_request_publisher(
        publisher_origin,
        resolver.address,
        code_page,
        resolved_page_publication_address,
    )?;

    // 예산은 시험만이 아니라 빌드가 지킨다. vblank를 넘기는 코드는 ROM에 들어가면
    // 안 되므로, 여기서 막지 않으면 그 판정이 시험을 돌리는 사람에게 넘어간다.
    // 의사결정 62번을 따른다.
    let reserve = trampoline::worst_case_reserve_cycles(bank_restore)?;
    let budget = budgeted_transport_cycles(reserve);
    let (largest_batch, frame_components) = transport::largest_fitting_tile_batch(
        runtime_code_cpu_start,
        atlas_page,
        chr_source_state,
        cold_request_mapper_register,
        budget,
    )?;
    ensure!(
        transport::TILES_PER_FRAME == largest_batch,
        "dialogue transport emits {} tile(s) per frame, but the candidate-bound emitted-code \
         cycle model selects {largest_batch} as the largest batch within the {budget}-cycle \
         transport budget (fixed={}, phase_route={}, overlay={}, restore={}, total={})",
        transport::TILES_PER_FRAME,
        frame_components.fixed,
        frame_components.phase_route,
        frame_components.overlay,
        frame_components.restore,
        frame_components.total(),
    );
    let frame_cycles = frame_components.total();
    ensure!(
        frame_cycles <= budget,
        "one transport frame costs {frame_cycles} cycles but only {budget} of the measured \
         {MAPPER_VBLANK_REMAINDER}-cycle mapper vblank remainder are budgeted after the \
         {SELECTOR_STACK_ENTRY_OVERHEAD}-cycle selector-stack entry overhead, the \
         {SAFETY_MARGIN_PERCENT}% margin, and the {reserve}-cycle trampoline reserve"
    );

    let publisher_address = publisher.head.address;
    ensure_disjoint(
        &[&trampoline_routine, &publisher.head],
        trampoline::TRAMPOLINE_CAVE_END,
    )?;
    ensure_disjoint(
        &[&publisher.tail],
        dispatcher_gate::SOURCE_IDENTITY_PUBLISHER_TAIL_CAVE_END,
    )?;
    ensure_disjoint(
        &[&ending_font_lifetime.exit_tail],
        consumer_font_page::ending_lifetime::ENDING_FONT_EXIT_TAIL_CAVE_END,
    )?;
    ensure_disjoint(
        &[&ending_font_lifetime.exit_head],
        consumer_font_page::ending_lifetime::ENDING_FONT_EXIT_HEAD_END,
    )?;
    ensure_disjoint(
        &[
            &font_page_routes.routine,
            &selector,
            &cold_presentation_selector,
            &changed_group_request_initializer,
            &ownership_transfer,
            &resolved_page_publication,
            &initial_request_publisher,
            &consumer_font_page_activation,
            &composite_font_page_publisher,
            &consumer_font_page_open,
            &consumer_font_page_close,
            &consumer_font_page_gameplay_handoff,
        ],
        chr_selector::SELECTOR_CAVE_END,
    )?;
    let mut fixed_routines = vec![
        font_page_routes.routine,
        trampoline_routine,
        publisher.head,
        publisher.tail,
        selector,
        cold_presentation_selector,
        changed_group_request_initializer,
        ownership_transfer,
        resolved_page_publication,
        initial_request_publisher,
        consumer_font_page_activation,
        composite_font_page_publisher,
        consumer_font_page_open,
        consumer_font_page_close,
        consumer_font_page_gameplay_handoff,
        ending_font_lifetime.exit_tail,
        ending_font_lifetime.exit_head,
    ];
    fixed_routines.extend(dynamic_producers.fixed_routines);
    fixed_routines.extend(consumer_catalog.fixed_routines);
    let mut code_routines = vec![transport, resolver, next_page_resolver];
    code_routines.extend(dynamic_producers.code_routines);
    code_routines.push(consumer_catalog.code_routine);
    let completed_page_entry = lifecycle.completed_page_entry;
    let handoff_residency_suspension_entry = lifecycle.handoff_residency_suspension_entry;
    let reclaimed_fixed_routines = vec![
        ReclaimedFixedRuntimeRoutine {
            routine: fixed_support,
            executable_byte_count: fixed_support_executable_byte_count,
            source_end_exclusive: dispatcher_gate::RECLAIMED_GATE_CAVE_END,
            expected_source_sha1: dispatcher_gate::EXPECTED_RECLAIMED_GATE_CAVE_SHA1,
        },
        ReclaimedFixedRuntimeRoutine {
            routine: lifecycle.routine,
            executable_byte_count: lifecycle.executable_byte_count,
            source_end_exclusive: lifecycle::LIFECYCLE_CAVE_END,
            expected_source_sha1: lifecycle::EXPECTED_SAMPLE_LIFECYCLE_SHA1,
        },
    ];

    let mut hooks = vec![
        DialogueRuntimeHook {
            role: DialogueRuntimeHookRole::ChrRamSelector,
            write_role: "dialogue CHR RAM selector hook",
            site: DialogueRuntimeHookSite::Fixed(chr_selector::SELECTOR_CHAIN_SITE),
            bytes: chr_selector::selector_hook_bytes(font_page_routes.select_active_page).to_vec(),
        },
        DialogueRuntimeHook {
            role: DialogueRuntimeHookRole::NmiPageComposer,
            write_role: "dialogue NMI page composer hook",
            site: DialogueRuntimeHookSite::Fixed(super::runtime_nmi_contract::CONSUMER_HOOK),
            bytes: trampoline::hook_bytes().to_vec(),
        },
        DialogueRuntimeHook {
            role: DialogueRuntimeHookRole::DispatcherGate,
            write_role: "dialogue dispatcher gate hook",
            site: DialogueRuntimeHookSite::Switchable {
                bank: 0x0A,
                address: dispatcher_gate::DISPATCHER_ENTRY,
            },
            bytes: dispatcher_gate::dispatcher_hook_bytes(gate_address).to_vec(),
        },
        DialogueRuntimeHook {
            role: DialogueRuntimeHookRole::InitialDirectEntryRequest,
            write_role: "dialogue initial direct-entry request hook",
            site: DialogueRuntimeHookSite::Switchable {
                bank: 0x0A,
                address: dispatcher_gate::COLD_ENTRY,
            },
            bytes: dispatcher_gate::request_hook_bytes(initial_request_publisher_address).to_vec(),
        },
    ];
    for (role, write_role, address) in [
        (
            DialogueRuntimeHookRole::E4TransitionEntryRequest,
            "dialogue E4 transition-entry request hook",
            lifecycle::E4_TRANSITION_SITE,
        ),
        (
            DialogueRuntimeHookRole::E6TransitionEntryRequest,
            "dialogue E6 transition-entry request hook",
            lifecycle::E6_TRANSITION_SITE,
        ),
        (
            DialogueRuntimeHookRole::E7CallerResumeRequest,
            "dialogue E7 caller-resume request hook",
            lifecycle::E7_RESUME_SITE,
        ),
    ] {
        hooks.push(DialogueRuntimeHook {
            role,
            write_role,
            site: DialogueRuntimeHookSite::Switchable {
                bank: 0x0A,
                address,
            },
            bytes: dispatcher_gate::request_hook_bytes(publisher_address).to_vec(),
        });
    }
    hooks.extend([
        DialogueRuntimeHook {
            role: DialogueRuntimeHookRole::CompletedPageAdvanceOrLifetimeEnd,
            write_role: "dialogue completed-page lifecycle hook",
            site: DialogueRuntimeHookSite::Switchable {
                bank: 0x0A,
                address: lifecycle::COMPLETED_PAGE_SITE,
            },
            bytes: lifecycle::completed_page_hook_bytes(completed_page_entry)?,
        },
        DialogueRuntimeHook {
            role: DialogueRuntimeHookRole::E7CallerHandoffResidencySuspension,
            write_role: "dialogue E7 caller-handoff residency suspension hook",
            site: DialogueRuntimeHookSite::Switchable {
                bank: 0x0A,
                address: lifecycle::E7_HANDOFF_SITE,
            },
            bytes: lifecycle::handoff_residency_suspension_hook_bytes(
                handoff_residency_suspension_entry,
            )
            .to_vec(),
        },
        DialogueRuntimeHook {
            role: DialogueRuntimeHookRole::BattleComposerInvalidatesDialogueResidency,
            write_role: "battle composer dialogue-residency invalidation hook",
            site: DialogueRuntimeHookSite::Fixed(chr_ram_ownership::BATTLE_COMPOSITION_CALL_SITE),
            bytes: chr_ram_ownership::ownership_transfer_hook_bytes(ownership_transfer_address)
                .to_vec(),
        },
    ]);
    hooks.extend(dynamic_producers.hooks);
    hooks.extend(consumer_catalog.hooks);
    hooks.push(
        consumer_font_page::fixed_menu_font_page_appender_installation(
            &fixed_menu_font_page_appender,
        )?,
    );
    hooks.extend(consumer_font_page::fixed_menu_font_page_hooks(
        fixed_menu_font_page_appender.address,
    )?);
    hooks.push(consumer_font_page::page_publisher_hook(
        composite_font_page_publisher_origin,
    )?);
    hooks.extend(consumer_font_page::screen_lifetime_hooks(
        consumer_font_page_open_origin,
        consumer_font_page_close_origin,
    )?);
    hooks.push(consumer_font_page::gameplay_handoff_hook(
        consumer_font_page_gameplay_handoff_origin,
    )?);
    hooks.extend(ending_font_lifetime_hooks);

    let plan = DialogueRuntimeCodePlan {
        code_routines,
        fixed_routines,
        reclaimed_fixed_routines,
        hooks,
        chr_restore_callee_cycles,
    };
    verify_planned_mapper_select_writes(&plan)?;
    Ok(plan)
}

fn verify_planned_mapper_select_writes(plan: &DialogueRuntimeCodePlan) -> Result<()> {
    for routine in plan.code_routines.iter().chain(&plan.fixed_routines) {
        verify_generated_executable_mapper_select_pairs(
            routine.role,
            routine.address,
            &routine.bytes,
        )?;
    }
    for reclaimed in &plan.reclaimed_fixed_routines {
        ensure!(
            reclaimed.executable_byte_count <= reclaimed.routine.bytes.len(),
            "{} executable extent exceeds its overwrite extent",
            reclaimed.routine.role
        );
        ensure!(
            reclaimed.routine.bytes[reclaimed.executable_byte_count..]
                .iter()
                .all(|byte| *byte == 0xFF),
            "{} reclaimed-cave padding is not exact $FF",
            reclaimed.routine.role
        );
        verify_generated_executable_mapper_select_pairs(
            reclaimed.routine.role,
            reclaimed.routine.address,
            &reclaimed.routine.bytes[..reclaimed.executable_byte_count],
        )?;
    }
    for hook in &plan.hooks {
        let address = match hook.site {
            DialogueRuntimeHookSite::Fixed(address)
            | DialogueRuntimeHookSite::Switchable { address, .. } => address,
        };
        verify_generated_executable_mapper_select_pairs(hook.write_role, address, &hook.bytes)?;
    }
    Ok(())
}

/// Verifies the typed, generated plan's direct mapper165 writes. This is intentionally not the
/// global ExecutableImage denominator: runtime-computed indirect addresses remain a separate
/// fail-closed admission gate. Direct aliases and absolute-indexed ranges are handled here.
fn verify_generated_executable_mapper_select_pairs(
    role: &str,
    origin: u16,
    bytes: &[u8],
) -> Result<()> {
    let mut offset = 0;
    let mut decoded = Vec::new();
    while offset < bytes.len() {
        let address = origin
            .checked_add(u16::try_from(offset)?)
            .context("generated executable address overflow")?;
        let instruction = decode_bytes(&bytes[offset..])
            .with_context(|| format!("decode generated executable {role} at +{offset:04X}"))?;
        ensure!(
            instruction.opcode_is_documented(),
            "generated executable {role} contains an undocumented opcode at +{offset:04X}"
        );
        let semantics = Rp2A03::semantics(&instruction, &address)
            .expect("RP2A03 static semantics are infallible");
        let mut direct_value_write = false;
        for access in semantics.location_accesses {
            if access.kind != AccessKind::Write {
                continue;
            }
            let Location::Memory(memory) = access.location else {
                continue;
            };
            match memory {
                MemoryAddress::Direct(target) => match decode_mapper165_write(target) {
                    Some(Mapper165Register::BankSelect) => anyhow::bail!(
                        "generated executable {role} directly writes mapper-select alias ${target:04X} at +{offset:04X}"
                    ),
                    Some(Mapper165Register::BankData) => direct_value_write = true,
                    Some(register) => anyhow::bail!(
                        "generated executable {role} directly writes unexpected mapper165 {register:?} alias ${target:04X} at +{offset:04X}"
                    ),
                    None => {}
                },
                MemoryAddress::Effective {
                    mode: AddressingMode::AbsoluteX | AddressingMode::AbsoluteY,
                    operand: Operand::Word(base),
                } => {
                    ensure!(
                        !(0..=u8::MAX).any(|index| {
                            decode_mapper165_write(base.wrapping_add(u16::from(index))).is_some()
                        }),
                        "generated executable {role} has an absolute-indexed write whose effective range can enter mapper165 ports at +{offset:04X}"
                    );
                }
                // Zero-page indexed writes wrap inside page zero and cannot reach mapper I/O.
                MemoryAddress::Effective {
                    mode: AddressingMode::ZeroPageX | AddressingMode::ZeroPageY,
                    ..
                }
                | MemoryAddress::Stack => {}
                // Indirect effective addresses need a whole-CFG pointer-range proof. They are not
                // silently classified as safe by this bounded direct-write verifier.
                MemoryAddress::Effective {
                    mode:
                        AddressingMode::ZeroPageIndexedIndirectX
                        | AddressingMode::ZeroPageIndirectIndexedY,
                    ..
                }
                | MemoryAddress::Pointer { .. }
                | MemoryAddress::InterruptVector => {}
                MemoryAddress::Effective { mode, .. } => anyhow::bail!(
                    "generated executable {role} has an unhandled effective write mode {mode:?} at +{offset:04X}"
                ),
            }
        }
        decoded.push((address, instruction, direct_value_write));
        offset += instruction.encoded_len();
    }

    let mut bypass_targets = BTreeSet::new();
    for (address, instruction, _) in &decoded {
        match rp2a03_direct_control_flow(instruction, *address)? {
            Rp2a03DirectControlFlow::Branch { target, .. }
            | Rp2a03DirectControlFlow::Jump {
                target: Some(target),
            }
            | Rp2a03DirectControlFlow::Call { target, .. } => {
                bypass_targets.insert(target);
            }
            Rp2a03DirectControlFlow::Jump { target: None } => anyhow::bail!(
                "generated executable {role} contains an indirect jump whose mapper-pair entry effects are unresolved at ${address:04X}"
            ),
            _ => {}
        }
    }
    for (value_index, (value_address, _, direct_value_write)) in decoded.iter().enumerate() {
        if !*direct_value_write {
            continue;
        }
        let mut selector = None;
        for selector_index in (0..value_index).rev() {
            let (address, preceding, _) = decoded[selector_index];
            if preceding.mnemonic() == Mnemonic::Jsr
                && preceding.operand()
                    == Operand::Word(
                        crate::mapper165::selector_safety::SELECT_REGISTER_ROUTINE_ADDRESS,
                    )
            {
                selector = Some((selector_index, address));
                break;
            }
            if !matches!(
                rp2a03_direct_control_flow(&preceding, address)?,
                Rp2a03DirectControlFlow::FallThrough { .. }
            ) {
                break;
            }
        }
        let (selector_index, selector_address) = selector.with_context(|| {
            format!(
                "generated executable {role} writes canonical mapper-value address $8001 at ${value_address:04X} without a same-block common selector call"
            )
        })?;
        let after_selector = decoded
            .get(selector_index + 1)
            .map(|(address, _, _)| *address)
            .unwrap_or(*value_address);
        ensure!(
            !bypass_targets
                .range(after_selector..=*value_address)
                .next()
                .is_some(),
            "generated executable {role} can branch between common selector call ${selector_address:04X} and mapper-value write ${value_address:04X}"
        );
    }
    Ok(())
}

/// ROM의 한 자리에 놓이는 실행 코드 조각이다.
#[derive(Debug)]
pub(in crate::full_translation_install) struct RuntimeRoutine {
    pub(in crate::full_translation_install) role: &'static str,
    pub(in crate::full_translation_install) address: u16,
    pub(in crate::full_translation_install) bytes: Vec<u8>,
}

/// 같은 동굴에 놓이는 조각들이 서로 겹치거나 동굴을 넘지 않아야 한다.
/// 겹치면 조용히 잘못된 코드가 실행되고, 넘으면 원본 자료를 덮는다.
pub(super) fn ensure_disjoint(routines: &[&RuntimeRoutine], cave_end: u16) -> Result<()> {
    let mut ordered: Vec<&RuntimeRoutine> = routines.to_vec();
    ordered.sort_by_key(|routine| routine.address);
    for pair in ordered.windows(2) {
        ensure!(
            usize::from(pair[0].address) + pair[0].bytes.len() <= usize::from(pair[1].address),
            "{} ends at {:04X} and overlaps {} at {:04X}",
            pair[0].role,
            usize::from(pair[0].address) + pair[0].bytes.len(),
            pair[1].role,
            pair[1].address
        );
    }
    if let Some(last) = ordered.last() {
        ensure!(
            usize::from(last.address) + last.bytes.len() <= usize::from(cave_end),
            "{} ends at {:04X} and reaches past the reserved cave end {cave_end:04X}",
            last.role,
            usize::from(last.address) + last.bytes.len()
        );
    }
    Ok(())
}

/// 명령 목록을 이어 붙였을 때 다음 명령이 놓일 주소다. 분기 대상을 되메울 때 쓴다.
fn next_address(origin: u16, instructions: &[Instruction]) -> Result<u16> {
    let length = assemble_at(origin, instructions)
        .context("cannot measure a dialogue runtime routine")?
        .len();
    u16::try_from(usize::from(origin) + length)
        .context("dialogue runtime routine crosses the CPU address space")
}

/// 명령 목록이 최악의 경우 쓰는 사이클이다.
///
/// `JSR`는 명령 자체의 6사이클만 세고 불려 가는 코드의 비용은 세지 않는다. 그래서
/// 호출이 섞인 목록을 그냥 더하면 예산이 조용히 과소평가된다. vblank 예산에서
/// 과소평가는 실기 손상이므로, 이 함수는 호출을 만나면 그 자리에서 거부한다.
///
/// 호출이 필요한 코드는 `worst_case_cycles_with_calls`로 불린 곳의 실측 비용을 함께
/// 넘겨야 한다. «얼마인지 모르는 것을 6이라고 세지 않는다»가 규칙이다.
fn worst_case_cycles(instructions: &[Instruction]) -> Result<u32> {
    worst_case_cycles_with_calls(instructions, &[])
}

/// 불려 가는 코드의 최악 사이클을 주소별로 함께 받는다.
fn worst_case_cycles_with_calls(
    instructions: &[Instruction],
    callee_cycles: &[(u16, u32)],
) -> Result<u32> {
    let mut total = 0;
    for instruction in instructions {
        total += u32::from(instruction.worst_case_cycles());
        if let Instruction::JsrAbsolute(target) = instruction {
            let cost = (*target
                == crate::mapper165::selector_safety::SELECT_REGISTER_ROUTINE_ADDRESS)
                .then_some(crate::mapper165::selector_safety::SELECT_REGISTER_CALLEE_CYCLES)
                .or_else(|| {
                    callee_cycles
                        .iter()
                        .find(|(address, _)| address == target)
                        .map(|(_, cost)| *cost)
                })
                .with_context(|| {
                    format!(
                        "a cycle budget counted JSR {target:04X} as six cycles; \
                         the cost of the called code is unknown and must be measured"
                    )
                })?;
            total += cost;
        }
    }
    Ok(total)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn selector_stack_entry_overhead_is_removed_from_the_source_measurement() {
        assert_eq!(SOURCE_MEASURED_VBLANK_REMAINDER, 1_704);
        assert_eq!(SELECTOR_STACK_ENTRY_OVERHEAD, 12);
        assert_eq!(MAPPER_VBLANK_REMAINDER, 1_692);
    }

    #[test]
    fn overlapping_routines_are_refused() {
        let first = RuntimeRoutine {
            role: "first",
            address: 0xF400,
            bytes: vec![0; 16],
        };
        let second = RuntimeRoutine {
            role: "second",
            address: 0xF408,
            bytes: vec![0; 4],
        };

        let error = ensure_disjoint(&[&first, &second], 0xF4B0).unwrap_err();

        assert!(error.to_string().contains("overlaps"));
    }

    #[test]
    fn a_routine_past_the_cave_end_is_refused() {
        let only = RuntimeRoutine {
            role: "only",
            address: 0xF4A0,
            bytes: vec![0; 32],
        };

        let error = ensure_disjoint(&[&only], 0xF4B0).unwrap_err();

        assert!(error.to_string().contains("past the reserved cave end"));
    }

    #[test]
    fn generated_code_cannot_bypass_the_common_selector_writer() {
        let direct = assemble_at(
            0xA000,
            &[
                Instruction::LdaImmediate(6),
                Instruction::StaAbsolute(0x8000),
                Instruction::Rts,
            ],
        )
        .unwrap();
        let error =
            verify_generated_executable_mapper_select_pairs("direct selector", 0xA000, &direct)
                .unwrap_err();

        assert!(error.to_string().contains("mapper-select alias"));
    }

    #[test]
    fn generated_code_cannot_write_an_unowned_mapper_register_alias() {
        let direct = assemble_at(
            0xA000,
            &[
                Instruction::LdaImmediate(0),
                Instruction::StaAbsolute(0xBFFE),
                Instruction::Rts,
            ],
        )
        .unwrap();

        let error = verify_generated_executable_mapper_select_pairs(
            "direct mirroring alias",
            0xA000,
            &direct,
        )
        .unwrap_err();

        assert!(error.to_string().contains("Mirroring alias $BFFE"));
    }

    #[test]
    fn generated_value_writes_require_a_same_block_selector_call() {
        let unpaired = assemble_at(
            0xA000,
            &[
                Instruction::LdaImmediate(0x20),
                Instruction::StaAbsolute(0x8001),
                Instruction::Rts,
            ],
        )
        .unwrap();
        assert!(
            verify_generated_executable_mapper_select_pairs("unpaired value", 0xA000, &unpaired)
                .unwrap_err()
                .to_string()
                .contains("without a same-block common selector call")
        );

        let paired = assemble_at(
            0xA000,
            &[
                Instruction::LdaImmediate(6),
                crate::mapper165::selector_safety::select_register_instruction(),
                Instruction::LdaImmediate(0x20),
                Instruction::StaAbsolute(0x8001),
                Instruction::Rts,
            ],
        )
        .unwrap();
        verify_generated_executable_mapper_select_pairs("paired value", 0xA000, &paired).unwrap();
    }

    #[test]
    fn generated_branches_cannot_enter_between_a_selector_and_its_value() {
        let bytes = assemble_at(
            0xA000,
            &[
                Instruction::BeqAbsolute(0xA009),
                Instruction::LdaImmediate(6),
                crate::mapper165::selector_safety::select_register_instruction(),
                Instruction::LdaImmediate(0x20),
                Instruction::StaAbsolute(0x8001),
                Instruction::Rts,
            ],
        )
        .unwrap();
        let error =
            verify_generated_executable_mapper_select_pairs("branch-bypass value", 0xA000, &bytes)
                .unwrap_err();

        assert!(error.to_string().contains("can branch between"));
    }

    #[test]
    fn generated_absolute_indexed_writes_cannot_reach_mapper_aliases() {
        let bytes = assemble_at(
            0xA000,
            &[Instruction::StaAbsoluteX(0x7F80), Instruction::Rts],
        )
        .unwrap();
        let error = verify_generated_executable_mapper_select_pairs(
            "indexed mapper candidate",
            0xA000,
            &bytes,
        )
        .unwrap_err();

        assert!(error.to_string().contains("absolute-indexed write"));
    }
}
