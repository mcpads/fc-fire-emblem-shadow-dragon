pub(super) const SCREEN_ROLES: [&str; 9] = [
    "weapon_shop_item_list",
    "weapon_shop_purchase_confirmation",
    "weapon_shop_purchase_result",
    "weapon_shop_exit_message",
    "weapon_shop_inventory_full_message",
    "weapon_shop_insufficient_funds_message",
    "weapon_shop_item_restriction_confirmation",
    "weapon_shop_declined_continue_prompt",
    "weapon_shop_purchase_inventory_full_exit",
];

pub(super) const DIALOGUE_SCREEN_ROLES: [&str; 9] = SCREEN_ROLES;

pub(super) const ITEM_NAME_SCREEN_ROLES: [&str; 8] = [
    "weapon_shop_item_list",
    "weapon_shop_purchase_confirmation",
    "weapon_shop_purchase_result",
    "weapon_shop_exit_message",
    "weapon_shop_inventory_full_message",
    "weapon_shop_item_restriction_confirmation",
    "weapon_shop_declined_continue_prompt",
    "weapon_shop_purchase_inventory_full_exit",
];

pub(super) const CHOICE_LABEL_SCREEN_ROLES: [&str; 5] = [
    "weapon_shop_purchase_confirmation",
    "weapon_shop_purchase_result",
    "weapon_shop_insufficient_funds_message",
    "weapon_shop_item_restriction_confirmation",
    "weapon_shop_declined_continue_prompt",
];

pub(super) const DECLINE_ROUTE_DIALOGUE_RUNTIME_SCREEN_ROLES: [&str; 4] = [
    "weapon_shop_item_list",
    "weapon_shop_purchase_confirmation",
    "weapon_shop_declined_continue_prompt",
    "weapon_shop_exit_message",
];
pub(super) const DECLINE_ROUTE_ITEM_NAME_RUNTIME_SCREEN_ROLES: [&str; 4] =
    DECLINE_ROUTE_DIALOGUE_RUNTIME_SCREEN_ROLES;
pub(super) const DECLINE_ROUTE_CHOICE_LABEL_RUNTIME_SCREEN_ROLES: [&str; 1] =
    ["weapon_shop_purchase_confirmation"];
