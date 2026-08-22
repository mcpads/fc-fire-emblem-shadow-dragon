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
        ("map_menu_labels", 8),
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

    let domains = assemble_domain_consumers(DomainConsumerAssemblyInputs {
        required_domains: &required,
        target_unit_counts: &target_unit_counts,
        targets: &targets,
        current: &current,
        all_chapter_titles_encoded: true,
        global_dialogue_runtime_planned: true,
        dynamic_dialogue_producers_bound: true,
        additional_globally_planned_roles: &additional_global_roles,
    })
    .unwrap();
    let by_id = domains
        .iter()
        .map(|domain| (domain.id, domain))
        .collect::<BTreeMap<_, _>>();

    assert!(by_id["main_dialogue"].all_declared_consumers_statically_accounted);
    assert!(!by_id["unit_names"].all_declared_consumers_statically_accounted);
    assert!(by_id["location_names"].all_declared_consumers_statically_accounted);
    assert_eq!(
        by_id["unit_names"].newly_planned_declared_screen_roles,
        [ENDING_CHARACTER_EPILOGUE]
    );
    assert!(
        by_id["item_names"]
            .globally_planned_declared_screen_roles
            .is_empty()
    );
    assert!(
        !by_id["item_names"]
            .unaccounted_declared_screen_roles
            .iter()
            .any(|role| role == "battle_animation")
    );
    assert!(
        by_id["item_names"]
            .unaccounted_declared_screen_roles
            .iter()
            .any(|role| role == "item_inventory_list")
    );
    assert_eq!(
        domains
            .iter()
            .filter(|domain| domain.all_declared_consumers_statically_accounted)
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
    let target_unit_counts = BTreeMap::from([("map_menu_labels", 8)]);
    let targets = BTreeMap::from([(
        "map_menu_labels",
        vec!["map_funds_summary".to_owned(), "map_menu".to_owned()],
    )]);
    let current = BTreeMap::new();
    let additional = BTreeMap::from([(
        "map_menu_labels",
        BTreeSet::from(["map_funds_summary".to_owned(), "map_menu".to_owned()]),
    )]);

    let domains = assemble_domain_consumers(DomainConsumerAssemblyInputs {
        required_domains: &required,
        target_unit_counts: &target_unit_counts,
        targets: &targets,
        current: &current,
        all_chapter_titles_encoded: false,
        global_dialogue_runtime_planned: false,
        dynamic_dialogue_producers_bound: false,
        additional_globally_planned_roles: &additional,
    })
    .unwrap();

    assert_eq!(domains[0].globally_planned_target_unit_count, 8);
    assert_eq!(
        domains[0].globally_planned_declared_screen_roles,
        ["map_funds_summary", "map_menu"]
    );
    assert!(domains[0].all_declared_consumers_statically_accounted);
}

#[test]
fn declared_runtime_roles_bind_to_every_domain_that_declares_the_screen() {
    let mut plan = ConsumerInstallationPlan {
        strategy: "test",
        current_candidate_sha1: "candidate".to_owned(),
        current_build_report_sha1: "report".to_owned(),
        declared_domain_count: 2,
        domains: vec![
            DomainConsumerInstallation {
                id: "main_dialogue",
                target_unit_count: 1,
                current_candidate_installed_target_unit_count: 1,
                globally_planned_target_unit_count: 1,
                declared_screen_roles: vec!["shared_screen".to_owned()],
                current_candidate_carried_declared_screen_roles: vec![],
                globally_planned_declared_screen_roles: vec!["shared_screen".to_owned()],
                newly_planned_declared_screen_roles: vec!["shared_screen".to_owned()],
                statically_accounted_declared_screen_roles: vec!["shared_screen".to_owned()],
                unaccounted_declared_screen_roles: vec![],
                current_candidate_historical_declared_runtime_roles: vec![],
                runtime_observed_declared_screen_roles: vec![],
                all_declared_consumers_statically_accounted: true,
            },
            DomainConsumerInstallation {
                id: "choice_labels",
                target_unit_count: 1,
                current_candidate_installed_target_unit_count: 1,
                globally_planned_target_unit_count: 1,
                declared_screen_roles: vec!["shared_screen".to_owned()],
                current_candidate_carried_declared_screen_roles: vec![],
                globally_planned_declared_screen_roles: vec!["shared_screen".to_owned()],
                newly_planned_declared_screen_roles: vec!["shared_screen".to_owned()],
                statically_accounted_declared_screen_roles: vec!["shared_screen".to_owned()],
                unaccounted_declared_screen_roles: vec![],
                current_candidate_historical_declared_runtime_roles: vec![],
                runtime_observed_declared_screen_roles: vec![],
                all_declared_consumers_statically_accounted: true,
            },
        ],
        declared_domain_with_carried_consumers_count: 0,
        declared_domain_with_global_plan_count: 2,
        statically_accounted_declared_domain_count: 2,
        declared_domain_with_unaccounted_consumers_count: 0,
        declared_consumer_historical_runtime_role_count: 0,
        declared_consumer_runtime_observed_role_count: 0,
        all_declared_consumers_statically_accounted: true,
        current_candidate_runtime_evidence_inherited: false,
        declared_consumer_runtime_replay_required: true,
    };

    let roles = BTreeSet::from(["shared_screen".to_owned()]);
    plan.bind_declared_consumer_runtime_roles(&roles, &roles)
        .unwrap();

    assert_eq!(plan.declared_consumer_runtime_observed_role_count, 1);
    assert!(!plan.declared_consumer_runtime_replay_required);
    assert!(
        plan.domains
            .iter()
            .all(|domain| { domain.runtime_observed_declared_screen_roles == ["shared_screen"] })
    );
}

#[test]
fn unknown_declared_consumer_runtime_role_is_rejected() {
    let mut plan = ConsumerInstallationPlan {
        strategy: "test",
        current_candidate_sha1: "candidate".to_owned(),
        current_build_report_sha1: "report".to_owned(),
        declared_domain_count: 0,
        domains: vec![],
        declared_domain_with_carried_consumers_count: 0,
        declared_domain_with_global_plan_count: 0,
        statically_accounted_declared_domain_count: 0,
        declared_domain_with_unaccounted_consumers_count: 0,
        declared_consumer_historical_runtime_role_count: 0,
        declared_consumer_runtime_observed_role_count: 0,
        all_declared_consumers_statically_accounted: true,
        current_candidate_runtime_evidence_inherited: false,
        declared_consumer_runtime_replay_required: true,
    };

    let error = plan
        .bind_declared_consumer_runtime_roles(
            &BTreeSet::from(["unknown".to_owned()]),
            &BTreeSet::new(),
        )
        .err()
        .unwrap();
    assert!(
        error
            .to_string()
            .contains("unknown translation screen roles")
    );
}
