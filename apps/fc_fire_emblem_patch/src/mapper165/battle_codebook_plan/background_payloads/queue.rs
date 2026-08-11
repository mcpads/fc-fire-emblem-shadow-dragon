use std::collections::BTreeSet;

use anyhow::{Context, Result, ensure};

use crate::japanese_encoding::is_japanese_text_code;

#[derive(Default)]
pub(super) struct QueueCodeOwnership {
    pub(super) preserved_active: BTreeSet<u8>,
    pub(super) japanese_active: BTreeSet<u8>,
}

pub(super) fn add_queue(
    ownership: &mut QueueCodeOwnership,
    queue: &[u8],
    active_codes: &BTreeSet<u8>,
) -> Result<()> {
    let mut cursor = 0;
    loop {
        let address_high = *queue
            .get(cursor)
            .context("battle source PPU queue has no zero address-high terminator")?;
        if address_high == 0 {
            return Ok(());
        }
        ensure!(
            cursor + 3 <= queue.len(),
            "battle source PPU queue header is truncated"
        );
        let start_address = u16::from_be_bytes([address_high, queue[cursor + 1]]);
        let descriptor = queue[cursor + 2];
        let data_len = usize::from(descriptor & 0x3F);
        ensure!(
            data_len > 0,
            "battle source PPU queue has a zero-length command"
        );
        let encoded_len = if descriptor & 0x40 == 0 { data_len } else { 1 };
        let data_start = cursor + 3;
        let data_end = data_start
            .checked_add(encoded_len)
            .context("battle source PPU queue length overflow")?;
        ensure!(
            data_end <= queue.len(),
            "battle source PPU queue data is truncated"
        );
        let address_step = if descriptor & 0x80 == 0 { 1 } else { 32 };
        for index in 0..data_len {
            let address = start_address.wrapping_add((index * address_step) as u16) & 0x3FFF;
            if !is_nametable_tile_address(address) {
                continue;
            }
            let code = if encoded_len == 1 {
                queue[data_start]
            } else {
                queue[data_start + index]
            };
            classify_code(ownership, code, active_codes);
        }
        cursor = data_end;
    }
}

pub(super) fn ownership_for_candidates(
    candidates: impl IntoIterator<Item = u8>,
    active_codes: &BTreeSet<u8>,
) -> QueueCodeOwnership {
    let mut ownership = QueueCodeOwnership::default();
    for code in candidates {
        classify_code(&mut ownership, code, active_codes);
    }
    ownership
}

fn classify_code(ownership: &mut QueueCodeOwnership, code: u8, active_codes: &BTreeSet<u8>) {
    if !active_codes.contains(&code) {
        return;
    }
    if is_japanese_text_code(code) {
        ownership.japanese_active.insert(code);
    } else {
        ownership.preserved_active.insert(code);
    }
}

fn is_nametable_tile_address(address: u16) -> bool {
    (0x2000..0x3000).contains(&address) && address & 0x03FF < 0x03C0
}

pub(super) fn expected_global_preserved_codes() -> BTreeSet<u8> {
    [0xAE]
        .into_iter()
        .chain(0xC0..=0xC7)
        .chain(0xCB..=0xCF)
        .chain(0xD0..=0xDB)
        .chain(0xE0..=0xE4)
        .chain(0xF5..=0xFC)
        .collect()
}

pub(super) fn hex_codes(codes: &BTreeSet<u8>) -> Vec<String> {
    codes.iter().map(|code| format!("0x{code:02X}")).collect()
}
