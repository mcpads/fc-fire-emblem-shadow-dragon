//! 대사와 일반 카탈로그가 공유하는 이름 material 및 글꼴 선택을 실행 의미로 검증한다.

use anyhow::{Context, Result, bail, ensure};

use super::{
    CLASS_NAME_ONLY_COMPOSITE_STATE, COMPOSITE_STATE, KIND_CLASS, KIND_ITEM, KIND_SHOP_ITEM_LIST,
    KIND_UNIT_OR_ENEMY, MATERIAL_ROUTE_CATALOG, MATERIAL_ROUTE_DIALOGUE,
};
use crate::full_translation_install::{
    runtime_code::RuntimeRoutine, shop_item_residency::ShopItemResidencyRuntimeContract,
    storage_residency::StorageItemListRuntimeRoute,
};

#[derive(Debug, Eq, PartialEq)]
pub(super) struct ItemMaterialRoute {
    pub(super) directory: u16,
    pub(super) material_page: u8,
    pub(super) material_base: u16,
}

#[derive(Clone, Copy, Debug)]
pub(super) enum StorageItemConsumer {
    RecordAppender,
    DirectOrListAppender,
}

pub(super) fn select_shop_item_material(
    routine: &RuntimeRoutine,
    material: ShopItemResidencyRuntimeContract,
    outer_state: u8,
    facility: u8,
    directory: u8,
    e7_resume: u8,
) -> Result<ItemMaterialRoute> {
    let predicate = [
        0xAD,
        material.outer_state_address as u8,
        (material.outer_state_address >> 8) as u8,
        0xC9,
        material.composition_state,
    ];
    let mut memory = Box::new([0_u8; 0x10000]);
    memory[usize::from(material.outer_state_address)] = outer_state;
    memory[usize::from(material.selected_facility_address)] = facility;
    memory[usize::from(material.dialogue_directory_address)] = directory;
    memory[usize::from(material.e7_caller_resume_flag_address)] = e7_resume;
    select_material_from_predicate(routine, material, &predicate, memory, "shop")
}

pub(super) fn select_storage_item_material(
    routine: &RuntimeRoutine,
    material: ShopItemResidencyRuntimeContract,
    storage: StorageItemListRuntimeRoute,
    consumer: StorageItemConsumer,
    composite_state: u8,
    caller_state: u8,
) -> Result<ItemMaterialRoute> {
    let predicate = [
        0xAD,
        COMPOSITE_STATE as u8,
        (COMPOSITE_STATE >> 8) as u8,
        0xC9,
        storage.deposit.composite_state,
    ];
    let mut memory = Box::new([0_u8; 0x10000]);
    memory[usize::from(COMPOSITE_STATE)] = composite_state;
    memory[usize::from(storage.caller_state_address)] = caller_state;
    let predicate_index = match consumer {
        StorageItemConsumer::RecordAppender => 0,
        StorageItemConsumer::DirectOrListAppender => 1,
    };
    select_material_from_predicate_at(
        routine,
        material,
        &predicate,
        predicate_index,
        2,
        memory,
        "storage",
    )
}

fn select_material_from_predicate(
    routine: &RuntimeRoutine,
    material: ShopItemResidencyRuntimeContract,
    predicate: &[u8],
    memory: Box<[u8; 0x10000]>,
    role: &str,
) -> Result<ItemMaterialRoute> {
    select_material_from_predicate_at(routine, material, predicate, 0, 1, memory, role)
}

fn select_material_from_predicate_at(
    routine: &RuntimeRoutine,
    material: ShopItemResidencyRuntimeContract,
    predicate: &[u8],
    predicate_index: usize,
    expected_predicate_count: usize,
    mut memory: Box<[u8; 0x10000]>,
    role: &str,
) -> Result<ItemMaterialRoute> {
    let predicate_offsets = routine
        .bytes
        .windows(predicate.len())
        .enumerate()
        .filter_map(|(offset, window)| (window == predicate).then_some(offset))
        .collect::<Vec<_>>();
    ensure!(
        predicate_offsets.len() == expected_predicate_count,
        "consumer catalog emitted {} {role} item selectors",
        predicate_offsets.len()
    );
    let start = usize::from(routine.address);
    let end = start
        .checked_add(routine.bytes.len())
        .context("consumer catalog routine range overflow")?;
    ensure!(
        end <= memory.len(),
        "consumer catalog routine exceeds CPU memory"
    );
    memory[start..end].copy_from_slice(&routine.bytes);

    let mut pc = routine.address
        + u16::try_from(predicate_offsets[predicate_index])
            .context("item selector offset exceeds u16")?;
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
                        MATERIAL_ROUTE_CATALOG => material.catalog_material_base,
                        MATERIAL_ROUTE_DIALOGUE => material.dialogue_material_base,
                        route => bail!("{role} item selector chose unknown material route {route}"),
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
            other => bail!("{role} item selector reached unsupported opcode {other:02X}"),
        }
    }
    bail!("{role} item selector did not choose a material")
}

