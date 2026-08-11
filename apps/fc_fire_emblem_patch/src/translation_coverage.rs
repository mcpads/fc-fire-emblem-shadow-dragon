use std::{collections::BTreeSet, fs, path::Path};

use anyhow::{Context, Result, ensure};

use crate::{
    rom::EXPECTED_SOURCE_SHA1, screen_contracts::inspect_screen_translation_partition, sha1_hex,
};

mod installed;
mod lifetimes;
mod population;
mod report;
mod screen_targets;

use installed::inspect_current_installation;
use lifetimes::{LifetimeInputBindings, inspect_translation_lifetimes};
use population::{TranslationPopulationInputs, inspect_translation_populations};
pub(crate) use report::TranslationCoverageSummary;
use report::{
    CapacityState, CoverageSummary, GlobalTranslationCoverageReport, ScreenPopulationReport,
    SourceBindingState, TranslationDomainReport, TranslationInputState,
};
use screen_targets::{DOMAIN_SEEDS, bind_domain_screen_targets};

pub(crate) struct TranslationCoverageInputs<'a> {
    pub(crate) source_path: &'a Path,
    pub(crate) main_dialogue_workspace_path: &'a Path,
    pub(crate) battle_dialogue_workspace_path: &'a Path,
    pub(crate) fixed_text_workspace_path: &'a Path,
    pub(crate) options_localization_path: &'a Path,
    pub(crate) roster_localization_path: &'a Path,
    pub(crate) front_end_menu_localization_path: &'a Path,
    pub(crate) unit_name_localization_path: &'a Path,
    pub(crate) class_profile_localization_path: &'a Path,
    pub(crate) chapter_title_localization_path: &'a Path,
    pub(crate) choice_label_localization_path: &'a Path,
    pub(crate) map_menu_localization_path: &'a Path,
    pub(crate) title_graphics_localization_path: &'a Path,
    pub(crate) unit_ui_label_localization_path: &'a Path,
    pub(crate) item_action_label_localization_path: &'a Path,
    pub(crate) transition_label_localization_path: &'a Path,
    pub(crate) location_name_localization_path: &'a Path,
    pub(crate) current_build_output_path: &'a Path,
    pub(crate) current_build_report_path: &'a Path,
    pub(crate) main_dialogue_glyph_workset_report_path: &'a Path,
    pub(crate) battle_surface_constraints_report_path: &'a Path,
    pub(crate) report_path: &'a Path,
}

