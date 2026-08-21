use std::{
    collections::{BTreeMap, BTreeSet},
    path::Path,
};

use anyhow::{Context, Result, ensure};

use crate::{
    dialogue_assets::{
        MainDialogueSlicePlan, MaximumDialogueRuntimeEvidence,
        load_maximum_dialogue_runtime_evidence,
    },
    font_slots::{FONT_PAGE_SIZE, active_hangul_codes},
    rom::Rom,
    sha1_hex,
};

use super::{
    dialogue_probe_font::{assign_glyph_codes_excluding, build_font_page},
    encode_chr_page_register,
    maximum_dialogue_boundary::load_observed_page_boundaries,
};

pub(super) const SCREEN_ROLE: &str = "chapter_7_castle_clear_maximum_dialogue";
pub(super) const TARGET_RECORD_ID: &str = "village-and-outro-dialogue:024";
pub(super) const DISPLAY_LINES_PER_PAGE: usize = 4;
pub(super) const COMPLETED_PAGE_COUNT: usize = 15;
pub(super) const PAGES_PER_FONT_GROUP: usize = 5;
pub(super) const FONT_GROUP_COUNT: usize = 3;

const SOURCE_FONT_PHYSICAL_PAGE: usize = 2;
const APPENDED_PHYSICAL_PAGE_COUNT: usize = 4;

pub(super) struct MaximumDialoguePagePlan {
    pub(super) encoded_record: Vec<u8>,
    pub(super) assignments: Vec<BTreeMap<char, u8>>,
    pub(super) page_groups: Vec<usize>,
    pub(super) group_page_counts: Vec<usize>,
    pub(super) group_unique_glyph_counts: Vec<usize>,
    pub(super) page_pack: Vec<u8>,
    pub(super) physical_chr_pages: Vec<u8>,
    pub(super) mapper_registers: Vec<u8>,
    pub(super) font_page_sha1s: Vec<String>,
    pub(super) completed_page_pointers: Vec<u16>,
    pub(super) group_transition_pointers: [u16; 2],
    pub(super) evidence_manifest_sha1: String,
    pub(super) page_boundary_manifest_sha1: String,
    pub(super) record_page_boundary_topology_sha1: String,
    pub(super) boundary_observation_output_sha1: String,
    pub(super) temporal_sample_count: usize,
    pub(super) unique_nametable_count: usize,
    pub(super) preserved_screen_active_code_count: usize,
    pub(super) preserved_source_active_code_count: usize,
    pub(super) preserved_active_code_count: usize,
}

