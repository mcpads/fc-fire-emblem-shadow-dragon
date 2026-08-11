use std::collections::{BTreeMap, BTreeSet};

use anyhow::{Context, Result, ensure};

use crate::screen_contracts::ScreenTranslationPartition;

pub(crate) struct TranslationDomainSeed {
    pub(crate) id: &'static str,
    pub(crate) target_unit: &'static str,
}

pub(crate) struct DomainScreenTargets {
    pub(crate) id: &'static str,
    pub(crate) target_unit: &'static str,
    pub(crate) screen_roles: Vec<String>,
}

pub(crate) const DOMAIN_SEEDS: &[TranslationDomainSeed] = &[
    domain("battle_dialogue", "dialogue_line"),
    domain("battle_forecast_label", "inline_label"),
    domain("battle_message_templates", "message_template"),
    domain("chapter_save_offer_label", "fixed_label"),
    domain("chapter_titles", "chapter_title"),
    domain("choice_labels", "choice_label"),
    domain("class_names", "class_name"),
    domain("class_profiles", "class_profile"),
    domain("ending_record_labels", "ending_aggregate_record"),
    domain("enemy_names", "enemy_name"),
    domain("front_end_menu_labels", "menu_label"),
    domain("item_action_labels", "item_action_label"),
    domain("item_names", "item_name"),
    domain("location_names", "location_name"),
    domain("main_dialogue", "dialogue_line"),
    domain("map_menu_labels", "map_menu_label"),
    domain("options_labels", "option_label"),
    domain("roster_header", "roster_header"),
    domain("terrain_names", "terrain_name"),
    domain("title_graphics", "title_surface"),
    domain("unit_names", "unit_name"),
    domain("unit_ui_labels", "unit_ui_label"),
];

const SCREEN_TARGETS: &[ScreenTargetSeed] = &[
    screen("title", &["title_graphics"]),
    screen("new_game_choice", &["front_end_menu_labels"]),
    screen("class_profile", &["class_profiles"]),
    screen("intro_dialogue", &["main_dialogue"]),
    screen("later_intro_dialogue", &["main_dialogue"]),
    screen("map_menu", &["map_menu_labels"]),
    screen("options", &["options_labels"]),
    screen(
        "unit_summary",
        &["unit_names", "class_names", "item_names", "unit_ui_labels"],
    ),
    screen("unit_command_menu", &["unit_ui_labels"]),
    screen(
        "unit_status",
        &["unit_names", "class_names", "item_names", "unit_ui_labels"],
    ),
    screen("unit_roster", &["roster_header", "unit_names"]),
    screen(
        "battle_animation",
        &[
            "unit_names",
            "enemy_names",
            "class_names",
            "item_names",
            "terrain_names",
            "battle_message_templates",
            "battle_forecast_label",
            "battle_dialogue",
        ],
    ),
    screen("game_over", &["main_dialogue"]),
    screen("chapter_clear_epilogue_dialogue", &["main_dialogue"]),
    screen(
        "chapter_save_offer",
        &["chapter_save_offer_label", "choice_labels"],
    ),
    screen(
        "chapter_save_complete_continue_prompt",
        &["main_dialogue", "choice_labels"],
    ),
    screen("chapter_save_complete_power_off_notice", &["main_dialogue"]),
    screen(
        "ending_chapter_record_scroll",
        &["chapter_titles", "ending_record_labels"],
    ),
    screen(
        "ending_character_epilogue",
        &["main_dialogue", "unit_names", "location_names"],
    ),
    screen(
        "chapter_intro_title_dialogue_composite",
        &["chapter_titles", "main_dialogue"],
    ),
    screen("suspend_message", &["main_dialogue"]),
    screen("weapon_shop_item_list", &["main_dialogue", "item_names"]),
    screen(
        "weapon_shop_purchase_confirmation",
        &["main_dialogue", "item_names", "choice_labels"],
    ),
    screen(
        "weapon_shop_purchase_result",
        &["main_dialogue", "item_names", "choice_labels"],
    ),
    screen("weapon_shop_exit_message", &["main_dialogue", "item_names"]),
    screen(
        "weapon_shop_inventory_full_message",
        &["main_dialogue", "item_names"],
    ),
    screen(
        "weapon_shop_insufficient_funds_message",
        &["main_dialogue", "choice_labels"],
    ),
    screen(
        "weapon_shop_item_restriction_confirmation",
        &["main_dialogue", "item_names", "choice_labels"],
    ),
    screen(
        "weapon_shop_declined_continue_prompt",
        &["main_dialogue", "item_names", "choice_labels"],
    ),
    screen(
        "weapon_shop_purchase_inventory_full_exit",
        &["main_dialogue", "item_names"],
    ),
    screen("item_inventory_list", &["item_names"]),
    screen("item_action_menu", &["item_names", "item_action_labels"]),
    screen("item_equip_result", &["main_dialogue"]),
    screen("item_use_result", &["main_dialogue"]),
    screen("item_transfer_result", &["main_dialogue"]),
    screen("item_discard_result", &["main_dialogue"]),
];

