use super::*;

pub(super) fn chapter_title_report(
    inputs: &CumulativeReportInputs<'_>,
) -> CumulativeChapterTitleReport {
    let chapter_title_plan = &inputs.input_plan.chapter_title_plan;
    let chapter_one_title = inputs.chapter_one_title;
    let chapter_two_title = inputs.chapter_two_title;
    let chapter_one_encoded_title = inputs.chapter_one_encoded_title;
    let chapter_two_encoded_title = inputs.chapter_two_encoded_title;

    CumulativeChapterTitleReport {
        workspace_sha1: chapter_title_plan.workspace_sha1.clone(),
        workspace_entry_count: chapter_title_plan.entry_count,
        translated_entry_count: chapter_title_plan.translated_entry_count,
        installed_entry_count: 2,
        installed_chapter_indices: vec![CHAPTER_ONE_INDEX, CHAPTER_TWO_INDEX],
        installed_source_storage_byte_count: chapter_one_title.source_storage_byte_count
            + chapter_two_title.source_storage_byte_count,
        installed_output_storage_byte_count: chapter_one_encoded_title.len()
            + chapter_two_encoded_title.len(),
        original_digits_preserved: true,
        intro_title_table_installed: true,
        ending_scroll_duplicate_installed: false,
        review_complete: chapter_title_plan.review_complete,
    }
}

