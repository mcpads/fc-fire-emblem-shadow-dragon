//! 대사 런타임이 ROM에 넣는 실행 코드다.
//!
//! 갈래를 나눈 기준은 «무엇이 바뀌면 이 파일이 바뀌는가»다. 전송 루프는 프레임
//! 예산이 바뀌면 바뀌고, 트램폴린은 원본 NMI 계약이 바뀌면 바뀐다.

use std::collections::BTreeSet;

use anyhow::{Context, Result, ensure};
use serde::Serialize;

use super::{
    consumer_catalog::ConsumerCatalogRuntimeLayout,
    runtime_bank_contract::bind_bank_restore_contract,
    runtime_nmi_contract::bind_synchronous_composer_resume,
    screen_font_residency::ScreenFontPageRoutes,
    shop_item_residency::ShopItemResidencyRuntimeContract,
    storage_residency::StorageItemListRuntimeRoute,
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
mod font_page_route;
pub(in crate::full_translation_install) mod lifecycle;
mod mapper_write_verification;
pub(in crate::full_translation_install) mod resolve_request;
mod resolved_page_publication;
mod speaker_prefix;
pub(super) mod synchronous_composer;
pub(in crate::full_translation_install) mod transport;

pub(in crate::full_translation_install) use assembly::RuntimeRoutine;
use assembly::{ensure_routines_fit_cave, next_address};
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
    ConsumerCatalogDirectItemEntry,
    ConsumerCatalogDirectItemNormalizer,
    ShopItemListAppender,
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
    pub(in crate::full_translation_install) expected_source_sha1: String,
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
}

impl DialogueRuntimeCodePlan {
    pub(in crate::full_translation_install) fn hook_roles(&self) -> Vec<DialogueRuntimeHookRole> {
        self.hooks.iter().map(|hook| hook.role).collect()
    }

    /// 네 종류의 새 레코드 진입이 하나의 물리 줄 초기화 경계를 공유하고, 같은
    /// 레코드의 다음 페이지는 그 초기화를 우회하는지 조립 결과에서 다시 확인한다.
    pub(in crate::full_translation_install) fn new_record_line_buffer_reset_routes_bound(
        &self,
    ) -> Result<bool> {
        let resolver = self
            .code_routines
            .iter()
            .find(|routine| routine.role == resolve_request::INITIAL_PAGE_REQUEST_RESOLVER_ROLE)
            .context("new-record request resolver is missing")?;
        ensure!(
            resolve_request::contains_new_record_line_buffer_reset(resolver)?,
            "new-record request resolver no longer clears the six physical dialogue rows"
        );
        let next_page_resolver = self
            .code_routines
            .iter()
            .find(|routine| routine.role == resolve_request::NEXT_PAGE_REQUEST_RESOLVER_ROLE)
            .context("same-record next-page resolver is missing")?;
        ensure!(
            !resolve_request::contains_new_record_line_buffer_reset(next_page_resolver)?,
            "same-record next-page resolver unexpectedly clears the physical dialogue rows"
        );

        let initial_publisher = self
            .fixed_routines
            .iter()
            .find(|routine| routine.role == dispatcher_gate::INITIAL_REQUEST_PUBLISHER_ROLE)
            .context("initial request publisher is missing")?;
        let transition_publisher = self
            .fixed_routines
            .iter()
            .find(|routine| routine.role == dispatcher_gate::SOURCE_IDENTITY_REQUEST_PUBLISHER_ROLE)
            .context("source-identity request publisher is missing")?;
        for publisher in [initial_publisher, transition_publisher] {
            let resolver_call = [0x20, resolver.address as u8, (resolver.address >> 8) as u8];
            ensure!(
                publisher
                    .bytes
                    .windows(resolver_call.len())
                    .any(|window| window == resolver_call),
                "{} no longer reaches the new-record resolver",
                publisher.role
            );
        }

        for (role, expected_target) in [
            (
                DialogueRuntimeHookRole::InitialDirectEntryRequest,
                initial_publisher.address,
            ),
            (
                DialogueRuntimeHookRole::E4TransitionEntryRequest,
                transition_publisher.address,
            ),
            (
                DialogueRuntimeHookRole::E6TransitionEntryRequest,
                transition_publisher.address,
            ),
            (
                DialogueRuntimeHookRole::E7CallerResumeRequest,
                transition_publisher.address,
            ),
        ] {
            let matching = self
                .hooks
                .iter()
                .filter(|hook| hook.role == role)
                .collect::<Vec<_>>();
            ensure!(
                matching.len() == 1,
                "new-record line-buffer route has {} hooks for {role:?}",
                matching.len()
            );
            let hook = matching[0];
            ensure!(
                matches!(
                    hook.site,
                    DialogueRuntimeHookSite::Switchable { bank: 0x0A, .. }
                ) && hook.bytes == [0x20, expected_target as u8, (expected_target >> 8) as u8,],
                "{role:?} no longer reaches its new-record request publisher"
            );
        }
        Ok(true)
    }

