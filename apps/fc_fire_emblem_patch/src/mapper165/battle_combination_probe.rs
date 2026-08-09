use std::{collections::BTreeSet, fs, path::Path};

use anyhow::{Context, Result, ensure};
use serde::Serialize;

use crate::{
    battle_text_workset::FORECAST_LABEL_GLYPHS,
    dialogue_assets::plan_battle_dialogue_records,
    rom::{EXPECTED_SOURCE_SHA1, Rom},
    sha1_hex,
    text_inventory::plan_fixed_text,
    tracked::TrackedImage,
};

use super::{
    OUTPUT_MAPPER, assemble_mapper165_parity_bytes,
    dialogue_probe_font::{
        SOURCE_FONT_PHYSICAL_PAGE, assign_glyph_codes, assignment_sha1, install_font_glyphs,
    },
};

const GAMEPLAY_DIALOGUE_SELECTOR: usize = 62;
const GAMEPLAY_FIXED_SELECTIONS: [(&str, usize); 8] = [
    ("enemy-names", 4),
    ("unit-names", 3),
    ("class-names", 0),
    ("class-names", 7),
    ("item-names", 11),
    ("item-names", 26),
    ("terrain-names", 0),
    ("terrain-names", 11),
];
const FORECAST_LABEL_FILE_OFFSET: usize = 0x156C6;
const FORECAST_LABEL_SOURCE: [u8; 10] =
    [0x22, 0x4D, 0x06, 0x11, 0x08, 0x01, 0x09, 0x02, 0x05, 0x00];

#[derive(Debug, Serialize)]
struct BattleCombinationProbeReport {
    schema: u8,
    source_sha1: &'static str,
    fixed_workspace_sha1: String,
    dialogue_workspace_sha1: String,
    output_sha1: String,
    output_mapper: u16,
    combination_role: &'static str,
    fixed_unique_entry_count: usize,
    fixed_visible_reference_count: usize,
    message_template_count: usize,
    dialogue_selector: usize,
    codebook_glyph_count: usize,
    codebook_assignment_sha1: String,
    font_physical_page: usize,
    tracked_write_count: usize,
    fixed_strings_reencoded: bool,
    message_templates_reencoded: bool,
    forecast_label_reencoded: bool,
    dialogue_record_reencoded: bool,
    shared_local_codebook: bool,
    translation_text_emitted: bool,
    glyph_characters_emitted: bool,
    runtime_verified: bool,
    release_eligible: bool,
    next_gate: &'static str,
}

pub(crate) struct BattleCombinationProbeSummary {
    pub(crate) output_sha1: String,
    pub(crate) report_sha1: String,
    pub(crate) glyph_count: usize,
    pub(crate) tracked_write_count: usize,
}

