use std::{fs, path::Path};

use anyhow::{Context, Result, ensure};
use serde::Serialize;

use crate::{
    dialogue_assets::plan_battle_dialogue_records,
    font::{load_dalmoori, rasterize_glyph},
    font_slots::FONT_PAGE_SIZE,
    rom::{EXPECTED_SOURCE_SHA1, HEADER_SIZE, Rom},
    sha1_hex,
    text_inventory::plan_fixed_text,
};

use super::{
    OUTPUT_MAPPER, battle_codebook_plan::plan_battle_cache_composition_material,
    dialogue_probe_font::SOURCE_FONT_PHYSICAL_PAGE, install_mapper165_parity_bytes,
};

const EXPANDED_PRG_SIZE: usize = 512 * 1024;
const FIXED_BANK_SIZE: usize = 16 * 1024;
pub(super) const GLYPH_ATLAS_PRG_OFFSET: usize = 256 * 1024;
const GLYPH_TILE_SIZE: usize = 16;
pub(super) const GLYPH_ATLAS_MMC3_PAGE: u8 = 0x20;
const MATERIAL_MMC3_PAGE_SIZE: usize = 8 * 1024;
pub(super) const PHYSICAL_CODE_TABLE_PRG_OFFSET: usize = GLYPH_ATLAS_PRG_OFFSET + 0x1400;
pub(super) const PHYSICAL_CODE_TABLE_CPU_ADDRESS: u16 = 0x9400;
pub(super) const CANONICAL_ABSTRACT_COLOR_COUNT: usize = 213;
pub(super) const PROTECTED_ABSTRACT_COLORS_PRG_OFFSET: usize =
    PHYSICAL_CODE_TABLE_PRG_OFFSET + CANONICAL_ABSTRACT_COLOR_COUNT;
pub(super) const PROTECTED_ABSTRACT_COLORS_CPU_ADDRESS: u16 =
    PHYSICAL_CODE_TABLE_CPU_ADDRESS + CANONICAL_ABSTRACT_COLOR_COUNT as u16;
pub(super) const PROTECTED_ABSTRACT_COLOR_COUNT: usize = 42;
pub(super) const SAFE_ABSTRACT_COLORS_PRG_OFFSET: usize =
    PROTECTED_ABSTRACT_COLORS_PRG_OFFSET + PROTECTED_ABSTRACT_COLOR_COUNT;
pub(super) const SAFE_ABSTRACT_COLORS_CPU_ADDRESS: u16 =
    PROTECTED_ABSTRACT_COLORS_CPU_ADDRESS + PROTECTED_ABSTRACT_COLOR_COUNT as u16;
pub(super) const SAFE_ABSTRACT_COLOR_COUNT: usize = 171;
pub(super) const COLOR_BIT_MASKS_PRG_OFFSET: usize =
    SAFE_ABSTRACT_COLORS_PRG_OFFSET + SAFE_ABSTRACT_COLOR_COUNT;
pub(super) const COLOR_BIT_MASKS_CPU_ADDRESS: u16 =
    SAFE_ABSTRACT_COLORS_CPU_ADDRESS + SAFE_ABSTRACT_COLOR_COUNT as u16;
pub(super) const DYNAMIC_ASSIGNMENT_CODE_PRG_OFFSET: usize = GLYPH_ATLAS_PRG_OFFSET + 0x15C0;
pub(super) const DYNAMIC_ASSIGNMENT_CODE_CPU_ADDRESS: u16 = 0x95C0;
pub(super) const SOURCE_PAGE_PRG_OFFSET: usize = GLYPH_ATLAS_PRG_OFFSET + MATERIAL_MMC3_PAGE_SIZE;
pub(super) const SOURCE_PAGE_MMC3_PAGE: u8 = 0x21;
pub(super) const RECIPE_BLOB_PRG_OFFSET: usize = SOURCE_PAGE_PRG_OFFSET + FONT_PAGE_SIZE;
pub(super) const RECIPE_BLOB_MMC3_PAGE: u8 = SOURCE_PAGE_MMC3_PAGE;

