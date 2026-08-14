use super::*;

#[test]
fn global_dialogue_plan_advances_only_proven_cross_domain_consumers() {
    let targets = inspect_domain_screen_targets()
        .unwrap()
        .into_iter()
        .map(|domain| (domain.id, domain.screen_roles))
        .collect::<BTreeMap<_, _>>();
    let required = [
        "chapter_save_offer_label",
        "chapter_titles",
        "choice_labels",
        "class_names",
        "ending_record_labels",
        "enemy_names",
        "item_action_labels",
        "item_names",
        "location_names",
        "main_dialogue",
        "map_menu_labels",
        "unit_names",
        "unit_ui_labels",
    ];
    let target_unit_counts = BTreeMap::from([
        ("chapter_save_offer_label", 1),
        ("chapter_titles", 25),
        ("choice_labels", 2),
        ("class_names", 22),
        ("ending_record_labels", 1),
        ("enemy_names", 69),
        ("item_action_labels", 4),
        ("item_names", 91),
        ("location_names", 24),
        ("main_dialogue", 2_541),
        ("map_menu_labels", 6),
        ("unit_names", 53),
        ("unit_ui_labels", 25),
    ]);
    let current = BTreeMap::from([
        (
            "chapter_titles",
            installation_with_complete_screens(2, &["chapter_intro_title_dialogue_composite"], &[]),
        ),
        (
            "unit_names",
            installation_with_complete_screens(
                52,
                &[
                    "battle_animation",
                    "unit_roster",
                    "unit_status",
                    "unit_summary",
                ],
                &[],
            ),
        ),
        (
            "item_names",
            installation_with_complete_screens(64, &["battle_animation"], &["battle_animation"]),
        ),
    ]);
    let additional_global_roles = BTreeMap::new();

    let domains = assemble_domain_consumers(
        &required,
        &target_unit_counts,
        &targets,
        &current,
        true,
        true,
        true,
        &additional_global_roles,
    )
    .unwrap();
    let by_id = domains
        .iter()
        .map(|domain| (domain.id, domain))
        .collect::<BTreeMap<_, _>>();

    assert!(by_id["main_dialogue"].all_consumers_statically_accounted);
    assert!(!by_id["unit_names"].all_consumers_statically_accounted);
    assert!(by_id["location_names"].all_consumers_statically_accounted);
    assert_eq!(
        by_id["unit_names"].newly_planned_screen_roles,
        [ENDING_CHARACTER_EPILOGUE]
    );
    assert!(by_id["item_names"].globally_planned_screen_roles.is_empty());
    assert!(
        !by_id["item_names"]
            .remaining_screen_roles
            .iter()
            .any(|role| role == "battle_animation")
    );
    assert!(
        by_id["item_names"]
            .remaining_screen_roles
            .iter()
            .any(|role| role == "item_inventory_list")
    );
    assert_eq!(
        domains
            .iter()
            .filter(|domain| domain.all_consumers_statically_accounted)
            .count(),
        2
    );
}

fn installation_with_complete_screens(
    installed_target_unit_count: usize,
    installed_screen_roles: &[&str],
    consumer_complete_screen_roles: &[&str],
) -> DomainInstallation {
    DomainInstallation {
        installed_target_unit_count,
        installed_screen_roles: installed_screen_roles
            .iter()
            .map(|role| (*role).to_owned())
            .collect(),
        consumer_complete_screen_roles: consumer_complete_screen_roles
            .iter()
            .map(|role| (*role).to_owned())
            .collect(),
        runtime_bound_screen_roles: Vec::new(),
    }
}

#[test]
fn exact_additional_consumer_roles_close_only_the_named_domain_screens() {
    let required = ["map_menu_labels"];
    let target_unit_counts = BTreeMap::from([("map_menu_labels", 6)]);
    let targets = BTreeMap::from([("map_menu_labels", vec!["map_menu".to_owned()])]);
    let current = BTreeMap::new();
    let additional = BTreeMap::from([("map_menu_labels", BTreeSet::from(["map_menu".to_owned()]))]);

    let domains = assemble_domain_consumers(
        &required,
        &target_unit_counts,
        &targets,
        &current,
        false,
        false,
        false,
        &additional,
    )
    .unwrap();

    assert_eq!(domains[0].globally_planned_target_unit_count, 6);
    assert_eq!(domains[0].globally_planned_screen_roles, ["map_menu"]);
    assert!(domains[0].all_consumers_statically_accounted);
}
