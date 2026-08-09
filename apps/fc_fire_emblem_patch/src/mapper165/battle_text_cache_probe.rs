use std::{collections::BTreeSet, fs, path::Path};

use anyhow::{Context, Result, ensure};
use serde::Serialize;

use crate::{
    battle_text_workset::FORECAST_LABEL_GLYPHS,
    dialogue_assets::plan_battle_dialogue_records,
    font::{load_dalmoori, rasterize_glyph},
    rom::{EXPECTED_SOURCE_SHA1, HEADER_SIZE, Rom},
    sha1_hex,
    text_inventory::plan_fixed_text,
};

use super::{OUTPUT_MAPPER, assemble_mapper165_parity_bytes};

const EXPANDED_PRG_SIZE: usize = 512 * 1024;
const FIXED_BANK_SIZE: usize = 16 * 1024;
const GLYPH_ATLAS_PRG_OFFSET: usize = 256 * 1024;
const GLYPH_TILE_SIZE: usize = 16;
const GLYPH_ATLAS_MMC3_PAGE: u8 = 0x20;

#[derive(Debug, Serialize)]
struct BattleTextCacheBaseReport {
    schema: u8,
    source_sha1: &'static str,
    fixed_workspace_sha1: String,
    dialogue_workspace_sha1: String,
    output_sha1: String,
    output_mapper: u16,
    prg_size: usize,
    chr_size: usize,
    glyph_count: usize,
    glyph_atlas_byte_count: usize,
    glyph_atlas_sha1: String,
    glyph_atlas_mmc3_page: u8,
    original_prg_prefix_preserved: bool,
    active_fixed_bank_duplicate_sha1: String,
    chr_sha1: String,
    glyph_characters_emitted: bool,
    translation_text_emitted: bool,
    runtime_cache_installed: bool,
    release_eligible: bool,
    next_gate: &'static str,
}

pub(crate) struct BattleTextCacheBaseSummary {
    pub(crate) output_sha1: String,
    pub(crate) report_sha1: String,
    pub(crate) glyph_count: usize,
    pub(crate) glyph_atlas_byte_count: usize,
}

pub(crate) fn build_battle_text_cache_base(
    source_path: &Path,
    fixed_workspace_path: &Path,
    dialogue_workspace_path: &Path,
    output_path: &Path,
    report_path: &Path,
) -> Result<BattleTextCacheBaseSummary> {
    let source_rom = Rom::from_path(source_path)?;
    source_rom.verify_supported_japanese()?;
    let fixed = plan_fixed_text(&source_rom, fixed_workspace_path)?;
    let dialogue = plan_battle_dialogue_records(&source_rom, dialogue_workspace_path)?;
    let glyphs = fixed
        .unique_glyphs()
        .union(&dialogue.unique_glyphs())
        .copied()
        .chain(FORECAST_LABEL_GLYPHS)
        .collect::<BTreeSet<_>>();
    let glyph_atlas = rasterize_atlas(&glyphs)?;
    ensure!(
        glyph_atlas.len() <= 8 * 1024,
        "battle glyph atlas exceeds one MMC3 PRG page"
    );

    let parity = assemble_mapper165_parity_bytes(&source_rom)?;
    let parity_rom = Rom::parse(parity).context("parse mapper 165 battle cache base")?;
    let output = expand_prg_with_atlas(&parity_rom, &glyph_atlas)?;
    let output_rom = Rom::parse(output.clone()).context("parse expanded battle cache base")?;
    ensure!(
        output_rom.mapper() == OUTPUT_MAPPER,
        "battle cache base mapper changed"
    );
    ensure!(
        output_rom.prg().len() == EXPANDED_PRG_SIZE,
        "battle cache base PRG size changed"
    );
    ensure!(
        output_rom.chr() == parity_rom.chr(),
        "battle cache base CHR changed"
    );
    ensure!(
        &output_rom.prg()[..parity_rom.prg().len()] == parity_rom.prg(),
        "battle cache base changed the parity PRG prefix"
    );
    let parity_fixed = &parity_rom.prg()[parity_rom.prg().len() - FIXED_BANK_SIZE..];
    let active_fixed = &output_rom.prg()[EXPANDED_PRG_SIZE - FIXED_BANK_SIZE..];
    ensure!(
        active_fixed == parity_fixed,
        "expanded active fixed bank is not the mapper parity fixed bank"
    );
    let output_sha1 = sha1_hex(&output);
    let report = BattleTextCacheBaseReport {
        schema: 1,
        source_sha1: EXPECTED_SOURCE_SHA1,
        fixed_workspace_sha1: sha1_hex(&fs::read(fixed_workspace_path)?),
        dialogue_workspace_sha1: sha1_hex(&fs::read(dialogue_workspace_path)?),
        output_sha1: output_sha1.clone(),
        output_mapper: output_rom.mapper(),
        prg_size: output_rom.prg().len(),
        chr_size: output_rom.chr().len(),
        glyph_count: glyphs.len(),
        glyph_atlas_byte_count: glyph_atlas.len(),
        glyph_atlas_sha1: sha1_hex(&glyph_atlas),
        glyph_atlas_mmc3_page: GLYPH_ATLAS_MMC3_PAGE,
        original_prg_prefix_preserved: true,
        active_fixed_bank_duplicate_sha1: sha1_hex(active_fixed),
        chr_sha1: sha1_hex(output_rom.chr()),
        glyph_characters_emitted: false,
        translation_text_emitted: false,
        runtime_cache_installed: false,
        release_eligible: false,
        next_gate: "bind a battle-transition upload window, copy selected atlas tiles into mapper 165 CHR RAM, and restore the original PRG bank",
    };
    let mut report_bytes =
        serde_json::to_vec_pretty(&report).context("serialize battle cache base report")?;
    report_bytes.push(b'\n');
    write_file(output_path, &output)?;
    write_file(report_path, &report_bytes)?;
    Ok(BattleTextCacheBaseSummary {
        output_sha1,
        report_sha1: sha1_hex(&report_bytes),
        glyph_count: glyphs.len(),
        glyph_atlas_byte_count: glyph_atlas.len(),
    })
}

