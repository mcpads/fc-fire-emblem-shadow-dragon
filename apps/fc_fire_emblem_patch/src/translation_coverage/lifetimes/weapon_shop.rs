use std::collections::BTreeSet;

use anyhow::{Context, Result, ensure};

use crate::font_slots::ACTIVE_HANGUL_SLOT_COUNT;

use super::super::{report::TranslationLifetimeDemandReport, weapon_shop::SCREEN_ROLES};

pub(super) struct InputBindings<'a> {
    pub(super) shared_page_target_glyph_count: usize,
    pub(super) shared_page_preserved_active_code_count: usize,
    pub(super) shared_page_total_slot_demand: usize,
    pub(super) capacity_bound_screen_roles: &'a [String],
    pub(super) evidence_report_sha1: &'a str,
}

pub(super) fn inspect(bindings: InputBindings<'_>) -> Result<Vec<TranslationLifetimeDemandReport>> {
    let bound_roles = bindings
        .capacity_bound_screen_roles
        .iter()
        .map(String::as_str)
        .collect::<BTreeSet<_>>();
    ensure!(
        bound_roles == SCREEN_ROLES.into_iter().collect::<BTreeSet<_>>(),
        "weapon-shop shared-page capacity is not bound to all nine screen roles"
    );
    let component_total = bindings
        .shared_page_target_glyph_count
        .checked_add(bindings.shared_page_preserved_active_code_count)
        .context("weapon-shop shared-page lifetime demand overflow")?;
    ensure!(
        bindings.shared_page_target_glyph_count > 0
            && bindings.shared_page_preserved_active_code_count > 0
            && component_total == bindings.shared_page_total_slot_demand
            && component_total <= ACTIVE_HANGUL_SLOT_COUNT,
        "weapon-shop shared-page lifetime components or fit result changed"
    );

    Ok(SCREEN_ROLES
        .into_iter()
        .map(|screen_role| TranslationLifetimeDemandReport {
            screen_role,
            measurement_basis: "conservative installed shared-page working set covering every weapon-shop state",
            target_glyph_count: bindings.shared_page_target_glyph_count,
            preserved_active_source_code_count: bindings
                .shared_page_preserved_active_code_count,
            additional_target_glyph_reservation_count: 0,
            total_slot_demand: bindings.shared_page_total_slot_demand,
            active_slot_count: ACTIVE_HANGUL_SLOT_COUNT,
            fits_active_page: true,
            evidence_report_sha1: bindings.evidence_report_sha1.to_owned(),
        })
        .collect())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn one_installed_shared_page_bounds_every_weapon_shop_screen() {
        let roles = SCREEN_ROLES.map(str::to_owned);
        let demands = inspect(InputBindings {
            shared_page_target_glyph_count: 60,
            shared_page_preserved_active_code_count: 90,
            shared_page_total_slot_demand: 150,
            capacity_bound_screen_roles: &roles,
            evidence_report_sha1: "build-report",
        })
        .unwrap();

        assert_eq!(demands.len(), 9);
        assert!(demands.iter().all(|demand| demand.total_slot_demand == 150));
        assert!(demands.iter().all(|demand| demand.fits_active_page));
    }
}
