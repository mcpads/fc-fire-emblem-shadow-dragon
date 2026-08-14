use std::{
    collections::{BTreeMap, BTreeSet},
    path::Path,
};

use anyhow::{Context, Result, ensure};
use serde::Serialize;

use crate::translation_coverage::{
    DomainInstallation, inspect_current_installation, inspect_domain_screen_targets,
};

const ENDING_CHARACTER_EPILOGUE: &str = "ending_character_epilogue";

pub(super) struct ConsumerInstallationInputs<'a> {
    pub(super) current_candidate_path: &'a Path,
    pub(super) current_build_report_path: &'a Path,
    pub(super) required_domains: &'a [&'static str],
    pub(super) target_unit_counts: &'a BTreeMap<&'static str, usize>,
    pub(super) all_chapter_titles_encoded: bool,
    pub(super) all_dialogue_records_encoded: bool,
    pub(super) all_dialogue_runtime_hooks_emitted: bool,
    pub(super) dynamic_dialogue_producers_bound: bool,
}

#[derive(Serialize)]
pub(super) struct ConsumerInstallationPlan {
    strategy: &'static str,
    current_candidate_sha1: String,
    current_build_report_sha1: String,
    required_domain_count: usize,
    domains: Vec<DomainConsumerInstallation>,
    carried_consumer_domain_count: usize,
    globally_advanced_domain_count: usize,
    all_consumers_statically_accounted_domain_count: usize,
    unresolved_consumer_domain_count: usize,
    current_candidate_historical_runtime_role_count: usize,
    final_artifact_runtime_bound_role_count: usize,
    all_required_consumers_statically_accounted: bool,
    current_candidate_runtime_evidence_inherited: bool,
    final_artifact_runtime_replay_required: bool,
}

#[derive(Serialize)]
struct DomainConsumerInstallation {
    id: &'static str,
    target_unit_count: usize,
    current_candidate_installed_target_unit_count: usize,
    globally_planned_target_unit_count: usize,
    target_screen_roles: Vec<String>,
    current_candidate_carried_screen_roles: Vec<String>,
    globally_planned_screen_roles: Vec<String>,
    newly_planned_screen_roles: Vec<String>,
    statically_accounted_screen_roles: Vec<String>,
    remaining_screen_roles: Vec<String>,
    current_candidate_historical_runtime_roles: Vec<String>,
    final_artifact_runtime_bound_screen_roles: Vec<String>,
    all_consumers_statically_accounted: bool,
}

impl ConsumerInstallationPlan {
    pub(super) fn all_required_consumers_statically_accounted(&self) -> bool {
        self.all_required_consumers_statically_accounted
    }

    pub(super) fn fully_planned_domain_count(&self) -> usize {
        self.all_consumers_statically_accounted_domain_count
    }

    pub(super) fn domain_all_consumers_statically_accounted(&self, domain_id: &str) -> bool {
        self.domains
            .iter()
            .find(|domain| domain.id == domain_id)
            .is_some_and(|domain| domain.all_consumers_statically_accounted)
    }

    pub(super) fn domain_has_carried_consumers(&self, domain_id: &str) -> bool {
        self.domains
            .iter()
            .find(|domain| domain.id == domain_id)
            .is_some_and(|domain| !domain.current_candidate_carried_screen_roles.is_empty())
    }

    pub(super) fn domain_has_newly_planned_consumers(&self, domain_id: &str) -> bool {
        self.domains
            .iter()
            .find(|domain| domain.id == domain_id)
            .is_some_and(|domain| {
                !domain.globally_planned_screen_roles.is_empty()
                    && (!domain.newly_planned_screen_roles.is_empty()
                        || domain.current_candidate_installed_target_unit_count
                            < domain.target_unit_count)
            })
    }
}

pub(super) fn plan_consumer_installation(
    inputs: ConsumerInstallationInputs<'_>,
) -> Result<ConsumerInstallationPlan> {
    let current = inspect_current_installation(
        inputs.current_build_report_path,
        inputs.current_candidate_path,
    )?;
    let targets = inspect_domain_screen_targets()?;
    let domains = assemble_domain_consumers(
        inputs.required_domains,
        inputs.target_unit_counts,
        &targets
            .into_iter()
            .map(|domain| (domain.id, domain.screen_roles))
            .collect(),
        &current.domains,
        inputs.all_chapter_titles_encoded,
        inputs.all_dialogue_records_encoded && inputs.all_dialogue_runtime_hooks_emitted,
        inputs.dynamic_dialogue_producers_bound,
    )?;

    let carried_consumer_domain_count = domains
        .iter()
        .filter(|domain| !domain.current_candidate_carried_screen_roles.is_empty())
        .count();
    let globally_advanced_domain_count = domains
        .iter()
        .filter(|domain| {
            !domain.globally_planned_screen_roles.is_empty()
                && (!domain.newly_planned_screen_roles.is_empty()
                    || domain.current_candidate_installed_target_unit_count
                        < domain.target_unit_count)
        })
        .count();
    let all_consumers_statically_accounted_domain_count = domains
        .iter()
        .filter(|domain| domain.all_consumers_statically_accounted)
        .count();
    let unresolved_consumer_domain_count =
        inputs.required_domains.len() - all_consumers_statically_accounted_domain_count;
    let current_candidate_historical_runtime_role_count = domains
        .iter()
        .map(|domain| domain.current_candidate_historical_runtime_roles.len())
        .sum();

    Ok(ConsumerInstallationPlan {
        strategy: "bind the exact cumulative candidate first, add only consumers supplied by the global dialogue runtime, and leave every other screen unresolved",
        current_candidate_sha1: current.build_output_sha1,
        current_build_report_sha1: current.build_report_sha1,
        required_domain_count: inputs.required_domains.len(),
        domains,
        carried_consumer_domain_count,
        globally_advanced_domain_count,
        all_consumers_statically_accounted_domain_count,
        unresolved_consumer_domain_count,
        current_candidate_historical_runtime_role_count,
        final_artifact_runtime_bound_role_count: 0,
        all_required_consumers_statically_accounted: unresolved_consumer_domain_count == 0,
        current_candidate_runtime_evidence_inherited: false,
        final_artifact_runtime_replay_required: true,
    })
}

