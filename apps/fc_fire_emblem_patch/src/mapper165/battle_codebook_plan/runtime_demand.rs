use std::collections::{BTreeMap, BTreeSet};

use anyhow::{Context, Result, ensure};
use serde::Serialize;

use crate::{font_slots::ACTIVE_HANGUL_SLOT_COUNT, sha1_hex};

use super::conflict_graph::{BattleGlyphFamilies, StableColoringPlan};

mod exact_maximum;

use exact_maximum::{ExactModeledMaximum, find_exact_modeled_maximum};

const ABSTRACT_COLOR_CAPACITY: usize = 256;
const MASK_WORD_COUNT: usize = ABSTRACT_COLOR_CAPACITY.div_ceil(u64::BITS as usize);

#[derive(Clone, Copy, Debug, Default, Eq, Ord, PartialEq, PartialOrd)]
struct ColorMask([u64; MASK_WORD_COUNT]);

struct ParticipantModeMasks {
    role: &'static str,
    players: Vec<ColorMask>,
    enemies: Vec<ColorMask>,
}

#[derive(Debug, Serialize)]
pub(super) struct BattleRuntimeDemandPlan {
    strategy: &'static str,
    participant_mode_count: usize,
    participant_mode_roles: Vec<&'static str>,
    player_participant_candidate_count: usize,
    enemy_participant_candidate_count: usize,
    terrain_entry_count: usize,
    dialogue_record_count: usize,
    distinct_player_choice_count: usize,
    distinct_enemy_choice_count: usize,
    distinct_terrain_pair_choice_count: usize,
    distinct_dialogue_choice_count: usize,
    common_overlay_glyph_count: usize,
    maximum_new_overlay_glyph_counts: BTreeMap<&'static str, usize>,
    conservative_maximum_overlay_glyph_count: usize,
    active_slot_count: usize,
    minimum_graphics_headroom: usize,
    choice_family_sha1: String,
    every_choice_family_source_bound: bool,
    family_maxima_added_without_cross_family_overlap_credit: bool,
    conservative_upper_bound_proven: bool,
    exact_modeled_maximum_overlay_glyph_count: usize,
    exact_modeled_minimum_graphics_headroom: usize,
    conservative_upper_bound_is_tight: bool,
    exact_modeled_maximum_witness: ExactModeledMaximum,
    exact_modeled_maximum_runtime_input: Option<ExactModeledRuntimeInput>,
    exact_modeled_maximum_proven: bool,
    glyph_characters_emitted: bool,
    translation_text_emitted: bool,
}

#[derive(Clone, Copy, Debug, Serialize)]
pub(super) struct ExactModeledRuntimeInput {
    pub(super) staged_participant_identities: [u8; 2],
    pub(super) class_record_identities: [u8; 2],
    pub(super) item_source_indices: [u8; 2],
    pub(super) terrain_source_indices: [u8; 2],
    pub(super) dialogue_selector: u8,
}

