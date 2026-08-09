mod battle_source;
mod ending_epilogue;
mod ending_source;
mod report;
mod runtime_routes;
mod source_binding;
mod source_spec;
#[cfg(test)]
mod tests;
mod translation_surfaces;
mod unit_record_history;

use std::{fs, path::Path};

use anyhow::{Context, Result, ensure};
use serde::Serialize;

use crate::{
    dialogue_inventory::inspect_chapter_intro_contexts,
    rom::{EXPECTED_SOURCE_SHA1, HEADER_SIZE, Rom},
    sha1_hex,
    typed_source::{TypedInstructionBinding, decode_rp2a03_sequence},
};

use report::*;
use runtime_routes::*;
use source_binding::*;
use source_spec::*;
use translation_surfaces::{TranslationSurfaceContracts, bind_translation_surfaces};

fn source_region_specs() -> impl Iterator<Item = SourceRegionSpec> {
    SOURCE_REGIONS
        .iter()
        .chain(ending_source::SOURCE_REGIONS.iter())
        .chain(battle_source::SOURCE_REGIONS.iter())
        .chain(ending_epilogue::SOURCE_REGIONS.iter())
        .chain(unit_record_history::SOURCE_REGIONS.iter())
        .copied()
}

pub struct ChapterTransitionSummary {
    pub report_sha1: String,
    pub screen_count: usize,
    pub chapter_context_count: usize,
    pub chapter_title_count: usize,
    pub chapter_intro_runtime_sample_count: usize,
    pub source_region_count: usize,
    pub next_observation_gate_role: &'static str,
}

pub fn analyze_chapter_transitions(
    source_path: &Path,
    report_path: &Path,
) -> Result<ChapterTransitionSummary> {
    let rom = Rom::from_path(source_path)?;
    rom.verify_supported_japanese()?;
    let report = build_report(&rom)?;
    let mut report_bytes =
        serde_json::to_vec_pretty(&report).context("serialize chapter-transition report")?;
    report_bytes.push(b'\n');
    let report_sha1 = sha1_hex(&report_bytes);

    if let Some(parent) = report_path.parent() {
        fs::create_dir_all(parent)
            .with_context(|| format!("create output directory {}", parent.display()))?;
    }
    fs::write(report_path, report_bytes)
        .with_context(|| format!("write {}", report_path.display()))?;

    Ok(ChapterTransitionSummary {
        report_sha1,
        screen_count: report.observed_screens.len(),
        chapter_context_count: report.chapter_intro_contexts.unique_context_count,
        chapter_title_count: report.chapter_titles.pointer_count,
        chapter_intro_runtime_sample_count: report.chapter_intro_runtime_samples.len(),
        source_region_count: report.source_regions.len(),
        next_observation_gate_role: report.next_universalization_gate,
    })
}

fn build_report(rom: &Rom) -> Result<ChapterTransitionReport> {
    let source_regions = source_region_specs()
        .map(|spec| bind_source_region(rom, spec))
        .collect::<Result<Vec<_>>>()?;
    let chapter_intro_contexts = bind_chapter_intro_contexts(rom)?;
    let chapter_titles = bind_chapter_titles(rom)?;
    let translation_surfaces = bind_translation_surfaces(rom)?;

    Ok(ChapterTransitionReport {
        schema: 1,
        source_sha1: EXPECTED_SOURCE_SHA1,
        scope: Scope {
            translation_direction: "Japanese to Korean",
            preserve_existing_english_and_digits: true,
            dialogue_content_emitted: false,
            proof_boundary: "source-bound chapter context, title, NEXT STORY, both save-choice branches, regular-save checksum producers, terminal-notice sound-test unlock, all sound-test controller effects, and the battle-test and ending state machines; runtime observes every sound-test control, the repeating shared battle lifetimes, and the automatic mixed-language ending through its static terminal phase, plus the chapter-one-to-two sequence, chapter-eleven intro reachability, and continuous accelerated chapter-eleven-victory-to-chapter-twelve-intro route; no dialogue source, translation, or ROM mutation",
        },
        observed_screens: transition_screens(),
        chapter_intro_contexts,
        chapter_titles,
        regular_save_reachability: regular_save_reachability(),
        save_offer_no_branch: save_offer_no_branch_contract(),
        save_complete_no_branch: save_complete_no_branch_contract(),
        sound_test_controls: sound_test_control_contract(),
        unit_record_history: unit_record_history::unit_record_history_contract(),
        translation_surfaces,
        chapter_intro_runtime_samples: chapter_intro_runtime_samples(),
        fixed_labels: vec![
            FixedLabelBinding {
                screen_role: "next_story_banner",
                index: 0x3E,
                index_hex: "0x3E".to_owned(),
                source_text: "NEXT STORY",
                translation_handling: "preserve original English",
                pointer: 0x91FB,
                pointer_hex: "0x91FB".to_owned(),
                composer: location(0x0B, 0x886A),
            },
            FixedLabelBinding {
                screen_role: "chapter_save_offer",
                index: 0x32,
                index_hex: "0x32".to_owned(),
                source_text: "セーブしますか?",
                translation_handling: "translate Japanese only",
                pointer: 0x91AA,
                pointer_hex: "0x91AA".to_owned(),
                composer: location(0x0B, 0x8AE6),
            },
        ],
        source_regions,
        next_universalization_gate: "ending_character_epilogue_variant_union",
        unresolved: vec![
            "The chapter-one epilogue and save-complete dialogue use the main dialogue engine, but their dialogue source content is intentionally outside this public report.",
            "The save-offer no choice and save-complete no choice are source-bound and runtime-observed; the latter opens a terminal power-off notice with a source-bound sound-test unlock.",
            "Every sound-test control and both downstream state machines are source-bound and runtime-observed; the shared battle text tables and writers, ending chapter-record stream, turn interpolation, and character-epilogue dialogue tables are now structurally bound without emitting their content.",
            "The separate battle-dialogue state machine now bounds twenty-eight pointer-referenced EF-terminated records and one unreferenced structural record; the latter remains preserved and is not admitted as a translation target.",
            "The ordinary favorable, unfavorable, and defeat battle routes close battle polarity; the remaining character-epilogue variants keep the complete CHR, glyph, portrait, sprite, and temporal union open.",
            "The accelerated continuous route remains reachability evidence, while the separate no-cheat favorable and unfavorable routes plus the adverse defeat route cover battle polarity without turning acceleration into difficulty evidence.",
            "Chapter-two, chapter-eleven, and chapter-twelve intro samples do not generalize the remaining twenty-two chapters or all title lifetimes.",
        ],
        release_eligible: false,
    })
}
