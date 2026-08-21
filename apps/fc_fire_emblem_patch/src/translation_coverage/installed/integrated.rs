//! Rebinds the cumulative installation inventory to the exact globally
//! integrated artifact.
//!
//! The cumulative build report still owns screen-capacity measurements. It is
//! not, however, evidence that those consumers survived later global runtime
//! and storage rewrites. The integrated report therefore replaces installation
//! state for its declared domains and clears inherited completion/runtime
//! claims for every other domain until the exact final artifact observes them.

use std::{
    collections::{BTreeMap, BTreeSet},
    fs,
    path::Path,
};

use anyhow::{Context, Result, ensure};
use serde::Deserialize;

use super::{
    CurrentInstallation, InstalledChoiceLifetime, InstalledFixedMenuLifetime,
    InstalledLifetimeDemand, inspect_current_installation,
};
use crate::{
    fixed_menu_labels::{
        FIXED_MENU_LABEL_SPECS, UNIT_SELECTION_HELP_LINE_SPECS, fixed_menu_screen_roles,
    },
    font_slots::ACTIVE_HANGUL_SLOT_COUNT,
    full_translation_install::{FULL_TRANSLATION_REPORT_SCHEMA, STORAGE_CHOICE_DIALOGUE_RECORD_ID},
    rom::EXPECTED_SOURCE_SHA1,
    sha1_hex,
    translation_coverage::{
        DomainInstallation, inspect_domain_screen_targets,
        screen_targets::STORAGE_FOLLOW_UP_CHOICE_SCREEN_ROLE,
    },
};

const CARRIED_BATTLE_DOMAIN_IDS: [&str; 4] = [
    "battle_dialogue",
    "battle_forecast_label",
    "battle_message_templates",
    "terrain_names",
];
const CARRIED_UI_DOMAIN_IDS: [&str; 5] = [
    "class_profiles",
    "front_end_menu_labels",
    "options_labels",
    "roster_header",
    "title_graphics",
];

#[derive(Debug, Deserialize)]
struct IntegratedBuildReport {
    schema: u8,
    source_sha1: String,
    declared_installation_domain_count: usize,
    declared_installation_domains: Vec<String>,
    consumer_installation: IntegratedConsumerInstallation,
    carried_ui_domain_preservation: CarriedUiDomainPreservation,
    carried_battle_domain_preservation: CarriedBattleDomainPreservation,
    integrated_write_set: IntegratedWriteSet,
    choice_residency: IntegratedChoiceResidency,
    consumer_codebook: IntegratedConsumerCodebook,
    unit_selection_help_residency: IntegratedUnitSelectionHelpResidency,
    storage_dialogue_residency: IntegratedStorageDialogueResidency,
    final_artifact_runtime_evidence: FinalArtifactRuntimeEvidence,
    installation_gates: IntegratedInstallationGates,
    rom_emitted: bool,
}

#[derive(Debug, Deserialize)]
struct IntegratedChoiceResidency {
    composite_state: u8,
    continue_prompt_record_id: String,
    front_end_result_record_ids: Vec<String>,
    storage_choice_record_id: String,
    direct_choice_composite_producer_count: usize,
    resident_record_ids: Vec<String>,
    resident_workset_count: usize,
    choice_glyph_count: usize,
    fixed_code_count: usize,
    maximum_augmented_workset_slot_demand: usize,
    storage_follow_up_workset_count: usize,
    storage_follow_up_target_glyph_count: usize,
    storage_follow_up_preserved_active_code_count: usize,
    storage_follow_up_total_slot_demand: usize,
    fixed_assignment_sha1: String,
    every_choice_glyph_has_one_stable_code: bool,
    every_resident_page_contains_every_choice_glyph: bool,
}

#[derive(Debug, Deserialize)]
struct IntegratedConsumerCodebook {
    schema: u8,
    static_page_count: usize,
    pages: Vec<IntegratedStaticConsumerPage>,
    page_bytes_planned: bool,
}

#[derive(Debug, Deserialize)]
struct IntegratedStaticConsumerPage {
    id: String,
    variant: String,
    screen_roles: Vec<String>,
    domain_ids: Vec<String>,
    target_glyph_count: usize,
    preserved_active_code_count: usize,
    slot_demand: usize,
    assignment_sha1: String,
    page_sha1: String,
}

#[derive(Debug, Deserialize)]
struct IntegratedUnitSelectionHelpResidency {
    resident_workset_count: usize,
    help_line_count: usize,
    help_glyph_count: usize,
    fixed_code_count: usize,
    maximum_augmented_workset_target_glyph_count: usize,
    maximum_augmented_workset_preserved_active_code_count: usize,
    maximum_augmented_workset_slot_demand: usize,
    source_lifecycle_bound: bool,
    codes_selected_by_the_global_consumer_codebook: bool,
    every_help_glyph_keeps_one_code_through_the_dialogue_handoff: bool,
}

#[derive(Debug, Deserialize)]
struct IntegratedStorageDialogueResidency {
    source_dispatch_count: usize,
    source_direct_record_store_count: usize,
    fixed_glyph_count: usize,
    fixed_code_count: usize,
    maximum_augmented_workset_target_glyph_count: usize,
    maximum_augmented_workset_preserved_active_code_count: usize,
    maximum_augmented_workset_slot_demand: usize,
    every_storage_label_glyph_uses_its_installed_code: bool,
    every_overlay_dialogue_page_contains_its_visible_storage_label_glyphs: bool,
    storage_dialogue_does_not_reselect_the_static_menu_page: bool,
    capacity_notice_keeps_its_standalone_static_page: bool,
    item_list_dialogue_pages_use_canonical_item_codes: bool,
}

#[derive(Debug, Deserialize)]
struct IntegratedConsumerInstallation {
    current_candidate_sha1: String,
    current_build_report_sha1: String,
    declared_domain_count: usize,
    domains: Vec<IntegratedConsumerDomain>,
    statically_accounted_declared_domain_count: usize,
    declared_domain_with_unaccounted_consumers_count: usize,
    all_declared_consumers_statically_accounted: bool,
    current_candidate_runtime_evidence_inherited: bool,
}

#[derive(Debug, Deserialize)]
struct IntegratedConsumerDomain {
    id: String,
    target_unit_count: usize,
    globally_planned_target_unit_count: usize,
    declared_screen_roles: Vec<String>,
    statically_accounted_declared_screen_roles: Vec<String>,
    unaccounted_declared_screen_roles: Vec<String>,
    runtime_observed_declared_screen_roles: Vec<String>,
    all_declared_consumers_statically_accounted: bool,
}

#[derive(Debug, Deserialize)]
struct CarriedUiDomainPreservation {
    cumulative_candidate_sha1: String,
    cumulative_report_sha1: String,
    integrated_image_sha1: String,
    domain_count: usize,
    domains: Vec<CarriedUiDomain>,
    all_translation_inputs_rebound: bool,
    all_storage_regions_rebound: bool,
    all_font_regions_rebound: bool,
    all_consumer_routes_rebound: bool,
    all_final_semantic_bindings_verified: bool,
    complete: bool,
}

#[derive(Debug, Deserialize)]
struct CarriedUiDomain {
    id: String,
    target_unit_count: usize,
    screen_roles: Vec<String>,
    translation_input_bound: bool,
    final_semantic_binding: FinalSemanticBinding,
    stored_target_content_matches_translation_input: bool,
    final_visual_material_matches_target_content: bool,
    storage_regions: Vec<CarriedUiRegion>,
    font_regions: Vec<CarriedUiRegion>,
    consumer_regions: Vec<CarriedUiRegion>,
    consumer_route_binding_ids: Vec<String>,
    complete_for_declared_domain_plan: bool,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq)]
#[serde(rename_all = "snake_case")]
enum FinalSemanticBinding {
    PreservedStorageAndVisualMaterial,
    ReencodedStorageAndVisualMaterial,
}

#[derive(Debug, Deserialize)]
struct CarriedUiRegion {
    role: String,
    binding_kind: String,
    file_offset_hex: String,
    byte_count: usize,
    sha1: String,
    final_bytes_match_binding: bool,
}

