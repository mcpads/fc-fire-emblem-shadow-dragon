use std::collections::{BTreeMap, BTreeSet};

use anyhow::{Context, Result, ensure};

use crate::{font_slots::active_hangul_codes, sha1_hex};

pub(super) const UNUSED_REMAP_TARGET: u8 = 0xFF;

#[derive(Debug)]
pub(super) struct SelectedPhysicalCodeAssignment {
    pub(super) color_codes: BTreeMap<u8, u8>,
    pub(super) protected_code_remap_targets: Vec<u8>,
    pub(super) remap_pairs: Vec<(u8, u8)>,
    pub(super) collision_count: usize,
    pub(super) identity_code_count: usize,
    pub(super) remaining_safe_code_count: usize,
    pub(super) assignment_sha1: String,
}

#[derive(Debug)]
pub(super) struct SelectedAssignmentCapacityProof {
    pub(super) maximum_selected_code_count: usize,
    pub(super) protected_code_count: usize,
    pub(super) safe_code_count: usize,
    pub(super) maximum_collision_count: usize,
    pub(super) remap_table_byte_count: usize,
    pub(super) identity_code_count_at_maximum_collision: usize,
    pub(super) remaining_safe_code_count: usize,
    pub(super) strongest_assignment_sha1: String,
}

pub(super) fn assign_selected_physical_codes(
    selected_abstract_colors: &BTreeSet<u8>,
    protected_physical_codes: &BTreeSet<u8>,
) -> Result<SelectedPhysicalCodeAssignment> {
    assign_selected_physical_codes_with_canonical_map(
        selected_abstract_colors,
        protected_physical_codes,
        &active_hangul_codes(),
    )
}

pub(super) fn assign_selected_physical_codes_with_canonical_map(
    selected_abstract_colors: &BTreeSet<u8>,
    protected_physical_codes: &BTreeSet<u8>,
    canonical_color_codes: &[u8],
) -> Result<SelectedPhysicalCodeAssignment> {
    let active_codes = active_hangul_codes();
    let active_set = active_codes.iter().copied().collect::<BTreeSet<_>>();
    ensure!(
        protected_physical_codes.is_subset(&active_set),
        "selected battle protection includes a reserved font code"
    );
    ensure!(
        selected_abstract_colors
            .iter()
            .all(|color| usize::from(*color) < canonical_color_codes.len()),
        "selected battle contains an abstract color outside the active codebook"
    );
    ensure!(
        canonical_color_codes.len() == active_codes.len()
            && canonical_color_codes
                .iter()
                .copied()
                .collect::<BTreeSet<_>>()
                == active_set,
        "selected battle canonical code map is not a permutation of the active codes"
    );
    ensure!(
        selected_abstract_colors.len() + protected_physical_codes.len() <= active_codes.len(),
        "selected battle needs {} text codes plus {} protected codes but only {} active codes exist",
        selected_abstract_colors.len(),
        protected_physical_codes.len(),
        active_codes.len()
    );

    let canonical_code_owners = canonical_color_codes
        .iter()
        .copied()
        .enumerate()
        .map(|(color, code)| {
            Ok((
                code,
                u8::try_from(color).context("active abstract color exceeds one byte")?,
            ))
        })
        .collect::<Result<BTreeMap<_, _>>>()?;
    let colliding_colors = selected_abstract_colors
        .iter()
        .copied()
        .filter(|color| {
            protected_physical_codes.contains(&canonical_color_codes[usize::from(*color)])
        })
        .collect::<Vec<_>>();
    let available_safe_codes = active_codes
        .iter()
        .copied()
        .filter(|code| !protected_physical_codes.contains(code))
        .filter(|code| {
            let owner = canonical_code_owners[code];
            !selected_abstract_colors.contains(&owner)
        })
        .collect::<Vec<_>>();
    ensure!(
        available_safe_codes.len() >= colliding_colors.len(),
        "selected battle has {} protected-code collisions but only {} safe replacement codes",
        colliding_colors.len(),
        available_safe_codes.len()
    );

    let replacements = colliding_colors
        .iter()
        .copied()
        .zip(available_safe_codes.iter().copied())
        .collect::<BTreeMap<_, _>>();
    let color_codes = selected_abstract_colors
        .iter()
        .copied()
        .map(|color| {
            let canonical = canonical_color_codes[usize::from(color)];
            (
                color,
                replacements.get(&color).copied().unwrap_or(canonical),
            )
        })
        .collect::<BTreeMap<_, _>>();
    let assigned_codes = color_codes.values().copied().collect::<BTreeSet<_>>();
    ensure!(
        assigned_codes.len() == color_codes.len(),
        "selected battle physical assignment reused a code"
    );
    ensure!(
        assigned_codes.is_disjoint(protected_physical_codes),
        "selected battle physical assignment overwrites a protected code"
    );

    let protected_code_remap_targets = protected_physical_codes
        .iter()
        .map(|protected_code| {
            let owner = canonical_code_owners[protected_code];
            replacements
                .get(&owner)
                .copied()
                .unwrap_or(UNUSED_REMAP_TARGET)
        })
        .collect::<Vec<_>>();
    let remap_pairs = protected_physical_codes
        .iter()
        .copied()
        .zip(protected_code_remap_targets.iter().copied())
        .filter(|(_, target)| *target != UNUSED_REMAP_TARGET)
        .collect::<Vec<_>>();
    ensure!(
        protected_code_remap_targets
            .iter()
            .all(|code| *code == UNUSED_REMAP_TARGET || active_set.contains(code)),
        "selected battle remap table contains a reserved target"
    );
    ensure!(
        !active_set.contains(&UNUSED_REMAP_TARGET),
        "selected battle remap sentinel became an active font code"
    );

    let identity_code_count = color_codes
        .iter()
        .filter(|(color, code)| canonical_color_codes[usize::from(**color)] == **code)
        .count();
    let remaining_safe_code_count =
        active_codes.len() - protected_physical_codes.len() - selected_abstract_colors.len();
    let assignment_sha1 = assignment_sha1(
        &color_codes,
        protected_physical_codes,
        &protected_code_remap_targets,
    );
    Ok(SelectedPhysicalCodeAssignment {
        color_codes,
        protected_code_remap_targets,
        remap_pairs,
        collision_count: colliding_colors.len(),
        identity_code_count,
        remaining_safe_code_count,
        assignment_sha1,
    })
}

