use crate::{
    dialogue_assets::{EncodedMainDialogueBundle, MainDialogueDisplayPlan},
    font_slots::{FONT_PAGE_SIZE, FONT_TILE_SIZE},
    mapper165::battle_codebook_plan::GlyphWorksetPagePlan,
    sha1_hex,
};

use super::{
    super::{
        chapter_intro_residency::ChapterIntroResidencyPlan,
        current_candidate::DialoguePagePoolCapacity,
        dynamic_composition::DialogueRuntimeCompositionPlan,
        dynamic_input_producers::DynamicInputProducerPlan,
        dynamic_inputs::{
            DynamicDialogueInputPlan, DynamicProducerEncodingPlan, DynamicStringPageCodePlan,
        },
        runtime_code,
        runtime_identity::DialogueRuntimeIdentityPlan,
        runtime_material::DialogueRuntimeMaterialPlan,
        transition_residency::TransitionResidencyPlan,
    },
    ChapterIntroResidency, DialogueCodebook, DialoguePagePool, DialogueRuntimeComposition,
    DialogueStorage, InstallationGates,
};

pub(in crate::full_translation_install) struct DialogueCodebookReportInputs<'a> {
    pub(in crate::full_translation_install) display: &'a MainDialogueDisplayPlan,
    pub(in crate::full_translation_install) codebook: &'a GlyphWorksetPagePlan,
    pub(in crate::full_translation_install) font_page_pack: &'a [u8],
    pub(in crate::full_translation_install) technical_installation_complete: bool,
    pub(in crate::full_translation_install) transition_residency: &'a TransitionResidencyPlan,
}

pub(in crate::full_translation_install) fn project_dialogue_codebook(
    inputs: DialogueCodebookReportInputs<'_>,
) -> DialogueCodebook {
    DialogueCodebook {
        canonical_record_count: inputs.display.canonical_record_count,
        page_workset_count: inputs.display.page_worksets.len(),
        unique_workset_count: inputs.codebook.unique_workset_count,
        literal_glyph_count: inputs.display.unique_glyphs().len(),
        unique_glyph_count: inputs.codebook.glyph_count,
        active_slot_count: crate::font_slots::ACTIVE_HANGUL_SLOT_COUNT,
        maximum_workset_slot_demand: inputs.codebook.maximum_workset_slot_demand,
        maximum_page_slot_demand: inputs.codebook.maximum_page_slot_demand,
        greedy_page_count: inputs.codebook.greedy_page_count,
        packing_strategy: inputs.codebook.packing_strategy,
        constraint_solver_version: inputs.codebook.constraint_solver_version.clone(),
        constraint_solver_timeout_seconds: inputs.codebook.constraint_solver_timeout_seconds,
        packing_sha1: inputs.codebook.packing_sha1.clone(),
        page_assignment_sha1: inputs.codebook.page_assignment_sha1.clone(),
        static_page_upper_bound_count: inputs.codebook.page_assignments.len(),
        static_page_pack_sha1: sha1_hex(inputs.font_page_pack),
        static_page_pack_preserves_every_workset_code: true,
        canonical_records_connected: true,
        page_local_bundle_encoding_connected: true,
        glyph_characters_encoded_into_installed_runtime_atlas: inputs
            .technical_installation_complete,
        transition_stable_lifetime_count: inputs.transition_residency.lifetime_count,
        multi_record_transition_stable_lifetime_count: inputs
            .transition_residency
            .multi_record_lifetime_count,
        maximum_transition_stable_lifetime_record_count: inputs
            .transition_residency
            .maximum_lifetime_record_count,
        maximum_transition_stable_lifetime_workset_count: inputs
            .transition_residency
            .maximum_lifetime_workset_count,
        maximum_transition_stable_lifetime_slot_demand: inputs
            .transition_residency
            .maximum_lifetime_slot_demand,
        every_resident_transition_uses_one_codebook: true,
    }
}

pub(in crate::full_translation_install) fn project_chapter_intro_residency(
    plan: &ChapterIntroResidencyPlan,
) -> ChapterIntroResidency {
    ChapterIntroResidency {
        chapter_context_count: plan.chapter_context_count,
        resident_workset_count: plan.resident_workset_count,
        title_glyph_count: plan.title_glyph_count,
        fixed_code_count: plan.fixed_code_count,
        encoded_title_count: plan.encoded_titles.len(),
        maximum_augmented_workset_slot_demand: plan.maximum_augmented_workset_slot_demand,
        fixed_assignment_sha1: plan.fixed_assignment_sha1.clone(),
        every_title_glyph_has_one_stable_code: true,
        title_storage_connected: true,
    }
}

