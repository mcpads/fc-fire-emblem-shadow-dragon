use std::{collections::BTreeMap, ops::RangeInclusive};

use anyhow::{Context, Result, ensure};

use crate::{mapper165::battle_codebook_plan::IndirectWriteDestinationBounds, rom::Rom};

mod chapter_save;
mod gameplay_paths;
mod source_regions;

use chapter_save::bind_chapter_save_path_destinations;
use gameplay_paths::bind_gameplay_path_destinations;
use source_regions::bind_unit_record_writer_source;

const UNIT_RECORD_STRIDE: u16 = 0x1B;
const ALLIED_RECORD_BASE: u16 = 0x6A90;
const ENEMY_RECORD_BASE: u16 = 0x7078;
const RECORD_SCAN_CAPACITY: usize = 0x36;
const RECORD_BYTE_ZERO_OFFSET: u16 = 0x00;
const ACTION_BYTE_OFFSET: u16 = 0x12;
const COPIED_CLASS_OFFSET: u16 = 0x06;
const TURN_COUNTER_OFFSET: u16 = 0x0F;
const DERIVED_MAP_X_OFFSET: u16 = 0x10;
const DERIVED_MAP_Y_OFFSET: u16 = 0x11;
const SHIFTED_RECORD_IDENTITY_OFFSET: u16 = 0x36;
const SHIFTED_RECORD_FIELD_OFFSET: u16 = 0x47;

const FIRST_ACTION_WRITER: (u8, u16, u8) = (0x06, 0x88C9, 0x00);
const SECOND_ACTION_WRITER: (u8, u16, u8) = (0x06, 0x88D9, 0x74);
const FIRST_ALLIED_ACTION_WRITER: (u8, u16, u8) = (0x06, 0x8648, 0x00);
const MATCHED_ALLIED_ACTION_WRITER: (u8, u16, u8) = (0x06, 0x866D, 0x00);
const SHIFTED_RECORD_FIELD_WRITER: (u8, u16, u8) = (0x02, 0xAA55, 0x65);
const SHIFTED_RECORD_IDENTITY_WRITER: (u8, u16, u8) = (0x02, 0xAA5B, 0x65);
const DERIVED_MAP_X_WRITER: (u8, u16, u8) = (0x08, 0xBA99, 0x74);
const DERIVED_MAP_Y_WRITER: (u8, u16, u8) = (0x08, 0xBAA3, 0x74);
const UNIT_RECORD_COPY_WRITER: (u8, u16, u8) = (0x08, 0xBB49, 0x74);
const COPIED_CLASS_WRITER: (u8, u16, u8) = (0x08, 0xBB71, 0x74);
const SELECTED_ALLIED_CLASS_WRITER: (u8, u16, u8) = (0x06, 0x88E9, 0x74);
const SELECTED_ALLIED_BYTE_ZERO_WRITER: (u8, u16, u8) = (0x06, 0x88F2, 0x00);
const MAP_OCCUPANCY_WRITER: (u8, u16, u8) = (0x08, 0xBB7D, 0x00);
const MAP_OCCUPANCY_REFRESH_WRITER: (u8, u16, u8) = (0x06, 0xA205, 0x00);
const ALLIED_TURN_COUNTER_WRITER: (u8, u16, u8) = (0x06, 0xA247, 0x74);
const ALLIED_RECORD_REBUILD_WRITER: (u8, u16, u8) = (0x06, 0xA27C, 0x00);
const MAP_LAYER_CLEAR_WRITER: (u8, u16, u8) = (0x06, 0xBB48, 0x6C);
const ALLIED_ACTION_CLEAR_WRITER: (u8, u16, u8) = (0x06, 0xA20B, 0x74);
const ALLIED_RECORD_BYTE_ZERO_REBUILD_WRITER: (u8, u16, u8) = (0x06, 0xA2A7, 0x00);
const SELECTED_UNIT_MAP_CLASS_WRITER: (u8, u16, u8) = (0x06, 0xB884, 0x00);
const SELECTED_UNIT_MAP_OCCUPANCY_WRITER: (u8, u16, u8) = (0x06, 0xB8A5, 0x00);
const UNIT_RECORD_WRITER_SITES: &[(u8, u16, u8)] = &[
    FIRST_ALLIED_ACTION_WRITER,
    MATCHED_ALLIED_ACTION_WRITER,
    FIRST_ACTION_WRITER,
    SECOND_ACTION_WRITER,
    SHIFTED_RECORD_FIELD_WRITER,
    SHIFTED_RECORD_IDENTITY_WRITER,
    DERIVED_MAP_X_WRITER,
    DERIVED_MAP_Y_WRITER,
    UNIT_RECORD_COPY_WRITER,
    COPIED_CLASS_WRITER,
    SELECTED_ALLIED_CLASS_WRITER,
    SELECTED_ALLIED_BYTE_ZERO_WRITER,
    MAP_OCCUPANCY_WRITER,
    MAP_OCCUPANCY_REFRESH_WRITER,
    ALLIED_TURN_COUNTER_WRITER,
    ALLIED_RECORD_REBUILD_WRITER,
    MAP_LAYER_CLEAR_WRITER,
    ALLIED_ACTION_CLEAR_WRITER,
    ALLIED_RECORD_BYTE_ZERO_REBUILD_WRITER,
    SELECTED_UNIT_MAP_CLASS_WRITER,
    SELECTED_UNIT_MAP_OCCUPANCY_WRITER,
];

