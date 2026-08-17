use super::*;

pub(super) fn options_menu_report(
    inputs: &CumulativeReportInputs<'_>,
) -> CumulativeOptionsMenuReport {
    let ui_stage = inputs.ui_stage;
    CumulativeOptionsMenuReport {
        installed_entry_count: 3,
        screen_evidence_manifest_sha1: ui_stage.options_screen_evidence_manifest_sha1.clone(),
        temporal_sample_count: ui_stage.options_temporal_sample_count,
        unique_nametable_count: ui_stage.options_unique_nametable_count,
        observed_row_states: ui_stage.options_observed_row_states.clone(),
        target_glyph_count: ui_stage.options_target_glyph_count,
        visible_active_code_count: ui_stage.options_visible_active_code_count,
        preserved_active_code_count: ui_stage.options_preserved_active_code_count,
        total_slot_demand: ui_stage.options_total_slot_demand,
        capacity_bound_to_build: true,
    }
}

pub(super) fn front_end_menu_report(
    inputs: &CumulativeReportInputs<'_>,
) -> CumulativeFrontEndMenuReport {
    let front_end_menu_plan = &inputs.input_plan.front_end_menu_plan;
    let front_end_stage = inputs.front_end_stage;
    CumulativeFrontEndMenuReport {
        workspace_sha1: front_end_menu_plan.workspace_sha1.clone(),
        workspace_entry_count: front_end_menu_plan.entries.len(),
        installed_entry_count: front_end_menu_plan.entries.len(),
        installed_source_storage_byte_count: front_end_menu_plan
            .entries
            .iter()
            .map(|entry| entry.source_storage_byte_count)
            .sum(),
        installed_output_storage_byte_count: front_end_stage
            .encoded_entries
            .iter()
            .map(Vec::len)
            .sum(),
        original_english_and_digits_preserved: true,
        screen_evidence_manifest_sha1: front_end_stage.page.manifest_sha1.clone(),
        temporal_sample_count: front_end_stage.page.temporal_sample_count,
        unique_nametable_count: front_end_stage.page.unique_nametable_count,
        unique_glyph_count: front_end_stage.page.assignments.len(),
        glyph_assignment_sha1: assignment_sha1(&front_end_stage.page.assignments),
        preserved_screen_active_code_count: front_end_stage.page.preserved_screen_active_code_count,
        preserved_source_active_code_count: front_end_stage.page.preserved_source_active_code_count,
        preserved_result_dialogue_active_code_count: front_end_stage
            .page
            .preserved_result_dialogue_active_code_count,
        preserved_active_code_count: front_end_stage.page.preserved_active_code_count,
        font_physical_page: front_end_stage.page.physical_chr_page,
        font_mapper_register: front_end_stage.page.mapper_register,
        font_page_sha1: front_end_stage.page.page_sha1.clone(),
        font_page_pack_sha1: sha1_hex(&front_end_stage.page.page_pack),
        central_fe_companion_refresh_routed: true,
        no_save_source_lifetime_bound: true,
        save_slot_selection_source_lifetime_bound: true,
        runtime_variants_bound_to_build: false,
        review_complete: front_end_menu_plan.review_complete,
    }
}

pub(super) fn playable_unit_name_report(
    inputs: &CumulativeReportInputs<'_>,
) -> CumulativeUnitNameReport {
    let unit_name_plan = &inputs.input_plan.unit_name_plan;
    let unit_name_stage = inputs.unit_name_stage;
    let roster_page_total_slot_demand = inputs.roster_page_total_slot_demand;
    CumulativeUnitNameReport {
        workspace_sha1: unit_name_plan.workspace_sha1.clone(),
        workspace_entry_count: unit_name_plan.entries.len(),
        unique_glyph_count: unit_name_plan.unique_glyphs().len(),
        roster_page_target_glyph_count: unit_name_stage.page.roster_assignments.len(),
        roster_page_preserved_active_code_count: unit_name_stage.page.preserved_roster_code_count,
        roster_page_total_slot_demand,
        roster_projection_byte_count: unit_name_stage.tables.roster.pointer_table.len()
            + unit_name_stage.tables.roster.strings.len(),
        unit_ui_projection_byte_count: unit_name_stage.tables.unit_ui.pointer_table.len()
            + unit_name_stage.tables.unit_ui.strings.len(),
        roster_assignment_sha1: assignment_sha1(&unit_name_stage.page.roster_assignments),
        unit_ui_assignment_sha1: assignment_sha1(&unit_name_stage.page.unit_ui_assignments),
        roster_page_pack_sha1: unit_name_stage.page.roster_page_pack_sha1.clone(),
        unit_ui_page_pack_sha1: unit_name_stage.page.unit_ui_page_pack_sha1.clone(),
        unit_ui_font_physical_page: unit_name_stage.page.unit_ui_physical_page,
        unit_ui_font_mapper_register: unit_name_stage.page.unit_ui_mapper_register,
        screen_evidence_manifest_sha1: unit_name_stage.page.evidence_manifest_sha1.clone(),
        temporal_sample_count: unit_name_stage.page.temporal_sample_count,
        unique_nametable_count: unit_name_stage.page.unique_nametable_count,
        preserved_unit_ui_code_count: unit_name_stage.page.preserved_unit_ui_code_count,
        roster_projection_installed: true,
        unit_summary_projection_installed: true,
        source_battle_table_preserved: false,
        source_ending_table_preserved: true,
        roster_capacity_bound_to_build: true,
        runtime_bound_to_build: false,
        review_complete: unit_name_plan.review_complete,
    }
}

