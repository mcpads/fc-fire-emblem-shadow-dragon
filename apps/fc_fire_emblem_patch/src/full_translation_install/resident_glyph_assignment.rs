//! 여러 화면 수명에서 같은 저장 바이트를 읽는 글리프에 하나의 물리 코드를 배정한다.

use std::collections::{BTreeMap, BTreeSet};

use anyhow::{Context, Result, ensure};

use crate::{
    font_slots::{ACTIVE_HANGUL_SLOT_COUNT, active_hangul_codes},
    mapper165::battle_codebook_plan::GlyphWorkset,
};

pub(super) struct ResidentGlyphWorksetPlan {
    pub(super) glyph_codes: BTreeMap<char, u8>,
    pub(super) augmented_worksets: Vec<GlyphWorkset>,
    pub(super) maximum_augmented_workset_slot_demand: usize,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) struct WorksetDemandComponents {
    pub(super) target_glyph_count: usize,
    pub(super) preserved_active_code_count: usize,
    pub(super) total_slot_demand: usize,
}

/// 설치된 작업집합 가운데 실제 슬롯 합이 가장 큰 하나의 구성요소를 돌려준다.
/// 서로 다른 작업집합의 최대 target/preserved 값을 합쳐 존재하지 않는 화면을 만들지 않는다.
pub(super) fn maximum_workset_demand_components(
    role: &str,
    worksets: &[GlyphWorkset],
) -> Result<WorksetDemandComponents> {
    let maximum = worksets
        .iter()
        .map(|workset| WorksetDemandComponents {
            target_glyph_count: workset.target_glyphs.len(),
            preserved_active_code_count: workset.preserved_active_codes.len(),
            total_slot_demand: workset.target_glyphs.len() + workset.preserved_active_codes.len(),
        })
        .max_by_key(|demand| {
            (
                demand.total_slot_demand,
                demand.target_glyph_count,
                demand.preserved_active_code_count,
            )
        })
        .with_context(|| format!("{role} has no installed worksets"))?;
    ensure!(
        maximum.total_slot_demand <= ACTIVE_HANGUL_SLOT_COUNT,
        "{role} needs {} active slots but only {ACTIVE_HANGUL_SLOT_COUNT} exist",
        maximum.total_slot_demand
    );
    Ok(maximum)
}

/// 같은 글리프 집합이 여러 대사 작업집합 위에 계속 보일 때 한 코드 배정을 고르고
/// 대상 작업집합 모두에 주입한다. 이미 고정된 같은 글리프 코드는 이어받고, 다른
/// 글리프나 원본 보존 코드와 충돌하는 코드는 후보에서 제외한다.
pub(super) fn assign_and_augment_resident_worksets(
    role: &str,
    resident_glyphs: &BTreeSet<char>,
    resident_workset_indices: &BTreeSet<usize>,
    dialogue_worksets: &[GlyphWorkset],
    prior_glyph_codes: &BTreeMap<char, u8>,
) -> Result<ResidentGlyphWorksetPlan> {
    ensure!(
        !resident_glyphs.is_empty() && !resident_workset_indices.is_empty(),
        "{role} has an empty glyph or workset population"
    );
    ensure!(
        resident_workset_indices
            .iter()
            .all(|index| *index < dialogue_worksets.len()),
        "{role} references a workset outside the dialogue population"
    );

    let mut forbidden_codes_by_glyph = resident_glyphs
        .iter()
        .copied()
        .map(|glyph| (glyph, BTreeSet::new()))
        .collect::<BTreeMap<_, _>>();
    let mut preassigned_codes_by_glyph = prior_glyph_codes
        .iter()
        .filter(|(glyph, _)| resident_glyphs.contains(glyph))
        .map(|(glyph, code)| (*glyph, BTreeSet::from([*code])))
        .collect::<BTreeMap<_, _>>();
    for workset_index in resident_workset_indices {
        let workset = &dialogue_worksets[*workset_index];
        for glyph in resident_glyphs {
            let forbidden = forbidden_codes_by_glyph
                .get_mut(glyph)
                .expect("resident glyph was initialized");
            forbidden.extend(workset.preserved_active_codes.iter().copied());
            for (fixed_glyph, fixed_code) in &workset.fixed_glyph_codes {
                if fixed_glyph == glyph {
                    preassigned_codes_by_glyph
                        .entry(*glyph)
                        .or_default()
                        .insert(*fixed_code);
                } else {
                    forbidden.insert(*fixed_code);
                }
            }
        }
    }
    let glyph_codes = assign_resident_glyph_codes(
        role,
        &forbidden_codes_by_glyph,
        &preassigned_codes_by_glyph,
        &active_hangul_codes().into_iter().collect(),
    )?;

    let (augmented_worksets, maximum_augmented_workset_slot_demand) = augment_resident_worksets(
        role,
        &glyph_codes,
        resident_workset_indices,
        dialogue_worksets,
    )?;

    Ok(ResidentGlyphWorksetPlan {
        glyph_codes,
        augmented_worksets,
        maximum_augmented_workset_slot_demand,
    })
}

