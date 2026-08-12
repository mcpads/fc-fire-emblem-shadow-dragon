use std::collections::{BTreeMap, BTreeSet};

use anyhow::{Context, Result, ensure};

use crate::{
    dialogue_assets::MainDialogueBundlePlan,
    font::{load_dalmoori, rasterize_glyph},
    font_slots::{FONT_PAGE_SIZE, FONT_TILE_SIZE},
    mapper165::battle_codebook_plan::GlyphWorksetPagePlan,
};

pub(super) struct DialogueRuntimeCompositionPlan {
    pub(super) glyph_atlas: Vec<u8>,
    pub(super) glyph_atlas_tile_count: usize,
    pub(super) four_by_four_block_count: usize,
    pub(super) four_by_four_block_index_bit_count: usize,
    pub(super) four_by_four_block_atlas_byte_count: usize,
    pub(super) static_page_group_count: usize,
    pub(super) static_page_group_overlay_reference_count: usize,
    pub(super) maximum_static_page_group_overlay_tile_count: usize,
    pub(super) visible_page_recipe_count: usize,
    pub(super) visible_page_recipe_reference_count: usize,
    pub(super) visible_page_overlay_reference_count: usize,
    pub(super) maximum_visible_page_overlay_tile_count: usize,
    pub(super) sequential_page_transition_count: usize,
    pub(super) distinct_visible_page_recipe_transition_count: usize,
    pub(super) unchanged_visible_page_recipe_transition_count: usize,
    pub(super) maximum_delta_tile_count: usize,
    pub(super) maximum_delta_ppu_write_count: usize,
    pub(super) total_delta_ppu_write_count: usize,
    pub(super) rebuild_every_visible_page_ppu_write_count: usize,
    pub(super) initial_rebuild_then_delta_ppu_write_count: usize,
    pub(super) direct_visible_page_recipe_byte_count: usize,
    pub(super) bitpacked_visible_page_recipe_byte_count: usize,
    pub(super) bitmap_and_atlas_index_visible_page_recipe_byte_count: usize,
    pub(super) direct_delta_recipe_byte_count: usize,
    pub(super) bitpacked_delta_recipe_byte_count: usize,
    pub(super) dense_group_lookup_byte_count: usize,
    pub(super) record_page_group_selector_byte_count: usize,
    pub(super) record_selector_directory_byte_count: usize,
    pub(super) scan_material_byte_count: usize,
    pub(super) dynamic_string_control_count: usize,
    pub(super) dynamic_string_page_count: usize,
    pub(super) dynamic_string_selector_count: usize,
}

#[derive(Clone, Eq, Ord, PartialEq, PartialOrd)]
struct VisiblePageRecipe {
    static_page_group_index: usize,
    target_glyphs: Vec<char>,
}