pub(in crate::full_translation_install) fn project_dialogue_page_pool(
    capacity: &DialoguePagePoolCapacity,
    remaining_available_page_count: usize,
    prebuilt_font_page_upper_bound: usize,
) -> DialoguePagePool {
    DialoguePagePool {
        current_candidate_sha1: capacity.current_candidate_sha1.clone(),
        current_chr_page_count: capacity.current_chr_page_count,
        first_installable_physical_page: capacity.first_installable_physical_page,
        superseded_maximum_dialogue_page_count: capacity.superseded_maximum_dialogue_page_count,
        appendable_page_count: capacity.appendable_page_count,
        available_page_count: capacity.available_page_count,
        cold_request_presentation_page_count: 1,
        remaining_available_page_count,
        prebuilt_font_page_upper_bound,
        prebuilt_upper_bound_fits_available_pages: prebuilt_font_page_upper_bound
            <= remaining_available_page_count,
        exact_available_page_fit_decided: false,
        mapper_capacity_bound: true,
        current_candidate_bound: true,
    }
}

pub(in crate::full_translation_install) fn project_dialogue_storage(
    encoded: &EncodedMainDialogueBundle,
    record_count: usize,
    source_owned_storage_byte_count: usize,
    planned_storage_byte_count: usize,
) -> DialogueStorage {
    DialogueStorage {
        region_count: encoded.regions.len(),
        record_count,
        pointer_write_count: encoded.pointer_writes.len(),
        source_owned_storage_byte_count,
        planned_storage_byte_count,
        remaining_storage_byte_count: source_owned_storage_byte_count - planned_storage_byte_count,
        every_pointer_within_source_owned_regions: true,
    }
}

pub(in crate::full_translation_install) struct InstallationGateReportInputs {
    pub(in crate::full_translation_install) translation_input_complete: bool,
    pub(in crate::full_translation_install) all_declared_consumers_statically_accounted: bool,
    pub(in crate::full_translation_install) carried_ui_domains_complete: bool,
    pub(in crate::full_translation_install) carried_battle_domains_complete: bool,
    pub(in crate::full_translation_install) technical_installation_complete: bool,
    pub(in crate::full_translation_install) declared_consumer_runtime_observation_complete: bool,
}

pub(in crate::full_translation_install) fn project_installation_gates(
    inputs: InstallationGateReportInputs,
) -> InstallationGates {
    InstallationGates {
        all_translation_inputs_loaded: inputs.translation_input_complete,
        all_dialogue_records_encoded: true,
        all_visible_dialogue_text_encoded: true,
        all_dialogue_pointers_planned: true,
        all_dialogue_page_code_assignments_found: true,
        all_dialogue_page_worksets_packed: true,
        all_resident_dialogue_transitions_use_one_codebook: true,
        all_chapter_titles_encoded_with_resident_codes: true,
        all_chapter_title_storage_writes_planned: true,
        cold_request_presentation_page_planned: true,
        cold_request_presentation_write_planned: true,
        dialogue_runtime_composition_planned: true,
        all_declared_consumer_writes_planned: inputs.all_declared_consumers_statically_accounted,
        all_carried_ui_domains_reinspected: inputs.carried_ui_domains_complete,
        all_carried_battle_domains_reinspected: inputs.carried_battle_domains_complete,
        declared_plan_technical_installation_complete: inputs
            .all_declared_consumers_statically_accounted
            && inputs.technical_installation_complete,
        declared_consumer_runtime_observation_complete: inputs
            .declared_consumer_runtime_observation_complete,
    }
}

pub(in crate::full_translation_install) struct DialogueRuntimeCompositionReportInputs<'a> {
    pub(in crate::full_translation_install) composition: DialogueRuntimeCompositionPlan,
    pub(in crate::full_translation_install) dynamic_inputs: &'a DynamicDialogueInputPlan,
    pub(in crate::full_translation_install) dynamic_page_codes: DynamicStringPageCodePlan,
    pub(in crate::full_translation_install) dynamic_string_producers_bound: bool,
    pub(in crate::full_translation_install) dynamic_input_producers: DynamicInputProducerPlan,
    pub(in crate::full_translation_install) dynamic_producer_encoding: DynamicProducerEncodingPlan,
    pub(in crate::full_translation_install) runtime_identity: DialogueRuntimeIdentityPlan,
    pub(in crate::full_translation_install) atlas_scan_and_identity_byte_count: usize,
    pub(in crate::full_translation_install) runtime_material: DialogueRuntimeMaterialPlan,
    pub(in crate::full_translation_install) technical_installation_complete: bool,
    pub(in crate::full_translation_install) page_capacity: &'a DialoguePagePoolCapacity,
}

