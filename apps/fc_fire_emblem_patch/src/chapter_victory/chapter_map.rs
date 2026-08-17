use anyhow::{Context, Result, ensure};
use serde::Serialize;

use crate::{
    chapter_map_source::{
        EARLY_CHAPTER_MAP_BANK, EARLY_CHAPTER_MAP_COUNT, EARLY_CHAPTER_MAP_POINTER_TABLE,
        bind_chapter_map_source_records,
    },
    rom::HEADER_SIZE,
    sha1_hex,
};

const CHAPTER_ELEVEN_NUMBER: u8 = 11;
const CHAPTER_ELEVEN_INDEX: usize = 10;
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
    let maps = bind_chapter_map_source_records(prg)?;
    let map = *maps
        .get(CHAPTER_ELEVEN_INDEX)
        .context("chapter-eleven map is absent")?;
    ensure!(
        map.chapter_number() == CHAPTER_ELEVEN_NUMBER && map.prg_bank() == EARLY_CHAPTER_MAP_BANK,
        "chapter-eleven map identity changed"
    );
    let storage = map.storage_bytes(prg)?;
    let payload = storage
        .get(4..)
        .context("chapter-eleven map payload is absent")?;
    let victory_tiles = bind_victory_tiles(payload, map.column_count())?;
    let header = map.header();

    Ok(ChapterMapBinding {
        chapter_number_one_based: CHAPTER_ELEVEN_NUMBER,
        chapter_index_zero_based: CHAPTER_ELEVEN_INDEX as u8,
        pointer_table: map_location(EARLY_CHAPTER_MAP_BANK, EARLY_CHAPTER_MAP_POINTER_TABLE),
        pointer_count: EARLY_CHAPTER_MAP_COUNT,
        map_pointer: map.cpu_address(),
        map_pointer_hex: format!("0x{:04X}", map.cpu_address()),
        map_file_offset: HEADER_SIZE + map.prg_offset(),
        map_file_offset_hex: format!("0x{:05X}", HEADER_SIZE + map.prg_offset()),
        maximum_row_index: header[0],
        maximum_column_index: header[1],
        row_count: map.row_count(),
        column_count: map.column_count(),
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

fn map_location(prg_bank: u8, cpu_address: u16) -> MapLocation {
    MapLocation {
        prg_bank,
        prg_bank_hex: format!("0x{prg_bank:02X}"),
        cpu_address,
        cpu_address_hex: format!("0x{cpu_address:04X}"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{chapter_map_source::install_chapter_map_source_fixture, rom::PRG_SIZE};

    fn fixture_prg() -> Vec<u8> {
        let mut prg = vec![0; PRG_SIZE];
        install_chapter_map_source_fixture(&mut prg);
        let map = bind_chapter_map_source_records(&prg).unwrap()[CHAPTER_ELEVEN_INDEX];
        let payload_start = map.prg_offset() + 4;
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
        let map_offset =
            bind_chapter_map_source_records(&prg).unwrap()[CHAPTER_ELEVEN_INDEX].prg_offset() + 4;
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
