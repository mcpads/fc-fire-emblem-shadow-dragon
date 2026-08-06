use std::{fs, path::Path};

use anyhow::{Context, Result, bail, ensure};
use clap::ValueEnum;
use serde::Serialize;

use crate::sha1_hex;

const NAMETABLE_PAGE_LEN: usize = 0x400;
const TILE_COLUMN_COUNT: usize = 32;
const TILE_ROW_COUNT: usize = 30;
const TILE_COUNT: usize = TILE_COLUMN_COUNT * TILE_ROW_COUNT;
const ATTRIBUTE_TABLE_OFFSET: usize = 0x3C0;

#[derive(Debug, Clone, Copy, PartialEq, Eq, ValueEnum)]
pub(crate) enum Mmc4Latch {
    Fd,
    Fe,
}

impl Mmc4Latch {
    fn label(self) -> &'static str {
        match self {
            Self::Fd => "FD",
            Self::Fe => "FE",
        }
    }
}

#[derive(Debug, Serialize)]
struct LatchNametableReport {
    schema: u32,
    input_sha1: String,
    input_len: usize,
    nametable_index: usize,
    traversal: &'static str,
    trigger_application: &'static str,
    fd_bank: u8,
    fe_bank: u8,
    initial_latch: &'static str,
    ending_latch: &'static str,
    fd_trigger_count: usize,
    fe_trigger_count: usize,
    tile_attribute_count: usize,
    unused_tail_fill: u8,
    output_len: usize,
    output_sha1: String,
    unresolved_boundaries: Vec<&'static str>,
    release_eligible: bool,
}

pub struct ProjectionSummary {
    pub output_sha1: String,
    pub report_sha1: String,
    pub fd_trigger_count: usize,
    pub fe_trigger_count: usize,
    pub ending_latch: &'static str,
}

struct ProjectedAttributes {
    bytes: Vec<u8>,
    fd_trigger_count: usize,
    fe_trigger_count: usize,
    ending_latch: Mmc4Latch,
}

pub fn project_mmc4_latch_nametable(
    input_path: &Path,
    nametable_index: usize,
    fd_bank: u8,
    fe_bank: u8,
    initial_latch: Mmc4Latch,
    output_path: &Path,
    report_path: &Path,
) -> Result<ProjectionSummary> {
    let input = fs::read(input_path)
        .with_context(|| format!("read nametable input {}", input_path.display()))?;
    let nametable = select_nametable_page(&input, nametable_index)?;
    let projection = project_attributes(nametable, fd_bank, fe_bank, initial_latch)?;
    let unused_tail_fill = bank_for_latch(initial_latch, fd_bank, fe_bank);
    let output_sha1 = sha1_hex(&projection.bytes);
    let report = LatchNametableReport {
        schema: 1,
        input_sha1: sha1_hex(&input),
        input_len: input.len(),
        nametable_index,
        traversal: "zero-scroll row-major 32x30 background tile order",
        trigger_application: "the trigger tile uses the previous latch; FD or FE applies to following tiles",
        fd_bank,
        fe_bank,
        initial_latch: initial_latch.label(),
        ending_latch: projection.ending_latch.label(),
        fd_trigger_count: projection.fd_trigger_count,
        fe_trigger_count: projection.fe_trigger_count,
        tile_attribute_count: TILE_COUNT,
        unused_tail_fill,
        output_len: projection.bytes.len(),
        output_sha1: output_sha1.clone(),
        unresolved_boundaries: vec![
            "The projection does not model fine scroll, the PPU's two-tile prefetch, or cross-nametable fetch order.",
            "MMC5 extended attributes are one-screen mirrored, so simultaneous nametable ownership still needs a viewport-aware policy.",
            "Sprite CHR selection remains on the ordinary MMC5 CHR registers and is not represented by this output.",
            "The output is a developer probe input, not a release asset or a runtime update implementation.",
        ],
        release_eligible: false,
    };
    let report_bytes =
        serde_json::to_vec_pretty(&report).context("serialize MMC4 latch projection report")?;
    write_file(output_path, &projection.bytes)?;
    write_file(report_path, &report_bytes)?;
    Ok(ProjectionSummary {
        output_sha1,
        report_sha1: sha1_hex(&report_bytes),
        fd_trigger_count: projection.fd_trigger_count,
        fe_trigger_count: projection.fe_trigger_count,
        ending_latch: projection.ending_latch.label(),
    })
}

fn select_nametable_page(input: &[u8], nametable_index: usize) -> Result<&[u8]> {
    match input.len() {
        NAMETABLE_PAGE_LEN => {
            ensure!(
                nametable_index == 0,
                "a 1 KiB nametable input only has index 0"
            );
            Ok(input)
        }
        len if len == NAMETABLE_PAGE_LEN * 2 => {
            ensure!(
                nametable_index < 2,
                "a 2 KiB nametable input only has indices 0 and 1"
            );
            let start = nametable_index * NAMETABLE_PAGE_LEN;
            Ok(&input[start..start + NAMETABLE_PAGE_LEN])
        }
        len => bail!("nametable input must be exactly 1024 or 2048 bytes, found {len}"),
    }
}