pub(super) fn prove_selected_assignment_capacity(
    maximum_selected_code_count: usize,
    protected_physical_codes: &BTreeSet<u8>,
) -> Result<SelectedAssignmentCapacityProof> {
    let active_codes = active_hangul_codes();
    let active_set = active_codes.iter().copied().collect::<BTreeSet<_>>();
    ensure!(
        protected_physical_codes.is_subset(&active_set),
        "selected battle capacity protection includes a reserved font code"
    );
    ensure!(
        maximum_selected_code_count + protected_physical_codes.len() <= active_codes.len(),
        "selected battle capacity needs {maximum_selected_code_count} text codes plus {} protected codes but only {} active codes exist",
        protected_physical_codes.len(),
        active_codes.len()
    );

    let mut selected_abstract_colors = active_codes
        .iter()
        .enumerate()
        .filter(|(_, code)| protected_physical_codes.contains(code))
        .take(maximum_selected_code_count)
        .map(|(color, _)| u8::try_from(color).context("active abstract color exceeds one byte"))
        .collect::<Result<BTreeSet<_>>>()?;
    for (color, code) in active_codes.iter().enumerate() {
        if selected_abstract_colors.len() == maximum_selected_code_count {
            break;
        }
        if protected_physical_codes.contains(code) {
            continue;
        }
        selected_abstract_colors
            .insert(u8::try_from(color).context("active abstract color exceeds one byte")?);
    }
    ensure!(
        selected_abstract_colors.len() == maximum_selected_code_count,
        "selected battle capacity proof could not construct its strongest input"
    );

    let assignment =
        assign_selected_physical_codes(&selected_abstract_colors, protected_physical_codes)?;
    let maximum_collision_count = maximum_selected_code_count.min(protected_physical_codes.len());
    ensure!(
        assignment.color_codes.len() == maximum_selected_code_count,
        "selected battle capacity proof lost an abstract color"
    );
    ensure!(
        assignment.collision_count == maximum_collision_count,
        "selected battle capacity proof did not exercise every possible protected-code collision"
    );
    ensure!(
        assignment.protected_code_remap_targets.len() == protected_physical_codes.len(),
        "selected battle capacity proof produced the wrong remap table width"
    );
    ensure!(
        assignment.remap_pairs.len() == assignment.collision_count,
        "selected battle capacity proof lost a sparse remap pair"
    );

    Ok(SelectedAssignmentCapacityProof {
        maximum_selected_code_count,
        protected_code_count: protected_physical_codes.len(),
        safe_code_count: active_codes.len() - protected_physical_codes.len(),
        maximum_collision_count,
        remap_table_byte_count: assignment.protected_code_remap_targets.len(),
        identity_code_count_at_maximum_collision: assignment.identity_code_count,
        remaining_safe_code_count: assignment.remaining_safe_code_count,
        strongest_assignment_sha1: assignment.assignment_sha1,
    })
}

