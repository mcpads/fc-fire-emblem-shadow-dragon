//! 유닛 선택 도움말이 바로 뒤의 주 대사 위에 남는 글꼴 수명을 소유한다.
//!
//! 원본 상태기는 `0x25` 고정 도움말을 합성한 다음 `B1:52` 주 대사를 연다. 이 모듈은
//! 먼저 그 대사 작업집합이 금지·고정하는 코드만 전역 소비자 코드북에 제공한다. 코드
//! 번호는 모든 화면 수명을 함께 본 전역 코드북이 고르고, 그 결과를 도움말 저장과
//! 후속 대사 작업집합 양쪽에 적용한다.

use std::collections::{BTreeMap, BTreeSet};

use anyhow::{Context, Result, ensure};
use serde::Serialize;

use crate::{
    dialogue_assets::MainDialogueDisplayPlan,
    dialogue_inventory::inspect_main_dialogue_runtime_identities,
    fixed_menu_labels::UNIT_SELECTION_HELP_LINE_SPECS,
    map_dialogue_lifecycle::bind_outer_screen_map_dialogue_lifecycle,
    mapper165::battle_codebook_plan::GlyphWorkset, rom::Rom,
    semantic_translation::SemanticTranslationPlan, text_inventory::FixedTextLogicalByte,
};

use super::resident_glyph_assignment::{
    assignment_sha1, augment_resident_worksets, maximum_workset_demand_components,
};

const EXPECTED_HELP_DIALOGUE_RECORD_ID: &str = "shop-and-item-dialogue:082";
type GlyphCodeConstraints = (BTreeMap<char, BTreeSet<u8>>, BTreeMap<char, u8>);

pub(super) struct UnitSelectionHelpLifetimePlan {
    directory_selector: u8,
    entry_index: usize,
    dialogue_record_id: String,
    resident_workset_indices: BTreeSet<usize>,
    help_glyphs: BTreeSet<char>,
    forbidden_codes_by_glyph: BTreeMap<char, BTreeSet<u8>>,
    preassigned_glyph_codes: BTreeMap<char, u8>,
}

impl UnitSelectionHelpLifetimePlan {
    pub(super) fn help_glyphs(&self) -> &BTreeSet<char> {
        &self.help_glyphs
    }

    pub(super) fn forbidden_codes_by_glyph(&self) -> &BTreeMap<char, BTreeSet<u8>> {
        &self.forbidden_codes_by_glyph
    }

    pub(super) fn preassigned_glyph_codes(&self) -> &BTreeMap<char, u8> {
        &self.preassigned_glyph_codes
    }
}

#[derive(Serialize)]
pub(super) struct UnitSelectionHelpResidencyPlan {
    strategy: &'static str,
    dialogue_directory_selector_hex: String,
    dialogue_entry_index_hex: String,
    dialogue_record_id: String,
    resident_workset_count: usize,
    help_line_count: usize,
    help_glyph_count: usize,
    fixed_code_count: usize,
    maximum_augmented_workset_target_glyph_count: usize,
    maximum_augmented_workset_preserved_active_code_count: usize,
    maximum_augmented_workset_slot_demand: usize,
    fixed_assignment_sha1: String,
    source_lifecycle_bound: bool,
    codes_selected_by_the_global_consumer_codebook: bool,
    every_help_glyph_keeps_one_code_through_the_dialogue_handoff: bool,
    #[serde(skip)]
    pub(super) augmented_worksets: Vec<GlyphWorkset>,
}

pub(super) fn plan_unit_selection_help_lifetime(
    source: &Rom,
    display: &MainDialogueDisplayPlan,
    fixed_menu_labels: &SemanticTranslationPlan,
    dialogue_worksets: &[GlyphWorkset],
) -> Result<UnitSelectionHelpLifetimePlan> {
    ensure!(
        display.page_worksets.len() == dialogue_worksets.len(),
        "unit-selection help residency lost visible dialogue worksets"
    );
    let lifecycle = bind_outer_screen_map_dialogue_lifecycle(source)?;
    let directory_selector = lifecycle.help_dialogue_directory_selector();
    let entry_index = lifecycle.help_dialogue_entry_index();
    let matching_records = inspect_main_dialogue_runtime_identities(source.data())?
        .into_iter()
        .filter(|binding| {
            binding.directory_selector == directory_selector
                && binding.entry_indices.contains(&entry_index)
        })
        .collect::<Vec<_>>();
    ensure!(
        matching_records.len() == 1,
        "unit-selection help handoff identity {:02X}:{entry_index:02X} resolves to {} records",
        directory_selector,
        matching_records.len()
    );
    let dialogue_record_id = matching_records[0].record_id.clone();
    ensure!(
        dialogue_record_id == EXPECTED_HELP_DIALOGUE_RECORD_ID,
        "unit-selection help handoff record changed to {dialogue_record_id}"
    );

    let resident_workset_indices = resident_workset_indices(display, &dialogue_record_id)?;
    let help_glyphs = help_glyphs(fixed_menu_labels)?;
    let (forbidden_codes_by_glyph, preassigned_glyph_codes) =
        build_code_constraints(&help_glyphs, &resident_workset_indices, dialogue_worksets)?;

    Ok(UnitSelectionHelpLifetimePlan {
        directory_selector,
        entry_index,
        dialogue_record_id,
        resident_workset_indices,
        help_glyphs,
        forbidden_codes_by_glyph,
        preassigned_glyph_codes,
    })
}

