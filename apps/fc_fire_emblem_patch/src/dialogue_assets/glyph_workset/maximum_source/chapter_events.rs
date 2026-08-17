use anyhow::{Context, Result, ensure};
use serde::Serialize;

use crate::{
    chapter_map_source::{
        ChapterMapSourceRecord, EARLY_CHAPTER_MAP_BANK, EARLY_CHAPTER_MAP_COUNT,
        EARLY_CHAPTER_MAP_POINTER_TABLE, bind_chapter_map_source_records,
    },
    rom::HEADER_SIZE,
    sha1_hex,
};

use super::source_regions::{PRG_BANK_SIZE, prg_offset};

pub(super) const CHAPTER_EVENT_POINTER_BANK: u8 = 0x03;
pub(super) const CHAPTER_EVENT_POINTER_ADDRESS: u16 = 0xA0F1;
const CHAPTER_COUNT: usize = 25;
pub(super) const CHAPTER_EVENT_RECORD_SIZE: usize = 5;
pub(super) const CHAPTER_EVENT_POINTERS: [u16; CHAPTER_COUNT] = [
    0xA123, 0xA14C, 0xA166, 0xA180, 0xA1A9, 0xA1C8, 0xA1E7, 0xA1F7, 0xA207, 0xA226, 0xA240, 0xA273,
    0xA297, 0xA2AC, 0xA2CB, 0xA2E5, 0xA313, 0xA33C, 0xA347, 0xA37A, 0xA385, 0xA38B, 0xA39B, 0xA3AB,
    0xA3BB,
];

pub(super) const CHAPTER_MAP_BANK: u8 = EARLY_CHAPTER_MAP_BANK;
const CHAPTER_MAP_POINTER_ADDRESS: u16 = EARLY_CHAPTER_MAP_POINTER_TABLE;

#[derive(Debug, Serialize)]
pub(super) struct DataRegionBinding {
    role: &'static str,
    prg_bank: u8,
    prg_bank_hex: String,
    cpu_address: u16,
    cpu_address_hex: String,
    file_offset: usize,
    file_offset_hex: String,
    byte_count: usize,
    source_sha1: String,
}

#[derive(Debug)]
pub(super) struct ChapterEventRecord {
    pub(super) chapter_number: u8,
    pub(super) record_index: usize,
    pub(super) row: u8,
    pub(super) column: u8,
    pub(super) event_code: u8,
    pub(super) dialogue_entry: u8,
    pub(super) value: u8,
    pub(super) file_offset: usize,
}

pub(super) struct ChapterMapSample {
    pub(super) pointer: u16,
    pub(super) tile_code: u8,
    pub(super) map_sha1: String,
}

pub(super) fn bind_chapter_event_directory(
    prg: &[u8],
) -> Result<(Vec<ChapterEventRecord>, DataRegionBinding)> {
    let pointer_offset = prg_offset(CHAPTER_EVENT_POINTER_BANK, CHAPTER_EVENT_POINTER_ADDRESS)?;
    let pointer_byte_count = CHAPTER_COUNT * 2;
    let pointer_bytes = prg
        .get(pointer_offset..pointer_offset + pointer_byte_count)
        .context("chapter-event pointer table is outside PRG")?;
    let pointers = pointer_bytes
        .chunks_exact(2)
        .map(|bytes| u16::from_le_bytes([bytes[0], bytes[1]]))
        .collect::<Vec<_>>();
    ensure!(
        pointers == CHAPTER_EVENT_POINTERS,
        "chapter-event pointer table changed"
    );

    let mut records = Vec::new();
    for (chapter_index, &pointer) in pointers.iter().enumerate() {
        let start = prg_offset(CHAPTER_EVENT_POINTER_BANK, pointer)?;
        let end = pointers
            .get(chapter_index + 1)
            .copied()
            .map(|next| prg_offset(CHAPTER_EVENT_POINTER_BANK, next))
            .transpose()?
            .unwrap_or((usize::from(CHAPTER_EVENT_POINTER_BANK) + 1) * PRG_BANK_SIZE);
        ensure!(
            start < end,
            "chapter-event table pointers are not increasing"
        );
        let mut cursor = start;
        let mut record_index = 0;
        loop {
            let row = *prg
                .get(cursor)
                .context("chapter-event table has no terminator")?;
            if row == 0 {
                break;
            }
            let bytes = prg
                .get(cursor..cursor + CHAPTER_EVENT_RECORD_SIZE)
                .context("chapter-event record is truncated")?;
            ensure!(
                cursor + CHAPTER_EVENT_RECORD_SIZE <= end,
                "chapter-event record crosses the next table pointer"
            );
            records.push(ChapterEventRecord {
                chapter_number: (chapter_index + 1) as u8,
                record_index,
                row: bytes[0],
                column: bytes[1],
                event_code: bytes[2],
                dialogue_entry: bytes[3],
                value: bytes[4],
                file_offset: HEADER_SIZE + cursor,
            });
            cursor += CHAPTER_EVENT_RECORD_SIZE;
            record_index += 1;
        }
        ensure!(
            cursor < end,
            "chapter-event terminator crosses table boundary"
        );
    }

    Ok((
        records,
        data_region_binding(
            prg,
            "chapter_event_pointer_table",
            CHAPTER_EVENT_POINTER_BANK,
            CHAPTER_EVENT_POINTER_ADDRESS,
            pointer_byte_count,
        )?,
    ))
}

