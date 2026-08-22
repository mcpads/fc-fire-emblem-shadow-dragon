use std::{collections::BTreeSet, fs, path::Path};

use anyhow::{Context, Result, ensure};
use serde::Serialize;

use crate::{
    battle_text_workset::{
        FORECAST_LABEL_FILE_OFFSET, FORECAST_LABEL_GLYPHS, FORECAST_LABEL_SOURCE,
    },
    dialogue_assets::plan_battle_dialogue_records,
    font_slots::FONT_PAGE_SIZE,
    rom::{EXPECTED_SOURCE_SHA1, Rom},
    sha1_hex,
    temporal_surface::load_observed_battle_temporal_evidence,
    text_inventory::plan_fixed_text,
    tracked::TrackedImage,
};

use super::{
    OUTPUT_MAPPER,
    battle_codebook_plan::{
        plan_battle_cache_composition_material, plan_canonical_battle_codebook,
        surface_constraints::select_observed_battle_surfaces,
    },
    battle_text_material::{
        CANONICAL_ABSTRACT_COLOR_COUNT, COLOR_BIT_MASKS, COLOR_BIT_MASKS_CPU_ADDRESS,
        COLOR_BIT_MASKS_PRG_OFFSET, DynamicAssignmentMaterial, GLYPH_ATLAS_MMC3_PAGE,
        GLYPH_ATLAS_PRG_OFFSET, PHYSICAL_CODE_TABLE_CPU_ADDRESS, PHYSICAL_CODE_TABLE_PRG_OFFSET,
        PROTECTED_ABSTRACT_COLORS_CPU_ADDRESS, PROTECTED_ABSTRACT_COLORS_PRG_OFFSET,
        RECIPE_BLOB_MMC3_PAGE, RECIPE_BLOB_PRG_OFFSET, SAFE_ABSTRACT_COLORS_CPU_ADDRESS,
        SAFE_ABSTRACT_COLORS_PRG_OFFSET, SOURCE_PAGE_MMC3_PAGE, SOURCE_PAGE_PRG_OFFSET,
        expand_prg_with_material, rasterize_atlas,
    },
    dialogue_font_page::SOURCE_FONT_PHYSICAL_PAGE,
    install_mapper165_parity_bytes,
};

#[cfg(test)]
use super::battle_text_material::{PROTECTED_ABSTRACT_COLOR_COUNT, SAFE_ABSTRACT_COLOR_COUNT};

const EXPANDED_PRG_SIZE: usize = 512 * 1024;
const FIXED_BANK_SIZE: usize = 16 * 1024;
const GLYPH_TILE_SIZE: usize = 16;

#[derive(Debug, Serialize)]
struct BattleTextRuntimeBaseReport {
    schema: u8,
    source_sha1: &'static str,
    fixed_workspace_sha1: String,
    dialogue_workspace_sha1: String,
    temporal_manifest_sha1: String,
    output_sha1: String,
    output_mapper: u16,
    prg_size: usize,
    chr_size: usize,
    fixed_source_entry_count: usize,
    fixed_reinserted_entry_count: usize,
    fixed_preserved_nonbattle_entry_count: usize,
    installed_unit_name_count: usize,
    installed_enemy_name_count: usize,
    installed_class_name_count: usize,
    installed_item_name_count: usize,
    installed_terrain_name_count: usize,
    installed_battle_message_template_count: usize,
    installed_battle_forecast_label_count: usize,
    dialogue_record_count: usize,
    dialogue_pointer_write_count: usize,
    dialogue_translated_line_count: usize,
    forecast_label_write_count: usize,
    text_tracked_write_count: usize,
    observed_battle_sample_count: usize,
    observed_runtime_tuple_count: usize,
    maximum_observed_overlay_count: usize,
    stable_color_count: usize,
    borrowed_logical_code_count: usize,
    abstract_assignment_sha1: String,
    canonical_assignment_sha1: String,
    canonical_code_table_byte_count: usize,
    canonical_code_table_sha1: String,
    canonical_code_table_cpu_address_hex: String,
    protected_physical_code_count: usize,
    protected_abstract_color_count: usize,
    protected_abstract_colors_sha1: String,
    protected_abstract_colors_cpu_address_hex: String,
    safe_abstract_color_count: usize,
    safe_abstract_colors_sha1: String,
    safe_abstract_colors_cpu_address_hex: String,
    color_bit_mask_byte_count: usize,
    color_bit_masks_sha1: String,
    color_bit_masks_cpu_address_hex: String,
    maximum_remap_pair_count: usize,
    glyph_atlas_tile_count: usize,
    glyph_atlas_byte_count: usize,
    glyph_atlas_sha1: String,
    glyph_atlas_mmc3_page: u8,
    source_page_byte_count: usize,
    source_page_sha1: String,
    source_page_mmc3_page: u8,
    recipe_blob_byte_count: usize,
    recipe_blob_sha1: String,
    recipe_blob_mmc3_page: u8,
    original_chr_preserved: bool,
    original_english_and_digits_preserved: bool,
    battle_catalog_fixed_text_reinserted: bool,
    battle_dialogue_reinserted: bool,
    forecast_label_reinserted: bool,
    dynamic_assignment_source_contract_complete: bool,
    translation_review_complete: bool,
    runtime_loader_installed: bool,
    translation_text_emitted: bool,
    glyph_characters_emitted: bool,
    runtime_verified: bool,
    release_eligible: bool,
    next_gate: &'static str,
}

