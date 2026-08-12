//! 소지품이 가득 찼을 때 열리는 맡기기·버리기 상태 머신의 동적 입력 경로다.
//!
//! 이 흐름은 상점이나 마을에서 물건을 얻는 순간 끼어든다. 들머리에서 지금 든 물건의
//! 이름을 슬롯 0에 한 번 써 두고, 그 뒤 상태들이 같은 슬롯을 다시 쓰지 않고 읽는다.
//! 그래서 생산과 소비가 한 화면 안에 있지 않다. 인덱스로 고른 물건을 버릴 때만
//! 슬롯 1에 따로 쓴다.
//!
//! 세 소비처의 `{EC:xx}`는 레코드 프리픽스를 바로잡기 전까지 잘린 네 바이트 안에
//! 있어 보이지 않았다. 의사결정 57번을 따른다.

use std::collections::{BTreeMap, BTreeSet};

use anyhow::{Context, Result, ensure};

use super::{ResolvedProducerRoute, selected_record_routes};
use crate::{
    dialogue_inventory::switchable_cpu_to_file_offset,
    full_translation_install::dynamic_inputs::DynamicStringDomain, rom::Rom,
};

const FAMILY: &str = "item_overflow_state_machine";
const TABLE: &str = "shop-and-item-dialogue";

/// `06:$B130`. 지금 든 물건 `$77B0`의 이름을 슬롯 0에 쓰고, 흐름의 첫 화면을 연다.
/// 뒤따르는 맡김·버림 화면은 이 한 번의 쓰기를 그대로 읽는다.
const SLOT_ZERO_ENTRY: [u8; 26] = [
    0xA0, 0x00, 0xAD, 0xB0, 0x77, 0x20, 0xEC, 0x9A, 0xA9, 0x00, 0x8D, 0xDC, 0x77, 0x8D, 0xF0, 0x77,
    0xA9, 0x40, 0x8D, 0xF1, 0x77, 0xA9, 0xB1, 0x8D, 0xF4, 0x77,
];
/// `06:$B1DE`. 보관소에 넣은 뒤 맡겼다는 화면을 연다.
const SLOT_ZERO_STORED: [u8; 16] = [
    0x20, 0xD6, 0xA0, 0xAE, 0xED, 0x76, 0xBD, 0x78, 0xB1, 0x85, 0x26, 0xA9, 0x43, 0x8D, 0xF1, 0x77,
];
/// `06:$B221`. 지금 든 물건을 그대로 버릴 때의 화면이다. 공통 저장 `$B25B`으로 뛴다.
const SLOT_ZERO_DISCARDED: [u8; 9] = [0xA9, 0x00, 0x8D, 0xB1, 0x77, 0xA9, 0x46, 0xD0, 0x31];
/// `06:$B23C`. 소지품에서 고른 물건의 이름을 슬롯 1에 쓰고 버렸다는 화면을 연다.
const SLOT_ONE_DISCARDED: [u8; 35] = [
    0xAC, 0xB1, 0x77, 0xB1, 0x74, 0xA0, 0x01, 0x20, 0xEC, 0x9A, 0xAC, 0xB1, 0x77, 0xA9, 0x00, 0x91,
    0x74, 0xC8, 0xC8, 0xC8, 0xC8, 0x91, 0x74, 0x8C, 0xB1, 0x77, 0x20, 0x5A, 0x95, 0xA9, 0x45, 0x8D,
    0xF1, 0x77, 0xAD,
];

/// 각 배열 안에서 대사 번호를 담은 자리다. 번호를 따로 적어 두면 코드와 어긋날 수
/// 있으므로 결속에 쓰는 값은 확인한 바이트열에서 그대로 꺼낸다.
const STORED_RECORD_INDEX: usize = 12;
const DISCARDED_RECORD_INDEX: usize = 6;
const SLOT_ONE_RECORD_INDEX: usize = 30;

pub(super) fn resolve(
    rom: &Rom,
    classified: &BTreeMap<(&'static str, u8), DynamicStringDomain>,
) -> Result<Vec<ResolvedProducerRoute>> {
    for (role, cpu_address, expected) in [
        ("slot-zero entry", 0xB130u16, &SLOT_ZERO_ENTRY[..]),
        ("stored confirmation", 0xB1DE, &SLOT_ZERO_STORED[..]),
        ("discarded held item", 0xB221, &SLOT_ZERO_DISCARDED[..]),
        ("discarded chosen item", 0xB23C, &SLOT_ONE_DISCARDED[..]),
    ] {
        ensure!(
            source_bytes(rom, cpu_address, expected.len())? == expected,
            "item overflow producer changed at its {role} sequence"
        );
    }
    // 버리기 분기는 공통 저장 자리로 뛴다. 두 화면이 한 저장을 나눠 쓰는 구조라
    // 이 도달 관계가 끊기면 화면 번호가 엉킨다.
    let branch_target = 0xB221u16 + 9 + u16::from(SLOT_ZERO_DISCARDED[8]);
    ensure!(
        branch_target == 0xB23C + 31,
        "item overflow discard branch no longer joins the shared dialogue store"
    );

    let mut routes = selected_record_routes(
        classified,
        TABLE,
        &BTreeSet::from([
            usize::from(SLOT_ZERO_STORED[STORED_RECORD_INDEX]),
            usize::from(SLOT_ZERO_DISCARDED[DISCARDED_RECORD_INDEX]),
        ]),
        &BTreeMap::from([(0, DynamicStringDomain::ItemName)]),
        FAMILY,
    );
    routes.extend(selected_record_routes(
        classified,
        TABLE,
        &BTreeSet::from([usize::from(SLOT_ONE_DISCARDED[SLOT_ONE_RECORD_INDEX])]),
        &BTreeMap::from([(1, DynamicStringDomain::ItemName)]),
        FAMILY,
    ));
    ensure!(
        routes.len() == 3,
        "item overflow producer no longer names its three item-name consumers"
    );
    Ok(routes)
}

fn source_bytes(rom: &Rom, cpu_address: u16, byte_count: usize) -> Result<&[u8]> {
    let file_offset = switchable_cpu_to_file_offset(0x06, cpu_address)?;
    rom.data()
        .get(file_offset..file_offset + byte_count)
        .context("item overflow producer source is outside the ROM")
}
