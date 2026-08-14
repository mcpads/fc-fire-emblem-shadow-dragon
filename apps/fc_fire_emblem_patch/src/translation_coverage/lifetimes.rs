use std::{collections::BTreeSet, fs, path::Path};

use anyhow::{Context, Result, ensure};
use serde::Deserialize;

use crate::{font_slots::ACTIVE_HANGUL_SLOT_COUNT, rom::EXPECTED_SOURCE_SHA1, sha1_hex};

use super::installed::InstalledIntroDialogueCapacity;
use super::report::{StrongestLifetimeReport, TranslationLifetimeDemandReport};

mod chapter_intro_composite;
mod chapter_save;
mod ending_chapter_record;
mod front_end_menu;
mod full_page_bound;
mod intro_dialogue;
mod item_flow;
mod map_menu;
mod options_menu;
mod title_graphics;
mod unit_roster;
mod unit_ui;
mod weapon_shop;

const MAIN_DIALOGUE_REPORT_SCHEMA: u8 = 6;
const BATTLE_REPORT_SCHEMA: u8 = 13;

pub(super) struct LifetimeInputBindings<'a> {
    pub(super) source_path: &'a Path,
    pub(super) main_dialogue_workspace_path: &'a Path,
    pub(super) fixed_text_workspace_path: &'a Path,
    pub(super) unit_name_workspace_path: &'a Path,
    pub(super) item_action_label_workspace_path: &'a Path,
    pub(super) choice_label_workspace_path: &'a Path,
    pub(super) transition_label_workspace_path: &'a Path,
    pub(super) chapter_title_workspace_path: &'a Path,
    pub(super) chapter_save_continue_prompt_manifest_path: &'a Path,
    pub(super) map_menu_localization_path: &'a Path,
    pub(super) main_dialogue_workspace_sha1: &'a str,
    pub(super) item_action_label_workspace_sha1: &'a str,
    pub(super) choice_label_workspace_sha1: &'a str,
    pub(super) transition_label_workspace_sha1: &'a str,
    pub(super) chapter_title_workspace_sha1: &'a str,
    pub(super) map_menu_localization_sha1: &'a str,
    pub(super) class_profile_page_target_glyph_counts: &'a [usize],
    pub(super) class_profile_preserved_active_code_count: usize,
    pub(super) class_profile_runtime_bound_to_build: bool,
    pub(super) front_end_target_glyph_count: usize,
    pub(super) front_end_preserved_active_code_count: usize,
    pub(super) front_end_no_save_source_lifetime_bound: bool,
    pub(super) front_end_save_slot_selection_source_lifetime_bound: bool,
    pub(super) options_target_glyph_count: usize,
    pub(super) options_preserved_active_code_count: usize,
    pub(super) options_total_slot_demand: usize,
    pub(super) options_capacity_bound_to_build: bool,
    pub(super) title_logo_installed_unique_tile_count: usize,
    pub(super) title_logo_source_owned_tile_count: usize,
    pub(super) title_logo_installed_tilemap_cell_count: usize,
    pub(super) title_logo_installed_runtime_cleared_top_strip_cell_count: usize,
    pub(super) title_logo_installed_runtime_reasserted_logo_cell_count: usize,
    pub(super) title_logo_runtime_bound_to_build: bool,
    pub(super) current_build_report_sha1: &'a str,
    pub(super) roster_page_target_glyph_count: usize,
    pub(super) roster_page_preserved_active_code_count: usize,
    pub(super) roster_page_total_slot_demand: usize,
    pub(super) weapon_shop_shared_page_target_glyph_count: usize,
    pub(super) weapon_shop_shared_page_preserved_active_code_count: usize,
    pub(super) weapon_shop_shared_page_total_slot_demand: usize,
    pub(super) weapon_shop_capacity_bound_screen_roles: &'a [String],
    pub(super) unit_name_workspace_sha1: &'a str,
    pub(super) unit_ui_label_workspace_sha1: &'a str,
    pub(super) battle_fixed_workspace_sha1: &'a str,
    pub(super) battle_dialogue_workspace_sha1: &'a str,
    pub(super) battle_temporal_manifest_sha1: &'a str,
    pub(super) intro_dialogue_capacities: &'a [InstalledIntroDialogueCapacity],
}

pub(super) struct TranslationLifetimeInventory {
    pub(super) demands: Vec<TranslationLifetimeDemandReport>,
    pub(super) unmeasured_screen_roles: Vec<String>,
    pub(super) strongest: StrongestLifetimeReport,
}

