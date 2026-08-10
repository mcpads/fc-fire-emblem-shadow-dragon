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
    battle_cache_coverage::BattleTextCoverage,
    battle_codebook_plan::{ScreenCodeConstraint, plan_constrained_battle_codebook},
    dialogue_probe_font::{SOURCE_FONT_PHYSICAL_PAGE, install_font_glyphs},
};

pub(super) struct GameplayBattleCombinationImage {
    pub(super) data: Vec<u8>,
    pub(super) parity: Vec<u8>,
    pub(super) fixed_workspace_sha1: String,
    pub(super) dialogue_workspace_sha1: String,
    pub(super) physical_codebook_assignment_sha1: String,
    pub(super) cache_glyph_assignment_sha1: String,
    pub(super) abstract_codebook_assignment_sha1: String,
    pub(super) stable_color_count: usize,
    pub(super) constrained_screen_count: usize,
    pub(super) constrained_color_count: usize,
    pub(super) text_coverage: BattleTextCoverage,
    pub(super) glyph_count: usize,
    pub(super) preserved_active_code_count: usize,
    pub(super) tracked_writes: Vec<crate::tracked::WriteReport>,
}

const GAMEPLAY_DIALOGUE_SELECTOR: usize = 62;
const ENEMY_NAME_SOURCE_INDEX: usize = 4;
const PLAYER_NAME_SOURCE_INDEX: usize = 3;
const CLASS_SOURCE_INDICES: [usize; 2] = [0, 7];
const ITEM_SOURCE_INDICES: [usize; 2] = [11, 26];
const TERRAIN_SOURCE_INDICES: [usize; 2] = [0, 11];
const GAMEPLAY_FIXED_SELECTIONS: [(&str, usize); 8] = [
    ("enemy-names", ENEMY_NAME_SOURCE_INDEX),
    ("unit-names", PLAYER_NAME_SOURCE_INDEX),
    ("class-names", CLASS_SOURCE_INDICES[0]),
    ("class-names", CLASS_SOURCE_INDICES[1]),
    ("item-names", ITEM_SOURCE_INDICES[0]),
    ("item-names", ITEM_SOURCE_INDICES[1]),
    ("terrain-names", TERRAIN_SOURCE_INDICES[0]),
    ("terrain-names", TERRAIN_SOURCE_INDICES[1]),
];
pub(super) const FORECAST_LABEL_FILE_OFFSET: usize = 0x156C6;
pub(super) const FORECAST_LABEL_SOURCE: [u8; 10] =
    [0x22, 0x4D, 0x06, 0x11, 0x08, 0x01, 0x09, 0x02, 0x05, 0x00];
