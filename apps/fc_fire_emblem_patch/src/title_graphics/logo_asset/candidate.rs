use std::{
    fs,
    io::Cursor,
    path::{Component, Path},
};

use anyhow::{Context, Result, ensure};
use serde::Deserialize;

use crate::sha1_hex;

use super::{LOGO_PIXEL_HEIGHT, LOGO_PIXEL_WIDTH};

const MANIFEST_SCHEMA: u8 = 1;

#[derive(Debug, Deserialize)]
struct CandidateManifest {
    schema: u8,
    candidate_file: String,
    candidate_sha1: String,
    crop: Crop,
    target: TargetPlacement,
}

#[derive(Clone, Copy, Debug, Deserialize)]
struct Crop {
    left: usize,
    top: usize,
    width: usize,
    height: usize,
}

#[derive(Clone, Copy, Debug, Deserialize)]
struct TargetPlacement {
    content_width: usize,
    content_height: usize,
    left_padding: usize,
}

struct RgbImage {
    width: usize,
    height: usize,
    pixels: Vec<u8>,
}

pub(super) struct QuantizedCandidate {
    pub(super) candidate_sha1: String,
    pub(super) logo_indices: Vec<u8>,
}

pub(super) fn load_and_quantize(manifest_path: &Path) -> Result<QuantizedCandidate> {
    let manifest_bytes = fs::read(manifest_path).with_context(|| {
        format!(
            "read title-logo candidate manifest {}",
            manifest_path.display()
        )
    })?;
    let manifest: CandidateManifest =
        serde_json::from_slice(&manifest_bytes).with_context(|| {
            format!(
                "parse title-logo candidate manifest {}",
                manifest_path.display()
            )
        })?;
    validate_manifest(&manifest)?;
    let candidate_path = manifest_path
        .parent()
        .unwrap_or_else(|| Path::new("."))
        .join(&manifest.candidate_file);
    let candidate_bytes = fs::read(&candidate_path)
        .with_context(|| format!("read title-logo candidate {}", candidate_path.display()))?;
    ensure!(
        sha1_hex(&candidate_bytes) == manifest.candidate_sha1,
        "title-logo candidate image changed"
    );
    let candidate = decode_rgb_png(&candidate_bytes)?;
    validate_crop(&candidate, manifest.crop)?;
    let logo_indices = quantize_logo(&candidate, manifest.crop, manifest.target)?;
    let mut palette_index_pixel_counts = [0_usize; 4];
    for index in &logo_indices {
        palette_index_pixel_counts[usize::from(*index)] += 1;
    }
    ensure!(
        palette_index_pixel_counts[1..]
            .iter()
            .all(|count| *count > 0),
        "title-logo candidate does not exercise all three animated palette roles"
    );
    Ok(QuantizedCandidate {
        candidate_sha1: manifest.candidate_sha1,
        logo_indices,
    })
}

fn validate_manifest(manifest: &CandidateManifest) -> Result<()> {
    ensure!(
        manifest.schema == MANIFEST_SCHEMA,
        "unsupported title-logo candidate manifest schema"
    );
    let candidate_path = Path::new(&manifest.candidate_file);
    ensure!(
        !candidate_path.as_os_str().is_empty()
            && candidate_path
                .components()
                .all(|component| matches!(component, Component::Normal(_))),
        "title-logo candidate file must be a relative path"
    );
    ensure!(
        manifest.candidate_sha1.len() == 40
            && manifest
                .candidate_sha1
                .bytes()
                .all(|byte| byte.is_ascii_hexdigit() && !byte.is_ascii_uppercase()),
        "title-logo candidate SHA-1 must be forty lowercase hexadecimal characters"
    );
    ensure!(
        manifest.crop.width > 0
            && manifest.crop.height > 0
            && manifest.target.content_width > 0
            && manifest.target.content_height == LOGO_PIXEL_HEIGHT
            && manifest.target.left_padding + manifest.target.content_width <= LOGO_PIXEL_WIDTH,
        "title-logo crop or target placement is outside the 216x40 logo surface"
    );
    Ok(())
}

