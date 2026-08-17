use std::{
    collections::{BTreeMap, BTreeSet},
    path::Path,
};

use anyhow::{Context, Result, ensure};

use crate::{
    chapter_transition::{plan_chapter_titles, plan_transition_labels},
    choice_labels::plan_choice_labels,
    dialogue_assets::{plan_all_main_dialogue_records, validate_main_dialogue_workspace},
    dialogue_inventory::inspect_main_dialogue_graph,
    fixed_menu_labels::plan_fixed_menu_labels,
    fixed_string_consumers::{FixedStringConsumerCensus, inspect_fixed_string_consumers},
    fixed_string_ownership::{FixedStringOwnershipReport, inspect_fixed_string_ownership},
    font_slots::FONT_PAGE_SIZE,
    front_end_menu::plan_front_end_menu,
    item_flow::plan_item_action_labels,
    localization::OptionsLocalization,
    map_menu::plan_map_menu,
    mapper165::{
        CarriedBattleDomainInputs, CarriedBattleDomainPreservation, CarriedUiDomainInputs,
        CarriedUiDomainPreservation,
        battle_codebook_plan::{
            build_glyph_workset_font_page_pack, plan_glyph_workset_page_upper_bound,
            verify_glyph_workset_font_page_pack,
        },
        bind_installed_front_end_mapper_register,
        font_pair_projection::RightFontPageProjection,
        inspect_carried_battle_domains, inspect_carried_ui_domains,
    },
    rom::{EXPECTED_SOURCE_SHA1, Rom},
    sha1_hex,
    text_inventory::{plan_fixed_text, plan_location_name_text},
    unit_names::plan_unit_names,
    unit_ui_text::{plan_unit_ui_labels, preserved_unit_ui_display_codes},
};

mod chapter_intro_residency;
mod chapter_save_projection;
mod choice_residency;
mod cold_request_presentation;
mod consumer_catalog;
mod consumer_codebook;
mod consumer_installation;
mod cross_domain_material;
mod current_candidate;
mod dialogue_bank_layout;
mod dynamic_composition;
mod dynamic_input_producers;
mod dynamic_inputs;
mod ending_record_projection;
mod final_runtime_evidence;
mod fixed_ui_projection;
mod front_end_result_residency;
mod installation_layout;
mod installation_readiness;
mod integrated_write_set;
mod main_dialogue_route_population;
mod report;
mod resident_glyph_assignment;
mod runtime_bank_contract;
mod runtime_code;
mod runtime_control_flow;
mod runtime_cursor_storage;
mod runtime_identity;
mod runtime_material;
mod runtime_nmi_contract;
mod runtime_state_storage;
mod screen_font_residency;
mod transition_residency;

use chapter_intro_residency::plan_chapter_intro_residency;
use chapter_save_projection::{
    ChapterSaveProjectionInputs, ChapterSaveProjectionPlan, plan_chapter_save_projection,
};
use choice_residency::{ChoiceResidencyPlan, plan_choice_residency};
use cold_request_presentation::plan_cold_request_presentation_page;
use consumer_catalog::{ConsumerCatalogInputs, ConsumerCatalogPlan, plan_consumer_catalog};
use consumer_codebook::{ConsumerCodebookInputs, ConsumerCodebookPlan, plan_consumer_codebook};
use consumer_installation::{
    ConsumerInstallationInputs, ConsumerInstallationPlan, plan_consumer_installation,
};
use cross_domain_material::{CrossDomainMaterialInputs, plan_cross_domain_material};
use current_candidate::{CurrentCandidateInputs, inspect_dialogue_page_pool_capacity};
use dynamic_composition::plan_dialogue_runtime_composition;
use dynamic_input_producers::{DynamicInputProducerPlan, inspect_dynamic_input_producers};
use dynamic_inputs::{
    DynamicProducerEncodingPlan, bind_dynamic_producer_encoding, bind_dynamic_string_page_codes,
    plan_dynamic_dialogue_inputs,
};
use ending_record_projection::{
    EndingRecordProjectionInputs, EndingRecordProjectionPlan, plan_ending_record_projection,
};
use final_runtime_evidence::{FinalArtifactRuntimeEvidence, load_final_artifact_runtime_evidence};
use fixed_ui_projection::{
    FixedUiProjectionInputs, FixedUiProjectionPlan, plan_fixed_ui_projection,
};
use front_end_result_residency::{
    FrontEndResultResidencyPlan, plan_front_end_result_menu_residency,
    plan_front_end_result_residency,
};
use installation_layout::{InstallationLayoutPlan, plan_installation_layout};
use installation_readiness::{InstallationReadiness, InstallationReadinessInputs};
use integrated_write_set::{
    IntegratedWriteSetInputs, IntegratedWriteSetPlan, plan_integrated_write_set,
};
use main_dialogue_route_population::{
    MainDialogueRoutePopulationPlan, plan_main_dialogue_route_population,
};
use report::{
    DialogueCodebookReportInputs, DialogueRuntimeCompositionReportInputs,
    FullTranslationInstallReport, InstallationGateReportInputs, TranslationInputs,
    project_chapter_intro_residency, project_dialogue_codebook, project_dialogue_page_pool,
    project_dialogue_runtime_composition, project_dialogue_storage, project_installation_gates,
};
use runtime_code::{plan_dialogue_runtime_code, resolve_request::MaterialLayout};
use runtime_control_flow::{
    DialogueRuntimeControlFlowPlan, RuntimeControlFlowInputs, plan_dialogue_runtime_control_flow,
};
use runtime_identity::{DialogueRuntimeIdentityPlan, plan_dialogue_runtime_identity};
use runtime_material::{
    DialogueRuntimeMaterialPlan, RuntimeMaterialInputs, plan_dialogue_runtime_material,
};
pub(crate) use runtime_state_storage::bind_dialogue_interrupt_audio_mapper_write_slice;
use runtime_state_storage::{DialogueRuntimeStateStoragePlan, plan_dialogue_runtime_state_storage};
use screen_font_residency::{
    DialogueSurfaceInputs, ScreenFontResidencyInputs, ScreenFontResidencyPlan,
    finalize_screen_font_residency, plan_screen_font_residency,
};
use transition_residency::{bind_transition_lifetime_worksets, plan_transition_residency};

