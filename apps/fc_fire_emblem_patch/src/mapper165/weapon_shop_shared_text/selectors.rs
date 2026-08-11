use anyhow::{Result, ensure};

use crate::{
    choice_labels::{CHOICE_LABEL_SOURCE_PRG_BANK, POINTER_LOAD_ADDRESS, POINTER_LOAD_BYTES},
    rp2a03::{Instruction, assemble_at},
};

use super::ITEM_POINTER_TABLE_ADDRESS;

pub(crate) const CODE_RANGES: [(u16, u16); 2] = [(0xF390, 0xF400), (0xF4B0, 0xF580)];
const IDENTITY_PREDICATE_ADDRESS: u16 = 0xF390;
const IDENTITY_TRUE_ADDRESS: u16 = 0xF3CA;
const IDENTITY_FALSE_ADDRESS: u16 = 0xF3CD;
const IDENTITY_PREDICATE_END: u16 = 0xF3D0;

pub(crate) const ITEM_LIST_SELECTOR_ADDRESS: u16 = IDENTITY_PREDICATE_END;
const ITEM_LIST_ORIGINAL_POINTER_ADDRESS: u16 = 0xF3F5;
const ITEM_LIST_SELECTOR_END: u16 = 0xF400;
pub(crate) const ITEM_LIST_POINTER_LOAD_PRG_BANK: u8 = 0x0B;
pub(crate) const ITEM_LIST_POINTER_LOAD_ADDRESS: u16 = 0x8E74;
pub(crate) const ITEM_LIST_POINTER_LOAD_BYTES: [u8; 10] =
    [0xB9, 0xD5, 0xDA, 0x85, 0x00, 0xB9, 0xD6, 0xDA, 0x85, 0x01];
const ORIGINAL_ITEM_POINTER_TABLE_ADDRESS: u16 = 0xDAD5;

pub(crate) const CHOICE_SELECTOR_ADDRESS: u16 = 0xF4B0;
const CHOICE_CANDIDATE_ADDRESS: u16 = 0xF4B8;
const CHOICE_YES_ADDRESS: u16 = 0xF4CA;
const CHOICE_ORIGINAL_POINTER_ADDRESS: u16 = 0xF4D3;
const CHOICE_SELECTOR_END: u16 = 0xF4DE;
pub(crate) const CHOICE_POINTER_LOAD_ADDRESS: u16 = POINTER_LOAD_ADDRESS;
pub(crate) const CHOICE_POINTER_LOAD_BYTES: [u8; 10] = POINTER_LOAD_BYTES;
pub(crate) const CHOICE_POINTER_LOAD_PRG_BANK: u8 = CHOICE_LABEL_SOURCE_PRG_BANK;

pub(crate) const SELECTED_ITEM_SELECTOR_ADDRESS: u16 = CHOICE_SELECTOR_END;
const SELECTED_ITEM_ORIGINAL_POINTER_ADDRESS: u16 = 0xF4F3;
const SELECTED_ITEM_SELECTOR_END: u16 = 0xF4FE;
pub(crate) const SELECTED_ITEM_POINTER_LOAD_PRG_BANK: u8 = 0x06;
pub(crate) const SELECTED_ITEM_POINTER_LOAD_ADDRESS: u16 = 0x9B07;
pub(crate) const SELECTED_ITEM_POINTER_LOAD_BYTES: [u8; 10] =
    [0xB9, 0xD5, 0xDA, 0x85, 0x00, 0xB9, 0xD6, 0xDA, 0x85, 0x01];

