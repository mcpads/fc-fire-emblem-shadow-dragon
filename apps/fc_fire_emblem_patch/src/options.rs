use std::{fs, io::Cursor, path::Path};

use anyhow::{Context, Result, bail};

use crate::{
    localization::{OptionsLocalization, ValidatedLocalization},
    rom::{CHR_FILE_OFFSET, Rom},
    sha1_hex,
    tracked::{TrackedImage, WriteReport},
};

pub(crate) const OPTIONS_TABLE_OFFSET: usize = 0x2CADE;
pub(crate) const SOURCE_OPTIONS_TABLE: [u8; 24] = [
    0x3A, 0x32, 0x5F, 0x44, 0x0F, 0xED, 0x30, 0x46, 0x53, 0x3F, 0x3B, 0x8B, 0x5F, 0xED, 0x32, 0x33,
    0x31, 0x44, 0x40, 0x31, 0x50, 0x3F, 0xED, 0xEF,
];

const SOURCE_JAPANESE_TILES: &[(u8, [u8; 16])] = &[
    (
        0x30,
        [
            0x00, 0xFE, 0x02, 0x14, 0x10, 0x10, 0x10, 0x20, 0, 0, 0, 0, 0, 0, 0, 0,
        ],
    ),
    (
        0x31,
        [
            0x04, 0x04, 0x08, 0x10, 0x68, 0x08, 0x08, 0x08, 0, 0, 0, 0, 0, 0, 0, 0,
        ],
    ),
    (
        0x32,
        [
            0x10, 0x10, 0xFC, 0x84, 0x84, 0x04, 0x08, 0x70, 0, 0, 0, 0, 0, 0, 0, 0,
        ],
    ),
    (
        0x33,
        [
            0x00, 0x7C, 0x10, 0x10, 0x10, 0x10, 0x10, 0xFE, 0, 0, 0, 0, 0, 0, 0, 0,
        ],
    ),
    (
        0x3A,
        [
            0x44, 0x44, 0xFE, 0x44, 0x44, 0x04, 0x04, 0x38, 0, 0, 0, 0, 0, 0, 0, 0,
        ],
    ),
    (
        0x3B,
        [
            0x02, 0xE2, 0x02, 0xE2, 0x02, 0x02, 0x04, 0xF8, 0, 0, 0, 0, 0, 0, 0, 0,
        ],
    ),
    (
        0x3F,
        [
            0x00, 0x00, 0x00, 0x00, 0x7C, 0x00, 0x00, 0x00, 0, 0, 0, 0, 0, 0, 0, 0,
        ],
    ),
    (
        0x40,
        [
            0x40, 0x7C, 0x44, 0xA4, 0x14, 0x08, 0x08, 0x30, 0, 0, 0, 0, 0, 0, 0, 0,
        ],
    ),
    (
        0x44,
        [
            0x20, 0x20, 0x20, 0x38, 0x24, 0x22, 0x20, 0x20, 0, 0, 0, 0, 0, 0, 0, 0,
        ],
    ),
    (
        0x46,
        [
            0x00, 0x7C, 0x00, 0x00, 0x00, 0x00, 0x00, 0xFE, 0, 0, 0, 0, 0, 0, 0, 0,
        ],
    ),
    (
        0x50,
        [
            0x00, 0xFE, 0x02, 0x04, 0x44, 0x28, 0x10, 0x08, 0, 0, 0, 0, 0, 0, 0, 0,
        ],
    ),
    (
        0x53,
        [
            0x08, 0x08, 0x48, 0x28, 0x10, 0x18, 0x24, 0xC0, 0, 0, 0, 0, 0, 0, 0, 0,
        ],
    ),
];

pub struct BuildReport {
    pub output_sha1: String,
    pub writes: Vec<WriteReport>,
}