pub(super) const COLOR_BIT_MASKS: [u8; 8] = [0x01, 0x02, 0x04, 0x08, 0x10, 0x20, 0x40, 0x80];

pub(super) struct DynamicAssignmentMaterial<'a> {
    pub(super) canonical_color_codes: &'a [u8],
    pub(super) protected_abstract_colors: &'a [u8],
    pub(super) safe_abstract_colors: &'a [u8],
}

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
    source_page_byte_count: usize,
    source_page_sha1: String,
    source_page_mmc3_page: u8,
    recipe_blob_byte_count: usize,
    recipe_blob_sha1: String,
    recipe_blob_mmc3_page: u8,
    material_page_count: usize,
    original_prg_prefix_preserved: bool,
    active_fixed_bank_duplicate_sha1: String,
    chr_sha1: String,
    glyph_characters_emitted: bool,
    translation_text_emitted: bool,
    runtime_cache_installed: bool,
    runtime_recipe_loader_installed: bool,
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
    let material = plan_battle_cache_composition_material(&source_rom, &fixed, &dialogue)?;
    let glyph_atlas = rasterize_atlas(&material.atlas_glyphs)?;
    ensure!(
        glyph_atlas.len() <= MATERIAL_MMC3_PAGE_SIZE,
        "battle glyph atlas exceeds one MMC3 PRG page"
    );
    ensure!(
        material.recipe_blob.len() <= MATERIAL_MMC3_PAGE_SIZE,
        "battle recipe blob exceeds one MMC3 PRG page"
    );

    let parity = install_mapper165_parity_bytes(&source_rom)?;
    let parity_rom = Rom::parse(parity).context("parse mapper 165 battle cache base")?;
    let source_page_start = SOURCE_FONT_PHYSICAL_PAGE * FONT_PAGE_SIZE;
    let source_page = parity_rom
        .chr()
        .get(source_page_start..source_page_start + FONT_PAGE_SIZE)
        .context("battle source page is outside mapper parity CHR")?;
    let output = expand_prg_with_material(
        &parity_rom,
        &glyph_atlas,
        None,
        source_page,
        &material.recipe_blob,
    )?;
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
        schema: 2,
        source_sha1: EXPECTED_SOURCE_SHA1,
        fixed_workspace_sha1: sha1_hex(&fs::read(fixed_workspace_path)?),
        dialogue_workspace_sha1: sha1_hex(&fs::read(dialogue_workspace_path)?),
        output_sha1: output_sha1.clone(),
        output_mapper: output_rom.mapper(),
        prg_size: output_rom.prg().len(),
        chr_size: output_rom.chr().len(),
        glyph_count: material.atlas_glyphs.len(),
        glyph_atlas_byte_count: glyph_atlas.len(),
        glyph_atlas_sha1: sha1_hex(&glyph_atlas),
        glyph_atlas_mmc3_page: GLYPH_ATLAS_MMC3_PAGE,
        source_page_byte_count: source_page.len(),
        source_page_sha1: sha1_hex(source_page),
        source_page_mmc3_page: SOURCE_PAGE_MMC3_PAGE,
        recipe_blob_byte_count: material.recipe_blob.len(),
        recipe_blob_sha1: sha1_hex(&material.recipe_blob),
        recipe_blob_mmc3_page: RECIPE_BLOB_MMC3_PAGE,
        material_page_count: 2,
        original_prg_prefix_preserved: true,
        active_fixed_bank_duplicate_sha1: sha1_hex(active_fixed),
        chr_sha1: sha1_hex(output_rom.chr()),
        glyph_characters_emitted: false,
        translation_text_emitted: false,
        runtime_cache_installed: false,
        runtime_recipe_loader_installed: false,
        release_eligible: false,
        next_gate: "implement the transition loader that restores the source page and applies selected recipes before choosing CHR RAM",
    };
    let mut report_bytes =
        serde_json::to_vec_pretty(&report).context("serialize battle cache base report")?;
    report_bytes.push(b'\n');
    write_file(output_path, &output)?;
    write_file(report_path, &report_bytes)?;
    Ok(BattleTextCacheBaseSummary {
        output_sha1,
        report_sha1: sha1_hex(&report_bytes),
        glyph_count: material.atlas_glyphs.len(),
        glyph_atlas_byte_count: glyph_atlas.len(),
    })
}

