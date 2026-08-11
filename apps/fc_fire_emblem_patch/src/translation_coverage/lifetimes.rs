use std::{collections::BTreeSet, fs, path::Path};

use anyhow::{Context, Result, ensure};
use serde::Deserialize;

use crate::{font_slots::ACTIVE_HANGUL_SLOT_COUNT, rom::EXPECTED_SOURCE_SHA1, sha1_hex};

use super::report::{StrongestLifetimeReport, TranslationLifetimeDemandReport};

const MAIN_DIALOGUE_REPORT_SCHEMA: u8 = 3;
const BATTLE_REPORT_SCHEMA: u8 = 12;

pub(super) struct LifetimeInputBindings<'a> {
    pub(super) main_dialogue_workspace_sha1: &'a str,
    pub(super) battle_fixed_workspace_sha1: &'a str,
    pub(super) battle_dialogue_workspace_sha1: &'a str,
    pub(super) battle_temporal_manifest_sha1: &'a str,
}

pub(super) struct TranslationLifetimeInventory {
    pub(super) demands: Vec<TranslationLifetimeDemandReport>,
    pub(super) unmeasured_screen_roles: Vec<String>,
    pub(super) strongest: StrongestLifetimeReport,
}

#[derive(Debug, Deserialize)]
struct MainDialogueGlyphWorksetReport {
    schema: u8,
    source_sha1: String,
    workspace_sha1: String,
    max_transition_chain_unique_glyph_count: usize,
    observed_screen_lifetimes: Vec<ObservedScreenLifetime>,
    capacity: MainDialogueCapacity,
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

    build_translation_lifetime_inventory(
        main,
        battle,
        sha1_hex(&main_bytes),
        sha1_hex(&battle_bytes),
        japanese_bearing_screen_roles,
    )
}

fn build_translation_lifetime_inventory(
    main: MainDialogueGlyphWorksetReport,
    battle: BattleSurfaceConstraintsReport,
    main_report_sha1: String,
    battle_report_sha1: String,
    japanese_bearing_screen_roles: &[String],
) -> Result<TranslationLifetimeInventory> {
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
    for lifetime in main.observed_screen_lifetimes {
        let (screen_role, measurement_basis) = match lifetime.screen_role.as_str() {
            "weapon-shop purchase handoff" => (
                "weapon_shop_purchase_confirmation",
                "observed purchase handoff with retained item and choice text",
            ),
            "ending character epilogue family" => (
                "ending_character_epilogue",
                "observed epilogue family union with name and location reservations",
            ),
            "turn-boundary game over" => (
                "game_over",
                "observed turn-boundary game-over union and selected dialogue",
            ),
            other => anyhow::bail!("unknown measured main-dialogue lifetime {other}"),
        };
        ensure!(
            lifetime.filled_unique_glyph_count
                + lifetime.preserved_active_source_code_count
                + lifetime.additional_target_glyph_reservation_count
                == lifetime.filled_slot_demand
                && lifetime.filled_set_fits_one_page_so_far
                    == (lifetime.filled_slot_demand <= ACTIVE_HANGUL_SLOT_COUNT),
            "{screen_role} lifetime demand components or fit result changed"
        );
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
    let strongest = demands
        .iter()
        .max_by_key(|demand| demand.total_slot_demand)
        .context("translation lifetime inventory has no measured demand")?;

    Ok(TranslationLifetimeInventory {
        strongest: StrongestLifetimeReport {
            state: "partial",
            compared_lifetime_count: demands.len(),
            japanese_bearing_screen_count: japanese_bearing_screen_roles.len(),
            selected_screen_role: Some(strongest.screen_role),
            selected_slot_demand: Some(strongest.total_slot_demand),
            unassigned_main_dialogue_maximum_target_glyph_count: main
                .max_transition_chain_unique_glyph_count,
            next_gate: "bind the 175-glyph main-dialogue transition maximum to its actual screen lifetime and preserved source codes before calling the 170-slot battle demand the global maximum",
        },
        demands,
        unmeasured_screen_roles,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn measured_maximum_stays_partial_while_a_larger_dialogue_chain_has_no_screen_bound() {
        let main = MainDialogueGlyphWorksetReport {
            schema: MAIN_DIALOGUE_REPORT_SCHEMA,
            source_sha1: EXPECTED_SOURCE_SHA1.to_owned(),
            workspace_sha1: "main".to_owned(),
            max_transition_chain_unique_glyph_count: 175,
            observed_screen_lifetimes: vec![
                observed("weapon-shop purchase handoff", 9, 17, 0),
                observed("ending character epilogue family", 33, 99, 18),
                observed("turn-boundary game over", 30, 90, 0),
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
        let roles = [
            "battle_animation",
            "ending_character_epilogue",
            "game_over",
            "map_menu",
            "weapon_shop_purchase_confirmation",
        ]
        .map(str::to_owned);

        let inventory = build_translation_lifetime_inventory(
            main,
            battle,
            "main-report".to_owned(),
            "battle-report".to_owned(),
            &roles,
        )
        .unwrap();

        assert_eq!(inventory.demands.len(), 4);
        assert_eq!(inventory.strongest.state, "partial");
        assert_eq!(
            inventory.strongest.selected_screen_role,
            Some("battle_animation")
        );
        assert_eq!(inventory.strongest.selected_slot_demand, Some(170));
        assert_eq!(
            inventory
                .strongest
                .unassigned_main_dialogue_maximum_target_glyph_count,
            175
        );
        assert_eq!(inventory.unmeasured_screen_roles, ["map_menu"]);
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
