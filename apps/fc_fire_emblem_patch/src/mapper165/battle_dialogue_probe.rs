use std::{fs, path::Path};

use anyhow::{Context, Result, ensure};
use serde::Serialize;

use crate::{
    dialogue_assets::{EncodedBattleDialogueRecord, plan_battle_dialogue_records},
    rom::{EXPECTED_SOURCE_SHA1, Rom},
    sha1_hex,
    tracked::TrackedImage,
};

use super::{
    OUTPUT_MAPPER, assemble_mapper165_parity_bytes,
    dialogue_probe_font::{
        SOURCE_FONT_PHYSICAL_PAGE, assign_glyph_codes, assignment_sha1, install_font_glyphs,
    },
};

#[derive(Debug, Serialize)]
struct BattleDialogueProbeReport {
    schema: u8,
    source_sha1: &'static str,
    workspace_sha1: String,
    output_sha1: String,
    output_mapper: u16,
    record_count: usize,
    pointer_write_count: usize,
    translated_line_count: usize,
    translated_record_storage_byte_count: usize,
    preserved_unreferenced_file_offset_hex: String,
    preserved_unreferenced_storage_byte_count: usize,
    preserved_unreferenced_storage_sha1: String,
    remaining_storage_byte_count: usize,
    unique_glyph_count: usize,
    glyph_assignment_sha1: String,
    font_physical_page: usize,
    font_page_scope: &'static str,
    dialogue_content_emitted: bool,
    glyph_characters_emitted: bool,
    tracked_write_count: usize,
    runtime_verified: bool,
    unresolved: Vec<&'static str>,
    release_eligible: bool,
}

pub(crate) struct BattleDialogueProbeSummary {
    pub(crate) output_sha1: String,
    pub(crate) report_sha1: String,
    pub(crate) record_count: usize,
    pub(crate) pointer_write_count: usize,
    pub(crate) translated_line_count: usize,
    pub(crate) unique_glyph_count: usize,
    pub(crate) tracked_write_count: usize,
}

pub(crate) fn build_battle_dialogue_probe(
    source_path: &Path,
    workspace_path: &Path,
    output_path: &Path,
    report_path: &Path,
) -> Result<BattleDialogueProbeSummary> {
    let source_rom = Rom::from_path(source_path)?;
    source_rom.verify_supported_japanese()?;
    let plan = plan_battle_dialogue_records(&source_rom, workspace_path)?;
    let assignments = assign_glyph_codes(&plan.unique_glyphs())?;
    let encoded_records = plan.encoded_records(&assignments)?;
    ensure!(
        encoded_records.len() == 28,
        "battle-dialogue probe record count changed"
    );

    let parity_base = assemble_mapper165_parity_bytes(&source_rom)?;
    let parity_rom = Rom::parse(parity_base.clone()).context("parse mapper 165 battle base")?;
    ensure!(
        parity_rom.mapper() == OUTPUT_MAPPER,
        "battle-dialogue probe base mapper changed"
    );
    let preserved_start = plan.preserved_unreferenced_file_offset;
    let preserved_end = plan.preserved_unreferenced_end_file_offset_exclusive;
    ensure!(
        sha1_hex(&parity_base[preserved_start..preserved_end])
            == plan.preserved_unreferenced_storage_sha1,
        "battle-dialogue preserved record changed in mapper base"
    );

    let mut image = TrackedImage::new(parity_base.clone());
    for record in &encoded_records {
        install_record(&mut image, &parity_base, record)?;
        install_pointers(&mut image, &parity_base, record)?;
    }
    install_font_glyphs(&mut image, &parity_base, &assignments)?;
    image.verify_all_changes_tracked(&parity_base)?;
    let tracked_write_count = image.writes().len();
    let output = image.into_data();
    verify_output_records(&output, &encoded_records)?;
    ensure!(
        sha1_hex(&output[preserved_start..preserved_end])
            == plan.preserved_unreferenced_storage_sha1,
        "battle-dialogue probe changed the preserved record"
    );
    let output_rom = Rom::parse(output.clone()).context("parse battle-dialogue probe")?;
    ensure!(
        output_rom.mapper() == OUTPUT_MAPPER,
        "battle-dialogue probe mapper changed"
    );
    let output_sha1 = sha1_hex(&output);
    let pointer_write_count = encoded_records
        .iter()
        .map(|record| record.pointer_file_offsets.len())
        .sum();
    let preserved_storage_byte_count = preserved_end - preserved_start;
    let report = BattleDialogueProbeReport {
        schema: 1,
        source_sha1: EXPECTED_SOURCE_SHA1,
        workspace_sha1: plan.workspace_sha1,
        output_sha1: output_sha1.clone(),
        output_mapper: output_rom.mapper(),
        record_count: encoded_records.len(),
        pointer_write_count,
        translated_line_count: plan.translated_line_count,
        translated_record_storage_byte_count: plan.translated_record_storage_byte_count,
        preserved_unreferenced_file_offset_hex: format!("0x{preserved_start:05X}"),
        preserved_unreferenced_storage_byte_count: preserved_storage_byte_count,
        preserved_unreferenced_storage_sha1: plan.preserved_unreferenced_storage_sha1,
        remaining_storage_byte_count: plan.remaining_storage_byte_count,
        unique_glyph_count: assignments.len(),
        glyph_assignment_sha1: assignment_sha1(&assignments),
        font_physical_page: SOURCE_FONT_PHYSICAL_PAGE,
        font_page_scope: "development-only source page zero replacement",
        dialogue_content_emitted: false,
        glyph_characters_emitted: false,
        tracked_write_count,
        runtime_verified: false,
        unresolved: vec![
            "Cold battle-dialogue visibility and control flow remain external runtime gates.",
            "This probe replaces source font page zero globally instead of selecting a battle-screen lifetime page.",
            "Battle UI names and labels still use original Japanese tables outside this translated dialogue scope.",
            "The ignored workspace remains the translation authority and requires user approval before release eligibility.",
        ],
        release_eligible: false,
    };
    let report_bytes =
        serde_json::to_vec_pretty(&report).context("serialize battle-dialogue probe report")?;
    let report_sha1 = sha1_hex(&report_bytes);
    write_file(output_path, &output)?;
    write_file(report_path, &report_bytes)?;

    Ok(BattleDialogueProbeSummary {
        output_sha1,
        report_sha1,
        record_count: encoded_records.len(),
        pointer_write_count,
        translated_line_count: plan.translated_line_count,
        unique_glyph_count: assignments.len(),
        tracked_write_count,
    })
}