pub(super) fn plan_dialogue_runtime_composition(
    dialogue: &MainDialogueBundlePlan,
    codebook: &GlyphWorksetPagePlan,
    source_font_page: &[u8],
    static_page_pack: &[u8],
) -> Result<DialogueRuntimeCompositionPlan> {
    ensure!(
        source_font_page.len() == FONT_PAGE_SIZE,
        "dialogue runtime composition source page length changed"
    );
    ensure!(
        static_page_pack.len() == codebook.page_assignments.len() * FONT_PAGE_SIZE,
        "dialogue runtime composition static page pack length changed"
    );
    let dialogue_glyphs = codebook
        .page_assignments
        .iter()
        .flat_map(|assignments| assignments.keys().copied())
        .collect::<BTreeSet<_>>()
        .into_iter()
        .collect::<Vec<_>>();
    ensure!(
        dialogue_glyphs.len() == codebook.glyph_count,
        "dialogue runtime composition lost glyph-atlas entries"
    );
    ensure!(
        dialogue_glyphs.len() <= 3 * 256,
        "dialogue glyph atlas needs more than three dense lookup classes"
    );
    ensure!(
        codebook.page_assignments.len() <= usize::from(u8::MAX) + 1,
        "dialogue static-page groups do not fit one-byte selectors"
    );
    let font = load_dalmoori()?;
    let mut glyph_atlas = Vec::with_capacity(dialogue_glyphs.len() * 8);
    for glyph in &dialogue_glyphs {
        let tile = rasterize_glyph(&font, *glyph)?;
        ensure!(
            tile[8..].iter().all(|byte| *byte == 0),
            "dialogue glyph atlas needs a nonzero high bitplane"
        );
        glyph_atlas.extend_from_slice(&tile[..8]);
    }
    let quadrant_compression = measure_four_by_four_block_atlas(&glyph_atlas)?;

    let static_page_group_overlay_reference_count = codebook
        .page_assignments
        .iter()
        .map(BTreeMap::len)
        .sum::<usize>();
    let maximum_static_page_group_overlay_tile_count = codebook
        .page_assignments
        .iter()
        .map(BTreeMap::len)
        .max()
        .unwrap_or(0);

    let workset_recipes = dialogue
        .page_worksets
        .iter()
        .enumerate()
        .map(|(workset_index, workset)| {
            let static_page_group_index = codebook.workset_page_indices[workset_index];
            let group_assignments = codebook
                .page_assignments
                .get(static_page_group_index)
                .context("visible dialogue page refers to a missing static page group")?;
            ensure!(
                workset
                    .target_glyphs
                    .iter()
                    .all(|glyph| group_assignments.contains_key(glyph)),
                "visible dialogue page lost a page-local glyph assignment"
            );
            Ok(VisiblePageRecipe {
                static_page_group_index,
                target_glyphs: workset.target_glyphs.iter().copied().collect(),
            })
        })
        .collect::<Result<Vec<_>>>()?;
    let unique_recipes = workset_recipes.iter().cloned().collect::<BTreeSet<_>>();
    let recipe_indices = unique_recipes
        .iter()
        .cloned()
        .enumerate()
        .map(|(index, recipe)| (recipe, index))
        .collect::<BTreeMap<_, _>>();
    let workset_recipe_indices = workset_recipes
        .iter()
        .map(|recipe| recipe_indices[recipe])
        .collect::<Vec<_>>();
    let recipe_pages = unique_recipes
        .iter()
        .map(|recipe| {
            compose_visible_page(
                source_font_page,
                static_page_pack,
                &codebook.page_assignments,
                recipe,
            )
        })
        .collect::<Result<Vec<_>>>()?;
    let recipe_target_codes = unique_recipes
        .iter()
        .map(|recipe| target_codes(&codebook.page_assignments, recipe))
        .collect::<Result<Vec<_>>>()?;

    let visible_page_recipe_reference_count = unique_recipes
        .iter()
        .map(|recipe| recipe.target_glyphs.len())
        .sum::<usize>();
    let visible_page_overlay_reference_count = workset_recipes
        .iter()
        .map(|recipe| recipe.target_glyphs.len())
        .sum::<usize>();
    let maximum_visible_page_overlay_tile_count = workset_recipes
        .iter()
        .map(|recipe| recipe.target_glyphs.len())
        .max()
        .unwrap_or(0);

    let record_worksets = record_workset_indices(dialogue)?;
    let mut sequential_page_transition_count = 0;
    let mut unchanged_visible_page_recipe_transition_count = 0;
    let mut distinct_recipe_transitions = BTreeSet::new();
    let mut maximum_delta_tile_count = 0;
    let mut total_delta_ppu_write_count = 0;
    let mut maximum_delta_ppu_write_count = 0;
    let mut initial_overlay_reference_count = 0;
    for workset_indices in record_worksets.values() {
        if let Some(first_workset_index) = workset_indices.first() {
            initial_overlay_reference_count +=
                workset_recipes[*first_workset_index].target_glyphs.len();
        }
        for pair in workset_indices.windows(2) {
            sequential_page_transition_count += 1;
            let from_recipe = workset_recipe_indices[pair[0]];
            let to_recipe = workset_recipe_indices[pair[1]];
            if from_recipe == to_recipe {
                unchanged_visible_page_recipe_transition_count += 1;
                continue;
            }
            distinct_recipe_transitions.insert((from_recipe, to_recipe));
            let changed_tiles =
                changed_tile_count(&recipe_pages[from_recipe], &recipe_pages[to_recipe])?;
            let ppu_writes = delta_ppu_write_count(
                &recipe_pages[from_recipe],
                &recipe_pages[to_recipe],
                &recipe_target_codes[from_recipe],
                &recipe_target_codes[to_recipe],
            )?;
            maximum_delta_tile_count = maximum_delta_tile_count.max(changed_tiles);
            total_delta_ppu_write_count += ppu_writes;
            maximum_delta_ppu_write_count = maximum_delta_ppu_write_count.max(ppu_writes);
        }
    }
    let distinct_delta_reference_count = distinct_recipe_transitions
        .iter()
        .map(|(from_recipe, to_recipe)| {
            changed_tile_count(&recipe_pages[*from_recipe], &recipe_pages[*to_recipe])
        })
        .collect::<Result<Vec<_>>>()?
        .into_iter()
        .sum::<usize>();

    let rebuild_every_visible_page_ppu_write_count = dialogue.page_worksets.len() * FONT_PAGE_SIZE
        + visible_page_overlay_reference_count * FONT_TILE_SIZE;
    let initial_rebuild_then_delta_ppu_write_count = record_worksets.len() * FONT_PAGE_SIZE
        + initial_overlay_reference_count * FONT_TILE_SIZE
        + total_delta_ppu_write_count;
    let visible_recipe_directory_byte_count = (unique_recipes.len() + 1) * 2;
    let direct_visible_page_recipe_byte_count =
        visible_page_recipe_reference_count * 3 + visible_recipe_directory_byte_count;
    let bitpacked_visible_page_recipe_byte_count = (visible_page_recipe_reference_count * 18)
        .div_ceil(8)
        + visible_recipe_directory_byte_count;
    let bitmap_and_atlas_index_visible_page_recipe_byte_count = unique_recipes.len() * 32
        + (visible_page_recipe_reference_count * 10).div_ceil(8)
        + visible_recipe_directory_byte_count;
    let delta_recipe_directory_byte_count = (distinct_recipe_transitions.len() + 1) * 2;
    let direct_delta_recipe_byte_count =
        distinct_delta_reference_count * 3 + delta_recipe_directory_byte_count;
    let bitpacked_delta_recipe_byte_count =
        (distinct_delta_reference_count * 18).div_ceil(8) + delta_recipe_directory_byte_count;
    let dense_group_lookup_byte_count = codebook.page_assignments.len() * (256 + 64);
    let record_page_group_selector_byte_count = dialogue.page_worksets.len();
    let record_selector_directory_byte_count = (record_worksets.len() + 1) * 2;
    let scan_material_byte_count = dense_group_lookup_byte_count
        + record_page_group_selector_byte_count
        + record_selector_directory_byte_count;
    let dynamic_string_control_count = dialogue
        .page_worksets
        .iter()
        .map(|workset| workset.dynamic_string_control_count)
        .sum();
    let dynamic_string_page_count = dialogue
        .page_worksets
        .iter()
        .filter(|workset| workset.dynamic_string_control_count != 0)
        .count();
    let dynamic_string_selector_count = dialogue
        .page_worksets
        .iter()
        .flat_map(|workset| workset.dynamic_string_selectors.iter().copied())
        .collect::<BTreeSet<_>>()
        .len();

    Ok(DialogueRuntimeCompositionPlan {
        glyph_atlas,
        glyph_atlas_tile_count: dialogue_glyphs.len(),
        four_by_four_block_count: quadrant_compression.block_count,
        four_by_four_block_index_bit_count: quadrant_compression.index_bit_count,
        four_by_four_block_atlas_byte_count: quadrant_compression.byte_count,
        static_page_group_count: codebook.page_assignments.len(),
        static_page_group_overlay_reference_count,
        maximum_static_page_group_overlay_tile_count,
        visible_page_recipe_count: unique_recipes.len(),
        visible_page_recipe_reference_count,
        visible_page_overlay_reference_count,
        maximum_visible_page_overlay_tile_count,
        sequential_page_transition_count,
        distinct_visible_page_recipe_transition_count: distinct_recipe_transitions.len(),
        unchanged_visible_page_recipe_transition_count,
        maximum_delta_tile_count,
        maximum_delta_ppu_write_count,
        total_delta_ppu_write_count,
        rebuild_every_visible_page_ppu_write_count,
        initial_rebuild_then_delta_ppu_write_count,
        direct_visible_page_recipe_byte_count,
        bitpacked_visible_page_recipe_byte_count,
        bitmap_and_atlas_index_visible_page_recipe_byte_count,
        direct_delta_recipe_byte_count,
        bitpacked_delta_recipe_byte_count,
        dense_group_lookup_byte_count,
        record_page_group_selector_byte_count,
        record_selector_directory_byte_count,
        scan_material_byte_count,
        dynamic_string_control_count,
        dynamic_string_page_count,
        dynamic_string_selector_count,
    })
}

