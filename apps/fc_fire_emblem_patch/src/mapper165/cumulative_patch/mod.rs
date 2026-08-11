use std::{collections::BTreeSet, fs, path::Path};

use anyhow::{Context, Result, ensure};

use crate::{
    chapter_transition::plan_chapter_titles,
    choice_labels::plan_choice_labels,
    class_profile::plan_class_profiles,
    dialogue_assets::{
        MainDialogueBundlePlan, MainDialogueSlicePlan, plan_main_dialogue_bundle,
        plan_main_dialogue_slice, validate_main_dialogue_workspace,
    },
    font_slots::{ACTIVE_HANGUL_SLOT_COUNT, FONT_PAGE_SIZE},
    front_end_menu::plan_front_end_menu,
    mmc5_prg::{count_direct_transfers_to_range, fixed_bank_file_offset},
    rom::{EXPECTED_SOURCE_SHA1, Rom},
    sha1_hex,
    text_inventory::plan_fixed_text,
    title_graphics::{install_title_logo_asset, plan_title_graphics},
    tracked::TrackedImage,
    unit_names::plan_unit_names,
};

use super::{
    OUTPUT_MAPPER, SELECT_RIGHT_FD_CHR_BANK_FOR_PAIR_ADDRESS,
    battle_composition_loader_probe::CUMULATIVE_RUNTIME_LAYOUT,
    dialogue_lifetime_page::{SCREEN_ROLE, build_page_routine_at, plan_dialogue_lifetime_page},
    dialogue_probe_font::assignment_sha1,
    hangul_page_probe::build_mapper165_hangul_page_probe,
    maximum_dialogue_page::{
        COMPLETED_PAGE_COUNT as MAXIMUM_DIALOGUE_PAGE_COUNT,
        DISPLAY_LINES_PER_PAGE as MAXIMUM_DIALOGUE_LINES_PER_PAGE,
        SCREEN_ROLE as MAXIMUM_DIALOGUE_SCREEN_ROLE,
        TARGET_RECORD_ID as MAXIMUM_DIALOGUE_RECORD_ID,
    },
    roster_page::{
        PAGE_REGISTERS as ROSTER_PAGE_REGISTERS, PAGE_ROUTINE_ADDRESS as ROSTER_SELECTOR_ADDRESS,
        build_page_routine as build_roster_selector,
        build_page_routine_with_fallback as build_chained_roster_selector,
    },
    shop_dialogue_page::{
        PAGE_ROUTINE_ADDRESS as SHOP_DIALOGUE_SELECTOR_ADDRESS,
        RECORD_IDS as SHOP_DIALOGUE_RECORD_IDS, SCREEN_ROLE as SHOP_DIALOGUE_SCREEN_ROLE,
    },
    weapon_shop_shared_text::ITEM_NAME_SOURCE_INDICES as WEAPON_SHOP_ITEM_NAME_SOURCE_INDICES,
    weapon_shop_shared_text::SCREEN_ROLE as WEAPON_SHOP_SHARED_TEXT_SCREEN_ROLE,
};

mod battle_stage;
mod chapter_page_selector;
mod class_profile_runtime;
mod class_profile_stage;
mod front_end_stage;
mod maximum_dialogue_runtime_evidence;
mod maximum_dialogue_stage;
mod report;
mod shop_dialogue_runtime;
mod shop_dialogue_stage;
mod title_logo_runtime;
mod unit_name_stage;
mod verify;
mod weapon_shop_shared_text_runtime;
mod weapon_shop_shared_text_stage;

use battle_stage::{BattleStageInputs, install_battle_stage};
use chapter_page_selector::{ChapterPageSequence, build_chapter_page_selector};
use class_profile_runtime::verify_class_profile_runtime_evidence;
use class_profile_stage::install_class_profile_stage;
use front_end_stage::install_front_end_stage;
use maximum_dialogue_runtime_evidence::verify_maximum_dialogue_runtime_evidence;
use maximum_dialogue_stage::{MaximumDialogueStageInputs, install_maximum_dialogue_stage};
use report::{
    CumulativeBattleTextReport, CumulativeChapterTitleReport, CumulativeClassProfileReport,
    CumulativeDialogueLifetimeReport, CumulativeDialogueReport, CumulativeFrontEndMenuReport,
    CumulativeMaximumDialogueReport, CumulativeOptionsMenuReport, CumulativePatchReport,
    CumulativeStageReport, CumulativeTitleLogoReport, CumulativeUnitNameReport,
    CumulativeWeaponShopSharedTextReport, SelectorChainReport,
};
use shop_dialogue_runtime::verify_shop_dialogue_runtime_evidence;
use shop_dialogue_stage::install_shop_dialogue_stage;
use title_logo_runtime::verify_title_logo_runtime_evidence;
use unit_name_stage::install_unit_name_stage;
use verify::{install_chapter_title, install_dialogue_record, verify_cumulative_output};
use weapon_shop_shared_text_runtime::verify_weapon_shop_shared_text_runtime_evidence;
use weapon_shop_shared_text_stage::install_weapon_shop_shared_text_stage;

