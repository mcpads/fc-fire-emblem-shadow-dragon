use crate::{
    chapter_transition::ChapterTitlePlannedEntry,
    dialogue_assets::{MainDialogueBundlePlan, MainDialogueSlicePlan},
    rom::{EXPECTED_SOURCE_SHA1, Rom},
    sha1_hex,
    title_graphics::InstalledTitleLogo,
};

use super::super::super::{
    BoundFontPageFallbackGraph, FontPageFallbackNodeRole,
    dialogue_lifetime_page::{DialogueLifetimePagePlan, SCREEN_ROLE},
    dialogue_probe_font::assignment_sha1,
    hangul_page_probe::HangulPageProbeSummary,
    maximum_dialogue_page::{
        COMPLETED_PAGE_COUNT as MAXIMUM_DIALOGUE_PAGE_COUNT,
        DISPLAY_LINES_PER_PAGE as MAXIMUM_DIALOGUE_LINES_PER_PAGE,
        SCREEN_ROLE as MAXIMUM_DIALOGUE_SCREEN_ROLE,
    },
    shop_dialogue_page::{SCREEN_ROLE as SHOP_DIALOGUE_SCREEN_ROLE, ShopDialoguePagePlan},
    weapon_shop_shared_text::SCREEN_ROLE as WEAPON_SHOP_SHARED_TEXT_SCREEN_ROLE,
};
use super::super::{
    CHAPTER_ONE_INDEX, CHAPTER_TWO_INDEX, CHAPTER_TWO_SCREEN_ROLE, REPORT_SCHEMA,
    WEAPON_SHOP_CAPACITY_BOUND_SCREEN_ROLES, battle_stage::BattleStageOutput,
    class_profile_runtime::ClassProfileRuntimeEvidence,
    class_profile_stage::ClassProfileStageOutput, front_end_stage::FrontEndStageOutput,
    input_plan::CumulativeInputPlan,
    maximum_dialogue_runtime_evidence::MaximumDialogueRuntimeEvidence,
    maximum_dialogue_stage::MaximumDialogueStageOutput,
    shop_dialogue_runtime::ShopDialogueRuntimeEvidence,
    shop_dialogue_stage::ShopDialogueStageOutput, title_logo_runtime::TitleLogoRuntimeEvidence,
    unit_name_stage::UnitNameStageOutput,
    weapon_shop_shared_text_runtime::WeaponShopSharedTextRuntimeEvidence,
    weapon_shop_shared_text_stage::WeaponShopSharedTextStageOutput,
};
use super::*;

mod dialogue;
mod selector_graph;
mod stages;
mod surfaces;

use dialogue::{chapter_title_report, main_dialogue_report};
use selector_graph::selector_fallback_graph_report;
use stages::stage_reports;
use surfaces::{
    battle_text_report, class_profile_report, front_end_menu_report, options_menu_report,
    playable_unit_name_report, title_logo_report, weapon_shop_shared_text_report,
};

