use super::*;

#[derive(Debug, Serialize)]
pub(super) struct ShopFlowReport {
    pub(super) schema: u8,
    pub(super) source_sha1: &'static str,
    pub(super) scope: Scope,
    pub(super) route: ShopRoute,
    pub(super) screens: Vec<ShopScreen>,
    pub(super) preflight_branches: Vec<PreflightBranch>,
    pub(super) purchase_mutation: PurchaseMutation,
    pub(super) runtime_e7_handoff_observation: RuntimeE7HandoffObservation,
    pub(super) runtime_purchase_observation: RuntimePurchaseObservation,
    pub(super) runtime_exit_observation: RuntimeExitObservation,
    pub(super) runtime_inventory_full_observation: RuntimeInventoryFullObservation,
    pub(super) runtime_insufficient_funds_observation: RuntimeInsufficientFundsObservation,
    pub(super) runtime_item_restriction_observation: RuntimeItemRestrictionObservation,
    pub(super) dialogue_table: ShopDialogueTableBinding,
    pub(super) source_regions: Vec<SourceRegionBinding>,
    pub(super) unresolved_downstream_roles: Vec<&'static str>,
    pub(super) release_eligible: bool,
}

#[derive(Debug, Serialize)]
pub(super) struct RuntimeE7HandoffObservation {
    pub(super) source_screen_role: &'static str,
    pub(super) input: &'static str,
    pub(super) source_outer_state: u8,
    pub(super) handoff_outer_state: u8,
    pub(super) settled_outer_state: u8,
    pub(super) caller_flag_address: u16,
    pub(super) caller_flag_address_hex: &'static str,
    pub(super) caller_flag_value: u8,
    pub(super) observer_prg_bank: u8,
    pub(super) observer_prg_bank_hex: &'static str,
    pub(super) observer_read_cpu_address: u16,
    pub(super) observer_read_cpu_address_hex: &'static str,
    pub(super) chr_pair_at_handoff: ChrPair,
    pub(super) item_list_screenshot_sha256: &'static str,
    pub(super) handoff_screenshot_sha256: &'static str,
    pub(super) settled_screenshot_sha256: &'static str,
    pub(super) item_list_nametable_sha256: &'static str,
    pub(super) handoff_nametable_sha256: &'static str,
    pub(super) settled_nametable_sha256: &'static str,
    pub(super) item_list_to_handoff_changed_byte_count: usize,
    pub(super) handoff_to_settled_changed_byte_count: usize,
    pub(super) retained_visible_content: &'static [&'static str],
    pub(super) page_lifetime_requirement: &'static str,
}

#[derive(Debug, Serialize)]
pub(super) struct Scope {
    pub(super) translation_direction: &'static str,
    pub(super) preserve_existing_english_and_digits: bool,
    pub(super) dialogue_content_emitted: bool,
    pub(super) proof_boundary: &'static str,
}

#[derive(Debug, Serialize)]
pub(super) struct ShopRoute {
    pub(super) command_result: u8,
    pub(super) command_result_hex: String,
    pub(super) selected_facility_address: u16,
    pub(super) selected_facility_address_hex: String,
    pub(super) weapon_shop_facility_index: u8,
    pub(super) initial_dialogue_entry_index: u8,
    pub(super) initial_dialogue_entry_index_address: u16,
    pub(super) dialogue_directory_selector: u8,
    pub(super) dialogue_directory_selector_hex: String,
    pub(super) dialogue_directory_selector_address: u16,
    pub(super) outer_state_address: u16,
    pub(super) menu_controller_index_address: u16,
    pub(super) menu_selection_base_address: u16,
    pub(super) observed_menu_controller_index: u8,
    pub(super) observed_menu_selection_address: u16,
    pub(super) outer_state_dispatcher: CodeLocation,
    pub(super) outer_state_handlers: Vec<StateHandler>,
}

#[derive(Debug, Serialize)]
pub(super) struct ShopScreen {
    pub(super) screen_role: &'static str,
    pub(super) runtime_observed: bool,
    pub(super) outer_state: u8,
    pub(super) menu_controller_state: Option<u8>,
    pub(super) selectable_entry_count: usize,
    pub(super) choice_mask: u8,
    pub(super) choice_mask_hex: String,
    pub(super) chr_pair: ChrPair,
    pub(super) translation_target: &'static str,
    pub(super) preserved_original: &'static [&'static str],
    pub(super) visible_components: &'static [&'static str],
    pub(super) temporal_observation: &'static str,
    pub(super) input_actions: Vec<InputAction>,
}

#[derive(Debug, Serialize)]
pub(super) struct ChrPair {
    pub(super) left_fd: u8,
    pub(super) left_fe: u8,
    pub(super) right_fd: u8,
    pub(super) right_fe: u8,
}