const UI_STAGE_ROM_NAME: &str = "mapper165-ui.nes";
const UI_STAGE_REPORT_NAME: &str = "mapper165-ui.json";
const CHAPTER_ONE_STAGE_ROM_NAME: &str = "chapter1-intro.nes";
const FRONT_END_STAGE_ROM_NAME: &str = "front-end-menu.nes";
const UNIT_NAME_STAGE_ROM_NAME: &str = "unit-names.nes";
const CLASS_PROFILE_STAGE_ROM_NAME: &str = "class-profiles.nes";
const SHOP_DIALOGUE_STAGE_ROM_NAME: &str = "weapon-shop-dialogue.nes";
const SHOP_SHARED_TEXT_STAGE_ROM_NAME: &str = "weapon-shop-shared-text.nes";
const MAXIMUM_DIALOGUE_STAGE_ROM_NAME: &str = "maximum-dialogue.nes";
const TITLE_LOGO_STAGE_ROM_NAME: &str = "title-logo.nes";
const DIALOGUE_SELECTOR_ADDRESS: u16 = 0xFBD4;
const DIALOGUE_SELECTOR_CAVE_END: u16 = 0xFC20;
const CHAPTER_ONE_DIALOGUE_SELECTOR_ADDRESS: u16 = 0xFBD8;
const CHAPTER_ONE_INDEX: u8 = 0;
const CHAPTER_TWO_INDEX: u8 = 1;
const CHAPTER_TWO_SCREEN_ROLE: &str = "chapter_2_intro_dialogue";
const CHAPTER_TWO_INTRO_RECORD_ID: &str = "chapter-intro-dialogue:005";
const CHAPTER_ONE_INTRO_RECORD_IDS: [&str; 4] = [
    "chapter-intro-dialogue:000",
    "chapter-intro-dialogue:002",
    "chapter-intro-dialogue:003",
    "chapter-intro-dialogue:004",
];
const WEAPON_SHOP_CAPACITY_BOUND_SCREEN_ROLES: [&str; 9] = [
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

pub(crate) struct CumulativePatchSummary {
    pub(crate) output_sha1: String,
    pub(crate) report_sha1: String,
    pub(crate) stage_count: usize,
    pub(crate) installed_dialogue_record_count: usize,
    pub(crate) installed_dialogue_line_count: usize,
    pub(crate) installed_chapter_title_count: usize,
    pub(crate) installed_glyph_slot_count: usize,
    pub(crate) tracked_write_count: usize,
}

pub(crate) struct CumulativePatchInputs<'a> {
    pub(crate) source_path: &'a Path,
    pub(crate) options_localization_path: &'a Path,
    pub(crate) roster_localization_path: &'a Path,
    pub(crate) options_screen_evidence_path: &'a Path,
    pub(crate) main_dialogue_workspace_path: &'a Path,
    pub(crate) chapter_title_localization_path: &'a Path,
    pub(crate) front_end_menu_localization_path: &'a Path,
    pub(crate) unit_name_localization_path: &'a Path,
    pub(crate) class_profile_localization_path: &'a Path,
    pub(crate) fixed_text_workspace_path: &'a Path,
    pub(crate) battle_dialogue_workspace_path: &'a Path,
    pub(crate) battle_temporal_manifest_path: &'a Path,
    pub(crate) choice_label_localization_path: &'a Path,
    pub(crate) chapter_one_intro_evidence_path: &'a Path,
    pub(crate) chapter_two_intro_evidence_path: &'a Path,
    pub(crate) front_end_menu_evidence_path: &'a Path,
    pub(crate) unit_name_evidence_path: &'a Path,
    pub(crate) class_profile_evidence_path: &'a Path,
    pub(crate) class_profile_runtime_evidence_path: &'a Path,
    pub(crate) shop_dialogue_evidence_path: &'a Path,
    pub(crate) shop_dialogue_runtime_evidence_path: &'a Path,
    pub(crate) weapon_shop_shared_text_runtime_evidence_path: &'a Path,
    pub(crate) maximum_dialogue_evidence_path: &'a Path,
    pub(crate) maximum_dialogue_page_boundary_path: &'a Path,
    pub(crate) maximum_dialogue_runtime_evidence_path: Option<&'a Path>,
    pub(crate) title_graphics_localization_path: &'a Path,
    pub(crate) title_logo_asset_path: &'a Path,
    pub(crate) title_logo_runtime_evidence_path: &'a Path,
    pub(crate) stage_directory: &'a Path,
    pub(crate) output_path: &'a Path,
    pub(crate) report_path: &'a Path,
}

