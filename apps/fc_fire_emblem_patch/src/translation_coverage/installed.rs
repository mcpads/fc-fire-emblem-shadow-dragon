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
    options_menu: CurrentOptionsMenu,
    front_end_menu: CurrentFrontEndMenu,
    playable_unit_names: CurrentUnitNames,
    automatic_class_profiles: CurrentClassProfiles,
    title_logo: CurrentTitleLogo,
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
    #[serde(default)]
    screen_evidence_manifest_sha1: String,
    unique_glyph_count: usize,
    preserved_active_code_count: usize,
    #[serde(default)]
    temporal_sample_count: usize,
    #[serde(default)]
    unique_nametable_count: usize,
    font_physical_page: u8,
    font_mapper_register: u8,
    #[serde(default)]
    runtime_evidence_manifest_sha1: Option<String>,
    #[serde(default)]
    runtime_sample_count: usize,
    #[serde(default)]
    runtime_unique_image_count: usize,
    runtime_bound_to_dialogue_stage_output: bool,
}

#[derive(Clone, Debug)]
pub(crate) struct InstalledIntroDialogueCapacity {
    pub(crate) screen_role: &'static str,
    pub(crate) target_glyph_count: usize,
    pub(crate) preserved_active_code_count: usize,
    pub(crate) total_slot_demand: usize,
    pub(crate) screen_evidence_manifest_sha1: String,
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
    unique_glyph_count: usize,
    preserved_active_code_count: usize,
    no_save_source_lifetime_bound: bool,
    runtime_variants_bound_to_build: bool,
}

#[derive(Debug, Deserialize)]
struct CurrentOptionsMenu {
    installed_entry_count: usize,
    temporal_sample_count: usize,
    observed_row_states: Vec<u8>,
    target_glyph_count: usize,
    visible_active_code_count: usize,
    preserved_active_code_count: usize,
    total_slot_demand: usize,
    capacity_bound_to_build: bool,
}