#[derive(Debug, Deserialize)]
struct CarriedBattleDomainPreservation {
    cumulative_candidate_sha1: String,
    cumulative_report_sha1: String,
    integrated_image_sha1: String,
    domain_count: usize,
    domains: Vec<CarriedBattleDomain>,
    shared_screen_roles: Vec<String>,
    shared_font_regions: Vec<CarriedUiRegion>,
    shared_consumer_regions: Vec<CarriedUiRegion>,
    shared_consumer_route_binding_ids: Vec<String>,
    all_translation_inputs_rebound: bool,
    all_storage_regions_rebound: bool,
    shared_font_supply_rebound: bool,
    shared_consumer_route_rebound: bool,
    complete: bool,
}

#[derive(Debug, Deserialize)]
struct CarriedBattleDomain {
    id: String,
    target_unit_count: usize,
    translation_input_bound: bool,
    storage_regions: Vec<CarriedUiRegion>,
    complete_for_declared_domain_plan: bool,
}

#[derive(Debug, Deserialize)]
struct IntegratedWriteSet {
    declared_domain_count: usize,
    domains: Vec<IntegratedWriteDomain>,
    declared_domain_with_expected_writes_count: usize,
    statically_accounted_declared_domain_count: usize,
    original_candidate_sha1: String,
    planned_final_image_byte_count: usize,
    integrated_image_sha1: String,
    every_change_tracked: bool,
    required_mutation_identity_set_complete: bool,
    required_runtime_routine_identities_installed: bool,
    required_runtime_hook_identities_installed: bool,
    final_replacement_bytes_match_manifest: bool,
    technical_installation_complete: bool,
    one_shared_image: bool,
    all_declared_domains_contribute_expected_writes: bool,
    rom_emitted: bool,
}

#[derive(Debug, Deserialize)]
struct IntegratedWriteDomain {
    id: String,
    expected_write_count: usize,
    translation_input_loaded: bool,
    glyph_lifetime_bound: bool,
    storage_and_address_writes_contributed: bool,
    runtime_material_writes_contributed: bool,
    font_supply_writes_contributed: bool,
    all_declared_consumer_writes_contributed: bool,
    complete_for_declared_domain_plan: bool,
}

#[derive(Debug, Deserialize)]
struct FinalArtifactRuntimeEvidence {
    provided: bool,
    manifest_sha1: String,
    artifact_sha1: String,
    run_count: usize,
    route_count: usize,
    route_checkpoint_count: usize,
    observation_count: usize,
    sample_count: usize,
    bound_screen_roles: Vec<String>,
    every_run_started_from_cold_boot: bool,
    savestate_free: bool,
    every_sample_image_digest_bound: bool,
}

#[derive(Debug, Deserialize)]
struct IntegratedInstallationGates {
    all_translation_inputs_loaded: bool,
    all_dialogue_records_encoded: bool,
    all_visible_dialogue_text_encoded: bool,
    all_dialogue_pointers_planned: bool,
    all_dialogue_page_code_assignments_found: bool,
    all_dialogue_page_worksets_packed: bool,
    all_resident_dialogue_transitions_use_one_codebook: bool,
    all_chapter_titles_encoded_with_resident_codes: bool,
    all_chapter_title_storage_writes_planned: bool,
    cold_request_presentation_page_planned: bool,
    cold_request_presentation_write_planned: bool,
    dialogue_runtime_composition_planned: bool,
    all_declared_consumer_writes_planned: bool,
    declared_plan_technical_installation_complete: bool,
    all_carried_ui_domains_reinspected: bool,
    all_carried_battle_domains_reinspected: bool,
}

#[derive(Debug)]
struct IntegratedInstallationEvidence {
    output_sha1: String,
    report_sha1: String,
    final_static_domain_ids: BTreeSet<String>,
    domains: BTreeMap<String, DomainInstallation>,
    fixed_menu_lifetime: InstalledFixedMenuLifetime,
    choice_lifetime: InstalledChoiceLifetime,
}

pub(crate) fn inspect_integrated_installation(
    cumulative_report_path: &Path,
    cumulative_output_path: &Path,
    integrated_report_path: &Path,
    integrated_output_path: &Path,
) -> Result<CurrentInstallation> {
    let mut current = inspect_current_installation(cumulative_report_path, cumulative_output_path)?;
    let report_bytes = fs::read(integrated_report_path).with_context(|| {
        format!(
            "read integrated build report {}",
            integrated_report_path.display()
        )
    })?;
    let output_bytes = fs::read(integrated_output_path).with_context(|| {
        format!(
            "read integrated build output {}",
            integrated_output_path.display()
        )
    })?;
    let evidence = bind_integrated_installation(
        &report_bytes,
        &output_bytes,
        &current.build_output_sha1,
        &current.build_report_sha1,
    )?;
    apply_integrated_installation(&mut current, evidence)?;
    Ok(current)
}