pub(super) fn class_profile_report(
    inputs: &CumulativeReportInputs<'_>,
) -> CumulativeClassProfileReport {
    let class_profile_plan = &inputs.input_plan.class_profile_plan;
    let class_profile_stage = inputs.class_profile_stage;
    let class_profile_runtime = inputs.class_profile_runtime;
    CumulativeClassProfileReport {
        workspace_sha1: class_profile_plan.workspace_sha1.clone(),
        workspace_entry_count: class_profile_plan.entries.len(),
        installed_entry_count: class_profile_plan.entries.len(),
        installed_description_line_count: class_profile_plan.description_line_count(),
        installed_source_storage_byte_count: class_profile_plan
            .entries
            .iter()
            .map(|entry| {
                entry.title_source_storage_byte_count + entry.description_source_storage_byte_count
            })
            .sum(),
        installed_output_storage_byte_count: class_profile_stage
            .encoded_titles
            .iter()
            .chain(&class_profile_stage.encoded_descriptions)
            .map(Vec::len)
            .sum(),
        total_unique_glyph_count: class_profile_plan.unique_glyphs().len(),
        page_unique_glyph_counts: [
            class_profile_stage.page.assignments[0].len(),
            class_profile_stage.page.assignments[1].len(),
        ],
        glyph_assignment_sha1s: [
            assignment_sha1(&class_profile_stage.page.assignments[0]),
            assignment_sha1(&class_profile_stage.page.assignments[1]),
        ],
        font_physical_pages: class_profile_stage.page.physical_pages,
        font_mapper_registers: class_profile_stage.page.mapper_registers,
        font_page_sha1s: class_profile_stage.page.page_sha1s.clone(),
        font_page_pack_sha1: sha1_hex(&class_profile_stage.page.page_pack),
        screen_evidence_manifest_sha1: class_profile_stage.page.evidence_manifest_sha1.clone(),
        temporal_sample_count: class_profile_stage.page.temporal_sample_count,
        unique_image_count: class_profile_stage.page.unique_image_count,
        runtime_evidence_manifest_sha1: class_profile_runtime
            .as_ref()
            .map_or_else(String::new, |runtime| runtime.manifest_sha1.clone()),
        runtime_sample_count: class_profile_runtime
            .as_ref()
            .map_or(0, |runtime| runtime.sample_count),
        runtime_unique_image_count: class_profile_runtime
            .as_ref()
            .map_or(0, |runtime| runtime.unique_image_count),
        visible_code_count: class_profile_stage.page.visible_code_count,
        preserved_active_code_count: class_profile_stage.page.preserved_active_code_count,
        original_english_digits_and_ui_preserved: true,
        profile_index_page_selector_installed: true,
        runtime_bound_to_build: class_profile_runtime.is_some(),
        review_complete: class_profile_plan.review_complete,
    }
}

