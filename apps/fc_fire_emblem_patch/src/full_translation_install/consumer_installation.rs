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
    pub(super) all_dialogue_runtime_hook_roles_assembled: bool,
    pub(super) dynamic_dialogue_producers_bound: bool,
    pub(super) globally_planned_consumer_roles: &'a BTreeMap<&'static str, BTreeSet<String>>,
}

#[derive(Serialize)]
pub(super) struct ConsumerInstallationPlan {
    strategy: &'static str,
    current_candidate_sha1: String,
    current_build_report_sha1: String,
    declared_domain_count: usize,
    domains: Vec<DomainConsumerInstallation>,
    declared_domain_with_carried_consumers_count: usize,
    declared_domain_with_global_plan_count: usize,
    statically_accounted_declared_domain_count: usize,
    declared_domain_with_unaccounted_consumers_count: usize,
    declared_consumer_historical_runtime_role_count: usize,
    declared_consumer_runtime_observed_role_count: usize,
    all_declared_consumers_statically_accounted: bool,
    current_candidate_runtime_evidence_inherited: bool,
    declared_consumer_runtime_replay_required: bool,
}

#[derive(Serialize)]
struct DomainConsumerInstallation {
    id: &'static str,
    target_unit_count: usize,
    current_candidate_installed_target_unit_count: usize,
    globally_planned_target_unit_count: usize,
    declared_screen_roles: Vec<String>,
    current_candidate_carried_declared_screen_roles: Vec<String>,
    globally_planned_declared_screen_roles: Vec<String>,
    newly_planned_declared_screen_roles: Vec<String>,
    statically_accounted_declared_screen_roles: Vec<String>,
    unaccounted_declared_screen_roles: Vec<String>,
    current_candidate_historical_declared_runtime_roles: Vec<String>,
    runtime_observed_declared_screen_roles: Vec<String>,
    all_declared_consumers_statically_accounted: bool,
}

impl ConsumerInstallationPlan {
    pub(super) fn all_declared_consumers_statically_accounted(&self) -> bool {
        self.all_declared_consumers_statically_accounted
    }

    pub(super) fn statically_accounted_declared_domain_count(&self) -> usize {
        self.statically_accounted_declared_domain_count
    }

    pub(super) fn domain_has_all_declared_consumers_statically_accounted(
        &self,
        domain_id: &str,
    ) -> bool {
        self.domains
            .iter()
            .find(|domain| domain.id == domain_id)
            .is_some_and(|domain| domain.all_declared_consumers_statically_accounted)
    }

    pub(super) fn domain_has_carried_consumers(&self, domain_id: &str) -> bool {
        self.domains
            .iter()
            .find(|domain| domain.id == domain_id)
            .is_some_and(|domain| {
                !domain
                    .current_candidate_carried_declared_screen_roles
                    .is_empty()
            })
    }

    pub(super) fn domain_has_newly_planned_consumers(&self, domain_id: &str) -> bool {
        self.domains
            .iter()
            .find(|domain| domain.id == domain_id)
            .is_some_and(|domain| {
                !domain.globally_planned_declared_screen_roles.is_empty()
                    && (!domain.newly_planned_declared_screen_roles.is_empty()
                        || domain.current_candidate_installed_target_unit_count
                            < domain.target_unit_count)
            })
    }

    pub(super) fn bind_declared_consumer_runtime_roles(
        &mut self,
        observed_roles: &BTreeSet<String>,
        registered_roles: &BTreeSet<String>,
    ) -> Result<()> {
        let target_roles = self
            .domains
            .iter()
            .flat_map(|domain| domain.declared_screen_roles.iter().cloned())
            .collect::<BTreeSet<_>>();
        let unknown_roles = observed_roles
            .difference(registered_roles)
            .cloned()
            .collect::<Vec<_>>();
        ensure!(
            unknown_roles.is_empty(),
            "final-artifact runtime evidence names unknown translation screen roles: {}",
            unknown_roles.join(", ")
        );

        for domain in &mut self.domains {
            domain.runtime_observed_declared_screen_roles = domain
                .declared_screen_roles
                .iter()
                .filter(|role| observed_roles.contains(*role))
                .cloned()
                .collect();
        }
        let required_observed_roles = observed_roles
            .intersection(&target_roles)
            .cloned()
            .collect::<BTreeSet<_>>();
        self.declared_consumer_runtime_observed_role_count = required_observed_roles.len();
        self.declared_consumer_runtime_replay_required = required_observed_roles != target_roles;
        Ok(())
    }

