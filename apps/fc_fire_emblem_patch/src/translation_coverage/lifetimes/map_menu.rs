use std::path::Path;

use anyhow::{Context, Result, ensure};
use serde::Serialize;

use crate::{
    font_slots::ACTIVE_HANGUL_SLOT_COUNT,
    map_menu::plan_map_menu,
    rom::{EXPECTED_SOURCE_SHA1, Rom},
    sha1_hex,
};

use super::super::report::TranslationLifetimeDemandReport;
use super::full_page_bound;

pub(super) struct InputBindings<'a> {
    pub(super) source_path: &'a Path,
    pub(super) localization_path: &'a Path,
    pub(super) localization_sha1: &'a str,
}

#[derive(Serialize)]
struct EvidenceDigest<'a> {
    schema: u8,
    source_sha1: &'static str,
    localization_sha1: &'a str,
    screen_role: &'static str,
    source_reclaimable_active_code_count: usize,
    target_glyph_count: usize,
    preservation_policy: &'static str,
}

pub(super) fn inspect(bindings: InputBindings<'_>) -> Result<TranslationLifetimeDemandReport> {
    let rom = Rom::from_path(bindings.source_path)?;
    rom.verify_supported_japanese()?;
    let plan = plan_map_menu(&rom, bindings.localization_path)?;
    ensure!(
        plan.workspace_sha1 == bindings.localization_sha1
            && plan.entry_count == 6
            && plan.translated_entry_count == 6,
        "map-menu lifetime translation input changed"
    );
    let bound = full_page_bound::measure(
        &plan.target_glyphs,
        &plan.source_reclaimable_active_codes,
        "map_menu",
    )?;
    let evidence = EvidenceDigest {
        schema: 1,
        source_sha1: EXPECTED_SOURCE_SHA1,
        localization_sha1: bindings.localization_sha1,
        screen_role: "map_menu",
        source_reclaimable_active_code_count: bound.source_reclaimable_active_code_count,
        target_glyph_count: bound.target_glyph_count,
        preservation_policy: "preserve every active code except the exact six source-label code union",
    };
    let evidence_bytes = serde_json::to_vec(&evidence).context("serialize map-menu evidence")?;

    Ok(TranslationLifetimeDemandReport {
        screen_role: "map_menu",
        measurement_basis: "full-page upper bound preserving every active code except the exact six source-label code union",
        target_glyph_count: bound.target_glyph_count,
        preserved_active_source_code_count: bound.preserved_active_source_code_count,
        additional_target_glyph_reservation_count: 0,
        total_slot_demand: bound.total_slot_demand,
        active_slot_count: ACTIVE_HANGUL_SLOT_COUNT,
        fits_active_page: true,
        evidence_report_sha1: sha1_hex(&evidence_bytes),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn complete_map_menu_fits_without_screen_specific_preservation_assumptions() {
        let source = Path::new(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../../roms/Fire Emblem - Ankoku Ryuu to Hikari no Tsurugi (Japan).nes"
        ));
        if !source.exists() {
            return;
        }
        let localization = Path::new(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../../assets/translation/map-menu.ko.json"
        ));
        let localization_sha1 = sha1_hex(&std::fs::read(localization).unwrap());
        let demand = inspect(InputBindings {
            source_path: source,
            localization_path: localization,
            localization_sha1: &localization_sha1,
        })
        .unwrap();

        assert_eq!(demand.target_glyph_count, 17);
        assert_eq!(demand.preserved_active_source_code_count, 186);
        assert_eq!(demand.total_slot_demand, 203);
    }
}
