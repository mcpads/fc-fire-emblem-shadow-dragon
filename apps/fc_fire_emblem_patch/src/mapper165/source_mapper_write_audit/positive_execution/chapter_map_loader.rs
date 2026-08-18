use std::ops::RangeInclusive;

use anyhow::{Context, Result, ensure};
use retro_rp2a03::{AddressingMode, Mnemonic, Operand, decode_bytes};

use crate::{
    chapter_map_source::{ChapterMapSourceRecord, bind_chapter_map_source_records},
    mapper165::battle_codebook_plan::IndirectWriteDestinationBounds,
    rom::Rom,
    sha1_hex,
    typed_source::decode_rp2a03_sequence,
};

const SOURCE_PRG_BANK_BYTE_COUNT: usize = 16 * 1024;
const FIXED_PRG_BANK: u8 = 0x0F;
const FIXED_CPU_START: u16 = 0xC000;

const CHAPTER_MAP_LOADER_START: u16 = 0xD385;
const CHAPTER_MAP_LOADER_END: u16 = 0xD40B;
const CHAPTER_MAP_LOADER_SHA1: &str = "6046f0e34403c13d70a860752d3e18f1ae8b3568";
const CHAPTER_MAP_WRITER: u16 = 0xD3DA;
const CHAPTER_MAP_DESTINATION_POINTER: u8 = 0x6A;
const CHAPTER_MAP_DESTINATION_START: u16 = 0x72AF;
const CHAPTER_MAP_ROW_STRIDE: usize = 0x20;
const CURRENT_CHAPTER_ADDRESS: u16 = 0x7674;

pub(super) const CHAPTER_MAP_INDIRECT_WRITE_SITE: (u8, u16, u8) = (
    FIXED_PRG_BANK,
    CHAPTER_MAP_WRITER,
    CHAPTER_MAP_DESTINATION_POINTER,
);

pub(super) struct BoundChapterMapDimensions {
    maximum_row_index: u8,
    maximum_column_index: u8,
}

impl BoundChapterMapDimensions {
    pub(super) fn maximum_row_index(&self) -> u8 {
        self.maximum_row_index
    }

    pub(super) fn maximum_column_index(&self) -> u8 {
        self.maximum_column_index
    }
}

pub(super) struct ChapterMapLoaderContract {
    indirect_write_destination: IndirectWriteDestinationBounds,
    dimensions: BoundChapterMapDimensions,
}

impl ChapterMapLoaderContract {
    pub(super) fn indirect_write_destination(&self) -> &IndirectWriteDestinationBounds {
        &self.indirect_write_destination
    }

    pub(super) fn dimensions(&self) -> &BoundChapterMapDimensions {
        &self.dimensions
    }
}

pub(super) fn bind_chapter_map_loader(source: &Rom) -> Result<ChapterMapLoaderContract> {
    bind_chapter_map_loader_code(source)?;
    let records = bind_chapter_map_source_records(source.prg())?;
    let destination_ranges = records
        .iter()
        .map(chapter_map_destination_range)
        .collect::<Result<Vec<_>>>()?;
    let maximum_end = destination_ranges
        .iter()
        .map(|range| *range.end())
        .max()
        .context("chapter map source contains no destination range")?;
    ensure!(
        destination_ranges
            .iter()
            .all(|range| *range.start() == CHAPTER_MAP_DESTINATION_START),
        "chapter map loader destination base changed"
    );
    let maximum_row_index = records
        .iter()
        .map(|record| record.row_count() - 1)
        .max()
        .context("chapter map source contains no row domain")?;
    let maximum_column_index = records
        .iter()
        .map(|record| record.column_count() - 1)
        .max()
        .context("chapter map source contains no column domain")?;
    ensure!(
        maximum_row_index < CHAPTER_MAP_ROW_STRIDE && maximum_column_index < CHAPTER_MAP_ROW_STRIDE,
        "chapter map dimensions escape the source 32-byte row layout"
    );
    Ok(ChapterMapLoaderContract {
        indirect_write_destination: IndirectWriteDestinationBounds::from_source_ranges(
            "chapter_map_ram_image",
            vec![CHAPTER_MAP_DESTINATION_START..=maximum_end],
        )?,
        dimensions: BoundChapterMapDimensions {
            maximum_row_index: u8::try_from(maximum_row_index)?,
            maximum_column_index: u8::try_from(maximum_column_index)?,
        },
    })
}