    pub(super) fn declared_consumer_runtime_replay_required(&self) -> bool {
        self.declared_consumer_runtime_replay_required
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
        inputs.all_dialogue_records_encoded && inputs.all_dialogue_runtime_hook_roles_assembled,
        inputs.dynamic_dialogue_producers_bound,
        inputs.globally_planned_consumer_roles,
    )?;

    let declared_domain_with_carried_consumers_count = domains
        .iter()
        .filter(|domain| {
            !domain
                .current_candidate_carried_declared_screen_roles
                .is_empty()
        })
        .count();
    let declared_domain_with_global_plan_count = domains
        .iter()
        .filter(|domain| {
            !domain.globally_planned_declared_screen_roles.is_empty()
                && (!domain.newly_planned_declared_screen_roles.is_empty()
                    || domain.current_candidate_installed_target_unit_count
                        < domain.target_unit_count)
        })
        .count();
    let statically_accounted_declared_domain_count = domains
        .iter()
        .filter(|domain| domain.all_declared_consumers_statically_accounted)
        .count();
    let declared_domain_with_unaccounted_consumers_count =
        inputs.required_domains.len() - statically_accounted_declared_domain_count;
    let declared_consumer_historical_runtime_role_count = domains
        .iter()
        .map(|domain| {
            domain
                .current_candidate_historical_declared_runtime_roles
                .len()
        })
        .sum();

    Ok(ConsumerInstallationPlan {
        strategy: "bind the exact cumulative candidate first, union only source-bound runtime and storage-projection consumers within the declared domain plan, and report every remaining declared screen role as unaccounted without implying a whole-game census",
        current_candidate_sha1: current.build_output_sha1,
        current_build_report_sha1: current.build_report_sha1,
        declared_domain_count: inputs.required_domains.len(),
        domains,
        declared_domain_with_carried_consumers_count,
        declared_domain_with_global_plan_count,
        statically_accounted_declared_domain_count,
        declared_domain_with_unaccounted_consumers_count,
        declared_consumer_historical_runtime_role_count,
        declared_consumer_runtime_observed_role_count: 0,
        all_declared_consumers_statically_accounted:
            declared_domain_with_unaccounted_consumers_count == 0,
        current_candidate_runtime_evidence_inherited: false,
        declared_consumer_runtime_replay_required: true,
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
    additional_globally_planned_roles: &BTreeMap<&'static str, BTreeSet<String>>,
) -> Result<Vec<DomainConsumerInstallation>> {
    ensure!(
        required_domains
            .iter()
            .copied()
            .collect::<BTreeSet<_>>()
            .len()
            == required_domains.len(),
        "consumer installation contains duplicate declared domains"
    );

    required_domains
        .iter()
        .copied()
        .map(|id| {
            let declared_screen_roles = targets
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
                .consumer_complete_screen_roles
                .into_iter()
                .collect::<BTreeSet<_>>();
            ensure!(
                current_candidate_carried_screen_roles.is_subset(&declared_screen_roles),
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
                globally_planned_screen_roles.extend(declared_screen_roles.iter().cloned());
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
            globally_planned_screen_roles.extend(
                additional_globally_planned_roles
                    .get(id)
                    .into_iter()
                    .flatten()
                    .cloned(),
            );
            ensure!(
                globally_planned_screen_roles.is_subset(&declared_screen_roles),
                "global dialogue runtime plans {id} outside its canonical consumer set"
            );

            let newly_planned_screen_roles = globally_planned_screen_roles
                .difference(&current_candidate_carried_screen_roles)
                .cloned()
                .collect::<BTreeSet<_>>();
            let statically_accounted_screen_roles = current_candidate_carried_screen_roles
                .union(&globally_planned_screen_roles)
                .cloned()
                .collect::<BTreeSet<_>>();
            let unaccounted_declared_screen_roles = declared_screen_roles
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
                declared_screen_roles: declared_screen_roles.into_iter().collect(),
                current_candidate_carried_declared_screen_roles:
                    current_candidate_carried_screen_roles.into_iter().collect(),
                globally_planned_declared_screen_roles: globally_planned_screen_roles
                    .into_iter()
                    .collect(),
                newly_planned_declared_screen_roles: newly_planned_screen_roles
                    .into_iter()
                    .collect(),
                statically_accounted_declared_screen_roles: statically_accounted_screen_roles
                    .into_iter()
                    .collect(),
                all_declared_consumers_statically_accounted: unaccounted_declared_screen_roles
                    .is_empty(),
                unaccounted_declared_screen_roles,
                current_candidate_historical_declared_runtime_roles:
                    current_candidate_historical_runtime_roles
                        .into_iter()
                        .collect(),
                runtime_observed_declared_screen_roles: Vec::new(),
            })
        })
        .collect()
}

#[cfg(test)]
mod tests;
