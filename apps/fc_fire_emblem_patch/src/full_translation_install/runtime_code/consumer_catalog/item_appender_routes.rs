//! 아이템 ID를 직접 받는 원천 호출과 레코드 기반 호출을 하나의 카탈로그 appender로
//! 합류시킨다.
//!
//! 원천 `$8E6F` 진입점에는 네 직접 호출자가 있고, `$875F` 진입점은 상태 `0A/0B`가
//! 공유하는 레코드 기반 아이템 루프에서 호출된다. 화면별로 훅을 늘리는 대신 `$8E6F`
//! 하나를 정규화 루틴으로 보내 직접 ID를 기존 상점 목록의 even pointer offset으로
//! 바꾼다. 그러면 대사 상점과 일반 카탈로그의 material 선택도 한 런타임에서 유지된다.

use anyhow::{Context, Result, ensure};

use super::{FIXED_BRIDGE_ORIGIN, KIND_SHOP_ITEM_LIST, UNIT_UI_BANK, direct_jsr_sites};
use crate::{
    dialogue_inventory::switchable_cpu_to_file_offset,
    fixed_string_consumers::FixedStringConsumerInspection,
    full_translation_install::{
        runtime_code::{DialogueRuntimeHook, DialogueRuntimeHookRole, DialogueRuntimeHookSite},
        screen_font_residency::{
            ITEM_NAME_APPENDER_PUBLISHED_COMPOSITE_STATES, ScreenFontResidencyPolicy,
            composite_font_residency_policy,
        },
    },
    rom::Rom,
    rp2a03::{Instruction, assemble_at},
    typed_source::decode_rp2a03_sequence,
};

const PRG_BANK_BYTE_COUNT: usize = 16 * 1024;
const DIRECT_ITEM_APPENDER_ENTRY: u16 = 0x8E6F;
const DIRECT_ITEM_APPENDER_SOURCE: [u8; 25] = [
    0x38, 0xE9, 0x01, 0x0A, 0xA8, 0xB9, 0xD5, 0xDA, 0x85, 0x00, 0xB9, 0xD6, 0xDA, 0x85, 0x01, 0x20,
    0xFA, 0x8E, 0xA9, 0xED, 0x9D, 0x51, 0x04, 0xE8, 0x60,
];
const DIRECT_ITEM_NORMALIZER_ORIGIN: u16 = 0xBA75;
const DIRECT_ITEM_NORMALIZER_END: u16 = 0xBA7F;
const GENERIC_ITEM_LOOP_ADDRESS: u16 = 0x86C1;
const GENERIC_ITEM_LOOP_SOURCE: [u8; 26] = [
    0xA9, 0x0E, 0x8D, 0xCF, 0x05, 0x20, 0xC8, 0x97, 0xA9, 0x70, 0x85, 0x70, 0x20, 0x3C, 0x8E, 0xA0,
    0x13, 0xB1, 0x74, 0x84, 0x12, 0xF0, 0x07, 0x20, 0x5F, 0x87,
];

#[derive(Clone, Copy)]
struct DirectItemCaller {
    state: u8,
    handler: u16,
    call: u16,
    source: &'static [u8],
}

const DIRECT_ITEM_CALLERS: [DirectItemCaller; 4] = [
    DirectItemCaller {
        state: 0x01,
        handler: 0x8088,
        call: 0x80B7,
        source: &[
            0x20, 0x6F, 0x8E, 0xCA, 0xA9, 0x2E, 0x9D, 0x51, 0x04, 0xE8, 0xA9, 0xED, 0x9D, 0x51,
            0x04, 0xE8,
        ],
    },
    DirectItemCaller {
        state: 0x15,
        handler: 0x8965,
        call: 0x8980,
        source: &[
            0x20, 0x6F, 0x8E, 0xCA, 0x8E, 0x50, 0x04, 0xA9, 0x08, 0x20, 0x3D, 0x81,
        ],
    },
    DirectItemCaller {
        state: 0x1E,
        handler: 0x8B3A,
        call: 0x8B48,
        source: &[
            0x20, 0x6F, 0x8E, 0xCA, 0x8E, 0x50, 0x04, 0xA9, 0x08, 0x20, 0x3D, 0x81,
        ],
    },
    DirectItemCaller {
        state: 0x24,
        handler: 0x8DC6,
        call: 0x8DD7,
        source: &[
            0x20, 0x6F, 0x8E, 0xAC, 0xB0, 0x77, 0x88, 0xB9, 0x7F, 0xD8, 0x30, 0x12,
        ],
    },
];

