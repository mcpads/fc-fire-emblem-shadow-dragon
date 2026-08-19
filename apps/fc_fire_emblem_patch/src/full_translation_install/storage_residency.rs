//! 저장소 화면군의 대사와 그 위에 남는 고정 선택 라벨을 한 코드 배정으로 묶는다.
//!
//! 상태 1D와 23은 주 대사 페이지가 화면에 남아 있는 동안 bank 0B 고정 문자열을
//! 덧붙인다. 따라서 고정 문자열용 정적 페이지를 다시 고르면 대사 타일이 깨지고,
//! 반대로 대사 페이지에 선택 라벨의 코드가 없으면 라벨이 깨진다.
//!
//! 라벨만으로는 부족하다. 원본은 레코드가 바뀌어도 여섯 줄 버퍼를 비우지 않으므로,
//! 짧은 레코드는 앞선 레코드가 쓴 뒷줄을 화면에 그대로 남긴다. 그래서 이 단계는
//! 저장소·소지품 초과 두 상태기가 고르는 모든 페이지에 대해, 그 페이지가 쓰지 않는
//! 줄 슬롯에 같은 수명의 다른 페이지가 남길 수 있는 글자까지 같은 코드로 상주시킨다.
//! 레코드 전체 합집합은 배정 가능한 코드 수를 넘기지만 줄 슬롯 꼬리는 들어간다.

use std::collections::{BTreeMap, BTreeSet};

use anyhow::{Context, Result, ensure};
use serde::Serialize;

use crate::{
    dialogue_assets::MainDialogueDisplayPlan,
    dialogue_assets::MainDialoguePageWorkset,
    dialogue_inventory::{MainDialogueGraphReport, main_dialogue_transition_chain_record_ids},
    fixed_menu_labels::FIXED_MENU_LABEL_SPECS,
    font_slots::{ACTIVE_HANGUL_SLOT_COUNT, active_hangul_codes},
    mapper165::battle_codebook_plan::GlyphWorkset,
    rom::Rom,
    semantic_translation::SemanticTranslationPlan,
    text_inventory::FixedTextLogicalByte,
};

use super::resident_glyph_assignment::{assign_resident_glyph_codes, assignment_sha1};

mod source_binding;

use source_binding::bind_storage_dialogue_sources;

const DIALOGUE_TABLE_ID: &str = "shop-and-item-dialogue";
const STORAGE_DIALOGUE_LABEL_INDICES: [u8; 3] = [0x35, 0x36, 0x46];
const FACILITY_OVERLAY_LABEL_INDICES: [u8; 2] = [0x35, 0x36];
const OVERFLOW_OVERLAY_LABEL_INDICES: [u8; 2] = [0x35, 0x46];
const STANDALONE_CAPACITY_LABEL_INDEX: u8 = 0x47;

#[derive(Serialize)]
pub(super) struct StorageDialogueResidencyPlan {
    strategy: &'static str,
    dialogue_table_id: &'static str,
    dialogue_composite_states: [u8; 2],
    resident_fixed_label_indices: [u8; 3],
    standalone_static_label_index: u8,
    source_dispatch_count: usize,
    source_direct_record_store_count: usize,
    source_binding_sha1: String,
    source_selected_facility_record_count: usize,
    source_selected_overflow_record_count: usize,
    facility_overlay_record_ids: Vec<String>,
    overflow_overlay_record_ids: Vec<String>,
    overlay_record_ids: Vec<String>,
    resident_workset_count: usize,
    fixed_glyph_count: usize,
    fixed_code_count: usize,
    maximum_augmented_workset_slot_demand: usize,
    fixed_assignment_sha1: String,
    every_storage_label_glyph_uses_its_installed_code: bool,
    every_overlay_dialogue_page_contains_its_visible_storage_label_glyphs: bool,
    every_page_holds_the_line_slots_the_lifetime_can_leave_behind: bool,
    visible_lifetime_page_count: usize,
    storage_dialogue_does_not_reselect_the_static_menu_page: bool,
    capacity_notice_keeps_its_standalone_static_page: bool,
    #[serde(skip)]
    pub(super) augmented_worksets: Vec<GlyphWorkset>,
    #[serde(skip)]
    fixed_glyph_codes: BTreeMap<char, u8>,
}

