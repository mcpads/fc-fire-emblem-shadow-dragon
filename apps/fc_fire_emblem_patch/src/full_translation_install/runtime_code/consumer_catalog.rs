//! 유닛 UI와 아이템 화면의 공유 원문 표를 카탈로그 전용 문자열로 우회한다.
//!
//! 전투 런타임은 아이템·병종·이름 원문 표를 자기 화면 코드북으로 사용한다. 그 표를
//! 다시 UI 코드로 덮으면 한쪽 화면은 반드시 깨진다. 이 모듈은 유닛 요약과 그 아이템
//! 흐름이 실제로 호출하는 세 `JSR`만 고정 뱅크 브리지로 보내고, 별도 PRG 페이지의
//! 카탈로그 코드를 `0x0451,X`에 직접 합성한다. 같은 함수를 쓰는 명단·별도 프로필과
//! 원문 표·전투 소비자는 그대로 둔다.

use anyhow::{Context, Result, ensure};

use super::{
    DialogueRuntimeHook, DialogueRuntimeHookRole, DialogueRuntimeHookSite, RuntimeRoutine,
    ensure_routines_fit_cave, next_address,
};
use crate::{
    dialogue_inventory::switchable_cpu_to_file_offset,
    fixed_string_consumers::inspect_fixed_string_consumers,
    front_end_menu::RECORD_ACTION_COMPOSITE_STATE,
    full_translation_install::{
        consumer_catalog::ConsumerCatalogRuntimeLayout,
        screen_font_residency::CLASS_NAME_ONLY_COMPOSITE_STATE,
        shop_item_residency::ShopItemResidencyRuntimeContract,
        storage_residency::StorageItemListRuntimeRoute,
    },
    mapper165::font_pair_projection::TRANSLATED_FE_PAGE_FLAG,
    rom::Rom,
    rp2a03::{Instruction, assemble_at},
    typed_source::decode_rp2a03_sequence,
};

use super::consumer_font_page::COMPOSITE_STATE;
use crate::battle_runtime_state::BATTLE_RUNTIME_STATE;

mod item_appender_routes;
mod shop_item_list;
mod shop_item_route;

pub(super) use shop_item_route::{
    verify_shop_item_residency_route, verify_storage_item_residency_route,
};

const UNIT_UI_BANK: u8 = 0x0B;
const ENTRY_STUB_CAVE_END: u16 = 0xF807;
const FIXED_BRIDGE_ORIGIN: u16 = 0xFAF3;
const FIXED_BRIDGE_END: u16 = 0xFB20;

const PPU_CONTROL: u16 = 0x2000;
const PPU_CONTROL_SHADOW: u8 = 0xCD;
const NMI_ENABLE_MASK: u8 = 0x80;
const BANK_VALUE_REGISTER: u16 = 0x8001;
const PRG_8000_REGISTER: u8 = 6;
const PRG_A000_REGISTER: u8 = 7;
const PRG_BANK_SHADOW: u8 = 0x29;
const PAIRED_BANK_HELPER: u16 = 0xFA20;

const CURRENT_RECORD_POINTER: u8 = 0x74;
const CURRENT_ITEM_OFFSET: u8 = 0x12;
const CURRENT_UNIT_RECORD: u16 = BATTLE_RUNTIME_STATE.battle_record_addresses[0];
const ENEMY_RECORD_FLAG: u8 = 0x80;
const COMPOSITE_BUFFER: u16 = 0x0451;
const STRING_TERMINATOR: u8 = 0xEF;
const SEGMENT_SEPARATOR: u8 = 0xED;

const ITEM_ENTRY_COUNT: u8 = 91;
const CLASS_ENTRY_COUNT: u8 = 22;
const UNIT_ENTRY_COUNT: u8 = 53;
const ENEMY_ENTRY_COUNT: u8 = 69;

const KIND_ITEM: u8 = 0;
const KIND_UNIT_OR_ENEMY: u8 = 1;
const KIND_CLASS: u8 = 2;
const KIND_SHOP_ITEM_LIST: u8 = 3;
const KIND_COUNT: u8 = 4;
const MATERIAL_ROUTE_CATALOG: u8 = 0;
const MATERIAL_ROUTE_DIALOGUE: u8 = 1;

struct StorageItemMaterialDispatch {
    dialogue_jump_indices: [usize; 2],
    fallback_jump_indices: [usize; 3],
}

#[derive(Clone, Copy)]
enum HookTransfer {
    CallStub,
    ReplaceEntry,
}

#[derive(Clone, Copy)]
struct HookSite {
    role: DialogueRuntimeHookRole,
    write_role: &'static str,
    address: u16,
    expected_source: &'static [u8],
    expected_candidate: Option<&'static [u8]>,
    kind: u8,
    transfer: HookTransfer,
}

