use std::collections::{BTreeMap, BTreeSet};

use anyhow::{Context, Result, ensure};

use crate::font_slots::active_hangul_codes;

use super::{DialogueRecordKey, report::ObservedScreenLifetimeReport};

const SHOP_PURCHASE_SCREEN_ROLE: &str = "weapon-shop purchase handoff";
const SHOP_PURCHASE_LIFETIME_RECORDS: [(&str, usize); 2] =
    [("shop-and-item-dialogue", 0), ("shop-and-item-dialogue", 1)];
const SHOP_PURCHASE_RETAINED_SOURCE_CODES: [u8; 17] = [
    0x01, 0x03, 0x04, 0x06, 0x12, 0x13, 0x19, 0x1A, 0x21, 0x25, 0x26, 0x29, 0x2A, 0x32, 0x35,
    0x4E, 0x5F,
];

pub(super) fn observed_screen_lifetime_reports(
    filled_glyphs_by_record: &BTreeMap<DialogueRecordKey, BTreeSet<char>>,
    approved_glyphs_by_record: &BTreeMap<DialogueRecordKey, BTreeSet<char>>,
    active_slot_count: usize,
    working_set_ready: bool,
) -> Result<Vec<ObservedScreenLifetimeReport>> {
    let shop_table_is_present = filled_glyphs_by_record
        .keys()
        .any(|(table_id, _)| table_id == "shop-and-item-dialogue");
    if !shop_table_is_present {
        return Ok(Vec::new());
    }

    let active_codes = active_hangul_codes().into_iter().collect::<BTreeSet<_>>();
    let retained_source_codes = SHOP_PURCHASE_RETAINED_SOURCE_CODES
        .into_iter()
        .collect::<BTreeSet<_>>();
    ensure!(
        retained_source_codes.len() == SHOP_PURCHASE_RETAINED_SOURCE_CODES.len(),
        "{SHOP_PURCHASE_SCREEN_ROLE} retained source codes contain duplicates"
    );
    ensure!(
        retained_source_codes.is_subset(&active_codes),
        "{SHOP_PURCHASE_SCREEN_ROLE} retained source codes include a reserved font slot"
    );

    let filled_glyphs = glyph_union_for_records(
        filled_glyphs_by_record,
        &SHOP_PURCHASE_LIFETIME_RECORDS,
        SHOP_PURCHASE_SCREEN_ROLE,
    )?;
    let approved_glyphs = glyph_union_for_records(
        approved_glyphs_by_record,
        &SHOP_PURCHASE_LIFETIME_RECORDS,
        SHOP_PURCHASE_SCREEN_ROLE,
    )?;
    let preserved_active_source_code_count = retained_source_codes.len();
    let filled_slot_demand = preserved_active_source_code_count + filled_glyphs.len();
    let approved_slot_demand =
        working_set_ready.then_some(preserved_active_source_code_count + approved_glyphs.len());

    Ok(vec![ObservedScreenLifetimeReport {
        screen_role: SHOP_PURCHASE_SCREEN_ROLE,
        source_record_count: SHOP_PURCHASE_LIFETIME_RECORDS.len(),
        filled_unique_glyph_count: filled_glyphs.len(),
        preserved_active_source_code_count,
        filled_slot_demand,
        filled_set_fits_one_page_so_far: filled_slot_demand <= active_slot_count,
        approved_unique_glyph_count: approved_glyphs.len(),
        approved_slot_demand,
        approved_set_fits_one_page: approved_slot_demand
            .map(|slot_demand| slot_demand <= active_slot_count),
    }])
}

fn glyph_union_for_records(
    glyphs_by_record: &BTreeMap<DialogueRecordKey, BTreeSet<char>>,
    records: &[(&str, usize)],
    screen_role: &str,
) -> Result<BTreeSet<char>> {
    let mut glyphs = BTreeSet::new();
    for &(table_id, canonical_entry_index) in records {
        let key = (table_id.to_owned(), canonical_entry_index);
        glyphs.extend(
            glyphs_by_record
                .get(&key)
                .with_context(|| {
                    format!(
                        "{screen_role} record {table_id}:{canonical_entry_index} is missing from the workspace"
                    )
                })?
                .iter()
                .copied(),
        );
    }
    Ok(glyphs)
}
