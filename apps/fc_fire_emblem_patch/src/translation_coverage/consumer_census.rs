use std::collections::{BTreeMap, BTreeSet};

use anyhow::{Result, ensure};
use serde::Serialize;

use crate::{
    fixed_menu_labels::inspect_fixed_menu_translation_consumers,
    item_flow::inspect_item_action_translation_consumers,
    map_menu::inspect_map_menu_translation_consumers,
    mapper165::battle_codebook_plan::inspect_known_terrain_name_translation_routes,
    rom::Rom,
    screen_contracts::ScreenTranslationPartition,
    translation_consumer::{ScreenConsumerSourceBinding, TranslationConsumerSourceEvidence},
    unit_ui_text::inspect_unit_ui_translation_consumers,
};

use super::screen_targets::{DOMAIN_SEEDS, DomainScreenTargets};

const KNOWN_ROUTE_DOMAIN_IDS: [&str; 5] = [
    "fixed_menu_labels",
    "item_action_labels",
    "map_menu_labels",
    "terrain_names",
    "unit_ui_labels",
];
const COMPLETE_CENSUS_DOMAIN_IDS: [&str; 0] = [];

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum ConsumerEvidenceState {
    /// 아래에 열거한 경로가 원천에 결속됐다는 뜻일 뿐, 참조 분모가 완전하다는 뜻은 아니다.
    KnownRoutesBound,
    /// 원천 population, 생산자, 참조 후보, 소비자와 화면 소유권의 분모가 모두 닫혔다.
    CompleteCensusBound,
    Unresolved,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct DomainConsumerEvidence {
    pub(crate) id: &'static str,
    pub(crate) state: ConsumerEvidenceState,
    pub(crate) population_ids: Vec<String>,
    pub(crate) source_bindings: Vec<ScreenConsumerSourceBinding>,
    pub(crate) census_boundary_source_binding_ids: Vec<String>,
}

#[derive(Debug, Clone)]
struct DomainSourceEvidence {
    domain_id: &'static str,
    intended_state: ConsumerEvidenceState,
    source: TranslationConsumerSourceEvidence,
    census_boundary_source_binding_ids: Vec<String>,
}

impl DomainSourceEvidence {
    fn known_routes(domain_id: &'static str, source: TranslationConsumerSourceEvidence) -> Self {
        Self {
            domain_id,
            intended_state: ConsumerEvidenceState::KnownRoutesBound,
            source,
            census_boundary_source_binding_ids: Vec::new(),
        }
    }
}

pub(crate) fn inspect_translation_consumer_evidence(
    rom: &Rom,
    partition: &ScreenTranslationPartition,
    targets: &[DomainScreenTargets],
) -> Result<Vec<DomainConsumerEvidence>> {
    rom.verify_supported_japanese()?;
    bind_translation_consumer_evidence(
        partition,
        targets,
        vec![
            DomainSourceEvidence::known_routes(
                "unit_ui_labels",
                inspect_unit_ui_translation_consumers(rom.data())?,
            ),
            DomainSourceEvidence::known_routes(
                "fixed_menu_labels",
                inspect_fixed_menu_translation_consumers(rom)?,
            ),
            DomainSourceEvidence::known_routes(
                "item_action_labels",
                inspect_item_action_translation_consumers(rom)?,
            ),
            DomainSourceEvidence::known_routes(
                "map_menu_labels",
                inspect_map_menu_translation_consumers(rom)?,
            ),
            DomainSourceEvidence::known_routes(
                "terrain_names",
                inspect_known_terrain_name_translation_routes(rom)?,
            ),
        ],
    )
}

fn bind_translation_consumer_evidence(
    partition: &ScreenTranslationPartition,
    targets: &[DomainScreenTargets],
    evidence: Vec<DomainSourceEvidence>,
) -> Result<Vec<DomainConsumerEvidence>> {
    let expected_domain_ids = DOMAIN_SEEDS
        .iter()
        .map(|domain| domain.id)
        .collect::<BTreeSet<_>>();
    let target_domain_ids = targets
        .iter()
        .map(|target| target.id)
        .collect::<BTreeSet<_>>();
    ensure!(
        target_domain_ids.len() == targets.len(),
        "consumer census target metadata repeats a domain"
    );
    ensure!(
        target_domain_ids == expected_domain_ids,
        "consumer census target metadata does not cover the domain registry"
    );

    let known_route_domain_ids = KNOWN_ROUTE_DOMAIN_IDS.into_iter().collect::<BTreeSet<_>>();
    let complete_census_domain_ids = COMPLETE_CENSUS_DOMAIN_IDS
        .into_iter()
        .collect::<BTreeSet<_>>();
    ensure!(
        known_route_domain_ids.is_disjoint(&complete_census_domain_ids),
        "consumer evidence registries overlap"
    );
    let implemented_domain_ids = known_route_domain_ids
        .union(&complete_census_domain_ids)
        .copied()
        .collect::<BTreeSet<_>>();
    let mut evidence_by_domain = BTreeMap::new();
    for domain in evidence {
        ensure!(
            expected_domain_ids.contains(domain.domain_id),
            "consumer census evidence uses unknown domain {}",
            domain.domain_id
        );
        let expected_state = if known_route_domain_ids.contains(domain.domain_id) {
            ConsumerEvidenceState::KnownRoutesBound
        } else if complete_census_domain_ids.contains(domain.domain_id) {
            ConsumerEvidenceState::CompleteCensusBound
        } else {
            anyhow::bail!(
                "consumer census evidence for {} has no owning source implementation",
                domain.domain_id
            );
        };
        ensure!(
            domain.intended_state == expected_state,
            "consumer census evidence state changed for {}",
            domain.domain_id
        );
        let domain_id = domain.domain_id;
        ensure!(
            evidence_by_domain.insert(domain_id, domain).is_none(),
            "consumer census repeats source evidence for {}",
            domain_id
        );
    }
    ensure!(
        evidence_by_domain.keys().copied().collect::<BTreeSet<_>>() == implemented_domain_ids,
        "consumer census is missing an implemented source domain"
    );

    let japanese_screen_roles = partition
        .japanese_bearing_screens
        .iter()
        .map(|screen| screen.role.as_str())
        .collect::<BTreeSet<_>>();
    let mut census = Vec::with_capacity(targets.len());
    for target in targets {
        let Some(mut evidence) = evidence_by_domain.remove(target.id) else {
            census.push(DomainConsumerEvidence {
                id: target.id,
                state: ConsumerEvidenceState::Unresolved,
                population_ids: Vec::new(),
                source_bindings: Vec::new(),
                census_boundary_source_binding_ids: Vec::new(),
            });
            continue;
        };
        let mut source = evidence.source;

        match evidence.intended_state {
            ConsumerEvidenceState::KnownRoutesBound => ensure!(
                evidence.census_boundary_source_binding_ids.is_empty(),
                "known-route evidence for {} claims a complete census boundary",
                target.id
            ),
            ConsumerEvidenceState::CompleteCensusBound => {
                ensure!(
                    !evidence.census_boundary_source_binding_ids.is_empty(),
                    "complete consumer census domain {} has no source boundary",
                    target.id
                );
                let mut boundary_ids = BTreeSet::new();
                for binding_id in &evidence.census_boundary_source_binding_ids {
                    ensure!(
                        !binding_id.trim().is_empty(),
                        "complete consumer census domain {} contains an empty source boundary",
                        target.id
                    );
                    ensure!(
                        boundary_ids.insert(binding_id.as_str()),
                        "complete consumer census domain {} repeats source boundary {}",
                        target.id,
                        binding_id
                    );
                }
            }
            ConsumerEvidenceState::Unresolved => anyhow::bail!(
                "consumer census source evidence cannot declare {} unresolved",
                target.id
            ),
        }

        ensure!(
            !source.population_ids.is_empty(),
            "consumer census domain {} has an empty source population",
            target.id
        );
        let mut population_ids = BTreeSet::new();
        for population_id in &source.population_ids {
            ensure!(
                !population_id.trim().is_empty(),
                "consumer census domain {} contains an empty population id",
                target.id
            );
            ensure!(
                population_ids.insert(population_id.as_str()),
                "consumer census domain {} repeats population {}",
                target.id,
                population_id
            );
        }
        ensure!(
            !source.screen_bindings.is_empty(),
            "consumer census domain {} has no screen binding",
            target.id
        );
        let mut actual_screen_roles = BTreeSet::new();
        let mut assigned_population_ids = BTreeSet::new();
        for binding in &source.screen_bindings {
            ensure!(
                !binding.screen_role.trim().is_empty(),
                "consumer census domain {} contains an empty screen role",
                target.id
            );
            ensure!(
                japanese_screen_roles.contains(binding.screen_role),
                "consumer census domain {} binds unknown or non-Japanese screen {}",
                target.id,
                binding.screen_role
            );
            ensure!(
                actual_screen_roles.insert(binding.screen_role),
                "consumer census domain {} repeats screen binding {}",
                target.id,
                binding.screen_role
            );
            ensure!(
                !binding.population_ids.is_empty(),
                "consumer census domain {} screen {} has no edge population",
                target.id,
                binding.screen_role
            );
            let mut edge_population_ids = BTreeSet::new();
            for population_id in &binding.population_ids {
                ensure!(
                    !population_id.trim().is_empty(),
                    "consumer census domain {} screen {} contains an empty edge population id",
                    target.id,
                    binding.screen_role
                );
                ensure!(
                    edge_population_ids.insert(population_id.as_str()),
                    "consumer census domain {} screen {} repeats edge population {}",
                    target.id,
                    binding.screen_role,
                    population_id
                );
                ensure!(
                    population_ids.contains(population_id.as_str()),
                    "consumer census domain {} screen {} assigns unknown population {}",
                    target.id,
                    binding.screen_role,
                    population_id
                );
                assigned_population_ids.insert(population_id.as_str());
            }
            ensure!(
                !binding.source_binding_ids.is_empty(),
                "consumer census domain {} screen {} has no source binding",
                target.id,
                binding.screen_role
            );
            let mut source_binding_ids = BTreeSet::new();
            for binding_id in &binding.source_binding_ids {
                ensure!(
                    !binding_id.trim().is_empty(),
                    "consumer census domain {} screen {} contains an empty source binding",
                    target.id,
                    binding.screen_role
                );
                ensure!(
                    source_binding_ids.insert(binding_id.as_str()),
                    "consumer census domain {} screen {} repeats source binding {}",
                    target.id,
                    binding.screen_role,
                    binding_id
                );
            }
        }
        ensure!(
            assigned_population_ids == population_ids,
            "consumer census domain {} leaves source population unassigned to known routes",
            target.id
        );
        let expected_screen_roles = target
            .screen_roles
            .iter()
            .map(String::as_str)
            .collect::<BTreeSet<_>>();
        ensure!(
            actual_screen_roles == expected_screen_roles,
            "consumer census domain {} source screens do not exactly match expected metadata",
            target.id
        );
        source
            .screen_bindings
            .sort_by_key(|binding| binding.screen_role);
        census.push(DomainConsumerEvidence {
            id: target.id,
            state: evidence.intended_state,
            population_ids: source.population_ids,
            source_bindings: source.screen_bindings,
            census_boundary_source_binding_ids: std::mem::take(
                &mut evidence.census_boundary_source_binding_ids,
            ),
        });
    }
    ensure!(
        evidence_by_domain.is_empty(),
        "consumer census left unmatched source evidence"
    );
    Ok(census)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        screen_contracts::inspect_screen_translation_partition,
        translation_coverage::screen_targets::bind_domain_screen_targets,
    };

    fn metadata() -> (ScreenTranslationPartition, Vec<DomainScreenTargets>) {
        let partition = inspect_screen_translation_partition().unwrap();
        let targets = bind_domain_screen_targets(&partition).unwrap();
        (partition, targets)
    }

    fn source(
        population_ids: &[&str],
        screen_roles: &[&'static str],
    ) -> TranslationConsumerSourceEvidence {
        TranslationConsumerSourceEvidence {
            population_ids: population_ids.iter().map(|id| (*id).to_owned()).collect(),
            screen_bindings: screen_roles
                .iter()
                .map(|role| ScreenConsumerSourceBinding {
                    screen_role: role,
                    population_ids: population_ids.iter().map(|id| (*id).to_owned()).collect(),
                    source_binding_ids: vec![format!("source:{role}")],
                })
                .collect(),
        }
    }

    fn evidence() -> Vec<DomainSourceEvidence> {
        vec![
            DomainSourceEvidence::known_routes(
                "unit_ui_labels",
                source(
                    &["unit-ui-label:00"],
                    &["unit_summary", "unit_command_menu", "unit_status"],
                ),
            ),
            DomainSourceEvidence::known_routes(
                "item_action_labels",
                source(&["item-action-label:13"], &["item_action_menu"]),
            ),
            DomainSourceEvidence::known_routes(
                "map_menu_labels",
                source(
                    &["map-menu:roster", "map-funds-summary:map"],
                    &["map_menu", "map_funds_summary"],
                ),
            ),
            DomainSourceEvidence::known_routes(
                "terrain_names",
                source(&["terrain-names:000"], &["battle_animation"]),
            ),
            DomainSourceEvidence::known_routes(
                "fixed_menu_labels",
                source(
                    &["fixed-menu-label:2C"],
                    &[
                        "unit_selection",
                        "game_speed_selection",
                        "storage_action_menu",
                        "storage_overflow_action",
                        "storage_capacity_notice",
                    ],
                ),
            ),
        ]
    }

    #[test]
    fn keeps_all_censuses_incomplete_while_binding_five_known_route_sets() {
        let (partition, targets) = metadata();
        let census = bind_translation_consumer_evidence(&partition, &targets, evidence()).unwrap();

        assert_eq!(census.len(), 23);
        assert_eq!(DOMAIN_SEEDS.len(), 23);
        assert_eq!(
            census
                .iter()
                .filter(|domain| domain.state == ConsumerEvidenceState::CompleteCensusBound)
                .count(),
            0
        );
        assert_eq!(
            census
                .iter()
                .filter(|domain| domain.state == ConsumerEvidenceState::KnownRoutesBound)
                .map(|domain| domain.id)
                .collect::<BTreeSet<_>>(),
            KNOWN_ROUTE_DOMAIN_IDS.into_iter().collect()
        );
        let terrain = census
            .iter()
            .find(|domain| domain.id == "terrain_names")
            .unwrap();
        assert_eq!(terrain.state, ConsumerEvidenceState::KnownRoutesBound);
        assert!(terrain.census_boundary_source_binding_ids.is_empty());
        assert_eq!(
            census
                .iter()
                .filter(|domain| domain.state == ConsumerEvidenceState::Unresolved)
                .count(),
            18
        );
        assert_eq!(
            census
                .iter()
                .filter(|domain| domain.state != ConsumerEvidenceState::CompleteCensusBound)
                .count(),
            23
        );
    }

    #[test]
    fn rejects_empty_duplicate_unknown_and_unassigned_edge_populations() {
        let (partition, targets) = metadata();

        let mut empty = evidence();
        empty[0].source.screen_bindings[0].population_ids.clear();
        assert!(
            bind_translation_consumer_evidence(&partition, &targets, empty)
                .unwrap_err()
                .to_string()
                .contains("has no edge population")
        );

        let mut duplicate = evidence();
        duplicate[0].source.screen_bindings[0]
            .population_ids
            .push("unit-ui-label:00".to_owned());
        assert!(
            bind_translation_consumer_evidence(&partition, &targets, duplicate)
                .unwrap_err()
                .to_string()
                .contains("repeats edge population")
        );

        let mut unknown = evidence();
        unknown[0].source.screen_bindings[0].population_ids = vec!["unit-ui-label:FF".to_owned()];
        assert!(
            bind_translation_consumer_evidence(&partition, &targets, unknown)
                .unwrap_err()
                .to_string()
                .contains("assigns unknown population")
        );

        let mut unassigned = evidence();
        unassigned[0]
            .source
            .population_ids
            .push("unit-ui-label:01".to_owned());
        assert!(
            bind_translation_consumer_evidence(&partition, &targets, unassigned)
                .unwrap_err()
                .to_string()
                .contains("leaves source population unassigned")
        );
    }

    #[test]
    fn rejects_missing_extra_and_unknown_domain_evidence() {
        let (partition, targets) = metadata();

        let mut missing = evidence();
        missing.pop();
        assert!(
            bind_translation_consumer_evidence(&partition, &targets, missing)
                .unwrap_err()
                .to_string()
                .contains("missing an implemented source domain")
        );

        let mut extra = evidence();
        extra.push(DomainSourceEvidence::known_routes(
            "main_dialogue",
            source(&["dialogue:00"], &["intro_dialogue"]),
        ));
        assert!(
            bind_translation_consumer_evidence(&partition, &targets, extra)
                .unwrap_err()
                .to_string()
                .contains("has no owning source implementation")
        );

        let mut unknown = evidence();
        unknown.push(DomainSourceEvidence::known_routes(
            "unknown_domain",
            source(&["unknown:00"], &["map_menu"]),
        ));
        assert!(
            bind_translation_consumer_evidence(&partition, &targets, unknown)
                .unwrap_err()
                .to_string()
                .contains("uses unknown domain")
        );
    }

    #[test]
    fn rejects_missing_extra_and_unknown_screen_bindings() {
        let (partition, targets) = metadata();

        let mut missing = evidence();
        missing[0].source.screen_bindings.pop();
        assert!(
            bind_translation_consumer_evidence(&partition, &targets, missing)
                .unwrap_err()
                .to_string()
                .contains("do not exactly match")
        );

        let mut extra = evidence();
        extra[0]
            .source
            .screen_bindings
            .push(ScreenConsumerSourceBinding {
                screen_role: "map_menu",
                population_ids: vec!["unit-ui-label:00".to_owned()],
                source_binding_ids: vec!["source:map_menu".to_owned()],
            });
        assert!(
            bind_translation_consumer_evidence(&partition, &targets, extra)
                .unwrap_err()
                .to_string()
                .contains("do not exactly match")
        );

        let mut unknown = evidence();
        unknown[0]
            .source
            .screen_bindings
            .push(ScreenConsumerSourceBinding {
                screen_role: "unknown_screen",
                population_ids: vec!["unit-ui-label:00".to_owned()],
                source_binding_ids: vec!["source:unknown".to_owned()],
            });
        assert!(
            bind_translation_consumer_evidence(&partition, &targets, unknown)
                .unwrap_err()
                .to_string()
                .contains("unknown or non-Japanese screen")
        );
    }

    #[test]
    fn rejects_empty_and_duplicate_population_ids() {
        let (partition, targets) = metadata();

        for population_ids in [Vec::new(), vec!["".to_owned()]] {
            let mut candidate = evidence();
            candidate[0].source.population_ids = population_ids;
            assert!(bind_translation_consumer_evidence(&partition, &targets, candidate).is_err());
        }

        let mut duplicate = evidence();
        duplicate[0]
            .source
            .population_ids
            .push("unit-ui-label:00".to_owned());
        assert!(
            bind_translation_consumer_evidence(&partition, &targets, duplicate)
                .unwrap_err()
                .to_string()
                .contains("repeats population")
        );
    }

    #[test]
    fn rejects_empty_and_duplicate_source_bindings() {
        let (partition, targets) = metadata();

        let mut no_screen_binding = evidence();
        no_screen_binding[0].source.screen_bindings.clear();
        assert!(
            bind_translation_consumer_evidence(&partition, &targets, no_screen_binding)
                .unwrap_err()
                .to_string()
                .contains("has no screen binding")
        );

        let mut empty_screen_role = evidence();
        empty_screen_role[0].source.screen_bindings[0].screen_role = "";
        assert!(
            bind_translation_consumer_evidence(&partition, &targets, empty_screen_role)
                .unwrap_err()
                .to_string()
                .contains("empty screen role")
        );

        for source_binding_ids in [Vec::new(), vec!["".to_owned()]] {
            let mut candidate = evidence();
            candidate[0].source.screen_bindings[0].source_binding_ids = source_binding_ids;
            assert!(bind_translation_consumer_evidence(&partition, &targets, candidate).is_err());
        }

        let mut duplicate = evidence();
        let repeated = duplicate[0].source.screen_bindings[0].source_binding_ids[0].clone();
        duplicate[0].source.screen_bindings[0]
            .source_binding_ids
            .push(repeated);
        assert!(
            bind_translation_consumer_evidence(&partition, &targets, duplicate)
                .unwrap_err()
                .to_string()
                .contains("repeats source binding")
        );

        let mut duplicate_screen = evidence();
        let repeated = duplicate_screen[0].source.screen_bindings[0].clone();
        duplicate_screen[0].source.screen_bindings.push(repeated);
        assert!(
            bind_translation_consumer_evidence(&partition, &targets, duplicate_screen)
                .unwrap_err()
                .to_string()
                .contains("repeats screen binding")
        );

        let mut duplicate_domain = evidence();
        duplicate_domain.push(duplicate_domain[0].clone());
        assert!(
            bind_translation_consumer_evidence(&partition, &targets, duplicate_domain)
                .unwrap_err()
                .to_string()
                .contains("repeats source evidence")
        );
    }

    #[test]
    fn rejects_census_boundary_or_complete_state_for_known_routes() {
        let (partition, targets) = metadata();

        let mut false_boundary = evidence();
        false_boundary[3]
            .census_boundary_source_binding_ids
            .push("source:terrain-reference-census".to_owned());
        assert!(
            bind_translation_consumer_evidence(&partition, &targets, false_boundary)
                .unwrap_err()
                .to_string()
                .contains("claims a complete census boundary")
        );

        let mut misclassified = evidence();
        misclassified[3].intended_state = ConsumerEvidenceState::CompleteCensusBound;
        assert!(
            bind_translation_consumer_evidence(&partition, &targets, misclassified)
                .unwrap_err()
                .to_string()
                .contains("evidence state changed")
        );
    }
}
