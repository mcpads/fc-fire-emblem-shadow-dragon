use std::collections::BTreeSet;

use anyhow::{Context, Result, ensure};
use serde::Serialize;

use crate::{
    font_slots::{PRESERVED_DISPLAY_CODES, active_hangul_codes},
    japanese_encoding::is_japanese_text_code,
    rom::Rom,
    sha1_hex,
    source_literals::{
        TranslationSurfaceLiteralInventory, classify_translation_surface_literal_codes,
    },
    text_inventory::decode_source_markup,
};

use super::{SourceRegionSpec, bind_source_region, source_file_offset};

const ENDING_BANK: u8 = 0x04;
pub(crate) const ENDING_BRIDGE_PHASE: u8 = 0x0B;
pub(crate) const ENDING_BRIDGE_DRAW_CALL_SITE: u16 = 0x9F9C;
pub(crate) const ENDING_BRIDGE_CLEAR_CALL_SITE: u16 = 0x9F5D;
pub(crate) const ENDING_BRIDGE_DRAW_CALL_SOURCE: [u8; 3] = [0x20, 0x6C, 0xE5];
pub(crate) const ENDING_BRIDGE_CLEAR_CALL_SOURCE: [u8; 3] = [0x20, 0x2D, 0xC7];
pub(crate) const ENDING_BRIDGE_SOURCE_RENDERER: u16 = 0xE56C;
pub(crate) const ENDING_BRIDGE_SOURCE_CLEAR: u16 = 0xC72D;

const ENDING_BRIDGE_HANDLER_ADDRESS: u16 = 0x9F83;
const ENDING_BRIDGE_HANDLER_BYTES: &[u8] = &[
    0xA5, 0x2F, 0xD0, 0x39, 0xA9, 0x9F, 0x85, 0x01, 0xA9, 0xC1, 0x85, 0x00, 0xA9, 0x21, 0x85, 0x03,
    0xA9, 0xAC, 0x85, 0x02, 0xAD, 0x3A, 0x77, 0x85, 0x04, 0x20, 0x6C, 0xE5, 0xA9, 0x05, 0x85, 0x2F,
    0xEE, 0x3A, 0x77, 0xAD, 0x3A, 0x77, 0xC9, 0x07, 0xD0, 0x13, 0xA9, 0x05, 0x8D, 0x3A, 0x77, 0x20,
    0x54, 0xA5, 0xA9, 0x80, 0x85, 0x22, 0xA9, 0x1F, 0x85, 0x2F, 0xEE, 0x31, 0x77, 0x60,
];
const ENDING_BRIDGE_CLEAR_HANDLER_ADDRESS: u16 = 0x9F57;
const ENDING_BRIDGE_CLEAR_HANDLER_BYTES: &[u8] = &[
    0x20, 0x1F, 0xC7, 0x20, 0x3D, 0xC2, 0x20, 0x2D, 0xC7, 0xEE, 0x31, 0x77, 0x60,
];
const ENDING_BRIDGE_TEXT_ADDRESS: u16 = 0x9FC1;
const ENDING_BRIDGE_TEXT_SOURCE: [u8; 9] = [0x0E, 0x0B, 0x13, 0x9B, 0x9B, 0x9B, 0x9B, 0x9B, 0xEF];
const ENDING_BRIDGE_TERMINATOR: u8 = 0xEF;

pub(super) const SOURCE_REGIONS: &[SourceRegionSpec] = &[
    SourceRegionSpec::code(
        "draw_ending_bridge",
        ENDING_BANK,
        ENDING_BRIDGE_HANDLER_ADDRESS,
        ENDING_BRIDGE_HANDLER_BYTES,
    ),
    SourceRegionSpec::code(
        "clear_ending_bridge",
        ENDING_BANK,
        ENDING_BRIDGE_CLEAR_HANDLER_ADDRESS,
        ENDING_BRIDGE_CLEAR_HANDLER_BYTES,
    ),
    SourceRegionSpec::data(
        "ending_bridge_text",
        ENDING_BANK,
        ENDING_BRIDGE_TEXT_ADDRESS,
        &ENDING_BRIDGE_TEXT_SOURCE,
    ),
];

#[derive(Debug, Serialize)]
pub(super) struct EndingBridgeTranslationSurface {
    screen_role: &'static str,
    ending_phase_address: u16,
    ending_phase_address_hex: &'static str,
    ending_phase: u8,
    ending_phase_hex: &'static str,
    prg_bank: u8,
    prg_bank_hex: &'static str,
    handler_address: u16,
    handler_address_hex: &'static str,
    text_address: u16,
    text_address_hex: &'static str,
    text_file_offset: usize,
    text_file_offset_hex: String,
    source_sha1: String,
    visible_cell_count: usize,
    literal_inventory: TranslationSurfaceLiteralInventory,
    translation_handling: &'static str,
}

