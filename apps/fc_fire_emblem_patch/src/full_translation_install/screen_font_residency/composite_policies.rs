//! 원본의 직접 합성 상태 전체가 현재 글꼴 페이지를 어떻게 다루는지 한 표로 소유한다.
//!
//! 번역 표면만 열거하면 나머지 상태는 런타임의 묵시적 기본 분기로 빠진다. 지원 원본은
//! 상태 `02..26`을 모두 직접 생산하므로, 상위 선택기나 appender가 이미 고른 페이지를
//! 유지하는 경우도 명시적인 정책으로 둔다.

use anyhow::{Result, ensure};

use crate::{
    choice_labels::CHOICE_LABEL_COMPOSITE_STATE,
    dialogue_inventory::MAIN_DIALOGUE_COMPOSITE_APPENDER_STATES,
    fixed_menu_labels::{
        GAME_SPEED_SELECTION_COMPOSITE_STATE, STATIC_FONT_PAGE_APPENDER_COMPOSITE_STATES,
        STORAGE_CAPACITY_NOTICE_COMPOSITE_STATE, UNIT_SELECTION_COMPOSITE_STATE,
    },
    front_end_menu::{
        RECORD_ACTION_COMPOSITE_STATE, RECORD_LIST_COMPOSITE_STATE,
        SAVE_SLOT_SELECTION_COMPOSITE_STATE, START_MENU_COMPOSITE_STATE,
    },
    full_translation_install::storage_residency::{
        STORAGE_ACTION_MENU_COMPOSITE_STATE, STORAGE_OVERFLOW_ACTION_COMPOSITE_STATE,
    },
    mapper165::{OPTIONS_FONT_PAGE_COMPOSITE_STATES, ROSTER_FONT_PAGE_COMPOSITE_STATE},
    shop_flow::SHOP_ITEM_COMPOSITE_STATE,
};

use super::STORAGE_DIALOGUE_OVERLAY_COMPOSITE_STATES;
use super::source_page_composites::SOURCE_PAGE_COMPOSITE_STATES;

pub(in crate::full_translation_install) const MAP_MENU_COMPOSITE_STATE: u8 = 0x03;
pub(in crate::full_translation_install) const UNIT_SUMMARY_COMPOSITE_STATE: u8 = 0x04;
pub(in crate::full_translation_install) const UNIT_COMMAND_COMPOSITE_STATE: u8 = 0x05;
pub(in crate::full_translation_install) const ATTACK_WEAPON_SELECTION_COMPOSITE_STATE: u8 = 0x06;
pub(in crate::full_translation_install) const UNIT_ITEM_LIST_COMPOSITE_STATE: u8 = 0x07;
pub(in crate::full_translation_install) const ITEM_ACTION_COMPOSITE_STATE: u8 = 0x09;
pub(in crate::full_translation_install) const UNIT_ACTION_ITEM_COMPOSITE_STATE: u8 = 0x0A;
pub(in crate::full_translation_install) const UNIT_STATUS_COMPOSITE_STATE: u8 = 0x0F;
pub(in crate::full_translation_install) const MAP_FUNDS_COMPOSITE_STATE: u8 = 0x13;
pub(in crate::full_translation_install) const MAP_SUMMARY_COMPOSITE_STATE: u8 = 0x14;
pub(in crate::full_translation_install) const CHAPTER_SAVE_OFFER_COMPOSITE_STATE: u8 = 0x1C;
pub(in crate::full_translation_install) const ITEM_USE_RESULT_COMPOSITE_STATE: u8 = 0x1E;
pub(in crate::full_translation_install) const STORAGE_ITEM_DETAIL_COMPOSITE_STATE: u8 = 0x24;
pub(in crate::full_translation_install) const ITEM_NAME_APPENDER_PUBLISHED_COMPOSITE_STATES: [u8;
    3] = [
    UNIT_ACTION_ITEM_COMPOSITE_STATE,
    ITEM_USE_RESULT_COMPOSITE_STATE,
    STORAGE_ITEM_DETAIL_COMPOSITE_STATE,
];

