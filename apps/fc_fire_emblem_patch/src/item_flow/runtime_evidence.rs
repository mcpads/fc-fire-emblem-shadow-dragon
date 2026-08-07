use serde::Serialize;

#[derive(Debug, Serialize)]
pub(super) struct RuntimeObservation {
    screen_role: &'static str,
    main_state: u8,
    main_state_hex: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    left_chr_pair: Option<&'static str>,
    #[serde(skip_serializing_if = "Option::is_none")]
    right_chr_pair: Option<&'static str>,
    screenshot_phase_sha256: &'static [&'static str],
    temporal_observation: &'static str,
    #[serde(skip_serializing_if = "Option::is_none")]
    menu: Option<MenuEvidence>,
    #[serde(skip_serializing_if = "Option::is_none")]
    dialogue: Option<DialogueEvidence>,
    #[serde(skip_serializing_if = "Option::is_none")]
    variants: Option<&'static [RuntimeVariant]>,
    inventory: InventoryEvidence,
}

#[derive(Debug, Serialize)]
struct MenuEvidence {
    controller_index: u8,
    effective_choice_mask_address: u16,
    effective_choice_mask_address_hex: String,
    effective_selection_address: u16,
    effective_selection_address_hex: String,
    choice_mask: u8,
    choice_mask_hex: String,
}

#[derive(Debug, Serialize)]
struct DialogueEvidence {
    result_index: u8,
    result_index_hex: String,
    return_main_state: u8,
    return_main_state_hex: String,
}

#[derive(Debug, Serialize)]
struct InventoryEvidence {
    source_items: &'static [&'static str],
    source_record_before: &'static str,
    source_record_after: &'static str,
    #[serde(skip_serializing_if = "Option::is_none")]
    target_record_after: Option<&'static str>,
    mutation_observed: bool,
}

#[derive(Debug, Serialize)]
struct RuntimeVariant {
    role: &'static str,
    setup: &'static str,
    result_code: u8,
    result_code_hex: &'static str,
    source_record_before: &'static str,
    source_record_after: &'static str,
    observed_effect: &'static str,
}

