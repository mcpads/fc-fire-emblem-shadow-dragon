use std::{fs, path::Path};

use anyhow::{Context, Result, ensure};
use serde::Serialize;

use crate::{
    dialogue_assets::{MainDialogueSlicePlan, plan_main_dialogue_slice},
    font_slots::FONT_PAGE_SIZE,
    mmc5_prg::{count_direct_transfers_to_range, fixed_bank_file_offset},
    rom::{EXPECTED_SOURCE_SHA1, PRG_SIZE, Rom},
    sha1_hex,
    tracked::TrackedImage,
};

use super::{
    OUTPUT_MAPPER, SELECT_RIGHT_FD_CHR_BANK_FOR_PAIR_ADDRESS,
    dialogue_font_page::assignment_sha1,
    dialogue_lifetime_page::{
        CENTRAL_RIGHT_FD_SELECTOR_CALL_ADDRESS, OUTPUT_CHR_BANK_COUNT, PAGE_ROUTINE_ADDRESS,
        PAGE_ROUTINE_END, PHYSICAL_CHR_PAGE, SCREEN_ROLE, build_page_routine,
        central_right_fd_selector_call, plan_dialogue_lifetime_page,
    },
    install_mapper165_parity_bytes,
};

#[derive(Debug, Serialize)]
struct DialogueSliceProbeReport {
    schema: u8,
    source_sha1: &'static str,
    workspace_sha1: String,
    output_sha1: String,
    output_mapper: u16,
    record_id: String,
    translated_line_count: usize,
    transition_chain_record_count: usize,
    source_storage_byte_count: usize,
    planned_storage_byte_count: usize,
    remaining_storage_byte_count: usize,
    unique_glyph_count: usize,
    glyph_assignment_sha1: String,
    assigned_code_count: usize,
    preserved_screen_active_code_count: usize,
    preserved_source_active_code_count: usize,
    preserved_active_code_count: usize,
    screen_evidence_manifest_sha1: String,
    temporal_sample_count: usize,
    unique_nametable_count: usize,
    font_physical_page: u8,
    font_mapper_register: u8,
    font_page_sha1: String,
    font_page_pack_sha1: String,
    font_page_scope: &'static str,
    selector_call_address: String,
    selector_routine_start: String,
    selector_routine_end_exclusive: String,
    selector_exact_contract_only: bool,
    selector_preserves_accumulator_and_status: bool,
    original_chr_preserved: bool,
    direct_code_cave_transfer_count: usize,
    tracked_write_count: usize,
    runtime_verified: bool,
    unresolved: Vec<&'static str>,
    release_eligible: bool,
}

pub(crate) struct DialogueSliceProbeSummary {
    pub(crate) output_sha1: String,
    pub(crate) report_sha1: String,
    pub(crate) translated_line_count: usize,
    pub(crate) unique_glyph_count: usize,
    pub(crate) planned_storage_byte_count: usize,
    pub(crate) remaining_storage_byte_count: usize,
    pub(crate) preserved_active_code_count: usize,
    pub(crate) temporal_sample_count: usize,
    pub(crate) tracked_write_count: usize,
}

