use anyhow::{Context, Result, ensure};
use serde::Serialize;

use crate::{rom::HEADER_SIZE, sha1_hex};

const PRG_BANK_SIZE: usize = 16 * 1024;
const CHAPTER_MAP_BANK: u8 = 0x02;
const CHAPTER_MAP_POINTER_TABLE_ADDRESS: u16 = 0x8000;
const CHAPTER_MAP_POINTER_COUNT: usize = 13;
const CHAPTER_ELEVEN_INDEX: usize = 10;
const CHAPTER_ELEVEN_MAP_ADDRESS: u16 = 0x9C42;
const CHAPTER_ELEVEN_HEADER: [u8; 4] = [0x18, 0x1F, 0x0A, 0x00];
const CHAPTER_ELEVEN_ROW_COUNT: usize = 25;
const CHAPTER_ELEVEN_COLUMN_COUNT: usize = 32;
const CASTLE_TILE_CODE: u8 = 0x4B;
const CASTLE_LABEL_INDEX: u8 = 0x38;

#[derive(Debug, Serialize)]
pub(super) struct ChapterMapBinding {
    chapter_number_one_based: u8,
    chapter_index_zero_based: u8,
    pointer_table: MapLocation,
    pointer_count: usize,
    map_pointer: u16,
    map_pointer_hex: String,
    map_file_offset: usize,
    map_file_offset_hex: String,
    maximum_row_index: u8,
    maximum_column_index: u8,
    row_count: usize,
    column_count: usize,
    map_payload_sha1: String,
    pub(super) victory_tiles: Vec<MapTile>,
}

#[derive(Debug, Serialize, PartialEq, Eq)]
pub(super) struct MapTile {
    row: u8,
    column: u8,
    tile_code: u8,
    tile_code_hex: String,
    command_label_index: u8,
    command_label_index_hex: String,
    source_label: &'static str,
    translation_handling: &'static str,
}

#[derive(Debug, Serialize)]
struct MapLocation {
    prg_bank: u8,
    prg_bank_hex: String,
    cpu_address: u16,
    cpu_address_hex: String,
}

pub(super) fn bind_chapter_eleven_map(prg: &[u8]) -> Result<ChapterMapBinding> {
    let table_offset = map_prg_offset(CHAPTER_MAP_POINTER_TABLE_ADDRESS)?;
    let table_end = table_offset + CHAPTER_MAP_POINTER_COUNT * 2;
    let table = prg
        .get(table_offset..table_end)
        .context("chapter map pointer table is outside PRG")?;
    let pointers = table
        .chunks_exact(2)
        .map(|bytes| u16::from_le_bytes([bytes[0], bytes[1]]))
        .collect::<Vec<_>>();
    ensure!(
        pointers.len() == CHAPTER_MAP_POINTER_COUNT,
        "chapter map pointer count changed"
    );
    ensure!(
        pointers[CHAPTER_ELEVEN_INDEX] == CHAPTER_ELEVEN_MAP_ADDRESS,
        "chapter-eleven map pointer changed"
    );

    let map_offset = map_prg_offset(CHAPTER_ELEVEN_MAP_ADDRESS)?;
    let header = prg
        .get(map_offset..map_offset + CHAPTER_ELEVEN_HEADER.len())
        .context("chapter-eleven map header is outside PRG")?;
    ensure!(
        header == CHAPTER_ELEVEN_HEADER,
        "chapter-eleven map header changed"
    );
    let row_count = usize::from(header[0]) + 1;
    let column_count = usize::from(header[1]) + 1;
    ensure!(
        row_count == CHAPTER_ELEVEN_ROW_COUNT,
        "chapter-eleven row count changed"
    );
    ensure!(
        column_count == CHAPTER_ELEVEN_COLUMN_COUNT,
        "chapter-eleven column count changed"
    );
    let payload_start = map_offset + CHAPTER_ELEVEN_HEADER.len();
    let payload_len = row_count
        .checked_mul(column_count)
        .context("chapter-eleven map dimensions overflow")?;
    let payload = prg
        .get(payload_start..payload_start + payload_len)
        .context("chapter-eleven map payload is outside PRG")?;
    let victory_tiles = bind_victory_tiles(payload, column_count)?;

    Ok(ChapterMapBinding {
        chapter_number_one_based: 11,
        chapter_index_zero_based: CHAPTER_ELEVEN_INDEX as u8,
        pointer_table: map_location(CHAPTER_MAP_POINTER_TABLE_ADDRESS),
        pointer_count: pointers.len(),
        map_pointer: CHAPTER_ELEVEN_MAP_ADDRESS,
        map_pointer_hex: format!("0x{CHAPTER_ELEVEN_MAP_ADDRESS:04X}"),
        map_file_offset: HEADER_SIZE + map_offset,
        map_file_offset_hex: format!("0x{:05X}", HEADER_SIZE + map_offset),
        maximum_row_index: header[0],
        maximum_column_index: header[1],
        row_count,
        column_count,
        map_payload_sha1: sha1_hex(payload),
        victory_tiles,
    })
}