pub(crate) fn build_cumulative_patch(
    inputs: CumulativePatchInputs<'_>,
) -> Result<CumulativePatchSummary> {
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

    let ui_stage_rom_path = inputs.stage_directory.join(UI_STAGE_ROM_NAME);
    let ui_stage_report_path = inputs.stage_directory.join(UI_STAGE_REPORT_NAME);
    let ui_stage = build_mapper165_hangul_page_probe(
        inputs.source_path,
        inputs.options_localization_path,
        inputs.roster_localization_path,
        inputs.options_screen_evidence_path,
        &ui_stage_rom_path,
        &ui_stage_report_path,
    )?;
    let ui_stage_bytes = fs::read(&ui_stage_rom_path)
        .with_context(|| format!("read cumulative UI stage {}", ui_stage_rom_path.display()))?;
    ensure!(
        sha1_hex(&ui_stage_bytes) == ui_stage.output_sha1,
        "cumulative UI stage hash changed after production"
    );
    let ui_stage_rom =
        Rom::parse(ui_stage_bytes.clone()).context("parse cumulative mapper 165 UI stage")?;
    ensure!(
        ui_stage_rom.mapper() == OUTPUT_MAPPER,
        "cumulative UI stage mapper changed"
    );
    ensure!(
        ui_stage_rom.data()[5] == 19,
        "cumulative UI stage CHR bank count changed"
    );

    let chapter_one_plans = CHAPTER_ONE_INTRO_RECORD_IDS
        .iter()
        .map(|record_id| {
            plan_main_dialogue_slice(&source_rom, inputs.main_dialogue_workspace_path, record_id)
        })
        .collect::<Result<Vec<_>>>()?;
    ensure!(
        chapter_one_plans[0].transition_chain_record_count == chapter_one_plans.len(),
        "Chapter 1 intro cumulative record set no longer covers its transition chain"
    );
    ensure!(
        chapter_one_plans
            .windows(2)
            .all(|pair| pair[0].workspace_sha1 == pair[1].workspace_sha1),
        "cumulative dialogue plans came from different workspaces"
    );
    ensure!(
        chapter_one_plans[0].workspace_sha1 == dialogue_workspace.workspace_sha1,
        "cumulative dialogue plan no longer matches the validated workspace"
    );
    let chapter_one_title = chapter_title_plan.entry(CHAPTER_ONE_INDEX)?.clone();
    let mut chapter_one_glyphs = chapter_one_plans
        .iter()
        .flat_map(MainDialogueSlicePlan::unique_glyphs)
        .collect::<BTreeSet<_>>();
    chapter_one_glyphs.extend(chapter_one_title.unique_glyphs());
    let chapter_one_preserved_source_codes = chapter_one_plans
        .iter()
        .flat_map(|plan| plan.preserved_source_codes.iter().copied())
        .collect::<BTreeSet<_>>();
    let chapter_one_physical_chr_page = u8::try_from(ui_stage_rom.chr().len() / FONT_PAGE_SIZE)
        .context("Chapter 1 cumulative dialogue physical CHR page does not fit u8")?;
    ensure!(
        chapter_one_physical_chr_page == 38 && chapter_one_physical_chr_page.is_multiple_of(2),
        "Chapter 1 cumulative dialogue page no longer begins at physical CHR page 38"
    );
    let chapter_one_page = plan_dialogue_lifetime_page(
        &ui_stage_rom,
        inputs.chapter_one_intro_evidence_path,
        SCREEN_ROLE,
        CHAPTER_ONE_INTRO_RECORD_IDS[0],
        &chapter_one_glyphs,
        &chapter_one_preserved_source_codes,
        chapter_one_physical_chr_page,
    )?;
    let chapter_one_encoded_records = chapter_one_plans
        .iter()
        .map(|plan| plan.encoded_bytes(&chapter_one_page.assignments))
        .collect::<Result<Vec<_>>>()?;
    let chapter_one_encoded_title =
        chapter_one_title.encoded_storage_bytes(&chapter_one_page.assignments)?;

    let chapter_one_selector = build_page_routine_at(
        CHAPTER_ONE_DIALOGUE_SELECTOR_ADDRESS,
        chapter_one_page.mapper_register,
        SELECT_RIGHT_FD_CHR_BANK_FOR_PAIR_ADDRESS,
    )?;
    let dialogue_selector_offset = fixed_bank_file_offset(DIALOGUE_SELECTOR_ADDRESS)?;
    let chapter_one_dialogue_selector_offset =
        fixed_bank_file_offset(CHAPTER_ONE_DIALOGUE_SELECTOR_ADDRESS)?;
    let selector_cave_byte_count = usize::from(
        DIALOGUE_SELECTOR_CAVE_END
            .checked_sub(DIALOGUE_SELECTOR_ADDRESS)
            .context("cumulative dialogue selector cave range underflow")?,
    );
    ensure!(
        ui_stage_bytes
            [dialogue_selector_offset..dialogue_selector_offset + selector_cave_byte_count]
            .iter()
            .all(|byte| *byte == 0xFF),
        "cumulative dialogue selector cave is no longer all FF"
    );
    ensure!(
        count_direct_transfers_to_range(
            source_rom.prg(),
            DIALOGUE_SELECTOR_ADDRESS,
            DIALOGUE_SELECTOR_CAVE_END,
        )? == 0,
        "cumulative dialogue selector cave has pre-existing direct transfers"
    );

    let source_roster_selector =
        build_roster_selector(ROSTER_PAGE_REGISTERS[0], ROSTER_PAGE_REGISTERS[1])?;
    let chapter_one_roster_selector = build_chained_roster_selector(
        ROSTER_PAGE_REGISTERS[0],
        ROSTER_PAGE_REGISTERS[1],
        CHAPTER_ONE_DIALOGUE_SELECTOR_ADDRESS,
    )?;
    ensure!(
        source_roster_selector.len() == chapter_one_roster_selector.len(),
        "cumulative roster selector chaining changed routine size"
    );
    let cumulative_roster_selector = build_chained_roster_selector(
        ROSTER_PAGE_REGISTERS[0],
        ROSTER_PAGE_REGISTERS[1],
        DIALOGUE_SELECTOR_ADDRESS,
    )?;

    let mut chapter_one_expanded_base = ui_stage_bytes.clone();
    chapter_one_expanded_base.extend_from_slice(&chapter_one_page.page_pack);
    ensure!(
        chapter_one_expanded_base.len() == ui_stage_bytes.len() + 2 * FONT_PAGE_SIZE,
        "Chapter 1 cumulative dialogue stage must append one 8 KiB CHR bank"
    );
    let mut chapter_one_image = TrackedImage::new(chapter_one_expanded_base.clone());
    chapter_one_image.write_expected(
        "expand cumulative mapper 165 CHR from 19 to 20 banks",
        5,
        &[19],
        &[20],
    )?;
    for (plan, encoded_record) in chapter_one_plans.iter().zip(&chapter_one_encoded_records) {
        install_dialogue_record(
            &mut chapter_one_image,
            &ui_stage_bytes,
            plan,
            encoded_record,
        )?;
    }
    install_chapter_title(
        &mut chapter_one_image,
        &ui_stage_bytes,
        &chapter_one_title,
        &chapter_one_encoded_title,
    )?;
    chapter_one_image.write_expected(
        "chain roster selector to Chapter 1 intro selector",
        fixed_bank_file_offset(ROSTER_SELECTOR_ADDRESS)?,
        &source_roster_selector,
        &chapter_one_roster_selector,
    )?;
    chapter_one_image.write_expected(
        "Chapter 1 intro cumulative dialogue selector",
        chapter_one_dialogue_selector_offset,
        &vec![0xFF; chapter_one_selector.len()],
        &chapter_one_selector,
    )?;
    chapter_one_image.verify_all_changes_tracked(&chapter_one_expanded_base)?;
    let chapter_one_tracked_write_count = chapter_one_image.writes().len();
    let chapter_one_output = chapter_one_image.into_data();
    let chapter_one_output_sha1 = sha1_hex(&chapter_one_output);
    let chapter_one_output_rom =
        Rom::parse(chapter_one_output.clone()).context("parse Chapter 1 cumulative stage")?;
    verify_cumulative_output(
        &ui_stage_rom,
        &chapter_one_output_rom,
        &chapter_one_page.page_pack,
        &[(&chapter_one_plans[..], &chapter_one_encoded_records[..])],
        &[(&chapter_one_title, &chapter_one_encoded_title)],
        &chapter_one_roster_selector,
        CHAPTER_ONE_DIALOGUE_SELECTOR_ADDRESS,
        &chapter_one_selector,
    )?;
    write_file(
        &inputs.stage_directory.join(CHAPTER_ONE_STAGE_ROM_NAME),
        &chapter_one_output,
    )?;

    let chapter_two_plans = vec![plan_main_dialogue_slice(
        &source_rom,
        inputs.main_dialogue_workspace_path,
        CHAPTER_TWO_INTRO_RECORD_ID,
    )?];
    ensure!(
        chapter_two_plans[0].transition_chain_record_count == chapter_two_plans.len(),
        "Chapter 2 intro record no longer covers its transition chain"
    );
    ensure!(
        chapter_two_plans[0].workspace_sha1 == dialogue_workspace.workspace_sha1,
        "Chapter 2 dialogue plan no longer matches the validated workspace"
    );
    let chapter_two_title = chapter_title_plan.entry(CHAPTER_TWO_INDEX)?.clone();
    let mut chapter_two_glyphs = chapter_two_plans
        .iter()
        .flat_map(MainDialogueSlicePlan::unique_glyphs)
        .collect::<BTreeSet<_>>();
    chapter_two_glyphs.extend(chapter_two_title.unique_glyphs());
    let chapter_two_preserved_source_codes = chapter_two_plans
        .iter()
        .flat_map(|plan| plan.preserved_source_codes.iter().copied())
        .collect::<BTreeSet<_>>();
    let chapter_two_physical_chr_page =
        u8::try_from(chapter_one_output_rom.chr().len() / FONT_PAGE_SIZE)
            .context("Chapter 2 cumulative dialogue physical CHR page does not fit u8")?;
    ensure!(
        chapter_two_physical_chr_page == 40 && chapter_two_physical_chr_page.is_multiple_of(2),
        "Chapter 2 cumulative dialogue page no longer begins at physical CHR page 40"
    );
    let chapter_two_page = plan_dialogue_lifetime_page(
        &chapter_one_output_rom,
        inputs.chapter_two_intro_evidence_path,
        CHAPTER_TWO_SCREEN_ROLE,
        CHAPTER_TWO_INTRO_RECORD_ID,
        &chapter_two_glyphs,
        &chapter_two_preserved_source_codes,
        chapter_two_physical_chr_page,
    )?;
    let chapter_two_encoded_records = chapter_two_plans
        .iter()
        .map(|plan| plan.encoded_bytes(&chapter_two_page.assignments))
        .collect::<Result<Vec<_>>>()?;
    let chapter_two_encoded_title =
        chapter_two_title.encoded_storage_bytes(&chapter_two_page.assignments)?;
    let dialogue_selector = build_chapter_page_selector(
        DIALOGUE_SELECTOR_ADDRESS,
        ChapterPageSequence {
            admitted_chapter_count: 2,
            first_mapper_register: chapter_one_page.mapper_register,
        },
        SELECT_RIGHT_FD_CHR_BANK_FOR_PAIR_ADDRESS,
    )?;
    ensure!(
        chapter_one_page
            .mapper_register
            .checked_add(8)
            .is_some_and(|expected| expected == chapter_two_page.mapper_register),
        "cumulative chapter font pages are no longer contiguous"
    );
    ensure!(
        dialogue_selector.len() <= selector_cave_byte_count,
        "cumulative chapter selector no longer fits its checked FF cave"
    );

    let mut expanded_base = chapter_one_output.clone();
    expanded_base.extend_from_slice(&chapter_two_page.page_pack);
    ensure!(
        expanded_base.len() == chapter_one_output.len() + 2 * FONT_PAGE_SIZE,
        "Chapter 2 cumulative dialogue stage must append one 8 KiB CHR bank"
    );
    let mut image = TrackedImage::new(expanded_base.clone());
    image.write_expected(
        "expand cumulative mapper 165 CHR from 20 to 21 banks",
        5,
        &[20],
        &[21],
    )?;
    for (plan, encoded_record) in chapter_two_plans.iter().zip(&chapter_two_encoded_records) {
        install_dialogue_record(&mut image, &chapter_one_output, plan, encoded_record)?;
    }
    install_chapter_title(
        &mut image,
        &chapter_one_output,
        &chapter_two_title,
        &chapter_two_encoded_title,
    )?;
    image.write_expected(
        "move roster fallback to cumulative chapter selector",
        fixed_bank_file_offset(ROSTER_SELECTOR_ADDRESS)?,
        &chapter_one_roster_selector,
        &cumulative_roster_selector,
    )?;
    let mut expected_selector = vec![0xFF; dialogue_selector.len()];
    let chapter_one_selector_start =
        usize::from(CHAPTER_ONE_DIALOGUE_SELECTOR_ADDRESS - DIALOGUE_SELECTOR_ADDRESS);
    expected_selector[chapter_one_selector_start..].copy_from_slice(&chapter_one_selector);
    image.write_expected(
        "extend cumulative chapter intro selector through Chapter 2",
        dialogue_selector_offset,
        &expected_selector,
        &dialogue_selector,
    )?;
    image.verify_all_changes_tracked(&expanded_base)?;
    let tracked_write_count = chapter_one_tracked_write_count + image.writes().len();
    let chapter_two_output = image.into_data();
    let chapter_two_output_rom =
        Rom::parse(chapter_two_output.clone()).context("parse Chapter 2 cumulative stage")?;
    let mut appended_page_packs = chapter_one_page.page_pack.clone();
    appended_page_packs.extend_from_slice(&chapter_two_page.page_pack);
    verify_cumulative_output(
        &ui_stage_rom,
        &chapter_two_output_rom,
        &appended_page_packs,
        &[
            (&chapter_one_plans[..], &chapter_one_encoded_records[..]),
            (&chapter_two_plans[..], &chapter_two_encoded_records[..]),
        ],
        &[
            (&chapter_one_title, &chapter_one_encoded_title),
            (&chapter_two_title, &chapter_two_encoded_title),
        ],
        &cumulative_roster_selector,
        DIALOGUE_SELECTOR_ADDRESS,
        &dialogue_selector,
    )?;

    let front_end_stage = install_front_end_stage(
        &chapter_two_output,
        &source_rom,
        &front_end_menu_plan,
        inputs.front_end_menu_evidence_path,
        &cumulative_roster_selector,
        &dialogue_selector,
    )?;
    write_file(
        &inputs.stage_directory.join(FRONT_END_STAGE_ROM_NAME),
        &front_end_stage.output,
    )?;
    let unit_name_stage = install_unit_name_stage(
        &front_end_stage.output,
        &source_rom,
        &unit_name_plan,
        inputs.roster_localization_path,
        inputs.unit_name_evidence_path,
    )?;
    write_file(
        &inputs.stage_directory.join(UNIT_NAME_STAGE_ROM_NAME),
        &unit_name_stage.output,
    )?;
    let class_profile_stage = install_class_profile_stage(
        &unit_name_stage.output,
        &source_rom,
        &class_profile_plan,
        inputs.class_profile_evidence_path,
    )?;
    write_file(
        &inputs.stage_directory.join(CLASS_PROFILE_STAGE_ROM_NAME),
        &class_profile_stage.output,
    )?;
    let shop_dialogue_stage = install_shop_dialogue_stage(
        &class_profile_stage.output,
        &source_rom,
        &shop_dialogue_plan,
        unit_name_stage.page.unit_ui_mapper_register,
        inputs.shop_dialogue_evidence_path,
    )?;
    write_file(
        &inputs.stage_directory.join(SHOP_DIALOGUE_STAGE_ROM_NAME),
        &shop_dialogue_stage.output,
    )?;
    let weapon_shop_shared_text_stage = install_weapon_shop_shared_text_stage(
        &shop_dialogue_stage.output,
        &source_rom,
        &shop_dialogue_stage.page,
        &fixed_text_plan,
        &choice_label_plan,
    )?;
    write_file(
        &inputs.stage_directory.join(SHOP_SHARED_TEXT_STAGE_ROM_NAME),
        &weapon_shop_shared_text_stage.output,
    )?;
    let battle_stage = install_battle_stage(BattleStageInputs {
        prior_output: &weapon_shop_shared_text_stage.output,
        source_rom: &source_rom,
        source_path: inputs.source_path,
        fixed_workspace_path: inputs.fixed_text_workspace_path,
        dialogue_workspace_path: inputs.battle_dialogue_workspace_path,
        temporal_manifest_path: inputs.battle_temporal_manifest_path,
        stage_directory: inputs.stage_directory,
    })?;
    ensure!(
        WEAPON_SHOP_ITEM_NAME_SOURCE_INDICES
            .iter()
            .all(|source_index| battle_stage
                .installed_item_source_indices
                .contains(source_index)),
        "weapon-shop item-name projection is no longer a subset of the installed battle catalog"
    );
    let maximum_dialogue_stage = install_maximum_dialogue_stage(MaximumDialogueStageInputs {
        prior_output: &battle_stage.output,
        source_rom: &source_rom,
        record: &maximum_dialogue_plan,
        evidence_path: inputs.maximum_dialogue_evidence_path,
        page_boundary_path: inputs.maximum_dialogue_page_boundary_path,
    })?;
    write_file(
        &inputs.stage_directory.join(MAXIMUM_DIALOGUE_STAGE_ROM_NAME),
        &maximum_dialogue_stage.output,
    )?;
    let maximum_dialogue_runtime = inputs
        .maximum_dialogue_runtime_evidence_path
        .map(|path| {
            verify_maximum_dialogue_runtime_evidence(
                path,
                &maximum_dialogue_stage.output_sha1,
                &maximum_dialogue_plan.workspace_sha1,
                &maximum_dialogue_stage.page.completed_page_pointers,
                &maximum_dialogue_stage.page.page_groups,
                &maximum_dialogue_stage.page.mapper_registers,
            )
        })
        .transpose()?;
    let title_logo_stage = install_title_logo_asset(
        &maximum_dialogue_stage.output,
        &source_rom,
        inputs.title_logo_asset_path,
    )?;
    write_file(
        &inputs.stage_directory.join(TITLE_LOGO_STAGE_ROM_NAME),
        &title_logo_stage.output,
    )?;
    let output = title_logo_stage.output.clone();
    let output_rom = Rom::parse(output.clone()).context("parse cumulative Korean patch")?;
    let tracked_write_count = tracked_write_count
        + front_end_stage.tracked_write_count
        + unit_name_stage.tracked_write_count
        + class_profile_stage.tracked_write_count
        + shop_dialogue_stage.tracked_write_count
        + weapon_shop_shared_text_stage.tracked_write_count
        + battle_stage.tracked_write_count
        + maximum_dialogue_stage.tracked_write_count
        + title_logo_stage.tracked_write_count;

    let translated_line_count = chapter_one_plans
        .iter()
        .chain(&chapter_two_plans)
        .map(|plan| plan.translated_line_count)
        .sum::<usize>()
        + shop_dialogue_plan.translated_line_count
        + maximum_dialogue_plan.translated_line_count;
    let source_storage_byte_count = chapter_one_plans
        .iter()
        .chain(&chapter_two_plans)
        .map(|plan| plan.source_storage_byte_count)
        .sum::<usize>()
        + shop_dialogue_plan.source_record_storage_byte_count
        + maximum_dialogue_plan.source_storage_byte_count;
    let planned_storage_byte_count = chapter_one_encoded_records
        .iter()
        .chain(&chapter_two_encoded_records)
        .map(Vec::len)
        .sum::<usize>()
        + shop_dialogue_plan.planned_record_storage_byte_count
        + maximum_dialogue_stage.page.encoded_record.len();
    let installed_main_dialogue_record_count =
        chapter_one_plans.len() + chapter_two_plans.len() + shop_dialogue_plan.record_ids.len() + 1;
    let installed_dialogue_glyph_slot_count = chapter_one_page.assignments.len()
        + chapter_two_page.assignments.len()
        + weapon_shop_shared_text_stage.plan.page.assignments.len()
        + maximum_dialogue_stage
            .page
            .assignments
            .iter()
            .map(|assignments| assignments.len())
            .sum::<usize>();
    let installed_glyph_slot_count = installed_dialogue_glyph_slot_count
        + front_end_stage.page.assignments.len()
        + unit_name_stage.page.roster_assignments.len()
        + unit_name_stage.page.unit_ui_assignments.len()
        + class_profile_stage.page.assignments[0].len()
        + class_profile_stage.page.assignments[1].len()
        + battle_stage.stable_color_count;
    let output_sha1 = sha1_hex(&output);
    let title_logo_runtime = verify_title_logo_runtime_evidence(
        inputs.title_logo_runtime_evidence_path,
        &title_logo_stage.output_sha1,
    )?;
    let shop_dialogue_runtime = verify_shop_dialogue_runtime_evidence(
        inputs.shop_dialogue_runtime_evidence_path,
        &shop_dialogue_stage.output_sha1,
        shop_dialogue_stage.page.mapper_register,
    )?;
    let class_profile_runtime = verify_class_profile_runtime_evidence(
        inputs.class_profile_runtime_evidence_path,
        &class_profile_stage.output_sha1,
        class_profile_stage.page.mapper_registers[1],
    )?;
    let weapon_shop_shared_text_runtime = verify_weapon_shop_shared_text_runtime_evidence(
        inputs.weapon_shop_shared_text_runtime_evidence_path,
        &weapon_shop_shared_text_stage.output_sha1,
        weapon_shop_shared_text_stage.plan.page.mapper_register,
    )?;
    let weapon_shop_shared_page_total_slot_demand = weapon_shop_shared_text_stage
        .plan
        .page
        .assignments
        .len()
        .checked_add(
            weapon_shop_shared_text_stage
                .plan
                .page
                .preserved_active_code_count,
        )
        .context("weapon-shop shared-page slot demand overflow")?;
    ensure!(
        weapon_shop_shared_page_total_slot_demand <= ACTIVE_HANGUL_SLOT_COUNT,
        "weapon-shop shared page exceeds the active glyph slots"
    );
    let roster_page_total_slot_demand = unit_name_stage
        .page
        .roster_assignments
        .len()
        .checked_add(unit_name_stage.page.preserved_roster_code_count)
        .context("unit-roster page slot demand overflow")?;
    ensure!(
        roster_page_total_slot_demand <= ACTIVE_HANGUL_SLOT_COUNT,
        "unit-roster page exceeds the active glyph slots"
    );
    let weapon_shop_capacity_roles = WEAPON_SHOP_CAPACITY_BOUND_SCREEN_ROLES
        .into_iter()
        .collect::<BTreeSet<_>>();
    ensure!(
        weapon_shop_shared_text_runtime
            .dialogue_screen_roles
            .iter()
            .chain(&weapon_shop_shared_text_runtime.item_name_screen_roles)
            .chain(&weapon_shop_shared_text_runtime.choice_label_screen_roles)
            .all(|role| weapon_shop_capacity_roles.contains(role)),
        "weapon-shop runtime evidence names a screen outside the shared-page capacity contract"
    );
    ensure!(
        sha1_hex(&unit_name_stage.output) == unit_name_stage.output_sha1,
        "unit-name stage output hash changed before class-profile installation"
    );
    ensure!(
        sha1_hex(&front_end_stage.output) == front_end_stage.output_sha1,
        "front-end stage output hash changed before unit-name installation"
    );
    ensure!(
        sha1_hex(&weapon_shop_shared_text_stage.output)
            == weapon_shop_shared_text_stage.output_sha1,
        "weapon-shop shared-text output hash changed before battle installation"
    );
    let chapter_two_output_sha1 = sha1_hex(&chapter_two_output);
    let stages = vec![
        CumulativeStageReport {
            role: "mapper165_options_and_roster",
            output_sha1: ui_stage.output_sha1,
            report_sha1: Some(ui_stage.report_sha1),
        },
        CumulativeStageReport {
            role: "chapter_1_intro_title_and_dialogue_transition_chain",
            output_sha1: chapter_one_output_sha1,
            report_sha1: None,
        },
        CumulativeStageReport {
            role: "chapter_2_intro_title_and_dialogue",
            output_sha1: chapter_two_output_sha1,
            report_sha1: None,
        },
        CumulativeStageReport {
            role: "front_end_menu",
            output_sha1: front_end_stage.output_sha1.clone(),
            report_sha1: None,
        },
        CumulativeStageReport {
            role: "playable_unit_names_for_roster_and_unit_ui",
            output_sha1: unit_name_stage.output_sha1.clone(),
            report_sha1: None,
        },
        CumulativeStageReport {
            role: "automatic_class_profile_titles_and_descriptions",
            output_sha1: class_profile_stage.output_sha1.clone(),
            report_sha1: None,
        },
        CumulativeStageReport {
            role: "weapon_shop_dialogue_branches",
            output_sha1: shop_dialogue_stage.output_sha1.clone(),
            report_sha1: None,
        },
        CumulativeStageReport {
            role: "weapon_shop_shared_item_names_and_choice_labels",
            output_sha1: weapon_shop_shared_text_stage.output_sha1.clone(),
            report_sha1: None,
        },
        CumulativeStageReport {
            role: "battle_text_and_dynamic_composition",
            output_sha1: battle_stage.output_sha1.clone(),
            report_sha1: Some(battle_stage.loader_report_sha1.clone()),
        },
        CumulativeStageReport {
            role: "chapter_7_maximum_dialogue_page_reload",
            output_sha1: maximum_dialogue_stage.output_sha1.clone(),
            report_sha1: None,
        },
        CumulativeStageReport {
            role: "source_bound_korean_title_logo",
            output_sha1: title_logo_stage.output_sha1.clone(),
            report_sha1: None,
        },
    ];
    let report = CumulativePatchReport {
        schema: 1,
        source_sha1: EXPECTED_SOURCE_SHA1,
        output_sha1: output_sha1.clone(),
        output_mapper: output_rom.mapper(),
        prg_size: output_rom.prg().len(),
        chr_size: output_rom.chr().len(),
        stage_count: stages.len(),
        stages,
        chapter_titles: CumulativeChapterTitleReport {
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
        },
        main_dialogue: CumulativeDialogueReport {
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
                    &shop_dialogue_runtime,
                ),
            ],
            maximum_page_reloaded_lifetime: CumulativeMaximumDialogueReport {
                screen_role: MAXIMUM_DIALOGUE_SCREEN_ROLE,
                target_record_id: maximum_dialogue_plan.record_id.clone(),
                workspace_sha1: maximum_dialogue_plan.workspace_sha1.clone(),
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
                preserved_active_code_count: maximum_dialogue_stage
                    .page
                    .preserved_active_code_count,
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
                font_group_selector_byte_count: maximum_dialogue_stage
                    .font_group_selector_byte_count,
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
        },
        options_menu: CumulativeOptionsMenuReport {
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
        },
        front_end_menu: CumulativeFrontEndMenuReport {
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
            preserved_screen_active_code_count: front_end_stage
                .page
                .preserved_screen_active_code_count,
            preserved_source_active_code_count: front_end_stage
                .page
                .preserved_source_active_code_count,
            preserved_active_code_count: front_end_stage.page.preserved_active_code_count,
            font_physical_page: front_end_stage.page.physical_chr_page,
            font_mapper_register: front_end_stage.page.mapper_register,
            font_page_sha1: front_end_stage.page.page_sha1.clone(),
            font_page_pack_sha1: sha1_hex(&front_end_stage.page.page_pack),
            central_fe_companion_refresh_routed: true,
            no_save_source_lifetime_bound: true,
            runtime_variants_bound_to_build: false,
            review_complete: front_end_menu_plan.review_complete,
        },
        playable_unit_names: CumulativeUnitNameReport {
            workspace_sha1: unit_name_plan.workspace_sha1.clone(),
            workspace_entry_count: unit_name_plan.entries.len(),
            unique_glyph_count: unit_name_plan.unique_glyphs().len(),
            roster_page_target_glyph_count: unit_name_stage.page.roster_assignments.len(),
            roster_page_preserved_active_code_count: unit_name_stage
                .page
                .preserved_roster_code_count,
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
        },
        automatic_class_profiles: CumulativeClassProfileReport {
            workspace_sha1: class_profile_plan.workspace_sha1.clone(),
            workspace_entry_count: class_profile_plan.entries.len(),
            installed_entry_count: class_profile_plan.entries.len(),
            installed_description_line_count: class_profile_plan.description_line_count(),
            installed_source_storage_byte_count: class_profile_plan
                .entries
                .iter()
                .map(|entry| {
                    entry.title_source_storage_byte_count
                        + entry.description_source_storage_byte_count
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
            runtime_evidence_manifest_sha1: class_profile_runtime.manifest_sha1.clone(),
            runtime_sample_count: class_profile_runtime.sample_count,
            runtime_unique_image_count: class_profile_runtime.unique_image_count,
            visible_code_count: class_profile_stage.page.visible_code_count,
            preserved_active_code_count: class_profile_stage.page.preserved_active_code_count,
            original_english_digits_and_ui_preserved: true,
            profile_index_page_selector_installed: true,
            runtime_bound_to_build: true,
            review_complete: class_profile_plan.review_complete,
        },
        title_logo: CumulativeTitleLogoReport {
            workspace_sha1: title_graphics_plan.workspace_sha1.clone(),
            asset_sha1: title_logo_stage.asset_sha1.clone(),
            source_owned_tile_count: title_logo_stage.source_owned_tile_count,
            installed_unique_tile_count: title_logo_stage.installed_unique_tile_count,
            installed_tilemap_cell_count: title_logo_stage.installed_tilemap_cell_count,
            physical_chr_page: title_logo_stage.physical_chr_page,
            installed_chr_page_sha1: title_logo_stage.installed_chr_page_sha1.clone(),
            installed_stream_sha1: title_logo_stage.installed_stream_sha1.clone(),
            installed_runtime_overlay_cell_count: title_logo_stage
                .installed_runtime_overlay_cell_count,
            installed_runtime_overlay_stream_sha1: title_logo_stage
                .installed_runtime_overlay_stream_sha1
                .clone(),
            preserved_title_stream_bytes_unchanged: title_logo_stage
                .preserved_title_stream_bytes_unchanged,
            preserved_runtime_overlay_control_bytes_unchanged: title_logo_stage
                .preserved_runtime_overlay_control_bytes_unchanged,
            unassigned_title_chr_patterns_unchanged: title_logo_stage
                .unassigned_title_chr_patterns_unchanged,
            source_sword_sprite_tm_and_copyright_assets_unchanged: true,
            runtime_evidence_manifest_sha1: title_logo_runtime.manifest_sha1.clone(),
            runtime_sample_count: title_logo_runtime.sample_count,
            runtime_unique_image_count: title_logo_runtime.unique_image_count,
            runtime_bound_to_build: true,
            review_complete: title_graphics_plan.review_complete,
        },
        weapon_shop_shared_text: CumulativeWeaponShopSharedTextReport {
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
            shared_page_unique_glyph_count: weapon_shop_shared_text_stage
                .plan
                .page
                .assignments
                .len(),
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
            runtime_evidence_manifest_sha1: weapon_shop_shared_text_runtime.manifest_sha1,
            runtime_evidence_output_sha1: weapon_shop_shared_text_runtime.output_sha1,
            runtime_sample_count: weapon_shop_shared_text_runtime.sample_count,
            runtime_unique_image_count: weapon_shop_shared_text_runtime.unique_image_count,
            runtime_bound_dialogue_screen_roles: weapon_shop_shared_text_runtime
                .dialogue_screen_roles,
            runtime_bound_item_name_screen_roles: weapon_shop_shared_text_runtime
                .item_name_screen_roles,
            runtime_bound_choice_label_screen_roles: weapon_shop_shared_text_runtime
                .choice_label_screen_roles,
            runtime_bound_to_stage_output: true,
            runtime_carried_forward_by_verified_writes: true,
            review_complete: weapon_shop_shared_text_stage.plan.review_complete,
        },
        battle_text: CumulativeBattleTextReport {
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
        },
        selector_chain: vec![
            SelectorChainReport {
                role: "battle_composition",
                cpu_address: format!(
                    "0x{:04X}",
                    CUMULATIVE_RUNTIME_LAYOUT.battle_central_right_fd_selector
                ),
                fallback_role: "maximum_dialogue",
                admitted_chapter_indices: Vec::new(),
            },
            SelectorChainReport {
                role: "maximum_dialogue",
                cpu_address: format!(
                    "0x{:04X}",
                    super::maximum_dialogue_runtime::INITIAL_PAGE_SELECTOR_ADDRESS
                ),
                fallback_role: "unit_roster",
                admitted_chapter_indices: Vec::new(),
            },
            SelectorChainReport {
                role: "unit_roster",
                cpu_address: format!("0x{ROSTER_SELECTOR_ADDRESS:04X}"),
                fallback_role: "unit_summary_and_status",
                admitted_chapter_indices: Vec::new(),
            },
            SelectorChainReport {
                role: "unit_summary_and_status",
                cpu_address: format!("0x{:04X}", super::unit_name_page::PAGE_ROUTINE_ADDRESS),
                fallback_role: "weapon_shop_dialogue",
                admitted_chapter_indices: Vec::new(),
            },
            SelectorChainReport {
                role: "weapon_shop_dialogue",
                cpu_address: format!("0x{SHOP_DIALOGUE_SELECTOR_ADDRESS:04X}"),
                fallback_role: "front_end_menu",
                admitted_chapter_indices: Vec::new(),
            },
            SelectorChainReport {
                role: "front_end_menu",
                cpu_address: format!("0x{:04X}", super::front_end_page::PAGE_ROUTINE_ADDRESS),
                fallback_role: "chapter_intro_dialogue",
                admitted_chapter_indices: Vec::new(),
            },
            SelectorChainReport {
                role: "chapter_intro_dialogue",
                cpu_address: format!("0x{DIALOGUE_SELECTOR_ADDRESS:04X}"),
                fallback_role: "original_pair_aware_selector",
                admitted_chapter_indices: vec![CHAPTER_ONE_INDEX, CHAPTER_TWO_INDEX],
            },
        ],
        original_chr_preserved: false,
        tracked_write_count,
        translation_input_complete: dialogue_workspace.translation_input_complete
            && chapter_title_plan.translated_entry_count == chapter_title_plan.entry_count
            && front_end_menu_plan.entries.len() == 7
            && unit_name_plan.entries.len() == 52
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
            "The installed weapon-shop shared page is capacity-bound to all nine screen roles at 150/210 slots. The decline route is runtime-bound to the eighth-stage output through item selection, choices, continue prompt, item-list return, exit message, and map restoration; the final cumulative output, purchase, and every preflight branch still need exact-output runtime evidence.",
            "Battle text and the dynamic composition loader are installed in this cumulative lineage, but the new cumulative output still needs cold-route battle and prior-screen regression evidence.",
            "The source-bound fifteen-page maximum dialogue has exact-output evidence for its state-bridged Chapter 7 seize entry, initial selector, all page font reloads, irregular temporal samples, and the final NEXT STORY exit; cold-route prior-screen continuity remains open.",
            "The source-bound Korean title logo is exact-output-bound through its initial and completed blink phases, preserved sword, two-cell TM, copyright line, and automatic profile exit. The later defeat-route title return and human visual approval remain open.",
            "The remaining main-dialogue screen lifetimes and translated non-dialogue surfaces are not yet installed in this cumulative lineage.",
            "The ending scroll owns a separate physical copy of all chapter titles; that duplicate consumer is not installed by this intro-title stage.",
            "Human translation review is incomplete, so this output is a development build rather than a release candidate.",
        ],
        release_eligible: false,
    };
    let mut report_bytes =
        serde_json::to_vec_pretty(&report).context("serialize cumulative Korean patch report")?;
    report_bytes.push(b'\n');
    let report_sha1 = sha1_hex(&report_bytes);
    write_file(inputs.output_path, &output)?;
    write_file(inputs.report_path, &report_bytes)?;

    Ok(CumulativePatchSummary {
        output_sha1,
        report_sha1,
        stage_count: report.stage_count,
        installed_dialogue_record_count: installed_main_dialogue_record_count
            + battle_stage.dialogue_record_count,
        installed_dialogue_line_count: translated_line_count
            + battle_stage.dialogue_translated_line_count,
        installed_chapter_title_count: 2,
        installed_glyph_slot_count,
        tracked_write_count,
    })
}

