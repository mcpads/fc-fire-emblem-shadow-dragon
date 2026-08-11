use std::path::Path;

use anyhow::{Context, Result, ensure};
use serde::Serialize;

use crate::{
    chapter_transition::{
        bind_ending_chapter_record_lifetime_source, plan_chapter_titles, plan_transition_labels,
    },
    font_slots::ACTIVE_HANGUL_SLOT_COUNT,
    rom::{EXPECTED_SOURCE_SHA1, Rom},
    sha1_hex,
};

use super::super::report::TranslationLifetimeDemandReport;

pub(super) struct InputBindings<'a> {
    pub(super) source_path: &'a Path,
    pub(super) chapter_title_workspace_path: &'a Path,
    pub(super) transition_label_workspace_path: &'a Path,
    pub(super) chapter_title_workspace_sha1: &'a str,
    pub(super) transition_label_workspace_sha1: &'a str,
}

#[derive(Serialize)]
struct EvidenceDigest<'a> {
    schema: u8,
    source_sha1: &'static str,
    chapter_title_workspace_sha1: &'a str,
    transition_label_workspace_sha1: &'a str,
    stream_record_count: usize,
    target_record_count: usize,
    chapter_title_count: usize,
    source_reclaimable_active_code_count: usize,
    preserved_active_stream_code_count: usize,
    target_glyph_count: usize,
    preservation_policy: &'static str,
}

pub(super) fn inspect(bindings: InputBindings<'_>) -> Result<TranslationLifetimeDemandReport> {
    let rom = Rom::from_path(bindings.source_path)?;
    rom.verify_supported_japanese()?;
    let chapter_titles = plan_chapter_titles(&rom, bindings.chapter_title_workspace_path)?;
    let transition_labels = plan_transition_labels(&rom, bindings.transition_label_workspace_path)?;
    ensure!(
        chapter_titles.workspace_sha1 == bindings.chapter_title_workspace_sha1
            && transition_labels.ending_record.workspace_sha1
                == bindings.transition_label_workspace_sha1
            && chapter_titles.entry_count == 25
            && chapter_titles.translated_entry_count == 25
            && transition_labels.ending_record.entry_count == 1,
        "ending chapter-record lifetime translation input changed"
    );
    let source = bind_ending_chapter_record_lifetime_source(&rom)?;
    ensure!(
        transition_labels
            .ending_record
            .source_reclaimable_active_codes
            .is_subset(&source.source_reclaimable_active_codes),
        "ending aggregate label is not contained in the chapter-record source lifetime"
    );

    let mut target_glyphs = chapter_titles.unique_glyphs();
    target_glyphs.extend(transition_labels.ending_record.target_glyphs);
    let preserved_active_stream_codes = source.preserved_active_stream_codes;
    let total_slot_demand = target_glyphs
        .len()
        .checked_add(preserved_active_stream_codes.len())
        .context("ending chapter-record lifetime slot demand overflow")?;
    ensure!(
        total_slot_demand <= ACTIVE_HANGUL_SLOT_COUNT,
        "ending_chapter_record_scroll needs {total_slot_demand} active slots but only {ACTIVE_HANGUL_SLOT_COUNT} exist"
    );

    let evidence = EvidenceDigest {
        schema: 1,
        source_sha1: EXPECTED_SOURCE_SHA1,
        chapter_title_workspace_sha1: bindings.chapter_title_workspace_sha1,
        transition_label_workspace_sha1: bindings.transition_label_workspace_sha1,
        stream_record_count: source.record_count,
        target_record_count: source.target_record_count,
        chapter_title_count: chapter_titles.entry_count,
        source_reclaimable_active_code_count: source.source_reclaimable_active_codes.len(),
        preserved_active_stream_code_count: preserved_active_stream_codes.len(),
        target_glyph_count: target_glyphs.len(),
        preservation_policy: "keep the complete twenty-five-title and total-turn Korean glyph union resident; preserve every active literal from the source-bound text-only scroll outside the translated records; chapter and turn digits remain globally reserved",
    };
    let evidence_bytes = serde_json::to_vec(&evidence)
        .context("serialize ending chapter-record lifetime evidence")?;

    Ok(TranslationLifetimeDemandReport {
        screen_role: "ending_chapter_record_scroll",
        measurement_basis: "complete twenty-five-title and total-turn Korean glyph union plus exact preserved active literals from the source-bound text-only scroll",
        target_glyph_count: target_glyphs.len(),
        preserved_active_source_code_count: preserved_active_stream_codes.len(),
        additional_target_glyph_reservation_count: 0,
        total_slot_demand,
        active_slot_count: ACTIVE_HANGUL_SLOT_COUNT,
        fits_active_page: true,
        evidence_report_sha1: sha1_hex(&evidence_bytes),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn complete_ending_chapter_record_translation_fits_one_page() {
        let source = Path::new(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../../roms/Fire Emblem - Ankoku Ryuu to Hikari no Tsurugi (Japan).nes"
        ));
        if !source.exists() {
            return;
        }
        let chapter_titles = Path::new(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../../assets/translation/chapter-titles.ko.json"
        ));
        let transition_labels = Path::new(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../../assets/translation/transition-labels.ko.json"
        ));
        let chapter_title_sha1 = sha1_hex(&std::fs::read(chapter_titles).unwrap());
        let transition_label_sha1 = sha1_hex(&std::fs::read(transition_labels).unwrap());
        let demand = inspect(InputBindings {
            source_path: source,
            chapter_title_workspace_path: chapter_titles,
            transition_label_workspace_path: transition_labels,
            chapter_title_workspace_sha1: &chapter_title_sha1,
            transition_label_workspace_sha1: &transition_label_sha1,
        })
        .unwrap();

        assert_eq!(demand.target_glyph_count, 91);
        assert_eq!(demand.preserved_active_source_code_count, 0);
        assert_eq!(demand.total_slot_demand, 91);
    }
}
