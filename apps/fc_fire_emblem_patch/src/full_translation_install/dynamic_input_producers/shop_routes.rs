use std::collections::{BTreeMap, BTreeSet};

use anyhow::{Context, Result, ensure};

use super::{ResolvedProducerRoute, selected_record_routes};
use crate::{
    dialogue_inventory::switchable_cpu_to_file_offset,
    full_translation_install::dynamic_inputs::DynamicStringDomain, rom::Rom,
    shop_flow::validate_shop_lifetime_source,
};

const FAMILY: &str = "shop_state_machine";
const FACILITY_DIALOGUE_TABLE_ADDRESS: u16 = 0x9A99;
const FACILITY_DIALOGUE_COUNT: usize = 5;
const GENERIC_NUMERIC_ROUTE: [u8; 24] = [
    0x20, 0x6E, 0xE6, 0xA9, 0x28, 0x38, 0xED, 0x81, 0x76, 0xA0, 0x00, 0x20, 0x4E, 0x9B, 0xA9, 0x2A,
    0x8D, 0xF1, 0x77, 0xEE, 0xDB, 0x05, 0xD0, 0x2C,
];

pub(super) fn resolve(
    rom: &Rom,
    classified: &BTreeMap<(&'static str, u8), DynamicStringDomain>,
) -> Result<Vec<ResolvedProducerRoute>> {
    validate_shop_lifetime_source(rom)?;
    let item_selection = source_bytes(rom, 0x9A0E, 139)?;
    for (role, sequence) in [
        (
            "selected item name in selector zero",
            &[0xA0, 0x00, 0xAD, 0xB0, 0x77, 0x20, 0xEC, 0x9A][..],
        ),
        (
            "facility-indexed dialogue selection",
            &[0xAE, 0xD0, 0x77, 0xBD, 0x99, 0x9A, 0x8D, 0xF1, 0x77][..],
        ),
    ] {
        ensure!(
            item_selection
                .windows(sequence.len())
                .filter(|candidate| *candidate == sequence)
                .count()
                == 1,
            "shop producer lost unique {role} sequence"
        );
    }

    let item_dialogue_records = source_bytes(
        rom,
        FACILITY_DIALOGUE_TABLE_ADDRESS,
        FACILITY_DIALOGUE_COUNT,
    )?
    .iter()
    .copied()
    .map(usize::from)
    .collect::<BTreeSet<_>>();
    let mut routes = selected_record_routes(
        classified,
        "shop-and-item-dialogue",
        &item_dialogue_records,
        &BTreeMap::from([(0, DynamicStringDomain::ItemName)]),
        FAMILY,
    );

    // 시설 대사표는 다섯 시설을 세 레코드로 보내고, 그중 몇이 품목명을 소비하는지는
    // 대사 본문이 정한다. 개수를 고정하면 레코드 프리픽스를 바로잡을 때처럼 소비처가
    // 하나 더 드러날 때마다 깨진다. 여기서 지킬 것은 시설 경로가 비지 않는다는 것이다.
    ensure!(
        !routes.is_empty(),
        "shop facility dialogue table no longer selects an item-name consumer"
    );

    let generic_numeric = source_bytes(rom, 0x9E67, GENERIC_NUMERIC_ROUTE.len())?;
    ensure!(
        generic_numeric == GENERIC_NUMERIC_ROUTE,
        "generic selector-zero numeric route changed"
    );
    // 이쪽은 코드가 레코드 하나와 선택자 하나를 직접 이름 부르므로 정확히 하나다.
    let numeric_routes = selected_record_routes(
        classified,
        "shop-and-item-dialogue",
        &BTreeSet::from([usize::from(GENERIC_NUMERIC_ROUTE[15])]),
        &BTreeMap::from([(0, DynamicStringDomain::PreservedNumeric)]),
        FAMILY,
    );
    ensure!(
        numeric_routes.len() == 1,
        "generic selector-zero numeric route no longer names one consumer record"
    );
    routes.extend(numeric_routes);
    Ok(routes)
}

fn source_bytes(rom: &Rom, cpu_address: u16, byte_count: usize) -> Result<&[u8]> {
    let file_offset = switchable_cpu_to_file_offset(0x06, cpu_address)?;
    rom.data()
        .get(file_offset..file_offset + byte_count)
        .context("shop producer source is outside the ROM")
}