pub(crate) struct EndingBridgeStorageSource {
    pub(crate) file_offset: usize,
    pub(crate) source_storage: Vec<u8>,
    pub(crate) source_sha1: String,
    pub(crate) japanese_markup: String,
    pub(crate) max_visible_cells: usize,
    pub(crate) source_reclaimable_active_codes: BTreeSet<u8>,
    pub(crate) preserved_visible_active_codes: BTreeSet<u8>,
}

pub(crate) fn bind_ending_bridge_storage_source(rom: &Rom) -> Result<EndingBridgeStorageSource> {
    for spec in SOURCE_REGIONS {
        bind_source_region(rom, *spec)?;
    }
    let file_offset = source_file_offset(ENDING_BANK, ENDING_BRIDGE_TEXT_ADDRESS)?;
    let source_storage = rom
        .data()
        .get(file_offset..file_offset + ENDING_BRIDGE_TEXT_SOURCE.len())
        .context("ending bridge text is outside the ROM")?
        .to_vec();
    ensure!(
        source_storage.last() == Some(&ENDING_BRIDGE_TERMINATOR),
        "ending bridge text terminator changed"
    );
    let visible = &source_storage[..source_storage.len() - 1];
    let (source_reclaimable_active_codes, preserved_visible_active_codes) =
        partition_visible_codes(visible);
    ensure!(
        source_reclaimable_active_codes == BTreeSet::from([0x0B, 0x0E, 0x13])
            && preserved_visible_active_codes == BTreeSet::from([0x9B])
            && visible.iter().all(|code| {
                source_reclaimable_active_codes.contains(code)
                    || preserved_visible_active_codes.contains(code)
            }),
        "ending bridge Japanese text or preserved ellipsis code changed"
    );

    Ok(EndingBridgeStorageSource {
        file_offset,
        source_sha1: sha1_hex(&source_storage),
        japanese_markup: decode_source_markup(visible),
        max_visible_cells: visible.len(),
        source_storage,
        source_reclaimable_active_codes,
        preserved_visible_active_codes,
    })
}

fn partition_visible_codes(visible: &[u8]) -> (BTreeSet<u8>, BTreeSet<u8>) {
    let active_codes = active_hangul_codes().into_iter().collect::<BTreeSet<_>>();
    let source_reclaimable_active_codes = visible
        .iter()
        .copied()
        .filter(|code| is_japanese_text_code(*code) && active_codes.contains(code))
        .collect();
    let preserved_visible_active_codes = visible
        .iter()
        .copied()
        .filter(|code| PRESERVED_DISPLAY_CODES.contains(code))
        .collect();
    (
        source_reclaimable_active_codes,
        preserved_visible_active_codes,
    )
}

pub(super) fn bind_ending_bridge_translation_surface(
    rom: &Rom,
) -> Result<EndingBridgeTranslationSurface> {
    let source = bind_ending_bridge_storage_source(rom)?;
    let literal_inventory = classify_translation_surface_literal_codes(
        source.source_storage[..source.max_visible_cells].to_vec(),
        "ending bridge surface",
    )?;
    Ok(EndingBridgeTranslationSurface {
        screen_role: "ending_bridge",
        ending_phase_address: 0x7731,
        ending_phase_address_hex: "0x7731",
        ending_phase: ENDING_BRIDGE_PHASE,
        ending_phase_hex: "0x0B",
        prg_bank: ENDING_BANK,
        prg_bank_hex: "0x04",
        handler_address: ENDING_BRIDGE_HANDLER_ADDRESS,
        handler_address_hex: "0x9F83",
        text_address: ENDING_BRIDGE_TEXT_ADDRESS,
        text_address_hex: "0x9FC1",
        text_file_offset: source.file_offset,
        text_file_offset_hex: format!("0x{:05X}", source.file_offset),
        source_sha1: source.source_sha1,
        visible_cell_count: source.max_visible_cells,
        literal_inventory,
        translation_handling: "translate the Japanese bridge, preserve its five period cells, and bind the shared ending font page from draw through clear",
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn period_cells_are_preserved_outside_the_hangul_slot_set() {
        let (reclaimable, preserved) = partition_visible_codes(&ENDING_BRIDGE_TEXT_SOURCE[..8]);

        assert_eq!(reclaimable, BTreeSet::from([0x0B, 0x0E, 0x13]));
        assert_eq!(preserved, BTreeSet::from([0x9B]));
    }
}
