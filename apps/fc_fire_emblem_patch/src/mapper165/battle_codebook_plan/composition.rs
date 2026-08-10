use std::collections::{BTreeMap, BTreeSet};

use anyhow::{Context, Result, ensure};
use serde::Serialize;

use crate::{
    battle_text_workset::FORECAST_LABEL_GLYPHS,
    dialogue_assets::BattleDialogueReinsertionPlan,
    font_slots::{FONT_PAGE_SIZE, FONT_TILE_SIZE},
    text_inventory::FixedTextPlan,
};

use super::conflict_graph::StableColoringPlan;

mod blob;

use blob::{RecipeCatalog, RecipePair, RecipeRole, select_runtime_recipes};

const SOURCE_PRG_BYTE_COUNT: usize = 256 * 1024;
const EXPANDED_PRG_BYTE_COUNT: usize = 512 * 1024;
const ACTIVE_FIXED_BANK_BYTE_COUNT: usize = 16 * 1024;
const MMC3_PRG_PAGE_BYTE_COUNT: usize = 8 * 1024;

#[derive(Debug, Serialize)]
pub(super) struct BattleCacheCompositionPlan {
    strategy: &'static str,
    common_recipe_count: usize,
    unit_name_recipe_count: usize,
    enemy_name_recipe_count: usize,
    class_recipe_count: usize,
    item_recipe_count: usize,
    terrain_recipe_count: usize,
    dialogue_recipe_count: usize,
    dialogue_selector_count: usize,
    total_recipe_count: usize,
    total_recipe_glyph_reference_count: usize,
    maximum_recipe_glyph_count: usize,
    maximum_runtime_recipe_count: usize,
    abstract_recipe_sha1: String,
    recipe_blob_byte_count: usize,
    unique_recipe_payload_count: usize,
    missing_directory_entry_count: usize,
    glyph_atlas_tile_count: usize,
    glyph_atlas_byte_count: usize,
    source_page_copy_byte_count: usize,
    maximum_overlay_glyph_count: usize,
    maximum_overlay_byte_count: usize,
    maximum_rebuild_byte_count: usize,
    modeled_participant_pair_count: u64,
    modeled_terrain_pair_count: u64,
    modeled_dialogue_choice_count: u64,
    modeled_tuple_count: u64,
    maximum_full_pages_after_base_material: usize,
    one_page_per_modeled_tuple_viable: bool,
    modeled_tuple_count_is_reachability_proof: bool,
    recipes_use_abstract_colors: bool,
    recipe_catalog_covers_modeled_text_inputs: bool,
    graphics_protection_catalog_complete: bool,
    physical_assignment_complete: bool,
    runtime_loader_implemented: bool,
    runtime_verified: bool,
}

pub(in crate::mapper165) struct BattleCacheCompositionMaterial {
    pub(super) plan: BattleCacheCompositionPlan,
    pub(in crate::mapper165) atlas_glyphs: Vec<char>,
    pub(in crate::mapper165) recipe_blob: Vec<u8>,
}

#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub(in crate::mapper165) struct BattleRuntimeRecipeInput {
    pub(in crate::mapper165) participant_record_identities: [u8; 2],
    pub(in crate::mapper165) class_record_identities: [u8; 2],
    pub(in crate::mapper165) item_source_indices: [u8; 2],
    pub(in crate::mapper165) terrain_source_indices: [u8; 2],
    pub(in crate::mapper165) dialogue_selector: u8,
}

#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub(in crate::mapper165) struct BattleRuntimeGlyphOverlay {
    pub(in crate::mapper165) abstract_color: u8,
    pub(in crate::mapper165) atlas_index: u16,
}

pub(in crate::mapper165) struct BattleRuntimeRecipeSelection {
    pub(in crate::mapper165) recipe_offsets: Vec<u16>,
    pub(in crate::mapper165) overlays: Vec<BattleRuntimeGlyphOverlay>,
    pub(in crate::mapper165) glyphs: BTreeSet<char>,
}

impl BattleCacheCompositionMaterial {
    pub(in crate::mapper165) fn select_runtime_recipes(
        &self,
        input: BattleRuntimeRecipeInput,
    ) -> Result<BattleRuntimeRecipeSelection> {
        let encoded = select_runtime_recipes(&self.recipe_blob, input)?;
        let glyphs = encoded
            .overlays
            .iter()
            .map(|pair| {
                self.atlas_glyphs
                    .get(usize::from(pair.atlas_index))
                    .copied()
                    .context("battle runtime recipe references a missing atlas glyph")
            })
            .collect::<Result<BTreeSet<_>>>()?;
        ensure!(
            glyphs.len() == encoded.overlays.len(),
            "battle runtime recipe selects two atlas glyphs for one abstract color"
        );
        Ok(BattleRuntimeRecipeSelection {
            recipe_offsets: encoded.recipe_offsets,
            overlays: encoded
                .overlays
                .into_iter()
                .map(|pair| BattleRuntimeGlyphOverlay {
                    abstract_color: pair.color,
                    atlas_index: pair.atlas_index,
                })
                .collect(),
            glyphs,
        })
    }
}

