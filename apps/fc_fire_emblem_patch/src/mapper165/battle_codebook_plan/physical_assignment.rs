use std::collections::{BTreeMap, BTreeSet};

use anyhow::{Context, Result, ensure};

use crate::{font_slots::active_hangul_codes, sha1_hex};

use super::conflict_graph::StableColoringPlan;

pub(in crate::mapper165) struct ScreenCodeConstraint {
    pub(in crate::mapper165) glyphs: BTreeSet<char>,
    pub(in crate::mapper165) preserved_active_codes: BTreeSet<u8>,
}

#[derive(Debug)]
pub(super) struct PhysicalCodeAssignment {
    pub(super) glyph_codes: BTreeMap<char, u8>,
    pub(super) assignment_sha1: String,
    pub(super) constrained_screen_count: usize,
    pub(super) constrained_color_count: usize,
}

pub(super) fn assign_physical_codes(
    coloring: &StableColoringPlan,
    constraints: &[ScreenCodeConstraint],
) -> Result<PhysicalCodeAssignment> {
    let active_codes = active_hangul_codes();
    ensure!(
        coloring.color_count <= active_codes.len(),
        "battle coloring needs {} physical codes but only {} are active",
        coloring.color_count,
        active_codes.len()
    );
    let active_set = active_codes.iter().copied().collect::<BTreeSet<_>>();
    let mut forbidden_codes = vec![BTreeSet::new(); coloring.color_count];
    let mut constrained_colors = BTreeSet::new();
    for constraint in constraints {
        ensure!(
            constraint.preserved_active_codes.is_subset(&active_set),
            "battle screen protection includes a reserved font code"
        );
        let screen_colors = constraint
            .glyphs
            .iter()
            .map(|glyph| {
                coloring
                    .glyph_colors()
                    .get(glyph)
                    .copied()
                    .with_context(|| format!("battle screen contains unplanned glyph {glyph:?}"))
            })
            .collect::<Result<Vec<_>>>()?;
        ensure!(
            screen_colors.iter().copied().collect::<BTreeSet<_>>().len() == screen_colors.len(),
            "battle screen needs two glyphs assigned to one abstract color"
        );
        for color in screen_colors {
            constrained_colors.insert(color);
            forbidden_codes[color].extend(&constraint.preserved_active_codes);
        }
    }

    let mut colors = (0..coloring.color_count).collect::<Vec<_>>();
    colors.sort_by_key(|color| (std::cmp::Reverse(forbidden_codes[*color].len()), *color));
    let mut code_owners = BTreeMap::new();
    let mut color_codes = vec![None; coloring.color_count];
    for color in colors {
        let mut visited_codes = BTreeSet::new();
        ensure!(
            assign_color(
                color,
                &active_codes,
                &forbidden_codes,
                &mut visited_codes,
                &mut code_owners,
                &mut color_codes,
            ),
            "battle physical code constraints cannot place abstract color {color}"
        );
    }
    let color_codes = color_codes
        .into_iter()
        .map(|code| code.context("battle physical assignment lost a color"))
        .collect::<Result<Vec<_>>>()?;
    ensure!(
        color_codes.iter().copied().collect::<BTreeSet<_>>().len() == color_codes.len(),
        "battle physical assignment reused a tile code"
    );
    let glyph_codes = coloring
        .glyph_colors()
        .iter()
        .map(|(glyph, color)| (*glyph, color_codes[*color]))
        .collect::<BTreeMap<_, _>>();
    let assignment_sha1 = glyph_assignment_sha1(&glyph_codes);
    Ok(PhysicalCodeAssignment {
        glyph_codes,
        assignment_sha1,
        constrained_screen_count: constraints.len(),
        constrained_color_count: constrained_colors.len(),
    })
}