const RECORD_ITEM_APPENDER_CALL_SOURCE: [u8; 7] = [0x20, 0x6B, 0x8E, 0xA0, 0x00, 0xB1, 0x74];
const UNIT_NAME_APPENDER_SOURCE: [u8; 50] = [
    0xA0, 0x00, 0xB1, 0x74, 0x29, 0x7F, 0xA8, 0x88, 0x98, 0x0A, 0xA8, 0xAD, 0xF4, 0x76, 0x29, 0x80,
    0xF0, 0x0C, 0xB9, 0xA4, 0xDF, 0x85, 0x00, 0xB9, 0xA5, 0xDF, 0x85, 0x01, 0xD0, 0x0A, 0xB9, 0x2B,
    0xDE, 0x85, 0x00, 0xB9, 0x2C, 0xDE, 0x85, 0x01, 0x20, 0xFA, 0x8E, 0xA9, 0xED, 0x9D, 0x51, 0x04,
    0xE8, 0x60,
];
const UNIT_NAME_APPENDER_CUMULATIVE: [u8; 50] = [
    0xA0, 0x00, 0xB1, 0x74, 0x29, 0x7F, 0xA8, 0x88, 0x98, 0x0A, 0xA8, 0xAD, 0xF4, 0x76, 0x29, 0x80,
    0xF0, 0x0C, 0xB9, 0xA4, 0xDF, 0x85, 0x00, 0xB9, 0xA5, 0xDF, 0x85, 0x01, 0xD0, 0x0A, 0x20, 0xD0,
    0xB6, 0xEA, 0xEA, 0xEA, 0xEA, 0xEA, 0xEA, 0xEA, 0x20, 0xFA, 0x8E, 0xA9, 0xED, 0x9D, 0x51, 0x04,
    0xE8, 0x60,
];
const CLASS_NAME_APPENDER_SOURCE: [u8; 29] = [
    0xA0, 0x01, 0xB1, 0x74, 0xA8, 0x88, 0x98, 0x0A, 0xA8, 0xB9, 0x1F, 0xDA, 0x85, 0x00, 0xB9, 0x20,
    0xDA, 0x85, 0x01, 0x20, 0xFA, 0x8E, 0xA9, 0xED, 0x9D, 0x51, 0x04, 0xE8, 0x60,
];

const UNIT_NAME_APPENDER_ENTRY: u16 = 0x8E88;
const CLASS_NAME_APPENDER_ENTRY: u16 = 0x8EBA;
const UNIT_NAME_APPENDER_CALLERS: [u16; 3] = [0x80A4, 0x8106, 0x8284];
const CLASS_NAME_APPENDER_CALLERS: [u16; 2] = [0x82A7, 0x8699];

const HOOK_SITES: [HookSite; 3] = [
    HookSite {
        role: DialogueRuntimeHookRole::ConsumerCatalogItemAppender,
        write_role: "consumer catalog item appender hook",
        address: 0x875F,
        expected_source: &RECORD_ITEM_APPENDER_CALL_SOURCE,
        expected_candidate: None,
        kind: KIND_ITEM,
        transfer: HookTransfer::CallStub,
    },
    HookSite {
        role: DialogueRuntimeHookRole::ConsumerCatalogUnitAppender,
        write_role: "consumer catalog shared unit-or-enemy appender entry",
        address: UNIT_NAME_APPENDER_ENTRY,
        expected_source: &UNIT_NAME_APPENDER_SOURCE,
        expected_candidate: Some(&UNIT_NAME_APPENDER_CUMULATIVE),
        kind: KIND_UNIT_OR_ENEMY,
        transfer: HookTransfer::ReplaceEntry,
    },
    HookSite {
        role: DialogueRuntimeHookRole::ConsumerCatalogClassAppender,
        write_role: "consumer catalog shared class appender entry",
        address: CLASS_NAME_APPENDER_ENTRY,
        expected_source: &CLASS_NAME_APPENDER_SOURCE,
        expected_candidate: None,
        kind: KIND_CLASS,
        transfer: HookTransfer::ReplaceEntry,
    },
];

pub(super) struct ConsumerCatalogRuntime {
    pub(super) fixed_routines: Vec<RuntimeRoutine>,
    pub(super) code_routine: RuntimeRoutine,
    pub(super) hooks: Vec<DialogueRuntimeHook>,
}

pub(super) fn bind_consumer_catalog_sites(source: &Rom, candidate: &Rom) -> Result<()> {
    let consumers = inspect_fixed_string_consumers(source)?;
    for site in HOOK_SITES {
        for (image_role, rom, expected) in [
            ("source", source, site.expected_source),
            (
                "candidate",
                candidate,
                site.expected_candidate.unwrap_or(site.expected_source),
            ),
        ] {
            let offset = switchable_cpu_to_file_offset(UNIT_UI_BANK, site.address)?;
            ensure!(
                rom.data().get(offset..offset + expected.len()) == Some(expected),
                "{image_role} {} changed at {UNIT_UI_BANK:02X}:{:04X}",
                site.write_role,
                site.address
            );
            decode_rp2a03_sequence(expected, site.address, site.write_role)?;
        }
    }
    ensure!(
        consumers.composite_handler_target(0x00) == Some(0x8054)
            && consumers.composite_handler_target(0x02) == Some(0x80F6)
            && consumers.composite_handler_target(0x04) == Some(0x826C),
        "shared unit-name appender caller handlers changed"
    );
    ensure!(
        consumers.composite_handler_target(0x0B) == Some(0x867D),
        "class-name-only appender caller handler changed"
    );
    for (image_role, rom) in [("source", source), ("candidate", candidate)] {
        ensure!(
            direct_jsr_sites(rom, UNIT_NAME_APPENDER_ENTRY)? == UNIT_NAME_APPENDER_CALLERS,
            "{image_role} shared unit-name appender caller census changed"
        );
        ensure!(
            direct_jsr_sites(rom, CLASS_NAME_APPENDER_ENTRY)? == CLASS_NAME_APPENDER_CALLERS,
            "{image_role} shared class-name appender caller census changed"
        );
    }
    item_appender_routes::bind_source_routes(source, candidate, &consumers)?;
    shop_item_list::bind_site(source, candidate)?;
    ensure!(
        fixed_bytes(
            candidate,
            FIXED_BRIDGE_ORIGIN,
            FIXED_BRIDGE_END - FIXED_BRIDGE_ORIGIN,
        )?
        .iter()
        .all(|byte| *byte == 0xFF),
        "consumer catalog fixed bridge cave is not exact FF"
    );
    Ok(())
}

pub(super) struct ConsumerCatalogRuntimeInputs {
    pub(super) code_origin: u16,
    pub(super) code_page: u8,
    pub(super) entry_stub_origin: u16,
    pub(super) font_page_activation: u16,
    pub(super) catalog_default_font_route: u8,
    pub(super) front_end_record_action_route: u8,
    pub(super) layout: ConsumerCatalogRuntimeLayout,
    pub(super) shop_item_residency: ShopItemResidencyRuntimeContract,
    pub(super) storage_item_list: StorageItemListRuntimeRoute,
}

