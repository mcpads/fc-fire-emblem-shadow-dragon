use std::collections::{BTreeMap, BTreeSet};

use anyhow::Result;

use crate::dialogue_inventory::MainDialogueGraphReport;
#[cfg(test)]
use crate::dialogue_inventory::MainDialogueTransitionEdgeReport;
use crate::font_slots::active_hangul_codes;

use super::*;

mod report;
#[cfg(test)]
mod tests;

use report::{
    GlyphCapacityReport, GlyphSetReport, GlyphWorksetScope, GlyphWorksetStatusCounts,
    MainDialogueGlyphWorksetReport, ObservedScreenLifetimeReport,
};

const SHOP_PURCHASE_SCREEN_ROLE: &str = "weapon-shop purchase handoff";
const SHOP_PURCHASE_LIFETIME_RECORDS: [(&str, usize); 2] =
    [("shop-and-item-dialogue", 0), ("shop-and-item-dialogue", 1)];
const SHOP_PURCHASE_RETAINED_SOURCE_CODES: [u8; 17] = [
    0x01, 0x03, 0x04, 0x06, 0x12, 0x13, 0x19, 0x1A, 0x21, 0x25, 0x26, 0x29, 0x2A, 0x32, 0x35,
    0x4E, 0x5F,
];

pub(crate) struct MainDialogueGlyphWorksetSummary {
    pub report_sha1: String,
    pub filled_line_count: usize,
    pub complete_line_count: usize,
    pub filled_unique_glyph_count: usize,
    pub approved_unique_glyph_count: usize,
    pub max_transition_chain_unique_glyph_count: usize,
    pub max_observed_screen_lifetime_slot_demand: usize,
    pub filled_transition_chains_fit_one_page: bool,
    pub filled_observed_screen_lifetimes_fit_one_page: bool,
    pub working_set_ready: bool,
}

pub(crate) fn analyze_main_dialogue_glyph_workset(
    source_path: &Path,
    workspace_path: &Path,
    report_path: &Path,
) -> Result<MainDialogueGlyphWorksetSummary> {
    let rom = Rom::from_path(source_path)?;
    rom.verify_supported_japanese()?;
    let workspace_bytes = fs::read(workspace_path)
        .with_context(|| format!("read main dialogue workspace {}", workspace_path.display()))?;
    let workspace: MainDialogueWorkspace = serde_json::from_slice(&workspace_bytes)
        .with_context(|| format!("parse main dialogue workspace {}", workspace_path.display()))?;
    let expected = build_workspace(rom.data())?;
    validate_workspace_binding(&workspace, &expected)?;
    validate_workspace_translations(&workspace)?;

    let graph = inspect_main_dialogue_graph(rom.data())?;
    let report = build_glyph_workset_report(&workspace, &graph, sha1_hex(&workspace_bytes))?;
    let mut report_bytes =
        serde_json::to_vec_pretty(&report).context("serialize main-dialogue glyph workset")?;
    report_bytes.push(b'\n');
    let report_sha1 = sha1_hex(&report_bytes);
    write_file(report_path, &report_bytes)?;

    Ok(MainDialogueGlyphWorksetSummary {
        report_sha1,
        filled_line_count: report.status_counts.filled,
        complete_line_count: report.status_counts.complete,
        filled_unique_glyph_count: report.filled_glyphs.unique_count,
        approved_unique_glyph_count: report.approved_glyphs.unique_count,
        max_transition_chain_unique_glyph_count: report.max_transition_chain_unique_glyph_count,
        max_observed_screen_lifetime_slot_demand: report
            .observed_screen_lifetimes
            .iter()
            .map(|lifetime| lifetime.filled_slot_demand)
            .max()
            .unwrap_or(0),
        filled_transition_chains_fit_one_page: report
            .capacity
            .filled_transition_chains_fit_one_page_so_far,
        filled_observed_screen_lifetimes_fit_one_page: report
            .capacity
            .filled_observed_screen_lifetimes_fit_one_page_so_far,
        working_set_ready: report.capacity.working_set_ready,
    })
}

