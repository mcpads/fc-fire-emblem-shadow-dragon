//! 전투 후 결과와 원본 영문 독립 합성 화면의 페이지 수명을 결속한다.
//!
//! 상태 `08`은 전투 후 EXP/LEVEL UP 결과이고, 상태 `10`은 `NEXT STORY` 전환이다.
//! 둘 다 직접 쓰는 문자열은 고정 문자열 소유권 장부에서 원본 보존 대상으로 분류되지만,
//! 상태 `08`은 직전 전투가 합성한 한글 CHR 페이지를 결과 표시까지 유지해야 한다. 상태
//! `10`만 독립 원본 화면으로서 페이지 0을 선택한다.

use anyhow::{Result, ensure};

use crate::{
    fixed_string_consumers::{
        CompositeStateProducer, FixedStringCallSite, FixedStringConsumerInspection,
    },
    fixed_string_ownership::is_preserved_fixed_string_index,
    japanese_encoding::is_japanese_text_code,
    mapper165::{
        inline_pointer_dispatch::bind_inline_pointer_dispatch,
        source_code_binding::bind_hashed_switchable_code_without_mmc4_writes,
    },
    rom::Rom,
};

const GAMEPLAY_PRG_BANK: u8 = 0x06;
const FIXED_STRING_PRG_BANK: u8 = 0x0B;

pub(super) const POST_BATTLE_RESULT_RETAINED_COMPOSITE_STATE: u8 = 0x08;
pub(super) const NEXT_STORY_COMPOSITE_STATE: u8 = 0x10;
pub(super) const SOURCE_PAGE_COMPOSITE_STATES: [u8; 1] = [NEXT_STORY_COMPOSITE_STATE];
const BOUND_SOURCE_COMPOSITE_STATES: [u8; 2] = [
    POST_BATTLE_RESULT_RETAINED_COMPOSITE_STATE,
    NEXT_STORY_COMPOSITE_STATE,
];

const POST_BATTLE_MAIN_DISPATCH_CALL: u16 = 0x8964;
const POST_BATTLE_MAIN_HANDLERS: [u16; 2] = [0xA15F, 0xA18D];
const POST_BATTLE_LIFETIME_END: u16 = 0xA1BC;
const POST_BATTLE_LIFETIME_SHA1: &str = "58c85a33e4c74edd034b68e14fbb7048e90e6d27";

const CHAPTER_SAVE_DISPATCH_CALL: u16 = 0xB5B1;
const CHAPTER_SAVE_HANDLERS: [u16; 2] = [0xB6C9, 0xB6E9];
const CHAPTER_SAVE_LIFETIME_END: u16 = 0xB6F3;
const CHAPTER_SAVE_LIFETIME_SHA1: &str = "8eb21e06dde4e60d2a83fdfa7bc7ec27566e2379";

const POST_BATTLE_COMPOSER: u16 = 0x85E5;
const POST_BATTLE_COMPOSER_END: u16 = 0x8613;
const POST_BATTLE_COMPOSER_SHA1: &str = "41df3865b183066f216b79488e7679c70175233e";
const NEXT_STORY_COMPOSER: u16 = 0x886A;
const NEXT_STORY_COMPOSER_END: u16 = 0x8891;
const NEXT_STORY_COMPOSER_SHA1: &str = "c383220ea27204ce7c663f40553d2c665f064e7f";

const EXPECTED_PRODUCERS: [CompositeStateProducer; 2] = [
    CompositeStateProducer {
        prg_bank: GAMEPLAY_PRG_BANK,
        cpu_address: 0xA180,
        transfer_opcode: 0x20,
        state: POST_BATTLE_RESULT_RETAINED_COMPOSITE_STATE,
    },
    CompositeStateProducer {
        prg_bank: GAMEPLAY_PRG_BANK,
        cpu_address: 0xB6E5,
        transfer_opcode: 0x4C,
        state: NEXT_STORY_COMPOSITE_STATE,
    },
];

#[derive(Clone, Debug, Eq, PartialEq)]
pub(super) struct BoundPostBattleResultAndNextStoryLifetimes {
    states: [u8; 1],
    preserved_fixed_string_indices: [u8; 3],
    producer_count: usize,
}

impl BoundPostBattleResultAndNextStoryLifetimes {
    pub(super) const fn states(&self) -> [u8; 1] {
        self.states
    }

    pub(super) const fn preserved_fixed_string_indices(&self) -> [u8; 3] {
        self.preserved_fixed_string_indices
    }

    pub(super) const fn producer_count(&self) -> usize {
        self.producer_count
    }
}