pub(crate) fn build_gameplay_battle_combination_probe(
    source_path: &Path,
    fixed_workspace_path: &Path,
    dialogue_workspace_path: &Path,
    output_path: &Path,
    report_path: &Path,
) -> Result<BattleCombinationProbeSummary> {
    let source_rom = Rom::from_path(source_path)?;
    source_rom.verify_supported_japanese()?;
    let fixed_plan = plan_fixed_text(&source_rom, fixed_workspace_path)?;
    let dialogue_plan = plan_battle_dialogue_records(&source_rom, dialogue_workspace_path)?;

    let selected_fixed = GAMEPLAY_FIXED_SELECTIONS
        .iter()
        .map(|(table_id, source_index)| {
            fixed_plan
                .entries
                .iter()
                .find(|entry| entry.table_id == *table_id && entry.source_index == *source_index)
                .with_context(|| format!("missing {table_id} entry {source_index}"))
        })
        .collect::<Result<Vec<_>>>()?;
    let selected_dialogue = dialogue_plan
        .records
        .iter()
        .find(|record| record.entry_indices.contains(&GAMEPLAY_DIALOGUE_SELECTOR))
        .context("gameplay battle dialogue selector is not translated")?;
    let message_templates = fixed_plan
        .entries
        .iter()
        .filter(|entry| entry.table_id == "battle-message-templates")
        .collect::<Vec<_>>();
    ensure!(
        message_templates.len() == 22,
        "battle message template count changed"
    );

    let glyphs = selected_fixed
        .iter()
        .flat_map(|entry| entry.unique_glyphs())
        .chain(
            message_templates
                .iter()
                .flat_map(|entry| entry.unique_glyphs()),
        )
        .chain(selected_dialogue.unique_glyphs())
        .chain(FORECAST_LABEL_GLYPHS)
        .collect::<BTreeSet<_>>();
    let assignments = assign_glyph_codes(&glyphs)?;

    let parity = assemble_mapper165_parity_bytes(&source_rom)?;
    let mut image = TrackedImage::new(parity.clone());
    for entry in selected_fixed {
        let mut replacement = entry.encoded_bytes(&assignments)?;
        ensure!(
            replacement.len() <= entry.source_storage_byte_count,
            "{} no longer fits its source storage",
            entry.id
        );
        replacement.push(0xEF);
        let expected = parity
            .get(entry.file_offset..entry.file_offset + replacement.len())
            .with_context(|| format!("{} source storage is outside mapper base", entry.id))?;
        image.write_expected(
            format!("gameplay combination fixed text {}", entry.id),
            entry.file_offset,
            expected,
            &replacement,
        )?;
    }
    for entry in &message_templates {
        let mut replacement = entry.encoded_bytes(&assignments)?;
        ensure!(
            replacement.len() <= entry.source_storage_byte_count,
            "{} no longer fits its source storage",
            entry.id
        );
        replacement.push(0xEF);
        let expected = parity
            .get(entry.file_offset..entry.file_offset + replacement.len())
            .with_context(|| format!("{} source storage is outside mapper base", entry.id))?;
        image.write_expected(
            format!("gameplay combination battle message {}", entry.id),
            entry.file_offset,
            expected,
            &replacement,
        )?;
    }

    let dialogue_bytes = selected_dialogue.encoded_bytes(&assignments)?;
    ensure!(
        dialogue_bytes.len() <= selected_dialogue.source_storage_byte_count,
        "sound-test dialogue no longer fits its source storage"
    );
    let dialogue_expected = parity
        .get(
            selected_dialogue.source_file_offset
                ..selected_dialogue.source_file_offset + dialogue_bytes.len(),
        )
        .context("sound-test dialogue source storage is outside mapper base")?;
    image.write_expected(
        "gameplay combination battle dialogue",
        selected_dialogue.source_file_offset,
        dialogue_expected,
        &dialogue_bytes,
    )?;
    let mut forecast_label = vec![0x22, 0x4E, 0x04];
    forecast_label.extend(FORECAST_LABEL_GLYPHS.iter().map(|glyph| assignments[glyph]));
    forecast_label.push(0x00);
    image.write_expected(
        "gameplay forecast terrain-effect label",
        FORECAST_LABEL_FILE_OFFSET,
        &FORECAST_LABEL_SOURCE[..forecast_label.len()],
        &forecast_label,
    )?;
    install_font_glyphs(&mut image, &parity, &assignments)?;
    image.verify_all_changes_tracked(&parity)?;
    let tracked_write_count = image.writes().len();
    let output = image.into_data();
    let output_rom = Rom::parse(output.clone()).context("parse battle combination probe")?;
    ensure!(
        output_rom.mapper() == OUTPUT_MAPPER,
        "battle combination probe mapper changed"
    );
    let output_sha1 = sha1_hex(&output);
    let report = BattleCombinationProbeReport {
        schema: 1,
        source_sha1: EXPECTED_SOURCE_SHA1,
        fixed_workspace_sha1: sha1_hex(&fs::read(fixed_workspace_path)?),
        dialogue_workspace_sha1: sha1_hex(&fs::read(dialogue_workspace_path)?),
        output_sha1: output_sha1.clone(),
        output_mapper: output_rom.mapper(),
        combination_role: "chapter-one favorable gameplay battle",
        fixed_unique_entry_count: GAMEPLAY_FIXED_SELECTIONS.len(),
        fixed_visible_reference_count: GAMEPLAY_FIXED_SELECTIONS.len(),
        message_template_count: message_templates.len(),
        dialogue_selector: GAMEPLAY_DIALOGUE_SELECTOR,
        codebook_glyph_count: assignments.len(),
        codebook_assignment_sha1: assignment_sha1(&assignments),
        font_physical_page: SOURCE_FONT_PHYSICAL_PAGE,
        tracked_write_count,
        fixed_strings_reencoded: true,
        message_templates_reencoded: true,
        forecast_label_reencoded: true,
        dialogue_record_reencoded: true,
        shared_local_codebook: true,
        translation_text_emitted: false,
        glyph_characters_emitted: false,
        runtime_verified: false,
        release_eligible: false,
        next_gate: "cold-load the mapper165 gameplay battle entry and run through names, classes, items, terrain, attack templates, damage, and selector 62 dialogue without text glitches",
    };
    let mut report_bytes =
        serde_json::to_vec_pretty(&report).context("serialize battle combination probe report")?;
    report_bytes.push(b'\n');
    write_file(output_path, &output)?;
    write_file(report_path, &report_bytes)?;
    Ok(BattleCombinationProbeSummary {
        output_sha1,
        report_sha1: sha1_hex(&report_bytes),
        glyph_count: assignments.len(),
        tracked_write_count,
    })
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
    fn report_does_not_emit_translation_content_or_private_paths() {
        let report = BattleCombinationProbeReport {
            schema: 1,
            source_sha1: EXPECTED_SOURCE_SHA1,
            fixed_workspace_sha1: "fixed".to_owned(),
            dialogue_workspace_sha1: "dialogue".to_owned(),
            output_sha1: "output".to_owned(),
            output_mapper: OUTPUT_MAPPER,
            combination_role: "chapter-one favorable gameplay battle",
            fixed_unique_entry_count: 8,
            fixed_visible_reference_count: 8,
            message_template_count: 22,
            dialogue_selector: GAMEPLAY_DIALOGUE_SELECTOR,
            codebook_glyph_count: 12,
            codebook_assignment_sha1: "assignment".to_owned(),
            font_physical_page: SOURCE_FONT_PHYSICAL_PAGE,
            tracked_write_count: 5,
            fixed_strings_reencoded: true,
            message_templates_reencoded: true,
            forecast_label_reencoded: true,
            dialogue_record_reencoded: true,
            shared_local_codebook: true,
            translation_text_emitted: false,
            glyph_characters_emitted: false,
            runtime_verified: false,
            release_eligible: false,
            next_gate: "runtime proof",
        };

        let json = serde_json::to_string(&report).unwrap();
        assert!(!json.contains("private/"));
        assert!(!json.contains('한'));
        assert!(!json.contains("korean"));
    }
}