pub(super) fn main_dialogue_report(
    inputs: &CumulativeReportInputs<'_>,
) -> CumulativeDialogueReport {
    let dialogue_workspace = &inputs.input_plan.dialogue_workspace;
    let shop_dialogue_plan = &inputs.input_plan.shop_dialogue_plan;
    let maximum_dialogue_plan = &inputs.input_plan.maximum_dialogue_plan;
    let chapter_one_plans = inputs.chapter_one_plans;
    let chapter_two_plans = inputs.chapter_two_plans;
    let chapter_one_encoded_records = inputs.chapter_one_encoded_records;
    let chapter_two_encoded_records = inputs.chapter_two_encoded_records;
    let chapter_one_page = inputs.chapter_one_page;
    let chapter_two_page = inputs.chapter_two_page;
    let shop_dialogue_stage = inputs.shop_dialogue_stage;
    let shop_dialogue_runtime = inputs.shop_dialogue_runtime;
    let maximum_dialogue_stage = inputs.maximum_dialogue_stage;
    let maximum_dialogue_runtime = inputs.maximum_dialogue_runtime;
    let installed_main_dialogue_record_count = inputs.installed_main_dialogue_record_count;
    let translated_line_count = inputs.translated_line_count;
    let installed_dialogue_glyph_slot_count = inputs.installed_dialogue_glyph_slot_count;
    let source_storage_byte_count = inputs.source_storage_byte_count;
    let planned_storage_byte_count = inputs.planned_storage_byte_count;

    CumulativeDialogueReport {
        workspace_sha1: chapter_one_plans[0].workspace_sha1.clone(),
        workspace_record_count: dialogue_workspace.record_count,
        workspace_filled_line_count: dialogue_workspace.filled_line_count,
        installed_record_count: installed_main_dialogue_record_count,
        installed_translated_line_count: translated_line_count,
        installed_shared_page_glyph_slot_count: installed_dialogue_glyph_slot_count,
        source_storage_byte_count,
        planned_storage_byte_count,
        remaining_storage_byte_count: source_storage_byte_count - planned_storage_byte_count,
        lifetimes: vec![
            dialogue_lifetime_report(
                SCREEN_ROLE,
                CHAPTER_ONE_INDEX,
                &chapter_one_plans,
                &chapter_one_encoded_records,
                &chapter_one_page,
            ),
            dialogue_lifetime_report(
                CHAPTER_TWO_SCREEN_ROLE,
                CHAPTER_TWO_INDEX,
                &chapter_two_plans,
                &chapter_two_encoded_records,
                &chapter_two_page,
            ),
            shop_dialogue_lifetime_report(
                &shop_dialogue_plan,
                &shop_dialogue_stage.page,
                shop_dialogue_runtime.as_ref(),
            ),
        ],
        maximum_page_reloaded_lifetime: CumulativeMaximumDialogueReport {
            screen_role: MAXIMUM_DIALOGUE_SCREEN_ROLE,
            target_record_id: maximum_dialogue_plan.record_id.clone(),
            workspace_sha1: maximum_dialogue_plan.workspace_sha1.clone(),
            record_page_boundary_topology_sha1: maximum_dialogue_stage
                .page
                .record_page_boundary_topology_sha1
                .clone(),
            screen_evidence_manifest_sha1: maximum_dialogue_stage
                .page
                .evidence_manifest_sha1
                .clone(),
            page_boundary_manifest_sha1: maximum_dialogue_stage
                .page
                .page_boundary_manifest_sha1
                .clone(),
            page_boundary_observation_output_sha1: maximum_dialogue_stage
                .page
                .boundary_observation_output_sha1
                .clone(),
            installed_translated_line_count: maximum_dialogue_plan.translated_line_count,
            source_storage_byte_count: maximum_dialogue_plan.source_storage_byte_count,
            planned_storage_byte_count: maximum_dialogue_stage.page.encoded_record.len(),
            remaining_storage_byte_count: maximum_dialogue_plan.source_storage_byte_count
                - maximum_dialogue_stage.page.encoded_record.len(),
            completed_page_count: MAXIMUM_DIALOGUE_PAGE_COUNT,
            display_lines_per_page: MAXIMUM_DIALOGUE_LINES_PER_PAGE,
            font_group_count: maximum_dialogue_stage.page.assignments.len(),
            page_group_indices: maximum_dialogue_stage.page.page_groups.clone(),
            group_page_counts: maximum_dialogue_stage.page.group_page_counts.clone(),
            group_unique_glyph_counts: maximum_dialogue_stage
                .page
                .group_unique_glyph_counts
                .clone(),
            glyph_assignment_sha1s: maximum_dialogue_stage
                .page
                .assignments
                .iter()
                .map(assignment_sha1)
                .collect(),
            preserved_screen_active_code_count: maximum_dialogue_stage
                .page
                .preserved_screen_active_code_count,
            preserved_source_active_code_count: maximum_dialogue_stage
                .page
                .preserved_source_active_code_count,
            preserved_active_code_count: maximum_dialogue_stage.page.preserved_active_code_count,
            temporal_sample_count: maximum_dialogue_stage.page.temporal_sample_count,
            unique_nametable_count: maximum_dialogue_stage.page.unique_nametable_count,
            font_physical_pages: maximum_dialogue_stage.page.physical_chr_pages.clone(),
            font_mapper_registers: maximum_dialogue_stage.page.mapper_registers.clone(),
            font_page_sha1s: maximum_dialogue_stage.page.font_page_sha1s.clone(),
            font_page_pack_sha1: sha1_hex(&maximum_dialogue_stage.page.page_pack),
            completed_page_pointers_hex: maximum_dialogue_stage
                .page
                .completed_page_pointers
                .iter()
                .map(|pointer| format!("0x{pointer:04X}"))
                .collect(),
            group_transition_pointers_hex: maximum_dialogue_stage
                .page
                .group_transition_pointers
                .iter()
                .map(|pointer| format!("0x{pointer:04X}"))
                .collect(),
            initial_selector_byte_count: maximum_dialogue_stage.initial_selector_byte_count,
            font_group_selector_byte_count: maximum_dialogue_stage.font_group_selector_byte_count,
            completed_page_transition_byte_count: maximum_dialogue_stage
                .completed_page_transition_byte_count,
            completed_page_reload_installed: true,
            final_page_exit_bypasses_reload: true,
            original_english_and_digits_preserved: true,
            runtime_evidence_manifest_sha1: maximum_dialogue_runtime
                .as_ref()
                .map(|runtime| runtime.manifest_sha1.clone()),
            runtime_sample_count: maximum_dialogue_runtime
                .as_ref()
                .map_or(0, |runtime| runtime.sample_count),
            runtime_page_count: maximum_dialogue_runtime
                .as_ref()
                .map_or(0, |runtime| runtime.page_count),
            runtime_unique_nametable_count: maximum_dialogue_runtime
                .as_ref()
                .map_or(0, |runtime| runtime.unique_nametable_count),
            runtime_temporal_screen_count: maximum_dialogue_runtime
                .as_ref()
                .map_or(0, |runtime| runtime.temporal_screen_count),
            runtime_pages_with_visual_phase_change: maximum_dialogue_runtime
                .as_ref()
                .map_or(0, |runtime| runtime.pages_with_visual_phase_change),
            runtime_visual_review_passed: maximum_dialogue_runtime
                .as_ref()
                .is_some_and(|runtime| runtime.visual_review_passed),
            initial_selector_runtime_bound_to_build: maximum_dialogue_runtime
                .as_ref()
                .is_some_and(|runtime| runtime.initial_selector_observed),
            page_reload_runtime_bound_to_build: maximum_dialogue_runtime
                .as_ref()
                .is_some_and(|runtime| runtime.page_reload_bound_to_build),
            final_exit_runtime_bound_to_build: maximum_dialogue_runtime
                .as_ref()
                .is_some_and(|runtime| runtime.final_exit_bound_to_build),
            runtime_bound_to_build: maximum_dialogue_runtime.as_ref().is_some_and(|runtime| {
                runtime.initial_selector_observed
                    && runtime.page_reload_bound_to_build
                    && runtime.final_exit_bound_to_build
            }),
        },
    }
}