pub(super) fn title_logo_report(inputs: &CumulativeReportInputs<'_>) -> CumulativeTitleLogoReport {
    let title_graphics_plan = &inputs.input_plan.title_graphics_plan;
    let title_logo_stage = inputs.title_logo_stage;
    let title_logo_runtime = inputs.title_logo_runtime;
    CumulativeTitleLogoReport {
        workspace_sha1: title_graphics_plan.workspace_sha1.clone(),
        asset_sha1: title_logo_stage.asset_sha1.clone(),
        source_owned_tile_count: title_logo_stage.source_owned_tile_count,
        installed_unique_tile_count: title_logo_stage.installed_unique_tile_count,
        installed_tilemap_cell_count: title_logo_stage.installed_tilemap_cell_count,
        physical_chr_page: title_logo_stage.physical_chr_page,
        installed_chr_page_sha1: title_logo_stage.installed_chr_page_sha1.clone(),
        installed_stream_sha1: title_logo_stage.installed_stream_sha1.clone(),
        installed_runtime_cleared_top_strip_cell_count: title_logo_stage
            .installed_runtime_cleared_top_strip_cell_count,
        installed_runtime_reasserted_logo_cell_count: title_logo_stage
            .installed_runtime_reasserted_logo_cell_count,
        installed_runtime_completion_stream_sha1: title_logo_stage
            .installed_runtime_completion_stream_sha1
            .clone(),
        preserved_title_stream_bytes_unchanged: title_logo_stage
            .preserved_title_stream_bytes_unchanged,
        preserved_runtime_completion_control_bytes_unchanged: title_logo_stage
            .preserved_runtime_completion_control_bytes_unchanged,
        unassigned_title_chr_patterns_unchanged: title_logo_stage
            .unassigned_title_chr_patterns_unchanged,
        source_sword_sprite_tm_and_copyright_assets_unchanged: true,
        runtime_evidence_manifest_sha1: title_logo_runtime
            .as_ref()
            .map_or_else(String::new, |runtime| runtime.manifest_sha1.clone()),
        runtime_sample_count: title_logo_runtime
            .as_ref()
            .map_or(0, |runtime| runtime.sample_count),
        runtime_unique_image_count: title_logo_runtime
            .as_ref()
            .map_or(0, |runtime| runtime.unique_image_count),
        runtime_bound_to_build: title_logo_runtime.is_some(),
        review_complete: title_graphics_plan.review_complete,
    }
}

pub(super) fn weapon_shop_shared_text_report(
    inputs: &CumulativeReportInputs<'_>,
) -> CumulativeWeaponShopSharedTextReport {
    let choice_label_plan = &inputs.input_plan.choice_label_plan;
    let shop_dialogue_stage = inputs.shop_dialogue_stage;
    let weapon_shop_shared_text_stage = inputs.weapon_shop_shared_text_stage;
    let weapon_shop_shared_text_runtime = inputs.weapon_shop_shared_text_runtime;
    let weapon_shop_shared_page_total_slot_demand =
        inputs.weapon_shop_shared_page_total_slot_demand;
    CumulativeWeaponShopSharedTextReport {
        screen_role: WEAPON_SHOP_SHARED_TEXT_SCREEN_ROLE,
        fixed_text_workspace_sha1: weapon_shop_shared_text_stage
            .plan
            .fixed_text_workspace_sha1
            .clone(),
        choice_label_workspace_sha1: weapon_shop_shared_text_stage
            .plan
            .choice_label_workspace_sha1
            .clone(),
        installed_item_name_count: weapon_shop_shared_text_stage
            .plan
            .projection
            .item_name_count,
        installed_choice_label_count: choice_label_plan.entries.len(),
        projected_item_pointer_count: weapon_shop_shared_text_stage
            .plan
            .projection
            .item_pointer_table
            .len()
            / 2,
        item_string_byte_count: weapon_shop_shared_text_stage
            .plan
            .projection
            .item_string_byte_count,
        choice_string_byte_count: weapon_shop_shared_text_stage
            .plan
            .projection
            .choice_string_byte_count,
        shared_page_unique_glyph_count: weapon_shop_shared_text_stage.plan.page.assignments.len(),
        shared_page_preserved_active_code_count: weapon_shop_shared_text_stage
            .plan
            .page
            .preserved_active_code_count,
        shared_page_total_slot_demand: weapon_shop_shared_page_total_slot_demand,
        added_glyph_count: weapon_shop_shared_text_stage.plan.page.assignments.len()
            - shop_dialogue_stage.page.assignments.len(),
        glyph_assignment_sha1: assignment_sha1(
            &weapon_shop_shared_text_stage.plan.page.assignments,
        ),
        font_physical_page: weapon_shop_shared_text_stage.plan.page.physical_chr_page,
        font_mapper_register: weapon_shop_shared_text_stage.plan.page.mapper_register,
        font_page_sha1: weapon_shop_shared_text_stage.plan.page.page_sha1.clone(),
        font_page_pack_sha1: sha1_hex(&weapon_shop_shared_text_stage.plan.page.page_pack),
        item_list_pointer_selector_installed: true,
        selected_item_pointer_selector_installed: true,
        choice_pointer_selector_installed: true,
        unconverted_consumers_fallback_to_source_tables: true,
        capacity_bound_screen_roles: WEAPON_SHOP_CAPACITY_BOUND_SCREEN_ROLES.to_vec(),
        runtime_evidence_manifest_sha1: weapon_shop_shared_text_runtime
            .as_ref()
            .map_or_else(String::new, |runtime| runtime.manifest_sha1.clone()),
        runtime_evidence_output_sha1: weapon_shop_shared_text_runtime
            .as_ref()
            .map_or_else(String::new, |runtime| runtime.output_sha1.clone()),
        runtime_sample_count: weapon_shop_shared_text_runtime
            .as_ref()
            .map_or(0, |runtime| runtime.sample_count),
        runtime_unique_image_count: weapon_shop_shared_text_runtime
            .as_ref()
            .map_or(0, |runtime| runtime.unique_image_count),
        runtime_bound_dialogue_screen_roles: weapon_shop_shared_text_runtime
            .as_ref()
            .map_or_else(Vec::new, |runtime| runtime.dialogue_screen_roles.clone()),
        runtime_bound_item_name_screen_roles: weapon_shop_shared_text_runtime
            .as_ref()
            .map_or_else(Vec::new, |runtime| runtime.item_name_screen_roles.clone()),
        runtime_bound_choice_label_screen_roles: weapon_shop_shared_text_runtime
            .as_ref()
            .map_or_else(Vec::new, |runtime| {
                runtime.choice_label_screen_roles.clone()
            }),
        runtime_bound_to_stage_output: weapon_shop_shared_text_runtime.is_some(),
        runtime_carried_forward_by_verified_writes: weapon_shop_shared_text_runtime.is_some(),
        review_complete: weapon_shop_shared_text_stage.plan.review_complete,
    }
}

