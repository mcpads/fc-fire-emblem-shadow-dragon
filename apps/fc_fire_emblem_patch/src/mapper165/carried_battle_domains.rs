//! Rebinds the cumulative battle translation to the exact integrated image.
//!
//! The four battle-only translation domains share one codebook, glyph atlas,
//! CHR-RAM compositor, and common text renderer.  Treating them as four
//! unrelated carried byte ranges would miss exactly the kind of cross-patch
//! conflict that the integrated build is meant to reject.  This inspector
//! therefore recomputes every translated payload, binds the shared material
//! once, and verifies the cumulative runtime after the two intentional global
//! integration replacements have been applied.

use std::{collections::BTreeSet, fs, path::Path};

use anyhow::{Context, Result, ensure};
use serde::{Deserialize, Serialize};

use crate::{
    battle_text_workset::{
        FORECAST_LABEL_FILE_OFFSET, FORECAST_LABEL_GLYPHS, FORECAST_LABEL_SOURCE,
    },
    dialogue_assets::plan_battle_dialogue_records,
    font_slots::FONT_PAGE_SIZE,
    mmc5_chr::switchable_bank_file_offset,
    rom::{EXPECTED_SOURCE_SHA1, HEADER_SIZE, Rom},
    sha1_hex,
    text_inventory::plan_fixed_text,
};

use super::{
    battle_codebook_plan::{
        bind_known_battle_text_consumer_topology, plan_battle_cache_composition_material,
        plan_canonical_battle_codebook,
    },
    battle_composition_loader_probe::{
        CUMULATIVE_RUNTIME_LAYOUT, cumulative_battle_central_right_fd_selector,
    },
    battle_text_cache_probe::{
        COLOR_BIT_MASKS, COLOR_BIT_MASKS_PRG_OFFSET, DYNAMIC_ASSIGNMENT_CODE_PRG_OFFSET,
        GLYPH_ATLAS_PRG_OFFSET, PHYSICAL_CODE_TABLE_PRG_OFFSET,
        PROTECTED_ABSTRACT_COLORS_PRG_OFFSET, RECIPE_BLOB_PRG_OFFSET,
        SAFE_ABSTRACT_COLORS_PRG_OFFSET, SOURCE_PAGE_PRG_OFFSET, rasterize_atlas,
    },
};

const EXPECTED_CUMULATIVE_REPORT_SCHEMA: u8 = 2;
const FIXED_BANK_BYTE_COUNT: usize = 0x4000;
const MATERIAL_RUNTIME_END_CPU_ADDRESS: u16 = 0x98A0;
const MATERIAL_RUNTIME_START_CPU_ADDRESS: u16 = 0x95C0;
const BATTLE_COMPOSITION_CALL_SITE: u16 = 0xFC49;

pub(crate) struct CarriedBattleDomainInputs<'a> {
    pub(crate) source: &'a Rom,
    pub(crate) cumulative: &'a Rom,
    pub(crate) integrated: &'a Rom,
    pub(crate) cumulative_report_path: &'a Path,
    pub(crate) fixed_workspace_path: &'a Path,
    pub(crate) dialogue_workspace_path: &'a Path,
    pub(crate) final_consumer_route: &'a FinalBattleConsumerRoute,
}

pub(crate) struct FinalBattleConsumerRoute {
    pub(crate) central_fallback_target: u16,
    pub(crate) composition_call_address: u16,
    pub(crate) composition_call_bytes: Vec<u8>,
    pub(crate) regions: Vec<FinalBattleConsumerRouteRegion>,
}

pub(crate) struct FinalBattleConsumerRouteRegion {
    pub(crate) role: &'static str,
    pub(crate) cpu_address: u16,
    pub(crate) bytes: Vec<u8>,
}

