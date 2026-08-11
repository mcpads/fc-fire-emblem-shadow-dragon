use std::{collections::BTreeSet, path::Path};

use anyhow::{Context, Result, ensure};

use crate::{
    font_slots::FONT_PAGE_SIZE,
    rom::{HEADER_SIZE, Rom},
    sha1_hex,
    tracked::TrackedImage,
};

use super::{
    TITLE_STREAM_BYTE_COUNT, TITLE_TRANSLATION_FIRST_COLUMN,
    TITLE_TRANSLATION_MAX_END_COLUMN_EXCLUSIVE,
    logo_asset::{LOGO_TILE_COLUMN_COUNT, load_source_bound_asset},
    source_chr_page, source_stream, title_stream_file_offset,
    title_translation_end_column_exclusive,
};

const TITLE_ROW_COUNT: usize = 5;
const TITLE_ROW_WIDTH: usize = 32;
const TITLE_ROW_COMMAND_BYTE_COUNT: usize = 3 + TITLE_ROW_WIDTH + 1;
const EXPECTED_OUTPUT_PHYSICAL_CHR_PAGE: usize = 0x16;
const TITLE_RUNTIME_OVERLAY_STREAM_OFFSET_FROM_TITLE_STREAM: usize = 0xD2;
const TITLE_RUNTIME_OVERLAY_CELL_COUNT: usize = 11;
const SOURCE_TITLE_RUNTIME_OVERLAY_STREAM: [u8; 25] = [
    0x21, 0xA8, 0x05, 0x16, 0x17, 0x18, 0x19, 0x1A, 0x21, 0xCD, 0x03, 0x2B, 0x2C, 0x2D, 0x21, 0xED,
    0x03, 0x3B, 0x3C, 0x3D, 0x23, 0xD3, 0x01, 0x21, 0x00,
];

pub(crate) struct InstalledTitleLogo {
    pub(crate) output: Vec<u8>,
    pub(crate) output_sha1: String,
    pub(crate) asset_sha1: String,
    pub(crate) source_owned_tile_count: usize,
    pub(crate) installed_unique_tile_count: usize,
    pub(crate) installed_tilemap_cell_count: usize,
    pub(crate) physical_chr_page: u8,
    pub(crate) installed_chr_page_sha1: String,
    pub(crate) installed_stream_sha1: String,
    pub(crate) installed_runtime_overlay_cell_count: usize,
    pub(crate) installed_runtime_overlay_stream_sha1: String,
    pub(crate) preserved_title_stream_bytes_unchanged: bool,
    pub(crate) preserved_runtime_overlay_control_bytes_unchanged: bool,
    pub(crate) unassigned_title_chr_patterns_unchanged: bool,
    pub(crate) tracked_write_count: usize,
}

