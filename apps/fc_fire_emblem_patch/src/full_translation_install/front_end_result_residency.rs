//! 복사·삭제 결과 대사 위에 남는 앞면 메뉴 글리프의 물리 코드를 보존한다.
//!
//! 결과 대사는 주 대사 CHR-RAM 페이지를 올리지만, 시작 메뉴와 기록 슬롯의 네임테이블
//! 셀은 지워지지 않는다. 누적 후보에 이미 저장된 메뉴 코드와 같은 글리프를 결과 대사
//! 페이지에도 같은 코드로 합성해 두 표면을 하나의 화면 수명으로 취급한다.

use std::collections::{BTreeMap, BTreeSet};

use anyhow::{Result, ensure};
use serde::Serialize;

use crate::{
    dialogue_assets::MainDialogueDisplayPlan,
    font_slots::{ACTIVE_HANGUL_SLOT_COUNT, active_hangul_codes},
    front_end_menu::{FRONT_END_RESULT_DIALOGUE_RECORD_IDS, FrontEndMenuPlan},
    mapper165::battle_codebook_plan::GlyphWorkset,
    rom::Rom,
};

use super::resident_glyph_assignment::assignment_sha1;

mod source_binding;

use source_binding::bind_front_end_result_routes;

#[derive(Serialize)]
pub(super) struct FrontEndResultResidencyPlan {
    strategy: &'static str,
    result_record_ids: [&'static str; 4],
    source_result_index_writer_count: usize,
    source_directory_writer_count: usize,
    source_route_binding_sha1: String,
    resident_workset_count: usize,
    retained_menu_glyph_count: usize,
    fixed_code_count: usize,
    maximum_augmented_workset_slot_demand: usize,
    fixed_assignment_sha1: String,
    every_result_page_contains_every_retained_menu_glyph: bool,
    installed_menu_storage_rebound: bool,
    #[serde(skip)]
    pub(super) augmented_worksets: Vec<GlyphWorkset>,
}

pub(super) fn plan_front_end_result_residency(
    source: &Rom,
    candidate: &Rom,
    display: &MainDialogueDisplayPlan,
    front_end: &FrontEndMenuPlan,
    dialogue_worksets: &[GlyphWorkset],
) -> Result<FrontEndResultResidencyPlan> {
    ensure!(
        display.page_worksets.len() == dialogue_worksets.len(),
        "front-end result residency lost dialogue page worksets"
    );
    let source_binding = bind_front_end_result_routes(source)?;
    let installed_menu_glyph_codes = front_end.bind_installed_glyph_codes(candidate.data())?;
    ensure!(
        !installed_menu_glyph_codes.is_empty(),
        "front-end result residency has no installed target glyphs"
    );

    let (augmented_worksets, resident_workset_indices, maximum_slot_demand) =
        augment_result_worksets(
            display,
            dialogue_worksets,
            &FRONT_END_RESULT_DIALOGUE_RECORD_IDS,
            &installed_menu_glyph_codes,
        )?;

    Ok(FrontEndResultResidencyPlan {
        strategy: "rebind the cumulative candidate's installed front-end glyph codes and overlay those exact glyphs into every source-bound copy, delete, and data-error result dialogue page",
        result_record_ids: FRONT_END_RESULT_DIALOGUE_RECORD_IDS,
        source_result_index_writer_count: source_binding.result_index_writer_count,
        source_directory_writer_count: source_binding.directory_writer_count,
        source_route_binding_sha1: source_binding.route_binding_sha1,
        resident_workset_count: resident_workset_indices.len(),
        retained_menu_glyph_count: installed_menu_glyph_codes.len(),
        fixed_code_count: installed_menu_glyph_codes.len(),
        maximum_augmented_workset_slot_demand: maximum_slot_demand,
        fixed_assignment_sha1: assignment_sha1(&installed_menu_glyph_codes),
        every_result_page_contains_every_retained_menu_glyph: true,
        installed_menu_storage_rebound: true,
        augmented_worksets,
    })
}

fn augment_result_worksets(
    display: &MainDialogueDisplayPlan,
    dialogue_worksets: &[GlyphWorkset],
    result_record_ids: &[&str],
    installed_menu_glyph_codes: &BTreeMap<char, u8>,
) -> Result<(Vec<GlyphWorkset>, BTreeSet<usize>, usize)> {
    ensure!(
        display.page_worksets.len() == dialogue_worksets.len(),
        "front-end result residency input lengths changed"
    );
    let result_record_ids = result_record_ids.iter().copied().collect::<BTreeSet<_>>();
    ensure!(
        result_record_ids.len() == FRONT_END_RESULT_DIALOGUE_RECORD_IDS.len(),
        "front-end result residency has duplicate record IDs"
    );
    let active_codes = active_hangul_codes().into_iter().collect::<BTreeSet<_>>();
    ensure!(
        installed_menu_glyph_codes
            .values()
            .all(|code| active_codes.contains(code)),
        "front-end result residency received a reserved menu glyph code"
    );

    let mut found_record_ids = BTreeSet::new();
    let mut resident_workset_indices = BTreeSet::new();
    for (index, page) in display.page_worksets.iter().enumerate() {
        if result_record_ids.contains(page.record_id.as_str()) {
            found_record_ids.insert(page.record_id.as_str());
            resident_workset_indices.insert(index);
        }
    }
    ensure!(
        found_record_ids == result_record_ids,
        "front-end result residency is missing a source-bound dialogue record"
    );

    let mut augmented_worksets = dialogue_worksets.to_vec();
    let mut maximum_slot_demand = 0;
    for (index, workset) in augmented_worksets.iter_mut().enumerate() {
        if resident_workset_indices.contains(&index) {
            for (glyph, code) in installed_menu_glyph_codes {
                ensure!(
                    !workset.preserved_active_codes.contains(code),
                    "front-end menu glyph {glyph:?} uses code {code:02X} preserved by its result dialogue page"
                );
                if let Some((other_glyph, _)) = workset
                    .fixed_glyph_codes
                    .iter()
                    .find(|(other_glyph, other_code)| *other_glyph != glyph && *other_code == code)
                {
                    anyhow::bail!(
                        "front-end menu code {code:02X} for {glyph:?} already means {other_glyph:?} on its result dialogue page"
                    );
                }
                if let Some(existing) = workset.fixed_glyph_codes.insert(*glyph, *code) {
                    ensure!(
                        existing == *code,
                        "front-end menu glyph {glyph:?} changes from fixed code {existing:02X} to {code:02X} on its result dialogue page"
                    );
                }
                workset.target_glyphs.insert(*glyph);
            }
        }
        let slot_demand = workset.target_glyphs.len() + workset.preserved_active_codes.len();
        maximum_slot_demand = maximum_slot_demand.max(slot_demand);
        ensure!(
            slot_demand <= ACTIVE_HANGUL_SLOT_COUNT,
            "front-end result dialogue page needs {slot_demand} active slots but only {ACTIVE_HANGUL_SLOT_COUNT} exist"
        );
    }

    for index in &resident_workset_indices {
        let workset = &augmented_worksets[*index];
        ensure!(
            installed_menu_glyph_codes.iter().all(|(glyph, code)| {
                workset.target_glyphs.contains(glyph)
                    && workset.fixed_glyph_codes.get(glyph) == Some(code)
            }),
            "front-end result residency lost an installed menu glyph assignment"
        );
    }

    Ok((
        augmented_worksets,
        resident_workset_indices,
        maximum_slot_demand,
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
            dynamic_string_selectors: BTreeSet::new(),
            dynamic_string_selector_counts: BTreeMap::new(),
            dynamic_string_control_count: 0,
            source_reclaimable_active_codes: BTreeSet::new(),
            preserved_target_active_codes: BTreeSet::new(),
        }
    }

    fn workset(glyph: char) -> GlyphWorkset {
        GlyphWorkset {
            target_glyphs: BTreeSet::from([glyph]),
            preserved_active_codes: BTreeSet::new(),
            fixed_glyph_codes: BTreeMap::new(),
        }
    }

    #[test]
    fn all_copy_delete_and_error_result_pages_keep_installed_menu_codes() {
        let mut pages = FRONT_END_RESULT_DIALOGUE_RECORD_IDS
            .iter()
            .enumerate()
            .map(|(index, id)| page(id, index))
            .collect::<Vec<_>>();
        pages.push(page("unrelated:000", 0));
        let display = MainDialogueDisplayPlan {
            canonical_record_count: pages.len(),
            record_ids: pages.iter().map(|page| page.record_id.clone()).collect(),
            page_worksets: pages,
        };
        let worksets = vec![
            workset('가'),
            workset('나'),
            workset('다'),
            workset('라'),
            workset('아'),
        ];
        let installed = BTreeMap::from([('기', 0x84), ('록', 0x85)]);

        let (augmented, resident, _) = augment_result_worksets(
            &display,
            &worksets,
            &FRONT_END_RESULT_DIALOGUE_RECORD_IDS,
            &installed,
        )
        .unwrap();

        assert_eq!(resident, BTreeSet::from([0, 1, 2, 3]));
        for workset in &augmented[..4] {
            assert_eq!(workset.fixed_glyph_codes.get(&'기'), Some(&0x84));
            assert_eq!(workset.fixed_glyph_codes.get(&'록'), Some(&0x85));
        }
        assert_eq!(augmented[4].target_glyphs, BTreeSet::from(['아']));
        assert!(augmented[4].fixed_glyph_codes.is_empty());
    }

    #[test]
    fn result_residency_rejects_a_code_that_already_means_another_glyph() {
        let display = MainDialogueDisplayPlan {
            canonical_record_count: FRONT_END_RESULT_DIALOGUE_RECORD_IDS.len(),
            record_ids: FRONT_END_RESULT_DIALOGUE_RECORD_IDS
                .iter()
                .map(|id| (*id).to_owned())
                .collect(),
            page_worksets: FRONT_END_RESULT_DIALOGUE_RECORD_IDS
                .iter()
                .enumerate()
                .map(|(index, id)| page(id, index))
                .collect(),
        };
        let mut worksets = FRONT_END_RESULT_DIALOGUE_RECORD_IDS
            .iter()
            .map(|_| workset('가'))
            .collect::<Vec<_>>();
        worksets[0].fixed_glyph_codes.insert('나', 0x84);
        worksets[0].target_glyphs.insert('나');

        let error = match augment_result_worksets(
            &display,
            &worksets,
            &FRONT_END_RESULT_DIALOGUE_RECORD_IDS,
            &BTreeMap::from([('기', 0x84)]),
        ) {
            Ok(_) => panic!("conflicting result-page code was accepted"),
            Err(error) => error,
        };

        assert!(error.to_string().contains("already means"));
    }

    #[test]
    fn result_residency_rejects_a_missing_route_record() {
        let display = MainDialogueDisplayPlan {
            canonical_record_count: 1,
            record_ids: vec![FRONT_END_RESULT_DIALOGUE_RECORD_IDS[0].to_owned()],
            page_worksets: vec![page(FRONT_END_RESULT_DIALOGUE_RECORD_IDS[0], 0)],
        };
        let error = match augment_result_worksets(
            &display,
            &[workset('가')],
            &FRONT_END_RESULT_DIALOGUE_RECORD_IDS,
            &BTreeMap::from([('기', 0x84)]),
        ) {
            Ok(_) => panic!("missing result route was accepted"),
            Err(error) => error,
        };

        assert!(error.to_string().contains("missing a source-bound"));
    }
}