#[derive(Clone)]
pub(in crate::mapper165::cumulative_patch) struct CumulativeReportInputs<'a> {
    pub(in crate::mapper165::cumulative_patch) input_plan: &'a CumulativeInputPlan,
    pub(in crate::mapper165::cumulative_patch) output_sha1: String,
    pub(in crate::mapper165::cumulative_patch) output_rom: &'a Rom,
    pub(in crate::mapper165::cumulative_patch) ui_stage: &'a HangulPageProbeSummary,
    pub(in crate::mapper165::cumulative_patch) chapter_one_output_sha1: String,
    pub(in crate::mapper165::cumulative_patch) chapter_two_output_sha1: String,
    pub(in crate::mapper165::cumulative_patch) chapter_one_plans: &'a [MainDialogueSlicePlan],
    pub(in crate::mapper165::cumulative_patch) chapter_two_plans: &'a [MainDialogueSlicePlan],
    pub(in crate::mapper165::cumulative_patch) chapter_one_title: &'a ChapterTitlePlannedEntry,
    pub(in crate::mapper165::cumulative_patch) chapter_two_title: &'a ChapterTitlePlannedEntry,
    pub(in crate::mapper165::cumulative_patch) chapter_one_encoded_records: &'a [Vec<u8>],
    pub(in crate::mapper165::cumulative_patch) chapter_two_encoded_records: &'a [Vec<u8>],
    pub(in crate::mapper165::cumulative_patch) chapter_one_encoded_title: &'a [u8],
    pub(in crate::mapper165::cumulative_patch) chapter_two_encoded_title: &'a [u8],
    pub(in crate::mapper165::cumulative_patch) chapter_one_page: &'a DialogueLifetimePagePlan,
    pub(in crate::mapper165::cumulative_patch) chapter_two_page: &'a DialogueLifetimePagePlan,
    pub(in crate::mapper165::cumulative_patch) front_end_stage: &'a FrontEndStageOutput,
    pub(in crate::mapper165::cumulative_patch) unit_name_stage: &'a UnitNameStageOutput,
    pub(in crate::mapper165::cumulative_patch) class_profile_stage: &'a ClassProfileStageOutput,
    pub(in crate::mapper165::cumulative_patch) shop_dialogue_stage: &'a ShopDialogueStageOutput,
    pub(in crate::mapper165::cumulative_patch) weapon_shop_shared_text_stage:
        &'a WeaponShopSharedTextStageOutput,
    pub(in crate::mapper165::cumulative_patch) battle_stage: &'a BattleStageOutput,
    pub(in crate::mapper165::cumulative_patch) maximum_dialogue_stage:
        &'a MaximumDialogueStageOutput,
    pub(in crate::mapper165::cumulative_patch) title_logo_stage: &'a InstalledTitleLogo,
    pub(in crate::mapper165::cumulative_patch) maximum_dialogue_runtime:
        &'a Option<MaximumDialogueRuntimeEvidence>,
    pub(in crate::mapper165::cumulative_patch) title_logo_runtime:
        &'a Option<TitleLogoRuntimeEvidence>,
    pub(in crate::mapper165::cumulative_patch) shop_dialogue_runtime:
        &'a Option<ShopDialogueRuntimeEvidence>,
    pub(in crate::mapper165::cumulative_patch) class_profile_runtime:
        &'a Option<ClassProfileRuntimeEvidence>,
    pub(in crate::mapper165::cumulative_patch) weapon_shop_shared_text_runtime:
        &'a Option<WeaponShopSharedTextRuntimeEvidence>,
    pub(in crate::mapper165::cumulative_patch) selector_fallback_graph:
        &'a BoundFontPageFallbackGraph,
    pub(in crate::mapper165::cumulative_patch) tracked_write_count: usize,
    pub(in crate::mapper165::cumulative_patch) translated_line_count: usize,
    pub(in crate::mapper165::cumulative_patch) source_storage_byte_count: usize,
    pub(in crate::mapper165::cumulative_patch) planned_storage_byte_count: usize,
    pub(in crate::mapper165::cumulative_patch) installed_main_dialogue_record_count: usize,
    pub(in crate::mapper165::cumulative_patch) installed_dialogue_glyph_slot_count: usize,
    pub(in crate::mapper165::cumulative_patch) weapon_shop_shared_page_total_slot_demand: usize,
    pub(in crate::mapper165::cumulative_patch) roster_page_total_slot_demand: usize,
}