impl StorageDialogueResidencyPlan {
    pub(super) fn owns_fixed_label(&self, index: u8) -> bool {
        STORAGE_DIALOGUE_LABEL_INDICES.contains(&index)
    }

    pub(super) fn encode_fixed_label(
        &self,
        index: u8,
        logical: &[FixedTextLogicalByte],
    ) -> Result<Vec<u8>> {
        ensure!(
            self.owns_fixed_label(index),
            "fixed label 0x{index:02X} is not owned by storage dialogue residency"
        );
        logical
            .iter()
            .map(|byte| match byte {
                FixedTextLogicalByte::Encoded(value) => Ok(*value),
                FixedTextLogicalByte::TargetGlyph(glyph) => self
                    .fixed_glyph_codes
                    .get(glyph)
                    .copied()
                    .with_context(|| format!("storage dialogue codebook lost {glyph:?}")),
            })
            .collect()
    }
}

pub(super) fn plan_storage_dialogue_residency(
    source: &Rom,
    graph: &MainDialogueGraphReport,
    display: &MainDialogueDisplayPlan,
    fixed_menu_labels: &SemanticTranslationPlan,
    dialogue_worksets: &[GlyphWorkset],
) -> Result<StorageDialogueResidencyPlan> {
    ensure!(
        display.page_worksets.len() == dialogue_worksets.len(),
        "storage dialogue residency lost visible dialogue worksets"
    );
    let source_binding = bind_storage_dialogue_sources(source)?;
    let facility_selected_record_ids = transition_record_ids(
        graph,
        &source_binding.facility_root_record_indices,
        "storage facility",
    )?;
    let overflow_selected_record_ids = transition_record_ids(
        graph,
        &source_binding.overflow_root_record_indices,
        "storage overflow",
    )?;
    ensure!(
        facility_selected_record_ids.is_disjoint(&overflow_selected_record_ids),
        "storage facility and overflow dialogue populations unexpectedly overlap"
    );
    ensure!(
        facility_selected_record_ids.len() + overflow_selected_record_ids.len() == 19,
        "storage dialogue state-machine population changed"
    );

    let facility_overlay_record_ids = transition_record_ids(
        graph,
        &BTreeSet::from([source_binding.facility_overlay_root_record_index]),
        "storage facility action-menu overlay",
    )?;
    let overflow_overlay_record_ids = transition_record_ids(
        graph,
        &BTreeSet::from([source_binding.overflow_overlay_root_record_index]),
        "storage overflow action-menu overlay",
    )?;
    ensure!(
        facility_overlay_record_ids.is_disjoint(&overflow_overlay_record_ids),
        "storage action-menu dialogue populations unexpectedly overlap"
    );
    ensure!(
        facility_overlay_record_ids.is_subset(&facility_selected_record_ids)
            && overflow_overlay_record_ids.is_subset(&overflow_selected_record_ids),
        "storage overlay dialogue escaped its source-selected state machine"
    );
    let overlay_record_ids = facility_overlay_record_ids
        .union(&overflow_overlay_record_ids)
        .cloned()
        .collect::<BTreeSet<_>>();
    ensure!(
        !overlay_record_ids.is_empty(),
        "storage action-menu dialogue population is empty"
    );

    let label_glyphs = storage_label_glyphs_by_index(fixed_menu_labels)?;
    let required_glyphs_by_record_id = overlay_glyph_requirements(
        &facility_overlay_record_ids,
        &overflow_overlay_record_ids,
        &label_glyphs,
    )?;
    let mut required_glyphs_by_workset =
        collect_required_workset_glyphs(display, &required_glyphs_by_record_id)?;

    let visible_lifetime_record_ids = facility_selected_record_ids
        .union(&overflow_selected_record_ids)
        .cloned()
        .collect::<BTreeSet<_>>();
    let lifetime_workset_indices = display
        .page_worksets
        .iter()
        .enumerate()
        .filter(|(_, page)| visible_lifetime_record_ids.contains(&page.record_id))
        .map(|(index, _)| index)
        .collect::<Vec<_>>();
    let lifetime_pages = lifetime_workset_indices
        .iter()
        .map(|index| &display.page_worksets[*index])
        .collect::<Vec<_>>();
    for (index, tail) in lifetime_workset_indices
        .iter()
        .zip(line_tail_requirements(&lifetime_pages))
    {
        required_glyphs_by_workset
            .entry(*index)
            .or_default()
            .extend(tail);
    }

    let fixed_glyph_codes =
        assign_storage_label_codes(dialogue_worksets, &required_glyphs_by_workset)?;
    let (augmented_worksets, resident_workset_count, maximum_augmented_workset_slot_demand) =
        augment_storage_worksets(
            display,
            dialogue_worksets,
            &required_glyphs_by_workset,
            &fixed_glyph_codes,
        )?;

    Ok(StorageDialogueResidencyPlan {
        strategy: "give every page of the two storage state machines one compatible code assignment: the overlaid action labels where they are displayed, plus the line slots a shorter page leaves for a longer page of the same lifetime, because the source never clears the six line buffers between records",
        dialogue_table_id: DIALOGUE_TABLE_ID,
        dialogue_composite_states: [0x1D, 0x23],
        resident_fixed_label_indices: STORAGE_DIALOGUE_LABEL_INDICES,
        standalone_static_label_index: STANDALONE_CAPACITY_LABEL_INDEX,
        source_dispatch_count: source_binding.source_dispatch_count,
        source_direct_record_store_count: source_binding.source_direct_record_store_count,
        source_binding_sha1: source_binding.source_binding_sha1,
        source_selected_facility_record_count: facility_selected_record_ids.len(),
        source_selected_overflow_record_count: overflow_selected_record_ids.len(),
        facility_overlay_record_ids: facility_overlay_record_ids.into_iter().collect(),
        overflow_overlay_record_ids: overflow_overlay_record_ids.into_iter().collect(),
        overlay_record_ids: overlay_record_ids.into_iter().collect(),
        resident_workset_count,
        fixed_glyph_count: fixed_glyph_codes.len(),
        fixed_code_count: fixed_glyph_codes.len(),
        maximum_augmented_workset_slot_demand,
        fixed_assignment_sha1: assignment_sha1(&fixed_glyph_codes),
        every_storage_label_glyph_uses_its_installed_code: true,
        every_overlay_dialogue_page_contains_its_visible_storage_label_glyphs: true,
        every_page_holds_the_line_slots_the_lifetime_can_leave_behind: true,
        visible_lifetime_page_count: lifetime_workset_indices.len(),
        storage_dialogue_does_not_reselect_the_static_menu_page: true,
        capacity_notice_keeps_its_standalone_static_page: true,
        augmented_worksets,
        fixed_glyph_codes,
    })
}