pub(super) fn plan_cache_composition(
    fixed: &FixedTextPlan,
    dialogue: &BattleDialogueReinsertionPlan,
    coloring: &StableColoringPlan,
    candidate_item_source_indices: &BTreeSet<usize>,
    enemy_name_source_indices: &BTreeSet<usize>,
    player_participant_candidate_count: usize,
    enemy_participant_candidate_count: usize,
    terrain_entry_count: usize,
) -> Result<BattleCacheCompositionMaterial> {
    ensure!(
        coloring.color_count <= u8::MAX.into(),
        "battle abstract color exceeds one-byte recipe encoding"
    );
    let atlas_indices = coloring
        .glyph_colors()
        .keys()
        .copied()
        .enumerate()
        .map(|(index, glyph)| {
            Ok((
                glyph,
                u16::try_from(index).context("battle atlas index exceeds recipe encoding")?,
            ))
        })
        .collect::<Result<BTreeMap<_, _>>>()?;
    let mut recipes = RecipeCatalog::default();

    let mut common_glyphs = fixed
        .entries
        .iter()
        .filter(|entry| entry.table_id == "battle-message-templates")
        .flat_map(|entry| entry.unique_glyphs())
        .collect::<BTreeSet<_>>();
    common_glyphs.extend(FORECAST_LABEL_GLYPHS);
    recipes.add(
        RecipeRole::Common,
        0,
        recipe_pairs(
            &common_glyphs,
            coloring,
            &atlas_indices,
            RecipeRole::Common,
            0,
        )?,
    )?;

    add_fixed_range(
        &mut recipes,
        RecipeRole::UnitName,
        fixed,
        "unit-names",
        0..52,
        coloring,
        &atlas_indices,
    )?;
    add_fixed_indices(
        &mut recipes,
        RecipeRole::EnemyName,
        fixed,
        "enemy-names",
        enemy_name_source_indices.iter().copied(),
        coloring,
        &atlas_indices,
    )?;
    add_fixed_range(
        &mut recipes,
        RecipeRole::Class,
        fixed,
        "class-names",
        0..24,
        coloring,
        &atlas_indices,
    )?;
    for source_index in candidate_item_source_indices {
        let entry = fixed
            .entry_for_source_index("item-names", *source_index)
            .with_context(|| format!("missing battle item recipe source {source_index}"))?;
        recipes.add(
            RecipeRole::Item,
            *source_index,
            recipe_pairs(
                &entry.unique_glyphs(),
                coloring,
                &atlas_indices,
                RecipeRole::Item,
                *source_index,
            )?,
        )?;
    }
    add_fixed_range(
        &mut recipes,
        RecipeRole::Terrain,
        fixed,
        "terrain-names",
        0..terrain_entry_count,
        coloring,
        &atlas_indices,
    )?;

    let mut dialogue_selector_count = 0;
    for record in &dialogue.records {
        recipes.add(
            RecipeRole::Dialogue,
            record.canonical_entry_index,
            recipe_pairs(
                &record.unique_glyphs(),
                coloring,
                &atlas_indices,
                RecipeRole::Dialogue,
                record.canonical_entry_index,
            )?,
        )?;
        for selector in &record.entry_indices {
            recipes.add_dialogue_alias(*selector, record.canonical_entry_index)?;
            dialogue_selector_count += 1;
        }
    }
    ensure!(
        dialogue_selector_count == 65,
        "battle composition recipe directory lost dialogue selectors"
    );
    let encoded_recipes = recipes.encode(coloring.color_count, coloring.glyph_count)?;

    let modeled_participant_pair_count = u64::try_from(player_participant_candidate_count)?
        .checked_mul(u64::try_from(enemy_participant_candidate_count)?)
        .context("battle participant tuple count overflow")?;
    let modeled_terrain_pair_count = u64::try_from(terrain_entry_count)?
        .checked_pow(2)
        .context("battle terrain tuple count overflow")?;
    let modeled_dialogue_choice_count = u64::try_from(dialogue.records.len())?;
    let modeled_tuple_count = modeled_participant_pair_count
        .checked_mul(modeled_terrain_pair_count)
        .and_then(|count| count.checked_mul(modeled_dialogue_choice_count))
        .context("battle modeled tuple count overflow")?;
    let glyph_atlas_byte_count = coloring
        .glyph_count
        .checked_mul(FONT_TILE_SIZE)
        .context("battle glyph atlas size overflow")?;
    let maximum_overlay_byte_count = coloring
        .color_count
        .checked_mul(FONT_TILE_SIZE)
        .context("battle overlay size overflow")?;
    let maximum_rebuild_byte_count = FONT_PAGE_SIZE
        .checked_add(maximum_overlay_byte_count)
        .context("battle rebuild size overflow")?;
    let expanded_material_capacity = EXPANDED_PRG_BYTE_COUNT
        .checked_sub(SOURCE_PRG_BYTE_COUNT + ACTIVE_FIXED_BANK_BYTE_COUNT)
        .context("expanded battle material capacity underflow")?;
    ensure!(
        glyph_atlas_byte_count <= MMC3_PRG_PAGE_BYTE_COUNT,
        "battle glyph atlas exceeds one material PRG page"
    );
    ensure!(
        FONT_PAGE_SIZE + encoded_recipes.bytes.len() <= MMC3_PRG_PAGE_BYTE_COUNT,
        "battle source page and recipes exceed one material PRG page"
    );
    let maximum_full_pages_after_base_material = expanded_material_capacity
        .checked_sub(2 * MMC3_PRG_PAGE_BYTE_COUNT)
        .context("battle base material exceeds expanded PRG capacity")?
        / FONT_PAGE_SIZE;

    let plan = BattleCacheCompositionPlan {
        strategy: "rebuild one CHR-RAM page from the original source page and selected glyph recipes at battle entry",
        common_recipe_count: 1,
        unit_name_recipe_count: 52,
        enemy_name_recipe_count: enemy_name_source_indices.len(),
        class_recipe_count: 24,
        item_recipe_count: candidate_item_source_indices.len(),
        terrain_recipe_count: terrain_entry_count,
        dialogue_recipe_count: dialogue.records.len(),
        dialogue_selector_count,
        total_recipe_count: encoded_recipes.logical_recipe_count,
        total_recipe_glyph_reference_count: encoded_recipes.glyph_reference_count,
        maximum_recipe_glyph_count: encoded_recipes.maximum_glyph_count,
        maximum_runtime_recipe_count: 10,
        abstract_recipe_sha1: encoded_recipes.sha1.clone(),
        recipe_blob_byte_count: encoded_recipes.bytes.len(),
        unique_recipe_payload_count: encoded_recipes.unique_payload_count,
        missing_directory_entry_count: encoded_recipes.missing_directory_entry_count,
        glyph_atlas_tile_count: coloring.glyph_count,
        glyph_atlas_byte_count,
        source_page_copy_byte_count: FONT_PAGE_SIZE,
        maximum_overlay_glyph_count: coloring.color_count,
        maximum_overlay_byte_count,
        maximum_rebuild_byte_count,
        modeled_participant_pair_count,
        modeled_terrain_pair_count,
        modeled_dialogue_choice_count,
        modeled_tuple_count,
        maximum_full_pages_after_base_material,
        one_page_per_modeled_tuple_viable: modeled_tuple_count
            <= u64::try_from(maximum_full_pages_after_base_material)?,
        modeled_tuple_count_is_reachability_proof: false,
        recipes_use_abstract_colors: true,
        recipe_catalog_covers_modeled_text_inputs: true,
        graphics_protection_catalog_complete: false,
        physical_assignment_complete: false,
        runtime_loader_implemented: false,
        runtime_verified: false,
    };
    Ok(BattleCacheCompositionMaterial {
        plan,
        atlas_glyphs: coloring.glyph_colors().keys().copied().collect(),
        recipe_blob: encoded_recipes.bytes,
    })
}

