use std::{fs, path::Path};

use anyhow::{Context, Result, ensure};
use serde::Serialize;

use crate::{
    chapter_transition::{plan_chapter_titles, plan_transition_labels},
    choice_labels::plan_choice_labels,
    dialogue_assets::{plan_all_main_dialogue_records, validate_main_dialogue_workspace},
    item_flow::plan_item_action_labels,
    map_menu::plan_map_menu,
    mapper165::battle_codebook_plan::{GlyphWorkset, plan_glyph_workset_codebook},
    rom::{EXPECTED_SOURCE_SHA1, Rom},
    sha1_hex,
    text_inventory::{plan_fixed_text, plan_location_name_text},
    unit_names::plan_unit_names,
    unit_ui_text::plan_unit_ui_labels,
};

const REQUIRED_DOMAIN_COUNT: usize = 13;

pub(crate) struct FullTranslationInstallInputs<'a> {
    pub(crate) source_path: &'a Path,
    pub(crate) main_dialogue_workspace_path: &'a Path,
    pub(crate) fixed_text_workspace_path: &'a Path,
    pub(crate) unit_name_localization_path: &'a Path,
    pub(crate) chapter_title_localization_path: &'a Path,
    pub(crate) choice_label_localization_path: &'a Path,
    pub(crate) map_menu_localization_path: &'a Path,
    pub(crate) unit_ui_label_localization_path: &'a Path,
    pub(crate) item_action_label_localization_path: &'a Path,
    pub(crate) transition_label_localization_path: &'a Path,
    pub(crate) location_name_localization_path: &'a Path,
    pub(crate) report_path: &'a Path,
}

pub(crate) struct FullTranslationInstallSummary {
    pub(crate) report_sha1: String,
    pub(crate) required_domain_count: usize,
    pub(crate) dialogue_record_count: usize,
    pub(crate) dialogue_page_workset_count: usize,
    pub(crate) dialogue_glyph_count: usize,
    pub(crate) dialogue_stable_color_count: usize,
    pub(crate) dialogue_pointer_write_count: usize,
    pub(crate) dialogue_planned_storage_byte_count: usize,
}