/// 줄 버퍼는 레코드가 바뀌어도 비워지지 않는다. 따라서 어떤 페이지가 쓰지 않는
/// 줄 슬롯에는 같은 수명의 다른 페이지가 남긴 글자가 그대로 보인다. 그 페이지의
/// 코드북은 남은 글자도 같은 뜻으로 담아야 한다.
fn line_tail_requirements(worksets: &[&MainDialoguePageWorkset]) -> Vec<BTreeSet<char>> {
    let slot_count = worksets
        .iter()
        .map(|workset| workset.visible_line_target_glyphs.len())
        .max()
        .unwrap_or_default();
    let mut glyphs_by_slot = vec![BTreeSet::new(); slot_count];
    for workset in worksets {
        for (slot, glyphs) in workset.visible_line_target_glyphs.iter().enumerate() {
            glyphs_by_slot[slot].extend(glyphs.iter().copied());
        }
    }

    worksets
        .iter()
        .map(|workset| {
            glyphs_by_slot
                .iter()
                .skip(workset.visible_line_target_glyphs.len())
                .flat_map(|glyphs| glyphs.iter().copied())
                .collect()
        })
        .collect()
}

fn transition_record_ids(
    graph: &MainDialogueGraphReport,
    roots: &BTreeSet<usize>,
    role: &str,
) -> Result<BTreeSet<String>> {
    let mut record_ids = BTreeSet::new();
    for root in roots {
        let chain = main_dialogue_transition_chain_record_ids(graph, DIALOGUE_TABLE_ID, *root)
            .with_context(|| format!("bind {role} dialogue transition chain at record {root}"))?;
        ensure!(
            !chain.is_empty(),
            "{role} dialogue transition chain is empty"
        );
        record_ids.extend(chain);
    }
    Ok(record_ids)
}

