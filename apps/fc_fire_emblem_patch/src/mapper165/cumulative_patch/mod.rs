use std::{collections::BTreeSet, fs, path::Path};

use anyhow::{Context, Result, ensure};

use crate::{
    dialogue_assets::{MainDialogueSlicePlan, plan_main_dialogue_slice},
    font_slots::{ACTIVE_HANGUL_SLOT_COUNT, FONT_PAGE_SIZE},
    mmc5_prg::{count_direct_transfers_to_range, fixed_bank_file_offset},
    rom::Rom,
    sha1_hex,
    title_graphics::install_title_logo_asset,
    tracked::TrackedImage,
};

use super::{
    OUTPUT_MAPPER, SELECT_RIGHT_FD_CHR_BANK_FOR_PAIR_ADDRESS, assemble_mapper165_parity_bytes,
    bind_cumulative_font_page_fallback_graph,
    dialogue_lifetime_page::{SCREEN_ROLE, build_page_routine_at, plan_dialogue_lifetime_page},
    hangul_page_probe::build_mapper165_hangul_page_probe_from_parity,
    roster_page::{
        PAGE_REGISTERS as ROSTER_PAGE_REGISTERS, PAGE_ROUTINE_ADDRESS as ROSTER_SELECTOR_ADDRESS,
        build_page_routine as build_roster_selector,
        build_page_routine_with_fallback as build_chained_roster_selector,
    },
    weapon_shop_shared_text::ITEM_NAME_SOURCE_INDICES as WEAPON_SHOP_ITEM_NAME_SOURCE_INDICES,
};

mod battle_stage;
mod class_profile_runtime;
mod class_profile_stage;
mod front_end_stage;
mod input_plan;
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

use super::chapter_page_selector::{ChapterPageSequence, build_chapter_page_selector};
use battle_stage::{BattleStageInputs, install_battle_stage};
use class_profile_runtime::verify_class_profile_runtime_evidence;
use class_profile_stage::install_class_profile_stage;
use front_end_stage::install_front_end_stage;
use input_plan::{CumulativeInputPlan, prepare_cumulative_inputs};
use maximum_dialogue_runtime_evidence::verify_maximum_dialogue_runtime_evidence;
use maximum_dialogue_stage::{MaximumDialogueStageInputs, install_maximum_dialogue_stage};
use report::{CumulativeReportInputs, assemble_cumulative_report};
use shop_dialogue_runtime::verify_shop_dialogue_runtime_evidence;
use shop_dialogue_stage::install_shop_dialogue_stage;
use title_logo_runtime::load_title_logo_runtime_evidence;
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
pub(crate) const REPORT_SCHEMA: u8 = 3;
pub(super) const DIALOGUE_SELECTOR_ADDRESS: u16 = 0xFBD4;
pub(super) const DIALOGUE_SELECTOR_CAVE_END: u16 = 0xFC20;
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
    pub(crate) class_profile_runtime_evidence_path: Option<&'a Path>,
    pub(crate) shop_dialogue_evidence_path: &'a Path,
    pub(crate) shop_dialogue_runtime_evidence_path: Option<&'a Path>,
    pub(crate) weapon_shop_shared_text_runtime_evidence_path: Option<&'a Path>,
    pub(crate) maximum_dialogue_evidence_path: &'a Path,
    pub(crate) maximum_dialogue_page_boundary_path: &'a Path,
    pub(crate) maximum_dialogue_runtime_evidence_path: Option<&'a Path>,
    pub(crate) title_graphics_localization_path: &'a Path,
    pub(crate) title_logo_asset_path: &'a Path,
    pub(crate) title_logo_runtime_evidence_path: Option<&'a Path>,
    pub(crate) stage_directory: &'a Path,
    pub(crate) output_path: &'a Path,
    pub(crate) report_path: &'a Path,
}