struct LifetimeReports {
    main_dialogue: MainDialogueGlyphWorksetReport,
    battle: BattleSurfaceConstraintsReport,
    main_dialogue_sha1: String,
    battle_sha1: String,
}

struct ConsumerLifetimeDemands {
    unit_ui: Vec<TranslationLifetimeDemandReport>,
    unit_roster: TranslationLifetimeDemandReport,
    weapon_shop: Vec<TranslationLifetimeDemandReport>,
    item_flow: Vec<TranslationLifetimeDemandReport>,
    front_end_menu: Vec<TranslationLifetimeDemandReport>,
    options_menu: TranslationLifetimeDemandReport,
    title_graphics: TranslationLifetimeDemandReport,
    map_menu: TranslationLifetimeDemandReport,
    chapter_save: Vec<TranslationLifetimeDemandReport>,
    ending_chapter_record: TranslationLifetimeDemandReport,
    intro_dialogue: Vec<TranslationLifetimeDemandReport>,
    chapter_intro_composite: TranslationLifetimeDemandReport,
}

#[derive(Debug, Deserialize)]
struct MainDialogueGlyphWorksetReport {
    schema: u8,
    source_sha1: String,
    workspace_sha1: String,
    max_transition_chain_unique_glyph_count: usize,
    maximum_source_binding: MaximumDialogueSourceBinding,
    observed_screen_lifetimes: Vec<ObservedScreenLifetime>,
    capacity: MainDialogueCapacity,
}

#[derive(Debug, Deserialize)]
struct MaximumDialogueSourceBinding {
    screen_lifetime_bound: bool,
}

#[derive(Debug, Deserialize)]
struct MainDialogueCapacity {
    active_slot_count: usize,
    translation_input_complete: bool,
}

#[derive(Debug, Deserialize)]
struct ObservedScreenLifetime {
    screen_role: String,
    filled_unique_glyph_count: usize,
    preserved_active_source_code_count: usize,
    additional_target_glyph_reservation_count: usize,
    filled_slot_demand: usize,
    filled_set_fits_one_page_so_far: bool,
}

#[derive(Debug, Deserialize)]
struct BattleSurfaceConstraintsReport {
    schema: u8,
    source_sha1: String,
    fixed_workspace_sha1: String,
    dialogue_workspace_sha1: String,
    temporal_manifest_sha1: String,
    exact_modeled_text_overlay_count: usize,
    conservative_global_preserved_active_code_count: usize,
    exact_modeled_global_combined_slot_demand: usize,
}

