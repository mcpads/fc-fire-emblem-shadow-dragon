use std::collections::{BTreeMap, BTreeSet};

use anyhow::{Context, Result, ensure};
use serde::Serialize;

use crate::{font_slots::active_hangul_codes, sha1_hex};

use super::conflict_graph::{BattleGlyphFamilies, StableColoringPlan};

mod borrowed_logical_codes;

use borrowed_logical_codes::select_source_safe_borrowed_codes;

const COLOR_MASK_WORD_COUNT: usize = 4;

#[derive(Debug)]
pub(super) struct ProtectedColorPlacementPlan {
    pub(super) canonical_color_codes: Vec<u8>,
    pub(super) protected_abstract_colors: Vec<u8>,
    pub(super) borrowed_logical_code_count: usize,
    pub(super) conservative_collision_count: usize,
    pub(super) report: ProtectedColorPlacementReport,
}

#[derive(Clone, Debug, Serialize)]
pub(super) struct ProtectedColorPlacementReport {
    strategy: &'static str,
    abstract_color_count: usize,
    active_physical_code_count: usize,
    borrowed_logical_code_count: usize,
    borrowed_logical_codes_sha1: String,
    preserved_literal_code_count: usize,
    protected_physical_code_count: usize,
    protected_abstract_color_count: usize,
    common_protected_color_count: usize,
    maximum_additional_collisions_by_family: BTreeMap<&'static str, usize>,
    conservative_maximum_collision_count: usize,
    runtime_pair_table_byte_count: usize,
    protected_abstract_colors_sha1: String,
    canonical_assignment_sha1: String,
    every_choice_family_included: bool,
    cross_family_overlap_credited: bool,
    globally_optimal_placement_proven: bool,
    translation_text_emitted: bool,
    glyph_characters_emitted: bool,
}

#[cfg(test)]
pub(super) fn test_report() -> ProtectedColorPlacementReport {
    ProtectedColorPlacementReport {
        strategy: "test",
        abstract_color_count: 210,
        active_physical_code_count: 210,
        borrowed_logical_code_count: 0,
        borrowed_logical_codes_sha1: sha1_hex(&[]),
        preserved_literal_code_count: 0,
        protected_physical_code_count: 39,
        protected_abstract_color_count: 39,
        common_protected_color_count: 0,
        maximum_additional_collisions_by_family: BTreeMap::new(),
        conservative_maximum_collision_count: 4,
        runtime_pair_table_byte_count: 9,
        protected_abstract_colors_sha1: "colors".to_owned(),
        canonical_assignment_sha1: "canonical".to_owned(),
        every_choice_family_included: true,
        cross_family_overlap_credited: false,
        globally_optimal_placement_proven: false,
        translation_text_emitted: false,
        glyph_characters_emitted: false,
    }
}

#[derive(Clone, Copy, Default)]
struct ColorMask([u64; COLOR_MASK_WORD_COUNT]);

struct ChoiceFamily {
    role: &'static str,
    choices: Vec<ColorMask>,
}

#[derive(Clone, Debug, Eq, Ord, PartialEq, PartialOrd)]
struct CandidateScore {
    conservative_collision_count: usize,
    maximum_family_collision_count: usize,
    common_collision_count: usize,
    occurrence_count: usize,
    color: usize,
}