pub(super) fn build_consumer_catalog_runtime(
    inputs: ConsumerCatalogRuntimeInputs,
) -> Result<ConsumerCatalogRuntime> {
    let ConsumerCatalogRuntimeInputs {
        code_origin,
        code_page,
        entry_stub_origin,
        font_page_activation,
        catalog_default_font_route,
        front_end_record_action_route,
        layout,
        shop_item_residency,
        storage_item_list,
    } = inputs;
    let code_routine = build_catalog_append_runtime(
        code_origin,
        font_page_activation,
        catalog_default_font_route,
        front_end_record_action_route,
        layout,
        shop_item_residency,
        storage_item_list,
    )?;
    shop_item_route::verify_catalog_font_page_routes(
        &code_routine,
        font_page_activation,
        catalog_default_font_route,
    )?;
    let mut next = entry_stub_origin;
    let mut fixed_routines = Vec::new();
    for site in HOOK_SITES {
        let routine = RuntimeRoutine {
            role: site.write_role,
            address: next,
            bytes: build_entry_stub(next, site.kind)?,
        };
        next = routine_end(&routine)?;
        fixed_routines.push(routine);
    }
    ensure_routines_fit_cave(
        &fixed_routines.iter().collect::<Vec<_>>(),
        entry_stub_origin,
        ENTRY_STUB_CAVE_END,
    )?;
    fixed_routines.push(RuntimeRoutine {
        role: "consumer catalog runtime-page bridge",
        address: FIXED_BRIDGE_ORIGIN,
        bytes: build_fixed_bridge(FIXED_BRIDGE_ORIGIN, code_routine.address, code_page)?,
    });
    ensure_routines_fit_cave(
        &[fixed_routines.last().expect("catalog bridge was appended")],
        FIXED_BRIDGE_ORIGIN,
        FIXED_BRIDGE_END,
    )?;

    let mut hooks = HOOK_SITES
        .into_iter()
        .zip(fixed_routines.iter().take(HOOK_SITES.len()))
        .map(|(site, routine)| {
            Ok(DialogueRuntimeHook {
                role: site.role,
                write_role: site.write_role,
                site: DialogueRuntimeHookSite::Switchable {
                    bank: UNIT_UI_BANK,
                    address: site.address,
                },
                bytes: assemble_at(
                    site.address,
                    &[match site.transfer {
                        HookTransfer::CallStub => Instruction::JsrAbsolute(routine.address),
                        HookTransfer::ReplaceEntry => Instruction::JmpAbsolute(routine.address),
                    }],
                )?,
            })
        })
        .collect::<Result<Vec<_>>>()?;
    hooks.extend(item_appender_routes::build_hooks()?);
    hooks.push(shop_item_list::build_hook(FIXED_BRIDGE_ORIGIN)?);

    Ok(ConsumerCatalogRuntime {
        fixed_routines,
        code_routine,
        hooks,
    })
}

/// 세 호출점 스텁은 kind만 다르고 정확히 다섯 바이트다. 기존 생산자 브리지 뒤의
/// 15바이트를 남김없이 사용하므로 어느 하나가 커지면 계획 단계에서 거부된다.
fn build_entry_stub(origin: u16, kind: u8) -> Result<Vec<u8>> {
    assemble_at(
        origin,
        &[
            Instruction::LdaImmediate(kind),
            Instruction::JmpAbsolute(FIXED_BRIDGE_ORIGIN),
        ],
    )
}

/// 원본 appender는 A·Y·플래그를 보존하지 않고 X만 출력 끝으로 전진시킨다. 브리지는
/// kind·원래 Y·출력 X를 순서대로 스택에 두고 코드 페이지를 매핑한다. 원래 Y는 상점
/// 목록이 이미 계산한 아이템 포인터 색인을 그대로 넘기는 데 필요하다. 호출 직후 세
/// 기존 소비자와 새 상점 훅 모두 A를 다시 쓰므로, 종전의 마지막 `LDA #$ED`는 관측되지
/// 않는 죽은 명령이었고 원래 Y 보존 공간으로 돌린다.
fn build_fixed_bridge(origin: u16, appender: u16, code_page: u8) -> Result<Vec<u8>> {
    assemble_at(
        origin,
        &[
            Instruction::Pha,
            Instruction::Tya,
            Instruction::Pha,
            Instruction::Txa,
            Instruction::Pha,
            Instruction::LdaZeroPage(PPU_CONTROL_SHADOW),
            Instruction::AndImmediate(!NMI_ENABLE_MASK),
            Instruction::StaAbsolute(PPU_CONTROL),
            Instruction::LdaImmediate(PRG_A000_REGISTER),
            crate::mapper165::selector_safety::select_register_instruction(),
            Instruction::LdaImmediate(code_page),
            Instruction::StaAbsolute(BANK_VALUE_REGISTER),
            Instruction::Pla,
            Instruction::Tax,
            Instruction::Pla,
            Instruction::Tay,
            Instruction::Pla,
            Instruction::JsrAbsolute(appender),
            Instruction::Txa,
            Instruction::Pha,
            Instruction::LdaZeroPage(PRG_BANK_SHADOW),
            Instruction::JsrAbsolute(PAIRED_BANK_HELPER),
            Instruction::LdaZeroPage(PPU_CONTROL_SHADOW),
            Instruction::StaAbsolute(PPU_CONTROL),
            Instruction::Pla,
            Instruction::Tax,
            Instruction::Rts,
        ],
    )
}