fn project_attributes(
    nametable: &[u8],
    fd_bank: u8,
    fe_bank: u8,
    initial_latch: Mmc4Latch,
) -> Result<ProjectedAttributes> {
    ensure!(
        nametable.len() == NAMETABLE_PAGE_LEN,
        "selected nametable page must be exactly 1024 bytes"
    );
    ensure!(fd_bank < 0x40, "FD bank must fit MMC5 ExRAM bits 0-5");
    ensure!(fe_bank < 0x40, "FE bank must fit MMC5 ExRAM bits 0-5");

    let unused_tail_fill = bank_for_latch(initial_latch, fd_bank, fe_bank);
    let mut bytes = vec![unused_tail_fill; NAMETABLE_PAGE_LEN];
    let mut latch = initial_latch;
    let mut fd_trigger_count = 0;
    let mut fe_trigger_count = 0;
    for row in 0..TILE_ROW_COUNT {
        for column in 0..TILE_COLUMN_COUNT {
            let tile_index = row * TILE_COLUMN_COUNT + column;
            let palette = tile_palette(nametable, column, row);
            let bank = bank_for_latch(latch, fd_bank, fe_bank);
            bytes[tile_index] = (palette << 6) | bank;

            match nametable[tile_index] {
                0xFD => {
                    fd_trigger_count += 1;
                    latch = Mmc4Latch::Fd;
                }
                0xFE => {
                    fe_trigger_count += 1;
                    latch = Mmc4Latch::Fe;
                }
                _ => {}
            }
        }
    }

    Ok(ProjectedAttributes {
        bytes,
        fd_trigger_count,
        fe_trigger_count,
        ending_latch: latch,
    })
}

fn bank_for_latch(latch: Mmc4Latch, fd_bank: u8, fe_bank: u8) -> u8 {
    match latch {
        Mmc4Latch::Fd => fd_bank,
        Mmc4Latch::Fe => fe_bank,
    }
}

fn tile_palette(nametable: &[u8], column: usize, row: usize) -> u8 {
    let attribute_index = ATTRIBUTE_TABLE_OFFSET + (row / 4) * 8 + column / 4;
    let shift = ((row % 4) / 2) * 4 + ((column % 4) / 2) * 2;
    (nametable[attribute_index] >> shift) & 0x03
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
    fn trigger_tile_uses_the_previous_bank_and_changes_following_tiles() {
        let mut nametable = vec![0; NAMETABLE_PAGE_LEN];
        nametable[1] = 0xFD;
        nametable[3] = 0xFE;

        let projection = project_attributes(&nametable, 0x00, 0x18, Mmc4Latch::Fe).unwrap();

        assert_eq!(&projection.bytes[..5], &[0x18, 0x18, 0x00, 0x00, 0x18]);
        assert_eq!(projection.fd_trigger_count, 1);
        assert_eq!(projection.fe_trigger_count, 1);
        assert_eq!(projection.ending_latch, Mmc4Latch::Fe);
    }

    #[test]
    fn original_attribute_quadrants_are_preserved_in_the_exram_high_bits() {
        let mut nametable = vec![0; NAMETABLE_PAGE_LEN];
        nametable[ATTRIBUTE_TABLE_OFFSET] = 0b11_10_01_00;

        let projection = project_attributes(&nametable, 0x07, 0x18, Mmc4Latch::Fd).unwrap();

        assert_eq!(projection.bytes[0], 0x07);
        assert_eq!(projection.bytes[2], 0x47);
        assert_eq!(projection.bytes[TILE_COLUMN_COUNT * 2], 0x87);
        assert_eq!(projection.bytes[TILE_COLUMN_COUNT * 2 + 2], 0xC7);
    }

    #[test]
    fn unused_attribute_tail_is_filled_with_the_initial_latch_bank() {
        let nametable = vec![0; NAMETABLE_PAGE_LEN];

        let projection = project_attributes(&nametable, 0x07, 0x18, Mmc4Latch::Fe).unwrap();

        assert!(
            projection.bytes[TILE_COUNT..]
                .iter()
                .all(|byte| *byte == 0x18)
        );
    }

    #[test]
    fn input_size_index_and_six_bit_bank_limits_are_fail_closed() {
        assert!(select_nametable_page(&vec![0; NAMETABLE_PAGE_LEN], 1).is_err());
        assert!(select_nametable_page(&vec![0; NAMETABLE_PAGE_LEN * 2], 2).is_err());
        assert!(select_nametable_page(&vec![0; 1000], 0).is_err());
        assert!(
            project_attributes(&vec![0; NAMETABLE_PAGE_LEN], 0x40, 0x18, Mmc4Latch::Fe).is_err()
        );
    }
}
