use super::*;

pub(super) fn render_font_page_sheet(
    page: &[u8],
    slots: &[SlotReport],
    scale: u32,
) -> Result<Vec<u8>> {
    ensure!(page.len() == CHR_PAGE_SIZE, "font page must be 4 KiB");
    ensure!(
        slots.len() == TILES_PER_PAGE,
        "font page slot count mismatch"
    );

    let label_scale = if scale >= 3 { 2 } else { 1 };
    let tile_pixels = 8 * scale;
    let cell_width = tile_pixels + 4;
    let cell_height = tile_pixels + 5 * label_scale + 7;
    let width = 16 * cell_width;
    let height = 16 * cell_height;
    let mut pixels = vec![0x12_u8; (width * height * 3) as usize];

    for (index, slot) in slots.iter().enumerate() {
        let left = (index as u32 % 16) * cell_width;
        let top = (index as u32 / 16) * cell_height;
        let border = match (slot.tile_reuse, slot.code_assignment, slot.plane_usage) {
            (Decision::Protected, _, _) => [0xFF, 0x5A, 0x5F],
            (_, Decision::Protected, _) => [0xFF, 0xA5, 0x30],
            (_, _, PlaneUsage::Blank) => [0x54, 0xA8, 0xFF],
            _ => [0x4E, 0x58, 0x69],
        };
        draw_border(
            &mut pixels,
            width,
            left,
            top,
            cell_width,
            cell_height,
            border,
        );
        let tile = &page[index * TILE_SIZE..(index + 1) * TILE_SIZE];
        draw_tile(&mut pixels, width, tile, left + 2, top + 2, scale);
        draw_hex_label(
            &mut pixels,
            width,
            slot.code,
            left + (cell_width - 7 * label_scale) / 2,
            top + tile_pixels + 4,
            label_scale,
        );
    }

    encode_rgb_png(width, height, &pixels)
}

pub(super) fn draw_tile(
    pixels: &mut [u8],
    width: u32,
    tile: &[u8],
    left: u32,
    top: u32,
    scale: u32,
) {
    const PALETTE: [[u8; 3]; 4] = [
        [0x08, 0x0C, 0x12],
        [0x6A, 0x7C, 0x92],
        [0xB9, 0xC7, 0xD8],
        [0xF4, 0xF7, 0xFB],
    ];
    for row in 0..8 {
        for column in 0..8 {
            let shift = 7 - column;
            let value = ((tile[row] >> shift) & 1) | (((tile[row + 8] >> shift) & 1) << 1);
            for y in 0..scale {
                for x in 0..scale {
                    set_pixel(
                        pixels,
                        width,
                        left + column as u32 * scale + x,
                        top + row as u32 * scale + y,
                        PALETTE[value as usize],
                    );
                }
            }
        }
    }
}

pub(super) fn draw_hex_label(
    pixels: &mut [u8],
    width: u32,
    code: u8,
    left: u32,
    top: u32,
    scale: u32,
) {
    for (digit_index, digit) in [code >> 4, code & 0x0F].iter().enumerate() {
        for (row, bits) in HEX_GLYPHS[*digit as usize].iter().enumerate() {
            for column in 0..3 {
                if bits & (1 << (2 - column)) == 0 {
                    continue;
                }
                for y in 0..scale {
                    for x in 0..scale {
                        set_pixel(
                            pixels,
                            width,
                            left + (digit_index as u32 * 4 + column) * scale + x,
                            top + row as u32 * scale + y,
                            [0xE8, 0xEC, 0xF2],
                        );
                    }
                }
            }
        }
    }
}

pub(super) fn draw_border(
    pixels: &mut [u8],
    width: u32,
    left: u32,
    top: u32,
    box_width: u32,
    box_height: u32,
    color: [u8; 3],
) {
    for x in left..left + box_width {
        set_pixel(pixels, width, x, top, color);
        set_pixel(pixels, width, x, top + box_height - 1, color);
    }
    for y in top..top + box_height {
        set_pixel(pixels, width, left, y, color);
        set_pixel(pixels, width, left + box_width - 1, y, color);
    }
}

pub(super) fn set_pixel(pixels: &mut [u8], width: u32, x: u32, y: u32, color: [u8; 3]) {
    let offset = ((y * width + x) * 3) as usize;
    pixels[offset..offset + 3].copy_from_slice(&color);
}

pub(super) fn encode_rgb_png(width: u32, height: u32, pixels: &[u8]) -> Result<Vec<u8>> {
    let mut encoded = Cursor::new(Vec::new());
    {
        let mut encoder = png::Encoder::new(&mut encoded, width, height);
        encoder.set_color(png::ColorType::Rgb);
        encoder.set_depth(png::BitDepth::Eight);
        let mut writer = encoder.write_header().context("write font sheet header")?;
        writer
            .write_image_data(pixels)
            .context("write font sheet pixels")?;
    }
    Ok(encoded.into_inner())
}

pub(super) fn write_file(path: &Path, data: &[u8]) -> Result<()> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)
            .with_context(|| format!("create output directory {}", parent.display()))?;
    }
    fs::write(path, data).with_context(|| format!("write {}", path.display()))
}

pub(super) fn hex_bytes(bytes: &[u8]) -> String {
    bytes
        .iter()
        .map(|byte| format!("{byte:02X}"))
        .collect::<Vec<_>>()
        .join(" ")
}