fn validate_crop(image: &RgbImage, crop: Crop) -> Result<()> {
    let right = crop
        .left
        .checked_add(crop.width)
        .context("title-logo crop right edge overflow")?;
    let bottom = crop
        .top
        .checked_add(crop.height)
        .context("title-logo crop bottom edge overflow")?;
    ensure!(
        right <= image.width && bottom <= image.height,
        "title-logo crop exceeds the candidate image"
    );
    Ok(())
}

fn decode_rgb_png(bytes: &[u8]) -> Result<RgbImage> {
    let decoder = png::Decoder::new(Cursor::new(bytes));
    let mut reader = decoder.read_info().context("read title-logo PNG header")?;
    let buffer_size = reader
        .output_buffer_size()
        .context("title-logo PNG output is too large")?;
    let mut pixels = vec![0_u8; buffer_size];
    let info = reader
        .next_frame(&mut pixels)
        .context("decode title-logo PNG")?;
    ensure!(
        info.color_type == png::ColorType::Rgb && info.bit_depth == png::BitDepth::Eight,
        "title-logo candidate PNG must use 8-bit RGB pixels"
    );
    pixels.truncate(info.buffer_size());
    Ok(RgbImage {
        width: usize::try_from(info.width).context("title-logo PNG width overflow")?,
        height: usize::try_from(info.height).context("title-logo PNG height overflow")?,
        pixels,
    })
}

fn quantize_logo(image: &RgbImage, crop: Crop, target: TargetPlacement) -> Result<Vec<u8>> {
    let mut indices = vec![0_u8; LOGO_PIXEL_WIDTH * LOGO_PIXEL_HEIGHT];
    for target_y in 0..target.content_height {
        for target_x in 0..target.content_width {
            let sample_left = crop.left + target_x * crop.width / target.content_width;
            let sample_right = crop.left
                + (target_x + 1)
                    .checked_mul(crop.width)
                    .context("title-logo horizontal sample overflow")?
                    .div_ceil(target.content_width);
            let sample_top = crop.top + target_y * crop.height / target.content_height;
            let sample_bottom = crop.top
                + (target_y + 1)
                    .checked_mul(crop.height)
                    .context("title-logo vertical sample overflow")?
                    .div_ceil(target.content_height);
            let rgb = average_rgb(image, sample_left, sample_right, sample_top, sample_bottom)?;
            let output_x = target.left_padding + target_x;
            indices[target_y * LOGO_PIXEL_WIDTH + output_x] = classify_palette_index(rgb);
        }
    }
    ensure!(
        indices.iter().any(|index| *index != 0),
        "title-logo candidate quantized to an empty surface"
    );
    Ok(indices)
}

fn average_rgb(
    image: &RgbImage,
    left: usize,
    right: usize,
    top: usize,
    bottom: usize,
) -> Result<[u8; 3]> {
    ensure!(
        left < right && top < bottom && right <= image.width && bottom <= image.height,
        "title-logo downsample window is invalid"
    );
    let mut sums = [0_u64; 3];
    let mut count = 0_u64;
    for y in top..bottom {
        for x in left..right {
            let offset = (y * image.width + x) * 3;
            for (channel, sum) in sums.iter_mut().enumerate() {
                *sum += u64::from(image.pixels[offset + channel]);
            }
            count += 1;
        }
    }
    Ok([
        u8::try_from(sums[0] / count).expect("averaged red channel fits in u8"),
        u8::try_from(sums[1] / count).expect("averaged green channel fits in u8"),
        u8::try_from(sums[2] / count).expect("averaged blue channel fits in u8"),
    ])
}

fn classify_palette_index([red, green, blue]: [u8; 3]) -> u8 {
    let maximum = red.max(green).max(blue);
    if maximum < 40 {
        return 0;
    }
    if u16::from(blue) >= u16::from(red) + 18 && u16::from(blue) >= u16::from(green) + 8 {
        if red >= 170 && green >= 190 { 2 } else { 3 }
    } else if (u16::from(red) + u16::from(green) + u16::from(blue)) / 3 >= 64 {
        1
    } else {
        0
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn candidate_colors_map_to_the_original_animated_palette_roles() {
        assert_eq!(classify_palette_index([0, 0, 0]), 0);
        assert_eq!(classify_palette_index([255, 255, 255]), 1);
        assert_eq!(classify_palette_index([211, 231, 253]), 2);
        assert_eq!(classify_palette_index([104, 24, 252]), 3);
    }
}