pub(super) fn augment_resident_worksets(
    role: &str,
    glyph_codes: &BTreeMap<char, u8>,
    resident_workset_indices: &BTreeSet<usize>,
    dialogue_worksets: &[GlyphWorkset],
) -> Result<(Vec<GlyphWorkset>, usize)> {
    ensure!(
        !glyph_codes.is_empty() && !resident_workset_indices.is_empty(),
        "{role} has an empty glyph assignment or workset population"
    );
    ensure!(
        resident_workset_indices
            .iter()
            .all(|index| *index < dialogue_worksets.len()),
        "{role} references a workset outside the dialogue population"
    );
    let mut augmented_worksets = dialogue_worksets.to_vec();
    let mut maximum_augmented_workset_slot_demand = 0;
    for (index, workset) in augmented_worksets.iter_mut().enumerate() {
        if resident_workset_indices.contains(&index) {
            for (glyph, code) in glyph_codes {
                ensure!(
                    !workset.preserved_active_codes.contains(code),
                    "{role} glyph {glyph:?} uses preserved code {code:02X}"
                );
                ensure!(
                    workset
                        .fixed_glyph_codes
                        .iter()
                        .all(|(other_glyph, other_code)| {
                            other_glyph == glyph || other_code != code
                        }),
                    "{role} glyph {glyph:?} reuses code {code:02X} from another visible glyph"
                );
                workset.target_glyphs.insert(*glyph);
                if let Some(existing) = workset.fixed_glyph_codes.insert(*glyph, *code) {
                    ensure!(
                        existing == *code,
                        "{role} glyph {glyph:?} changes its preassigned fixed code"
                    );
                }
            }
        }
        maximum_augmented_workset_slot_demand = maximum_augmented_workset_slot_demand
            .max(workset.target_glyphs.len() + workset.preserved_active_codes.len());
    }
    ensure!(
        maximum_augmented_workset_slot_demand <= ACTIVE_HANGUL_SLOT_COUNT,
        "{role} needs {maximum_augmented_workset_slot_demand} active slots but only {ACTIVE_HANGUL_SLOT_COUNT} exist"
    );

    Ok((augmented_worksets, maximum_augmented_workset_slot_demand))
}

pub(super) fn assign_resident_glyph_codes(
    role: &str,
    forbidden_codes_by_glyph: &BTreeMap<char, BTreeSet<u8>>,
    preassigned_codes_by_glyph: &BTreeMap<char, BTreeSet<u8>>,
    active_codes: &BTreeSet<u8>,
) -> Result<BTreeMap<char, u8>> {
    ensure!(
        !forbidden_codes_by_glyph.is_empty(),
        "{role} has no resident glyphs"
    );
    let candidates = forbidden_codes_by_glyph
        .iter()
        .map(|(glyph, forbidden)| {
            let preassigned = preassigned_codes_by_glyph.get(glyph);
            ensure!(
                preassigned.is_none_or(|codes| codes.len() == 1),
                "{role} glyph {glyph:?} has conflicting preassigned codes: {preassigned:02X?}"
            );
            let allowed = match preassigned.and_then(|codes| codes.first().copied()) {
                Some(code) => {
                    ensure!(
                        active_codes.contains(&code) && !forbidden.contains(&code),
                        "{role} glyph {glyph:?} cannot keep preassigned code {code:02X}"
                    );
                    BTreeSet::from([code])
                }
                None => active_codes
                    .difference(forbidden)
                    .copied()
                    .collect::<BTreeSet<_>>(),
            };
            ensure!(
                !allowed.is_empty(),
                "{role} glyph {glyph:?} has no code valid across its resident pages"
            );
            Ok((*glyph, allowed))
        })
        .collect::<Result<BTreeMap<_, _>>>()?;
    let mut glyph_order = candidates.keys().copied().collect::<Vec<_>>();
    glyph_order.sort_by_key(|glyph| (candidates[glyph].len(), *glyph));

    let mut glyph_by_code = BTreeMap::<u8, char>::new();
    for glyph in glyph_order {
        let mut visited_codes = BTreeSet::new();
        ensure!(
            assign_one_glyph_code(glyph, &candidates, &mut glyph_by_code, &mut visited_codes),
            "{role} glyphs have no injective code assignment across resident pages"
        );
    }
    let assignments = glyph_by_code
        .into_iter()
        .map(|(code, glyph)| (glyph, code))
        .collect::<BTreeMap<_, _>>();
    ensure!(
        assignments.len() == candidates.len()
            && assignments
                .iter()
                .all(|(glyph, code)| candidates[glyph].contains(code)),
        "{role} matching returned an invalid code assignment"
    );
    Ok(assignments)
}