fn bind_integrated_installation(
    report_bytes: &[u8],
    output_bytes: &[u8],
    cumulative_output_sha1: &str,
    cumulative_report_sha1: &str,
) -> Result<IntegratedInstallationEvidence> {
    let report: IntegratedBuildReport =
        serde_json::from_slice(report_bytes).context("parse integrated build report")?;
    ensure!(
        report.schema == FULL_TRANSLATION_REPORT_SCHEMA
            && report.source_sha1 == EXPECTED_SOURCE_SHA1,
        "integrated build report is not bound to the supported source"
    );
    let output_sha1 = sha1_hex(output_bytes);
    ensure!(
        report.integrated_write_set.integrated_image_sha1 == output_sha1
            && report.final_artifact_runtime_evidence.artifact_sha1 == output_sha1
            && report.integrated_write_set.planned_final_image_byte_count == output_bytes.len(),
        "integrated build report and output ROM identity differ"
    );
    ensure!(
        report.consumer_installation.current_candidate_sha1 == cumulative_output_sha1
            && report.integrated_write_set.original_candidate_sha1 == cumulative_output_sha1
            && report.consumer_installation.current_build_report_sha1 == cumulative_report_sha1
            && report
                .carried_ui_domain_preservation
                .cumulative_candidate_sha1
                == cumulative_output_sha1
            && report.carried_ui_domain_preservation.cumulative_report_sha1
                == cumulative_report_sha1
            && report.carried_ui_domain_preservation.integrated_image_sha1 == output_sha1
            && report
                .carried_battle_domain_preservation
                .cumulative_candidate_sha1
                == cumulative_output_sha1
            && report
                .carried_battle_domain_preservation
                .cumulative_report_sha1
                == cumulative_report_sha1
            && report
                .carried_battle_domain_preservation
                .integrated_image_sha1
                == output_sha1,
        "integrated build report does not descend from the supplied cumulative build"
    );

    let declared_domain_ids = unique_strings(
        report.declared_installation_domains,
        "integrated declared installation domains",
    )?;
    ensure!(
        report.declared_installation_domain_count == declared_domain_ids.len()
            && report.consumer_installation.declared_domain_count == declared_domain_ids.len()
            && report.integrated_write_set.declared_domain_count == declared_domain_ids.len(),
        "integrated report domain counts disagree"
    );
    ensure!(
        report.rom_emitted
            && report.integrated_write_set.rom_emitted
            && report.integrated_write_set.every_change_tracked
            && report
                .integrated_write_set
                .required_mutation_identity_set_complete
            && report
                .integrated_write_set
                .required_runtime_routine_identities_installed
            && report
                .integrated_write_set
                .required_runtime_hook_identities_installed
            && report
                .integrated_write_set
                .final_replacement_bytes_match_manifest
            && report.integrated_write_set.technical_installation_complete
            && report.integrated_write_set.one_shared_image
            && report
                .integrated_write_set
                .all_declared_domains_contribute_expected_writes
            && report
                .consumer_installation
                .all_declared_consumers_statically_accounted
            && !report
                .consumer_installation
                .current_candidate_runtime_evidence_inherited
            && report
                .consumer_installation
                .declared_domain_with_unaccounted_consumers_count
                == 0
            && report
                .consumer_installation
                .statically_accounted_declared_domain_count
                == declared_domain_ids.len()
            && report
                .integrated_write_set
                .statically_accounted_declared_domain_count
                == declared_domain_ids.len()
            && report
                .integrated_write_set
                .declared_domain_with_expected_writes_count
                == declared_domain_ids.len()
            && report
                .carried_ui_domain_preservation
                .all_translation_inputs_rebound
            && report
                .carried_ui_domain_preservation
                .all_storage_regions_rebound
            && report
                .carried_ui_domain_preservation
                .all_font_regions_rebound
            && report
                .carried_ui_domain_preservation
                .all_consumer_routes_rebound
            && report
                .carried_ui_domain_preservation
                .all_final_semantic_bindings_verified
            && report.carried_ui_domain_preservation.complete
            && report
                .carried_battle_domain_preservation
                .all_translation_inputs_rebound
            && report
                .carried_battle_domain_preservation
                .all_storage_regions_rebound
            && report
                .carried_battle_domain_preservation
                .shared_font_supply_rebound
            && report
                .carried_battle_domain_preservation
                .shared_consumer_route_rebound
            && report.carried_battle_domain_preservation.complete
            && installation_gates_complete(&report.installation_gates),
        "integrated technical installation gates are incomplete"
    );

    let canonical_targets = inspect_domain_screen_targets()?
        .into_iter()
        .map(|target| {
            (
                target.id,
                target.screen_roles.into_iter().collect::<BTreeSet<_>>(),
            )
        })
        .collect::<BTreeMap<_, _>>();
    let report_sha1 = sha1_hex(report_bytes);
    let fixed_menu_lifetime = bind_fixed_menu_lifetime(
        &report.consumer_codebook,
        &report.unit_selection_help_residency,
        &report.storage_dialogue_residency,
        &canonical_targets,
        &report_sha1,
    )?;
    let choice_lifetime =
        bind_choice_lifetime(&report.choice_residency, &canonical_targets, &report_sha1)?;
    let observed_screen_roles =
        bind_runtime_evidence(&report.final_artifact_runtime_evidence, &output_sha1)?;
    let carried_domains =
        bind_carried_ui_domains(report.carried_ui_domain_preservation, &canonical_targets)?;
    let carried_domain_ids = carried_domains.keys().cloned().collect::<BTreeSet<_>>();
    let carried_battle_domains = bind_carried_battle_domains(
        report.carried_battle_domain_preservation,
        &canonical_targets,
    )?;
    let carried_battle_domain_ids = carried_battle_domains
        .keys()
        .cloned()
        .collect::<BTreeSet<_>>();
    ensure!(
        carried_domain_ids.is_disjoint(&declared_domain_ids)
            && carried_battle_domain_ids.is_disjoint(&declared_domain_ids)
            && carried_domain_ids.is_disjoint(&carried_battle_domain_ids),
        "an integrated domain is both newly installed and carried from the cumulative artifact"
    );
    let mut write_domains = BTreeMap::new();
    for domain in report.integrated_write_set.domains {
        ensure!(
            domain.expected_write_count > 0
                && domain.translation_input_loaded
                && domain.glyph_lifetime_bound
                && domain.storage_and_address_writes_contributed
                && domain.runtime_material_writes_contributed
                && domain.font_supply_writes_contributed
                && domain.all_declared_consumer_writes_contributed
                && domain.complete_for_declared_domain_plan,
            "integrated domain {} did not contribute its complete write set",
            domain.id
        );
        let id = domain.id.clone();
        ensure!(
            write_domains.insert(id, domain).is_none(),
            "integrated write set repeats a domain"
        );
    }
    ensure!(
        write_domains.keys().cloned().collect::<BTreeSet<_>>() == declared_domain_ids,
        "integrated write-set domains do not match the declared installation domains"
    );

    let mut domains = BTreeMap::new();
    for domain in report.consumer_installation.domains {
        ensure!(
            declared_domain_ids.contains(&domain.id),
            "integrated consumer installation contains undeclared domain {}",
            domain.id
        );
        let canonical_roles = canonical_targets
            .get(domain.id.as_str())
            .with_context(|| format!("integrated report uses unknown domain {}", domain.id))?;
        let declared_roles = unique_strings(
            domain.declared_screen_roles,
            "integrated domain declared screen roles",
        )?;
        let statically_accounted_roles = unique_strings(
            domain.statically_accounted_declared_screen_roles,
            "integrated domain statically accounted screen roles",
        )?;
        let runtime_roles = unique_strings(
            domain.runtime_observed_declared_screen_roles,
            "integrated domain runtime-observed screen roles",
        )?;
        let expected_runtime_roles = declared_roles
            .intersection(&observed_screen_roles)
            .cloned()
            .collect::<BTreeSet<_>>();
        ensure!(
            declared_roles == *canonical_roles
                && statically_accounted_roles == declared_roles
                && runtime_roles == expected_runtime_roles
                && domain.unaccounted_declared_screen_roles.is_empty()
                && domain.all_declared_consumers_statically_accounted
                && domain.target_unit_count > 0
                && domain.globally_planned_target_unit_count == domain.target_unit_count,
            "integrated domain {} does not close its declared consumer plan",
            domain.id
        );
        let installation = DomainInstallation {
            installed_target_unit_count: domain.target_unit_count,
            installed_screen_roles: statically_accounted_roles.iter().cloned().collect(),
            consumer_complete_screen_roles: statically_accounted_roles.into_iter().collect(),
            runtime_bound_screen_roles: runtime_roles.into_iter().collect(),
        };
        ensure!(
            domains.insert(domain.id, installation).is_none(),
            "integrated consumer installation repeats a domain"
        );
    }
    ensure!(
        domains.keys().cloned().collect::<BTreeSet<_>>() == declared_domain_ids,
        "integrated consumer domains do not match the declared installation domains"
    );
    domains.extend(carried_domains);
    domains.extend(carried_battle_domains);
    let fixed_menu_installation = domains
        .get("fixed_menu_labels")
        .context("integrated installation omitted the fixed-menu domain")?;
    let fixed_menu_roles = fixed_menu_screen_roles()
        .iter()
        .map(|role| (*role).to_owned())
        .collect::<BTreeSet<_>>();
    ensure!(
        fixed_menu_installation.installed_target_unit_count
            == FIXED_MENU_LABEL_SPECS.len() + UNIT_SELECTION_HELP_LINE_SPECS.len()
            && fixed_menu_installation
                .consumer_complete_screen_roles
                .iter()
                .cloned()
                .collect::<BTreeSet<_>>()
                == fixed_menu_roles,
        "integrated fixed-menu lifetime is not attached to its complete installed domain"
    );
    let final_static_domain_ids = domains.keys().cloned().collect::<BTreeSet<_>>();

    let declared_screen_roles = domains
        .values()
        .flat_map(|domain| domain.installed_screen_roles.iter().cloned())
        .collect::<BTreeSet<_>>();
    ensure!(
        observed_screen_roles.is_subset(&declared_screen_roles),
        "final-artifact evidence names a screen role outside the integrated installation"
    );
    for domain in domains.values_mut() {
        domain.runtime_bound_screen_roles = domain
            .installed_screen_roles
            .iter()
            .filter(|role| observed_screen_roles.contains(*role))
            .cloned()
            .collect();
    }
    let projected_runtime_roles = domains
        .values()
        .flat_map(|domain| domain.runtime_bound_screen_roles.iter().cloned())
        .collect::<BTreeSet<_>>();
    ensure!(
        projected_runtime_roles == observed_screen_roles,
        "integrated consumer runtime roles do not match final-artifact evidence"
    );

    Ok(IntegratedInstallationEvidence {
        output_sha1,
        report_sha1,
        final_static_domain_ids,
        domains,
        fixed_menu_lifetime,
        choice_lifetime,
    })
}

fn apply_integrated_installation(
    current: &mut CurrentInstallation,
    evidence: IntegratedInstallationEvidence,
) -> Result<()> {
    // A cumulative artifact is neither static nor runtime evidence for a later
    // integrated artifact. Preserve its installed byte counts for undeclared
    // domains, but require an exact-final installation declaration before
    // calling any consumer complete again. Observing another domain on the
    // same screen role must not implicitly promote this one.
    for (id, installation) in &mut current.domains {
        if !evidence.final_static_domain_ids.contains(*id) {
            installation.consumer_complete_screen_roles.clear();
            installation.runtime_bound_screen_roles.clear();
        }
    }
    for (id, installation) in evidence.domains {
        let static_id = crate::translation_coverage::screen_targets::DOMAIN_SEEDS
            .iter()
            .find(|seed| seed.id == id)
            .map(|seed| seed.id)
            .with_context(|| format!("integrated installation uses unknown domain {id}"))?;
        current.domains.insert(static_id, installation);
    }
    current.build_output_sha1 = evidence.output_sha1;
    current.build_report_sha1 = evidence.report_sha1;
    current.fixed_menu_lifetime = Some(evidence.fixed_menu_lifetime);
    let choice_lifetime_installed =
        ["main_dialogue", "choice_labels"]
            .into_iter()
            .all(|domain_id| {
                current.domains.get(domain_id).is_some_and(|installation| {
                    installation
                        .consumer_complete_screen_roles
                        .iter()
                        .any(|role| role == STORAGE_FOLLOW_UP_CHOICE_SCREEN_ROLE)
                })
            });
    current.choice_lifetime = choice_lifetime_installed.then_some(evidence.choice_lifetime);
    Ok(())
}

