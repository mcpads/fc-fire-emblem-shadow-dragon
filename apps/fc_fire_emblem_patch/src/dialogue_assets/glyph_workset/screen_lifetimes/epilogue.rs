use std::collections::{BTreeMap, BTreeSet};

use anyhow::{Result, ensure};

use crate::{
    dialogue_inventory::MainDialogueGraphReport,
    font_slots::active_hangul_codes,
};

use super::{
    DialogueRecordKey, ObservedScreenLifetimeReport, maximum_transition_chain_glyph_union,
};

const SCREEN_ROLE: &str = "ending character epilogue family";
const TABLE_IDS: [&str; 2] = ["epilogue-dialogue", "epilogue-routing-dialogue"];
const OBSERVED_NAMETABLE_ACTIVE_CODES: [u8; 99] = [
    0x00, 0x01, 0x02, 0x03, 0x04, 0x05, 0x06, 0x07, 0x08, 0x09, 0x0A, 0x0B, 0x0C, 0x0D, 0x0E,
    0x10, 0x11, 0x12, 0x13, 0x14, 0x15, 0x16, 0x18, 0x19, 0x1A, 0x1B, 0x1C, 0x1D, 0x20, 0x21,
    0x22, 0x23, 0x24, 0x25, 0x27, 0x28, 0x29, 0x2A, 0x2B, 0x2C, 0x2D, 0x2E, 0x2F, 0x30, 0x31,
    0x32, 0x33, 0x34, 0x35, 0x36, 0x37, 0x38, 0x39, 0x3A, 0x3B, 0x3C, 0x3D, 0x3E, 0x3F, 0x40,
    0x41, 0x42, 0x43, 0x44, 0x45, 0x46, 0x47, 0x48, 0x4A, 0x4B, 0x4C, 0x4D, 0x4E, 0x50, 0x51,
    0x52, 0x58, 0x59, 0x5A, 0x5B, 0x5C, 0x5F, 0x84, 0x85, 0x86, 0x87, 0x88, 0x89, 0x8A, 0x8B,
    0xA7, 0xA8, 0xA9, 0xF5, 0xF6, 0xF7, 0xF8, 0xF9, 0xFA,
];
const CHARACTER_NAME_DISPLAY_CELL_LIMIT: usize = 7;
const LOCATION_NAME_DISPLAY_CELL_LIMIT: usize = 11;

pub(super) fn ending_character_family_report(
    filled_glyphs_by_record: &BTreeMap<DialogueRecordKey, BTreeSet<char>>,
    approved_glyphs_by_record: &BTreeMap<DialogueRecordKey, BTreeSet<char>>,
    graph: &MainDialogueGraphReport,
    active_slot_count: usize,
    working_set_ready: bool,
) -> Result<Option<ObservedScreenLifetimeReport>> {
    let table_is_present = filled_glyphs_by_record
        .keys()
        .any(|(table_id, _)| TABLE_IDS.contains(&table_id.as_str()));
    if !table_is_present {
        return Ok(None);
    }

    let active_codes = active_hangul_codes().into_iter().collect::<BTreeSet<_>>();
    let observed_active_codes = OBSERVED_NAMETABLE_ACTIVE_CODES
        .into_iter()
        .collect::<BTreeSet<_>>();
    ensure!(
        observed_active_codes.len() == OBSERVED_NAMETABLE_ACTIVE_CODES.len(),
        "{SCREEN_ROLE} observed source codes contain duplicates"
    );
    ensure!(
        observed_active_codes.is_subset(&active_codes),
        "{SCREEN_ROLE} observed source codes include a reserved font slot"
    );

    let (source_record_count, filled_glyphs) =
        maximum_transition_chain_glyph_union(&TABLE_IDS, filled_glyphs_by_record, graph)?;
    let (_, approved_glyphs) =
        maximum_transition_chain_glyph_union(&TABLE_IDS, approved_glyphs_by_record, graph)?;
    let preserved_active_source_code_count = observed_active_codes.len();
    let additional_target_glyph_reservation_count =
        CHARACTER_NAME_DISPLAY_CELL_LIMIT + LOCATION_NAME_DISPLAY_CELL_LIMIT;
    let filled_slot_demand = preserved_active_source_code_count
        + additional_target_glyph_reservation_count
        + filled_glyphs.len();
    let approved_slot_demand = working_set_ready.then_some(
        preserved_active_source_code_count
            + additional_target_glyph_reservation_count
            + approved_glyphs.len(),
    );

    Ok(Some(ObservedScreenLifetimeReport {
        screen_role: SCREEN_ROLE,
        budget_basis: "conservative union of all 560 observed nametables, the maximum visible dialogue transition chain, and full character/location display-cell reservations",
        evidence_digest: "sha1:71546fe01803a13a5340c68334111bfa9f13b443",
        source_record_count,
        filled_unique_glyph_count: filled_glyphs.len(),
        preserved_active_source_code_count,
        additional_target_glyph_reservation_count,
        filled_slot_demand,
        filled_set_fits_one_page_so_far: filled_slot_demand <= active_slot_count,
        approved_unique_glyph_count: approved_glyphs.len(),
        approved_slot_demand,
        approved_set_fits_one_page: approved_slot_demand
            .map(|slot_demand| slot_demand <= active_slot_count),
    }))
}