struct BlockAtlasMeasurement {
    block_count: usize,
    index_bit_count: usize,
    byte_count: usize,
}

fn measure_four_by_four_block_atlas(atlas: &[u8]) -> Result<BlockAtlasMeasurement> {
    ensure!(
        atlas.len().is_multiple_of(8),
        "1bpp glyph atlas is not made of eight-row tiles"
    );
    let mut blocks = BTreeSet::new();
    for tile in atlas.chunks_exact(8) {
        for (row_start, shift) in [(0, 4), (0, 0), (4, 4), (4, 0)] {
            let block = tile[row_start..row_start + 4]
                .iter()
                .fold(0u16, |packed, row| {
                    (packed << 4) | u16::from((row >> shift) & 0x0F)
                });
            blocks.insert(block);
        }
    }
    let index_bit_count = if blocks.len() <= 1 {
        1
    } else {
        usize::BITS as usize - (blocks.len() - 1).leading_zeros() as usize
    };
    let index_count = atlas.len() / 8 * 4;
    Ok(BlockAtlasMeasurement {
        block_count: blocks.len(),
        index_bit_count,
        byte_count: blocks.len() * 2 + (index_count * index_bit_count).div_ceil(8),
    })
}

fn compose_visible_page(
    source_font_page: &[u8],
    static_page_pack: &[u8],
    static_page_assignments: &[BTreeMap<char, u8>],
    recipe: &VisiblePageRecipe,
) -> Result<Vec<u8>> {
    let group_start = recipe
        .static_page_group_index
        .checked_mul(FONT_PAGE_SIZE)
        .context("visible dialogue static-page offset overflow")?;
    let group_page = static_page_pack
        .get(group_start..group_start + FONT_PAGE_SIZE)
        .context("visible dialogue static page is outside the page pack")?;
    let assignments = static_page_assignments
        .get(recipe.static_page_group_index)
        .context("visible dialogue static page has no assignments")?;
    let mut page = source_font_page.to_vec();
    for glyph in &recipe.target_glyphs {
        let code = assignments
            .get(glyph)
            .with_context(|| format!("visible dialogue recipe lost glyph {glyph:?}"))?;
        let tile_start = usize::from(*code) * FONT_TILE_SIZE;
        page[tile_start..tile_start + FONT_TILE_SIZE]
            .copy_from_slice(&group_page[tile_start..tile_start + FONT_TILE_SIZE]);
    }
    Ok(page)
}