fn bind_choice_lifetime(
    choice: &IntegratedChoiceResidency,
    canonical_targets: &BTreeMap<&'static str, BTreeSet<String>>,
    report_sha1: &str,
) -> Result<InstalledChoiceLifetime> {
    let resident_record_ids = unique_strings(
        choice.resident_record_ids.clone(),
        "integrated choice residency records",
    )?;
    let front_end_record_ids = unique_strings(
        choice.front_end_result_record_ids.clone(),
        "integrated front-end choice residency records",
    )?;
    ensure!(
        choice.composite_state == crate::choice_labels::CHOICE_LABEL_COMPOSITE_STATE
            && choice.direct_choice_composite_producer_count == 3
            && choice.storage_choice_record_id == STORAGE_CHOICE_DIALOGUE_RECORD_ID
            && !choice.continue_prompt_record_id.is_empty()
            && front_end_record_ids.len() == 4
            && resident_record_ids.len() == front_end_record_ids.len() + 2
            && resident_record_ids.contains(&choice.continue_prompt_record_id)
            && resident_record_ids.contains(STORAGE_CHOICE_DIALOGUE_RECORD_ID)
            && front_end_record_ids.is_subset(&resident_record_ids)
            && choice.resident_workset_count >= resident_record_ids.len()
            && choice.storage_follow_up_workset_count > 0
            && choice.storage_follow_up_workset_count <= choice.resident_workset_count
            && choice.choice_glyph_count > 0
            && choice.fixed_code_count == choice.choice_glyph_count
            && !choice.fixed_assignment_sha1.is_empty()
            && choice.every_choice_glyph_has_one_stable_code
            && choice.every_resident_page_contains_every_choice_glyph,
        "integrated shared choice residency is incomplete"
    );
    let storage_follow_up = installed_lifetime_demand(
        "storage follow-up choice",
        choice.storage_follow_up_target_glyph_count,
        choice.storage_follow_up_preserved_active_code_count,
        choice.storage_follow_up_total_slot_demand,
    )?;
    ensure!(
        storage_follow_up.total_slot_demand <= choice.maximum_augmented_workset_slot_demand,
        "storage follow-up choice exceeds the installed shared choice bound"
    );
    for domain_id in ["main_dialogue", "choice_labels"] {
        ensure!(
            canonical_targets
                .get(domain_id)
                .is_some_and(|roles| roles.contains(STORAGE_FOLLOW_UP_CHOICE_SCREEN_ROLE)),
            "storage follow-up choice lost its {domain_id} screen ownership"
        );
    }
    ensure!(
        !report_sha1.is_empty(),
        "storage follow-up choice has no exact-final report identity"
    );
    Ok(InstalledChoiceLifetime {
        screen_role: STORAGE_FOLLOW_UP_CHOICE_SCREEN_ROLE.to_owned(),
        storage_follow_up,
        evidence_report_sha1: report_sha1.to_owned(),
    })
}

fn bind_fixed_menu_lifetime(
    codebook: &IntegratedConsumerCodebook,
    unit_selection: &IntegratedUnitSelectionHelpResidency,
    storage: &IntegratedStorageDialogueResidency,
    canonical_targets: &BTreeMap<&'static str, BTreeSet<String>>,
    report_sha1: &str,
) -> Result<InstalledFixedMenuLifetime> {
    ensure!(
        codebook.schema == 1
            && codebook.static_page_count == codebook.pages.len()
            && codebook.page_bytes_planned,
        "integrated consumer codebook is incomplete"
    );
    let page_ids = codebook
        .pages
        .iter()
        .map(|page| page.id.as_str())
        .collect::<BTreeSet<_>>();
    ensure!(
        page_ids.len() == codebook.pages.len(),
        "integrated consumer codebook repeats a static page"
    );
    let shared_pages = codebook
        .pages
        .iter()
        .filter(|page| page.id == "unit_command_menu")
        .collect::<Vec<_>>();
    ensure!(
        shared_pages.len() == 1,
        "integrated consumer codebook must contain one unit-command/fixed-menu page"
    );
    let shared_page = shared_pages[0];
    let shared_roles = shared_page
        .screen_roles
        .iter()
        .map(String::as_str)
        .collect::<BTreeSet<_>>();
    let shared_domains = shared_page
        .domain_ids
        .iter()
        .map(String::as_str)
        .collect::<BTreeSet<_>>();
    ensure!(
        shared_page.variant == "commands_fixed_menus_and_options_parent"
            && shared_roles == BTreeSet::from(["options", "unit_command_menu"])
            && shared_domains.contains("fixed_menu_labels")
            && !shared_page.assignment_sha1.is_empty()
            && !shared_page.page_sha1.is_empty(),
        "integrated fixed-menu static page lost its installed parent lifetime"
    );
    let shared_static_page = installed_lifetime_demand(
        "fixed-menu shared static page",
        shared_page.target_glyph_count,
        shared_page.preserved_active_code_count,
        shared_page.slot_demand,
    )?;

    ensure!(
        unit_selection.resident_workset_count > 0
            && unit_selection.help_line_count == UNIT_SELECTION_HELP_LINE_SPECS.len()
            && unit_selection.help_glyph_count > 0
            && unit_selection.fixed_code_count == unit_selection.help_glyph_count
            && unit_selection.source_lifecycle_bound
            && unit_selection.codes_selected_by_the_global_consumer_codebook
            && unit_selection.every_help_glyph_keeps_one_code_through_the_dialogue_handoff,
        "integrated unit-selection help lifetime is incomplete"
    );
    let unit_selection_help_dialogue = installed_lifetime_demand(
        "unit-selection help dialogue handoff",
        unit_selection.maximum_augmented_workset_target_glyph_count,
        unit_selection.maximum_augmented_workset_preserved_active_code_count,
        unit_selection.maximum_augmented_workset_slot_demand,
    )?;

    ensure!(
        storage.source_dispatch_count > 0
            && storage.source_direct_record_store_count > 0
            && storage.fixed_glyph_count > 0
            && storage.fixed_code_count == storage.fixed_glyph_count
            && storage.every_storage_label_glyph_uses_its_installed_code
            && storage.every_overlay_dialogue_page_contains_its_visible_storage_label_glyphs
            && storage.storage_dialogue_does_not_reselect_the_static_menu_page
            && storage.capacity_notice_keeps_its_standalone_static_page
            && storage.item_list_dialogue_pages_use_canonical_item_codes,
        "integrated storage dialogue lifetime is incomplete"
    );
    let storage_dialogue = installed_lifetime_demand(
        "storage dialogue and fixed-label overlay",
        storage.maximum_augmented_workset_target_glyph_count,
        storage.maximum_augmented_workset_preserved_active_code_count,
        storage.maximum_augmented_workset_slot_demand,
    )?;

    let canonical_roles = canonical_targets
        .get("fixed_menu_labels")
        .context("fixed-menu labels have no canonical screen roles")?;
    let owner_roles = fixed_menu_screen_roles()
        .iter()
        .map(|role| (*role).to_owned())
        .collect::<BTreeSet<_>>();
    ensure!(
        canonical_roles == &owner_roles && !report_sha1.is_empty(),
        "fixed-menu lifetime roles or exact-final report identity changed"
    );

    Ok(InstalledFixedMenuLifetime {
        screen_roles: canonical_roles.iter().cloned().collect(),
        shared_static_page,
        unit_selection_help_dialogue,
        storage_dialogue,
        evidence_report_sha1: report_sha1.to_owned(),
    })
}

fn installed_lifetime_demand(
    role: &str,
    target_glyph_count: usize,
    preserved_active_code_count: usize,
    total_slot_demand: usize,
) -> Result<InstalledLifetimeDemand> {
    let component_total = target_glyph_count
        .checked_add(preserved_active_code_count)
        .with_context(|| format!("{role} slot demand overflow"))?;
    ensure!(
        target_glyph_count > 0
            && component_total == total_slot_demand
            && total_slot_demand <= ACTIVE_HANGUL_SLOT_COUNT,
        "{role} components or active-page fit changed"
    );
    Ok(InstalledLifetimeDemand {
        target_glyph_count,
        preserved_active_code_count,
        total_slot_demand,
    })
}