pub(crate) struct BattleTextRuntimeBaseSummary {
    pub(crate) output_sha1: String,
    pub(crate) report_sha1: String,
    pub(crate) fixed_entry_count: usize,
    pub(crate) unit_name_count: usize,
    pub(crate) enemy_name_count: usize,
    pub(crate) class_name_count: usize,
    pub(crate) item_name_count: usize,
    pub(crate) terrain_name_count: usize,
    pub(crate) battle_message_template_count: usize,
    pub(crate) battle_forecast_label_count: usize,
    pub(crate) installed_item_source_indices: BTreeSet<usize>,
    pub(crate) dialogue_record_count: usize,
    pub(crate) dialogue_translated_line_count: usize,
    pub(crate) tracked_write_count: usize,
}

pub(crate) fn build_battle_text_runtime_base(
    source_path: &Path,
    fixed_workspace_path: &Path,
    dialogue_workspace_path: &Path,
    temporal_manifest_path: &Path,
    output_path: &Path,
    report_path: &Path,
) -> Result<BattleTextRuntimeBaseSummary> {
    let source_rom = Rom::from_path(source_path)?;
    let parity = install_mapper165_parity_bytes(&source_rom)?;
    build_battle_text_runtime_base_on_parity(
        &source_rom,
        source_path,
        &parity,
        fixed_workspace_path,
        dialogue_workspace_path,
        temporal_manifest_path,
        output_path,
        report_path,
    )
}