pub(super) fn bind_post_battle_result_and_next_story_lifetimes(
    source: &Rom,
    fixed_strings: &FixedStringConsumerInspection,
) -> Result<BoundPostBattleResultAndNextStoryLifetimes> {
    source.verify_supported_japanese()?;

    let post_battle_dispatch = bind_inline_pointer_dispatch(
        source,
        GAMEPLAY_PRG_BANK,
        POST_BATTLE_MAIN_DISPATCH_CALL,
        [0x20, 0x21],
        "post-battle result main-state dispatch",
    )?;
    ensure!(
        post_battle_dispatch.targets_in_selector_order() == POST_BATTLE_MAIN_HANDLERS,
        "post-battle result main-state lifetime changed"
    );
    bind_hashed_switchable_code_without_mmc4_writes(
        source,
        GAMEPLAY_PRG_BANK,
        POST_BATTLE_MAIN_HANDLERS[0],
        POST_BATTLE_LIFETIME_END,
        POST_BATTLE_LIFETIME_SHA1,
        "post-battle result main-state lifetime",
    )?;

    let chapter_save_dispatch = bind_inline_pointer_dispatch(
        source,
        GAMEPLAY_PRG_BANK,
        CHAPTER_SAVE_DISPATCH_CALL,
        [0x02, 0x03],
        "next-story chapter-save main-state dispatch",
    )?;
    ensure!(
        chapter_save_dispatch.targets_in_selector_order() == CHAPTER_SAVE_HANDLERS,
        "next-story chapter-save main-state lifetime changed"
    );
    bind_hashed_switchable_code_without_mmc4_writes(
        source,
        GAMEPLAY_PRG_BANK,
        CHAPTER_SAVE_HANDLERS[0],
        CHAPTER_SAVE_LIFETIME_END,
        CHAPTER_SAVE_LIFETIME_SHA1,
        "next-story chapter-save main-state lifetime",
    )?;

    ensure!(
        fixed_strings.composite_handler_target(POST_BATTLE_RESULT_RETAINED_COMPOSITE_STATE)
            == Some(POST_BATTLE_COMPOSER)
            && fixed_strings
                .composite_handler_target(POST_BATTLE_RESULT_RETAINED_COMPOSITE_STATE + 1,)
                == Some(POST_BATTLE_COMPOSER_END),
        "post-battle result composite handler range changed"
    );
    bind_hashed_switchable_code_without_mmc4_writes(
        source,
        FIXED_STRING_PRG_BANK,
        POST_BATTLE_COMPOSER,
        POST_BATTLE_COMPOSER_END,
        POST_BATTLE_COMPOSER_SHA1,
        "post-battle result composite handler",
    )?;
    ensure!(
        fixed_strings.composite_handler_target(NEXT_STORY_COMPOSITE_STATE)
            == Some(NEXT_STORY_COMPOSER)
            && fixed_strings.composite_handler_target(NEXT_STORY_COMPOSITE_STATE + 1)
                == Some(NEXT_STORY_COMPOSER_END),
        "next-story composite handler range changed"
    );
    bind_hashed_switchable_code_without_mmc4_writes(
        source,
        FIXED_STRING_PRG_BANK,
        NEXT_STORY_COMPOSER,
        NEXT_STORY_COMPOSER_END,
        NEXT_STORY_COMPOSER_SHA1,
        "next-story composite handler",
    )?;

    let producers = fixed_strings
        .composite_state_producers
        .iter()
        .copied()
        .filter(|producer| BOUND_SOURCE_COMPOSITE_STATES.contains(&producer.state))
        .collect::<Vec<_>>();
    ensure!(
        producers == EXPECTED_PRODUCERS,
        "source-page composite producer family changed: {producers:02X?}"
    );
    let calls = fixed_strings
        .call_sites
        .iter()
        .filter(|call| BOUND_SOURCE_COMPOSITE_STATES.contains(&call.composite_state))
        .collect::<Vec<_>>();
    ensure_bound_composite_calls(&calls)?;

    let preserved_fixed_string_indices = [0x0B, 0x12, 0x3E];
    for index in preserved_fixed_string_indices {
        ensure!(
            is_preserved_fixed_string_index(index),
            "source-page fixed string {index:02X} lost its preservation owner"
        );
        let record = fixed_strings
            .records
            .iter()
            .find(|record| record.index == index)
            .ok_or_else(|| anyhow::anyhow!("source-page fixed string {index:02X} is missing"))?;
        ensure!(
            !record
                .source_bytes
                .iter()
                .copied()
                .any(is_japanese_text_code),
            "source-page fixed string {index:02X} gained Japanese text"
        );
    }

    Ok(BoundPostBattleResultAndNextStoryLifetimes {
        states: SOURCE_PAGE_COMPOSITE_STATES,
        preserved_fixed_string_indices,
        producer_count: producers.len(),
    })
}

fn ensure_bound_composite_calls(calls: &[&FixedStringCallSite]) -> Result<()> {
    let identity = calls
        .iter()
        .map(|call| {
            (
                call.cpu_address,
                call.composite_state,
                call.possible_indices.as_slice(),
            )
        })
        .collect::<Vec<_>>();
    ensure!(
        identity
            == [
                (
                    0x8601,
                    POST_BATTLE_RESULT_RETAINED_COMPOSITE_STATE,
                    &[0x0B][..],
                ),
                (
                    0x8608,
                    POST_BATTLE_RESULT_RETAINED_COMPOSITE_STATE,
                    &[0x12][..],
                ),
                (0x8886, NEXT_STORY_COMPOSITE_STATE, &[0x3E][..]),
            ],
        "source-page composite fixed-string consumers changed: {identity:?}"
    );
    Ok(())
}