fn target_codes(
    static_page_assignments: &[BTreeMap<char, u8>],
    recipe: &VisiblePageRecipe,
) -> Result<BTreeSet<u8>> {
    let assignments = static_page_assignments
        .get(recipe.static_page_group_index)
        .context("visible dialogue target-code group is missing")?;
    recipe
        .target_glyphs
        .iter()
        .map(|glyph| {
            assignments
                .get(glyph)
                .copied()
                .with_context(|| format!("visible dialogue target-code lookup lost {glyph:?}"))
        })
        .collect()
}

fn record_workset_indices(dialogue: &MainDialogueBundlePlan) -> Result<BTreeMap<&str, Vec<usize>>> {
    let mut records = BTreeMap::<&str, Vec<(usize, usize)>>::new();
    for (workset_index, workset) in dialogue.page_worksets.iter().enumerate() {
        records
            .entry(workset.record_id.as_str())
            .or_default()
            .push((workset.page_index, workset_index));
    }
    records
        .into_iter()
        .map(|(record_id, mut pages)| {
            pages.sort_unstable_by_key(|(page_index, _)| *page_index);
            ensure!(
                pages
                    .iter()
                    .enumerate()
                    .all(|(expected, (actual, _))| expected == *actual),
                "dialogue runtime composition record {record_id} has a page-index gap"
            );
            Ok((
                record_id,
                pages
                    .into_iter()
                    .map(|(_, workset_index)| workset_index)
                    .collect(),
            ))
        })
        .collect()
}

