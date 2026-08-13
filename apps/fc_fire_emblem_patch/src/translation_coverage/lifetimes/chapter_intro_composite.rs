use std::{collections::BTreeSet, path::Path};

use anyhow::{Context, Result, ensure};
use serde::Serialize;

use crate::{
    chapter_transition::{bind_chapter_intro_lifetime_contexts, plan_chapter_titles},
    dialogue_assets::plan_main_dialogue_bundle,
    dialogue_inventory::{inspect_main_dialogue_graph, main_dialogue_transition_chain_record_ids},
    font_slots::ACTIVE_HANGUL_SLOT_COUNT,
    rom::{EXPECTED_SOURCE_SHA1, Rom},
    sha1_hex,
    source_font_page::bind_source_font_page_ownership,
};

use super::{super::report::TranslationLifetimeDemandReport, full_page_bound};

const SCREEN_ROLE: &str = "chapter_intro_title_dialogue_composite";
const CHAPTER_COUNT: usize = 25;

pub(super) struct InputBindings<'a> {
    pub(super) source_path: &'a Path,
    pub(super) main_dialogue_workspace_path: &'a Path,
    pub(super) chapter_title_workspace_path: &'a Path,
    pub(super) main_dialogue_workspace_sha1: &'a str,
    pub(super) chapter_title_workspace_sha1: &'a str,
}

#[derive(Serialize)]
struct EvidenceDigest<'a> {
    schema: u8,
    source_sha1: &'static str,
    main_dialogue_workspace_sha1: &'a str,
    chapter_title_workspace_sha1: &'a str,
    chapter_context_count: usize,
    measured_chapter_count: usize,
    measured_completed_page_count: usize,
    source_font_page_sha1: &'static str,
    source_page_japanese_text_active_code_count: usize,
    source_page_preserved_non_japanese_active_code_count: usize,
    maximum_chapter_index: u8,
    maximum_transition_chain_record_count: usize,
    maximum_chapter_completed_page_count: usize,
    maximum_target_glyph_count: usize,
    maximum_preserved_active_source_code_count: usize,
    maximum_total_slot_demand: usize,
    maximum_unpartitioned_page_slot_demand: usize,
    maximum_resident_chain_chapter_index: u8,
    maximum_resident_chain_slot_demand: usize,
    page_granular_reload_required: bool,
    preservation_policy: &'static str,
}

struct ChapterDemand {
    chapter_index: u8,
    transition_chain_record_count: usize,
    completed_page_count: usize,
    target_glyph_count: usize,
    preserved_active_source_code_count: usize,
    total_slot_demand: usize,
    unpartitioned_page_slot_demand: usize,
    resident_chain_slot_demand: usize,
}