pub(in crate::mapper165::cumulative_patch) fn dialogue_lifetime_report(
    screen_role: &'static str,
    chapter_index: u8,
    plans: &[MainDialogueSlicePlan],
    encoded_records: &[Vec<u8>],
    page: &DialogueLifetimePagePlan,
) -> CumulativeDialogueLifetimeReport {
    let installed_translated_line_count = plans
        .iter()
        .map(|plan| plan.translated_line_count)
        .sum::<usize>();
    let source_storage_byte_count = plans
        .iter()
        .map(|plan| plan.source_storage_byte_count)
        .sum::<usize>();
    let planned_storage_byte_count = encoded_records.iter().map(Vec::len).sum::<usize>();

    CumulativeDialogueLifetimeReport {
        screen_role,
        chapter_index,
        screen_evidence_manifest_sha1: page.manifest_sha1.clone(),
        installed_record_count: plans.len(),
        installed_translated_line_count,
        source_storage_byte_count,
        planned_storage_byte_count,
        remaining_storage_byte_count: source_storage_byte_count - planned_storage_byte_count,
        unique_glyph_count: page.assignments.len(),
        glyph_assignment_sha1: assignment_sha1(&page.assignments),
        preserved_screen_active_code_count: page.preserved_screen_active_code_count,
        preserved_source_active_code_count: page.preserved_source_active_code_count,
        preserved_active_code_count: page.preserved_active_code_count,
        temporal_sample_count: page.temporal_sample_count,
        unique_nametable_count: page.unique_nametable_count,
        font_physical_page: page.physical_chr_page,
        font_mapper_register: page.mapper_register,
        font_page_sha1: page.page_sha1.clone(),
        font_page_pack_sha1: sha1_hex(&page.page_pack),
        runtime_evidence_manifest_sha1: None,
        runtime_sample_count: 0,
        runtime_unique_image_count: 0,
        runtime_bound_to_dialogue_stage_output: false,
    }
}

pub(in crate::mapper165::cumulative_patch) fn shop_dialogue_lifetime_report(
    plan: &MainDialogueBundlePlan,
    page: &ShopDialoguePagePlan,
    runtime: Option<&ShopDialogueRuntimeEvidence>,
) -> CumulativeDialogueLifetimeReport {
    CumulativeDialogueLifetimeReport {
        screen_role: SHOP_DIALOGUE_SCREEN_ROLE,
        chapter_index: CHAPTER_ONE_INDEX,
        screen_evidence_manifest_sha1: page.manifest_sha1.clone(),
        installed_record_count: plan.record_ids.len(),
        installed_translated_line_count: plan.translated_line_count,
        source_storage_byte_count: plan.source_record_storage_byte_count,
        planned_storage_byte_count: plan.planned_record_storage_byte_count,
        remaining_storage_byte_count: plan.source_record_storage_byte_count
            - plan.planned_record_storage_byte_count,
        unique_glyph_count: page.assignments.len(),
        glyph_assignment_sha1: assignment_sha1(&page.assignments),
        preserved_screen_active_code_count: page.preserved_screen_active_code_count,
        preserved_source_active_code_count: page.preserved_source_active_code_count,
        preserved_active_code_count: page.preserved_active_code_count,
        temporal_sample_count: page.sample_count,
        unique_nametable_count: page.unique_nametable_count,
        font_physical_page: page.physical_chr_page,
        font_mapper_register: page.mapper_register,
        font_page_sha1: page.page_sha1.clone(),
        font_page_pack_sha1: sha1_hex(&page.page_pack),
        runtime_evidence_manifest_sha1: runtime.map(|runtime| runtime.manifest_sha1.clone()),
        runtime_sample_count: runtime.map_or(0, |runtime| runtime.sample_count),
        runtime_unique_image_count: runtime.map_or(0, |runtime| runtime.unique_image_count),
        runtime_bound_to_dialogue_stage_output: runtime.is_some(),
    }
}