pub(super) fn bind_source_routes(
    source: &Rom,
    candidate: &Rom,
    consumers: &FixedStringConsumerInspection,
) -> Result<()> {
    ensure!(
        consumers.composite_handler_target(0x0A) == Some(GENERIC_ITEM_LOOP_ADDRESS),
        "generic item appender state 0A handler changed"
    );
    ensure!(
        ITEM_NAME_APPENDER_PUBLISHED_COMPOSITE_STATES == [0x0A, 0x1E, 0x24],
        "item-name appender residency population changed"
    );
    for state in ITEM_NAME_APPENDER_PUBLISHED_COMPOSITE_STATES {
        ensure!(
            composite_font_residency_policy(state)
                == Some(ScreenFontResidencyPolicy::ItemNamePublishedByAppender),
            "item-name appender state {state:02X} lost its residency policy"
        );
    }

    for (image_role, rom) in [("source", source), ("candidate", candidate)] {
        bind_exact_typed_sequence(
            rom,
            image_role,
            GENERIC_ITEM_LOOP_ADDRESS,
            &GENERIC_ITEM_LOOP_SOURCE,
            "generic item loop leading to the shared record appender",
        )?;
        for caller in DIRECT_ITEM_CALLERS {
            ensure!(
                consumers.composite_handler_target(caller.state) == Some(caller.handler),
                "direct item caller state {:02X} handler changed",
                caller.state
            );
            bind_exact_typed_sequence(
                rom,
                image_role,
                caller.call,
                caller.source,
                "direct item ID appender caller and continuation",
            )?;
        }
        let entry_prefix = switchable_slice(rom, DIRECT_ITEM_APPENDER_ENTRY, 5)?;
        ensure!(
            entry_prefix == &DIRECT_ITEM_APPENDER_SOURCE[..5],
            "{image_role} direct item appender normalization prefix changed"
        );
        decode_rp2a03_sequence(
            entry_prefix,
            DIRECT_ITEM_APPENDER_ENTRY,
            "direct item ID to pointer-offset normalization",
        )?;

        let cave = switchable_slice(
            rom,
            DIRECT_ITEM_NORMALIZER_ORIGIN,
            usize::from(DIRECT_ITEM_NORMALIZER_END - DIRECT_ITEM_NORMALIZER_ORIGIN),
        )?;
        ensure!(
            cave.iter().all(|byte| *byte == 0xFF),
            "{image_role} direct item normalizer cave is not exact FF"
        );

        let transfers = direct_jsr_sites(rom, DIRECT_ITEM_APPENDER_ENTRY)?;
        ensure!(
            transfers == DIRECT_ITEM_CALLERS.map(|caller| caller.call),
            "{image_role} direct item appender caller census changed: {transfers:?}"
        );
    }

    let source_entry = switchable_slice(
        source,
        DIRECT_ITEM_APPENDER_ENTRY,
        DIRECT_ITEM_APPENDER_SOURCE.len(),
    )?;
    ensure!(
        source_entry == DIRECT_ITEM_APPENDER_SOURCE,
        "source direct item appender body changed"
    );
    decode_rp2a03_sequence(
        source_entry,
        DIRECT_ITEM_APPENDER_ENTRY,
        "source direct item appender body",
    )?;

    let bank = switchable_slice(source, 0x8000, PRG_BANK_BYTE_COUNT)?;
    let cave_references = bank
        .windows(2)
        .enumerate()
        .filter_map(|(offset, bytes)| {
            let target = u16::from_le_bytes([bytes[0], bytes[1]]);
            (DIRECT_ITEM_NORMALIZER_ORIGIN..DIRECT_ITEM_NORMALIZER_END)
                .contains(&target)
                .then_some((0x8000_u16 + u16::try_from(offset).ok()?, target))
        })
        .collect::<Vec<_>>();
    ensure!(
        cave_references.is_empty(),
        "source bank 0B gained a literal reference into the direct item normalizer cave: {cave_references:?}"
    );
    Ok(())
}

