use std::{cmp::Reverse, collections::BTreeMap};

use anyhow::{Result, ensure};
use serde::Serialize;

use crate::sha1_hex;

use super::ColorMask;

#[derive(Debug, Serialize)]
pub(super) struct ExactModeledMaximum {
    search_strategy: &'static str,
    pub(super) player_choice_index: usize,
    pub(super) enemy_choice_index: usize,
    pub(super) terrain_left_index: usize,
    pub(super) terrain_right_index: usize,
    pub(super) dialogue_choice_index: usize,
    pub(super) overlay_glyph_count: usize,
    union_mask_sha1: String,
    glyph_characters_emitted: bool,
    translation_text_emitted: bool,
}

#[derive(Clone, Copy)]
struct IndexedChoice {
    source_index: usize,
    mask: ColorMask,
    nonbase_count: usize,
}

#[derive(Clone, Copy)]
struct ContextChoice {
    dialogue_choice_index: usize,
    terrain_pair_index: usize,
    mask: ColorMask,
}

pub(super) fn find_exact_modeled_maximum(
    base: ColorMask,
    player_choices: &[ColorMask],
    enemy_choices: &[ColorMask],
    terrain_entries: &[ColorMask],
    dialogue_choices: &[ColorMask],
    conservative_upper_bound: usize,
) -> Result<ExactModeledMaximum> {
    ensure!(
        !player_choices.is_empty(),
        "exact demand has no player choices"
    );
    ensure!(
        !enemy_choices.is_empty(),
        "exact demand has no enemy choices"
    );
    ensure!(
        !terrain_entries.is_empty(),
        "exact demand has no terrain choices"
    );
    ensure!(
        !dialogue_choices.is_empty(),
        "exact demand has no dialogue choices"
    );

    let mut players = distinct_choices(player_choices, base);
    let mut enemies = distinct_choices(enemy_choices, base);
    players.sort_unstable_by_key(|choice| (Reverse(choice.nonbase_count), choice.source_index));
    enemies.sort_unstable_by_key(|choice| (Reverse(choice.nonbase_count), choice.source_index));
    let maximum_player_nonbase_count = players[0].nonbase_count;
    let maximum_enemy_nonbase_count = enemies[0].nonbase_count;
    let mut contexts = distinct_contexts(terrain_entries, dialogue_choices);
    contexts.sort_unstable_by_key(|choice| {
        (
            Reverse(choice.mask.count()),
            choice.dialogue_choice_index,
            choice.terrain_pair_index,
        )
    });

    let mut best: Option<(ContextChoice, IndexedChoice, IndexedChoice, ColorMask)> = None;
    for context in contexts {
        let best_count = best.map_or(0, |(_, _, _, mask)| mask.count());
        if context.mask.count() + maximum_player_nonbase_count + maximum_enemy_nonbase_count
            <= best_count
        {
            continue;
        }

        let mut player_contexts = players
            .iter()
            .copied()
            .map(|player| (context.mask.union(player.mask), player))
            .collect::<Vec<_>>();
        player_contexts
            .sort_unstable_by_key(|(mask, player)| (Reverse(mask.count()), player.source_index));
        for (player_context, player) in player_contexts {
            let best_count = best.map_or(0, |(_, _, _, mask)| mask.count());
            if player_context.count() + maximum_enemy_nonbase_count <= best_count {
                break;
            }
            for enemy in &enemies {
                let best_count = best.map_or(0, |(_, _, _, mask)| mask.count());
                if player_context.count() + enemy.nonbase_count <= best_count {
                    break;
                }
                let union = player_context.union(enemy.mask);
                if union.count() > best_count {
                    best = Some((context, player, *enemy, union));
                    if union.count() == conservative_upper_bound {
                        return Ok(witness(
                            context,
                            player,
                            *enemy,
                            union,
                            terrain_entries.len(),
                        ));
                    }
                }
            }
        }
    }

    let (context, player, enemy, union) = best.expect("nonempty choice families have a witness");
    Ok(witness(
        context,
        player,
        enemy,
        union,
        terrain_entries.len(),
    ))
}

fn distinct_choices(choices: &[ColorMask], base: ColorMask) -> Vec<IndexedChoice> {
    let mut first_source_indices = BTreeMap::new();
    for (source_index, mask) in choices.iter().copied().enumerate() {
        first_source_indices.entry(mask).or_insert(source_index);
    }
    first_source_indices
        .into_iter()
        .map(|(mask, source_index)| IndexedChoice {
            source_index,
            mask,
            nonbase_count: mask.without(base).count(),
        })
        .collect()
}

fn distinct_contexts(
    terrain_entries: &[ColorMask],
    dialogue_choices: &[ColorMask],
) -> Vec<ContextChoice> {
    let mut first_sources = BTreeMap::new();
    for (dialogue_choice_index, dialogue) in dialogue_choices.iter().copied().enumerate() {
        for (terrain_left_index, left) in terrain_entries.iter().copied().enumerate() {
            for (terrain_right_index, right) in terrain_entries.iter().copied().enumerate() {
                let terrain_pair_index =
                    terrain_left_index * terrain_entries.len() + terrain_right_index;
                first_sources
                    .entry(dialogue.union(left).union(right))
                    .or_insert((dialogue_choice_index, terrain_pair_index));
            }
        }
    }
    first_sources
        .into_iter()
        .map(
            |(mask, (dialogue_choice_index, terrain_pair_index))| ContextChoice {
                dialogue_choice_index,
                terrain_pair_index,
                mask,
            },
        )
        .collect()
}

fn witness(
    context: ContextChoice,
    player: IndexedChoice,
    enemy: IndexedChoice,
    union: ColorMask,
    terrain_entry_count: usize,
) -> ExactModeledMaximum {
    let mut mask_bytes = Vec::new();
    for word in union.0 {
        mask_bytes.extend_from_slice(&word.to_le_bytes());
    }
    ExactModeledMaximum {
        search_strategy: "distinct dialogue-terrain contexts with exact participant branch-and-bound",
        player_choice_index: player.source_index,
        enemy_choice_index: enemy.source_index,
        terrain_left_index: context.terrain_pair_index / terrain_entry_count,
        terrain_right_index: context.terrain_pair_index % terrain_entry_count,
        dialogue_choice_index: context.dialogue_choice_index,
        overlay_glyph_count: union.count(),
        union_mask_sha1: sha1_hex(&mask_bytes),
        glyph_characters_emitted: false,
        translation_text_emitted: false,
    }
}

#[cfg(test)]
impl ExactModeledMaximum {
    pub(super) fn test_witness(overlay_glyph_count: usize) -> Self {
        Self {
            search_strategy: "test witness",
            player_choice_index: 0,
            enemy_choice_index: 0,
            terrain_left_index: 0,
            terrain_right_index: 0,
            dialogue_choice_index: 0,
            overlay_glyph_count,
            union_mask_sha1: "witness".to_owned(),
            glyph_characters_emitted: false,
            translation_text_emitted: false,
        }
    }
}
