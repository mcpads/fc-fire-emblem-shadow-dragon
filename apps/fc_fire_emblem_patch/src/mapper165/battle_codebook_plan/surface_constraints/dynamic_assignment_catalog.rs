use std::collections::{BTreeMap, BTreeSet};

use anyhow::{Context, Result, ensure};
use serde::Serialize;

use crate::sha1_hex;

use super::super::{
    ScreenCodeConstraint, conflict_graph::StableColoringPlan,
    selected_physical_assignment::assign_selected_physical_codes_with_canonical_map,
};

#[derive(Debug, Serialize)]
pub(super) struct RouteDynamicAssignmentCoverage {
    pub(super) route_role: String,
    pub(super) sample_count: usize,
    pub(super) every_sample_assignment_found: bool,
    pub(super) assignment_catalog_sha1: String,
    pub(super) maximum_selected_color_count: usize,
    pub(super) maximum_remap_pair_count: usize,
}

pub(super) struct DynamicAssignmentCatalog {
    pub(super) route_coverage: Vec<RouteDynamicAssignmentCoverage>,
    pub(super) catalog_sha1: String,
    pub(super) maximum_selected_color_count: usize,
    pub(super) maximum_remap_pair_count: usize,
}

pub(super) fn catalog_observed_dynamic_assignments(
    constraints: &[(String, ScreenCodeConstraint)],
    coloring: &StableColoringPlan,
    canonical_color_codes: &[u8],
    protected_abstract_colors: &[u8],
    maximum_remap_pair_count: usize,
) -> Result<DynamicAssignmentCatalog> {
    let protected_canonical_codes = protected_abstract_colors
        .iter()
        .map(|color| {
            canonical_color_codes
                .get(usize::from(*color))
                .copied()
                .context("observed battle protected color is outside the canonical table")
        })
        .collect::<Result<BTreeSet<_>>>()?;
    let mut assignments_by_route = BTreeMap::<String, Vec<ObservedDynamicAssignment>>::new();
    for (route_role, constraint) in constraints {
        let selected_abstract_colors = constraint
            .glyphs
            .iter()
            .map(|glyph| {
                coloring
                    .glyph_colors()
                    .get(glyph)
                    .copied()
                    .context("observed battle glyph is absent from the logical codebook")
                    .and_then(|color| {
                        u8::try_from(color)
                            .context("observed battle logical color exceeds one byte")
                    })
            })
            .collect::<Result<BTreeSet<_>>>()?;
        ensure!(
            selected_abstract_colors.len() == constraint.glyphs.len(),
            "observed battle needs two selected glyphs in one logical color"
        );
        let assignment = assign_selected_physical_codes_with_canonical_map(
            &selected_abstract_colors,
            &protected_canonical_codes,
            canonical_color_codes,
        )?;
        ensure!(
            assignment.remap_pairs.len() <= maximum_remap_pair_count,
            "observed battle needs {} remap pairs but the runtime supports {maximum_remap_pair_count}",
            assignment.remap_pairs.len(),
        );
        assignments_by_route
            .entry(route_role.clone())
            .or_default()
            .push(ObservedDynamicAssignment {
                assignment_sha1: assignment.assignment_sha1,
                selected_color_count: selected_abstract_colors.len(),
                remap_pair_count: assignment.remap_pairs.len(),
            });
    }
    ensure!(
        !assignments_by_route.is_empty(),
        "observed battle dynamic assignment catalog is empty"
    );

    let mut catalog_bytes = Vec::new();
    let mut route_coverage = Vec::with_capacity(assignments_by_route.len());
    let mut maximum_selected_color_count = 0;
    let mut observed_maximum_remap_pair_count = 0;
    for (route_role, assignments) in assignments_by_route {
        let mut route_bytes = Vec::new();
        for assignment in &assignments {
            route_bytes.extend_from_slice(assignment.assignment_sha1.as_bytes());
            route_bytes.extend_from_slice(
                &u16::try_from(assignment.selected_color_count)
                    .context("observed selected color count exceeds two bytes")?
                    .to_le_bytes(),
            );
            route_bytes.push(
                u8::try_from(assignment.remap_pair_count)
                    .context("observed remap-pair count exceeds one byte")?,
            );
            maximum_selected_color_count =
                maximum_selected_color_count.max(assignment.selected_color_count);
            observed_maximum_remap_pair_count =
                observed_maximum_remap_pair_count.max(assignment.remap_pair_count);
        }
        let assignment_catalog_sha1 = sha1_hex(&route_bytes);
        catalog_bytes.extend_from_slice(route_role.as_bytes());
        catalog_bytes.push(0);
        catalog_bytes.extend_from_slice(assignment_catalog_sha1.as_bytes());
        route_coverage.push(RouteDynamicAssignmentCoverage {
            route_role,
            sample_count: assignments.len(),
            every_sample_assignment_found: true,
            assignment_catalog_sha1,
            maximum_selected_color_count: assignments
                .iter()
                .map(|assignment| assignment.selected_color_count)
                .max()
                .unwrap_or(0),
            maximum_remap_pair_count: assignments
                .iter()
                .map(|assignment| assignment.remap_pair_count)
                .max()
                .unwrap_or(0),
        });
    }
    Ok(DynamicAssignmentCatalog {
        route_coverage,
        catalog_sha1: sha1_hex(&catalog_bytes),
        maximum_selected_color_count,
        maximum_remap_pair_count: observed_maximum_remap_pair_count,
    })
}

