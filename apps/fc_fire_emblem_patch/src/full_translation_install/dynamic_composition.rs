use std::collections::{BTreeMap, BTreeSet};

use anyhow::{Context, Result, ensure};

use super::runtime_material::{
    MATERIAL_HEADER_BYTE_COUNT, MATERIAL_SECTION_COUNT, SECTION_DESCRIPTOR_BYTE_COUNT,
};
use crate::{
    dialogue_assets::MainDialogueDisplayPlan,
    dialogue_inventory::MainDialogueGraphReport,
    font::{load_dalmoori, rasterize_glyph},
    font_slots::{FONT_PAGE_SIZE, FONT_TILE_SIZE},
    mapper165::battle_codebook_plan::{GlyphWorkset, GlyphWorksetPagePlan},
};

use super::dynamic_inputs::DynamicStringPageCodePlan;

/// MMC3 뱅크 한 장이다. 레시피 덩이는 이 경계를 걸치면 안 된다.
const MMC3_PAGE_BYTE_COUNT: usize = 8 * 1024;
/// 가시 페이지 레시피 항목 하나의 크기다. 코드 하나와 atlas CPU 주소 둘이다.
const PAGE_RECIPE_ENTRY_BYTE_COUNT: usize = 3;
/// atlas가 타일 하나에 쓰는 바이트다. 1bpp 8×8.
const GLYPH_ATLAS_TILE_BYTE_COUNT: usize = 8;