fn build_catalog_append_runtime(
    origin: u16,
    font_page_activation: u16,
    catalog_default_font_route: u8,
    front_end_record_action_route: u8,
    layout: ConsumerCatalogRuntimeLayout,
    shop_item_residency: ShopItemResidencyRuntimeContract,
    storage_item_list: StorageItemListRuntimeRoute,
) -> Result<RuntimeRoutine> {
    ensure!(
        catalog_default_font_route != 0 && catalog_default_font_route & !0xFD == 0,
        "consumer catalog default route is not a valid nonempty FD/FE route"
    );
    ensure!(
        front_end_record_action_route & TRANSLATED_FE_PAGE_FLAG != 0,
        "consumer catalog record-action route does not select the translated FE page"
    );
    ensure!(
        layout.material_base == shop_item_residency.catalog_material_base
            && layout.material_page == shop_item_residency.catalog_material_page
            && layout.item_directory == shop_item_residency.catalog_item_directory,
        "consumer catalog runtime no longer uses the shop residency fallback material"
    );
    ensure!(
        storage_item_list.deposit.composite_state
            == crate::full_translation_install::screen_font_residency::UNIT_ITEM_LIST_COMPOSITE_STATE
            && storage_item_list.withdraw.composite_state
                == crate::full_translation_install::screen_font_residency::ITEM_USE_RESULT_COMPOSITE_STATE
            && storage_item_list.overflow.composite_state
                == crate::full_translation_install::screen_font_residency::STORAGE_ITEM_DETAIL_COMPOSITE_STATE
            && storage_item_list.deposit.caller_state == storage_item_list.overflow.caller_state
            && storage_item_list.deposit.caller_state != storage_item_list.withdraw.caller_state,
        "storage item material routes no longer distinguish deposit, withdraw, and overflow item-list contexts"
    );
    // The appender's caller owns X as the next composite-buffer position.  TSX is
    // needed only to inspect this temporary call frame, so save the incoming X
    // alongside kind and Y and restore it before selecting or copying material.
    // Leaving the stack pointer in X makes every catalog consumer write at a
    // call-depth-dependent address rather than append to the caller's buffer.
    let mut instructions = vec![
        Instruction::Pha,
        Instruction::Tya,
        Instruction::Pha,
        Instruction::Txa,
        Instruction::Pha,
    ];
    for address in 0x00..=0x05 {
        instructions.extend([Instruction::LdaZeroPage(address), Instruction::Pha]);
    }
    instructions.extend([
        Instruction::Tsx,
        Instruction::LdaAbsoluteX(0x0109),
        Instruction::StaZeroPage(0x05),
        Instruction::LdaAbsoluteX(0x0108),
        Instruction::Tay,
        Instruction::LdaAbsoluteX(0x0107),
        Instruction::Tax,
        Instruction::LdaZeroPage(0x05),
        Instruction::CmpImmediate(KIND_COUNT),
    ]);
    let valid_kind = append_jump_if_carry_clear(origin, &mut instructions)?;
    let invalid_kind = push_jump(&mut instructions, origin);
    let valid_kind_target = next_address(origin, &instructions)?;
    patch_jump(&mut instructions, valid_kind, valid_kind_target);
    instructions.push(Instruction::CmpImmediate(KIND_ITEM));
    let item_kind = append_jump_if_equal(origin, &mut instructions)?;
    instructions.push(Instruction::CmpImmediate(KIND_UNIT_OR_ENEMY));
    let unit_kind = append_jump_if_equal(origin, &mut instructions)?;
    instructions.push(Instruction::CmpImmediate(KIND_CLASS));
    let class_kind = append_jump_if_equal(origin, &mut instructions)?;
    let shop_item_kind = push_jump(&mut instructions, origin);

    let item = next_address(origin, &instructions)?;
    patch_jump(&mut instructions, item_kind, item);
    instructions.extend([
        Instruction::LdyZeroPage(CURRENT_ITEM_OFFSET),
        Instruction::LdaIndirectY(CURRENT_RECORD_POINTER),
        Instruction::CmpImmediate(1),
    ]);
    let item_minimum = append_jump_if_carry_set(origin, &mut instructions)?;
    let invalid_item_minimum = push_jump(&mut instructions, origin);
    let item_minimum_target = next_address(origin, &instructions)?;
    patch_jump(&mut instructions, item_minimum, item_minimum_target);
    instructions.push(Instruction::CmpImmediate(ITEM_ENTRY_COUNT + 1));
    let item_bounded = append_jump_if_carry_clear(origin, &mut instructions)?;
    let invalid_item_maximum = push_jump(&mut instructions, origin);
    let item_bounded_target = next_address(origin, &instructions)?;
    patch_jump(&mut instructions, item_bounded, item_bounded_target);
    instructions.extend([
        Instruction::Sec,
        Instruction::SbcImmediate(1),
        Instruction::StaZeroPage(0x04),
    ]);
    let record_item_storage_dispatch =
        append_storage_item_material_dispatch(origin, &mut instructions, storage_item_list)?;

    let dialogue_storage_item = next_address(origin, &instructions)?;
    for jump in record_item_storage_dispatch.dialogue_jump_indices {
        patch_jump(&mut instructions, jump, dialogue_storage_item);
    }
    set_pointer(
        &mut instructions,
        shop_item_residency.dialogue_item_directory,
    );
    set_material_route(
        &mut instructions,
        shop_item_residency.dialogue_material_page,
        MATERIAL_ROUTE_DIALOGUE,
    );
    let directory_ready_from_storage_item = push_jump(&mut instructions, origin);

    let catalog_item = next_address(origin, &instructions)?;
    for jump in record_item_storage_dispatch.fallback_jump_indices {
        patch_jump(&mut instructions, jump, catalog_item);
    }
    set_pointer(&mut instructions, layout.item_directory);
    set_material_route(
        &mut instructions,
        layout.material_page,
        MATERIAL_ROUTE_CATALOG,
    );
    let directory_ready_from_catalog_item = push_jump(&mut instructions, origin);

    let unit = next_address(origin, &instructions)?;
    patch_jump(&mut instructions, unit_kind, unit);
    instructions.extend([
        Instruction::LdyImmediate(0),
        Instruction::LdaIndirectY(CURRENT_RECORD_POINTER),
        Instruction::AndImmediate(0x7F),
        Instruction::CmpImmediate(1),
    ]);
    let unit_minimum = append_jump_if_carry_set(origin, &mut instructions)?;
    let invalid_unit_minimum = push_jump(&mut instructions, origin);
    let unit_minimum_target = next_address(origin, &instructions)?;
    patch_jump(&mut instructions, unit_minimum, unit_minimum_target);
    instructions.extend([
        Instruction::Sec,
        Instruction::SbcImmediate(1),
        Instruction::StaZeroPage(0x04),
        Instruction::LdaAbsolute(CURRENT_UNIT_RECORD),
        Instruction::AndImmediate(ENEMY_RECORD_FLAG),
    ]);
    let enemy = append_jump_if_not_equal(origin, &mut instructions)?;
    instructions.extend([
        Instruction::LdaZeroPage(0x04),
        Instruction::CmpImmediate(UNIT_ENTRY_COUNT),
    ]);
    let unit_bounded = append_jump_if_carry_clear(origin, &mut instructions)?;
    let invalid_unit_maximum = push_jump(&mut instructions, origin);
    let unit_bounded_target = next_address(origin, &instructions)?;
    patch_jump(&mut instructions, unit_bounded, unit_bounded_target);
    set_pointer(&mut instructions, layout.unit_directory);
    set_material_route(
        &mut instructions,
        layout.material_page,
        MATERIAL_ROUTE_CATALOG,
    );
    let directory_ready_from_unit = push_jump(&mut instructions, origin);

    let shop_item = next_address(origin, &instructions)?;
    patch_jump(&mut instructions, shop_item_kind, shop_item);
    instructions.extend([
        Instruction::Tya,
        Instruction::CmpImmediate(ITEM_ENTRY_COUNT * 2),
    ]);
    let shop_item_bounded = append_jump_if_carry_clear(origin, &mut instructions)?;
    let invalid_shop_item_maximum = push_jump(&mut instructions, origin);
    let shop_item_bounded_target = next_address(origin, &instructions)?;
    patch_jump(
        &mut instructions,
        shop_item_bounded,
        shop_item_bounded_target,
    );
    instructions.extend([Instruction::Tya, Instruction::AndImmediate(1)]);
    let shop_item_even = append_jump_if_equal(origin, &mut instructions)?;
    let invalid_shop_item_alignment = push_jump(&mut instructions, origin);
    let shop_item_even_target = next_address(origin, &instructions)?;
    patch_jump(&mut instructions, shop_item_even, shop_item_even_target);
    instructions.extend([
        Instruction::Tya,
        Instruction::LsrAccumulator,
        Instruction::StaZeroPage(0x04),
    ]);
    let direct_item_storage_dispatch =
        append_storage_item_material_dispatch(origin, &mut instructions, storage_item_list)?;
    for jump in direct_item_storage_dispatch.dialogue_jump_indices {
        patch_jump(&mut instructions, jump, dialogue_storage_item);
    }

    let ordinary_shop_item_context = next_address(origin, &instructions)?;
    for jump in direct_item_storage_dispatch.fallback_jump_indices {
        patch_jump(&mut instructions, jump, ordinary_shop_item_context);
    }
    instructions.extend([
        Instruction::LdaAbsolute(shop_item_residency.outer_state_address),
        Instruction::CmpImmediate(shop_item_residency.composition_state),
    ]);
    let catalog_shop_item_state = append_jump_if_not_equal(origin, &mut instructions)?;
    instructions.extend([
        Instruction::LdaAbsolute(shop_item_residency.dialogue_directory_address),
        Instruction::CmpImmediate(shop_item_residency.dialogue_directory_selector),
    ]);
    let catalog_shop_item_directory = append_jump_if_not_equal(origin, &mut instructions)?;
    instructions.push(Instruction::LdaAbsolute(
        shop_item_residency.e7_caller_resume_flag_address,
    ));
    let catalog_shop_item_page = append_jump_if_equal(origin, &mut instructions)?;
    instructions.push(Instruction::LdaAbsolute(
        shop_item_residency.selected_facility_address,
    ));
    let weapon_shop = append_compare_and_jump_if_equal(
        origin,
        &mut instructions,
        shop_item_residency.selling_facilities[0],
    )?;
    let tool_shop = append_compare_and_jump_if_equal(
        origin,
        &mut instructions,
        shop_item_residency.selling_facilities[1],
    )?;
    instructions.push(Instruction::CmpImmediate(
        shop_item_residency.selling_facilities[2],
    ));
    let catalog_shop_item_facility = append_jump_if_not_equal(origin, &mut instructions)?;

    let dialogue_shop_item = next_address(origin, &instructions)?;
    for jump in [weapon_shop, tool_shop] {
        patch_jump(&mut instructions, jump, dialogue_shop_item);
    }
    set_pointer(
        &mut instructions,
        shop_item_residency.dialogue_item_directory,
    );
    set_material_route(
        &mut instructions,
        shop_item_residency.dialogue_material_page,
        MATERIAL_ROUTE_DIALOGUE,
    );
    let directory_ready_from_shop_item = push_jump(&mut instructions, origin);

    let catalog_shop_item = next_address(origin, &instructions)?;
    for jump in [
        catalog_shop_item_state,
        catalog_shop_item_directory,
        catalog_shop_item_page,
        catalog_shop_item_facility,
    ] {
        patch_jump(&mut instructions, jump, catalog_shop_item);
    }
    set_pointer(&mut instructions, layout.item_directory);
    set_material_route(
        &mut instructions,
        layout.material_page,
        MATERIAL_ROUTE_CATALOG,
    );
    let directory_ready_from_shop_item_fallback = push_jump(&mut instructions, origin);

    let enemy_target = next_address(origin, &instructions)?;
    patch_jump(&mut instructions, enemy, enemy_target);
    instructions.extend([
        Instruction::LdaZeroPage(0x04),
        Instruction::CmpImmediate(ENEMY_ENTRY_COUNT),
    ]);
    let enemy_bounded = append_jump_if_carry_clear(origin, &mut instructions)?;
    let invalid_enemy_maximum = push_jump(&mut instructions, origin);
    let enemy_bounded_target = next_address(origin, &instructions)?;
    patch_jump(&mut instructions, enemy_bounded, enemy_bounded_target);
    set_pointer(&mut instructions, layout.enemy_directory);
    set_material_route(
        &mut instructions,
        layout.material_page,
        MATERIAL_ROUTE_CATALOG,
    );
    let directory_ready_from_enemy = push_jump(&mut instructions, origin);

    let class = next_address(origin, &instructions)?;
    patch_jump(&mut instructions, class_kind, class);
    instructions.extend([
        Instruction::LdyImmediate(1),
        Instruction::LdaIndirectY(CURRENT_RECORD_POINTER),
        Instruction::CmpImmediate(1),
    ]);
    let class_minimum = append_jump_if_carry_set(origin, &mut instructions)?;
    let invalid_class_minimum = push_jump(&mut instructions, origin);
    let class_minimum_target = next_address(origin, &instructions)?;
    patch_jump(&mut instructions, class_minimum, class_minimum_target);
    instructions.push(Instruction::CmpImmediate(CLASS_ENTRY_COUNT + 1));
    let class_bounded = append_jump_if_carry_clear(origin, &mut instructions)?;
    let invalid_class_maximum = push_jump(&mut instructions, origin);
    let class_bounded_target = next_address(origin, &instructions)?;
    patch_jump(&mut instructions, class_bounded, class_bounded_target);
    instructions.extend([
        Instruction::Sec,
        Instruction::SbcImmediate(1),
        Instruction::StaZeroPage(0x04),
    ]);
    set_pointer(&mut instructions, layout.class_directory);
    set_material_route(
        &mut instructions,
        layout.material_page,
        MATERIAL_ROUTE_CATALOG,
    );

    let directory_ready = next_address(origin, &instructions)?;
    for jump in [
        directory_ready_from_shop_item,
        directory_ready_from_storage_item,
        directory_ready_from_catalog_item,
        directory_ready_from_shop_item_fallback,
        directory_ready_from_unit,
        directory_ready_from_enemy,
    ] {
        patch_jump(&mut instructions, jump, directory_ready);
    }
    instructions.extend([
        Instruction::LdaZeroPage(0x04),
        Instruction::AslAccumulator,
        Instruction::Clc,
        Instruction::AdcZeroPage(0x00),
        Instruction::StaZeroPage(0x00),
        Instruction::LdaZeroPage(0x01),
        Instruction::AdcImmediate(0),
        Instruction::StaZeroPage(0x01),
        Instruction::LdaImmediate(PRG_8000_REGISTER),
        crate::mapper165::selector_safety::select_register_instruction(),
        Instruction::LdaZeroPage(0x02),
        Instruction::StaAbsolute(BANK_VALUE_REGISTER),
        Instruction::LdyImmediate(0),
        Instruction::LdaIndirectY(0x00),
        Instruction::StaZeroPage(0x04),
        Instruction::Iny,
        Instruction::LdaIndirectY(0x00),
        Instruction::StaZeroPage(0x01),
        Instruction::LdaZeroPage(0x03),
        Instruction::CmpImmediate(MATERIAL_ROUTE_DIALOGUE),
    ]);
    let dialogue_material_base = append_jump_if_equal(origin, &mut instructions)?;
    append_material_base(&mut instructions, layout.material_base);
    let material_base_ready_from_catalog = push_jump(&mut instructions, origin);
    let dialogue_material_base_target = next_address(origin, &instructions)?;
    patch_jump(
        &mut instructions,
        dialogue_material_base,
        dialogue_material_base_target,
    );
    append_material_base(
        &mut instructions,
        shop_item_residency.dialogue_material_base,
    );
    let material_base_ready = next_address(origin, &instructions)?;
    patch_jump(
        &mut instructions,
        material_base_ready_from_catalog,
        material_base_ready,
    );
    instructions.extend([
        Instruction::LdaZeroPage(0x05),
        Instruction::CmpImmediate(KIND_ITEM),
    ]);
    let catalog_item_kind = append_jump_if_equal(origin, &mut instructions)?;
    instructions.push(Instruction::CmpImmediate(KIND_SHOP_ITEM_LIST));
    let catalog_shop_item_kind = append_jump_if_equal(origin, &mut instructions)?;
    instructions.push(Instruction::CmpImmediate(KIND_CLASS));
    let catalog_class_kind = append_jump_if_equal(origin, &mut instructions)?;
    let skip_catalog_page_activation = push_jump(&mut instructions, origin);
    let catalog_item_activation = next_address(origin, &instructions)?;
    patch_jump(
        &mut instructions,
        catalog_item_kind,
        catalog_item_activation,
    );
    patch_jump(
        &mut instructions,
        catalog_shop_item_kind,
        catalog_item_activation,
    );
    instructions.extend([
        Instruction::LdaZeroPage(0x03),
        Instruction::CmpImmediate(MATERIAL_ROUTE_DIALOGUE),
    ]);
    let retain_dialogue_item_page = append_jump_if_equal(origin, &mut instructions)?;
    instructions.extend([
        Instruction::LdaImmediate(catalog_default_font_route),
        Instruction::JsrAbsolute(font_page_activation),
    ]);
    let catalog_item_activation_finished = next_address(origin, &instructions)?;
    patch_jump(
        &mut instructions,
        retain_dialogue_item_page,
        catalog_item_activation_finished,
    );
    let item_activation_complete = push_jump(&mut instructions, origin);

    let catalog_class_activation = next_address(origin, &instructions)?;
    patch_jump(
        &mut instructions,
        catalog_class_kind,
        catalog_class_activation,
    );
    instructions.extend([
        Instruction::LdaAbsolute(COMPOSITE_STATE),
        Instruction::CmpImmediate(CLASS_NAME_ONLY_COMPOSITE_STATE),
    ]);
    let retain_existing_class_page = append_jump_if_not_equal(origin, &mut instructions)?;
    instructions.extend([
        Instruction::LdaImmediate(catalog_default_font_route),
        Instruction::JsrAbsolute(font_page_activation),
    ]);

    let catalog_page_activation_finished = next_address(origin, &instructions)?;
    for jump in [
        skip_catalog_page_activation,
        item_activation_complete,
        retain_existing_class_page,
    ] {
        patch_jump(&mut instructions, jump, catalog_page_activation_finished);
    }
    instructions.extend([
        Instruction::LdyImmediate(0),
        Instruction::LdaZeroPage(0x05),
        Instruction::CmpImmediate(KIND_UNIT_OR_ENEMY),
    ]);
    let name_prefix = append_jump_if_equal(origin, &mut instructions)?;
    let copy_without_prefix = push_jump(&mut instructions, origin);
    let name_prefix_target = next_address(origin, &instructions)?;
    patch_jump(&mut instructions, name_prefix, name_prefix_target);
    instructions.extend([
        Instruction::LdaIndirectY(0x00),
        Instruction::Pha,
        Instruction::LdaAbsolute(COMPOSITE_STATE),
        Instruction::CmpImmediate(RECORD_ACTION_COMPOSITE_STATE),
    ]);
    let mirror_name_route = append_jump_if_equal(origin, &mut instructions)?;
    instructions.push(Instruction::Pla);
    let activate_preserved_name_route = push_jump(&mut instructions, origin);
    let mirror_name_route_target = next_address(origin, &instructions)?;
    patch_jump(
        &mut instructions,
        mirror_name_route,
        mirror_name_route_target,
    );
    instructions.extend([
        Instruction::Pla,
        Instruction::LdaImmediate(front_end_record_action_route),
    ]);
    let activate_name_route_target = next_address(origin, &instructions)?;
    patch_jump(
        &mut instructions,
        activate_preserved_name_route,
        activate_name_route_target,
    );
    instructions.extend([
        Instruction::JsrAbsolute(font_page_activation),
        Instruction::Iny,
    ]);
    let copy_loop = next_address(origin, &instructions)?;
    patch_jump(&mut instructions, copy_without_prefix, copy_loop);
    instructions.extend([
        Instruction::LdaIndirectY(0x00),
        Instruction::CmpImmediate(STRING_TERMINATOR),
    ]);
    let copy_finished = append_jump_if_equal(origin, &mut instructions)?;
    instructions.extend([
        Instruction::StaAbsoluteX(COMPOSITE_BUFFER),
        Instruction::Inx,
        Instruction::Iny,
        Instruction::JmpAbsolute(copy_loop),
    ]);
    let copy_finished_target = next_address(origin, &instructions)?;
    patch_jump(&mut instructions, copy_finished, copy_finished_target);
    instructions.extend([
        Instruction::LdaImmediate(SEGMENT_SEPARATOR),
        Instruction::StaAbsoluteX(COMPOSITE_BUFFER),
        Instruction::Inx,
        Instruction::LdaImmediate(PRG_8000_REGISTER),
        crate::mapper165::selector_safety::select_register_instruction(),
        Instruction::LdaZeroPage(PRG_BANK_SHADOW),
        Instruction::AndImmediate(0x0F),
        Instruction::AslAccumulator,
        Instruction::StaAbsolute(BANK_VALUE_REGISTER),
    ]);

    let cleanup = next_address(origin, &instructions)?;
    for jump in [
        invalid_kind,
        invalid_item_minimum,
        invalid_item_maximum,
        invalid_unit_minimum,
        invalid_unit_maximum,
        invalid_enemy_maximum,
        invalid_class_minimum,
        invalid_class_maximum,
        invalid_shop_item_maximum,
        invalid_shop_item_alignment,
    ] {
        patch_jump(&mut instructions, jump, cleanup);
    }
    for address in (0x00..=0x05).rev() {
        instructions.extend([Instruction::Pla, Instruction::StaZeroPage(address)]);
    }
    // Discard the saved incoming X/Y/kind frame.  X itself deliberately keeps
    // the advanced output position produced by the copy loop.
    instructions.extend([Instruction::Pla, Instruction::Pla, Instruction::Pla]);
    instructions.push(Instruction::Rts);

    Ok(RuntimeRoutine {
        role: "consumer catalog indexed string appender",
        address: origin,
        bytes: assemble_at(origin, &instructions)?,
    })
}