pub(super) fn finalize_unit_selection_help_residency(
    lifetime: UnitSelectionHelpLifetimePlan,
    dialogue_worksets: &[GlyphWorkset],
    installed_help_glyph_codes: BTreeMap<char, u8>,
) -> Result<UnitSelectionHelpResidencyPlan> {
    ensure!(
        installed_help_glyph_codes
            .keys()
            .copied()
            .collect::<BTreeSet<_>>()
            == lifetime.help_glyphs,
        "global consumer codebook lost unit-selection help glyphs"
    );
    ensure!(
        lifetime
            .preassigned_glyph_codes
            .iter()
            .all(|(glyph, code)| installed_help_glyph_codes.get(glyph) == Some(code)),
        "global consumer codebook changed a unit-selection help code fixed by its dialogue"
    );
    ensure!(
        installed_help_glyph_codes
            .iter()
            .all(|(glyph, code)| !lifetime.forbidden_codes_by_glyph[glyph].contains(code)),
        "global consumer codebook assigned a unit-selection help code forbidden by its dialogue"
    );
    let (augmented_worksets, maximum_augmented_workset_slot_demand) = augment_resident_worksets(
        "unit-selection help dialogue residency",
        &installed_help_glyph_codes,
        &lifetime.resident_workset_indices,
        dialogue_worksets,
    )?;
    let maximum_demand = maximum_workset_demand_components(
        "unit-selection help dialogue residency",
        &augmented_worksets,
    )?;
    ensure!(
        maximum_demand.total_slot_demand == maximum_augmented_workset_slot_demand,
        "unit-selection help maximum workset components changed"
    );

    Ok(UnitSelectionHelpResidencyPlan {
        strategy: "bind the source state-25 help overlay and its B1:52 dialogue handoff, let the global consumer conflict graph select the inline codes, then keep those codes in every visible page of the dialogue record",
        dialogue_directory_selector_hex: format!("0x{:02X}", lifetime.directory_selector),
        dialogue_entry_index_hex: format!("0x{:02X}", lifetime.entry_index),
        dialogue_record_id: lifetime.dialogue_record_id,
        resident_workset_count: lifetime.resident_workset_indices.len(),
        help_line_count: UNIT_SELECTION_HELP_LINE_SPECS.len(),
        help_glyph_count: lifetime.help_glyphs.len(),
        fixed_code_count: installed_help_glyph_codes.len(),
        maximum_augmented_workset_target_glyph_count: maximum_demand.target_glyph_count,
        maximum_augmented_workset_preserved_active_code_count: maximum_demand
            .preserved_active_code_count,
        maximum_augmented_workset_slot_demand,
        fixed_assignment_sha1: assignment_sha1(&installed_help_glyph_codes),
        source_lifecycle_bound: true,
        codes_selected_by_the_global_consumer_codebook: true,
        every_help_glyph_keeps_one_code_through_the_dialogue_handoff: true,
        augmented_worksets,
    })
}

fn resident_workset_indices(
    display: &MainDialogueDisplayPlan,
    dialogue_record_id: &str,
) -> Result<BTreeSet<usize>> {
    let indices = display
        .page_worksets
        .iter()
        .enumerate()
        .filter_map(|(index, page)| (page.record_id == dialogue_record_id).then_some(index))
        .collect::<BTreeSet<_>>();
    ensure!(
        !indices.is_empty(),
        "unit-selection help dialogue {dialogue_record_id} has no visible workset"
    );
    Ok(indices)
}

