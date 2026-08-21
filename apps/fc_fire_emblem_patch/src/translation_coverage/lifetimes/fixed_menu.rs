use std::collections::BTreeSet;

use anyhow::{Context, Result, ensure};

use crate::{
    fixed_menu_labels::fixed_menu_screen_roles,
    font_slots::ACTIVE_HANGUL_SLOT_COUNT,
    translation_coverage::installed::{InstalledFixedMenuLifetime, InstalledLifetimeDemand},
};

use super::super::report::TranslationLifetimeDemandReport;

pub(super) fn inspect(
    installed: &InstalledFixedMenuLifetime,
) -> Result<Vec<TranslationLifetimeDemandReport>> {
    let installed_roles = installed
        .screen_roles
        .iter()
        .map(String::as_str)
        .collect::<BTreeSet<_>>();
    let expected_roles = fixed_menu_screen_roles()
        .iter()
        .copied()
        .collect::<BTreeSet<_>>();
    ensure!(
        installed_roles == expected_roles,
        "installed fixed-menu lifetime does not cover its complete screen family"
    );
    ensure!(
        !installed.evidence_report_sha1.is_empty(),
        "fixed-menu lifetime has no exact-final report identity"
    );
    validate_demand(
        "fixed-menu shared static page",
        &installed.shared_static_page,
    )?;
    validate_demand(
        "unit-selection help dialogue handoff",
        &installed.unit_selection_help_dialogue,
    )?;
    validate_demand("storage dialogue overlay", &installed.storage_dialogue)?;

    fixed_menu_screen_roles()
        .iter()
        .map(|screen_role| {
            let screen_role = *screen_role;
            let (demand, measurement_basis) = match screen_role {
                "unit_selection" => larger_unit_selection_demand(installed),
                "game_speed_selection" => (
                    &installed.shared_static_page,
                    "installed shared fixed-menu page including the still-visible options parent",
                ),
                "storage_action_menu" | "storage_overflow_action" => (
                    &installed.storage_dialogue,
                    "largest installed storage dialogue page with its fixed labels and canonical item-name overlay",
                ),
                "storage_capacity_notice" => (
                    &installed.shared_static_page,
                    "installed shared fixed-menu page for the standalone storage-capacity notice",
                ),
                other => anyhow::bail!("unknown fixed-menu screen role {other}"),
            };
            Ok(TranslationLifetimeDemandReport {
                screen_role,
                measurement_basis,
                target_glyph_count: demand.target_glyph_count,
                preserved_active_source_code_count: demand.preserved_active_code_count,
                additional_target_glyph_reservation_count: 0,
                total_slot_demand: demand.total_slot_demand,
                active_slot_count: ACTIVE_HANGUL_SLOT_COUNT,
                fits_active_page: true,
                evidence_report_sha1: installed.evidence_report_sha1.clone(),
            })
        })
        .collect()
}

fn larger_unit_selection_demand(
    installed: &InstalledFixedMenuLifetime,
) -> (&InstalledLifetimeDemand, &'static str) {
    if installed.unit_selection_help_dialogue.total_slot_demand
        >= installed.shared_static_page.total_slot_demand
    {
        (
            &installed.unit_selection_help_dialogue,
            "larger of the installed unit-selection static page and the source-bound state-25 help/dialogue handoff",
        )
    } else {
        (
            &installed.shared_static_page,
            "larger of the installed unit-selection static page and the source-bound state-25 help/dialogue handoff",
        )
    }
}

fn validate_demand(role: &str, demand: &InstalledLifetimeDemand) -> Result<()> {
    let component_total = demand
        .target_glyph_count
        .checked_add(demand.preserved_active_code_count)
        .with_context(|| format!("{role} slot demand overflow"))?;
    ensure!(
        demand.target_glyph_count > 0
            && component_total == demand.total_slot_demand
            && demand.total_slot_demand <= ACTIVE_HANGUL_SLOT_COUNT,
        "{role} components or active-page fit changed"
    );
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn demand(target: usize, preserved: usize) -> InstalledLifetimeDemand {
        InstalledLifetimeDemand {
            target_glyph_count: target,
            preserved_active_code_count: preserved,
            total_slot_demand: target + preserved,
        }
    }

    fn installed() -> InstalledFixedMenuLifetime {
        InstalledFixedMenuLifetime {
            screen_roles: fixed_menu_screen_roles()
                .iter()
                .map(|role| (*role).to_owned())
                .collect(),
            shared_static_page: demand(86, 0),
            unit_selection_help_dialogue: demand(155, 40),
            storage_dialogue: demand(143, 52),
            evidence_report_sha1: "exact-final-report".to_owned(),
        }
    }

    #[test]
    fn one_installed_family_measures_all_five_fixed_menu_roles() {
        let demands = inspect(&installed()).unwrap();
        let by_role = demands
            .iter()
            .map(|demand| (demand.screen_role, demand.total_slot_demand))
            .collect::<std::collections::BTreeMap<_, _>>();

        assert_eq!(
            by_role.keys().copied().collect::<BTreeSet<_>>(),
            fixed_menu_screen_roles()
                .iter()
                .copied()
                .collect::<BTreeSet<_>>()
        );
        assert_eq!(by_role["game_speed_selection"], 86);
        assert_eq!(by_role["storage_capacity_notice"], 86);
        assert_eq!(by_role["unit_selection"], 195);
        assert_eq!(by_role["storage_action_menu"], 195);
        assert_eq!(by_role["storage_overflow_action"], 195);
    }

    #[test]
    fn unit_selection_uses_the_larger_parent_or_dialogue_handoff() {
        let mut installation = installed();
        installation.shared_static_page = demand(200, 0);

        let demands = inspect(&installation).unwrap();
        let unit_selection = demands
            .iter()
            .find(|demand| demand.screen_role == "unit_selection")
            .unwrap();
        assert_eq!(unit_selection.total_slot_demand, 200);
    }

    #[test]
    fn missing_role_or_invalid_capacity_cannot_close_the_family() {
        let mut missing_role = installed();
        missing_role
            .screen_roles
            .retain(|role| role != "storage_capacity_notice");
        assert!(inspect(&missing_role).is_err());

        let mut invalid_capacity = installed();
        invalid_capacity.storage_dialogue.total_slot_demand = ACTIVE_HANGUL_SLOT_COUNT + 1;
        assert!(inspect(&invalid_capacity).is_err());
    }
}