pub(crate) fn build_battle_text_runtime_base_on_parity(
    source_rom: &Rom,
    source_path: &Path,
    parity: &[u8],
    fixed_workspace_path: &Path,
    dialogue_workspace_path: &Path,
    temporal_manifest_path: &Path,
    output_path: &Path,
    report_path: &Path,
) -> Result<BattleTextRuntimeBaseSummary> {
    source_rom.verify_supported_japanese()?;
    let fixed = plan_fixed_text(source_rom, fixed_workspace_path)?;
    let dialogue = plan_battle_dialogue_records(source_rom, dialogue_workspace_path)?;
    let material = plan_battle_cache_composition_material(source_rom, &fixed, &dialogue)?;
    let evidence = load_observed_battle_temporal_evidence(source_path, temporal_manifest_path)?;
    let observed = select_observed_battle_surfaces(source_rom, &material, &evidence)?;
    let codebook = plan_canonical_battle_codebook(source_rom, &fixed, &dialogue)?;
    ensure!(
        codebook.color_codes.len() == CANONICAL_ABSTRACT_COLOR_COUNT,
        "battle runtime base canonical table does not fill the logical codebook"
    );
    ensure!(
        codebook
            .color_codes
            .iter()
            .copied()
            .collect::<std::collections::BTreeSet<_>>()
            .len()
            == codebook.color_codes.len(),
        "battle runtime base canonical table reuses a tile code"
    );

    let parity_rom = Rom::parse(parity.to_vec()).context("parse mapper 165 battle text parity")?;
    let mut image = TrackedImage::new(parity.to_vec());
    let fixed_installation =
        install_fixed_text(&mut image, parity, &fixed, &material, &codebook.glyph_codes)?;
    let fixed_reinserted_entry_count = fixed_installation.total_count();
    let fixed_preserved_nonbattle_entry_count = fixed
        .entries
        .len()
        .checked_sub(fixed_reinserted_entry_count)
        .context("battle fixed-text reinsertion count exceeds its source catalog")?;
    let dialogue_pointer_write_count =
        install_battle_dialogue(&mut image, parity, &dialogue, &codebook.glyph_codes)?;
    install_forecast_label(&mut image, parity, &codebook.glyph_codes)?;
    image.verify_all_changes_tracked(parity)?;
    let text_tracked_write_count = image.writes().len();
    ensure!(
        text_tracked_write_count
            == fixed_reinserted_entry_count
                + dialogue.records.len()
                + dialogue_pointer_write_count
                + 1,
        "battle runtime text write accounting changed"
    );
    let translated_parity = image.into_data();
    let translated_parity_rom =
        Rom::parse(translated_parity).context("parse translated mapper 165 battle text")?;

    let glyph_atlas = rasterize_atlas(&material.atlas_glyphs)?;
    ensure!(
        glyph_atlas.len() == material.atlas_glyphs.len() * GLYPH_TILE_SIZE,
        "battle runtime glyph atlas size changed"
    );
    let source_page_start = SOURCE_FONT_PHYSICAL_PAGE * FONT_PAGE_SIZE;
    let source_page = translated_parity_rom
        .chr()
        .get(source_page_start..source_page_start + FONT_PAGE_SIZE)
        .context("battle runtime source page is outside mapper parity CHR")?;
    let output = expand_prg_with_material(
        &translated_parity_rom,
        &glyph_atlas,
        Some(&DynamicAssignmentMaterial {
            canonical_color_codes: &codebook.color_codes,
            protected_abstract_colors: &codebook.protected_abstract_colors,
            safe_abstract_colors: &codebook.safe_abstract_colors,
        }),
        source_page,
        &material.recipe_blob,
    )?;
    let output_rom = Rom::parse(output.clone()).context("parse battle text runtime base")?;
    ensure!(
        output_rom.mapper() == OUTPUT_MAPPER,
        "battle text runtime base mapper changed"
    );
    ensure!(
        output_rom.prg().len() == EXPANDED_PRG_SIZE,
        "battle text runtime base PRG size changed"
    );
    ensure!(
        output_rom.chr() == parity_rom.chr(),
        "battle text runtime base changed source CHR"
    );
    ensure!(
        &output_rom.prg()[..translated_parity_rom.prg().len()] == translated_parity_rom.prg(),
        "battle text runtime base changed the translated PRG prefix"
    );
    let translated_fixed =
        &translated_parity_rom.prg()[translated_parity_rom.prg().len() - FIXED_BANK_SIZE..];
    let active_fixed = &output_rom.prg()[EXPANDED_PRG_SIZE - FIXED_BANK_SIZE..];
    ensure!(
        active_fixed == translated_fixed,
        "battle text runtime base did not duplicate the translated active fixed bank"
    );
    ensure!(
        output_rom.prg()[GLYPH_ATLAS_PRG_OFFSET..GLYPH_ATLAS_PRG_OFFSET + glyph_atlas.len()]
            == glyph_atlas,
        "battle runtime glyph atlas changed after expansion"
    );
    ensure!(
        output_rom.prg()[PHYSICAL_CODE_TABLE_PRG_OFFSET
            ..PHYSICAL_CODE_TABLE_PRG_OFFSET + codebook.color_codes.len()]
            == codebook.color_codes,
        "battle runtime canonical-code table changed after expansion"
    );
    ensure!(
        output_rom.prg()[PROTECTED_ABSTRACT_COLORS_PRG_OFFSET
            ..PROTECTED_ABSTRACT_COLORS_PRG_OFFSET + codebook.protected_abstract_colors.len()]
            == codebook.protected_abstract_colors,
        "battle runtime protected abstract-color list changed after expansion"
    );
    ensure!(
        output_rom.prg()[SAFE_ABSTRACT_COLORS_PRG_OFFSET
            ..SAFE_ABSTRACT_COLORS_PRG_OFFSET + codebook.safe_abstract_colors.len()]
            == codebook.safe_abstract_colors,
        "battle runtime safe abstract-color list changed after expansion"
    );
    ensure!(
        output_rom.prg()
            [COLOR_BIT_MASKS_PRG_OFFSET..COLOR_BIT_MASKS_PRG_OFFSET + COLOR_BIT_MASKS.len()]
            == COLOR_BIT_MASKS,
        "battle runtime color bit-mask table changed after expansion"
    );
    ensure!(
        &output_rom.prg()[SOURCE_PAGE_PRG_OFFSET..SOURCE_PAGE_PRG_OFFSET + source_page.len()]
            == source_page,
        "battle runtime source page changed after expansion"
    );
    ensure!(
        output_rom.prg()
            [RECIPE_BLOB_PRG_OFFSET..RECIPE_BLOB_PRG_OFFSET + material.recipe_blob.len()]
            == material.recipe_blob,
        "battle runtime recipe blob changed after expansion"
    );

    let output_sha1 = sha1_hex(&output);
    let report = BattleTextRuntimeBaseReport {
        schema: 4,
        source_sha1: EXPECTED_SOURCE_SHA1,
        fixed_workspace_sha1: sha1_hex(&fs::read(fixed_workspace_path)?),
        dialogue_workspace_sha1: sha1_hex(&fs::read(dialogue_workspace_path)?),
        temporal_manifest_sha1: evidence.manifest_sha1,
        output_sha1: output_sha1.clone(),
        output_mapper: output_rom.mapper(),
        prg_size: output_rom.prg().len(),
        chr_size: output_rom.chr().len(),
        fixed_source_entry_count: fixed.entries.len(),
        fixed_reinserted_entry_count,
        fixed_preserved_nonbattle_entry_count,
        installed_unit_name_count: fixed_installation.unit_name_count,
        installed_enemy_name_count: fixed_installation.enemy_name_count,
        installed_class_name_count: fixed_installation.class_name_count,
        installed_item_name_count: fixed_installation.item_name_count,
        installed_terrain_name_count: fixed_installation.terrain_name_count,
        installed_battle_message_template_count: fixed_installation.battle_message_template_count,
        installed_battle_forecast_label_count: 1,
        dialogue_record_count: dialogue.records.len(),
        dialogue_pointer_write_count,
        dialogue_translated_line_count: dialogue.translated_line_count,
        forecast_label_write_count: 1,
        text_tracked_write_count,
        observed_battle_sample_count: observed.constraints.len(),
        observed_runtime_tuple_count: observed.runtime_input_count,
        maximum_observed_overlay_count: observed.maximum_selected_overlay_count,
        stable_color_count: codebook.stable_color_count,
        borrowed_logical_code_count: codebook.borrowed_logical_code_count,
        abstract_assignment_sha1: codebook.abstract_assignment_sha1,
        canonical_assignment_sha1: codebook.canonical_assignment_sha1,
        canonical_code_table_byte_count: codebook.color_codes.len(),
        canonical_code_table_sha1: sha1_hex(&codebook.color_codes),
        canonical_code_table_cpu_address_hex: format!("0x{PHYSICAL_CODE_TABLE_CPU_ADDRESS:04X}"),
        protected_physical_code_count: codebook.protected_physical_code_count,
        protected_abstract_color_count: codebook.protected_abstract_colors.len(),
        protected_abstract_colors_sha1: sha1_hex(&codebook.protected_abstract_colors),
        protected_abstract_colors_cpu_address_hex: format!(
            "0x{PROTECTED_ABSTRACT_COLORS_CPU_ADDRESS:04X}"
        ),
        safe_abstract_color_count: codebook.safe_abstract_colors.len(),
        safe_abstract_colors_sha1: sha1_hex(&codebook.safe_abstract_colors),
        safe_abstract_colors_cpu_address_hex: format!("0x{SAFE_ABSTRACT_COLORS_CPU_ADDRESS:04X}"),
        color_bit_mask_byte_count: COLOR_BIT_MASKS.len(),
        color_bit_masks_sha1: sha1_hex(&COLOR_BIT_MASKS),
        color_bit_masks_cpu_address_hex: format!("0x{COLOR_BIT_MASKS_CPU_ADDRESS:04X}"),
        maximum_remap_pair_count: codebook.maximum_remap_pair_count,
        glyph_atlas_tile_count: material.atlas_glyphs.len(),
        glyph_atlas_byte_count: glyph_atlas.len(),
        glyph_atlas_sha1: sha1_hex(&glyph_atlas),
        glyph_atlas_mmc3_page: GLYPH_ATLAS_MMC3_PAGE,
        source_page_byte_count: source_page.len(),
        source_page_sha1: sha1_hex(source_page),
        source_page_mmc3_page: SOURCE_PAGE_MMC3_PAGE,
        recipe_blob_byte_count: material.recipe_blob.len(),
        recipe_blob_sha1: sha1_hex(&material.recipe_blob),
        recipe_blob_mmc3_page: RECIPE_BLOB_MMC3_PAGE,
        original_chr_preserved: true,
        original_english_and_digits_preserved: true,
        battle_catalog_fixed_text_reinserted: true,
        battle_dialogue_reinserted: true,
        forecast_label_reinserted: true,
        dynamic_assignment_source_contract_complete: true,
        translation_review_complete: false,
        runtime_loader_installed: false,
        translation_text_emitted: false,
        glyph_characters_emitted: false,
        runtime_verified: false,
        release_eligible: false,
        next_gate: "install dynamic assignment and project the shared text renderer through the resulting remap pairs",
    };
    let mut report_bytes =
        serde_json::to_vec_pretty(&report).context("serialize battle text runtime base report")?;
    report_bytes.push(b'\n');
    write_file(output_path, &output)?;
    write_file(report_path, &report_bytes)?;
    Ok(BattleTextRuntimeBaseSummary {
        output_sha1,
        report_sha1: sha1_hex(&report_bytes),
        fixed_entry_count: fixed_reinserted_entry_count,
        unit_name_count: fixed_installation.unit_name_count,
        enemy_name_count: fixed_installation.enemy_name_count,
        class_name_count: fixed_installation.class_name_count,
        item_name_count: fixed_installation.item_name_count,
        terrain_name_count: fixed_installation.terrain_name_count,
        battle_message_template_count: fixed_installation.battle_message_template_count,
        battle_forecast_label_count: 1,
        installed_item_source_indices: fixed_installation.installed_item_source_indices,
        dialogue_record_count: dialogue.records.len(),
        dialogue_translated_line_count: dialogue.translated_line_count,
        tracked_write_count: text_tracked_write_count,
    })
}