pub(super) fn bind_chapter_map_pointers(
    prg: &[u8],
) -> Result<(Vec<ChapterMapSourceRecord>, DataRegionBinding)> {
    let pointers = bind_chapter_map_source_records(prg)?
        .into_iter()
        .filter(|record| record.prg_bank() == CHAPTER_MAP_BANK)
        .collect::<Vec<_>>();
    ensure!(
        pointers.len() == EARLY_CHAPTER_MAP_COUNT,
        "early chapter-map pointer population changed"
    );
    let pointer_byte_count = EARLY_CHAPTER_MAP_COUNT * 2;
    Ok((
        pointers,
        data_region_binding(
            prg,
            "chapter_map_pointer_table",
            CHAPTER_MAP_BANK,
            CHAPTER_MAP_POINTER_ADDRESS,
            pointer_byte_count,
        )?,
    ))
}

pub(super) fn bind_all_chapter_map_pointers(prg: &[u8]) -> Result<Vec<ChapterMapSourceRecord>> {
    bind_chapter_map_source_records(prg)
}

pub(super) fn bind_chapter_event_tile_code(
    prg: &[u8],
    map_pointers: &[ChapterMapSourceRecord],
    record: &ChapterEventRecord,
) -> Result<u8> {
    let map = *map_pointers
        .get(usize::from(record.chapter_number) - 1)
        .context("chapter event has no map pointer")?;
    ensure!(
        map.chapter_number() == record.chapter_number,
        "chapter event map order changed"
    );
    map.tile_code(prg, record.row, record.column)
}

pub(super) fn bind_chapter_map_sample(
    prg: &[u8],
    map_pointers: &[ChapterMapSourceRecord],
    record: &ChapterEventRecord,
    expected_pointer: u16,
    expected_header: [u8; 4],
) -> Result<ChapterMapSample> {
    let map = *map_pointers
        .get(usize::from(record.chapter_number) - 1)
        .context("chapter event has no map pointer")?;
    ensure!(
        map.chapter_number() == record.chapter_number && map.cpu_address() == expected_pointer,
        "chapter {} map pointer changed",
        record.chapter_number
    );
    ensure!(
        map.header() == expected_header,
        "chapter {} map header changed",
        record.chapter_number
    );
    Ok(ChapterMapSample {
        pointer: map.cpu_address(),
        tile_code: map.tile_code(prg, record.row, record.column)?,
        map_sha1: sha1_hex(map.storage_bytes(prg)?),
    })
}

fn data_region_binding(
    prg: &[u8],
    role: &'static str,
    prg_bank: u8,
    cpu_address: u16,
    byte_count: usize,
) -> Result<DataRegionBinding> {
    let offset = prg_offset(prg_bank, cpu_address)?;
    let bytes = prg
        .get(offset..offset + byte_count)
        .with_context(|| format!("{role} is outside PRG"))?;
    Ok(DataRegionBinding {
        role,
        prg_bank,
        prg_bank_hex: format!("0x{prg_bank:02X}"),
        cpu_address,
        cpu_address_hex: format!("0x{cpu_address:04X}"),
        file_offset: HEADER_SIZE + offset,
        file_offset_hex: format!("0x{:05X}", HEADER_SIZE + offset),
        byte_count,
        source_sha1: sha1_hex(bytes),
    })
}

#[cfg(test)]
pub(super) fn install_event_and_map_fixture(prg: &mut [u8]) {
    crate::chapter_map_source::install_chapter_map_source_fixture(prg);
    let event_pointer_offset =
        prg_offset(CHAPTER_EVENT_POINTER_BANK, CHAPTER_EVENT_POINTER_ADDRESS).unwrap();
    for (index, pointer) in CHAPTER_EVENT_POINTERS.iter().enumerate() {
        prg[event_pointer_offset + index * 2..event_pointer_offset + index * 2 + 2]
            .copy_from_slice(&pointer.to_le_bytes());
        prg[prg_offset(CHAPTER_EVENT_POINTER_BANK, *pointer).unwrap()] = 0;
    }
    write_event_table(
        prg,
        CHAPTER_EVENT_POINTERS[6],
        &[
            [14, 13, 0x80, 0x17, 4],
            [27, 10, 0, 0x18, 0x20],
            [5, 8, 0, 0x13, 0],
        ],
    );
    write_event_table(
        prg,
        CHAPTER_EVENT_POINTERS[10],
        &[
            [8, 4, 0, 0x1C, 0x20],
            [8, 5, 0, 0x1C, 0x20],
            [8, 6, 0, 0x1C, 0x20],
            [22, 27, 0x80, 0x21, 4],
            [15, 21, 0, 0x15, 0],
            [15, 24, 0, 0x19, 0],
            [17, 22, 0, 0x18, 0],
        ],
    );
    write_map(prg, 0x8F12, [0x1D, 0x0F, 0x0E, 0], 27, 10, 0x4B);
    write_map(prg, 0x9C42, [0x18, 0x1F, 0x0A, 0], 17, 22, 0x46);
}

#[cfg(test)]
fn write_event_table(prg: &mut [u8], pointer: u16, records: &[[u8; 5]]) {
    let offset = prg_offset(CHAPTER_EVENT_POINTER_BANK, pointer).unwrap();
    for (index, record) in records.iter().enumerate() {
        let start = offset + index * CHAPTER_EVENT_RECORD_SIZE;
        prg[start..start + CHAPTER_EVENT_RECORD_SIZE].copy_from_slice(record);
    }
    prg[offset + records.len() * CHAPTER_EVENT_RECORD_SIZE] = 0;
}

#[cfg(test)]
fn write_map(prg: &mut [u8], pointer: u16, header: [u8; 4], row: u8, column: u8, tile: u8) {
    let offset = prg_offset(CHAPTER_MAP_BANK, pointer).unwrap();
    prg[offset..offset + 4].copy_from_slice(&header);
    let column_count = usize::from(header[1]) + 1;
    prg[offset + 4 + usize::from(row) * column_count + usize::from(column)] = tile;
}
