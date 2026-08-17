use std::ops::RangeInclusive;

use anyhow::{Context, Result, ensure};

use crate::{
    dialogue_inventory::{MainDialogueMenuLayoutBounds, inspect_main_dialogue_menu_layout_bounds},
    rom::Rom,
    sha1_hex,
    typed_source::decode_rp2a03_sequence,
};

use super::source_regions::source_bytes;

const MENU_ROW_MARKER_BASE: u16 = 0x0310;
pub(super) const MENU_CACHE_BASES: [u16; 5] = [0x7F4E, 0x7F6E, 0x7F8E, 0x7FAE, 0x7FCE];
const MAXIMUM_SAFE_MENU_WIDTH: u8 = 31;
const MAXIMUM_SAFE_MENU_ROW_COUNT: u8 = 32;
const MENU_DIMENSION_PRODUCER_REGION_START: u16 = 0x8000;
const MENU_DIMENSION_PRODUCER_REGION_END: u16 = 0x8E3C;
const MENU_DIMENSION_PRODUCER_REGION_SHA1: &str = "ab3661956a0c8473837755fe13625b7c8e25730f";

#[derive(Clone, Copy)]
pub(super) struct SharedMenuDimensionBounds {
    pub(super) maximum_width: u8,
    pub(super) maximum_row_count: u8,
}

pub(super) fn bind_menu_dimension_producers(source: &Rom) -> Result<SharedMenuDimensionBounds> {
    let dialogue_layout = inspect_main_dialogue_menu_layout_bounds(source.data())?;
    let bytes = source_bytes(
        source,
        0x0B,
        MENU_DIMENSION_PRODUCER_REGION_START,
        usize::from(MENU_DIMENSION_PRODUCER_REGION_END - MENU_DIMENSION_PRODUCER_REGION_START),
    )?;
    ensure!(
        sha1_hex(bytes) == MENU_DIMENSION_PRODUCER_REGION_SHA1,
        "shared-menu dimension producer region changed"
    );

    let mut maximum_width = 0_u8;
    let mut maximum_row_count = 0_u8;
    let mut writer_count = 0_usize;
    for (offset, window) in bytes.windows(3).enumerate() {
        let dimension = match window {
            [0x8D, 0xCF, 0x05] => MenuDimension::Width,
            [0x8D, 0xD0, 0x05] => MenuDimension::RowCount,
            _ => continue,
        };
        let address = MENU_DIMENSION_PRODUCER_REGION_START
            .checked_add(u16::try_from(offset).context("menu dimension writer offset overflow")?)
            .context("menu dimension writer address overflow")?;
        let maximum =
            bind_menu_dimension_writer(bytes, offset, address, dimension, dialogue_layout)?;
        match dimension {
            MenuDimension::Width => maximum_width = maximum_width.max(maximum),
            MenuDimension::RowCount => maximum_row_count = maximum_row_count.max(maximum),
        }
        writer_count += 1;
    }
    ensure!(
        writer_count != 0,
        "shared-menu dimension writer census is empty"
    );
    ensure!(
        maximum_width == dialogue_layout.maximum_width()
            && maximum_row_count == dialogue_layout.maximum_row_count(),
        "shared-menu direct producer bounds no longer cover the dialogue layout maximum"
    );
    ensure!(
        maximum_width <= MAXIMUM_SAFE_MENU_WIDTH,
        "shared-menu width {maximum_width} exceeds the five-bit source serializer"
    );
    ensure!(
        maximum_row_count <= MAXIMUM_SAFE_MENU_ROW_COUNT,
        "shared-menu row count {maximum_row_count} exceeds the source cache projection"
    );
    Ok(SharedMenuDimensionBounds {
        maximum_width,
        maximum_row_count,
    })
}

#[derive(Clone, Copy)]
enum MenuDimension {
    Width,
    RowCount,
}

fn bind_menu_dimension_writer(
    region: &[u8],
    store_offset: usize,
    store_address: u16,
    dimension: MenuDimension,
    dialogue_layout: MainDialogueMenuLayoutBounds,
) -> Result<u8> {
    if let Some(value) = preceding_immediate(region, store_offset) {
        bind_writer_sequence(
            region,
            store_offset - 2,
            store_offset + 3,
            store_address - 2,
        )?;
        return validate_dimension_bound(value, dimension, store_address);
    }

    let patterns: &[(&[u8], u8)] = match dimension {
        MenuDimension::Width => &[
            (
                &[0xAD, 0x1A, 0x78, 0x18, 0x69, 0x02],
                dialogue_layout.maximum_width(),
            ),
            (
                &[0xBD, 0x23, 0x78, 0x18, 0x69, 0x02],
                dialogue_layout.maximum_width(),
            ),
        ],
        MenuDimension::RowCount => &[
            (&[0x20, 0x40, 0x98, 0x0A, 0x18, 0x69, 0x02], 18),
            (&[0x20, 0xD0, 0x86, 0xAD, 0xD0, 0x05, 0x18, 0x69, 0x02], 20),
            (
                &[0xAD, 0x1B, 0x78, 0x0A, 0x18, 0x69, 0x04],
                dialogue_layout.maximum_row_count(),
            ),
            (
                &[0xBD, 0x25, 0x78, 0x18, 0x69, 0x02],
                dialogue_layout.maximum_row_count(),
            ),
        ],
    };
    for (pattern, maximum) in patterns {
        if store_offset >= pattern.len()
            && &region[store_offset - pattern.len()..store_offset] == *pattern
        {
            bind_writer_sequence(
                region,
                store_offset - pattern.len(),
                store_offset + 3,
                store_address - u16::try_from(pattern.len())?,
            )?;
            return validate_dimension_bound(*maximum, dimension, store_address);
        }
    }
    anyhow::bail!("shared-menu dimension writer at 0B:${store_address:04X} has an unowned producer")
}