pub(crate) fn build_dialogue_slice_probe(
    source_path: &Path,
    workspace_path: &Path,
    screen_evidence_path: &Path,
    record_id: &str,
    output_path: &Path,
    report_path: &Path,
) -> Result<DialogueSliceProbeSummary> {
    let source_rom = Rom::from_path(source_path)?;
    source_rom.verify_supported_japanese()?;
    let plan = plan_main_dialogue_slice(&source_rom, workspace_path, record_id)?;
    let glyphs = plan.unique_glyphs();

    let parity_base = install_mapper165_parity_bytes(&source_rom)?;
    let parity_rom = Rom::parse(parity_base.clone()).context("parse mapper 165 dialogue base")?;
    ensure!(
        parity_rom.mapper() == OUTPUT_MAPPER,
        "dialogue slice base mapper changed"
    );
    let lifetime_page = plan_dialogue_lifetime_page(
        &parity_rom,
        screen_evidence_path,
        SCREEN_ROLE,
        record_id,
        &glyphs,
        &plan.preserved_source_codes,
        PHYSICAL_CHR_PAGE,
    )?;
    let encoded_record = plan.encoded_bytes(&lifetime_page.assignments)?;
    let routine = build_page_routine(lifetime_page.mapper_register)?;
    ensure!(
        PAGE_ROUTINE_ADDRESS as usize + routine.len() == PAGE_ROUTINE_END as usize,
        "dialogue page selector routine size changed"
    );
    let cave_start = fixed_bank_file_offset(PAGE_ROUTINE_ADDRESS)?;
    let cave_end = cave_start + routine.len();
    ensure!(
        parity_base[cave_start..cave_end]
            .iter()
            .all(|byte| *byte == 0xFF),
        "dialogue page selector cave is no longer all FF"
    );
    let direct_code_cave_transfer_count =
        count_direct_transfers_to_range(source_rom.prg(), PAGE_ROUTINE_ADDRESS, PAGE_ROUTINE_END)?;
    ensure!(
        direct_code_cave_transfer_count == 0,
        "dialogue page selector cave has {direct_code_cave_transfer_count} pre-existing direct transfers"
    );

    let mut expanded_base = parity_base.clone();
    expanded_base.extend_from_slice(&lifetime_page.page_pack);
    ensure!(
        expanded_base.len() == parity_base.len() + 2 * FONT_PAGE_SIZE,
        "dialogue lifetime page must expand CHR by one 8 KiB bank"
    );
    let mut image = TrackedImage::new(expanded_base.clone());
    image.write_expected(
        "expand mapper 165 CHR from 17 to 18 banks",
        5,
        &[17],
        &[OUTPUT_CHR_BANK_COUNT],
    )?;
    install_record(&mut image, &parity_base, &plan, &encoded_record)?;
    image.write_expected(
        "chapter one intro dialogue page selector",
        cave_start,
        &vec![0xFF; routine.len()],
        &routine,
    )?;
    image.write_expected(
        "central right FD selector to chapter one intro selector",
        fixed_bank_file_offset(CENTRAL_RIGHT_FD_SELECTOR_CALL_ADDRESS)?,
        &central_right_fd_selector_call(SELECT_RIGHT_FD_CHR_BANK_FOR_PAIR_ADDRESS)?,
        &central_right_fd_selector_call(PAGE_ROUTINE_ADDRESS)?,
    )?;
    image.verify_all_changes_tracked(&expanded_base)?;
    let tracked_write_count = image.writes().len();
    let output = image.into_data();
    let output_rom = Rom::parse(output.clone()).context("parse dialogue slice probe")?;
    ensure!(
        output_rom.mapper() == OUTPUT_MAPPER,
        "dialogue slice probe mapper changed"
    );
    verify_output(
        &parity_rom,
        &output_rom,
        &lifetime_page.page_pack,
        &plan,
        &encoded_record,
        &routine,
    )?;
    let output_sha1 = sha1_hex(&output);
    let remaining_storage_byte_count = plan.source_storage_byte_count - encoded_record.len();
    let report = DialogueSliceProbeReport {
        schema: 2,
        source_sha1: EXPECTED_SOURCE_SHA1,
        workspace_sha1: plan.workspace_sha1.clone(),
        output_sha1: output_sha1.clone(),
        output_mapper: output_rom.mapper(),
        record_id: plan.record_id.clone(),
        translated_line_count: plan.translated_line_count,
        transition_chain_record_count: plan.transition_chain_record_count,
        source_storage_byte_count: plan.source_storage_byte_count,
        planned_storage_byte_count: encoded_record.len(),
        remaining_storage_byte_count,
        unique_glyph_count: lifetime_page.assignments.len(),
        glyph_assignment_sha1: assignment_sha1(&lifetime_page.assignments),
        assigned_code_count: lifetime_page.assignments.len(),
        preserved_screen_active_code_count: lifetime_page.preserved_screen_active_code_count,
        preserved_source_active_code_count: lifetime_page.preserved_source_active_code_count,
        preserved_active_code_count: lifetime_page.preserved_active_code_count,
        screen_evidence_manifest_sha1: lifetime_page.manifest_sha1,
        temporal_sample_count: lifetime_page.temporal_sample_count,
        unique_nametable_count: lifetime_page.unique_nametable_count,
        font_physical_page: lifetime_page.physical_chr_page,
        font_mapper_register: lifetime_page.mapper_register,
        font_page_sha1: lifetime_page.page_sha1,
        font_page_pack_sha1: sha1_hex(&lifetime_page.page_pack),
        font_page_scope: SCREEN_ROLE,
        selector_call_address: format!("0x{CENTRAL_RIGHT_FD_SELECTOR_CALL_ADDRESS:04X}"),
        selector_routine_start: format!("0x{PAGE_ROUTINE_ADDRESS:04X}"),
        selector_routine_end_exclusive: format!("0x{PAGE_ROUTINE_END:04X}"),
        selector_exact_contract_only: true,
        selector_preserves_accumulator_and_status: true,
        original_chr_preserved: true,
        direct_code_cave_transfer_count,
        tracked_write_count,
        runtime_verified: false,
        unresolved: vec![
            "Cold visible dialogue evidence stays external to this static build report, so runtime_verified remains false by construction.",
            "Only one fully filled record is inserted in place; the untranslated followup transition chain is preserved but still needs visible progression evidence.",
            "The exact Chapter 1 supplier contract and natural-page restoration need cold runtime verification.",
            "The report omits translation text and glyph characters; the ignored workspace remains the translation authority.",
        ],
        release_eligible: false,
    };
    let report_bytes =
        serde_json::to_vec_pretty(&report).context("serialize dialogue slice probe report")?;
    let report_sha1 = sha1_hex(&report_bytes);
    write_file(output_path, &output)?;
    write_file(report_path, &report_bytes)?;

    Ok(DialogueSliceProbeSummary {
        output_sha1,
        report_sha1,
        translated_line_count: plan.translated_line_count,
        unique_glyph_count: lifetime_page.assignments.len(),
        planned_storage_byte_count: encoded_record.len(),
        remaining_storage_byte_count,
        preserved_active_code_count: lifetime_page.preserved_active_code_count,
        temporal_sample_count: lifetime_page.temporal_sample_count,
        tracked_write_count,
    })
}