fn assemble_domain_consumers(
    required_domains: &[&'static str],
    target_unit_counts: &BTreeMap<&'static str, usize>,
    targets: &BTreeMap<&'static str, Vec<String>>,
    current: &BTreeMap<&'static str, DomainInstallation>,
    all_chapter_titles_encoded: bool,
    global_dialogue_runtime_planned: bool,
    dynamic_dialogue_producers_bound: bool,
) -> Result<Vec<DomainConsumerInstallation>> {
    ensure!(
        required_domains
            .iter()
            .copied()
            .collect::<BTreeSet<_>>()
            .len()
            == required_domains.len(),
        "consumer installation contains duplicate required domains"
    );

    required_domains
        .iter()
        .copied()
        .map(|id| {
            let target_screen_roles = targets
                .get(id)
                .with_context(|| format!("required translation domain {id} has no screen targets"))?
                .iter()
                .cloned()
                .collect::<BTreeSet<_>>();
            let target_unit_count = *target_unit_counts
                .get(id)
                .with_context(|| format!("required translation domain {id} has no target count"))?;
            ensure!(
                target_unit_count != 0,
                "required translation domain {id} has an empty target population"
            );
            let installation = current.get(id).cloned().unwrap_or_default();
            ensure!(
                installation.installed_target_unit_count <= target_unit_count,
                "current candidate installs more {id} units than the current translation input"
            );
            let current_candidate_carried_screen_roles = installation
                .installed_screen_roles
                .into_iter()
                .collect::<BTreeSet<_>>();
            ensure!(
                current_candidate_carried_screen_roles.is_subset(&target_screen_roles),
                "current candidate installs {id} outside its canonical consumer set"
            );
            let current_candidate_historical_runtime_roles = installation
                .runtime_bound_screen_roles
                .into_iter()
                .collect::<BTreeSet<_>>();
            ensure!(
                current_candidate_historical_runtime_roles
                    .is_subset(&current_candidate_carried_screen_roles),
                "current candidate runtime-binds an uninstalled {id} consumer"
            );

            let mut globally_planned_screen_roles = BTreeSet::new();
            if all_chapter_titles_encoded && id == "chapter_titles" {
                globally_planned_screen_roles
                    .insert("chapter_intro_title_dialogue_composite".to_owned());
            }
            if global_dialogue_runtime_planned && id == "main_dialogue" {
                globally_planned_screen_roles.extend(target_screen_roles.iter().cloned());
            }
            if global_dialogue_runtime_planned && dynamic_dialogue_producers_bound {
                match id {
                    // The epilogue source binding proves that its selector family consumes both
                    // translated domains through the global dialogue runtime. Other item/name
                    // surfaces remain owned by their distinct fixed-text consumers.
                    "unit_names" | "location_names" => {
                        globally_planned_screen_roles.insert(ENDING_CHARACTER_EPILOGUE.to_owned());
                    }
                    _ => {}
                }
            }
            ensure!(
                globally_planned_screen_roles.is_subset(&target_screen_roles),
                "global dialogue runtime plans {id} outside its canonical consumer set"
            );

            let newly_planned_screen_roles = globally_planned_screen_roles
                .difference(&current_candidate_carried_screen_roles)
                .cloned()
                .collect::<BTreeSet<_>>();
            let fully_carried_screen_roles =
                if installation.installed_target_unit_count == target_unit_count {
                    current_candidate_carried_screen_roles.clone()
                } else {
                    BTreeSet::new()
                };
            let statically_accounted_screen_roles = fully_carried_screen_roles
                .union(&globally_planned_screen_roles)
                .cloned()
                .collect::<BTreeSet<_>>();
            let remaining_screen_roles = target_screen_roles
                .difference(&statically_accounted_screen_roles)
                .cloned()
                .collect::<Vec<_>>();

            Ok(DomainConsumerInstallation {
                id,
                target_unit_count,
                current_candidate_installed_target_unit_count: installation
                    .installed_target_unit_count,
                globally_planned_target_unit_count: if globally_planned_screen_roles.is_empty() {
                    0
                } else {
                    target_unit_count
                },
                target_screen_roles: target_screen_roles.into_iter().collect(),
                current_candidate_carried_screen_roles: current_candidate_carried_screen_roles
                    .into_iter()
                    .collect(),
                globally_planned_screen_roles: globally_planned_screen_roles.into_iter().collect(),
                newly_planned_screen_roles: newly_planned_screen_roles.into_iter().collect(),
                statically_accounted_screen_roles: statically_accounted_screen_roles
                    .into_iter()
                    .collect(),
                all_consumers_statically_accounted: remaining_screen_roles.is_empty(),
                remaining_screen_roles,
                current_candidate_historical_runtime_roles:
                    current_candidate_historical_runtime_roles
                        .into_iter()
                        .collect(),
                final_artifact_runtime_bound_screen_roles: Vec::new(),
            })
        })
        .collect()
}

#[cfg(test)]
mod tests;
