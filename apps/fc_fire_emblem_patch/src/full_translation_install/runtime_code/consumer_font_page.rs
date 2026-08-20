//! 비대사 복합 UI가 열려 있는 동안의 글꼴 페이지를 한 수명으로 관리한다.
//!
//! `$05E8`은 현재 화면이 아니라 마지막으로 요청한 합성기 ID다. 이를 전역 CHR
//! selector에서 계속 읽으면 화면을 닫은 뒤에도 예전 UI가 지형·전투·제목 그래픽을
//! 가로챈다. 반대로 `$E690`의 게시를 «다음 CHR 기록기 한 번»으로만 해석하면 같은
//! 화면의 커서 이동 재합성이 페이지를 다시 게시한 뒤 닫기 기록기를 가로챈다.
//!
//! 이 모듈은 그 두 수명을 분리한다. `$E690`과 이름 appender는 `$07FD`에 현재 페이지와
//! FE 소유 비트를 게시하고 즉시 적용한다. bank 0B의 유일한 복합 UI 열기 호출은 같은
//! route를 다시 적용하되 게시값을 소비하지 않는다. 전역 selector도 수명 동안 이 값을
//! 우선하므로 커서 이동이나 원본 FD/FE 갱신 뒤에 번역 페이지가 사라지지 않는다. 유일한
//! 닫기 호출만 게시값을 지우고 원본 그래픽 페이지 복원을 그대로 수행한다.

use std::collections::BTreeSet;

use anyhow::{Context, Result, ensure};

pub(super) mod ending_lifetime;

use super::{
    DialogueRuntimeHook, DialogueRuntimeHookRole, DialogueRuntimeHookSite, RuntimeRoutine,
    chr_source_state::RIGHT_FD_SOURCE_SHADOW, next_address,
};
use crate::{
    fixed_string_consumers::{
        CompositeStateProducer, bind_direct_composite_state_producer_catalog,
        scan_direct_composite_state_producers,
    },
    front_end_menu::{
        RECORD_ACTION_COMPOSITE_STATE, RECORD_LIST_COMPOSITE_STATE,
        SAVE_SLOT_SELECTION_COMPOSITE_STATE, START_MENU_COMPOSITE_STATE,
    },
    full_translation_install::{
        runtime_state_storage::CONSUMER_FONT_PAGE,
        screen_font_residency::{
            ATTACK_WEAPON_SELECTION_COMPOSITE_STATE, CHAPTER_SAVE_OFFER_COMPOSITE_STATE,
            COMPOSITE_FONT_RESIDENCY_POLICIES, ITEM_ACTION_COMPOSITE_STATE,
            MAP_FUNDS_COMPOSITE_STATE, MAP_MENU_COMPOSITE_STATE, MAP_SUMMARY_COMPOSITE_STATE,
            ScreenFontPageRole, ScreenFontPageRoutes, UNIT_COMMAND_COMPOSITE_STATE,
            UNIT_ITEM_LIST_COMPOSITE_STATE, UNIT_STATUS_COMPOSITE_STATE,
            UNIT_SUMMARY_COMPOSITE_STATE,
        },
        storage_residency::{
            STORAGE_ACTION_MENU_COMPOSITE_STATE, STORAGE_OVERFLOW_ACTION_COMPOSITE_STATE,
            StorageItemListRuntimeRoute,
        },
    },
    rom::Rom,
    rp2a03::{Instruction, assemble_at},
    shop_flow::SHOP_ITEM_COMPOSITE_STATE,
    typed_source::decode_rp2a03_sequence,
    unit_ui_text::bind_unit_summary_status_page_inheritance_source,
};

const FIXED_BANK_BYTE_COUNT: usize = 16 * 1024;
const UNIT_UI_BANK: u8 = 0x0B;

pub(super) const COMPOSITE_STATE: u16 = 0x05E8;
const COMPOSITE_PAGE_ENTRY: u16 = 0xE690;
const COMPOSITE_PAGE_ENTRY_SOURCE: [u8; 12] = [
    0x8D, 0xE8, 0x05, 0xA9, 0x01, 0x85, 0x44, 0xA9, 0x0B, 0x4C, 0xFA, 0xC9,
];

const CENTRAL_RIGHT_FD_WRITER: u16 = 0xC9BE;
const APPEND_FIXED_STRING: u16 = 0x8EEE;
pub(super) const FIXED_MENU_FONT_PAGE_APPENDER_ORIGIN: u16 = 0xBA6B;
const FIXED_MENU_FONT_PAGE_APPENDER_END: u16 = 0xBA75;
const JSR_ABSOLUTE_OPCODE: u8 = 0x20;
const JMP_ABSOLUTE_OPCODE: u8 = 0x4C;
const SCREEN_OPEN_RIGHT_FD_CALL: u16 = 0x928A;
const SCREEN_CLOSE_RIGHT_FD_CALL: u16 = 0x9324;
const SCREEN_OPEN_SEQUENCE_ADDRESS: u16 = 0x927B;
const SCREEN_OPEN_SEQUENCE: [u8; 18] = [
    0xA9, 0x06, 0x85, 0x44, 0x20, 0xFA, 0xC9, 0x20, 0xF5, 0xE6, 0x20, 0x0D, 0xC7, 0xA9, 0x00, 0x20,
    0xBE, 0xC9,
];
const SCREEN_CLOSE_SEQUENCE_ADDRESS: u16 = 0x931C;
const SCREEN_CLOSE_SEQUENCE: [u8; 14] = [
    0x20, 0x0D, 0xC7, 0xA4, 0x99, 0xB9, 0xE4, 0xC1, 0x20, 0xBE, 0xC9, 0x20, 0x0C, 0xE7,
];
const GAMEPLAY_HANDOFF_SEQUENCE_ADDRESS: u16 = 0xF302;
const GAMEPLAY_HANDOFF_SEQUENCE: [u8; 8] = [0xA9, 0x00, 0x85, 0x23, 0x85, 0x24, 0x85, 0x84];
const GAMEPLAY_HANDOFF_HOOK_ADDRESS: u16 = 0xF304;
const GAMEPLAY_PHASE_LOW: u8 = 0x23;
const GAMEPLAY_PHASE_HIGH: u8 = 0x24;

