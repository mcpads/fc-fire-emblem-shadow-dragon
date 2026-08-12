use std::collections::{BTreeMap, BTreeSet};

use anyhow::{Result, ensure};

use super::{ResolvedProducerRoute, selected_record_routes};
use crate::{
    chapter_transition::validate_ending_character_epilogue_source,
    dialogue_inventory::inspect_main_dialogue_graph,
    full_translation_install::dynamic_inputs::DynamicStringDomain, rom::Rom,
};

const FAMILY: &str = "ending_character_epilogue";

pub(super) fn resolve(
    rom: &Rom,
    classified: &BTreeMap<(&'static str, u8), DynamicStringDomain>,
) -> Result<Vec<ResolvedProducerRoute>> {
    validate_ending_character_epilogue_source(rom)?;
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
    ensure!(
        routes.len() == 1,
        "ending location producer/consumer join changed"
    );
    Ok(routes)
}
