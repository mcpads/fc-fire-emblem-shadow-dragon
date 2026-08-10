use std::collections::BTreeSet;

use anyhow::{Context, Result, ensure};
use serde::Serialize;

use crate::{rom::Rom, sha1_hex, text_inventory::FixedTextPlan};

use super::item_domain::battle_item_source_index;

mod source_records;

use source_records::{
    CHAPTER_COUNT, ENEMY_RECORD_BYTE_COUNT, EnemyRecord, PointerTableBinding, SourceRoutineBinding,
    bind_enemy_source_domain,
};

pub(super) struct EnemyBattleDomain {
    pub(super) participant_glyph_sets: Vec<BTreeSet<char>>,
    pub(super) binding: EnemyBattleDomainBinding,
}

#[derive(Debug, Serialize)]
pub(super) struct EnemyBattleDomainBinding {
    chapter_count: usize,
    compact_record_byte_count: usize,
    identity_offset: usize,
    class_offset: usize,
    first_item_offset: usize,
    second_item_offset: usize,
    enemy_identity_to_name_source_index: &'static str,
    class_id_to_source_index: &'static str,
    item_id_to_source_index: &'static str,
    initial_records: PointerTableBinding,
    reinforcement_records: PointerTableBinding,
    initial_loader: SourceRoutineBinding,
    initial_record_builder: SourceRoutineBinding,
    reinforcement_selector: SourceRoutineBinding,
    reinforcement_record_builder: SourceRoutineBinding,
    combined_record_count: usize,
    unique_enemy_name_entry_count: usize,
    unique_enemy_class_entry_count: usize,
    unique_enemy_item_entry_count: usize,
    participant_candidate_count: usize,
    participant_candidate_sha1: String,
    initial_and_reinforcement_sources_bound: bool,
    renderer_class_id_count: usize,
    source_record_class_restrictions_used: bool,
    enemy_class_write_sites_enumerated: bool,
    renderer_complete_class_upper_bound: bool,
    participant_candidates_are_necessary_condition_superset: bool,
    item_candidates_filtered_by_runtime_eligibility: bool,
    item_slot_mutation_bound: bool,
    enemy_class_mutation_bound: bool,
    actual_enemy_battle_reachability_proven: bool,
}

#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
struct ParticipantCandidate {
    identity: u8,
    class_id: u8,
    item_id: u8,
}

pub(super) fn bind_enemy_battle_domain(
    rom: &Rom,
    fixed: &FixedTextPlan,
    eligible_enemy_class_item_pairs: &BTreeSet<(u8, u8)>,
) -> Result<EnemyBattleDomain> {
    let source = bind_enemy_source_domain(rom)?;
    let records = source.records;
    let renderer_class_ids = renderer_class_ids(fixed)?;
    let candidates = participant_candidates(
        &records,
        eligible_enemy_class_item_pairs,
        &renderer_class_ids,
    )?;

    let mut name_indices = BTreeSet::new();
    let mut class_indices = BTreeSet::new();
    let mut item_indices = BTreeSet::new();
    for candidate in &candidates {
        name_indices.insert(enemy_name_source_index(candidate.identity)?);
        class_indices.insert(one_based_source_index(candidate.class_id, "enemy class")?);
        if candidate.item_id != 0 {
            item_indices.insert(battle_item_source_index(candidate.item_id)?);
        }
    }

    let participant_glyph_sets = candidates
        .iter()
        .map(|candidate| participant_glyphs(fixed, *candidate))
        .collect::<Result<Vec<_>>>()?;
    let candidate_bytes = candidates
        .iter()
        .flat_map(|candidate| [candidate.identity, candidate.class_id, candidate.item_id])
        .collect::<Vec<_>>();

    Ok(EnemyBattleDomain {
        participant_glyph_sets,
        binding: EnemyBattleDomainBinding {
            chapter_count: CHAPTER_COUNT,
            compact_record_byte_count: ENEMY_RECORD_BYTE_COUNT,
            identity_offset: 0,
            class_offset: 1,
            first_item_offset: 3,
            second_item_offset: 4,
            enemy_identity_to_name_source_index: "(identity AND 0x7F) - 1",
            class_id_to_source_index: "class_id - 1",
            item_id_to_source_index: "item_id < 0x40 maps to item_id - 1; item_id >= 0x40 maps to 0x44",
            initial_records: source.initial_records,
            reinforcement_records: source.reinforcement_records,
            initial_loader: source.initial_loader,
            initial_record_builder: source.initial_record_builder,
            reinforcement_selector: source.reinforcement_selector,
            reinforcement_record_builder: source.reinforcement_record_builder,
            combined_record_count: records.len(),
            unique_enemy_name_entry_count: name_indices.len(),
            unique_enemy_class_entry_count: class_indices.len(),
            unique_enemy_item_entry_count: item_indices.len(),
            participant_candidate_count: candidates.len(),
            participant_candidate_sha1: sha1_hex(&candidate_bytes),
            initial_and_reinforcement_sources_bound: true,
            renderer_class_id_count: renderer_class_ids.len(),
            source_record_class_restrictions_used: false,
            enemy_class_write_sites_enumerated: false,
            renderer_complete_class_upper_bound: true,
            participant_candidates_are_necessary_condition_superset: true,
            item_candidates_filtered_by_runtime_eligibility: true,
            item_slot_mutation_bound: true,
            enemy_class_mutation_bound: true,
            actual_enemy_battle_reachability_proven: false,
        },
    })
}

