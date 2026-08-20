//! Selects the item-string material whose encoding is resident for one shop lifetime.

use anyhow::{Context, Result, bail, ensure};

use super::{MATERIAL_ROUTE_CATALOG, MATERIAL_ROUTE_DIALOGUE};
use crate::full_translation_install::{
    runtime_code::RuntimeRoutine, shop_item_residency::ShopItemResidencyRuntimeContract,
};

#[derive(Debug, Eq, PartialEq)]
pub(super) struct ItemMaterialRoute {
    pub(super) directory: u16,
    pub(super) material_page: u8,
    pub(super) material_base: u16,
}

pub(super) fn select_item_material(
    routine: &RuntimeRoutine,
    contract: ShopItemResidencyRuntimeContract,
    outer_state: u8,
    facility: u8,
    directory: u8,
    e7_resume: u8,
) -> Result<ItemMaterialRoute> {
    let predicate = [
        0xAD,
        contract.outer_state_address as u8,
        (contract.outer_state_address >> 8) as u8,
        0xC9,
        contract.composition_state,
    ];
    let predicate_offsets = routine
        .bytes
        .windows(predicate.len())
        .enumerate()
        .filter_map(|(offset, window)| (window == predicate).then_some(offset))
        .collect::<Vec<_>>();
    ensure!(
        predicate_offsets.len() == 1,
        "consumer catalog emitted {} shop item selectors",
        predicate_offsets.len()
    );
    let mut memory = Box::new([0_u8; 0x10000]);
    let start = usize::from(routine.address);
    let end = start
        .checked_add(routine.bytes.len())
        .context("consumer catalog routine range overflow")?;
    ensure!(
        end <= memory.len(),
        "consumer catalog routine exceeds CPU memory"
    );
    memory[start..end].copy_from_slice(&routine.bytes);
    memory[usize::from(contract.outer_state_address)] = outer_state;
    memory[usize::from(contract.selected_facility_address)] = facility;
    memory[usize::from(contract.dialogue_directory_address)] = directory;
    memory[usize::from(contract.e7_caller_resume_flag_address)] = e7_resume;

    let mut pc = routine.address
        + u16::try_from(predicate_offsets[0]).context("shop selector offset exceeds u16")?;
    let mut a = 0_u8;
    let mut zero = false;
    for _ in 0..64 {
        let opcode = memory[usize::from(pc)];
        pc = pc.wrapping_add(1);
        match opcode {
            0x4C => {
                let low = memory[usize::from(pc)];
                let high = memory[usize::from(pc.wrapping_add(1))];
                pc = u16::from_le_bytes([low, high]);
            }
            0x85 => {
                let address = memory[usize::from(pc)];
                pc = pc.wrapping_add(1);
                memory[usize::from(address)] = a;
                if address == 0x03 {
                    let material_base = match a {
                        MATERIAL_ROUTE_CATALOG => contract.catalog_material_base,
                        MATERIAL_ROUTE_DIALOGUE => contract.dialogue_material_base,
                        route => bail!("shop item selector chose unknown material route {route}"),
                    };
                    return Ok(ItemMaterialRoute {
                        directory: u16::from_le_bytes([memory[0], memory[1]]),
                        material_page: memory[2],
                        material_base,
                    });
                }
            }
            0xA9 => {
                a = memory[usize::from(pc)];
                pc = pc.wrapping_add(1);
                zero = a == 0;
            }
            0xAD => {
                let low = memory[usize::from(pc)];
                let high = memory[usize::from(pc.wrapping_add(1))];
                pc = pc.wrapping_add(2);
                a = memory[usize::from(u16::from_le_bytes([low, high]))];
                zero = a == 0;
            }
            0xC9 => {
                let value = memory[usize::from(pc)];
                pc = pc.wrapping_add(1);
                zero = a == value;
            }
            0xD0 => {
                let displacement = memory[usize::from(pc)] as i8;
                pc = pc.wrapping_add(1);
                if !zero {
                    pc = pc.wrapping_add_signed(i16::from(displacement));
                }
            }
            0xF0 => {
                let displacement = memory[usize::from(pc)] as i8;
                pc = pc.wrapping_add(1);
                if zero {
                    pc = pc.wrapping_add_signed(i16::from(displacement));
                }
            }
            other => bail!("shop item selector reached unsupported opcode {other:02X}"),
        }
    }
    bail!("shop item selector did not choose a material")
}

pub(in crate::full_translation_install::runtime_code) fn verify_shop_item_residency_route(
    routine: &RuntimeRoutine,
    contract: ShopItemResidencyRuntimeContract,
) -> Result<()> {
    let dialogue = ItemMaterialRoute {
        directory: contract.dialogue_item_directory,
        material_page: contract.dialogue_material_page,
        material_base: contract.dialogue_material_base,
    };
    let catalog = ItemMaterialRoute {
        directory: contract.catalog_item_directory,
        material_page: contract.catalog_material_page,
        material_base: contract.catalog_material_base,
    };
    for facility in contract.selling_facilities {
        ensure!(
            select_item_material(
                routine,
                contract,
                contract.composition_state,
                facility,
                contract.dialogue_directory_selector,
                1,
            )? == dialogue,
            "selling facility {facility:02X} does not use dialogue-encoded item material"
        );
    }
    for facility in contract.non_selling_facilities {
        ensure!(
            select_item_material(
                routine,
                contract,
                contract.composition_state,
                facility,
                contract.dialogue_directory_selector,
                1,
            )? == catalog,
            "non-selling facility {facility:02X} escaped the catalog item route"
        );
    }
    for (outer_state, facility, directory, e7_resume) in [
        (
            contract.composition_state.wrapping_add(1),
            contract.selling_facilities[0],
            contract.dialogue_directory_selector,
            1,
        ),
        (
            contract.composition_state,
            contract.selling_facilities[0],
            contract.dialogue_directory_selector.wrapping_add(1),
            1,
        ),
        (
            contract.composition_state,
            contract.selling_facilities[0],
            contract.dialogue_directory_selector,
            0,
        ),
    ] {
        ensure!(
            select_item_material(
                routine,
                contract,
                outer_state,
                facility,
                directory,
                e7_resume,
            )? == catalog,
            "shop item selector uses dialogue material outside its complete source lifetime"
        );
    }
    Ok(())
}
