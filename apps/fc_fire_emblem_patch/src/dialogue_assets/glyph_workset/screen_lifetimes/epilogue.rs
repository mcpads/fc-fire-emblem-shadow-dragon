use std::collections::{BTreeMap, BTreeSet};

use anyhow::{Result, ensure};

use crate::{dialogue_inventory::MainDialogueGraphReport, font_slots::active_hangul_codes};

use super::{
    DialogueRecordKey, ObservedScreenLifetimeReport, maximum_transition_chain_glyph_union,
};

const SCREEN_ROLE: &str = "ending character epilogue family";
const TABLE_IDS: [&str; 2] = ["epilogue-dialogue", "epilogue-routing-dialogue"];
// The 560 bound ending samples use physical nametable 0, rows 17..26 and columns
// 9..26 as the dialogue interior. Those cells are owned by the translated literal and
// dynamic-name renderer, so their Japanese tile codes are replaceable, not background
// residency. Outside that interior the only active-font codes are the six window-border
// tiles below; portrait/background codes live in globally protected slots.
const OBSERVED_NAMETABLE_ACTIVE_CODES_OUTSIDE_DIALOGUE_INTERIOR: [u8; 6] =
    [0xF5, 0xF6, 0xF7, 0xF8, 0xF9, 0xFA];
const CHARACTER_NAME_DISPLAY_CELL_LIMIT: usize = 7;
const LOCATION_NAME_DISPLAY_CELL_LIMIT: usize = 11;

pub(crate) fn ending_character_epilogue_preserved_active_codes() -> BTreeSet<u8> {
    OBSERVED_NAMETABLE_ACTIVE_CODES_OUTSIDE_DIALOGUE_INTERIOR
        .into_iter()
        .collect()
}

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
    let observed_active_codes = ending_character_epilogue_preserved_active_codes();
    ensure!(
        observed_active_codes.len()
            == OBSERVED_NAMETABLE_ACTIVE_CODES_OUTSIDE_DIALOGUE_INTERIOR.len(),
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
        budget_basis: "union of all 560 observed nametables outside the dialogue interior, the maximum visible dialogue transition chain, and full character/location display-cell reservations",
        evidence_digest: "sha1:71546fe01803a13a5340c68334111bfa9f13b443".to_owned(),
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