pub(in crate::mapper165::cumulative_patch) fn assemble_cumulative_report(
    inputs: CumulativeReportInputs<'_>,
) -> CumulativePatchReport {
    let report_inputs = inputs.clone();
    let CumulativeReportInputs {
        input_plan,
        output_sha1,
        output_rom,
        weapon_shop_shared_text_stage,
        battle_stage,
        maximum_dialogue_runtime,
        title_logo_runtime,
        weapon_shop_shared_text_runtime,
        tracked_write_count,
        ..
    } = inputs;
    let dialogue_workspace = &input_plan.dialogue_workspace;
    let chapter_title_plan = &input_plan.chapter_title_plan;
    let title_graphics_plan = &input_plan.title_graphics_plan;
    let front_end_menu_plan = &input_plan.front_end_menu_plan;
    let unit_name_plan = &input_plan.unit_name_plan;
    let class_profile_plan = &input_plan.class_profile_plan;
    let choice_label_plan = &input_plan.choice_label_plan;

    let stages = stage_reports(&report_inputs);
    let report = CumulativePatchReport {
        schema: REPORT_SCHEMA,
        source_sha1: EXPECTED_SOURCE_SHA1,
        output_sha1: output_sha1.clone(),
        output_mapper: output_rom.mapper(),
        prg_size: output_rom.prg().len(),
        chr_size: output_rom.chr().len(),
        stage_count: stages.len(),
        stages,
        chapter_titles: chapter_title_report(&report_inputs),
        main_dialogue: main_dialogue_report(&report_inputs),
        options_menu: options_menu_report(&report_inputs),
        front_end_menu: front_end_menu_report(&report_inputs),
        playable_unit_names: playable_unit_name_report(&report_inputs),
        automatic_class_profiles: class_profile_report(&report_inputs),
        title_logo: title_logo_report(&report_inputs),
        weapon_shop_shared_text: weapon_shop_shared_text_report(&report_inputs),
        battle_text: battle_text_report(&report_inputs),
        selector_fallback_graph: selector_fallback_graph_report(
            report_inputs.selector_fallback_graph,
        ),
        original_chr_preserved: false,
        tracked_write_count,
        translation_input_complete: dialogue_workspace.translation_input_complete
            && chapter_title_plan.translated_entry_count == chapter_title_plan.entry_count
            && front_end_menu_plan.entries.len() == 7
            && unit_name_plan.entries.len() == 53
            && class_profile_plan.entries.len() == 22
            && weapon_shop_shared_text_stage
                .plan
                .projection
                .item_name_count
                == 6
            && choice_label_plan.entries.len() == 2
            && battle_stage.dialogue_record_count == 28
            && title_graphics_plan.translated_surface_count == 1,
        review_complete: dialogue_workspace.review_complete
            && chapter_title_plan.review_complete
            && front_end_menu_plan.review_complete
            && unit_name_plan.review_complete
            && class_profile_plan.review_complete
            && weapon_shop_shared_text_stage.plan.review_complete
            && title_graphics_plan.review_complete,
        runtime_verified: false,
        unresolved: vec![
            "The translated Chapter 1 and Chapter 2 title bars need cold-route runtime regression together with every installed dialogue page and natural map restoration.",
            "Private observations passed the installed no-save and valid-save front-end variants, but installed runtime evidence is not yet build-bound and the suspend-data variant is unverified.",
            "Playable-unit names are installed for roster, map unit-summary/status, and battle consumers; ending consumers remain Japanese backlog until their own font lifetimes are installed.",
            "The translated playable-unit name pages still need build-bound cold runtime evidence across roster, unit summary, unit status, and their exit paths.",
            if weapon_shop_shared_text_runtime.is_some() {
                "The installed weapon-shop shared page is capacity-bound to all nine screen roles at 150/210 slots. The decline route is runtime-bound to the eighth-stage output through item selection, choices, continue prompt, item-list return, exit message, and map restoration; the final cumulative output, purchase, and every preflight branch still need exact-output runtime evidence."
            } else {
                "The installed weapon-shop shared page is capacity-bound to all nine screen roles, but exact-output runtime evidence was explicitly deferred for this development build. Every shop route remains dynamically unbound to this artifact."
            },
            "Battle text and the dynamic composition loader are installed in this cumulative lineage, but the new cumulative output still needs cold-route battle and prior-screen regression evidence.",
            if maximum_dialogue_runtime.is_some() {
                "The source-bound fifteen-page maximum dialogue has exact-output evidence for its state-bridged Chapter 7 seize entry, initial selector, all page font reloads, irregular temporal samples, and the final NEXT STORY exit; cold-route prior-screen continuity remains open."
            } else {
                "The source-bound fifteen-page maximum dialogue is installed, but exact-output runtime evidence was not supplied for this development build. Initial selection, page reloads, final exit, and cold-route prior-screen continuity remain dynamically unbound."
            },
            if title_logo_runtime.is_some() {
                "The source-bound Korean title logo is exact-output-bound through its initial and completed blink phases, preserved sword, two-cell TM, copyright line, and automatic profile exit. The later defeat-route title return and human visual approval remain open."
            } else {
                "The source-bound Korean title logo is installed, but exact-output runtime evidence was explicitly deferred for this development build. Initial and completed blink phases, the later defeat-route title return, and human visual approval remain open."
            },
            "The remaining main-dialogue screen lifetimes and translated non-dialogue surfaces are not yet installed in this cumulative lineage.",
            "The ending scroll owns a separate physical copy of all chapter titles; that duplicate consumer is not installed by this intro-title stage.",
            "Human translation review is incomplete, so this output is a development build rather than a release candidate.",
        ],
        release_eligible: false,
    };
    report
}