pub(super) fn plan_protected_color_placement(
    families: &BattleGlyphFamilies,
    coloring: &StableColoringPlan,
    protected_physical_codes: &BTreeSet<u8>,
    preserved_literal_codes: &BTreeSet<u8>,
) -> Result<ProtectedColorPlacementPlan> {
    let active_codes = active_hangul_codes();
    let active_set = active_codes.iter().copied().collect::<BTreeSet<_>>();
    ensure!(
        coloring.color_count >= active_codes.len() && coloring.color_count <= usize::from(u8::MAX),
        "protected battle color placement needs {} logical colors outside its supported {}..={} range",
        coloring.color_count,
        active_codes.len(),
        u8::MAX,
    );
    ensure!(
        protected_physical_codes.is_subset(&active_set),
        "protected battle color placement includes a reserved font code"
    );
    let borrowed_logical_code_count = coloring.color_count - active_codes.len();
    let borrowed_logical_codes =
        select_source_safe_borrowed_codes(borrowed_logical_code_count, preserved_literal_codes)?;
    ensure!(
        borrowed_logical_codes.is_disjoint(&active_set),
        "battle borrowed logical codes overlap the active physical code pool"
    );
    let protected_source_codes = protected_physical_codes
        .union(&borrowed_logical_codes)
        .copied()
        .collect::<BTreeSet<_>>();

    let common = glyph_mask(&families.base, coloring)?;
    let mut choice_families = Vec::new();
    let mut participant_family_pairs = Vec::new();
    for mode in &families.participant_modes {
        let player_index = choice_families.len();
        choice_families.push(ChoiceFamily {
            role: mode.role,
            choices: choice_masks(&mode.player_participants, coloring, common)?,
        });
        let enemy_index = choice_families.len();
        choice_families.push(ChoiceFamily {
            role: mode.role,
            choices: choice_masks(&mode.enemy_participants, coloring, common)?,
        });
        participant_family_pairs.push((player_index, enemy_index));
    }
    let terrain_family_index = choice_families.len();
    choice_families.push(ChoiceFamily {
        role: "two_terrains",
        choices: terrain_pair_masks(&families.terrains, coloring, common)?,
    });
    let dialogue_family_index = choice_families.len();
    choice_families.push(ChoiceFamily {
        role: "dialogue_record",
        choices: choice_masks(&families.dialogue_records, coloring, common)?,
    });
    for family in &choice_families {
        ensure!(
            !family.choices.is_empty(),
            "protected battle color placement has no {} choices",
            family.role
        );
    }

    let (selected_colors, common_collision_count, family_collision_counts) =
        select_protected_colors(
            coloring.color_count,
            protected_source_codes.len(),
            common,
            &choice_families,
            &participant_family_pairs,
            &[terrain_family_index, dialogue_family_index],
        )?;

    let maximum_participant_pair_collision_count = participant_family_pairs
        .iter()
        .map(|(player, enemy)| {
            maximum_collision_count(&family_collision_counts[*player])
                + maximum_collision_count(&family_collision_counts[*enemy])
        })
        .max()
        .unwrap_or(0);
    let maximum_additional_collisions_by_family = BTreeMap::from([
        ("participant_pair", maximum_participant_pair_collision_count),
        (
            "two_terrains",
            maximum_collision_count(&family_collision_counts[terrain_family_index]),
        ),
        (
            "dialogue_record",
            maximum_collision_count(&family_collision_counts[dialogue_family_index]),
        ),
    ]);
    let conservative_collision_count = common_collision_count
        + maximum_additional_collisions_by_family
            .values()
            .copied()
            .sum::<usize>();
    ensure!(
        conservative_collision_count <= protected_physical_codes.len(),
        "protected battle color placement collision bound exceeds its protected colors"
    );

    let canonical_color_codes = assign_canonical_codes(
        coloring.color_count,
        &selected_colors,
        &protected_source_codes,
        protected_physical_codes,
    )?;
    let selected_color_bytes = selected_colors
        .iter()
        .map(|color| u8::try_from(*color).context("protected abstract color exceeds one byte"))
        .collect::<Result<Vec<_>>>()?;
    let canonical_assignment_bytes = canonical_color_codes
        .iter()
        .copied()
        .enumerate()
        .flat_map(|(color, code)| [color as u8, code])
        .collect::<Vec<_>>();
    let runtime_pair_table_byte_count = 1 + conservative_collision_count * 2;
    Ok(ProtectedColorPlacementPlan {
        canonical_color_codes,
        protected_abstract_colors: selected_color_bytes.clone(),
        borrowed_logical_code_count,
        conservative_collision_count,
        report: ProtectedColorPlacementReport {
            strategy: "use source-safe preserved-display codes only for logical colors above the active physical width, then place every protected canonical source code outside always-live common colors and minimize the conservative selected-family remap bound",
            abstract_color_count: coloring.color_count,
            active_physical_code_count: active_codes.len(),
            borrowed_logical_code_count,
            borrowed_logical_codes_sha1: sha1_hex(
                &borrowed_logical_codes.iter().copied().collect::<Vec<_>>(),
            ),
            preserved_literal_code_count: preserved_literal_codes.len(),
            protected_physical_code_count: protected_physical_codes.len(),
            protected_abstract_color_count: selected_colors.len(),
            common_protected_color_count: common_collision_count,
            maximum_additional_collisions_by_family,
            conservative_maximum_collision_count: conservative_collision_count,
            runtime_pair_table_byte_count,
            protected_abstract_colors_sha1: sha1_hex(&selected_color_bytes),
            canonical_assignment_sha1: sha1_hex(&canonical_assignment_bytes),
            every_choice_family_included: true,
            cross_family_overlap_credited: false,
            globally_optimal_placement_proven: false,
            translation_text_emitted: false,
            glyph_characters_emitted: false,
        },
    })
}