fn storage_label_glyphs_by_index(
    fixed_menu_labels: &SemanticTranslationPlan,
) -> Result<BTreeMap<u8, BTreeSet<char>>> {
    let requested = STORAGE_DIALOGUE_LABEL_INDICES
        .into_iter()
        .collect::<BTreeSet<_>>();
    let specs = FIXED_MENU_LABEL_SPECS
        .iter()
        .filter(|spec| requested.contains(&spec.index))
        .collect::<Vec<_>>();
    ensure!(
        specs.len() == STORAGE_DIALOGUE_LABEL_INDICES.len(),
        "storage dialogue fixed-label population changed"
    );

    let mut glyphs_by_index = BTreeMap::new();
    for spec in specs {
        let id = format!("fixed-menu-label:{:02X}", spec.index);
        let logical = fixed_menu_labels
            .entry_logical_bytes(&id)
            .with_context(|| format!("fixed-menu translation lost {id}"))?;
        let glyphs = logical
            .iter()
            .filter_map(|byte| match byte {
                FixedTextLogicalByte::TargetGlyph(glyph) => Some(*glyph),
                FixedTextLogicalByte::Encoded(_) => None,
            })
            .collect::<BTreeSet<_>>();
        ensure!(
            !glyphs.is_empty(),
            "storage dialogue label {id} has no target glyphs"
        );
        ensure!(
            glyphs_by_index.insert(spec.index, glyphs).is_none(),
            "duplicate storage dialogue label index 0x{:02X}",
            spec.index
        );
    }
    ensure!(
        glyphs_by_index.len() == STORAGE_DIALOGUE_LABEL_INDICES.len(),
        "storage dialogue label glyph population changed"
    );
    Ok(glyphs_by_index)
}

fn overlay_glyph_requirements(
    facility_record_ids: &BTreeSet<String>,
    overflow_record_ids: &BTreeSet<String>,
    label_glyphs: &BTreeMap<u8, BTreeSet<char>>,
) -> Result<BTreeMap<String, BTreeSet<char>>> {
    let glyphs_for = |indices: &[u8]| -> Result<BTreeSet<char>> {
        let mut glyphs = BTreeSet::new();
        for index in indices {
            glyphs.extend(
                label_glyphs
                    .get(index)
                    .with_context(|| format!("storage label 0x{index:02X} lost its glyphs"))?,
            );
        }
        Ok(glyphs)
    };
    let facility_glyphs = glyphs_for(&FACILITY_OVERLAY_LABEL_INDICES)?;
    let overflow_glyphs = glyphs_for(&OVERFLOW_OVERLAY_LABEL_INDICES)?;
    let mut requirements = BTreeMap::new();
    for record_id in facility_record_ids {
        requirements.insert(record_id.clone(), facility_glyphs.clone());
    }
    for record_id in overflow_record_ids {
        ensure!(
            requirements
                .insert(record_id.clone(), overflow_glyphs.clone())
                .is_none(),
            "storage overlay record {record_id} has two label populations"
        );
    }
    Ok(requirements)
}

fn collect_required_workset_glyphs(
    display: &MainDialogueDisplayPlan,
    required_glyphs_by_record_id: &BTreeMap<String, BTreeSet<char>>,
) -> Result<BTreeMap<usize, BTreeSet<char>>> {
    let mut found_record_ids = BTreeSet::new();
    let requirements = display
        .page_worksets
        .iter()
        .enumerate()
        .filter_map(|(index, page)| {
            required_glyphs_by_record_id
                .get(&page.record_id)
                .map(|glyphs| {
                    found_record_ids.insert(page.record_id.clone());
                    (index, glyphs.clone())
                })
        })
        .collect::<BTreeMap<_, _>>();
    let expected_record_ids = required_glyphs_by_record_id
        .keys()
        .cloned()
        .collect::<BTreeSet<_>>();
    ensure!(
        found_record_ids == expected_record_ids,
        "storage dialogue residency is missing visible records: expected {expected_record_ids:?}, found {found_record_ids:?}"
    );
    Ok(requirements)
}

