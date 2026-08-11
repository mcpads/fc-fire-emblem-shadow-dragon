use anyhow::{Context, Result, ensure};

use crate::font_slots::ACTIVE_HANGUL_SLOT_COUNT;

use super::super::report::TranslationLifetimeDemandReport;

pub(super) struct InputBindings<'a> {
    pub(super) page_target_glyph_count: usize,
    pub(super) page_preserved_active_code_count: usize,
    pub(super) page_total_slot_demand: usize,
    pub(super) evidence_report_sha1: &'a str,
}

pub(super) fn inspect(bindings: InputBindings<'_>) -> Result<TranslationLifetimeDemandReport> {
    let component_total = bindings
        .page_target_glyph_count
        .checked_add(bindings.page_preserved_active_code_count)
        .context("unit-roster page lifetime demand overflow")?;
    ensure!(
        bindings.page_target_glyph_count > 0
            && bindings.page_preserved_active_code_count > 0
            && component_total == bindings.page_total_slot_demand
            && component_total <= ACTIVE_HANGUL_SLOT_COUNT,
        "unit-roster page lifetime components or fit result changed"
    );

    Ok(TranslationLifetimeDemandReport {
        screen_role: "unit_roster",
        measurement_basis: "installed roster header and complete playable-unit name page with preserved active codes",
        target_glyph_count: bindings.page_target_glyph_count,
        preserved_active_source_code_count: bindings.page_preserved_active_code_count,
        additional_target_glyph_reservation_count: 0,
        total_slot_demand: bindings.page_total_slot_demand,
        active_slot_count: ACTIVE_HANGUL_SLOT_COUNT,
        fits_active_page: true,
        evidence_report_sha1: bindings.evidence_report_sha1.to_owned(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn installed_roster_page_counts_target_and_preserved_codes_once() {
        let demand = inspect(InputBindings {
            page_target_glyph_count: 72,
            page_preserved_active_code_count: 18,
            page_total_slot_demand: 90,
            evidence_report_sha1: "build-report",
        })
        .unwrap();

        assert_eq!(demand.screen_role, "unit_roster");
        assert_eq!(demand.total_slot_demand, 90);
        assert!(demand.fits_active_page);
    }
}