fn verify_output(
    parity_rom: &Rom,
    output_rom: &Rom,
    page_pack: &[u8],
    plan: &MainDialogueSlicePlan,
    encoded_record: &[u8],
    routine: &[u8],
) -> Result<()> {
    ensure!(
        output_rom.mapper() == OUTPUT_MAPPER,
        "dialogue output mapper changed"
    );
    ensure!(
        output_rom.prg().len() == PRG_SIZE,
        "dialogue output PRG size changed"
    );
    ensure!(
        output_rom.chr().len() == parity_rom.chr().len() + page_pack.len(),
        "dialogue output CHR size changed"
    );
    ensure!(
        output_rom.chr()[..parity_rom.chr().len()] == *parity_rom.chr(),
        "dialogue lifetime probe changed original CHR"
    );
    ensure!(
        output_rom.chr()[parity_rom.chr().len()..] == *page_pack,
        "dialogue lifetime probe appended different page bytes"
    );
    ensure!(
        output_rom.data()[plan.source_file_offset..plan.source_file_offset + encoded_record.len()]
            == *encoded_record,
        "dialogue lifetime probe inserted different record bytes"
    );
    let cave_start = fixed_bank_file_offset(PAGE_ROUTINE_ADDRESS)?;
    ensure!(
        output_rom.data()[cave_start..cave_start + routine.len()] == *routine,
        "dialogue lifetime probe installed different selector bytes"
    );
    ensure!(
        output_rom.data()[fixed_bank_file_offset(CENTRAL_RIGHT_FD_SELECTOR_CALL_ADDRESS)?
            ..fixed_bank_file_offset(CENTRAL_RIGHT_FD_SELECTOR_CALL_ADDRESS)? + 3]
            == central_right_fd_selector_call(PAGE_ROUTINE_ADDRESS)?,
        "dialogue lifetime probe installed a different selector call"
    );
    Ok(())
}

fn install_record(
    image: &mut TrackedImage,
    parity_base: &[u8],
    plan: &MainDialogueSlicePlan,
    encoded_record: &[u8],
) -> Result<()> {
    let end = plan
        .source_file_offset
        .checked_add(encoded_record.len())
        .context("dialogue slice record range overflow")?;
    let expected = parity_base
        .get(plan.source_file_offset..end)
        .context("dialogue slice record is outside the parity base")?;
    image.write_expected(
        "mapper 165 main dialogue slice record",
        plan.source_file_offset,
        expected,
        encoded_record,
    )
}

fn write_file(path: &Path, data: &[u8]) -> Result<()> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)
            .with_context(|| format!("create output directory {}", parent.display()))?;
    }
    fs::write(path, data).with_context(|| format!("write {}", path.display()))
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;

    use super::*;

    #[test]
    fn serialized_report_does_not_emit_glyph_characters_or_dialogue() {
        let report = DialogueSliceProbeReport {
            schema: 2,
            source_sha1: EXPECTED_SOURCE_SHA1,
            workspace_sha1: "workspace".to_owned(),
            output_sha1: "output".to_owned(),
            output_mapper: OUTPUT_MAPPER,
            record_id: "record".to_owned(),
            translated_line_count: 1,
            transition_chain_record_count: 1,
            source_storage_byte_count: 2,
            planned_storage_byte_count: 2,
            remaining_storage_byte_count: 0,
            unique_glyph_count: 1,
            glyph_assignment_sha1: assignment_sha1(&BTreeMap::from([('한', 0x01)])),
            assigned_code_count: 1,
            preserved_screen_active_code_count: 1,
            preserved_source_active_code_count: 1,
            preserved_active_code_count: 1,
            screen_evidence_manifest_sha1: "manifest".to_owned(),
            temporal_sample_count: 3,
            unique_nametable_count: 1,
            font_physical_page: PHYSICAL_CHR_PAGE,
            font_mapper_register: 0x88,
            font_page_sha1: "page".to_owned(),
            font_page_pack_sha1: "pack".to_owned(),
            font_page_scope: SCREEN_ROLE,
            selector_call_address: "0xC9C2".to_owned(),
            selector_routine_start: "0xFB20".to_owned(),
            selector_routine_end_exclusive: "0xFB68".to_owned(),
            selector_exact_contract_only: true,
            selector_preserves_accumulator_and_status: true,
            original_chr_preserved: true,
            direct_code_cave_transfer_count: 0,
            tracked_write_count: 2,
            runtime_verified: false,
            unresolved: Vec::new(),
            release_eligible: false,
        };

        let json = serde_json::to_string(&report).unwrap();
        assert!(!json.contains('한'));
        assert!(!json.contains("private/"));
        assert!(!json.contains("source_markup"));
    }
}