fn add_fixed_range(
    recipes: &mut RecipeCatalog,
    role: RecipeRole,
    fixed: &FixedTextPlan,
    table_id: &str,
    source_indices: impl IntoIterator<Item = usize>,
    coloring: &StableColoringPlan,
    atlas_indices: &BTreeMap<char, u16>,
) -> Result<()> {
    add_fixed_indices(
        recipes,
        role,
        fixed,
        table_id,
        source_indices,
        coloring,
        atlas_indices,
    )
}

fn add_fixed_indices(
    recipes: &mut RecipeCatalog,
    role: RecipeRole,
    fixed: &FixedTextPlan,
    table_id: &str,
    source_indices: impl IntoIterator<Item = usize>,
    coloring: &StableColoringPlan,
    atlas_indices: &BTreeMap<char, u16>,
) -> Result<()> {
    for source_index in source_indices {
        let entry = fixed
            .entry_for_source_index(table_id, source_index)
            .with_context(|| format!("missing {table_id} recipe source {source_index}"))?;
        recipes.add(
            role,
            source_index,
            recipe_pairs(
                &entry.unique_glyphs(),
                coloring,
                atlas_indices,
                role,
                source_index,
            )?,
        )?;
    }
    Ok(())
}

fn recipe_pairs(
    glyphs: &BTreeSet<char>,
    coloring: &StableColoringPlan,
    atlas_indices: &BTreeMap<char, u16>,
    role: RecipeRole,
    source_index: usize,
) -> Result<Vec<RecipePair>> {
    glyphs
        .iter()
        .map(|glyph| {
            Ok(RecipePair {
                color: u8::try_from(
                    coloring
                        .glyph_colors()
                        .get(glyph)
                        .copied()
                        .with_context(|| {
                            format!(
                                "battle recipe role {} source {source_index} contains an uncolored glyph {glyph:?}",
                                role as u8
                            )
                        })?,
                )
                .context("battle recipe color exceeds encoding")?,
                atlas_index: atlas_indices
                    .get(glyph)
                    .copied()
                    .context("battle recipe glyph is absent from the atlas")?,
            })
        })
        .collect()
}