pub(super) const GAMEPLAY_BATTLE_PRESERVED_ACTIVE_CODES: [u8; 119] = [
    0x00, 0x01, 0x02, 0x03, 0x04, 0x05, 0x06, 0x07, 0x08, 0x09, 0x0B, 0x0C, 0x0E, 0x10, 0x11, 0x12,
    0x13, 0x15, 0x16, 0x19, 0x1A, 0x1C, 0x1D, 0x20, 0x21, 0x22, 0x23, 0x24, 0x25, 0x26, 0x27, 0x28,
    0x29, 0x2D, 0x2E, 0x2F, 0x30, 0x31, 0x32, 0x33, 0x34, 0x35, 0x36, 0x37, 0x38, 0x3B, 0x3D, 0x3E,
    0x3F, 0x40, 0x41, 0x42, 0x43, 0x44, 0x45, 0x46, 0x47, 0x4D, 0x4E, 0x4F, 0x50, 0x51, 0x52, 0x53,
    0x54, 0x5A, 0x5D, 0x5E, 0x5F, 0x8C, 0x8E, 0x8F, 0x9C, 0x9E, 0x9F, 0xAC, 0xAD, 0xAE, 0xAF, 0xBC,
    0xBD, 0xBE, 0xBF, 0xC0, 0xC1, 0xC2, 0xC3, 0xC4, 0xC5, 0xC6, 0xC7, 0xCB, 0xCD, 0xCE, 0xD0, 0xD1,
    0xD2, 0xD3, 0xD4, 0xD5, 0xD6, 0xD7, 0xD8, 0xD9, 0xDA, 0xDB, 0xE0, 0xE2, 0xE3, 0xE4, 0xF3, 0xF5,
    0xF6, 0xF7, 0xF8, 0xF9, 0xFA, 0xFB, 0xFC,
];

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
    preserved_active_code_count: usize,
    codebook_glyph_count: usize,
    physical_codebook_assignment_sha1: String,
    cache_glyph_assignment_sha1: String,
    abstract_codebook_assignment_sha1: String,
    stable_color_count: usize,
    constrained_screen_count: usize,
    constrained_color_count: usize,
    font_physical_page: usize,
    tracked_write_count: usize,
    fixed_strings_reencoded: bool,
    message_templates_reencoded: bool,
    forecast_label_reencoded: bool,
    dialogue_record_reencoded: bool,
    stable_cross_cache_codebook: bool,
    physical_assignment_catalog_complete: bool,
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
    let assembled = assemble_gameplay_battle_combination(
        source_path,
        fixed_workspace_path,
        dialogue_workspace_path,
    )?;
    let output = assembled.data;
    let output_rom = Rom::parse(output.clone()).context("parse battle combination probe")?;
    ensure!(
        output_rom.mapper() == OUTPUT_MAPPER,
        "battle combination probe mapper changed"
    );
    let output_sha1 = sha1_hex(&output);
    let report = BattleCombinationProbeReport {
        schema: 2,
        source_sha1: EXPECTED_SOURCE_SHA1,
        fixed_workspace_sha1: assembled.fixed_workspace_sha1,
        dialogue_workspace_sha1: assembled.dialogue_workspace_sha1,
        output_sha1: output_sha1.clone(),
        output_mapper: output_rom.mapper(),
        combination_role: "chapter-one Cain and Garuda soldier gameplay battle pair",
        fixed_unique_entry_count: GAMEPLAY_FIXED_SELECTIONS.len(),
        fixed_visible_reference_count: GAMEPLAY_FIXED_SELECTIONS.len(),
        message_template_count: 22,
        dialogue_selector: GAMEPLAY_DIALOGUE_SELECTOR,
        preserved_active_code_count: GAMEPLAY_BATTLE_PRESERVED_ACTIVE_CODES.len(),
        codebook_glyph_count: assembled.glyph_count,
        physical_codebook_assignment_sha1: assembled.physical_codebook_assignment_sha1,
        cache_glyph_assignment_sha1: assembled.cache_glyph_assignment_sha1,
        abstract_codebook_assignment_sha1: assembled.abstract_codebook_assignment_sha1,
        stable_color_count: assembled.stable_color_count,
        constrained_screen_count: assembled.constrained_screen_count,
        constrained_color_count: assembled.constrained_color_count,
        font_physical_page: SOURCE_FONT_PHYSICAL_PAGE,
        tracked_write_count: assembled.tracked_writes.len(),
        fixed_strings_reencoded: true,
        message_templates_reencoded: true,
        forecast_label_reencoded: true,
        dialogue_record_reencoded: true,
        stable_cross_cache_codebook: true,
        physical_assignment_catalog_complete: false,
        translation_text_emitted: false,
        glyph_characters_emitted: false,
        runtime_verified: false,
        release_eligible: false,
        next_gate: "add every cache page protection constraint to the physical assignment, then cold-load the mapper165 gameplay battle entry and verify the complete temporal text sequence",
    };
    let mut report_bytes =
        serde_json::to_vec_pretty(&report).context("serialize battle combination probe report")?;
    report_bytes.push(b'\n');
    write_file(output_path, &output)?;
    write_file(report_path, &report_bytes)?;
    Ok(BattleCombinationProbeSummary {
        output_sha1,
        report_sha1: sha1_hex(&report_bytes),
        glyph_count: assembled.glyph_count,
        tracked_write_count: assembled.tracked_writes.len(),
    })
}