pub(crate) fn build_weapon_shop_lifetime_identity_predicate() -> Result<Vec<u8>> {
    let routine = assemble_at(
        IDENTITY_PREDICATE_ADDRESS,
        &[
            Instruction::LdaAbsolute(0x05DB),
            Instruction::BeqAbsolute(IDENTITY_FALSE_ADDRESS),
            Instruction::CmpImmediate(0x0D),
            Instruction::BcsAbsolute(IDENTITY_FALSE_ADDRESS),
            Instruction::LdaAbsolute(0x77D0),
            Instruction::CmpImmediate(0x01),
            Instruction::BneAbsolute(IDENTITY_FALSE_ADDRESS),
            Instruction::LdaAbsolute(0x77F2),
            Instruction::CmpImmediate(0x0B),
            Instruction::BneAbsolute(IDENTITY_FALSE_ADDRESS),
            Instruction::LdaAbsolute(0x77F4),
            Instruction::CmpImmediate(0xB1),
            Instruction::BneAbsolute(IDENTITY_FALSE_ADDRESS),
            Instruction::LdaZeroPage(0x59),
            Instruction::CmpImmediate(0x1E),
            Instruction::BneAbsolute(IDENTITY_FALSE_ADDRESS),
            Instruction::LdaZeroPage(0x5A),
            Instruction::CmpImmediate(0x1E),
            Instruction::BneAbsolute(IDENTITY_FALSE_ADDRESS),
            Instruction::LdaZeroPage(0x5B),
            Instruction::CmpImmediate(0x00),
            Instruction::BneAbsolute(IDENTITY_FALSE_ADDRESS),
            Instruction::LdaZeroPage(0x5C),
            Instruction::CmpImmediate(0x15),
            Instruction::BeqAbsolute(IDENTITY_TRUE_ADDRESS),
            Instruction::CmpImmediate(0x18),
            Instruction::BneAbsolute(IDENTITY_FALSE_ADDRESS),
            Instruction::LdaImmediate(0x01),
            Instruction::Rts,
            Instruction::LdaImmediate(0x00),
            Instruction::Rts,
        ],
    )?;
    ensure!(
        usize::from(IDENTITY_PREDICATE_ADDRESS) + routine.len()
            == usize::from(IDENTITY_PREDICATE_END),
        "weapon-shop identity predicate size changed"
    );
    Ok(routine)
}

pub(crate) fn build_item_list_pointer_selector() -> Result<Vec<u8>> {
    let routine = assemble_at(
        ITEM_LIST_SELECTOR_ADDRESS,
        &[
            Instruction::LdaAbsolute(0x05DB),
            Instruction::CmpImmediate(0x03),
            Instruction::BneAbsolute(ITEM_LIST_ORIGINAL_POINTER_ADDRESS),
            Instruction::LdaAbsolute(0x77D0),
            Instruction::CmpImmediate(0x01),
            Instruction::BneAbsolute(ITEM_LIST_ORIGINAL_POINTER_ADDRESS),
            Instruction::LdaAbsolute(0x77F4),
            Instruction::CmpImmediate(0xB1),
            Instruction::BneAbsolute(ITEM_LIST_ORIGINAL_POINTER_ADDRESS),
            Instruction::Tya,
            Instruction::CmpImmediate((27 * 2) as u8),
            Instruction::BcsAbsolute(ITEM_LIST_ORIGINAL_POINTER_ADDRESS),
            Instruction::LdaAbsoluteY(ITEM_POINTER_TABLE_ADDRESS),
            Instruction::StaZeroPage(0x00),
            Instruction::LdaAbsoluteY(ITEM_POINTER_TABLE_ADDRESS + 1),
            Instruction::StaZeroPage(0x01),
            Instruction::Rts,
            Instruction::LdaAbsoluteY(ORIGINAL_ITEM_POINTER_TABLE_ADDRESS),
            Instruction::StaZeroPage(0x00),
            Instruction::LdaAbsoluteY(ORIGINAL_ITEM_POINTER_TABLE_ADDRESS + 1),
            Instruction::StaZeroPage(0x01),
            Instruction::Rts,
        ],
    )?;
    ensure!(
        usize::from(ITEM_LIST_SELECTOR_ADDRESS) + routine.len()
            == usize::from(ITEM_LIST_SELECTOR_END),
        "weapon-shop item-list selector size changed"
    );
    Ok(routine)
}