pub(super) fn battle_text_report(
    inputs: &CumulativeReportInputs<'_>,
) -> CumulativeBattleTextReport {
    let battle_stage = inputs.battle_stage;
    CumulativeBattleTextReport {
        fixed_text_workspace_sha1: battle_stage.fixed_workspace_sha1.clone(),
        dialogue_workspace_sha1: battle_stage.dialogue_workspace_sha1.clone(),
        temporal_manifest_sha1: battle_stage.temporal_manifest_sha1.clone(),
        runtime_base_report_sha1: battle_stage.runtime_base_report_sha1.clone(),
        loader_report_sha1: battle_stage.loader_report_sha1.clone(),
        installed_fixed_entry_count: battle_stage.fixed_entry_count,
        installed_unit_name_count: battle_stage.unit_name_count,
        installed_enemy_name_count: battle_stage.enemy_name_count,
        installed_class_name_count: battle_stage.class_name_count,
        installed_item_name_count: battle_stage.item_name_count,
        installed_terrain_name_count: battle_stage.terrain_name_count,
        installed_battle_message_template_count: battle_stage.battle_message_template_count,
        installed_battle_forecast_label_count: battle_stage.battle_forecast_label_count,
        weapon_shop_item_names_subset_of_battle_catalog: true,
        installed_dialogue_record_count: battle_stage.dialogue_record_count,
        installed_translated_line_count: battle_stage.dialogue_translated_line_count,
        stable_color_count: battle_stage.stable_color_count,
        glyph_atlas_tile_count: battle_stage.glyph_atlas_tile_count,
        observed_runtime_tuple_count: battle_stage.observed_runtime_tuple_count,
        maximum_observed_overlay_count: battle_stage.maximum_observed_overlay_count,
        maximum_observed_ppu_write_count: battle_stage.maximum_observed_ppu_write_count,
        runtime_routine_byte_count: battle_stage.runtime_routine_byte_count,
        text_diff_range_count: battle_stage.text_diff_range_count,
        cumulative_selector_ranges_preserved: true,
        original_english_digits_and_graphics_preserved: true,
        runtime_bound_to_build: false,
        review_complete: false,
    }
}