#[cfg(test)]
pub(super) fn test_plan() -> BattleCacheCompositionPlan {
    BattleCacheCompositionPlan {
        strategy: "runtime composition",
        common_recipe_count: 1,
        unit_name_recipe_count: 1,
        enemy_name_recipe_count: 1,
        class_recipe_count: 1,
        item_recipe_count: 1,
        terrain_recipe_count: 1,
        dialogue_recipe_count: 1,
        dialogue_selector_count: 1,
        total_recipe_count: 7,
        total_recipe_glyph_reference_count: 7,
        maximum_recipe_glyph_count: 1,
        maximum_runtime_recipe_count: 7,
        abstract_recipe_sha1: "recipe".to_owned(),
        recipe_blob_byte_count: 1,
        unique_recipe_payload_count: 1,
        missing_directory_entry_count: 0,
        glyph_atlas_tile_count: 1,
        glyph_atlas_byte_count: FONT_TILE_SIZE,
        source_page_copy_byte_count: FONT_PAGE_SIZE,
        maximum_overlay_glyph_count: 1,
        maximum_overlay_byte_count: FONT_TILE_SIZE,
        maximum_rebuild_byte_count: FONT_PAGE_SIZE + FONT_TILE_SIZE,
        modeled_participant_pair_count: 1,
        modeled_terrain_pair_count: 1,
        modeled_dialogue_choice_count: 1,
        modeled_tuple_count: 1,
        maximum_full_pages_after_base_material: 1,
        one_page_per_modeled_tuple_viable: true,
        modeled_tuple_count_is_reachability_proof: false,
        recipes_use_abstract_colors: true,
        recipe_catalog_covers_modeled_text_inputs: true,
        graphics_protection_catalog_complete: false,
        physical_assignment_complete: false,
        runtime_loader_implemented: false,
        runtime_verified: false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::mapper165::battle_codebook_plan::conflict_graph::{
        BattleGlyphFamilies, plan_stable_coloring,
    };

    #[test]
    fn recipe_catalog_uses_colors_and_atlas_indices_without_glyph_bytes() {
        let coloring = plan_stable_coloring(
            &BattleGlyphFamilies {
                base: "가나".chars().collect(),
                player_participants: vec![],
                enemy_participants: vec![],
                terrains: vec![],
                dialogue_records: vec![],
            },
            2,
        )
        .unwrap();
        let atlas_indices = coloring
            .glyph_colors()
            .keys()
            .copied()
            .enumerate()
            .map(|(index, glyph)| (glyph, u16::try_from(index).unwrap()))
            .collect();
        let mut catalog = RecipeCatalog::default();

        catalog
            .add(
                RecipeRole::Common,
                0,
                recipe_pairs(
                    &"가나".chars().collect(),
                    &coloring,
                    &atlas_indices,
                    RecipeRole::Common,
                    0,
                )
                .unwrap(),
            )
            .unwrap();

        for selector in 0..65 {
            catalog.add_dialogue_alias(selector, 0).unwrap();
        }
        catalog.add(RecipeRole::Dialogue, 0, Vec::new()).unwrap();
        let encoded = catalog.encode(2, 2).unwrap();

        assert_eq!(encoded.logical_recipe_count, 2);
        assert_eq!(encoded.glyph_reference_count, 2);
        assert!(
            !encoded
                .bytes
                .windows(3)
                .any(|bytes| bytes == "가".as_bytes())
        );
    }
}