pub(super) fn plan_runtime_demand(
    families: &BattleGlyphFamilies,
    coloring: &StableColoringPlan,
) -> Result<BattleRuntimeDemandPlan> {
    ensure!(
        coloring.color_count <= ABSTRACT_COLOR_CAPACITY,
        "battle runtime demand exceeds the one-byte abstract-color capacity"
    );
    let base = mask_for(&families.base, coloring)?;
    let participant_modes = families
        .participant_modes
        .iter()
        .map(|mode| {
            Ok(ParticipantModeMasks {
                role: mode.role,
                players: choice_masks(&mode.player_participants, coloring, base)?,
                enemies: choice_masks(&mode.enemy_participants, coloring, base)?,
            })
        })
        .collect::<Result<Vec<_>>>()?;
    let terrain_entries = choice_masks(&families.terrains, coloring, base)?;
    let terrains = pair_choices(&terrain_entries)?;
    let dialogues = choice_masks(&families.dialogue_records, coloring, base)?;
    ensure!(
        !participant_modes.is_empty(),
        "battle runtime demand has no participant modes"
    );
    for mode in &participant_modes {
        ensure!(
            !mode.players.is_empty() && !mode.enemies.is_empty(),
            "battle runtime demand has an empty {} participant mode",
            mode.role
        );
    }
    for (role, choices) in [("terrain pair", &terrains), ("dialogue", &dialogues)] {
        ensure!(
            !choices.is_empty(),
            "battle runtime demand has no {role} choices"
        );
    }

    let maximum_participant_pair_count = participant_modes
        .iter()
        .map(|mode| {
            maximum_new_color_count(&mode.players, base)
                + maximum_new_color_count(&mode.enemies, base)
        })
        .max()
        .context("battle runtime demand has no participant pair")?;
    let maximum_new_overlay_glyph_counts = BTreeMap::from([
        ("participant_pair", maximum_participant_pair_count),
        ("two_terrains", maximum_new_color_count(&terrains, base)),
        ("dialogue_record", maximum_new_color_count(&dialogues, base)),
    ]);
    let conservative_maximum_overlay_glyph_count = base.count()
        + maximum_new_overlay_glyph_counts
            .values()
            .copied()
            .sum::<usize>();
    ensure!(
        conservative_maximum_overlay_glyph_count <= ACTIVE_HANGUL_SLOT_COUNT,
        "one modeled battle can require at most {conservative_maximum_overlay_glyph_count} overlays but only {ACTIVE_HANGUL_SLOT_COUNT} slots exist"
    );
    let mut exact_modeled_maximum = None;
    for (mode_index, mode) in participant_modes.iter().enumerate() {
        let candidate = find_exact_modeled_maximum(
            mode_index,
            base,
            &mode.players,
            &mode.enemies,
            &terrain_entries,
            &dialogues,
            conservative_maximum_overlay_glyph_count,
        )?;
        if exact_modeled_maximum
            .as_ref()
            .is_none_or(|current: &ExactModeledMaximum| {
                candidate.overlay_glyph_count > current.overlay_glyph_count
            })
        {
            exact_modeled_maximum = Some(candidate);
        }
    }
    let exact_modeled_maximum =
        exact_modeled_maximum.context("battle runtime demand has no exact mode witness")?;
    ensure!(
        exact_modeled_maximum.overlay_glyph_count <= conservative_maximum_overlay_glyph_count,
        "exact modeled battle demand exceeds its conservative upper bound"
    );

    Ok(BattleRuntimeDemandPlan {
        strategy: "source-bound common demand plus the largest producer-specific participant pair and independent terrain and dialogue maxima",
        participant_mode_count: participant_modes.len(),
        participant_mode_roles: participant_modes.iter().map(|mode| mode.role).collect(),
        player_participant_candidate_count: families
            .participant_modes
            .iter()
            .map(|mode| mode.player_participants.len())
            .sum(),
        enemy_participant_candidate_count: families
            .participant_modes
            .iter()
            .map(|mode| mode.enemy_participants.len())
            .sum(),
        terrain_entry_count: families.terrains.len(),
        dialogue_record_count: families.dialogue_records.len(),
        distinct_player_choice_count: participant_modes
            .iter()
            .map(|mode| distinct_choice_count(&mode.players))
            .sum(),
        distinct_enemy_choice_count: participant_modes
            .iter()
            .map(|mode| distinct_choice_count(&mode.enemies))
            .sum(),
        distinct_terrain_pair_choice_count: distinct_choice_count(&terrains),
        distinct_dialogue_choice_count: distinct_choice_count(&dialogues),
        common_overlay_glyph_count: base.count(),
        maximum_new_overlay_glyph_counts,
        conservative_maximum_overlay_glyph_count,
        active_slot_count: ACTIVE_HANGUL_SLOT_COUNT,
        minimum_graphics_headroom: ACTIVE_HANGUL_SLOT_COUNT
            - conservative_maximum_overlay_glyph_count,
        choice_family_sha1: choice_family_sha1(&participant_modes, &terrains, &dialogues),
        every_choice_family_source_bound: true,
        family_maxima_added_without_cross_family_overlap_credit: true,
        conservative_upper_bound_proven: true,
        exact_modeled_maximum_overlay_glyph_count: exact_modeled_maximum.overlay_glyph_count,
        exact_modeled_minimum_graphics_headroom: ACTIVE_HANGUL_SLOT_COUNT
            - exact_modeled_maximum.overlay_glyph_count,
        conservative_upper_bound_is_tight: exact_modeled_maximum.overlay_glyph_count
            == conservative_maximum_overlay_glyph_count,
        exact_modeled_maximum_witness: exact_modeled_maximum,
        exact_modeled_maximum_runtime_input: None,
        exact_modeled_maximum_proven: true,
        glyph_characters_emitted: false,
        translation_text_emitted: false,
    })
}

impl BattleRuntimeDemandPlan {
    pub(super) fn maximum_overlay_glyph_count(&self) -> usize {
        self.conservative_maximum_overlay_glyph_count
    }

    pub(super) fn exact_maximum_overlay_glyph_count(&self) -> usize {
        self.exact_modeled_maximum_overlay_glyph_count
    }

