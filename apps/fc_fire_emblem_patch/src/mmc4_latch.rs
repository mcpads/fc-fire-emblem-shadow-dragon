use std::{fs, path::Path};

use anyhow::{Context, Result, bail, ensure};
use clap::ValueEnum;
use serde::{Deserialize, Serialize};

use crate::sha1_hex;

const NAMETABLE_PAGE_LEN: usize = 0x400;
const TILE_COLUMN_COUNT: usize = 32;
const TILE_ROW_COUNT: usize = 30;
const TILE_COUNT: usize = TILE_COLUMN_COUNT * TILE_ROW_COUNT;
const ATTRIBUTE_TABLE_OFFSET: usize = 0x3C0;
const PHYSICAL_NAMETABLE_COUNT: usize = 2;
const PPU_ADDRESS_SPACE_LEN: u16 = 0x4000;

#[derive(Debug, Clone, Copy, PartialEq, Eq, ValueEnum, Deserialize)]
#[serde(rename_all = "lowercase")]
pub(crate) enum Mmc4Latch {
    Fd,
    Fe,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "lowercase")]
pub(crate) enum NametableMirroring {
    Horizontal,
    Vertical,
}

impl NametableMirroring {
    fn label(self) -> &'static str {
        match self {
            Self::Horizontal => "horizontal",
            Self::Vertical => "vertical",
        }
    }

    fn physical_nametable(self, logical_nametable: usize) -> Result<usize> {
        ensure!(
            logical_nametable < 4,
            "logical nametable index must be between 0 and 3"
        );
        Ok(match self {
            Self::Horizontal => logical_nametable / 2,
            Self::Vertical => logical_nametable % 2,
        })
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "lowercase")]
pub(crate) enum PpuAddressIncrement {
    Across,
    Down,
}

impl PpuAddressIncrement {
    fn step(self) -> u16 {
        match self {
            Self::Across => 1,
            Self::Down => 32,
        }
    }
}

pub(crate) struct Mmc4NametableShadow {
    physical_nametables: Vec<u8>,
}

impl Mmc4NametableShadow {
    pub(crate) fn filled(value: u8) -> Self {
        Self {
            physical_nametables: vec![value; NAMETABLE_PAGE_LEN * PHYSICAL_NAMETABLE_COUNT],
        }
    }

    pub(crate) fn apply_ppu_transfer(
        &mut self,
        start_address: u16,
        increment: PpuAddressIncrement,
        data: &[u8],
        mirroring: NametableMirroring,
    ) -> Result<usize> {
        ensure!(
            start_address < PPU_ADDRESS_SPACE_LEN,
            "PPU transfer start address must fit the 14-bit PPU address space"
        );
        let mut address = start_address;
        let mut nametable_write_count = 0;
        for &value in data {
            if let Some((logical_nametable, page_offset)) = decode_nametable_address(address) {
                let physical_nametable = mirroring.physical_nametable(logical_nametable)?;
                let physical_offset = physical_nametable * NAMETABLE_PAGE_LEN + page_offset;
                self.physical_nametables[physical_offset] = value;
                nametable_write_count += 1;
            }
            address = address.wrapping_add(increment.step()) & (PPU_ADDRESS_SPACE_LEN - 1);
        }
        Ok(nametable_write_count)
    }

    #[cfg(test)]
    pub(crate) fn physical_bytes(&self) -> &[u8] {
        &self.physical_nametables
    }

    pub(crate) fn project_zero_scroll_attributes(
        &self,
        logical_nametable: usize,
        mirroring: NametableMirroring,
        fd_bank: u8,
        fe_bank: u8,
        initial_latch: Mmc4Latch,
    ) -> Result<Vec<u8>> {
        let physical_nametable = mirroring.physical_nametable(logical_nametable)?;
        let start = physical_nametable * NAMETABLE_PAGE_LEN;
        let projection = project_attributes(
            &self.physical_nametables[start..start + NAMETABLE_PAGE_LEN],
            fd_bank,
            fe_bank,
            initial_latch,
        )?;
        Ok(projection.bytes)
    }
}

