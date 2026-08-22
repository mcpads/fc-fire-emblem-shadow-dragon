use std::collections::BTreeSet;

use anyhow::{Context, Result, ensure};

use super::{HEADER_SIZE, MAX_ENTRY_BYTES, PRG_BANK_SIZE, PRG_FILE_END};

const PRG_BANK: usize = 0x07;
const CPU_BASE: u16 = 0x8000;
const BANK_FILE_OFFSET: usize = HEADER_SIZE + PRG_BANK * PRG_BANK_SIZE;
const LOADER_FILE_OFFSET: usize = 0x1C2EC;
const LOADER_BYTES: [u8; 13] = [
    0x0A, 0xA8, 0xB9, 0xED, 0x82, 0x85, 0x08, 0xB9, 0xEE, 0x82, 0x85, 0x09, 0x60,
];
const POINTER_TABLE_FILE_OFFSET: usize = 0x1C2FD;
const DATA_FILE_OFFSET: usize = 0x1C329;
const DATA_END_FILE_OFFSET: usize = 0x1C434;
const TERMINATOR: u8 = 0xEF;
const EXPECTED_POINTERS: [u16; 22] = [
    0x8319, 0x8321, 0x8329, 0x8333, 0x8339, 0x8344, 0x8351, 0x8360, 0x836E, 0x8374, 0x837B, 0x8383,
    0x8393, 0x839F, 0x83A8, 0x83B8, 0x83C5, 0x83D4, 0x83E6, 0x83FC, 0x840C, 0x8419,
];

#[derive(Clone, Debug)]
pub(super) struct BattleMessageTemplate {
    pub(super) index: usize,
    pub(super) pointer_cpu_address: u16,
    pub(super) file_offset: usize,
    pub(super) raw_bytes: Vec<u8>,
}

pub(super) fn extract_battle_message_templates(
    source: &[u8],
) -> Result<Vec<BattleMessageTemplate>> {
    ensure!(
        source.len() >= PRG_FILE_END,
        "source ROM is shorter than declared PRG"
    );
    let loader_end = LOADER_FILE_OFFSET + LOADER_BYTES.len();
    ensure!(
        source[LOADER_FILE_OFFSET..loader_end] == LOADER_BYTES,
        "battle message pointer loader changed"
    );
    ensure!(
        POINTER_TABLE_FILE_OFFSET + EXPECTED_POINTERS.len() * 2 == DATA_FILE_OFFSET,
        "battle message pointer table no longer ends at its first string"
    );

    let mut templates = Vec::with_capacity(EXPECTED_POINTERS.len());
    let mut seen = BTreeSet::new();
    for (index, expected_pointer) in EXPECTED_POINTERS.iter().copied().enumerate() {
        let pointer_offset = POINTER_TABLE_FILE_OFFSET + index * 2;
        let pointer = u16::from_le_bytes([source[pointer_offset], source[pointer_offset + 1]]);
        ensure!(
            pointer == expected_pointer,
            "battle message pointer {index} changed: expected {expected_pointer:#06X}, got {pointer:#06X}"
        );
        ensure!(
            seen.insert(pointer),
            "duplicate battle message pointer {pointer:#06X}"
        );
        let file_offset = BANK_FILE_OFFSET
            .checked_add(usize::from(pointer.checked_sub(CPU_BASE).context(
                "battle message pointer is below the switchable-bank CPU window",
            )?))
            .context("battle message file offset overflow")?;
        ensure!(
            (DATA_FILE_OFFSET..DATA_END_FILE_OFFSET).contains(&file_offset),
            "battle message pointer {pointer:#06X} is outside the proven data interval"
        );
        let search_end = file_offset
            .saturating_add(MAX_ENTRY_BYTES)
            .min(DATA_END_FILE_OFFSET);
        let terminator_offset = source[file_offset..search_end]
            .iter()
            .position(|byte| *byte == TERMINATOR)
            .map(|relative| file_offset + relative)
            .with_context(|| format!("battle message {index} has no EF terminator"))?;
        let expected_next = EXPECTED_POINTERS
            .get(index + 1)
            .map(|pointer| BANK_FILE_OFFSET + usize::from(*pointer - CPU_BASE))
            .unwrap_or(DATA_END_FILE_OFFSET);
        ensure!(
            terminator_offset + 1 == expected_next,
            "battle message {index} does not end at the next proven boundary"
        );
        templates.push(BattleMessageTemplate {
            index,
            pointer_cpu_address: pointer,
            file_offset,
            raw_bytes: source[file_offset..terminator_offset].to_vec(),
        });
    }
    Ok(templates)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn declared_table_is_contiguous() {
        assert_eq!(
            POINTER_TABLE_FILE_OFFSET + EXPECTED_POINTERS.len() * 2,
            DATA_FILE_OFFSET
        );
        assert_eq!(EXPECTED_POINTERS[0], 0x8319);
        assert_eq!(EXPECTED_POINTERS.last(), Some(&0x8419));
    }
}