pub(super) fn build_hooks() -> Result<Vec<DialogueRuntimeHook>> {
    let entry = DialogueRuntimeHook {
        role: DialogueRuntimeHookRole::ConsumerCatalogDirectItemEntry,
        write_role: "consumer catalog direct-item appender entry hook",
        site: DialogueRuntimeHookSite::Switchable {
            bank: UNIT_UI_BANK,
            address: DIRECT_ITEM_APPENDER_ENTRY,
        },
        bytes: assemble_at(
            DIRECT_ITEM_APPENDER_ENTRY,
            &[Instruction::JmpAbsolute(DIRECT_ITEM_NORMALIZER_ORIGIN)],
        )?,
    };
    let normalizer = DialogueRuntimeHook {
        role: DialogueRuntimeHookRole::ConsumerCatalogDirectItemNormalizer,
        write_role: "consumer catalog direct-item ID normalizer",
        site: DialogueRuntimeHookSite::Switchable {
            bank: UNIT_UI_BANK,
            address: DIRECT_ITEM_NORMALIZER_ORIGIN,
        },
        bytes: assemble_at(
            DIRECT_ITEM_NORMALIZER_ORIGIN,
            &[
                Instruction::Sec,
                Instruction::SbcImmediate(1),
                Instruction::AslAccumulator,
                Instruction::Tay,
                Instruction::LdaImmediate(KIND_SHOP_ITEM_LIST),
                Instruction::JmpAbsolute(FIXED_BRIDGE_ORIGIN),
            ],
        )?,
    };
    ensure!(
        normalizer.bytes.len()
            == usize::from(DIRECT_ITEM_NORMALIZER_END - DIRECT_ITEM_NORMALIZER_ORIGIN),
        "direct item normalizer no longer exactly fills its owned cave"
    );
    Ok(vec![entry, normalizer])
}

fn bind_exact_typed_sequence(
    rom: &Rom,
    image_role: &str,
    address: u16,
    expected: &[u8],
    role: &str,
) -> Result<()> {
    let actual = switchable_slice(rom, address, expected.len())?;
    ensure!(
        actual == expected,
        "{image_role} {role} changed at {UNIT_UI_BANK:02X}:{address:04X}"
    );
    decode_rp2a03_sequence(actual, address, role)?;
    Ok(())
}

fn switchable_slice(rom: &Rom, address: u16, length: usize) -> Result<&[u8]> {
    let offset = switchable_cpu_to_file_offset(UNIT_UI_BANK, address)?;
    rom.data()
        .get(offset..offset + length)
        .context("consumer catalog item-appender range is outside the ROM")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn one_shared_entry_redirect_covers_all_direct_item_callers() {
        let hooks = build_hooks().unwrap();
        assert_eq!(hooks.len(), 2);
        assert_eq!(
            hooks[0].bytes,
            [
                0x4C,
                DIRECT_ITEM_NORMALIZER_ORIGIN as u8,
                (DIRECT_ITEM_NORMALIZER_ORIGIN >> 8) as u8,
            ]
        );
        assert_eq!(
            hooks[1].bytes,
            [
                0x38,
                0xE9,
                0x01,
                0x0A,
                0xA8,
                0xA9,
                KIND_SHOP_ITEM_LIST,
                0x4C,
                FIXED_BRIDGE_ORIGIN as u8,
                (FIXED_BRIDGE_ORIGIN >> 8) as u8,
            ]
        );
        assert_eq!(
            DIRECT_ITEM_CALLERS.map(|caller| caller.call),
            [0x80B7, 0x8980, 0x8B48, 0x8DD7]
        );
    }

    #[test]
    fn direct_item_bounds_map_to_the_existing_shop_offset_contract() {
        let normalize = |id: u8| id.wrapping_sub(1).wrapping_mul(2);
        assert_eq!(normalize(1), 0);
        assert_eq!(normalize(91), 180);
        assert!(normalize(0) >= 91 * 2);
        assert!(normalize(92) >= 91 * 2);
    }
}