fn preceding_immediate(region: &[u8], store_offset: usize) -> Option<u8> {
    (store_offset >= 2 && region[store_offset - 2] == 0xA9).then_some(region[store_offset - 1])
}

fn bind_writer_sequence(region: &[u8], start: usize, end: usize, cpu_start: u16) -> Result<()> {
    let sequence = region
        .get(start..end)
        .context("shared-menu dimension writer sequence is outside its source region")?;
    decode_rp2a03_sequence(sequence, cpu_start, "shared-menu dimension producer")?;
    Ok(())
}

fn validate_dimension_bound(value: u8, dimension: MenuDimension, address: u16) -> Result<u8> {
    let (name, maximum) = match dimension {
        MenuDimension::Width => ("width", MAXIMUM_SAFE_MENU_WIDTH),
        MenuDimension::RowCount => ("row count", MAXIMUM_SAFE_MENU_ROW_COUNT),
    };
    ensure!(
        value != 0,
        "shared-menu {name} producer at 0B:${address:04X} can produce zero"
    );
    ensure!(
        value <= maximum,
        "shared-menu {name} producer at 0B:${address:04X} can produce {value}, above {maximum}"
    );
    Ok(value)
}

pub(super) fn row_marker_destination_range(maximum_width: u8) -> Result<RangeInclusive<u16>> {
    ensure!(
        maximum_width != 0 && maximum_width <= MAXIMUM_SAFE_MENU_WIDTH,
        "shared-menu row-marker width is outside the source serializer range"
    );
    let maximum_offset = u16::from(maximum_width)
        .checked_mul(u16::from(u8::MAX))
        .context("shared-menu row-marker destination offset overflow")?;
    let end = MENU_ROW_MARKER_BASE
        .checked_add(maximum_offset)
        .context("shared-menu row-marker destination overflow")?;
    ensure!(
        end < 0x8000,
        "shared-menu row-marker destination reaches mapper space"
    );
    Ok(MENU_ROW_MARKER_BASE..=end)
}

pub(super) fn cache_destination_ranges(
    maximum_width: u8,
    maximum_row_count: u8,
) -> Result<Vec<RangeInclusive<u16>>> {
    ensure!(
        maximum_width != 0 && maximum_width <= MAXIMUM_SAFE_MENU_WIDTH,
        "shared-menu cache width is outside the source serializer range"
    );
    ensure!(
        maximum_row_count != 0 && maximum_row_count <= MAXIMUM_SAFE_MENU_ROW_COUNT,
        "shared-menu cache row count is outside the source projection range"
    );
    let horizontal_spans = maximum_span_count(maximum_width, 32)?;
    let vertical_spans = maximum_span_count(maximum_row_count, 4)?;
    let byte_count = u16::from(horizontal_spans)
        .checked_mul(u16::from(vertical_spans))
        .context("shared-menu cache byte count overflow")?;
    ensure!(byte_count != 0, "shared-menu cache byte count is zero");
    MENU_CACHE_BASES
        .iter()
        .map(|base| {
            let end = base
                .checked_add(byte_count - 1)
                .context("shared-menu cache destination overflow")?;
            ensure!(
                end < 0x8000,
                "shared-menu cache destination reaches mapper space"
            );
            Ok(*base..=end)
        })
        .collect()
}

fn maximum_span_count(item_count: u8, items_per_span: u8) -> Result<u8> {
    ensure!(item_count != 0, "shared-menu span item count is zero");
    ensure!(items_per_span != 0, "shared-menu span width is zero");
    let additional = item_count
        .checked_sub(1)
        .expect("nonzero item count")
        .div_ceil(items_per_span);
    1_u8.checked_add(additional)
        .context("shared-menu span count overflow")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn row_marker_range_stays_below_mapper_space_at_serializer_limit() {
        let range = row_marker_destination_range(MAXIMUM_SAFE_MENU_WIDTH).unwrap();

        assert_eq!(*range.start(), MENU_ROW_MARKER_BASE);
        assert_eq!(
            *range.end(),
            MENU_ROW_MARKER_BASE + u16::from(MAXIMUM_SAFE_MENU_WIDTH) * u16::from(u8::MAX)
        );
        assert!(*range.end() < 0x8000);
    }

    #[test]
    fn cache_ranges_follow_menu_span_semantics() {
        let ranges = cache_destination_ranges(22, 32).unwrap();
        let expected_byte_count = u16::from(maximum_span_count(22, 32).unwrap())
            * u16::from(maximum_span_count(32, 4).unwrap());

        assert_eq!(ranges.len(), MENU_CACHE_BASES.len());
        for (range, base) in ranges.iter().zip(MENU_CACHE_BASES) {
            assert_eq!(*range.start(), base);
            assert_eq!(*range.end() - *range.start() + 1, expected_byte_count);
            assert!(*range.end() < 0x8000);
        }
    }

    #[test]
    fn destination_bounds_reject_empty_or_unrepresentable_dimensions() {
        assert!(row_marker_destination_range(0).is_err());
        assert!(row_marker_destination_range(MAXIMUM_SAFE_MENU_WIDTH + 1).is_err());
        assert!(cache_destination_ranges(0, 1).is_err());
        assert!(cache_destination_ranges(1, 0).is_err());
        assert!(
            cache_destination_ranges(MAXIMUM_SAFE_MENU_WIDTH + 1, MAXIMUM_SAFE_MENU_ROW_COUNT,)
                .is_err()
        );
        assert!(
            cache_destination_ranges(MAXIMUM_SAFE_MENU_WIDTH, MAXIMUM_SAFE_MENU_ROW_COUNT + 1,)
                .is_err()
        );
    }
}