fn append_storage_item_material_dispatch(
    origin: u16,
    instructions: &mut Vec<Instruction>,
    storage: StorageItemListRuntimeRoute,
) -> Result<StorageItemMaterialDispatch> {
    instructions.extend([
        Instruction::LdaAbsolute(COMPOSITE_STATE),
        Instruction::CmpImmediate(storage.deposit.composite_state),
    ]);
    let deposit_composite = append_jump_if_equal(origin, instructions)?;
    instructions.push(Instruction::CmpImmediate(storage.overflow.composite_state));
    let overflow_composite = append_jump_if_equal(origin, instructions)?;
    instructions.push(Instruction::CmpImmediate(storage.withdraw.composite_state));
    let unrelated_composite = append_jump_if_not_equal(origin, instructions)?;

    instructions.extend([
        Instruction::LdaAbsolute(storage.caller_state_address),
        Instruction::CmpImmediate(storage.withdraw.caller_state),
    ]);
    let withdraw_dialogue = append_jump_if_equal(origin, instructions)?;
    let wrong_withdraw_caller = push_jump(instructions, origin);

    let deposit_or_overflow_context = next_address(origin, instructions)?;
    for jump in [deposit_composite, overflow_composite] {
        patch_jump(instructions, jump, deposit_or_overflow_context);
    }
    instructions.extend([
        Instruction::LdaAbsolute(storage.caller_state_address),
        Instruction::CmpImmediate(storage.deposit.caller_state),
    ]);
    let deposit_or_overflow_dialogue = append_jump_if_equal(origin, instructions)?;
    let wrong_deposit_or_overflow_caller = push_jump(instructions, origin);

    Ok(StorageItemMaterialDispatch {
        dialogue_jump_indices: [withdraw_dialogue, deposit_or_overflow_dialogue],
        fallback_jump_indices: [
            unrelated_composite,
            wrong_withdraw_caller,
            wrong_deposit_or_overflow_caller,
        ],
    })
}