pub(super) fn inspect_translation_lifetimes(
    main_dialogue_report_path: &Path,
    battle_report_path: &Path,
    unit_ui_report_path: &Path,
    bindings: LifetimeInputBindings<'_>,
    japanese_bearing_screen_roles: &[String],
) -> Result<TranslationLifetimeInventory> {
    let main_bytes = fs::read(main_dialogue_report_path).with_context(|| {
        format!(
            "read main-dialogue glyph workset report {}",
            main_dialogue_report_path.display()
        )
    })?;
    let main: MainDialogueGlyphWorksetReport =
        serde_json::from_slice(&main_bytes).with_context(|| {
            format!(
                "parse main-dialogue glyph workset report {}",
                main_dialogue_report_path.display()
            )
        })?;
    ensure!(
        main.schema == MAIN_DIALOGUE_REPORT_SCHEMA
            && main.source_sha1 == EXPECTED_SOURCE_SHA1
            && main.workspace_sha1 == bindings.main_dialogue_workspace_sha1
            && main.capacity.active_slot_count == ACTIVE_HANGUL_SLOT_COUNT
            && main.capacity.translation_input_complete,
        "main-dialogue lifetime report is stale or incomplete"
    );

    let battle_bytes = fs::read(battle_report_path).with_context(|| {
        format!(
            "read battle surface-constraints report {}",
            battle_report_path.display()
        )
    })?;
    let battle: BattleSurfaceConstraintsReport = serde_json::from_slice(&battle_bytes)
        .with_context(|| {
            format!(
                "parse battle surface-constraints report {}",
                battle_report_path.display()
            )
        })?;
    ensure!(
        battle.schema == BATTLE_REPORT_SCHEMA
            && battle.source_sha1 == EXPECTED_SOURCE_SHA1
            && battle.fixed_workspace_sha1 == bindings.battle_fixed_workspace_sha1
            && battle.dialogue_workspace_sha1 == bindings.battle_dialogue_workspace_sha1
            && battle.temporal_manifest_sha1 == bindings.battle_temporal_manifest_sha1,
        "battle lifetime report is stale or incomplete"
    );
    ensure!(
        battle.exact_modeled_text_overlay_count
            + battle.conservative_global_preserved_active_code_count
            == battle.exact_modeled_global_combined_slot_demand,
        "battle lifetime demand components no longer sum to the exact modeled total"
    );

    let unit_ui_demands = unit_ui::inspect(
        unit_ui_report_path,
        unit_ui::InputBindings {
            fixed_text_workspace_sha1: bindings.battle_fixed_workspace_sha1,
            unit_name_workspace_sha1: bindings.unit_name_workspace_sha1,
            unit_ui_label_workspace_sha1: bindings.unit_ui_label_workspace_sha1,
        },
    )?;
    let unit_roster_demand = unit_roster::inspect(unit_roster::InputBindings {
        page_target_glyph_count: bindings.roster_page_target_glyph_count,
        page_preserved_active_code_count: bindings.roster_page_preserved_active_code_count,
        page_total_slot_demand: bindings.roster_page_total_slot_demand,
        evidence_report_sha1: bindings.current_build_report_sha1,
    })?;
    let weapon_shop_demands = weapon_shop::inspect(weapon_shop::InputBindings {
        shared_page_target_glyph_count: bindings.weapon_shop_shared_page_target_glyph_count,
        shared_page_preserved_active_code_count: bindings
            .weapon_shop_shared_page_preserved_active_code_count,
        shared_page_total_slot_demand: bindings.weapon_shop_shared_page_total_slot_demand,
        capacity_bound_screen_roles: bindings.weapon_shop_capacity_bound_screen_roles,
        evidence_report_sha1: bindings.current_build_report_sha1,
    })?;
    let item_flow_demands = item_flow::inspect(item_flow::InputBindings {
        source_path: bindings.source_path,
        main_dialogue_workspace_path: bindings.main_dialogue_workspace_path,
        fixed_text_workspace_path: bindings.fixed_text_workspace_path,
        unit_name_workspace_path: bindings.unit_name_workspace_path,
        item_action_label_workspace_path: bindings.item_action_label_workspace_path,
        main_dialogue_workspace_sha1: bindings.main_dialogue_workspace_sha1,
        fixed_text_workspace_sha1: bindings.battle_fixed_workspace_sha1,
        unit_name_workspace_sha1: bindings.unit_name_workspace_sha1,
        item_action_label_workspace_sha1: bindings.item_action_label_workspace_sha1,
    })?;
    let front_end_menu_demands = front_end_menu::inspect(front_end_menu::InputBindings {
        target_glyph_count: bindings.front_end_target_glyph_count,
        preserved_active_code_count: bindings.front_end_preserved_active_code_count,
        no_save_source_lifetime_bound: bindings.front_end_no_save_source_lifetime_bound,
        save_slot_selection_source_lifetime_bound: bindings
            .front_end_save_slot_selection_source_lifetime_bound,
        evidence_report_sha1: bindings.current_build_report_sha1,
    })?;
    let options_menu_demand = options_menu::inspect(options_menu::InputBindings {
        target_glyph_count: bindings.options_target_glyph_count,
        preserved_active_code_count: bindings.options_preserved_active_code_count,
        total_slot_demand: bindings.options_total_slot_demand,
        capacity_bound_to_build: bindings.options_capacity_bound_to_build,
        evidence_report_sha1: bindings.current_build_report_sha1,
    })?;
    let title_graphics_demand = title_graphics::inspect(title_graphics::InputBindings {
        installed_unique_tile_count: bindings.title_logo_installed_unique_tile_count,
        source_owned_tile_count: bindings.title_logo_source_owned_tile_count,
        installed_tilemap_cell_count: bindings.title_logo_installed_tilemap_cell_count,
        installed_runtime_cleared_top_strip_cell_count: bindings
            .title_logo_installed_runtime_cleared_top_strip_cell_count,
        installed_runtime_reasserted_logo_cell_count: bindings
            .title_logo_installed_runtime_reasserted_logo_cell_count,
        runtime_bound_to_build: bindings.title_logo_runtime_bound_to_build,
        evidence_report_sha1: bindings.current_build_report_sha1,
    })?;
    let map_menu_demand = map_menu::inspect(map_menu::InputBindings {
        source_path: bindings.source_path,
        localization_path: bindings.map_menu_localization_path,
        localization_sha1: bindings.map_menu_localization_sha1,
    })?;
    let chapter_save_demands = chapter_save::inspect(chapter_save::InputBindings {
        source_path: bindings.source_path,
        main_dialogue_workspace_path: bindings.main_dialogue_workspace_path,
        choice_label_workspace_path: bindings.choice_label_workspace_path,
        transition_label_workspace_path: bindings.transition_label_workspace_path,
        continue_prompt_manifest_path: bindings.chapter_save_continue_prompt_manifest_path,
        main_dialogue_workspace_sha1: bindings.main_dialogue_workspace_sha1,
        choice_label_workspace_sha1: bindings.choice_label_workspace_sha1,
        transition_label_workspace_sha1: bindings.transition_label_workspace_sha1,
    })?;
    let ending_chapter_record_demand =
        ending_chapter_record::inspect(ending_chapter_record::InputBindings {
            source_path: bindings.source_path,
            chapter_title_workspace_path: bindings.chapter_title_workspace_path,
            transition_label_workspace_path: bindings.transition_label_workspace_path,
            chapter_title_workspace_sha1: bindings.chapter_title_workspace_sha1,
            transition_label_workspace_sha1: bindings.transition_label_workspace_sha1,
        })?;
    let intro_dialogue_demands = intro_dialogue::inspect(intro_dialogue::InputBindings {
        capacities: bindings.intro_dialogue_capacities,
        current_build_report_sha1: bindings.current_build_report_sha1,
    })?;
    let chapter_intro_composite_demand =
        chapter_intro_composite::inspect(chapter_intro_composite::InputBindings {
            source_path: bindings.source_path,
            main_dialogue_workspace_path: bindings.main_dialogue_workspace_path,
            chapter_title_workspace_path: bindings.chapter_title_workspace_path,
            main_dialogue_workspace_sha1: bindings.main_dialogue_workspace_sha1,
            chapter_title_workspace_sha1: bindings.chapter_title_workspace_sha1,
        })?;

    build_translation_lifetime_inventory(
        LifetimeReports {
            main_dialogue: main,
            battle,
            main_dialogue_sha1: sha1_hex(&main_bytes),
            battle_sha1: sha1_hex(&battle_bytes),
        },
        bindings,
        ConsumerLifetimeDemands {
            unit_ui: unit_ui_demands,
            unit_roster: unit_roster_demand,
            weapon_shop: weapon_shop_demands,
            item_flow: item_flow_demands,
            front_end_menu: front_end_menu_demands,
            options_menu: options_menu_demand,
            title_graphics: title_graphics_demand,
            map_menu: map_menu_demand,
            chapter_save: chapter_save_demands,
            ending_chapter_record: ending_chapter_record_demand,
            intro_dialogue: intro_dialogue_demands,
            chapter_intro_composite: chapter_intro_composite_demand,
        },
        japanese_bearing_screen_roles,
    )
}

