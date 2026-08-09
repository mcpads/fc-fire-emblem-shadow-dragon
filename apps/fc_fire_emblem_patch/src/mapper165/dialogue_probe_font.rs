use std::collections::{BTreeMap, BTreeSet};

use anyhow::{Context, Result, ensure};

use crate::{
    font::{load_dalmoori, rasterize_glyph},
    font_slots::{FONT_PAGE_SIZE, FONT_TILE_SIZE, active_hangul_codes},
    rom::CHR_FILE_OFFSET,
    sha1_hex,
    tracked::TrackedImage,
};

pub(super) const SOURCE_FONT_PHYSICAL_PAGE: usize = 2;

pub(super) fn assign_glyph_codes(glyphs: &BTreeSet<char>) -> Result<BTreeMap<char, u8>> {
    let active_codes = active_hangul_codes();
    ensure!(
        glyphs.len() <= active_codes.len(),
        "dialogue probe needs {} glyphs but the active page owns only {} slots",
        glyphs.len(),
        active_codes.len()
    );
    Ok(glyphs.iter().copied().zip(active_codes).collect())
}

pub(super) fn install_font_glyphs(
    image: &mut TrackedImage,
    base: &[u8],
    assignments: &BTreeMap<char, u8>,
) -> Result<()> {
    let font = load_dalmoori()?;
    let page_start = CHR_FILE_OFFSET + SOURCE_FONT_PHYSICAL_PAGE * FONT_PAGE_SIZE;
    for (character, code) in assignments {
        let offset = page_start + usize::from(*code) * FONT_TILE_SIZE;
        let expected = base
            .get(offset..offset + FONT_TILE_SIZE)
            .context("dialogue probe font tile is outside the mapper base")?;
        let replacement = rasterize_glyph(&font, *character)?;
        image.write_expected(
            format!("mapper 165 dialogue glyph code {code:02X}"),
            offset,
            expected,
            &replacement,
        )?;
    }
    Ok(())
}

pub(super) fn assignment_sha1(assignments: &BTreeMap<char, u8>) -> String {
    let mut bytes = Vec::new();
    for (character, code) in assignments {
        bytes.extend_from_slice(character.to_string().as_bytes());
        bytes.push(*code);
    }
    sha1_hex(&bytes)
}