fn set_pointer(instructions: &mut Vec<Instruction>, address: u16) {
    instructions.extend([
        Instruction::LdaImmediate(address as u8),
        Instruction::StaZeroPage(0x00),
        Instruction::LdaImmediate((address >> 8) as u8),
        Instruction::StaZeroPage(0x01),
    ]);
}

fn set_material_route(instructions: &mut Vec<Instruction>, page: u8, route: u8) {
    instructions.extend([
        Instruction::LdaImmediate(page),
        Instruction::StaZeroPage(0x02),
        Instruction::LdaImmediate(route),
        Instruction::StaZeroPage(0x03),
    ]);
}

fn append_material_base(instructions: &mut Vec<Instruction>, base: u16) {
    instructions.extend([
        Instruction::Clc,
        Instruction::LdaZeroPage(0x04),
        Instruction::AdcImmediate(base as u8),
        Instruction::StaZeroPage(0x00),
        Instruction::LdaZeroPage(0x01),
        Instruction::AdcImmediate((base >> 8) as u8),
        Instruction::StaZeroPage(0x01),
    ]);
}

fn append_compare_and_jump_if_equal(
    origin: u16,
    instructions: &mut Vec<Instruction>,
    value: u8,
) -> Result<usize> {
    instructions.push(Instruction::CmpImmediate(value));
    append_jump_if_equal(origin, instructions)
}

