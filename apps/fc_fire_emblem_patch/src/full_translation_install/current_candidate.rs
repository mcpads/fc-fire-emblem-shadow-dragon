use std::{fs, path::Path};

use anyhow::{Context, Result, ensure};
use serde::Deserialize;

use crate::{
    font_slots::FONT_PAGE_SIZE,
    mapper165::{MAXIMUM_CHR_PAGE_COUNT, cumulative_patch::REPORT_SCHEMA},
    rom::{EXPECTED_SOURCE_SHA1, HEADER_SIZE, Rom},
    sha1_hex,
};

const FIXED_BANK_SIZE: usize = 16 * 1024;
const FONT_GROUP_SELECTOR_START: u16 = 0xF341;
const FONT_GROUP_SELECTOR_END: u16 = 0xF378;
const INITIAL_SELECTOR_START: u16 = 0xF990;
const INITIAL_SELECTOR_END: u16 = 0xFA00;

pub(super) struct CurrentCandidateInputs<'a> {
    pub(super) source_rom: &'a Rom,
    pub(super) candidate_path: &'a Path,
    pub(super) build_report_path: &'a Path,
}

pub(super) struct DialoguePagePoolCapacity {
    pub(super) current_candidate_sha1: String,
    pub(super) current_chr_page_count: usize,
    pub(super) first_installable_physical_page: u8,
    pub(super) superseded_maximum_dialogue_page_count: usize,
    pub(super) appendable_page_count: usize,
    pub(super) available_page_count: usize,
    pub(super) battle_glyph_atlas_tile_count: usize,
    pub(super) battle_maximum_ppu_write_count: usize,
    pub(super) battle_runtime_routine_byte_count: usize,
    pub(super) battle_runtime_bound_to_build: bool,
    pub(super) maximum_dialogue_font_group_selector_range_sha1: String,
    pub(super) maximum_dialogue_initial_selector_range_sha1: String,
}

#[derive(Deserialize)]
struct CurrentBuildReport {
    schema: u8,
    source_sha1: String,
    output_sha1: String,
    output_mapper: u16,
    prg_size: usize,
    chr_size: usize,
    main_dialogue: CurrentDialogueReport,
    battle_text: CurrentBattleTextReport,
}

#[derive(Deserialize)]
struct CurrentDialogueReport {
    maximum_page_reloaded_lifetime: CurrentMaximumDialogueReport,
}

#[derive(Deserialize)]
struct CurrentMaximumDialogueReport {
    font_physical_pages: Vec<u8>,
    font_page_sha1s: Vec<String>,
    font_page_pack_sha1: String,
    font_group_selector_range_sha1: String,
    initial_selector_range_sha1: String,
    completed_page_reload_installed: bool,
}

#[derive(Deserialize)]
struct CurrentBattleTextReport {
    glyph_atlas_tile_count: usize,
    maximum_observed_ppu_write_count: usize,
    runtime_routine_byte_count: usize,
    runtime_bound_to_build: bool,
}