pub(super) fn inspect(bindings: InputBindings<'_>) -> Result<TranslationLifetimeDemandReport> {
    let rom = Rom::from_path(bindings.source_path)?;
    rom.verify_supported_japanese()?;
    let contexts = bind_chapter_intro_lifetime_contexts(&rom)?;
    let graph = inspect_main_dialogue_graph(rom.data())?;
    let chapter_titles = plan_chapter_titles(&rom, bindings.chapter_title_workspace_path)?;
    let source_page = bind_source_font_page_ownership(&rom)?;
    ensure!(
        contexts.len() == CHAPTER_COUNT
            && chapter_titles.entry_count == CHAPTER_COUNT
            && chapter_titles.translated_entry_count == CHAPTER_COUNT
            && chapter_titles.workspace_sha1 == bindings.chapter_title_workspace_sha1,
        "chapter-intro composite title or E5 population changed"
    );

    let mut demands = Vec::with_capacity(CHAPTER_COUNT);
    for context in contexts {
        let record_ids = main_dialogue_transition_chain_record_ids(
            &graph,
            "chapter-intro-dialogue",
            context.canonical_entry_index,
        )?;
        let record_id_refs = record_ids.iter().map(String::as_str).collect::<Vec<_>>();
        let dialogue = plan_main_dialogue_bundle(
            &rom,
            bindings.main_dialogue_workspace_path,
            &record_id_refs,
        )?;
        ensure!(
            dialogue.workspace_sha1 == bindings.main_dialogue_workspace_sha1
                && dialogue.record_ids == record_ids,
            "chapter {} intro dialogue chain does not match global coverage",
            usize::from(context.chapter_index) + 1
        );
        let title = chapter_titles.entry(context.chapter_index)?;
        let title_glyphs = title.unique_glyphs();
        let title_reclaimable_active_codes = title.source_reclaimable_active_codes(&rom)?;
        let mut target_glyphs = dialogue.unique_glyphs();
        target_glyphs.extend(title_glyphs.iter().copied());
        let resident_preserved_active_codes = dialogue
            .preserved_source_codes
            .intersection(source_page.active_codes())
            .copied()
            .chain(source_page.preserved_non_japanese_codes().iter().copied())
            .collect::<BTreeSet<_>>();
        let resident_chain_slot_demand = target_glyphs
            .len()
            .checked_add(resident_preserved_active_codes.len())
            .context("chapter-intro resident-chain demand overflow")?;
        let page_bounds = dialogue
            .page_worksets
            .iter()
            .map(|page| {
                let mut page_target_glyphs = page.target_glyphs.clone();
                page_target_glyphs.extend(title_glyphs.iter().copied());
                let mut page_reclaimable_active_codes =
                    page.source_reclaimable_active_codes.clone();
                page_reclaimable_active_codes
                    .extend(title_reclaimable_active_codes.iter().copied());
                let unpartitioned = full_page_bound::calculate(
                    &page_target_glyphs,
                    &page_reclaimable_active_codes,
                    SCREEN_ROLE,
                )?;
                let preserved_active_codes = source_page
                    .preserved_non_japanese_codes()
                    .iter()
                    .copied()
                    .chain(page.preserved_target_active_codes.iter().copied())
                    .collect::<BTreeSet<_>>();
                ensure!(
                    preserved_active_codes.is_subset(source_page.active_codes()),
                    "chapter-intro current-page preservation contains a reserved code"
                );
                let total_slot_demand = page_target_glyphs
                    .len()
                    .checked_add(preserved_active_codes.len())
                    .context("chapter-intro completed-page demand overflow")?;
                Ok(CompletedPageDemand {
                    target_glyph_count: page_target_glyphs.len(),
                    preserved_active_source_code_count: preserved_active_codes.len(),
                    total_slot_demand,
                    unpartitioned_slot_demand: unpartitioned.total_slot_demand,
                })
            })
            .collect::<Result<Vec<_>>>()?;
        let maximum_page = page_bounds
            .iter()
            .max_by_key(|bound| bound.total_slot_demand)
            .context("chapter-intro dialogue chain has no completed page")?;
        demands.push(ChapterDemand {
            chapter_index: context.chapter_index,
            transition_chain_record_count: record_ids.len(),
            completed_page_count: page_bounds.len(),
            target_glyph_count: maximum_page.target_glyph_count,
            preserved_active_source_code_count: maximum_page.preserved_active_source_code_count,
            total_slot_demand: maximum_page.total_slot_demand,
            unpartitioned_page_slot_demand: maximum_page.unpartitioned_slot_demand,
            resident_chain_slot_demand,
        });
    }
    ensure!(
        demands.len() == CHAPTER_COUNT,
        "chapter-intro composite did not measure every chapter"
    );
    let maximum = demands
        .iter()
        .max_by_key(|demand| demand.total_slot_demand)
        .context("chapter-intro composite has no chapter demand")?;
    let maximum_resident_chain = demands
        .iter()
        .max_by_key(|demand| demand.resident_chain_slot_demand)
        .context("chapter-intro composite has no resident-chain demand")?;
    let maximum_unpartitioned_page_slot_demand = demands
        .iter()
        .map(|demand| demand.unpartitioned_page_slot_demand)
        .max()
        .context("chapter-intro composite has no unpartitioned page demand")?;
    let measured_completed_page_count = demands
        .iter()
        .map(|demand| demand.completed_page_count)
        .sum();
    let page_granular_reload_required =
        maximum_resident_chain.resident_chain_slot_demand > ACTIVE_HANGUL_SLOT_COUNT;
    ensure!(
        page_granular_reload_required,
        "chapter-intro composite no longer needs the declared page-granular reload"
    );
    let evidence = EvidenceDigest {
        schema: 1,
        source_sha1: EXPECTED_SOURCE_SHA1,
        main_dialogue_workspace_sha1: bindings.main_dialogue_workspace_sha1,
        chapter_title_workspace_sha1: bindings.chapter_title_workspace_sha1,
        chapter_context_count: CHAPTER_COUNT,
        measured_chapter_count: demands.len(),
        measured_completed_page_count,
        source_font_page_sha1: source_page.page_sha1(),
        source_page_japanese_text_active_code_count: source_page.japanese_text_codes().len(),
        source_page_preserved_non_japanese_active_code_count: source_page
            .preserved_non_japanese_codes()
            .len(),
        maximum_chapter_index: maximum.chapter_index,
        maximum_transition_chain_record_count: maximum.transition_chain_record_count,
        maximum_chapter_completed_page_count: maximum.completed_page_count,
        maximum_target_glyph_count: maximum.target_glyph_count,
        maximum_preserved_active_source_code_count: maximum.preserved_active_source_code_count,
        maximum_total_slot_demand: maximum.total_slot_demand,
        maximum_unpartitioned_page_slot_demand,
        maximum_resident_chain_chapter_index: maximum_resident_chain.chapter_index,
        maximum_resident_chain_slot_demand: maximum_resident_chain.resident_chain_slot_demand,
        page_granular_reload_required,
        preservation_policy: "for each of the twenty-five source-bound E5 chapter contexts, keep the exact Korean chapter title resident, divide the complete E4/E6 dialogue transition chain into the renderer's four-line completed pages, reclaim the source-bound Japanese text ownership partition, and preserve all ninety-nine non-Japanese active source codes plus current-page encoded literals",
    };
    let evidence_bytes = serde_json::to_vec(&evidence)
        .context("serialize chapter-intro composite lifetime evidence")?;

    Ok(TranslationLifetimeDemandReport {
        screen_role: SCREEN_ROLE,
        measurement_basis: "maximum completed page across all twenty-five E5-bound chapter-title and complete intro-dialogue chains; preserve the full non-Japanese source-font partition and reload at completed-page boundaries",
        target_glyph_count: maximum.target_glyph_count,
        preserved_active_source_code_count: maximum.preserved_active_source_code_count,
        additional_target_glyph_reservation_count: 0,
        total_slot_demand: maximum.total_slot_demand,
        active_slot_count: ACTIVE_HANGUL_SLOT_COUNT,
        fits_active_page: maximum.total_slot_demand <= ACTIVE_HANGUL_SLOT_COUNT,
        evidence_report_sha1: sha1_hex(&evidence_bytes),
    })
}