fn push_jump(instructions: &mut Vec<Instruction>, placeholder: u16) -> usize {
    let index = instructions.len();
    instructions.push(Instruction::JmpAbsolute(placeholder));
    index
}

fn patch_jump(instructions: &mut [Instruction], index: usize, target: u16) {
    instructions[index] = Instruction::JmpAbsolute(target);
}

fn append_jump_if_equal(origin: u16, instructions: &mut Vec<Instruction>) -> Result<usize> {
    append_conditional_jump(origin, instructions, Instruction::BneAbsolute)
}

fn append_jump_if_not_equal(origin: u16, instructions: &mut Vec<Instruction>) -> Result<usize> {
    append_conditional_jump(origin, instructions, Instruction::BeqAbsolute)
}

fn append_jump_if_carry_clear(origin: u16, instructions: &mut Vec<Instruction>) -> Result<usize> {
    append_conditional_jump(origin, instructions, Instruction::BcsAbsolute)
}

fn append_jump_if_carry_set(origin: u16, instructions: &mut Vec<Instruction>) -> Result<usize> {
    append_conditional_jump(origin, instructions, Instruction::BccAbsolute)
}

fn append_conditional_jump(
    origin: u16,
    instructions: &mut Vec<Instruction>,
    inverse: fn(u16) -> Instruction,
) -> Result<usize> {
    let branch_address = next_address(origin, instructions)?;
    let after = branch_address
        .checked_add(5)
        .context("consumer catalog conditional jump address overflow")?;
    instructions.push(inverse(after));
    Ok(push_jump(instructions, origin))
}

