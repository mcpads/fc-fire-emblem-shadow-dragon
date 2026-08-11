use std::{collections::BTreeSet, fs, ops::Range, path::Path};

use anyhow::{Context, Result, ensure};
use serde::Deserialize;

use crate::{
    rom::{HEADER_SIZE, PRG_SIZE, Rom},
    sha1_hex,
    tracked::TrackedImage,
};

use super::super::{
    OUTPUT_MAPPER, assemble_mapper165_parity_bytes,
    battle_composition_loader_probe::{
        BattleCompositionLoaderBuild, CUMULATIVE_RUNTIME_LAYOUT, build_battle_composition_loader,
    },
    battle_text_runtime_base::build_battle_text_runtime_base,
};
use super::{ROSTER_SELECTOR_ADDRESS, write_file};

const EXPANDED_PRG_SIZE: usize = 512 * 1024;
const FIXED_BANK_SIZE: usize = 16 * 1024;
const BATTLE_RUNTIME_BASE_ROM_NAME: &str = "battle-text-runtime-base.nes";
const BATTLE_RUNTIME_BASE_REPORT_NAME: &str = "battle-text-runtime-base.json";
const CUMULATIVE_BATTLE_BASE_ROM_NAME: &str = "cumulative-battle-base.nes";
const CUMULATIVE_BATTLE_BASE_REPORT_NAME: &str = "cumulative-battle-base.json";
pub(super) const BATTLE_STAGE_ROM_NAME: &str = "battle-composition.nes";
pub(super) const BATTLE_STAGE_REPORT_NAME: &str = "battle-composition.json";
const PRESERVED_SELECTOR_RANGES: [Range<u16>; 2] = [0xFB20..0xFC20, 0xFC60..0xFC99];

#[derive(Debug, Deserialize)]
struct BattleRuntimeBaseMetadata {
    fixed_workspace_sha1: String,
    dialogue_workspace_sha1: String,
    temporal_manifest_sha1: String,
    stable_color_count: usize,
    glyph_atlas_tile_count: usize,
    observed_runtime_tuple_count: usize,
    maximum_observed_overlay_count: usize,
}

pub(super) struct BattleStageOutput {
    pub(super) output: Vec<u8>,
    pub(super) output_sha1: String,
    pub(super) loader_report_sha1: String,
    pub(super) runtime_base_report_sha1: String,
    pub(super) fixed_workspace_sha1: String,
    pub(super) dialogue_workspace_sha1: String,
    pub(super) temporal_manifest_sha1: String,
    pub(super) fixed_entry_count: usize,
    pub(super) unit_name_count: usize,
    pub(super) enemy_name_count: usize,
    pub(super) class_name_count: usize,
    pub(super) item_name_count: usize,
    pub(super) terrain_name_count: usize,
    pub(super) battle_message_template_count: usize,
    pub(super) battle_forecast_label_count: usize,
    pub(super) installed_item_source_indices: BTreeSet<usize>,
    pub(super) dialogue_record_count: usize,
    pub(super) dialogue_translated_line_count: usize,
    pub(super) stable_color_count: usize,
    pub(super) glyph_atlas_tile_count: usize,
    pub(super) observed_runtime_tuple_count: usize,
    pub(super) maximum_observed_overlay_count: usize,
    pub(super) maximum_observed_ppu_write_count: usize,
    pub(super) runtime_routine_byte_count: usize,
    pub(super) tracked_write_count: usize,
    pub(super) text_diff_range_count: usize,
}

pub(super) struct BattleStageInputs<'a> {
    pub(super) prior_output: &'a [u8],
    pub(super) source_rom: &'a Rom,
    pub(super) source_path: &'a Path,
    pub(super) fixed_workspace_path: &'a Path,
    pub(super) dialogue_workspace_path: &'a Path,
    pub(super) temporal_manifest_path: &'a Path,
    pub(super) stage_directory: &'a Path,
}

