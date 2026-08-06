use anyhow::{Result, ensure};
use fontdue::{Font, FontSettings};

const DALMOORI_TTF: &[u8] = include_bytes!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../../assets/fonts/dalmoori.ttf"
));
const PIXEL_SIZE: f32 = 8.0;
const COVERAGE_THRESHOLD: u8 = 128;

pub fn load_dalmoori() -> Result<Font> {
    Font::from_bytes(DALMOORI_TTF, FontSettings::default())
        .map_err(|error| anyhow::anyhow!("load vendored Dalmoori font: {error}"))
}

pub fn rasterize_glyph(font: &Font, character: char) -> Result<[u8; 16]> {
    ensure!(
        font.lookup_glyph_index(character) != 0,
        "Dalmoori has no glyph for {character:?}"
    );
    let (metrics, bitmap) = font.rasterize(character, PIXEL_SIZE);
    ensure!(
        metrics.width <= 8 && metrics.height <= 8,
        "Dalmoori glyph {character:?} is {}x{} at 8px",
        metrics.width,
        metrics.height
    );

    let mut tile = [0_u8; 16];
    let offset_x = (8 - metrics.width) / 2;
    let offset_y = 7 - metrics.ymin - metrics.height as i32;
    ensure!(
        offset_y >= 0 && offset_y as usize + metrics.height <= 8,
        "Dalmoori glyph {character:?} falls outside the 8x8 cell"
    );
    let offset_y = offset_y as usize;
    for row in 0..metrics.height {
        for column in 0..metrics.width {
            if bitmap[row * metrics.width + column] >= COVERAGE_THRESHOLD {
                tile[offset_y + row] |= 1 << (7 - (offset_x + column));
            }
        }
    }
    ensure!(
        tile[..8].iter().any(|row| *row != 0),
        "Dalmoori glyph {character:?} rasterized to an empty tile"
    );
    Ok(tile)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rasterizes_hangul_into_the_low_nes_bitplane() {
        let tile = rasterize_glyph(&load_dalmoori().unwrap(), '한').unwrap();

        assert!(tile[..8].iter().any(|byte| *byte != 0));
        assert!(tile[8..].iter().all(|byte| *byte == 0));
    }
}