fn assign_storage_label_codes(
    dialogue_worksets: &[GlyphWorkset],
    required_glyphs_by_workset: &BTreeMap<usize, BTreeSet<char>>,
) -> Result<BTreeMap<char, u8>> {
    let fixed_glyphs = required_glyphs_by_workset
        .values()
        .flat_map(|glyphs| glyphs.iter().copied())
        .collect::<BTreeSet<_>>();
    // 전이 수명이 이 페이지들의 작업집합을 하나로 합치므로, 한 페이지가 보존하는
    // 코드는 그 글자를 요구하지 않는 페이지에서도 배정할 수 없다.
    let mut lifetime_preserved_codes = BTreeSet::new();
    for workset_index in required_glyphs_by_workset.keys() {
        let workset = dialogue_worksets
            .get(*workset_index)
            .context("storage dialogue workset index is outside the workset population")?;
        lifetime_preserved_codes.extend(workset.preserved_active_codes.iter().copied());
    }

    let mut forbidden_codes_by_glyph = fixed_glyphs
        .iter()
        .copied()
        .map(|glyph| (glyph, lifetime_preserved_codes.clone()))
        .collect::<BTreeMap<_, _>>();
    let mut preassigned_codes_by_glyph = BTreeMap::<char, BTreeSet<u8>>::new();
    for (workset_index, required_glyphs) in required_glyphs_by_workset {
        let workset = dialogue_worksets
            .get(*workset_index)
            .context("storage dialogue workset index is outside the workset population")?;
        for glyph in required_glyphs {
            for (fixed_glyph, fixed_code) in &workset.fixed_glyph_codes {
                if fixed_glyph == glyph {
                    preassigned_codes_by_glyph
                        .entry(*glyph)
                        .or_default()
                        .insert(*fixed_code);
                } else {
                    forbidden_codes_by_glyph
                        .get_mut(glyph)
                        .expect("storage glyph was initialized")
                        .insert(*fixed_code);
                }
            }
        }
    }

    assign_resident_glyph_codes(
        "storage dialogue fixed-label residency",
        &forbidden_codes_by_glyph,
        &preassigned_codes_by_glyph,
        &active_hangul_codes().into_iter().collect(),
    )
}