fn rasterize_atlas(glyphs: &BTreeSet<char>) -> Result<Vec<u8>> {
    let font = load_dalmoori()?;
    glyphs.iter().try_fold(
        Vec::with_capacity(glyphs.len() * GLYPH_TILE_SIZE),
        |mut atlas, glyph| {
            atlas.extend_from_slice(&rasterize_glyph(&font, *glyph)?);
            Ok(atlas)
        },
    )
}

fn expand_prg_with_atlas(parity_rom: &Rom, atlas: &[u8]) -> Result<Vec<u8>> {
    ensure!(
        parity_rom.prg().len() == 256 * 1024,
        "mapper parity PRG size changed"
    );
    let mut header = parity_rom.data()[..HEADER_SIZE].to_vec();
    header[4] = u8::try_from(EXPANDED_PRG_SIZE / (16 * 1024))?;
    let mut expanded_prg = vec![0xFF; EXPANDED_PRG_SIZE];
    expanded_prg[..parity_rom.prg().len()].copy_from_slice(parity_rom.prg());
    let atlas_end = GLYPH_ATLAS_PRG_OFFSET
        .checked_add(atlas.len())
        .context("glyph atlas range overflow")?;
    expanded_prg[GLYPH_ATLAS_PRG_OFFSET..atlas_end].copy_from_slice(atlas);
    let source_fixed_start = parity_rom.prg().len() - FIXED_BANK_SIZE;
    expanded_prg[EXPANDED_PRG_SIZE - FIXED_BANK_SIZE..]
        .copy_from_slice(&parity_rom.prg()[source_fixed_start..]);
    let mut output = Vec::with_capacity(HEADER_SIZE + EXPANDED_PRG_SIZE + parity_rom.chr().len());
    output.extend_from_slice(&header);
    output.extend_from_slice(&expanded_prg);
    output.extend_from_slice(parity_rom.chr());
    Ok(output)
}

fn write_file(path: &Path, data: &[u8]) -> Result<()> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).with_context(|| format!("create {}", parent.display()))?;
    }
    fs::write(path, data).with_context(|| format!("write {}", path.display()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn expanded_prg_places_the_atlas_before_a_duplicate_fixed_bank() {
        let mut image = vec![0; HEADER_SIZE + 256 * 1024];
        image[..4].copy_from_slice(b"NES\x1A");
        image[4] = 0x10;
        image[6] = 0x50;
        image[7] = 0xA0;
        image[HEADER_SIZE + 256 * 1024 - FIXED_BANK_SIZE..].fill(0xA5);
        let rom = Rom::parse(image).unwrap();
        let output = expand_prg_with_atlas(&rom, &[1, 2, 3]).unwrap();
        let expanded = Rom::parse(output).unwrap();
        assert_eq!(
            &expanded.prg()[GLYPH_ATLAS_PRG_OFFSET..GLYPH_ATLAS_PRG_OFFSET + 3],
            &[1, 2, 3]
        );
        assert!(
            expanded.prg()[EXPANDED_PRG_SIZE - FIXED_BANK_SIZE..]
                .iter()
                .all(|byte| *byte == 0xA5)
        );
    }
}