fn build_glyph_workset_report(
    workspace: &MainDialogueWorkspace,
    graph: &MainDialogueGraphReport,
    workspace_sha1: String,
) -> Result<MainDialogueGlyphWorksetReport> {
    let mut status_counts = GlyphWorksetStatusCounts::default();
    let mut filled_glyphs = BTreeSet::new();
    let mut approved_glyphs = BTreeSet::new();
    let mut target_glyph_occurrence_count = 0;
    let mut max_line_unique_glyph_count = 0;
    let mut max_record_unique_glyph_count = 0;
    let mut filled_glyphs_by_record = BTreeMap::new();
    let mut approved_glyphs_by_record = BTreeMap::new();

    for record in &workspace.records {
        let mut record_glyphs = BTreeSet::new();
        let mut approved_record_glyphs = BTreeSet::new();
        for line in &record.lines {
            status_counts.add(line.status);
            if line.status == TranslationStatus::Untranslated {
                continue;
            }
            let line_glyphs = encode_korean_markup(&line.korean)?
                .into_iter()
                .filter_map(|byte| match byte {
                    LogicalDialogueByte::TargetGlyph(character) => Some(character),
                    LogicalDialogueByte::Encoded(_) => None,
                })
                .collect::<Vec<_>>();
            target_glyph_occurrence_count += line_glyphs.len();
            let line_unique_glyphs = line_glyphs.iter().copied().collect::<BTreeSet<_>>();
            max_line_unique_glyph_count = max_line_unique_glyph_count.max(line_unique_glyphs.len());
            record_glyphs.extend(line_unique_glyphs.iter().copied());
            filled_glyphs.extend(line_unique_glyphs.iter().copied());
            if line.status == TranslationStatus::Complete {
                approved_glyphs.extend(line_unique_glyphs.iter().copied());
                approved_record_glyphs.extend(line_unique_glyphs);
            }
        }
        max_record_unique_glyph_count = max_record_unique_glyph_count.max(record_glyphs.len());
        let key = (record.table_id.clone(), record.canonical_entry_index);
        ensure!(
            filled_glyphs_by_record
                .insert(key.clone(), record_glyphs)
                .is_none(),
            "duplicate main-dialogue workspace record {}:{}",
            record.table_id,
            record.canonical_entry_index
        );
        approved_glyphs_by_record.insert(key, approved_record_glyphs);
    }

    let line_count = status_counts.total();
    let active_slot_count = active_hangul_codes().len();
    let max_transition_chain_unique_glyph_count =
        max_transition_chain_glyph_count(graph, &filled_glyphs_by_record)?;
    let max_approved_transition_chain_unique_glyph_count =
        max_transition_chain_glyph_count(graph, &approved_glyphs_by_record)?;
    let translation_input_complete = line_count > 0 && status_counts.complete == line_count;
    let working_set_ready = translation_input_complete;
    let observed_screen_lifetimes = observed_screen_lifetime_reports(
        &filled_glyphs_by_record,
        &approved_glyphs_by_record,
        active_slot_count,
        working_set_ready,
    )?;
    let filled_observed_screen_lifetimes_fit_one_page = observed_screen_lifetimes
        .iter()
        .all(|lifetime| lifetime.filled_set_fits_one_page_so_far);
    let approved_single_page_fit =
        working_set_ready.then_some(approved_glyphs.len() <= active_slot_count);
    let approved_transition_chains_fit_one_page = working_set_ready
        .then_some(max_approved_transition_chain_unique_glyph_count <= active_slot_count);
    let approved_observed_screen_lifetimes_fit_one_page = working_set_ready.then(|| {
        observed_screen_lifetimes
            .iter()
            .all(|lifetime| lifetime.approved_set_fits_one_page == Some(true))
    });
    let unresolved = if working_set_ready {
        vec![
            "other caller-handoff screen lifetimes and line-width checks remain separate from the glyph working-set count",
        ]
    } else {
        vec![
            "reviewed Korean translation input is incomplete, so the approved working set is not final",
            "other caller-handoff screen lifetimes and line-width checks remain separate from the glyph working-set count",
        ]
    };

    Ok(MainDialogueGlyphWorksetReport {
        schema: 1,
        source_sha1: EXPECTED_SOURCE_SHA1,
        workspace_sha1,
        scope: GlyphWorksetScope {
            translation_direction: "Japanese to Korean only",
            preserve_existing_english_and_digits: true,
            dialogue_content_emitted: false,
            glyph_characters_emitted: false,
            workspace_paths_emitted: false,
            approved_status: "complete",
        },
        record_count: workspace.records.len(),
        line_count,
        status_counts,
        target_glyph_occurrence_count,
        filled_glyphs: glyph_set_report(&filled_glyphs),
        approved_glyphs: glyph_set_report(&approved_glyphs),
        max_line_unique_glyph_count,
        max_record_unique_glyph_count,
        max_transition_chain_unique_glyph_count,
        observed_screen_lifetimes,
        capacity: GlyphCapacityReport {
            active_slot_count,
            translation_input_complete,
            working_set_ready,
            filled_set_fits_one_page_so_far: filled_glyphs.len() <= active_slot_count,
            filled_transition_chains_fit_one_page_so_far: max_transition_chain_unique_glyph_count
                <= active_slot_count,
            filled_observed_screen_lifetimes_fit_one_page_so_far:
                filled_observed_screen_lifetimes_fit_one_page,
            approved_single_page_fit,
            approved_transition_chains_fit_one_page,
            approved_observed_screen_lifetimes_fit_one_page,
            final_page_plan_eligible: working_set_ready,
        },
        unresolved,
        release_eligible: false,
    })
}

