use std::collections::BTreeMap;

use anyhow::{Context, Result, ensure};

use crate::{
    rp2a03::{Instruction, assemble_at},
    unit_names::UnitNamePlan,
};

pub(super) const SOURCE_PRG_BANK: u8 = 0x0B;
pub(super) const CAVE_START_ADDRESS: u16 = 0xB6D0;
pub(super) const CAVE_END_ADDRESS: u16 = 0xB940;
pub(super) const SELECTOR_ADDRESS: u16 = CAVE_START_ADDRESS;
pub(super) const ROSTER_POINTER_TABLE_ADDRESS: u16 = 0xB700;
pub(super) const ROSTER_STRING_DATA_ADDRESS: u16 = 0xB768;
pub(super) const UNIT_UI_POINTER_TABLE_ADDRESS: u16 = 0xB820;
pub(super) const UNIT_UI_STRING_DATA_ADDRESS: u16 = 0xB888;
pub(super) const PLAYER_POINTER_LOAD_ADDRESS: u16 = 0x8EA6;
pub(super) const PLAYER_POINTER_LOAD_LEN: usize = 10;
pub(super) const SOURCE_PLAYER_POINTER_LOAD: [u8; PLAYER_POINTER_LOAD_LEN] =
    [0xB9, 0x2B, 0xDE, 0x85, 0x00, 0xB9, 0x2C, 0xDE, 0x85, 0x01];
const ORIGINAL_POINTER_TABLE_ADDRESS: u16 = 0xDE2B;
const TERMINATOR: u8 = 0xEF;

pub(super) struct UnitNameTableProjection {
    pub(super) pointer_table: Vec<u8>,
    pub(super) strings: Vec<u8>,
}

pub(super) struct UnitNameTablePlan {
    pub(super) selector: Vec<u8>,
    pub(super) selector_call: Vec<u8>,
    pub(super) roster: UnitNameTableProjection,
    pub(super) unit_ui: UnitNameTableProjection,
}

pub(super) fn plan_unit_name_tables(
    names: &UnitNamePlan,
    roster_assignments: &BTreeMap<char, u8>,
    unit_ui_assignments: &BTreeMap<char, u8>,
) -> Result<UnitNameTablePlan> {
    let selector = build_pointer_selector()?;
    let selector_call = build_pointer_selector_call()?;
    let roster = build_projection(names, roster_assignments, ROSTER_STRING_DATA_ADDRESS)?;
    let unit_ui = build_projection(names, unit_ui_assignments, UNIT_UI_STRING_DATA_ADDRESS)?;
    ensure!(
        ROSTER_POINTER_TABLE_ADDRESS as usize + roster.pointer_table.len()
            <= ROSTER_STRING_DATA_ADDRESS as usize,
        "roster unit-name pointer table overlaps its strings"
    );
    ensure!(
        ROSTER_STRING_DATA_ADDRESS as usize + roster.strings.len()
            <= UNIT_UI_POINTER_TABLE_ADDRESS as usize,
        "roster unit-name strings overlap the unit-UI table"
    );
    ensure!(
        UNIT_UI_POINTER_TABLE_ADDRESS as usize + unit_ui.pointer_table.len()
            <= UNIT_UI_STRING_DATA_ADDRESS as usize,
        "unit-UI name pointer table overlaps its strings"
    );
    ensure!(
        UNIT_UI_STRING_DATA_ADDRESS as usize + unit_ui.strings.len() <= CAVE_END_ADDRESS as usize,
        "unit-UI name strings exceed the proven bank-0B cave"
    );
    Ok(UnitNameTablePlan {
        selector,
        selector_call,
        roster,
        unit_ui,
    })
}

fn build_projection(
    names: &UnitNamePlan,
    assignments: &BTreeMap<char, u8>,
    string_data_address: u16,
) -> Result<UnitNameTableProjection> {
    let mut pointer_table = Vec::with_capacity(names.entries.len() * 2);
    let mut strings = Vec::new();
    for entry in &names.entries {
        let string_offset =
            u16::try_from(strings.len()).context("unit-name string pack is too large")?;
        let pointer = string_data_address
            .checked_add(string_offset)
            .context("unit-name string pointer overflow")?;
        pointer_table.extend_from_slice(&pointer.to_le_bytes());
        strings.extend(entry.encoded_bytes(assignments)?);
        strings.push(TERMINATOR);
    }
    ensure!(
        pointer_table.len() == names.entries.len() * 2,
        "unit-name pointer table size changed"
    );
    Ok(UnitNameTableProjection {
        pointer_table,
        strings,
    })
}

fn build_pointer_selector() -> Result<Vec<u8>> {
    const ROSTER_STATE: u8 = 0x02;
    const UNIT_SUMMARY_STATE: u8 = 0x04;
    const ROSTER_ADDRESS: u16 = 0xB6E6;
    const ORIGINAL_ADDRESS: u16 = 0xB6F1;
    assemble_at(
        SELECTOR_ADDRESS,
        &[
            Instruction::LdaAbsolute(0x05E8),
            Instruction::CmpImmediate(ROSTER_STATE),
            Instruction::BeqAbsolute(ROSTER_ADDRESS),
            Instruction::CmpImmediate(UNIT_SUMMARY_STATE),
            Instruction::BneAbsolute(ORIGINAL_ADDRESS),
            Instruction::LdaAbsoluteY(UNIT_UI_POINTER_TABLE_ADDRESS),
            Instruction::StaZeroPage(0x00),
            Instruction::LdaAbsoluteY(UNIT_UI_POINTER_TABLE_ADDRESS + 1),
            Instruction::StaZeroPage(0x01),
            Instruction::Rts,
            Instruction::LdaAbsoluteY(ROSTER_POINTER_TABLE_ADDRESS),
            Instruction::StaZeroPage(0x00),
            Instruction::LdaAbsoluteY(ROSTER_POINTER_TABLE_ADDRESS + 1),
            Instruction::StaZeroPage(0x01),
            Instruction::Rts,
            Instruction::LdaAbsoluteY(ORIGINAL_POINTER_TABLE_ADDRESS),
            Instruction::StaZeroPage(0x00),
            Instruction::LdaAbsoluteY(ORIGINAL_POINTER_TABLE_ADDRESS + 1),
            Instruction::StaZeroPage(0x01),
            Instruction::Rts,
        ],
    )
}

fn build_pointer_selector_call() -> Result<Vec<u8>> {
    let mut bytes = assemble_at(
        PLAYER_POINTER_LOAD_ADDRESS,
        &[Instruction::JsrAbsolute(SELECTOR_ADDRESS)],
    )?;
    bytes.resize(PLAYER_POINTER_LOAD_LEN, 0xEA);
    Ok(bytes)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pointer_selector_translates_only_roster_and_unit_summary_consumers() {
        let selector = build_pointer_selector().unwrap();
        let call = build_pointer_selector_call().unwrap();

        assert_eq!(call.len(), PLAYER_POINTER_LOAD_LEN);
        assert_eq!(&call[..3], &[0x20, 0xD0, 0xB6]);
        assert!(selector.windows(2).any(|bytes| bytes == [0xC9, 0x02]));
        assert!(selector.windows(2).any(|bytes| bytes == [0xC9, 0x04]));
        assert!(selector.windows(3).any(|bytes| bytes == [0xB9, 0x2B, 0xDE]));
    }
}
