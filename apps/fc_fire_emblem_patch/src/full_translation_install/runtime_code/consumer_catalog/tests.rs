use super::shop_item_route::{
    ItemMaterialRoute, StorageItemConsumer, select_shop_item_material, select_storage_item_material,
};
use super::*;
use crate::dialogue_runtime_state::MAIN_DIALOGUE_RUNTIME_STATE;

fn execute_until_kind_validation(
    routine: &RuntimeRoutine,
    kind: u8,
    original_y: u8,
    output_x: u8,
    initial_sp: u8,
    scratch: [u8; 6],
) -> (u8, u8, u8) {
    let mut memory = Box::new([0_u8; 0x10000]);
    let start = usize::from(routine.address);
    memory[start..start + routine.bytes.len()].copy_from_slice(&routine.bytes);
    memory[..scratch.len()].copy_from_slice(&scratch);
    let mut pc = routine.address;
    let mut a = kind;
    let mut x = output_x;
    let mut y = original_y;
    let mut sp = initial_sp;
    for _ in 0..64 {
        let opcode = memory[usize::from(pc)];
        pc = pc.wrapping_add(1);
        match opcode {
            0x48 => {
                memory[0x0100 + usize::from(sp)] = a;
                sp = sp.wrapping_sub(1);
            }
            0x85 => {
                let address = memory[usize::from(pc)];
                pc = pc.wrapping_add(1);
                memory[usize::from(address)] = a;
            }
            0x98 => a = y,
            0x8A => a = x,
            0xA5 => {
                let address = memory[usize::from(pc)];
                pc = pc.wrapping_add(1);
                a = memory[usize::from(address)];
            }
            0xA8 => y = a,
            0xAA => x = a,
            0xBA => x = sp,
            0xBD => {
                let low = memory[usize::from(pc)];
                let high = memory[usize::from(pc.wrapping_add(1))];
                pc = pc.wrapping_add(2);
                let address = u16::from_le_bytes([low, high]).wrapping_add(u16::from(x));
                a = memory[usize::from(address)];
            }
            0xC9 => {
                let value = memory[usize::from(pc)];
                assert_eq!(value, KIND_COUNT);
                return (a, x, y);
            }
            other => panic!("catalog call-frame setup reached unsupported opcode {other:02X}"),
        }
    }
    panic!("catalog call-frame setup did not reach kind validation")
}

fn layout() -> ConsumerCatalogRuntimeLayout {
    ConsumerCatalogRuntimeLayout {
        material_page: 0x32,
        material_base: 0x8000,
        item_directory: 0x8010,
        class_directory: 0x80C6,
        unit_directory: 0x80F2,
        enemy_directory: 0x815C,
    }
}

fn shop_item_residency() -> ShopItemResidencyRuntimeContract {
    ShopItemResidencyRuntimeContract {
        outer_state_address: MAIN_DIALOGUE_RUNTIME_STATE.map_dialogue_outer_state_address,
        composition_state: 0x03,
        composite_state: 0x15,
        selected_facility_address: 0x77D0,
        dialogue_directory_address: MAIN_DIALOGUE_RUNTIME_STATE.directory_selector_address,
        dialogue_directory_selector: 0xB1,
        e7_caller_resume_flag_address: MAIN_DIALOGUE_RUNTIME_STATE.caller_handoff_flag_address,
        selling_facilities: [0x01, 0x02, 0x05],
        non_selling_facilities: [0x03, 0x04],
        dialogue_material_page: 0x31,
        dialogue_material_base: 0x8000,
        dialogue_item_directory: 0x8110,
        catalog_material_page: layout().material_page,
        catalog_material_base: layout().material_base,
        catalog_item_directory: layout().item_directory,
    }
}

fn storage_item_list() -> StorageItemListRuntimeRoute {
    use crate::full_translation_install::storage_residency::StorageItemListRuntimeContext;

    StorageItemListRuntimeRoute {
        caller_state_address: MAIN_DIALOGUE_RUNTIME_STATE.map_dialogue_outer_state_address,
        deposit: StorageItemListRuntimeContext {
            composite_state:
                crate::full_translation_install::screen_font_residency::UNIT_ITEM_LIST_COMPOSITE_STATE,
            caller_state: 0x06,
        },
        withdraw: StorageItemListRuntimeContext {
            composite_state:
                crate::full_translation_install::screen_font_residency::ITEM_USE_RESULT_COMPOSITE_STATE,
            caller_state: 0x0A,
        },
        overflow: StorageItemListRuntimeContext {
            composite_state:
                crate::full_translation_install::screen_font_residency::STORAGE_ITEM_DETAIL_COMPOSITE_STATE,
            caller_state: 0x06,
        },
    }
}