pub(super) struct BoundUnitRecordAddressDomain {
    runtime_row_pointers: Vec<u16>,
    map_layer_row_pointers: Vec<u16>,
}

impl BoundUnitRecordAddressDomain {
    pub(super) fn allied_field_destination_ranges(
        &self,
        field_offset: u16,
    ) -> Result<Vec<RangeInclusive<u16>>> {
        record_field_destination_ranges(ALLIED_RECORD_BASE, RECORD_SCAN_CAPACITY, field_offset)
    }

    pub(super) fn enemy_field_destination_ranges(
        &self,
        field_offset: u16,
    ) -> Result<Vec<RangeInclusive<u16>>> {
        record_field_destination_ranges(ENEMY_RECORD_BASE, RECORD_SCAN_CAPACITY, field_offset)
    }

    pub(super) fn enemy_field_destination_ranges_within(
        &self,
        record_count: usize,
        field_offset: u16,
    ) -> Result<Vec<RangeInclusive<u16>>> {
        ensure!(
            record_count <= RECORD_SCAN_CAPACITY,
            "enemy unit-record subset exceeds the source-bound record capacity"
        );
        record_field_destination_ranges(ENEMY_RECORD_BASE, record_count, field_offset)
    }

    pub(super) fn enemy_record_copy_destination_range(
        &self,
        record_count: usize,
    ) -> Result<RangeInclusive<u16>> {
        ensure!(
            record_count <= RECORD_SCAN_CAPACITY,
            "enemy unit-record copy exceeds the source-bound record capacity"
        );
        record_copy_destination_range(ENEMY_RECORD_BASE, record_count)
    }

    pub(super) fn runtime_row_destination_ranges(
        &self,
        maximum_index: u8,
    ) -> Result<Vec<RangeInclusive<u16>>> {
        indexed_pointer_destination_ranges(&self.runtime_row_pointers, maximum_index)
    }

    pub(super) fn map_layer_row_destination_ranges(
        &self,
        row_count: usize,
        maximum_index: u8,
    ) -> Result<Vec<RangeInclusive<u16>>> {
        ensure!(
            row_count > 0 && row_count <= self.map_layer_row_pointers.len(),
            "map-layer row subset escapes the source-bound pointer table"
        );
        indexed_pointer_destination_ranges(&self.map_layer_row_pointers[..row_count], maximum_index)
    }

    pub(super) fn all_map_layer_destination_ranges(
        &self,
        maximum_index: u8,
    ) -> Result<Vec<RangeInclusive<u16>>> {
        indexed_pointer_destination_ranges(&self.map_layer_row_pointers, maximum_index)
    }
}

pub(super) struct UnitRecordWriteContract {
    destinations: BTreeMap<(u8, u16, u8), IndirectWriteDestinationBounds>,
    address_domain: BoundUnitRecordAddressDomain,
}