#[derive(Debug, Serialize)]
pub(super) struct InputAction {
    pub(super) input: &'static str,
    pub(super) immediate_effect: &'static str,
    pub(super) persistent_gameplay_mutation: bool,
    pub(super) next_role: &'static str,
}

#[derive(Debug, Serialize)]
pub(super) struct PreflightBranch {
    pub(super) condition: &'static str,
    pub(super) dialogue_entry_index: u8,
    pub(super) first_outer_state: u8,
    pub(super) settled_outer_state: u8,
    pub(super) mutates_funds_or_inventory: bool,
    pub(super) next_role: &'static str,
}

#[derive(Debug, Serialize)]
pub(super) struct PurchaseMutation {
    pub(super) accepted_menu_result: u8,
    pub(super) declined_menu_results: [u8; 2],
    pub(super) selected_item_address: u16,
    pub(super) selected_item_address_hex: String,
    pub(super) stored_funds_address: u16,
    pub(super) stored_funds_address_hex: String,
    pub(super) stored_funds_unit: &'static str,
    pub(super) inventory_destination: &'static str,
    pub(super) durability_destination: &'static str,
    pub(super) accepted_dialogue_entry_index: u8,
    pub(super) declined_dialogue_entry_index: u8,
    pub(super) mutation_boundary: &'static str,
}

#[derive(Debug, Serialize)]
pub(super) struct RuntimePurchaseObservation {
    pub(super) source_screen_role: &'static str,
    pub(super) result_screen_role: &'static str,
    pub(super) stored_funds_before: u16,
    pub(super) stored_funds_after: u16,
    pub(super) displayed_funds_before: u16,
    pub(super) displayed_funds_after: u16,
    pub(super) item_destination_address: u16,
    pub(super) item_destination_address_hex: String,
    pub(super) item_before: u8,
    pub(super) item_after: u8,
    pub(super) durability_destination_address: u16,
    pub(super) durability_destination_address_hex: String,
    pub(super) durability_before: u8,
    pub(super) durability_after: u8,
    pub(super) result_outer_state: u8,
    pub(super) result_chr_pair: ChrPair,
    pub(super) result_screenshot_sha256: &'static str,
}

#[derive(Debug, Serialize)]
pub(super) struct RuntimeExitObservation {
    pub(super) source_screen_role: &'static str,
    pub(super) exit_dialogue_entry_index: u8,
    pub(super) exit_outer_state: u8,
    pub(super) branch_mutated_funds_or_inventory: bool,
    pub(super) exit_screenshot_sha256: &'static str,
    pub(super) exit_temporal_observation: &'static str,
    pub(super) advance_input: &'static str,
    pub(super) completion_flag_address: u16,
    pub(super) completion_flag_address_hex: String,
    pub(super) completion_flag_value: u8,
    pub(super) outer_state_after_completion: u8,
    pub(super) returned_screen_role: &'static str,
    pub(super) returned_chr_pair: ChrPair,
    pub(super) returned_screenshot_sha256: &'static str,
    pub(super) completion_effect: &'static str,
}

#[derive(Debug, Serialize)]
pub(super) struct RuntimeInventoryFullObservation {
    pub(super) setup_kind: &'static str,
    pub(super) setup_inventory_items: [u8; 4],
    pub(super) setup_inventory_durability: [u8; 4],
    pub(super) outer_state_sequence: [u8; 3],
    pub(super) dialogue_entry_sequence: [u8; 2],
    pub(super) stored_funds_before: u16,
    pub(super) stored_funds_after: u16,
    pub(super) inventory_items_after: [u8; 4],
    pub(super) inventory_durability_after: [u8; 4],
    pub(super) branch_mutated_funds_or_inventory: bool,
    pub(super) screenshot_sha256: &'static str,
    pub(super) chr_pair: ChrPair,
    pub(super) temporal_observation: &'static str,
    pub(super) advance_input: &'static str,
    pub(super) outer_state_after_completion: u8,
    pub(super) completion_flag_value: u8,
    pub(super) returned_screen_role: &'static str,
    pub(super) returned_screenshot_sha256: &'static str,
    pub(super) evidence_scope: &'static str,
}