fn chapter_map_destination_range(record: &ChapterMapSourceRecord) -> Result<RangeInclusive<u16>> {
    destination_range_for_dimensions(record.row_count(), record.column_count()).with_context(|| {
        format!(
            "chapter {} map cannot fit the source loader destination",
            record.chapter_number()
        )
    })
}

fn destination_range_for_dimensions(
    row_count: usize,
    column_count: usize,
) -> Result<RangeInclusive<u16>> {
    ensure!(row_count > 0, "chapter map has no rows");
    ensure!(column_count > 0, "chapter map has no columns");
    ensure!(
        column_count <= CHAPTER_MAP_ROW_STRIDE,
        "chapter map width {column_count} exceeds the loader row stride {CHAPTER_MAP_ROW_STRIDE}"
    );
    let last_row_offset = row_count
        .checked_sub(1)
        .and_then(|rows| rows.checked_mul(CHAPTER_MAP_ROW_STRIDE))
        .context("chapter map row destination offset overflow")?;
    let last_column_offset = column_count
        .checked_sub(1)
        .context("chapter map column destination offset underflow")?;
    let final_offset = last_row_offset
        .checked_add(last_column_offset)
        .context("chapter map destination offset overflow")?;
    let destination_end = CHAPTER_MAP_DESTINATION_START
        .checked_add(u16::try_from(final_offset)?)
        .context("chapter map destination address overflow")?;
    ensure!(
        destination_end < 0x8000,
        "chapter map destination reaches mapper register space at ${destination_end:04X}"
    );
    ensure!(
        destination_end < CURRENT_CHAPTER_ADDRESS,
        "chapter map destination overlaps current-chapter state at ${destination_end:04X}"
    );
    Ok(CHAPTER_MAP_DESTINATION_START..=destination_end)
}

fn bind_chapter_map_loader_code(source: &Rom) -> Result<()> {
    let byte_count = usize::from(CHAPTER_MAP_LOADER_END - CHAPTER_MAP_LOADER_START);
    let bytes = fixed_source_bytes(source, CHAPTER_MAP_LOADER_START, byte_count)?;
    ensure!(
        sha1_hex(bytes) == CHAPTER_MAP_LOADER_SHA1,
        "source chapter map loader changed"
    );
    decode_rp2a03_sequence(bytes, CHAPTER_MAP_LOADER_START, "source chapter map loader")?;

    for (address, mnemonic, mode, operand) in [
        (
            0xD38B,
            Mnemonic::Lda,
            AddressingMode::Absolute,
            Operand::Word(0x7674),
        ),
        (
            0xD389,
            Mnemonic::Ldy,
            AddressingMode::Immediate,
            Operand::Byte(0x02),
        ),
        (
            0xD392,
            Mnemonic::Sbc,
            AddressingMode::Immediate,
            Operand::Byte(0x0D),
        ),
        (
            0xD394,
            Mnemonic::Ldy,
            AddressingMode::Immediate,
            Operand::Byte(0x09),
        ),
        (
            0xD39C,
            Mnemonic::Jsr,
            AddressingMode::Absolute,
            Operand::Word(0xC9A6),
        ),
        (
            0xD3A5,
            Mnemonic::Lda,
            AddressingMode::AbsoluteY,
            Operand::Word(0x8000),
        ),
        (
            0xD3AA,
            Mnemonic::Lda,
            AddressingMode::AbsoluteY,
            Operand::Word(0x8001),
        ),
        (
            0xD3AF,
            Mnemonic::Lda,
            AddressingMode::Immediate,
            Operand::Byte(0xAF),
        ),
        (
            0xD3B1,
            Mnemonic::Sta,
            AddressingMode::ZeroPage,
            Operand::Byte(0x6A),
        ),
        (
            0xD3B3,
            Mnemonic::Lda,
            AddressingMode::Immediate,
            Operand::Byte(0x72),
        ),
        (
            0xD3BB,
            Mnemonic::Sta,
            AddressingMode::Absolute,
            Operand::Word(0x7676),
        ),
        (
            0xD3C5,
            Mnemonic::Sta,
            AddressingMode::Absolute,
            Operand::Word(0x7677),
        ),
        (
            0xD3B5,
            Mnemonic::Sta,
            AddressingMode::ZeroPage,
            Operand::Byte(0x6B),
        ),
        (
            CHAPTER_MAP_WRITER,
            Mnemonic::Sta,
            AddressingMode::ZeroPageIndirectIndexedY,
            Operand::Byte(CHAPTER_MAP_DESTINATION_POINTER),
        ),
        (
            0xD3E7,
            Mnemonic::Ldy,
            AddressingMode::Immediate,
            Operand::Byte(0x20),
        ),
        (
            0xD3E9,
            Mnemonic::Jsr,
            AddressingMode::Absolute,
            Operand::Word(0xD400),
        ),
        (
            0xD3F2,
            Mnemonic::Jmp,
            AddressingMode::Absolute,
            Operand::Word(0xC9A6),
        ),
    ] {
        ensure_loader_instruction(bytes, address, mnemonic, mode, operand)?;
    }
    Ok(())
}