#[derive(Debug, Deserialize)]
struct CurrentUnitNames {
    workspace_entry_count: usize,
    roster_page_target_glyph_count: usize,
    roster_page_preserved_active_code_count: usize,
    roster_page_total_slot_demand: usize,
    roster_projection_installed: bool,
    unit_summary_projection_installed: bool,
    source_battle_table_preserved: bool,
    source_ending_table_preserved: bool,
    roster_capacity_bound_to_build: bool,
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
struct CurrentTitleLogo {
    source_owned_tile_count: usize,
    installed_unique_tile_count: usize,
    installed_tilemap_cell_count: usize,
    installed_runtime_cleared_top_strip_cell_count: usize,
    installed_runtime_reasserted_logo_cell_count: usize,
    preserved_title_stream_bytes_unchanged: bool,
    preserved_runtime_completion_control_bytes_unchanged: bool,
    unassigned_title_chr_patterns_unchanged: bool,
    source_sword_sprite_tm_and_copyright_assets_unchanged: bool,
    runtime_evidence_manifest_sha1: String,
    runtime_sample_count: usize,
    runtime_unique_image_count: usize,
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
    #[serde(default)]
    runtime_evidence_manifest_sha1: String,
    runtime_evidence_output_sha1: String,
    #[serde(default)]
    runtime_sample_count: usize,
    #[serde(default)]
    runtime_unique_image_count: usize,
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
    pub(crate) front_end_target_glyph_count: usize,
    pub(crate) front_end_preserved_active_code_count: usize,
    pub(crate) front_end_no_save_source_lifetime_bound: bool,
    pub(crate) options_target_glyph_count: usize,
    pub(crate) options_preserved_active_code_count: usize,
    pub(crate) options_total_slot_demand: usize,
    pub(crate) options_capacity_bound_to_build: bool,
    pub(crate) title_logo_source_owned_tile_count: usize,
    pub(crate) title_logo_installed_unique_tile_count: usize,
    pub(crate) title_logo_installed_tilemap_cell_count: usize,
    pub(crate) title_logo_installed_runtime_cleared_top_strip_cell_count: usize,
    pub(crate) title_logo_installed_runtime_reasserted_logo_cell_count: usize,
    pub(crate) title_logo_runtime_bound_to_build: bool,
    pub(crate) roster_page_target_glyph_count: usize,
    pub(crate) roster_page_preserved_active_code_count: usize,
    pub(crate) roster_page_total_slot_demand: usize,
    pub(crate) weapon_shop_shared_page_target_glyph_count: usize,
    pub(crate) weapon_shop_shared_page_preserved_active_code_count: usize,
    pub(crate) weapon_shop_shared_page_total_slot_demand: usize,
    pub(crate) weapon_shop_capacity_bound_screen_roles: Vec<String>,
    pub(crate) battle_fixed_workspace_sha1: String,
    pub(crate) battle_dialogue_workspace_sha1: String,
    pub(crate) battle_temporal_manifest_sha1: String,
    pub(crate) intro_dialogue_capacities: Vec<InstalledIntroDialogueCapacity>,
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
    validate_unit_roster_lifetime(&report)?;
    validate_front_end_lifetime(&report)?;
    validate_options_lifetime(&report)?;
    validate_title_logo_lifetime(&report)?;
    let intro_dialogue_capacities = collect_intro_dialogue_capacities(&report)?;
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
        front_end_target_glyph_count: report.front_end_menu.unique_glyph_count,
        front_end_preserved_active_code_count: report.front_end_menu.preserved_active_code_count,
        front_end_no_save_source_lifetime_bound: report
            .front_end_menu
            .no_save_source_lifetime_bound,
        options_target_glyph_count: report.options_menu.target_glyph_count,
        options_preserved_active_code_count: report.options_menu.preserved_active_code_count,
        options_total_slot_demand: report.options_menu.total_slot_demand,
        options_capacity_bound_to_build: report.options_menu.capacity_bound_to_build,
        title_logo_source_owned_tile_count: report.title_logo.source_owned_tile_count,
        title_logo_installed_unique_tile_count: report.title_logo.installed_unique_tile_count,
        title_logo_installed_tilemap_cell_count: report.title_logo.installed_tilemap_cell_count,
        title_logo_installed_runtime_cleared_top_strip_cell_count: report
            .title_logo
            .installed_runtime_cleared_top_strip_cell_count,
        title_logo_installed_runtime_reasserted_logo_cell_count: report
            .title_logo
            .installed_runtime_reasserted_logo_cell_count,
        title_logo_runtime_bound_to_build: report.title_logo.runtime_bound_to_build,
        roster_page_target_glyph_count: report.playable_unit_names.roster_page_target_glyph_count,
        roster_page_preserved_active_code_count: report
            .playable_unit_names
            .roster_page_preserved_active_code_count,
        roster_page_total_slot_demand: report.playable_unit_names.roster_page_total_slot_demand,
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
        intro_dialogue_capacities,
    })
}

fn collect_intro_dialogue_capacities(
    report: &CurrentBuildReport,
) -> Result<Vec<InstalledIntroDialogueCapacity>> {
    [
        ("chapter_1_intro_dialogue", "intro_dialogue"),
        ("chapter_2_intro_dialogue", "later_intro_dialogue"),
    ]
    .into_iter()
    .map(|(installed_role, screen_role)| {
        let matches = report
            .main_dialogue
            .lifetimes
            .iter()
            .filter(|lifetime| lifetime.screen_role == installed_role)
            .collect::<Vec<_>>();
        ensure!(
            matches.len() == 1,
            "current build must contain one {installed_role} capacity"
        );
        let lifetime = matches[0];
        let total_slot_demand = lifetime
            .unique_glyph_count
            .checked_add(lifetime.preserved_active_code_count)
            .context("installed intro-dialogue capacity overflow")?;
        ensure!(
            !lifetime.screen_evidence_manifest_sha1.is_empty()
                && lifetime.temporal_sample_count >= 4
                && lifetime.unique_nametable_count >= 1
                && lifetime.unique_glyph_count > 0
                && total_slot_demand <= crate::font_slots::ACTIVE_HANGUL_SLOT_COUNT,
            "current {installed_role} capacity evidence changed"
        );
        Ok(InstalledIntroDialogueCapacity {
            screen_role,
            target_glyph_count: lifetime.unique_glyph_count,
            preserved_active_code_count: lifetime.preserved_active_code_count,
            total_slot_demand,
            screen_evidence_manifest_sha1: lifetime.screen_evidence_manifest_sha1.clone(),
        })
    })
    .collect()
}

