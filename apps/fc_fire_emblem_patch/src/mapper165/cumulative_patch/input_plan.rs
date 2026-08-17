use std::collections::BTreeSet;

use anyhow::{Result, ensure};

use crate::{
    chapter_transition::{ChapterTitlePlan, plan_chapter_titles},
    choice_labels::{ChoiceLabelPlan, plan_choice_labels},
    class_profile::{ClassProfilePlan, plan_class_profiles},
    dialogue_assets::{
        DialogueWorkspaceValidationSummary, MainDialogueBundlePlan, MainDialogueSlicePlan,
        plan_main_dialogue_bundle, plan_main_dialogue_slice, validate_main_dialogue_workspace,
    },
    front_end_menu::{FRONT_END_RESULT_DIALOGUE_RECORD_IDS, FrontEndMenuPlan, plan_front_end_menu},
    rom::Rom,
    text_inventory::{FixedTextPlan, plan_fixed_text},
    title_graphics::{TitleGraphicsPlan, plan_title_graphics},
    unit_names::{UnitNamePlan, plan_unit_names},
};

use super::super::{
    maximum_dialogue_page::TARGET_RECORD_ID as MAXIMUM_DIALOGUE_RECORD_ID,
    shop_dialogue_page::RECORD_IDS as SHOP_DIALOGUE_RECORD_IDS,
};
use super::CumulativePatchInputs;

pub(super) struct CumulativeInputPlan {
    pub(super) source_rom: Rom,
    pub(super) dialogue_workspace: DialogueWorkspaceValidationSummary,
    pub(super) chapter_title_plan: ChapterTitlePlan,
    pub(super) title_graphics_plan: TitleGraphicsPlan,
    pub(super) front_end_menu_plan: FrontEndMenuPlan,
    pub(super) front_end_result_preserved_codes: BTreeSet<u8>,
    pub(super) unit_name_plan: UnitNamePlan,
    pub(super) class_profile_plan: ClassProfilePlan,
    pub(super) fixed_text_plan: FixedTextPlan,
    pub(super) choice_label_plan: ChoiceLabelPlan,
    pub(super) shop_dialogue_plan: MainDialogueBundlePlan,
    pub(super) maximum_dialogue_plan: MainDialogueSlicePlan,
}

pub(super) fn prepare_cumulative_inputs(
    inputs: &CumulativePatchInputs<'_>,
) -> Result<CumulativeInputPlan> {
    let source_rom = Rom::from_path(inputs.source_path)?;
    source_rom.verify_supported_japanese()?;
    let dialogue_workspace =
        validate_main_dialogue_workspace(inputs.source_path, inputs.main_dialogue_workspace_path)?;
    ensure!(
        dialogue_workspace.translation_input_complete,
        "cumulative build requires complete Japanese-to-Korean dialogue input"
    );
    let chapter_title_plan =
        plan_chapter_titles(&source_rom, inputs.chapter_title_localization_path)?;
    ensure!(
        chapter_title_plan.translated_entry_count == chapter_title_plan.entry_count,
        "cumulative build requires complete Japanese-to-Korean chapter-title input"
    );
    let title_graphics_plan =
        plan_title_graphics(&source_rom, inputs.title_graphics_localization_path)?;
    ensure!(
        title_graphics_plan.translated_surface_count == 1,
        "cumulative build requires one translated Korean title-logo surface"
    );
    let front_end_menu_plan =
        plan_front_end_menu(&source_rom, inputs.front_end_menu_localization_path)?;
    ensure!(
        front_end_menu_plan.entries.len() == 7,
        "cumulative front-end menu scope no longer has seven entries"
    );
    let front_end_result_dialogue_plan = plan_main_dialogue_bundle(
        &source_rom,
        inputs.main_dialogue_workspace_path,
        &FRONT_END_RESULT_DIALOGUE_RECORD_IDS,
    )?;
    ensure!(
        front_end_result_dialogue_plan.workspace_sha1 == dialogue_workspace.workspace_sha1
            && front_end_result_dialogue_plan
                .record_ids
                .iter()
                .map(String::as_str)
                .eq(FRONT_END_RESULT_DIALOGUE_RECORD_IDS),
        "front-end result dialogue plan no longer matches its validated workspace population"
    );
    let front_end_result_preserved_codes = front_end_result_dialogue_plan
        .page_worksets
        .iter()
        .flat_map(|page| page.preserved_target_active_codes.iter().copied())
        .collect::<BTreeSet<_>>();
    ensure!(
        !front_end_result_preserved_codes.is_empty(),
        "front-end result dialogue pages have no protected active codes"
    );
    let unit_name_plan = plan_unit_names(&source_rom, inputs.unit_name_localization_path)?;
    let class_profile_plan =
        plan_class_profiles(&source_rom, inputs.class_profile_localization_path)?;
    let fixed_text_plan = plan_fixed_text(&source_rom, inputs.fixed_text_workspace_path)?;
    let choice_label_plan = plan_choice_labels(&source_rom, inputs.choice_label_localization_path)?;
    let shop_dialogue_plan = plan_main_dialogue_bundle(
        &source_rom,
        inputs.main_dialogue_workspace_path,
        &SHOP_DIALOGUE_RECORD_IDS,
    )?;
    ensure!(
        shop_dialogue_plan.workspace_sha1 == dialogue_workspace.workspace_sha1,
        "weapon-shop dialogue plans no longer match the validated workspace"
    );
    ensure!(
        shop_dialogue_plan
            .record_ids
            .iter()
            .map(String::as_str)
            .eq(SHOP_DIALOGUE_RECORD_IDS),
        "weapon-shop dialogue plan order changed"
    );
    let maximum_dialogue_plan = plan_main_dialogue_slice(
        &source_rom,
        inputs.main_dialogue_workspace_path,
        MAXIMUM_DIALOGUE_RECORD_ID,
    )?;
    ensure!(
        maximum_dialogue_plan.workspace_sha1 == dialogue_workspace.workspace_sha1
            && maximum_dialogue_plan.transition_chain_record_count == 1,
        "maximum dialogue plan no longer matches its validated single-record lifetime"
    );

    Ok(CumulativeInputPlan {
        source_rom,
        dialogue_workspace,
        chapter_title_plan,
        title_graphics_plan,
        front_end_menu_plan,
        front_end_result_preserved_codes,
        unit_name_plan,
        class_profile_plan,
        fixed_text_plan,
        choice_label_plan,
        shop_dialogue_plan,
        maximum_dialogue_plan,
    })
}
