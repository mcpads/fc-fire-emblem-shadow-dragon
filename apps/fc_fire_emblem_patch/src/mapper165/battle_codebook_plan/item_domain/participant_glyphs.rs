use std::collections::BTreeSet;

use anyhow::{Context, Result, ensure};

use crate::text_inventory::FixedTextPlan;

use super::{
    IDENTITY_RESTRICTED_LOADOUT_COUNT, ITEM_ENTRY_COUNT, UNIT_SOURCE_ENTRY_COUNT,
    UNRESTRICTED_LOADOUT_COUNT,
    eligibility_tables::{PlayerLoadoutCandidate, battle_item_source_index},
};

pub(super) struct BattleItemGlyphSets {
    pub(super) item_glyph_sets: Vec<BTreeSet<char>>,
    pub(super) player_participant_glyph_sets: Vec<BTreeSet<char>>,
    pub(super) player_participant_inputs: Vec<PlayerParticipantInput>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(in crate::mapper165::battle_codebook_plan) struct PlayerParticipantInput {
    pub(in crate::mapper165::battle_codebook_plan) identity: u8,
    pub(in crate::mapper165::battle_codebook_plan) class_id: u8,
    pub(in crate::mapper165::battle_codebook_plan) item_source_index: u8,
}

pub(super) fn plan_battle_item_glyph_sets(
    fixed: &FixedTextPlan,
    candidate_source_indices: &BTreeSet<usize>,
    player_loadouts: &BTreeSet<PlayerLoadoutCandidate>,
) -> Result<BattleItemGlyphSets> {
    let item_entries = fixed
        .entries
        .iter()
        .filter(|entry| entry.table_id == "item-names")
        .collect::<Vec<_>>();
    ensure!(
        item_entries.len() == ITEM_ENTRY_COUNT,
        "fixed-text item entry count does not match the action-flags table"
    );
    for (expected_source_index, entry) in item_entries.iter().enumerate() {
        ensure!(
            entry.source_index == expected_source_index,
            "fixed-text item source indices are not contiguous at {expected_source_index}"
        );
    }
    let item_glyph_sets = item_entries
        .into_iter()
        .filter(|entry| candidate_source_indices.contains(&entry.source_index))
        .map(|entry| entry.unique_glyphs())
        .collect::<Vec<_>>();
    ensure!(
        item_glyph_sets.len() == candidate_source_indices.len(),
        "battle item domain lost a candidate translation entry"
    );

    let unit_entries = fixed
        .entries
        .iter()
        .filter(|entry| entry.table_id == "unit-names")
        .collect::<Vec<_>>();
    ensure!(
        unit_entries.len() == UNIT_SOURCE_ENTRY_COUNT,
        "fixed-text unit entry count changed"
    );
    for (expected_source_index, entry) in unit_entries.iter().enumerate() {
        ensure!(
            entry.source_index == expected_source_index,
            "fixed-text unit source indices are not contiguous at {expected_source_index}"
        );
    }

    let mut player_participant_glyph_sets = Vec::new();
    let mut player_participant_inputs = Vec::new();
    for loadout in player_loadouts {
        let class_source_index = usize::from(loadout.class_id) - 1;
        let item_source_index = battle_item_source_index(loadout.item_id)?;
        let mut loadout_glyphs = fixed
            .entry_for_source_index("class-names", class_source_index)
            .with_context(|| format!("missing class translation for source {class_source_index}"))?
            .unique_glyphs();
        loadout_glyphs.extend(
            fixed
                .entry_for_source_index("item-names", item_source_index)
                .with_context(|| {
                    format!("missing item translation for source {item_source_index}")
                })?
                .unique_glyphs(),
        );
        let eligible_units = if loadout.required_identity == 0 {
            unit_entries.clone()
        } else {
            let unit_source_index = usize::from(loadout.required_identity) - 1;
            vec![*unit_entries.get(unit_source_index).with_context(|| {
                format!(
                    "identity-restricted item refers to missing unit source {unit_source_index}"
                )
            })?]
        };
        for unit in eligible_units {
            let mut glyphs = unit.unique_glyphs();
            glyphs.extend(&loadout_glyphs);
            player_participant_glyph_sets.push(glyphs);
            player_participant_inputs.push(PlayerParticipantInput {
                identity: u8::try_from(unit.source_index + 1)
                    .context("player participant identity exceeds one byte")?,
                class_id: loadout.class_id,
                item_source_index: u8::try_from(item_source_index)
                    .context("player item source index exceeds one byte")?,
            });
        }
    }
    let player_participant_candidate_count =
        UNRESTRICTED_LOADOUT_COUNT * UNIT_SOURCE_ENTRY_COUNT + IDENTITY_RESTRICTED_LOADOUT_COUNT;
    ensure!(
        player_participant_glyph_sets.len() == player_participant_candidate_count,
        "player participant candidate count changed"
    );
    ensure!(
        player_participant_inputs.len() == player_participant_glyph_sets.len(),
        "player participant inputs and glyph sets lost alignment"
    );
    Ok(BattleItemGlyphSets {
        item_glyph_sets,
        player_participant_glyph_sets,
        player_participant_inputs,
    })
}
