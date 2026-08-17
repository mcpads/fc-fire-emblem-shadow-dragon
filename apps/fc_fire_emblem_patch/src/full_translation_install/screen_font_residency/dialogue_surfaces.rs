//! 주 대사에 차례로 합쳐지는 글리프 수명과 최종 페이지 코드북을 결속한다.
//!
//! 각 단계의 개별 계획이 성공해도 뒤 단계가 앞 단계의 고정 코드나 글리프를 잃으면
//! 최종 화면에서는 다시 가블이 난다. 이 모듈은 단계별 작업집합이 단조롭게 확장되고,
//! 최종 코드북의 선택 페이지가 그 결과를 실제로 수용하는지 한 번 더 검사한다.

use std::collections::BTreeSet;

use anyhow::{Result, ensure};
use serde::Serialize;

use crate::{
    font_slots::ACTIVE_HANGUL_SLOT_COUNT,
    mapper165::battle_codebook_plan::{GlyphWorkset, GlyphWorksetPagePlan},
};

const RESIDENCY_STAGE_NAMES: [&str; 5] = [
    "dynamic_inputs",
    "chapter_intro",
    "choice_and_front_end_menu",
    "front_end_result",
    "transition_lifetime",
];

pub(in crate::full_translation_install) struct DialogueSurfaceInputs<'a> {
    pub(in crate::full_translation_install) dynamic_inputs: &'a [GlyphWorkset],
    pub(in crate::full_translation_install) chapter_intro: &'a [GlyphWorkset],
    pub(in crate::full_translation_install) choice_and_front_end_menu: &'a [GlyphWorkset],
    pub(in crate::full_translation_install) front_end_result: &'a [GlyphWorkset],
    pub(in crate::full_translation_install) transition_lifetime: &'a [GlyphWorkset],
    pub(in crate::full_translation_install) codebook: &'a GlyphWorksetPagePlan,
}

#[derive(Debug, Serialize)]
pub(super) struct DialogueSurfacePlan {
    strategy: &'static str,
    residency_stage_count: usize,
    visible_workset_count: usize,
    target_glyph_count: usize,
    codebook_page_count: usize,
    maximum_workset_slot_demand: usize,
    maximum_page_slot_demand: usize,
    every_residency_stage_is_monotonic: bool,
    every_workset_selects_one_codebook_page: bool,
    every_selected_page_contains_its_workset_glyphs: bool,
    every_fixed_glyph_keeps_its_code: bool,
}

pub(super) fn plan_dialogue_surfaces(
    inputs: DialogueSurfaceInputs<'_>,
) -> Result<DialogueSurfacePlan> {
    let stages = [
        inputs.dynamic_inputs,
        inputs.chapter_intro,
        inputs.choice_and_front_end_menu,
        inputs.front_end_result,
        inputs.transition_lifetime,
    ];
    let visible_workset_count = stages[0].len();
    ensure!(
        visible_workset_count > 0,
        "dialogue screen residency has no visible worksets"
    );
    ensure!(
        stages
            .iter()
            .all(|stage| stage.len() == visible_workset_count),
        "dialogue screen residency stages disagree on visible workset count"
    );

    for (stage_index, pair) in stages.windows(2).enumerate() {
        let earlier_name = RESIDENCY_STAGE_NAMES[stage_index];
        let later_name = RESIDENCY_STAGE_NAMES[stage_index + 1];
        for (workset_index, (earlier, later)) in pair[0].iter().zip(pair[1]).enumerate() {
            ensure_workset_extends(earlier, later).map_err(|error| {
                anyhow::anyhow!(
                    "dialogue screen residency {earlier_name}->{later_name} workset {workset_index} is not monotonic: {error}"
                )
            })?;
        }
    }

    ensure!(
        inputs.codebook.workset_count == visible_workset_count
            && inputs.codebook.workset_page_indices.len() == visible_workset_count,
        "dialogue screen residency codebook lost visible worksets"
    );
    ensure!(
        inputs.codebook.maximum_workset_slot_demand <= ACTIVE_HANGUL_SLOT_COUNT
            && inputs.codebook.maximum_page_slot_demand <= ACTIVE_HANGUL_SLOT_COUNT,
        "dialogue screen residency codebook exceeds the active font capacity"
    );

    let mut all_target_glyphs = BTreeSet::new();
    for (workset_index, workset) in inputs.transition_lifetime.iter().enumerate() {
        let page_index = inputs.codebook.workset_page_indices[workset_index];
        let assignments = inputs
            .codebook
            .page_assignments
            .get(page_index)
            .ok_or_else(|| {
                anyhow::anyhow!(
                    "dialogue screen residency workset {workset_index} selects missing page {page_index}"
                )
            })?;
        ensure!(
            workset
                .target_glyphs
                .iter()
                .all(|glyph| assignments.contains_key(glyph)),
            "dialogue screen residency page {page_index} lost a glyph from workset {workset_index}"
        );
        ensure!(
            workset
                .fixed_glyph_codes
                .iter()
                .all(|(glyph, code)| assignments.get(glyph) == Some(code)),
            "dialogue screen residency page {page_index} changed a fixed glyph code from workset {workset_index}"
        );
        ensure!(
            assignments
                .values()
                .all(|code| !workset.preserved_active_codes.contains(code)),
            "dialogue screen residency page {page_index} overwrites a preserved code from workset {workset_index}"
        );
        all_target_glyphs.extend(workset.target_glyphs.iter().copied());
    }

    ensure!(
        all_target_glyphs.len() == inputs.codebook.glyph_count,
        "dialogue screen residency and codebook disagree on target glyph population"
    );

    Ok(DialogueSurfacePlan {
        strategy: "carry every visible dialogue workset monotonically through dynamic strings, chapter titles, choices, retained front-end results, and transition lifetimes; then rebind each final workset to its selected codebook page",
        residency_stage_count: RESIDENCY_STAGE_NAMES.len(),
        visible_workset_count,
        target_glyph_count: all_target_glyphs.len(),
        codebook_page_count: inputs.codebook.page_assignments.len(),
        maximum_workset_slot_demand: inputs.codebook.maximum_workset_slot_demand,
        maximum_page_slot_demand: inputs.codebook.maximum_page_slot_demand,
        every_residency_stage_is_monotonic: true,
        every_workset_selects_one_codebook_page: true,
        every_selected_page_contains_its_workset_glyphs: true,
        every_fixed_glyph_keeps_its_code: true,
    })
}