fn validate_front_end_lifetime(report: &CurrentBuildReport) -> Result<()> {
    let front_end = &report.front_end_menu;
    ensure!(
        front_end.installed_entry_count == 7
            && front_end.unique_glyph_count > 0
            && front_end.preserved_active_code_count > 0
            && front_end.no_save_source_lifetime_bound,
        "current front-end menu lifetime changed"
    );
    Ok(())
}

fn validate_options_lifetime(report: &CurrentBuildReport) -> Result<()> {
    let options = &report.options_menu;
    let observed_rows = options
        .observed_row_states
        .iter()
        .copied()
        .collect::<BTreeSet<_>>();
    ensure!(
        options.installed_entry_count == 3
            && options.temporal_sample_count >= 2
            && observed_rows.contains(&0x20)
            && observed_rows.contains(&0x30)
            && options.target_glyph_count > 0
            && options.preserved_active_code_count > 0
            && options.visible_active_code_count == options.total_slot_demand
            && options.total_slot_demand
                == options.target_glyph_count + options.preserved_active_code_count
            && options.total_slot_demand <= crate::font_slots::ACTIVE_HANGUL_SLOT_COUNT
            && options.capacity_bound_to_build,
        "current options-menu lifetime changed"
    );
    Ok(())
}

fn validate_title_logo_lifetime(report: &CurrentBuildReport) -> Result<()> {
    let title = &report.title_logo;
    ensure!(
        title.installed_unique_tile_count > 0
            && title.installed_unique_tile_count <= title.source_owned_tile_count
            && title.installed_tilemap_cell_count >= title.installed_unique_tile_count
            && title.installed_runtime_cleared_top_strip_cell_count == 26
            && title.installed_runtime_reasserted_logo_cell_count == 11
            && title.preserved_title_stream_bytes_unchanged
            && title.preserved_runtime_completion_control_bytes_unchanged
            && title.unassigned_title_chr_patterns_unchanged
            && title.source_sword_sprite_tm_and_copyright_assets_unchanged,
        "current title-logo lifetime changed"
    );
    if title.runtime_bound_to_build {
        ensure!(
            !title.runtime_evidence_manifest_sha1.is_empty()
                && title.runtime_sample_count == 4
                && title.runtime_unique_image_count == 4,
            "bound title-logo runtime evidence is incomplete"
        );
    } else {
        ensure!(
            title.runtime_evidence_manifest_sha1.is_empty()
                && title.runtime_sample_count == 0
                && title.runtime_unique_image_count == 0,
            "unbound title-logo runtime evidence retains stale claims"
        );
    }
    Ok(())
}