fn bind_carried_ui_domains(
    carried: CarriedUiDomainPreservation,
    canonical_targets: &BTreeMap<&'static str, BTreeSet<String>>,
) -> Result<BTreeMap<String, DomainInstallation>> {
    let expected_ids = CARRIED_UI_DOMAIN_IDS
        .into_iter()
        .map(str::to_owned)
        .collect::<BTreeSet<_>>();
    let expected_counts = BTreeMap::from([
        ("class_profiles", 22_usize),
        ("front_end_menu_labels", 7),
        ("options_labels", 3),
        ("roster_header", 1),
        ("title_graphics", 1),
    ]);
    ensure!(
        carried.domain_count == carried.domains.len() && carried.domain_count == expected_ids.len(),
        "carried UI domain count changed"
    );
    let mut domains = BTreeMap::new();
    for domain in carried.domains {
        ensure!(
            expected_ids.contains(&domain.id),
            "integrated report carries unknown UI domain {}",
            domain.id
        );
        let canonical_roles = canonical_targets.get(domain.id.as_str()).with_context(|| {
            format!(
                "carried UI domain {} has no canonical screen set",
                domain.id
            )
        })?;
        let roles = unique_strings(domain.screen_roles, "carried UI screen roles")?;
        let route_ids = unique_strings(
            domain.consumer_route_binding_ids,
            "carried UI consumer route binding IDs",
        )?;
        ensure!(
            roles == *canonical_roles
                && domain.target_unit_count == expected_counts[domain.id.as_str()]
                && domain.translation_input_bound
                && domain.stored_target_content_matches_translation_input
                && domain.final_visual_material_matches_target_content
                && domain.complete_for_declared_domain_plan
                && !route_ids.is_empty(),
            "carried UI domain {} does not close its exact final plan",
            domain.id
        );
        let storage_and_visual_binding_kinds = match domain.final_semantic_binding {
            FinalSemanticBinding::PreservedStorageAndVisualMaterial => {
                &["cumulative_bytes_preserved"][..]
            }
            FinalSemanticBinding::ReencodedStorageAndVisualMaterial => {
                &["integrated_route_replacement"][..]
            }
        };
        bind_carried_regions(
            &domain.storage_regions,
            "carried UI storage",
            storage_and_visual_binding_kinds,
        )?;
        bind_carried_regions(
            &domain.font_regions,
            "carried UI font supply",
            storage_and_visual_binding_kinds,
        )?;
        bind_carried_regions(
            &domain.consumer_regions,
            "carried UI consumer route",
            &["cumulative_bytes_preserved", "integrated_route_replacement"],
        )?;
        let installation = DomainInstallation {
            installed_target_unit_count: domain.target_unit_count,
            installed_screen_roles: roles.iter().cloned().collect(),
            consumer_complete_screen_roles: roles.into_iter().collect(),
            runtime_bound_screen_roles: Vec::new(),
        };
        ensure!(
            domains.insert(domain.id, installation).is_none(),
            "carried UI installation repeats a domain"
        );
    }
    ensure!(
        domains.keys().cloned().collect::<BTreeSet<_>>() == expected_ids,
        "carried UI domain registry is incomplete"
    );
    Ok(domains)
}

fn bind_carried_battle_domains(
    carried: CarriedBattleDomainPreservation,
    canonical_targets: &BTreeMap<&'static str, BTreeSet<String>>,
) -> Result<BTreeMap<String, DomainInstallation>> {
    let expected_ids = CARRIED_BATTLE_DOMAIN_IDS
        .into_iter()
        .map(str::to_owned)
        .collect::<BTreeSet<_>>();
    let expected_counts = BTreeMap::from([
        ("battle_dialogue", 70_usize),
        ("battle_forecast_label", 1),
        ("battle_message_templates", 22),
        ("terrain_names", 16),
    ]);
    let shared_roles = unique_strings(
        carried.shared_screen_roles,
        "carried battle shared screen roles",
    )?;
    ensure!(
        shared_roles == BTreeSet::from(["battle_animation".to_owned()]),
        "carried battle domains no longer share the battle-animation lifetime"
    );
    bind_carried_regions(
        &carried.shared_font_regions,
        "carried battle font supply",
        &["cumulative_bytes_preserved"],
    )?;
    bind_carried_regions(
        &carried.shared_consumer_regions,
        "carried battle consumer route",
        &["cumulative_bytes_preserved", "integrated_route_replacement"],
    )?;
    let route_ids = unique_strings(
        carried.shared_consumer_route_binding_ids,
        "carried battle consumer route binding IDs",
    )?;
    ensure!(
        carried.domain_count == carried.domains.len()
            && carried.domain_count == expected_ids.len()
            && !route_ids.is_empty(),
        "carried battle shared route inventory changed"
    );

    let mut domains = BTreeMap::new();
    for domain in carried.domains {
        ensure!(
            expected_ids.contains(&domain.id),
            "integrated report carries unknown battle domain {}",
            domain.id
        );
        let canonical_roles = canonical_targets.get(domain.id.as_str()).with_context(|| {
            format!(
                "carried battle domain {} has no canonical screen set",
                domain.id
            )
        })?;
        ensure!(
            *canonical_roles == shared_roles
                && domain.target_unit_count == expected_counts[domain.id.as_str()]
                && domain.translation_input_bound
                && domain.complete_for_declared_domain_plan,
            "carried battle domain {} does not close its exact final plan",
            domain.id
        );
        bind_carried_regions(
            &domain.storage_regions,
            "carried battle storage",
            &["cumulative_bytes_preserved"],
        )?;
        let installation = DomainInstallation {
            installed_target_unit_count: domain.target_unit_count,
            installed_screen_roles: shared_roles.iter().cloned().collect(),
            consumer_complete_screen_roles: shared_roles.iter().cloned().collect(),
            runtime_bound_screen_roles: Vec::new(),
        };
        ensure!(
            domains.insert(domain.id, installation).is_none(),
            "carried battle installation repeats a domain"
        );
    }
    ensure!(
        domains.keys().cloned().collect::<BTreeSet<_>>() == expected_ids,
        "carried battle domain registry is incomplete"
    );
    Ok(domains)
}

fn bind_carried_regions(
    regions: &[CarriedUiRegion],
    role: &str,
    allowed_binding_kinds: &[&str],
) -> Result<()> {
    ensure!(!regions.is_empty(), "{role} has no exact final regions");
    let mut identities = BTreeSet::new();
    for region in regions {
        let offset = region
            .file_offset_hex
            .strip_prefix("0x")
            .context("carried UI region offset is not hexadecimal")?;
        ensure!(
            !region.role.is_empty()
                && region.byte_count > 0
                && !offset.is_empty()
                && usize::from_str_radix(offset, 16).is_ok()
                && region.sha1.len() == 40
                && region.sha1.bytes().all(|byte| byte.is_ascii_hexdigit())
                && allowed_binding_kinds.contains(&region.binding_kind.as_str())
                && region.final_bytes_match_binding,
            "{role} region {} is not bound to the final artifact",
            region.role
        );
        ensure!(
            identities.insert((region.role.as_str(), region.file_offset_hex.as_str())),
            "{role} repeats an exact region identity"
        );
    }
    Ok(())
}

fn bind_runtime_evidence(
    evidence: &FinalArtifactRuntimeEvidence,
    output_sha1: &str,
) -> Result<BTreeSet<String>> {
    ensure!(
        evidence.artifact_sha1 == output_sha1,
        "final-artifact runtime evidence is bound to another ROM"
    );
    let roles = unique_strings(
        evidence.bound_screen_roles.clone(),
        "final-artifact runtime screen roles",
    )?;
    if evidence.provided {
        ensure!(
            !evidence.manifest_sha1.is_empty()
                && evidence.run_count > 0
                && (evidence.observation_count > 0 || evidence.route_count > 0)
                && (evidence.route_count == 0 || evidence.route_checkpoint_count > 0)
                && evidence.sample_count > 0
                && evidence.every_run_started_from_cold_boot
                && evidence.savestate_free
                && evidence.every_sample_image_digest_bound,
            "provided final-artifact runtime evidence is incomplete"
        );
    } else {
        ensure!(
            evidence.manifest_sha1.is_empty()
                && evidence.run_count == 0
                && evidence.route_count == 0
                && evidence.route_checkpoint_count == 0
                && evidence.observation_count == 0
                && evidence.sample_count == 0
                && roles.is_empty()
                && !evidence.every_run_started_from_cold_boot
                && !evidence.savestate_free
                && !evidence.every_sample_image_digest_bound,
            "unprovided final-artifact runtime evidence retains stale claims"
        );
    }
    Ok(roles)
}