struct ObservedDynamicAssignment {
    assignment_sha1: String,
    selected_color_count: usize,
    remap_pair_count: usize,
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::font_slots::{PRESERVED_DISPLAY_CODES, active_hangul_codes};
    use crate::mapper165::battle_codebook_plan::conflict_graph::{
        BattleGlyphFamilies, plan_stable_coloring,
    };

    #[test]
    fn every_observed_sample_uses_the_runtime_dynamic_assignment() {
        let glyphs = (0..u32::try_from(
            crate::mapper165::battle_text_material::CANONICAL_ABSTRACT_COLOR_COUNT,
        )
        .unwrap())
            .map(|offset| char::from_u32(0xAC00 + offset).unwrap())
            .collect::<BTreeSet<_>>();
        let coloring = plan_stable_coloring(
            &BattleGlyphFamilies {
                base: glyphs,
                participant_modes: vec![],
                terrains: vec![],
                dialogue_records: vec![],
            },
            210,
        )
        .unwrap();
        let mut canonical = active_hangul_codes();
        canonical.extend(PRESERVED_DISPLAY_CODES.into_iter().take(2));
        let glyph_by_color = coloring
            .glyph_colors()
            .iter()
            .map(|(glyph, color)| (*color, *glyph))
            .collect::<BTreeMap<_, _>>();
        let constraints = vec![
            (
                "favorable".to_owned(),
                ScreenCodeConstraint {
                    glyphs: BTreeSet::from([glyph_by_color[&0], glyph_by_color[&210]]),
                },
            ),
            (
                "unfavorable".to_owned(),
                ScreenCodeConstraint {
                    glyphs: BTreeSet::from([glyph_by_color[&1], glyph_by_color[&211]]),
                },
            ),
        ];

        let catalog = catalog_observed_dynamic_assignments(
            &constraints,
            &coloring,
            &canonical,
            &[210, 211],
            2,
        )
        .unwrap();

        assert_eq!(catalog.route_coverage.len(), 2);
        assert!(
            catalog
                .route_coverage
                .iter()
                .all(|route| route.every_sample_assignment_found && route.sample_count == 1)
        );
        assert_eq!(catalog.maximum_selected_color_count, 2);
        assert_eq!(catalog.maximum_remap_pair_count, 1);
    }
}
