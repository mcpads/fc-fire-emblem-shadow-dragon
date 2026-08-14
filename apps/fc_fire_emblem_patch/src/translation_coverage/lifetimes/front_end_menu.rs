use anyhow::{Context, Result, ensure};

use crate::font_slots::ACTIVE_HANGUL_SLOT_COUNT;

use super::super::report::TranslationLifetimeDemandReport;

pub(super) struct InputBindings<'a> {
    pub(super) target_glyph_count: usize,
    pub(super) preserved_active_code_count: usize,
    pub(super) no_save_source_lifetime_bound: bool,
    pub(super) save_slot_selection_source_lifetime_bound: bool,
    pub(super) evidence_report_sha1: &'a str,
}

pub(super) fn inspect(bindings: InputBindings<'_>) -> Result<Vec<TranslationLifetimeDemandReport>> {
    ensure!(
        bindings.target_glyph_count > 0
            && bindings.preserved_active_code_count > 0
            && bindings.no_save_source_lifetime_bound
            && bindings.save_slot_selection_source_lifetime_bound,
        "front-end menu lifetimes are not bound to both source screens"
    );
    let total_slot_demand = bindings
        .target_glyph_count
        .checked_add(bindings.preserved_active_code_count)
        .context("front-end menu lifetime slot demand overflow")?;
    ensure!(
        total_slot_demand <= ACTIVE_HANGUL_SLOT_COUNT,
        "front-end menu lifetime exceeds the active Hangul page"
    );
    Ok([
        (
            "new_game_choice",
            "installed seven-entry front-end glyph set plus its frozen no-save temporal screen union",
        ),
        (
            "save_slot_selection",
            "same installed front-end glyph set plus the source-bound state-21 save-slot producer",
        ),
    ]
    .into_iter()
    .map(|(screen_role, measurement_basis)| TranslationLifetimeDemandReport {
        screen_role,
        measurement_basis,
        target_glyph_count: bindings.target_glyph_count,
        preserved_active_source_code_count: bindings.preserved_active_code_count,
        additional_target_glyph_reservation_count: 0,
        total_slot_demand,
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
    fn measures_the_complete_no_save_front_end_menu() {
        let demands = inspect(InputBindings {
            target_glyph_count: 15,
            preserved_active_code_count: 12,
            no_save_source_lifetime_bound: true,
            save_slot_selection_source_lifetime_bound: true,
            evidence_report_sha1: "build-report",
        })
        .unwrap();

        assert_eq!(
            demands
                .iter()
                .map(|demand| demand.screen_role)
                .collect::<Vec<_>>(),
            ["new_game_choice", "save_slot_selection"]
        );
        assert!(demands.iter().all(|demand| demand.total_slot_demand == 27));
    }
}
