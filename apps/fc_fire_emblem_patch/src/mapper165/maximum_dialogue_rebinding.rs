use std::path::Path;

use anyhow::{Context, Result, ensure};

use crate::{
    dialogue_assets::plan_main_dialogue_slice, font_slots::FONT_PAGE_SIZE, rom::Rom, sha1_hex,
};

use super::{
    OUTPUT_MAPPER,
    maximum_dialogue_boundary::load_observed_page_boundaries,
    maximum_dialogue_page::{
        DISPLAY_LINES_PER_PAGE, FONT_GROUP_COUNT, PAGES_PER_FONT_GROUP, TARGET_RECORD_ID,
    },
};

const FIRST_MAXIMUM_DIALOGUE_FONT_PAGE: usize = 50;

#[derive(Debug)]
pub(crate) struct MaximumDialogueBoundaryRebindingSummary {
    pub(crate) reference_output_sha1: String,
    pub(crate) candidate_output_sha1: String,
    pub(crate) record_page_boundary_topology_sha1: String,
    pub(crate) page_count: usize,
    pub(crate) logical_byte_count: usize,
    pub(crate) target_glyph_byte_count: usize,
}

pub(crate) fn verify_maximum_dialogue_boundary_rebinding(
    source_path: &Path,
    workspace_path: &Path,
    page_boundary_path: &Path,
    reference_output_path: &Path,
    candidate_output_path: &Path,
) -> Result<MaximumDialogueBoundaryRebindingSummary> {
    let source = Rom::from_path(source_path)?;
    source.verify_supported_japanese()?;
    let record = plan_main_dialogue_slice(&source, workspace_path, TARGET_RECORD_ID)?;
    load_observed_page_boundaries(page_boundary_path, &record)?;
    let completed_page_pointers = record.completed_page_pointers(DISPLAY_LINES_PER_PAGE)?;
    let page_groups = (0..completed_page_pointers.len())
        .map(|page_index| page_index / PAGES_PER_FONT_GROUP)
        .collect::<Vec<_>>();
    ensure!(
        page_groups.last().copied() == Some(FONT_GROUP_COUNT - 1),
        "maximum dialogue rebinding font groups changed"
    );

    let reference = Rom::from_path(reference_output_path)?;
    let candidate = Rom::from_path(candidate_output_path)?;
    validate_output_layout(&reference, "reference")?;
    validate_output_layout(&candidate, "candidate")?;

    let record_start = record.source_file_offset;
    let record_length = completed_page_pointers
        .last()
        .context("maximum dialogue rebinding has no completed page")?
        .checked_sub(record.source_pointer_cpu_address())
        .context("maximum dialogue rebinding final pointer precedes the record")?;
    let record_end = record_start
        .checked_add(usize::from(record_length))
        .context("maximum dialogue rebinding record range overflow")?;
    let reference_record = reference
        .data()
        .get(record_start..record_end)
        .context("reference maximum dialogue record is outside PRG")?;
    let candidate_record = candidate
        .data()
        .get(record_start..record_end)
        .context("candidate maximum dialogue record is outside PRG")?;
    let candidate_pages = maximum_dialogue_font_pages(&candidate)?;

    let reference_target_count =
        record.verify_encoded_page_topology(reference_record, &completed_page_pointers)?;
    let candidate_target_count = record.verify_encoded_page_rendering(
        candidate_record,
        &completed_page_pointers,
        &page_groups,
        &candidate_pages,
    )?;
    ensure!(
        reference_target_count == candidate_target_count,
        "maximum dialogue target-glyph byte count changed"
    );

    Ok(MaximumDialogueBoundaryRebindingSummary {
        reference_output_sha1: sha1_hex(reference.data()),
        candidate_output_sha1: sha1_hex(candidate.data()),
        record_page_boundary_topology_sha1: record.page_boundary_topology_sha1(),
        page_count: completed_page_pointers.len(),
        logical_byte_count: reference_record.len(),
        target_glyph_byte_count: reference_target_count,
    })
}

fn validate_output_layout(rom: &Rom, role: &str) -> Result<()> {
    ensure!(
        rom.mapper() == OUTPUT_MAPPER,
        "maximum dialogue {role} output is not mapper 165"
    );
    ensure!(
        rom.chr().len() >= (FIRST_MAXIMUM_DIALOGUE_FONT_PAGE + FONT_GROUP_COUNT) * FONT_PAGE_SIZE,
        "maximum dialogue {role} output lacks the installed font pages"
    );
    Ok(())
}

fn maximum_dialogue_font_pages(rom: &Rom) -> Result<Vec<&[u8]>> {
    (0..FONT_GROUP_COUNT)
        .map(|group| {
            let start = (FIRST_MAXIMUM_DIALOGUE_FONT_PAGE + group) * FONT_PAGE_SIZE;
            rom.chr()
                .get(start..start + FONT_PAGE_SIZE)
                .context("maximum dialogue installed font page is outside CHR")
        })
        .collect()
}