#[derive(Debug, Serialize)]
pub(crate) struct CarriedBattleDomainPreservation {
    strategy: &'static str,
    cumulative_candidate_sha1: String,
    cumulative_report_sha1: String,
    integrated_image_sha1: String,
    domain_count: usize,
    domains: Vec<CarriedBattleDomain>,
    shared_screen_roles: Vec<&'static str>,
    shared_font_regions: Vec<FinalRegionBinding>,
    shared_consumer_regions: Vec<FinalRegionBinding>,
    shared_consumer_route_binding_ids: Vec<&'static str>,
    all_translation_inputs_rebound: bool,
    all_storage_regions_rebound: bool,
    shared_font_supply_rebound: bool,
    shared_consumer_route_rebound: bool,
    human_review_complete: bool,
    complete: bool,
}

impl CarriedBattleDomainPreservation {
    pub(crate) fn complete(&self) -> bool {
        self.complete
    }

    pub(crate) fn human_review_complete(&self) -> bool {
        self.human_review_complete
    }
}

#[derive(Debug, Serialize)]
struct CarriedBattleDomain {
    id: &'static str,
    target_unit_count: usize,
    translation_input_bound: bool,
    review_complete: bool,
    storage_regions: Vec<FinalRegionBinding>,
    complete_for_declared_domain_plan: bool,
}

#[derive(Debug, Serialize)]
struct FinalRegionBinding {
    role: &'static str,
    binding_kind: &'static str,
    file_offset_hex: String,
    byte_count: usize,
    sha1: String,
    final_bytes_match_binding: bool,
}

#[derive(Debug, Deserialize)]
struct CumulativeReport {
    schema: u8,
    source_sha1: String,
    output_sha1: String,
    output_mapper: u16,
    prg_size: usize,
    chr_size: usize,
    battle_text: BattleTextReport,
}

#[derive(Debug, Deserialize)]
struct BattleTextReport {
    fixed_text_workspace_sha1: String,
    dialogue_workspace_sha1: String,
    temporal_manifest_sha1: String,
    runtime_base_report_sha1: String,
    loader_report_sha1: String,
    installed_fixed_entry_count: usize,
    installed_unit_name_count: usize,
    installed_enemy_name_count: usize,
    installed_class_name_count: usize,
    installed_item_name_count: usize,
    installed_terrain_name_count: usize,
    installed_battle_message_template_count: usize,
    installed_battle_forecast_label_count: usize,
    installed_dialogue_record_count: usize,
    installed_translated_line_count: usize,
    stable_color_count: usize,
    glyph_atlas_tile_count: usize,
    text_diff_range_count: usize,
    cumulative_selector_ranges_preserved: bool,
    original_english_digits_and_graphics_preserved: bool,
    review_complete: bool,
}