#[derive(Default)]
struct BattleFixedTextInstallation {
    unit_name_count: usize,
    enemy_name_count: usize,
    class_name_count: usize,
    item_name_count: usize,
    terrain_name_count: usize,
    battle_message_template_count: usize,
    installed_item_source_indices: BTreeSet<usize>,
}

impl BattleFixedTextInstallation {
    fn record(&mut self, table_id: &str, source_index: usize) -> Result<()> {
        match table_id {
            "unit-names" => self.unit_name_count += 1,
            "enemy-names" => self.enemy_name_count += 1,
            "class-names" => self.class_name_count += 1,
            "item-names" => {
                self.item_name_count += 1;
                ensure!(
                    self.installed_item_source_indices.insert(source_index),
                    "battle fixed-text installation repeats item source index {source_index}"
                );
            }
            "terrain-names" => self.terrain_name_count += 1,
            "battle-message-templates" => self.battle_message_template_count += 1,
            _ => anyhow::bail!("unknown installed battle fixed-text table {table_id}"),
        }
        Ok(())
    }

    fn total_count(&self) -> usize {
        self.unit_name_count
            + self.enemy_name_count
            + self.class_name_count
            + self.item_name_count
            + self.terrain_name_count
            + self.battle_message_template_count
    }
}

fn install_fixed_text(
    image: &mut TrackedImage,
    parity: &[u8],
    fixed: &crate::text_inventory::FixedTextPlan,
    material: &super::battle_codebook_plan::BattleCacheCompositionMaterial,
    assignments: &std::collections::BTreeMap<char, u8>,
) -> Result<BattleFixedTextInstallation> {
    let mut installation = BattleFixedTextInstallation::default();
    for entry in &fixed.entries {
        if !material.includes_fixed_entry(&entry.table_id, entry.source_index)? {
            continue;
        }
        let mut replacement = entry.encoded_bytes(assignments)?;
        ensure!(
            replacement.len() <= entry.source_storage_byte_count,
            "{} no longer fits its source storage",
            entry.id
        );
        replacement.push(0xEF);
        let expected = parity
            .get(entry.file_offset..entry.file_offset + replacement.len())
            .with_context(|| format!("{} source storage is outside mapper parity", entry.id))?;
        image.write_expected(
            format!("battle runtime fixed text {}", entry.id),
            entry.file_offset,
            expected,
            &replacement,
        )?;
        installation.record(&entry.table_id, entry.source_index)?;
    }
    Ok(installation)
}