/// Each site is the first fixed-menu label append on its execution path. The optional hook role
/// distinguishes a standalone fixed-label screen from a label drawn over live main dialogue.
/// Storage action and overflow labels must retain the dialogue route selected for the underlying
/// record, while the storage-capacity notice is a standalone fixed-label screen.
const FIXED_MENU_FONT_PAGE_CALLS: [(u16, u8, Option<DialogueRuntimeHookRole>, &'static str); 6] = [
    (
        0x8A3C,
        0x2C,
        Some(DialogueRuntimeHookRole::FixedMenuUnitSelectionAppender),
        "unit-selection fixed-menu font-page hook",
    ),
    (
        0x8A6D,
        0x30,
        Some(DialogueRuntimeHookRole::FixedMenuFastSpeedAppender),
        "fast-speed fixed-menu font-page hook",
    ),
    (
        0x8A7A,
        0x31,
        Some(DialogueRuntimeHookRole::FixedMenuSlowSpeedAppender),
        "slow-speed fixed-menu font-page hook",
    ),
    (
        0x8B1D,
        0x35,
        None,
        "storage-action fixed-menu font-page hook",
    ),
    (
        0x8DA8,
        0x35,
        None,
        "storage-overflow fixed-menu font-page hook",
    ),
    (
        0x8E31,
        0x47,
        Some(DialogueRuntimeHookRole::FixedMenuStorageCapacityAppender),
        "storage-capacity fixed-menu font-page hook",
    ),
];
const EXPECTED_FONT_PAGE_STATE_PRODUCERS: [CompositeStateProducer; 22] = [
    CompositeStateProducer::new(0x02, 0xA693, 0x4C, START_MENU_COMPOSITE_STATE),
    CompositeStateProducer::new(0x02, 0xA6CC, 0x4C, SAVE_SLOT_SELECTION_COMPOSITE_STATE),
    CompositeStateProducer::new(0x02, 0xA6D5, 0x4C, RECORD_LIST_COMPOSITE_STATE),
    CompositeStateProducer::new(0x02, 0xA6DE, 0x4C, RECORD_LIST_COMPOSITE_STATE),
    CompositeStateProducer::new(0x02, 0xA6E7, 0x4C, RECORD_LIST_COMPOSITE_STATE),
    CompositeStateProducer::new(0x02, 0xA79A, 0x20, RECORD_ACTION_COMPOSITE_STATE),
    CompositeStateProducer::new(0x06, 0x882D, 0x20, UNIT_SUMMARY_COMPOSITE_STATE),
    CompositeStateProducer::new(0x06, 0x8F1A, 0x20, UNIT_SUMMARY_COMPOSITE_STATE),
    CompositeStateProducer::new(0x06, 0x903C, 0x20, UNIT_COMMAND_COMPOSITE_STATE),
    CompositeStateProducer::new(0x06, 0x90AF, 0x20, ATTACK_WEAPON_SELECTION_COMPOSITE_STATE),
    CompositeStateProducer::new(0x06, 0x93E2, 0x4C, UNIT_ITEM_LIST_COMPOSITE_STATE),
    CompositeStateProducer::new(0x06, 0x941D, 0x20, ITEM_ACTION_COMPOSITE_STATE),
    CompositeStateProducer::new(0x06, 0x9A07, 0x20, SHOP_ITEM_COMPOSITE_STATE),
    CompositeStateProducer::new(0x06, 0x9E12, 0x4C, STORAGE_ACTION_MENU_COMPOSITE_STATE),
    CompositeStateProducer::new(0x06, 0x9EB4, 0x20, UNIT_ITEM_LIST_COMPOSITE_STATE),
    CompositeStateProducer::new(0x06, 0xA0BE, 0x20, MAP_FUNDS_COMPOSITE_STATE),
    CompositeStateProducer::new(0x06, 0xA30D, 0x4C, MAP_MENU_COMPOSITE_STATE),
    CompositeStateProducer::new(0x06, 0xAF0C, 0x20, UNIT_STATUS_COMPOSITE_STATE),
    CompositeStateProducer::new(0x06, 0xB17F, 0x4C, STORAGE_OVERFLOW_ACTION_COMPOSITE_STATE),
    CompositeStateProducer::new(0x06, 0xB40B, 0x4C, MAP_SUMMARY_COMPOSITE_STATE),
    CompositeStateProducer::new(0x06, 0xB413, 0x4C, MAP_FUNDS_COMPOSITE_STATE),
    CompositeStateProducer::new(0x06, 0xB78A, 0x4C, CHAPTER_SAVE_OFFER_COMPOSITE_STATE),
];

mod runtime_emission;
mod source_binding;

pub(super) use runtime_emission::{
    build_composite_font_page_publisher, build_consumer_font_page_activation,
    build_consumer_font_page_close, build_consumer_font_page_gameplay_handoff,
    build_consumer_font_page_open, build_fixed_menu_font_page_appender,
    fixed_menu_font_page_appender_installation, fixed_menu_font_page_hooks, gameplay_handoff_hook,
    page_publisher_hook, screen_lifetime_hooks,
};
pub(super) use source_binding::bind_consumer_font_page_lifetime;

#[cfg(test)]
mod tests;