pub(super) struct DialogueRuntimeCompositionPlan {
    pub(super) glyph_atlas: Vec<u8>,
    pub(super) glyph_atlas_characters: Vec<char>,
    pub(super) scan_material: Vec<u8>,
    pub(super) scan_material_sha1: String,
    pub(super) page_recipe_blocks: Vec<u8>,
    pub(super) page_recipe_blocks_sha1: String,
    pub(super) glyph_atlas_tile_count: usize,
    pub(super) dialogue_codebook_glyph_count: usize,
    pub(super) additional_cross_domain_glyph_count: usize,
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
    pub(super) resident_group_transition_count: usize,
    pub(super) resident_group_change_count: usize,
    pub(super) resident_group_reuse_count: usize,
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
    /// 작업집합마다 하나씩인 가시 페이지 레시피 오프셋 표의 길이다.
    pub(super) page_recipe_reference_byte_count: usize,
    /// 레시피 덩이 전체의 길이다. 페이지 정렬 여백을 포함한다.
    pub(super) page_recipe_block_byte_count: usize,
    /// 스캔 재료 안에서 레시피 오프셋 표와 레코드 디렉터리가 시작하는 자리다.
    pub(super) page_recipe_reference_offset: usize,
    pub(super) record_recipe_directory_offset: usize,
    pub(super) record_recipe_directory_byte_count: usize,
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
    transition_graph: &MainDialogueGraphReport,
    runtime_worksets: &[GlyphWorkset],
    codebook: &GlyphWorksetPagePlan,
    dynamic_page_codes: &DynamicStringPageCodePlan,
    source_font_page: &[u8],
    static_page_pack: &[u8],
    additional_target_glyphs: &BTreeSet<char>,
) -> Result<DialogueRuntimeCompositionPlan> {
    ensure!(
        source_font_page.len() == FONT_PAGE_SIZE,
        "dialogue runtime composition source page length changed"
    );
    ensure!(
        static_page_pack.len() == codebook.page_assignments.len() * FONT_PAGE_SIZE,
        "dialogue runtime composition static page pack length changed"
    );
    let dialogue_codebook_glyphs = codebook
        .page_assignments
        .iter()
        .flat_map(|assignments| assignments.keys().copied())
        .collect::<BTreeSet<_>>();
    ensure!(
        dialogue_codebook_glyphs.len() == codebook.glyph_count,
        "dialogue runtime composition lost glyph-atlas entries"
    );
    let additional_cross_domain_glyph_count = additional_target_glyphs
        .difference(&dialogue_codebook_glyphs)
        .count();
    let dialogue_glyphs = dialogue_codebook_glyphs
        .union(additional_target_glyphs)
        .copied()
        .collect::<Vec<_>>();
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

    ensure!(
        runtime_worksets.len() == dialogue.page_worksets.len(),
        "dialogue runtime workset population changed"
    );
    let workset_recipes = build_visible_page_recipes(
        runtime_worksets,
        &codebook.workset_page_indices,
        &codebook.page_assignments,
    )?;
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
    let resident_transitions = resident_group_transitions(
        transition_graph,
        &record_worksets,
        &dynamic_page_codes.workset_page_selectors,
    )?;
    let resident_group_transition_count = resident_transitions.len();
    let resident_group_change_count = resident_transitions
        .iter()
        .filter(|transition| transition.from_selector != transition.to_selector)
        .count();
    let resident_group_reuse_count = resident_group_transition_count - resident_group_change_count;
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
    let page_recipe_reference_byte_count = dialogue.page_worksets.len() * 2;
    let record_recipe_directory_byte_count = (record_worksets.len() + 1) * 2;
    // 스캔 재료는 용기 안에서 헤더·구역 표·글리프 atlas 뒤에 놓인다. 레시피 덩이의
    // 페이지 정렬은 이 절대 위치를 알아야 정해진다.
    let atlas_container_offset =
        MATERIAL_HEADER_BYTE_COUNT + MATERIAL_SECTION_COUNT * SECTION_DESCRIPTOR_BYTE_COUNT;
    ensure!(
        atlas_container_offset + glyph_atlas.len() <= MMC3_PAGE_BYTE_COUNT,
        "the glyph atlas no longer fits one MMC3 page, so its address is not one constant"
    );
    let atlas_cpu_base = u16::try_from(0x8000 + atlas_container_offset)
        .context("glyph atlas CPU base does not fit the 8000 window")?;
    let scan_section_container_offset = atlas_container_offset + glyph_atlas.len();
    let encoded_scan = encode_scan_material(
        dialogue,
        codebook,
        dynamic_page_codes,
        &glyph_atlas_indices,
        &record_worksets,
        &workset_recipe_indices,
        &unique_recipes.iter().cloned().collect::<Vec<_>>(),
        atlas_cpu_base,
        scan_section_container_offset,
    )?;
    let scan_material = encoded_scan.bytes;
    let page_recipe_reference_offset = encoded_scan.page_recipe_reference_offset;
    let record_recipe_directory_offset = encoded_scan.record_recipe_directory_offset;
    let page_recipe_blocks = encoded_scan.page_recipe_blocks;
    let scan_material_byte_count = scan_material.len();
    let page_recipe_block_byte_count = page_recipe_blocks.len();
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
        glyph_atlas_characters: dialogue_glyphs.clone(),
        scan_material_sha1: crate::sha1_hex(&scan_material),
        scan_material,
        page_recipe_blocks_sha1: crate::sha1_hex(&page_recipe_blocks),
        page_recipe_blocks,
        glyph_atlas_tile_count: dialogue_glyphs.len(),
        dialogue_codebook_glyph_count: codebook.glyph_count,
        additional_cross_domain_glyph_count,
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
        resident_group_transition_count,
        resident_group_change_count,
        resident_group_reuse_count,
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
        page_recipe_reference_byte_count,
        page_recipe_block_byte_count,
        page_recipe_reference_offset,
        record_recipe_directory_offset,
        record_recipe_directory_byte_count,
        scan_material_byte_count,
        dynamic_string_control_count,
        dynamic_string_page_count,
        dynamic_string_selector_count,
    })
}

/// 가시 대사 본문뿐 아니라 같은 수명에 머무는 이름·장 제목·선택지·결과 화면의
/// 글리프까지 페이지 레시피에 넣는다. 정적 그룹 전체를 싣지는 않지만, 최종 상주권
/// 계획이 요구한 글리프를 하나라도 줄여서는 안 된다.
fn build_visible_page_recipes(
    runtime_worksets: &[GlyphWorkset],
    workset_page_indices: &[usize],
    page_assignments: &[BTreeMap<char, u8>],
) -> Result<Vec<VisiblePageRecipe>> {
    ensure!(
        runtime_worksets.len() == workset_page_indices.len(),
        "visible-page recipe inputs have different workset counts"
    );
    runtime_worksets
        .iter()
        .zip(workset_page_indices)
        .enumerate()
        .map(|(workset_index, (workset, static_page_group_index))| {
            let group_assignments = page_assignments
                .get(*static_page_group_index)
                .context("visible dialogue page refers to a missing static page group")?;
            ensure!(
                workset
                    .target_glyphs
                    .iter()
                    .all(|glyph| group_assignments.contains_key(glyph)),
                "visible dialogue page {workset_index} lost a runtime-lifetime glyph assignment"
            );
            Ok(VisiblePageRecipe {
                static_page_group_index: *static_page_group_index,
                target_glyphs: workset.target_glyphs.iter().copied().collect(),
            })
        })
        .collect()
}