fn install_battle_dialogue(
    image: &mut TrackedImage,
    parity: &[u8],
    dialogue: &crate::dialogue_assets::BattleDialogueReinsertionPlan,
    assignments: &std::collections::BTreeMap<char, u8>,
) -> Result<usize> {
    let records = dialogue.encoded_records(assignments)?;
    let mut pointer_write_count = 0;
    for record in &records {
        let expected = parity
            .get(record.planned_file_offset..record.planned_file_offset + record.bytes.len())
            .with_context(|| {
                format!(
                    "battle dialogue record {} target is outside mapper parity",
                    record.canonical_entry_index
                )
            })?;
        image.write_expected(
            format!(
                "battle runtime dialogue record {}",
                record.canonical_entry_index
            ),
            record.planned_file_offset,
            expected,
            &record.bytes,
        )?;
    }
    for record in &records {
        let replacement = record.planned_pointer_cpu_address.to_le_bytes();
        for pointer_file_offset in &record.pointer_file_offsets {
            let expected = parity
                .get(*pointer_file_offset..*pointer_file_offset + replacement.len())
                .context("battle dialogue pointer is outside mapper parity")?;
            image.write_expected(
                format!(
                    "battle runtime dialogue pointer {}",
                    record.canonical_entry_index
                ),
                *pointer_file_offset,
                expected,
                &replacement,
            )?;
            pointer_write_count += 1;
        }
    }
    Ok(pointer_write_count)
}