pub(crate) fn build_choice_pointer_selector(
    yes_index: u8,
    yes_pointer: u16,
    no_index: u8,
    no_pointer: u16,
) -> Result<Vec<u8>> {
    let routine = assemble_at(
        CHOICE_SELECTOR_ADDRESS,
        &[
            Instruction::CpyImmediate(yes_index * 2),
            Instruction::BeqAbsolute(CHOICE_CANDIDATE_ADDRESS),
            Instruction::CpyImmediate(no_index * 2),
            Instruction::BneAbsolute(CHOICE_ORIGINAL_POINTER_ADDRESS),
            Instruction::JsrAbsolute(IDENTITY_PREDICATE_ADDRESS),
            Instruction::BeqAbsolute(CHOICE_ORIGINAL_POINTER_ADDRESS),
            Instruction::CpyImmediate(yes_index * 2),
            Instruction::BeqAbsolute(CHOICE_YES_ADDRESS),
            Instruction::LdaImmediate(no_pointer as u8),
            Instruction::StaZeroPage(0x00),
            Instruction::LdaImmediate((no_pointer >> 8) as u8),
            Instruction::StaZeroPage(0x01),
            Instruction::Rts,
            Instruction::LdaImmediate(yes_pointer as u8),
            Instruction::StaZeroPage(0x00),
            Instruction::LdaImmediate((yes_pointer >> 8) as u8),
            Instruction::StaZeroPage(0x01),
            Instruction::Rts,
            Instruction::LdaAbsoluteY(0x8FC2),
            Instruction::StaZeroPage(0x00),
            Instruction::LdaAbsoluteY(0x8FC3),
            Instruction::StaZeroPage(0x01),
            Instruction::Rts,
        ],
    )?;
    ensure!(
        usize::from(CHOICE_SELECTOR_ADDRESS) + routine.len() == usize::from(CHOICE_SELECTOR_END),
        "weapon-shop choice selector size changed"
    );
    Ok(routine)
}

pub(crate) fn build_selected_item_pointer_selector() -> Result<Vec<u8>> {
    let routine = assemble_at(
        SELECTED_ITEM_SELECTOR_ADDRESS,
        &[
            Instruction::JsrAbsolute(IDENTITY_PREDICATE_ADDRESS),
            Instruction::BeqAbsolute(SELECTED_ITEM_ORIGINAL_POINTER_ADDRESS),
            Instruction::Tya,
            Instruction::CmpImmediate((27 * 2) as u8),
            Instruction::BcsAbsolute(SELECTED_ITEM_ORIGINAL_POINTER_ADDRESS),
            Instruction::LdaAbsoluteY(ITEM_POINTER_TABLE_ADDRESS),
            Instruction::StaZeroPage(0x00),
            Instruction::LdaAbsoluteY(ITEM_POINTER_TABLE_ADDRESS + 1),
            Instruction::StaZeroPage(0x01),
            Instruction::Rts,
            Instruction::LdaAbsoluteY(ORIGINAL_ITEM_POINTER_TABLE_ADDRESS),
            Instruction::StaZeroPage(0x00),
            Instruction::LdaAbsoluteY(ORIGINAL_ITEM_POINTER_TABLE_ADDRESS + 1),
            Instruction::StaZeroPage(0x01),
            Instruction::Rts,
        ],
    )?;
    ensure!(
        usize::from(SELECTED_ITEM_SELECTOR_ADDRESS) + routine.len()
            == usize::from(SELECTED_ITEM_SELECTOR_END),
        "weapon-shop selected-item selector size changed"
    );
    Ok(routine)
}

pub(crate) fn build_item_list_pointer_load_call() -> Result<Vec<u8>> {
    build_pointer_load_call(ITEM_LIST_POINTER_LOAD_ADDRESS, ITEM_LIST_SELECTOR_ADDRESS)
}