    pub(super) fn exact_witness_indices(&self) -> [usize; 6] {
        [
            self.exact_modeled_maximum_witness.participant_mode_index,
            self.exact_modeled_maximum_witness.player_choice_index,
            self.exact_modeled_maximum_witness.enemy_choice_index,
            self.exact_modeled_maximum_witness.terrain_left_index,
            self.exact_modeled_maximum_witness.terrain_right_index,
            self.exact_modeled_maximum_witness.dialogue_choice_index,
        ]
    }

    pub(super) fn bind_exact_runtime_input(
        &mut self,
        input: ExactModeledRuntimeInput,
    ) -> Result<()> {
        ensure!(
            self.exact_modeled_maximum_runtime_input.is_none(),
            "exact battle demand runtime input was already bound"
        );
        self.exact_modeled_maximum_runtime_input = Some(input);
        Ok(())
    }
}

fn choice_masks(
    choices: &[BTreeSet<char>],
    coloring: &StableColoringPlan,
    base: ColorMask,
) -> Result<Vec<ColorMask>> {
    choices
        .iter()
        .map(|choice| Ok(base.union(mask_for(choice, coloring)?)))
        .collect()
}

fn pair_choices(entries: &[ColorMask]) -> Result<Vec<ColorMask>> {
    let pair_count = entries
        .len()
        .checked_mul(entries.len())
        .context("battle runtime terrain-pair count overflow")?;
    let mut pairs = Vec::with_capacity(pair_count);
    for left in entries {
        for right in entries {
            pairs.push(left.union(*right));
        }
    }
    Ok(pairs)
}

fn mask_for(glyphs: &BTreeSet<char>, coloring: &StableColoringPlan) -> Result<ColorMask> {
    let mut mask = ColorMask::default();
    for glyph in glyphs {
        let color = coloring
            .glyph_colors()
            .get(glyph)
            .copied()
            .with_context(|| format!("battle runtime demand contains uncolored glyph {glyph:?}"))?;
        ensure!(
            color < ABSTRACT_COLOR_CAPACITY,
            "battle runtime demand color {color} exceeds the one-byte abstract-color space"
        );
        ensure!(
            !mask.contains(color),
            "one battle choice maps two glyphs to abstract color {color}"
        );
        mask.insert(color);
    }
    Ok(mask)
}

fn maximum_new_color_count(choices: &[ColorMask], base: ColorMask) -> usize {
    choices
        .iter()
        .map(|choice| choice.without(base).count())
        .max()
        .unwrap_or(0)
}

fn distinct_choice_count(choices: &[ColorMask]) -> usize {
    choices.iter().copied().collect::<BTreeSet<_>>().len()
}

fn choice_family_sha1(
    participant_modes: &[ParticipantModeMasks],
    terrains: &[ColorMask],
    dialogues: &[ColorMask],
) -> String {
    let mut bytes = Vec::new();
    for mode in participant_modes {
        bytes.extend_from_slice(mode.role.as_bytes());
        bytes.push(0);
        append_choice_masks(&mut bytes, &mode.players);
        append_choice_masks(&mut bytes, &mode.enemies);
    }
    append_choice_masks(&mut bytes, terrains);
    append_choice_masks(&mut bytes, dialogues);
    sha1_hex(&bytes)
}

fn append_choice_masks(bytes: &mut Vec<u8>, family: &[ColorMask]) {
    bytes.extend_from_slice(&(family.len() as u64).to_le_bytes());
    for mask in family {
        for word in mask.0 {
            bytes.extend_from_slice(&word.to_le_bytes());
        }
    }
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

    fn count(self) -> usize {
        self.0.iter().map(|word| word.count_ones() as usize).sum()
    }
}