pub(crate) fn build_cumulative_patch(
    inputs: CumulativePatchInputs<'_>,
) -> Result<CumulativePatchSummary> {
    let input_plan = prepare_cumulative_inputs(&inputs)?;
    let CumulativeInputPlan {
        ref source_rom,
        ref dialogue_workspace,
        ref chapter_title_plan,
        ref front_end_menu_plan,
        ref front_end_result_preserved_codes,
        ref unit_name_plan,
        ref class_profile_plan,
        ref fixed_text_plan,
        ref choice_label_plan,
        ref shop_dialogue_plan,
        ref maximum_dialogue_plan,
        ..
    } = input_plan;
    let parity = assemble_mapper165_parity_bytes(source_rom)?;
    let ui_stage_rom_path = inputs.stage_directory.join(UI_STAGE_ROM_NAME);
    let ui_stage_report_path = inputs.stage_directory.join(UI_STAGE_REPORT_NAME);
    let ui_stage = build_mapper165_hangul_page_probe_from_parity(
        source_rom,
        &parity,
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
        &front_end_result_preserved_codes,
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
        source_rom,
        parity: &parity,
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
    let selector_fallback_graph = bind_cumulative_font_page_fallback_graph(&output_rom)?;
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
    let title_logo_runtime = load_title_logo_runtime_evidence(
        inputs.title_logo_runtime_evidence_path,
        &title_logo_stage.output_sha1,
    )?;
    let shop_dialogue_runtime = inputs
        .shop_dialogue_runtime_evidence_path
        .map(|path| {
            verify_shop_dialogue_runtime_evidence(
                path,
                &shop_dialogue_stage.output_sha1,
                shop_dialogue_stage.page.mapper_register,
            )
        })
        .transpose()?;
    let class_profile_runtime = inputs
        .class_profile_runtime_evidence_path
        .map(|path| {
            verify_class_profile_runtime_evidence(
                path,
                &class_profile_stage.output_sha1,
                class_profile_stage.page.mapper_registers[1],
            )
        })
        .transpose()?;
    let weapon_shop_shared_text_runtime = inputs
        .weapon_shop_shared_text_runtime_evidence_path
        .map(|path| {
            verify_weapon_shop_shared_text_runtime_evidence(
                path,
                &weapon_shop_shared_text_stage.output_sha1,
                weapon_shop_shared_text_stage.plan.page.mapper_register,
            )
        })
        .transpose()?;
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
    if let Some(runtime) = &weapon_shop_shared_text_runtime {
        ensure!(
            runtime
                .dialogue_screen_roles
                .iter()
                .chain(&runtime.item_name_screen_roles)
                .chain(&runtime.choice_label_screen_roles)
                .all(|role| weapon_shop_capacity_roles.contains(role)),
            "weapon-shop runtime evidence names a screen outside the shared-page capacity contract"
        );
    }
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
    let report = assemble_cumulative_report(CumulativeReportInputs {
        input_plan: &input_plan,
        output_sha1: output_sha1.clone(),
        output_rom: &output_rom,
        ui_stage: &ui_stage,
        chapter_one_output_sha1,
        chapter_two_output_sha1,
        chapter_one_plans: &chapter_one_plans,
        chapter_two_plans: &chapter_two_plans,
        chapter_one_title: &chapter_one_title,
        chapter_two_title: &chapter_two_title,
        chapter_one_encoded_records: &chapter_one_encoded_records,
        chapter_two_encoded_records: &chapter_two_encoded_records,
        chapter_one_encoded_title: &chapter_one_encoded_title,
        chapter_two_encoded_title: &chapter_two_encoded_title,
        chapter_one_page: &chapter_one_page,
        chapter_two_page: &chapter_two_page,
        front_end_stage: &front_end_stage,
        unit_name_stage: &unit_name_stage,
        class_profile_stage: &class_profile_stage,
        shop_dialogue_stage: &shop_dialogue_stage,
        weapon_shop_shared_text_stage: &weapon_shop_shared_text_stage,
        battle_stage: &battle_stage,
        maximum_dialogue_stage: &maximum_dialogue_stage,
        title_logo_stage: &title_logo_stage,
        maximum_dialogue_runtime: &maximum_dialogue_runtime,
        title_logo_runtime: &title_logo_runtime,
        shop_dialogue_runtime: &shop_dialogue_runtime,
        class_profile_runtime: &class_profile_runtime,
        weapon_shop_shared_text_runtime: &weapon_shop_shared_text_runtime,
        selector_fallback_graph: &selector_fallback_graph,
        tracked_write_count,
        translated_line_count,
        source_storage_byte_count,
        planned_storage_byte_count,
        installed_main_dialogue_record_count,
        installed_dialogue_glyph_slot_count,
        weapon_shop_shared_page_total_slot_demand,
        roster_page_total_slot_demand,
    });
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