pub(super) fn runtime_observations() -> Vec<RuntimeObservation> {
    const SOURCE_BEFORE: &str = "05010112120030060706070207090400081800020f00002a160000";
    const SOURCE_EQUIPPED_SECOND: &str = "050101121200300607060702070904000818000f020000162a0000";
    const SOURCE_WITH_FIRST_ITEM_REMOVED: &str =
        "050101121200300607060702070904000818000f00000016000000";
    const TRANSFER_TARGET_AFTER: &str = "040101121200300705050603070904000817000c020200262a2a00";
    const USE_SUCCESS_BEFORE: &str = "05010111120030060706070207090400081800020f40002a160500";
    const USE_SUCCESS_AFTER: &str = "05010112120030060706070207090400081800020f40002a160400";
    const USE_FULL_HP: &str = "05010112120030060706070207090400081800020f40002a160500";
    const USE_EXHAUSTED_BEFORE: &str = "05010111120030060706070207090400081800020f40002a160100";
    const USE_EXHAUSTED_AFTER: &str = "05010112120030060706070207090400081800020f00002a160000";

    vec![
        RuntimeObservation {
            screen_role: "item_inventory_list",
            main_state: 0x1B,
            main_state_hex: "0x1B".to_owned(),
            left_chr_pair: Some("1A/1A"),
            right_chr_pair: Some("00/15"),
            screenshot_phase_sha256: &[
                "d2cd864619c55a6128fc3611ef2991d8f451c606a7604b6ae434379d8aa3f3f3",
                "ab447c688ce3d79f6430ae48d3731d36d9e36a81318b44155c8897ec4fe09e11",
                "0444f7efdd0ee4664f8c669278d2be4e8a6ed79b0f81f256c46abebb088298cc",
            ],
            temporal_observation: "152 regular plus 168 irregularly spaced input-free frames kept both item rows and CHR fixed while cursor and map sprites cycled through three screenshot phases",
            menu: Some(menu(1, 0x7FEE, 0x7FF3, 0x03)),
            dialogue: None,
            variants: None,
            inventory: inventory(
                &[
                    "item 02 / durability 2A / displayed てつのつるぎ 42",
                    "item 0F / durability 16 / displayed てやり 22",
                ],
                SOURCE_BEFORE,
                SOURCE_BEFORE,
                None,
            ),
        },
        RuntimeObservation {
            screen_role: "item_action_menu",
            main_state: 0x1C,
            main_state_hex: "0x1C".to_owned(),
            left_chr_pair: Some("1A/1A"),
            right_chr_pair: Some("00/15"),
            screenshot_phase_sha256: &[
                "b14877d722366f39faa1c5a265babfe508b1fc97019a90dd64102ad12be8262f",
                "aa0d289b4a62ef84e61ef537eab263965ba6ab841c696eacb2d1fcd9f39456a1",
                "02ad14ba7245db9a03852ea4be0180f1122f9076fb14720fde8b85c22737a8ef",
            ],
            temporal_observation: "for item 02, 152 regular plus 168 irregularly spaced input-free frames kept the three available action rows and CHR fixed while cursor and map sprites cycled through three screenshot phases",
            menu: Some(menu(2, 0x7FEF, 0x7FF4, 0x0D)),
            dialogue: None,
            variants: None,
            inventory: inventory(
                &[
                    "selected item 02 at record offset 13",
                    "normalized action mask 0D selects action codes 0, 2, and 3",
                ],
                SOURCE_BEFORE,
                SOURCE_BEFORE,
                None,
            ),
        },
        RuntimeObservation {
            screen_role: "item_equip_result",
            main_state: 0x1E,
            main_state_hex: "0x1E".to_owned(),
            left_chr_pair: Some("1A/1A"),
            right_chr_pair: Some("00/15"),
            screenshot_phase_sha256: &[
                "d557b987a728692f7aaf5f6439481169b947e2705f82a2c258a26416cda711a8",
                "6d927385e5f4b58089a82e31b55727ec7d7c319448b4b7fb1fa7cd9470dc991e",
                "51626f178bd3d4dbc4a951a063605f4e8c373ee0f82600cdc848c2cc071ba15d",
                "837bc93026211ef93cf8f17ee404e9e33121f55349133e40c9b506f7c77118c3",
            ],
            temporal_observation: "the result message completed by 77 frames and remained in input wait through 227 frames; a second-slot branch separately proved the item and durability swap",
            menu: None,
            dialogue: Some(dialogue(0x19, 0x19)),
            variants: None,
            inventory: inventory(
                &["selected slot 14 became the equipped first slot together with its durability"],
                SOURCE_BEFORE,
                SOURCE_EQUIPPED_SECOND,
                None,
            ),
        },
        RuntimeObservation {
            screen_role: "item_use_result",
            main_state: 0x1E,
            main_state_hex: "0x1E".to_owned(),
            left_chr_pair: Some("1A/1A"),
            right_chr_pair: Some("00/15"),
            screenshot_phase_sha256: &[
                "7f3f1bf053706eed54a72c5af87286e3e44d321921550e6756aa97b3ccb3fd4a",
                "b750f48e6b60723e31f13865545f21ec6a351800eecdef50d567858c9a923879",
                "63bec5b19fa3a11d2f14534249dadce08720a28a866b0962e342ca192dad0f64",
            ],
            temporal_observation: "action code 1 automatically drew the initial use text, ran the item-family effect at progression 2, and settled at progression 3; 47 additional input-free frames kept the completed result and CHR fixed until A dismissed it and completed the unit action",
            menu: None,
            dialogue: Some(dialogue(0x1A, 0x19)),
            variants: Some(&[
                RuntimeVariant {
                    role: "positive_heal",
                    setup: "item 40 with durability 05 at HP 17/18",
                    result_code: 0x1D,
                    result_code_hex: "0x1D",
                    source_record_before: USE_SUCCESS_BEFORE,
                    source_record_after: USE_SUCCESS_AFTER,
                    observed_effect: "HP increased by 1 and durability decreased from 5 to 4 in the same effect frame",
                },
                RuntimeVariant {
                    role: "full_hp_no_effect",
                    setup: "item 40 with durability 05 at HP 18/18",
                    result_code: 0x30,
                    result_code_hex: "0x30",
                    source_record_before: USE_FULL_HP,
                    source_record_after: USE_FULL_HP,
                    observed_effect: "the no-effect result preserved HP, item, and durability",
                },
                RuntimeVariant {
                    role: "exhausted_use",
                    setup: "item 40 with durability 01 at HP 17/18",
                    result_code: 0x1D,
                    result_code_hex: "0x1D",
                    source_record_before: USE_EXHAUSTED_BEFORE,
                    source_record_after: USE_EXHAUSTED_AFTER,
                    observed_effect: "HP increased by 1, durability reached zero, and the selected item slot was cleared",
                },
            ]),
            inventory: inventory(
                &[
                    "selected item 40 at record offset 15",
                    "the reversible reachability setup does not establish natural acquisition provenance",
                ],
                USE_SUCCESS_BEFORE,
                USE_SUCCESS_AFTER,
                None,
            ),
        },
        RuntimeObservation {
            screen_role: "item_transfer_target_selection",
            main_state: 0x1D,
            main_state_hex: "0x1D".to_owned(),
            left_chr_pair: Some("1A/1A"),
            right_chr_pair: Some("15/15"),
            screenshot_phase_sha256: &[
                "6970facad14608f9c5043e64b81dc5c03d8072956c106313b8120ce178ec99cf",
                "806833414d56c2ba2f8cfab8d3d37f95c2b87ce233b997b03a4bee5873e644e6",
            ],
            temporal_observation: "211 irregularly spaced input-free frames showed a textless map overlay whose active candidate sprite or marker can be absent in one capture phase",
            menu: None,
            dialogue: None,
            variants: None,
            inventory: inventory(
                &[
                    "B returned to item_inventory_list at state 1B without changing the source record",
                ],
                SOURCE_BEFORE,
                SOURCE_BEFORE,
                None,
            ),
        },
        RuntimeObservation {
            screen_role: "item_transfer_result",
            main_state: 0x1E,
            main_state_hex: "0x1E".to_owned(),
            left_chr_pair: None,
            right_chr_pair: None,
            screenshot_phase_sha256: &[
                "dc312131adb2d12e6788571a4e4a764168fcc53e4d7910b821b2239dcb82a474",
                "fee5ea5770d770d995916e4d9036cfc8e4499fdc5bb438010f9a43a7f81f3407",
            ],
            temporal_observation: "the completed transfer result remained in input wait for 218 additional irregularly spaced frames while map sprites animated",
            menu: None,
            dialogue: Some(dialogue(0x1B, 0x1A)),
            variants: None,
            inventory: inventory(
                &[
                    "item 02 and durability 2A moved from the source first slot to the selected target empty slot",
                ],
                SOURCE_BEFORE,
                SOURCE_WITH_FIRST_ITEM_REMOVED,
                Some(TRANSFER_TARGET_AFTER),
            ),
        },
        RuntimeObservation {
            screen_role: "item_discard_result",
            main_state: 0x1E,
            main_state_hex: "0x1E".to_owned(),
            left_chr_pair: None,
            right_chr_pair: None,
            screenshot_phase_sha256: &[
                "8f7105f1a786f5684916462382fceb096bfd95a835d5959f00e3075979bde340",
                "f83ae95cd8c5d0d2573845a34a24e6e52ef80c76c1c3a3b69ded1bd8d937ac09",
                "90a02763e7f89ad025420407a9bb8c1e35d14d7ffe8286ed7f36f76d6ba65cb8",
            ],
            temporal_observation: "the completed discard result remained in input wait for 200 additional irregularly spaced frames while map sprites animated",
            menu: None,
            dialogue: Some(dialogue(0x1C, 0x1A)),
            variants: None,
            inventory: inventory(
                &[
                    "the selected first slot was cleared and the remaining item and durability were compacted forward",
                ],
                SOURCE_BEFORE,
                SOURCE_WITH_FIRST_ITEM_REMOVED,
                None,
            ),
        },
    ]
}

fn menu(
    controller_index: u8,
    mask_address: u16,
    selection_address: u16,
    choice_mask: u8,
) -> MenuEvidence {
    MenuEvidence {
        controller_index,
        effective_choice_mask_address: mask_address,
        effective_choice_mask_address_hex: format!("0x{mask_address:04X}"),
        effective_selection_address: selection_address,
        effective_selection_address_hex: format!("0x{selection_address:04X}"),
        choice_mask,
        choice_mask_hex: format!("0x{choice_mask:02X}"),
    }
}

fn dialogue(result_index: u8, return_main_state: u8) -> DialogueEvidence {
    DialogueEvidence {
        result_index,
        result_index_hex: format!("0x{result_index:02X}"),
        return_main_state,
        return_main_state_hex: format!("0x{return_main_state:02X}"),
    }
}

fn inventory(
    source_items: &'static [&'static str],
    source_record_before: &'static str,
    source_record_after: &'static str,
    target_record_after: Option<&'static str>,
) -> InventoryEvidence {
    InventoryEvidence {
        source_items,
        source_record_before,
        source_record_after,
        target_record_after,
        mutation_observed: source_record_before != source_record_after
            || target_record_after.is_some(),
    }
}
