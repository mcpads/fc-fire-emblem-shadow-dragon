use serde::Serialize;

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum PatternWindow {
    Left,
    Right,
}

impl PatternWindow {
    pub(crate) fn label(self) -> &'static str {
        match self {
            Self::Left => "ppu_0000",
            Self::Right => "ppu_1000",
        }
    }
}

#[derive(Debug, Clone, Copy)]
pub(crate) struct ObservedChrPair {
    pub(crate) screen_role: &'static str,
    pub(crate) pattern_window: PatternWindow,
    pub(crate) fd_source_page: u8,
    pub(crate) fe_source_page: u8,
}

impl ObservedChrPair {
    pub(crate) const fn new(
        screen_role: &'static str,
        pattern_window: PatternWindow,
        fd_source_page: u8,
        fe_source_page: u8,
    ) -> Self {
        Self {
            screen_role,
            pattern_window,
            fd_source_page,
            fe_source_page,
        }
    }
}

pub(crate) const OBSERVED_CHR_PAIRS: &[ObservedChrPair] = &[
    pair("title", PatternWindow::Left, 0x14, 0x14),
    pair("title", PatternWindow::Right, 0x00, 0x14),
    pair("new_game_choice", PatternWindow::Left, 0x1A, 0x1A),
    pair("new_game_choice", PatternWindow::Right, 0x00, 0x00),
    pair("save_slot_selection", PatternWindow::Left, 0x1A, 0x1A),
    pair("save_slot_selection", PatternWindow::Right, 0x00, 0x00),
    pair("intro_terrain", PatternWindow::Left, 0x1A, 0x1A),
    pair("intro_terrain", PatternWindow::Right, 0x15, 0x15),
    pair("class_profile", PatternWindow::Left, 0x14, 0x14),
    pair("class_profile", PatternWindow::Right, 0x00, 0x14),
    pair("intro_dialogue", PatternWindow::Left, 0x07, 0x07),
    pair("intro_dialogue", PatternWindow::Right, 0x00, 0x18),
    pair("game_over", PatternWindow::Left, 0x07, 0x07),
    pair("game_over", PatternWindow::Right, 0x00, 0x18),
    pair("later_intro_dialogue", PatternWindow::Left, 0x11, 0x11),
    pair("later_intro_dialogue", PatternWindow::Right, 0x00, 0x18),
    pair("map_idle", PatternWindow::Left, 0x1A, 0x1A),
    pair("map_idle", PatternWindow::Right, 0x15, 0x15),
    pair("map_idle", PatternWindow::Right, 0x18, 0x18),
    pair("map_idle", PatternWindow::Right, 0x19, 0x19),
    pair("unit_summary", PatternWindow::Left, 0x1A, 0x1A),
    pair("unit_summary", PatternWindow::Right, 0x00, 0x15),
    pair("unit_summary", PatternWindow::Right, 0x00, 0x18),
    pair("unit_summary", PatternWindow::Right, 0x00, 0x19),
    pair("unit_command_menu", PatternWindow::Left, 0x1A, 0x1A),
    pair("unit_command_menu", PatternWindow::Right, 0x00, 0x15),
    pair("unit_command_menu", PatternWindow::Right, 0x00, 0x18),
    pair("unit_command_menu", PatternWindow::Right, 0x00, 0x19),
    pair("unit_status", PatternWindow::Left, 0x13, 0x13),
    pair("unit_status", PatternWindow::Right, 0x00, 0x15),
    pair("unit_status", PatternWindow::Right, 0x00, 0x18),
    pair("unit_status", PatternWindow::Right, 0x00, 0x19),
    pair("item_inventory_list", PatternWindow::Left, 0x1A, 0x1A),
    pair("item_inventory_list", PatternWindow::Right, 0x00, 0x15),
    pair("item_action_menu", PatternWindow::Left, 0x1A, 0x1A),
    pair("item_action_menu", PatternWindow::Right, 0x00, 0x15),
    pair("item_equip_result", PatternWindow::Left, 0x1A, 0x1A),
    pair("item_equip_result", PatternWindow::Right, 0x00, 0x15),
    pair("item_use_result", PatternWindow::Left, 0x1A, 0x1A),
    pair("item_use_result", PatternWindow::Right, 0x00, 0x15),
    pair(
        "item_transfer_target_selection",
        PatternWindow::Left,
        0x1A,
        0x1A,
    ),
    pair(
        "item_transfer_target_selection",
        PatternWindow::Right,
        0x15,
        0x15,
    ),
    pair("map_menu", PatternWindow::Left, 0x1A, 0x1A),
    pair("map_menu", PatternWindow::Right, 0x00, 0x19),
    pair("options", PatternWindow::Left, 0x1A, 0x1A),
    pair("options", PatternWindow::Right, 0x00, 0x15),
    pair("unit_roster", PatternWindow::Left, 0x18, 0x18),
    pair("unit_roster", PatternWindow::Right, 0x00, 0x15),
    pair("unit_roster", PatternWindow::Right, 0x00, 0x18),
    pair("unit_roster", PatternWindow::Right, 0x00, 0x19),
    pair("battle_animation", PatternWindow::Left, 0x02, 0x06),
    pair("battle_animation", PatternWindow::Left, 0x06, 0x06),
    pair("battle_animation", PatternWindow::Right, 0x02, 0x06),
    pair(
        "chapter_clear_epilogue_dialogue",
        PatternWindow::Left,
        0x11,
        0x11,
    ),
    pair(
        "chapter_clear_epilogue_dialogue",
        PatternWindow::Right,
        0x00,
        0x18,
    ),
    pair("next_story_banner", PatternWindow::Left, 0x1B, 0x1B),
    pair("next_story_banner", PatternWindow::Right, 0x00, 0x18),
    pair("chapter_save_offer", PatternWindow::Left, 0x1B, 0x1B),
    pair("chapter_save_offer", PatternWindow::Right, 0x00, 0x18),
    pair(
        "chapter_save_complete_continue_prompt",
        PatternWindow::Left,
        0x1C,
        0x1C,
    ),
    pair(
        "chapter_save_complete_continue_prompt",
        PatternWindow::Right,
        0x00,
        0x18,
    ),
    pair(
        "chapter_save_complete_power_off_notice",
        PatternWindow::Left,
        0x1C,
        0x1C,
    ),
    pair(
        "chapter_save_complete_power_off_notice",
        PatternWindow::Right,
        0x00,
        0x18,
    ),
    pair("sound_test", PatternWindow::Left, 0x1C, 0x1C),
    pair("sound_test", PatternWindow::Right, 0x00, 0x18),
    pair(
        "ending_opening_and_cast_scroll",
        PatternWindow::Left,
        0x1C,
        0x1C,
    ),
    pair(
        "ending_opening_and_cast_scroll",
        PatternWindow::Right,
        0x00,
        0x00,
    ),
    pair(
        "ending_chapter_record_scroll",
        PatternWindow::Left,
        0x1C,
        0x1C,
    ),
    pair(
        "ending_chapter_record_scroll",
        PatternWindow::Right,
        0x00,
        0x00,
    ),
    pair("ending_staff_credits", PatternWindow::Left, 0x1C, 0x1C),
    pair("ending_staff_credits", PatternWindow::Right, 0x00, 0x00),
    pair("ending_character_epilogue", PatternWindow::Left, 0x04, 0x04),
    pair("ending_character_epilogue", PatternWindow::Left, 0x07, 0x07),
    pair("ending_character_epilogue", PatternWindow::Left, 0x0A, 0x0A),
    pair("ending_character_epilogue", PatternWindow::Left, 0x0B, 0x0B),
    pair("ending_character_epilogue", PatternWindow::Left, 0x0F, 0x0F),
    pair("ending_character_epilogue", PatternWindow::Left, 0x10, 0x10),
    pair("ending_character_epilogue", PatternWindow::Left, 0x11, 0x11),
    pair("ending_character_epilogue", PatternWindow::Left, 0x13, 0x13),
    pair("ending_character_epilogue", PatternWindow::Left, 0x17, 0x17),
    pair("ending_character_epilogue", PatternWindow::Left, 0x1A, 0x1A),
    pair("ending_character_epilogue", PatternWindow::Left, 0x1C, 0x1C),
    pair("ending_character_epilogue", PatternWindow::Left, 0x1D, 0x1D),
    pair("ending_character_epilogue", PatternWindow::Left, 0x1F, 0x1F),
    pair(
        "ending_character_epilogue",
        PatternWindow::Right,
        0x00,
        0x00,
    ),
    pair("ending_final_signature", PatternWindow::Left, 0x1C, 0x1C),
    pair("ending_final_signature", PatternWindow::Right, 0x18, 0x00),
    pair(
        "chapter_transition_blackout",
        PatternWindow::Left,
        0x1A,
        0x1A,
    ),
    pair(
        "chapter_transition_blackout",
        PatternWindow::Left,
        0x1B,
        0x1B,
    ),
    pair(
        "chapter_transition_blackout",
        PatternWindow::Right,
        0x18,
        0x18,
    ),
    pair(
        "chapter_intro_title_dialogue_composite",
        PatternWindow::Left,
        0x13,
        0x13,
    ),
    pair(
        "chapter_intro_title_dialogue_composite",
        PatternWindow::Right,
        0x00,
        0x18,
    ),
    pair(
        "chapter_intro_title_dialogue_composite",
        PatternWindow::Left,
        0x0F,
        0x0F,
    ),
    pair(
        "chapter_intro_title_dialogue_composite",
        PatternWindow::Left,
        0x1A,
        0x1A,
    ),
    pair("weapon_shop_item_list", PatternWindow::Left, 0x1E, 0x1E),
    pair("weapon_shop_item_list", PatternWindow::Right, 0x00, 0x15),
    pair(
        "weapon_shop_purchase_confirmation",
        PatternWindow::Left,
        0x1E,
        0x1E,
    ),
    pair(
        "weapon_shop_purchase_confirmation",
        PatternWindow::Right,
        0x00,
        0x15,
    ),
    pair(
        "weapon_shop_purchase_result",
        PatternWindow::Left,
        0x1E,
        0x1E,
    ),
    pair(
        "weapon_shop_purchase_result",
        PatternWindow::Right,
        0x00,
        0x15,
    ),
    pair("weapon_shop_exit_message", PatternWindow::Left, 0x1E, 0x1E),
    pair("weapon_shop_exit_message", PatternWindow::Right, 0x00, 0x15),
    pair(
        "weapon_shop_inventory_full_message",
        PatternWindow::Left,
        0x1E,
        0x1E,
    ),
    pair(
        "weapon_shop_inventory_full_message",
        PatternWindow::Right,
        0x00,
        0x15,
    ),
    pair(
        "weapon_shop_insufficient_funds_message",
        PatternWindow::Left,
        0x1E,
        0x1E,
    ),
    pair(
        "weapon_shop_insufficient_funds_message",
        PatternWindow::Right,
        0x00,
        0x15,
    ),
    pair(
        "weapon_shop_item_restriction_confirmation",
        PatternWindow::Left,
        0x1E,
        0x1E,
    ),
    pair(
        "weapon_shop_item_restriction_confirmation",
        PatternWindow::Right,
        0x00,
        0x15,
    ),
    pair(
        "weapon_shop_declined_continue_prompt",
        PatternWindow::Left,
        0x1E,
        0x1E,
    ),
    pair(
        "weapon_shop_declined_continue_prompt",
        PatternWindow::Right,
        0x00,
        0x15,
    ),
    pair(
        "weapon_shop_purchase_inventory_full_exit",
        PatternWindow::Left,
        0x1E,
        0x1E,
    ),
    pair(
        "weapon_shop_purchase_inventory_full_exit",
        PatternWindow::Right,
        0x00,
        0x15,
    ),
];

const fn pair(
    screen_role: &'static str,
    pattern_window: PatternWindow,
    fd_source_page: u8,
    fe_source_page: u8,
) -> ObservedChrPair {
    ObservedChrPair::new(screen_role, pattern_window, fd_source_page, fe_source_page)
}
