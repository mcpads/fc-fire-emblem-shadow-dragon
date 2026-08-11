use std::collections::BTreeSet;

use anyhow::{Result, ensure};

use crate::font_slots::{ACTIVE_HANGUL_SLOT_COUNT, active_hangul_codes};

pub(super) struct FullPageReplacementBound {
    pub(super) target_glyph_count: usize,
    pub(super) source_reclaimable_active_code_count: usize,
    pub(super) preserved_active_source_code_count: usize,
    pub(super) total_slot_demand: usize,
}

pub(super) fn measure(
    target_glyphs: &BTreeSet<char>,
    source_reclaimable_active_codes: &BTreeSet<u8>,
    consumer_role: &str,
) -> Result<FullPageReplacementBound> {
    let bound = calculate(
        target_glyphs,
        source_reclaimable_active_codes,
        consumer_role,
    )?;
    ensure!(
        bound.total_slot_demand <= ACTIVE_HANGUL_SLOT_COUNT,
        "{consumer_role} full-page replacement needs {} active slots but only {ACTIVE_HANGUL_SLOT_COUNT} exist",
        bound.total_slot_demand
    );
    Ok(bound)
}

pub(super) fn calculate(
    target_glyphs: &BTreeSet<char>,
    source_reclaimable_active_codes: &BTreeSet<u8>,
    consumer_role: &str,
) -> Result<FullPageReplacementBound> {
    let active_codes = active_hangul_codes().into_iter().collect::<BTreeSet<_>>();
    ensure!(
        !target_glyphs.is_empty(),
        "{consumer_role} full-page bound has no Korean target glyphs"
    );
    ensure!(
        !source_reclaimable_active_codes.is_empty()
            && source_reclaimable_active_codes.is_subset(&active_codes),
        "{consumer_role} full-page bound contains no reclaimable source codes or a reserved code"
    );
    let preserved_active_source_code_count = ACTIVE_HANGUL_SLOT_COUNT
        .checked_sub(source_reclaimable_active_codes.len())
        .expect("reclaimable active codes are a subset of the active page");
    let total_slot_demand = preserved_active_source_code_count
        .checked_add(target_glyphs.len())
        .expect("full-page slot demand overflow");
    Ok(FullPageReplacementBound {
        target_glyph_count: target_glyphs.len(),
        source_reclaimable_active_code_count: source_reclaimable_active_codes.len(),
        preserved_active_source_code_count,
        total_slot_demand,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn replacement_bound_preserves_every_active_code_not_proven_reclaimable() {
        let bound = measure(
            &BTreeSet::from(['가', '나']),
            &BTreeSet::from([0x00, 0x01, 0x02]),
            "fixture",
        )
        .unwrap();

        assert_eq!(bound.target_glyph_count, 2);
        assert_eq!(bound.source_reclaimable_active_code_count, 3);
        assert_eq!(bound.preserved_active_source_code_count, 207);
        assert_eq!(bound.total_slot_demand, 209);
    }

    #[test]
    fn replacement_bound_rejects_reserved_source_codes() {
        assert!(measure(&BTreeSet::from(['가']), &BTreeSet::from([0x0F]), "fixture").is_err());
    }
}