fn observed_screen_lifetime_reports(
    filled_glyphs_by_record: &BTreeMap<DialogueRecordKey, BTreeSet<char>>,
    approved_glyphs_by_record: &BTreeMap<DialogueRecordKey, BTreeSet<char>>,
    active_slot_count: usize,
    working_set_ready: bool,
) -> Result<Vec<ObservedScreenLifetimeReport>> {
    let shop_table_is_present = filled_glyphs_by_record
        .keys()
        .any(|(table_id, _)| table_id == "shop-and-item-dialogue");
    if !shop_table_is_present {
        return Ok(Vec::new());
    }

    let active_codes = active_hangul_codes().into_iter().collect::<BTreeSet<_>>();
    let retained_source_codes = SHOP_PURCHASE_RETAINED_SOURCE_CODES
        .into_iter()
        .collect::<BTreeSet<_>>();
    ensure!(
        retained_source_codes.len() == SHOP_PURCHASE_RETAINED_SOURCE_CODES.len(),
        "{SHOP_PURCHASE_SCREEN_ROLE} retained source codes contain duplicates"
    );
    ensure!(
        retained_source_codes.is_subset(&active_codes),
        "{SHOP_PURCHASE_SCREEN_ROLE} retained source codes include a reserved font slot"
    );

    let filled_glyphs = glyph_union_for_records(
        filled_glyphs_by_record,
        &SHOP_PURCHASE_LIFETIME_RECORDS,
        SHOP_PURCHASE_SCREEN_ROLE,
    )?;
    let approved_glyphs = glyph_union_for_records(
        approved_glyphs_by_record,
        &SHOP_PURCHASE_LIFETIME_RECORDS,
        SHOP_PURCHASE_SCREEN_ROLE,
    )?;
    let preserved_active_source_code_count = retained_source_codes.len();
    let filled_slot_demand = preserved_active_source_code_count + filled_glyphs.len();
    let approved_slot_demand =
        working_set_ready.then_some(preserved_active_source_code_count + approved_glyphs.len());

    Ok(vec![ObservedScreenLifetimeReport {
        screen_role: SHOP_PURCHASE_SCREEN_ROLE,
        source_record_count: SHOP_PURCHASE_LIFETIME_RECORDS.len(),
        filled_unique_glyph_count: filled_glyphs.len(),
        preserved_active_source_code_count,
        filled_slot_demand,
        filled_set_fits_one_page_so_far: filled_slot_demand <= active_slot_count,
        approved_unique_glyph_count: approved_glyphs.len(),
        approved_slot_demand,
        approved_set_fits_one_page: approved_slot_demand
            .map(|slot_demand| slot_demand <= active_slot_count),
    }])
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

type DialogueRecordKey = (String, usize);

fn max_transition_chain_glyph_count(
    graph: &MainDialogueGraphReport,
    glyphs_by_record: &BTreeMap<DialogueRecordKey, BTreeSet<char>>,
) -> Result<usize> {
    let mut next_record = BTreeMap::new();
    let mut target_records = BTreeSet::new();
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
            glyphs_by_record.contains_key(&source),
            "main-dialogue transition source {}:{} is missing from the workspace",
            edge.source_table_id,
            edge.source_canonical_entry_index
        );
        ensure!(
            glyphs_by_record.contains_key(&target),
            "main-dialogue transition target {}:{} is missing from the workspace",
            edge.target_table_id,
            edge.target_canonical_entry_index
        );
        ensure!(
            next_record.insert(source.clone(), target.clone()).is_none(),
            "main-dialogue record {}:{} has multiple transition targets",
            source.0,
            source.1
        );
        target_records.insert(target);
    }

    let roots = glyphs_by_record
        .keys()
        .filter(|key| !target_records.contains(*key))
        .cloned()
        .collect::<Vec<_>>();
    let mut reached_records = BTreeSet::new();
    let mut max_unique_glyph_count = 0;
    for root in roots {
        let mut current = root;
        let mut chain_records = BTreeSet::new();
        let mut chain_glyphs = BTreeSet::new();
        loop {
            ensure!(
                chain_records.insert(current.clone()),
                "main-dialogue transition chain contains a cycle at {}:{}",
                current.0,
                current.1
            );
            reached_records.insert(current.clone());
            chain_glyphs.extend(
                glyphs_by_record
                    .get(&current)
                    .context("main-dialogue transition chain lost a workspace record")?
                    .iter()
                    .copied(),
            );
            let Some(next) = next_record.get(&current) else {
                break;
            };
            current = next.clone();
        }
        max_unique_glyph_count = max_unique_glyph_count.max(chain_glyphs.len());
    }
    ensure!(
        reached_records.len() == glyphs_by_record.len(),
        "main-dialogue transition graph has records unreachable from any root"
    );
    Ok(max_unique_glyph_count)
}

fn glyph_set_report(glyphs: &BTreeSet<char>) -> GlyphSetReport {
    let encoded = glyphs.iter().collect::<String>();
    GlyphSetReport {
        unique_count: glyphs.len(),
        sorted_set_sha1: sha1_hex(encoded.as_bytes()),
    }
}
