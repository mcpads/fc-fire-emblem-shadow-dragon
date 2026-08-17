use anyhow::{Context, Result, ensure};

use super::*;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct MainDialogueMenuLayoutBounds {
    maximum_width: u8,
    maximum_row_count: u8,
}

impl MainDialogueMenuLayoutBounds {
    pub(crate) const fn maximum_width(self) -> u8 {
        self.maximum_width
    }

    pub(crate) const fn maximum_row_count(self) -> u8 {
        self.maximum_row_count
    }
}

pub(crate) fn inspect_main_dialogue_menu_layout_bounds(
    source: &[u8],
) -> Result<MainDialogueMenuLayoutBounds> {
    let report = build_report(source)?;
    let mut dimensions = Vec::new();
    let mut e5_layout_count = 0_usize;
    let mut fixed_header_layout_count = 0_usize;
    let mut e8_layout_count = 0_usize;

    for table in report
        .tables
        .iter()
        .filter(|table| table.directory_binding.is_some())
    {
        for entry in table.entries.iter().filter(|entry| {
            entry.target_kind == "script_entry_start" && is_canonical_dialogue_entry(entry)
        }) {
            let prefix = entry
                .main_record_prefix
                .as_ref()
                .context("canonical main-dialogue entry has no prefix report")?;
            if prefix.e5_prefix_present {
                let end = entry
                    .file_offset
                    .checked_add(OPTIONAL_PREFIX_BYTE_COUNT)
                    .context("main-dialogue E5 layout prefix range overflow")?;
                let bytes = source
                    .get(entry.file_offset..end)
                    .context("main-dialogue E5 layout prefix is outside the source")?;
                dimensions.push(e5_menu_dimensions(bytes)?);
                e5_layout_count += 1;
            }
            if prefix.fixed_record_header_byte_count == FIXED_RECORD_HEADER_BYTE_COUNT {
                let start = prefix.fixed_record_header_file_offset;
                let end = start
                    .checked_add(FIXED_RECORD_HEADER_BYTE_COUNT)
                    .context("main-dialogue fixed layout header range overflow")?;
                let bytes = source
                    .get(start..end)
                    .context("main-dialogue fixed layout header is outside the source")?;
                dimensions.push(fixed_header_menu_dimensions(bytes)?);
                fixed_header_layout_count += 1;
            }
            if prefix.e8_prefix_present {
                let start = prefix
                    .first_line_file_offset
                    .checked_sub(OPTIONAL_PREFIX_BYTE_COUNT)
                    .context("main-dialogue E8 layout prefix start underflow")?;
                let bytes = source
                    .get(start..prefix.first_line_file_offset)
                    .context("main-dialogue E8 layout prefix is outside the source")?;
                dimensions.push(e8_menu_dimensions(bytes)?);
                e8_layout_count += 1;
            }
        }
    }

    ensure!(
        e5_layout_count != 0 && fixed_header_layout_count != 0 && e8_layout_count != 0,
        "main-dialogue menu layout source lost an E5, fixed-header, or E8 producer"
    );
    let maximum_width = dimensions
        .iter()
        .map(|(width, _)| *width)
        .max()
        .context("main-dialogue menu layout source is empty")?;
    let maximum_row_count = dimensions
        .iter()
        .map(|(_, row_count)| *row_count)
        .max()
        .context("main-dialogue menu layout source is empty")?;
    Ok(MainDialogueMenuLayoutBounds {
        maximum_width,
        maximum_row_count,
    })
}

fn e5_menu_dimensions(prefix: &[u8]) -> Result<(u8, u8)> {
    ensure!(
        prefix.len() == OPTIONAL_PREFIX_BYTE_COUNT && prefix[0] == OPTIONAL_E5_PREFIX_CODE,
        "main-dialogue E5 menu prefix changed"
    );
    nonzero_menu_dimensions(prefix[3], prefix[4], "main-dialogue E5 menu prefix")
}

fn fixed_header_menu_dimensions(header: &[u8]) -> Result<(u8, u8)> {
    ensure!(
        header.len() == FIXED_RECORD_HEADER_BYTE_COUNT,
        "main-dialogue fixed menu header length changed"
    );
    let width = header[2]
        .checked_add(2)
        .context("main-dialogue fixed menu width overflow")?;
    let row_count = header[3]
        .checked_mul(2)
        .and_then(|value| value.checked_add(2))
        .context("main-dialogue fixed menu row count overflow")?;
    nonzero_menu_dimensions(width, row_count, "main-dialogue fixed menu header")
}

fn e8_menu_dimensions(prefix: &[u8]) -> Result<(u8, u8)> {
    ensure!(
        prefix.len() == OPTIONAL_PREFIX_BYTE_COUNT && prefix[0] == OPTIONAL_E8_PREFIX_CODE,
        "main-dialogue E8 menu prefix changed"
    );
    let width = prefix[3]
        .checked_add(2)
        .context("main-dialogue E8 menu width overflow")?;
    let row_count = prefix[4]
        .checked_add(2)
        .context("main-dialogue E8 menu row count overflow")?;
    nonzero_menu_dimensions(width, row_count, "main-dialogue E8 menu prefix")
}

fn nonzero_menu_dimensions(width: u8, row_count: u8, role: &str) -> Result<(u8, u8)> {
    ensure!(width != 0, "{role} has zero width");
    ensure!(row_count != 0, "{role} has zero rows");
    Ok((width, row_count))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn derives_shared_menu_dimensions_from_dialogue_prefix_semantics() {
        assert_eq!(
            e5_menu_dimensions(&[0xE5, 0x00, 0x00, 20, 4, 0x00]).unwrap(),
            (20, 4)
        );
        assert_eq!(
            fixed_header_menu_dimensions(&[0x40, 0xA0, 20, 15]).unwrap(),
            (22, 32)
        );
        assert_eq!(
            e8_menu_dimensions(&[0xE8, 0x00, 0x00, 6, 8, 0x00]).unwrap(),
            (8, 10)
        );
    }

    #[test]
    fn rejects_overflowing_or_empty_dialogue_menu_dimensions() {
        assert!(fixed_header_menu_dimensions(&[0x40, 0xA0, 0xFE, 15]).is_err());
        assert!(fixed_header_menu_dimensions(&[0x40, 0xA0, 20, 0x80]).is_err());
        assert!(e5_menu_dimensions(&[0xE5, 0x00, 0x00, 0, 4, 0x00]).is_err());
        assert_eq!(
            e8_menu_dimensions(&[0xE8, 0x00, 0x00, 6, 0, 0x00]).unwrap(),
            (8, 2)
        );
    }
}