fn bind_victory_tiles(payload: &[u8], column_count: usize) -> Result<Vec<MapTile>> {
    let tiles = payload
        .iter()
        .enumerate()
        .filter(|(_, tile)| **tile == CASTLE_TILE_CODE)
        .map(|(index, tile)| MapTile {
            row: (index / column_count) as u8,
            column: (index % column_count) as u8,
            tile_code: *tile,
            tile_code_hex: format!("0x{tile:02X}"),
            command_label_index: CASTLE_LABEL_INDEX,
            command_label_index_hex: format!("0x{CASTLE_LABEL_INDEX:02X}"),
            source_label: "しろ",
            translation_handling: "translate Japanese only",
        })
        .collect::<Vec<_>>();
    ensure!(tiles.len() == 2, "chapter-eleven castle tile count changed");
    ensure!(
        tiles[0].row == 8 && tiles[0].column == 5,
        "first chapter-eleven castle coordinate changed"
    );
    ensure!(
        tiles[1].row == 8 && tiles[1].column == 6,
        "second chapter-eleven castle coordinate changed"
    );
    Ok(tiles)
}

fn map_prg_offset(cpu_address: u16) -> Result<usize> {
    ensure!(
        (0x8000..0xC000).contains(&cpu_address),
        "chapter map address is outside 0x8000..0xBFFF"
    );
    Ok(usize::from(CHAPTER_MAP_BANK) * PRG_BANK_SIZE + usize::from(cpu_address - 0x8000))
}

fn map_location(cpu_address: u16) -> MapLocation {
    MapLocation {
        prg_bank: CHAPTER_MAP_BANK,
        prg_bank_hex: format!("0x{CHAPTER_MAP_BANK:02X}"),
        cpu_address,
        cpu_address_hex: format!("0x{cpu_address:04X}"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::rom::PRG_SIZE;

    fn fixture_prg() -> Vec<u8> {
        let mut prg = vec![0; PRG_SIZE];
        let pointer_table_offset = map_prg_offset(CHAPTER_MAP_POINTER_TABLE_ADDRESS).unwrap();
        for index in 0..CHAPTER_MAP_POINTER_COUNT {
            let pointer = if index == CHAPTER_ELEVEN_INDEX {
                CHAPTER_ELEVEN_MAP_ADDRESS
            } else {
                0x9000 + index as u16 * 0x10
            };
            prg[pointer_table_offset + index * 2..pointer_table_offset + index * 2 + 2]
                .copy_from_slice(&pointer.to_le_bytes());
        }
        let map_offset = map_prg_offset(CHAPTER_ELEVEN_MAP_ADDRESS).unwrap();
        prg[map_offset..map_offset + 4].copy_from_slice(&CHAPTER_ELEVEN_HEADER);
        let payload_start = map_offset + 4;
        prg[payload_start + 8 * 32 + 5] = CASTLE_TILE_CODE;
        prg[payload_start + 8 * 32 + 6] = CASTLE_TILE_CODE;
        prg
    }

    #[test]
    fn binds_only_the_two_horizontal_chapter_eleven_castle_tiles() {
        let binding = bind_chapter_eleven_map(&fixture_prg()).unwrap();
        assert_eq!(binding.row_count, 25);
        assert_eq!(binding.column_count, 32);
        assert_eq!(
            binding
                .victory_tiles
                .iter()
                .map(|tile| (tile.row, tile.column, tile.tile_code))
                .collect::<Vec<_>>(),
            [(8, 5, 0x4B), (8, 6, 0x4B)]
        );
    }

    #[test]
    fn fails_closed_when_a_victory_tile_moves() {
        let mut prg = fixture_prg();
        let map_offset = map_prg_offset(CHAPTER_ELEVEN_MAP_ADDRESS).unwrap() + 4;
        prg[map_offset + 8 * 32 + 5] = 0;
        prg[map_offset + 8 * 32 + 7] = CASTLE_TILE_CODE;
        assert!(
            bind_chapter_eleven_map(&prg)
                .unwrap_err()
                .to_string()
                .contains("coordinate changed")
        );
    }
}