fn assign_one_glyph_code(
    glyph: char,
    candidates: &BTreeMap<char, BTreeSet<u8>>,
    glyph_by_code: &mut BTreeMap<u8, char>,
    visited_codes: &mut BTreeSet<u8>,
) -> bool {
    for code in &candidates[&glyph] {
        if !visited_codes.insert(*code) {
            continue;
        }
        let displaced = glyph_by_code.get(code).copied();
        if displaced.is_none_or(|other| {
            assign_one_glyph_code(other, candidates, glyph_by_code, visited_codes)
        }) {
            glyph_by_code.insert(*code, glyph);
            return true;
        }
    }
    false
}

pub(super) fn assignment_sha1(assignments: &BTreeMap<char, u8>) -> String {
    let mut bytes = Vec::with_capacity(assignments.len() * 5);
    for (glyph, code) in assignments {
        bytes.extend_from_slice(&u32::from(*glyph).to_le_bytes());
        bytes.push(*code);
    }
    crate::sha1_hex(&bytes)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn workset(glyphs: &str, preserved: &[u8], fixed: &[(char, u8)]) -> GlyphWorkset {
        GlyphWorkset {
            target_glyphs: glyphs.chars().collect(),
            preserved_active_codes: preserved.iter().copied().collect(),
            fixed_glyph_codes: fixed.iter().copied().collect(),
        }
    }

    #[test]
    fn matching_reserves_the_scarce_code_for_the_constrained_glyph() {
        let active = BTreeSet::from([1, 2]);
        let forbidden = BTreeMap::from([('가', BTreeSet::new()), ('나', BTreeSet::from([2]))]);

        let assignments =
            assign_resident_glyph_codes("synthetic", &forbidden, &BTreeMap::new(), &active)
                .unwrap();

        assert_eq!(assignments[&'나'], 1);
        assert_eq!(assignments[&'가'], 2);
    }

    #[test]
    fn matching_fails_when_two_glyphs_have_only_one_shared_code() {
        let active = BTreeSet::from([1]);
        let forbidden = BTreeMap::from([('가', BTreeSet::new()), ('나', BTreeSet::new())]);

        let error = assign_resident_glyph_codes("synthetic", &forbidden, &BTreeMap::new(), &active)
            .unwrap_err();

        assert!(error.to_string().contains("no injective code assignment"));
    }

    #[test]
    fn maximum_components_come_from_one_real_workset() {
        let active = active_hangul_codes();
        let worksets = vec![
            workset("가나다", &active[..1], &[]),
            workset("라", &active[..5], &[]),
        ];

        let demand = maximum_workset_demand_components("fixture", &worksets).unwrap();

        assert_eq!(demand.target_glyph_count, 1);
        assert_eq!(demand.preserved_active_code_count, 5);
        assert_eq!(demand.total_slot_demand, 6);
    }

    #[test]
    fn a_preassigned_glyph_keeps_its_code() {
        let active = BTreeSet::from([1, 2]);
        let forbidden = BTreeMap::from([('가', BTreeSet::new()), ('나', BTreeSet::new())]);
        let preassigned = BTreeMap::from([('가', BTreeSet::from([2]))]);

        let assignments =
            assign_resident_glyph_codes("synthetic", &forbidden, &preassigned, &active).unwrap();

        assert_eq!(assignments[&'가'], 2);
        assert_eq!(assignments[&'나'], 1);
    }

    #[test]
    fn overlay_codes_are_installed_in_every_resident_workset() {
        let active = active_hangul_codes();
        let preserved = active[0];
        let existing = active[1];
        let prior = active[2];
        let worksets = vec![
            workset("다", &[preserved], &[('나', existing)]),
            workset("라", &[], &[]),
        ];

        let plan = assign_and_augment_resident_worksets(
            "overlay",
            &BTreeSet::from(['가', '나']),
            &BTreeSet::from([0]),
            &worksets,
            &BTreeMap::from([('가', prior)]),
        )
        .unwrap();

        assert_eq!(plan.glyph_codes[&'가'], prior);
        assert_eq!(plan.glyph_codes[&'나'], existing);
        assert_eq!(plan.augmented_worksets[0].fixed_glyph_codes[&'가'], prior);
        assert_eq!(
            plan.augmented_worksets[0].fixed_glyph_codes[&'나'],
            existing
        );
        assert!(!plan.augmented_worksets[1].target_glyphs.contains(&'가'));
    }

    #[test]
    fn overlay_rejects_a_prior_code_preserved_by_its_dialogue() {
        let active = active_hangul_codes();
        let code = active[0];
        let error = assign_and_augment_resident_worksets(
            "overlay",
            &BTreeSet::from(['가']),
            &BTreeSet::from([0]),
            &[workset("나", &[code], &[])],
            &BTreeMap::from([('가', code)]),
        )
        .err()
        .expect("preserved code must reject the overlay assignment");

        assert!(error.to_string().contains("cannot keep preassigned code"));
    }
}