pub(super) fn install_battle_stage(inputs: BattleStageInputs<'_>) -> Result<BattleStageOutput> {
    let runtime_base_path = inputs.stage_directory.join(BATTLE_RUNTIME_BASE_ROM_NAME);
    let runtime_base_report_path = inputs.stage_directory.join(BATTLE_RUNTIME_BASE_REPORT_NAME);
    let runtime_base = build_battle_text_runtime_base(
        inputs.source_path,
        inputs.fixed_workspace_path,
        inputs.dialogue_workspace_path,
        inputs.temporal_manifest_path,
        &runtime_base_path,
        &runtime_base_report_path,
    )?;
    let runtime_base_bytes = fs::read(&runtime_base_path)
        .with_context(|| format!("read {}", runtime_base_path.display()))?;
    ensure!(
        sha1_hex(&runtime_base_bytes) == runtime_base.output_sha1,
        "battle runtime base changed after production"
    );
    let runtime_base_report_bytes = fs::read(&runtime_base_report_path)
        .with_context(|| format!("read {}", runtime_base_report_path.display()))?;
    ensure!(
        sha1_hex(&runtime_base_report_bytes) == runtime_base.report_sha1,
        "battle runtime base report changed after production"
    );
    let metadata: BattleRuntimeBaseMetadata = serde_json::from_slice(&runtime_base_report_bytes)
        .context("parse battle runtime base metadata")?;

    let parity = assemble_mapper165_parity_bytes(inputs.source_rom)?;
    let parity_rom = Rom::parse(parity).context("parse cumulative battle parity baseline")?;
    let prior_rom =
        Rom::parse(inputs.prior_output.to_vec()).context("parse pre-battle cumulative output")?;
    let standalone_rom =
        Rom::parse(runtime_base_bytes).context("parse standalone battle runtime base")?;
    ensure!(
        prior_rom.mapper() == OUTPUT_MAPPER
            && prior_rom.prg().len() == PRG_SIZE
            && standalone_rom.mapper() == OUTPUT_MAPPER
            && standalone_rom.prg().len() == EXPANDED_PRG_SIZE,
        "cumulative battle stage input layout changed"
    );
    ensure!(
        parity_rom.prg().len() == PRG_SIZE
            && standalone_rom.prg()[..PRG_SIZE].len() == parity_rom.prg().len(),
        "cumulative battle parity comparison range changed"
    );

    let diff_ranges = differing_ranges(parity_rom.prg(), &standalone_rom.prg()[..PRG_SIZE]);
    ensure!(
        !diff_ranges.is_empty(),
        "cumulative battle stage found no translated text writes"
    );
    let mut image = TrackedImage::new(inputs.prior_output.to_vec());
    for (index, range) in diff_ranges.iter().enumerate() {
        image.write_expected(
            format!("cumulative battle text range {index}"),
            HEADER_SIZE + range.start,
            &parity_rom.prg()[range.clone()],
            &standalone_rom.prg()[range.clone()],
        )?;
    }
    image.verify_all_changes_tracked(inputs.prior_output)?;
    let translated_cumulative = image.into_data();
    let translated_rom =
        Rom::parse(translated_cumulative).context("parse cumulative output with battle text")?;
    let cumulative_base = expand_cumulative_prg(&translated_rom, &standalone_rom)?;
    let cumulative_base_rom =
        Rom::parse(cumulative_base.clone()).context("parse expanded cumulative battle base")?;
    ensure!(
        cumulative_base_rom.chr() == prior_rom.chr(),
        "cumulative battle expansion changed an earlier CHR page"
    );
    ensure!(
        cumulative_base_rom.prg()[..PRG_SIZE] == *translated_rom.prg(),
        "cumulative battle expansion changed the translated PRG prefix"
    );
    ensure!(
        cumulative_base_rom.prg()[EXPANDED_PRG_SIZE - FIXED_BANK_SIZE..]
            == translated_rom.prg()[PRG_SIZE - FIXED_BANK_SIZE..],
        "cumulative battle expansion did not duplicate the active fixed bank"
    );

    let cumulative_base_path = inputs.stage_directory.join(CUMULATIVE_BATTLE_BASE_ROM_NAME);
    let cumulative_base_report_path = inputs
        .stage_directory
        .join(CUMULATIVE_BATTLE_BASE_REPORT_NAME);
    write_file(&cumulative_base_path, &cumulative_base)?;
    let cumulative_base_sha1 = sha1_hex(&cumulative_base);
    let mut cumulative_contract: serde_json::Value =
        serde_json::from_slice(&runtime_base_report_bytes)
            .context("parse cumulative battle base contract")?;
    cumulative_contract["output_sha1"] = serde_json::Value::String(cumulative_base_sha1);
    let mut cumulative_contract_bytes = serde_json::to_vec_pretty(&cumulative_contract)
        .context("serialize cumulative battle base contract")?;
    cumulative_contract_bytes.push(b'\n');
    write_file(&cumulative_base_report_path, &cumulative_contract_bytes)?;

    let output_path = inputs.stage_directory.join(BATTLE_STAGE_ROM_NAME);
    let report_path = inputs.stage_directory.join(BATTLE_STAGE_REPORT_NAME);
    let loader = build_battle_composition_loader(BattleCompositionLoaderBuild {
        source_path: inputs.source_path,
        temporal_manifest_path: inputs.temporal_manifest_path,
        base_path: &cumulative_base_path,
        base_report_path: &cumulative_base_report_path,
        output_path: &output_path,
        report_path: &report_path,
        layout: CUMULATIVE_RUNTIME_LAYOUT,
        central_fallback_target: ROSTER_SELECTOR_ADDRESS,
    })?;
    let output =
        fs::read(&output_path).with_context(|| format!("read {}", output_path.display()))?;
    ensure!(
        sha1_hex(&output) == loader.output_sha1,
        "cumulative battle loader output changed after production"
    );
    let output_rom = Rom::parse(output.clone()).context("parse cumulative battle stage")?;
    ensure!(
        output_rom.mapper() == OUTPUT_MAPPER
            && output_rom.prg().len() == EXPANDED_PRG_SIZE
            && output_rom.chr() == prior_rom.chr(),
        "cumulative battle loader changed the intended media layout"
    );
    for range in PRESERVED_SELECTOR_RANGES {
        ensure!(
            fixed_bank_slice(&output_rom, range.clone())?
                == fixed_bank_slice(&prior_rom, range.clone())?,
            "cumulative battle loader changed selector range {:04X}..{:04X}",
            range.start,
            range.end
        );
    }

    Ok(BattleStageOutput {
        output,
        output_sha1: loader.output_sha1,
        loader_report_sha1: loader.report_sha1,
        runtime_base_report_sha1: runtime_base.report_sha1,
        fixed_workspace_sha1: metadata.fixed_workspace_sha1,
        dialogue_workspace_sha1: metadata.dialogue_workspace_sha1,
        temporal_manifest_sha1: metadata.temporal_manifest_sha1,
        fixed_entry_count: runtime_base.fixed_entry_count,
        unit_name_count: runtime_base.unit_name_count,
        enemy_name_count: runtime_base.enemy_name_count,
        class_name_count: runtime_base.class_name_count,
        item_name_count: runtime_base.item_name_count,
        terrain_name_count: runtime_base.terrain_name_count,
        battle_message_template_count: runtime_base.battle_message_template_count,
        battle_forecast_label_count: runtime_base.battle_forecast_label_count,
        installed_item_source_indices: runtime_base.installed_item_source_indices,
        dialogue_record_count: runtime_base.dialogue_record_count,
        dialogue_translated_line_count: runtime_base.dialogue_translated_line_count,
        stable_color_count: metadata.stable_color_count,
        glyph_atlas_tile_count: metadata.glyph_atlas_tile_count,
        observed_runtime_tuple_count: metadata.observed_runtime_tuple_count,
        maximum_observed_overlay_count: metadata.maximum_observed_overlay_count,
        maximum_observed_ppu_write_count: loader.maximum_observed_ppu_write_count,
        runtime_routine_byte_count: loader.runtime_routine_byte_count,
        tracked_write_count: diff_ranges.len() + loader.runtime_tracked_write_count,
        text_diff_range_count: diff_ranges.len(),
    })
}

