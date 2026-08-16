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
    },
    rom::Rom,
    rp2a03::{Instruction, assemble_at},
    typed_source::decode_rp2a03_sequence,
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

/// Each site is the first fixed-menu label append on its execution path.  The speed selector has
/// two mutually exclusive paths, so both calls are hooked.  The storage screens append their
/// remaining labels linearly after the listed call and retain the selected page for the screen
/// lifetime.
const FIXED_MENU_FONT_PAGE_CALLS: [(u16, u8, DialogueRuntimeHookRole, &'static str); 6] = [
    (
        0x8A3C,
        0x2C,
        DialogueRuntimeHookRole::FixedMenuUnitSelectionAppender,
        "unit-selection fixed-menu font-page hook",
    ),
    (
        0x8A6D,
        0x30,
        DialogueRuntimeHookRole::FixedMenuFastSpeedAppender,
        "fast-speed fixed-menu font-page hook",
    ),
    (
        0x8A7A,
        0x31,
        DialogueRuntimeHookRole::FixedMenuSlowSpeedAppender,
        "slow-speed fixed-menu font-page hook",
    ),
    (
        0x8B1D,
        0x35,
        DialogueRuntimeHookRole::FixedMenuStorageActionAppender,
        "storage-action fixed-menu font-page hook",
    ),
    (
        0x8DA8,
        0x35,
        DialogueRuntimeHookRole::FixedMenuStorageOverflowAppender,
        "storage-overflow fixed-menu font-page hook",
    ),
    (
        0x8E31,
        0x47,
        DialogueRuntimeHookRole::FixedMenuStorageCapacityAppender,
        "storage-capacity fixed-menu font-page hook",
    ),
];
const EXPECTED_FONT_PAGE_STATE_PRODUCERS: [CompositeStateProducer; 19] = [
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
    CompositeStateProducer::new(0x06, 0x9EB4, 0x20, UNIT_ITEM_LIST_COMPOSITE_STATE),
    CompositeStateProducer::new(0x06, 0xA0BE, 0x20, MAP_FUNDS_COMPOSITE_STATE),
    CompositeStateProducer::new(0x06, 0xA30D, 0x4C, MAP_MENU_COMPOSITE_STATE),
    CompositeStateProducer::new(0x06, 0xAF0C, 0x20, UNIT_STATUS_COMPOSITE_STATE),
    CompositeStateProducer::new(0x06, 0xB40B, 0x4C, MAP_SUMMARY_COMPOSITE_STATE),
    CompositeStateProducer::new(0x06, 0xB413, 0x4C, MAP_FUNDS_COMPOSITE_STATE),
    CompositeStateProducer::new(0x06, 0xB78A, 0x4C, CHAPTER_SAVE_OFFER_COMPOSITE_STATE),
];

/// 합성 진입과 화면 열기·닫기의 중앙 FD 공급을 원본과 후보 양쪽에 묶는다.
/// bank 0B에서 `$C9BE`를 직접 부르는 곳이 이 두 자리뿐이어야, 열기만 현재 UI
/// 페이지를 적용하고 닫기는 원본 selector 사슬로 복귀한다는 분모가 닫힌다.
pub(super) fn bind_consumer_font_page_lifetime(source: &Rom, candidate: &Rom) -> Result<()> {
    bind_direct_composite_state_producers(source, candidate)?;
    bind_fixed_menu_font_page_appender_cave(source, candidate)?;
    for (image_role, rom) in [("source", source), ("candidate", candidate)] {
        for (call, index, _, role) in FIXED_MENU_FONT_PAGE_CALLS {
            let producer = call
                .checked_sub(2)
                .context("fixed-menu appender producer address underflow")?;
            let expected = [0xA9, index, 0x20, 0xEE, 0x8E];
            let actual = switchable_slice(rom, UNIT_UI_BANK, producer, expected.len())?;
            ensure!(
                actual == expected,
                "{image_role} {role} changed at {UNIT_UI_BANK:02X}:{producer:04X}"
            );
            decode_rp2a03_sequence(
                actual,
                producer,
                "load and append the first fixed-menu label on one execution path",
            )?;
        }

        let entry = fixed_slice(rom, COMPOSITE_PAGE_ENTRY, COMPOSITE_PAGE_ENTRY_SOURCE.len())?;
        ensure!(
            entry == COMPOSITE_PAGE_ENTRY_SOURCE,
            "{image_role} composite page entry changed at {COMPOSITE_PAGE_ENTRY:04X}"
        );
        decode_rp2a03_sequence(
            entry,
            COMPOSITE_PAGE_ENTRY,
            "publish and dispatch one composite request",
        )?;

        for (address, expected, role) in [
            (
                SCREEN_OPEN_SEQUENCE_ADDRESS,
                SCREEN_OPEN_SEQUENCE.as_slice(),
                "open one composite screen and supply its central right-FD page",
            ),
            (
                SCREEN_CLOSE_SEQUENCE_ADDRESS,
                SCREEN_CLOSE_SEQUENCE.as_slice(),
                "close one composite screen and restore its central right-FD page",
            ),
        ] {
            let actual = switchable_slice(rom, UNIT_UI_BANK, address, expected.len())?;
            ensure!(
                actual == expected,
                "{image_role} {role} changed at {UNIT_UI_BANK:02X}:{address:04X}"
            );
            decode_rp2a03_sequence(actual, address, role)?;
        }

        let gameplay_handoff = fixed_slice(
            rom,
            GAMEPLAY_HANDOFF_SEQUENCE_ADDRESS,
            GAMEPLAY_HANDOFF_SEQUENCE.len(),
        )?;
        ensure!(
            gameplay_handoff == GAMEPLAY_HANDOFF_SEQUENCE,
            "{image_role} gameplay initialization handoff changed at {GAMEPLAY_HANDOFF_SEQUENCE_ADDRESS:04X}"
        );
        decode_rp2a03_sequence(
            gameplay_handoff,
            GAMEPLAY_HANDOFF_SEQUENCE_ADDRESS,
            "clear the front-end font lifetime while initializing gameplay state",
        )?;

        let transfers = direct_transfer_sites_in_bank(rom, UNIT_UI_BANK, CENTRAL_RIGHT_FD_WRITER)?;
        ensure!(
            transfers
                == [
                    (SCREEN_OPEN_RIGHT_FD_CALL, JSR_ABSOLUTE_OPCODE),
                    (SCREEN_CLOSE_RIGHT_FD_CALL, JSR_ABSOLUTE_OPCODE),
                ],
            "{image_role} bank {UNIT_UI_BANK:02X} central right-FD direct-transfer census changed: {transfers:?}"
        );
    }
    Ok(())
}

/// Bank 0B is already selected while its composite handlers append fixed labels.  Keeping this
/// ten-byte wrapper in that bank avoids spending scarce fixed-bank selector space.  The selected
/// range is exact FF in both inputs and no adjacent little-endian source word names any address in
/// it; a future table, call, or data owner therefore fails this admission instead of being covered.
fn bind_fixed_menu_font_page_appender_cave(source: &Rom, candidate: &Rom) -> Result<()> {
    let length =
        usize::from(FIXED_MENU_FONT_PAGE_APPENDER_END - FIXED_MENU_FONT_PAGE_APPENDER_ORIGIN);
    for (image_role, rom) in [("source", source), ("candidate", candidate)] {
        let bytes = switchable_slice(
            rom,
            UNIT_UI_BANK,
            FIXED_MENU_FONT_PAGE_APPENDER_ORIGIN,
            length,
        )?;
        ensure!(
            bytes.iter().all(|byte| *byte == 0xFF),
            "{image_role} fixed-menu font-page appender cave is not exact FF"
        );
    }

    let bank_start = usize::from(UNIT_UI_BANK) * FIXED_BANK_BYTE_COUNT;
    let bank = source
        .prg()
        .get(bank_start..bank_start + FIXED_BANK_BYTE_COUNT)
        .context("source bank 0B is missing")?;
    let literal_references = bank
        .windows(2)
        .enumerate()
        .filter_map(|(offset, bytes)| {
            let target = u16::from_le_bytes([bytes[0], bytes[1]]);
            (FIXED_MENU_FONT_PAGE_APPENDER_ORIGIN..FIXED_MENU_FONT_PAGE_APPENDER_END)
                .contains(&target)
                .then_some((0x8000_u16 + u16::try_from(offset).ok()?, target))
        })
        .collect::<Vec<_>>();
    ensure!(
        literal_references.is_empty(),
        "source bank 0B gained a literal reference into the fixed-menu appender cave: {literal_references:?}"
    );
    Ok(())
}

/// `$E690`의 모든 직접 호출은 바로 앞 `LDA #state`와 한 명령 단위로 결속한다.
/// 페이지를 게시하는 상태의 생산자 집합도 별도로 고정해, 재사용 상태가 늘거나 새
/// 호출자가 생겼을 때 화면 역할을 추측한 채 통과하지 못하게 한다.
fn bind_direct_composite_state_producers(source: &Rom, candidate: &Rom) -> Result<()> {
    let source_catalog = bind_direct_composite_state_producer_catalog(source)?;
    ensure!(
        scan_direct_composite_state_producers(candidate)? == source_catalog,
        "candidate direct composite-state producer catalog changed"
    );

    let font_page_states = COMPOSITE_FONT_RESIDENCY_POLICIES
        .iter()
        .map(|(state, _)| *state)
        .collect::<BTreeSet<_>>();
    let page_producers = source_catalog
        .into_iter()
        .filter(|producer| font_page_states.contains(&producer.state))
        .collect::<Vec<_>>();
    ensure!(
        page_producers == EXPECTED_FONT_PAGE_STATE_PRODUCERS,
        "font-page composite-state producer routes changed: {page_producers:?}"
    );
    Ok(())
}

/// 원본 `STA $05E8`를 같은 길이의 호출로 바꾼다. 호출 뒤의 `LDA #$01`이 A와
/// 플래그를 즉시 새로 정하므로 게시기는 X/Y만 건드리지 않으면 된다.
pub(super) fn page_publisher_hook(publisher: u16) -> Result<DialogueRuntimeHook> {
    Ok(DialogueRuntimeHook {
        role: DialogueRuntimeHookRole::ConsumerFontPagePublisher,
        write_role: "consumer font page publisher hook",
        site: DialogueRuntimeHookSite::Fixed(COMPOSITE_PAGE_ENTRY),
        bytes: assemble_at(COMPOSITE_PAGE_ENTRY, &[Instruction::JsrAbsolute(publisher)])?,
    })
}

/// 복합 UI의 열기·닫기 두 호출만 각 수명 routine으로 보낸다. 둘 다 원본과 같은
/// 3바이트 `JSR`이어서 주변 명령 경계와 반환 주소를 바꾸지 않는다.
pub(super) fn screen_lifetime_hooks(open: u16, close: u16) -> Result<[DialogueRuntimeHook; 2]> {
    Ok([
        DialogueRuntimeHook {
            role: DialogueRuntimeHookRole::ConsumerFontPageOpen,
            write_role: "consumer font page screen-open hook",
            site: DialogueRuntimeHookSite::Switchable {
                bank: UNIT_UI_BANK,
                address: SCREEN_OPEN_RIGHT_FD_CALL,
            },
            bytes: assemble_at(SCREEN_OPEN_RIGHT_FD_CALL, &[Instruction::JsrAbsolute(open)])?,
        },
        DialogueRuntimeHook {
            role: DialogueRuntimeHookRole::ConsumerFontPageClose,
            write_role: "consumer font page screen-close hook",
            site: DialogueRuntimeHookSite::Switchable {
                bank: UNIT_UI_BANK,
                address: SCREEN_CLOSE_RIGHT_FD_CALL,
            },
            bytes: assemble_at(
                SCREEN_CLOSE_RIGHT_FD_CALL,
                &[Instruction::JsrAbsolute(close)],
            )?,
        },
    ])
}

/// 저장 기록을 불러온 뒤 맵 초기화가 시작되는 원본의 두 제로 페이지 저장을 같은
/// 의미의 고정 루틴으로 보낸다. 네 바이트 자리를 그대로 소유하므로 뒤의 `$84` 상태
/// 초기화 경계는 움직이지 않는다.
pub(super) fn gameplay_handoff_hook(handoff: u16) -> Result<DialogueRuntimeHook> {
    Ok(DialogueRuntimeHook {
        role: DialogueRuntimeHookRole::ConsumerFontPageGameplayHandoff,
        write_role: "front-end font page gameplay-handoff hook",
        site: DialogueRuntimeHookSite::Fixed(GAMEPLAY_HANDOFF_HOOK_ADDRESS),
        bytes: assemble_at(
            GAMEPLAY_HANDOFF_HOOK_ADDRESS,
            &[Instruction::JsrAbsolute(handoff), Instruction::Nop],
        )?,
    })
}

/// The six path-leading fixed-menu appender calls have the same three-byte footprint as their
/// source `JSR $8EEE`.  Redirecting only those calls avoids treating unrelated composite states as
/// font-page owners.  The wrapper restores the label index before tail-calling the source appender.
pub(super) fn fixed_menu_font_page_hooks(wrapper: u16) -> Result<Vec<DialogueRuntimeHook>> {
    FIXED_MENU_FONT_PAGE_CALLS
        .into_iter()
        .map(|(address, _, role, write_role)| {
            Ok(DialogueRuntimeHook {
                role,
                write_role,
                site: DialogueRuntimeHookSite::Switchable {
                    bank: UNIT_UI_BANK,
                    address,
                },
                bytes: assemble_at(address, &[Instruction::JsrAbsolute(wrapper)])?,
            })
        })
        .collect()
}

/// Carries the source-bound bank-0B cave routine through the same checked switchable-bank mutation
/// path as its call-site hooks.  Its distinct role prevents it from being mistaken for one of the
/// six replaced calls.
pub(super) fn fixed_menu_font_page_appender_installation(
    routine: &RuntimeRoutine,
) -> Result<DialogueRuntimeHook> {
    ensure!(
        routine.address == FIXED_MENU_FONT_PAGE_APPENDER_ORIGIN
            && routine.bytes.len()
                == usize::from(
                    FIXED_MENU_FONT_PAGE_APPENDER_END - FIXED_MENU_FONT_PAGE_APPENDER_ORIGIN,
                ),
        "fixed-menu font-page appender no longer fills its admitted bank-0B cave"
    );
    Ok(DialogueRuntimeHook {
        role: DialogueRuntimeHookRole::FixedMenuFontPageAppenderRoutine,
        write_role: "fixed-menu font-page appender routine",
        site: DialogueRuntimeHookSite::Switchable {
            bank: UNIT_UI_BANK,
            address: routine.address,
        },
        bytes: routine.bytes.clone(),
    })
}

/// 계획 단계에서 검증한 FD/FE route를 현재 UI 페이지로 게시하고 즉시 적용한다.
/// 합성기는 `ScreenFontPageRoutes`의 상수만 넘기고, 이름 appender는 경계가 검증된
/// catalog record의 첫 바이트만 넘긴다. `$07FD=0`인 화면 열기는 이 routine을 부르기
/// 전에 원본 writer로 빠지므로, 이 작은 routine에는 실행 중 allowlist를 다시 복제하지
/// 않는다.
pub(super) fn build_consumer_font_page_activation(
    origin: u16,
    apply_route: u16,
    pages: ScreenFontPageRoutes,
) -> Result<RuntimeRoutine> {
    pages.validate()?;
    let instructions = [
        Instruction::StaAbsolute(CONSUMER_FONT_PAGE),
        Instruction::JmpAbsolute(apply_route),
    ];

    Ok(RuntimeRoutine {
        role: "consumer font page activation",
        address: origin,
        bytes: assemble_at(origin, &instructions)?,
    })
}

/// Selects the fixed-menu page without losing the source appender's index in A.  A `JMP` into the
/// original appender makes its `RTS` return directly to the original screen composer, exactly as
/// the replaced `JSR $8EEE` did.
pub(super) fn build_fixed_menu_font_page_appender(
    origin: u16,
    activation: u16,
    pages: ScreenFontPageRoutes,
) -> Result<RuntimeRoutine> {
    pages.validate()?;
    let instructions = [
        Instruction::Pha,
        Instruction::LdaImmediate(pages.unit_command),
        Instruction::JsrAbsolute(activation),
        Instruction::Pla,
        Instruction::JmpAbsolute(APPEND_FIXED_STRING),
    ];
    Ok(RuntimeRoutine {
        role: "fixed-menu font-page appender",
        address: origin,
        bytes: assemble_at(origin, &instructions)?,
    })
}

/// 이번 합성에 필요한 고정 페이지를 게시한다. 요약·상태처럼 이름별 페이지가
/// 필요한 화면은 0으로 시작하고, 합성 중 unit/enemy appender가 실제 페이지를
/// 게시한다. 아이템·병종·동작 문자열은 모든 카탈로그 페이지에서 같은 코드를 쓰므로
/// 기본 카탈로그 페이지를 게시한다.
pub(super) fn build_composite_font_page_publisher(
    origin: u16,
    activation: u16,
    pages: ScreenFontPageRoutes,
) -> Result<RuntimeRoutine> {
    pages.validate()?;
    let mut instructions = vec![Instruction::StaAbsolute(COMPOSITE_STATE)];
    let mut route_jumps = Vec::with_capacity(COMPOSITE_FONT_RESIDENCY_POLICIES.len());
    for (state, policy) in COMPOSITE_FONT_RESIDENCY_POLICIES {
        let Some(page) = policy.static_page() else {
            continue;
        };
        if [MAP_FUNDS_COMPOSITE_STATE, MAP_SUMMARY_COMPOSITE_STATE].contains(&state) {
            continue;
        }
        instructions.push(Instruction::CmpImmediate(state));
        let jump = instructions.len();
        instructions.push(Instruction::BeqAbsolute(origin));
        route_jumps.push((jump, page));
    }
    // States $13 and $14 are adjacent and share the map-menu page. A final
    // unsigned range check represents both without growing two equality routes.
    instructions.extend([
        Instruction::Sec,
        Instruction::SbcImmediate(MAP_FUNDS_COMPOSITE_STATE),
        Instruction::CmpImmediate(2),
    ]);
    let map_summary_range_jump = instructions.len();
    instructions.push(Instruction::BccAbsolute(origin));
    instructions.extend([
        Instruction::LdaImmediate(0),
        Instruction::StaAbsolute(CONSUMER_FONT_PAGE),
    ]);
    instructions.push(Instruction::Rts);
    for page in [
        ScreenFontPageRole::FrontEndMenu,
        ScreenFontPageRole::FrontEndRecordAction,
        ScreenFontPageRole::CatalogDefault,
        ScreenFontPageRole::UnitCommand,
        ScreenFontPageRole::MapMenu,
        ScreenFontPageRole::ChapterSaveOffer,
    ] {
        let target = next_address(origin, &instructions)?;
        for (jump, page_role) in &route_jumps {
            if *page_role == page {
                instructions[*jump] = Instruction::BeqAbsolute(target);
            }
        }
        if page == ScreenFontPageRole::MapMenu {
            instructions[map_summary_range_jump] = Instruction::BccAbsolute(target);
        }
        instructions.extend([
            Instruction::LdaImmediate(page.mapper_route(pages)),
            // Every validated mapper route is nonzero. The taken relative branch is
            // therefore an exact unconditional transfer while keeping the publisher
            // inside its source-bound fixed-bank cave.
            Instruction::BneAbsolute(activation),
        ]);
    }

    Ok(RuntimeRoutine {
        role: "composite consumer font page publisher",
        address: origin,
        bytes: assemble_at(origin, &instructions)?,
    })
}

/// bank 0B의 복합 UI 열기에서 현재 route를 다시 적용한다. 게시값은 닫기 경계까지
/// 유지하며, 유효한 route가 없으면 원본 `LDA #0; JSR $C9BE` 의미로 돌아간다.
pub(super) fn build_consumer_font_page_open(
    origin: u16,
    activation: u16,
) -> Result<RuntimeRoutine> {
    let mut instructions = vec![Instruction::LdaAbsolute(CONSUMER_FONT_PAGE)];
    let no_page = instructions.len();
    instructions.push(Instruction::BeqAbsolute(origin));
    instructions.extend([
        Instruction::JsrAbsolute(activation),
        Instruction::LdaImmediate(0),
        Instruction::StaZeroPage(RIGHT_FD_SOURCE_SHADOW),
        Instruction::Rts,
    ]);
    let no_page_target = next_address(origin, &instructions)?;
    instructions[no_page] = Instruction::BeqAbsolute(no_page_target);
    instructions.extend([
        Instruction::LdaImmediate(0),
        Instruction::JmpAbsolute(CENTRAL_RIGHT_FD_WRITER),
    ]);

    Ok(RuntimeRoutine {
        role: "consumer font page screen open",
        address: origin,
        bytes: assemble_at(origin, &instructions)?,
    })
}

/// 프런트엔드에서 저장 기록을 불러와 맵 초기화로 넘어가는 경계다. 원본의 `$23`과
/// `$24` 초기화를 보존하면서, 첫 맵 프레임 전에 프런트엔드 글꼴 소유권을 반납한다.
pub(super) fn build_consumer_font_page_gameplay_handoff(origin: u16) -> Result<RuntimeRoutine> {
    let instructions = [
        Instruction::StaZeroPage(GAMEPLAY_PHASE_LOW),
        Instruction::StaZeroPage(GAMEPLAY_PHASE_HIGH),
        Instruction::StaAbsolute(CONSUMER_FONT_PAGE),
        Instruction::Rts,
    ];
    Ok(RuntimeRoutine {
        role: "front-end font page gameplay handoff",
        address: origin,
        bytes: assemble_at(origin, &instructions)?,
    })
}

/// 복합 UI 닫기는 route를 먼저 지운 뒤 원본 A를 그대로 중앙 FD 기록기에 넘긴다.
/// 이후 전역 selector는 `$07FD=0`을 보고 대사 또는 기존 그래픽 사슬로 복귀하므로
/// 지형·전투·제목이 마지막 UI 페이지를 상속하지 않는다.
pub(super) fn build_consumer_font_page_close(
    origin: u16,
    restore_source_pair: u16,
) -> Result<RuntimeRoutine> {
    let instructions = [
        Instruction::Pha,
        Instruction::LdaImmediate(0),
        Instruction::StaAbsolute(CONSUMER_FONT_PAGE),
        Instruction::Pla,
        Instruction::JsrAbsolute(CENTRAL_RIGHT_FD_WRITER),
        Instruction::JmpAbsolute(restore_source_pair),
    ];
    Ok(RuntimeRoutine {
        role: "consumer font page screen close",
        address: origin,
        bytes: assemble_at(origin, &instructions)?,
    })
}

fn fixed_slice(rom: &Rom, address: u16, len: usize) -> Result<&[u8]> {
    ensure!(address >= 0xC000, "fixed source address is below $C000");
    let start = rom
        .prg()
        .len()
        .checked_sub(FIXED_BANK_BYTE_COUNT)
        .and_then(|base| base.checked_add(usize::from(address - 0xC000)))
        .context("fixed source offset overflow")?;
    rom.prg()
        .get(start..start + len)
        .context("fixed source range is outside PRG")
}

fn switchable_slice(rom: &Rom, bank: u8, address: u16, len: usize) -> Result<&[u8]> {
    ensure!(
        (0x8000..0xC000).contains(&address),
        "switchable source address is outside $8000..$BFFF"
    );
    let start = usize::from(bank) * FIXED_BANK_BYTE_COUNT + usize::from(address - 0x8000);
    rom.prg()
        .get(start..start + len)
        .context("switchable source range is outside PRG")
}

fn direct_transfer_sites_in_bank(rom: &Rom, bank: u8, target: u16) -> Result<Vec<(u16, u8)>> {
    let bytes = switchable_slice(rom, bank, 0x8000, FIXED_BANK_BYTE_COUNT)?;
    let operand = target.to_le_bytes();
    let mut transfers = bytes
        .windows(3)
        .enumerate()
        .filter_map(|(offset, window)| {
            ([JSR_ABSOLUTE_OPCODE, JMP_ABSOLUTE_OPCODE].contains(&window[0])
                && window[1..] == operand)
                .then_some((
                    0x8000 + u16::try_from(offset).expect("16 KiB offset fits u16"),
                    window[0],
                ))
        })
        .collect::<Vec<_>>();
    transfers.sort_unstable();
    Ok(transfers)
}

#[cfg(test)]
mod tests {
    use super::*;

    const ORIGIN: u16 = 0xF620;
    const APPLY_ROUTE: u16 = 0xF900;
    const RESTORE_SOURCE_PAIR: u16 = 0xF360;
    const REUSED_STATE_WITHOUT_FONT_OWNERSHIP: u8 = 0x20;
    const STATUS_CARRY: u8 = 0x01;
    const STATUS_ZERO: u8 = 0x02;

    fn pages() -> ScreenFontPageRoutes {
        ScreenFontPageRoutes {
            front_end_menu: 0xA9,
            front_end_record_action: 0xDD,
            unit_command: 0xCC,
            map_menu: 0xD0,
            ending_record: 0xD9,
            chapter_save_offer: 0xD4,
            catalog: [0xDC, 0xE0],
        }
    }

    #[derive(Default)]
    struct RunResult {
        applied_route: Option<u8>,
        appended_fixed_string_index: Option<u8>,
        central_writer_value: Option<u8>,
        restored_source_pair: bool,
        a: u8,
    }

    struct TestCpu {
        memory: Box<[u8; 0x10000]>,
        a: u8,
        p: u8,
        sp: u8,
        pc: u16,
        applied_route: Option<u8>,
        appended_fixed_string_index: Option<u8>,
        central_writer_value: Option<u8>,
        restored_source_pair: bool,
    }

    impl TestCpu {
        fn new(
            memory: Box<[u8; 0x10000]>,
            routines: &[&RuntimeRoutine],
            entry: u16,
            a: u8,
            p: u8,
        ) -> Self {
            let mut memory = memory;
            for routine in routines {
                let start = usize::from(routine.address);
                let end = start + routine.bytes.len();
                memory[start..end].copy_from_slice(&routine.bytes);
            }
            Self {
                memory,
                a,
                p,
                sp: 0xFD,
                pc: entry,
                applied_route: None,
                appended_fixed_string_index: None,
                central_writer_value: None,
                restored_source_pair: false,
            }
        }

        fn run(mut self) -> (Box<[u8; 0x10000]>, RunResult) {
            for _ in 0..256 {
                let opcode = self.read_pc();
                match opcode {
                    0xEA => {}
                    0x08 => self.push(self.p),
                    0x20 => {
                        let target = self.read_word_pc();
                        if target == APPLY_ROUTE {
                            self.applied_route = Some(self.a);
                        } else if target == CENTRAL_RIGHT_FD_WRITER {
                            self.central_writer_value = Some(self.a);
                        } else {
                            let return_address = self.pc.wrapping_sub(1);
                            self.push((return_address >> 8) as u8);
                            self.push(return_address as u8);
                            self.pc = target;
                        }
                    }
                    0x28 => {
                        self.p = self.pop();
                    }
                    0x38 => self.p |= STATUS_CARRY,
                    0x48 => self.push(self.a),
                    0x4C => {
                        let target = self.read_word_pc();
                        if target == CENTRAL_RIGHT_FD_WRITER {
                            self.central_writer_value = Some(self.a);
                            return self.finish();
                        }
                        if target == APPLY_ROUTE {
                            self.applied_route = Some(self.a);
                            if self.sp == 0xFD {
                                return self.finish();
                            }
                            let low = self.pop();
                            let high = self.pop();
                            self.pc = u16::from_le_bytes([low, high]).wrapping_add(1);
                            continue;
                        }
                        if target == RESTORE_SOURCE_PAIR {
                            self.restored_source_pair = true;
                            return self.finish();
                        }
                        if target == APPEND_FIXED_STRING {
                            self.appended_fixed_string_index = Some(self.a);
                            return self.finish();
                        }
                        self.pc = target;
                    }
                    0x60 => {
                        if self.sp == 0xFD {
                            return self.finish();
                        }
                        let low = self.pop();
                        let high = self.pop();
                        self.pc = u16::from_le_bytes([low, high]).wrapping_add(1);
                    }
                    0x68 => {
                        self.a = self.pop();
                        self.set_zero(self.a == 0);
                    }
                    0x8D => {
                        let address = self.read_word_pc();
                        self.memory[usize::from(address)] = self.a;
                    }
                    0x85 => {
                        let address = self.read_pc();
                        self.memory[usize::from(address)] = self.a;
                    }
                    0xA9 => {
                        self.a = self.read_pc();
                        self.set_zero(self.a == 0);
                    }
                    0xAD => {
                        let address = self.read_word_pc();
                        self.a = self.memory[usize::from(address)];
                        self.set_zero(self.a == 0);
                    }
                    0xC9 => {
                        let value = self.read_pc();
                        self.set_zero(self.a == value);
                        self.set_carry(self.a >= value);
                    }
                    0xE9 => {
                        let value = self.read_pc();
                        let borrow = u8::from(self.p & STATUS_CARRY == 0);
                        let required = u16::from(value) + u16::from(borrow);
                        let result = self.a.wrapping_sub(value).wrapping_sub(borrow);
                        self.set_carry(u16::from(self.a) >= required);
                        self.a = result;
                        self.set_zero(self.a == 0);
                    }
                    0xF0 => {
                        let displacement = self.read_pc() as i8;
                        if self.p & STATUS_ZERO != 0 {
                            self.pc = self.pc.wrapping_add_signed(i16::from(displacement));
                        }
                    }
                    0xD0 => {
                        let displacement = self.read_pc() as i8;
                        if self.p & STATUS_ZERO == 0 {
                            self.pc = self.pc.wrapping_add_signed(i16::from(displacement));
                        }
                    }
                    0x90 => {
                        let displacement = self.read_pc() as i8;
                        if self.p & STATUS_CARRY == 0 {
                            self.pc = self.pc.wrapping_add_signed(i16::from(displacement));
                        }
                    }
                    other => panic!("test runtime reached unsupported opcode {other:02X}"),
                }
            }
            panic!("test runtime did not terminate");
        }

        fn finish(self) -> (Box<[u8; 0x10000]>, RunResult) {
            (
                self.memory,
                RunResult {
                    applied_route: self.applied_route,
                    appended_fixed_string_index: self.appended_fixed_string_index,
                    central_writer_value: self.central_writer_value,
                    restored_source_pair: self.restored_source_pair,
                    a: self.a,
                },
            )
        }

        fn read_pc(&mut self) -> u8 {
            let value = self.memory[usize::from(self.pc)];
            self.pc = self.pc.wrapping_add(1);
            value
        }

        fn read_word_pc(&mut self) -> u16 {
            let low = self.read_pc();
            let high = self.read_pc();
            u16::from_le_bytes([low, high])
        }

        fn push(&mut self, value: u8) {
            self.memory[0x100 + usize::from(self.sp)] = value;
            self.sp = self.sp.wrapping_sub(1);
        }

        fn pop(&mut self) -> u8 {
            self.sp = self.sp.wrapping_add(1);
            self.memory[0x100 + usize::from(self.sp)]
        }

        fn set_zero(&mut self, set: bool) {
            if set {
                self.p |= STATUS_ZERO;
            } else {
                self.p &= !STATUS_ZERO;
            }
        }

        fn set_carry(&mut self, set: bool) {
            if set {
                self.p |= STATUS_CARRY;
            } else {
                self.p &= !STATUS_CARRY;
            }
        }
    }

    fn run_routines(
        memory: Box<[u8; 0x10000]>,
        routines: &[&RuntimeRoutine],
        entry: u16,
        a: u8,
        p: u8,
    ) -> (Box<[u8; 0x10000]>, RunResult) {
        TestCpu::new(memory, routines, entry, a, p).run()
    }

    #[test]
    fn static_redraw_maps_immediately_and_screen_close_clears_the_page() {
        let pages = pages();
        let activation = build_consumer_font_page_activation(ORIGIN, APPLY_ROUTE, pages).unwrap();
        let publisher_origin = ORIGIN + u16::try_from(activation.bytes.len()).unwrap();
        let publisher =
            build_composite_font_page_publisher(publisher_origin, activation.address, pages)
                .unwrap();
        let close_origin = publisher.address + u16::try_from(publisher.bytes.len()).unwrap();
        let close = build_consumer_font_page_close(close_origin, RESTORE_SOURCE_PAIR).unwrap();
        let memory = vec![0; 0x10000].into_boxed_slice().try_into().unwrap();

        let (memory, first_draw) = run_routines(
            memory,
            &[&activation, &publisher],
            publisher.address,
            ITEM_ACTION_COMPOSITE_STATE,
            0xA5,
        );
        assert_eq!(memory[usize::from(CONSUMER_FONT_PAGE)], pages.catalog[0]);
        assert_eq!(first_draw.applied_route, Some(pages.catalog[0]));

        // 커서 이동이 같은 합성기를 다시 부르면 페이지를 즉시 다시 고르되, 닫기
        // 경계가 올 때까지 현재 UI 수명 안에 남는다.
        let (memory, redraw) = run_routines(
            memory,
            &[&activation, &publisher],
            publisher.address,
            ITEM_ACTION_COMPOSITE_STATE,
            0x24,
        );
        assert_eq!(redraw.applied_route, Some(pages.catalog[0]));
        assert_eq!(memory[usize::from(CONSUMER_FONT_PAGE)], pages.catalog[0]);

        let restore_page = 0x19;
        let (memory, closed) = run_routines(memory, &[&close], close.address, restore_page, 0x64);
        assert_eq!(closed.central_writer_value, Some(restore_page));
        assert!(closed.restored_source_pair);
        assert_eq!(closed.applied_route, None);
        assert_eq!(memory[usize::from(CONSUMER_FONT_PAGE)], 0);
    }

    #[test]
    fn map_funds_and_summary_states_share_the_map_menu_page_until_close() {
        let pages = pages();
        let activation = build_consumer_font_page_activation(ORIGIN, APPLY_ROUTE, pages).unwrap();
        let publisher_origin = ORIGIN + u16::try_from(activation.bytes.len()).unwrap();
        let publisher =
            build_composite_font_page_publisher(publisher_origin, activation.address, pages)
                .unwrap();
        let close_origin = publisher.address + u16::try_from(publisher.bytes.len()).unwrap();
        let close = build_consumer_font_page_close(close_origin, RESTORE_SOURCE_PAIR).unwrap();
        let memory = vec![0; 0x10000].into_boxed_slice().try_into().unwrap();

        let (memory, funds) = run_routines(
            memory,
            &[&activation, &publisher],
            publisher.address,
            MAP_FUNDS_COMPOSITE_STATE,
            0xA5,
        );
        assert_eq!(funds.applied_route, Some(pages.map_menu));
        assert_eq!(memory[usize::from(CONSUMER_FONT_PAGE)], pages.map_menu);

        let (memory, summary) = run_routines(
            memory,
            &[&activation, &publisher],
            publisher.address,
            MAP_SUMMARY_COMPOSITE_STATE,
            0x24,
        );
        assert_eq!(summary.applied_route, Some(pages.map_menu));
        assert_eq!(memory[usize::from(CONSUMER_FONT_PAGE)], pages.map_menu);

        // The compact unsigned range must admit exactly $13 and $14. Adjacent
        // states are unrelated composites and must release the old page.
        for state in [
            MAP_FUNDS_COMPOSITE_STATE - 1,
            MAP_SUMMARY_COMPOSITE_STATE + 1,
        ] {
            let mut adjacent_memory = memory.clone();
            adjacent_memory[usize::from(CONSUMER_FONT_PAGE)] = pages.map_menu;
            let (adjacent_memory, adjacent) = run_routines(
                adjacent_memory,
                &[&activation, &publisher],
                publisher.address,
                state,
                0x24,
            );
            assert_eq!(adjacent.applied_route, None);
            assert_eq!(adjacent_memory[usize::from(CONSUMER_FONT_PAGE)], 0);
        }

        let (memory, closed) = run_routines(memory, &[&close], close.address, 0x19, 0x64);
        assert_eq!(closed.central_writer_value, Some(0x19));
        assert!(closed.restored_source_pair);
        assert_eq!(memory[usize::from(CONSUMER_FONT_PAGE)], 0);
    }

    #[test]
    fn dynamic_name_page_can_change_during_one_open_screen() {
        let pages = pages();
        let activation = build_consumer_font_page_activation(ORIGIN, APPLY_ROUTE, pages).unwrap();
        let memory = vec![0; 0x10000].into_boxed_slice().try_into().unwrap();

        let (memory, first_name) = run_routines(
            memory,
            &[&activation],
            activation.address,
            pages.catalog[1],
            0x22,
        );
        assert_eq!(first_name.applied_route, Some(pages.catalog[1]));
        assert_eq!(memory[usize::from(CONSUMER_FONT_PAGE)], pages.catalog[1]);

        let (memory, next_name) = run_routines(
            memory,
            &[&activation],
            activation.address,
            pages.catalog[0],
            0x24,
        );
        assert_eq!(next_name.applied_route, Some(pages.catalog[0]));
        assert_eq!(memory[usize::from(CONSUMER_FONT_PAGE)], pages.catalog[0]);
    }

    #[test]
    fn fixed_menu_appender_selects_the_static_page_and_preserves_the_label_index() {
        let pages = pages();
        let activation = build_consumer_font_page_activation(ORIGIN, APPLY_ROUTE, pages).unwrap();
        let wrapper_origin = ORIGIN + u16::try_from(activation.bytes.len()).unwrap();
        let wrapper =
            build_fixed_menu_font_page_appender(wrapper_origin, activation.address, pages).unwrap();
        let memory = vec![0; 0x10000].into_boxed_slice().try_into().unwrap();

        let (memory, result) = run_routines(
            memory,
            &[&activation, &wrapper],
            wrapper.address,
            0x47,
            0xA5,
        );

        assert_eq!(result.applied_route, Some(pages.unit_command));
        assert_eq!(result.appended_fixed_string_index, Some(0x47));
        assert_eq!(memory[usize::from(CONSUMER_FONT_PAGE)], pages.unit_command);

        let hooks = fixed_menu_font_page_hooks(wrapper.address).unwrap();
        assert_eq!(hooks.len(), FIXED_MENU_FONT_PAGE_CALLS.len());
        assert_eq!(
            hooks
                .iter()
                .map(|hook| match hook.site {
                    DialogueRuntimeHookSite::Switchable { bank, address } => (bank, address),
                    DialogueRuntimeHookSite::Fixed(_) => panic!("fixed-menu hook became fixed"),
                })
                .collect::<Vec<_>>(),
            FIXED_MENU_FONT_PAGE_CALLS
                .iter()
                .map(|(address, _, _, _)| (UNIT_UI_BANK, *address))
                .collect::<Vec<_>>()
        );
        assert!(hooks.iter().all(|hook| hook.bytes[0] == 0x20));
    }

    #[test]
    fn every_front_end_state_selects_its_page_without_prior_residency() {
        let pages = pages();
        let activation = build_consumer_font_page_activation(ORIGIN, APPLY_ROUTE, pages).unwrap();
        let publisher_origin = ORIGIN + u16::try_from(activation.bytes.len()).unwrap();
        let publisher =
            build_composite_font_page_publisher(publisher_origin, activation.address, pages)
                .unwrap();

        let memory = vec![0; 0x10000].into_boxed_slice().try_into().unwrap();
        let (memory, start_menu) = run_routines(
            memory,
            &[&activation, &publisher],
            publisher.address,
            START_MENU_COMPOSITE_STATE,
            0xA5,
        );
        assert_eq!(
            memory[usize::from(CONSUMER_FONT_PAGE)],
            pages.front_end_menu
        );
        assert_eq!(start_menu.applied_route, Some(pages.front_end_menu));

        for (state, expected_route) in [
            (RECORD_LIST_COMPOSITE_STATE, pages.front_end_menu),
            (SAVE_SLOT_SELECTION_COMPOSITE_STATE, pages.front_end_menu),
            (RECORD_ACTION_COMPOSITE_STATE, pages.front_end_record_action),
        ] {
            let memory: Box<[u8; 0x10000]> =
                vec![0; 0x10000].into_boxed_slice().try_into().unwrap();

            let (memory, result) = run_routines(
                memory,
                &[&activation, &publisher],
                publisher.address,
                state,
                0xA5,
            );

            assert_eq!(memory[usize::from(COMPOSITE_STATE)], state);
            assert_eq!(memory[usize::from(CONSUMER_FONT_PAGE)], expected_route);
            assert_eq!(result.applied_route, Some(expected_route));
        }
    }

    #[test]
    fn unsupported_composite_clears_the_previous_screen_page() {
        let pages = pages();
        let activation = build_consumer_font_page_activation(ORIGIN, APPLY_ROUTE, pages).unwrap();
        let publisher_origin = ORIGIN + u16::try_from(activation.bytes.len()).unwrap();
        let publisher =
            build_composite_font_page_publisher(publisher_origin, activation.address, pages)
                .unwrap();
        let mut memory: Box<[u8; 0x10000]> =
            vec![0; 0x10000].into_boxed_slice().try_into().unwrap();
        memory[usize::from(CONSUMER_FONT_PAGE)] = pages.catalog[1];

        let (memory, result) = run_routines(
            memory,
            &[&activation, &publisher],
            publisher.address,
            0x0A,
            0x00,
        );

        assert_eq!(memory[usize::from(COMPOSITE_STATE)], 0x0A);
        assert_eq!(memory[usize::from(CONSUMER_FONT_PAGE)], 0);
        assert_eq!(result.applied_route, None);
    }

    #[test]
    fn reused_state_20_never_claims_the_ending_font_page() {
        let pages = pages();
        let activation = build_consumer_font_page_activation(ORIGIN, APPLY_ROUTE, pages).unwrap();
        let publisher_origin = ORIGIN + u16::try_from(activation.bytes.len()).unwrap();
        let publisher =
            build_composite_font_page_publisher(publisher_origin, activation.address, pages)
                .unwrap();

        let mut memory: Box<[u8; 0x10000]> =
            vec![0; 0x10000].into_boxed_slice().try_into().unwrap();
        memory[usize::from(CONSUMER_FONT_PAGE)] = pages.catalog[0];
        let (memory, result) = run_routines(
            memory,
            &[&activation, &publisher],
            publisher.address,
            REUSED_STATE_WITHOUT_FONT_OWNERSHIP,
            0,
        );
        assert_eq!(result.applied_route, None);
        assert_eq!(memory[usize::from(CONSUMER_FONT_PAGE)], 0);
    }

    #[test]
    fn screen_open_reapplies_and_retains_the_page_until_close() {
        let pages = pages();
        let activation = build_consumer_font_page_activation(ORIGIN, APPLY_ROUTE, pages).unwrap();
        let open_origin = ORIGIN + u16::try_from(activation.bytes.len()).unwrap();
        let open = build_consumer_font_page_open(open_origin, activation.address).unwrap();
        let mut memory: Box<[u8; 0x10000]> =
            vec![0; 0x10000].into_boxed_slice().try_into().unwrap();
        memory[usize::from(CONSUMER_FONT_PAGE)] = pages.unit_command;
        memory[usize::from(RIGHT_FD_SOURCE_SHADOW)] = 0x19;

        let (memory, result) =
            run_routines(memory, &[&activation, &open], open.address, 0x55, 0xA4);

        assert_eq!(result.applied_route, Some(pages.unit_command));
        assert_eq!(result.central_writer_value, None);
        assert_eq!(result.a, 0);
        assert_eq!(memory[usize::from(CONSUMER_FONT_PAGE)], pages.unit_command);
        assert_eq!(memory[usize::from(RIGHT_FD_SOURCE_SHADOW)], 0);
    }

    #[test]
    fn empty_page_uses_the_source_writer_without_calling_activation() {
        let pages = pages();
        let activation = build_consumer_font_page_activation(ORIGIN, APPLY_ROUTE, pages).unwrap();
        let open_origin = ORIGIN + u16::try_from(activation.bytes.len()).unwrap();
        let open = build_consumer_font_page_open(open_origin, activation.address).unwrap();
        let memory: Box<[u8; 0x10000]> = vec![0; 0x10000].into_boxed_slice().try_into().unwrap();

        let (memory, result) =
            run_routines(memory, &[&activation, &open], open.address, 0x11, 0xA4);

        assert_eq!(result.central_writer_value, Some(0));
        assert_eq!(result.applied_route, None);
        assert_eq!(memory[usize::from(CONSUMER_FONT_PAGE)], 0);
    }

    #[test]
    fn gameplay_handoff_releases_the_front_end_page_before_state_reset() {
        let routine = build_consumer_font_page_gameplay_handoff(ORIGIN).unwrap();
        let mut memory: Box<[u8; 0x10000]> =
            vec![0; 0x10000].into_boxed_slice().try_into().unwrap();
        memory[usize::from(CONSUMER_FONT_PAGE)] = pages().front_end_menu;
        memory[usize::from(GAMEPLAY_PHASE_LOW)] = 0xA5;
        memory[usize::from(GAMEPLAY_PHASE_HIGH)] = 0x5A;

        let (memory, result) = run_routines(memory, &[&routine], routine.address, 0, 0x64);

        assert_eq!(result.a, 0);
        assert_eq!(memory[usize::from(GAMEPLAY_PHASE_LOW)], 0);
        assert_eq!(memory[usize::from(GAMEPLAY_PHASE_HIGH)], 0);
        assert_eq!(memory[usize::from(CONSUMER_FONT_PAGE)], 0);

        let hook = gameplay_handoff_hook(routine.address).unwrap();
        assert!(matches!(
            hook.site,
            DialogueRuntimeHookSite::Fixed(GAMEPLAY_HANDOFF_HOOK_ADDRESS)
        ));
        assert_eq!(
            hook.bytes,
            [
                0x20,
                routine.address as u8,
                (routine.address >> 8) as u8,
                0xEA,
            ]
        );
    }

    #[test]
    fn page_roles_cannot_share_the_empty_sentinel_or_each_other() {
        let mut invalid = pages();
        invalid.map_menu = 0;
        assert!(
            invalid
                .validate()
                .unwrap_err()
                .to_string()
                .contains("empty sentinel")
        );

        invalid = pages();
        invalid.map_menu = invalid.unit_command;
        assert!(
            invalid
                .validate()
                .unwrap_err()
                .to_string()
                .contains("same translated page")
        );

        invalid = pages();
        invalid.map_menu |= 2;
        assert!(
            invalid
                .validate()
                .unwrap_err()
                .to_string()
                .contains("invalid FD/FE page route")
        );
    }
}