pub(super) fn plan_maximum_dialogue_pages(
    cumulative_rom: &Rom,
    record: &MainDialogueSlicePlan,
    evidence_path: &Path,
    page_boundary_path: &Path,
) -> Result<MaximumDialoguePagePlan> {
    ensure!(
        record.record_id == TARGET_RECORD_ID,
        "maximum dialogue page plan targets a different record"
    );
    ensure!(
        record.line_count() == 57,
        "maximum dialogue line count changed"
    );
    let evidence: MaximumDialogueRuntimeEvidence =
        load_maximum_dialogue_runtime_evidence(evidence_path, record.line_count())?;
    ensure!(
        evidence.completed_page_count == COMPLETED_PAGE_COUNT,
        "maximum dialogue completed-page count changed"
    );
    let observed_boundaries = load_observed_page_boundaries(page_boundary_path, record)?;
    let completed_page_pointers = record.completed_page_pointers(DISPLAY_LINES_PER_PAGE)?;

    let page_glyphs = record.page_unique_glyphs(&completed_page_pointers)?;
    ensure!(
        page_glyphs.len() == COMPLETED_PAGE_COUNT,
        "maximum dialogue page partition changed"
    );
    let page_groups = (0..COMPLETED_PAGE_COUNT)
        .map(|page_index| page_index / PAGES_PER_FONT_GROUP)
        .collect::<Vec<_>>();
    ensure!(
        page_groups.last().copied() == Some(FONT_GROUP_COUNT - 1),
        "maximum dialogue font-group partition changed"
    );
    let mut group_glyphs = vec![BTreeSet::new(); FONT_GROUP_COUNT];
    for (page, group) in page_glyphs.iter().zip(&page_groups) {
        group_glyphs[*group].extend(page.iter().copied());
    }

    let active_codes = active_hangul_codes().into_iter().collect::<BTreeSet<_>>();
    let preserved_screen_active_codes = evidence
        .screen_codes
        .intersection(&active_codes)
        .copied()
        .collect::<BTreeSet<_>>();
    let preserved_source_active_codes = record
        .preserved_source_codes
        .intersection(&active_codes)
        .copied()
        .collect::<BTreeSet<_>>();
    let preserved_active_codes = preserved_screen_active_codes
        .union(&preserved_source_active_codes)
        .copied()
        .collect::<BTreeSet<_>>();
    let assignments = group_glyphs
        .iter()
        .map(|glyphs| assign_glyph_codes_excluding(glyphs, &preserved_active_codes))
        .collect::<Result<Vec<_>>>()?;
    let encoded_record =
        record.encoded_bytes_by_page_group(&completed_page_pointers, &page_groups, &assignments)?;
    ensure!(
        encoded_record.len() <= record.source_storage_byte_count,
        "maximum dialogue page encoding exceeds its owned storage"
    );

    ensure!(
        cumulative_rom.chr().len().is_multiple_of(FONT_PAGE_SIZE),
        "maximum dialogue cumulative CHR is not 4 KiB aligned"
    );
    let first_physical_page = cumulative_rom.chr().len() / FONT_PAGE_SIZE;
    ensure!(
        first_physical_page.is_multiple_of(2),
        "maximum dialogue CHR extension must begin on an 8 KiB boundary"
    );
    let source_font_start = SOURCE_FONT_PHYSICAL_PAGE * FONT_PAGE_SIZE;
    let source_fd_page = cumulative_rom
        .chr()
        .get(source_font_start..source_font_start + FONT_PAGE_SIZE)
        .context("maximum dialogue source FD font page is outside CHR")?;
    let source_fe_page = cumulative_rom
        .chr()
        .get(source_font_start + FONT_PAGE_SIZE..source_font_start + 2 * FONT_PAGE_SIZE)
        .context("maximum dialogue source FE font page is outside CHR")?;
    let mut page_pack = Vec::with_capacity(APPENDED_PHYSICAL_PAGE_COUNT * FONT_PAGE_SIZE);
    let mut font_page_sha1s = Vec::with_capacity(FONT_GROUP_COUNT);
    for assignment in &assignments {
        let page = build_font_page(source_fd_page, assignment)?;
        font_page_sha1s.push(sha1_hex(&page));
        page_pack.extend_from_slice(&page);
    }
    page_pack.extend_from_slice(source_fe_page);
    ensure!(
        page_pack.len() == APPENDED_PHYSICAL_PAGE_COUNT * FONT_PAGE_SIZE,
        "maximum dialogue extension must occupy two CHR banks"
    );

    let physical_chr_pages = (0..FONT_GROUP_COUNT)
        .map(|index| {
            u8::try_from(first_physical_page + index)
                .context("maximum dialogue physical CHR page does not fit u8")
        })
        .collect::<Result<Vec<_>>>()?;
    let mapper_registers = physical_chr_pages
        .iter()
        .map(|page| encode_chr_page_register(*page))
        .collect::<Result<Vec<_>>>()?;
    ensure!(
        completed_page_pointers.len() == COMPLETED_PAGE_COUNT
            && completed_page_pointers
                .windows(2)
                .all(|pair| pair[0] < pair[1]),
        "maximum dialogue completed-page pointers are not strictly increasing"
    );
    let group_transition_pointers = [
        completed_page_pointers[PAGES_PER_FONT_GROUP - 1],
        completed_page_pointers[2 * PAGES_PER_FONT_GROUP - 1],
    ];
    let group_page_counts = (0..FONT_GROUP_COUNT)
        .map(|group| {
            page_groups
                .iter()
                .filter(|candidate| **candidate == group)
                .count()
        })
        .collect();

    Ok(MaximumDialoguePagePlan {
        encoded_record,
        assignments,
        page_groups,
        group_page_counts,
        group_unique_glyph_counts: group_glyphs.iter().map(BTreeSet::len).collect(),
        page_pack,
        physical_chr_pages,
        mapper_registers,
        font_page_sha1s,
        completed_page_pointers,
        group_transition_pointers,
        evidence_manifest_sha1: evidence.manifest_sha1,
        page_boundary_manifest_sha1: observed_boundaries.manifest_sha1,
        record_page_boundary_topology_sha1: record.page_boundary_topology_sha1(),
        boundary_observation_output_sha1: observed_boundaries.observation_output_sha1,
        temporal_sample_count: evidence.temporal_sample_count,
        unique_nametable_count: evidence.unique_nametable_count,
        preserved_screen_active_code_count: preserved_screen_active_codes.len(),
        preserved_source_active_code_count: preserved_source_active_codes.len(),
        preserved_active_code_count: preserved_active_codes.len(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fifteen_observed_pages_form_three_contiguous_five_page_groups() {
        let groups = (0..COMPLETED_PAGE_COUNT)
            .map(|page_index| page_index / PAGES_PER_FONT_GROUP)
            .collect::<Vec<_>>();

        assert_eq!(groups, [0, 0, 0, 0, 0, 1, 1, 1, 1, 1, 2, 2, 2, 2, 2]);
        assert_eq!(groups.iter().copied().max(), Some(FONT_GROUP_COUNT - 1));
    }
}