fn routine_end(routine: &RuntimeRoutine) -> Result<u16> {
    u16::try_from(usize::from(routine.address) + routine.bytes.len())
        .context("consumer catalog routine address overflow")
}

fn fixed_bytes(rom: &Rom, start: u16, length: u16) -> Result<&[u8]> {
    let base = rom
        .prg()
        .len()
        .checked_sub(16 * 1024)
        .context("candidate PRG has no fixed bank")?;
    let offset = base + usize::from(start - 0xC000);
    rom.prg()
        .get(offset..offset + usize::from(length))
        .context("consumer catalog fixed bridge is outside candidate")
}

pub(super) fn direct_jsr_sites(rom: &Rom, target: u16) -> Result<Vec<u16>> {
    let bank_offset = switchable_cpu_to_file_offset(UNIT_UI_BANK, 0x8000)?;
    let bank = rom
        .data()
        .get(bank_offset..bank_offset + 16 * 1024)
        .context("consumer catalog bank is outside the ROM")?;
    let operand = target.to_le_bytes();
    Ok(bank
        .windows(3)
        .enumerate()
        .filter_map(|(offset, bytes)| {
            (bytes[0] == 0x20 && bytes[1..] == operand)
                .then_some(0x8000_u16 + u16::try_from(offset).expect("16 KiB offset fits u16"))
        })
        .collect())
}

pub(super) fn verify_shop_item_list_hook(hooks: &[DialogueRuntimeHook]) -> Result<()> {
    shop_item_list::verify_hook(hooks, FIXED_BRIDGE_ORIGIN)
}

#[cfg(test)]
mod tests;
