use std::collections::BTreeMap;

use anyhow::{Context, Result, ensure};

use crate::text_inventory::FixedTextLogicalByte;

use super::entries::MaterialEntry;

const SECTION_MAGIC: &[u8; 4] = b"KTX1";
const SECTION_HEADER_BYTE_COUNT: usize = 8;
pub(super) const GLYPH_CELL_FLAG: u16 = 0x8000;

pub(super) fn encode_section(
    entries: &[MaterialEntry],
    atlas_indices: &BTreeMap<char, u16>,
) -> Result<Vec<u8>> {
    ensure!(
        !entries.is_empty(),
        "cross-domain material section is empty"
    );
    let entry_count = u16::try_from(entries.len()).context("material entry count exceeds u16")?;
    let directory_byte_count = entries
        .len()
        .checked_mul(2)
        .context("material directory size overflow")?;
    let payload_start = SECTION_HEADER_BYTE_COUNT
        .checked_add(directory_byte_count)
        .context("material payload start overflow")?;
    let mut payload = Vec::new();
    let mut offsets = Vec::with_capacity(entries.len());
    for entry in entries {
        let offset = payload_start
            .checked_add(payload.len())
            .context("material entry offset overflow")?;
        offsets.push(u16::try_from(offset).context("material entry offset exceeds u16")?);
        let cell_count =
            u16::try_from(entry.logical_bytes.len()).context("material cell count exceeds u16")?;
        payload.extend_from_slice(&cell_count.to_le_bytes());
        for logical in &entry.logical_bytes {
            let cell = match logical {
                FixedTextLogicalByte::Encoded(value) => u16::from(*value),
                FixedTextLogicalByte::TargetGlyph(glyph) => {
                    GLYPH_CELL_FLAG
                        | atlas_indices.get(glyph).copied().with_context(|| {
                            format!("shared glyph atlas lost material glyph {glyph:?}")
                        })?
                }
            };
            payload.extend_from_slice(&cell.to_le_bytes());
        }
    }
    let mut bytes = Vec::with_capacity(payload_start + payload.len());
    bytes.extend_from_slice(SECTION_MAGIC);
    bytes.extend_from_slice(&entry_count.to_le_bytes());
    bytes.extend_from_slice(&(SECTION_HEADER_BYTE_COUNT as u16).to_le_bytes());
    for offset in offsets {
        bytes.extend_from_slice(&offset.to_le_bytes());
    }
    bytes.extend_from_slice(&payload);
    ensure!(
        bytes.len() <= usize::from(u16::MAX),
        "cross-domain material section exceeds its u16 directory"
    );
    Ok(bytes)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn section_cells_distinguish_original_bytes_from_shared_atlas_indices() {
        let entries = vec![MaterialEntry {
            id: "entry".to_owned(),
            source_binding: "source".to_owned(),
            logical_bytes: vec![
                FixedTextLogicalByte::Encoded(0x8D),
                FixedTextLogicalByte::TargetGlyph('가'),
            ],
        }];
        let bytes = encode_section(&entries, &BTreeMap::from([('가', 0x0123)])).unwrap();

        assert_eq!(&bytes[..4], SECTION_MAGIC);
        assert_eq!(u16::from_le_bytes([bytes[4], bytes[5]]), 1);
        let entry_offset = usize::from(u16::from_le_bytes([bytes[8], bytes[9]]));
        assert_eq!(
            u16::from_le_bytes([bytes[entry_offset], bytes[entry_offset + 1]]),
            2
        );
        assert_eq!(
            &bytes[entry_offset + 2..entry_offset + 6],
            [0x8D, 0x00, 0x23, 0x81]
        );
    }
}