fn dialogue_material(material: ShopItemResidencyRuntimeContract) -> ItemMaterialRoute {
    ItemMaterialRoute {
        directory: material.dialogue_item_directory,
        material_page: material.dialogue_material_page,
        material_base: material.dialogue_material_base,
    }
}

fn catalog_material(material: ShopItemResidencyRuntimeContract) -> ItemMaterialRoute {
    ItemMaterialRoute {
        directory: material.catalog_item_directory,
        material_page: material.catalog_material_page,
        material_base: material.catalog_material_base,
    }
}

pub(in crate::full_translation_install::runtime_code) fn verify_shop_item_residency_route(
    routine: &RuntimeRoutine,
    material: ShopItemResidencyRuntimeContract,
) -> Result<()> {
    let dialogue = dialogue_material(material);
    let catalog = catalog_material(material);
    for facility in material.selling_facilities {
        ensure!(
            select_shop_item_material(
                routine,
                material,
                material.composition_state,
                facility,
                material.dialogue_directory_selector,
                1,
            )? == dialogue,
            "selling facility {facility:02X} does not use dialogue-encoded item material"
        );
    }
    for facility in material.non_selling_facilities {
        ensure!(
            select_shop_item_material(
                routine,
                material,
                material.composition_state,
                facility,
                material.dialogue_directory_selector,
                1,
            )? == catalog,
            "non-selling facility {facility:02X} escaped the catalog item route"
        );
    }
    for (outer_state, facility, directory, e7_resume) in [
        (
            material.composition_state.wrapping_add(1),
            material.selling_facilities[0],
            material.dialogue_directory_selector,
            1,
        ),
        (
            material.composition_state,
            material.selling_facilities[0],
            material.dialogue_directory_selector.wrapping_add(1),
            1,
        ),
        (
            material.composition_state,
            material.selling_facilities[0],
            material.dialogue_directory_selector,
            0,
        ),
    ] {
        ensure!(
            select_shop_item_material(
                routine,
                material,
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

pub(in crate::full_translation_install::runtime_code) fn verify_storage_item_residency_route(
    routine: &RuntimeRoutine,
    material: ShopItemResidencyRuntimeContract,
    storage: StorageItemListRuntimeRoute,
) -> Result<()> {
    for (consumer, context) in [
        (StorageItemConsumer::RecordAppender, storage.deposit),
        (StorageItemConsumer::DirectOrListAppender, storage.withdraw),
        (StorageItemConsumer::DirectOrListAppender, storage.overflow),
    ] {
        ensure!(
            select_storage_item_material(
                routine,
                material,
                storage,
                consumer,
                context.composite_state,
                context.caller_state,
            )? == dialogue_material(material),
            "storage item-list context {:02X}/{:02X} does not use dialogue-encoded item material",
            context.composite_state,
            context.caller_state,
        );
    }
    for (consumer, composite_state, caller_state) in [
        (
            StorageItemConsumer::RecordAppender,
            storage.deposit.composite_state,
            storage.withdraw.caller_state,
        ),
        (
            StorageItemConsumer::DirectOrListAppender,
            storage.overflow.composite_state,
            storage.withdraw.caller_state,
        ),
        (
            StorageItemConsumer::DirectOrListAppender,
            storage.withdraw.composite_state,
            storage.deposit.caller_state,
        ),
        // Composite state 0x1E is shared with ordinary item-use results. Only the source-bound
        // storage caller may retain dialogue material.
        (
            StorageItemConsumer::DirectOrListAppender,
            storage.withdraw.composite_state,
            0x03,
        ),
        (
            StorageItemConsumer::RecordAppender,
            storage.deposit.composite_state.wrapping_add(1),
            storage.deposit.caller_state,
        ),
    ] {
        ensure!(
            select_storage_item_material(
                routine,
                material,
                storage,
                consumer,
                composite_state,
                caller_state,
            )? == catalog_material(material),
            "item selector uses dialogue material outside the storage item-list lifetime"
        );
    }
    Ok(())
}

pub(super) fn verify_catalog_font_page_routes(
    routine: &RuntimeRoutine,
    font_page_activation: u16,
    catalog_default_route: u8,
) -> Result<()> {
    for (kind, material_route, composite_state, expected) in [
        (
            KIND_ITEM,
            MATERIAL_ROUTE_CATALOG,
            CLASS_NAME_ONLY_COMPOSITE_STATE,
            Some(catalog_default_route),
        ),
        (
            KIND_SHOP_ITEM_LIST,
            MATERIAL_ROUTE_CATALOG,
            CLASS_NAME_ONLY_COMPOSITE_STATE,
            Some(catalog_default_route),
        ),
        (
            KIND_CLASS,
            MATERIAL_ROUTE_CATALOG,
            CLASS_NAME_ONLY_COMPOSITE_STATE,
            Some(catalog_default_route),
        ),
        (
            KIND_ITEM,
            MATERIAL_ROUTE_DIALOGUE,
            CLASS_NAME_ONLY_COMPOSITE_STATE,
            None,
        ),
        (
            KIND_SHOP_ITEM_LIST,
            MATERIAL_ROUTE_DIALOGUE,
            CLASS_NAME_ONLY_COMPOSITE_STATE,
            None,
        ),
        (
            KIND_UNIT_OR_ENEMY,
            MATERIAL_ROUTE_CATALOG,
            CLASS_NAME_ONLY_COMPOSITE_STATE,
            None,
        ),
        (KIND_CLASS, MATERIAL_ROUTE_CATALOG, 0x04, None),
    ] {
        ensure!(
            observe_catalog_font_page_activation(
                routine,
                font_page_activation,
                kind,
                material_route,
                composite_state,
            )? == expected,
            "consumer kind {kind} material route {material_route} state {composite_state:02X} selected the wrong font page"
        );
    }
    Ok(())
}

fn observe_catalog_font_page_activation(
    routine: &RuntimeRoutine,
    font_page_activation: u16,
    kind: u8,
    material_route: u8,
    composite_state: u8,
) -> Result<Option<u8>> {
    let predicate = [0xA5, 0x05, 0xC9, KIND_ITEM];
    let offsets = routine
        .bytes
        .windows(predicate.len())
        .enumerate()
        .filter_map(|(offset, bytes)| (bytes == predicate).then_some(offset))
        .collect::<Vec<_>>();
    ensure!(
        offsets.len() == 1,
        "consumer catalog emitted {} item font-page selectors",
        offsets.len()
    );

    let mut memory = Box::new([0_u8; 0x10000]);
    let start = usize::from(routine.address);
    let end = start
        .checked_add(routine.bytes.len())
        .context("consumer catalog routine range overflow")?;
    memory[start..end].copy_from_slice(&routine.bytes);
    memory[0x05] = kind;
    memory[0x03] = material_route;
    memory[usize::from(COMPOSITE_STATE)] = composite_state;

    let mut pc = routine.address
        + u16::try_from(offsets[0]).context("item font selector offset exceeds u16")?;
    let mut a = 0_u8;
    let mut zero = false;
    let mut activated = None;
    for _ in 0..32 {
        let opcode = memory[usize::from(pc)];
        pc = pc.wrapping_add(1);
        match opcode {
            0x20 => {
                let low = memory[usize::from(pc)];
                let high = memory[usize::from(pc.wrapping_add(1))];
                pc = pc.wrapping_add(2);
                let target = u16::from_le_bytes([low, high]);
                ensure!(
                    target == font_page_activation,
                    "item font route called unexpected subroutine {target:04X}"
                );
                activated = Some(a);
            }
            0x4C => {
                let low = memory[usize::from(pc)];
                let high = memory[usize::from(pc.wrapping_add(1))];
                pc = u16::from_le_bytes([low, high]);
            }
            0xA0 => {
                ensure!(
                    memory[usize::from(pc)] == 0,
                    "item font route did not rejoin at the material copy boundary"
                );
                return Ok(activated);
            }
            0xA5 => {
                let address = memory[usize::from(pc)];
                pc = pc.wrapping_add(1);
                a = memory[usize::from(address)];
                zero = a == 0;
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
            other => bail!("catalog font route reached unsupported opcode {other:02X}"),
        }
    }
    bail!("catalog font route did not reach the material copy boundary")
}
