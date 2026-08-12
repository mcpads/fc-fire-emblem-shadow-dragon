use std::collections::{BTreeMap, BTreeSet};

use anyhow::{Context, Result, ensure};

use super::{ResolvedProducerRoute, selected_record_routes};
use crate::{
    dialogue_inventory::{inspect_main_dialogue_graph, switchable_cpu_to_file_offset},
    full_translation_install::dynamic_inputs::DynamicStringDomain,
    item_flow::validate_item_lifetime_source,
    rom::Rom,
};

const FAMILY: &str = "item_action_and_result_state_machine";
const ITEM_ACTION_DIALOGUE_TABLE_ADDRESS: u16 = 0x9516;
const ITEM_ACTION_DIALOGUE_COUNT: usize = 4;
const HEAL_RESULT_PREFIX: [u8; 26] = [
    0xA9, 0x1D, 0x8D, 0xF1, 0x77, 0xA9, 0x02, 0x85, 0x01, 0xA9, 0x12, 0x85, 0x08, 0xA9, 0x79, 0x85,
    0x09, 0x20, 0xEA, 0xC7, 0xA0, 0x02, 0xA9, 0xEF, 0x91, 0x08,
];
const STAT_RESULT_PREFIX: [u8; 21] = [
    0xBD, 0xD0, 0x97, 0x8D, 0xF1, 0x77, 0xA0, 0x02, 0xA5, 0x0A, 0x09, 0x60, 0x8D, 0x12, 0x79, 0xA9,
    0xEF, 0x8D, 0x13, 0x79, 0x60,
];

pub(super) fn resolve(
    rom: &Rom,
    classified: &BTreeMap<(&'static str, u8), DynamicStringDomain>,
) -> Result<Vec<ResolvedProducerRoute>> {
    validate_item_lifetime_source(rom)?;
    let mut routes = resolve_action_dialogues(rom, classified)?;
    routes.extend(resolve_result_dialogues(rom, classified)?);
    ensure!(
        routes.len() == 20,
        "item producer/consumer join changed: expected 20 routes, found {}",
        routes.len(),
    );
    Ok(routes)
}

fn resolve_action_dialogues(
    rom: &Rom,
    classified: &BTreeMap<(&'static str, u8), DynamicStringDomain>,
) -> Result<Vec<ResolvedProducerRoute>> {
    let execute_item_action = source_bytes(rom, 0x944C, 202)?;
    for (role, sequence) in [
        (
            "transfer recipient in selector two",
            &[0xA0, 0x02, 0xAD, 0x15, 0x77, 0x20, 0x1D, 0x9B][..],
        ),
        (
            "acting unit in selector zero",
            &[0xA0, 0x00, 0xAD, 0xF4, 0x76, 0x20, 0x1D, 0x9B][..],
        ),
        (
            "selected item in selector one",
            &[0xA0, 0x01, 0xAD, 0xB0, 0x77, 0x20, 0xEC, 0x9A][..],
        ),
        (
            "action-indexed result dialogue",
            &[0xAC, 0xB2, 0x77, 0xB9, 0x16, 0x95, 0x8D, 0xF1, 0x77][..],
        ),
    ] {
        ensure!(
            execute_item_action
                .windows(sequence.len())
                .filter(|candidate| *candidate == sequence)
                .count()
                == 1,
            "item action producer lost unique {role} sequence"
        );
    }
    let selected_records = source_bytes(
        rom,
        ITEM_ACTION_DIALOGUE_TABLE_ADDRESS,
        ITEM_ACTION_DIALOGUE_COUNT,
    )?
    .iter()
    .copied()
    .map(usize::from)
    .collect::<BTreeSet<_>>();
    ensure!(
        selected_records.len() == ITEM_ACTION_DIALOGUE_COUNT,
        "item action dialogue table gained aliases"
    );
    Ok(selected_record_routes(
        classified,
        "shop-and-item-dialogue",
        &selected_records,
        &BTreeMap::from([
            (0, DynamicStringDomain::PlayableUnitName),
            (1, DynamicStringDomain::ItemName),
            (2, DynamicStringDomain::PlayableUnitName),
        ]),
        FAMILY,
    ))
}

fn resolve_result_dialogues(
    rom: &Rom,
    classified: &BTreeMap<(&'static str, u8), DynamicStringDomain>,
) -> Result<Vec<ResolvedProducerRoute>> {
    let heal = source_bytes(rom, 0x9653, 61)?;
    ensure!(
        heal.windows(HEAL_RESULT_PREFIX.len())
            .filter(|candidate| *candidate == HEAL_RESULT_PREFIX)
            .count()
            == 1,
        "healing item result producer changed"
    );
    let stat_boost = source_bytes(rom, 0x978C, 78)?;
    ensure!(
        stat_boost
            .windows(STAT_RESULT_PREFIX.len())
            .filter(|candidate| *candidate == STAT_RESULT_PREFIX)
            .count()
            == 1,
        "stat-boost result producer changed"
    );
    let stat_dialogue_records = stat_boost
        .get(stat_boost.len() - 10..)
        .context("stat-boost result dialogue table is truncated")?;
    ensure!(
        stat_dialogue_records == [0x1E, 0x1F, 0x20, 0x21, 0x22, 0x23, 0x24, 0x25, 0x26, 0x27],
        "stat-boost result dialogue table changed"
    );

    let mut selected_records = stat_dialogue_records
        .iter()
        .copied()
        .map(usize::from)
        .collect::<BTreeSet<_>>();
    selected_records.insert(usize::from(HEAL_RESULT_PREFIX[1]));
    let graph = inspect_main_dialogue_graph(rom.data())?;
    let transition_targets = graph
        .transition_edges
        .iter()
        .filter(|edge| {
            edge.source_table_id == "shop-and-item-dialogue"
                && selected_records.contains(&edge.source_canonical_entry_index)
                && edge.target_table_id == "shop-and-item-dialogue"
        })
        .map(|edge| edge.target_canonical_entry_index)
        .collect::<Vec<_>>();
    selected_records.extend(transition_targets);

    Ok(selected_record_routes(
        classified,
        "shop-and-item-dialogue",
        &selected_records,
        &BTreeMap::from([(2, DynamicStringDomain::PreservedNumeric)]),
        FAMILY,
    ))
}

fn source_bytes(rom: &Rom, cpu_address: u16, byte_count: usize) -> Result<&[u8]> {
    let file_offset = switchable_cpu_to_file_offset(0x06, cpu_address)?;
    rom.data()
        .get(file_offset..file_offset + byte_count)
        .context("item producer source is outside the ROM")
}