#[test]
fn three_five_byte_stubs_fill_the_remaining_producer_cave() {
    let runtime = build_consumer_catalog_runtime(
        0xA600,
        0x30,
        0xF7F8,
        0xF620,
        0xDC,
        0xDD,
        layout(),
        shop_item_residency(),
        storage_item_list(),
    )
    .unwrap();

    assert_eq!(runtime.fixed_routines.len(), 4);
    assert!(
        runtime.fixed_routines[..3]
            .iter()
            .all(|routine| routine.bytes.len() == 5)
    );
    assert_eq!(
        routine_end(&runtime.fixed_routines[2]).unwrap(),
        ENTRY_STUB_CAVE_END
    );
}

#[test]
fn fixed_bridge_fits_the_independent_forty_five_byte_cave() {
    let bytes = build_fixed_bridge(FIXED_BRIDGE_ORIGIN, 0xA600, 0x30).unwrap();

    assert!(usize::from(FIXED_BRIDGE_ORIGIN) + bytes.len() <= usize::from(FIXED_BRIDGE_END));
    assert_eq!(bytes.last(), Some(&0x60));
}

#[test]
fn fixed_bridge_restores_output_x_original_y_and_catalog_kind_in_order() {
    let bytes = build_fixed_bridge(FIXED_BRIDGE_ORIGIN, 0xA600, 0x30).unwrap();

    assert!(bytes.starts_with(&[0x48, 0x98, 0x48, 0x8A, 0x48]));
    assert!(
        bytes
            .windows(8)
            .any(|window| window == [0x68, 0xAA, 0x68, 0xA8, 0x68, 0x20, 0x00, 0xA6])
    );
    assert!(!bytes.ends_with(&[0xA9, SEGMENT_SEPARATOR, 0x60]));
    assert_eq!(
        bytes.len(),
        usize::from(FIXED_BRIDGE_END - FIXED_BRIDGE_ORIGIN)
    );
}

#[test]
fn appender_uses_the_callers_output_position_after_reading_its_stack_frame() {
    let runtime = build_consumer_catalog_runtime(
        0xA600,
        0x30,
        0xF7F8,
        0xF620,
        0xDC,
        0xDD,
        layout(),
        shop_item_residency(),
        storage_item_list(),
    )
    .unwrap();

    for (kind, original_y, output_x, initial_sp) in [(0, 0x7E, 0x00, 0xF1), (3, 0x54, 0x6F, 0xB7)] {
        assert_eq!(
            execute_until_kind_validation(
                &runtime.code_routine,
                kind,
                original_y,
                output_x,
                initial_sp,
                [0x11, 0x22, 0x33, 0x44, 0x55, 0x66],
            ),
            (kind, output_x, original_y)
        );
    }
}

#[test]
fn catalog_calls_and_shop_consumer_hook_have_distinct_owned_extents() {
    let runtime = build_consumer_catalog_runtime(
        0xA600,
        0x30,
        0xF7F8,
        0xF620,
        0xDC,
        0xDD,
        layout(),
        shop_item_residency(),
        storage_item_list(),
    )
    .unwrap();

    assert_eq!(runtime.hooks.len(), HOOK_SITES.len() + 3);
    assert!(
        runtime.hooks[..HOOK_SITES.len()]
            .iter()
            .all(|hook| hook.bytes.len() == 3)
    );
    assert_eq!(runtime.hooks[0].bytes[0], 0x20);
    assert_eq!(runtime.hooks[1].bytes[0], 0x4C);
    assert_eq!(runtime.hooks[2].bytes[0], 0x4C);
    for (hook, address) in runtime.hooks[..HOOK_SITES.len()].iter().zip([
        0x875F,
        UNIT_NAME_APPENDER_ENTRY,
        CLASS_NAME_APPENDER_ENTRY,
    ]) {
        assert!(matches!(
            hook.site,
            DialogueRuntimeHookSite::Switchable {
                bank: UNIT_UI_BANK,
                address: actual,
            } if actual == address
        ));
    }
    assert_eq!(
        runtime.hooks[HOOK_SITES.len()].role,
        DialogueRuntimeHookRole::ConsumerCatalogDirectItemEntry
    );
    assert_eq!(
        runtime.hooks[HOOK_SITES.len() + 1].role,
        DialogueRuntimeHookRole::ConsumerCatalogDirectItemNormalizer
    );
    let shop = runtime.hooks.last().unwrap();
    assert_eq!(shop.role, DialogueRuntimeHookRole::ShopItemListAppender);
    assert_eq!(shop.bytes.len(), 10);
}

#[test]
fn all_three_item_selling_facilities_use_the_dialogue_item_encoding() {
    let runtime = build_consumer_catalog_runtime(
        0xA600,
        0x30,
        0xF7F8,
        0xF620,
        0xDC,
        0xDD,
        layout(),
        shop_item_residency(),
        storage_item_list(),
    )
    .unwrap();

    verify_shop_item_residency_route(&runtime.code_routine, shop_item_residency()).unwrap();
}

