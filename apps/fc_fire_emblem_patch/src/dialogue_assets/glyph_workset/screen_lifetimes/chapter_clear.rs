use std::collections::{BTreeMap, BTreeSet};

use anyhow::{Result, ensure};

use crate::font_slots::active_hangul_codes;

use super::{DialogueRecordKey, MaximumTransitionChainReport, ObservedScreenLifetimeReport};

const SCREEN_ROLE: &str = "chapter-clear epilogue maximum";
const TABLE_ID: &str = "village-and-outro-dialogue";
const ENTRY_INDEX: usize = 24;
const OBSERVED_NAMETABLE_ACTIVE_CODES: [u8; 48] = [
    0x01, 0x02, 0x04, 0x06, 0x07, 0x09, 0x0A, 0x0B, 0x0C, 0x10, 0x13, 0x19, 0x1A, 0x1B, 0x23, 0x28,
    0x29, 0x2F, 0x38, 0x39, 0x3C, 0x3F, 0x45, 0x46, 0x4A, 0x5B, 0x86, 0x8A, 0x92, 0x93, 0x95, 0x9A,
    0x9E, 0xAB, 0xCE, 0xD6, 0xD7, 0xDE, 0xE6, 0xE7, 0xEB, 0xEC, 0xF5, 0xF6, 0xF7, 0xF8, 0xF9, 0xFA,
];

pub(super) fn maximum_epilogue_report(
    filled_glyphs_by_record: &BTreeMap<DialogueRecordKey, BTreeSet<char>>,
    approved_glyphs_by_record: &BTreeMap<DialogueRecordKey, BTreeSet<char>>,
    maximum: &MaximumTransitionChainReport,
    preserved_source_codes: &BTreeSet<u8>,
    active_slot_count: usize,
    working_set_ready: bool,
) -> Result<ObservedScreenLifetimeReport> {
    ensure!(
        maximum.start_table_id == TABLE_ID
            && maximum.start_canonical_entry_index == ENTRY_INDEX
            && maximum.record_count == 1
            && maximum.table_ids == [TABLE_ID]
            && maximum.unique_glyph_count == 175,
        "maximum main-dialogue chain is no longer the observed chapter-clear epilogue record"
    );
    let records = [(TABLE_ID, ENTRY_INDEX)];
    let filled_glyphs =
        super::glyph_union_for_records(filled_glyphs_by_record, &records, SCREEN_ROLE)?;
    let approved_glyphs =
        super::glyph_union_for_records(approved_glyphs_by_record, &records, SCREEN_ROLE)?;

    let active_codes = active_hangul_codes().into_iter().collect::<BTreeSet<_>>();
    let observed_active_codes = OBSERVED_NAMETABLE_ACTIVE_CODES
        .into_iter()
        .collect::<BTreeSet<_>>();
    ensure!(
        observed_active_codes.len() == OBSERVED_NAMETABLE_ACTIVE_CODES.len()
            && observed_active_codes.is_subset(&active_codes),
        "{SCREEN_ROLE} observed code set changed or includes a reserved font slot"
    );
    let preserved_source_active_codes = preserved_source_codes
        .intersection(&active_codes)
        .copied()
        .collect::<BTreeSet<_>>();
    let preserved_active_codes = observed_active_codes
        .union(&preserved_source_active_codes)
        .copied()
        .collect::<BTreeSet<_>>();
    let filled_slot_demand = filled_glyphs.len() + preserved_active_codes.len();
    let approved_slot_demand =
        working_set_ready.then_some(approved_glyphs.len() + preserved_active_codes.len());

    Ok(ObservedScreenLifetimeReport {
        screen_role: SCREEN_ROLE,
        budget_basis: "union of four irregular chapter-eleven epilogue nametables outside the dialogue interior, exact preserved record/runtime codes, and the 175-glyph chapter-clear record",
        evidence_digest: "sha1:0351e181881a3571c116a6b859f870fbe8c83581",
        source_record_count: records.len(),
        filled_unique_glyph_count: filled_glyphs.len(),
        preserved_active_source_code_count: preserved_active_codes.len(),
        additional_target_glyph_reservation_count: 0,
        filled_slot_demand,
        filled_set_fits_one_page_so_far: filled_slot_demand <= active_slot_count,
        approved_unique_glyph_count: approved_glyphs.len(),
        approved_slot_demand,
        approved_set_fits_one_page: approved_slot_demand
            .map(|slot_demand| slot_demand <= active_slot_count),
    })
}