struct CompletedPageDemand {
    target_glyph_count: usize,
    preserved_active_source_code_count: usize,
    total_slot_demand: usize,
    unpartitioned_slot_demand: usize,
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::dialogue_inventory::{MainDialogueGraphReport, MainDialogueTransitionEdgeReport};

    #[test]
    fn transition_chain_follows_cross_table_targets_from_the_canonical_root() {
        let graph = MainDialogueGraphReport {
            node_count: 3,
            transition_edge_count: 2,
            terminal_reachable_node_count: 3,
            caller_handoff_boundary_reachable_node_count: 0,
            max_transition_edge_count_to_boundary: 2,
            cycle_count: 0,
            unresolved_node_count: 0,
            transition_edges: vec![
                edge("chapter-intro-dialogue", 5, "chapter-intro-dialogue", 7),
                edge("chapter-intro-dialogue", 7, "village-and-outro-dialogue", 2),
            ],
        };

        assert_eq!(
            main_dialogue_transition_chain_record_ids(&graph, "chapter-intro-dialogue", 5).unwrap(),
            [
                "chapter-intro-dialogue:005",
                "chapter-intro-dialogue:007",
                "village-and-outro-dialogue:002",
            ]
        );
    }

    fn edge(
        source_table_id: &'static str,
        source_index: usize,
        target_table_id: &'static str,
        target_index: usize,
    ) -> MainDialogueTransitionEdgeReport {
        MainDialogueTransitionEdgeReport {
            source_table_id,
            source_canonical_entry_index: source_index,
            source_entry_indices: vec![source_index],
            source_pointer_cpu_address: 0,
            source_pointer_cpu_address_hex: "0x0000".to_owned(),
            source_file_offset: 0,
            source_file_offset_hex: "0x00000".to_owned(),
            control: 0xE6,
            control_hex: "E6".to_owned(),
            target_table_id,
            target_entry_index: target_index,
            target_canonical_entry_index: target_index,
            target_pointer_cpu_address: 0,
            target_pointer_cpu_address_hex: "0x0000".to_owned(),
            target_file_offset: 0,
            target_file_offset_hex: "0x00000".to_owned(),
        }
    }
}