fn installation_gates_complete(gates: &IntegratedInstallationGates) -> bool {
    gates.all_translation_inputs_loaded
        && gates.all_dialogue_records_encoded
        && gates.all_visible_dialogue_text_encoded
        && gates.all_dialogue_pointers_planned
        && gates.all_dialogue_page_code_assignments_found
        && gates.all_dialogue_page_worksets_packed
        && gates.all_resident_dialogue_transitions_use_one_codebook
        && gates.all_chapter_titles_encoded_with_resident_codes
        && gates.all_chapter_title_storage_writes_planned
        && gates.cold_request_presentation_page_planned
        && gates.cold_request_presentation_write_planned
        && gates.dialogue_runtime_composition_planned
        && gates.all_declared_consumer_writes_planned
        && gates.declared_plan_technical_installation_complete
        && gates.all_carried_ui_domains_reinspected
        && gates.all_carried_battle_domains_reinspected
}

fn unique_strings(values: Vec<String>, role: &str) -> Result<BTreeSet<String>> {
    let count = values.len();
    let values = values.into_iter().collect::<BTreeSet<_>>();
    ensure!(
        values.len() == count && values.iter().all(|value| !value.is_empty()),
        "{role} contain empty or duplicate values"
    );
    Ok(values)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn region(role: &str, offset: &str, binding_kind: &str) -> serde_json::Value {
        serde_json::json!({
            "role": role,
            "binding_kind": binding_kind,
            "file_offset_hex": offset,
            "byte_count": 1,
            "sha1": "0000000000000000000000000000000000000000",
            "final_bytes_match_binding": true
        })
    }

    fn carried_domain(
        id: &str,
        target_unit_count: usize,
        screen_roles: &[&str],
    ) -> serde_json::Value {
        serde_json::json!({
            "id": id,
            "target_unit_count": target_unit_count,
            "screen_roles": screen_roles,
            "translation_input_bound": true,
            "review_complete": false,
            "final_semantic_binding": "preserved_storage_and_visual_material",
            "stored_target_content_matches_translation_input": true,
            "final_visual_material_matches_target_content": true,
            "storage_regions": [region("storage", "0x001000", "cumulative_bytes_preserved")],
            "font_regions": [region("font", "0x002000", "cumulative_bytes_preserved")],
            "consumer_regions": [region("consumer", "0x003000", "integrated_route_replacement")],
            "consumer_route_binding_ids": [format!("{id}:route")],
            "complete_for_declared_domain_plan": true
        })
    }

    fn carried_battle_domain(id: &str, target_unit_count: usize) -> serde_json::Value {
        serde_json::json!({
            "id": id,
            "target_unit_count": target_unit_count,
            "translation_input_bound": true,
            "review_complete": false,
            "storage_regions": [region("battle-storage", "0x004000", "cumulative_bytes_preserved")],
            "complete_for_declared_domain_plan": true
        })
    }

    fn report_json(runtime_role: Option<&str>) -> Vec<u8> {
        let runtime_roles = runtime_role.into_iter().collect::<Vec<_>>();
        let carried_domains = vec![
            carried_domain("class_profiles", 22, &["class_profile"]),
            carried_domain(
                "front_end_menu_labels",
                7,
                &["new_game_choice", "save_slot_selection"],
            ),
            carried_domain("options_labels", 3, &["options"]),
            carried_domain("roster_header", 1, &["unit_roster"]),
            carried_domain("title_graphics", 1, &["title"]),
        ];
        let carried_battle_domains = vec![
            carried_battle_domain("battle_dialogue", 70),
            carried_battle_domain("battle_forecast_label", 1),
            carried_battle_domain("battle_message_templates", 22),
            carried_battle_domain("terrain_names", 16),
        ];
        serde_json::to_vec(&serde_json::json!({
            "schema": FULL_TRANSLATION_REPORT_SCHEMA,
            "source_sha1": EXPECTED_SOURCE_SHA1,
            "declared_installation_domain_count": 2,
            "declared_installation_domains": ["fixed_menu_labels", "map_menu_labels"],
            "consumer_installation": {
                "current_candidate_sha1": "base-output",
                "current_build_report_sha1": "base-report",
                "declared_domain_count": 2,
                "domains": [
                    {
                        "id": "map_menu_labels",
                        "target_unit_count": 8,
                        "globally_planned_target_unit_count": 8,
                        "declared_screen_roles": ["map_funds_summary", "map_menu"],
                        "statically_accounted_declared_screen_roles": ["map_funds_summary", "map_menu"],
                        "unaccounted_declared_screen_roles": [],
                        "runtime_observed_declared_screen_roles": runtime_roles,
                        "all_declared_consumers_statically_accounted": true
                    },
                    {
                        "id": "fixed_menu_labels",
                        "target_unit_count": 13,
                        "globally_planned_target_unit_count": 13,
                        "declared_screen_roles": [
                            "game_speed_selection",
                            "storage_action_menu",
                            "storage_capacity_notice",
                            "storage_overflow_action",
                            "unit_selection"
                        ],
                        "statically_accounted_declared_screen_roles": [
                            "game_speed_selection",
                            "storage_action_menu",
                            "storage_capacity_notice",
                            "storage_overflow_action",
                            "unit_selection"
                        ],
                        "unaccounted_declared_screen_roles": [],
                        "runtime_observed_declared_screen_roles": [],
                        "all_declared_consumers_statically_accounted": true
                    }
                ],
                "statically_accounted_declared_domain_count": 2,
                "declared_domain_with_unaccounted_consumers_count": 0,
                "all_declared_consumers_statically_accounted": true,
                "current_candidate_runtime_evidence_inherited": false
            },
            "carried_ui_domain_preservation": {
                "cumulative_candidate_sha1": "base-output",
                "cumulative_report_sha1": "base-report",
                "integrated_image_sha1": sha1_hex(b"final"),
                "domain_count": 5,
                "domains": carried_domains,
                "all_translation_inputs_rebound": true,
                "all_storage_regions_rebound": true,
                "all_font_regions_rebound": true,
                "all_consumer_routes_rebound": true,
                "all_final_semantic_bindings_verified": true,
                "human_review_complete": false,
                "complete": true
            },
            "carried_battle_domain_preservation": {
                "cumulative_candidate_sha1": "base-output",
                "cumulative_report_sha1": "base-report",
                "integrated_image_sha1": sha1_hex(b"final"),
                "domain_count": 4,
                "domains": carried_battle_domains,
                "shared_screen_roles": ["battle_animation"],
                "shared_font_regions": [region("battle-font", "0x005000", "cumulative_bytes_preserved")],
                "shared_consumer_regions": [region("battle-consumer", "0x006000", "integrated_route_replacement")],
                "shared_consumer_route_binding_ids": ["battle:route"],
                "all_translation_inputs_rebound": true,
                "all_storage_regions_rebound": true,
                "shared_font_supply_rebound": true,
                "shared_consumer_route_rebound": true,
                "human_review_complete": false,
                "complete": true
            },
            "integrated_write_set": {
                "declared_domain_count": 2,
                "domains": [
                    {
                        "id": "map_menu_labels",
                        "expected_write_count": 1,
                        "translation_input_loaded": true,
                        "glyph_lifetime_bound": true,
                        "storage_and_address_writes_contributed": true,
                        "runtime_material_writes_contributed": true,
                        "font_supply_writes_contributed": true,
                        "all_declared_consumer_writes_contributed": true,
                        "complete_for_declared_domain_plan": true
                    },
                    {
                        "id": "fixed_menu_labels",
                        "expected_write_count": 1,
                        "translation_input_loaded": true,
                        "glyph_lifetime_bound": true,
                        "storage_and_address_writes_contributed": true,
                        "runtime_material_writes_contributed": true,
                        "font_supply_writes_contributed": true,
                        "all_declared_consumer_writes_contributed": true,
                        "complete_for_declared_domain_plan": true
                    }
                ],
                "declared_domain_with_expected_writes_count": 2,
                "statically_accounted_declared_domain_count": 2,
                "original_candidate_sha1": "base-output",
                "planned_final_image_byte_count": 5,
                "integrated_image_sha1": sha1_hex(b"final"),
                "every_change_tracked": true,
                "required_mutation_identity_set_complete": true,
                "required_runtime_routine_identities_installed": true,
                "required_runtime_hook_identities_installed": true,
                "final_replacement_bytes_match_manifest": true,
                "technical_installation_complete": true,
                "one_shared_image": true,
                "all_declared_domains_contribute_expected_writes": true,
                "rom_emitted": true
            },
            "choice_residency": {
                "composite_state": 12,
                "continue_prompt_record_id": "victory-and-defeat-dialogue:000",
                "front_end_result_record_ids": [
                    "front-end-dialogue:000",
                    "front-end-dialogue:001",
                    "front-end-dialogue:002",
                    "front-end-dialogue:003"
                ],
                "storage_choice_record_id": "shop-and-item-dialogue:045",
                "direct_choice_composite_producer_count": 3,
                "resident_record_ids": [
                    "front-end-dialogue:000",
                    "front-end-dialogue:001",
                    "front-end-dialogue:002",
                    "front-end-dialogue:003",
                    "shop-and-item-dialogue:045",
                    "victory-and-defeat-dialogue:000"
                ],
                "resident_workset_count": 6,
                "choice_glyph_count": 4,
                "fixed_code_count": 4,
                "maximum_augmented_workset_slot_demand": 195,
                "storage_follow_up_workset_count": 1,
                "storage_follow_up_target_glyph_count": 143,
                "storage_follow_up_preserved_active_code_count": 52,
                "storage_follow_up_total_slot_demand": 195,
                "fixed_assignment_sha1": "choice-assignment",
                "every_choice_glyph_has_one_stable_code": true,
                "every_resident_page_contains_every_choice_glyph": true
            },
            "consumer_codebook": {
                "schema": 1,
                "static_page_count": 1,
                "pages": [{
                    "id": "unit_command_menu",
                    "variant": "commands_fixed_menus_and_options_parent",
                    "screen_roles": ["unit_command_menu", "options"],
                    "domain_ids": ["unit_ui_labels", "fixed_menu_labels", "options_menu"],
                    "target_glyph_count": 86,
                    "preserved_active_code_count": 0,
                    "slot_demand": 86,
                    "assignment_sha1": "assignment",
                    "page_sha1": "page"
                }],
                "page_bytes_planned": true
            },
            "unit_selection_help_residency": {
                "resident_workset_count": 1,
                "help_line_count": 6,
                "help_glyph_count": 40,
                "fixed_code_count": 40,
                "maximum_augmented_workset_target_glyph_count": 155,
                "maximum_augmented_workset_preserved_active_code_count": 40,
                "maximum_augmented_workset_slot_demand": 195,
                "source_lifecycle_bound": true,
                "codes_selected_by_the_global_consumer_codebook": true,
                "every_help_glyph_keeps_one_code_through_the_dialogue_handoff": true
            },
            "storage_dialogue_residency": {
                "source_dispatch_count": 2,
                "source_direct_record_store_count": 17,
                "fixed_glyph_count": 8,
                "fixed_code_count": 8,
                "maximum_augmented_workset_target_glyph_count": 143,
                "maximum_augmented_workset_preserved_active_code_count": 52,
                "maximum_augmented_workset_slot_demand": 195,
                "every_storage_label_glyph_uses_its_installed_code": true,
                "every_overlay_dialogue_page_contains_its_visible_storage_label_glyphs": true,
                "storage_dialogue_does_not_reselect_the_static_menu_page": true,
                "capacity_notice_keeps_its_standalone_static_page": true,
                "item_list_dialogue_pages_use_canonical_item_codes": true
            },
            "final_artifact_runtime_evidence": {
                "provided": runtime_role.is_some(),
                "manifest_sha1": runtime_role.map(|_| "runtime").unwrap_or(""),
                "artifact_sha1": sha1_hex(b"final"),
                "run_count": usize::from(runtime_role.is_some()),
                "route_count": 0,
                "route_checkpoint_count": 0,
                "observation_count": usize::from(runtime_role.is_some()),
                "sample_count": usize::from(runtime_role.is_some()),
                "bound_screen_roles": runtime_roles,
                "every_run_started_from_cold_boot": runtime_role.is_some(),
                "savestate_free": runtime_role.is_some(),
                "every_sample_image_digest_bound": runtime_role.is_some()
            },
            "installation_gates": {
                "all_translation_inputs_loaded": true,
                "all_dialogue_records_encoded": true,
                "all_visible_dialogue_text_encoded": true,
                "all_dialogue_pointers_planned": true,
                "all_dialogue_page_code_assignments_found": true,
                "all_dialogue_page_worksets_packed": true,
                "all_resident_dialogue_transitions_use_one_codebook": true,
                "all_chapter_titles_encoded_with_resident_codes": true,
                "all_chapter_title_storage_writes_planned": true,
                "cold_request_presentation_page_planned": true,
                "cold_request_presentation_write_planned": true,
                "dialogue_runtime_composition_planned": true,
                "all_declared_consumer_writes_planned": true,
                "declared_plan_technical_installation_complete": true,
                "all_carried_ui_domains_reinspected": true,
                "all_carried_battle_domains_reinspected": true
            },
            "rom_emitted": true
        }))
        .unwrap()
    }

    fn report_json_with_carried_runtime_roles(runtime_roles: &[&str]) -> Vec<u8> {
        let mut value: serde_json::Value = serde_json::from_slice(&report_json(None)).unwrap();
        let evidence = &mut value["final_artifact_runtime_evidence"];
        evidence["provided"] = true.into();
        evidence["manifest_sha1"] = "runtime".into();
        evidence["run_count"] = 1.into();
        evidence["observation_count"] = runtime_roles.len().into();
        evidence["sample_count"] = (runtime_roles.len() * 3).into();
        evidence["bound_screen_roles"] = serde_json::json!(runtime_roles);
        evidence["every_run_started_from_cold_boot"] = true.into();
        evidence["savestate_free"] = true.into();
        evidence["every_sample_image_digest_bound"] = true.into();
        serde_json::to_vec(&value).unwrap()
    }

    fn reencode_carried_storage_and_visual_material(
        value: &mut serde_json::Value,
        domain_id: &str,
    ) {
        let domain = value["carried_ui_domain_preservation"]["domains"]
            .as_array_mut()
            .unwrap()
            .iter_mut()
            .find(|domain| domain["id"] == domain_id)
            .unwrap();
        domain["final_semantic_binding"] = "reencoded_storage_and_visual_material".into();
        for region_family in ["storage_regions", "font_regions"] {
            for region in domain[region_family].as_array_mut().unwrap() {
                region["binding_kind"] = "integrated_route_replacement".into();
            }
        }
    }

    #[test]
    fn exact_final_report_replaces_declared_domain_installation() {
        let report = report_json(Some("map_menu"));
        let evidence =
            bind_integrated_installation(&report, b"final", "base-output", "base-report").unwrap();
        let domain = &evidence.domains["map_menu_labels"];
        assert_eq!(domain.installed_target_unit_count, 8);
        assert_eq!(
            domain.consumer_complete_screen_roles,
            ["map_funds_summary", "map_menu"]
        );
        assert_eq!(domain.runtime_bound_screen_roles, ["map_menu"]);
        assert_eq!(evidence.output_sha1, sha1_hex(b"final"));
        assert_eq!(
            evidence.fixed_menu_lifetime.screen_roles,
            [
                "game_speed_selection",
                "storage_action_menu",
                "storage_capacity_notice",
                "storage_overflow_action",
                "unit_selection",
            ]
        );
        assert_eq!(
            evidence
                .fixed_menu_lifetime
                .unit_selection_help_dialogue
                .total_slot_demand,
            195
        );
        assert_eq!(
            evidence
                .fixed_menu_lifetime
                .storage_dialogue
                .total_slot_demand,
            195
        );
        assert_eq!(
            evidence.choice_lifetime.screen_role,
            STORAGE_FOLLOW_UP_CHOICE_SCREEN_ROLE
        );
        assert_eq!(
            evidence.choice_lifetime.storage_follow_up.total_slot_demand,
            195
        );
    }

    #[test]
    fn fixed_menu_lifetime_component_or_role_drift_fails_closed() {
        let mut value: serde_json::Value = serde_json::from_slice(&report_json(None)).unwrap();
        value["unit_selection_help_residency"]["maximum_augmented_workset_target_glyph_count"] =
            154.into();
        let report = serde_json::to_vec(&value).unwrap();
        assert!(
            bind_integrated_installation(&report, b"final", "base-output", "base-report")
                .unwrap_err()
                .to_string()
                .contains("components")
        );

        let mut value: serde_json::Value = serde_json::from_slice(&report_json(None)).unwrap();
        value["consumer_installation"]["domains"][1]["statically_accounted_declared_screen_roles"] =
            serde_json::json!([
                "game_speed_selection",
                "storage_action_menu",
                "storage_overflow_action",
                "unit_selection"
            ]);
        let report = serde_json::to_vec(&value).unwrap();
        assert!(
            bind_integrated_installation(&report, b"final", "base-output", "base-report")
                .unwrap_err()
                .to_string()
                .contains("does not close its declared consumer plan")
        );
    }

    #[test]
    fn storage_follow_up_choice_lifetime_component_or_record_drift_fails_closed() {
        let mut value: serde_json::Value = serde_json::from_slice(&report_json(None)).unwrap();
        value["choice_residency"]["storage_follow_up_total_slot_demand"] = 194.into();
        let report = serde_json::to_vec(&value).unwrap();
        assert!(
            bind_integrated_installation(&report, b"final", "base-output", "base-report")
                .unwrap_err()
                .to_string()
                .contains("components")
        );

        let mut value: serde_json::Value = serde_json::from_slice(&report_json(None)).unwrap();
        value["choice_residency"]["storage_choice_record_id"] = "shop-and-item-dialogue:044".into();
        let report = serde_json::to_vec(&value).unwrap();
        assert!(
            bind_integrated_installation(&report, b"final", "base-output", "base-report")
                .unwrap_err()
                .to_string()
                .contains("incomplete")
        );
    }

    #[test]
    fn carried_static_routes_do_not_inherit_cumulative_runtime_claims() {
        let evidence = bind_integrated_installation(
            &report_json(None),
            b"final",
            "base-output",
            "base-report",
        )
        .unwrap();
        let options = &evidence.domains["options_labels"];
        assert_eq!(options.installed_target_unit_count, 3);
        assert_eq!(options.consumer_complete_screen_roles, ["options"]);
        assert!(
            options.runtime_bound_screen_roles.is_empty(),
            "carried cumulative runtime evidence must not be inherited"
        );
        let terrain = &evidence.domains["terrain_names"];
        assert_eq!(terrain.installed_target_unit_count, 16);
        assert_eq!(terrain.consumer_complete_screen_roles, ["battle_animation"]);
        assert!(terrain.runtime_bound_screen_roles.is_empty());
    }

    #[test]
    fn semantically_reencoded_carried_storage_and_visual_material_is_accepted() {
        let mut value: serde_json::Value = serde_json::from_slice(&report_json(None)).unwrap();
        reencode_carried_storage_and_visual_material(&mut value, "roster_header");
        let report = serde_json::to_vec(&value).unwrap();

        let evidence =
            bind_integrated_installation(&report, b"final", "base-output", "base-report").unwrap();

        assert_eq!(
            evidence.domains["roster_header"].consumer_complete_screen_roles,
            ["unit_roster"]
        );
    }

    #[test]
    fn carried_reencoding_without_final_semantic_proof_fails_closed() {
        let mut value: serde_json::Value = serde_json::from_slice(&report_json(None)).unwrap();
        reencode_carried_storage_and_visual_material(&mut value, "roster_header");
        let domain = value["carried_ui_domain_preservation"]["domains"]
            .as_array_mut()
            .unwrap()
            .iter_mut()
            .find(|domain| domain["id"] == "roster_header")
            .unwrap();
        domain["stored_target_content_matches_translation_input"] = false.into();
        let report = serde_json::to_vec(&value).unwrap();

        assert!(
            bind_integrated_installation(&report, b"final", "base-output", "base-report")
                .unwrap_err()
                .to_string()
                .contains("does not close its exact final plan")
        );
    }

    #[test]
    fn exact_final_evidence_projects_onto_carried_ui_domains() {
        let report = report_json_with_carried_runtime_roles(&[
            "class_profile",
            "new_game_choice",
            "save_slot_selection",
        ]);
        let evidence =
            bind_integrated_installation(&report, b"final", "base-output", "base-report").unwrap();

        assert_eq!(
            evidence.domains["class_profiles"].runtime_bound_screen_roles,
            ["class_profile"]
        );
        assert_eq!(
            evidence.domains["front_end_menu_labels"].runtime_bound_screen_roles,
            ["new_game_choice", "save_slot_selection"]
        );
        assert!(
            evidence.domains["options_labels"]
                .runtime_bound_screen_roles
                .is_empty()
        );
    }

    #[test]
    fn exact_final_battle_observation_projects_onto_every_visible_domain() {
        let report = report_json_with_carried_runtime_roles(&["battle_animation"]);
        let evidence =
            bind_integrated_installation(&report, b"final", "base-output", "base-report").unwrap();

        for domain_id in CARRIED_BATTLE_DOMAIN_IDS {
            assert_eq!(
                evidence.domains[domain_id].runtime_bound_screen_roles,
                ["battle_animation"]
            );
        }
    }

    #[test]
    fn final_evidence_role_outside_the_integrated_installation_fails_closed() {
        let report = report_json_with_carried_runtime_roles(&["missing_screen_role"]);
        assert!(
            bind_integrated_installation(&report, b"final", "base-output", "base-report")
                .unwrap_err()
                .to_string()
                .contains("outside the integrated installation")
        );
    }

    #[test]
    fn output_or_cumulative_identity_drift_fails_closed() {
        let report = report_json(None);
        assert!(
            bind_integrated_installation(&report, b"wrong", "base-output", "base-report")
                .unwrap_err()
                .to_string()
                .contains("output ROM identity")
        );
        assert!(
            bind_integrated_installation(&report, b"final", "wrong-base", "base-report")
                .unwrap_err()
                .to_string()
                .contains("does not descend")
        );
    }

    #[test]
    fn incomplete_domain_write_or_consumer_plan_fails_closed() {
        let mut value: serde_json::Value = serde_json::from_slice(&report_json(None)).unwrap();
        value["integrated_write_set"]["domains"][0]["font_supply_writes_contributed"] =
            false.into();
        let report = serde_json::to_vec(&value).unwrap();
        assert!(
            bind_integrated_installation(&report, b"final", "base-output", "base-report")
                .unwrap_err()
                .to_string()
                .contains("complete write set")
        );
    }

    #[test]
    fn repeated_write_domain_fails_closed() {
        let mut value: serde_json::Value = serde_json::from_slice(&report_json(None)).unwrap();
        let duplicate = value["integrated_write_set"]["domains"][0].clone();
        value["integrated_write_set"]["domains"]
            .as_array_mut()
            .unwrap()
            .push(duplicate);
        let report = serde_json::to_vec(&value).unwrap();
        assert!(
            bind_integrated_installation(&report, b"final", "base-output", "base-report")
                .unwrap_err()
                .to_string()
                .contains("repeats a domain")
        );
    }

    #[test]
    fn carried_region_or_registry_drift_fails_closed() {
        let mut value: serde_json::Value = serde_json::from_slice(&report_json(None)).unwrap();
        value["carried_ui_domain_preservation"]["domains"][0]["consumer_regions"][0]["final_bytes_match_binding"] =
            false.into();
        let report = serde_json::to_vec(&value).unwrap();
        assert!(
            bind_integrated_installation(&report, b"final", "base-output", "base-report")
                .unwrap_err()
                .to_string()
                .contains("not bound to the final artifact")
        );

        let mut value: serde_json::Value = serde_json::from_slice(&report_json(None)).unwrap();
        value["carried_ui_domain_preservation"]["domains"][0]["id"] =
            "unknown_carried_domain".into();
        let report = serde_json::to_vec(&value).unwrap();
        assert!(
            bind_integrated_installation(&report, b"final", "base-output", "base-report")
                .unwrap_err()
                .to_string()
                .contains("unknown UI domain")
        );
    }

    #[test]
    fn carried_identity_drift_fails_closed() {
        let mut value: serde_json::Value = serde_json::from_slice(&report_json(None)).unwrap();
        value["carried_ui_domain_preservation"]["cumulative_report_sha1"] = "other".into();
        let report = serde_json::to_vec(&value).unwrap();
        assert!(
            bind_integrated_installation(&report, b"final", "base-output", "base-report")
                .unwrap_err()
                .to_string()
                .contains("does not descend")
        );
    }
}