pub(crate) fn install_title_logo_asset(
    prior_output: &[u8],
    source_rom: &Rom,
    asset_path: &Path,
) -> Result<InstalledTitleLogo> {
    let asset = load_source_bound_asset(source_rom, asset_path)?;
    let prior_rom = Rom::parse(prior_output.to_vec()).context("parse pre-title cumulative ROM")?;
    let source_stream = source_stream(source_rom)?;
    let stream_offset = title_stream_file_offset();
    ensure!(
        prior_output.get(stream_offset..stream_offset + TITLE_STREAM_BYTE_COUNT)
            == Some(source_stream),
        "pre-title cumulative ROM changed the source title stream"
    );

    let mut installed_stream = source_stream.to_vec();
    let mut installed_tilemap_cell_count = 0;
    for row in 0..TITLE_ROW_COUNT {
        let translation_end = title_translation_end_column_exclusive(row);
        let replacement_width = translation_end - TITLE_TRANSLATION_FIRST_COLUMN;
        let source_start = row * TITLE_ROW_COMMAND_BYTE_COUNT + 3 + TITLE_TRANSLATION_FIRST_COLUMN;
        let asset_start =
            row * (TITLE_TRANSLATION_MAX_END_COLUMN_EXCLUSIVE - TITLE_TRANSLATION_FIRST_COLUMN);
        installed_stream[source_start..source_start + replacement_width]
            .copy_from_slice(&asset.tilemap[asset_start..asset_start + replacement_width]);
        installed_tilemap_cell_count += replacement_width;
    }
    ensure_preserved_stream_bytes(source_stream, &installed_stream)?;
    let runtime_overlay_offset =
        stream_offset + TITLE_RUNTIME_OVERLAY_STREAM_OFFSET_FROM_TITLE_STREAM;
    let source_runtime_overlay = source_rom
        .data()
        .get(
            runtime_overlay_offset
                ..runtime_overlay_offset + SOURCE_TITLE_RUNTIME_OVERLAY_STREAM.len(),
        )
        .context("source title runtime-overlay stream is outside the ROM")?;
    ensure!(
        source_runtime_overlay == SOURCE_TITLE_RUNTIME_OVERLAY_STREAM,
        "source title runtime-overlay stream changed"
    );
    ensure!(
        prior_output.get(
            runtime_overlay_offset
                ..runtime_overlay_offset + SOURCE_TITLE_RUNTIME_OVERLAY_STREAM.len()
        ) == Some(source_runtime_overlay),
        "pre-title cumulative ROM changed the source title runtime-overlay stream"
    );
    let installed_runtime_overlay = build_installed_runtime_overlay_stream(&asset.tilemap)?;
    ensure_runtime_overlay_control_bytes_unchanged(
        source_runtime_overlay,
        &installed_runtime_overlay,
    )?;

    let source_page = source_chr_page(source_rom)?;
    let matching_pages = prior_rom
        .chr()
        .chunks_exact(FONT_PAGE_SIZE)
        .enumerate()
        .filter(|(_, page)| *page == source_page)
        .map(|(index, _)| index)
        .collect::<Vec<_>>();
    ensure!(
        matching_pages == [EXPECTED_OUTPUT_PHYSICAL_CHR_PAGE],
        "pre-title cumulative ROM lost the unique mapped source title CHR page"
    );
    let physical_chr_page = matching_pages[0];
    let mut installed_page = source_page.to_vec();
    let assigned_codes = asset
        .assignments
        .iter()
        .map(|(code, _)| *code)
        .collect::<BTreeSet<_>>();
    for (code, pattern) in &asset.assignments {
        let start = usize::from(*code) * 16;
        installed_page[start..start + 16].copy_from_slice(pattern);
    }
    ensure_unassigned_patterns_unchanged(source_page, &installed_page, &assigned_codes)?;

    let chr_page_offset = HEADER_SIZE
        + prior_rom.prg().len()
        + physical_chr_page
            .checked_mul(FONT_PAGE_SIZE)
            .context("title CHR page file offset overflow")?;
    let mut image = TrackedImage::new(prior_output.to_vec());
    image.write_expected(
        "install Korean title-logo tilemap",
        stream_offset,
        source_stream,
        &installed_stream,
    )?;
    image.write_expected(
        "install Korean title-logo runtime overlay",
        runtime_overlay_offset,
        source_runtime_overlay,
        &installed_runtime_overlay,
    )?;
    image.write_expected(
        "install Korean title-logo patterns",
        chr_page_offset,
        source_page,
        &installed_page,
    )?;
    image.verify_all_changes_tracked(prior_output)?;
    let tracked_write_count = image.writes().len();
    let output = image.into_data();
    let output_rom = Rom::parse(output.clone()).context("parse title-logo cumulative stage")?;
    ensure!(
        output_rom.mapper() == prior_rom.mapper()
            && output_rom.prg().len() == prior_rom.prg().len()
            && output_rom.chr().len() == prior_rom.chr().len(),
        "title-logo stage changed the cumulative ROM layout"
    );
    ensure!(
        output[stream_offset..stream_offset + installed_stream.len()] == installed_stream,
        "title-logo output stream changed after installation"
    );
    ensure!(
        output[runtime_overlay_offset..runtime_overlay_offset + installed_runtime_overlay.len()]
            == installed_runtime_overlay,
        "title-logo runtime-overlay stream changed after installation"
    );
    let output_page = &output_rom.chr()
        [physical_chr_page * FONT_PAGE_SIZE..(physical_chr_page + 1) * FONT_PAGE_SIZE];
    ensure!(
        output_page == installed_page,
        "title-logo output CHR page changed after installation"
    );

    Ok(InstalledTitleLogo {
        output_sha1: sha1_hex(&output),
        output,
        asset_sha1: asset.asset_sha1,
        source_owned_tile_count: asset.source_owned_tile_count,
        installed_unique_tile_count: asset.assignments.len(),
        installed_tilemap_cell_count,
        physical_chr_page: u8::try_from(physical_chr_page)
            .context("title-logo physical CHR page does not fit u8")?,
        installed_chr_page_sha1: sha1_hex(&installed_page),
        installed_stream_sha1: sha1_hex(&installed_stream),
        installed_runtime_overlay_cell_count: TITLE_RUNTIME_OVERLAY_CELL_COUNT,
        installed_runtime_overlay_stream_sha1: sha1_hex(&installed_runtime_overlay),
        preserved_title_stream_bytes_unchanged: true,
        preserved_runtime_overlay_control_bytes_unchanged: true,
        unassigned_title_chr_patterns_unchanged: true,
        tracked_write_count,
    })
}

