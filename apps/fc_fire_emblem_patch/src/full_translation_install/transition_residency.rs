//! 한 화면에 남아 있을 수 있는 주 대사 페이지들이 같은 글리프 코드를 쓰게 한다.
//!
//! 원본 상태 10은 논리적으로 다음 레코드로 넘어가는 경계지만 PPU 네임테이블을
//! 지우는 경계는 아니다. 따라서 그 시점에 다른 코드북을 올리면 이전 대사의 타일
//! 번호가 새 글리프로 재해석된다. 한 레코드의 연속 페이지와 명시적 E4/E6 전이
//! 사슬 전체를 하나의 코드북 수명으로 묶어 이 타이밍 의존성을 없앤다.

use std::collections::{BTreeMap, BTreeSet};

use anyhow::{Context, Result, ensure};

use crate::{
    dialogue_assets::MainDialogueDisplayPlan, dialogue_inventory::MainDialogueGraphReport,
    font_slots::ACTIVE_HANGUL_SLOT_COUNT, mapper165::battle_codebook_plan::GlyphWorkset,
};

pub(super) struct TransitionResidencyPlan {
    pub(super) augmented_worksets: Vec<GlyphWorkset>,
    pub(super) lifetime_count: usize,
    pub(super) multi_record_lifetime_count: usize,
    pub(super) maximum_lifetime_record_count: usize,
    pub(super) maximum_lifetime_workset_count: usize,
    pub(super) maximum_lifetime_slot_demand: usize,
}

pub(super) struct TransitionLifetimeWorksets {
    pub(super) record_indices: Vec<usize>,
    pub(super) workset_indices: Vec<usize>,
}

pub(super) fn plan_transition_residency(
    display: &MainDialogueDisplayPlan,
    graph: &MainDialogueGraphReport,
    worksets: &[GlyphWorkset],
) -> Result<TransitionResidencyPlan> {
    ensure!(
        display.page_worksets.len() == worksets.len(),
        "dialogue transition residency lost visible page worksets"
    );

    let lifetimes = bind_transition_lifetime_worksets(display, graph)?;
    apply_transition_residency(display, worksets, &lifetimes)
}

pub(super) fn bind_transition_lifetime_worksets(
    display: &MainDialogueDisplayPlan,
    graph: &MainDialogueGraphReport,
) -> Result<Vec<TransitionLifetimeWorksets>> {
    let record_indices = display
        .record_ids
        .iter()
        .enumerate()
        .map(|(index, record_id)| (record_id.as_str(), index))
        .collect::<BTreeMap<_, _>>();
    ensure!(
        record_indices.len() == display.record_ids.len(),
        "dialogue transition residency has duplicate record IDs"
    );

    let edges = graph
        .transition_edges
        .iter()
        .map(|edge| {
            (
                format!(
                    "{}:{:03}",
                    edge.source_table_id, edge.source_canonical_entry_index
                ),
                format!(
                    "{}:{:03}",
                    edge.target_table_id, edge.target_canonical_entry_index
                ),
            )
        })
        .collect::<Vec<_>>();
    let edge_refs = edges
        .iter()
        .map(|(source, target)| (source.as_str(), target.as_str()))
        .collect::<Vec<_>>();
    build_transition_lifetime_worksets(display, &record_indices, &edge_refs)
}

#[cfg(test)]
fn build_transition_residency(
    display: &MainDialogueDisplayPlan,
    worksets: &[GlyphWorkset],
    record_indices: &BTreeMap<&str, usize>,
    edges: &[(&str, &str)],
) -> Result<TransitionResidencyPlan> {
    let lifetimes = build_transition_lifetime_worksets(display, record_indices, edges)?;
    apply_transition_residency(display, worksets, &lifetimes)
}