fn participant_candidates(
    records: &[EnemyRecord],
    eligible_enemy_class_item_pairs: &BTreeSet<(u8, u8)>,
    renderer_class_ids: &BTreeSet<u8>,
) -> Result<BTreeSet<ParticipantCandidate>> {
    let mut candidates = BTreeSet::new();
    for record in records {
        let identity = record.bytes[0];
        enemy_name_source_index(identity)?;
        for class_id in renderer_class_ids {
            let items = [record.bytes[3], record.bytes[4]]
                .into_iter()
                .filter(|item| {
                    *item != 0 && eligible_enemy_class_item_pairs.contains(&(*class_id, *item))
                })
                .collect::<BTreeSet<_>>();
            if items.is_empty() {
                candidates.insert(ParticipantCandidate {
                    identity,
                    class_id: *class_id,
                    item_id: 0,
                });
            } else {
                for item_id in &items {
                    battle_item_source_index(*item_id)?;
                    candidates.insert(ParticipantCandidate {
                        identity,
                        class_id: *class_id,
                        item_id: *item_id,
                    });
                }
            }
        }
    }
    Ok(candidates)
}

fn renderer_class_ids(fixed: &FixedTextPlan) -> Result<BTreeSet<u8>> {
    let source_indices = fixed
        .entries
        .iter()
        .filter(|entry| entry.table_id == "class-names")
        .flat_map(|entry| {
            std::iter::once(entry.source_index).chain(entry.alias_indices.iter().copied())
        })
        .collect::<BTreeSet<_>>();
    ensure!(
        source_indices == (0..24).collect(),
        "class-name renderer source indices are no longer contiguous from 0 through 23"
    );
    source_indices
        .into_iter()
        .map(|source_index| {
            u8::try_from(source_index + 1).context("class identity exceeds report encoding")
        })
        .collect()
}

fn participant_glyphs(
    fixed: &FixedTextPlan,
    candidate: ParticipantCandidate,
) -> Result<BTreeSet<char>> {
    let mut glyphs = fixed
        .entry_for_source_index("enemy-names", enemy_name_source_index(candidate.identity)?)
        .context("missing fixed enemy-name translation")?
        .unique_glyphs();
    glyphs.extend(
        fixed
            .entry_for_source_index(
                "class-names",
                one_based_source_index(candidate.class_id, "enemy class")?,
            )
            .context("missing fixed enemy-class translation")?
            .unique_glyphs(),
    );
    if candidate.item_id != 0 {
        let item_source_index = battle_item_source_index(candidate.item_id)?;
        glyphs.extend(
            fixed
                .entry_for_source_index("item-names", item_source_index)
                .context("missing fixed enemy-item translation")?
                .unique_glyphs(),
        );
    }
    Ok(glyphs)
}

fn enemy_name_source_index(identity: u8) -> Result<usize> {
    ensure!(
        identity & 0x80 != 0,
        "enemy identity {identity:02X} lacks bit 7"
    );
    one_based_source_index(identity & 0x7F, "enemy identity")
}

fn one_based_source_index(identity: u8, role: &str) -> Result<usize> {
    usize::from(identity)
        .checked_sub(1)
        .with_context(|| format!("{role} is zero"))
}

#[cfg(test)]
pub(super) fn test_binding() -> EnemyBattleDomainBinding {
    EnemyBattleDomainBinding {
        chapter_count: CHAPTER_COUNT,
        compact_record_byte_count: ENEMY_RECORD_BYTE_COUNT,
        identity_offset: 0,
        class_offset: 1,
        first_item_offset: 3,
        second_item_offset: 4,
        enemy_identity_to_name_source_index: "identity",
        class_id_to_source_index: "class",
        item_id_to_source_index: "item",
        initial_records: source_records::test_table("initial"),
        reinforcement_records: source_records::test_table("reinforcement"),
        initial_loader: source_records::test_routine("initial loader"),
        initial_record_builder: source_records::test_routine("initial builder"),
        reinforcement_selector: source_records::test_routine("reinforcement selector"),
        reinforcement_record_builder: source_records::test_routine("reinforcement builder"),
        combined_record_count: 2,
        unique_enemy_name_entry_count: 1,
        unique_enemy_class_entry_count: 1,
        unique_enemy_item_entry_count: 1,
        participant_candidate_count: 1,
        participant_candidate_sha1: "candidates".to_owned(),
        initial_and_reinforcement_sources_bound: true,
        renderer_class_id_count: 24,
        source_record_class_restrictions_used: false,
        enemy_class_write_sites_enumerated: false,
        renderer_complete_class_upper_bound: true,
        participant_candidates_are_necessary_condition_superset: true,
        item_candidates_filtered_by_runtime_eligibility: true,
        item_slot_mutation_bound: true,
        enemy_class_mutation_bound: true,
        actual_enemy_battle_reachability_proven: false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn participant_candidates_cover_all_renderer_classes_and_eligible_source_items() {
        let records = [EnemyRecord {
            bytes: [0xC5, 10, 1, 1, 6, 13, 11, 60, 0, 13, 11],
        }];

        let eligible_pairs = [(10, 1), (10, 6)].into_iter().collect();
        let class_ids = (1..=24).collect();
        let candidates = participant_candidates(&records, &eligible_pairs, &class_ids).unwrap();

        assert!(candidates.contains(&ParticipantCandidate {
            identity: 0xC5,
            class_id: 10,
            item_id: 1,
        }));
        assert!(candidates.contains(&ParticipantCandidate {
            identity: 0xC5,
            class_id: 10,
            item_id: 6,
        }));
        assert!(candidates.contains(&ParticipantCandidate {
            identity: 0xC5,
            class_id: 1,
            item_id: 0,
        }));
        assert!(candidates.contains(&ParticipantCandidate {
            identity: 0xC5,
            class_id: 24,
            item_id: 0,
        }));
        assert_eq!(candidates.len(), 25);
        assert_eq!(enemy_name_source_index(0xC5).unwrap(), 68);
    }
}