pub(crate) fn build_choice_pointer_load_call() -> Result<Vec<u8>> {
    build_pointer_load_call(CHOICE_POINTER_LOAD_ADDRESS, CHOICE_SELECTOR_ADDRESS)
}

pub(crate) fn build_selected_item_pointer_load_call() -> Result<Vec<u8>> {
    build_pointer_load_call(
        SELECTED_ITEM_POINTER_LOAD_ADDRESS,
        SELECTED_ITEM_SELECTOR_ADDRESS,
    )
}

fn build_pointer_load_call(origin: u16, selector: u16) -> Result<Vec<u8>> {
    let mut call = assemble_at(origin, &[Instruction::JsrAbsolute(selector)])?;
    call.resize(10, assemble_at(origin, &[Instruction::Nop])?[0]);
    Ok(call)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn selectors_fall_back_to_the_original_pointer_tables() {
        let item_list = build_item_list_pointer_selector().unwrap();
        let selected_item = build_selected_item_pointer_selector().unwrap();
        let choice = build_choice_pointer_selector(0x22, 0xF480, 0x23, 0xF482).unwrap();

        assert!(
            item_list
                .windows(3)
                .any(|bytes| bytes == [0xB9, 0xD5, 0xDA])
        );
        assert!(
            selected_item
                .windows(3)
                .any(|bytes| bytes == [0xB9, 0xD5, 0xDA])
        );
        assert!(choice.windows(3).any(|bytes| bytes == [0xB9, 0xC2, 0x8F]));
        assert!(choice.windows(2).any(|bytes| bytes == [0xC0, 0x44]));
        assert!(choice.windows(2).any(|bytes| bytes == [0xC0, 0x46]));
    }

    #[test]
    fn item_list_selector_binds_the_observed_shop_composition_state() {
        let item_list = build_item_list_pointer_selector().unwrap();

        assert!(
            item_list
                .windows(5)
                .any(|bytes| bytes == [0xAD, 0xDB, 0x05, 0xC9, 0x03])
        );
        assert!(
            item_list
                .windows(5)
                .any(|bytes| bytes == [0xAD, 0xD0, 0x77, 0xC9, 0x01])
        );
        assert!(
            item_list
                .windows(5)
                .any(|bytes| bytes == [0xAD, 0xF4, 0x77, 0xC9, 0xB1])
        );
        assert_eq!(ITEM_LIST_POINTER_LOAD_PRG_BANK, 0x0B);
        assert_eq!(ITEM_LIST_POINTER_LOAD_ADDRESS, 0x8E74);
    }

    #[test]
    fn selected_item_selector_binds_the_weapon_shop_lifetime() {
        let selected_item = build_selected_item_pointer_selector().unwrap();

        assert_eq!(&selected_item[..3], &[0x20, 0x90, 0xF3]);
        assert!(
            selected_item
                .windows(3)
                .any(|bytes| bytes == [0xB9, 0x00, 0xF5])
        );
        assert_eq!(SELECTED_ITEM_POINTER_LOAD_PRG_BANK, 0x06);
        assert_eq!(SELECTED_ITEM_POINTER_LOAD_ADDRESS, 0x9B07);
    }

    #[test]
    fn pointer_load_hooks_preserve_the_owned_ten_byte_spans() {
        let item_list = build_item_list_pointer_load_call().unwrap();
        let selected_item = build_selected_item_pointer_load_call().unwrap();
        let choice = build_choice_pointer_load_call().unwrap();

        assert_eq!(item_list.len(), ITEM_LIST_POINTER_LOAD_BYTES.len());
        assert_eq!(&item_list[..3], &[0x20, 0xD0, 0xF3]);
        assert_eq!(selected_item.len(), SELECTED_ITEM_POINTER_LOAD_BYTES.len());
        assert_eq!(&selected_item[..3], &[0x20, 0xDE, 0xF4]);
        assert_eq!(choice.len(), CHOICE_POINTER_LOAD_BYTES.len());
        assert_eq!(&choice[..3], &[0x20, 0xB0, 0xF4]);
    }
}
