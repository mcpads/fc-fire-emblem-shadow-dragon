use std::{
    collections::{BTreeMap, BTreeSet},
    fs,
    path::Path,
};

use anyhow::{Context, Result, ensure};
use serde::Deserialize;

use crate::{rom::EXPECTED_SOURCE_SHA1, sha1_hex};

use super::{
    report::DomainInstallation,
    weapon_shop::{
        CHOICE_LABEL_SCREEN_ROLES, DECLINE_ROUTE_CHOICE_LABEL_RUNTIME_SCREEN_ROLES,
        DECLINE_ROUTE_DIALOGUE_RUNTIME_SCREEN_ROLES, DECLINE_ROUTE_ITEM_NAME_RUNTIME_SCREEN_ROLES,
        DIALOGUE_SCREEN_ROLES, ITEM_NAME_SCREEN_ROLES, SCREEN_ROLES,
    },
};

#[derive(Debug, Deserialize)]
struct CurrentBuildReport {
    schema: u8,
    source_sha1: String,
    output_sha1: String,
    stages: Vec<CurrentBuildStage>,
    chapter_titles: CurrentChapterTitles,
    main_dialogue: CurrentMainDialogue,
    front_end_menu: CurrentFrontEndMenu,
    playable_unit_names: CurrentUnitNames,
    automatic_class_profiles: CurrentClassProfiles,
    weapon_shop_shared_text: CurrentWeaponShopSharedText,
    battle_text: CurrentBattleText,
}

#[derive(Debug, Deserialize)]
struct CurrentBuildStage {
    role: String,
    #[serde(default)]
    output_sha1: Option<String>,
}

#[derive(Debug, Deserialize)]
struct CurrentChapterTitles {
    installed_entry_count: usize,
    intro_title_table_installed: bool,
    ending_scroll_duplicate_installed: bool,
}

#[derive(Debug, Deserialize)]
struct CurrentMainDialogue {
    installed_translated_line_count: usize,
    lifetimes: Vec<CurrentDialogueLifetime>,
    maximum_page_reloaded_lifetime: CurrentMaximumDialogueLifetime,
}

#[derive(Debug, Deserialize)]
struct CurrentDialogueLifetime {
    screen_role: String,
    unique_glyph_count: usize,
    preserved_active_code_count: usize,
    font_physical_page: u8,
    font_mapper_register: u8,
    runtime_bound_to_dialogue_stage_output: bool,
}

#[derive(Debug, Deserialize)]
struct CurrentMaximumDialogueLifetime {
    screen_role: String,
    installed_translated_line_count: usize,
    completed_page_count: usize,
    runtime_bound_to_build: bool,
}

#[derive(Debug, Deserialize)]
struct CurrentFrontEndMenu {
    installed_entry_count: usize,
    runtime_variants_bound_to_build: bool,
}

#[derive(Debug, Deserialize)]
struct CurrentUnitNames {
    workspace_entry_count: usize,
    roster_projection_installed: bool,
    unit_summary_projection_installed: bool,
    source_battle_table_preserved: bool,
    source_ending_table_preserved: bool,
    runtime_bound_to_build: bool,
}

#[derive(Debug, Deserialize)]
struct CurrentClassProfiles {
    workspace_entry_count: usize,
    installed_entry_count: usize,
    page_unique_glyph_counts: Vec<usize>,
    preserved_active_code_count: usize,
    runtime_bound_to_build: bool,
}

#[derive(Debug, Deserialize)]
struct CurrentWeaponShopSharedText {
    screen_role: String,
    installed_item_name_count: usize,
    installed_choice_label_count: usize,
    shared_page_unique_glyph_count: usize,
    shared_page_preserved_active_code_count: usize,
    shared_page_total_slot_demand: usize,
    added_glyph_count: usize,
    font_physical_page: u8,
    font_mapper_register: u8,
    item_list_pointer_selector_installed: bool,
    selected_item_pointer_selector_installed: bool,
    choice_pointer_selector_installed: bool,
    unconverted_consumers_fallback_to_source_tables: bool,
    capacity_bound_screen_roles: Vec<String>,
    runtime_evidence_output_sha1: String,
    runtime_bound_dialogue_screen_roles: Vec<String>,
    runtime_bound_item_name_screen_roles: Vec<String>,
    runtime_bound_choice_label_screen_roles: Vec<String>,
    runtime_bound_to_stage_output: bool,
    runtime_carried_forward_by_verified_writes: bool,
}

