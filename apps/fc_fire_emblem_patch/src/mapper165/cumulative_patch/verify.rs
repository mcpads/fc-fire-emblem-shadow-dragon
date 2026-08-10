use anyhow::{Context, Result, ensure};

use crate::{
    dialogue_assets::MainDialogueSlicePlan,
    mmc5_prg::fixed_bank_file_offset,
    rom::{PRG_SIZE, Rom},
    tracked::TrackedImage,
};

use super::{DIALOGUE_SELECTOR_ADDRESS, OUTPUT_MAPPER, ROSTER_SELECTOR_ADDRESS};

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

pub(super) fn verify_cumulative_output(
    ui_stage_rom: &Rom,
    output_rom: &Rom,
    page_pack: &[u8],
    plans: &[MainDialogueSlicePlan],
    encoded_records: &[Vec<u8>],
    roster_selector: &[u8],
    dialogue_selector: &[u8],
) -> Result<()> {
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
    for (plan, encoded_record) in plans.iter().zip(encoded_records) {
        ensure!(
            output_rom.data()
                [plan.source_file_offset..plan.source_file_offset + encoded_record.len()]
                == *encoded_record,
            "cumulative output record {} changed",
            plan.record_id
        );
    }
    let roster_selector_offset = fixed_bank_file_offset(ROSTER_SELECTOR_ADDRESS)?;
    ensure!(
        output_rom.data()[roster_selector_offset..roster_selector_offset + roster_selector.len()]
            == *roster_selector,
        "cumulative roster selector chain changed"
    );
    let dialogue_selector_offset = fixed_bank_file_offset(DIALOGUE_SELECTOR_ADDRESS)?;
    ensure!(
        output_rom.data()
            [dialogue_selector_offset..dialogue_selector_offset + dialogue_selector.len()]
            == *dialogue_selector,
        "cumulative dialogue selector changed"
    );
    Ok(())
}
