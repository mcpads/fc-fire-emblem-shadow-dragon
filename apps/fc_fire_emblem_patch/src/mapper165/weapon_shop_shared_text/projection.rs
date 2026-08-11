use std::collections::BTreeMap;

use anyhow::{Context, Result, ensure};

use crate::{
    choice_labels::ChoiceLabelPlan, mmc5_prg::fixed_bank_file_offset, rom::Rom,
    text_inventory::FixedTextPlannedEntry,
};

use super::ITEM_NAME_SOURCE_INDICES;

const SOURCE_ITEM_POINTER_TABLE_ADDRESS: u16 = 0xDAD5;
const PROJECTED_ITEM_POINTER_COUNT: usize = 27;
const ITEM_TERMINATOR: u8 = 0xEF;

pub(crate) const ITEM_POINTER_TABLE_ADDRESS: u16 = 0xF500;
pub(crate) const STRING_DATA_ADDRESS: u16 = 0xF540;

pub(crate) struct WeaponShopTextProjection {
    pub(crate) item_pointer_table: Vec<u8>,
    pub(crate) strings: Vec<u8>,
    pub(crate) yes_pointer: u16,
    pub(crate) no_pointer: u16,
    pub(crate) item_name_count: usize,
    pub(crate) item_string_byte_count: usize,
    pub(crate) choice_string_byte_count: usize,
}

pub(crate) fn build_weapon_shop_text_projection(
    source_rom: &Rom,
    item_entries: &[FixedTextPlannedEntry],
    choice_labels: &ChoiceLabelPlan,
    assignments: &BTreeMap<char, u8>,
) -> Result<WeaponShopTextProjection> {
    ensure!(
        item_entries.len() == ITEM_NAME_SOURCE_INDICES.len()
            && item_entries
                .iter()
                .map(|entry| entry.source_index)
                .eq(ITEM_NAME_SOURCE_INDICES),
        "weapon-shop item projection order changed"
    );
    let table_offset = fixed_bank_file_offset(SOURCE_ITEM_POINTER_TABLE_ADDRESS)?;
    let table_len = PROJECTED_ITEM_POINTER_COUNT * 2;
    let mut item_pointer_table = source_rom
        .data()
        .get(table_offset..table_offset + table_len)
        .context("weapon-shop source item pointer prefix is outside the ROM")?
        .to_vec();

    let mut strings = Vec::new();
    for entry in item_entries {
        let pointer = string_pointer(strings.len())?;
        let table_index = entry
            .source_index
            .checked_mul(2)
            .context("weapon-shop item pointer index overflow")?;
        item_pointer_table[table_index..table_index + 2].copy_from_slice(&pointer.to_le_bytes());
        strings.extend(entry.encoded_bytes(assignments)?);
        strings.push(ITEM_TERMINATOR);
    }
    let item_string_byte_count = strings.len();

    let yes_pointer = string_pointer(strings.len())?;
    strings.extend(
        choice_labels
            .entry("choice-label:yes")?
            .encoded_bytes(assignments)?,
    );
    let no_pointer = string_pointer(strings.len())?;
    strings.extend(
        choice_labels
            .entry("choice-label:no")?
            .encoded_bytes(assignments)?,
    );
    let choice_string_byte_count = strings.len() - item_string_byte_count;
    ensure!(
        usize::from(STRING_DATA_ADDRESS) + strings.len() <= 0xF580,
        "weapon-shop shared strings exceed the checked fixed-bank cave"
    );

    Ok(WeaponShopTextProjection {
        item_pointer_table,
        strings,
        yes_pointer,
        no_pointer,
        item_name_count: item_entries.len(),
        item_string_byte_count,
        choice_string_byte_count,
    })
}

fn string_pointer(offset: usize) -> Result<u16> {
    STRING_DATA_ADDRESS
        .checked_add(u16::try_from(offset).context("weapon-shop string pack is too large")?)
        .context("weapon-shop string pointer overflow")
}