fn build_transition_lifetime_worksets(
    display: &MainDialogueDisplayPlan,
    record_indices: &BTreeMap<&str, usize>,
    edges: &[(&str, &str)],
) -> Result<Vec<TransitionLifetimeWorksets>> {
    let mut parents = (0..record_indices.len()).collect::<Vec<_>>();
    for (source, target) in edges {
        let source_index = *record_indices
            .get(source)
            .with_context(|| format!("dialogue transition source {source} is missing"))?;
        let target_index = *record_indices
            .get(target)
            .with_context(|| format!("dialogue transition target {target} is missing"))?;
        union(&mut parents, source_index, target_index);
    }

    let mut worksets_by_record = vec![Vec::new(); record_indices.len()];
    for (workset_index, page) in display.page_worksets.iter().enumerate() {
        let record_index = *record_indices
            .get(page.record_id.as_str())
            .with_context(|| format!("dialogue page record {} is missing", page.record_id))?;
        worksets_by_record[record_index].push(workset_index);
    }
    ensure!(
        worksets_by_record.iter().all(|indices| !indices.is_empty()),
        "dialogue transition residency has a record without a visible page"
    );

    let mut records_by_root = BTreeMap::<usize, Vec<usize>>::new();
    for record_index in 0..record_indices.len() {
        let root = find(&mut parents, record_index);
        records_by_root.entry(root).or_default().push(record_index);
    }

    Ok(records_by_root
        .into_values()
        .map(|record_indices| {
            let workset_indices = record_indices
                .iter()
                .flat_map(|record_index| worksets_by_record[*record_index].iter().copied())
                .collect();
            TransitionLifetimeWorksets {
                record_indices,
                workset_indices,
            }
        })
        .collect())
}

fn apply_transition_residency(
    display: &MainDialogueDisplayPlan,
    worksets: &[GlyphWorkset],
    lifetimes: &[TransitionLifetimeWorksets],
) -> Result<TransitionResidencyPlan> {
    let mut augmented_worksets = worksets.to_vec();
    let mut multi_record_lifetime_count = 0;
    let mut maximum_lifetime_record_count = 0;
    let mut maximum_lifetime_workset_count = 0;
    let mut maximum_lifetime_slot_demand = 0;
    for lifetime in lifetimes {
        if lifetime.record_indices.len() > 1 {
            multi_record_lifetime_count += 1;
        }
        maximum_lifetime_record_count =
            maximum_lifetime_record_count.max(lifetime.record_indices.len());

        maximum_lifetime_workset_count =
            maximum_lifetime_workset_count.max(lifetime.workset_indices.len());
        let lifetime_record_ids = lifetime
            .record_indices
            .iter()
            .map(|record_index| display.record_ids[*record_index].as_str())
            .collect::<Vec<_>>();
        let merged = merge_worksets(
            lifetime
                .workset_indices
                .iter()
                .map(|workset_index| &worksets[*workset_index]),
        )
        .with_context(|| {
            format!("merge dialogue transition lifetime for records {lifetime_record_ids:?}")
        })?;
        let slot_demand = merged.target_glyphs.len() + merged.preserved_active_codes.len();
        ensure!(
            slot_demand <= ACTIVE_HANGUL_SLOT_COUNT,
            "dialogue transition lifetime {lifetime_record_ids:?} needs {slot_demand} active slots ({} target glyphs plus {} preserved codes) but only {ACTIVE_HANGUL_SLOT_COUNT} exist",
            merged.target_glyphs.len(),
            merged.preserved_active_codes.len(),
        );
        maximum_lifetime_slot_demand = maximum_lifetime_slot_demand.max(slot_demand);
        for workset_index in &lifetime.workset_indices {
            augmented_worksets[*workset_index] = merged.clone();
        }
    }

    Ok(TransitionResidencyPlan {
        augmented_worksets,
        lifetime_count: lifetimes.len(),
        multi_record_lifetime_count,
        maximum_lifetime_record_count,
        maximum_lifetime_workset_count,
        maximum_lifetime_slot_demand,
    })
}

