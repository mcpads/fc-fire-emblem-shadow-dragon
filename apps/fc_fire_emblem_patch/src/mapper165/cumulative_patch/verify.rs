use anyhow::{Context, Result, ensure};

use crate::{
    chapter_transition::ChapterTitlePlannedEntry,
    dialogue_assets::MainDialogueSlicePlan,
    mmc5_prg::fixed_bank_file_offset,
    rom::{PRG_SIZE, Rom},
    tracked::TrackedImage,
};

use super::{OUTPUT_MAPPER, ROSTER_SELECTOR_ADDRESS};

pub(super) fn install_dialogue_record(
    image: &mut TrackedImage,
    base: &[u8],
    plan: &MainDialogueSlicePlan,
    encoded_record: &[u8],
) -> Result<()> {
    let end = plan
        .source_file_offset
        .checked_add(encoded_record.len())
        .context("cumulative dialogue record range overflow")?;
    let expected = base
        .get(plan.source_file_offset..end)
        .context("cumulative dialogue record is outside the UI stage")?;
    image.write_expected(
        format!("cumulative main dialogue record {}", plan.record_id),
        plan.source_file_offset,
        expected,
        encoded_record,
    )
}

pub(super) fn install_chapter_title(
    image: &mut TrackedImage,
    base: &[u8],
    plan: &ChapterTitlePlannedEntry,
    encoded_title: &[u8],
) -> Result<()> {
    ensure!(
        encoded_title.len() == plan.source_storage_byte_count,
        "cumulative chapter title changed its owned storage size"
    );
    let end = plan
        .file_offset
        .checked_add(encoded_title.len())
        .context("cumulative chapter-title range overflow")?;
    let expected = base
        .get(plan.file_offset..end)
        .context("cumulative chapter title is outside the stage")?;
    image.write_expected(
        format!("cumulative chapter title {}", plan.id),
        plan.file_offset,
        expected,
        encoded_title,
    )
}

pub(super) struct CumulativeOutputVerificationInputs<'a> {
    pub(super) ui_stage_rom: &'a Rom,
    pub(super) output_rom: &'a Rom,
    pub(super) page_pack: &'a [u8],
    pub(super) record_groups: &'a [(&'a [MainDialogueSlicePlan], &'a [Vec<u8>])],
    pub(super) chapter_titles: &'a [(&'a ChapterTitlePlannedEntry, &'a [u8])],
    pub(super) roster_selector: &'a [u8],
    pub(super) dialogue_selector_address: u16,
    pub(super) dialogue_selector: &'a [u8],
}

pub(super) fn verify_cumulative_output(
    inputs: CumulativeOutputVerificationInputs<'_>,
) -> Result<()> {
    let CumulativeOutputVerificationInputs {
        ui_stage_rom,
        output_rom,
        page_pack,
        record_groups,
        chapter_titles,
        roster_selector,
        dialogue_selector_address,
        dialogue_selector,
    } = inputs;
    ensure!(
        output_rom.mapper() == OUTPUT_MAPPER,
        "cumulative output mapper changed"
    );
    ensure!(
        output_rom.prg().len() == PRG_SIZE,
        "cumulative output PRG size changed"
    );
    ensure!(
        output_rom.chr().len() == ui_stage_rom.chr().len() + page_pack.len(),
        "cumulative output CHR size changed"
    );
    ensure!(
        output_rom.chr()[..ui_stage_rom.chr().len()] == *ui_stage_rom.chr(),
        "cumulative dialogue stage changed an earlier CHR page"
    );
    ensure!(
        output_rom.chr()[ui_stage_rom.chr().len()..] == *page_pack,
        "cumulative dialogue stage appended different page bytes"
    );
    for (plans, encoded_records) in record_groups {
        ensure!(
            plans.len() == encoded_records.len(),
            "cumulative dialogue verification lost an encoded record"
        );
        for (plan, encoded_record) in plans.iter().zip(*encoded_records) {
            ensure!(
                output_rom.data()
                    [plan.source_file_offset..plan.source_file_offset + encoded_record.len()]
                    == *encoded_record,
                "cumulative output record {} changed",
                plan.record_id
            );
        }
    }
    for (plan, encoded_title) in chapter_titles {
        ensure!(
            output_rom.data()[plan.file_offset..plan.file_offset + encoded_title.len()]
                == **encoded_title,
            "cumulative output chapter title {} changed",
            plan.id
        );
    }
    let roster_selector_offset = fixed_bank_file_offset(ROSTER_SELECTOR_ADDRESS)?;
    ensure!(
        output_rom.data()[roster_selector_offset..roster_selector_offset + roster_selector.len()]
            == *roster_selector,
        "cumulative roster selector chain changed"
    );
    let dialogue_selector_offset = fixed_bank_file_offset(dialogue_selector_address)?;
    ensure!(
        output_rom.data()
            [dialogue_selector_offset..dialogue_selector_offset + dialogue_selector.len()]
            == *dialogue_selector,
        "cumulative dialogue selector changed"
    );
    crate::mapper165::selector_safety::verify_active_fixed_bank_nonindexed_absolute_mapper_select_store(
        output_rom,
    )?;
    Ok(())
}
