use std::{collections::BTreeSet, path::Path};

use anyhow::{Context, Result, ensure};

use crate::{
    font_slots::FONT_PAGE_SIZE,
    rom::{HEADER_SIZE, Rom},
    sha1_hex,
    tracked::TrackedImage,
};

use super::{
    TITLE_STREAM_BYTE_COUNT, TITLE_TRANSLATION_END_COLUMN_EXCLUSIVE,
    TITLE_TRANSLATION_FIRST_COLUMN, logo_asset::load_source_bound_asset, source_chr_page,
    source_stream, title_stream_file_offset,
};

const TITLE_ROW_COUNT: usize = 5;
const TITLE_ROW_WIDTH: usize = 32;
const TITLE_ROW_COMMAND_BYTE_COUNT: usize = 3 + TITLE_ROW_WIDTH + 1;
const EXPECTED_OUTPUT_PHYSICAL_CHR_PAGE: usize = 0x16;

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
    pub(crate) preserved_title_stream_bytes_unchanged: bool,
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
    for row in 0..TITLE_ROW_COUNT {
        let source_start = row * TITLE_ROW_COMMAND_BYTE_COUNT + 3 + TITLE_TRANSLATION_FIRST_COLUMN;
        let asset_start =
            row * (TITLE_TRANSLATION_END_COLUMN_EXCLUSIVE - TITLE_TRANSLATION_FIRST_COLUMN);
        installed_stream[source_start
            ..source_start + TITLE_TRANSLATION_END_COLUMN_EXCLUSIVE
                - TITLE_TRANSLATION_FIRST_COLUMN]
            .copy_from_slice(
                &asset.tilemap[asset_start
                    ..asset_start + TITLE_TRANSLATION_END_COLUMN_EXCLUSIVE
                        - TITLE_TRANSLATION_FIRST_COLUMN],
            );
    }
    ensure_preserved_stream_bytes(source_stream, &installed_stream)?;

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
        installed_tilemap_cell_count: asset.tilemap.len(),
        physical_chr_page: u8::try_from(physical_chr_page)
            .context("title-logo physical CHR page does not fit u8")?,
        installed_chr_page_sha1: sha1_hex(&installed_page),
        installed_stream_sha1: sha1_hex(&installed_stream),
        preserved_title_stream_bytes_unchanged: true,
        unassigned_title_chr_patterns_unchanged: true,
        tracked_write_count,
    })
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
            start + TITLE_TRANSLATION_FIRST_COLUMN..start + TITLE_TRANSLATION_END_COLUMN_EXCLUSIVE,
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