#[derive(Debug, Serialize)]
pub(super) struct RuntimeInsufficientFundsObservation {
    pub(super) setup_kind: &'static str,
    pub(super) stored_funds_before_setup: u16,
    pub(super) stored_funds_after_setup: u16,
    pub(super) inventory_items: [u8; 4],
    pub(super) inventory_durability: [u8; 4],
    pub(super) outer_state_sequence: [u8; 6],
    pub(super) dialogue_entry_sequence: [u8; 2],
    pub(super) branch_mutated_funds_or_inventory: bool,
    pub(super) screenshot_sha256: &'static str,
    pub(super) chr_pair: ChrPair,
    pub(super) temporal_observation: &'static str,
    pub(super) continue_input: &'static str,
    pub(super) outer_state_after_continue: u8,
    pub(super) funds_after_continue: u16,
    pub(super) inventory_items_after_continue: [u8; 4],
    pub(super) returned_screen_role: &'static str,
    pub(super) returned_screenshot_sha256: &'static str,
    pub(super) evidence_scope: &'static str,
}

#[derive(Debug, Serialize)]
pub(super) struct RuntimeItemRestrictionObservation {
    pub(super) setup_kind: &'static str,
    pub(super) eligibility_case: ItemEligibilityCase,
    pub(super) warning_outer_state_sequence: [u8; 4],
    pub(super) warning_dialogue_entry_index: u8,
    pub(super) warning_mutated_funds_or_inventory: bool,
    pub(super) warning_screenshot_sha256: &'static str,
    pub(super) chr_pair: ChrPair,
    pub(super) warning_temporal_observation: &'static str,
    pub(super) decline_route: RestrictionDeclineRoute,
    pub(super) accepted_route: RestrictionAcceptedRoute,
    pub(super) evidence_scope: &'static str,
}

#[derive(Debug, Serialize)]
pub(super) struct ItemEligibilityCase {
    pub(super) selected_unit_id: u8,
    pub(super) selected_unit_class: u8,
    pub(super) selected_unit_weapon_level: u8,
    pub(super) selected_shop_ordinal: u8,
    pub(super) selected_item_id: u8,
    pub(super) required_weapon_level: u8,
    pub(super) item_flag_byte: u8,
    pub(super) allowed_class_ids: [u8; 4],
    pub(super) failure_reason: &'static str,
    pub(super) menu_controller_index_address: u16,
    pub(super) menu_controller_index_value: u8,
    pub(super) menu_selection_base_address: u16,
    pub(super) effective_menu_selection_address: u16,
}

#[derive(Debug, Serialize)]
pub(super) struct RestrictionDeclineRoute {
    pub(super) input: &'static str,
    pub(super) outer_state_sequence: [u8; 6],
    pub(super) dialogue_entry_index: u8,
    pub(super) mutated_funds_or_inventory: bool,
    pub(super) prompt_screen_role: &'static str,
    pub(super) prompt_screenshot_sha256: &'static str,
    pub(super) prompt_temporal_observation: &'static str,
    pub(super) continue_input: &'static str,
    pub(super) returned_outer_state: u8,
    pub(super) returned_screen_role: &'static str,
    pub(super) returned_screenshot_sha256: &'static str,
}

#[derive(Debug, Serialize)]
pub(super) struct RestrictionAcceptedRoute {
    pub(super) input: &'static str,
    pub(super) outer_state_sequence: [u8; 2],
    pub(super) dialogue_entry_sequence: [u8; 2],
    pub(super) stored_funds_before: u16,
    pub(super) stored_funds_after: u16,
    pub(super) item_destination_address: u16,
    pub(super) item_value: u8,
    pub(super) durability_destination_address: u16,
    pub(super) durability_value: u8,
    pub(super) result_screen_role: &'static str,
    pub(super) result_screenshot_sha256: &'static str,
    pub(super) result_temporal_observation: &'static str,
    pub(super) completion_input: &'static str,
    pub(super) completion_flag_value: u8,
    pub(super) outer_state_after_completion: u8,
    pub(super) returned_screen_role: &'static str,
    pub(super) returned_screenshot_sha256: &'static str,
}

#[derive(Debug, Serialize)]
pub(super) struct SourceRegionBinding {
    pub(super) role: &'static str,
    pub(super) prg_bank: u8,
    pub(super) prg_bank_hex: String,
    pub(super) cpu_address: u16,
    pub(super) cpu_address_hex: String,
    pub(super) file_offset: usize,
    pub(super) file_offset_hex: String,
    pub(super) byte_count: usize,
    pub(super) source_sha1: String,
}

#[derive(Debug, Serialize)]
pub(super) struct CodeLocation {
    pub(super) prg_bank: u8,
    pub(super) prg_bank_hex: String,
    pub(super) cpu_address: u16,
    pub(super) cpu_address_hex: String,
}

#[derive(Debug, Serialize)]
pub(super) struct StateHandler {
    pub(super) state: usize,
    pub(super) cpu_address: u16,
    pub(super) cpu_address_hex: String,
}

pub struct ShopFlowSummary {
    pub report_sha1: String,
    pub screen_count: usize,
    pub source_region_count: usize,
    pub next_screen_role: &'static str,
}