pub(crate) fn inspect_carried_battle_domains(
    inputs: CarriedBattleDomainInputs<'_>,
) -> Result<CarriedBattleDomainPreservation> {
    inputs.source.verify_supported_japanese()?;
    ensure!(
        inputs.cumulative.mapper() == 165
            && inputs.integrated.mapper() == 165
            && inputs.cumulative.prg().len() == inputs.integrated.prg().len()
            && inputs.cumulative.chr().len() <= inputs.integrated.chr().len(),
        "carried battle artifacts do not share the mapper-165 cumulative layout"
    );

    let report_bytes = fs::read(inputs.cumulative_report_path)
        .with_context(|| format!("read {}", inputs.cumulative_report_path.display()))?;
    let report: CumulativeReport = serde_json::from_slice(&report_bytes)
        .with_context(|| format!("parse {}", inputs.cumulative_report_path.display()))?;
    ensure!(
        report.schema == EXPECTED_CUMULATIVE_REPORT_SCHEMA
            && report.source_sha1 == EXPECTED_SOURCE_SHA1
            && report.output_sha1 == sha1_hex(inputs.cumulative.data())
            && report.output_mapper == inputs.cumulative.mapper()
            && report.prg_size == inputs.cumulative.prg().len()
            && report.chr_size == inputs.cumulative.chr().len(),
        "carried battle report does not describe the exact cumulative candidate"
    );

    let fixed = plan_fixed_text(inputs.source, inputs.fixed_workspace_path)?;
    let dialogue = plan_battle_dialogue_records(inputs.source, inputs.dialogue_workspace_path)?;
    let material = plan_battle_cache_composition_material(inputs.source, &fixed, &dialogue)?;
    let codebook = plan_canonical_battle_codebook(inputs.source, &fixed, &dialogue)?;
    bind_known_battle_text_consumer_topology(inputs.source)?;

    ensure!(
        report.battle_text.fixed_text_workspace_sha1 == fixed.workspace_sha1
            && report.battle_text.dialogue_workspace_sha1 == dialogue.workspace_sha1
            && is_sha1(&report.battle_text.temporal_manifest_sha1)
            && is_sha1(&report.battle_text.runtime_base_report_sha1)
            && is_sha1(&report.battle_text.loader_report_sha1)
            && report.battle_text.installed_fixed_entry_count
                == report.battle_text.installed_unit_name_count
                    + report.battle_text.installed_enemy_name_count
                    + report.battle_text.installed_class_name_count
                    + report.battle_text.installed_item_name_count
                    + report.battle_text.installed_terrain_name_count
                    + report.battle_text.installed_battle_message_template_count
            && report.battle_text.installed_terrain_name_count == 16
            && report.battle_text.installed_battle_message_template_count == 22
            && report.battle_text.installed_battle_forecast_label_count == 1
            && report.battle_text.installed_dialogue_record_count == dialogue.records.len()
            && report.battle_text.installed_translated_line_count == dialogue.translated_line_count
            && report.battle_text.stable_color_count == codebook.stable_color_count
            && report.battle_text.glyph_atlas_tile_count == material.atlas_glyphs.len()
            && report.battle_text.text_diff_range_count > 0
            && report.battle_text.cumulative_selector_ranges_preserved
            && report
                .battle_text
                .original_english_digits_and_graphics_preserved,
        "cumulative battle report no longer matches its source-bound inputs"
    );

    let terrain_storage = bind_fixed_storage(
        "terrain-names",
        "terrain_name_storage",
        16,
        &fixed,
        &material,
        &codebook.glyph_codes,
        inputs.cumulative,
        inputs.integrated,
    )?;
    let template_storage = bind_fixed_storage(
        "battle-message-templates",
        "battle_message_template_storage",
        22,
        &fixed,
        &material,
        &codebook.glyph_codes,
        inputs.cumulative,
        inputs.integrated,
    )?;
    let dialogue_storage = bind_dialogue_storage(
        &dialogue,
        &codebook.glyph_codes,
        inputs.cumulative,
        inputs.integrated,
    )?;
    let forecast_storage = vec![bind_forecast_storage(
        &codebook.glyph_codes,
        inputs.cumulative,
        inputs.integrated,
    )?];

    let shared_font_regions = bind_shared_font_material(
        inputs.source,
        inputs.cumulative,
        inputs.integrated,
        &material,
        &codebook,
    )?;
    let shared_consumer_regions = bind_shared_consumer_route(&inputs)?;
    let shared_consumer_route_binding_ids = vec![
        "04:800F:guard_final_battle_dialogue_cache",
        "04:BF40:refresh_battle_dialogue_cache",
        "05:85A5:read_battle_dialogue_override",
        "07:AC17:initialize_sound_test_battle_remap",
        "0F:C191:dispatch_battle_composition",
        "0F:E57F:project_shared_battle_text_code",
        "0F:FA80:select_battle_right_fd_page",
        "0F:FAA0:select_battle_right_fe_page",
        "0F:FC20:compose_shared_battle_page",
        "0F:FF1D:select_battle_or_integrated_fallback_page",
        "integrated:battle_composer_invalidates_dialogue_residency",
    ];

    let domains = vec![
        complete_domain(
            "battle_dialogue",
            dialogue.translated_line_count,
            report.battle_text.review_complete,
            dialogue_storage,
        ),
        complete_domain(
            "battle_forecast_label",
            1,
            report.battle_text.review_complete,
            forecast_storage,
        ),
        complete_domain(
            "battle_message_templates",
            22,
            fixed
                .entries
                .iter()
                .filter(|entry| entry.table_id == "battle-message-templates")
                .all(|entry| entry.review_complete),
            template_storage,
        ),
        complete_domain(
            "terrain_names",
            16,
            fixed
                .entries
                .iter()
                .filter(|entry| entry.table_id == "terrain-names")
                .all(|entry| entry.review_complete),
            terrain_storage,
        ),
    ];
    ensure!(
        domains
            .iter()
            .map(|domain| domain.id)
            .collect::<BTreeSet<_>>()
            .len()
            == domains.len()
            && domains
                .iter()
                .all(|domain| domain.complete_for_declared_domain_plan)
            && !shared_font_regions.is_empty()
            && !shared_consumer_regions.is_empty()
            && !shared_consumer_route_binding_ids.is_empty(),
        "carried battle domain preservation is incomplete"
    );

    let human_review_complete = domains.iter().all(|domain| domain.review_complete);
    Ok(CarriedBattleDomainPreservation {
        strategy: "recompute all four battle payloads, then bind their one shared codebook, font material, compositor, and renderer route on the exact integrated artifact",
        cumulative_candidate_sha1: report.output_sha1,
        cumulative_report_sha1: sha1_hex(&report_bytes),
        integrated_image_sha1: sha1_hex(inputs.integrated.data()),
        domain_count: domains.len(),
        domains,
        shared_screen_roles: vec!["battle_animation"],
        shared_font_regions,
        shared_consumer_regions,
        shared_consumer_route_binding_ids,
        all_translation_inputs_rebound: true,
        all_storage_regions_rebound: true,
        shared_font_supply_rebound: true,
        shared_consumer_route_rebound: true,
        human_review_complete,
        complete: true,
    })
}