fn assignment_sha1(
    color_codes: &BTreeMap<u8, u8>,
    protected_physical_codes: &BTreeSet<u8>,
    protected_code_remap_targets: &[u8],
) -> String {
    let mut bytes = Vec::new();
    for (color, code) in color_codes {
        bytes.extend_from_slice(&[*color, *code]);
    }
    bytes.extend(protected_physical_codes);
    bytes.extend_from_slice(protected_code_remap_targets);
    sha1_hex(&bytes)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn noncolliding_selected_codes_keep_their_canonical_identity() {
        let active = active_hangul_codes();
        let selected = BTreeSet::from([0, 1, 2]);
        let protected = BTreeSet::from([active[10], active[11]]);
        let assignment = assign_selected_physical_codes(&selected, &protected).unwrap();

        assert_eq!(
            assignment.color_codes,
            BTreeMap::from([(0, active[0]), (1, active[1]), (2, active[2])])
        );
        assert_eq!(assignment.collision_count, 0);
        assert_eq!(assignment.identity_code_count, 3);
        assert_eq!(
            assignment.protected_code_remap_targets,
            vec![UNUSED_REMAP_TARGET; 2]
        );
    }

    #[test]
    fn protected_canonical_codes_receive_safe_unused_targets() {
        let active = active_hangul_codes();
        let selected = BTreeSet::from([0, 1, 2]);
        let protected = BTreeSet::from([active[0], active[2]]);
        let assignment = assign_selected_physical_codes(&selected, &protected).unwrap();

        assert_eq!(assignment.collision_count, 2);
        assert_eq!(assignment.identity_code_count, 1);
        assert_eq!(assignment.color_codes[&1], active[1]);
        assert!(
            assignment
                .color_codes
                .values()
                .all(|code| !protected.contains(code))
        );
        assert_eq!(
            assignment
                .protected_code_remap_targets
                .iter()
                .filter(|target| **target != UNUSED_REMAP_TARGET)
                .count(),
            2
        );
    }

    #[test]
    fn strongest_battle_uses_all_sparse_remaps_and_keeps_headroom() {
        let active = active_hangul_codes();
        let protected = active[..39].iter().copied().collect::<BTreeSet<_>>();
        let proof = prove_selected_assignment_capacity(134, &protected).unwrap();

        assert_eq!(proof.maximum_selected_code_count, 134);
        assert_eq!(proof.protected_code_count, 39);
        assert_eq!(proof.safe_code_count, 171);
        assert_eq!(proof.maximum_collision_count, 39);
        assert_eq!(proof.remap_table_byte_count, 39);
        assert_eq!(proof.identity_code_count_at_maximum_collision, 95);
        assert_eq!(proof.remaining_safe_code_count, 37);
    }

    #[test]
    fn selected_assignment_rejects_capacity_overflow() {
        let active = active_hangul_codes();
        let selected = (0..172).map(|color| color as u8).collect::<BTreeSet<_>>();
        let protected = active[..39].iter().copied().collect::<BTreeSet<_>>();

        let error = assign_selected_physical_codes(&selected, &protected).unwrap_err();

        assert!(error.to_string().contains("text codes plus"));
    }

    #[test]
    fn capacity_proof_handles_fewer_selected_codes_than_protected_codes() {
        let active = active_hangul_codes();
        let protected = active[..39].iter().copied().collect::<BTreeSet<_>>();
        let proof = prove_selected_assignment_capacity(5, &protected).unwrap();

        assert_eq!(proof.maximum_collision_count, 5);
        assert_eq!(proof.identity_code_count_at_maximum_collision, 0);
        assert_eq!(proof.remaining_safe_code_count, 166);
    }

    #[test]
    fn selected_assignment_is_deterministic() {
        let active = active_hangul_codes();
        let selected = BTreeSet::from([0, 2, 5, 9]);
        let protected = BTreeSet::from([active[0], active[5], active[8]]);

        let first = assign_selected_physical_codes(&selected, &protected).unwrap();
        let second = assign_selected_physical_codes(&selected, &protected).unwrap();

        assert_eq!(first.assignment_sha1, second.assignment_sha1);
        assert_eq!(first.color_codes, second.color_codes);
        assert_eq!(
            first.protected_code_remap_targets,
            second.protected_code_remap_targets
        );
    }

    #[test]
    fn custom_canonical_map_controls_which_selected_colors_collide() {
        let active = active_hangul_codes();
        let protected = BTreeSet::from([active[0], active[1]]);
        let mut canonical = active.clone();
        canonical.swap(0, 10);
        canonical.swap(1, 11);
        let selected = BTreeSet::from([0, 1, 10]);

        let assignment =
            assign_selected_physical_codes_with_canonical_map(&selected, &protected, &canonical)
                .unwrap();

        assert_eq!(assignment.collision_count, 1);
        assert_eq!(assignment.remap_pairs.len(), 1);
        assert_eq!(assignment.color_codes[&0], canonical[0]);
        assert_eq!(assignment.color_codes[&1], canonical[1]);
        assert_ne!(assignment.color_codes[&10], canonical[10]);
    }
}