#[derive(Debug, Deserialize)]
struct CurrentBattleText {
    fixed_text_workspace_sha1: String,
    dialogue_workspace_sha1: String,
    temporal_manifest_sha1: String,
    installed_fixed_entry_count: usize,
    installed_unit_name_count: usize,
    installed_enemy_name_count: usize,
    installed_class_name_count: usize,
    installed_item_name_count: usize,
    installed_terrain_name_count: usize,
    installed_battle_message_template_count: usize,
    installed_battle_forecast_label_count: usize,
    installed_translated_line_count: usize,
    weapon_shop_item_names_subset_of_battle_catalog: bool,
    cumulative_selector_ranges_preserved: bool,
    original_english_digits_and_graphics_preserved: bool,
    runtime_bound_to_build: bool,
}

pub(crate) struct CurrentInstallation {
    pub(crate) build_output_sha1: String,
    pub(crate) build_report_sha1: String,
    pub(crate) domains: BTreeMap<&'static str, DomainInstallation>,
    pub(crate) class_profile_page_target_glyph_counts: Vec<usize>,
    pub(crate) class_profile_preserved_active_code_count: usize,
    pub(crate) class_profile_runtime_bound_to_build: bool,
    pub(crate) weapon_shop_shared_page_target_glyph_count: usize,
    pub(crate) weapon_shop_shared_page_preserved_active_code_count: usize,
    pub(crate) weapon_shop_shared_page_total_slot_demand: usize,
    pub(crate) weapon_shop_capacity_bound_screen_roles: Vec<String>,
    pub(crate) battle_fixed_workspace_sha1: String,
    pub(crate) battle_dialogue_workspace_sha1: String,
    pub(crate) battle_temporal_manifest_sha1: String,
}

pub(crate) fn inspect_current_installation(
    build_report_path: &Path,
    build_output_path: &Path,
) -> Result<CurrentInstallation> {
    let report_bytes = fs::read(build_report_path)
        .with_context(|| format!("read current build report {}", build_report_path.display()))?;
    let report: CurrentBuildReport = serde_json::from_slice(&report_bytes)
        .with_context(|| format!("parse current build report {}", build_report_path.display()))?;
    ensure!(
        report.schema == 1 && report.source_sha1 == EXPECTED_SOURCE_SHA1,
        "current build report is not bound to the supported source"
    );
    let output_bytes = fs::read(build_output_path)
        .with_context(|| format!("read current build output {}", build_output_path.display()))?;
    let output_sha1 = sha1_hex(&output_bytes);
    ensure!(
        output_sha1 == report.output_sha1,
        "current build report and output ROM hashes differ"
    );
    ensure!(
        report.automatic_class_profiles.workspace_entry_count == 22
            && report.automatic_class_profiles.installed_entry_count == 22
            && report
                .automatic_class_profiles
                .page_unique_glyph_counts
                .len()
                == 2,
        "current class-profile installation no longer covers two complete profile groups"
    );
    validate_weapon_shop_lifetime(&report)?;
    let domains = collect_domain_installations(&report)?;

    Ok(CurrentInstallation {
        build_output_sha1: output_sha1,
        build_report_sha1: sha1_hex(&report_bytes),
        domains,
        class_profile_page_target_glyph_counts: report
            .automatic_class_profiles
            .page_unique_glyph_counts,
        class_profile_preserved_active_code_count: report
            .automatic_class_profiles
            .preserved_active_code_count,
        class_profile_runtime_bound_to_build: report
            .automatic_class_profiles
            .runtime_bound_to_build,
        weapon_shop_shared_page_target_glyph_count: report
            .weapon_shop_shared_text
            .shared_page_unique_glyph_count,
        weapon_shop_shared_page_preserved_active_code_count: report
            .weapon_shop_shared_text
            .shared_page_preserved_active_code_count,
        weapon_shop_shared_page_total_slot_demand: report
            .weapon_shop_shared_text
            .shared_page_total_slot_demand,
        weapon_shop_capacity_bound_screen_roles: report
            .weapon_shop_shared_text
            .capacity_bound_screen_roles,
        battle_fixed_workspace_sha1: report.battle_text.fixed_text_workspace_sha1,
        battle_dialogue_workspace_sha1: report.battle_text.dialogue_workspace_sha1,
        battle_temporal_manifest_sha1: report.battle_text.temporal_manifest_sha1,
    })
}