fn install_forecast_label(
    image: &mut TrackedImage,
    parity: &[u8],
    assignments: &std::collections::BTreeMap<char, u8>,
) -> Result<()> {
    let mut replacement = vec![0x22, 0x4E, 0x04];
    replacement.extend(
        FORECAST_LABEL_GLYPHS
            .iter()
            .map(|glyph| {
                assignments
                    .get(glyph)
                    .copied()
                    .with_context(|| format!("battle forecast label lost glyph {glyph:?}"))
            })
            .collect::<Result<Vec<_>>>()?,
    );
    replacement.push(0x00);
    ensure!(
        replacement.len() <= FORECAST_LABEL_SOURCE.len(),
        "battle forecast label exceeds its source storage"
    );
    ensure!(
        parity.get(FORECAST_LABEL_FILE_OFFSET..FORECAST_LABEL_FILE_OFFSET + replacement.len())
            == Some(&FORECAST_LABEL_SOURCE[..replacement.len()]),
        "battle forecast label source binding changed"
    );
    image.write_expected(
        "battle runtime forecast label",
        FORECAST_LABEL_FILE_OFFSET,
        &FORECAST_LABEL_SOURCE[..replacement.len()],
        &replacement,
    )
}

fn write_file(path: &Path, bytes: &[u8]) -> Result<()> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).with_context(|| format!("create {}", parent.display()))?;
    }
    fs::write(path, bytes).with_context(|| format!("write {}", path.display()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn report_omits_translation_content_and_private_paths() {
        let report = BattleTextRuntimeBaseReport {
            schema: 4,
            source_sha1: EXPECTED_SOURCE_SHA1,
            fixed_workspace_sha1: "fixed".to_owned(),
            dialogue_workspace_sha1: "dialogue".to_owned(),
            temporal_manifest_sha1: "temporal".to_owned(),
            output_sha1: "output".to_owned(),
            output_mapper: OUTPUT_MAPPER,
            prg_size: EXPANDED_PRG_SIZE,
            chr_size: 0,
            fixed_source_entry_count: 273,
            fixed_reinserted_entry_count: 232,
            fixed_preserved_nonbattle_entry_count: 41,
            installed_unit_name_count: 53,
            installed_enemy_name_count: 55,
            installed_class_name_count: 22,
            installed_item_name_count: 64,
            installed_terrain_name_count: 16,
            installed_battle_message_template_count: 22,
            installed_battle_forecast_label_count: 1,
            dialogue_record_count: 28,
            dialogue_pointer_write_count: 65,
            dialogue_translated_line_count: 70,
            forecast_label_write_count: 1,
            text_tracked_write_count: 366,
            observed_battle_sample_count: 32,
            observed_runtime_tuple_count: 5,
            maximum_observed_overlay_count: 88,
            stable_color_count: CANONICAL_ABSTRACT_COLOR_COUNT,
            borrowed_logical_code_count: 3,
            abstract_assignment_sha1: "abstract".to_owned(),
            canonical_assignment_sha1: "canonical".to_owned(),
            canonical_code_table_byte_count: CANONICAL_ABSTRACT_COLOR_COUNT,
            canonical_code_table_sha1: "table".to_owned(),
            canonical_code_table_cpu_address_hex: "0x9400".to_owned(),
            protected_physical_code_count: 39,
            protected_abstract_color_count: PROTECTED_ABSTRACT_COLOR_COUNT,
            protected_abstract_colors_sha1: "protected".to_owned(),
            protected_abstract_colors_cpu_address_hex: format!(
                "0x{PROTECTED_ABSTRACT_COLORS_CPU_ADDRESS:04X}"
            ),
            safe_abstract_color_count: SAFE_ABSTRACT_COLOR_COUNT,
            safe_abstract_colors_sha1: "safe".to_owned(),
            safe_abstract_colors_cpu_address_hex: format!(
                "0x{SAFE_ABSTRACT_COLORS_CPU_ADDRESS:04X}"
            ),
            color_bit_mask_byte_count: 8,
            color_bit_masks_sha1: "masks".to_owned(),
            color_bit_masks_cpu_address_hex: format!("0x{COLOR_BIT_MASKS_CPU_ADDRESS:04X}"),
            maximum_remap_pair_count: 8,
            glyph_atlas_tile_count: 296,
            glyph_atlas_byte_count: 4736,
            glyph_atlas_sha1: "atlas".to_owned(),
            glyph_atlas_mmc3_page: GLYPH_ATLAS_MMC3_PAGE,
            source_page_byte_count: FONT_PAGE_SIZE,
            source_page_sha1: "source-page".to_owned(),
            source_page_mmc3_page: SOURCE_PAGE_MMC3_PAGE,
            recipe_blob_byte_count: 3896,
            recipe_blob_sha1: "recipe".to_owned(),
            recipe_blob_mmc3_page: RECIPE_BLOB_MMC3_PAGE,
            original_chr_preserved: true,
            original_english_and_digits_preserved: true,
            battle_catalog_fixed_text_reinserted: true,
            battle_dialogue_reinserted: true,
            forecast_label_reinserted: true,
            dynamic_assignment_source_contract_complete: true,
            translation_review_complete: false,
            runtime_loader_installed: false,
            translation_text_emitted: false,
            glyph_characters_emitted: false,
            runtime_verified: false,
            release_eligible: false,
            next_gate: "runtime loader",
        };

        let json = serde_json::to_string(&report).unwrap();
        assert!(!json.contains("private/"));
        assert!(!json.contains('한'));
        assert!(!json.contains("korean"));
    }
}