fn verify_output_records(output: &[u8], records: &[EncodedBattleDialogueRecord]) -> Result<()> {
    for record in records {
        let end = record
            .planned_file_offset
            .checked_add(record.bytes.len())
            .context("battle-dialogue verification range overflow")?;
        ensure!(
            output.get(record.planned_file_offset..end) == Some(record.bytes.as_slice()),
            "battle-dialogue record {} did not round-trip from the output",
            record.canonical_entry_index
        );
        let expected_pointer = record.planned_pointer_cpu_address.to_le_bytes();
        for &offset in &record.pointer_file_offsets {
            ensure!(
                output.get(offset..offset + 2) == Some(expected_pointer.as_slice()),
                "battle-dialogue pointer at {offset:#X} did not resolve to record {}",
                record.canonical_entry_index
            );
        }
    }
    Ok(())
}

fn install_record(
    image: &mut TrackedImage,
    base: &[u8],
    record: &EncodedBattleDialogueRecord,
) -> Result<()> {
    let end = record
        .planned_file_offset
        .checked_add(record.bytes.len())
        .context("battle-dialogue record range overflow")?;
    let expected = base
        .get(record.planned_file_offset..end)
        .context("battle-dialogue planned record is outside the mapper base")?;
    image.write_expected(
        format!(
            "mapper 165 battle dialogue record {}",
            record.canonical_entry_index
        ),
        record.planned_file_offset,
        expected,
        &record.bytes,
    )
}

fn install_pointers(
    image: &mut TrackedImage,
    base: &[u8],
    record: &EncodedBattleDialogueRecord,
) -> Result<()> {
    let replacement = record.planned_pointer_cpu_address.to_le_bytes();
    for &offset in &record.pointer_file_offsets {
        let expected = base
            .get(offset..offset + 2)
            .context("battle-dialogue pointer is outside the mapper base")?;
        image.write_expected(
            format!(
                "mapper 165 battle dialogue pointer {} at {offset:05X}",
                record.canonical_entry_index
            ),
            offset,
            expected,
            &replacement,
        )?;
    }
    Ok(())
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
    fn serialized_report_omits_dialogue_glyphs_and_private_paths() {
        let report = BattleDialogueProbeReport {
            schema: 1,
            source_sha1: EXPECTED_SOURCE_SHA1,
            workspace_sha1: "workspace".to_owned(),
            output_sha1: "output".to_owned(),
            output_mapper: OUTPUT_MAPPER,
            record_count: 28,
            pointer_write_count: 65,
            translated_line_count: 70,
            translated_record_storage_byte_count: 1026,
            preserved_unreferenced_file_offset_hex: "0x1056A".to_owned(),
            preserved_unreferenced_storage_byte_count: 16,
            preserved_unreferenced_storage_sha1: "preserved".to_owned(),
            remaining_storage_byte_count: 126,
            unique_glyph_count: 1,
            glyph_assignment_sha1: assignment_sha1(&BTreeMap::from([('한', 0x01)])),
            font_physical_page: SOURCE_FONT_PHYSICAL_PAGE,
            font_page_scope: "development-only source page zero replacement",
            dialogue_content_emitted: false,
            glyph_characters_emitted: false,
            tracked_write_count: 1,
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