fn ensure_loader_instruction(
    loader: &[u8],
    address: u16,
    mnemonic: Mnemonic,
    mode: AddressingMode,
    operand: Operand,
) -> Result<()> {
    let offset = usize::from(
        address
            .checked_sub(CHAPTER_MAP_LOADER_START)
            .context("chapter map loader instruction precedes its source region")?,
    );
    let instruction = decode_bytes(
        loader
            .get(offset..)
            .context("chapter map loader instruction is outside its source region")?,
    )
    .with_context(|| format!("decode chapter map loader instruction at ${address:04X}"))?;
    ensure!(
        instruction.mnemonic() == mnemonic
            && instruction.addressing_mode() == mode
            && instruction.operand() == operand,
        "chapter map loader instruction changed at ${address:04X}"
    );
    Ok(())
}

fn fixed_source_bytes(source: &Rom, address: u16, byte_count: usize) -> Result<&[u8]> {
    ensure!(
        address >= FIXED_CPU_START,
        "chapter map loader address is below the fixed PRG window"
    );
    let offset = usize::from(FIXED_PRG_BANK)
        .checked_mul(SOURCE_PRG_BANK_BYTE_COUNT)
        .and_then(|base| base.checked_add(usize::from(address - FIXED_CPU_START)))
        .context("chapter map loader PRG offset overflow")?;
    source
        .prg()
        .get(offset..offset + byte_count)
        .context("chapter map loader is outside source PRG")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{chapter_map_source::install_chapter_map_source_fixture, rom::PRG_SIZE};

    #[test]
    fn every_supported_chapter_map_fits_the_same_ram_image() {
        let mut prg = vec![0; PRG_SIZE];
        install_chapter_map_source_fixture(&mut prg);
        let records = bind_chapter_map_source_records(&prg).unwrap();
        let ranges = records
            .iter()
            .map(chapter_map_destination_range)
            .collect::<Result<Vec<_>>>()
            .unwrap();

        assert_eq!(ranges.len(), 25);
        assert!(
            ranges
                .iter()
                .all(|range| *range.start() == CHAPTER_MAP_DESTINATION_START)
        );
        assert_eq!(ranges.iter().map(|range| *range.end()).max(), Some(0x766E));
    }

    #[test]
    fn rejects_a_map_wider_than_the_loader_row_stride() {
        let error = destination_range_for_dimensions(30, 33).unwrap_err();

        assert!(error.to_string().contains("exceeds the loader row stride"));
    }

    #[test]
    fn rejects_a_map_whose_last_cell_reaches_mapper_space() {
        let error = destination_range_for_dimensions(107, 32).unwrap_err();

        assert!(error.to_string().contains("reaches mapper register space"));
    }

    #[test]
    fn rejects_a_map_that_overwrites_the_current_chapter_state() {
        let error = destination_range_for_dimensions(31, 32).unwrap_err();

        assert!(error.to_string().contains("overlaps current-chapter state"));
    }
}