fn build_translation_lifetime_inventory(
    reports: LifetimeReports,
    bindings: LifetimeInputBindings<'_>,
    consumer_demands: ConsumerLifetimeDemands,
    japanese_bearing_screen_roles: &[String],
) -> Result<TranslationLifetimeInventory> {
    let LifetimeReports {
        main_dialogue: main,
        battle,
        main_dialogue_sha1: main_report_sha1,
        battle_sha1: battle_report_sha1,
    } = reports;
    let mut demands = vec![TranslationLifetimeDemandReport {
        screen_role: "battle_animation",
        measurement_basis: "exact modeled battle text maximum plus the conservative global preserved-code union",
        target_glyph_count: battle.exact_modeled_text_overlay_count,
        preserved_active_source_code_count: battle.conservative_global_preserved_active_code_count,
        additional_target_glyph_reservation_count: 0,
        total_slot_demand: battle.exact_modeled_global_combined_slot_demand,
        active_slot_count: ACTIVE_HANGUL_SLOT_COUNT,
        fits_active_page: battle.exact_modeled_global_combined_slot_demand
            <= ACTIVE_HANGUL_SLOT_COUNT,
        evidence_report_sha1: battle_report_sha1,
    }];
    ensure!(
        bindings.class_profile_runtime_bound_to_build,
        "class-profile lifetime is not runtime-bound to the current build"
    );
    let class_profile_target_glyph_count = bindings
        .class_profile_page_target_glyph_counts
        .iter()
        .copied()
        .max()
        .context("class-profile lifetime has no installed page groups")?;
    let class_profile_total_slot_demand = class_profile_target_glyph_count
        .checked_add(bindings.class_profile_preserved_active_code_count)
        .context("class-profile lifetime slot demand overflow")?;
    demands.push(TranslationLifetimeDemandReport {
        screen_role: "class_profile",
        measurement_basis:
            "largest exact-output-bound installed profile-group working set plus preserved active codes",
        target_glyph_count: class_profile_target_glyph_count,
        preserved_active_source_code_count: bindings.class_profile_preserved_active_code_count,
        additional_target_glyph_reservation_count: 0,
        total_slot_demand: class_profile_total_slot_demand,
        active_slot_count: ACTIVE_HANGUL_SLOT_COUNT,
        fits_active_page: class_profile_total_slot_demand <= ACTIVE_HANGUL_SLOT_COUNT,
        evidence_report_sha1: bindings.current_build_report_sha1.to_owned(),
    });
    demands.extend(consumer_demands.unit_ui);
    demands.push(consumer_demands.unit_roster);
    demands.extend(consumer_demands.weapon_shop);
    demands.extend(consumer_demands.item_flow);
    demands.extend(consumer_demands.front_end_menu);
    demands.push(consumer_demands.options_menu);
    demands.push(consumer_demands.title_graphics);
    demands.push(consumer_demands.map_menu);
    demands.extend(consumer_demands.chapter_save);
    demands.push(consumer_demands.ending_chapter_record);
    demands.extend(consumer_demands.intro_dialogue);
    demands.push(consumer_demands.chapter_intro_composite);
    for lifetime in main.observed_screen_lifetimes {
        ensure!(
            lifetime.filled_unique_glyph_count
                + lifetime.preserved_active_source_code_count
                + lifetime.additional_target_glyph_reservation_count
                == lifetime.filled_slot_demand
                && lifetime.filled_set_fits_one_page_so_far
                    == (lifetime.filled_slot_demand <= ACTIVE_HANGUL_SLOT_COUNT),
            "{} lifetime demand components or fit result changed",
            lifetime.screen_role
        );
        let (screen_role, measurement_basis) = match lifetime.screen_role.as_str() {
            "weapon-shop purchase handoff" => {
                let installed_demand = demands
                    .iter()
                    .find(|demand| demand.screen_role == "weapon_shop_purchase_confirmation")
                    .context("weapon-shop purchase handoff lost its installed shared-page bound")?;
                ensure!(
                    lifetime.filled_slot_demand <= installed_demand.total_slot_demand,
                    "installed weapon-shop shared-page bound no longer covers the observed purchase handoff"
                );
                continue;
            }
            "ending character epilogue family" => (
                "ending_character_epilogue",
                "observed epilogue family union with name and location reservations",
            ),
            "turn-boundary game over" => (
                "game_over",
                "observed turn-boundary game-over union and selected dialogue",
            ),
            "chapter-seven maximum dialogue page" => (
                "chapter_clear_epilogue_dialogue",
                "observed fifteen-page maximum dialogue with page-granular Korean demand",
            ),
            other => anyhow::bail!("unknown measured main-dialogue lifetime {other}"),
        };
        demands.push(TranslationLifetimeDemandReport {
            screen_role,
            measurement_basis,
            target_glyph_count: lifetime.filled_unique_glyph_count,
            preserved_active_source_code_count: lifetime.preserved_active_source_code_count,
            additional_target_glyph_reservation_count: lifetime
                .additional_target_glyph_reservation_count,
            total_slot_demand: lifetime.filled_slot_demand,
            active_slot_count: ACTIVE_HANGUL_SLOT_COUNT,
            fits_active_page: lifetime.filled_set_fits_one_page_so_far,
            evidence_report_sha1: main_report_sha1.clone(),
        });
    }
    demands.sort_by_key(|demand| demand.screen_role);

    let measured_roles = demands
        .iter()
        .map(|demand| demand.screen_role)
        .collect::<BTreeSet<_>>();
    ensure!(
        measured_roles.len() == demands.len(),
        "translation lifetime inventory repeats a measured screen role"
    );
    let japanese_roles = japanese_bearing_screen_roles
        .iter()
        .map(String::as_str)
        .collect::<BTreeSet<_>>();
    ensure!(
        measured_roles.is_subset(&japanese_roles),
        "translation lifetime inventory measures a screen outside the Japanese-bearing partition"
    );
    let unmeasured_screen_roles = japanese_roles
        .difference(&measured_roles)
        .map(|role| (*role).to_owned())
        .collect::<Vec<_>>();
    let all_lifetimes_measured = unmeasured_screen_roles.is_empty();
    let strongest = demands
        .iter()
        .max_by_key(|demand| demand.total_slot_demand)
        .context("translation lifetime inventory has no measured demand")?;
    Ok(TranslationLifetimeInventory {
        strongest: StrongestLifetimeReport {
            state: if all_lifetimes_measured {
                "complete"
            } else {
                "partial"
            },
            compared_lifetime_count: demands.len(),
            japanese_bearing_screen_count: japanese_bearing_screen_roles.len(),
            selected_screen_role: Some(strongest.screen_role),
            selected_slot_demand: Some(strongest.total_slot_demand),
            main_dialogue_maximum_target_glyph_count: main.max_transition_chain_unique_glyph_count,
            main_dialogue_maximum_screen_bound: main.maximum_source_binding.screen_lifetime_bound,
            next_gate: if all_lifetimes_measured {
                "install the remaining translated domains and verify zero target Japanese on one final ROM"
            } else {
                "compare the remaining unmeasured screen lifetimes"
            },
        },
        demands,
        unmeasured_screen_roles,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn full_page_map_menu_bound_can_become_the_strongest_lifetime() {
        let main = MainDialogueGlyphWorksetReport {
            schema: MAIN_DIALOGUE_REPORT_SCHEMA,
            source_sha1: EXPECTED_SOURCE_SHA1.to_owned(),
            workspace_sha1: "main".to_owned(),
            max_transition_chain_unique_glyph_count: 175,
            maximum_source_binding: MaximumDialogueSourceBinding {
                screen_lifetime_bound: true,
            },
            observed_screen_lifetimes: vec![
                observed("weapon-shop purchase handoff", 9, 17, 0),
                observed("ending character epilogue family", 33, 99, 18),
                observed("turn-boundary game over", 30, 90, 0),
                observed("chapter-seven maximum dialogue page", 35, 100, 0),
            ],
            capacity: MainDialogueCapacity {
                active_slot_count: ACTIVE_HANGUL_SLOT_COUNT,
                translation_input_complete: true,
            },
        };
        let battle = BattleSurfaceConstraintsReport {
            schema: BATTLE_REPORT_SCHEMA,
            source_sha1: EXPECTED_SOURCE_SHA1.to_owned(),
            fixed_workspace_sha1: "fixed".to_owned(),
            dialogue_workspace_sha1: "battle".to_owned(),
            temporal_manifest_sha1: "temporal".to_owned(),
            exact_modeled_text_overlay_count: 131,
            conservative_global_preserved_active_code_count: 39,
            exact_modeled_global_combined_slot_demand: 170,
        };
        let unit_ui_demands = unit_ui_demands();
        let unit_roster_demand = unit_roster::inspect(unit_roster::InputBindings {
            page_target_glyph_count: 72,
            page_preserved_active_code_count: 18,
            page_total_slot_demand: 90,
            evidence_report_sha1: "build-report",
        })
        .unwrap();
        let shop_roles = crate::translation_coverage::weapon_shop::SCREEN_ROLES.map(str::to_owned);
        let weapon_shop_demands = weapon_shop::inspect(weapon_shop::InputBindings {
            shared_page_target_glyph_count: 60,
            shared_page_preserved_active_code_count: 90,
            shared_page_total_slot_demand: 150,
            capacity_bound_screen_roles: &shop_roles,
            evidence_report_sha1: "build-report",
        })
        .unwrap();
        let item_flow_demands = item_flow_demands();
        let front_end_menu_demands = front_end_menu::inspect(front_end_menu::InputBindings {
            target_glyph_count: 15,
            preserved_active_code_count: 12,
            no_save_source_lifetime_bound: true,
            save_slot_selection_source_lifetime_bound: true,
            evidence_report_sha1: "build-report",
        })
        .unwrap();
        let options_menu_demand = options_menu::inspect(options_menu::InputBindings {
            target_glyph_count: 12,
            preserved_active_code_count: 78,
            total_slot_demand: 90,
            capacity_bound_to_build: true,
            evidence_report_sha1: "build-report",
        })
        .unwrap();
        let map_menu_demand = TranslationLifetimeDemandReport {
            screen_role: "map_menu",
            measurement_basis: "fixture full-page upper bound",
            target_glyph_count: 17,
            preserved_active_source_code_count: 186,
            additional_target_glyph_reservation_count: 0,
            total_slot_demand: 203,
            active_slot_count: ACTIVE_HANGUL_SLOT_COUNT,
            fits_active_page: true,
            evidence_report_sha1: "map-menu-evidence".to_owned(),
        };
        let mut roles = [
            "battle_animation",
            "class_profile",
            "chapter_clear_epilogue_dialogue",
            "chapter_intro_title_dialogue_composite",
            "ending_chapter_record_scroll",
            "ending_character_epilogue",
            "game_over",
            "map_menu",
            "new_game_choice",
            "save_slot_selection",
            "options",
            "unit_command_menu",
            "unit_roster",
            "unit_status",
            "unit_summary",
            "title",
        ]
        .map(str::to_owned)
        .to_vec();
        roles.extend(shop_roles.iter().cloned());
        roles.extend(
            item_flow_demands
                .iter()
                .map(|demand| demand.screen_role.to_owned()),
        );

        let inventory = build_translation_lifetime_inventory(
            LifetimeReports {
                main_dialogue: main,
                battle,
                main_dialogue_sha1: "main-report".to_owned(),
                battle_sha1: "battle-report".to_owned(),
            },
            LifetimeInputBindings {
                source_path: Path::new("source.nes"),
                main_dialogue_workspace_path: Path::new("main.json"),
                fixed_text_workspace_path: Path::new("fixed.json"),
                unit_name_workspace_path: Path::new("units.json"),
                item_action_label_workspace_path: Path::new("item-actions.json"),
                choice_label_workspace_path: Path::new("choices.json"),
                transition_label_workspace_path: Path::new("transitions.json"),
                chapter_title_workspace_path: Path::new("chapter-titles.json"),
                chapter_save_continue_prompt_manifest_path: Path::new("save-runtime.json"),
                map_menu_localization_path: Path::new("map-menu.json"),
                main_dialogue_workspace_sha1: "main",
                item_action_label_workspace_sha1: "item-actions",
                choice_label_workspace_sha1: "choices",
                transition_label_workspace_sha1: "transitions",
                chapter_title_workspace_sha1: "chapter-titles",
                map_menu_localization_sha1: "map-menu",
                class_profile_page_target_glyph_counts: &[143, 161],
                class_profile_preserved_active_code_count: 12,
                class_profile_runtime_bound_to_build: true,
                front_end_target_glyph_count: 15,
                front_end_preserved_active_code_count: 12,
                front_end_no_save_source_lifetime_bound: true,
                front_end_save_slot_selection_source_lifetime_bound: true,
                options_target_glyph_count: 12,
                options_preserved_active_code_count: 78,
                options_total_slot_demand: 90,
                options_capacity_bound_to_build: true,
                title_logo_installed_unique_tile_count: 117,
                title_logo_source_owned_tile_count: 121,
                title_logo_installed_tilemap_cell_count: 134,
                title_logo_installed_runtime_cleared_top_strip_cell_count: 26,
                title_logo_installed_runtime_reasserted_logo_cell_count: 11,
                title_logo_runtime_bound_to_build: true,
                current_build_report_sha1: "build-report",
                roster_page_target_glyph_count: 72,
                roster_page_preserved_active_code_count: 18,
                roster_page_total_slot_demand: 90,
                weapon_shop_shared_page_target_glyph_count: 60,
                weapon_shop_shared_page_preserved_active_code_count: 90,
                weapon_shop_shared_page_total_slot_demand: 150,
                weapon_shop_capacity_bound_screen_roles: &shop_roles,
                unit_name_workspace_sha1: "units",
                unit_ui_label_workspace_sha1: "labels",
                battle_fixed_workspace_sha1: "fixed",
                battle_dialogue_workspace_sha1: "battle",
                battle_temporal_manifest_sha1: "temporal",
                intro_dialogue_capacities: &[],
            },
            ConsumerLifetimeDemands {
                unit_ui: unit_ui_demands,
                unit_roster: unit_roster_demand,
                weapon_shop: weapon_shop_demands,
                item_flow: item_flow_demands,
                front_end_menu: front_end_menu_demands,
                options_menu: options_menu_demand,
                title_graphics: title_graphics::inspect(title_graphics::InputBindings {
                    installed_unique_tile_count: 117,
                    source_owned_tile_count: 121,
                    installed_tilemap_cell_count: 134,
                    installed_runtime_cleared_top_strip_cell_count: 26,
                    installed_runtime_reasserted_logo_cell_count: 11,
                    runtime_bound_to_build: true,
                    evidence_report_sha1: "build-report",
                })
                .unwrap(),
                map_menu: map_menu_demand,
                chapter_save: Vec::new(),
                ending_chapter_record: TranslationLifetimeDemandReport {
                    screen_role: "ending_chapter_record_scroll",
                    measurement_basis: "fixture complete ending-record union",
                    target_glyph_count: 91,
                    preserved_active_source_code_count: 0,
                    additional_target_glyph_reservation_count: 0,
                    total_slot_demand: 91,
                    active_slot_count: ACTIVE_HANGUL_SLOT_COUNT,
                    fits_active_page: true,
                    evidence_report_sha1: "ending-record-evidence".to_owned(),
                },
                intro_dialogue: Vec::new(),
                chapter_intro_composite: TranslationLifetimeDemandReport {
                    screen_role: "chapter_intro_title_dialogue_composite",
                    measurement_basis: "fixture complete chapter-intro composite bound",
                    target_glyph_count: 100,
                    preserved_active_source_code_count: 100,
                    additional_target_glyph_reservation_count: 0,
                    total_slot_demand: 200,
                    active_slot_count: ACTIVE_HANGUL_SLOT_COUNT,
                    fits_active_page: true,
                    evidence_report_sha1: "chapter-intro-evidence".to_owned(),
                },
            },
            &roles,
        )
        .unwrap();

        assert_eq!(inventory.demands.len(), 31);
        assert_eq!(inventory.strongest.state, "complete");
        assert_eq!(inventory.strongest.selected_screen_role, Some("map_menu"));
        assert_eq!(inventory.strongest.selected_slot_demand, Some(203));
        assert_eq!(
            inventory.strongest.main_dialogue_maximum_target_glyph_count,
            175
        );
        assert!(inventory.strongest.main_dialogue_maximum_screen_bound);
        assert!(inventory.unmeasured_screen_roles.is_empty());
    }

    fn unit_ui_demands() -> Vec<TranslationLifetimeDemandReport> {
        [
            ("unit_summary", 36),
            ("unit_status", 30),
            ("unit_command_menu", 30),
        ]
        .into_iter()
        .map(|(screen_role, demand)| TranslationLifetimeDemandReport {
            screen_role,
            measurement_basis: "fixture upper bound",
            target_glyph_count: demand,
            preserved_active_source_code_count: 0,
            additional_target_glyph_reservation_count: 0,
            total_slot_demand: demand,
            active_slot_count: ACTIVE_HANGUL_SLOT_COUNT,
            fits_active_page: true,
            evidence_report_sha1: "unit-ui-report".to_owned(),
        })
        .collect()
    }

    fn item_flow_demands() -> Vec<TranslationLifetimeDemandReport> {
        [
            "item_inventory_list",
            "item_action_menu",
            "item_use_result",
            "item_equip_result",
            "item_transfer_result",
            "item_discard_result",
        ]
        .into_iter()
        .map(|screen_role| TranslationLifetimeDemandReport {
            screen_role,
            measurement_basis: "fixture item-flow upper bound",
            target_glyph_count: 20,
            preserved_active_source_code_count: 0,
            additional_target_glyph_reservation_count: 0,
            total_slot_demand: 20,
            active_slot_count: ACTIVE_HANGUL_SLOT_COUNT,
            fits_active_page: true,
            evidence_report_sha1: "item-flow-evidence".to_owned(),
        })
        .collect()
    }

    fn observed(
        screen_role: &str,
        target_glyph_count: usize,
        preserved_active_source_code_count: usize,
        additional_target_glyph_reservation_count: usize,
    ) -> ObservedScreenLifetime {
        let filled_slot_demand = target_glyph_count
            + preserved_active_source_code_count
            + additional_target_glyph_reservation_count;
        ObservedScreenLifetime {
            screen_role: screen_role.to_owned(),
            filled_unique_glyph_count: target_glyph_count,
            preserved_active_source_code_count,
            additional_target_glyph_reservation_count,
            filled_slot_demand,
            filled_set_fits_one_page_so_far: filled_slot_demand <= ACTIVE_HANGUL_SLOT_COUNT,
        }
    }
}