    pub(in crate::full_translation_install) fn consumer_catalog_paths_planned(&self) -> bool {
        let roles = self.hook_roles().into_iter().collect::<BTreeSet<_>>();
        [
            DialogueRuntimeHookRole::ConsumerCatalogItemAppender,
            DialogueRuntimeHookRole::ConsumerCatalogUnitAppender,
            DialogueRuntimeHookRole::ConsumerCatalogClassAppender,
            DialogueRuntimeHookRole::ConsumerCatalogDirectItemEntry,
            DialogueRuntimeHookRole::ConsumerCatalogDirectItemNormalizer,
            DialogueRuntimeHookRole::ShopItemListAppender,
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

    pub(in crate::full_translation_install) fn verify_shop_item_residency_routes(
        &self,
        contract: &ShopItemResidencyRuntimeContract,
    ) -> Result<()> {
        let item_appender = self
            .code_routines
            .iter()
            .find(|routine| routine.role == "consumer catalog indexed string appender")
            .context("consumer catalog item appender is missing")?;
        consumer_catalog::verify_shop_item_residency_route(item_appender, *contract)?;
        consumer_catalog::verify_shop_item_list_hook(&self.hooks)?;
        let selector = self
            .fixed_routines
            .iter()
            .find(|routine| routine.role == "dialogue CHR RAM selector")
            .context("dialogue CHR selector is missing")?;
        chr_selector::verify_e7_dialogue_page_residency(
            selector,
            contract.e7_caller_resume_flag_address,
        )?;
        Ok(())
    }

    pub(in crate::full_translation_install) fn verify_storage_item_residency_routes(
        &self,
        material: &ShopItemResidencyRuntimeContract,
        storage: &StorageItemListRuntimeRoute,
        pages: ScreenFontPageRoutes,
    ) -> Result<()> {
        let item_appender = self
            .code_routines
            .iter()
            .find(|routine| routine.role == "consumer catalog indexed string appender")
            .context("consumer catalog item appender is missing")?;
        consumer_catalog::verify_storage_item_residency_route(item_appender, *material, *storage)?;

        let activation = self
            .fixed_routines
            .iter()
            .find(|routine| routine.role == "consumer font page activation")
            .context("consumer font page activation is missing")?;
        let publisher = self
            .fixed_routines
            .iter()
            .find(|routine| routine.role == "composite consumer font page publisher")
            .context("composite consumer font page publisher is missing")?;
        let expected = consumer_font_page::build_composite_font_page_publisher(
            publisher.address,
            activation.address,
            pages,
            *storage,
        )?;
        ensure!(
            publisher.bytes == expected.routine.bytes,
            "storage item-list route no longer refines the installed composite font publisher"
        );
        Ok(())
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
pub(super) struct DialogueRuntimeCodeInputs<'a> {
    pub(super) source: &'a Rom,
    pub(super) candidate: &'a Rom,
    pub(super) maximum_dialogue_font_group_selector_range_sha1: &'a str,
    pub(super) runtime_code_cpu_start: u16,
    pub(super) atlas_page: u8,
    pub(super) code_page: u8,
    pub(super) layout: resolve_request::MaterialLayout,
    pub(super) consumer_catalog_layout: ConsumerCatalogRuntimeLayout,
    pub(super) shop_item_residency: ShopItemResidencyRuntimeContract,
    pub(super) storage_item_list: StorageItemListRuntimeRoute,
    pub(super) cold_request_mapper_register: u8,
    pub(super) consumer_font_pages: ScreenFontPageRoutes,
}

pub(in crate::full_translation_install) fn plan_dialogue_runtime_code(
    inputs: DialogueRuntimeCodeInputs<'_>,
) -> Result<DialogueRuntimeCodePlan> {
    let DialogueRuntimeCodeInputs {
        source,
        candidate,
        maximum_dialogue_font_group_selector_range_sha1,
        runtime_code_cpu_start,
        atlas_page,
        code_page,
        layout,
        consumer_catalog_layout,
        shop_item_residency,
        storage_item_list,
        cold_request_mapper_register,
        consumer_font_pages,
    } = inputs;
    let bank_restore = bind_bank_restore_contract(candidate)?;
    bind_synchronous_composer_resume(source, candidate)?;
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
    chr_source_state::bind_chr_source_state(candidate)?;

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
    let synchronous_composer = synchronous_composer::build_synchronous_composer(
        bank_restore,
        transport.address,
        code_page,
    )?;

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
        synchronous_composer.address,
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
    let consumer_font_page::CompositeFontPagePublisher {
        routine: composite_font_page_publisher,
        source_page_selection,
    } = consumer_font_page::build_composite_font_page_publisher(
        composite_font_page_publisher_origin,
        consumer_font_page_activation.address,
        consumer_font_pages,
        storage_item_list,
    )?;
    let consumer_font_page_open_origin = composite_font_page_publisher.address
        + u16::try_from(composite_font_page_publisher.bytes.len())
            .context("composite font page publisher length overflow")?;
    let consumer_font_page_open = consumer_font_page::build_consumer_font_page_open(
        consumer_font_page_open_origin,
        consumer_font_page_activation.address,
        source_page_selection,
    )?;
    let consumer_font_page_close_origin = consumer_font_page_open.address
        + u16::try_from(consumer_font_page_open.bytes.len())
            .context("consumer font page open length overflow")?;
    let consumer_font_page_close = consumer_font_page::build_consumer_font_page_close(
        consumer_font_page_close_origin,
        restore_source_pair_address,
    )?;
    // The ending exit-tail owner leaves one exact eight-byte suffix in its source-bound cave.
    // Gameplay handoff is independent of selector ordering, so placing it there keeps the
    // selector cave available for screen-residency policy without admitting another raw gap.
    let consumer_font_page_gameplay_handoff_origin =
        consumer_font_page::ending_lifetime::ENDING_FONT_EXIT_TAIL_END;
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
        consumer_catalog::ConsumerCatalogRuntimeInputs {
            code_origin: catalog_code_origin,
            code_page,
            entry_stub_origin: catalog_stub_origin,
            font_page_activation: consumer_font_page_activation.address,
            catalog_default_font_route: consumer_font_pages.catalog[0],
            front_end_record_action_route: consumer_font_pages.front_end_record_action,
            layout: consumer_catalog_layout,
            shop_item_residency,
            storage_item_list,
        },
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

    let publisher_origin = synchronous_composer.address
        + u16::try_from(synchronous_composer.bytes.len())
            .context("synchronous dialogue composer length overflow")?;
    let publisher = dispatcher_gate::build_source_identity_request_publisher(
        publisher_origin,
        resolver.address,
        code_page,
        resolved_page_publication_address,
    )?;

    let publisher_address = publisher.head.address;
    ensure_routines_fit_cave(
        &[&synchronous_composer, &publisher.head],
        synchronous_composer::COMPOSER_ORIGIN,
        synchronous_composer::COMPOSER_CAVE_END,
    )?;
    ensure_routines_fit_cave(
        &[&publisher.tail],
        dispatcher_gate::SOURCE_IDENTITY_PUBLISHER_TAIL_ORIGIN,
        dispatcher_gate::SOURCE_IDENTITY_PUBLISHER_TAIL_CAVE_END,
    )?;
    ensure_routines_fit_cave(
        &[
            &ending_font_lifetime.exit_tail,
            &consumer_font_page_gameplay_handoff,
        ],
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
        ],
        chr_selector::SELECTOR_CAVE_ORIGIN,
        chr_selector::SELECTOR_CAVE_END,
    )?;
    let mut fixed_routines = vec![
        font_page_routes.routine,
        synchronous_composer,
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
            expected_source_sha1: maximum_dialogue_font_group_selector_range_sha1.to_owned(),
        },
        ReclaimedFixedRuntimeRoutine {
            routine: lifecycle.routine,
            executable_byte_count: lifecycle.executable_byte_count,
            source_end_exclusive: lifecycle::LIFECYCLE_CAVE_END,
            expected_source_sha1: lifecycle::EXPECTED_SAMPLE_LIFECYCLE_SHA1.to_owned(),
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
    };
    plan.new_record_line_buffer_reset_routes_bound()?;
    verify_planned_mapper_select_writes(&plan)?;
    Ok(plan)
}