/// 대사 런타임 재료 용기가 시작하는 MMC3 8 KiB 페이지다.
const MAIN_DIALOGUE_MATERIAL_FIRST_PAGE: u8 = 0x2C;
/// MMC3 페이지 하나다.
const MMC3_PAGE_BYTE_COUNT: usize = 8 * 1024;
/// 런타임 식별표 헤더의 길이다.
const IDENTITY_HEADER_BYTE_COUNT: usize = 16;
/// 그 뒤 selector 디렉터리의 길이다.
const IDENTITY_SELECTOR_DIRECTORY_BYTE_COUNT: usize = 256;
const REQUIRED_DOMAIN_COUNT: usize = 14;
const REQUIRED_DOMAINS: [&str; REQUIRED_DOMAIN_COUNT] = [
    "chapter_save_offer_label",
    "chapter_titles",
    "choice_labels",
    "class_names",
    "ending_record_labels",
    "enemy_names",
    "fixed_menu_labels",
    "item_action_labels",
    "item_names",
    "location_names",
    "main_dialogue",
    "map_menu_labels",
    "unit_names",
    "unit_ui_labels",
];

pub(crate) struct FullTranslationInstallInputs<'a> {
    pub(crate) source_path: &'a Path,
    pub(crate) main_dialogue_workspace_path: &'a Path,
    pub(crate) battle_dialogue_workspace_path: &'a Path,
    pub(crate) fixed_text_workspace_path: &'a Path,
    pub(crate) options_localization_path: &'a Path,
    pub(crate) roster_localization_path: &'a Path,
    pub(crate) front_end_menu_localization_path: &'a Path,
    pub(crate) class_profile_localization_path: &'a Path,
    pub(crate) title_graphics_localization_path: &'a Path,
    pub(crate) title_logo_asset_path: &'a Path,
    pub(crate) unit_name_localization_path: &'a Path,
    pub(crate) chapter_title_localization_path: &'a Path,
    pub(crate) choice_label_localization_path: &'a Path,
    pub(crate) map_menu_localization_path: &'a Path,
    pub(crate) unit_ui_label_localization_path: &'a Path,
    pub(crate) item_action_label_localization_path: &'a Path,
    pub(crate) fixed_menu_label_localization_path: &'a Path,
    pub(crate) transition_label_localization_path: &'a Path,
    pub(crate) location_name_localization_path: &'a Path,
    pub(crate) current_candidate_path: &'a Path,
    pub(crate) current_build_report_path: &'a Path,
    pub(crate) final_runtime_evidence_path: Option<&'a Path>,
    /// CLI가 검증된 통합 이미지를 실제 파일로 내보낼 예정인지 보고서에 반영한다.
    pub(crate) output_will_be_emitted: bool,
}

pub(crate) const FULL_TRANSLATION_REPORT_SCHEMA: u8 = 29;

pub(crate) struct FullTranslationInstallSummary {
    pub(crate) report_sha1: String,
    pub(crate) declared_installation_domain_count: usize,
    pub(crate) dialogue_record_count: usize,
    pub(crate) dialogue_page_workset_count: usize,
    pub(crate) dialogue_glyph_count: usize,
    pub(crate) dialogue_maximum_page_slot_demand: usize,
    pub(crate) dialogue_static_page_upper_bound_count: usize,
    pub(crate) dialogue_pointer_write_count: usize,
    pub(crate) dialogue_planned_storage_byte_count: usize,
    pub(crate) integrated_image_sha1: String,
}

pub(crate) struct FullTranslationInstallArtifacts {
    pub(crate) summary: FullTranslationInstallSummary,
    pub(crate) integrated_image: Vec<u8>,
    pub(crate) report_bytes: Vec<u8>,
}