impl UnitRecordWriteContract {
    pub(super) fn destinations(&self) -> &BTreeMap<(u8, u16, u8), IndirectWriteDestinationBounds> {
        &self.destinations
    }

    pub(super) fn address_domain(&self) -> &BoundUnitRecordAddressDomain {
        &self.address_domain
    }
}

pub(super) fn bind_unit_record_write_destinations(source: &Rom) -> Result<UnitRecordWriteContract> {
    let source_binding = bind_unit_record_writer_source(source)?;
    let mut destinations = unit_record_write_destinations(
        &source_binding.runtime_row_pointers,
        &source_binding.map_layer_row_pointers,
    )?;
    for (site, destination) in bind_gameplay_path_destinations(
        source,
        &source_binding.runtime_row_pointers,
        &source_binding.map_layer_row_pointers,
    )? {
        ensure!(
            destinations.insert(site, destination).is_none(),
            "gameplay unit-record writer overlaps an existing destination owner at {:02X}:${:04X}",
            site.0,
            site.1,
        );
    }
    for (site, destination) in bind_chapter_save_path_destinations(source)? {
        ensure!(
            destinations.insert(site, destination).is_none(),
            "chapter-save unit-record writer overlaps an existing destination owner at {:02X}:${:04X}",
            site.0,
            site.1,
        );
    }
    Ok(UnitRecordWriteContract {
        destinations,
        address_domain: BoundUnitRecordAddressDomain {
            runtime_row_pointers: source_binding.runtime_row_pointers,
            map_layer_row_pointers: source_binding.map_layer_row_pointers,
        },
    })
}