pub(super) fn inspect_dialogue_page_pool_capacity(
    inputs: CurrentCandidateInputs<'_>,
) -> Result<DialoguePagePoolCapacity> {
    let report_bytes = fs::read(inputs.build_report_path)
        .with_context(|| format!("read {}", inputs.build_report_path.display()))?;
    let report: CurrentBuildReport = serde_json::from_slice(&report_bytes)
        .with_context(|| format!("parse {}", inputs.build_report_path.display()))?;
    let candidate = Rom::from_path(inputs.candidate_path)?;
    let candidate_sha1 = sha1_hex(candidate.data());
    ensure!(
        report.schema == REPORT_SCHEMA
            && report.source_sha1 == EXPECTED_SOURCE_SHA1
            && report.output_sha1 == candidate_sha1
            && report.output_mapper == candidate.mapper()
            && report.prg_size == candidate.prg().len()
            && report.chr_size == candidate.chr().len(),
        "current cumulative build report does not describe the candidate ROM"
    );
    ensure!(
        candidate.mapper() == 165 && candidate.chr().len().is_multiple_of(FONT_PAGE_SIZE),
        "current cumulative candidate is not a 4 KiB-aligned mapper 165 ROM"
    );
    let current_chr_page_count = candidate.chr().len() / FONT_PAGE_SIZE;
    ensure!(
        current_chr_page_count <= usize::from(MAXIMUM_CHR_PAGE_COUNT),
        "current cumulative candidate already exceeds mapper 165 CHR capacity"
    );

    let battle_text = report.battle_text;
    let maximum = report.main_dialogue.maximum_page_reloaded_lifetime;
    ensure!(
        maximum.completed_page_reload_installed
            && maximum.font_physical_pages.len() == maximum.font_page_sha1s.len()
            && !maximum.font_physical_pages.is_empty()
            && maximum
                .font_physical_pages
                .windows(2)
                .all(|pages| pages[1] == pages[0] + 1),
        "current maximum-dialogue font pages are not a contiguous installed range"
    );
    let first_installable_physical_page = maximum.font_physical_pages[0];
    let padding_page = maximum
        .font_physical_pages
        .last()
        .copied()
        .context("maximum-dialogue report has no font page")?
        .checked_add(1)
        .context("maximum-dialogue padding page overflow")?;
    ensure!(
        usize::from(padding_page) + 1 == current_chr_page_count,
        "maximum-dialogue pages are not the replaceable tail of the current candidate"
    );
    let page_pack_start = usize::from(first_installable_physical_page) * FONT_PAGE_SIZE;
    ensure!(
        sha1_hex(&candidate.chr()[page_pack_start..]) == maximum.font_page_pack_sha1,
        "current maximum-dialogue tail no longer matches its build report"
    );
    for (page, expected_sha1) in maximum
        .font_physical_pages
        .iter()
        .zip(&maximum.font_page_sha1s)
    {
        let start = usize::from(*page) * FONT_PAGE_SIZE;
        ensure!(
            sha1_hex(&candidate.chr()[start..start + FONT_PAGE_SIZE]) == *expected_sha1,
            "current maximum-dialogue font page {page} no longer matches its build report"
        );
    }
    let source_fe_page = inputs
        .source_rom
        .chr()
        .get(FONT_PAGE_SIZE..2 * FONT_PAGE_SIZE)
        .context("supported source has no FE font page")?;
    let padding_start = usize::from(padding_page) * FONT_PAGE_SIZE;
    ensure!(
        candidate.chr()[padding_start..padding_start + FONT_PAGE_SIZE] == *source_fe_page,
        "current maximum-dialogue alignment page is not the source FE page"
    );
    bind_selector_range(
        &candidate,
        FONT_GROUP_SELECTOR_START,
        FONT_GROUP_SELECTOR_END,
        &maximum.font_group_selector_range_sha1,
        "font-group selector",
    )?;
    bind_selector_range(
        &candidate,
        INITIAL_SELECTOR_START,
        INITIAL_SELECTOR_END,
        &maximum.initial_selector_range_sha1,
        "initial selector",
    )?;

    let first_installable_page = usize::from(first_installable_physical_page);
    Ok(DialoguePagePoolCapacity {
        current_candidate_sha1: candidate_sha1,
        current_chr_page_count,
        first_installable_physical_page,
        superseded_maximum_dialogue_page_count: current_chr_page_count - first_installable_page,
        appendable_page_count: usize::from(MAXIMUM_CHR_PAGE_COUNT) - current_chr_page_count,
        available_page_count: usize::from(MAXIMUM_CHR_PAGE_COUNT) - first_installable_page,
        battle_glyph_atlas_tile_count: battle_text.glyph_atlas_tile_count,
        battle_maximum_ppu_write_count: battle_text.maximum_observed_ppu_write_count,
        battle_runtime_routine_byte_count: battle_text.runtime_routine_byte_count,
        battle_runtime_bound_to_build: battle_text.runtime_bound_to_build,
        maximum_dialogue_font_group_selector_range_sha1: maximum.font_group_selector_range_sha1,
        maximum_dialogue_initial_selector_range_sha1: maximum.initial_selector_range_sha1,
    })
}

fn bind_selector_range(
    candidate: &Rom,
    start: u16,
    end: u16,
    expected_sha1: &str,
    role: &str,
) -> Result<()> {
    ensure!(start >= 0xC000 && start < end, "invalid {role} range");
    let fixed_start = candidate
        .prg()
        .len()
        .checked_sub(FIXED_BANK_SIZE)
        .context("current candidate PRG is smaller than one fixed bank")?;
    let start_offset = HEADER_SIZE + fixed_start + usize::from(start - 0xC000);
    let end_offset = HEADER_SIZE + fixed_start + usize::from(end - 0xC000);
    let bytes = candidate
        .data()
        .get(start_offset..end_offset)
        .with_context(|| format!("current maximum-dialogue {role} is outside the candidate"))?;
    ensure!(
        sha1_hex(bytes) == expected_sha1,
        "current maximum-dialogue {role} no longer matches its build report"
    );
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn selector_ownership_follows_the_exact_report_bound_candidate() {
        let original_bytes = crate::test_support::synthetic_mapper165_rom_bytes(0xFF);
        let original = Rom::parse(original_bytes.clone()).unwrap();
        let fixed_start = original.prg().len() - FIXED_BANK_SIZE;
        let selector_offset =
            HEADER_SIZE + fixed_start + usize::from(FONT_GROUP_SELECTOR_START - 0xC000);
        let selector_end =
            HEADER_SIZE + fixed_start + usize::from(FONT_GROUP_SELECTOR_END - 0xC000);
        let original_sha1 = sha1_hex(&original_bytes[selector_offset..selector_end]);
        bind_selector_range(
            &original,
            FONT_GROUP_SELECTOR_START,
            FONT_GROUP_SELECTOR_END,
            &original_sha1,
            "font-group selector",
        )
        .unwrap();

        let mut changed_bytes = original_bytes;
        changed_bytes[selector_offset] = 0xEA;
        let changed = Rom::parse(changed_bytes.clone()).unwrap();
        let changed_sha1 = sha1_hex(&changed_bytes[selector_offset..selector_end]);
        assert_ne!(changed_sha1, original_sha1);
        bind_selector_range(
            &changed,
            FONT_GROUP_SELECTOR_START,
            FONT_GROUP_SELECTOR_END,
            &changed_sha1,
            "font-group selector",
        )
        .unwrap();
        assert!(
            bind_selector_range(
                &changed,
                FONT_GROUP_SELECTOR_START,
                FONT_GROUP_SELECTOR_END,
                &original_sha1,
                "font-group selector",
            )
            .is_err()
        );
    }
}