fn help_glyphs(plan: &SemanticTranslationPlan) -> Result<BTreeSet<char>> {
    let mut glyphs = BTreeSet::new();
    for spec in UNIT_SELECTION_HELP_LINE_SPECS {
        let logical = plan
            .entry_logical_bytes(spec.id)
            .with_context(|| format!("fixed-menu plan lost {}", spec.id))?;
        glyphs.extend(logical.iter().filter_map(|byte| match byte {
            FixedTextLogicalByte::TargetGlyph(glyph) => Some(*glyph),
            FixedTextLogicalByte::Encoded(_) => None,
        }));
    }
    ensure!(
        !glyphs.is_empty(),
        "unit-selection help translation has no target glyphs"
    );
    Ok(glyphs)
}

fn build_code_constraints(
    help_glyphs: &BTreeSet<char>,
    resident_workset_indices: &BTreeSet<usize>,
    dialogue_worksets: &[GlyphWorkset],
) -> Result<GlyphCodeConstraints> {
    let mut forbidden = help_glyphs
        .iter()
        .copied()
        .map(|glyph| (glyph, BTreeSet::new()))
        .collect::<BTreeMap<_, _>>();
    let mut preassigned_sets = BTreeMap::<char, BTreeSet<u8>>::new();
    for workset_index in resident_workset_indices {
        let workset = dialogue_worksets
            .get(*workset_index)
            .context("unit-selection help workset index exceeds the dialogue population")?;
        for glyph in help_glyphs {
            let glyph_forbidden = forbidden
                .get_mut(glyph)
                .expect("help glyph constraint was initialized");
            glyph_forbidden.extend(workset.preserved_active_codes.iter().copied());
            for (fixed_glyph, fixed_code) in &workset.fixed_glyph_codes {
                if fixed_glyph == glyph {
                    preassigned_sets
                        .entry(*glyph)
                        .or_default()
                        .insert(*fixed_code);
                } else {
                    glyph_forbidden.insert(*fixed_code);
                }
            }
        }
    }
    let preassigned = preassigned_sets
        .into_iter()
        .map(|(glyph, codes)| {
            ensure!(
                codes.len() == 1,
                "unit-selection help glyph {glyph:?} has conflicting dialogue codes"
            );
            Ok((glyph, *codes.first().expect("checked one code")))
        })
        .collect::<Result<BTreeMap<_, _>>>()?;
    Ok((forbidden, preassigned))
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

    fn workset(preserved: &[u8], fixed: &[(char, u8)]) -> GlyphWorkset {
        GlyphWorkset {
            target_glyphs: BTreeSet::new(),
            preserved_active_codes: preserved.iter().copied().collect(),
            fixed_glyph_codes: fixed.iter().copied().collect(),
        }
    }

    #[test]
    fn every_visible_page_of_the_handoff_record_is_resident() {
        let display = MainDialogueDisplayPlan {
            canonical_record_count: 2,
            page_worksets: vec![
                page(EXPECTED_HELP_DIALOGUE_RECORD_ID, 0),
                page(EXPECTED_HELP_DIALOGUE_RECORD_ID, 1),
                page("unrelated:000", 0),
            ],
            record_ids: vec![
                EXPECTED_HELP_DIALOGUE_RECORD_ID.to_owned(),
                "unrelated:000".to_owned(),
            ],
        };

        assert_eq!(
            resident_workset_indices(&display, EXPECTED_HELP_DIALOGUE_RECORD_ID).unwrap(),
            BTreeSet::from([0, 1])
        );
    }

    #[test]
    fn dialogue_constraints_are_inputs_to_the_global_assignment() {
        let (forbidden, preassigned) = build_code_constraints(
            &BTreeSet::from(['가', '나']),
            &BTreeSet::from([0]),
            &[workset(&[0x90], &[('가', 0x91), ('다', 0x92)])],
        )
        .unwrap();

        assert_eq!(preassigned, BTreeMap::from([('가', 0x91)]));
        assert_eq!(forbidden[&'가'], BTreeSet::from([0x90, 0x92]));
        assert_eq!(forbidden[&'나'], BTreeSet::from([0x90, 0x91, 0x92]));
    }

    #[test]
    fn a_missing_handoff_record_fails_closed() {
        let display = MainDialogueDisplayPlan {
            canonical_record_count: 1,
            page_worksets: vec![page("unrelated:000", 0)],
            record_ids: vec!["unrelated:000".to_owned()],
        };

        assert!(
            resident_workset_indices(&display, EXPECTED_HELP_DIALOGUE_RECORD_ID)
                .unwrap_err()
                .to_string()
                .contains("has no visible workset")
        );
    }
}
