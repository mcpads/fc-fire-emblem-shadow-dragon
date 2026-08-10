use std::{collections::BTreeSet, fs, path::Path};

use anyhow::{Context, Result, ensure};

use crate::{
    dialogue_assets::{
        MainDialogueSlicePlan, plan_main_dialogue_slice, validate_main_dialogue_workspace,
    },
    font_slots::FONT_PAGE_SIZE,
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

mod report;
mod verify;

use report::{
    CumulativeDialogueReport, CumulativePatchReport, CumulativeStageReport, SelectorChainReport,
};
use verify::{install_dialogue_record, verify_cumulative_output};

const UI_STAGE_ROM_NAME: &str = "mapper165-ui.nes";
const UI_STAGE_REPORT_NAME: &str = "mapper165-ui.json";
const DIALOGUE_SELECTOR_ADDRESS: u16 = 0xFBD8;
const DIALOGUE_SELECTOR_END: u16 = 0xFC20;
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
    pub(crate) unique_glyph_count: usize,
    pub(crate) tracked_write_count: usize,
}

pub(crate) struct CumulativePatchInputs<'a> {
    pub(crate) source_path: &'a Path,
    pub(crate) options_localization_path: &'a Path,
    pub(crate) roster_localization_path: &'a Path,
    pub(crate) main_dialogue_workspace_path: &'a Path,
    pub(crate) chapter_one_intro_evidence_path: &'a Path,
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

    let plans = CHAPTER_ONE_INTRO_RECORD_IDS
        .iter()
        .map(|record_id| {
            plan_main_dialogue_slice(&source_rom, inputs.main_dialogue_workspace_path, record_id)
        })
        .collect::<Result<Vec<_>>>()?;
    ensure!(
        plans[0].transition_chain_record_count == plans.len(),
        "Chapter 1 intro cumulative record set no longer covers its transition chain"
    );
    ensure!(
        plans
            .windows(2)
            .all(|pair| pair[0].workspace_sha1 == pair[1].workspace_sha1),
        "cumulative dialogue plans came from different workspaces"
    );
    ensure!(
        plans[0].workspace_sha1 == dialogue_workspace.workspace_sha1,
        "cumulative dialogue plan no longer matches the validated workspace"
    );
    let glyphs = plans
        .iter()
        .flat_map(MainDialogueSlicePlan::unique_glyphs)
        .collect::<BTreeSet<_>>();
    let preserved_source_codes = plans
        .iter()
        .flat_map(|plan| plan.preserved_source_codes.iter().copied())
        .collect::<BTreeSet<_>>();
    let physical_chr_page = u8::try_from(ui_stage_rom.chr().len() / FONT_PAGE_SIZE)
        .context("cumulative dialogue physical CHR page does not fit u8")?;
    ensure!(
        physical_chr_page == 38 && physical_chr_page.is_multiple_of(2),
        "cumulative dialogue page no longer begins at physical CHR page 38"
    );
    let lifetime_page = plan_dialogue_lifetime_page(
        &ui_stage_rom,
        inputs.chapter_one_intro_evidence_path,
        CHAPTER_ONE_INTRO_RECORD_IDS[0],
        &glyphs,
        &preserved_source_codes,
        physical_chr_page,
    )?;
    let encoded_records = plans
        .iter()
        .map(|plan| plan.encoded_bytes(&lifetime_page.assignments))
        .collect::<Result<Vec<_>>>()?;

    let dialogue_selector = build_page_routine_at(
        DIALOGUE_SELECTOR_ADDRESS,
        lifetime_page.mapper_register,
        SELECT_RIGHT_FD_CHR_BANK_FOR_PAIR_ADDRESS,
    )?;
    ensure!(
        DIALOGUE_SELECTOR_ADDRESS as usize + dialogue_selector.len()
            == DIALOGUE_SELECTOR_END as usize,
        "cumulative dialogue selector size changed"
    );
    let dialogue_selector_offset = fixed_bank_file_offset(DIALOGUE_SELECTOR_ADDRESS)?;
    ensure!(
        ui_stage_bytes
            [dialogue_selector_offset..dialogue_selector_offset + dialogue_selector.len()]
            .iter()
            .all(|byte| *byte == 0xFF),
        "cumulative dialogue selector cave is no longer all FF"
    );
    ensure!(
        count_direct_transfers_to_range(
            source_rom.prg(),
            DIALOGUE_SELECTOR_ADDRESS,
            DIALOGUE_SELECTOR_END,
        )? == 0,
        "cumulative dialogue selector cave has pre-existing direct transfers"
    );

    let source_roster_selector =
        build_roster_selector(ROSTER_PAGE_REGISTERS[0], ROSTER_PAGE_REGISTERS[1])?;
    let cumulative_roster_selector = build_chained_roster_selector(
        ROSTER_PAGE_REGISTERS[0],
        ROSTER_PAGE_REGISTERS[1],
        DIALOGUE_SELECTOR_ADDRESS,
    )?;
    ensure!(
        source_roster_selector.len() == cumulative_roster_selector.len(),
        "cumulative roster selector chaining changed routine size"
    );

    let mut expanded_base = ui_stage_bytes.clone();
    expanded_base.extend_from_slice(&lifetime_page.page_pack);
    ensure!(
        expanded_base.len() == ui_stage_bytes.len() + 2 * FONT_PAGE_SIZE,
        "cumulative dialogue stage must append one 8 KiB CHR bank"
    );
    let mut image = TrackedImage::new(expanded_base.clone());
    image.write_expected(
        "expand cumulative mapper 165 CHR from 19 to 20 banks",
        5,
        &[19],
        &[20],
    )?;
    for (plan, encoded_record) in plans.iter().zip(&encoded_records) {
        install_dialogue_record(&mut image, &ui_stage_bytes, plan, encoded_record)?;
    }
    image.write_expected(
        "chain roster selector to Chapter 1 intro selector",
        fixed_bank_file_offset(ROSTER_SELECTOR_ADDRESS)?,
        &source_roster_selector,
        &cumulative_roster_selector,
    )?;
    image.write_expected(
        "Chapter 1 intro cumulative dialogue selector",
        dialogue_selector_offset,
        &vec![0xFF; dialogue_selector.len()],
        &dialogue_selector,
    )?;
    image.verify_all_changes_tracked(&expanded_base)?;
    let tracked_write_count = image.writes().len();
    let output = image.into_data();
    let output_rom = Rom::parse(output.clone()).context("parse cumulative Korean patch")?;
    verify_cumulative_output(
        &ui_stage_rom,
        &output_rom,
        &lifetime_page.page_pack,
        &plans,
        &encoded_records,
        &cumulative_roster_selector,
        &dialogue_selector,
    )?;

    let translated_line_count = plans
        .iter()
        .map(|plan| plan.translated_line_count)
        .sum::<usize>();
    let source_storage_byte_count = plans
        .iter()
        .map(|plan| plan.source_storage_byte_count)
        .sum::<usize>();
    let planned_storage_byte_count = encoded_records.iter().map(Vec::len).sum::<usize>();
    let output_sha1 = sha1_hex(&output);
    let report = CumulativePatchReport {
        schema: 1,
        source_sha1: EXPECTED_SOURCE_SHA1,
        output_sha1: output_sha1.clone(),
        output_mapper: output_rom.mapper(),
        prg_size: output_rom.prg().len(),
        chr_size: output_rom.chr().len(),
        stage_count: 2,
        stages: vec![
            CumulativeStageReport {
                role: "mapper165_options_and_roster",
                output_sha1: ui_stage.output_sha1,
                report_sha1: Some(ui_stage.report_sha1),
            },
            CumulativeStageReport {
                role: "chapter_1_intro_dialogue_transition_chain",
                output_sha1: output_sha1.clone(),
                report_sha1: None,
            },
        ],
        main_dialogue: CumulativeDialogueReport {
            screen_role: SCREEN_ROLE,
            workspace_sha1: plans[0].workspace_sha1.clone(),
            workspace_record_count: dialogue_workspace.record_count,
            workspace_filled_line_count: dialogue_workspace.filled_line_count,
            screen_evidence_manifest_sha1: lifetime_page.manifest_sha1,
            installed_record_count: plans.len(),
            installed_translated_line_count: translated_line_count,
            source_storage_byte_count,
            planned_storage_byte_count,
            remaining_storage_byte_count: source_storage_byte_count - planned_storage_byte_count,
            unique_glyph_count: lifetime_page.assignments.len(),
            glyph_assignment_sha1: assignment_sha1(&lifetime_page.assignments),
            preserved_screen_active_code_count: lifetime_page.preserved_screen_active_code_count,
            preserved_source_active_code_count: lifetime_page.preserved_source_active_code_count,
            preserved_active_code_count: lifetime_page.preserved_active_code_count,
            temporal_sample_count: lifetime_page.temporal_sample_count,
            unique_nametable_count: lifetime_page.unique_nametable_count,
            font_physical_page: lifetime_page.physical_chr_page,
            font_mapper_register: lifetime_page.mapper_register,
            font_page_sha1: lifetime_page.page_sha1,
            font_page_pack_sha1: sha1_hex(&lifetime_page.page_pack),
        },
        selector_chain: vec![
            SelectorChainReport {
                role: "unit_roster",
                cpu_address: format!("0x{ROSTER_SELECTOR_ADDRESS:04X}"),
                fallback_role: "chapter_1_intro_dialogue",
            },
            SelectorChainReport {
                role: "chapter_1_intro_dialogue",
                cpu_address: format!("0x{DIALOGUE_SELECTOR_ADDRESS:04X}"),
                fallback_role: "original_pair_aware_selector",
            },
        ],
        original_chr_preserved: true,
        tracked_write_count,
        translation_input_complete: dialogue_workspace.translation_input_complete,
        review_complete: dialogue_workspace.review_complete,
        runtime_verified: false,
        unresolved: vec![
            "The cumulative static build needs a cold runtime regression across title, options, roster, every Chapter 1 intro dialogue page, and natural map restoration.",
            "The remaining main-dialogue screen lifetimes and translated non-dialogue surfaces are not yet installed in this cumulative lineage.",
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
        installed_dialogue_record_count: plans.len(),
        installed_dialogue_line_count: translated_line_count,
        unique_glyph_count: lifetime_page.assignments.len(),
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
        let dialogue_selector = build_page_routine_at(
            DIALOGUE_SELECTOR_ADDRESS,
            0x98,
            SELECT_RIGHT_FD_CHR_BANK_FOR_PAIR_ADDRESS,
        )
        .unwrap();

        assert!(
            usize::from(ROSTER_SELECTOR_ADDRESS) + roster_selector.len()
                <= usize::from(DIALOGUE_SELECTOR_ADDRESS)
        );
        assert_eq!(
            usize::from(DIALOGUE_SELECTOR_ADDRESS) + dialogue_selector.len(),
            usize::from(DIALOGUE_SELECTOR_END)
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
    }
}