fn select_protected_colors(
    color_count: usize,
    protected_color_count: usize,
    common: ColorMask,
    choice_families: &[ChoiceFamily],
    participant_family_pairs: &[(usize, usize)],
    independent_family_indices: &[usize],
) -> Result<(BTreeSet<usize>, usize, Vec<Vec<usize>>)> {
    ensure!(
        color_count <= COLOR_MASK_WORD_COUNT * u64::BITS as usize,
        "protected battle color placement exceeds its mask capacity"
    );
    let non_common_color_count = (0..color_count)
        .filter(|color| !common.contains(*color))
        .count();
    ensure!(
        non_common_color_count >= protected_color_count,
        "protected battle color placement has only {non_common_color_count} colors outside the always-live common set"
    );
    let color_occurrences = (0..color_count)
        .map(|color| {
            choice_families
                .iter()
                .flat_map(|family| &family.choices)
                .filter(|choice| choice.contains(color))
                .count()
        })
        .collect::<Vec<_>>();
    let mut selected = ColorMask::default();
    let mut selected_colors = BTreeSet::new();
    let mut family_collision_counts = choice_families
        .iter()
        .map(|family| vec![0usize; family.choices.len()])
        .collect::<Vec<_>>();
    let mut common_collision_count = 0usize;
    while selected_colors.len() < protected_color_count {
        let candidate = (0..color_count)
            .filter(|color| !selected.contains(*color) && !common.contains(*color))
            .map(|color| {
                score_candidate(
                    color,
                    common,
                    common_collision_count,
                    choice_families,
                    &family_collision_counts,
                    participant_family_pairs,
                    independent_family_indices,
                    color_occurrences[color],
                )
            })
            .min()
            .context("protected battle color placement has no remaining color")?;
        let color = candidate.color;
        selected.insert(color);
        selected_colors.insert(color);
        common_collision_count += usize::from(common.contains(color));
        for (family, counts) in choice_families.iter().zip(&mut family_collision_counts) {
            for (choice, count) in family.choices.iter().zip(counts) {
                *count += usize::from(choice.contains(color));
            }
        }
    }
    Ok((
        selected_colors,
        common_collision_count,
        family_collision_counts,
    ))
}

fn score_candidate(
    color: usize,
    common: ColorMask,
    common_collision_count: usize,
    families: &[ChoiceFamily],
    family_collision_counts: &[Vec<usize>],
    participant_family_pairs: &[(usize, usize)],
    independent_family_indices: &[usize],
    occurrence_count: usize,
) -> CandidateScore {
    let common_collision_count = common_collision_count + usize::from(common.contains(color));
    let family_maxima = families
        .iter()
        .zip(family_collision_counts)
        .map(|(family, counts)| {
            family
                .choices
                .iter()
                .zip(counts)
                .map(|(choice, count)| *count + usize::from(choice.contains(color)))
                .max()
                .unwrap_or(0)
        })
        .collect::<Vec<_>>();
    let participant_collision_count = participant_family_pairs
        .iter()
        .map(|(player, enemy)| family_maxima[*player] + family_maxima[*enemy])
        .max()
        .unwrap_or(0);
    let independent_collision_count = independent_family_indices
        .iter()
        .map(|index| family_maxima[*index])
        .sum::<usize>();
    CandidateScore {
        conservative_collision_count: common_collision_count
            + participant_collision_count
            + independent_collision_count,
        maximum_family_collision_count: family_maxima.iter().copied().max().unwrap_or(0),
        common_collision_count,
        occurrence_count,
        color,
    }
}

fn maximum_collision_count(counts: &[usize]) -> usize {
    counts.iter().copied().max().unwrap_or(0)
}

fn choice_masks(
    choices: &[BTreeSet<char>],
    coloring: &StableColoringPlan,
    common: ColorMask,
) -> Result<Vec<ColorMask>> {
    choices
        .iter()
        .map(|choice| Ok(glyph_mask(choice, coloring)?.without(common)))
        .collect()
}

fn terrain_pair_masks(
    terrains: &[BTreeSet<char>],
    coloring: &StableColoringPlan,
    common: ColorMask,
) -> Result<Vec<ColorMask>> {
    let terrains = choice_masks(terrains, coloring, common)?;
    let mut pairs = Vec::with_capacity(terrains.len() * terrains.len());
    for left in &terrains {
        for right in &terrains {
            pairs.push(left.union(*right));
        }
    }
    Ok(pairs)
}

