//! 대사 런타임이 ROM에 넣는 실행 코드다.
//!
//! 갈래를 나눈 기준은 «무엇이 바뀌면 이 파일이 바뀌는가»다. 전송 루프는 프레임
//! 예산이 바뀌면 바뀌고, 트램폴린은 원본 NMI 계약이 바뀌면 바뀐다.

use std::collections::BTreeSet;

use anyhow::{Context, Result, ensure};
use serde::Serialize;

use super::{
    consumer_catalog::ConsumerCatalogRuntimeLayout,
    runtime_bank_contract::bind_bank_restore_contract, runtime_nmi_contract::bind_quiet_frame_gate,
    screen_font_residency::ScreenFontPageRoutes,
};
use crate::{
    mapper165::{
        FinalBattleConsumerRoute, FinalBattleConsumerRouteRegion, FinalConsumerRouteRegion,
        FinalRosterConsumerRoute,
    },
    rom::Rom,
    typed_source::decode_rp2a03_sequence,
};

mod assembly;
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
mod mapper_write_verification;
pub(in crate::full_translation_install) mod resolve_request;
mod resolved_page_publication;
mod speaker_prefix;
pub(super) mod trampoline;
pub(in crate::full_translation_install) mod transport;

pub(in crate::full_translation_install) use assembly::RuntimeRoutine;
use assembly::{
    ensure_routines_fit_cave, next_address, worst_case_cycles, worst_case_cycles_with_calls,
};
use mapper_write_verification::verify_planned_mapper_select_writes;

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
    FixedMenuStorageCapacityAppender,
    DialogueSpeakerPrefixProjection,
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
    consumer_font_pages: ScreenFontPageRoutes,
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
    speaker_prefix::bind_speaker_prefix_output(source, candidate)?;
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
    let page_recipe_request_initializer_origin = cold_presentation_selector.address
        + u16::try_from(cold_presentation_selector.bytes.len())
            .context("cold-request presentation selector length overflow")?;
    let page_recipe_request_initializer =
        resolved_page_publication::build_page_recipe_request_initializer(
            page_recipe_request_initializer_origin,
            cold_presentation_selector_address,
        )?;
    let page_recipe_request_initializer_address = page_recipe_request_initializer.address;
    let ownership_transfer_origin = page_recipe_request_initializer.address
        + u16::try_from(page_recipe_request_initializer.bytes.len())
            .context("page-recipe request initializer length overflow")?;
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
        page_recipe_request_initializer_address,
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
        consumer_font_pages.front_end_record_action,
        consumer_catalog_layout,
    )?;
    ensure_routines_fit_cave(
        &[
            &gate,
            &ending_font_lifetime.restore_source_pair,
            &ending_font_lifetime.enter_ending_record,
        ],
        dispatcher_gate::RECLAIMED_GATE_CAVE_ORIGIN,
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
    ensure_routines_fit_cave(
        &[&trampoline_routine, &publisher.head],
        trampoline::TRAMPOLINE_ORIGIN,
        trampoline::TRAMPOLINE_CAVE_END,
    )?;
    ensure_routines_fit_cave(
        &[&publisher.tail],
        dispatcher_gate::SOURCE_IDENTITY_PUBLISHER_TAIL_ORIGIN,
        dispatcher_gate::SOURCE_IDENTITY_PUBLISHER_TAIL_CAVE_END,
    )?;
    ensure_routines_fit_cave(
        &[&ending_font_lifetime.exit_tail],
        consumer_font_page::ending_lifetime::ENDING_FONT_EXIT_TAIL_ORIGIN,
        consumer_font_page::ending_lifetime::ENDING_FONT_EXIT_TAIL_CAVE_END,
    )?;
    ensure_routines_fit_cave(
        &[&ending_font_lifetime.exit_head],
        consumer_font_page::ending_lifetime::ENDING_FONT_EXIT_HEAD_ORIGIN,
        consumer_font_page::ending_lifetime::ENDING_FONT_EXIT_HEAD_END,
    )?;
    ensure_routines_fit_cave(
        &[
            &font_page_routes.routine,
            &selector,
            &cold_presentation_selector,
            &page_recipe_request_initializer,
            &ownership_transfer,
            &resolved_page_publication,
            &initial_request_publisher,
            &consumer_font_page_activation,
            &composite_font_page_publisher,
            &consumer_font_page_open,
            &consumer_font_page_close,
            &consumer_font_page_gameplay_handoff,
        ],
        chr_selector::SELECTOR_CAVE_ORIGIN,
        chr_selector::SELECTOR_CAVE_END,
    )?;
    let mut fixed_routines = vec![
        font_page_routes.routine,
        trampoline_routine,
        publisher.head,
        publisher.tail,
        selector,
        cold_presentation_selector,
        page_recipe_request_initializer,
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
        speaker_prefix::blank_speaker_prefix_output_hook()?,
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn selector_stack_entry_overhead_is_removed_from_the_source_measurement() {
        assert_eq!(SOURCE_MEASURED_VBLANK_REMAINDER, 1_704);
        assert_eq!(SELECTOR_STACK_ENTRY_OVERHEAD, 12);
        assert_eq!(MAPPER_VBLANK_REMAINDER, 1_692);
    }
}
