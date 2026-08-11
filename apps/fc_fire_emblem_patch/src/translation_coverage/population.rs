use std::{collections::BTreeMap, fs, path::Path};

use anyhow::{Context, Result, ensure};

use crate::{
    chapter_transition::{plan_chapter_titles, plan_transition_labels},
    choice_labels::plan_choice_labels,
    class_profile::plan_class_profiles,
    dialogue_assets::{validate_battle_dialogue_workspace, validate_main_dialogue_workspace},
    front_end_menu::plan_front_end_menu,
    item_flow::plan_item_action_labels,
    localization::OptionsLocalization,
    map_menu::plan_map_menu,
    rom::Rom,
    roster_localization::RosterLocalization,
    sha1_hex,
    suspend_message::bind_suspend_message_to_main_dialogue,
    text_inventory::{FixedTextPlan, plan_fixed_text, plan_location_name_text},
    title_graphics::plan_title_graphics,
    unit_names::plan_unit_names,
    unit_ui_text::plan_unit_ui_labels,
};

use super::report::{DomainPopulation, SourceBindingState, TranslationInputState};

pub(crate) struct TranslationPopulationInputs<'a> {
    pub(crate) source_path: &'a Path,
    pub(crate) main_dialogue_workspace_path: &'a Path,
    pub(crate) battle_dialogue_workspace_path: &'a Path,
    pub(crate) fixed_text_workspace_path: &'a Path,
    pub(crate) options_localization_path: &'a Path,
    pub(crate) roster_localization_path: &'a Path,
    pub(crate) front_end_menu_localization_path: &'a Path,
    pub(crate) unit_name_localization_path: &'a Path,
    pub(crate) class_profile_localization_path: &'a Path,
    pub(crate) chapter_title_localization_path: &'a Path,
    pub(crate) choice_label_localization_path: &'a Path,
    pub(crate) map_menu_localization_path: &'a Path,
    pub(crate) title_graphics_localization_path: &'a Path,
    pub(crate) unit_ui_label_localization_path: &'a Path,
    pub(crate) item_action_label_localization_path: &'a Path,
    pub(crate) transition_label_localization_path: &'a Path,
    pub(crate) location_name_localization_path: &'a Path,
}