fn unit_record_write_destinations(
    runtime_row_pointers: &[u16],
    map_layer_row_pointers: &[u16],
) -> Result<BTreeMap<(u8, u16, u8), IndirectWriteDestinationBounds>> {
    let action_targets = record_field_destination_ranges(
        ALLIED_RECORD_BASE,
        RECORD_SCAN_CAPACITY,
        ACTION_BYTE_OFFSET,
    )?;
    let shifted_field_targets = record_field_destination_ranges(
        ALLIED_RECORD_BASE,
        RECORD_SCAN_CAPACITY,
        SHIFTED_RECORD_FIELD_OFFSET,
    )?;
    let shifted_identity_targets = record_field_destination_ranges(
        ALLIED_RECORD_BASE,
        RECORD_SCAN_CAPACITY,
        SHIFTED_RECORD_IDENTITY_OFFSET,
    )?;
    let derived_map_x_targets = record_field_destination_ranges(
        ALLIED_RECORD_BASE,
        RECORD_SCAN_CAPACITY,
        DERIVED_MAP_X_OFFSET,
    )?;
    let derived_map_y_targets = record_field_destination_ranges(
        ALLIED_RECORD_BASE,
        RECORD_SCAN_CAPACITY,
        DERIVED_MAP_Y_OFFSET,
    )?;
    let copied_record_targets = vec![
        record_copy_destination_range(ALLIED_RECORD_BASE, RECORD_SCAN_CAPACITY)?,
        record_copy_destination_range(ENEMY_RECORD_BASE, RECORD_SCAN_CAPACITY)?,
    ];
    let copied_class_targets = allied_and_enemy_field_destinations(COPIED_CLASS_OFFSET)?;
    let selected_allied_class_targets = record_field_destination_ranges(
        ALLIED_RECORD_BASE,
        RECORD_SCAN_CAPACITY,
        COPIED_CLASS_OFFSET,
    )?;
    let selected_allied_byte_zero_targets = record_field_destination_ranges(
        ALLIED_RECORD_BASE,
        RECORD_SCAN_CAPACITY,
        RECORD_BYTE_ZERO_OFFSET,
    )?;
    let map_occupancy_targets = indexed_pointer_destination_ranges(runtime_row_pointers, 0xFF)?;
    let allied_turn_counter_targets = record_field_destination_ranges(
        ALLIED_RECORD_BASE,
        RECORD_SCAN_CAPACITY,
        TURN_COUNTER_OFFSET,
    )?;
    let allied_record_rebuild_targets = vec![record_copy_destination_range(
        ALLIED_RECORD_BASE,
        RECORD_SCAN_CAPACITY,
    )?];
    let map_layer_targets = indexed_pointer_destination_ranges(map_layer_row_pointers, 0x1F)?;

    let mut destinations = BTreeMap::new();
    insert_destination(
        &mut destinations,
        FIRST_ALLIED_ACTION_WRITER,
        "action byte of the first allied unit record",
        vec![ALLIED_RECORD_BASE + ACTION_BYTE_OFFSET..=ALLIED_RECORD_BASE + ACTION_BYTE_OFFSET],
    )?;
    insert_destination(
        &mut destinations,
        MATCHED_ALLIED_ACTION_WRITER,
        "action byte of the allied unit record matched by identity",
        action_targets.clone(),
    )?;
    for site in [FIRST_ACTION_WRITER, SECOND_ACTION_WRITER] {
        insert_destination(
            &mut destinations,
            site,
            "selected allied unit action byte",
            action_targets.clone(),
        )?;
    }
    insert_destination(
        &mut destinations,
        SHIFTED_RECORD_FIELD_WRITER,
        "selected allied unit shifted-record field",
        shifted_field_targets,
    )?;
    insert_destination(
        &mut destinations,
        SHIFTED_RECORD_IDENTITY_WRITER,
        "identity of the allied record after the shifted source",
        shifted_identity_targets,
    )?;
    insert_destination(
        &mut destinations,
        DERIVED_MAP_X_WRITER,
        "selected allied unit derived map-x byte",
        derived_map_x_targets,
    )?;
    insert_destination(
        &mut destinations,
        DERIVED_MAP_Y_WRITER,
        "selected allied unit derived map-y byte",
        derived_map_y_targets,
    )?;
    insert_destination(
        &mut destinations,
        UNIT_RECORD_COPY_WRITER,
        "first available allied or enemy unit record",
        copied_record_targets,
    )?;
    insert_destination(
        &mut destinations,
        COPIED_CLASS_WRITER,
        "class byte of the copied allied or enemy unit record",
        copied_class_targets,
    )?;
    insert_destination(
        &mut destinations,
        SELECTED_ALLIED_CLASS_WRITER,
        "class byte of the selected allied unit record",
        selected_allied_class_targets,
    )?;
    insert_destination(
        &mut destinations,
        SELECTED_ALLIED_BYTE_ZERO_WRITER,
        "byte zero of the selected allied unit record derived from byte one",
        selected_allied_byte_zero_targets.clone(),
    )?;
    insert_destination(
        &mut destinations,
        MAP_OCCUPANCY_WRITER,
        "runtime map occupancy cell selected from unit coordinates",
        map_occupancy_targets,
    )?;
    insert_destination(
        &mut destinations,
        MAP_OCCUPANCY_REFRESH_WRITER,
        "runtime map occupancy cell selected from allied unit coordinates",
        indexed_pointer_destination_ranges(runtime_row_pointers, 0xFF)?,
    )?;
    insert_destination(
        &mut destinations,
        ALLIED_TURN_COUNTER_WRITER,
        "turn counter field of one allied unit record",
        allied_turn_counter_targets,
    )?;
    insert_destination(
        &mut destinations,
        ALLIED_RECORD_REBUILD_WRITER,
        "inactive allied unit record selected by identity",
        allied_record_rebuild_targets,
    )?;
    insert_destination(
        &mut destinations,
        MAP_LAYER_CLEAR_WRITER,
        "one source-bound 32-byte map-layer row",
        map_layer_targets,
    )?;
    insert_destination(
        &mut destinations,
        ALLIED_ACTION_CLEAR_WRITER,
        "action byte of one allied unit record removed from map occupancy",
        action_targets,
    )?;
    insert_destination(
        &mut destinations,
        ALLIED_RECORD_BYTE_ZERO_REBUILD_WRITER,
        "byte zero of an inactive allied unit record selected by identity",
        selected_allied_byte_zero_targets,
    )?;
    for site in [
        SELECTED_UNIT_MAP_CLASS_WRITER,
        SELECTED_UNIT_MAP_OCCUPANCY_WRITER,
    ] {
        insert_destination(
            &mut destinations,
            site,
            "runtime map occupancy cell selected from unit coordinates",
            indexed_pointer_destination_ranges(runtime_row_pointers, 0xFF)?,
        )?;
    }
    ensure!(
        destinations
            .keys()
            .copied()
            .collect::<std::collections::BTreeSet<_>>()
            == UNIT_RECORD_WRITER_SITES.iter().copied().collect(),
        "unit-record destination owner omitted or invented an indirect writer site"
    );
    Ok(destinations)
}

