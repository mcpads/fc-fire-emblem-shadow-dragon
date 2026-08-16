use std::collections::{BTreeMap, BTreeSet};

use anyhow::{Context, Result, ensure};

use crate::dialogue_inventory::MainDialogueGraphReport;

use super::{DialogueRecordKey, report::ObservedScreenLifetimeReport};

mod epilogue;
mod game_over;
mod shop;

pub(crate) use epilogue::ending_character_epilogue_preserved_active_codes;

pub(super) fn observed_screen_lifetime_reports(
    filled_glyphs_by_record: &BTreeMap<DialogueRecordKey, BTreeSet<char>>,
    approved_glyphs_by_record: &BTreeMap<DialogueRecordKey, BTreeSet<char>>,
    graph: &MainDialogueGraphReport,
    active_slot_count: usize,
    working_set_ready: bool,
) -> Result<Vec<ObservedScreenLifetimeReport>> {
    let mut reports = Vec::new();
    if let Some(report) = shop::purchase_handoff_report(
        filled_glyphs_by_record,
        approved_glyphs_by_record,
        active_slot_count,
        working_set_ready,
    )? {
        reports.push(report);
    }
    if let Some(report) = epilogue::ending_character_family_report(
        filled_glyphs_by_record,
        approved_glyphs_by_record,
        graph,
        active_slot_count,
        working_set_ready,
    )? {
        reports.push(report);
    }
    if let Some(report) = game_over::turn_boundary_game_over_report(
        filled_glyphs_by_record,
        approved_glyphs_by_record,
        active_slot_count,
        working_set_ready,
    )? {
        reports.push(report);
    }
    Ok(reports)
}

fn glyph_union_for_records(
    glyphs_by_record: &BTreeMap<DialogueRecordKey, BTreeSet<char>>,
    records: &[(&str, usize)],
    screen_role: &str,
) -> Result<BTreeSet<char>> {
    let mut glyphs = BTreeSet::new();
    for &(table_id, canonical_entry_index) in records {
        let key = (table_id.to_owned(), canonical_entry_index);
        glyphs.extend(
            glyphs_by_record
                .get(&key)
                .with_context(|| {
                    format!(
                        "{screen_role} record {table_id}:{canonical_entry_index} is missing from the workspace"
                    )
                })?
                .iter()
                .copied(),
        );
    }
    Ok(glyphs)
}

fn maximum_transition_chain_glyph_union(
    table_ids: &[&str],
    glyphs_by_record: &BTreeMap<DialogueRecordKey, BTreeSet<char>>,
    graph: &MainDialogueGraphReport,
) -> Result<(usize, BTreeSet<char>)> {
    let mut next_record = BTreeMap::new();
    for edge in &graph.transition_edges {
        let source = (
            edge.source_table_id.to_owned(),
            edge.source_canonical_entry_index,
        );
        let target = (
            edge.target_table_id.to_owned(),
            edge.target_canonical_entry_index,
        );
        ensure!(
            next_record.insert(source.clone(), target).is_none(),
            "main-dialogue record {}:{} has multiple transition targets",
            source.0,
            source.1
        );
    }

    let mut maximum_record_count = 0;
    let mut maximum_glyphs = BTreeSet::new();
    for start in glyphs_by_record
        .keys()
        .filter(|(table_id, _)| table_ids.contains(&table_id.as_str()))
    {
        let mut current = start.clone();
        let mut chain_records = BTreeSet::new();
        let mut chain_glyphs = BTreeSet::new();
        loop {
            ensure!(
                chain_records.insert(current.clone()),
                "observed screen lifetime transition chain contains a cycle at {}:{}",
                current.0,
                current.1
            );
            chain_glyphs.extend(
                glyphs_by_record
                    .get(&current)
                    .with_context(|| {
                        format!(
                            "observed screen lifetime transition target {}:{} is missing from the workspace",
                            current.0, current.1
                        )
                    })?
                    .iter()
                    .copied(),
            );
            let Some(next) = next_record.get(&current) else {
                break;
            };
            current = next.clone();
        }
        if chain_glyphs.len() > maximum_glyphs.len() {
            maximum_record_count = chain_records.len();
            maximum_glyphs = chain_glyphs;
        }
    }
    Ok((maximum_record_count, maximum_glyphs))
}
