use anyhow::{Result, ensure};
use serde::Serialize;

use crate::{dialogue_assets::EncodedMainDialogueDisplayStorage, rom::Rom, tracked::TrackedImage};

use super::relocated_dialogue_banks::append_relocated_dialogue_writes;

#[derive(Serialize)]
pub(super) struct IntegratedWriteSetPlan {
    required_domain_count: usize,
    domains: Vec<DomainWriteContribution>,
    contributing_domain_count: usize,
    fully_planned_domain_count: usize,
    expected_write_count: usize,
    changed_byte_count: usize,
    every_change_tracked: bool,
    one_shared_image: bool,
    all_domains_contribute_expected_writes: bool,
    output_materialized_in_memory_only: bool,
    rom_emitted: bool,
}

#[derive(Serialize)]
struct DomainWriteContribution {
    id: &'static str,
    translation_input_loaded: bool,
    glyph_lifetime_bound: bool,
    storage_and_address_writes_contributed: bool,
    font_supply_writes_contributed: bool,
    all_consumer_writes_contributed: bool,
    expected_write_count: usize,
    complete_in_integrated_plan: bool,
}

pub(super) fn plan_integrated_write_set(
    candidate: &Rom,
    dialogue_storage: &EncodedMainDialogueDisplayStorage,
    required_domains: &[&'static str],
    expected_dialogue_write_count: usize,
) -> Result<IntegratedWriteSetPlan> {
    let mut image = TrackedImage::new(candidate.data().to_vec());
    append_relocated_dialogue_writes(&mut image, candidate, dialogue_storage)?;
    ensure!(
        image.writes().len() == expected_dialogue_write_count,
        "integrated write set and complete dialogue write set disagree"
    );
    image.verify_all_changes_tracked(candidate.data())?;
    let expected_write_count = image.writes().len();
    let output = image.into_data();
    let changed_byte_count = candidate
        .data()
        .iter()
        .zip(&output)
        .filter(|(before, after)| before != after)
        .count();

    let domains = domain_contributions(required_domains, expected_dialogue_write_count)?;
    let contributing_domain_count = domains
        .iter()
        .filter(|domain| domain.expected_write_count != 0)
        .count();
    let fully_planned_domain_count = domains
        .iter()
        .filter(|domain| domain.complete_in_integrated_plan)
        .count();
    ensure!(
        contributing_domain_count == 1 && fully_planned_domain_count == 0,
        "integrated write gate advanced without every domain layer"
    );

    Ok(IntegratedWriteSetPlan {
        required_domain_count: required_domains.len(),
        domains,
        contributing_domain_count,
        fully_planned_domain_count,
        expected_write_count,
        changed_byte_count,
        every_change_tracked: true,
        one_shared_image: true,
        all_domains_contribute_expected_writes: false,
        output_materialized_in_memory_only: true,
        rom_emitted: false,
    })
}

fn domain_contributions(
    required_domains: &[&'static str],
    expected_dialogue_write_count: usize,
) -> Result<Vec<DomainWriteContribution>> {
    ensure!(
        required_domains.len() == 13
            && required_domains.contains(&"main_dialogue")
            && required_domains
                .iter()
                .copied()
                .collect::<std::collections::BTreeSet<_>>()
                .len()
                == required_domains.len(),
        "integrated write set requires thirteen unique domains including main dialogue"
    );
    Ok(required_domains
        .iter()
        .map(|id| {
            let dialogue = *id == "main_dialogue";
            DomainWriteContribution {
                id,
                translation_input_loaded: true,
                glyph_lifetime_bound: true,
                storage_and_address_writes_contributed: dialogue,
                font_supply_writes_contributed: false,
                all_consumer_writes_contributed: false,
                expected_write_count: if dialogue {
                    expected_dialogue_write_count
                } else {
                    0
                },
                complete_in_integrated_plan: false,
            }
        })
        .collect())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn storage_only_dialogue_contribution_does_not_count_as_a_complete_domain() {
        let domains = domain_contributions(
            &[
                "chapter_save_offer_label",
                "chapter_titles",
                "choice_labels",
                "class_names",
                "ending_record_labels",
                "enemy_names",
                "item_action_labels",
                "item_names",
                "location_names",
                "main_dialogue",
                "map_menu_labels",
                "unit_names",
                "unit_ui_labels",
            ],
            538,
        )
        .unwrap();

        assert_eq!(
            domains
                .iter()
                .filter(|domain| domain.expected_write_count != 0)
                .count(),
            1
        );
        assert!(
            domains
                .iter()
                .all(|domain| !domain.complete_in_integrated_plan)
        );
    }
}