fn glyph_mask(glyphs: &BTreeSet<char>, coloring: &StableColoringPlan) -> Result<ColorMask> {
    let mut mask = ColorMask::default();
    for glyph in glyphs {
        let color = coloring
            .glyph_colors()
            .get(glyph)
            .copied()
            .with_context(|| {
                format!("protected battle color placement has uncolored glyph {glyph:?}")
            })?;
        ensure!(
            color < coloring.color_count,
            "protected battle color placement color exceeds the codebook"
        );
        mask.insert(color);
    }
    Ok(mask)
}

fn assign_canonical_codes(
    color_count: usize,
    protected_abstract_colors: &BTreeSet<usize>,
    protected_source_codes: &BTreeSet<u8>,
    protected_physical_codes: &BTreeSet<u8>,
) -> Result<Vec<u8>> {
    ensure!(
        protected_abstract_colors.len() == protected_source_codes.len(),
        "protected battle canonical assignment cardinality changed"
    );
    let active_codes = active_hangul_codes();
    ensure!(
        color_count
            == active_codes.len() + protected_source_codes.len() - protected_physical_codes.len(),
        "protected battle canonical assignment logical and physical partitions disagree"
    );
    let safe_physical_codes = active_codes
        .iter()
        .copied()
        .filter(|code| !protected_physical_codes.contains(code))
        .collect::<Vec<_>>();
    let safe_abstract_colors = (0..color_count)
        .filter(|color| !protected_abstract_colors.contains(color))
        .collect::<Vec<_>>();
    ensure!(
        safe_abstract_colors.len() == safe_physical_codes.len(),
        "protected battle canonical safe partitions differ"
    );
    let mut assignments = vec![None; color_count];
    for (color, code) in protected_abstract_colors
        .iter()
        .copied()
        .zip(protected_source_codes.iter().copied())
    {
        assignments[color] = Some(code);
    }
    for (color, code) in safe_abstract_colors.into_iter().zip(safe_physical_codes) {
        assignments[color] = Some(code);
    }
    let assignments = assignments
        .into_iter()
        .map(|assignment| assignment.context("protected battle canonical assignment lost a color"))
        .collect::<Result<Vec<_>>>()?;
    ensure!(
        assignments.iter().copied().collect::<BTreeSet<_>>().len() == assignments.len(),
        "protected battle canonical assignment reused a physical code"
    );
    Ok(assignments)
}

impl ColorMask {
    fn insert(&mut self, color: usize) {
        self.0[color / u64::BITS as usize] |= 1 << (color % u64::BITS as usize);
    }

    fn contains(self, color: usize) -> bool {
        self.0[color / u64::BITS as usize] & (1 << (color % u64::BITS as usize)) != 0
    }

    fn union(self, other: Self) -> Self {
        let mut words = self.0;
        for (word, other_word) in words.iter_mut().zip(other.0) {
            *word |= other_word;
        }
        Self(words)
    }

    fn without(self, other: Self) -> Self {
        let mut words = self.0;
        for (word, other_word) in words.iter_mut().zip(other.0) {
            *word &= !other_word;
        }
        Self(words)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn canonical_assignment_places_selected_colors_on_protected_codes() {
        let active = active_hangul_codes();
        let protected_codes = BTreeSet::from([active[1], active[3]]);
        let protected_colors = BTreeSet::from([0, 2]);
        let assignment = assign_canonical_codes(
            active.len(),
            &protected_colors,
            &protected_codes,
            &protected_codes,
        )
        .unwrap();

        assert_eq!(
            protected_colors
                .iter()
                .map(|color| assignment[*color])
                .collect::<BTreeSet<_>>(),
            protected_codes
        );
        assert_eq!(
            assignment.iter().copied().collect::<BTreeSet<_>>().len(),
            active.len()
        );
    }

    #[test]
    fn placement_avoids_always_live_color_and_spreads_across_alternatives() {
        let mut common = ColorMask::default();
        common.insert(0);
        let choices = (1..=4)
            .map(|color| {
                let mut choice = ColorMask::default();
                choice.insert(color);
                choice
            })
            .collect();
        let families = [ChoiceFamily {
            role: "alternatives",
            choices,
        }];

        let (selected, common_count, family_counts) =
            select_protected_colors(5, 2, common, &families, &[], &[0]).unwrap();

        assert_eq!(selected.len(), 2);
        assert!(!selected.contains(&0));
        assert_eq!(common_count, 0);
        assert_eq!(family_counts[0].iter().copied().max(), Some(1));
    }
}
