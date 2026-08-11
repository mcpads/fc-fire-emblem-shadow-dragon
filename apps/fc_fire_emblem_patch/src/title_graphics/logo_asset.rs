use std::{fs, path::Path};

use anyhow::{Context, Result};
use serde::Serialize;

use crate::{
    rom::{EXPECTED_SOURCE_SHA1, Rom},
    sha1_hex,
};

use super::{bind_source, source_stream};

mod candidate;
mod preview;
mod tile_plan;

pub(super) const TITLE_ROW_COUNT: usize = 5;
pub(super) const LOGO_TILE_COLUMN_COUNT: usize = 27;
pub(super) const LOGO_PIXEL_WIDTH: usize = LOGO_TILE_COLUMN_COUNT * 8;
pub(super) const LOGO_PIXEL_HEIGHT: usize = TITLE_ROW_COUNT * 8;

#[derive(Debug, Serialize)]
struct TitleLogoAssetReport {
    schema: u8,
    source_sha1: &'static str,
    candidate_sha1: String,
    quantized_logo_sha1: String,
    asset_sha1: String,
    preview_sha1: String,
    logo_tile_columns: usize,
    logo_tile_rows: usize,
    source_owned_tile_count: usize,
    target_unique_nonblank_tile_count: usize,
    target_nonblank_tile_cell_count: usize,
    target_blank_tile_cell_count: usize,
    palette_index_pixel_counts: [usize; 4],
    fits_source_owned_tile_budget: bool,
    source_owned_codes_disjoint_from_preserved_title_codes: bool,
    preserved_title_stream_bytes_unchanged: bool,
    source_sword_sprite_assets_unchanged: bool,
    initial_and_final_palette_phases_rendered: bool,
}

pub(crate) struct TitleLogoAssetSummary {
    pub(crate) source_owned_tile_count: usize,
    pub(crate) target_unique_nonblank_tile_count: usize,
    pub(crate) asset_sha1: String,
    pub(crate) preview_sha1: String,
    pub(crate) report_sha1: String,
}

pub(crate) fn build_title_logo_asset(
    source_path: &Path,
    manifest_path: &Path,
    asset_path: &Path,
    preview_path: &Path,
    report_path: &Path,
) -> Result<TitleLogoAssetSummary> {
    let rom = Rom::from_path(source_path)?;
    rom.verify_supported_japanese()?;
    bind_source(&rom)?;

    let candidate = candidate::load_and_quantize(manifest_path)?;
    let ownership = tile_plan::bind_title_tile_ownership(source_stream(&rom)?)?;
    let tile_plan = tile_plan::build(&candidate.logo_indices, &ownership.source_owned_codes)?;
    let asset = tile_plan::encode_asset(&tile_plan)?;
    let preview = preview::encode_phase_preview(&candidate.logo_indices)?;
    let asset_sha1 = sha1_hex(&asset);
    let preview_sha1 = sha1_hex(&preview);
    let mut palette_index_pixel_counts = [0_usize; 4];
    for index in &candidate.logo_indices {
        palette_index_pixel_counts[usize::from(*index)] += 1;
    }
    let target_nonblank_tile_cell_count = tile_plan.nonblank_cell_count();
    let report = TitleLogoAssetReport {
        schema: 1,
        source_sha1: EXPECTED_SOURCE_SHA1,
        candidate_sha1: candidate.candidate_sha1,
        quantized_logo_sha1: sha1_hex(&candidate.logo_indices),
        asset_sha1: asset_sha1.clone(),
        preview_sha1: preview_sha1.clone(),
        logo_tile_columns: LOGO_TILE_COLUMN_COUNT,
        logo_tile_rows: TITLE_ROW_COUNT,
        source_owned_tile_count: ownership.source_owned_codes.len(),
        target_unique_nonblank_tile_count: tile_plan.assignment_count(),
        target_nonblank_tile_cell_count,
        target_blank_tile_cell_count: tile_plan.cell_count() - target_nonblank_tile_cell_count,
        palette_index_pixel_counts,
        fits_source_owned_tile_budget: true,
        source_owned_codes_disjoint_from_preserved_title_codes: true,
        preserved_title_stream_bytes_unchanged: true,
        source_sword_sprite_assets_unchanged: true,
        initial_and_final_palette_phases_rendered: true,
    };
    let mut report_bytes =
        serde_json::to_vec_pretty(&report).context("serialize title-logo asset report")?;
    report_bytes.push(b'\n');
    let report_sha1 = sha1_hex(&report_bytes);

    write_file(asset_path, &asset)?;
    write_file(preview_path, &preview)?;
    write_file(report_path, &report_bytes)?;

    Ok(TitleLogoAssetSummary {
        source_owned_tile_count: ownership.source_owned_codes.len(),
        target_unique_nonblank_tile_count: tile_plan.assignment_count(),
        asset_sha1,
        preview_sha1,
        report_sha1,
    })
}

fn write_file(path: &Path, data: &[u8]) -> Result<()> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)
            .with_context(|| format!("create output directory {}", parent.display()))?;
    }
    fs::write(path, data).with_context(|| format!("write {}", path.display()))
}
