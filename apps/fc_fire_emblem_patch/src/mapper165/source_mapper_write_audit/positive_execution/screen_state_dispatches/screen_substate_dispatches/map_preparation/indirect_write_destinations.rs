use std::{
    collections::{BTreeMap, BTreeSet},
    ops::RangeInclusive,
};

use anyhow::{Context, Result, ensure};

use crate::{mapper165::battle_codebook_plan::IndirectWriteDestinationBounds, rom::Rom, sha1_hex};

use super::{
    BoundChapterMapDimensions, BoundUnitRecordAddressDomain, MAP_PREPARATION_BANK,
    fixed_source_bytes,
};

const DISPLAY_ROW_POINTER_TABLE: u16 = 0xED79;
pub(super) const DISPLAY_ROW_POINTER_COUNT: usize = 30;
const DISPLAY_ROW_POINTER_TABLE_SHA1: &str = "cddd4158c6aece55116d1d86f6fe41c56b3df0db";
pub(super) const DISPLAY_ROW_BASE: u16 = 0x7AF0;
pub(super) const DISPLAY_ROW_STRIDE: u16 = 0x20;

const INDIRECT_WRITER_SITES: [(u8, u16, u8); 18] = [
    (MAP_PREPARATION_BANK, 0x8062, 0x6C),
    (MAP_PREPARATION_BANK, 0x80BD, 0x9B),
    (MAP_PREPARATION_BANK, 0x811E, 0x9B),
    (MAP_PREPARATION_BANK, 0x8128, 0x9B),
    (MAP_PREPARATION_BANK, 0x821F, 0x9B),
    (MAP_PREPARATION_BANK, 0x8364, 0x9D),
    (MAP_PREPARATION_BANK, 0x8ACB, 0x6C),
    (MAP_PREPARATION_BANK, 0x8AD5, 0x6C),
    (MAP_PREPARATION_BANK, 0x8BFC, 0x6C),
    (MAP_PREPARATION_BANK, 0x8CC1, 0x6C),
    (MAP_PREPARATION_BANK, 0x8D7A, 0x9F),
    (MAP_PREPARATION_BANK, 0x8DA8, 0x9F),
    (MAP_PREPARATION_BANK, 0x8FEF, 0x6C),
    (MAP_PREPARATION_BANK, 0x912D, 0x6C),
    (MAP_PREPARATION_BANK, 0x933A, 0x9D),
    (MAP_PREPARATION_BANK, 0x9387, 0x9D),
    (MAP_PREPARATION_BANK, 0x93DB, 0x9F),
    (MAP_PREPARATION_BANK, 0x9439, 0x04),
];

pub(super) fn bind_indirect_write_destinations(
    source: &Rom,
    unit_record_domain: &BoundUnitRecordAddressDomain,
    chapter_map_dimensions: &BoundChapterMapDimensions,
) -> Result<BTreeMap<(u8, u16, u8), IndirectWriteDestinationBounds>> {
    let display_row_pointers = bind_display_row_pointers(source)?;
    ensure!(
        usize::from(chapter_map_dimensions.maximum_row_index()) < display_row_pointers.len(),
        "chapter map row domain exceeds the map-preparation display-row table"
    );
    let active_display_rows =
        &display_row_pointers[..=usize::from(chapter_map_dimensions.maximum_row_index())];
    let display_row_destinations = indexed_pointer_destination_ranges(
        active_display_rows,
        chapter_map_dimensions.maximum_column_index(),
    )?;
    let neighboring_display_cell_destinations =
        indexed_pointer_destination_ranges(&display_row_pointers, u8::MAX)?;
    let runtime_row_destinations = unit_record_domain
        .runtime_row_destination_ranges(chapter_map_dimensions.maximum_column_index())?;
    let map_layer_destinations = unit_record_domain.map_layer_row_destination_ranges(
        usize::from(chapter_map_dimensions.maximum_row_index()) + 1,
        chapter_map_dimensions.maximum_column_index(),
    )?;
    let all_map_layer_destinations =
        unit_record_domain.all_map_layer_destination_ranges(u8::MAX)?;
    let shifted_allied_record_destinations =
        unit_record_domain.allied_field_destination_ranges(0x36)?;

    let mut destinations = BTreeMap::new();
    insert_destination(
        &mut destinations,
        (MAP_PREPARATION_BANK, 0x8062, 0x6C),
        "source-bound map-layer rows selected by chapter dimensions",
        map_layer_destinations,
    )?;
    for site in [
        (MAP_PREPARATION_BANK, 0x8ACB, 0x6C),
        (MAP_PREPARATION_BANK, 0x8AD5, 0x6C),
        (MAP_PREPARATION_BANK, 0x8BFC, 0x6C),
        (MAP_PREPARATION_BANK, 0x8CC1, 0x6C),
        (MAP_PREPARATION_BANK, 0x8FEF, 0x6C),
        (MAP_PREPARATION_BANK, 0x912D, 0x6C),
    ] {
        insert_destination(
            &mut destinations,
            site,
            "cell in one source-bound map-layer row",
            all_map_layer_destinations.clone(),
        )?;
    }
    insert_destination(
        &mut destinations,
        (MAP_PREPARATION_BANK, 0x80BD, 0x9B),
        "source-bound map-preparation display rows",
        display_row_destinations,
    )?;
    for site in [
        (MAP_PREPARATION_BANK, 0x811E, 0x9B),
        (MAP_PREPARATION_BANK, 0x8128, 0x9B),
        (MAP_PREPARATION_BANK, 0x821F, 0x9B),
    ] {
        insert_destination(
            &mut destinations,
            site,
            "neighboring cell in one source-bound map-preparation display row",
            neighboring_display_cell_destinations.clone(),
        )?;
    }
    for site in [
        (MAP_PREPARATION_BANK, 0x8D7A, 0x9F),
        (MAP_PREPARATION_BANK, 0x8DA8, 0x9F),
    ] {
        insert_destination(
            &mut destinations,
            site,
            "workspace after one source-bound allied unit record",
            shifted_allied_record_destinations.clone(),
        )?;
    }
    insert_destination(
        &mut destinations,
        (MAP_PREPARATION_BANK, 0x8364, 0x9D),
        "field sixteen of the first twenty enemy unit records",
        unit_record_domain.enemy_field_destination_ranges_within(0x14, 0x16)?,
    )?;
    insert_destination(
        &mut destinations,
        (MAP_PREPARATION_BANK, 0x933A, 0x9D),
        "first twenty enemy unit records selected during map preparation",
        vec![unit_record_domain.enemy_record_copy_destination_range(0x14)?],
    )?;
    insert_destination(
        &mut destinations,
        (MAP_PREPARATION_BANK, 0x9387, 0x9D),
        "field two of one source-bound enemy unit record",
        unit_record_domain.enemy_field_destination_ranges(0x02)?,
    )?;
    insert_destination(
        &mut destinations,
        (MAP_PREPARATION_BANK, 0x93DB, 0x9F),
        "field two of one source-bound allied unit record",
        unit_record_domain.allied_field_destination_ranges(0x02)?,
    )?;
    insert_destination(
        &mut destinations,
        (MAP_PREPARATION_BANK, 0x9439, 0x04),
        "runtime map row cell selected by bounded map coordinates",
        runtime_row_destinations,
    )?;
    ensure!(
        destinations.keys().copied().collect::<BTreeSet<_>>()
            == INDIRECT_WRITER_SITES.into_iter().collect(),
        "map-preparation destination owner omitted or invented an indirect writer"
    );
    Ok(destinations)
}