pub(super) fn assemble_gameplay_battle_combination(
    source_path: &Path,
    fixed_workspace_path: &Path,
    dialogue_workspace_path: &Path,
) -> Result<GameplayBattleCombinationImage> {
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
    let preserved_active_codes = GAMEPLAY_BATTLE_PRESERVED_ACTIVE_CODES
        .into_iter()
        .collect::<BTreeSet<_>>();
    let codebook = plan_constrained_battle_codebook(
        &source_rom,
        &fixed_plan,
        &dialogue_plan,
        &[ScreenCodeConstraint {
            glyphs: glyphs.clone(),
            preserved_active_codes: preserved_active_codes.clone(),
        }],
    )?;
    let assignments = glyphs
        .iter()
        .map(|glyph| {
            codebook
                .glyph_codes
                .get(glyph)
                .copied()
                .map(|code| (*glyph, code))
                .with_context(|| format!("stable battle codebook lost glyph {glyph:?}"))
        })
        .collect::<Result<std::collections::BTreeMap<_, _>>>()?;
    ensure!(
        assignments
            .values()
            .all(|code| !preserved_active_codes.contains(code)),
        "stable battle codebook overwrites a preserved chapter-one tile"
    );

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
    let tracked_writes = image.writes().to_vec();
    let output = image.into_data();
    Ok(GameplayBattleCombinationImage {
        data: output,
        parity,
        fixed_workspace_sha1: sha1_hex(&fs::read(fixed_workspace_path)?),
        dialogue_workspace_sha1: sha1_hex(&fs::read(dialogue_workspace_path)?),
        physical_codebook_assignment_sha1: codebook.physical_assignment_sha1,
        cache_glyph_assignment_sha1: super::dialogue_probe_font::assignment_sha1(&assignments),
        abstract_codebook_assignment_sha1: codebook.abstract_assignment_sha1,
        stable_color_count: codebook.stable_color_count,
        constrained_screen_count: codebook.constrained_screen_count,
        constrained_color_count: codebook.constrained_color_count,
        text_coverage: BattleTextCoverage::from_source_indices(
            PLAYER_NAME_SOURCE_INDEX,
            ENEMY_NAME_SOURCE_INDEX,
            CLASS_SOURCE_INDICES,
            ITEM_SOURCE_INDICES,
            TERRAIN_SOURCE_INDICES,
        )?,
        glyph_count: assignments.len(),
        preserved_active_code_count: GAMEPLAY_BATTLE_PRESERVED_ACTIVE_CODES.len(),
        tracked_writes,
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
            schema: 2,
            source_sha1: EXPECTED_SOURCE_SHA1,
            fixed_workspace_sha1: "fixed".to_owned(),
            dialogue_workspace_sha1: "dialogue".to_owned(),
            output_sha1: "output".to_owned(),
            output_mapper: OUTPUT_MAPPER,
            combination_role: "chapter-one Cain and Garuda soldier gameplay battle pair",
            fixed_unique_entry_count: 8,
            fixed_visible_reference_count: 8,
            message_template_count: 22,
            dialogue_selector: GAMEPLAY_DIALOGUE_SELECTOR,
            preserved_active_code_count: GAMEPLAY_BATTLE_PRESERVED_ACTIVE_CODES.len(),
            codebook_glyph_count: 12,
            physical_codebook_assignment_sha1: "physical".to_owned(),
            cache_glyph_assignment_sha1: "cache".to_owned(),
            abstract_codebook_assignment_sha1: "abstract".to_owned(),
            stable_color_count: 12,
            constrained_screen_count: 1,
            constrained_color_count: 12,
            font_physical_page: SOURCE_FONT_PHYSICAL_PAGE,
            tracked_write_count: 5,
            fixed_strings_reencoded: true,
            message_templates_reencoded: true,
            forecast_label_reencoded: true,
            dialogue_record_reencoded: true,
            stable_cross_cache_codebook: true,
            physical_assignment_catalog_complete: false,
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