struct ScreenTargetSeed {
    role: &'static str,
    domain_ids: &'static [&'static str],
}

const fn domain(id: &'static str, target_unit: &'static str) -> TranslationDomainSeed {
    TranslationDomainSeed { id, target_unit }
}

const fn screen(role: &'static str, domain_ids: &'static [&'static str]) -> ScreenTargetSeed {
    ScreenTargetSeed { role, domain_ids }
}

pub(crate) fn bind_domain_screen_targets(
    partition: &ScreenTranslationPartition,
) -> Result<Vec<DomainScreenTargets>> {
    let domain_units = DOMAIN_SEEDS
        .iter()
        .map(|domain| (domain.id, domain.target_unit))
        .collect::<BTreeMap<_, _>>();
    ensure!(
        domain_units.len() == DOMAIN_SEEDS.len(),
        "translation coverage contains duplicate domain ids"
    );
    let target_roles = partition
        .japanese_bearing_screens
        .iter()
        .map(|screen| screen.role.as_str())
        .collect::<BTreeSet<_>>();
    let mut mapped_roles = BTreeSet::new();
    let mut domain_roles = DOMAIN_SEEDS
        .iter()
        .map(|domain| (domain.id, BTreeSet::new()))
        .collect::<BTreeMap<_, _>>();
    for seed in SCREEN_TARGETS {
        ensure!(
            target_roles.contains(seed.role),
            "translation coverage maps non-Japanese or unknown screen {}",
            seed.role
        );
        ensure!(
            mapped_roles.insert(seed.role),
            "translation coverage repeats screen {}",
            seed.role
        );
        ensure!(
            !seed.domain_ids.is_empty(),
            "translation coverage screen {} has no domain",
            seed.role
        );
        let mut screen_domains = BTreeSet::new();
        for domain_id in seed.domain_ids {
            ensure!(
                screen_domains.insert(*domain_id),
                "translation coverage screen {} repeats domain {}",
                seed.role,
                domain_id
            );
            domain_roles
                .get_mut(domain_id)
                .with_context(|| format!("screen {} uses unknown domain {domain_id}", seed.role))?
                .insert(seed.role.to_owned());
        }
    }
    ensure!(
        mapped_roles == target_roles,
        "translation coverage does not partition every Japanese-bearing screen"
    );
    DOMAIN_SEEDS
        .iter()
        .map(|seed| {
            let roles = domain_roles
                .remove(seed.id)
                .with_context(|| format!("translation domain {} disappeared", seed.id))?;
            ensure!(
                !roles.is_empty(),
                "translation domain {} has no screen consumer",
                seed.id
            );
            Ok(DomainScreenTargets {
                id: seed.id,
                target_unit: seed.target_unit,
                screen_roles: roles.into_iter().collect(),
            })
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::screen_contracts::inspect_screen_translation_partition;
    use crate::translation_coverage::weapon_shop::{
        CHOICE_LABEL_SCREEN_ROLES, DIALOGUE_SCREEN_ROLES, ITEM_NAME_SCREEN_ROLES,
    };

    #[test]
    fn all_japanese_bearing_screens_have_exactly_one_screen_partition() {
        let partition = inspect_screen_translation_partition().unwrap();
        let domains = bind_domain_screen_targets(&partition).unwrap();

        assert_eq!(partition.screen_count, 45);
        assert_eq!(partition.japanese_bearing_screens.len(), 36);
        assert_eq!(partition.preserved_original_only_screen_count, 5);
        assert_eq!(partition.no_text_screen_count, 4);
        assert_eq!(domains.len(), DOMAIN_SEEDS.len());
    }

    #[test]
    fn weapon_shop_domains_cover_retained_items_and_every_visible_choice_window() {
        let partition = inspect_screen_translation_partition().unwrap();
        let domains = bind_domain_screen_targets(&partition).unwrap();

        for (domain_id, expected_roles) in [
            ("main_dialogue", DIALOGUE_SCREEN_ROLES.as_slice()),
            ("item_names", ITEM_NAME_SCREEN_ROLES.as_slice()),
            ("choice_labels", CHOICE_LABEL_SCREEN_ROLES.as_slice()),
        ] {
            let actual = domains
                .iter()
                .find(|domain| domain.id == domain_id)
                .unwrap()
                .screen_roles
                .iter()
                .filter(|role| role.starts_with("weapon_shop_"))
                .map(String::as_str)
                .collect::<BTreeSet<_>>();
            assert_eq!(
                actual,
                expected_roles.iter().copied().collect::<BTreeSet<_>>()
            );
        }
    }
}