pub fn build_options_poc(
    source_path: &Path,
    localization_path: &Path,
    output_path: &Path,
    preview_path: &Path,
    preview_scale: u32,
) -> Result<BuildReport> {
    let source_rom = Rom::from_path(source_path)?;
    source_rom.verify_supported_japanese()?;
    let localization = OptionsLocalization::from_path(localization_path)?.validate()?;
    let preview = encode_preview(&localization, preview_scale)?;

    let source = source_rom.data().to_vec();
    let mut image = TrackedImage::new(source.clone());
    image.write_expected(
        "Japanese options text table",
        OPTIONS_TABLE_OFFSET,
        &SOURCE_OPTIONS_TABLE,
        &localization.replacement_table,
    )?;
    for (code, expected_tile) in SOURCE_JAPANESE_TILES {
        let replacement = localization
            .tiles
            .get(code)
            .ok_or_else(|| anyhow::anyhow!("missing replacement tile {code:02X}"))?;
        image.write_expected(
            format!("Japanese glyph {code:02X}"),
            CHR_FILE_OFFSET + *code as usize * 16,
            expected_tile,
            replacement,
        )?;
    }
    image.verify_all_changes_tracked(&source)?;
    let writes = image.writes().to_vec();
    let output = image.into_data();

    write_file(output_path, &output)?;
    write_file(preview_path, &preview)?;
    Ok(BuildReport {
        output_sha1: sha1_hex(&output),
        writes,
    })
}

fn encode_preview(localization: &ValidatedLocalization, scale: u32) -> Result<Vec<u8>> {
    if scale == 0 {
        bail!("preview scale must be greater than zero");
    }
    let columns = localization
        .entries
        .iter()
        .map(|entry| entry.korean_codes.len())
        .max()
        .unwrap_or(0);
    let row_gap = 2_u32;
    let width = columns as u32 * 8 * scale;
    let height = (localization.entries.len() as u32 * 8
        + (localization.entries.len().saturating_sub(1) as u32 * row_gap))
        * scale;
    let mut pixels = vec![0x08_u8; (width * height) as usize];

    for (entry_index, entry) in localization.entries.iter().enumerate() {
        let row_origin = entry_index as u32 * (8 + row_gap) * scale;
        for (glyph_index, code) in entry.korean_codes.iter().enumerate() {
            let tile = localization.tiles[code];
            for (row, row_bits) in tile.iter().take(8).enumerate() {
                for column in 0..8_usize {
                    if row_bits & (1 << (7 - column)) == 0 {
                        continue;
                    }
                    for sy in 0..scale as usize {
                        for sx in 0..scale as usize {
                            let x = glyph_index * 8 * scale as usize + column * scale as usize + sx;
                            let y = row_origin as usize + row * scale as usize + sy;
                            pixels[y * width as usize + x] = 0xFF;
                        }
                    }
                }
            }
        }
    }

    let mut encoded = Cursor::new(Vec::new());
    {
        let mut encoder = png::Encoder::new(&mut encoded, width, height);
        encoder.set_color(png::ColorType::Grayscale);
        encoder.set_depth(png::BitDepth::Eight);
        let mut writer = encoder.write_header().context("write preview PNG header")?;
        writer
            .write_image_data(&pixels)
            .context("write preview PNG pixels")?;
    }
    Ok(encoded.into_inner())
}

fn write_file(path: &Path, data: &[u8]) -> Result<()> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)
            .with_context(|| format!("create output directory {}", parent.display()))?;
    }
    fs::write(path, data).with_context(|| format!("write {}", path.display()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_hangul_tile_replaces_a_known_japanese_option_glyph() {
        let localization: OptionsLocalization = serde_json::from_str(include_str!(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../../assets/translation/options.ko.json"
        )))
        .unwrap();
        let validated = localization.validate().unwrap();

        let expected_codes: Vec<u8> = SOURCE_JAPANESE_TILES
            .iter()
            .map(|(code, _)| *code)
            .collect();
        let actual_codes: Vec<u8> = validated.tiles.keys().copied().collect();
        assert_eq!(actual_codes, expected_codes);
    }
}
