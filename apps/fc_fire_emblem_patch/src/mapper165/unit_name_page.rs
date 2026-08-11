mod evidence;

use std::{collections::BTreeMap, path::Path};

use anyhow::{Context, Result, ensure};

use crate::{
    font_slots::{FONT_PAGE_SIZE, active_hangul_codes},
    japanese_encoding::is_japanese_text_code,
    rom::Rom,
    roster_localization::{ValidatedRosterLocalization, roster_visible_codes},
    rp2a03::{Instruction, assemble_at},
    sha1_hex,
    unit_names::UnitNamePlan,
    unit_ui_text::preserved_codes_for_unit_name_projection,
};

use super::{
    dialogue_probe_font::{assign_glyph_codes_excluding, build_font_page},
    encode_chr_page_register,
};
use evidence::load_unit_name_screen_evidence;

pub(super) const PAGE_ROUTINE_ADDRESS: u16 = 0xF700;
pub(super) const PAGE_ROUTINE_END: u16 = 0xF748;
const CHECK_SUMMARY_SECOND_PAGE_ADDRESS: u16 = 0xF71A;
const CHECK_RIGHT_PAIR_ADDRESS: u16 = 0xF720;
const WRITE_PAGE_ADDRESS: u16 = 0xF734;
const FALLBACK_ADDRESS: u16 = 0xF743;
const SOURCE_FONT_PAGE: usize = 0;

pub(super) struct UnitNamePagePlan {
    pub(super) roster_assignments: BTreeMap<char, u8>,
    pub(super) unit_ui_assignments: BTreeMap<char, u8>,
    pub(super) roster_page_pack: Vec<u8>,
    pub(super) unit_ui_page_pack: Vec<u8>,
    pub(super) unit_ui_physical_page: u8,
    pub(super) unit_ui_mapper_register: u8,
    pub(super) roster_page_pack_sha1: String,
    pub(super) unit_ui_page_pack_sha1: String,
    pub(super) evidence_manifest_sha1: String,
    pub(super) temporal_sample_count: usize,
    pub(super) unique_nametable_count: usize,
    pub(super) preserved_roster_code_count: usize,
    pub(super) preserved_unit_ui_code_count: usize,
}

pub(super) fn plan_unit_name_pages(
    source_rom: &Rom,
    evidence_path: &Path,
    roster_localization: &ValidatedRosterLocalization,
    names: &UnitNamePlan,
    unit_ui_physical_page: u8,
) -> Result<UnitNamePagePlan> {
    source_rom.verify_supported_japanese()?;
    let source_font_page = source_rom
        .chr()
        .get(SOURCE_FONT_PAGE * FONT_PAGE_SIZE..(SOURCE_FONT_PAGE + 1) * FONT_PAGE_SIZE)
        .context("source ROM has no unit-name font page")?;
    let source_companion_page = source_rom
        .chr()
        .get(FONT_PAGE_SIZE..2 * FONT_PAGE_SIZE)
        .context("source ROM has no font companion page")?;
    let glyphs = names.unique_glyphs();

    let roster_fixed = roster_localization.glyph_assignments();
    let active_codes = active_hangul_codes()
        .into_iter()
        .collect::<std::collections::BTreeSet<_>>();
    let roster_remaining = glyphs
        .iter()
        .copied()
        .filter(|glyph| !roster_fixed.values().any(|fixed| fixed == glyph))
        .collect();
    let mut roster_preserved = roster_visible_codes();
    roster_preserved.retain(|code| active_codes.contains(code) && !roster_fixed.contains_key(code));
    let mut roster_excluded = roster_preserved.clone();
    roster_excluded.extend(roster_fixed.keys().copied());
    let mut roster_assignments = assign_glyph_codes_excluding(&roster_remaining, &roster_excluded)?;
    for (code, glyph) in roster_fixed {
        ensure!(
            roster_assignments.insert(glyph, code).is_none(),
            "roster unit-name glyph received two codes"
        );
    }
    ensure!(
        glyphs
            .iter()
            .all(|glyph| roster_assignments.contains_key(glyph)),
        "roster unit-name codebook lost a glyph"
    );
    let roster_page = build_font_page(source_font_page, &roster_assignments)?;
    let mut roster_page_pack = roster_page.clone();
    roster_page_pack.extend_from_slice(&roster_page);

    let evidence = load_unit_name_screen_evidence(evidence_path)?;
    let mut unit_ui_preserved = preserved_codes_for_unit_name_projection(source_rom.data())?;
    unit_ui_preserved.extend(
        evidence
            .visible_codes
            .into_iter()
            .filter(|code| !is_japanese_text_code(*code)),
    );
    unit_ui_preserved.retain(|code| active_codes.contains(code));
    let unit_ui_assignments = assign_glyph_codes_excluding(&glyphs, &unit_ui_preserved)?;
    let unit_ui_font_page = build_font_page(source_font_page, &unit_ui_assignments)?;
    let mut unit_ui_page_pack = unit_ui_font_page;
    unit_ui_page_pack.extend_from_slice(source_companion_page);
    let unit_ui_mapper_register = encode_chr_page_register(unit_ui_physical_page)?;

    Ok(UnitNamePagePlan {
        roster_page_pack_sha1: sha1_hex(&roster_page_pack),
        unit_ui_page_pack_sha1: sha1_hex(&unit_ui_page_pack),
        roster_assignments,
        unit_ui_assignments,
        roster_page_pack,
        unit_ui_page_pack,
        unit_ui_physical_page,
        unit_ui_mapper_register,
        evidence_manifest_sha1: evidence.manifest_sha1,
        temporal_sample_count: evidence.temporal_sample_count,
        unique_nametable_count: evidence.unique_nametable_count,
        preserved_roster_code_count: roster_preserved.len(),
        preserved_unit_ui_code_count: unit_ui_preserved.len(),
    })
}

