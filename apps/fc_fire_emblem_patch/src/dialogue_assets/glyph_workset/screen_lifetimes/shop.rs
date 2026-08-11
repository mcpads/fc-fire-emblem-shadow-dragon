use std::collections::{BTreeMap, BTreeSet};

use anyhow::{Result, ensure};

use crate::font_slots::active_hangul_codes;

use super::{DialogueRecordKey, ObservedScreenLifetimeReport, glyph_union_for_records};

const SCREEN_ROLE: &str = "weapon-shop purchase handoff";
const LIFETIME_RECORDS: [(&str, usize); 2] =
    [("shop-and-item-dialogue", 0), ("shop-and-item-dialogue", 1)];
const RETAINED_SOURCE_CODES: [u8; 17] = [
    0x01, 0x03, 0x04, 0x06, 0x12, 0x13, 0x19, 0x1A, 0x21, 0x25, 0x26, 0x29, 0x2A, 0x32, 0x35, 0x4E,
    0x5F,
];

pub(super) fn purchase_handoff_report(
    filled_glyphs_by_record: &BTreeMap<DialogueRecordKey, BTreeSet<char>>,
    approved_glyphs_by_record: &BTreeMap<DialogueRecordKey, BTreeSet<char>>,
    active_slot_count: usize,
    working_set_ready: bool,
) -> Result<Option<ObservedScreenLifetimeReport>> {
    let table_is_present = filled_glyphs_by_record
        .keys()
        .any(|(table_id, _)| table_id == "shop-and-item-dialogue");
    if !table_is_present {
        return Ok(None);
    }

    let active_codes = active_hangul_codes().into_iter().collect::<BTreeSet<_>>();
    let retained_source_codes = RETAINED_SOURCE_CODES.into_iter().collect::<BTreeSet<_>>();
    ensure!(
        retained_source_codes.len() == RETAINED_SOURCE_CODES.len(),
        "{SCREEN_ROLE} retained source codes contain duplicates"
    );
    ensure!(
        retained_source_codes.is_subset(&active_codes),
        "{SCREEN_ROLE} retained source codes include a reserved font slot"
    );

    let filled_glyphs =
        glyph_union_for_records(filled_glyphs_by_record, &LIFETIME_RECORDS, SCREEN_ROLE)?;
    let approved_glyphs =
        glyph_union_for_records(approved_glyphs_by_record, &LIFETIME_RECORDS, SCREEN_ROLE)?;
    let preserved_active_source_code_count = retained_source_codes.len();
    let filled_slot_demand = preserved_active_source_code_count + filled_glyphs.len();
    let approved_slot_demand =
        working_set_ready.then_some(preserved_active_source_code_count + approved_glyphs.len());

    Ok(Some(ObservedScreenLifetimeReport {
        screen_role: SCREEN_ROLE,
        budget_basis: "one exact E7 handoff frame with the retained six-item list, purchase question, and later yes/no codes",
        evidence_digest: "sha256:bfd547fdbcc8eac92baee4163ae0e4fe0c96571d07dcb600c53571b59e6fe2ea"
            .to_owned(),
        source_record_count: LIFETIME_RECORDS.len(),
        filled_unique_glyph_count: filled_glyphs.len(),
        preserved_active_source_code_count,
        additional_target_glyph_reservation_count: 0,
        filled_slot_demand,
        filled_set_fits_one_page_so_far: filled_slot_demand <= active_slot_count,
        approved_unique_glyph_count: approved_glyphs.len(),
        approved_slot_demand,
        approved_set_fits_one_page: approved_slot_demand
            .map(|slot_demand| slot_demand <= active_slot_count),
    }))
}