fn augment_storage_worksets(
    display: &MainDialogueDisplayPlan,
    dialogue_worksets: &[GlyphWorkset],
    required_glyphs_by_workset: &BTreeMap<usize, BTreeSet<char>>,
    fixed_glyph_codes: &BTreeMap<char, u8>,
) -> Result<(Vec<GlyphWorkset>, usize, usize)> {
    ensure!(
        !required_glyphs_by_workset.is_empty() && !fixed_glyph_codes.is_empty(),
        "storage dialogue residency has an empty page or glyph population"
    );
    let mut found_workset_indices = BTreeSet::new();
    let mut resident_workset_count = 0;
    let mut maximum_augmented_workset_slot_demand = 0;
    let mut augmented_worksets = dialogue_worksets.to_vec();

    for (index, (page, workset)) in display
        .page_worksets
        .iter()
        .zip(&mut augmented_worksets)
        .enumerate()
    {
        if let Some(required_glyphs) = required_glyphs_by_workset.get(&index) {
            found_workset_indices.insert(index);
            resident_workset_count += 1;
            for glyph in required_glyphs {
                let code = fixed_glyph_codes
                    .get(glyph)
                    .with_context(|| format!("storage overlay lost code for {glyph:?}"))?;
                ensure!(
                    !workset.preserved_active_codes.contains(code),
                    "storage dialogue {} page {} preserves code 0x{code:02X} needed by {glyph:?}",
                    page.record_id,
                    page.page_index,
                );
                ensure!(
                    workset
                        .fixed_glyph_codes
                        .iter()
                        .all(|(existing_glyph, existing_code)| {
                            existing_glyph == glyph || existing_code != code
                        }),
                    "storage dialogue {} page {} already assigns code 0x{code:02X} to another glyph",
                    page.record_id,
                    page.page_index,
                );
                workset.target_glyphs.insert(*glyph);
                if let Some(previous) = workset.fixed_glyph_codes.insert(*glyph, *code) {
                    ensure!(
                        previous == *code,
                        "storage dialogue {} page {} changes fixed code for {glyph:?}",
                        page.record_id,
                        page.page_index,
                    );
                }
            }
        }
        maximum_augmented_workset_slot_demand = maximum_augmented_workset_slot_demand
            .max(workset.target_glyphs.len() + workset.preserved_active_codes.len());
    }
    ensure!(
        found_workset_indices == required_glyphs_by_workset.keys().copied().collect(),
        "storage dialogue residency is missing visible pages"
    );
    ensure!(
        resident_workset_count == required_glyphs_by_workset.len(),
        "storage dialogue residency lost a visible page"
    );
    ensure!(
        maximum_augmented_workset_slot_demand <= ACTIVE_HANGUL_SLOT_COUNT,
        "storage dialogue page needs {maximum_augmented_workset_slot_demand} active slots but only {ACTIVE_HANGUL_SLOT_COUNT} exist"
    );

    Ok((
        augmented_worksets,
        resident_workset_count,
        maximum_augmented_workset_slot_demand,
    ))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::dialogue_assets::MainDialoguePageWorkset;

    fn page(record_id: &str, page_index: usize) -> MainDialoguePageWorkset {
        MainDialoguePageWorkset {
            record_id: record_id.to_owned(),
            page_index,
            target_glyphs: BTreeSet::new(),
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

    fn lined_page(record_id: &str, lines: &[&str]) -> MainDialoguePageWorkset {
        let mut workset = page(record_id, 0);
        workset.visible_line_target_glyphs =
            lines.iter().map(|line| line.chars().collect()).collect();
        workset.target_glyphs = lines.iter().flat_map(|line| line.chars()).collect();
        workset
    }

    #[test]
    fn a_short_page_must_hold_the_lines_a_longer_page_leaves_behind() {
        let long = lined_page("shop-and-item-dialogue:041", &["가나", "다라"]);
        let short = lined_page("shop-and-item-dialogue:006", &["마"]);

        let required = line_tail_requirements(&[&long, &short]);

        // 두 줄을 쓰는 페이지는 뒤에 남길 것이 없다.
        assert_eq!(required[0], BTreeSet::new());
        // 한 줄만 쓰는 페이지는 둘째 줄 슬롯에 남는 글자를 함께 담아야 한다.
        assert_eq!(required[1], BTreeSet::from(['다', '라']));
    }

    #[test]
    fn a_page_that_writes_every_line_slot_requires_no_tail() {
        let long = lined_page("shop-and-item-dialogue:041", &["가", "나", "다"]);
        let same = lined_page("shop-and-item-dialogue:042", &["라", "마", "바"]);

        let required = line_tail_requirements(&[&long, &same]);

        assert!(required.iter().all(BTreeSet::is_empty));
    }

    #[test]
    fn every_page_of_a_storage_record_gets_the_installed_label_codes() {
        let display = MainDialogueDisplayPlan {
            canonical_record_count: 2,
            page_worksets: vec![
                page("shop-and-item-dialogue:041", 0),
                page("shop-and-item-dialogue:041", 1),
                page("unrelated:000", 0),
            ],
            record_ids: vec![
                "shop-and-item-dialogue:041".to_owned(),
                "unrelated:000".to_owned(),
            ],
        };
        let worksets = vec![workset("가"), workset("나"), workset("다")];
        let requirements = BTreeMap::from([
            (0, BTreeSet::from(['보', '관'])),
            (1, BTreeSet::from(['보', '관'])),
        ]);
        let fixed = BTreeMap::from([('보', 0xA0), ('관', 0xA1)]);

        let (augmented, count, _) =
            augment_storage_worksets(&display, &worksets, &requirements, &fixed).unwrap();

        assert_eq!(count, 2);
        for workset in &augmented[..2] {
            assert_eq!(workset.fixed_glyph_codes.get(&'보'), Some(&0xA0));
            assert_eq!(workset.fixed_glyph_codes.get(&'관'), Some(&0xA1));
        }
        assert_eq!(augmented[2].target_glyphs, BTreeSet::from(['다']));
        assert!(augmented[2].fixed_glyph_codes.is_empty());
    }

    #[test]
    fn storage_assignment_avoids_codes_already_owned_by_dialogue_glyphs() {
        let mut first = workset("가");
        first.fixed_glyph_codes.insert('기', 0x87);
        let second = workset("나");
        let requirements = BTreeMap::from([
            (0, BTreeSet::from(['보', '관'])),
            (1, BTreeSet::from(['보'])),
        ]);

        let assigned = assign_storage_label_codes(&[first, second], &requirements).unwrap();

        assert_ne!(assigned.get(&'보'), Some(&0x87));
        assert_ne!(assigned.get(&'관'), Some(&0x87));
        assert_ne!(assigned.get(&'보'), assigned.get(&'관'));
    }

    #[test]
    fn assignment_avoids_every_code_any_page_of_the_lifetime_preserves() {
        // 전이 수명은 같은 수명의 페이지 작업집합을 하나로 합친다. 따라서 어떤
        // 페이지가 보존하는 코드는 그 글자를 요구하지 않는 페이지에서도 쓸 수 없다.
        let mut first = workset("가");
        let mut second = workset("나");
        second.preserved_active_codes.insert(0x00);
        first.preserved_active_codes.insert(0x01);

        let assignment = assign_storage_label_codes(
            &[first, second],
            &BTreeMap::from([(0, BTreeSet::from(['보'])), (1, BTreeSet::from(['관']))]),
        )
        .unwrap();

        assert!(
            !assignment
                .values()
                .any(|code| *code == 0x00 || *code == 0x01)
        );
    }

    #[test]
    fn preserved_dialogue_code_cannot_be_reused_for_a_storage_label() {
        let display = MainDialogueDisplayPlan {
            canonical_record_count: 1,
            page_worksets: vec![page("shop-and-item-dialogue:041", 0)],
            record_ids: vec!["shop-and-item-dialogue:041".to_owned()],
        };
        let mut workset = workset("가");
        workset.preserved_active_codes.insert(0xA0);

        let error = match augment_storage_worksets(
            &display,
            &[workset],
            &BTreeMap::from([(0, BTreeSet::from(['보']))]),
            &BTreeMap::from([('보', 0xA0)]),
        ) {
            Ok(_) => panic!("preserved code collision unexpectedly passed"),
            Err(error) => error,
        };

        assert!(error.to_string().contains("preserves code"));
    }

    #[test]
    fn facility_and_overflow_pages_receive_only_the_labels_they_display() {
        let display = MainDialogueDisplayPlan {
            canonical_record_count: 3,
            page_worksets: vec![
                page("shop-and-item-dialogue:041", 0),
                page("shop-and-item-dialogue:064", 0),
                page("shop-and-item-dialogue:065", 0),
            ],
            record_ids: vec![
                "shop-and-item-dialogue:041".to_owned(),
                "shop-and-item-dialogue:064".to_owned(),
                "shop-and-item-dialogue:065".to_owned(),
            ],
        };
        let requirements = BTreeMap::from([
            (0, BTreeSet::from(['보', '관', '찾', '기'])),
            (
                1,
                BTreeSet::from(['보', '관', '하', '나', '버', '리', '기']),
            ),
        ]);
        let fixed = BTreeMap::from([
            ('보', 0xA0),
            ('관', 0xA1),
            ('찾', 0xA2),
            ('기', 0xA3),
            ('하', 0xA4),
            ('나', 0xA5),
            ('버', 0xA6),
            ('리', 0xA7),
        ]);

        let (augmented, count, _) = augment_storage_worksets(
            &display,
            &[workset("가"), workset("나"), workset("다")],
            &requirements,
            &fixed,
        )
        .unwrap();

        assert_eq!(count, 2);
        assert!(augmented[0].fixed_glyph_codes.contains_key(&'찾'));
        assert!(!augmented[0].fixed_glyph_codes.contains_key(&'버'));
        assert!(augmented[1].fixed_glyph_codes.contains_key(&'버'));
        assert!(!augmented[1].fixed_glyph_codes.contains_key(&'찾'));
        assert!(augmented[2].fixed_glyph_codes.is_empty());
    }
}