fn validate_weapon_shop_lifetime(report: &CurrentBuildReport) -> Result<()> {
    let shop_lifetimes = report
        .main_dialogue
        .lifetimes
        .iter()
        .filter(|lifetime| lifetime.screen_role == "weapon_shop_dialogue_lifetime")
        .collect::<Vec<_>>();
    ensure!(
        shop_lifetimes.len() == 1,
        "current build must contain one weapon-shop dialogue lifetime"
    );
    let dialogue = shop_lifetimes[0];
    let shared = &report.weapon_shop_shared_text;
    ensure!(
        shared.screen_role == "weapon_shop_shared_text"
            && shared.shared_page_unique_glyph_count
                == dialogue.unique_glyph_count + shared.added_glyph_count
            && shared.shared_page_preserved_active_code_count
                == dialogue.preserved_active_code_count
            && shared.shared_page_total_slot_demand
                == shared.shared_page_unique_glyph_count
                    + shared.shared_page_preserved_active_code_count
            && shared.font_physical_page == dialogue.font_physical_page
            && shared.font_mapper_register == dialogue.font_mapper_register
            && shared.item_list_pointer_selector_installed
            && shared.selected_item_pointer_selector_installed
            && shared.choice_pointer_selector_installed
            && shared.unconverted_consumers_fallback_to_source_tables,
        "current weapon-shop shared-page installation changed"
    );
    let capacity_roles = shared
        .capacity_bound_screen_roles
        .iter()
        .map(String::as_str)
        .collect::<BTreeSet<_>>();
    ensure!(
        capacity_roles == SCREEN_ROLES.into_iter().collect::<BTreeSet<_>>(),
        "current weapon-shop capacity contract does not cover all nine screen roles"
    );
    let dialogue_runtime_roles = shared
        .runtime_bound_dialogue_screen_roles
        .iter()
        .map(String::as_str)
        .collect::<BTreeSet<_>>();
    let item_name_runtime_roles = shared
        .runtime_bound_item_name_screen_roles
        .iter()
        .map(String::as_str)
        .collect::<BTreeSet<_>>();
    let choice_label_runtime_roles = shared
        .runtime_bound_choice_label_screen_roles
        .iter()
        .map(String::as_str)
        .collect::<BTreeSet<_>>();
    ensure!(
        shared.runtime_evidence_output_sha1
            == report
                .stages
                .iter()
                .find(|stage| stage.role == "weapon_shop_shared_item_names_and_choice_labels")
                .and_then(|stage| stage.output_sha1.as_deref())
                .context("current build lost the weapon-shop shared-text stage")?
            && shared.runtime_bound_to_stage_output
            && shared.runtime_carried_forward_by_verified_writes
            && dialogue.runtime_bound_to_dialogue_stage_output
            && dialogue_runtime_roles
                == DECLINE_ROUTE_DIALOGUE_RUNTIME_SCREEN_ROLES
                    .into_iter()
                    .collect::<BTreeSet<_>>()
            && item_name_runtime_roles
                == DECLINE_ROUTE_ITEM_NAME_RUNTIME_SCREEN_ROLES
                    .into_iter()
                    .collect::<BTreeSet<_>>()
            && choice_label_runtime_roles
                == DECLINE_ROUTE_CHOICE_LABEL_RUNTIME_SCREEN_ROLES
                    .into_iter()
                    .collect::<BTreeSet<_>>()
            && dialogue_runtime_roles.is_subset(&capacity_roles)
            && item_name_runtime_roles.is_subset(&capacity_roles)
            && choice_label_runtime_roles.is_subset(&capacity_roles),
        "current weapon-shop runtime evidence scope changed"
    );
    Ok(())
}