const DIRECT_COMPOSITE_STATE_START: u8 = 0x02;
const DIRECT_COMPOSITE_STATE_END_INCLUSIVE: u8 = 0x26;
const DIRECT_COMPOSITE_STATE_COUNT: usize =
    (DIRECT_COMPOSITE_STATE_END_INCLUSIVE - DIRECT_COMPOSITE_STATE_START + 1) as usize;
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(in crate::full_translation_install) enum ScreenFontPageRole {
    FrontEndMenu,
    FrontEndRecordAction,
    UnitCommand,
    MapMenu,
    ChapterSaveOffer,
    CatalogDefault,
}

#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub(in crate::full_translation_install) enum DelegatedFontPageOwner {
    ChoiceDialogueResidency,
    MainDialogueRuntimeSelector,
    UnitRosterSelector,
    UnitSelectionAppender,
    GameSpeedAppender,
    OptionsSelector,
    StorageCapacityAppender,
}

impl DelegatedFontPageOwner {
    pub(in crate::full_translation_install) const fn id(self) -> &'static str {
        match self {
            Self::ChoiceDialogueResidency => "choice_dialogue_residency",
            Self::MainDialogueRuntimeSelector => "main_dialogue_runtime_selector",
            Self::UnitRosterSelector => "unit_roster_selector",
            Self::UnitSelectionAppender => "unit_selection_appender",
            Self::GameSpeedAppender => "game_speed_appender",
            Self::OptionsSelector => "options_selector",
            Self::StorageCapacityAppender => "storage_capacity_appender",
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(in crate::full_translation_install) enum ScreenFontResidencyPolicy {
    Static(ScreenFontPageRole),
    StorageDialogueOrStatic(ScreenFontPageRole),
    ItemNamePublishedByAppender,
    UnitOrEnemyNamePublishedByAppender,
    UnitOrEnemyNameRetainedFromSummary,
    CompletedDialoguePageRetained,
    ActiveDialogueCallerRestored,
    /// This standalone screen uses only source-preserved Latin/digits and
    /// explicitly returns both the residency state and the mapper to page zero.
    SourcePageSelected,
    Delegated(DelegatedFontPageOwner),
    /// The central composite-state publisher performs no page write and no
    /// other page owner has yet been admitted for this state. This remains a
    /// visible gap rather than inheriting the previous page by assumption.
    UnresolvedPageOwner,
}

/// The runtime emitter consumes this prefix in its historical order. Keeping
/// that order avoids changing branch layout merely because the coverage table
/// now also names states with no central override.
const NON_DELEGATED_POLICIES: &[(u8, ScreenFontResidencyPolicy)] = &[
    (
        MAP_MENU_COMPOSITE_STATE,
        ScreenFontResidencyPolicy::Static(ScreenFontPageRole::MapMenu),
    ),
    (
        MAP_FUNDS_COMPOSITE_STATE,
        ScreenFontResidencyPolicy::Static(ScreenFontPageRole::MapMenu),
    ),
    (
        MAP_SUMMARY_COMPOSITE_STATE,
        ScreenFontResidencyPolicy::Static(ScreenFontPageRole::MapMenu),
    ),
    (
        UNIT_COMMAND_COMPOSITE_STATE,
        ScreenFontResidencyPolicy::Static(ScreenFontPageRole::UnitCommand),
    ),
    (
        ATTACK_WEAPON_SELECTION_COMPOSITE_STATE,
        ScreenFontResidencyPolicy::Static(ScreenFontPageRole::CatalogDefault),
    ),
    (
        UNIT_ITEM_LIST_COMPOSITE_STATE,
        ScreenFontResidencyPolicy::StorageDialogueOrStatic(ScreenFontPageRole::CatalogDefault),
    ),
    (
        ITEM_ACTION_COMPOSITE_STATE,
        ScreenFontResidencyPolicy::Static(ScreenFontPageRole::CatalogDefault),
    ),
    (
        CHAPTER_SAVE_OFFER_COMPOSITE_STATE,
        ScreenFontResidencyPolicy::Static(ScreenFontPageRole::ChapterSaveOffer),
    ),
    (
        START_MENU_COMPOSITE_STATE,
        ScreenFontResidencyPolicy::Static(ScreenFontPageRole::FrontEndMenu),
    ),
    (
        RECORD_LIST_COMPOSITE_STATE,
        ScreenFontResidencyPolicy::Static(ScreenFontPageRole::FrontEndMenu),
    ),
    (
        SAVE_SLOT_SELECTION_COMPOSITE_STATE,
        ScreenFontResidencyPolicy::Static(ScreenFontPageRole::FrontEndMenu),
    ),
    (
        RECORD_ACTION_COMPOSITE_STATE,
        ScreenFontResidencyPolicy::Static(ScreenFontPageRole::FrontEndRecordAction),
    ),
    (
        UNIT_SUMMARY_COMPOSITE_STATE,
        ScreenFontResidencyPolicy::UnitOrEnemyNamePublishedByAppender,
    ),
    (
        UNIT_STATUS_COMPOSITE_STATE,
        ScreenFontResidencyPolicy::UnitOrEnemyNameRetainedFromSummary,
    ),
    (
        STORAGE_ACTION_MENU_COMPOSITE_STATE,
        ScreenFontResidencyPolicy::CompletedDialoguePageRetained,
    ),
    (
        STORAGE_OVERFLOW_ACTION_COMPOSITE_STATE,
        ScreenFontResidencyPolicy::CompletedDialoguePageRetained,
    ),
    (
        SHOP_ITEM_COMPOSITE_STATE,
        ScreenFontResidencyPolicy::ActiveDialogueCallerRestored,
    ),
    (
        SOURCE_PAGE_COMPOSITE_STATES[0],
        ScreenFontResidencyPolicy::SourcePageSelected,
    ),
    (
        SOURCE_PAGE_COMPOSITE_STATES[1],
        ScreenFontResidencyPolicy::SourcePageSelected,
    ),
    (
        ITEM_NAME_APPENDER_PUBLISHED_COMPOSITE_STATES[0],
        ScreenFontResidencyPolicy::ItemNamePublishedByAppender,
    ),
    (
        ITEM_NAME_APPENDER_PUBLISHED_COMPOSITE_STATES[1],
        ScreenFontResidencyPolicy::ItemNamePublishedByAppender,
    ),
    (
        ITEM_NAME_APPENDER_PUBLISHED_COMPOSITE_STATES[2],
        ScreenFontResidencyPolicy::ItemNamePublishedByAppender,
    ),
];

const DELEGATED_POLICIES: &[(u8, DelegatedFontPageOwner)] = &[
    (
        CHOICE_LABEL_COMPOSITE_STATE,
        DelegatedFontPageOwner::ChoiceDialogueResidency,
    ),
    (
        MAIN_DIALOGUE_COMPOSITE_APPENDER_STATES[0],
        DelegatedFontPageOwner::MainDialogueRuntimeSelector,
    ),
    (
        ROSTER_FONT_PAGE_COMPOSITE_STATE,
        DelegatedFontPageOwner::UnitRosterSelector,
    ),
    (
        UNIT_SELECTION_COMPOSITE_STATE,
        DelegatedFontPageOwner::UnitSelectionAppender,
    ),
    (
        OPTIONS_FONT_PAGE_COMPOSITE_STATES[0],
        DelegatedFontPageOwner::OptionsSelector,
    ),
    (
        GAME_SPEED_SELECTION_COMPOSITE_STATE,
        DelegatedFontPageOwner::GameSpeedAppender,
    ),
    (
        OPTIONS_FONT_PAGE_COMPOSITE_STATES[1],
        DelegatedFontPageOwner::OptionsSelector,
    ),
    (
        MAIN_DIALOGUE_COMPOSITE_APPENDER_STATES[1],
        DelegatedFontPageOwner::MainDialogueRuntimeSelector,
    ),
    (
        MAIN_DIALOGUE_COMPOSITE_APPENDER_STATES[2],
        DelegatedFontPageOwner::MainDialogueRuntimeSelector,
    ),
    (
        STORAGE_CAPACITY_NOTICE_COMPOSITE_STATE,
        DelegatedFontPageOwner::StorageCapacityAppender,
    ),
];

const fn build_direct_state_policies()
-> [(u8, ScreenFontResidencyPolicy); DIRECT_COMPOSITE_STATE_COUNT] {
    let mut policies = [(
        DIRECT_COMPOSITE_STATE_START,
        ScreenFontResidencyPolicy::UnresolvedPageOwner,
    ); DIRECT_COMPOSITE_STATE_COUNT];
    let mut policy_index = 0;
    while policy_index < NON_DELEGATED_POLICIES.len() {
        policies[policy_index] = NON_DELEGATED_POLICIES[policy_index];
        policy_index += 1;
    }
    let mut state = DIRECT_COMPOSITE_STATE_START;
    while state <= DIRECT_COMPOSITE_STATE_END_INCLUSIVE {
        let mut override_index = 0;
        let mut has_central_override = false;
        while override_index < NON_DELEGATED_POLICIES.len() {
            if NON_DELEGATED_POLICIES[override_index].0 == state {
                has_central_override = true;
            }
            override_index += 1;
        }
        if !has_central_override {
            let mut delegated_index = 0;
            let mut policy = ScreenFontResidencyPolicy::UnresolvedPageOwner;
            while delegated_index < DELEGATED_POLICIES.len() {
                if DELEGATED_POLICIES[delegated_index].0 == state {
                    policy =
                        ScreenFontResidencyPolicy::Delegated(DELEGATED_POLICIES[delegated_index].1);
                }
                delegated_index += 1;
            }
            policies[policy_index] = (state, policy);
            policy_index += 1;
        }
        state += 1;
    }
    policies
}

/// 지원 원본에서 직접 생산되는 `02..26` 상태의 전수 정책이다. 고정 표면은 진입
/// 즉시 페이지를 게시하고, 중앙 override가 없는 상태는 상위 동적 선택기나 대사
/// appender의 소유권을 덮지 않는다. 후자는 해당 화면이 번역 글꼴을 요구하지 않는다는
/// 완전성 주장이 아니다.
pub(in crate::full_translation_install) const COMPOSITE_FONT_RESIDENCY_POLICIES: [(
    u8,
    ScreenFontResidencyPolicy,
);
    DIRECT_COMPOSITE_STATE_COUNT] = build_direct_state_policies();

pub(in crate::full_translation_install) fn composite_font_residency_policy(
    state: u8,
) -> Option<ScreenFontResidencyPolicy> {
    COMPOSITE_FONT_RESIDENCY_POLICIES
        .iter()
        .find_map(|(candidate, policy)| (*candidate == state).then_some(*policy))
}

impl ScreenFontResidencyPolicy {
    pub(in crate::full_translation_install) fn static_page(self) -> Option<ScreenFontPageRole> {
        match self {
            Self::Static(page) | Self::StorageDialogueOrStatic(page) => Some(page),
            Self::ItemNamePublishedByAppender
            | Self::UnitOrEnemyNamePublishedByAppender
            | Self::UnitOrEnemyNameRetainedFromSummary
            | Self::CompletedDialoguePageRetained
            | Self::ActiveDialogueCallerRestored
            | Self::SourcePageSelected
            | Self::Delegated(_)
            | Self::UnresolvedPageOwner => None,
        }
    }
}

pub(super) fn validate_composite_state_policies() -> Result<()> {
    let states = COMPOSITE_FONT_RESIDENCY_POLICIES
        .iter()
        .map(|(state, _)| *state)
        .collect::<std::collections::BTreeSet<_>>();
    ensure!(
        states.len() == COMPOSITE_FONT_RESIDENCY_POLICIES.len()
            && states
                == (DIRECT_COMPOSITE_STATE_START..=DIRECT_COMPOSITE_STATE_END_INCLUSIVE).collect(),
        "screen font residency does not cover every direct composite state exactly once"
    );
    let delegated = COMPOSITE_FONT_RESIDENCY_POLICIES
        .iter()
        .filter_map(|(state, policy)| match policy {
            ScreenFontResidencyPolicy::Delegated(owner) => Some((*state, *owner)),
            _ => None,
        })
        .collect::<Vec<_>>();
    ensure!(
        delegated == DELEGATED_POLICIES,
        "delegated font-page ownership changed"
    );
    let main_dialogue_states = delegated
        .iter()
        .filter_map(|(state, owner)| {
            (*owner == DelegatedFontPageOwner::MainDialogueRuntimeSelector).then_some(*state)
        })
        .collect::<Vec<_>>();
    ensure!(
        main_dialogue_states == MAIN_DIALOGUE_COMPOSITE_APPENDER_STATES,
        "main-dialogue composite state ownership changed"
    );
    let delegated_fixed_menu_states = delegated
        .iter()
        .filter_map(|(state, owner)| {
            matches!(
                owner,
                DelegatedFontPageOwner::UnitSelectionAppender
                    | DelegatedFontPageOwner::GameSpeedAppender
                    | DelegatedFontPageOwner::StorageCapacityAppender
            )
            .then_some(*state)
        })
        .collect::<Vec<_>>();
    ensure!(
        delegated_fixed_menu_states == STATIC_FONT_PAGE_APPENDER_COMPOSITE_STATES,
        "fixed-menu appender state ownership changed"
    );
    ensure!(
        composite_font_residency_policy(UNIT_ITEM_LIST_COMPOSITE_STATE)
            == Some(ScreenFontResidencyPolicy::StorageDialogueOrStatic(
                ScreenFontPageRole::CatalogDefault,
            )),
        "item-list font residency no longer distinguishes the storage dialogue lifetime from the standalone catalog page"
    );
    ensure!(
        COMPOSITE_FONT_RESIDENCY_POLICIES
            .iter()
            .filter_map(|(state, policy)| {
                (*policy == ScreenFontResidencyPolicy::ItemNamePublishedByAppender)
                    .then_some(*state)
            })
            .collect::<Vec<_>>()
            == ITEM_NAME_APPENDER_PUBLISHED_COMPOSITE_STATES,
        "item-name appender composite state ownership changed"
    );
    ensure!(
        composite_font_residency_policy(UNIT_SUMMARY_COMPOSITE_STATE)
            == Some(ScreenFontResidencyPolicy::UnitOrEnemyNamePublishedByAppender),
        "unit-summary font residency no longer delegates page publication to its name appender"
    );
    ensure!(
        composite_font_residency_policy(UNIT_STATUS_COMPOSITE_STATE)
            == Some(ScreenFontResidencyPolicy::UnitOrEnemyNameRetainedFromSummary),
        "unit-status font residency no longer retains the page published by unit summary"
    );
    let completed_dialogue_states = COMPOSITE_FONT_RESIDENCY_POLICIES
        .iter()
        .filter_map(|(state, policy)| {
            (*policy == ScreenFontResidencyPolicy::CompletedDialoguePageRetained).then_some(*state)
        })
        .collect::<Vec<_>>();
    ensure!(
        completed_dialogue_states == STORAGE_DIALOGUE_OVERLAY_COMPOSITE_STATES,
        "completed-dialogue font residency no longer matches the source-bound storage overlay states"
    );
    ensure!(
        composite_font_residency_policy(SHOP_ITEM_COMPOSITE_STATE)
            == Some(ScreenFontResidencyPolicy::ActiveDialogueCallerRestored),
        "shop item composition no longer restores the active E7 dialogue page"
    );
    let source_page_states = COMPOSITE_FONT_RESIDENCY_POLICIES
        .iter()
        .filter_map(|(state, policy)| {
            (*policy == ScreenFontResidencyPolicy::SourcePageSelected).then_some(*state)
        })
        .collect::<Vec<_>>();
    ensure!(
        source_page_states == SOURCE_PAGE_COMPOSITE_STATES,
        "source-page composite state ownership changed"
    );
    Ok(())
}