pub(crate) fn analyze_translation_coverage(
    inputs: TranslationCoverageInputs<'_>,
) -> Result<TranslationCoverageSummary> {
    let partition = inspect_screen_translation_partition()?;
    let screen_targets = bind_domain_screen_targets(&partition)?;
    let mut populations = inspect_translation_populations(&TranslationPopulationInputs {
        source_path: inputs.source_path,
        main_dialogue_workspace_path: inputs.main_dialogue_workspace_path,
        battle_dialogue_workspace_path: inputs.battle_dialogue_workspace_path,
        fixed_text_workspace_path: inputs.fixed_text_workspace_path,
        options_localization_path: inputs.options_localization_path,
        roster_localization_path: inputs.roster_localization_path,
        front_end_menu_localization_path: inputs.front_end_menu_localization_path,
        unit_name_localization_path: inputs.unit_name_localization_path,
        class_profile_localization_path: inputs.class_profile_localization_path,
        chapter_title_localization_path: inputs.chapter_title_localization_path,
        choice_label_localization_path: inputs.choice_label_localization_path,
        map_menu_localization_path: inputs.map_menu_localization_path,
        title_graphics_localization_path: inputs.title_graphics_localization_path,
        unit_ui_label_localization_path: inputs.unit_ui_label_localization_path,
        item_action_label_localization_path: inputs.item_action_label_localization_path,
        transition_label_localization_path: inputs.transition_label_localization_path,
        location_name_localization_path: inputs.location_name_localization_path,
    })?;
    let mut installation = inspect_current_installation(
        inputs.current_build_report_path,
        inputs.current_build_output_path,
    )?;

    let expected_domain_ids = DOMAIN_SEEDS
        .iter()
        .map(|domain| domain.id)
        .collect::<BTreeSet<_>>();
    ensure!(
        populations.keys().copied().collect::<BTreeSet<_>>() == expected_domain_ids,
        "translation populations do not cover the complete domain registry"
    );
    ensure!(
        installation
            .domains
            .keys()
            .all(|domain_id| expected_domain_ids.contains(domain_id)),
        "current installation uses an unknown translation domain"
    );

    let mut domains = Vec::with_capacity(screen_targets.len());
    for targets in screen_targets {
        let population = populations
            .remove(targets.id)
            .with_context(|| format!("translation population lost domain {}", targets.id))?;
        let installed = installation.domains.remove(targets.id).unwrap_or_default();
        let target_roles = targets
            .screen_roles
            .iter()
            .map(String::as_str)
            .collect::<BTreeSet<_>>();
        ensure!(
            installed
                .installed_screen_roles
                .iter()
                .all(|role| target_roles.contains(role.as_str())),
            "domain {} installs a screen outside its consumer set",
            targets.id
        );
        let installed_roles = installed
            .installed_screen_roles
            .iter()
            .map(String::as_str)
            .collect::<BTreeSet<_>>();
        ensure!(
            installed
                .runtime_bound_screen_roles
                .iter()
                .all(|role| installed_roles.contains(role.as_str())),
            "domain {} runtime-binds a screen that is not installed",
            targets.id
        );
        if let Some(target_count) = population.target_unit_count {
            ensure!(
                installed.installed_target_unit_count <= target_count,
                "domain {} installs more target units than its source population",
                targets.id
            );
        } else {
            ensure!(
                installed.installed_target_unit_count == 0,
                "domain {} installs units before its source population is bound",
                targets.id
            );
        }
        let all_target_units_installed = population
            .target_unit_count
            .is_some_and(|count| installed.installed_target_unit_count == count);
        let all_consumers_installed = all_target_units_installed && installed_roles == target_roles;
        let runtime_roles = installed
            .runtime_bound_screen_roles
            .iter()
            .map(String::as_str)
            .collect::<BTreeSet<_>>();
        let all_consumers_runtime_bound = all_consumers_installed && runtime_roles == target_roles;
        domains.push(TranslationDomainReport {
            id: targets.id,
            target_unit: targets.target_unit,
            source_binding: population.source_binding,
            target_unit_count: population.target_unit_count,
            translated_target_unit_count: population.translated_target_unit_count,
            translation_input: population.translation_input,
            review_complete: population.review_complete,
            translation_input_sha1: population.translation_input_sha1,
            installed_target_unit_count: installed.installed_target_unit_count,
            target_screen_roles: targets.screen_roles,
            installed_screen_roles: installed.installed_screen_roles,
            runtime_bound_screen_roles: installed.runtime_bound_screen_roles,
            all_target_units_installed,
            all_consumers_installed,
            all_consumers_runtime_bound,
            capacity_state: CapacityState::NotEvaluatedInGlobalContext,
        });
    }
    ensure!(
        populations.is_empty() && installation.domains.is_empty(),
        "translation coverage left unmatched domain data"
    );

    let summary = CoverageSummary {
        domain_count: domains.len(),
        source_bound_domain_count: domains
            .iter()
            .filter(|domain| domain.source_binding == SourceBindingState::Bound)
            .count(),
        translation_input_complete_domain_count: domains
            .iter()
            .filter(|domain| domain.translation_input == TranslationInputState::Complete)
            .count(),
        review_complete_domain_count: domains
            .iter()
            .filter(|domain| domain.review_complete)
            .count(),
        all_consumers_installed_domain_count: domains
            .iter()
            .filter(|domain| domain.all_consumers_installed)
            .count(),
        all_consumers_runtime_bound_domain_count: domains
            .iter()
            .filter(|domain| domain.all_consumers_runtime_bound)
            .count(),
        unresolved_source_domain_ids: domains
            .iter()
            .filter(|domain| domain.source_binding == SourceBindingState::Unresolved)
            .map(|domain| domain.id)
            .collect(),
        incomplete_translation_input_domain_ids: domains
            .iter()
            .filter(|domain| domain.translation_input != TranslationInputState::Complete)
            .map(|domain| domain.id)
            .collect(),
        pending_review_domain_ids: domains
            .iter()
            .filter(|domain| !domain.review_complete)
            .map(|domain| domain.id)
            .collect(),
        incomplete_installation_domain_ids: domains
            .iter()
            .filter(|domain| !domain.all_consumers_installed)
            .map(|domain| domain.id)
            .collect(),
    };
    let translation_input_sha1 = |domain_id: &str| -> Result<&str> {
        domains
            .iter()
            .find(|domain| domain.id == domain_id)
            .and_then(|domain| domain.translation_input_sha1.as_deref())
            .with_context(|| format!("translation domain {domain_id} has no input hash"))
    };
    let japanese_bearing_screen_roles = partition
        .japanese_bearing_screens
        .iter()
        .map(|screen| screen.role.clone())
        .collect::<Vec<_>>();
    let lifetime_inventory = inspect_translation_lifetimes(
        inputs.main_dialogue_glyph_workset_report_path,
        inputs.battle_surface_constraints_report_path,
        LifetimeInputBindings {
            main_dialogue_workspace_sha1: translation_input_sha1("main_dialogue")?,
            battle_fixed_workspace_sha1: &installation.battle_fixed_workspace_sha1,
            battle_dialogue_workspace_sha1: &installation.battle_dialogue_workspace_sha1,
            battle_temporal_manifest_sha1: &installation.battle_temporal_manifest_sha1,
        },
        &japanese_bearing_screen_roles,
    )?;
    let report = GlobalTranslationCoverageReport {
        schema: 1,
        source_sha1: EXPECTED_SOURCE_SHA1,
        build_output_sha1: installation.build_output_sha1,
        screen_population: ScreenPopulationReport {
            screen_count: partition.screen_count,
            japanese_bearing_screen_count: partition.japanese_bearing_screens.len(),
            preserved_original_only_screen_count: partition.preserved_original_only_screen_count,
            no_text_screen_count: partition.no_text_screen_count,
            mapped_japanese_bearing_screen_count: partition.japanese_bearing_screens.len(),
            unmapped_japanese_bearing_screen_roles: Vec::new(),
        },
        domains,
        lifetime_demands: lifetime_inventory.demands,
        unmeasured_screen_roles: lifetime_inventory.unmeasured_screen_roles,
        strongest_lifetime: lifetime_inventory.strongest,
        summary,
        translation_text_emitted: false,
        glyph_characters_emitted: false,
        release_eligible: false,
    };
    let mut report_bytes =
        serde_json::to_vec_pretty(&report).context("serialize translation coverage report")?;
    report_bytes.push(b'\n');
    if let Some(parent) = inputs.report_path.parent() {
        fs::create_dir_all(parent).with_context(|| format!("create {}", parent.display()))?;
    }
    fs::write(inputs.report_path, &report_bytes)
        .with_context(|| format!("write {}", inputs.report_path.display()))?;
    Ok(TranslationCoverageSummary {
        report_sha1: sha1_hex(&report_bytes),
        japanese_bearing_screen_count: report.screen_population.japanese_bearing_screen_count,
        domain_count: report.summary.domain_count,
        unresolved_source_domain_count: report.summary.unresolved_source_domain_ids.len(),
        all_consumers_installed_domain_count: report.summary.all_consumers_installed_domain_count,
    })
}
