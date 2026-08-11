use anyhow::{Result, ensure};

use crate::translation_coverage::report::TranslationLifetimeDemandReport;

pub(super) struct InputBindings<'a> {
    pub(super) installed_unique_tile_count: usize,
    pub(super) source_owned_tile_count: usize,
    pub(super) installed_tilemap_cell_count: usize,
    pub(super) installed_runtime_cleared_top_strip_cell_count: usize,
    pub(super) installed_runtime_reasserted_logo_cell_count: usize,
    pub(super) runtime_bound_to_build: bool,
    pub(super) evidence_report_sha1: &'a str,
}

pub(super) fn inspect(bindings: InputBindings<'_>) -> Result<TranslationLifetimeDemandReport> {
    ensure!(
        bindings.installed_unique_tile_count > 0
            && bindings.installed_unique_tile_count <= bindings.source_owned_tile_count
            && bindings.installed_tilemap_cell_count >= bindings.installed_unique_tile_count
            && bindings.installed_runtime_cleared_top_strip_cell_count == 26
            && bindings.installed_runtime_reasserted_logo_cell_count == 11
            && bindings.runtime_bound_to_build
            && !bindings.evidence_report_sha1.is_empty(),
        "installed title-graphics lifetime is incomplete or exceeds its source-owned tile budget"
    );
    Ok(TranslationLifetimeDemandReport {
        screen_role: "title",
        measurement_basis: "installed unique logo patterns within the source-owned title-tile budget, including the completed-phase top-strip clearing and logo-cell reassertion",
        target_glyph_count: bindings.installed_unique_tile_count,
        preserved_active_source_code_count: 0,
        additional_target_glyph_reservation_count: 0,
        total_slot_demand: bindings.installed_unique_tile_count,
        active_slot_count: bindings.source_owned_tile_count,
        fits_active_page: true,
        evidence_report_sha1: bindings.evidence_report_sha1.to_owned(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn installed_logo_patterns_must_fit_the_source_owned_title_budget() {
        let demand = inspect(InputBindings {
            installed_unique_tile_count: 117,
            source_owned_tile_count: 121,
            installed_tilemap_cell_count: 134,
            installed_runtime_cleared_top_strip_cell_count: 26,
            installed_runtime_reasserted_logo_cell_count: 11,
            runtime_bound_to_build: true,
            evidence_report_sha1: "build-report",
        })
        .unwrap();

        assert_eq!(demand.screen_role, "title");
        assert_eq!(demand.total_slot_demand, 117);
        assert_eq!(demand.active_slot_count, 121);
        assert!(demand.fits_active_page);

        assert!(
            inspect(InputBindings {
                installed_unique_tile_count: 122,
                source_owned_tile_count: 121,
                installed_tilemap_cell_count: 134,
                installed_runtime_cleared_top_strip_cell_count: 26,
                installed_runtime_reasserted_logo_cell_count: 11,
                runtime_bound_to_build: true,
                evidence_report_sha1: "build-report",
            })
            .is_err()
        );
    }
}