fn expand_cumulative_prg(cumulative: &Rom, standalone: &Rom) -> Result<Vec<u8>> {
    ensure!(
        cumulative.prg().len() == PRG_SIZE && standalone.prg().len() == EXPANDED_PRG_SIZE,
        "cumulative battle PRG expansion input size changed"
    );
    let mut header = cumulative.data()[..HEADER_SIZE].to_vec();
    header[4] = u8::try_from(EXPANDED_PRG_SIZE / FIXED_BANK_SIZE)?;
    let mut expanded_prg = vec![0xFF; EXPANDED_PRG_SIZE];
    expanded_prg[..PRG_SIZE].copy_from_slice(cumulative.prg());
    expanded_prg[PRG_SIZE..EXPANDED_PRG_SIZE - FIXED_BANK_SIZE]
        .copy_from_slice(&standalone.prg()[PRG_SIZE..EXPANDED_PRG_SIZE - FIXED_BANK_SIZE]);
    expanded_prg[EXPANDED_PRG_SIZE - FIXED_BANK_SIZE..]
        .copy_from_slice(&cumulative.prg()[PRG_SIZE - FIXED_BANK_SIZE..]);
    let mut output = Vec::with_capacity(HEADER_SIZE + expanded_prg.len() + cumulative.chr().len());
    output.extend_from_slice(&header);
    output.extend_from_slice(&expanded_prg);
    output.extend_from_slice(cumulative.chr());
    Ok(output)
}

fn differing_ranges(expected: &[u8], replacement: &[u8]) -> Vec<Range<usize>> {
    assert_eq!(expected.len(), replacement.len());
    let mut ranges = Vec::new();
    let mut start = None;
    for (index, (expected, replacement)) in expected.iter().zip(replacement).enumerate() {
        match (start, expected == replacement) {
            (None, false) => start = Some(index),
            (Some(range_start), true) => {
                ranges.push(range_start..index);
                start = None;
            }
            _ => {}
        }
    }
    if let Some(range_start) = start {
        ranges.push(range_start..expected.len());
    }
    ranges
}

fn fixed_bank_slice(rom: &Rom, range: Range<u16>) -> Result<&[u8]> {
    ensure!(
        range.start >= 0xC000 && range.start <= range.end,
        "invalid fixed-bank comparison range"
    );
    let fixed_start = rom
        .prg()
        .len()
        .checked_sub(FIXED_BANK_SIZE)
        .context("ROM is smaller than one fixed bank")?;
    let start = fixed_start + usize::from(range.start - 0xC000);
    let end = fixed_start + usize::from(range.end - 0xC000);
    rom.prg()
        .get(start..end)
        .context("fixed-bank comparison range is outside ROM")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn differing_ranges_group_only_adjacent_changes() {
        assert_eq!(
            differing_ranges(&[0, 1, 2, 3, 4, 5], &[9, 1, 8, 7, 4, 6]),
            vec![0..1, 2..4, 5..6]
        );
    }
}
