use std::{collections::BTreeSet, fs, path::Path};

use anyhow::{Context, Result};
use serde::Serialize;

use crate::{
    dialogue_assets::plan_battle_dialogue_records,
    font_slots::ACTIVE_HANGUL_SLOT_COUNT,
    rom::{EXPECTED_SOURCE_SHA1, Rom},
    sha1_hex,
    text_inventory::plan_fixed_text,
};

pub(crate) const FORECAST_LABEL_GLYPHS: [char; 4] = ['지', '형', '효', '과'];
pub(crate) const FORECAST_LABEL_FILE_OFFSET: usize = 0x156C6;
pub(crate) const FORECAST_LABEL_SOURCE: [u8; 10] =
    [0x22, 0x4D, 0x06, 0x11, 0x08, 0x01, 0x09, 0x02, 0x05, 0x00];

#[derive(Debug)]
pub(crate) struct BattleTextWorksetSummary {
    pub(crate) report_sha1: String,
    pub(crate) fixed_glyph_count: usize,
    pub(crate) dialogue_glyph_count: usize,
    pub(crate) union_glyph_count: usize,
    pub(crate) conservative_combination_upper_bound: usize,
}

#[derive(Debug, Serialize)]
struct BattleTextWorksetReport {
    schema: u8,
    source_sha1: &'static str,
    fixed_workspace_sha1: String,
    dialogue_workspace_sha1: String,
    fixed_entry_count: usize,
    fixed_glyph_count: usize,
    dialogue_record_count: usize,
    dialogue_glyph_count: usize,
    union_glyph_count: usize,
    overlap_glyph_count: usize,
    active_slot_count: usize,
    all_text_fits_one_page: bool,
    max_dialogue_record_glyph_count: usize,
    max_name_entry_glyph_count: usize,
    max_class_entry_glyph_count: usize,
    max_item_entry_glyph_count: usize,
    max_terrain_entry_glyph_count: usize,
    max_message_template_glyph_count: usize,
    forecast_label_glyph_count: usize,
    conservative_combination_upper_bound: usize,
    conservative_combination_fits_one_page: bool,
    encoded_original_byte_count: usize,
    glyph_characters_emitted: bool,
    translation_text_emitted: bool,
    next_gate: &'static str,
}

pub(crate) fn analyze_battle_text_workset(
    source_path: &Path,
    fixed_workspace_path: &Path,
    dialogue_workspace_path: &Path,
    report_path: &Path,
) -> Result<BattleTextWorksetSummary> {
    let rom = Rom::from_path(source_path)?;
    rom.verify_supported_japanese()?;
    let fixed = plan_fixed_text(&rom, fixed_workspace_path)?;
    let dialogue = plan_battle_dialogue_records(&rom, dialogue_workspace_path)?;
    let fixed_glyphs = fixed.unique_glyphs();
    let dialogue_glyphs = dialogue.unique_glyphs();
    let union = fixed_glyphs
        .union(&dialogue_glyphs)
        .copied()
        .chain(FORECAST_LABEL_GLYPHS)
        .collect::<BTreeSet<_>>();
    let overlap_glyph_count = fixed_glyphs.intersection(&dialogue_glyphs).count();
    let max_name_entry_glyph_count = fixed
        .table_max_entry_glyph_count("unit-names")
        .max(fixed.table_max_entry_glyph_count("enemy-names"));
    let max_class_entry_glyph_count = fixed.table_max_entry_glyph_count("class-names");
    let max_item_entry_glyph_count = fixed.table_max_entry_glyph_count("item-names");
    let max_terrain_entry_glyph_count = fixed.table_max_entry_glyph_count("terrain-names");
    let max_message_template_glyph_count =
        fixed.table_max_entry_glyph_count("battle-message-templates");
    let forecast_label_glyph_count = FORECAST_LABEL_GLYPHS.len();
    let max_dialogue_record_glyph_count = dialogue.max_record_unique_glyph_count();
    let conservative_combination_upper_bound = max_dialogue_record_glyph_count
        + 2 * max_name_entry_glyph_count
        + 2 * max_class_entry_glyph_count
        + 2 * max_item_entry_glyph_count
        + 2 * max_terrain_entry_glyph_count
        + max_message_template_glyph_count
        + forecast_label_glyph_count;
    let fixed_workspace_bytes = fs::read(fixed_workspace_path)
        .with_context(|| format!("read {}", fixed_workspace_path.display()))?;
    let dialogue_workspace_bytes = fs::read(dialogue_workspace_path)
        .with_context(|| format!("read {}", dialogue_workspace_path.display()))?;
    let report = BattleTextWorksetReport {
        schema: 1,
        source_sha1: EXPECTED_SOURCE_SHA1,
        fixed_workspace_sha1: sha1_hex(&fixed_workspace_bytes),
        dialogue_workspace_sha1: sha1_hex(&dialogue_workspace_bytes),
        fixed_entry_count: fixed.entries.len(),
        fixed_glyph_count: fixed_glyphs.len(),
        dialogue_record_count: dialogue.records.len(),
        dialogue_glyph_count: dialogue_glyphs.len(),
        union_glyph_count: union.len(),
        overlap_glyph_count,
        active_slot_count: ACTIVE_HANGUL_SLOT_COUNT,
        all_text_fits_one_page: union.len() <= ACTIVE_HANGUL_SLOT_COUNT,
        max_dialogue_record_glyph_count,
        max_name_entry_glyph_count,
        max_class_entry_glyph_count,
        max_item_entry_glyph_count,
        max_terrain_entry_glyph_count,
        max_message_template_glyph_count,
        forecast_label_glyph_count,
        conservative_combination_upper_bound,
        conservative_combination_fits_one_page: conservative_combination_upper_bound
            <= ACTIVE_HANGUL_SLOT_COUNT,
        encoded_original_byte_count: fixed.encoded_original_byte_count(),
        glyph_characters_emitted: false,
        translation_text_emitted: false,
        next_gate: "assign one stable codebook per concurrently visible battle combination and bind its runtime selector",
    };
    let mut report_bytes =
        serde_json::to_vec_pretty(&report).context("serialize battle text workset")?;
    report_bytes.push(b'\n');
    if let Some(parent) = report_path.parent() {
        fs::create_dir_all(parent).with_context(|| format!("create {}", parent.display()))?;
    }
    fs::write(report_path, &report_bytes)
        .with_context(|| format!("write {}", report_path.display()))?;
    Ok(BattleTextWorksetSummary {
        report_sha1: sha1_hex(&report_bytes),
        fixed_glyph_count: fixed_glyphs.len(),
        dialogue_glyph_count: dialogue_glyphs.len(),
        union_glyph_count: union.len(),
        conservative_combination_upper_bound,
    })
}
