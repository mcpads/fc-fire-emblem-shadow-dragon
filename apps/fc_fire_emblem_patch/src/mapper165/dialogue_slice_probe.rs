use std::{collections::BTreeMap, fs, path::Path};

use anyhow::{Context, Result, ensure};
use serde::Serialize;

use crate::{
    dialogue_assets::{MainDialogueSlicePlan, plan_main_dialogue_slice},
    font::{load_dalmoori, rasterize_glyph},
    font_slots::{FONT_PAGE_SIZE, FONT_TILE_SIZE, active_hangul_codes},
    rom::{CHR_FILE_OFFSET, EXPECTED_SOURCE_SHA1, Rom},
    sha1_hex,
    tracked::TrackedImage,
};

use super::{OUTPUT_MAPPER, assemble_mapper165_parity_bytes};

const SOURCE_FONT_PHYSICAL_PAGE: usize = 2;

#[derive(Debug, Serialize)]
struct DialogueSliceProbeReport {
    schema: u8,
    source_sha1: &'static str,
    workspace_sha1: String,
    output_sha1: String,
    output_mapper: u16,
    record_id: String,
    translated_line_count: usize,
    source_storage_byte_count: usize,
    planned_storage_byte_count: usize,
    remaining_storage_byte_count: usize,
    unique_glyph_count: usize,
    glyph_assignment_sha1: String,
    assigned_code_count: usize,
    font_physical_page: usize,
    font_page_scope: &'static str,
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
    pub(crate) tracked_write_count: usize,
}

pub(crate) fn build_dialogue_slice_probe(
    source_path: &Path,
    workspace_path: &Path,
    record_id: &str,
    output_path: &Path,
    report_path: &Path,
) -> Result<DialogueSliceProbeSummary> {
    let source_rom = Rom::from_path(source_path)?;
    source_rom.verify_supported_japanese()?;
    let plan = plan_main_dialogue_slice(&source_rom, workspace_path, record_id)?;
    let glyphs = plan.unique_glyphs();
    let active_codes = active_hangul_codes();
    ensure!(
        glyphs.len() <= active_codes.len(),
        "dialogue slice needs {} glyphs but the active page owns only {} slots",
        glyphs.len(),
        active_codes.len()
    );
    let assignments = glyphs
        .iter()
        .copied()
        .zip(active_codes)
        .collect::<BTreeMap<_, _>>();
    let encoded_record = plan.encoded_bytes(&assignments)?;

    let parity_base = assemble_mapper165_parity_bytes(&source_rom)?;
    let parity_rom = Rom::parse(parity_base.clone()).context("parse mapper 165 dialogue base")?;
    ensure!(
        parity_rom.mapper() == OUTPUT_MAPPER,
        "dialogue slice base mapper changed"
    );
    let mut image = TrackedImage::new(parity_base.clone());
    install_record(&mut image, &parity_base, &plan, &encoded_record)?;
    install_font_glyphs(&mut image, &parity_base, &assignments)?;
    image.verify_all_changes_tracked(&parity_base)?;
    let tracked_write_count = image.writes().len();
    let output = image.into_data();
    let output_rom = Rom::parse(output.clone()).context("parse dialogue slice probe")?;
    ensure!(
        output_rom.mapper() == OUTPUT_MAPPER,
        "dialogue slice probe mapper changed"
    );
    let output_sha1 = sha1_hex(&output);
    let remaining_storage_byte_count = plan.source_storage_byte_count - encoded_record.len();
    let report = DialogueSliceProbeReport {
        schema: 1,
        source_sha1: EXPECTED_SOURCE_SHA1,
        workspace_sha1: plan.workspace_sha1.clone(),
        output_sha1: output_sha1.clone(),
        output_mapper: output_rom.mapper(),
        record_id: plan.record_id.clone(),
        translated_line_count: plan.translated_line_count,
        source_storage_byte_count: plan.source_storage_byte_count,
        planned_storage_byte_count: encoded_record.len(),
        remaining_storage_byte_count,
        unique_glyph_count: assignments.len(),
        glyph_assignment_sha1: assignment_sha1(&assignments),
        assigned_code_count: assignments.len(),
        font_physical_page: SOURCE_FONT_PHYSICAL_PAGE,
        font_page_scope: "development-only source page zero replacement",
        tracked_write_count,
        runtime_verified: false,
        unresolved: vec![
            "Cold visible dialogue evidence stays external to this static build report, so runtime_verified remains false by construction.",
            "This probe replaces source font page zero globally instead of selecting a screen-lifetime page.",
            "Only one fully filled record is inserted in place; complete-scene flow, line width, and later records remain unverified.",
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
        unique_glyph_count: assignments.len(),
        planned_storage_byte_count: encoded_record.len(),
        remaining_storage_byte_count,
        tracked_write_count,
    })
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

fn install_font_glyphs(
    image: &mut TrackedImage,
    parity_base: &[u8],
    assignments: &BTreeMap<char, u8>,
) -> Result<()> {
    let font = load_dalmoori()?;
    let page_start = CHR_FILE_OFFSET + SOURCE_FONT_PHYSICAL_PAGE * FONT_PAGE_SIZE;
    for (character, code) in assignments {
        let offset = page_start + usize::from(*code) * FONT_TILE_SIZE;
        let expected = parity_base
            .get(offset..offset + FONT_TILE_SIZE)
            .context("dialogue slice font tile is outside the parity base")?;
        let replacement = rasterize_glyph(&font, *character)?;
        image.write_expected(
            format!("mapper 165 dialogue slice glyph code {code:02X}"),
            offset,
            expected,
            &replacement,
        )?;
    }
    Ok(())
}

fn assignment_sha1(assignments: &BTreeMap<char, u8>) -> String {
    let mut bytes = Vec::new();
    for (character, code) in assignments {
        bytes.extend_from_slice(character.to_string().as_bytes());
        bytes.push(*code);
    }
    sha1_hex(&bytes)
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
    use super::*;

    #[test]
    fn serialized_report_does_not_emit_glyph_characters_or_dialogue() {
        let report = DialogueSliceProbeReport {
            schema: 1,
            source_sha1: EXPECTED_SOURCE_SHA1,
            workspace_sha1: "workspace".to_owned(),
            output_sha1: "output".to_owned(),
            output_mapper: OUTPUT_MAPPER,
            record_id: "record".to_owned(),
            translated_line_count: 1,
            source_storage_byte_count: 2,
            planned_storage_byte_count: 2,
            remaining_storage_byte_count: 0,
            unique_glyph_count: 1,
            glyph_assignment_sha1: assignment_sha1(&BTreeMap::from([('한', 0x01)])),
            assigned_code_count: 1,
            font_physical_page: SOURCE_FONT_PHYSICAL_PAGE,
            font_page_scope: "development-only source page zero replacement",
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