fn build_installed_runtime_overlay_stream(tilemap: &[u8]) -> Result<[u8; 25]> {
    ensure!(
        tilemap.len() == LOGO_TILE_COLUMN_COUNT * TITLE_ROW_COUNT,
        "title-logo tilemap dimensions changed"
    );
    let mut installed = SOURCE_TITLE_RUNTIME_OVERLAY_STREAM;
    installed[3..8].copy_from_slice(&tilemap[6..11]);
    installed[11..14]
        .copy_from_slice(&tilemap[LOGO_TILE_COLUMN_COUNT + 11..LOGO_TILE_COLUMN_COUNT + 14]);
    installed[17..20].copy_from_slice(
        &tilemap[2 * LOGO_TILE_COLUMN_COUNT + 11..2 * LOGO_TILE_COLUMN_COUNT + 14],
    );
    Ok(installed)
}

fn ensure_runtime_overlay_control_bytes_unchanged(source: &[u8], installed: &[u8]) -> Result<()> {
    ensure!(
        source.len() == SOURCE_TITLE_RUNTIME_OVERLAY_STREAM.len()
            && installed.len() == SOURCE_TITLE_RUNTIME_OVERLAY_STREAM.len(),
        "title runtime-overlay stream length changed"
    );
    let replaced_offsets = (3..8).chain(11..14).chain(17..20).collect::<BTreeSet<_>>();
    ensure!(
        source
            .iter()
            .zip(installed)
            .enumerate()
            .all(|(offset, (before, after))| replaced_offsets.contains(&offset) || before == after),
        "title runtime-overlay installation changed a control byte"
    );
    Ok(())
}

fn ensure_preserved_stream_bytes(source: &[u8], installed: &[u8]) -> Result<()> {
    ensure!(
        source.len() == installed.len(),
        "title stream length changed during installation"
    );
    let mut translation_offsets = BTreeSet::new();
    for row in 0..TITLE_ROW_COUNT {
        let start = row * TITLE_ROW_COMMAND_BYTE_COUNT + 3;
        translation_offsets.extend(
            start + TITLE_TRANSLATION_FIRST_COLUMN
                ..start + title_translation_end_column_exclusive(row),
        );
    }
    ensure!(
        source.iter().zip(installed).enumerate().all(
            |(offset, (before, after))| translation_offsets.contains(&offset) || before == after
        ),
        "title-logo installation changed a preserved title-stream byte"
    );
    Ok(())
}

fn ensure_unassigned_patterns_unchanged(
    source: &[u8],
    installed: &[u8],
    assigned_codes: &BTreeSet<u8>,
) -> Result<()> {
    ensure!(
        source.len() == FONT_PAGE_SIZE && installed.len() == FONT_PAGE_SIZE,
        "title CHR page size changed during installation"
    );
    ensure!(
        (0_u16..=255).all(|code| {
            assigned_codes.contains(&(code as u8))
                || source[usize::from(code) * 16..usize::from(code + 1) * 16]
                    == installed[usize::from(code) * 16..usize::from(code + 1) * 16]
        }),
        "title-logo installation changed an unassigned CHR pattern"
    );
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn completed_title_overlay_reasserts_the_static_korean_logo_cells() {
        let tilemap = (0..TITLE_ROW_COUNT
            * (TITLE_TRANSLATION_MAX_END_COLUMN_EXCLUSIVE - TITLE_TRANSLATION_FIRST_COLUMN))
            .map(|index| u8::try_from(index + 1).unwrap())
            .collect::<Vec<_>>();

        let overlay = build_installed_runtime_overlay_stream(&tilemap).unwrap();

        assert_eq!(&overlay[3..8], &tilemap[6..11]);
        assert_eq!(&overlay[11..14], &tilemap[38..41]);
        assert_eq!(&overlay[17..20], &tilemap[65..68]);
    }
}