fn collect_domain_installations(
    report: &CurrentBuildReport,
) -> Result<BTreeMap<&'static str, DomainInstallation>> {
    let mut domains = BTreeMap::new();
    let shop_dialogue_runtime_roles = report
        .weapon_shop_shared_text
        .runtime_bound_dialogue_screen_roles
        .iter()
        .map(String::as_str)
        .collect::<BTreeSet<_>>();
    let shop_item_name_runtime_roles = report
        .weapon_shop_shared_text
        .runtime_bound_item_name_screen_roles
        .iter()
        .map(String::as_str)
        .collect::<BTreeSet<_>>();
    let shop_choice_label_runtime_roles = report
        .weapon_shop_shared_text
        .runtime_bound_choice_label_screen_roles
        .iter()
        .map(String::as_str)
        .collect::<BTreeSet<_>>();
    let ui_stage_installed = report
        .stages
        .iter()
        .any(|stage| stage.role == "mapper165_options_and_roster");
    if ui_stage_installed {
        put(
            &mut domains,
            "options_labels",
            installation(3, &["options"], &[]),
        )?;
        put(
            &mut domains,
            "roster_header",
            installation(1, &["unit_roster"], &[]),
        )?;
    }

    let front_end_runtime = report
        .front_end_menu
        .runtime_variants_bound_to_build
        .then_some("new_game_choice")
        .into_iter()
        .collect::<Vec<_>>();
    put(
        &mut domains,
        "front_end_menu_labels",
        installation(
            report.front_end_menu.installed_entry_count,
            &["new_game_choice"],
            &front_end_runtime,
        ),
    )?;

    let mut unit_name_screens = Vec::new();
    if report.playable_unit_names.roster_projection_installed {
        unit_name_screens.push("unit_roster");
    }
    if report.playable_unit_names.unit_summary_projection_installed {
        unit_name_screens.extend(["unit_summary", "unit_status"]);
    }
    if !report.playable_unit_names.source_battle_table_preserved {
        unit_name_screens.push("battle_animation");
    }
    ensure!(
        report.playable_unit_names.source_ending_table_preserved,
        "current build no longer declares the untranslated ending unit-name consumer"
    );
    ensure!(
        report.battle_text.installed_unit_name_count
            == report.playable_unit_names.workspace_entry_count
            && !report.playable_unit_names.source_battle_table_preserved,
        "current battle and shared unit-name installations disagree"
    );
    let unit_name_runtime = if report.playable_unit_names.runtime_bound_to_build {
        unit_name_screens.clone()
    } else {
        Vec::new()
    };
    put(
        &mut domains,
        "unit_names",
        installation(
            report.playable_unit_names.workspace_entry_count,
            &unit_name_screens,
            &unit_name_runtime,
        ),
    )?;

    let profile_runtime = report
        .automatic_class_profiles
        .runtime_bound_to_build
        .then_some("class_profile")
        .into_iter()
        .collect::<Vec<_>>();
    put(
        &mut domains,
        "class_profiles",
        installation(
            report.automatic_class_profiles.installed_entry_count,
            &["class_profile"],
            &profile_runtime,
        ),
    )?;

    let title_screens = report
        .chapter_titles
        .intro_title_table_installed
        .then_some("chapter_intro_title_dialogue_composite")
        .into_iter()
        .chain(
            report
                .chapter_titles
                .ending_scroll_duplicate_installed
                .then_some("ending_chapter_record_scroll"),
        )
        .collect::<Vec<_>>();
    put(
        &mut domains,
        "chapter_titles",
        installation(
            report.chapter_titles.installed_entry_count,
            &title_screens,
            &[],
        ),
    )?;

    let mut main_dialogue_screens = Vec::new();
    let mut main_dialogue_runtime = Vec::new();
    for lifetime in &report.main_dialogue.lifetimes {
        let roles: &[&str] = match lifetime.screen_role.as_str() {
            "chapter_1_intro_dialogue" => {
                &["intro_dialogue", "chapter_intro_title_dialogue_composite"]
            }
            "chapter_2_intro_dialogue" => &[
                "later_intro_dialogue",
                "chapter_intro_title_dialogue_composite",
            ],
            "weapon_shop_dialogue_lifetime" => DIALOGUE_SCREEN_ROLES.as_slice(),
            other => {
                return Err(anyhow::anyhow!(
                    "unknown installed dialogue lifetime {other}"
                ));
            }
        };
        main_dialogue_screens.extend_from_slice(roles);
        if lifetime.screen_role == "weapon_shop_dialogue_lifetime" {
            main_dialogue_runtime.extend(
                roles
                    .iter()
                    .copied()
                    .filter(|role| shop_dialogue_runtime_roles.contains(role)),
            );
        } else if lifetime.runtime_bound_to_dialogue_stage_output {
            main_dialogue_runtime.extend_from_slice(roles);
        }
    }
    let maximum = &report.main_dialogue.maximum_page_reloaded_lifetime;
    ensure!(
        maximum.screen_role == "chapter_7_castle_clear_maximum_dialogue"
            && maximum.installed_translated_line_count > 0
            && maximum.completed_page_count == 15,
        "current maximum-dialogue installation changed"
    );
    main_dialogue_screens.push("chapter_clear_epilogue_dialogue");
    if maximum.runtime_bound_to_build {
        main_dialogue_runtime.push("chapter_clear_epilogue_dialogue");
    }
    put(
        &mut domains,
        "main_dialogue",
        installation(
            report.main_dialogue.installed_translated_line_count,
            &main_dialogue_screens,
            &main_dialogue_runtime,
        ),
    )?;

    ensure!(
        report.battle_text.cumulative_selector_ranges_preserved
            && report
                .battle_text
                .original_english_digits_and_graphics_preserved,
        "current battle installation does not preserve its cumulative selectors and protected source text"
    );
    ensure!(
        report.battle_text.installed_fixed_entry_count
            == report.battle_text.installed_unit_name_count
                + report.battle_text.installed_enemy_name_count
                + report.battle_text.installed_class_name_count
                + report.battle_text.installed_item_name_count
                + report.battle_text.installed_terrain_name_count
                + report.battle_text.installed_battle_message_template_count,
        "current battle fixed-text domain counts do not cover the installed total"
    );
    let battle_runtime = report
        .battle_text
        .runtime_bound_to_build
        .then_some("battle_animation")
        .into_iter()
        .collect::<Vec<_>>();
    for (domain_id, installed_count) in [
        (
            "battle_dialogue",
            report.battle_text.installed_translated_line_count,
        ),
        (
            "battle_forecast_label",
            report.battle_text.installed_battle_forecast_label_count,
        ),
        (
            "battle_message_templates",
            report.battle_text.installed_battle_message_template_count,
        ),
        ("class_names", report.battle_text.installed_class_name_count),
        ("enemy_names", report.battle_text.installed_enemy_name_count),
        (
            "terrain_names",
            report.battle_text.installed_terrain_name_count,
        ),
    ] {
        put(
            &mut domains,
            domain_id,
            installation(installed_count, &["battle_animation"], &battle_runtime),
        )?;
    }

    ensure!(
        report
            .battle_text
            .weapon_shop_item_names_subset_of_battle_catalog
            && report.battle_text.installed_item_name_count
                >= report.weapon_shop_shared_text.installed_item_name_count,
        "current weapon-shop item names are not covered by the installed battle item catalog"
    );
    let mut item_name_screens = ITEM_NAME_SCREEN_ROLES.to_vec();
    item_name_screens.push("battle_animation");
    let mut item_name_runtime = ITEM_NAME_SCREEN_ROLES
        .into_iter()
        .filter(|role| shop_item_name_runtime_roles.contains(role))
        .collect::<Vec<_>>();
    if report.battle_text.runtime_bound_to_build {
        item_name_runtime.push("battle_animation");
    }
    put(
        &mut domains,
        "item_names",
        installation(
            report.battle_text.installed_item_name_count,
            &item_name_screens,
            &item_name_runtime,
        ),
    )?;
    let choice_runtime = CHOICE_LABEL_SCREEN_ROLES
        .into_iter()
        .filter(|role| shop_choice_label_runtime_roles.contains(role))
        .collect::<Vec<_>>();
    put(
        &mut domains,
        "choice_labels",
        installation(
            report.weapon_shop_shared_text.installed_choice_label_count,
            &CHOICE_LABEL_SCREEN_ROLES,
            &choice_runtime,
        ),
    )?;

    Ok(domains)
}

