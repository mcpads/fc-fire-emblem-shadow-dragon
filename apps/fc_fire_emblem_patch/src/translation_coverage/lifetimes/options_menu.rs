use anyhow::{Result, ensure};

use crate::font_slots::ACTIVE_HANGUL_SLOT_COUNT;

use super::super::report::TranslationLifetimeDemandReport;

pub(super) struct InputBindings<'a> {
    pub(super) target_glyph_count: usize,
    pub(super) preserved_active_code_count: usize,
    pub(super) total_slot_demand: usize,
    pub(super) capacity_bound_to_build: bool,
    pub(super) evidence_report_sha1: &'a str,
}

pub(super) fn inspect(bindings: InputBindings<'_>) -> Result<TranslationLifetimeDemandReport> {
    ensure!(
        bindings.target_glyph_count > 0
            && bindings.preserved_active_code_count > 0
            && bindings.total_slot_demand
                == bindings.target_glyph_count + bindings.preserved_active_code_count
            && bindings.capacity_bound_to_build,
        "options-menu lifetime is not bound to the current build"
    );
    ensure!(
        bindings.total_slot_demand <= ACTIVE_HANGUL_SLOT_COUNT,
        "options-menu lifetime exceeds the active Hangul page"
    );
    Ok(TranslationLifetimeDemandReport {
        screen_role: "options",
        measurement_basis: "installed three-label glyph set plus the frozen full-screen active-code union across both selected rows",
        target_glyph_count: bindings.target_glyph_count,
        preserved_active_source_code_count: bindings.preserved_active_code_count,
        additional_target_glyph_reservation_count: 0,
        total_slot_demand: bindings.total_slot_demand,
        active_slot_count: ACTIVE_HANGUL_SLOT_COUNT,
        fits_active_page: true,
        evidence_report_sha1: bindings.evidence_report_sha1.to_owned(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn measures_the_installed_options_screen() {
        let demand = inspect(InputBindings {
            target_glyph_count: 12,
            preserved_active_code_count: 78,
            total_slot_demand: 90,
            capacity_bound_to_build: true,
            evidence_report_sha1: "build-report",
        })
        .unwrap();

        assert_eq!(demand.screen_role, "options");
        assert_eq!(demand.total_slot_demand, 90);
    }
}
