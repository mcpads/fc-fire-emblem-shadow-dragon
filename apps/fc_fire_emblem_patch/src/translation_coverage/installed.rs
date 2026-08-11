use std::{collections::BTreeMap, fs, path::Path};

use anyhow::{Context, Result, ensure};
use serde::Deserialize;

use crate::{rom::EXPECTED_SOURCE_SHA1, sha1_hex};

use super::report::DomainInstallation;

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
}

#[derive(Debug, Deserialize)]
struct CurrentBuildStage {
    role: String,
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
}

#[derive(Debug, Deserialize)]
struct CurrentDialogueLifetime {
    screen_role: String,
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
    source_battle_and_ending_table_preserved: bool,
    runtime_bound_to_build: bool,
}

#[derive(Debug, Deserialize)]
struct CurrentClassProfiles {
    installed_entry_count: usize,
    runtime_bound_to_build: bool,
}

#[derive(Debug, Deserialize)]
struct CurrentWeaponShopSharedText {
    installed_item_name_count: usize,
    installed_choice_label_count: usize,
    runtime_bound_to_build: bool,
}

pub(crate) struct CurrentInstallation {
    pub(crate) build_output_sha1: String,
    pub(crate) domains: BTreeMap<&'static str, DomainInstallation>,
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

    let mut domains = BTreeMap::new();
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
    ensure!(
        report
            .playable_unit_names
            .source_battle_and_ending_table_preserved,
        "current build no longer declares the untranslated battle and ending unit-name consumers"
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
            "weapon_shop_dialogue_lifetime" => WEAPON_SHOP_DIALOGUE_SCREEN_ROLES,
            other => {
                return Err(anyhow::anyhow!(
                    "unknown installed dialogue lifetime {other}"
                ));
            }
        };
        main_dialogue_screens.extend_from_slice(roles);
        if lifetime.runtime_bound_to_build {
            main_dialogue_runtime.extend_from_slice(roles);
        }
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

    let shop_runtime = report.weapon_shop_shared_text.runtime_bound_to_build;
    let installed_item_screens = ["weapon_shop_item_list", "weapon_shop_purchase_confirmation"];
    put(
        &mut domains,
        "item_names",
        installation(
            report.weapon_shop_shared_text.installed_item_name_count,
            &installed_item_screens,
            if shop_runtime {
                &installed_item_screens
            } else {
                &[]
            },
        ),
    )?;
    let installed_choice_screens = [
        "weapon_shop_purchase_confirmation",
        "weapon_shop_item_restriction_confirmation",
    ];
    put(
        &mut domains,
        "choice_labels",
        installation(
            report.weapon_shop_shared_text.installed_choice_label_count,
            &installed_choice_screens,
            if shop_runtime {
                &installed_choice_screens
            } else {
                &[]
            },
        ),
    )?;

    Ok(CurrentInstallation {
        build_output_sha1: output_sha1,
        domains,
    })
}

const WEAPON_SHOP_DIALOGUE_SCREEN_ROLES: &[&str] = &[
    "weapon_shop_purchase_confirmation",
    "weapon_shop_purchase_result",
    "weapon_shop_exit_message",
    "weapon_shop_inventory_full_message",
    "weapon_shop_insufficient_funds_message",
    "weapon_shop_item_restriction_confirmation",
    "weapon_shop_declined_continue_prompt",
    "weapon_shop_purchase_inventory_full_exit",
];

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