fn decode_nametable_address(address: u16) -> Option<(usize, usize)> {
    let mirrored_address = match address {
        0x2000..=0x2FFF => address,
        0x3000..=0x3EFF => address - 0x1000,
        _ => return None,
    };
    let offset = usize::from(mirrored_address - 0x2000);
    Some((offset / NAMETABLE_PAGE_LEN, offset % NAMETABLE_PAGE_LEN))
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

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct PpuTransferReplayInput {
    schema: u32,
    initial_nametable_byte: u8,
    mirroring: NametableMirroring,
    selected_logical_nametable: usize,
    fd_bank: u8,
    fe_bank: u8,
    initial_latch: Mmc4Latch,
    transfers: Vec<PpuTransferInput>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct PpuTransferInput {
    start_address: u16,
    increment: PpuAddressIncrement,
    data_hex: String,
}

#[derive(Debug, Serialize)]
struct PpuTransferReplayReport {
    schema: u32,
    input_sha1: String,
    input_transfer_count: usize,
    input_data_byte_count: usize,
    nametable_write_count: usize,
    non_nametable_write_count: usize,
    mirroring: &'static str,
    selected_logical_nametable: usize,
    fd_bank: u8,
    fe_bank: u8,
    initial_latch: &'static str,
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

pub struct TransferReplaySummary {
    pub output_sha1: String,
    pub report_sha1: String,
    pub nametable_write_count: usize,
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

pub fn replay_mmc4_latch_ppu_transfers(
    input_path: &Path,
    output_path: &Path,
    report_path: &Path,
) -> Result<TransferReplaySummary> {
    let input_bytes = fs::read(input_path)
        .with_context(|| format!("read PPU transfer input {}", input_path.display()))?;
    let input: PpuTransferReplayInput = serde_json::from_slice(&input_bytes)
        .with_context(|| format!("parse PPU transfer input {}", input_path.display()))?;
    ensure!(input.schema == 1, "PPU transfer replay schema must be 1");
    ensure!(
        !input.transfers.is_empty(),
        "PPU transfer replay must contain at least one transfer"
    );

    let decoded_transfers = input
        .transfers
        .iter()
        .map(|transfer| {
            Ok((
                transfer,
                decode_hex(&transfer.data_hex).with_context(|| {
                    format!(
                        "decode PPU transfer at address 0x{:04X}",
                        transfer.start_address
                    )
                })?,
            ))
        })
        .collect::<Result<Vec<_>>>()?;
    let input_data_byte_count = decoded_transfers
        .iter()
        .map(|(_, data)| data.len())
        .sum::<usize>();
    ensure!(
        input_data_byte_count > 0,
        "PPU transfer replay contains no data bytes"
    );

    let mut shadow = Mmc4NametableShadow::filled(input.initial_nametable_byte);
    let mut nametable_write_count = 0;
    for (transfer, data) in &decoded_transfers {
        nametable_write_count += shadow.apply_ppu_transfer(
            transfer.start_address,
            transfer.increment,
            data,
            input.mirroring,
        )?;
    }
    ensure!(
        nametable_write_count > 0,
        "PPU transfer replay does not write a nametable"
    );
    let output = shadow.project_zero_scroll_attributes(
        input.selected_logical_nametable,
        input.mirroring,
        input.fd_bank,
        input.fe_bank,
        input.initial_latch,
    )?;
    let output_sha1 = sha1_hex(&output);
    let report = PpuTransferReplayReport {
        schema: 1,
        input_sha1: sha1_hex(&input_bytes),
        input_transfer_count: input.transfers.len(),
        input_data_byte_count,
        nametable_write_count,
        non_nametable_write_count: input_data_byte_count - nametable_write_count,
        mirroring: input.mirroring.label(),
        selected_logical_nametable: input.selected_logical_nametable,
        fd_bank: input.fd_bank,
        fe_bank: input.fe_bank,
        initial_latch: input.initial_latch.label(),
        output_len: output.len(),
        output_sha1: output_sha1.clone(),
        unresolved_boundaries: vec![
            "The replay is a host-side semantic reference and does not prove that the ROM owns every PPU write.",
            "The selected viewport is zero-scroll and does not model the PPU's two-tile prefetch or cross-nametable fetch order.",
            "Input transfer payloads are local evidence and are represented in the report only by aggregate counts and the input SHA-1.",
        ],
        release_eligible: false,
    };
    let report_bytes =
        serde_json::to_vec_pretty(&report).context("serialize PPU transfer replay report")?;

    write_file(output_path, &output)?;
    write_file(report_path, &report_bytes)?;
    Ok(TransferReplaySummary {
        output_sha1,
        report_sha1: sha1_hex(&report_bytes),
        nametable_write_count,
    })
}

fn decode_hex(encoded: &str) -> Result<Vec<u8>> {
    ensure!(
        encoded.len().is_multiple_of(2),
        "hex payload must contain an even number of digits"
    );
    encoded
        .as_bytes()
        .chunks_exact(2)
        .map(|pair| {
            let high = decode_hex_digit(pair[0])?;
            let low = decode_hex_digit(pair[1])?;
            Ok((high << 4) | low)
        })
        .collect()
}

fn decode_hex_digit(digit: u8) -> Result<u8> {
    match digit {
        b'0'..=b'9' => Ok(digit - b'0'),
        b'a'..=b'f' => Ok(digit - b'a' + 10),
        b'A'..=b'F' => Ok(digit - b'A' + 10),
        _ => bail!("hex payload contains a non-hex digit"),
    }
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
    fn ppu_transfers_update_the_selected_physical_nametable_before_projection() {
        let mut nametable = vec![0; NAMETABLE_PAGE_LEN];
        nametable[1] = 0xFD;
        nametable[3] = 0xFE;
        nametable[ATTRIBUTE_TABLE_OFFSET] = 0b11_10_01_00;
        let expected = project_attributes(&nametable, 0x00, 0x18, Mmc4Latch::Fe)
            .unwrap()
            .bytes;
        let mut shadow = Mmc4NametableShadow::filled(0xFF);

        let write_count = shadow
            .apply_ppu_transfer(
                0x2400,
                PpuAddressIncrement::Across,
                &nametable,
                NametableMirroring::Vertical,
            )
            .unwrap();
        let actual = shadow
            .project_zero_scroll_attributes(
                1,
                NametableMirroring::Vertical,
                0x00,
                0x18,
                Mmc4Latch::Fe,
            )
            .unwrap();

        assert_eq!(write_count, NAMETABLE_PAGE_LEN);
        assert_eq!(actual, expected);
    }

    #[test]
    fn mirroring_selects_the_physical_page_that_owns_a_logical_write() {
        let mut horizontal = Mmc4NametableShadow::filled(0);
        horizontal
            .apply_ppu_transfer(
                0x2400,
                PpuAddressIncrement::Across,
                &[0xFD],
                NametableMirroring::Horizontal,
            )
            .unwrap();
        let horizontal_page_zero = horizontal
            .project_zero_scroll_attributes(
                0,
                NametableMirroring::Horizontal,
                0x00,
                0x18,
                Mmc4Latch::Fe,
            )
            .unwrap();

        let mut vertical = Mmc4NametableShadow::filled(0);
        vertical
            .apply_ppu_transfer(
                0x2400,
                PpuAddressIncrement::Across,
                &[0xFD],
                NametableMirroring::Vertical,
            )
            .unwrap();
        let vertical_page_zero = vertical
            .project_zero_scroll_attributes(
                0,
                NametableMirroring::Vertical,
                0x00,
                0x18,
                Mmc4Latch::Fe,
            )
            .unwrap();

        assert_eq!(&horizontal_page_zero[..2], &[0x18, 0x00]);
        assert_eq!(&vertical_page_zero[..2], &[0x18, 0x18]);
    }

    #[test]
    fn vertical_ppu_increment_changes_latch_state_between_rows() {
        let mut shadow = Mmc4NametableShadow::filled(0);
        shadow
            .apply_ppu_transfer(
                0x2000,
                PpuAddressIncrement::Down,
                &[0xFD, 0xFE],
                NametableMirroring::Vertical,
            )
            .unwrap();

        let projection = shadow
            .project_zero_scroll_attributes(
                0,
                NametableMirroring::Vertical,
                0x00,
                0x18,
                Mmc4Latch::Fe,
            )
            .unwrap();

        assert_eq!(projection[0], 0x18);
        assert!(
            projection[1..=TILE_COLUMN_COUNT]
                .iter()
                .all(|byte| *byte == 0)
        );
        assert_eq!(projection[TILE_COLUMN_COUNT + 1], 0x18);
    }

    #[test]
    fn attribute_table_writes_update_palette_bits_without_changing_chr_banks() {
        let mut shadow = Mmc4NametableShadow::filled(0);
        shadow
            .apply_ppu_transfer(
                0x23C0,
                PpuAddressIncrement::Across,
                &[0b11_10_01_00],
                NametableMirroring::Vertical,
            )
            .unwrap();

        let projection = shadow
            .project_zero_scroll_attributes(
                0,
                NametableMirroring::Vertical,
                0x07,
                0x18,
                Mmc4Latch::Fd,
            )
            .unwrap();

        assert_eq!(projection[0] & 0x3F, 0x07);
        assert_eq!(projection[2], 0x47);
        assert_eq!(projection[TILE_COLUMN_COUNT * 2], 0x87);
        assert_eq!(projection[TILE_COLUMN_COUNT * 2 + 2], 0xC7);
    }

    #[test]
    fn mirrored_ppu_addresses_update_nametables_and_palette_addresses_do_not() {
        let mut shadow = Mmc4NametableShadow::filled(0);
        let mirrored_count = shadow
            .apply_ppu_transfer(
                0x3000,
                PpuAddressIncrement::Across,
                &[0xFD],
                NametableMirroring::Vertical,
            )
            .unwrap();
        let palette_count = shadow
            .apply_ppu_transfer(
                0x3F00,
                PpuAddressIncrement::Across,
                &[0xFE],
                NametableMirroring::Vertical,
            )
            .unwrap();
        let projection = shadow
            .project_zero_scroll_attributes(
                0,
                NametableMirroring::Vertical,
                0x00,
                0x18,
                Mmc4Latch::Fe,
            )
            .unwrap();

        assert_eq!(mirrored_count, 1);
        assert_eq!(palette_count, 0);
        assert_eq!(&projection[..2], &[0x18, 0x00]);
    }

    #[test]
    fn ppu_transfer_replay_input_is_typed_and_fail_closed() {
        let input: PpuTransferReplayInput = serde_json::from_str(
            r#"{
                "schema": 1,
                "initial_nametable_byte": 255,
                "mirroring": "horizontal",
                "selected_logical_nametable": 1,
                "fd_bank": 0,
                "fe_bank": 24,
                "initial_latch": "fe",
                "transfers": [{
                    "start_address": 8192,
                    "increment": "down",
                    "data_hex": "FDfe"
                }]
            }"#,
        )
        .unwrap();

        assert_eq!(input.mirroring, NametableMirroring::Horizontal);
        assert_eq!(input.transfers[0].increment, PpuAddressIncrement::Down);
        assert_eq!(
            decode_hex(&input.transfers[0].data_hex).unwrap(),
            [0xFD, 0xFE]
        );
        assert!(decode_hex("f").is_err());
        assert!(decode_hex("fg").is_err());
        assert!(
            serde_json::from_str::<PpuTransferReplayInput>(
                r#"{
                    "schema": 1,
                    "initial_nametable_byte": 0,
                    "mirroring": "vertical",
                    "selected_logical_nametable": 0,
                    "fd_bank": 0,
                    "fe_bank": 24,
                    "initial_latch": "fd",
                    "transfers": [],
                    "unexpected": true
                }"#,
            )
            .is_err()
        );
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
