use std::collections::{BTreeMap, BTreeSet};

use anyhow::{Context, Result, ensure};

use super::{
    ResolvedProducerRoute, SourceRegionSpec, bind_source_region, selected_record_routes,
    source_region,
};
use crate::{
    dialogue_inventory::switchable_cpu_to_file_offset,
    full_translation_install::dynamic_inputs::DynamicStringDomain, rom::Rom,
};

const FAMILY: &str = "arena_wager_lifetime";
const ARENA_LIFECYCLE: SourceRegionSpec = source_region(
    "arena_dialogue_and_wager_lifecycle",
    0x06,
    0x9C63,
    0x15B,
    "3c619312d274050eb1f3c26e6b5257c6c9450048",
);
const WAGER_PRODUCER: [u8; 20] = [
    0x20, 0x6E, 0xE6, 0xAD, 0x2A, 0x77, 0xA0, 0x01, 0x20, 0x4E, 0x9B, 0xA9, 0x0F, 0x8D, 0xF1, 0x77,
    0xEE, 0xDB, 0x05, 0x60,
];
const AFTER_BATTLE_HANDLER: [u8; 48] = [
    0x20, 0x5A, 0x9C, 0xAD, 0x18, 0x77, 0xD0, 0x0A, 0xAD, 0x2A, 0x77, 0x20, 0x32, 0x9C, 0xA9, 0x15,
    0xD0, 0x0E, 0xAD, 0xF7, 0x76, 0xD0, 0x07, 0x20, 0x6C, 0xBE, 0xA9, 0x16, 0xD0, 0x02, 0xA9, 0x17,
    0x8D, 0xF1, 0x77, 0xA9, 0x19, 0x85, 0x26, 0xA9, 0x00, 0x8D, 0xDF, 0x05, 0xEE, 0xDB, 0x05, 0x60,
];

pub(super) fn resolve(
    rom: &Rom,
    classified: &BTreeMap<(&'static str, u8), DynamicStringDomain>,
) -> Result<Vec<ResolvedProducerRoute>> {
    bind_source_region(rom.data(), ARENA_LIFECYCLE)?;
    let wager = source_bytes(rom, 0x9CB1, WAGER_PRODUCER.len())?;
    ensure!(
        wager == WAGER_PRODUCER,
        "arena wager producer source changed"
    );
    let after_battle = source_bytes(rom, 0x9D8E, AFTER_BATTLE_HANDLER.len())?;
    ensure!(
        after_battle == AFTER_BATTLE_HANDLER,
        "arena after-battle wager lifetime changed"
    );

    let selected_records = BTreeSet::from([
        usize::from(WAGER_PRODUCER[12]),
        usize::from(AFTER_BATTLE_HANDLER[15]),
    ]);
    let produced_domains = BTreeMap::from([(1, DynamicStringDomain::PreservedNumeric)]);
    let routes = selected_record_routes(
        classified,
        "shop-and-item-dialogue",
        &selected_records,
        &produced_domains,
        FAMILY,
    );
    ensure!(
        routes.len() == selected_records.len(),
        "arena producer/consumer join changed"
    );
    Ok(routes)
}

fn source_bytes(rom: &Rom, cpu_address: u16, byte_count: usize) -> Result<&[u8]> {
    let file_offset = switchable_cpu_to_file_offset(0x06, cpu_address)?;
    rom.data()
        .get(file_offset..file_offset + byte_count)
        .context("arena producer source is outside the ROM")
}