fn installation(
    installed_target_unit_count: usize,
    installed_screen_roles: &[&str],
    runtime_bound_screen_roles: &[&str],
) -> DomainInstallation {
    let mut installed_screen_roles = installed_screen_roles
        .iter()
        .map(|role| (*role).to_owned())
        .collect::<Vec<_>>();
    installed_screen_roles.sort();
    installed_screen_roles.dedup();
    let mut runtime_bound_screen_roles = runtime_bound_screen_roles
        .iter()
        .map(|role| (*role).to_owned())
        .collect::<Vec<_>>();
    runtime_bound_screen_roles.sort();
    runtime_bound_screen_roles.dedup();
    DomainInstallation {
        installed_target_unit_count,
        installed_screen_roles,
        runtime_bound_screen_roles,
    }
}

fn put(
    domains: &mut BTreeMap<&'static str, DomainInstallation>,
    domain_id: &'static str,
    installation: DomainInstallation,
) -> Result<()> {
    ensure!(
        domains.insert(domain_id, installation).is_none(),
        "current installation repeats domain {domain_id}"
    );
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cumulative_battle_stage_projects_each_domain_without_claiming_ending_or_runtime() {
        let report: CurrentBuildReport = serde_json::from_value(serde_json::json!({
            "schema": 1,
            "source_sha1": EXPECTED_SOURCE_SHA1,
            "output_sha1": "output-sha1",
            "stages": [
                {"role": "mapper165_options_and_roster"},
                {
                    "role": "weapon_shop_shared_item_names_and_choice_labels",
                    "output_sha1": "shop-stage-output"
                }
            ],
            "chapter_titles": {
                "installed_entry_count": 0,
                "intro_title_table_installed": false,
                "ending_scroll_duplicate_installed": false
            },
            "main_dialogue": {
                "installed_translated_line_count": 69,
                "lifetimes": [{
                    "screen_role": "weapon_shop_dialogue_lifetime",
                    "unique_glyph_count": 48,
                    "preserved_active_code_count": 90,
                    "font_physical_page": 48,
                    "font_mapper_register": 192,
                    "runtime_bound_to_dialogue_stage_output": true
                }],
                "maximum_page_reloaded_lifetime": {
                    "screen_role": "chapter_7_castle_clear_maximum_dialogue",
                    "installed_translated_line_count": 57,
                    "completed_page_count": 15,
                    "runtime_bound_to_build": true
                }
            },
            "front_end_menu": {
                "installed_entry_count": 0,
                "runtime_variants_bound_to_build": false
            },
            "playable_unit_names": {
                "workspace_entry_count": 52,
                "roster_projection_installed": true,
                "unit_summary_projection_installed": true,
                "source_battle_table_preserved": false,
                "source_ending_table_preserved": true,
                "runtime_bound_to_build": false
            },
            "automatic_class_profiles": {
                "workspace_entry_count": 0,
                "installed_entry_count": 0,
                "page_unique_glyph_counts": [],
                "preserved_active_code_count": 0,
                "runtime_bound_to_build": false
            },
            "weapon_shop_shared_text": {
                "screen_role": "weapon_shop_shared_text",
                "installed_item_name_count": 6,
                "installed_choice_label_count": 2,
                "shared_page_unique_glyph_count": 60,
                "shared_page_preserved_active_code_count": 90,
                "shared_page_total_slot_demand": 150,
                "added_glyph_count": 12,
                "font_physical_page": 48,
                "font_mapper_register": 192,
                "item_list_pointer_selector_installed": true,
                "selected_item_pointer_selector_installed": true,
                "choice_pointer_selector_installed": true,
                "unconverted_consumers_fallback_to_source_tables": true,
                "capacity_bound_screen_roles": [
                    "weapon_shop_item_list",
                    "weapon_shop_purchase_confirmation",
                    "weapon_shop_purchase_result",
                    "weapon_shop_exit_message",
                    "weapon_shop_inventory_full_message",
                    "weapon_shop_insufficient_funds_message",
                    "weapon_shop_item_restriction_confirmation",
                    "weapon_shop_declined_continue_prompt",
                    "weapon_shop_purchase_inventory_full_exit"
                ],
                "runtime_bound_dialogue_screen_roles": [
                    "weapon_shop_item_list",
                    "weapon_shop_purchase_confirmation",
                    "weapon_shop_declined_continue_prompt",
                    "weapon_shop_exit_message"
                ],
                "runtime_bound_item_name_screen_roles": [
                    "weapon_shop_item_list",
                    "weapon_shop_purchase_confirmation",
                    "weapon_shop_declined_continue_prompt",
                    "weapon_shop_exit_message"
                ],
                "runtime_bound_choice_label_screen_roles": [
                    "weapon_shop_purchase_confirmation"
                ],
                "runtime_evidence_output_sha1": "shop-stage-output",
                "runtime_bound_to_stage_output": true,
                "runtime_carried_forward_by_verified_writes": true
            },
            "battle_text": {
                "fixed_text_workspace_sha1": "fixed-workspace",
                "dialogue_workspace_sha1": "dialogue-workspace",
                "temporal_manifest_sha1": "temporal-manifest",
                "installed_fixed_entry_count": 231,
                "installed_unit_name_count": 52,
                "installed_enemy_name_count": 55,
                "installed_class_name_count": 22,
                "installed_item_name_count": 64,
                "installed_terrain_name_count": 16,
                "installed_battle_message_template_count": 22,
                "installed_battle_forecast_label_count": 1,
                "installed_translated_line_count": 70,
                "weapon_shop_item_names_subset_of_battle_catalog": true,
                "cumulative_selector_ranges_preserved": true,
                "original_english_digits_and_graphics_preserved": true,
                "runtime_bound_to_build": false
            }
        }))
        .unwrap();

        validate_weapon_shop_lifetime(&report).unwrap();
        let installations = collect_domain_installations(&report).unwrap();
        let unit_names = &installations["unit_names"];
        assert_eq!(unit_names.installed_target_unit_count, 52);
        assert_eq!(
            unit_names.installed_screen_roles,
            [
                "battle_animation",
                "unit_roster",
                "unit_status",
                "unit_summary"
            ]
        );
        assert!(unit_names.runtime_bound_screen_roles.is_empty());

        let item_names = &installations["item_names"];
        assert_eq!(item_names.installed_target_unit_count, 64);
        assert_eq!(
            item_names.installed_screen_roles,
            [
                "battle_animation",
                "weapon_shop_declined_continue_prompt",
                "weapon_shop_exit_message",
                "weapon_shop_inventory_full_message",
                "weapon_shop_item_list",
                "weapon_shop_item_restriction_confirmation",
                "weapon_shop_purchase_confirmation",
                "weapon_shop_purchase_inventory_full_exit",
                "weapon_shop_purchase_result"
            ]
        );
        assert_eq!(
            item_names.runtime_bound_screen_roles,
            [
                "weapon_shop_declined_continue_prompt",
                "weapon_shop_exit_message",
                "weapon_shop_item_list",
                "weapon_shop_purchase_confirmation"
            ]
        );
        assert_eq!(
            installations["choice_labels"].installed_screen_roles,
            [
                "weapon_shop_declined_continue_prompt",
                "weapon_shop_insufficient_funds_message",
                "weapon_shop_item_restriction_confirmation",
                "weapon_shop_purchase_confirmation",
                "weapon_shop_purchase_result"
            ]
        );
        assert_eq!(
            installations["choice_labels"].runtime_bound_screen_roles,
            ["weapon_shop_purchase_confirmation"]
        );
        assert_eq!(
            installations["battle_dialogue"].installed_target_unit_count,
            70
        );
        assert_eq!(installations["enemy_names"].installed_target_unit_count, 55);
        assert!(
            installations["battle_dialogue"]
                .runtime_bound_screen_roles
                .is_empty()
        );
        assert_eq!(
            installations["main_dialogue"].installed_screen_roles,
            [
                "chapter_clear_epilogue_dialogue",
                "weapon_shop_declined_continue_prompt",
                "weapon_shop_exit_message",
                "weapon_shop_insufficient_funds_message",
                "weapon_shop_inventory_full_message",
                "weapon_shop_item_list",
                "weapon_shop_item_restriction_confirmation",
                "weapon_shop_purchase_confirmation",
                "weapon_shop_purchase_inventory_full_exit",
                "weapon_shop_purchase_result"
            ]
        );
        assert_eq!(
            installations["main_dialogue"].installed_target_unit_count,
            69
        );
        assert_eq!(
            installations["main_dialogue"].runtime_bound_screen_roles,
            [
                "chapter_clear_epilogue_dialogue",
                "weapon_shop_declined_continue_prompt",
                "weapon_shop_exit_message",
                "weapon_shop_item_list",
                "weapon_shop_purchase_confirmation"
            ]
        );
    }
}