fn indexed_pointer_destination_ranges(
    pointers: &[u16],
    maximum_index: u8,
) -> Result<Vec<RangeInclusive<u16>>> {
    ensure!(!pointers.is_empty(), "indexed pointer table is empty");
    ensure!(
        pointers.windows(2).all(|pair| pair[0] < pair[1]),
        "indexed pointer table is not strictly ordered"
    );
    let mut merged = Vec::<RangeInclusive<u16>>::new();
    for &pointer in pointers {
        let end = pointer
            .checked_add(u16::from(maximum_index))
            .context("indexed pointer destination overflow")?;
        ensure!(
            end < 0x8000,
            "indexed pointer destination reaches mapper space at ${end:04X}"
        );
        if let Some(previous) = merged.last_mut()
            && pointer <= previous.end().saturating_add(1)
        {
            let start = *previous.start();
            let joined_end = end.max(*previous.end());
            *previous = start..=joined_end;
        } else {
            merged.push(pointer..=end);
        }
    }
    Ok(merged)
}

fn allied_and_enemy_field_destinations(field_offset: u16) -> Result<Vec<RangeInclusive<u16>>> {
    let mut ranges =
        record_field_destination_ranges(ALLIED_RECORD_BASE, RECORD_SCAN_CAPACITY, field_offset)?;
    ranges.extend(record_field_destination_ranges(
        ENEMY_RECORD_BASE,
        RECORD_SCAN_CAPACITY,
        field_offset,
    )?);
    ensure!(
        ranges
            .windows(2)
            .all(|pair| pair[0].end() < pair[1].start()),
        "allied and enemy unit-record field destinations overlap or are unordered"
    );
    Ok(ranges)
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
        "unit-record indirect writer is duplicated at {:02X}:${:04X}",
        site.0,
        site.1,
    );
    Ok(())
}

fn record_field_destination_ranges(
    base: u16,
    record_count: usize,
    field_offset: u16,
) -> Result<Vec<RangeInclusive<u16>>> {
    ensure!(record_count > 0, "unit-record field has no records");
    (0..record_count)
        .map(|index| {
            let address = record_address(base, index)?
                .checked_add(field_offset)
                .context("unit-record field destination overflow")?;
            ensure!(
                address < 0x8000,
                "unit-record field destination reaches mapper space at ${address:04X}"
            );
            Ok(address..=address)
        })
        .collect()
}

fn record_copy_destination_range(base: u16, record_count: usize) -> Result<RangeInclusive<u16>> {
    ensure!(record_count > 0, "unit-record copy has no records");
    let end = record_address(base, record_count)?
        .checked_sub(1)
        .context("unit-record copy destination underflow")?;
    ensure!(
        end < 0x8000,
        "unit-record copy destination reaches mapper space at ${end:04X}"
    );
    Ok(base..=end)
}