pub(in crate::full_translation_install) fn project_dialogue_runtime_composition(
    inputs: DialogueRuntimeCompositionReportInputs<'_>,
) -> DialogueRuntimeComposition {
    let composition = inputs.composition;
    let glyph_atlas_byte_count = composition.glyph_atlas.len();
    let glyph_atlas_sha1 = sha1_hex(&composition.glyph_atlas);

    DialogueRuntimeComposition {
        strategy_selected: true,
        glyph_atlas_tile_count: composition.glyph_atlas_tile_count,
        dialogue_codebook_glyph_count: composition.dialogue_codebook_glyph_count,
        additional_cross_domain_glyph_count: composition.additional_cross_domain_glyph_count,
        glyph_atlas_covers_every_required_domain_glyph: true,
        stored_bytes_per_glyph: 8,
        composed_bytes_per_glyph: FONT_TILE_SIZE,
        glyph_atlas_byte_count,
        glyph_atlas_prg_8k_page_count: glyph_atlas_byte_count.div_ceil(8 * 1024),
        glyph_atlas_sha1,
        generated_high_bitplane_is_zero: true,
        page_recipe_reference_byte_count: composition.page_recipe_reference_byte_count,
        page_recipe_block_byte_count: composition.page_recipe_block_byte_count,
        page_recipe_blocks_sha1: composition.page_recipe_blocks_sha1,
        page_recipe_reference_offset: composition.page_recipe_reference_offset,
        record_recipe_directory_offset: composition.record_recipe_directory_offset,
        four_by_four_block_count: composition.four_by_four_block_count,
        four_by_four_block_index_bit_count: composition.four_by_four_block_index_bit_count,
        four_by_four_block_atlas_byte_count: composition.four_by_four_block_atlas_byte_count,
        static_page_group_count: composition.static_page_group_count,
        static_page_group_overlay_reference_count: composition
            .static_page_group_overlay_reference_count,
        maximum_static_page_group_overlay_tile_count: composition
            .maximum_static_page_group_overlay_tile_count,
        visible_page_recipe_count: composition.visible_page_recipe_count,
        visible_page_recipe_reference_count: composition.visible_page_recipe_reference_count,
        visible_page_overlay_reference_count: composition.visible_page_overlay_reference_count,
        maximum_visible_page_overlay_tile_count: composition
            .maximum_visible_page_overlay_tile_count,
        cold_page_restore_frame_count: usize::from(runtime_code::transport::RESTORE_CHUNK_COUNT)
            .div_ceil(usize::from(
                runtime_code::transport::RESTORE_CHUNKS_PER_FRAME,
            )),
        maximum_cold_page_preparation_frame_count: usize::from(
            runtime_code::transport::RESTORE_CHUNK_COUNT,
        )
        .div_ceil(usize::from(
            runtime_code::transport::RESTORE_CHUNKS_PER_FRAME,
        )) + composition
            .maximum_visible_page_overlay_tile_count
            .div_ceil(usize::from(runtime_code::transport::TILES_PER_FRAME)),
        maximum_resident_page_overlay_frame_count: composition
            .maximum_visible_page_overlay_tile_count
            .div_ceil(usize::from(runtime_code::transport::TILES_PER_FRAME)),
        maximum_visible_page_rebuild_ppu_write_count: FONT_PAGE_SIZE
            + composition.maximum_visible_page_overlay_tile_count * FONT_TILE_SIZE,
        sequential_page_transition_count: composition.sequential_page_transition_count,
        distinct_visible_page_recipe_transition_count: composition
            .distinct_visible_page_recipe_transition_count,
        unchanged_visible_page_recipe_transition_count: composition
            .unchanged_visible_page_recipe_transition_count,
        resident_group_transition_count: composition.resident_group_transition_count,
        resident_group_change_count: composition.resident_group_change_count,
        resident_group_reuse_count: composition.resident_group_reuse_count,
        maximum_delta_tile_count: composition.maximum_delta_tile_count,
        maximum_delta_ppu_write_count: composition.maximum_delta_ppu_write_count,
        total_delta_ppu_write_count: composition.total_delta_ppu_write_count,
        rebuild_every_visible_page_ppu_write_count: composition
            .rebuild_every_visible_page_ppu_write_count,
        initial_rebuild_then_delta_ppu_write_count: composition
            .initial_rebuild_then_delta_ppu_write_count,
        direct_visible_page_recipe_byte_count: composition.direct_visible_page_recipe_byte_count,
        bitpacked_visible_page_recipe_byte_count: composition
            .bitpacked_visible_page_recipe_byte_count,
        bitmap_and_atlas_index_visible_page_recipe_byte_count: composition
            .bitmap_and_atlas_index_visible_page_recipe_byte_count,
        direct_delta_recipe_byte_count: composition.direct_delta_recipe_byte_count,
        bitpacked_delta_recipe_byte_count: composition.bitpacked_delta_recipe_byte_count,
        visible_page_recipe_strategy_selected: true,
        script_scan_covers_dynamic_strings: false,
        dynamic_string_control_count: composition.dynamic_string_control_count,
        dynamic_string_page_count: composition.dynamic_string_page_count,
        dynamic_string_selector_count: composition.dynamic_string_selector_count,
        dynamic_string_domain_count: inputs.dynamic_inputs.declared_domain_count,
        translated_dynamic_page_count: inputs.dynamic_inputs.translated_dynamic_page_count,
        preserved_numeric_page_count: inputs.dynamic_inputs.preserved_numeric_page_count,
        translated_dynamic_glyph_count: inputs.dynamic_inputs.translated_dynamic_glyph_count,
        combined_dialogue_glyph_count: inputs.dynamic_inputs.combined_dialogue_glyph_count,
        maximum_possible_domain_glyph_count: inputs
            .dynamic_inputs
            .maximum_possible_domain_glyph_count,
        maximum_augmented_workset_slot_demand: inputs
            .dynamic_inputs
            .maximum_augmented_workset_slot_demand,
        maximum_rendered_target_glyph_upper_bound: inputs
            .dynamic_inputs
            .maximum_rendered_target_glyph_upper_bound,
        mixed_dynamic_domain_page_count: inputs.dynamic_inputs.mixed_dynamic_domain_page_count,
        dynamic_string_domains_classified: inputs.dynamic_inputs.every_dynamic_control_classified,
        dynamic_augmented_worksets_fit: inputs.dynamic_inputs.every_augmented_workset_fits,
        canonical_dynamic_code_count: inputs.dynamic_page_codes.canonical_code_count,
        translated_dynamic_page_group_count: inputs
            .dynamic_page_codes
            .translated_dynamic_page_group_count,
        dynamic_page_code_identity_entry_count: inputs.dynamic_page_codes.identity_entry_count,
        dynamic_page_code_material_byte_count: inputs
            .dynamic_page_codes
            .selected_material_byte_count,
        dynamic_page_code_strategy: inputs.dynamic_page_codes.selected_strategy,
        dynamic_page_code_material_sha1: inputs.dynamic_page_codes.material_sha1,
        canonical_dynamic_codes_are_page_physical_codes: inputs
            .dynamic_page_codes
            .canonical_codes_are_page_physical_codes,
        page_selectors_use_plain_group_indices: inputs
            .dynamic_page_codes
            .page_selectors_use_plain_group_indices,
        every_translated_dynamic_page_directly_consumable: inputs
            .dynamic_page_codes
            .every_translated_dynamic_page_directly_consumable,
        dynamic_string_producers_bound: inputs.dynamic_string_producers_bound,
        dynamic_string_producers: inputs.dynamic_input_producers,
        dynamic_producer_encoding: inputs.dynamic_producer_encoding,
        dense_group_lookup_byte_count: composition.dense_group_lookup_byte_count,
        record_recipe_directory_byte_count: composition.record_recipe_directory_byte_count,
        scan_material_byte_count: composition.scan_material_byte_count,
        scan_material_sha1: composition.scan_material_sha1,
        scan_material_serialized: true,
        atlas_and_scan_material_byte_count: glyph_atlas_byte_count
            + composition.scan_material_byte_count,
        dialogue_runtime_identity: inputs.runtime_identity,
        atlas_scan_and_identity_byte_count: inputs.atlas_scan_and_identity_byte_count,
        runtime_material_layout_and_assembly: inputs.runtime_material,
        runtime_page_scan_bound_to_assembled_control_flow: inputs.technical_installation_complete,
        current_battle_glyph_atlas_tile_count: inputs.page_capacity.battle_glyph_atlas_tile_count,
        current_battle_maximum_ppu_write_count: inputs.page_capacity.battle_maximum_ppu_write_count,
        current_battle_runtime_routine_byte_count: inputs
            .page_capacity
            .battle_runtime_routine_byte_count,
        current_battle_runtime_bound_to_build: inputs.page_capacity.battle_runtime_bound_to_build,
        battle_compositor_is_directly_reusable: false,
        main_dialogue_page_identity_material_serialized: true,
        main_dialogue_page_identity_bound_to_assembled_control_flow: inputs
            .technical_installation_complete,
        main_dialogue_transition_hook_planned: true,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn installation_gate_projection_does_not_promote_partial_static_installation() {
        let gates = project_installation_gates(InstallationGateReportInputs {
            translation_input_complete: true,
            all_declared_consumers_statically_accounted: false,
            carried_ui_domains_complete: true,
            carried_battle_domains_complete: true,
            technical_installation_complete: true,
            declared_consumer_runtime_observation_complete: false,
        });

        assert!(!gates.all_declared_consumer_writes_planned);
        assert!(!gates.declared_plan_technical_installation_complete);
        assert!(!gates.declared_consumer_runtime_observation_complete);
    }
}