pub(super) fn build_page_selector(mapper_register: u8, fallback_target: u16) -> Result<Vec<u8>> {
    ensure!(mapper_register != 0, "unit-UI page register cannot be zero");
    assemble_at(
        PAGE_ROUTINE_ADDRESS,
        &[
            Instruction::Php,
            Instruction::Pha,
            Instruction::LdaZeroPage(0x71),
            Instruction::CmpImmediate(0x10),
            Instruction::BneAbsolute(FALLBACK_ADDRESS),
            Instruction::LdaZeroPage(0x59),
            Instruction::CmpImmediate(0x1A),
            Instruction::BeqAbsolute(CHECK_SUMMARY_SECOND_PAGE_ADDRESS),
            Instruction::CmpImmediate(0x13),
            Instruction::BneAbsolute(FALLBACK_ADDRESS),
            Instruction::LdaZeroPage(0x5A),
            Instruction::CmpImmediate(0x13),
            Instruction::BneAbsolute(FALLBACK_ADDRESS),
            Instruction::BeqAbsolute(CHECK_RIGHT_PAIR_ADDRESS),
            Instruction::LdaZeroPage(0x5A),
            Instruction::CmpImmediate(0x1A),
            Instruction::BneAbsolute(FALLBACK_ADDRESS),
            Instruction::LdaZeroPage(0x5B),
            Instruction::CmpImmediate(0x00),
            Instruction::BneAbsolute(FALLBACK_ADDRESS),
            Instruction::LdaZeroPage(0x5C),
            Instruction::CmpImmediate(0x15),
            Instruction::BeqAbsolute(WRITE_PAGE_ADDRESS),
            Instruction::CmpImmediate(0x18),
            Instruction::BeqAbsolute(WRITE_PAGE_ADDRESS),
            Instruction::CmpImmediate(0x19),
            Instruction::BneAbsolute(FALLBACK_ADDRESS),
            Instruction::LdaImmediate(mapper_register),
            Instruction::Pha,
            Instruction::LdaImmediate(2),
            Instruction::StaAbsolute(0x8000),
            Instruction::Pla,
            Instruction::StaAbsolute(0x8001),
            Instruction::Pla,
            Instruction::Plp,
            Instruction::Rts,
            Instruction::Pla,
            Instruction::Plp,
            Instruction::JmpAbsolute(fallback_target),
        ],
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn selector_accepts_only_the_unit_window_position_and_observed_pairs() {
        let routine =
            build_page_selector(0xB0, super::super::front_end_page::PAGE_ROUTINE_ADDRESS).unwrap();

        assert_eq!(
            PAGE_ROUTINE_ADDRESS as usize + routine.len(),
            PAGE_ROUTINE_END as usize
        );
        assert!(routine.windows(2).any(|bytes| bytes == [0xA5, 0x71]));
        assert!(routine.windows(2).any(|bytes| bytes == [0xA9, 0xB0]));
        assert_eq!(&routine[routine.len() - 3..], &[0x4C, 0x60, 0xFC]);
    }
}