#[cfg(test)]
pub(super) fn test_plan() -> BattleRuntimeDemandPlan {
    BattleRuntimeDemandPlan {
        strategy: "conservative upper bound",
        participant_mode_count: 1,
        participant_mode_roles: vec!["test"],
        player_participant_candidate_count: 1,
        enemy_participant_candidate_count: 1,
        terrain_entry_count: 1,
        dialogue_record_count: 1,
        distinct_player_choice_count: 1,
        distinct_enemy_choice_count: 1,
        distinct_terrain_pair_choice_count: 1,
        distinct_dialogue_choice_count: 1,
        common_overlay_glyph_count: 1,
        maximum_new_overlay_glyph_counts: BTreeMap::new(),
        conservative_maximum_overlay_glyph_count: 1,
        active_slot_count: ACTIVE_HANGUL_SLOT_COUNT,
        minimum_graphics_headroom: ACTIVE_HANGUL_SLOT_COUNT - 1,
        choice_family_sha1: "choices".to_owned(),
        every_choice_family_source_bound: true,
        family_maxima_added_without_cross_family_overlap_credit: true,
        conservative_upper_bound_proven: true,
        exact_modeled_maximum_overlay_glyph_count: 1,
        exact_modeled_minimum_graphics_headroom: ACTIVE_HANGUL_SLOT_COUNT - 1,
        conservative_upper_bound_is_tight: true,
        exact_modeled_maximum_witness: ExactModeledMaximum::test_witness(1),
        exact_modeled_maximum_runtime_input: Some(ExactModeledRuntimeInput {
            staged_participant_identities: [1, 1],
            class_record_identities: [1, 1],
            item_source_indices: [0, 0],
            terrain_source_indices: [0, 0],
            dialogue_selector: 0,
        }),
        exact_modeled_maximum_proven: true,
        glyph_characters_emitted: false,
        translation_text_emitted: false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::mapper165::battle_codebook_plan::conflict_graph::{
        BattleParticipantMode, plan_stable_coloring,
    };

    fn set(glyphs: &str) -> BTreeSet<char> {
        glyphs.chars().collect()
    }

    #[test]
    fn demand_bound_does_not_credit_cross_family_overlap() {
        let families = BattleGlyphFamilies {
            base: set("가"),
            participant_modes: vec![BattleParticipantMode {
                role: "test",
                player_participants: vec![set("나"), set("나다")],
                enemy_participants: vec![set("라")],
            }],
            terrains: vec![set("마"), set("바")],
            dialogue_records: vec![set("사"), set("사아")],
        };
        let coloring = plan_stable_coloring(&families, ACTIVE_HANGUL_SLOT_COUNT).unwrap();
        let demand = plan_runtime_demand(&families, &coloring).unwrap();

        assert_eq!(demand.common_overlay_glyph_count, 1);
        assert_eq!(
            demand.maximum_new_overlay_glyph_counts["participant_pair"],
            3
        );
        assert_eq!(demand.maximum_new_overlay_glyph_counts["two_terrains"], 2);
        assert_eq!(demand.conservative_maximum_overlay_glyph_count, 8);
        assert_eq!(
            demand.minimum_graphics_headroom,
            ACTIVE_HANGUL_SLOT_COUNT - 8
        );
        assert_eq!(demand.exact_modeled_maximum_overlay_glyph_count, 8);
        assert!(demand.conservative_upper_bound_is_tight);
        assert!(demand.exact_modeled_maximum_proven);
    }

    #[test]
    fn exact_demand_does_not_double_count_glyphs_shared_across_families() {
        let families = BattleGlyphFamilies {
            base: set("가"),
            participant_modes: vec![BattleParticipantMode {
                role: "test",
                player_participants: vec![set("나다")],
                enemy_participants: vec![set("라")],
            }],
            terrains: vec![set("나")],
            dialogue_records: vec![set("다")],
        };
        let coloring = plan_stable_coloring(&families, ACTIVE_HANGUL_SLOT_COUNT).unwrap();
        let demand = plan_runtime_demand(&families, &coloring).unwrap();

        assert_eq!(demand.conservative_maximum_overlay_glyph_count, 6);
        assert_eq!(demand.exact_modeled_maximum_overlay_glyph_count, 4);
        assert!(!demand.conservative_upper_bound_is_tight);
        assert_eq!(demand.exact_modeled_maximum_witness.overlay_glyph_count, 4);
    }

    #[test]
    fn alternative_participant_producers_are_not_crossed_into_one_fake_battle() {
        let families = BattleGlyphFamilies {
            base: BTreeSet::new(),
            participant_modes: vec![
                BattleParticipantMode {
                    role: "gameplay",
                    player_participants: vec![set("가나다")],
                    enemy_participants: vec![set("라마")],
                },
                BattleParticipantMode {
                    role: "sound_test",
                    player_participants: vec![set("바사")],
                    enemy_participants: vec![set("아자차")],
                },
            ],
            terrains: vec![BTreeSet::new()],
            dialogue_records: vec![BTreeSet::new()],
        };
        let coloring = plan_stable_coloring(&families, ACTIVE_HANGUL_SLOT_COUNT).unwrap();
        let demand = plan_runtime_demand(&families, &coloring).unwrap();

        assert_eq!(demand.participant_mode_count, 2);
        assert_eq!(
            demand.maximum_new_overlay_glyph_counts["participant_pair"],
            5
        );
        assert_eq!(demand.exact_modeled_maximum_overlay_glyph_count, 5);
    }
}