fn bind_fixed_storage(
    table_id: &str,
    role: &'static str,
    expected_entry_count: usize,
    fixed: &crate::text_inventory::FixedTextPlan,
    material: &super::battle_codebook_plan::BattleCacheCompositionMaterial,
    assignments: &std::collections::BTreeMap<char, u8>,
    cumulative: &Rom,
    integrated: &Rom,
) -> Result<Vec<FinalRegionBinding>> {
    let mut regions = Vec::new();
    for entry in fixed
        .entries
        .iter()
        .filter(|entry| entry.table_id == table_id)
    {
        ensure!(
            material.includes_fixed_entry(&entry.table_id, entry.source_index)?,
            "carried battle material no longer includes {}",
            entry.id
        );
        let mut expected = entry.encoded_bytes(assignments)?;
        ensure!(
            expected.len() <= entry.source_storage_byte_count,
            "carried battle entry {} exceeds its source storage",
            entry.id
        );
        expected.push(0xEF);
        regions.push(bind_expected_region(
            role,
            entry.file_offset,
            &expected,
            cumulative,
            integrated,
        )?);
    }
    ensure!(
        regions.len() == expected_entry_count,
        "carried battle table {table_id} count changed"
    );
    Ok(regions)
}

fn bind_dialogue_storage(
    dialogue: &crate::dialogue_assets::BattleDialogueReinsertionPlan,
    assignments: &std::collections::BTreeMap<char, u8>,
    cumulative: &Rom,
    integrated: &Rom,
) -> Result<Vec<FinalRegionBinding>> {
    let records = dialogue.encoded_records(assignments)?;
    let mut regions = Vec::new();
    let mut pointer_offsets = BTreeSet::new();
    for record in &records {
        regions.push(bind_expected_region(
            "battle_dialogue_record_storage",
            record.planned_file_offset,
            &record.bytes,
            cumulative,
            integrated,
        )?);
        let pointer = record.planned_pointer_cpu_address.to_le_bytes();
        for offset in &record.pointer_file_offsets {
            ensure!(
                pointer_offsets.insert(*offset),
                "battle dialogue repeats pointer storage at {offset:06X}"
            );
            regions.push(bind_expected_region(
                "battle_dialogue_pointer_storage",
                *offset,
                &pointer,
                cumulative,
                integrated,
            )?);
        }
    }
    ensure!(
        records.len() == 28 && pointer_offsets.len() == 65,
        "battle dialogue storage topology changed"
    );
    Ok(regions)
}