#[derive(Serialize)]
struct FullTranslationInstallReport {
    schema: u8,
    source_sha1: &'static str,
    strategy: &'static str,
    required_domain_count: usize,
    required_domains: [&'static str; REQUIRED_DOMAIN_COUNT],
    translation_inputs: TranslationInputs,
    dialogue_codebook: DialogueCodebook,
    dialogue_storage: DialogueStorage,
    installation_gates: InstallationGates,
    rom_emitted: bool,
    dynamic_verification_started: bool,
    next_gate: &'static str,
}

#[derive(Serialize)]
struct TranslationInputs {
    main_dialogue_record_count: usize,
    fixed_text_physical_entry_count: usize,
    playable_unit_name_count: usize,
    chapter_title_count: usize,
    choice_label_count: usize,
    map_menu_label_count: usize,
    unit_ui_label_count: usize,
    item_action_label_count: usize,
    transition_label_count: usize,
    location_name_count: usize,
    translation_input_complete: bool,
    review_complete: bool,
}

#[derive(Serialize)]
struct DialogueCodebook {
    page_workset_count: usize,
    unique_glyph_count: usize,
    conflict_edge_count: usize,
    constructed_clique_glyph_count: usize,
    stable_color_count: usize,
    active_slot_count: usize,
    active_ceiling_assignment_found: bool,
    constrained_color_count: usize,
    abstract_assignment_sha1: String,
    physical_assignment_sha1: String,
    encoded_bundle_verified: bool,
    glyph_characters_emitted: bool,
}

#[derive(Serialize)]
struct DialogueStorage {
    region_count: usize,
    record_count: usize,
    pointer_write_count: usize,
    source_owned_storage_byte_count: usize,
    planned_storage_byte_count: usize,
    remaining_storage_byte_count: usize,
    every_pointer_within_source_owned_regions: bool,
}

#[derive(Serialize)]
struct InstallationGates {
    all_translation_inputs_loaded: bool,
    all_dialogue_records_encoded: bool,
    all_dialogue_pointers_planned: bool,
    all_dialogue_page_code_assignments_found: bool,
    cross_domain_consumer_writes_planned: bool,
    integrated_candidate_ready: bool,
}

pub(crate) fn plan_full_translation_installation(
    inputs: FullTranslationInstallInputs<'_>,
) -> Result<FullTranslationInstallSummary> {
    let rom = Rom::from_path(inputs.source_path)?;
    rom.verify_supported_japanese()?;

    let dialogue_validation =
        validate_main_dialogue_workspace(inputs.source_path, inputs.main_dialogue_workspace_path)?;
    ensure!(
        dialogue_validation.translation_input_complete,
        "all-record main dialogue installation still has untranslated Japanese"
    );
    let dialogue = plan_all_main_dialogue_records(&rom, inputs.main_dialogue_workspace_path)?;
    let fixed = plan_fixed_text(&rom, inputs.fixed_text_workspace_path)?;
    let unit_names = plan_unit_names(&rom, inputs.unit_name_localization_path)?;
    let chapter_titles = plan_chapter_titles(&rom, inputs.chapter_title_localization_path)?;
    let choices = plan_choice_labels(&rom, inputs.choice_label_localization_path)?;
    let map_menu = plan_map_menu(&rom, inputs.map_menu_localization_path)?;
    let unit_ui = plan_unit_ui_labels(&rom, inputs.unit_ui_label_localization_path)?;
    let item_actions = plan_item_action_labels(&rom, inputs.item_action_label_localization_path)?;
    let transitions = plan_transition_labels(&rom, inputs.transition_label_localization_path)?;
    let locations = plan_location_name_text(&rom, inputs.location_name_localization_path)?;

    ensure!(
        dialogue.record_ids.len() == 504
            && fixed.entries.len() == 272
            && unit_names.entries.len() == 52
            && chapter_titles.entry_count == 25
            && chapter_titles.translated_entry_count == 25
            && choices.entries.len() == 2
            && map_menu.entry_count == 6
            && map_menu.translated_entry_count == 6
            && unit_ui.entry_count == 25
            && item_actions.entry_count == 4
            && transitions.save_offer.entry_count == 1
            && transitions.ending_record.entry_count == 1
            && locations.entries.len() == 24,
        "full translation installation input population changed"
    );

    let worksets = dialogue
        .page_worksets
        .iter()
        .map(|workset| GlyphWorkset {
            target_glyphs: workset.target_glyphs.clone(),
            preserved_active_codes: workset.preserved_target_active_codes.clone(),
        })
        .collect::<Vec<_>>();
    let codebook = plan_glyph_workset_codebook(&worksets)?;
    ensure!(
        codebook.workset_count == dialogue.page_worksets.len(),
        "dialogue codebook lost visible page worksets"
    );
    let encoded = dialogue.encoded(&codebook.glyph_codes)?;
    ensure!(
        encoded.regions.len() == 11 && encoded.pointer_writes.len() == 517,
        "complete dialogue encoded layout changed"
    );
    let source_owned_storage_byte_count = encoded
        .regions
        .iter()
        .map(|region| region.source_storage.len())
        .sum::<usize>();
    let planned_storage_byte_count = encoded
        .regions
        .iter()
        .map(|region| region.used_storage_byte_count)
        .sum::<usize>();
    ensure!(
        planned_storage_byte_count <= source_owned_storage_byte_count,
        "complete dialogue encoded storage exceeds its source-owned regions"
    );
    let review_complete = dialogue_validation.review_complete
        && fixed.review_complete
        && unit_names.review_complete
        && chapter_titles.review_complete
        && choices.review_complete
        && map_menu.review_complete
        && unit_ui.review_complete
        && item_actions.review_complete
        && transitions.save_offer.review_complete
        && transitions.ending_record.review_complete
        && locations.review_complete;

    let report = FullTranslationInstallReport {
        schema: 1,
        source_sha1: EXPECTED_SOURCE_SHA1,
        strategy: "install all remaining translation domains in one cumulative candidate, run complete static gates, then run consumer-path dynamic regression on that same ROM",
        required_domain_count: REQUIRED_DOMAIN_COUNT,
        required_domains: [
            "chapter_save_offer_label",
            "chapter_titles",
            "choice_labels",
            "class_names",
            "ending_record_labels",
            "enemy_names",
            "item_action_labels",
            "item_names",
            "location_names",
            "main_dialogue",
            "map_menu_labels",
            "unit_names",
            "unit_ui_labels",
        ],
        translation_inputs: TranslationInputs {
            main_dialogue_record_count: dialogue.record_ids.len(),
            fixed_text_physical_entry_count: fixed.entries.len(),
            playable_unit_name_count: unit_names.entries.len(),
            chapter_title_count: chapter_titles.entry_count,
            choice_label_count: choices.entries.len(),
            map_menu_label_count: map_menu.entry_count,
            unit_ui_label_count: unit_ui.entry_count,
            item_action_label_count: item_actions.entry_count,
            transition_label_count: transitions.save_offer.entry_count
                + transitions.ending_record.entry_count,
            location_name_count: locations.entries.len(),
            translation_input_complete: true,
            review_complete,
        },
        dialogue_codebook: DialogueCodebook {
            page_workset_count: dialogue.page_worksets.len(),
            unique_glyph_count: codebook.glyph_count,
            conflict_edge_count: codebook.conflict_edge_count,
            constructed_clique_glyph_count: codebook.constructed_clique_glyph_count,
            stable_color_count: codebook.stable_color_count,
            active_slot_count: crate::font_slots::ACTIVE_HANGUL_SLOT_COUNT,
            active_ceiling_assignment_found: codebook.active_ceiling_assignment_found,
            constrained_color_count: codebook.constrained_color_count,
            abstract_assignment_sha1: codebook.abstract_assignment_sha1,
            physical_assignment_sha1: codebook.physical_assignment_sha1,
            encoded_bundle_verified: true,
            glyph_characters_emitted: false,
        },
        dialogue_storage: DialogueStorage {
            region_count: encoded.regions.len(),
            record_count: dialogue.record_ids.len(),
            pointer_write_count: encoded.pointer_writes.len(),
            source_owned_storage_byte_count,
            planned_storage_byte_count,
            remaining_storage_byte_count: source_owned_storage_byte_count
                - planned_storage_byte_count,
            every_pointer_within_source_owned_regions: true,
        },
        installation_gates: InstallationGates {
            all_translation_inputs_loaded: true,
            all_dialogue_records_encoded: true,
            all_dialogue_pointers_planned: true,
            all_dialogue_page_code_assignments_found: true,
            cross_domain_consumer_writes_planned: false,
            integrated_candidate_ready: false,
        },
        rom_emitted: false,
        dynamic_verification_started: false,
        next_gate: "connect the complete encoded dialogue bundle and every remaining UI consumer to one cumulative Expected Write plan; do not emit or run a partial ROM",
    };
    let mut report_bytes =
        serde_json::to_vec_pretty(&report).context("serialize full translation install plan")?;
    report_bytes.push(b'\n');
    if let Some(parent) = inputs.report_path.parent() {
        fs::create_dir_all(parent).with_context(|| format!("create {}", parent.display()))?;
    }
    fs::write(inputs.report_path, &report_bytes)
        .with_context(|| format!("write {}", inputs.report_path.display()))?;

    Ok(FullTranslationInstallSummary {
        report_sha1: sha1_hex(&report_bytes),
        required_domain_count: REQUIRED_DOMAIN_COUNT,
        dialogue_record_count: dialogue.record_ids.len(),
        dialogue_page_workset_count: dialogue.page_worksets.len(),
        dialogue_glyph_count: codebook.glyph_count,
        dialogue_stable_color_count: codebook.stable_color_count,
        dialogue_pointer_write_count: encoded.pointer_writes.len(),
        dialogue_planned_storage_byte_count: planned_storage_byte_count,
    })
}