fn bind_display_row_pointers(source: &Rom) -> Result<Vec<u16>> {
    let byte_count = DISPLAY_ROW_POINTER_COUNT
        .checked_mul(2)
        .context("map-preparation display-row table length overflow")?;
    let bytes = fixed_source_bytes(source, DISPLAY_ROW_POINTER_TABLE, byte_count)?;
    ensure!(
        sha1_hex(bytes) == DISPLAY_ROW_POINTER_TABLE_SHA1,
        "map-preparation display-row pointer table changed"
    );
    let pointers = bytes
        .chunks_exact(2)
        .map(|pair| u16::from_le_bytes([pair[0], pair[1]]))
        .collect::<Vec<_>>();
    let expected = (0..DISPLAY_ROW_POINTER_COUNT)
        .map(|index| {
            DISPLAY_ROW_BASE
                .checked_add(u16::try_from(index)? * DISPLAY_ROW_STRIDE)
                .context("map-preparation display-row pointer overflow")
        })
        .collect::<Result<Vec<_>>>()?;
    ensure!(
        pointers == expected,
        "map-preparation display-row pointer values changed"
    );
    Ok(pointers)
}

pub(super) fn indexed_pointer_destination_ranges(
    pointers: &[u16],
    maximum_index: u8,
) -> Result<Vec<RangeInclusive<u16>>> {
    ensure!(
        !pointers.is_empty(),
        "map-preparation pointer domain is empty"
    );
    ensure!(
        pointers.windows(2).all(|pair| pair[0] < pair[1]),
        "map-preparation pointer domain is not strictly ordered"
    );
    let mut merged = Vec::<RangeInclusive<u16>>::new();
    for &pointer in pointers {
        let end = pointer
            .checked_add(u16::from(maximum_index))
            .context("map-preparation indexed destination overflow")?;
        ensure!(
            end < 0x8000,
            "map-preparation destination reaches mapper space at ${end:04X}"
        );
        if let Some(previous) = merged.last_mut()
            && pointer <= previous.end().saturating_add(1)
        {
            let start = *previous.start();
            *previous = start..=end.max(*previous.end());
        } else {
            merged.push(pointer..=end);
        }
    }
    Ok(merged)
}

fn insert_destination(
    destinations: &mut BTreeMap<(u8, u16, u8), IndirectWriteDestinationBounds>,
    site: (u8, u16, u8),
    role: &'static str,
    ranges: Vec<RangeInclusive<u16>>,
) -> Result<()> {
    ensure!(
        destinations
            .insert(
                site,
                IndirectWriteDestinationBounds::from_source_ranges(role, ranges)?,
            )
            .is_none(),
        "map-preparation indirect writer is duplicated at {:02X}:${:04X}",
        site.0,
        site.1,
    );
    Ok(())
}
