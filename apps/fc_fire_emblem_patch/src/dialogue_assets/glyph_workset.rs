use std::collections::BTreeSet;

use anyhow::Result;

use crate::font_slots::active_hangul_codes;

use super::*;

mod report;
#[cfg(test)]
mod tests;

use report::{
    GlyphCapacityReport, GlyphSetReport, GlyphWorksetScope, GlyphWorksetStatusCounts,
    MainDialogueGlyphWorksetReport,
};

pub(crate) struct MainDialogueGlyphWorksetSummary {
    pub report_sha1: String,
    pub filled_line_count: usize,
    pub complete_line_count: usize,
    pub filled_unique_glyph_count: usize,
    pub approved_unique_glyph_count: usize,
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

    let report = build_glyph_workset_report(&workspace, sha1_hex(&workspace_bytes))?;
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
        working_set_ready: report.capacity.working_set_ready,
    })
}

fn build_glyph_workset_report(
    workspace: &MainDialogueWorkspace,
    workspace_sha1: String,
) -> Result<MainDialogueGlyphWorksetReport> {
    let mut status_counts = GlyphWorksetStatusCounts::default();
    let mut filled_glyphs = BTreeSet::new();
    let mut approved_glyphs = BTreeSet::new();
    let mut target_glyph_occurrence_count = 0;
    let mut max_line_unique_glyph_count = 0;
    let mut max_record_unique_glyph_count = 0;

    for record in &workspace.records {
        let mut record_glyphs = BTreeSet::new();
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
                approved_glyphs.extend(line_unique_glyphs);
            }
        }
        max_record_unique_glyph_count = max_record_unique_glyph_count.max(record_glyphs.len());
    }

    let line_count = status_counts.total();
    let active_slot_count = active_hangul_codes().len();
    let translation_input_complete = line_count > 0 && status_counts.complete == line_count;
    let working_set_ready = translation_input_complete;
    let approved_single_page_fit =
        working_set_ready.then_some(approved_glyphs.len() <= active_slot_count);
    let unresolved = if working_set_ready {
        vec![
            "screen-lifetime and line-width checks remain separate from the glyph working-set count",
        ]
    } else {
        vec![
            "reviewed Korean translation input is incomplete, so the approved working set is not final",
            "screen-lifetime and line-width checks remain separate from the glyph working-set count",
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
        capacity: GlyphCapacityReport {
            active_slot_count,
            translation_input_complete,
            working_set_ready,
            filled_set_fits_one_page_so_far: filled_glyphs.len() <= active_slot_count,
            approved_single_page_fit,
            final_page_plan_eligible: working_set_ready,
        },
        unresolved,
        release_eligible: false,
    })
}

fn glyph_set_report(glyphs: &BTreeSet<char>) -> GlyphSetReport {
    let encoded = glyphs.iter().collect::<String>();
    GlyphSetReport {
        unique_count: glyphs.len(),
        sorted_set_sha1: sha1_hex(encoded.as_bytes()),
    }
}