fn record_address(base: u16, record_index: usize) -> Result<u16> {
    let byte_offset = record_index
        .checked_mul(usize::from(UNIT_RECORD_STRIDE))
        .context("unit-record destination offset overflow")?;
    base.checked_add(u16::try_from(byte_offset)?)
        .context("unit-record destination address overflow")
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::BTreeSet;

    #[test]
    fn unit_record_destination_owner_covers_every_bound_writer() {
        let destinations = unit_record_write_destinations(
            &runtime_row_pointers(),
            &(0..30).map(|row| 0x7730 + row * 0x20).collect::<Vec<_>>(),
        )
        .unwrap();
        assert_eq!(
            destinations.keys().copied().collect::<BTreeSet<_>>(),
            UNIT_RECORD_WRITER_SITES.iter().copied().collect()
        );
        assert_eq!(
            destinations[&FIRST_ALLIED_ACTION_WRITER].destination_ranges(),
            &[0x6AA2..=0x6AA2]
        );
        assert_eq!(
            destinations[&MATCHED_ALLIED_ACTION_WRITER]
                .destination_ranges()
                .len(),
            RECORD_SCAN_CAPACITY
        );
        assert!(destinations.values().all(|destination| {
            destination
                .destination_ranges()
                .iter()
                .all(|range| *range.end() < 0x8000)
        }));
    }

    #[test]
    fn allied_action_targets_follow_every_record_at_stride_1b() {
        let ranges = record_field_destination_ranges(
            ALLIED_RECORD_BASE,
            RECORD_SCAN_CAPACITY,
            ACTION_BYTE_OFFSET,
        )
        .unwrap();
        assert_eq!(ranges.len(), 54);
        assert_eq!(ranges.first(), Some(&(0x6AA2..=0x6AA2)));
        assert_eq!(ranges.last(), Some(&(0x7039..=0x7039)));
        assert!(ranges.windows(2).all(|pair| {
            pair[1].start().checked_sub(*pair[0].start()) == Some(UNIT_RECORD_STRIDE)
        }));
    }

    #[test]
    fn shifted_field_and_record_copy_ranges_match_the_source_loops() {
        let shifted = record_field_destination_ranges(
            ALLIED_RECORD_BASE,
            RECORD_SCAN_CAPACITY,
            SHIFTED_RECORD_FIELD_OFFSET,
        )
        .unwrap();
        assert_eq!(shifted.first(), Some(&(0x6AD7..=0x6AD7)));
        assert_eq!(shifted.last(), Some(&(0x706E..=0x706E)));
        let shifted_identity = record_field_destination_ranges(
            ALLIED_RECORD_BASE,
            RECORD_SCAN_CAPACITY,
            SHIFTED_RECORD_IDENTITY_OFFSET,
        )
        .unwrap();
        assert_eq!(shifted_identity.first(), Some(&(0x6AC6..=0x6AC6)));
        assert_eq!(shifted_identity.last(), Some(&(0x705D..=0x705D)));
        assert_eq!(
            record_copy_destination_range(ALLIED_RECORD_BASE, RECORD_SCAN_CAPACITY).unwrap(),
            0x6A90..=0x7041
        );
        assert_eq!(
            record_copy_destination_range(ENEMY_RECORD_BASE, RECORD_SCAN_CAPACITY).unwrap(),
            0x7078..=0x7629
        );
        let copied_class = allied_and_enemy_field_destinations(COPIED_CLASS_OFFSET).unwrap();
        assert_eq!(copied_class.len(), 108);
        assert_eq!(copied_class.first(), Some(&(0x6A96..=0x6A96)));
        assert_eq!(copied_class.last(), Some(&(0x7615..=0x7615)));
    }

    #[test]
    fn mutated_record_domains_cannot_cross_into_mapper_space() {
        assert!(record_copy_destination_range(0x7FF0, 1).is_err());
        assert!(record_field_destination_ranges(0x7FF0, 2, 0x20).is_err());
        assert!(indexed_pointer_destination_ranges(&[0x7F80], 0xFF).is_err());
    }

    #[test]
    fn runtime_row_pointers_keep_every_possible_column_below_mapper_space() {
        assert_eq!(
            indexed_pointer_destination_ranges(&runtime_row_pointers(), 0xFF).unwrap(),
            vec![0x72AF..=0x774E, 0x7AF0..=0x7C0F]
        );
    }

    #[test]
    fn map_layer_rows_cover_exactly_thirty_contiguous_32_byte_rows() {
        let pointers = (0..30).map(|row| 0x7730 + row * 0x20).collect::<Vec<_>>();
        assert_eq!(
            indexed_pointer_destination_ranges(&pointers, 0x1F).unwrap(),
            vec![0x7730..=0x7AEF]
        );
    }

    fn runtime_row_pointers() -> Vec<u16> {
        (0..30)
            .map(|row| 0x72AF + row * 0x20)
            .chain([0x7AF0, 0x7B10])
            .collect()
    }
}
