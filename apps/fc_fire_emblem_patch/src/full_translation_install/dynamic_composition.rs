use std::collections::{BTreeMap, BTreeSet};

use anyhow::{Context, Result, ensure};

use crate::{
    dialogue_assets::MainDialogueDisplayPlan,
    font::{load_dalmoori, rasterize_glyph},
    font_slots::{FONT_PAGE_SIZE, FONT_TILE_SIZE},
    mapper165::battle_codebook_plan::GlyphWorksetPagePlan,
};

use super::dynamic_inputs::DynamicStringRemapPlan;

pub(super) struct DialogueRuntimeCompositionPlan {
    pub(super) glyph_atlas: Vec<u8>,
    pub(super) scan_material: Vec<u8>,
    pub(super) scan_material_sha1: String,
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
    /// 그룹마다 «이 그룹이 실제로 쓰는 코드» 목록의 시작 오프셋 표다.
    pub(super) group_tile_list_directory_byte_count: usize,
    /// 그 목록들의 총 길이다. 코드 한 바이트씩이다.
    pub(super) group_tile_list_byte_count: usize,
    /// 스캔 재료 안에서 목록 오프셋 표가 시작하는 자리다.
    pub(super) group_tile_list_directory_offset: usize,
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
    dialogue: &MainDialogueDisplayPlan,
    codebook: &GlyphWorksetPagePlan,
    dynamic_remap: &DynamicStringRemapPlan,
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
    let glyph_atlas_indices = dialogue_glyphs
        .iter()
        .copied()
        .enumerate()
        .map(|(index, glyph)| (glyph, index))
        .collect::<BTreeMap<_, _>>();
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
    let group_tile_list_directory_byte_count = (codebook.page_assignments.len() + 1) * 2;
    let group_tile_list_byte_count = codebook
        .page_assignments
        .iter()
        .map(|assignments| assignments.len())
        .sum::<usize>();
    let group_tile_list_directory_offset = dense_group_lookup_byte_count
        + record_page_group_selector_byte_count
        + record_selector_directory_byte_count;
    let scan_material_byte_count = group_tile_list_directory_offset
        + group_tile_list_directory_byte_count
        + group_tile_list_byte_count;
    let scan_material = encode_scan_material(
        dialogue,
        codebook,
        dynamic_remap,
        &glyph_atlas_indices,
        &record_worksets,
    )?;
    ensure!(
        scan_material.len() == scan_material_byte_count,
        "dialogue scan material measurement differs from its encoding"
    );
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
        scan_material_sha1: crate::sha1_hex(&scan_material),
        scan_material,
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
        group_tile_list_directory_byte_count,
        group_tile_list_byte_count,
        group_tile_list_directory_offset,
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

fn record_workset_indices(
    dialogue: &MainDialogueDisplayPlan,
) -> Result<BTreeMap<&str, Vec<usize>>> {
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

fn encode_scan_material(
    dialogue: &MainDialogueDisplayPlan,
    codebook: &GlyphWorksetPagePlan,
    dynamic_remap: &DynamicStringRemapPlan,
    glyph_atlas_indices: &BTreeMap<char, usize>,
    record_worksets: &BTreeMap<&str, Vec<usize>>,
) -> Result<Vec<u8>> {
    let mut encoded = encode_dense_group_lookups(&codebook.page_assignments, glyph_atlas_indices)?;
    let mut selectors = Vec::with_capacity(dialogue.page_worksets.len());
    let mut directory = Vec::with_capacity((dialogue.record_ids.len() + 1) * 2);
    ensure!(
        dynamic_remap.workset_page_selectors.len() == dialogue.page_worksets.len(),
        "dialogue scan material lost page selectors"
    );
    for record_id in &dialogue.record_ids {
        directory.extend_from_slice(
            &u16::try_from(selectors.len())
                .context("dialogue page-selector material exceeds a 16-bit offset")?
                .to_le_bytes(),
        );
        let indices = record_worksets
            .get(record_id.as_str())
            .with_context(|| format!("{record_id} has no runtime page selectors"))?;
        selectors.extend(
            indices
                .iter()
                .map(|index| dynamic_remap.workset_page_selectors[*index]),
        );
    }
    directory.extend_from_slice(
        &u16::try_from(selectors.len())
            .context("dialogue page-selector end exceeds a 16-bit offset")?
            .to_le_bytes(),
    );
    ensure!(
        selectors.len() == dialogue.page_worksets.len(),
        "dialogue scan material did not serialize every page selector exactly once"
    );
    encoded.extend_from_slice(&selectors);
    encoded.extend_from_slice(&directory);
    let (group_directory, group_lists) = encode_group_tile_lists(&codebook.page_assignments);
    encoded.extend_from_slice(&group_directory);
    encoded.extend_from_slice(&group_lists);
    Ok(encoded)
}

/// 그룹마다 실제로 쓰는 코드 목록을 만든다.
///
/// 런타임이 조밀 조회표의 클래스 비트를 훑어 존재 여부를 가리면 프레임마다 코드
/// 256개를 가변 시프트로 풀어야 한다. 그 비용은 타일 수에 비례하지 않아 예산에
/// 들어가지 않는다. 그래서 빌드가 미리 세운다.
///
/// atlas 색인은 담지 않는다. 기존 조밀 조회표가 코드로 곧바로 주기 때문이고,
/// 색인을 함께 담으면 세 배가 되어 용기에 들어가지 않는다.
fn encode_group_tile_lists(page_assignments: &[BTreeMap<char, u8>]) -> (Vec<u8>, Vec<u8>) {
    let mut directory = Vec::with_capacity((page_assignments.len() + 1) * 2);
    let mut lists = Vec::new();
    for assignments in page_assignments {
        directory.extend_from_slice(&(lists.len() as u16).to_le_bytes());
        // 코드 오름차순으로 담는다. 순서가 정해져 있어야 목록을 다시 만들었을 때
        // 같은 바이트가 나오고, 런타임 커서가 중간에서 이어받을 수 있다.
        let mut codes: Vec<u8> = assignments.values().copied().collect();
        codes.sort_unstable();
        lists.extend_from_slice(&codes);
    }
    directory.extend_from_slice(&(lists.len() as u16).to_le_bytes());
    (directory, lists)
}

fn encode_dense_group_lookups(
    page_assignments: &[BTreeMap<char, u8>],
    glyph_atlas_indices: &BTreeMap<char, usize>,
) -> Result<Vec<u8>> {
    let mut encoded = Vec::with_capacity(page_assignments.len() * (256 + 64));
    for (group_index, assignments) in page_assignments.iter().enumerate() {
        let mut low_indices = vec![0u8; 256];
        let mut high_classes = vec![0xFFu8; 64];
        for (glyph, code) in assignments {
            let atlas_index = glyph_atlas_indices.get(glyph).copied().with_context(|| {
                format!("dialogue page group {group_index} lost atlas glyph {glyph:?}")
            })?;
            ensure!(
                atlas_index < 3 * 256,
                "dialogue atlas index does not fit the two-bit lookup class"
            );
            low_indices[usize::from(*code)] = atlas_index as u8;
            let packed_index = usize::from(*code) / 4;
            let shift = usize::from(*code % 4) * 2;
            let mask = !(0b11 << shift);
            high_classes[packed_index] = (high_classes[packed_index] & mask)
                | (u8::try_from(atlas_index >> 8).expect("atlas class is below three") << shift);
        }
        encoded.extend_from_slice(&low_indices);
        encoded.extend_from_slice(&high_classes);
    }
    Ok(encoded)
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

    #[test]
    fn dense_group_lookup_uses_class_three_as_the_unassigned_sentinel() {
        let assignments = vec![BTreeMap::from([('가', 0x42), ('나', 0x43)])];
        let atlas = BTreeMap::from([('가', 5usize), ('나', 0x105usize)]);

        let encoded = encode_dense_group_lookups(&assignments, &atlas).unwrap();

        assert_eq!(encoded.len(), 320);
        assert_eq!(encoded[0x42], 5);
        assert_eq!(encoded[0x43], 5);
        let classes = encoded[256 + usize::from(0x42u8) / 4];
        assert_eq!((classes >> ((0x42 % 4) * 2)) & 0b11, 0);
        assert_eq!((classes >> ((0x43 % 4) * 2)) & 0b11, 1);
        assert_eq!(encoded[256] & 0b11, 0b11);
    }

    /// 목록은 그 그룹이 쓰는 코드를 하나도 빠뜨리지 않고, 쓰지 않는 코드를 담지
    /// 않아야 한다. 빠지면 그 글자가 CHR RAM에 안 올라가고, 남으면 원본 글꼴 타일을
    /// 덮는다. 둘 다 화면에 잘못된 글자를 낸다.
    #[test]
    fn every_group_list_holds_exactly_the_codes_that_group_assigns() {
        let assignments = vec![
            BTreeMap::from([('가', 0x42u8), ('나', 0x10)]),
            BTreeMap::from([('다', 0x80u8)]),
        ];

        let (directory, lists) = encode_group_tile_lists(&assignments);

        assert_eq!(directory.len(), (assignments.len() + 1) * 2);
        for (group, expected) in assignments.iter().enumerate() {
            let start = usize::from(u16::from_le_bytes([
                directory[group * 2],
                directory[group * 2 + 1],
            ]));
            let end = usize::from(u16::from_le_bytes([
                directory[group * 2 + 2],
                directory[group * 2 + 3],
            ]));
            let mut wanted: Vec<u8> = expected.values().copied().collect();
            wanted.sort_unstable();
            assert_eq!(&lists[start..end], wanted.as_slice());
        }
    }

    /// 오프셋 표의 마지막 항목이 전체 길이여야 마지막 그룹의 끝을 알 수 있다.
    #[test]
    fn the_group_directory_brackets_the_last_list() {
        let assignments = vec![BTreeMap::from([('가', 1u8), ('나', 2u8)])];

        let (directory, lists) = encode_group_tile_lists(&assignments);

        let end = usize::from(u16::from_le_bytes([directory[2], directory[3]]));
        assert_eq!(end, lists.len());
    }
}