pub(crate) fn plan_full_translation_installation(
    inputs: FullTranslationInstallInputs<'_>,
) -> Result<FullTranslationInstallArtifacts> {
    let rom = Rom::from_path(inputs.source_path)?;
    rom.verify_supported_japanese()?;
    let fixed_string_consumers = inspect_fixed_string_consumers(&rom)?;
    ensure!(
        fixed_string_consumers.records.len() == 72
            && fixed_string_consumers.call_sites.len() == 49
            && fixed_string_consumers.direct_producer_bound_indices.len() == 56,
        "fixed-string source population did not reach its declared consumer boundary"
    );
    let fixed_string_ownership = inspect_fixed_string_ownership(&fixed_string_consumers)?;
    let page_capacity = inspect_dialogue_page_pool_capacity(CurrentCandidateInputs {
        source_rom: &rom,
        candidate_path: inputs.current_candidate_path,
        build_report_path: inputs.current_build_report_path,
    })?;
    let current_candidate = Rom::from_path(inputs.current_candidate_path)?;
    ensure!(
        sha1_hex(current_candidate.data()) == page_capacity.current_candidate_sha1,
        "relocated dialogue bank plan and page-pool plan use different current candidates"
    );
    let front_end_mapper_register = bind_installed_front_end_mapper_register(&current_candidate)?;
    let front_end_mapper_route = RightFontPageProjection::for_screen_roles(
        rom.chr(),
        &["new_game_choice", "save_slot_selection"],
        0,
    )?
    .encode_mapper_route(front_end_mapper_register)?;

    let dialogue_validation =
        validate_main_dialogue_workspace(inputs.source_path, inputs.main_dialogue_workspace_path)?;
    ensure!(
        dialogue_validation.translation_input_complete,
        "all-record main dialogue installation still has untranslated Japanese"
    );
    let dialogue = plan_all_main_dialogue_records(&rom, inputs.main_dialogue_workspace_path)?;
    // 표시 계획은 정규 레코드에서 바로 만든다. 직접 진입과 전이 진입을 나누던 구조는
    // 프리픽스 파서 결함이 만든 것이어서 폐기했다. 의사결정 59번을 따른다.
    let display = crate::dialogue_assets::MainDialogueDisplayPlan::from_canonical_bundle(&dialogue);
    let fixed = plan_fixed_text(&rom, inputs.fixed_text_workspace_path)?;
    let front_end = plan_front_end_menu(&rom, inputs.front_end_menu_localization_path)?;
    let unit_names = plan_unit_names(&rom, inputs.unit_name_localization_path)?;
    let chapter_titles = plan_chapter_titles(&rom, inputs.chapter_title_localization_path)?;
    let choices = plan_choice_labels(&rom, inputs.choice_label_localization_path)?;
    let map_menu = plan_map_menu(&rom, inputs.map_menu_localization_path)?;
    let unit_ui = plan_unit_ui_labels(&rom, inputs.unit_ui_label_localization_path)?;
    let item_actions = plan_item_action_labels(&rom, inputs.item_action_label_localization_path)?;
    let fixed_menu_labels =
        plan_fixed_menu_labels(&rom, inputs.fixed_menu_label_localization_path)?;
    let options_localization = OptionsLocalization::from_path(inputs.options_localization_path)?;
    let validated_options = options_localization.validate()?;
    let options_glyph_codes = options_localization
        .glyphs
        .iter()
        .map(|glyph| (glyph.character, glyph.code))
        .collect::<BTreeMap<_, _>>();
    ensure!(
        validated_options.entries.len() == 3
            && options_glyph_codes.len() == options_localization.glyphs.len(),
        "options localization no longer has three labels with one code per glyph"
    );
    let transitions = plan_transition_labels(&rom, inputs.transition_label_localization_path)?;
    let locations = plan_location_name_text(&rom, inputs.location_name_localization_path)?;

    ensure!(
        dialogue.record_ids.len() == 504
            && fixed.entries.len() == 273
            && front_end.entries.len() == 7
            && unit_names.entries.len() == 53
            && chapter_titles.entry_count == 25
            && chapter_titles.translated_entry_count == 25
            && choices.entries.len() == 2
            && map_menu.entry_count == 8
            && map_menu.translated_entry_count == 8
            && unit_ui.entry_count == 25
            && item_actions.entry_count == 4
            && fixed_menu_labels.entry_count == 7
            && transitions.save_offer.entry_count == 1
            && transitions.ending_record.entry_count == 1
            && locations.entries.len() == 24,
        "full translation installation input population changed"
    );

    let dialogue_graph = inspect_main_dialogue_graph(rom.data())?;
    let transition_lifetimes = bind_transition_lifetime_worksets(&display, &dialogue_graph)?;

    let baseline_dynamic_inputs = plan_dynamic_dialogue_inputs(
        &display,
        &fixed.entries,
        &unit_names.entries,
        &locations.entries,
        &transition_lifetimes,
    )?;
    let baseline_codebook =
        plan_glyph_workset_page_upper_bound(&baseline_dynamic_inputs.augmented_worksets)?;
    let baseline_encoded = dialogue.encoded_by_page_groups(
        &baseline_codebook.workset_page_indices,
        &baseline_codebook.page_assignments,
    )?;
    ensure!(
        baseline_encoded.regions.len() == 11 && baseline_encoded.pointer_writes.len() == 517,
        "baseline dialogue encoded layout changed"
    );

    let dynamic_inputs = plan_dynamic_dialogue_inputs(
        &display,
        &fixed.entries,
        &unit_names.entries,
        &locations.entries,
        &transition_lifetimes,
    )?;
    let dynamic_input_producers = inspect_dynamic_input_producers(&rom)?;
    let mut dynamic_producer_encoding = bind_dynamic_producer_encoding(
        &current_candidate,
        &dynamic_inputs,
        &fixed.entries,
        &unit_names.entries,
        &locations.entries,
    )?;
    let chapter_intro_residency = plan_chapter_intro_residency(
        &rom,
        &display,
        &chapter_titles,
        &dynamic_inputs.augmented_worksets,
    )?;
    let front_end_result_menu_residency = plan_front_end_result_menu_residency(
        &rom,
        &current_candidate,
        &display,
        &front_end,
        &chapter_intro_residency.augmented_worksets,
    )?;
    let choice_residency = plan_choice_residency(
        &rom,
        &display,
        &choices,
        &front_end_result_menu_residency.augmented_worksets,
    )?;
    let source_font_page = rom
        .chr()
        .get(..FONT_PAGE_SIZE)
        .context("source dialogue font page is outside CHR")?;
    let cold_request_presentation = plan_cold_request_presentation_page(
        source_font_page,
        page_capacity.first_installable_physical_page,
    )?;
    let remaining_available_page_count = page_capacity
        .available_page_count
        .checked_sub(1)
        .context("cold-request presentation exhausted the reclaimable CHR page pool")?;
    let consumer_codebook = plan_consumer_codebook(ConsumerCodebookInputs {
        source_font_page,
        source_chr: rom.chr(),
        first_physical_page: cold_request_presentation
            .physical_page
            .checked_add(1)
            .context("consumer font page range overflow")?,
        available_page_count: remaining_available_page_count,
        dynamic_inputs: &dynamic_inputs,
        chapter_intro: &chapter_intro_residency,
        fixed: &fixed,
        unit_names: &unit_names,
        chapter_titles: &chapter_titles,
        choices: &choices,
        choice_glyph_codes: &choice_residency.choice_glyph_codes,
        map_menu: &map_menu,
        unit_ui: &unit_ui,
        item_actions: &item_actions,
        fixed_menu_labels: &fixed_menu_labels,
        options_glyph_codes: &options_glyph_codes,
        transitions: &transitions,
    })?;
    let chapter_save_projection = plan_chapter_save_projection(ChapterSaveProjectionInputs {
        candidate: &current_candidate,
        choices: &choices,
        choice_glyph_codes: &choice_residency.choice_glyph_codes,
        transitions: &transitions,
        consumer_codebook: &consumer_codebook,
    })?;
    let ending_record_projection = plan_ending_record_projection(EndingRecordProjectionInputs {
        source: &rom,
        candidate: &current_candidate,
        chapter_titles: &chapter_titles,
        transitions: &transitions,
        consumer_codebook: &consumer_codebook,
    })?;
    let consumer_catalog = plan_consumer_catalog(ConsumerCatalogInputs {
        source_font_page,
        source_chr: rom.chr(),
        first_physical_page: consumer_codebook.next_physical_page()?,
        available_page_count: consumer_codebook.remaining_page_count()?,
        preserved_unit_ui_display_codes: &preserved_unit_ui_display_codes(rom.data())?,
        resident_front_end_glyph_codes: front_end_result_menu_residency
            .installed_menu_glyph_codes(),
        fixed: &fixed,
        unit_names: &unit_names,
        unit_ui: &unit_ui,
        item_actions: &item_actions,
    })?;
    let screen_font_residency_draft = plan_screen_font_residency(ScreenFontResidencyInputs {
        front_end_menu_route: front_end_mapper_route,
        map_menu_route: consumer_codebook.mapper_route_for("map_menu")?,
        consumer_catalog: &consumer_catalog,
        consumer_codebook: &consumer_codebook,
        chapter_titles: &chapter_titles,
        choices: &choices,
        transitions: &transitions,
        fixed: &fixed,
        unit_names: &unit_names,
        unit_ui: &unit_ui,
        item_actions: &item_actions,
        fixed_menu_labels: &fixed_menu_labels,
        installed_front_end_glyph_codes: front_end_result_menu_residency
            .installed_menu_glyph_codes(),
        options_glyph_codes: &options_glyph_codes,
    })?;
    let front_end_result_residency = plan_front_end_result_residency(
        &display,
        &screen_font_residency_draft,
        front_end_result_menu_residency,
        &choice_residency.augmented_worksets,
    )?;
    let transition_residency = plan_transition_residency(
        &display,
        &dialogue_graph,
        &front_end_result_residency.augmented_worksets,
    )?;
    let codebook = plan_glyph_workset_page_upper_bound(&transition_residency.augmented_worksets)?;
    let screen_font_residency = finalize_screen_font_residency(
        screen_font_residency_draft,
        DialogueSurfaceInputs {
            dynamic_inputs: &dynamic_inputs.augmented_worksets,
            chapter_intro: &chapter_intro_residency.augmented_worksets,
            choice_and_front_end_menu: &choice_residency.augmented_worksets,
            front_end_result: &front_end_result_residency.augmented_worksets,
            transition_lifetime: &transition_residency.augmented_worksets,
            codebook: &codebook,
        },
        &current_candidate,
    )?;
    let dynamic_page_codes = bind_dynamic_string_page_codes(&dynamic_inputs, &codebook)?;
    ensure!(
        codebook.workset_count == display.page_worksets.len()
            && codebook.workset_page_indices.len() == display.page_worksets.len(),
        "dialogue codebook lost visible page worksets"
    );
    let fixed_ui_projection = plan_fixed_ui_projection(FixedUiProjectionInputs {
        candidate: &current_candidate,
        unit_ui: &unit_ui,
        item_actions: &item_actions,
        fixed_menu_labels: &fixed_menu_labels,
        map_menu: &map_menu,
        consumer_codebook: &consumer_codebook,
        consumer_catalog: &consumer_catalog,
    })?;
    let font_page_pack = build_glyph_workset_font_page_pack(source_font_page, &codebook)?;
    ensure!(
        font_page_pack.len() == codebook.page_assignments.len() * FONT_PAGE_SIZE,
        "dialogue font page pack length changed after rasterization"
    );
    verify_glyph_workset_font_page_pack(
        source_font_page,
        &transition_residency.augmented_worksets,
        &codebook,
        &font_page_pack,
    )?;
    // 전이 미러를 함께 내던 인코딩은 이중 진입과 함께 폐기했다. 정규 레코드의
    // 원천 소유 구간과 포인터만 낸다. 의사결정 59번을 따른다.
    let encoded_display = dialogue
        .encoded_by_page_groups(&codebook.workset_page_indices, &codebook.page_assignments)?;
    let mut cross_domain_target_glyphs = fixed.unique_glyphs();
    cross_domain_target_glyphs.extend(unit_names.unique_glyphs());
    cross_domain_target_glyphs.extend(chapter_titles.unique_glyphs());
    cross_domain_target_glyphs.extend(choices.unique_glyphs());
    cross_domain_target_glyphs.extend(map_menu.target_glyphs.iter().copied());
    cross_domain_target_glyphs.extend(unit_ui.unique_target_glyphs());
    cross_domain_target_glyphs.extend(item_actions.unique_target_glyphs());
    cross_domain_target_glyphs.extend(fixed_menu_labels.unique_target_glyphs());
    cross_domain_target_glyphs.extend(transitions.save_offer.target_glyphs.iter().copied());
    cross_domain_target_glyphs.extend(transitions.ending_record.target_glyphs.iter().copied());
    cross_domain_target_glyphs.extend(locations.unique_glyphs());
    let composition = plan_dialogue_runtime_composition(
        &display,
        &dialogue_graph,
        &transition_residency.augmented_worksets,
        &codebook,
        &dynamic_page_codes,
        source_font_page,
        &font_page_pack,
        &cross_domain_target_glyphs,
    )?;
    ensure!(
        cross_domain_target_glyphs.iter().all(|glyph| composition
            .glyph_atlas_characters
            .binary_search(glyph)
            .is_ok()),
        "shared glyph atlas lost a required cross-domain glyph"
    );
    let source_owned_storage_byte_count = baseline_encoded
        .regions
        .iter()
        .map(|region| region.source_storage.len())
        .sum::<usize>();
    let planned_storage_byte_count = encoded_display
        .regions
        .iter()
        .map(|region| region.used_storage_byte_count)
        .sum::<usize>();
    ensure!(
        planned_storage_byte_count <= source_owned_storage_byte_count,
        "complete dialogue encoded storage exceeds its source-owned regions"
    );
    let atlas_and_scan_byte_count = composition.glyph_atlas.len() + composition.scan_material.len();
    let runtime_identity = plan_dialogue_runtime_identity(rom.data(), &display)?;
    let atlas_scan_and_identity_byte_count = atlas_and_scan_byte_count
        .checked_add(runtime_identity.material.len())
        .context("dialogue runtime material length overflow")?;
    let mut runtime_material = plan_dialogue_runtime_material(RuntimeMaterialInputs {
        glyph_atlas: &composition.glyph_atlas,
        page_scan: &composition.scan_material,
        runtime_identity: &runtime_identity.material,
        dynamic_producer_encoding: &dynamic_producer_encoding.material,
    })?;
    let cross_domain_material = plan_cross_domain_material(CrossDomainMaterialInputs {
        main_dialogue_runtime_material_byte_count: runtime_material.material.len(),
        dialogue_page_recipe_blocks: &composition.page_recipe_blocks,
        dialogue_page_recipe_count: composition.visible_page_recipe_count,
        shared_atlas_characters: &composition.glyph_atlas_characters,
        fixed: &fixed,
        unit_names: &unit_names,
        chapter_titles: &chapter_titles,
        choices: &choices,
        map_menu: &map_menu,
        unit_ui: &unit_ui,
        item_actions: &item_actions,
        fixed_menu_labels: &fixed_menu_labels,
        transitions: &transitions,
        locations: &locations,
        consumer_catalog: &consumer_catalog,
    })?;
    // 실행 코드는 자료 배치가 끝난 뒤에 조립한다. 놓일 주소가 자료 길이에서 나오기
    // 때문이다. 코드 길이는 그 주소에 영향을 주지 않으므로 한 번에 정해진다.
    // 실행 코드는 자료 배치가 끝난 뒤에 조립한다. 읽을 표들이 어느 페이지의 어느
    // 주소에 놓이는지가 배치에서 나오기 때문이다.
    let atlas_offset = runtime_material.glyph_atlas_offset()?;
    let scan_offset = runtime_material.section_offset("page_scan")?;
    let identity_offset = runtime_material.section_offset("runtime_identity")?;
    let producer_encoding_offset = runtime_material.section_offset("dynamic_producer_encoding")?;
    ensure!(
        identity_offset / MMC3_PAGE_BYTE_COUNT
            == (identity_offset + runtime_identity.material.len() - 1) / MMC3_PAGE_BYTE_COUNT,
        "runtime identity material crosses its mapped MMC3 page"
    );
    ensure!(
        producer_encoding_offset / MMC3_PAGE_BYTE_COUNT
            == (producer_encoding_offset + dynamic_producer_encoding.material.len() - 1)
                / MMC3_PAGE_BYTE_COUNT,
        "dynamic producer encoding material crosses its mapped MMC3 page"
    );
    let page = |offset: usize| -> Result<u8> {
        Ok(MAIN_DIALOGUE_MATERIAL_FIRST_PAGE
            + u8::try_from(offset / MMC3_PAGE_BYTE_COUNT)
                .context("material page index overflow")?)
    };
    let window = |offset: usize| -> Result<u16> {
        u16::try_from(0x8000 + offset % MMC3_PAGE_BYTE_COUNT)
            .context("material CPU address does not fit the 8000 window")
    };
    let page_recipe_reference_address = scan_offset + composition.page_recipe_reference_offset;
    let record_recipe_directory_address = scan_offset + composition.record_recipe_directory_offset;
    ensure!(
        page_recipe_reference_address / MMC3_PAGE_BYTE_COUNT
            == (record_recipe_directory_address + composition.record_recipe_directory_byte_count
                - 1)
                / MMC3_PAGE_BYTE_COUNT,
        "dialogue page-recipe references and record directory do not share one mapped page"
    );
    let layout = MaterialLayout {
        identity_page: page(identity_offset)?,
        identity_material_base: window(identity_offset)?,
        identity_selector_directory: window(identity_offset + IDENTITY_HEADER_BYTE_COUNT)?,
        identity_table_descriptors: window(
            identity_offset + IDENTITY_HEADER_BYTE_COUNT + IDENTITY_SELECTOR_DIRECTORY_BYTE_COUNT,
        )?,
        scan_index_page: page(scan_offset + composition.page_recipe_reference_offset)?,
        page_recipe_references: window(scan_offset + composition.page_recipe_reference_offset)?,
        record_directory: window(scan_offset + composition.record_recipe_directory_offset)?,
        page_recipe_block_container_base: 0,
        container_first_page: cross_domain_material
            .dialogue_page_recipes()
            .first_mmc3_page,
        producer_encoding_page: page(producer_encoding_offset)?,
        producer_item_directory: window(
            producer_encoding_offset + dynamic_producer_encoding.item_directory_offset,
        )?,
        producer_unit_directory: window(
            producer_encoding_offset + dynamic_producer_encoding.unit_directory_offset,
        )?,
        producer_location_directory: window(
            producer_encoding_offset + dynamic_producer_encoding.location_directory_offset,
        )?,
        producer_encoding_base: window(producer_encoding_offset)?,
    };
    let dialogue_runtime_code = plan_dialogue_runtime_code(
        &rom,
        &current_candidate,
        runtime_material.runtime_code_cpu_start()?,
        page(atlas_offset)?,
        runtime_material.runtime_code_mmc3_page(),
        layout,
        cross_domain_material.consumer_catalog_runtime_layout()?,
        cold_request_presentation.mapper_register,
        screen_font_residency.routes(),
    )?;
    let assembled_hook_roles = dialogue_runtime_code.hook_roles();
    dynamic_producer_encoding.bind_runtime_hooks(&assembled_hook_roles)?;
    let dynamic_string_producers_bound = dynamic_input_producers
        .every_record_selector_route_bound()
        && dynamic_producer_encoding.canonical_outputs_ready();
    let main_dialogue_route_population = plan_main_dialogue_route_population(
        &rom,
        &display,
        &encoded_display,
        &dialogue_graph,
        &dynamic_input_producers,
        &assembled_hook_roles,
    )?;
    for routine in &dialogue_runtime_code.code_routines {
        runtime_material.place_runtime_code(routine.address, &routine.bytes)?;
    }
    let runtime_code_routines_assembled = !dialogue_runtime_code.code_routines.is_empty()
        && runtime_material.runtime_code_routine_placement_count()
            == dialogue_runtime_code.code_routines.len();
    ensure!(
        runtime_code_routines_assembled,
        "dialogue runtime material did not assemble every planned code routine"
    );

    let runtime_state_storage = plan_dialogue_runtime_state_storage(&rom)?;
    ensure!(
        runtime_state_storage.source_reservation_selection_complete(),
        "dialogue runtime-state storage selection is incomplete"
    );
    let selected_runtime_state_cpu_range = runtime_state_storage
        .selected_cpu_range_hex()
        .context("dialogue runtime-state selection has no CPU range")?;
    let runtime_control_flow = plan_dialogue_runtime_control_flow(RuntimeControlFlowInputs {
        source: &rom,
        candidate: &current_candidate,
        runtime_code_offset: runtime_material.runtime_code_offset,
        runtime_code_byte_count: runtime_material.material.len()
            - runtime_material.runtime_code_offset,
        selected_runtime_state_cpu_range,
        runtime_code_routines_assembled,
        assembled_hook_roles: &assembled_hook_roles,
        chr_restore_callee_cycles: dialogue_runtime_code.chr_restore_callee_cycles,
        canonical_dynamic_codes_are_page_physical_codes: dynamic_page_codes
            .canonical_codes_are_page_physical_codes,
    })?;
    let required_target_unit_counts = BTreeMap::from([
        (
            "chapter_save_offer_label",
            transitions.save_offer.entry_count,
        ),
        ("chapter_titles", chapter_titles.entry_count),
        ("choice_labels", choices.entries.len()),
        (
            "class_names",
            fixed
                .entries
                .iter()
                .filter(|entry| entry.table_id == "class-names")
                .count(),
        ),
        (
            "ending_record_labels",
            transitions.ending_record.entry_count,
        ),
        (
            "enemy_names",
            fixed
                .entries
                .iter()
                .filter(|entry| entry.table_id == "enemy-names")
                .count(),
        ),
        ("item_action_labels", item_actions.entry_count),
        ("fixed_menu_labels", fixed_menu_labels.entry_count),
        (
            "item_names",
            fixed
                .entries
                .iter()
                .filter(|entry| entry.table_id == "item-names")
                .count(),
        ),
        ("location_names", locations.entries.len()),
        ("main_dialogue", dialogue.translated_line_count),
        ("map_menu_labels", map_menu.entry_count),
        ("unit_names", unit_names.entries.len()),
        ("unit_ui_labels", unit_ui.entry_count),
    ]);
    ensure!(
        required_target_unit_counts.len() == REQUIRED_DOMAIN_COUNT,
        "full translation consumer target populations do not cover every required domain"
    );
    let mut globally_planned_consumer_roles = BTreeMap::<&'static str, BTreeSet<String>>::new();
    if dialogue_runtime_code.consumer_catalog_paths_planned() {
        let mut add_roles = |domain: &'static str, roles: &[&'static str]| {
            globally_planned_consumer_roles
                .entry(domain)
                .or_default()
                .extend(roles.iter().map(|role| (*role).to_owned()));
        };
        for domain in [
            "unit_ui_labels",
            "item_action_labels",
            "map_menu_labels",
            "fixed_menu_labels",
        ] {
            add_roles(domain, fixed_ui_projection.projected_screen_roles(domain));
        }
        for domain in ["unit_names", "enemy_names", "class_names"] {
            add_roles(domain, &["unit_summary", "unit_status"]);
        }
        add_roles(
            "item_names",
            &[
                "unit_summary",
                "unit_status",
                "item_inventory_list",
                "item_action_menu",
            ],
        );
        for domain in ["chapter_save_offer_label", "choice_labels"] {
            add_roles(
                domain,
                chapter_save_projection.projected_screen_roles(domain),
            );
        }
        for domain in ["chapter_titles", "ending_record_labels"] {
            add_roles(
                domain,
                ending_record_projection.projected_screen_roles(domain),
            );
        }
    }
    let all_required_dialogue_runtime_hook_roles_assembled =
        runtime_control_flow.all_required_hook_roles_assembled();
    let mut consumer_installation = plan_consumer_installation(ConsumerInstallationInputs {
        current_candidate_path: inputs.current_candidate_path,
        current_build_report_path: inputs.current_build_report_path,
        required_domains: &REQUIRED_DOMAINS,
        target_unit_counts: &required_target_unit_counts,
        all_chapter_titles_encoded: chapter_intro_residency.encoded_titles.len()
            == chapter_titles.entry_count,
        all_dialogue_records_encoded: dialogue.record_ids.len() == 504
            && encoded_display.pointer_writes.len() == 517,
        all_dialogue_runtime_hook_roles_assembled:
            all_required_dialogue_runtime_hook_roles_assembled,
        dynamic_dialogue_producers_bound: dynamic_string_producers_bound,
        globally_planned_consumer_roles: &globally_planned_consumer_roles,
    })?;
    let installation_layout = plan_installation_layout(
        &current_candidate,
        &page_capacity,
        runtime_material.material.len(),
        cross_domain_material.material_span_byte_count(),
        &cold_request_presentation,
    )?;
    let (installed_image, integrated_write_set) =
        plan_integrated_write_set(IntegratedWriteSetInputs {
            candidate: &current_candidate,
            encoded_dialogue: &encoded_display,
            dialogue_runtime_material: &runtime_material.material,
            dialogue_runtime_code: &dialogue_runtime_code,
            encoded_chapter_titles: &chapter_intro_residency.encoded_titles,
            cold_request_presentation: &cold_request_presentation,
            consumer_codebook: &consumer_codebook,
            consumer_catalog: &consumer_catalog,
            cross_domain_material: &cross_domain_material,
            fixed_ui_projection: &fixed_ui_projection,
            chapter_save_projection: &chapter_save_projection,
            ending_record_projection: &ending_record_projection,
            font_page_selector_forwarders: screen_font_residency.selector_forwarders(),
            consumer_installation: &consumer_installation,
            required_domains: &REQUIRED_DOMAINS,
            all_required_dialogue_runtime_hook_roles_assembled,
            output_will_be_emitted: inputs.output_will_be_emitted,
        })?;
    let integrated_rom =
        Rom::parse(installed_image.clone()).context("parse integrated translation image")?;
    let final_roster_consumer_route = dialogue_runtime_code.final_roster_consumer_route()?;
    let carried_ui_domain_preservation = inspect_carried_ui_domains(CarriedUiDomainInputs {
        source: &rom,
        cumulative: &current_candidate,
        integrated: &integrated_rom,
        cumulative_report_path: inputs.current_build_report_path,
        options_localization_path: inputs.options_localization_path,
        roster_localization_path: inputs.roster_localization_path,
        front_end_menu_localization_path: inputs.front_end_menu_localization_path,
        class_profile_localization_path: inputs.class_profile_localization_path,
        title_graphics_localization_path: inputs.title_graphics_localization_path,
        title_logo_asset_path: inputs.title_logo_asset_path,
        final_roster_consumer_route: &final_roster_consumer_route,
    })?;
    let carried_ui_domains_complete = carried_ui_domain_preservation.complete();
    ensure!(
        carried_ui_domains_complete,
        "carried UI domain final-artifact reinspection is incomplete"
    );
    let final_battle_consumer_route = dialogue_runtime_code.final_battle_consumer_route()?;
    let carried_battle_domain_preservation =
        inspect_carried_battle_domains(CarriedBattleDomainInputs {
            source: &rom,
            cumulative: &current_candidate,
            integrated: &integrated_rom,
            cumulative_report_path: inputs.current_build_report_path,
            fixed_workspace_path: inputs.fixed_text_workspace_path,
            dialogue_workspace_path: inputs.battle_dialogue_workspace_path,
            final_consumer_route: &final_battle_consumer_route,
        })?;
    let carried_battle_domains_complete = carried_battle_domain_preservation.complete();
    ensure!(
        carried_battle_domains_complete,
        "carried battle domain final-artifact reinspection is incomplete"
    );
    let translation_input_complete = dialogue_validation.translation_input_complete;
    let review_complete = dialogue_validation.review_complete
        && fixed.review_complete
        && front_end.review_complete
        && unit_names.review_complete
        && chapter_titles.review_complete
        && choices.review_complete
        && map_menu.review_complete
        && unit_ui.review_complete
        && item_actions.review_complete
        && fixed_menu_labels.review_complete
        && transitions.save_offer.review_complete
        && transitions.ending_record.review_complete
        && locations.review_complete
        && carried_ui_domain_preservation.human_review_complete()
        && carried_battle_domain_preservation.human_review_complete()
        && translation_input_complete;
    let all_declared_consumers_statically_accounted =
        consumer_installation.all_declared_consumers_statically_accounted();
    let technical_installation_complete = integrated_write_set.technical_installation_complete();
    let integrated_image_sha1 = sha1_hex(&installed_image);
    let final_artifact_runtime_evidence = load_final_artifact_runtime_evidence(
        inputs.final_runtime_evidence_path,
        &integrated_image_sha1,
    )?;
    let registered_runtime_screen_roles =
        crate::translation_coverage::inspect_domain_screen_targets()?
            .into_iter()
            .flat_map(|domain| domain.screen_roles)
            .collect::<BTreeSet<_>>();
    consumer_installation.bind_declared_consumer_runtime_roles(
        &final_artifact_runtime_evidence.bound_screen_roles(),
        &registered_runtime_screen_roles,
    )?;
    let dynamic_verification_started = final_artifact_runtime_evidence.verification_started();
    let declared_consumer_runtime_observation_complete =
        !consumer_installation.declared_consumer_runtime_replay_required();
    let next_gate = InstallationReadiness::evaluate(InstallationReadinessInputs {
        translation_input_complete,
        runtime_state_storage_complete: runtime_state_storage
            .source_reservation_selection_complete(),
        all_declared_consumers_statically_accounted,
        carried_domain_reinspection_complete: carried_ui_domains_complete
            && carried_battle_domains_complete,
        technical_installation_complete,
        review_complete,
        output_will_be_emitted: inputs.output_will_be_emitted,
        dynamic_verification_started,
        declared_consumer_runtime_observation_complete,
    })?
    .next_gate();
    let rom_emitted = inputs.output_will_be_emitted;
    let dialogue_runtime_composition_report =
        project_dialogue_runtime_composition(DialogueRuntimeCompositionReportInputs {
            composition,
            dynamic_inputs: &dynamic_inputs,
            dynamic_page_codes,
            dynamic_string_producers_bound,
            dynamic_input_producers,
            dynamic_producer_encoding,
            runtime_identity,
            atlas_scan_and_identity_byte_count,
            runtime_material,
            technical_installation_complete,
            page_capacity: &page_capacity,
        });
    let dialogue_codebook_report = project_dialogue_codebook(DialogueCodebookReportInputs {
        display: &display,
        codebook: &codebook,
        font_page_pack: &font_page_pack,
        technical_installation_complete,
        transition_residency: &transition_residency,
    });
    let chapter_intro_residency_report = project_chapter_intro_residency(&chapter_intro_residency);
    let dialogue_page_pool_report = project_dialogue_page_pool(
        &page_capacity,
        remaining_available_page_count,
        codebook.page_assignments.len(),
    );
    let dialogue_storage_report = project_dialogue_storage(
        &encoded_display,
        dialogue.record_ids.len(),
        source_owned_storage_byte_count,
        planned_storage_byte_count,
    );
    let installation_gates_report = project_installation_gates(InstallationGateReportInputs {
        translation_input_complete,
        all_declared_consumers_statically_accounted,
        carried_ui_domains_complete,
        carried_battle_domains_complete,
        technical_installation_complete,
        declared_consumer_runtime_observation_complete,
    });
    let report = FullTranslationInstallReport {
        schema: FULL_TRANSLATION_REPORT_SCHEMA,
        source_sha1: EXPECTED_SOURCE_SHA1,
        strategy: "install the declared translation domains in one cumulative candidate, close declared installation gates, then run declared consumer-path dynamic regression on that same ROM; whole-game consumer census remains separate",
        declared_installation_domain_count: REQUIRED_DOMAIN_COUNT,
        declared_installation_domains: REQUIRED_DOMAINS,
        translation_inputs: TranslationInputs {
            main_dialogue_record_count: dialogue.record_ids.len(),
            fixed_text_physical_entry_count: fixed.entries.len(),
            front_end_menu_label_count: front_end.entries.len(),
            playable_unit_name_count: unit_names.entries.len(),
            chapter_title_count: chapter_titles.entry_count,
            choice_label_count: choices.entries.len(),
            map_menu_label_count: map_menu.entry_count,
            unit_ui_label_count: unit_ui.entry_count,
            item_action_label_count: item_actions.entry_count,
            fixed_menu_label_count: fixed_menu_labels.entry_count,
            transition_label_count: transitions.save_offer.entry_count
                + transitions.ending_record.entry_count,
            location_name_count: locations.entries.len(),
            translation_input_complete,
            review_complete,
        },
        fixed_string_consumers: fixed_string_consumers.census,
        fixed_string_ownership,
        dialogue_codebook: dialogue_codebook_report,
        chapter_intro_residency: chapter_intro_residency_report,
        choice_residency,
        screen_font_residency,
        front_end_result_residency,
        chapter_save_projection,
        ending_record_projection,
        dialogue_page_pool: dialogue_page_pool_report,
        cross_domain_material,
        consumer_codebook,
        consumer_catalog,
        fixed_ui_projection,
        installation_layout,
        integrated_write_set,
        dialogue_runtime_control_flow_static_contract: runtime_control_flow,
        dialogue_runtime_state_storage_source_reservation: runtime_state_storage,
        main_dialogue_route_population,
        dialogue_runtime_composition: dialogue_runtime_composition_report,
        consumer_installation,
        carried_ui_domain_preservation,
        carried_battle_domain_preservation,
        final_artifact_runtime_evidence,
        dialogue_storage: dialogue_storage_report,
        installation_gates: installation_gates_report,
        rom_emitted,
        dynamic_verification_started,
        next_gate,
    };
    let mut report_bytes =
        serde_json::to_vec_pretty(&report).context("serialize full translation install plan")?;
    report_bytes.push(b'\n');

    Ok(FullTranslationInstallArtifacts {
        summary: FullTranslationInstallSummary {
            report_sha1: sha1_hex(&report_bytes),
            declared_installation_domain_count: REQUIRED_DOMAIN_COUNT,
            dialogue_record_count: dialogue.record_ids.len(),
            dialogue_page_workset_count: display.page_worksets.len(),
            dialogue_glyph_count: codebook.glyph_count,
            dialogue_maximum_page_slot_demand: codebook.maximum_page_slot_demand,
            dialogue_static_page_upper_bound_count: codebook.page_assignments.len(),
            dialogue_pointer_write_count: encoded_display.pointer_writes.len(),
            dialogue_planned_storage_byte_count: planned_storage_byte_count,
            integrated_image_sha1,
        },
        integrated_image: installed_image,
        report_bytes,
    })
}