fn changed_tile_count(from: &[u8], to: &[u8]) -> Result<usize> {
    ensure!(
        from.len() == FONT_PAGE_SIZE && to.len() == FONT_PAGE_SIZE,
        "dialogue delta page length changed"
    );
    Ok(from
        .chunks_exact(FONT_TILE_SIZE)
        .zip(to.chunks_exact(FONT_TILE_SIZE))
        .filter(|(from, to)| from != to)
        .count())
}

fn delta_ppu_write_count(
    from: &[u8],
    to: &[u8],
    from_target_codes: &BTreeSet<u8>,
    to_target_codes: &BTreeSet<u8>,
) -> Result<usize> {
    ensure!(
        from.len() == FONT_PAGE_SIZE && to.len() == FONT_PAGE_SIZE,
        "dialogue delta PPU page length changed"
    );
    Ok(from
        .chunks_exact(FONT_TILE_SIZE)
        .zip(to.chunks_exact(FONT_TILE_SIZE))
        .enumerate()
        .filter(|(_, (from, to))| from != to)
        .map(|(code, _)| {
            let code = u8::try_from(code).expect("font page has exactly 256 tiles");
            if from_target_codes.contains(&code) && to_target_codes.contains(&code) {
                8
            } else {
                FONT_TILE_SIZE
            }
        })
        .sum())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tile_delta_counts_only_changed_sixteen_byte_tiles() {
        let from = vec![0; FONT_PAGE_SIZE];
        let mut to = from.clone();
        to[FONT_TILE_SIZE] = 1;

        assert_eq!(changed_tile_count(&from, &to).unwrap(), 1);
    }

    #[test]
    fn packed_recipe_estimates_include_directory_offsets() {
        let overlay_count = 100usize;
        let recipe_count = 4usize;
        let directory_bytes = (recipe_count + 1) * 2;

        assert_eq!((overlay_count * 18).div_ceil(8) + directory_bytes, 235);
        assert_eq!(
            recipe_count * 32 + (overlay_count * 10).div_ceil(8) + directory_bytes,
            263
        );
    }

    #[test]
    fn target_to_target_delta_reuses_the_zero_high_bitplane() {
        let mut from = vec![0; FONT_PAGE_SIZE];
        let mut to = from.clone();
        from[0] = 1;
        to[0] = 2;
        let target_codes = BTreeSet::from([0]);

        assert_eq!(
            delta_ppu_write_count(&from, &to, &target_codes, &target_codes).unwrap(),
            8
        );
        assert_eq!(
            delta_ppu_write_count(&from, &to, &target_codes, &BTreeSet::new()).unwrap(),
            FONT_TILE_SIZE
        );
    }

    #[test]
    fn quadrant_dictionary_measurement_counts_dictionary_and_packed_indices() {
        let atlas = vec![0; 16];

        let measured = measure_four_by_four_block_atlas(&atlas).unwrap();

        assert_eq!(measured.block_count, 1);
        assert_eq!(measured.index_bit_count, 1);
        assert_eq!(measured.byte_count, 3);
    }
}