#[test]
fn every_storage_item_list_context_uses_dialogue_material() {
    let runtime = build_consumer_catalog_runtime(
        0xA600,
        0x30,
        0xF7F8,
        0xF620,
        0xDC,
        0xDD,
        layout(),
        shop_item_residency(),
        storage_item_list(),
    )
    .unwrap();
    verify_storage_item_residency_route(
        &runtime.code_routine,
        shop_item_residency(),
        storage_item_list(),
    )
    .unwrap();

    let expected = ItemMaterialRoute {
        directory: shop_item_residency().dialogue_item_directory,
        material_page: shop_item_residency().dialogue_material_page,
        material_base: shop_item_residency().dialogue_material_base,
    };
    for (consumer, context) in [
        (
            StorageItemConsumer::RecordAppender,
            storage_item_list().deposit,
        ),
        (
            StorageItemConsumer::DirectOrListAppender,
            storage_item_list().withdraw,
        ),
        (
            StorageItemConsumer::DirectOrListAppender,
            storage_item_list().overflow,
        ),
    ] {
        assert_eq!(
            select_storage_item_material(
                &runtime.code_routine,
                shop_item_residency(),
                storage_item_list(),
                consumer,
                context.composite_state,
                context.caller_state,
            )
            .unwrap(),
            expected
        );
    }
}

#[test]
fn ordinary_item_use_result_does_not_inherit_storage_dialogue_material() {
    let runtime = build_consumer_catalog_runtime(
        0xA600,
        0x30,
        0xF7F8,
        0xF620,
        0xDC,
        0xDD,
        layout(),
        shop_item_residency(),
        storage_item_list(),
    )
    .unwrap();
    let storage = storage_item_list();

    assert_eq!(
        select_storage_item_material(
            &runtime.code_routine,
            shop_item_residency(),
            storage,
            StorageItemConsumer::DirectOrListAppender,
            storage.withdraw.composite_state,
            0x03,
        )
        .unwrap(),
        ItemMaterialRoute {
            directory: layout().item_directory,
            material_page: layout().material_page,
            material_base: layout().material_base,
        }
    );
}

#[test]
fn nonshop_or_inactive_e7_lifetimes_keep_the_catalog_item_encoding() {
    let runtime = build_consumer_catalog_runtime(
        0xA600,
        0x30,
        0xF7F8,
        0xF620,
        0xDC,
        0xDD,
        layout(),
        shop_item_residency(),
        storage_item_list(),
    )
    .unwrap();
    let expected = ItemMaterialRoute {
        directory: layout().item_directory,
        material_page: layout().material_page,
        material_base: layout().material_base,
    };
    let contract = shop_item_residency();

    for state in [
        (
            contract.composition_state.wrapping_add(1),
            contract.selling_facilities[0],
            contract.dialogue_directory_selector,
            1,
        ),
        (
            contract.composition_state,
            contract.non_selling_facilities[0],
            contract.dialogue_directory_selector,
            1,
        ),
        (
            contract.composition_state,
            contract.non_selling_facilities[1],
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
        assert_eq!(
            select_shop_item_material(
                &runtime.code_routine,
                shop_item_residency(),
                state.0,
                state.1,
                state.2,
                state.3,
            )
            .unwrap(),
            expected
        );
    }
}

#[test]
fn record_action_name_uses_the_planned_screen_route_before_shared_activation() {
    let activation = 0xF620;
    let origin = 0xA600;
    let record_action_route = 0xDD;
    let runtime = build_consumer_catalog_runtime(
        origin,
        0x30,
        0xF7F8,
        activation,
        0xDC,
        record_action_route,
        layout(),
        shop_item_residency(),
        storage_item_list(),
    )
    .unwrap();
    let prefix = [
        0xB1,
        0x00,
        0x48,
        0xAD,
        COMPOSITE_STATE as u8,
        (COMPOSITE_STATE >> 8) as u8,
        0xC9,
        RECORD_ACTION_COMPOSITE_STATE,
        0xD0,
        0x03,
    ];
    let offset = runtime
        .code_routine
        .bytes
        .windows(prefix.len())
        .position(|window| window == prefix)
        .expect("unit/enemy page prefix did not test the record-action lifetime");
    let sequence = &runtime.code_routine.bytes[offset..offset + 24];
    let sequence_address = origin + u16::try_from(offset).unwrap();

    assert_eq!(
        u16::from_le_bytes([sequence[11], sequence[12]]),
        sequence_address + 17
    );
    assert_eq!(sequence[13], 0x68);
    assert_eq!(
        u16::from_le_bytes([sequence[15], sequence[16]]),
        sequence_address + 20
    );
    assert_eq!(&sequence[17..20], &[0x68, 0xA9, record_action_route]);
    assert_eq!(
        &sequence[20..24],
        &[0x20, activation as u8, (activation >> 8) as u8, 0xC8,]
    );
}
