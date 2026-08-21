use std::collections::{BTreeMap, BTreeSet};

use anyhow::{Context, Result, ensure};

use crate::{font_slots::active_hangul_codes, game_over_dialogue::GameOverDialogueSourceBinding};

use super::{DialogueRecordKey, ObservedScreenLifetimeReport, glyph_union_for_records};

const SCREEN_ROLE: &str = "turn-boundary game over";
const TABLE_ID: &str = "victory-and-defeat-dialogue";
const OBSERVED_NAMETABLE_ACTIVE_CODES: [u8; 90] = [
    0x00, 0x01, 0x02, 0x03, 0x04, 0x05, 0x06, 0x07, 0x0A, 0x0B, 0x0C, 0x0D, 0x0E, 0x10, 0x11, 0x12,
    0x13, 0x14, 0x15, 0x16, 0x17, 0x18, 0x1E, 0x20, 0x21, 0x24, 0x25, 0x28, 0x29, 0x2F, 0x30, 0x31,
    0x34, 0x35, 0x3C, 0x3D, 0x3F, 0x40, 0x43, 0x48, 0x49, 0x4C, 0x50, 0x58, 0x59, 0x5A, 0x5E, 0x87,
    0x8A, 0x8B, 0x8C, 0x8E, 0x8F, 0x92, 0x93, 0x98, 0x99, 0x9A, 0xA2, 0xA3, 0xA5, 0xAA, 0xAC, 0xAE,
    0xAF, 0xB0, 0xB1, 0xB2, 0xB3, 0xB5, 0xBC, 0xBD, 0xBE, 0xBF, 0xCF, 0xD0, 0xD1, 0xD2, 0xD3, 0xD4,
    0xD5, 0xDF, 0xE2, 0xE3, 0xF5, 0xF6, 0xF7, 0xF8, 0xF9, 0xFA,
];

pub(super) fn turn_boundary_game_over_report(
    filled_glyphs_by_record: &BTreeMap<DialogueRecordKey, BTreeSet<char>>,
    approved_glyphs_by_record: &BTreeMap<DialogueRecordKey, BTreeSet<char>>,
    source_binding: Option<&GameOverDialogueSourceBinding>,
    active_slot_count: usize,
    working_set_ready: bool,
) -> Result<Option<ObservedScreenLifetimeReport>> {
    if !filled_glyphs_by_record
        .keys()
        .any(|(table_id, _)| table_id == TABLE_ID)
    {
        return Ok(None);
    }
    let source_binding = source_binding
        .context("turn-boundary game-over records exist without a bound source selector family")?;
    let records = source_binding
        .record_indices
        .iter()
        .map(|index| (TABLE_ID, *index))
        .collect::<Vec<_>>();
    let filled_glyphs = glyph_union_for_records(filled_glyphs_by_record, &records, SCREEN_ROLE)?;
    let approved_glyphs =
        glyph_union_for_records(approved_glyphs_by_record, &records, SCREEN_ROLE)?;

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

    let preserved_active_source_code_count = observed_active_codes.len();
    let filled_slot_demand = preserved_active_source_code_count + filled_glyphs.len();
    let approved_slot_demand =
        working_set_ready.then_some(preserved_active_source_code_count + approved_glyphs.len());

    Ok(Some(ObservedScreenLifetimeReport {
        screen_role: SCREEN_ROLE,
        budget_basis: "conservative union of all 12 observed no-save game-over nametables and every Korean record produced by the source-bound game-over selector family",
        evidence_digest: format!(
            "temporal-sha1:ffd0fc3e8ccb44798fbc83c618ea068369fd114c;source-route-sha1:{}",
            source_binding.selector_routine_sha1
        ),
        source_record_count: records.len(),
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