struct ResidentGroupTransition {
    from_selector: u8,
    to_selector: u8,
}

/// 상주권을 유지하는 실제 생산자 전이를 빠짐없이 같은 모집단으로 만든다. 한 레코드
/// 안의 다음 페이지와 E4/E6 그래프 간선은 모두 직전 완성 그룹을 입력으로 넘긴다.
/// 독립 수명 진입은 상주권이 없으므로 여기 넣지 않는다.
fn resident_group_transitions(
    graph: &MainDialogueGraphReport,
    record_worksets: &BTreeMap<&str, Vec<usize>>,
    workset_page_selectors: &[u8],
) -> Result<Vec<ResidentGroupTransition>> {
    let selector = |workset_index: usize| {
        workset_page_selectors
            .get(workset_index)
            .copied()
            .context("resident transition workset selector is missing")
    };
    let mut transitions = Vec::new();
    for worksets in record_worksets.values() {
        for pair in worksets.windows(2) {
            transitions.push(ResidentGroupTransition {
                from_selector: selector(pair[0])?,
                to_selector: selector(pair[1])?,
            });
        }
    }
    for edge in &graph.transition_edges {
        let source_id = format!(
            "{}:{:03}",
            edge.source_table_id, edge.source_canonical_entry_index
        );
        let target_id = format!(
            "{}:{:03}",
            edge.target_table_id, edge.target_canonical_entry_index
        );
        let source_worksets = record_worksets
            .get(source_id.as_str())
            .with_context(|| format!("resident transition source {source_id} is missing"))?;
        let target_worksets = record_worksets
            .get(target_id.as_str())
            .with_context(|| format!("resident transition target {target_id} is missing"))?;
        transitions.push(ResidentGroupTransition {
            from_selector: selector(
                *source_worksets
                    .last()
                    .context("resident transition source has no visible page")?,
            )?,
            to_selector: selector(
                *target_worksets
                    .first()
                    .context("resident transition target has no visible page")?,
            )?,
        });
    }
    ensure!(
        transitions.len()
            == record_worksets
                .values()
                .map(|worksets| worksets.len().saturating_sub(1))
                .sum::<usize>()
                + graph.transition_edges.len(),
        "resident transition inventory is incomplete"
    );
    Ok(transitions)
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

struct EncodedScanMaterial {
    bytes: Vec<u8>,
    page_recipe_blocks: Vec<u8>,
    page_recipe_reference_offset: usize,
    record_recipe_directory_offset: usize,
}

fn encode_scan_material(
    dialogue: &MainDialogueDisplayPlan,
    codebook: &GlyphWorksetPagePlan,
    dynamic_page_codes: &DynamicStringPageCodePlan,
    glyph_atlas_indices: &BTreeMap<char, usize>,
    record_worksets: &BTreeMap<&str, Vec<usize>>,
    workset_recipe_indices: &[usize],
    unique_recipes: &[VisiblePageRecipe],
    atlas_cpu_base: u16,
    section_container_offset: usize,
) -> Result<EncodedScanMaterial> {
    let mut encoded = Vec::new();
    let reference_byte_count = dialogue.page_worksets.len() * 2;
    let directory_byte_count = (dialogue.record_ids.len() + 1) * 2;
    let index_byte_count = reference_byte_count + directory_byte_count;
    ensure!(
        index_byte_count <= MMC3_PAGE_BYTE_COUNT,
        "dialogue visible-page index does not fit one mapped MMC3 page"
    );
    let within_page = section_container_offset % MMC3_PAGE_BYTE_COUNT;
    if within_page + index_byte_count > MMC3_PAGE_BYTE_COUNT {
        encoded.resize(MMC3_PAGE_BYTE_COUNT - within_page, 0xFF);
    }
    let page_recipe_reference_offset = encoded.len();
    let record_recipe_directory_offset = page_recipe_reference_offset + reference_byte_count;
    let index_end = record_recipe_directory_offset + directory_byte_count;

    let (recipe_offsets, recipe_blocks) = encode_visible_page_recipe_blocks(
        unique_recipes,
        &codebook.page_assignments,
        glyph_atlas_indices,
        atlas_cpu_base,
        0,
    )?;

    let mut references = Vec::with_capacity(reference_byte_count);
    let mut directory = Vec::with_capacity((dialogue.record_ids.len() + 1) * 2);
    ensure!(
        dynamic_page_codes.workset_page_selectors.len() == dialogue.page_worksets.len(),
        "dialogue scan material lost codebook selectors"
    );
    ensure!(
        workset_recipe_indices.len() == dialogue.page_worksets.len(),
        "dialogue scan material lost visible-page recipe references"
    );
    for record_id in &dialogue.record_ids {
        directory.extend_from_slice(
            &u16::try_from(references.len() / 2)
                .context("dialogue page-recipe material exceeds a 16-bit index")?
                .to_le_bytes(),
        );
        let indices = record_worksets
            .get(record_id.as_str())
            .with_context(|| format!("{record_id} has no runtime page recipes"))?;
        for workset_index in indices {
            let recipe_index = workset_recipe_indices[*workset_index];
            let recipe_offset = recipe_offsets.get(recipe_index).with_context(|| {
                format!("workset {workset_index} selects missing recipe {recipe_index}")
            })?;
            references.extend_from_slice(&recipe_offset.to_le_bytes());
        }
    }
    directory.extend_from_slice(
        &u16::try_from(references.len() / 2)
            .context("dialogue page-recipe end exceeds a 16-bit index")?
            .to_le_bytes(),
    );
    ensure!(
        references.len() == reference_byte_count,
        "dialogue scan material did not serialize every page recipe exactly once"
    );
    ensure!(
        directory.len() == directory_byte_count,
        "dialogue page-recipe directory length changed"
    );
    let record_page_counts = dialogue
        .record_ids
        .iter()
        .map(|record_id| {
            record_worksets
                .get(record_id.as_str())
                .map(Vec::len)
                .with_context(|| format!("{record_id} has no runtime page recipes"))
        })
        .collect::<Result<Vec<_>>>()?;
    verify_visible_page_recipe_material(
        &references,
        &directory,
        &recipe_offsets,
        &recipe_blocks,
        &record_page_counts,
    )?;
    encoded.extend_from_slice(&references);
    encoded.extend_from_slice(&directory);
    ensure!(
        encoded.len() == index_end,
        "dialogue page-recipe index length changed"
    );
    Ok(EncodedScanMaterial {
        bytes: encoded,
        page_recipe_blocks: recipe_blocks,
        page_recipe_reference_offset,
        record_recipe_directory_offset,
    })
}

/// 레코드→페이지→레시피의 두 단계 색인과 레시피 덩이를 서로 다시 결속한다.
///
/// 직렬화 코드는 이 자료를 같은 함수에서 만들지만, 소비자는 서로 다른 PRG 창에서
/// 읽는다. 따라서 참조 하나가 덩이 중간이나 정렬 여백을 가리키는 회귀도 빌드에서
/// 닫아야 한다.
fn verify_visible_page_recipe_material(
    references: &[u8],
    directory: &[u8],
    recipe_offsets: &[u16],
    recipe_blocks: &[u8],
    record_page_counts: &[usize],
) -> Result<()> {
    ensure!(
        references.len().is_multiple_of(2),
        "visible-page recipe reference table has a partial entry"
    );
    ensure!(
        directory.len() == (record_page_counts.len() + 1) * 2,
        "visible-page recipe record directory length changed"
    );
    ensure!(
        recipe_offsets.windows(2).all(|pair| pair[0] < pair[1]),
        "visible-page recipe block offsets are not strictly increasing"
    );

    let block_starts = recipe_offsets.iter().copied().collect::<BTreeSet<_>>();
    ensure!(
        block_starts.len() == recipe_offsets.len(),
        "visible-page recipe block offset is duplicated"
    );
    for (index, offset) in recipe_offsets.iter().copied().enumerate() {
        let start = usize::from(offset);
        let count = usize::from(
            *recipe_blocks
                .get(start)
                .with_context(|| format!("visible-page recipe {index} starts outside its pool"))?,
        );
        let end = start
            .checked_add(1 + count * PAGE_RECIPE_ENTRY_BYTE_COUNT)
            .context("visible-page recipe block length overflow")?;
        ensure!(
            end <= recipe_blocks.len(),
            "visible-page recipe {index} ends outside its pool"
        );
        ensure!(
            start / MMC3_PAGE_BYTE_COUNT == (end - 1) / MMC3_PAGE_BYTE_COUNT,
            "visible-page recipe {index} crosses an MMC3 page"
        );
        let next = recipe_offsets
            .get(index + 1)
            .map(|offset| usize::from(*offset))
            .unwrap_or(recipe_blocks.len());
        ensure!(
            end <= next,
            "visible-page recipe {index} overlaps the next block"
        );
        ensure!(
            recipe_blocks[end..next].iter().all(|byte| *byte == 0xFF),
            "visible-page recipe {index} has non-padding bytes before the next block"
        );
    }

    let referenced_offsets = references
        .chunks_exact(2)
        .map(|entry| u16::from_le_bytes([entry[0], entry[1]]))
        .collect::<BTreeSet<_>>();
    ensure!(
        referenced_offsets == block_starts,
        "visible-page recipe references do not cover every block exactly by its start"
    );

    let mut expected_page_start = 0usize;
    for (record_index, page_count) in record_page_counts.iter().copied().enumerate() {
        let directory_offset = record_index * 2;
        let actual_page_start = usize::from(u16::from_le_bytes([
            directory[directory_offset],
            directory[directory_offset + 1],
        ]));
        ensure!(
            actual_page_start == expected_page_start,
            "visible-page recipe directory record {record_index} starts at {actual_page_start}, expected {expected_page_start}"
        );
        expected_page_start = expected_page_start
            .checked_add(page_count)
            .context("visible-page recipe page count overflow")?;
    }
    let final_offset = record_page_counts.len() * 2;
    let actual_page_end = usize::from(u16::from_le_bytes([
        directory[final_offset],
        directory[final_offset + 1],
    ]));
    ensure!(
        actual_page_end == references.len() / 2 && actual_page_end == expected_page_start,
        "visible-page recipe directory end does not cover every workset"
    );
    Ok(())
}

/// 가시 페이지 하나가 런타임에 필요한 전부를 한 덩이로 묶는다.
///
/// 덩이는 `[항목 수][코드, atlas 주소 하위, atlas 주소 상위] × n`이다. atlas 주소를
/// 그대로 담는 이유는 소비자가 계산을 하나도 하지 않게 하려는 것이다.
///
/// 조밀 조회표를 쓰지 않는 이유가 여기 있다. 그 표는 atlas 색인의 상위 두 비트를
/// 코드 4개당 1바이트에 packing해 두는데, 런타임이 그걸 풀려면 타일마다 가변 시프트를
/// 돌려야 한다. 6502에는 가변 시프트 명령이 없어 루프가 되고, 그 루프가 타일당
/// 40사이클 남짓을 먹는다. 빌드가 주소를 미리 더해 두면 그 비용이 0이 된다.
/// 크기도 조회표를 함께 두는 것보다 작다.
///
/// 덩이가 8 KiB 페이지 경계를 걸치면 소비자가 타일마다 뱅크를 한 번 더 바꿔야
/// 하므로, 걸칠 자리에서는 다음 페이지로 밀어 정렬한다.
fn encode_visible_page_recipe_blocks(
    recipes: &[VisiblePageRecipe],
    page_assignments: &[BTreeMap<char, u8>],
    glyph_atlas_indices: &BTreeMap<char, usize>,
    atlas_cpu_base: u16,
    section_container_offset: usize,
) -> Result<(Vec<u16>, Vec<u8>)> {
    let mut offsets = Vec::with_capacity(recipes.len());
    let mut blocks: Vec<u8> = Vec::new();
    for (recipe_index, recipe) in recipes.iter().enumerate() {
        let assignments = page_assignments
            .get(recipe.static_page_group_index)
            .with_context(|| {
                format!(
                    "visible-page recipe {recipe_index} selects missing codebook group {}",
                    recipe.static_page_group_index
                )
            })?;
        // 코드 오름차순이라야 다시 만들었을 때 같은 바이트가 나오고, 커서가 중간에서
        // 이어받을 수 있다.
        let mut entries: Vec<(u8, u16)> = Vec::with_capacity(recipe.target_glyphs.len());
        for glyph in &recipe.target_glyphs {
            let code = assignments.get(glyph).copied().with_context(|| {
                format!("visible-page recipe {recipe_index} lost codebook glyph {glyph:?}")
            })?;
            let atlas_index = glyph_atlas_indices.get(glyph).copied().with_context(|| {
                format!("visible-page recipe {recipe_index} lost atlas glyph {glyph:?}")
            })?;
            let offset = u16::try_from(atlas_index * GLYPH_ATLAS_TILE_BYTE_COUNT)
                .context("glyph atlas offset does not fit a 16-bit address")?;
            let address = atlas_cpu_base
                .checked_add(offset)
                .context("glyph atlas address leaves the 8000 window")?;
            ensure!(
                address < 0xA000,
                "glyph atlas address {address:04X} leaves the 8000 window"
            );
            entries.push((code, address));
        }
        entries.sort_unstable();
        ensure!(
            entries.windows(2).all(|pair| pair[0].0 != pair[1].0),
            "visible-page recipe {recipe_index} assigns one code more than once"
        );
        let block_length = 1 + entries.len() * PAGE_RECIPE_ENTRY_BYTE_COUNT;
        ensure!(
            block_length <= MMC3_PAGE_BYTE_COUNT,
            "visible-page recipe {recipe_index} needs {block_length} bytes and cannot fit one MMC3 page"
        );
        let start = section_container_offset + blocks.len();
        if start / MMC3_PAGE_BYTE_COUNT != (start + block_length - 1) / MMC3_PAGE_BYTE_COUNT {
            let next_page = (start / MMC3_PAGE_BYTE_COUNT + 1) * MMC3_PAGE_BYTE_COUNT;
            blocks.resize(blocks.len() + (next_page - start), 0xFF);
        }
        offsets.push(
            u16::try_from(blocks.len())
                .context("visible-page recipe block offset exceeds a 16-bit offset")?,
        );
        blocks.push(
            u8::try_from(entries.len())
                .context("visible-page recipe entry count does not fit u8")?,
        );
        for (code, address) in entries {
            blocks.push(code);
            blocks.extend_from_slice(&address.to_le_bytes());
        }
    }
    Ok((offsets, blocks))
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
    fn visible_page_recipe_keeps_every_runtime_lifetime_glyph() {
        let worksets = [GlyphWorkset {
            target_glyphs: BTreeSet::from(['대', '사', '시', '더', '장']),
            preserved_active_codes: BTreeSet::new(),
            fixed_glyph_codes: BTreeMap::new(),
        }];
        let assignments = [BTreeMap::from([
            ('대', 1),
            ('사', 2),
            ('시', 3),
            ('더', 4),
            ('장', 5),
            // 같은 정적 그룹의 다른 화면 글자는 현재 레시피에 들어오지 않는다.
            ('외', 6),
        ])];

        let recipes = build_visible_page_recipes(&worksets, &[0], &assignments).unwrap();

        assert_eq!(recipes.len(), 1);
        assert_eq!(recipes[0].target_glyphs, vec!['대', '더', '사', '시', '장']);
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

    /// 레시피는 그 가시 페이지가 쓰는 코드만 담아야 한다. 같은 그룹의 다른 페이지
    /// 글자를 함께 올리면 첫 표시가 수백 프레임 늦어지고, 빠뜨리면 화면이 깨진다.
    #[test]
    fn every_visible_page_block_holds_exactly_its_target_codes() {
        let assignments = vec![
            BTreeMap::from([('가', 0x42u8), ('나', 0x10)]),
            BTreeMap::from([('다', 0x80u8)]),
        ];
        let atlas = BTreeMap::from([('가', 0usize), ('나', 1), ('다', 2)]);
        let recipes = vec![
            VisiblePageRecipe {
                static_page_group_index: 0,
                target_glyphs: vec!['가'],
            },
            VisiblePageRecipe {
                static_page_group_index: 1,
                target_glyphs: vec!['다'],
            },
        ];

        let (offsets, blocks) =
            encode_visible_page_recipe_blocks(&recipes, &assignments, &atlas, 0x802E, 0).unwrap();

        assert_eq!(offsets.len(), recipes.len());
        for (recipe_index, expected) in [vec![0x42], vec![0x80]].iter().enumerate() {
            let start = usize::from(offsets[recipe_index]);
            let count = usize::from(blocks[start]);
            let codes: Vec<u8> = (0..count)
                .map(|entry| blocks[start + 1 + entry * PAGE_RECIPE_ENTRY_BYTE_COUNT])
                .collect();
            assert_eq!(&codes, expected);
        }
    }

    /// 항목이 담는 주소는 그 글자의 atlas 타일을 정확히 가리켜야 한다.
    /// 소비자는 이 주소를 그대로 포인터에 넣고 여덟 바이트를 읽는다.
    #[test]
    fn an_entry_addresses_the_atlas_tile_of_its_glyph() {
        let assignments = vec![BTreeMap::from([('가', 0x42u8)])];
        let atlas = BTreeMap::from([('가', 7usize)]);
        let recipes = vec![VisiblePageRecipe {
            static_page_group_index: 0,
            target_glyphs: vec!['가'],
        }];

        let (_, blocks) =
            encode_visible_page_recipe_blocks(&recipes, &assignments, &atlas, 0x802E, 0).unwrap();

        let address = u16::from_le_bytes([blocks[2], blocks[3]]);
        assert_eq!(address, 0x802E + 7 * GLYPH_ATLAS_TILE_BYTE_COUNT as u16);
    }

    /// 소비자는 한 타일에 목록과 atlas를 읽고 뱅크를 한 번 왕복한다. 덩이가 페이지
    /// 경계를 걸치면 왕복이 하나 더 붙으므로, 걸칠 자리에서는 다음 페이지로 민다.
    #[test]
    fn a_block_that_would_straddle_a_page_boundary_moves_to_the_next_page() {
        let assignments = vec![BTreeMap::from([('가', 1u8), ('나', 2u8)])];
        let atlas = BTreeMap::from([('가', 0usize), ('나', 1)]);
        let recipes = vec![VisiblePageRecipe {
            static_page_group_index: 0,
            target_glyphs: vec!['가', '나'],
        }];
        // 덩이가 일곱 바이트이므로 페이지 끝 세 바이트 앞에서는 걸친다.
        let section_offset = MMC3_PAGE_BYTE_COUNT - 3;

        let (offsets, blocks) = encode_visible_page_recipe_blocks(
            &recipes,
            &assignments,
            &atlas,
            0x802E,
            section_offset,
        )
        .unwrap();

        let start = section_offset + usize::from(offsets[0]);
        assert_eq!(start % MMC3_PAGE_BYTE_COUNT, 0);
        assert!(blocks[..3].iter().all(|byte| *byte == 0xFF));
    }

    #[test]
    fn recipe_index_covers_every_workset_and_exact_block_start() {
        let references = [0u8, 0, 4, 0, 0, 0];
        let directory = [0u8, 0, 2, 0, 3, 0];
        let offsets = [0u16, 4];
        let blocks = [1u8, 0x42, 0x2E, 0x80, 1, 0x43, 0x36, 0x80];

        verify_visible_page_recipe_material(&references, &directory, &offsets, &blocks, &[2, 1])
            .unwrap();
    }

    #[test]
    fn recipe_index_rejects_an_interior_reference_or_missing_block() {
        let directory = [0u8, 0, 2, 0];
        let offsets = [0u16, 4];
        let blocks = [1u8, 0x42, 0x2E, 0x80, 1, 0x43, 0x36, 0x80];

        assert!(
            verify_visible_page_recipe_material(
                &[0, 0, 5, 0],
                &directory,
                &offsets,
                &blocks,
                &[2],
            )
            .is_err()
        );
        assert!(
            verify_visible_page_recipe_material(
                &[0, 0, 0, 0],
                &directory,
                &offsets,
                &blocks,
                &[2],
            )
            .is_err()
        );
    }

    #[test]
    fn recipe_index_rejects_a_record_directory_gap() {
        let references = [0u8, 0, 4, 0];
        let offsets = [0u16, 4];
        let blocks = [1u8, 0x42, 0x2E, 0x80, 1, 0x43, 0x36, 0x80];

        assert!(
            verify_visible_page_recipe_material(
                &references,
                &[0, 0, 1, 0, 3, 0],
                &offsets,
                &blocks,
                &[2, 0],
            )
            .is_err()
        );
    }
}