pub(crate) fn inspect_translation_populations(
    inputs: &TranslationPopulationInputs<'_>,
) -> Result<BTreeMap<&'static str, DomainPopulation>> {
    let rom = Rom::from_path(inputs.source_path)?;
    rom.verify_supported_japanese()?;

    let main_dialogue =
        validate_main_dialogue_workspace(inputs.source_path, inputs.main_dialogue_workspace_path)?;
    let battle_dialogue = validate_battle_dialogue_workspace(
        inputs.source_path,
        inputs.battle_dialogue_workspace_path,
    )?;
    let fixed_text = plan_fixed_text(&rom, inputs.fixed_text_workspace_path)?;
    let front_end = plan_front_end_menu(&rom, inputs.front_end_menu_localization_path)?;
    let unit_names = plan_unit_names(&rom, inputs.unit_name_localization_path)?;
    let class_profiles = plan_class_profiles(&rom, inputs.class_profile_localization_path)?;
    let chapter_titles = plan_chapter_titles(&rom, inputs.chapter_title_localization_path)?;
    let choice_labels = plan_choice_labels(&rom, inputs.choice_label_localization_path)?;
    let map_menu = plan_map_menu(&rom, inputs.map_menu_localization_path)?;
    let title_graphics = plan_title_graphics(&rom, inputs.title_graphics_localization_path)?;
    let unit_ui_labels = plan_unit_ui_labels(&rom, inputs.unit_ui_label_localization_path)?;
    let item_action_labels =
        plan_item_action_labels(&rom, inputs.item_action_label_localization_path)?;
    let transition_labels =
        plan_transition_labels(&rom, inputs.transition_label_localization_path)?;
    let location_names = plan_location_name_text(&rom, inputs.location_name_localization_path)?;
    bind_suspend_message_to_main_dialogue(&rom)?;
    validate_duplicate_unit_name_inputs(&fixed_text, &unit_names.entries)?;

    let options_bytes = fs::read(inputs.options_localization_path).with_context(|| {
        format!(
            "read options localization {}",
            inputs.options_localization_path.display()
        )
    })?;
    let options = OptionsLocalization::from_path(inputs.options_localization_path)?;
    let validated_options = options.validate()?;
    let roster_bytes = fs::read(inputs.roster_localization_path).with_context(|| {
        format!(
            "read roster localization {}",
            inputs.roster_localization_path.display()
        )
    })?;
    let validated_roster =
        RosterLocalization::from_path(inputs.roster_localization_path)?.validate()?;

    let mut populations = BTreeMap::new();
    insert(
        &mut populations,
        "main_dialogue",
        complete_or_partial(
            main_dialogue.filled_line_count + main_dialogue.untranslated_japanese_line_count,
            main_dialogue.filled_line_count,
            main_dialogue.translation_input_complete,
            main_dialogue.review_complete,
            Some(main_dialogue.workspace_sha1),
        ),
    )?;
    insert(
        &mut populations,
        "battle_dialogue",
        complete_or_partial(
            battle_dialogue.filled_line_count + battle_dialogue.untranslated_japanese_line_count,
            battle_dialogue.filled_line_count,
            battle_dialogue.translation_input_complete,
            battle_dialogue.review_complete,
            Some(battle_dialogue.workspace_sha1),
        ),
    )?;

    for (domain_id, table_id) in [
        ("class_names", "class-names"),
        ("item_names", "item-names"),
        ("enemy_names", "enemy-names"),
        ("terrain_names", "terrain-names"),
        ("battle_message_templates", "battle-message-templates"),
    ] {
        insert(
            &mut populations,
            domain_id,
            fixed_text_population(&fixed_text, table_id)?,
        )?;
    }

    insert(
        &mut populations,
        "front_end_menu_labels",
        completed_population(
            front_end.entries.len(),
            front_end.review_complete,
            Some(front_end.workspace_sha1),
        ),
    )?;
    insert(
        &mut populations,
        "unit_names",
        completed_population(
            unit_names.entries.len(),
            unit_names.review_complete,
            Some(unit_names.workspace_sha1),
        ),
    )?;
    insert(
        &mut populations,
        "class_profiles",
        completed_population(
            class_profiles.entries.len(),
            class_profiles.review_complete,
            Some(class_profiles.workspace_sha1),
        ),
    )?;
    insert(
        &mut populations,
        "chapter_titles",
        complete_or_partial(
            chapter_titles.entry_count,
            chapter_titles.translated_entry_count,
            chapter_titles.translated_entry_count == chapter_titles.entry_count,
            chapter_titles.review_complete,
            Some(chapter_titles.workspace_sha1),
        ),
    )?;
    insert(
        &mut populations,
        "choice_labels",
        completed_population(
            choice_labels.entries.len(),
            choice_labels.review_complete,
            Some(choice_labels.workspace_sha1),
        ),
    )?;
    insert(
        &mut populations,
        "map_menu_labels",
        complete_or_partial(
            map_menu.entry_count,
            map_menu.translated_entry_count,
            map_menu.translated_entry_count == map_menu.entry_count,
            map_menu.review_complete,
            Some(map_menu.workspace_sha1),
        ),
    )?;
    insert(
        &mut populations,
        "title_graphics",
        complete_or_partial(
            1,
            title_graphics.translated_surface_count,
            title_graphics.translated_surface_count == 1,
            title_graphics.review_complete,
            Some(title_graphics.workspace_sha1),
        ),
    )?;
    insert(
        &mut populations,
        "options_labels",
        completed_population(
            validated_options.entries.len(),
            validated_options.review_complete,
            Some(sha1_hex(&options_bytes)),
        ),
    )?;
    insert(
        &mut populations,
        "roster_header",
        completed_population(
            1,
            validated_roster.review_complete,
            Some(sha1_hex(&roster_bytes)),
        ),
    )?;
    insert(
        &mut populations,
        "battle_forecast_label",
        completed_population(
            transition_labels.forecast.entry_count,
            transition_labels.forecast.review_complete,
            Some(transition_labels.forecast.workspace_sha1),
        ),
    )?;
    for (domain_id, plan) in [
        ("unit_ui_labels", unit_ui_labels),
        ("item_action_labels", item_action_labels),
    ] {
        insert(
            &mut populations,
            domain_id,
            completed_population(
                plan.entry_count,
                plan.review_complete,
                Some(plan.workspace_sha1),
            ),
        )?;
    }
    for (domain_id, plan) in [
        ("chapter_save_offer_label", transition_labels.save_offer),
        ("ending_record_labels", transition_labels.ending_record),
    ] {
        insert(
            &mut populations,
            domain_id,
            completed_population(
                plan.entry_count,
                plan.review_complete,
                Some(plan.workspace_sha1),
            ),
        )?;
    }
    insert(
        &mut populations,
        "location_names",
        completed_population(
            location_names.entries.len(),
            location_names.review_complete,
            Some(location_names.workspace_sha1),
        ),
    )?;

    Ok(populations)
}