pub(super) fn rasterize_atlas(glyphs: &[char]) -> Result<Vec<u8>> {
    let font = load_dalmoori()?;
    glyphs.iter().try_fold(
        Vec::with_capacity(glyphs.len() * GLYPH_TILE_SIZE),
        |mut atlas, glyph| {
            atlas.extend_from_slice(&rasterize_glyph(&font, *glyph)?);
            Ok(atlas)
        },
    )
}

pub(super) fn expand_prg_with_material(
    parity_rom: &Rom,
    atlas: &[u8],
    dynamic_assignment: Option<&DynamicAssignmentMaterial<'_>>,
    source_page: &[u8],
    recipe_blob: &[u8],
) -> Result<Vec<u8>> {
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
    ensure!(
        atlas_end <= PHYSICAL_CODE_TABLE_PRG_OFFSET,
        "battle glyph atlas overlaps the physical-code table"
    );
    expanded_prg[GLYPH_ATLAS_PRG_OFFSET..atlas_end].copy_from_slice(atlas);
    if let Some(dynamic) = dynamic_assignment {
        ensure!(
            dynamic.canonical_color_codes.len() == CANONICAL_ABSTRACT_COLOR_COUNT
                && dynamic.protected_abstract_colors.len() == PROTECTED_ABSTRACT_COLOR_COUNT
                && dynamic.safe_abstract_colors.len() == SAFE_ABSTRACT_COLOR_COUNT,
            "battle dynamic-assignment material dimensions changed"
        );
        ensure!(
            dynamic
                .protected_abstract_colors
                .iter()
                .chain(dynamic.safe_abstract_colors)
                .copied()
                .collect::<std::collections::BTreeSet<_>>()
                .len()
                == CANONICAL_ABSTRACT_COLOR_COUNT,
            "battle dynamic-assignment abstract color partitions overlap"
        );
        let physical_code_table_end = PHYSICAL_CODE_TABLE_PRG_OFFSET
            .checked_add(dynamic.canonical_color_codes.len())
            .context("canonical-code table range overflow")?;
        ensure!(
            physical_code_table_end == PROTECTED_ABSTRACT_COLORS_PRG_OFFSET,
            "battle canonical-code table no longer meets the protected-color list"
        );
        expanded_prg[PHYSICAL_CODE_TABLE_PRG_OFFSET..physical_code_table_end]
            .copy_from_slice(dynamic.canonical_color_codes);
        expanded_prg[PROTECTED_ABSTRACT_COLORS_PRG_OFFSET..SAFE_ABSTRACT_COLORS_PRG_OFFSET]
            .copy_from_slice(dynamic.protected_abstract_colors);
        expanded_prg[SAFE_ABSTRACT_COLORS_PRG_OFFSET..COLOR_BIT_MASKS_PRG_OFFSET]
            .copy_from_slice(dynamic.safe_abstract_colors);
        let mask_end = COLOR_BIT_MASKS_PRG_OFFSET + COLOR_BIT_MASKS.len();
        ensure!(
            mask_end <= DYNAMIC_ASSIGNMENT_CODE_PRG_OFFSET,
            "battle dynamic-assignment tables overlap runtime code"
        );
        expanded_prg[COLOR_BIT_MASKS_PRG_OFFSET..mask_end].copy_from_slice(&COLOR_BIT_MASKS);
    }
    ensure!(
        source_page.len() == FONT_PAGE_SIZE,
        "battle source material is not one 4 KiB page"
    );
    expanded_prg[SOURCE_PAGE_PRG_OFFSET..SOURCE_PAGE_PRG_OFFSET + source_page.len()]
        .copy_from_slice(source_page);
    let recipe_end = RECIPE_BLOB_PRG_OFFSET
        .checked_add(recipe_blob.len())
        .context("battle recipe material range overflow")?;
    ensure!(
        recipe_end <= SOURCE_PAGE_PRG_OFFSET + MATERIAL_MMC3_PAGE_SIZE,
        "battle source page and recipe material exceed their shared MMC3 page"
    );
    expanded_prg[RECIPE_BLOB_PRG_OFFSET..recipe_end].copy_from_slice(recipe_blob);
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
    fn expanded_prg_places_two_material_pages_before_a_duplicate_fixed_bank() {
        let mut image = vec![0; HEADER_SIZE + 256 * 1024];
        image[..4].copy_from_slice(b"NES\x1A");
        image[4] = 0x10;
        image[6] = 0x50;
        image[7] = 0xA0;
        image[HEADER_SIZE + 256 * 1024 - FIXED_BANK_SIZE..].fill(0xA5);
        let rom = Rom::parse(image).unwrap();
        let source_page = vec![2; FONT_PAGE_SIZE];
        let canonical = (u8::MIN..=u8::MAX)
            .take(CANONICAL_ABSTRACT_COLOR_COUNT)
            .collect::<Vec<_>>();
        let protected = canonical[..PROTECTED_ABSTRACT_COLOR_COUNT].to_vec();
        let safe = canonical[PROTECTED_ABSTRACT_COLOR_COUNT..].to_vec();
        let output = expand_prg_with_material(
            &rom,
            &[1, 2, 3],
            Some(&DynamicAssignmentMaterial {
                canonical_color_codes: &canonical,
                protected_abstract_colors: &protected,
                safe_abstract_colors: &safe,
            }),
            &source_page,
            &[4, 5],
        )
        .unwrap();
        let expanded = Rom::parse(output).unwrap();
        assert_eq!(GLYPH_ATLAS_MMC3_PAGE + 1, SOURCE_PAGE_MMC3_PAGE);
        assert_eq!(SOURCE_PAGE_MMC3_PAGE, RECIPE_BLOB_MMC3_PAGE);
        assert_eq!(
            &expanded.prg()[GLYPH_ATLAS_PRG_OFFSET..GLYPH_ATLAS_PRG_OFFSET + 3],
            &[1, 2, 3]
        );
        assert_eq!(
            &expanded.prg()
                [PHYSICAL_CODE_TABLE_PRG_OFFSET..PHYSICAL_CODE_TABLE_PRG_OFFSET + canonical.len()],
            canonical
        );
        assert_eq!(
            &expanded.prg()[PROTECTED_ABSTRACT_COLORS_PRG_OFFSET..SAFE_ABSTRACT_COLORS_PRG_OFFSET],
            protected
        );
        assert_eq!(
            &expanded.prg()[SAFE_ABSTRACT_COLORS_PRG_OFFSET..COLOR_BIT_MASKS_PRG_OFFSET],
            safe
        );
        assert_eq!(
            &expanded.prg()[COLOR_BIT_MASKS_PRG_OFFSET..COLOR_BIT_MASKS_PRG_OFFSET + 8],
            COLOR_BIT_MASKS
        );
        assert_eq!(
            &expanded.prg()[SOURCE_PAGE_PRG_OFFSET..SOURCE_PAGE_PRG_OFFSET + FONT_PAGE_SIZE],
            source_page
        );
        assert_eq!(
            &expanded.prg()[RECIPE_BLOB_PRG_OFFSET..RECIPE_BLOB_PRG_OFFSET + 2],
            &[4, 5]
        );
        assert!(
            expanded.prg()
                [RECIPE_BLOB_PRG_OFFSET + 2..SOURCE_PAGE_PRG_OFFSET + MATERIAL_MMC3_PAGE_SIZE]
                .iter()
                .all(|byte| *byte == 0xFF)
        );
        assert!(
            expanded.prg()[EXPANDED_PRG_SIZE - FIXED_BANK_SIZE..]
                .iter()
                .all(|byte| *byte == 0xA5)
        );
    }
}
