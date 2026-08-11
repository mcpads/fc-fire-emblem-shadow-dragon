use std::collections::{BTreeMap, BTreeSet};

use anyhow::{Context, Result, ensure};

use super::{LOGO_PIXEL_HEIGHT, LOGO_PIXEL_WIDTH, LOGO_TILE_COLUMN_COUNT, TITLE_ROW_COUNT};
use crate::title_graphics::{
    TITLE_STREAM_BYTE_COUNT, TITLE_TRANSLATION_END_COLUMN_EXCLUSIVE, TITLE_TRANSLATION_FIRST_COLUMN,
};

const ASSET_SCHEMA: u8 = 1;
const ASSET_MAGIC: &[u8; 4] = b"FETL";
const TITLE_ROW_WIDTH: usize = 32;
const TITLE_ROW_COMMAND_BYTE_COUNT: usize = 3 + TITLE_ROW_WIDTH + 1;
const BLANK_TILE_CODE: u8 = 0xFF;
const EXPECTED_SOURCE_OWNED_TILE_COUNT: usize = 122;

pub(super) struct TitleTileOwnership {
    pub(super) source_owned_codes: BTreeSet<u8>,
}

pub(super) struct TilePlan {
    tilemap: Vec<u8>,
    assignments: Vec<(u8, [u8; 16])>,
}

impl TilePlan {
    pub(super) fn cell_count(&self) -> usize {
        self.tilemap.len()
    }

    pub(super) fn nonblank_cell_count(&self) -> usize {
        self.tilemap
            .iter()
            .filter(|code| **code != BLANK_TILE_CODE)
            .count()
    }

    pub(super) fn assignment_count(&self) -> usize {
        self.assignments.len()
    }
}

pub(super) fn bind_title_tile_ownership(stream: &[u8]) -> Result<TitleTileOwnership> {
    ensure!(
        stream.len() == TITLE_STREAM_BYTE_COUNT,
        "title-logo stream length changed"
    );
    let mut source_owned_codes = BTreeSet::new();
    let mut preserved_codes = BTreeSet::new();
    for row in 0..TITLE_ROW_COUNT {
        let row_start = row * TITLE_ROW_COMMAND_BYTE_COUNT + 3;
        let row_bytes = &stream[row_start..row_start + TITLE_ROW_WIDTH];
        source_owned_codes.extend(
            row_bytes[TITLE_TRANSLATION_FIRST_COLUMN..TITLE_TRANSLATION_END_COLUMN_EXCLUSIVE]
                .iter()
                .copied()
                .filter(|code| *code != BLANK_TILE_CODE),
        );
        preserved_codes.extend(
            row_bytes[..TITLE_TRANSLATION_FIRST_COLUMN]
                .iter()
                .chain(&row_bytes[TITLE_TRANSLATION_END_COLUMN_EXCLUSIVE..])
                .copied()
                .filter(|code| *code != BLANK_TILE_CODE),
        );
    }
    ensure!(
        source_owned_codes.len() == EXPECTED_SOURCE_OWNED_TILE_COUNT,
        "title-logo source-owned tile population changed"
    );
    ensure!(
        !preserved_codes.is_empty() && source_owned_codes.is_disjoint(&preserved_codes),
        "title-logo source-owned and preserved tile codes overlap or are empty"
    );
    Ok(TitleTileOwnership { source_owned_codes })
}

pub(super) fn build(indices: &[u8], source_owned_codes: &BTreeSet<u8>) -> Result<TilePlan> {
    ensure!(
        indices.len() == LOGO_PIXEL_WIDTH * LOGO_PIXEL_HEIGHT,
        "title-logo quantized surface size changed"
    );
    let mut cell_patterns = Vec::with_capacity(LOGO_TILE_COLUMN_COUNT * TITLE_ROW_COUNT);
    let mut unique_nonblank = BTreeSet::new();
    for tile_y in 0..TITLE_ROW_COUNT {
        for tile_x in 0..LOGO_TILE_COLUMN_COUNT {
            let pattern = encode_tile(indices, tile_x, tile_y);
            if pattern.iter().any(|byte| *byte != 0) {
                unique_nonblank.insert(pattern);
            }
            cell_patterns.push(pattern);
        }
    }
    ensure!(
        unique_nonblank.len() <= source_owned_codes.len(),
        "title-logo candidate needs {} unique tiles but the source owns only {}",
        unique_nonblank.len(),
        source_owned_codes.len()
    );
    let code_by_pattern = unique_nonblank
        .iter()
        .copied()
        .zip(source_owned_codes.iter().copied())
        .collect::<BTreeMap<_, _>>();
    let tilemap = cell_patterns
        .iter()
        .map(|pattern| {
            code_by_pattern
                .get(pattern)
                .copied()
                .unwrap_or(BLANK_TILE_CODE)
        })
        .collect::<Vec<_>>();
    let assignments = code_by_pattern
        .into_iter()
        .map(|(pattern, code)| (code, pattern))
        .collect::<Vec<_>>();
    Ok(TilePlan {
        tilemap,
        assignments,
    })
}

fn encode_tile(indices: &[u8], tile_x: usize, tile_y: usize) -> [u8; 16] {
    let mut pattern = [0_u8; 16];
    for y in 0..8 {
        for x in 0..8 {
            let index = indices[(tile_y * 8 + y) * LOGO_PIXEL_WIDTH + tile_x * 8 + x];
            let bit = 1 << (7 - x);
            if index & 1 != 0 {
                pattern[y] |= bit;
            }
            if index & 2 != 0 {
                pattern[8 + y] |= bit;
            }
        }
    }
    pattern
}

pub(super) fn encode_asset(plan: &TilePlan) -> Result<Vec<u8>> {
    let tile_count = u8::try_from(plan.assignments.len())
        .context("title-logo asset has more than 255 tile assignments")?;
    let mut asset = Vec::with_capacity(
        ASSET_MAGIC.len() + 4 + plan.tilemap.len() + plan.assignments.len() * 17,
    );
    asset.extend_from_slice(ASSET_MAGIC);
    asset.push(ASSET_SCHEMA);
    asset.push(LOGO_TILE_COLUMN_COUNT as u8);
    asset.push(TITLE_ROW_COUNT as u8);
    asset.push(tile_count);
    asset.extend_from_slice(&plan.tilemap);
    for (code, pattern) in &plan.assignments {
        asset.push(*code);
        asset.extend_from_slice(pattern);
    }
    ensure!(
        asset.len() == ASSET_MAGIC.len() + 4 + plan.tilemap.len() + plan.assignments.len() * 17,
        "title-logo asset length changed"
    );
    Ok(asset)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn encoded_tile_keeps_each_two_bit_palette_index() {
        let mut indices = vec![0_u8; LOGO_PIXEL_WIDTH * LOGO_PIXEL_HEIGHT];
        indices[0] = 1;
        indices[1] = 2;
        indices[2] = 3;

        let pattern = encode_tile(&indices, 0, 0);

        assert_eq!(pattern[0], 0b1010_0000);
        assert_eq!(pattern[8], 0b0110_0000);
        assert!(pattern[1..8].iter().all(|byte| *byte == 0));
        assert!(pattern[9..].iter().all(|byte| *byte == 0));
    }
}
