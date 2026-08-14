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
    assign_glyph_codes_excluding(glyphs, &BTreeSet::new())
}

pub(super) fn assign_glyph_codes_excluding(
    glyphs: &BTreeSet<char>,
    preserved_active_codes: &BTreeSet<u8>,
) -> Result<BTreeMap<char, u8>> {
    let active_codes = active_hangul_codes().into_iter().collect::<BTreeSet<_>>();
    ensure!(
        preserved_active_codes.is_subset(&active_codes),
        "preserved screen codes include a reserved font code"
    );
    let available_codes = active_codes
        .difference(preserved_active_codes)
        .copied()
        .collect::<Vec<_>>();
    ensure!(
        glyphs.len() <= available_codes.len(),
        "dialogue probe needs {} glyphs but the screen-safe page owns only {} slots",
        glyphs.len(),
        available_codes.len()
    );
    Ok(glyphs.iter().copied().zip(available_codes).collect())
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

pub(crate) fn build_font_page(
    source_page: &[u8],
    assignments: &BTreeMap<char, u8>,
) -> Result<Vec<u8>> {
    let glyphs_by_code = assignments
        .iter()
        .map(|(glyph, code)| (*code, *glyph))
        .collect::<BTreeMap<_, _>>();
    ensure!(
        glyphs_by_code.len() == assignments.len(),
        "font page assigns one code to multiple glyphs"
    );
    build_font_page_by_code(source_page, &glyphs_by_code)
}

/// 한 글자 모양이 서로 다른 생산자 코드에 중복될 수 있는 페이지를 만든다.
pub(crate) fn build_font_page_by_code(
    source_page: &[u8],
    glyphs_by_code: &BTreeMap<u8, char>,
) -> Result<Vec<u8>> {
    ensure!(
        source_page.len() == FONT_PAGE_SIZE,
        "dialogue font source page must be exactly 4 KiB"
    );
    let font = load_dalmoori()?;
    let mut page = source_page.to_vec();
    for (code, character) in glyphs_by_code {
        let offset = usize::from(*code) * FONT_TILE_SIZE;
        page[offset..offset + FONT_TILE_SIZE].copy_from_slice(&rasterize_glyph(&font, *character)?);
    }
    Ok(page)
}

pub(super) fn assignment_sha1(assignments: &BTreeMap<char, u8>) -> String {
    let mut bytes = Vec::new();
    for (character, code) in assignments {
        bytes.extend_from_slice(character.to_string().as_bytes());
        bytes.push(*code);
    }
    sha1_hex(&bytes)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn excluded_codes_are_never_assigned_to_glyphs() {
        let glyphs = ['가', '나', '다'].into_iter().collect();
        let excluded = [0x00, 0x03, 0x3B].into_iter().collect();

        let assignments = assign_glyph_codes_excluding(&glyphs, &excluded).unwrap();

        assert_eq!(assignments.len(), glyphs.len());
        assert!(assignments.values().all(|code| !excluded.contains(code)));
    }

    #[test]
    fn excluded_codes_reduce_the_available_capacity() {
        let active_codes = active_hangul_codes();
        let excluded = [active_codes[0]].into_iter().collect();
        let glyphs = (0..active_codes.len())
            .map(|index| char::from_u32(0xAC00 + u32::try_from(index).unwrap()).unwrap())
            .collect();

        let error = assign_glyph_codes_excluding(&glyphs, &excluded).unwrap_err();

        assert!(error.to_string().contains("owns only 209 slots"));
    }

    #[test]
    fn page_builder_preserves_every_unassigned_tile() {
        let source_page = (0..FONT_PAGE_SIZE)
            .map(|index| u8::try_from(index % 251).unwrap())
            .collect::<Vec<_>>();
        let assignments = BTreeMap::from([('한', 0x01)]);

        let page = build_font_page(&source_page, &assignments).unwrap();

        assert_eq!(page.len(), source_page.len());
        assert_ne!(
            &page[FONT_TILE_SIZE..2 * FONT_TILE_SIZE],
            &source_page[FONT_TILE_SIZE..2 * FONT_TILE_SIZE]
        );
        assert_eq!(&page[..FONT_TILE_SIZE], &source_page[..FONT_TILE_SIZE]);
        assert_eq!(
            &page[2 * FONT_TILE_SIZE..],
            &source_page[2 * FONT_TILE_SIZE..]
        );
    }
}
