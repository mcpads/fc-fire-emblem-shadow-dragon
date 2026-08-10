use std::{collections::BTreeSet, fs, path::Path};

use anyhow::{Context, Result, ensure};

use crate::{
    chapter_transition::plan_chapter_titles,
    dialogue_assets::{
        MainDialogueSlicePlan, plan_main_dialogue_slice, validate_main_dialogue_workspace,
    },
    font_slots::FONT_PAGE_SIZE,
    front_end_menu::plan_front_end_menu,
    mmc5_prg::{count_direct_transfers_to_range, fixed_bank_file_offset},
    rom::{EXPECTED_SOURCE_SHA1, Rom},
    sha1_hex,
    tracked::TrackedImage,
};

use super::{
    OUTPUT_MAPPER, SELECT_RIGHT_FD_CHR_BANK_FOR_PAIR_ADDRESS,
    dialogue_lifetime_page::{SCREEN_ROLE, build_page_routine_at, plan_dialogue_lifetime_page},
    dialogue_probe_font::assignment_sha1,
    hangul_page_probe::build_mapper165_hangul_page_probe,
    roster_page::{
        PAGE_REGISTERS as ROSTER_PAGE_REGISTERS, PAGE_ROUTINE_ADDRESS as ROSTER_SELECTOR_ADDRESS,
        build_page_routine as build_roster_selector,
        build_page_routine_with_fallback as build_chained_roster_selector,
    },
};

mod chapter_page_selector;
mod front_end_stage;
mod report;
mod verify;

use chapter_page_selector::{ChapterPageSequence, build_chapter_page_selector};
use front_end_stage::install_front_end_stage;
use report::{
    CumulativeChapterTitleReport, CumulativeDialogueLifetimeReport, CumulativeDialogueReport,
    CumulativeFrontEndMenuReport, CumulativePatchReport, CumulativeStageReport,
    SelectorChainReport,
};
use verify::{install_chapter_title, install_dialogue_record, verify_cumulative_output};

const UI_STAGE_ROM_NAME: &str = "mapper165-ui.nes";
const UI_STAGE_REPORT_NAME: &str = "mapper165-ui.json";
const CHAPTER_ONE_STAGE_ROM_NAME: &str = "chapter1-intro.nes";
const FRONT_END_STAGE_ROM_NAME: &str = "front-end-menu.nes";
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
    pub(crate) main_dialogue_workspace_path: &'a Path,
    pub(crate) chapter_title_localization_path: &'a Path,
    pub(crate) front_end_menu_localization_path: &'a Path,
    pub(crate) chapter_one_intro_evidence_path: &'a Path,
    pub(crate) chapter_two_intro_evidence_path: &'a Path,
    pub(crate) front_end_menu_evidence_path: &'a Path,
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
    let front_end_menu_plan =
        plan_front_end_menu(&source_rom, inputs.front_end_menu_localization_path)?;
    ensure!(
        front_end_menu_plan.entries.len() == 7,
        "cumulative front-end menu scope no longer has seven entries"
    );

    let ui_stage_rom_path = inputs.stage_directory.join(UI_STAGE_ROM_NAME);
    let ui_stage_report_path = inputs.stage_directory.join(UI_STAGE_REPORT_NAME);
    let ui_stage = build_mapper165_hangul_page_probe(
        inputs.source_path,
        inputs.options_localization_path,
        inputs.roster_localization_path,
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
    let output = front_end_stage.output;
    let output_rom = Rom::parse(output.clone()).context("parse cumulative Korean patch")?;
    let tracked_write_count = tracked_write_count + front_end_stage.tracked_write_count;

    let translated_line_count = chapter_one_plans
        .iter()
        .chain(&chapter_two_plans)
        .map(|plan| plan.translated_line_count)
        .sum::<usize>();
    let source_storage_byte_count = chapter_one_plans
        .iter()
        .chain(&chapter_two_plans)
        .map(|plan| plan.source_storage_byte_count)
        .sum::<usize>();
    let planned_storage_byte_count = chapter_one_encoded_records
        .iter()
        .chain(&chapter_two_encoded_records)
        .map(Vec::len)
        .sum::<usize>();
    let installed_record_count = chapter_one_plans.len() + chapter_two_plans.len();
    let installed_dialogue_glyph_slot_count =
        chapter_one_page.assignments.len() + chapter_two_page.assignments.len();
    let installed_glyph_slot_count =
        installed_dialogue_glyph_slot_count + front_end_stage.page.assignments.len();
    let output_sha1 = sha1_hex(&output);
    ensure!(
        output_sha1 == front_end_stage.output_sha1,
        "front-end stage output hash changed before publication"
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
            output_sha1: output_sha1.clone(),
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
            installed_record_count,
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
            ],
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
        selector_chain: vec![
            SelectorChainReport {
                role: "unit_roster",
                cpu_address: format!("0x{ROSTER_SELECTOR_ADDRESS:04X}"),
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
        original_chr_preserved: true,
        tracked_write_count,
        translation_input_complete: dialogue_workspace.translation_input_complete
            && chapter_title_plan.translated_entry_count == chapter_title_plan.entry_count
            && front_end_menu_plan.entries.len() == 7,
        review_complete: dialogue_workspace.review_complete
            && chapter_title_plan.review_complete
            && front_end_menu_plan.review_complete,
        runtime_verified: false,
        unresolved: vec![
            "The translated Chapter 1 and Chapter 2 title bars need cold-route runtime regression together with every installed dialogue page and natural map restoration.",
            "Private observations passed the installed no-save and valid-save front-end variants, but installed runtime evidence is not yet build-bound and the suspend-data variant is unverified.",
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
        installed_dialogue_record_count: installed_record_count,
        installed_dialogue_line_count: translated_line_count,
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
    }
}