fn ensure_workset_extends(earlier: &GlyphWorkset, later: &GlyphWorkset) -> Result<()> {
    ensure!(
        earlier.target_glyphs.is_subset(&later.target_glyphs),
        "target glyphs were removed"
    );
    ensure!(
        earlier
            .preserved_active_codes
            .is_subset(&later.preserved_active_codes),
        "preserved codes were removed"
    );
    ensure!(
        earlier
            .fixed_glyph_codes
            .iter()
            .all(|(glyph, code)| later.fixed_glyph_codes.get(glyph) == Some(code)),
        "fixed glyph codes were removed or changed"
    );
    Ok(())
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;

    use super::*;

    fn workset(glyphs: &str, preserved: &[u8], fixed: &[(char, u8)]) -> GlyphWorkset {
        GlyphWorkset {
            target_glyphs: glyphs.chars().collect(),
            preserved_active_codes: preserved.iter().copied().collect(),
            fixed_glyph_codes: fixed.iter().copied().collect(),
        }
    }

    fn codebook(assignments: BTreeMap<char, u8>) -> GlyphWorksetPagePlan {
        GlyphWorksetPagePlan {
            glyph_count: assignments.len(),
            workset_count: 1,
            unique_workset_count: 1,
            maximum_workset_slot_demand: assignments.len(),
            maximum_page_slot_demand: assignments.len(),
            page_assignments: vec![assignments],
            workset_page_indices: vec![0],
            packing_sha1: "packing".to_owned(),
            page_assignment_sha1: "assignments".to_owned(),
            greedy_page_count: 1,
            packing_strategy: "test",
            constraint_solver_version: None,
            constraint_solver_timeout_seconds: None,
        }
    }

    fn inputs<'a>(
        stages: [&'a [GlyphWorkset]; 5],
        codebook: &'a GlyphWorksetPagePlan,
    ) -> DialogueSurfaceInputs<'a> {
        DialogueSurfaceInputs {
            dynamic_inputs: stages[0],
            chapter_intro: stages[1],
            choice_and_front_end_menu: stages[2],
            front_end_result: stages[3],
            transition_lifetime: stages[4],
            codebook,
        }
    }

    #[test]
    fn residency_stages_and_selected_page_form_one_surface_contract() {
        let dynamic = vec![workset("가", &[0xA0], &[])];
        let chapter = vec![workset("가나", &[0xA0], &[('나', 0xA1)])];
        let choice = vec![workset("가나다", &[0xA0], &[('나', 0xA1)])];
        let result = vec![workset("가나다라", &[0xA0], &[('나', 0xA1)])];
        let transition = vec![workset("가나다라마", &[0xA0], &[('나', 0xA1)])];
        let codebook = codebook(BTreeMap::from([
            ('가', 0xA2),
            ('나', 0xA1),
            ('다', 0xA3),
            ('라', 0xA4),
            ('마', 0xA5),
        ]));

        let plan = plan_dialogue_surfaces(inputs(
            [&dynamic, &chapter, &choice, &result, &transition],
            &codebook,
        ))
        .unwrap();

        assert_eq!(plan.visible_workset_count, 1);
        assert_eq!(plan.target_glyph_count, 5);
        assert_eq!(plan.codebook_page_count, 1);
    }

    #[test]
    fn later_stage_cannot_drop_an_earlier_glyph() {
        let earlier = vec![workset("가나", &[], &[])];
        let later = vec![workset("가", &[], &[])];
        let codebook = codebook(BTreeMap::from([('가', 0xA0)]));

        let error = plan_dialogue_surfaces(inputs(
            [&earlier, &later, &later, &later, &later],
            &codebook,
        ))
        .unwrap_err();

        assert!(error.to_string().contains("target glyphs were removed"));
    }

    #[test]
    fn selected_page_cannot_change_a_fixed_glyph_code() {
        let stage = vec![workset("가", &[], &[('가', 0xA0)])];
        let codebook = codebook(BTreeMap::from([('가', 0xA1)]));

        let error =
            plan_dialogue_surfaces(inputs([&stage, &stage, &stage, &stage, &stage], &codebook))
                .unwrap_err();

        assert!(error.to_string().contains("changed a fixed glyph code"));
    }
}