fn merge_worksets<'a>(worksets: impl Iterator<Item = &'a GlyphWorkset>) -> Result<GlyphWorkset> {
    let mut target_glyphs = BTreeSet::new();
    let mut preserved_active_codes = BTreeSet::new();
    let mut fixed_glyph_codes = BTreeMap::new();
    let mut glyph_by_fixed_code = BTreeMap::new();
    for workset in worksets {
        target_glyphs.extend(workset.target_glyphs.iter().copied());
        preserved_active_codes.extend(workset.preserved_active_codes.iter().copied());
        for (glyph, code) in &workset.fixed_glyph_codes {
            if let Some(previous) = fixed_glyph_codes.insert(*glyph, *code) {
                ensure!(
                    previous == *code,
                    "dialogue transition lifetime assigns two codes to glyph {glyph:?}"
                );
            }
            if let Some(previous_glyph) = glyph_by_fixed_code.insert(*code, *glyph) {
                ensure!(
                    previous_glyph == *glyph,
                    "dialogue transition lifetime assigns code {code:02X} to both {previous_glyph:?} and {glyph:?}"
                );
            }
        }
    }
    ensure!(
        fixed_glyph_codes
            .keys()
            .all(|glyph| target_glyphs.contains(glyph)),
        "dialogue transition lifetime fixes a code outside its glyph set"
    );
    let preserved_collisions = fixed_glyph_codes
        .iter()
        .filter(|(_, code)| preserved_active_codes.contains(code))
        .map(|(glyph, code)| (*glyph, *code))
        .collect::<Vec<_>>();
    ensure!(
        preserved_collisions.is_empty(),
        "dialogue transition lifetime fixes codes preserved by another visible page: {preserved_collisions:?}"
    );
    Ok(GlyphWorkset {
        target_glyphs,
        preserved_active_codes,
        fixed_glyph_codes,
    })
}

fn find(parents: &mut [usize], index: usize) -> usize {
    if parents[index] != index {
        parents[index] = find(parents, parents[index]);
    }
    parents[index]
}

fn union(parents: &mut [usize], left: usize, right: usize) {
    let left_root = find(parents, left);
    let right_root = find(parents, right);
    if left_root != right_root {
        parents[right_root] = left_root;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::dialogue_assets::MainDialoguePageWorkset;

    fn page(record_id: &str, page_index: usize, glyphs: &str) -> MainDialoguePageWorkset {
        MainDialoguePageWorkset {
            record_id: record_id.to_owned(),
            page_index,
            target_glyphs: glyphs.chars().collect(),
            visible_line_target_glyphs: Vec::new(),
            dynamic_string_selectors: BTreeSet::new(),
            dynamic_string_selector_counts: BTreeMap::new(),
            dynamic_string_control_count: 0,
            source_reclaimable_active_codes: BTreeSet::new(),
            preserved_target_active_codes: BTreeSet::new(),
        }
    }

    fn workset(glyphs: &str) -> GlyphWorkset {
        GlyphWorkset {
            target_glyphs: glyphs.chars().collect(),
            preserved_active_codes: BTreeSet::new(),
            fixed_glyph_codes: BTreeMap::new(),
        }
    }

    #[test]
    fn sequential_pages_and_explicit_record_transitions_share_one_demand() {
        let display = MainDialogueDisplayPlan {
            canonical_record_count: 2,
            page_worksets: vec![
                page("table:000", 0, "가"),
                page("table:000", 1, "나"),
                page("table:001", 0, "다"),
            ],
            record_ids: vec!["table:000".to_owned(), "table:001".to_owned()],
        };
        let record_indices = BTreeMap::from([("table:000", 0), ("table:001", 1)]);
        let plan = build_transition_residency(
            &display,
            &[workset("가"), workset("나"), workset("다")],
            &record_indices,
            &[("table:000", "table:001")],
        )
        .unwrap();
        let expected = "가나다".chars().collect::<BTreeSet<_>>();

        assert!(
            plan.augmented_worksets
                .iter()
                .all(|workset| workset.target_glyphs == expected)
        );
        assert_eq!(plan.lifetime_count, 1);
        assert_eq!(plan.multi_record_lifetime_count, 1);
        assert_eq!(plan.maximum_lifetime_record_count, 2);
        assert_eq!(plan.maximum_lifetime_workset_count, 3);
        assert_eq!(plan.maximum_lifetime_slot_demand, 3);
    }

    #[test]
    fn one_code_cannot_mean_two_glyphs_in_a_visible_transition_lifetime() {
        let left = GlyphWorkset {
            target_glyphs: BTreeSet::from(['가']),
            preserved_active_codes: BTreeSet::new(),
            fixed_glyph_codes: BTreeMap::from([('가', 0x40)]),
        };
        let right = GlyphWorkset {
            target_glyphs: BTreeSet::from(['나']),
            preserved_active_codes: BTreeSet::new(),
            fixed_glyph_codes: BTreeMap::from([('나', 0x40)]),
        };

        let error = match merge_worksets([&left, &right].into_iter()) {
            Ok(_) => panic!("conflicting fixed codes were accepted"),
            Err(error) => error,
        };
        assert!(error.to_string().contains("assigns code 40 to both"));
    }
}
