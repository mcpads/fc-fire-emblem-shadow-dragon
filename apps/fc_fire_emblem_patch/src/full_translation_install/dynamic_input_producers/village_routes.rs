use std::collections::{BTreeMap, BTreeSet};

use anyhow::{Context, Result, ensure};

use super::{ResolvedProducerRoute, selected_record_routes};
use crate::{
    dialogue_assets::bind_chapter_event_dialogue_routes,
    dialogue_inventory::inspect_main_dialogue_storage,
    full_translation_install::dynamic_inputs::DynamicStringDomain, rom::Rom,
};

const FAMILY: &str = "chapter_village_event_dispatch";
const TABLE_ID: &str = "village-and-outro-dialogue";
const C0_EVENT_TILES: [u8; 3] = [0x4B, 0xA5, 0xAB];

pub(super) fn resolve(
    rom: &Rom,
    classified: &BTreeMap<(&'static str, u8), DynamicStringDomain>,
) -> Result<Vec<ResolvedProducerRoute>> {
    let raw_to_canonical = raw_to_canonical_entries(rom)?;
    let event_routes = bind_chapter_event_dialogue_routes(rom.prg())?;
    ensure!(
        event_routes
            .iter()
            .map(|route| route.chapter_number)
            .collect::<BTreeSet<_>>()
            == (1..=25).collect(),
        "village event resolver no longer covers all 25 chapters"
    );

    let mut selected_by_domain = BTreeMap::<DynamicStringDomain, BTreeSet<usize>>::new();
    for route in event_routes
        .iter()
        .filter(|route| C0_EVENT_TILES.contains(&route.tile_code))
    {
        let Some(domain) = event_domain(route.event_code) else {
            continue;
        };
        let canonical_entry = raw_to_canonical
            .get(&route.dialogue_entry)
            .copied()
            .with_context(|| {
                format!(
                    "chapter {} event references unknown C0 entry {:02X}",
                    route.chapter_number, route.dialogue_entry
                )
            })?;
        selected_by_domain
            .entry(domain)
            .or_default()
            .insert(canonical_entry);
    }

    let mut routes = Vec::new();
    for (domain, selected_records) in selected_by_domain {
        routes.extend(selected_record_routes(
            classified,
            TABLE_ID,
            &selected_records,
            &BTreeMap::from([(0, domain)]),
            FAMILY,
        ));
    }
    routes.sort_unstable_by_key(|route| (route.record_id, route.selector));
    ensure!(
        routes
            .windows(2)
            .all(|pair| (pair[0].record_id, pair[0].selector)
                != (pair[1].record_id, pair[1].selector)),
        "village event families assign conflicting domains to one consumer"
    );
    ensure!(routes.len() == 7, "village producer/consumer join changed");
    Ok(routes)
}

fn raw_to_canonical_entries(rom: &Rom) -> Result<BTreeMap<u8, usize>> {
    let mut raw_to_canonical = BTreeMap::new();
    for record in inspect_main_dialogue_storage(rom.data())?
        .records
        .into_iter()
        .filter(|record| record.table_id == TABLE_ID)
    {
        for raw_entry in record.entry_indices {
            let raw_entry = u8::try_from(raw_entry).context("village entry index exceeds u8")?;
            ensure!(
                raw_to_canonical
                    .insert(raw_entry, record.canonical_entry_index)
                    .is_none(),
                "village raw entry {raw_entry:02X} maps to multiple canonical records"
            );
        }
    }
    Ok(raw_to_canonical)
}

fn event_domain(event_code: u8) -> Option<DynamicStringDomain> {
    match event_code {
        0x01..=0x77 => Some(DynamicStringDomain::ItemName),
        0x78..=0x7F => Some(DynamicStringDomain::PreservedNumeric),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn event_domains_are_partitioned_at_the_dispatch_boundary() {
        assert_eq!(event_domain(0), None);
        assert_eq!(event_domain(1), Some(DynamicStringDomain::ItemName));
        assert_eq!(event_domain(0x77), Some(DynamicStringDomain::ItemName));
        assert_eq!(
            event_domain(0x78),
            Some(DynamicStringDomain::PreservedNumeric)
        );
        assert_eq!(
            event_domain(0x7F),
            Some(DynamicStringDomain::PreservedNumeric)
        );
        assert_eq!(event_domain(0x80), None);
    }
}
