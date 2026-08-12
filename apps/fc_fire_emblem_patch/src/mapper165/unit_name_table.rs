use std::collections::BTreeMap;

use anyhow::{Context, Result, ensure};

use crate::{
    rp2a03::{Instruction, assemble_at},
    unit_names::UnitNamePlan,
};

pub(super) const SOURCE_PRG_BANK: u8 = 0x0B;
pub(super) const CAVE_START_ADDRESS: u16 = 0xB6D0;
/// 뱅크 `0B`에서 `CAVE_START_ADDRESS`부터 이어지는 `FF` 구간의 실측 끝이다.
/// 여기부터는 `$9251`로 시작하는 원본 포인터 표와 NMI 뱅크 로컬 디렉터리 `$BFC0`가 있다.
/// 이 구간은 뱅크 `0B`의 대사 저장 최상단 `$B474`보다 위라 전이 미러 payload와 겹치지 않는다.
pub(super) const CAVE_END_ADDRESS: u16 = 0xBFA0;
pub(super) const SELECTOR_ADDRESS: u16 = CAVE_START_ADDRESS;
pub(super) const ROSTER_POINTER_TABLE_ADDRESS: u16 = 0xB700;
pub(super) const PLAYER_POINTER_LOAD_ADDRESS: u16 = 0x8EA6;
pub(super) const PLAYER_POINTER_LOAD_LEN: usize = 10;
pub(super) const SOURCE_PLAYER_POINTER_LOAD: [u8; PLAYER_POINTER_LOAD_LEN] =
    [0xB9, 0x2B, 0xDE, 0x85, 0x00, 0xB9, 0x2C, 0xDE, 0x85, 0x01];
const ORIGINAL_POINTER_TABLE_ADDRESS: u16 = 0xDE2B;
const TERMINATOR: u8 = 0xEF;

pub(super) struct UnitNameTableProjection {
    pub(super) pointer_table_address: u16,
    pub(super) pointer_table: Vec<u8>,
    pub(super) string_data_address: u16,
    pub(super) strings: Vec<u8>,
}

impl UnitNameTableProjection {
    fn end_address(&self) -> usize {
        self.string_data_address as usize + self.strings.len()
    }
}

pub(super) struct UnitNameTablePlan {
    pub(super) selector: Vec<u8>,
    pub(super) selector_call: Vec<u8>,
    pub(super) roster: UnitNameTableProjection,
    pub(super) unit_ui: UnitNameTableProjection,
}

/// 네 구간을 실제 길이에서 이어 붙이고 케이브 끝 하나로만 검사한다.
///
/// 예전에는 구간마다 주소를 상수로 박아 두어 이름 한 글자가 길어져도 빌드가 깨졌다.
/// 아군명은 용어 확정에 따라 계속 길이가 바뀌므로 배치를 계산으로 옮긴다.
pub(super) fn plan_unit_name_tables(
    names: &UnitNamePlan,
    roster_assignments: &BTreeMap<char, u8>,
    unit_ui_assignments: &BTreeMap<char, u8>,
) -> Result<UnitNameTablePlan> {
    let pointer_table_len =
        u16::try_from(names.entries.len() * 2).context("unit-name pointer table is too large")?;

    let roster_string_data_address = ROSTER_POINTER_TABLE_ADDRESS
        .checked_add(pointer_table_len)
        .context("roster unit-name string data overflows the bank")?;
    let roster = build_projection(
        names,
        roster_assignments,
        ROSTER_POINTER_TABLE_ADDRESS,
        roster_string_data_address,
    )?;

    let unit_ui_pointer_table_address = u16::try_from(roster.end_address())
        .context("unit-UI name pointer table overflows the bank")?;
    let unit_ui_string_data_address = unit_ui_pointer_table_address
        .checked_add(pointer_table_len)
        .context("unit-UI name string data overflows the bank")?;
    let unit_ui = build_projection(
        names,
        unit_ui_assignments,
        unit_ui_pointer_table_address,
        unit_ui_string_data_address,
    )?;

    ensure!(
        ROSTER_POINTER_TABLE_ADDRESS >= SELECTOR_ADDRESS,
        "unit-name tables start before the bank-0B cave"
    );
    ensure!(
        unit_ui.end_address() <= CAVE_END_ADDRESS as usize,
        "unit-name tables need {} bytes but the proven bank-0B cave ends at 0x{CAVE_END_ADDRESS:04X}",
        unit_ui.end_address() - ROSTER_POINTER_TABLE_ADDRESS as usize
    );

    let selector = build_pointer_selector(unit_ui_pointer_table_address)?;
    let selector_call = build_pointer_selector_call()?;
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
    pointer_table_address: u16,
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
        pointer_table_address,
        pointer_table,
        string_data_address,
        strings,
    })
}

fn build_pointer_selector(unit_ui_pointer_table_address: u16) -> Result<Vec<u8>> {
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
            Instruction::LdaAbsoluteY(unit_ui_pointer_table_address),
            Instruction::StaZeroPage(0x00),
            Instruction::LdaAbsoluteY(unit_ui_pointer_table_address + 1),
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
        let selector = build_pointer_selector(0xB821).unwrap();
        let call = build_pointer_selector_call().unwrap();

        assert_eq!(call.len(), PLAYER_POINTER_LOAD_LEN);
        assert_eq!(&call[..3], &[0x20, 0xD0, 0xB6]);
        assert!(selector.windows(2).any(|bytes| bytes == [0xC9, 0x02]));
        assert!(selector.windows(2).any(|bytes| bytes == [0xC9, 0x04]));
        assert!(selector.windows(3).any(|bytes| bytes == [0xB9, 0x2B, 0xDE]));
    }
}
