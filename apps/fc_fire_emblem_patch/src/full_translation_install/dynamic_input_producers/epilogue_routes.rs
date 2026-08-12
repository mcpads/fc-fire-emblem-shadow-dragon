use std::collections::{BTreeMap, BTreeSet};

use anyhow::{Context, Result, ensure};

use super::{ResolvedProducerRoute, selected_record_routes};
use crate::{
    chapter_transition::validate_ending_character_epilogue_source,
    dialogue_inventory::{inspect_main_dialogue_graph, switchable_cpu_to_file_offset},
    full_translation_install::dynamic_inputs::DynamicStringDomain, rom::Rom,
};

const FAMILY: &str = "ending_character_epilogue";

/// `04:$A12E`. 후일담 순회는 아군 색인 `$773B`를 53에서 시작한다.
const UNIT_CURSOR_START: [u8; 5] = [0xA9, 0x35, 0x8D, 0x3B, 0x77];
/// `04:$A165`. 한 명을 처리하기 전에 색인을 하나 내리고, 그 아군의 이름을 슬롯 0에
/// 쓴 다음, 색인이 음수가 되면 순회를 끝낸다. 그래서 이름을 쓰는 일은 화면을
/// 고르는 일보다 반드시 앞선다.
const UNIT_CURSOR_STEP: [u8; 9] = [
    0xCE, 0x3B, 0x77, 0x20, 0x66, 0xA3, 0xAD, 0x3B, 0x77,
];
/// `04:$A366`. 색인이 음수가 아니면 아군명 포인터 표 `$DE2B`에서 이름을 꺼내
/// 동적 슬롯 0 `$78F2`에 `EF`까지 옮긴다.
const UNIT_NAME_WRITER: [u8; 29] = [
    0xAD, 0x3B, 0x77, 0x30, 0x18, 0x0A, 0xAA, 0xBD, 0x2B, 0xDE, 0x85, 0x0A, 0xBD, 0x2C, 0xDE, 0x85,
    0x0B, 0xA0, 0x00, 0xB1, 0x0A, 0x99, 0xF2, 0x78, 0xC8, 0xC9, 0xEF, 0xD0, 0xF6,
];
/// `04:$A195`. 화면 번호는 색인에 하나를 더한 값이다. 색인 0..52가 곧 항목 1..53이다.
const RECORD_FROM_UNIT_CURSOR: [u8; 6] = [0xAE, 0x3B, 0x77, 0xE8, 0x8A, 0x8D];
/// 순회가 닿는 마지막 항목 번호다. `UNIT_CURSOR_START`의 시작값과 같다.
const LAST_UNIT_ENTRY: usize = 53;
/// 마르스는 쓰러지면 그 자리에서 게임이 끝나므로 전사 분기가 없다. 그래서 라우팅
/// 표만 항목 1이 비어 있다.
const FIRST_ROUTING_ENTRY: usize = 2;

pub(super) fn resolve(
    rom: &Rom,
    classified: &BTreeMap<(&'static str, u8), DynamicStringDomain>,
) -> Result<Vec<ResolvedProducerRoute>> {
    validate_ending_character_epilogue_source(rom)?;
    let mut routes = resolve_unit_names(rom, classified)?;
    routes.extend(resolve_fallen_location(rom, classified)?);
    Ok(routes)
}

/// 살아남은 아군은 후일담 표로, 전사한 아군은 라우팅 표로 갈리지만 이름을 슬롯 0에
/// 넣는 생산자는 갈림길보다 앞서는 하나뿐이다. 두 표의 같은 항목 번호가 같은 아군을
/// 가리킨다.
fn resolve_unit_names(
    rom: &Rom,
    classified: &BTreeMap<(&'static str, u8), DynamicStringDomain>,
) -> Result<Vec<ResolvedProducerRoute>> {
    for (role, cpu_address, expected) in [
        ("unit cursor start", 0xA12Cu16, &UNIT_CURSOR_START[..]),
        ("unit cursor step", 0xA165, &UNIT_CURSOR_STEP[..]),
        ("unit name writer", 0xA366, &UNIT_NAME_WRITER[..]),
        ("record from unit cursor", 0xA195, &RECORD_FROM_UNIT_CURSOR[..]),
    ] {
        ensure!(
            source_bytes(rom, cpu_address, expected.len())? == expected,
            "ending epilogue producer changed at its {role} sequence"
        );
    }
    ensure!(
        usize::from(UNIT_CURSOR_START[1]) == LAST_UNIT_ENTRY,
        "ending epilogue unit cursor no longer starts at the last entry"
    );

    let mut routes = selected_record_routes(
        classified,
        "epilogue-dialogue",
        &(1..=LAST_UNIT_ENTRY).collect(),
        &BTreeMap::from([(0, DynamicStringDomain::PlayableUnitName)]),
        FAMILY,
    );
    routes.extend(selected_record_routes(
        classified,
        "epilogue-routing-dialogue",
        &(FIRST_ROUTING_ENTRY..=LAST_UNIT_ENTRY).collect(),
        &BTreeMap::from([(0, DynamicStringDomain::PlayableUnitName)]),
        FAMILY,
    ));
    Ok(routes)
}

fn resolve_fallen_location(
    rom: &Rom,
    classified: &BTreeMap<(&'static str, u8), DynamicStringDomain>,
) -> Result<Vec<ResolvedProducerRoute>> {
    let selected_records = inspect_main_dialogue_graph(rom.data())?
        .transition_edges
        .into_iter()
        .filter(|edge| {
            edge.source_table_id == "epilogue-routing-dialogue"
                && edge.target_table_id == "epilogue-dialogue"
        })
        .map(|edge| edge.target_canonical_entry_index)
        .collect::<BTreeSet<_>>();
    ensure!(
        !selected_records.is_empty(),
        "ending routing table has no direct epilogue transition target"
    );
    let produced_domains = BTreeMap::from([(1, DynamicStringDomain::LocationName)]);
    let routes = selected_record_routes(
        classified,
        "epilogue-dialogue",
        &selected_records,
        &produced_domains,
        FAMILY,
    );
    // 전사 지명을 넣는 자리는 코드가 표 하나와 선택자 하나를 직접 이름 부르므로
    // 정확히 하나다.
    ensure!(
        routes.len() == 1,
        "ending location producer/consumer join changed"
    );
    Ok(routes)
}

fn source_bytes(rom: &Rom, cpu_address: u16, byte_count: usize) -> Result<&[u8]> {
    let file_offset = switchable_cpu_to_file_offset(0x04, cpu_address)?;
    rom.data()
        .get(file_offset..file_offset + byte_count)
        .context("ending epilogue producer source is outside the ROM")
}