fn bind_forecast_storage(
    assignments: &std::collections::BTreeMap<char, u8>,
    cumulative: &Rom,
    integrated: &Rom,
) -> Result<FinalRegionBinding> {
    let mut expected = vec![0x22, 0x4E, 0x04];
    expected.extend(
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
    expected.push(0);
    ensure!(
        expected.len() <= FORECAST_LABEL_SOURCE.len(),
        "battle forecast label exceeds its source storage"
    );
    bind_expected_region(
        "battle_forecast_label_storage",
        FORECAST_LABEL_FILE_OFFSET,
        &expected,
        cumulative,
        integrated,
    )
}

fn bind_shared_font_material(
    source: &Rom,
    cumulative: &Rom,
    integrated: &Rom,
    material: &super::battle_codebook_plan::BattleCacheCompositionMaterial,
    codebook: &super::battle_codebook_plan::CanonicalBattleCodebook,
) -> Result<Vec<FinalRegionBinding>> {
    let atlas = rasterize_atlas(&material.atlas_glyphs)?;
    let source_page = source
        .chr()
        .get(..FONT_PAGE_SIZE)
        .context("source battle font page is outside CHR")?;
    let specs: [(&str, usize, &[u8]); 7] = [
        ("battle_glyph_atlas", GLYPH_ATLAS_PRG_OFFSET, &atlas),
        (
            "battle_canonical_code_table",
            PHYSICAL_CODE_TABLE_PRG_OFFSET,
            &codebook.color_codes,
        ),
        (
            "battle_protected_abstract_colors",
            PROTECTED_ABSTRACT_COLORS_PRG_OFFSET,
            &codebook.protected_abstract_colors,
        ),
        (
            "battle_safe_abstract_colors",
            SAFE_ABSTRACT_COLORS_PRG_OFFSET,
            &codebook.safe_abstract_colors,
        ),
        (
            "battle_color_bit_masks",
            COLOR_BIT_MASKS_PRG_OFFSET,
            &COLOR_BIT_MASKS,
        ),
        (
            "battle_source_font_page",
            SOURCE_PAGE_PRG_OFFSET,
            source_page,
        ),
        (
            "battle_recipe_blob",
            RECIPE_BLOB_PRG_OFFSET,
            &material.recipe_blob,
        ),
    ];
    specs
        .into_iter()
        .map(|(role, prg_offset, expected)| {
            bind_expected_region(
                role,
                HEADER_SIZE + prg_offset,
                expected,
                cumulative,
                integrated,
            )
        })
        .collect()
}

fn bind_shared_consumer_route(
    inputs: &CarriedBattleDomainInputs<'_>,
) -> Result<Vec<FinalRegionBinding>> {
    let route = inputs.final_consumer_route;
    ensure!(
        route.composition_call_address == BATTLE_COMPOSITION_CALL_SITE
            && route.composition_call_bytes.len() == 3
            && route.composition_call_bytes[0] == 0x20,
        "integrated battle ownership call changed"
    );
    let fixed_start = CUMULATIVE_RUNTIME_LAYOUT.dispatch;
    let central_start = CUMULATIVE_RUNTIME_LAYOUT.battle_central_right_fd_selector;
    let fixed_end = CUMULATIVE_RUNTIME_LAYOUT.fixed_cave_end;
    ensure!(
        fixed_start < route.composition_call_address
            && route.composition_call_address + 3 < central_start
            && central_start < fixed_end,
        "battle runtime replacement addresses no longer partition the fixed runtime"
    );
    let central = cumulative_battle_central_right_fd_selector(route.central_fallback_target)?;
    let central_end = central_start
        .checked_add(u16::try_from(central.len())?)
        .context("integrated battle central selector address overflow")?;
    ensure!(
        central_end <= fixed_end,
        "integrated battle central selector exceeds its cave"
    );

    let mut regions = vec![
        bind_preserved_region(
            "battle_fixed_runtime_before_ownership_call",
            active_fixed_file_offset(inputs.cumulative, fixed_start)?,
            usize::from(route.composition_call_address - fixed_start),
            inputs.cumulative,
            inputs.integrated,
        )?,
        bind_final_expected_region(
            "battle_dialogue_residency_ownership_call",
            active_fixed_file_offset(inputs.integrated, route.composition_call_address)?,
            &route.composition_call_bytes,
            inputs.integrated,
        )?,
        bind_preserved_region(
            "battle_fixed_runtime_after_ownership_call",
            active_fixed_file_offset(inputs.cumulative, route.composition_call_address + 3)?,
            usize::from(central_start - (route.composition_call_address + 3)),
            inputs.cumulative,
            inputs.integrated,
        )?,
        bind_final_expected_region(
            "battle_integrated_fallback_selector",
            active_fixed_file_offset(inputs.integrated, central_start)?,
            &central,
            inputs.integrated,
        )?,
        bind_preserved_region(
            "battle_fixed_runtime_after_fallback_selector",
            active_fixed_file_offset(inputs.cumulative, central_end)?,
            usize::from(fixed_end - central_end),
            inputs.cumulative,
            inputs.integrated,
        )?,
        bind_preserved_region(
            "battle_dynamic_assignment_runtime",
            HEADER_SIZE + DYNAMIC_ASSIGNMENT_CODE_PRG_OFFSET,
            usize::from(MATERIAL_RUNTIME_END_CPU_ADDRESS - MATERIAL_RUNTIME_START_CPU_ADDRESS),
            inputs.cumulative,
            inputs.integrated,
        )?,
    ];
    for region in &route.regions {
        regions.push(bind_final_expected_region(
            region.role,
            active_fixed_file_offset(inputs.integrated, region.cpu_address)?,
            &region.bytes,
            inputs.integrated,
        )?);
    }

    for (role, address, byte_count) in [
        ("battle_nmi_dispatch_hook", 0xC191, 3),
        ("battle_nmi_ppu_restore_calls", 0xC185, 6),
        ("battle_ppu_scroll_restore", 0xC36A, 14),
        ("battle_ppu_control_mask_restore", 0xC733, 11),
        ("battle_central_selector_call", 0xC9C2, 3),
        ("battle_shared_text_projection_hook", 0xE57F, 4),
        ("battle_direct_right_fd_redirect", 0xFA80, 3),
        ("battle_right_fe_redirect", 0xFAA0, 3),
        ("battle_central_fe_refresh_call", 0xFABB, 3),
    ] {
        regions.push(bind_preserved_region(
            role,
            active_fixed_file_offset(inputs.cumulative, address)?,
            byte_count,
            inputs.cumulative,
            inputs.integrated,
        )?);
    }
    for (role, bank, address, byte_count) in [
        ("battle_dialogue_final_loader", 0x04, 0x8000, 29),
        ("battle_dialogue_cache_refresh", 0x04, 0xBF40, 0x40),
        ("battle_dialogue_override_reader", 0x05, 0x85A5, 9),
        ("sound_test_battle_remap_initializer", 0x07, 0xAC17, 3),
    ] {
        regions.push(bind_preserved_region(
            role,
            switchable_bank_file_offset(bank, address)?,
            byte_count,
            inputs.cumulative,
            inputs.integrated,
        )?);
    }
    Ok(regions)
}

fn complete_domain(
    id: &'static str,
    target_unit_count: usize,
    review_complete: bool,
    storage_regions: Vec<FinalRegionBinding>,
) -> CarriedBattleDomain {
    CarriedBattleDomain {
        id,
        target_unit_count,
        translation_input_bound: true,
        review_complete,
        complete_for_declared_domain_plan: target_unit_count > 0 && !storage_regions.is_empty(),
        storage_regions,
    }
}

fn bind_expected_region(
    role: &'static str,
    offset: usize,
    expected: &[u8],
    cumulative: &Rom,
    integrated: &Rom,
) -> Result<FinalRegionBinding> {
    ensure!(!expected.is_empty(), "{role} expected region is empty");
    let binding = bind_preserved_region(role, offset, expected.len(), cumulative, integrated)?;
    ensure!(
        integrated.data()[offset..offset + expected.len()] == *expected,
        "integrated {role} does not match its recomputed bytes"
    );
    Ok(binding)
}

fn bind_final_expected_region(
    role: &'static str,
    offset: usize,
    expected: &[u8],
    integrated: &Rom,
) -> Result<FinalRegionBinding> {
    ensure!(!expected.is_empty(), "{role} final region is empty");
    let actual = integrated
        .data()
        .get(offset..offset + expected.len())
        .with_context(|| format!("integrated {role} is outside the artifact"))?;
    ensure!(
        actual == expected,
        "integrated {role} does not match its generated final route"
    );
    Ok(FinalRegionBinding {
        role,
        binding_kind: "integrated_route_replacement",
        file_offset_hex: format!("0x{offset:06X}"),
        byte_count: expected.len(),
        sha1: sha1_hex(actual),
        final_bytes_match_binding: true,
    })
}

fn bind_preserved_region(
    role: &'static str,
    offset: usize,
    byte_count: usize,
    cumulative: &Rom,
    integrated: &Rom,
) -> Result<FinalRegionBinding> {
    ensure!(byte_count > 0, "{role} region is empty");
    let before = cumulative
        .data()
        .get(offset..offset + byte_count)
        .with_context(|| format!("cumulative {role} is outside the artifact"))?;
    let after = integrated
        .data()
        .get(offset..offset + byte_count)
        .with_context(|| format!("integrated {role} is outside the artifact"))?;
    ensure!(
        before == after,
        "integrated {role} changed after cumulative installation"
    );
    Ok(FinalRegionBinding {
        role,
        binding_kind: "cumulative_bytes_preserved",
        file_offset_hex: format!("0x{offset:06X}"),
        byte_count,
        sha1: sha1_hex(after),
        final_bytes_match_binding: true,
    })
}

fn active_fixed_file_offset(rom: &Rom, cpu_address: u16) -> Result<usize> {
    ensure!(
        rom.prg().len() >= FIXED_BANK_BYTE_COUNT && (0xC000..=0xFFFF).contains(&cpu_address),
        "active fixed-bank address is outside the mapper CPU window"
    );
    Ok(HEADER_SIZE + rom.prg().len() - FIXED_BANK_BYTE_COUNT + usize::from(cpu_address - 0xC000))
}

fn is_sha1(value: &str) -> bool {
    value.len() == 40 && value.bytes().all(|byte| byte.is_ascii_hexdigit())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn preserved_region_rejects_a_later_integration_mutation() {
        let mut bytes = crate::test_support::synthetic_mapper165_rom_bytes(0);
        let cumulative = Rom::parse(bytes.clone()).unwrap();
        bytes[HEADER_SIZE + 4] ^= 1;
        let integrated = Rom::parse(bytes).unwrap();

        assert!(
            bind_preserved_region("battle test", HEADER_SIZE, 8, &cumulative, &integrated,)
                .unwrap_err()
                .to_string()
                .contains("changed after cumulative installation")
        );
    }

    #[test]
    fn final_region_rejects_a_wrong_integrated_route() {
        let mut bytes = crate::test_support::synthetic_mapper165_rom_bytes(0);
        let offset = HEADER_SIZE + 8;
        bytes[offset..offset + 3].copy_from_slice(&[0x20, 0x00, 0xF0]);
        let integrated = Rom::parse(bytes).unwrap();

        assert!(
            bind_final_expected_region("battle route", offset, &[0x20, 0x10, 0xF0], &integrated,)
                .is_err()
        );
    }

    #[test]
    fn technical_domain_completion_requires_storage() {
        assert!(
            !complete_domain("terrain_names", 16, false, Vec::new())
                .complete_for_declared_domain_plan
        );
        assert!(
            complete_domain(
                "terrain_names",
                16,
                false,
                vec![FinalRegionBinding {
                    role: "terrain",
                    binding_kind: "cumulative_bytes_preserved",
                    file_offset_hex: "0x000010".to_owned(),
                    byte_count: 1,
                    sha1: "0".repeat(40),
                    final_bytes_match_binding: true,
                }],
            )
            .complete_for_declared_domain_plan
        );
    }
}
