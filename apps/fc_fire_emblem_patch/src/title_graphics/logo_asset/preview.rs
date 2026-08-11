use std::io::Cursor;

use anyhow::{Context, Result};

use super::{LOGO_PIXEL_HEIGHT, LOGO_PIXEL_WIDTH};

const PHASE_GAP: usize = 8;
const INITIAL_PHASE_PALETTE: [[u8; 3]; 4] = [
    [0x00, 0x00, 0x00],
    [0x75, 0x27, 0xFE],
    [0x00, 0x00, 0x00],
    [0x75, 0x27, 0xFE],
];
const FINAL_PHASE_PALETTE: [[u8; 3]; 4] = [
    [0x00, 0x00, 0x00],
    [0xFF, 0xFE, 0xFF],
    [0xC0, 0xDF, 0xFF],
    [0x75, 0x27, 0xFE],
];

pub(super) fn encode_phase_preview(indices: &[u8]) -> Result<Vec<u8>> {
    let height = LOGO_PIXEL_HEIGHT * 2 + PHASE_GAP;
    let mut pixels = vec![0_u8; LOGO_PIXEL_WIDTH * height * 3];
    render_palette_phase(&mut pixels, 0, indices, INITIAL_PHASE_PALETTE);
    render_palette_phase(
        &mut pixels,
        LOGO_PIXEL_HEIGHT + PHASE_GAP,
        indices,
        FINAL_PHASE_PALETTE,
    );
    let mut encoded = Cursor::new(Vec::new());
    {
        let mut encoder = png::Encoder::new(
            &mut encoded,
            u32::try_from(LOGO_PIXEL_WIDTH).expect("logo width fits u32"),
            u32::try_from(height).expect("preview height fits u32"),
        );
        encoder.set_color(png::ColorType::Rgb);
        encoder.set_depth(png::BitDepth::Eight);
        let mut writer = encoder
            .write_header()
            .context("write title-logo preview PNG header")?;
        writer
            .write_image_data(&pixels)
            .context("write title-logo preview PNG pixels")?;
    }
    Ok(encoded.into_inner())
}

fn render_palette_phase(
    pixels: &mut [u8],
    output_top: usize,
    indices: &[u8],
    palette: [[u8; 3]; 4],
) {
    for y in 0..LOGO_PIXEL_HEIGHT {
        for x in 0..LOGO_PIXEL_WIDTH {
            let color = palette[usize::from(indices[y * LOGO_PIXEL_WIDTH + x])];
            let output_offset = ((output_top + y) * LOGO_PIXEL_WIDTH + x) * 3;
            pixels[output_offset..output_offset + 3].copy_from_slice(&color);
        }
    }
}