fn validate_unit_roster_lifetime(report: &CurrentBuildReport) -> Result<()> {
    let names = &report.playable_unit_names;
    ensure!(
        (52..=53).contains(&names.workspace_entry_count)
            && names.roster_projection_installed
            && names.roster_page_target_glyph_count > 0
            && names.roster_page_preserved_active_code_count > 0
            && names.roster_page_total_slot_demand
                == names.roster_page_target_glyph_count
                    + names.roster_page_preserved_active_code_count
            && names.roster_page_total_slot_demand <= crate::font_slots::ACTIVE_HANGUL_SLOT_COUNT
            && names.roster_capacity_bound_to_build,
        "current unit-roster page lifetime changed or lies outside the supported 52-to-53-entry cumulative lineage"
    );
    Ok(())
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
    let every_runtime_binding_published = shared.runtime_bound_to_stage_output
        && shared.runtime_carried_forward_by_verified_writes
        && dialogue.runtime_bound_to_dialogue_stage_output;
    let every_runtime_binding_unpublished = !shared.runtime_bound_to_stage_output
        && !shared.runtime_carried_forward_by_verified_writes
        && !dialogue.runtime_bound_to_dialogue_stage_output;
    if every_runtime_binding_published {
        ensure!(
            !shared.runtime_evidence_manifest_sha1.is_empty()
                && shared.runtime_sample_count > 0
                && shared.runtime_unique_image_count > 0
                && dialogue
                    .runtime_evidence_manifest_sha1
                    .as_deref()
                    .is_some_and(|sha1| !sha1.is_empty())
                && dialogue.runtime_sample_count > 0
                && dialogue.runtime_unique_image_count > 0
                && shared.runtime_evidence_output_sha1
                    == report
                        .stages
                        .iter()
                        .find(|stage| {
                            stage.role == "weapon_shop_shared_item_names_and_choice_labels"
                        })
                        .and_then(|stage| stage.output_sha1.as_deref())
                        .context("current build lost the weapon-shop shared-text stage")?
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
            "bound weapon-shop runtime evidence scope changed"
        );
    } else {
        ensure!(
            every_runtime_binding_unpublished
                && shared.runtime_evidence_manifest_sha1.is_empty()
                && shared.runtime_evidence_output_sha1.is_empty()
                && shared.runtime_sample_count == 0
                && shared.runtime_unique_image_count == 0
                && dialogue.runtime_evidence_manifest_sha1.is_none()
                && dialogue.runtime_sample_count == 0
                && dialogue.runtime_unique_image_count == 0
                && dialogue_runtime_roles.is_empty()
                && item_name_runtime_roles.is_empty()
                && choice_label_runtime_roles.is_empty(),
            "unbound weapon-shop runtime evidence retains stale claims"
        );
    }
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
    let unit_name_complete_screens = if report.playable_unit_names.workspace_entry_count == 53 {
        unit_name_screens.clone()
    } else {
        Vec::new()
    };
    put(
        &mut domains,
        "unit_names",
        installation_with_complete_screens(
            report.playable_unit_names.workspace_entry_count,
            &unit_name_screens,
            &unit_name_complete_screens,
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

    let title_logo_runtime = report
        .title_logo
        .runtime_bound_to_build
        .then_some("title")
        .into_iter()
        .collect::<Vec<_>>();
    put(
        &mut domains,
        "title_graphics",
        installation(1, &["title"], &title_logo_runtime),
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
    let complete_title_screens = if report.chapter_titles.installed_entry_count == 25 {
        title_screens.clone()
    } else {
        Vec::new()
    };
    put(
        &mut domains,
        "chapter_titles",
        installation_with_complete_screens(
            report.chapter_titles.installed_entry_count,
            &title_screens,
            &complete_title_screens,
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
    installation_with_complete_screens(
        installed_target_unit_count,
        installed_screen_roles,
        installed_screen_roles,
        runtime_bound_screen_roles,
    )
}

fn installation_with_complete_screens(
    installed_target_unit_count: usize,
    installed_screen_roles: &[&str],
    consumer_complete_screen_roles: &[&str],
    runtime_bound_screen_roles: &[&str],
) -> DomainInstallation {
    let mut installed_screen_roles = installed_screen_roles
        .iter()
        .map(|role| (*role).to_owned())
        .collect::<Vec<_>>();
    installed_screen_roles.sort();
    installed_screen_roles.dedup();
    let mut consumer_complete_screen_roles = consumer_complete_screen_roles
        .iter()
        .map(|role| (*role).to_owned())
        .collect::<Vec<_>>();
    consumer_complete_screen_roles.sort();
    consumer_complete_screen_roles.dedup();
    let mut runtime_bound_screen_roles = runtime_bound_screen_roles
        .iter()
        .map(|role| (*role).to_owned())
        .collect::<Vec<_>>();
    runtime_bound_screen_roles.sort();
    runtime_bound_screen_roles.dedup();
    DomainInstallation {
        installed_target_unit_count,
        installed_screen_roles,
        consumer_complete_screen_roles,
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
        let mut report: CurrentBuildReport = serde_json::from_value(serde_json::json!({
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
                    "runtime_evidence_manifest_sha1": "shop-dialogue-runtime",
                    "runtime_sample_count": 7,
                    "runtime_unique_image_count": 4,
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
                "unique_glyph_count": 0,
                "preserved_active_code_count": 0,
                "no_save_source_lifetime_bound": false,
                "runtime_variants_bound_to_build": false
            },
            "options_menu": {
                "installed_entry_count": 3,
                "temporal_sample_count": 2,
                "observed_row_states": [32, 48],
                "target_glyph_count": 12,
                "visible_active_code_count": 90,
                "preserved_active_code_count": 78,
                "total_slot_demand": 90,
                "capacity_bound_to_build": true
            },
            "playable_unit_names": {
                "workspace_entry_count": 53,
                "roster_page_target_glyph_count": 72,
                "roster_page_preserved_active_code_count": 18,
                "roster_page_total_slot_demand": 90,
                "roster_projection_installed": true,
                "unit_summary_projection_installed": true,
                "source_battle_table_preserved": false,
                "source_ending_table_preserved": true,
                "roster_capacity_bound_to_build": true,
                "runtime_bound_to_build": false
            },
            "automatic_class_profiles": {
                "workspace_entry_count": 0,
                "installed_entry_count": 0,
                "page_unique_glyph_counts": [],
                "preserved_active_code_count": 0,
                "runtime_bound_to_build": false
            },
            "title_logo": {
                "source_owned_tile_count": 121,
                "installed_unique_tile_count": 117,
                "installed_tilemap_cell_count": 134,
                "installed_runtime_cleared_top_strip_cell_count": 26,
                "installed_runtime_reasserted_logo_cell_count": 11,
                "preserved_title_stream_bytes_unchanged": true,
                "preserved_runtime_completion_control_bytes_unchanged": true,
                "unassigned_title_chr_patterns_unchanged": true,
                "source_sword_sprite_tm_and_copyright_assets_unchanged": true,
                "runtime_evidence_manifest_sha1": "title-runtime",
                "runtime_sample_count": 4,
                "runtime_unique_image_count": 4,
                "runtime_bound_to_build": true
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
                "runtime_evidence_manifest_sha1": "shop-shared-runtime",
                "runtime_sample_count": 17,
                "runtime_unique_image_count": 5,
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
                "installed_fixed_entry_count": 232,
                "installed_unit_name_count": 53,
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
        assert_eq!(unit_names.installed_target_unit_count, 53);
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
            installations["title_graphics"].installed_screen_roles,
            ["title"]
        );
        assert_eq!(
            installations["title_graphics"].runtime_bound_screen_roles,
            ["title"]
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

        let shop_dialogue = report
            .main_dialogue
            .lifetimes
            .iter_mut()
            .find(|lifetime| lifetime.screen_role == "weapon_shop_dialogue_lifetime")
            .unwrap();
        shop_dialogue.runtime_evidence_manifest_sha1 = None;
        shop_dialogue.runtime_sample_count = 0;
        shop_dialogue.runtime_unique_image_count = 0;
        shop_dialogue.runtime_bound_to_dialogue_stage_output = false;
        let shared = &mut report.weapon_shop_shared_text;
        shared.runtime_evidence_manifest_sha1.clear();
        shared.runtime_evidence_output_sha1.clear();
        shared.runtime_sample_count = 0;
        shared.runtime_unique_image_count = 0;
        shared.runtime_bound_dialogue_screen_roles.clear();
        shared.runtime_bound_item_name_screen_roles.clear();
        shared.runtime_bound_choice_label_screen_roles.clear();
        shared.runtime_bound_to_stage_output = false;
        shared.runtime_carried_forward_by_verified_writes = false;
        validate_weapon_shop_lifetime(&report).unwrap();
        assert!(
            collect_domain_installations(&report).unwrap()["item_names"]
                .runtime_bound_screen_roles
                .is_empty()
        );

        report.title_logo.runtime_evidence_manifest_sha1.clear();
        report.title_logo.runtime_sample_count = 0;
        report.title_logo.runtime_unique_image_count = 0;
        report.title_logo.runtime_bound_to_build = false;
        validate_title_logo_lifetime(&report).unwrap();
    }
}