fn assign_color(
    color: usize,
    active_codes: &[u8],
    forbidden_codes: &[BTreeSet<u8>],
    visited_codes: &mut BTreeSet<u8>,
    code_owners: &mut BTreeMap<u8, usize>,
    color_codes: &mut [Option<u8>],
) -> bool {
    for code in active_codes {
        if forbidden_codes[color].contains(code) || !visited_codes.insert(*code) {
            continue;
        }
        let previous_owner = code_owners.get(code).copied();
        if previous_owner.is_none_or(|owner| {
            assign_color(
                owner,
                active_codes,
                forbidden_codes,
                visited_codes,
                code_owners,
                color_codes,
            )
        }) {
            code_owners.insert(*code, color);
            color_codes[color] = Some(*code);
            return true;
        }
    }
    false
}

fn glyph_assignment_sha1(assignments: &BTreeMap<char, u8>) -> String {
    let mut bytes = Vec::new();
    for (glyph, code) in assignments {
        bytes.extend_from_slice(glyph.to_string().as_bytes());
        bytes.push(*code);
    }
    sha1_hex(&bytes)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::mapper165::battle_codebook_plan::conflict_graph::{
        BattleGlyphFamilies, plan_stable_coloring,
    };

    fn set(glyphs: &str) -> BTreeSet<char> {
        glyphs.chars().collect()
    }

    #[test]
    fn constrained_screen_glyphs_avoid_preserved_codes() {
        let coloring = plan_stable_coloring(
            &BattleGlyphFamilies {
                base: set("가나"),
                player_participants: vec![],
                enemy_participants: vec![],
                terrains: vec![],
                dialogue_records: vec![],
            },
            2,
        )
        .unwrap();
        let preserved = active_hangul_codes()[..2]
            .iter()
            .copied()
            .collect::<BTreeSet<_>>();
        let assignment = assign_physical_codes(
            &coloring,
            &[ScreenCodeConstraint {
                glyphs: set("가"),
                preserved_active_codes: preserved.clone(),
            }],
        )
        .unwrap();

        assert!(!preserved.contains(&assignment.glyph_codes[&'가']));
        assert_eq!(assignment.constrained_color_count, 1);
        assert_eq!(
            assignment
                .glyph_codes
                .values()
                .copied()
                .collect::<BTreeSet<_>>()
                .len(),
            coloring.color_count
        );
    }

    #[test]
    fn screen_rejects_two_visible_glyphs_sharing_one_color() {
        let coloring = plan_stable_coloring(
            &BattleGlyphFamilies {
                base: BTreeSet::new(),
                player_participants: vec![set("가"), set("나")],
                enemy_participants: vec![],
                terrains: vec![],
                dialogue_records: vec![],
            },
            1,
        )
        .unwrap();

        let error = assign_physical_codes(
            &coloring,
            &[ScreenCodeConstraint {
                glyphs: set("가나"),
                preserved_active_codes: BTreeSet::new(),
            }],
        )
        .unwrap_err();

        assert!(error.to_string().contains("one abstract color"));
    }

    #[test]
    fn constraints_from_multiple_screens_accumulate_deterministically() {
        let coloring = plan_stable_coloring(
            &BattleGlyphFamilies {
                base: set("가나"),
                player_participants: vec![],
                enemy_participants: vec![],
                terrains: vec![],
                dialogue_records: vec![],
            },
            2,
        )
        .unwrap();
        let active = active_hangul_codes();
        let constraints = [
            ScreenCodeConstraint {
                glyphs: set("가"),
                preserved_active_codes: [active[0]].into_iter().collect(),
            },
            ScreenCodeConstraint {
                glyphs: set("나"),
                preserved_active_codes: [active[1]].into_iter().collect(),
            },
        ];

        let first = assign_physical_codes(&coloring, &constraints).unwrap();
        let second = assign_physical_codes(&coloring, &constraints).unwrap();

        assert_ne!(first.glyph_codes[&'가'], active[0]);
        assert_ne!(first.glyph_codes[&'나'], active[1]);
        assert_ne!(first.glyph_codes[&'가'], first.glyph_codes[&'나']);
        assert_eq!(first.assignment_sha1, second.assignment_sha1);
        assert_eq!(first.constrained_screen_count, 2);
        assert_eq!(first.constrained_color_count, 2);
    }
}