fn dialogue_lifetime_report(
    screen_role: &'static str,
    chapter_index: u8,
    plans: &[MainDialogueSlicePlan],
    encoded_records: &[Vec<u8>],
    page: &super::dialogue_lifetime_page::DialogueLifetimePagePlan,
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

fn shop_dialogue_lifetime_report(
    plan: &MainDialogueBundlePlan,
    page: &super::shop_dialogue_page::ShopDialoguePagePlan,
    runtime: &shop_dialogue_runtime::ShopDialogueRuntimeEvidence,
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
        runtime_evidence_manifest_sha1: Some(runtime.manifest_sha1.clone()),
        runtime_sample_count: runtime.sample_count,
        runtime_unique_image_count: runtime.unique_image_count,
        runtime_bound_to_dialogue_stage_output: true,
    }
}

fn write_file(path: &Path, data: &[u8]) -> Result<()> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)
            .with_context(|| format!("create cumulative output directory {}", parent.display()))?;
    }
    fs::write(path, data).with_context(|| format!("write {}", path.display()))
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use super::*;

    #[test]
    fn cumulative_selector_addresses_do_not_overlap() {
        let roster_selector =
            build_roster_selector(ROSTER_PAGE_REGISTERS[0], ROSTER_PAGE_REGISTERS[1]).unwrap();
        let dialogue_selector = build_chapter_page_selector(
            DIALOGUE_SELECTOR_ADDRESS,
            ChapterPageSequence {
                admitted_chapter_count: 2,
                first_mapper_register: 0x98,
            },
            SELECT_RIGHT_FD_CHR_BANK_FOR_PAIR_ADDRESS,
        )
        .unwrap();

        assert!(
            usize::from(ROSTER_SELECTOR_ADDRESS) + roster_selector.len()
                <= usize::from(DIALOGUE_SELECTOR_ADDRESS)
        );
        assert!(
            usize::from(DIALOGUE_SELECTOR_ADDRESS) + dialogue_selector.len()
                <= usize::from(DIALOGUE_SELECTOR_CAVE_END)
        );
    }

    #[test]
    fn stage_paths_stay_below_the_requested_directory() {
        let directory = PathBuf::from("out/cumulative-stages");
        assert_eq!(
            directory.join(UI_STAGE_ROM_NAME),
            PathBuf::from("out/cumulative-stages/mapper165-ui.nes")
        );
        assert_eq!(
            directory.join(UI_STAGE_REPORT_NAME),
            PathBuf::from("out/cumulative-stages/mapper165-ui.json")
        );
        assert_eq!(
            directory.join(CHAPTER_ONE_STAGE_ROM_NAME),
            PathBuf::from("out/cumulative-stages/chapter1-intro.nes")
        );
        assert_eq!(
            directory.join(FRONT_END_STAGE_ROM_NAME),
            PathBuf::from("out/cumulative-stages/front-end-menu.nes")
        );
        assert_eq!(
            directory.join(UNIT_NAME_STAGE_ROM_NAME),
            PathBuf::from("out/cumulative-stages/unit-names.nes")
        );
        assert_eq!(
            directory.join(CLASS_PROFILE_STAGE_ROM_NAME),
            PathBuf::from("out/cumulative-stages/class-profiles.nes")
        );
        assert_eq!(
            directory.join(SHOP_SHARED_TEXT_STAGE_ROM_NAME),
            PathBuf::from("out/cumulative-stages/weapon-shop-shared-text.nes")
        );
    }
}