fn validate_duplicate_unit_name_inputs(
    fixed_text: &FixedTextPlan,
    public_entries: &[crate::text_inventory::FixedTextPlannedEntry],
) -> Result<()> {
    let fixed_entries = fixed_text
        .entries
        .iter()
        .filter(|entry| entry.table_id == "unit-names")
        .collect::<Vec<_>>();
    ensure!(
        fixed_entries.len() == public_entries.len(),
        "public and battle unit-name translation populations differ"
    );
    for (fixed, public) in fixed_entries.into_iter().zip(public_entries) {
        ensure!(
            fixed.source_index == public.source_index
                && fixed.alias_indices == public.alias_indices
                && fixed.logical_bytes == public.logical_bytes,
            "public and battle unit-name translations diverge at source index {}",
            public.source_index
        );
    }
    Ok(())
}

fn fixed_text_population(plan: &FixedTextPlan, table_id: &str) -> Result<DomainPopulation> {
    let entries = plan
        .entries
        .iter()
        .filter(|entry| entry.table_id == table_id)
        .collect::<Vec<_>>();
    ensure!(!entries.is_empty(), "fixed text lost table {table_id}");
    Ok(completed_population(
        entries.len(),
        entries.iter().all(|entry| entry.review_complete),
        Some(plan.workspace_sha1.clone()),
    ))
}

fn completed_population(
    count: usize,
    review_complete: bool,
    translation_input_sha1: Option<String>,
) -> DomainPopulation {
    DomainPopulation {
        source_binding: SourceBindingState::Bound,
        target_unit_count: Some(count),
        translated_target_unit_count: count,
        translation_input: TranslationInputState::Complete,
        review_complete,
        translation_input_sha1,
    }
}

fn complete_or_partial(
    count: usize,
    translated_count: usize,
    input_complete: bool,
    review_complete: bool,
    translation_input_sha1: Option<String>,
) -> DomainPopulation {
    DomainPopulation {
        source_binding: SourceBindingState::Bound,
        target_unit_count: Some(count),
        translated_target_unit_count: translated_count,
        translation_input: if input_complete {
            TranslationInputState::Complete
        } else if translated_count == 0 {
            TranslationInputState::Missing
        } else {
            TranslationInputState::Partial
        },
        review_complete,
        translation_input_sha1,
    }
}

fn insert(
    populations: &mut BTreeMap<&'static str, DomainPopulation>,
    domain_id: &'static str,
    population: DomainPopulation,
) -> Result<()> {
    ensure!(
        populations.insert(domain_id, population).is_none(),
        "translation population repeats domain {domain_id}"
    );
    Ok(())
}
