use std::{fs, path::Path};

use anyhow::{Context, Result, ensure};
use serde::Serialize;

use crate::{
    mmc5_prg::{SOURCE_RESET_ADDRESS, count_direct_transfers_to_range, fixed_bank_file_offset},
    rom::{CHR_FILE_OFFSET, EXPECTED_CHR_SHA1, EXPECTED_SOURCE_SHA1, PRG_SIZE, Rom},
    sha1_hex,
    static_analysis::find_absolute_write_candidates,
    tracked::TrackedImage,
};
pub(crate) mod banked_call_dispatch;
mod battle_cache_coverage;
pub(crate) mod battle_cache_upload_probe;
pub(crate) mod battle_codebook_plan;
pub(crate) mod battle_combination_probe;
pub(crate) mod battle_composition_loader_probe;
pub(crate) mod battle_composition_runtime_verify;
pub(crate) mod battle_dialogue_probe;
pub(crate) mod battle_text_cache_probe;
pub(crate) mod battle_text_runtime_base;
mod carried_battle_domains;
mod carried_ui_domains;
mod chapter_page_selector;
mod class_profile_page;
pub(crate) mod cumulative_patch;
mod dialogue_lifetime_page;
pub(crate) mod dialogue_probe_font;
pub(crate) mod dialogue_slice_probe;
pub(crate) mod direct_chr_pairs;
pub(crate) mod executable_mapper_writes;
mod final_font_page_forwarders;
mod font_page_fallback_graph;
pub(crate) mod font_pair_projection;
mod front_end_page;
pub(crate) use carried_battle_domains::{
    CarriedBattleDomainInputs, CarriedBattleDomainPreservation, FinalBattleConsumerRoute,
    FinalBattleConsumerRouteRegion, inspect_carried_battle_domains,
};
pub(crate) use carried_ui_domains::{
    CarriedUiDomainInputs, CarriedUiDomainPreservation, FinalConsumerRouteRegion,
    FinalRosterConsumerRoute, inspect_carried_ui_domains,
};
pub(crate) use final_font_page_forwarders::BoundFontPageSelector;
pub(crate) use final_font_page_forwarders::{
    bind_front_end_font_page_selector, bind_unit_name_font_page_selector,
    build_front_end_font_page_forwarder, build_unit_name_font_page_forwarder,
};
pub(crate) use font_page_fallback_graph::{
    BoundFontPageFallbackGraph, FontPageFallbackNodeRole, bind_cumulative_font_page_fallback_graph,
};
pub(crate) use front_end_page::bind_installed_front_end_mapper_register;
pub(crate) mod hangul_page_probe;
pub(crate) mod inline_pointer_dispatch;
mod maximum_dialogue_boundary;
mod maximum_dialogue_page;
pub(crate) mod maximum_dialogue_rebinding;
mod maximum_dialogue_runtime;
mod options_lifetime;
mod options_page;
mod roster_page;
mod runtime;
pub(crate) mod selector_safety;
mod shop_dialogue_page;
pub(crate) mod source_code_binding;
mod source_indexed_mapper_aliases;
mod source_mapper_write_audit;
#[cfg(test)]
mod tests;
pub(crate) mod trigger_planes;
mod trigger_variants;
mod unit_name_page;
mod unit_name_table;
mod weapon_shop_shared_text;
mod writer_census;
mod writer_sites;

pub(crate) use weapon_shop_shared_text::{
    ITEM_LIST_POINTER_LOAD_ADDRESS, ITEM_LIST_POINTER_LOAD_BYTES, ITEM_LIST_POINTER_LOAD_PRG_BANK,
    build_item_list_pointer_load_call,
};

pub(crate) const ROSTER_HEADER_FIXED_STRING_INDEX: u8 = roster_page::HEADER_RESOURCE_ID;
pub(crate) use options_page::{BoundOptionsCompositeLifetime, bind_options_composite_lifetime};
pub(crate) const OPTIONS_FONT_PAGE_COMPOSITE_STATES: [u8; 2] =
    options_page::OPTIONS_COMPOSITE_LIFETIME_STATES;
pub(crate) const ROSTER_FONT_PAGE_COMPOSITE_STATE: u8 = roster_page::COMPOSITE_STATE;

use runtime::{
    build_routines, replace_central_chr_writer, replace_central_prg_writer, replace_direct_writer,
    replace_mirroring_writer, validate_routine_placements,
};
use source_indexed_mapper_aliases::{
    SourceIndexedMapperAliasSafety, bind_source_indexed_mapper_aliases,
    install_guarded_indexed_menu_stores, verify_installed_guarded_indexed_menu_stores,
};
use source_mapper_write_audit::{SourceMapperWriteAudit, audit_source_mapper_writes};

use trigger_variants::{
    TriggerVariantPlan, install_observed_trigger_variants, verify_installed_trigger_variants,
};

use writer_census::{AbsoluteChrWriterCensus, bind_absolute_chr_writer_census};
use writer_sites::{CENTRAL_CHR_WRITERS, DIRECT_CHR_WRITERS, SOURCE_PRG_BANK_WRITERS};

const OUTPUT_MAPPER: u16 = 165;
const OUTPUT_CHR_PADDING_SIZE: usize = 8 * 1024;
const OUTPUT_CHR_BANK_COUNT: u8 = 17;
pub(crate) const FIRST_EXTENSION_CHR_PAGE: u8 = OUTPUT_CHR_BANK_COUNT * 2;
pub(crate) const MAXIMUM_CHR_PAGE_COUNT: u8 = 64;
const RESET_INITIALIZER_ADDRESS: u16 = 0xFA00;
const SELECT_PRG_BANK_ADDRESS: u16 = 0xFA20;
const SELECT_LEFT_FD_CHR_BANK_ADDRESS: u16 = 0xFA40;
const SELECT_LEFT_FE_CHR_BANK_ADDRESS: u16 = 0xFA60;
const SELECT_RIGHT_FD_CHR_BANK_ADDRESS: u16 = 0xFA80;
const SELECT_RIGHT_FE_CHR_BANK_ADDRESS: u16 = 0xFAA0;
const SELECT_CENTRAL_RIGHT_FE_CHR_BANK_ADDRESS: u16 = 0xFAB8;
const SELECT_RIGHT_FD_CHR_BANK_FOR_PAIR_ADDRESS: u16 = 0xFAC0;
const CODE_CAVE_START_ADDRESS: u16 = RESET_INITIALIZER_ADDRESS;
const CODE_CAVE_LEN: usize = 0x110;

const SOURCE_SELECT_PRG_BANK_AND_SAVE_ADDRESS: u16 = 0xC9A6;
const SOURCE_SELECT_HORIZONTAL_MIRRORING_ADDRESS: u16 = 0xC9CE;
const SOURCE_SELECT_VERTICAL_MIRRORING_ADDRESS: u16 = 0xC9D6;

pub(crate) fn encode_chr_page_register(physical_page: u8) -> Result<u8> {
    ensure!(
        physical_page < MAXIMUM_CHR_PAGE_COUNT,
        "physical CHR page {physical_page} exceeds mapper 165 capacity"
    );
    if physical_page == 0 {
        return Ok(1);
    }
    physical_page
        .checked_mul(4)
        .ok_or_else(|| anyhow::anyhow!("physical CHR page cannot be encoded for mapper 165"))
}

#[derive(Debug, Serialize)]
struct Mapper165ParityReport {
    schema: u32,
    source_sha1: &'static str,
    output_sha1: String,
    source_mapper: u16,
    output_mapper: u16,
    prg_size: usize,
    chr_size: usize,
    source_chr_sha1: &'static str,
    relocated_source_chr_sha1: String,
    output_chr_sha1: String,
    battery_flag_preserved: bool,
    chr_layout: ChrLayoutEvidence,
    trigger_plane_correction: TriggerPlaneCorrectionEvidence,
    code_cave: CodeCaveEvidence,
    direct_code_cave_transfer_count: usize,
    routines: Vec<RoutinePlacement>,
    prg_writer_count: usize,
    central_chr_writer_count: usize,
    direct_chr_writer_count: usize,
    source_indexed_mapper_alias_safety: SourceIndexedMapperAliasSafety,
    source_mapper_write_audit: SourceMapperWriteAudit,
    absolute_chr_writer_census: AbsoluteChrWriterCensus,
    tracked_writes: Vec<TrackedWrite>,
    unresolved_boundaries: Vec<&'static str>,
    release_eligible: bool,
}

#[derive(Debug, Serialize)]
struct ChrLayoutEvidence {
    reserved_prefix_size: usize,
    source_chr_offset: usize,
    source_4k_page_bias: u8,
    maximum_4k_chr_rom_pages: usize,
    remaining_4k_pages_at_maximum_size: usize,
}

#[derive(Debug, Serialize)]
struct TriggerPlaneCorrectionEvidence {
    installed_variants: Vec<InstalledTriggerVariantEvidence>,
    selector_entries: Vec<PairSelectorEvidence>,
    central_right_writers_pair_aware: bool,
    direct_writers_pair_aware: bool,
}

#[derive(Debug, Serialize)]
struct InstalledTriggerVariantEvidence {
    physical_4k_page: u8,
    mapper_register_value: u8,
    fd_source_page: u8,
    required_high_plane_sha1: String,
    compatible_fe_source_pages: Vec<u8>,
    pattern_windows: Vec<&'static str>,
}

#[derive(Debug, Serialize)]
struct PairSelectorEvidence {
    pattern_window: &'static str,
    fd_source_page: u8,
    fe_source_page: u8,
    mapper_register_value: u8,
}

#[derive(Debug, Serialize)]
struct CodeCaveEvidence {
    cpu_start: String,
    file_start: String,
    len: usize,
    expected_fill: &'static str,
}

#[derive(Debug, Serialize)]
struct RoutinePlacement {
    role: &'static str,
    cpu_address: String,
    len: usize,
}

#[derive(Debug, Serialize)]
struct TrackedWrite {
    label: String,
    file_offset: String,
    len: usize,
}

pub struct BuildSummary {
    pub output_sha1: String,
    pub report_sha1: String,
    pub tracked_write_count: usize,
}

struct AssembledParityImage {
    output: Vec<u8>,
    trigger_variant_plan: TriggerVariantPlan,
    cave_file_start: usize,
    direct_code_cave_transfer_count: usize,
    routines: Vec<runtime::AssembledRoutine>,
    tracked_writes: Vec<TrackedWrite>,
    source_mapper_write_audit: SourceMapperWriteAudit,
    source_indexed_mapper_alias_safety: SourceIndexedMapperAliasSafety,
    absolute_chr_writer_census: AbsoluteChrWriterCensus,
}

pub fn build_mapper165_parity_probe(
    source_path: &Path,
    output_path: &Path,
    report_path: &Path,
) -> Result<BuildSummary> {
    let source_rom = Rom::from_path(source_path)?;
    source_rom.verify_supported_japanese()?;
    let assembled = assemble_mapper165_parity_image(&source_rom)?;
    let output_rom =
        Rom::parse(assembled.output.clone()).context("parse mapper 165 parity probe")?;
    let output_sha1 = sha1_hex(&assembled.output);
    let relocated_source_chr_sha1 = sha1_hex(&output_rom.chr()[OUTPUT_CHR_PADDING_SIZE..]);
    let output_chr_sha1 = sha1_hex(output_rom.chr());
    let report = Mapper165ParityReport {
        schema: 9,
        source_sha1: EXPECTED_SOURCE_SHA1,
        output_sha1: output_sha1.clone(),
        source_mapper: source_rom.mapper(),
        output_mapper: output_rom.mapper(),
        prg_size: output_rom.prg().len(),
        chr_size: output_rom.chr().len(),
        source_chr_sha1: EXPECTED_CHR_SHA1,
        relocated_source_chr_sha1,
        output_chr_sha1,
        battery_flag_preserved: true,
        chr_layout: ChrLayoutEvidence {
            reserved_prefix_size: OUTPUT_CHR_PADDING_SIZE,
            source_chr_offset: OUTPUT_CHR_PADDING_SIZE,
            source_4k_page_bias: 2,
            maximum_4k_chr_rom_pages: 64,
            remaining_4k_pages_at_maximum_size: 30,
        },
        trigger_plane_correction: TriggerPlaneCorrectionEvidence {
            installed_variants: assembled
                .trigger_variant_plan
                .installed_variants
                .iter()
                .map(|variant| InstalledTriggerVariantEvidence {
                    physical_4k_page: variant.physical_page,
                    mapper_register_value: variant.mapper_register_value,
                    fd_source_page: variant.fd_source_page,
                    required_high_plane_sha1: sha1_hex(&variant.required_high_plane),
                    compatible_fe_source_pages: variant.compatible_fe_source_pages.clone(),
                    pattern_windows: variant
                        .pattern_windows
                        .iter()
                        .map(|window| window.label())
                        .collect(),
                })
                .collect(),
            selector_entries: assembled
                .trigger_variant_plan
                .selector_entries
                .iter()
                .map(|entry| PairSelectorEvidence {
                    pattern_window: entry.pattern_window.label(),
                    fd_source_page: entry.fd_source_page,
                    fe_source_page: entry.fe_source_page,
                    mapper_register_value: entry.mapper_register_value,
                })
                .collect(),
            central_right_writers_pair_aware: true,
            direct_writers_pair_aware: false,
        },
        code_cave: CodeCaveEvidence {
            cpu_start: format!("0x{CODE_CAVE_START_ADDRESS:04X}"),
            file_start: format!("0x{:06X}", assembled.cave_file_start),
            len: CODE_CAVE_LEN,
            expected_fill: "0xFF",
        },
        direct_code_cave_transfer_count: assembled.direct_code_cave_transfer_count,
        routines: assembled
            .routines
            .iter()
            .map(|routine| RoutinePlacement {
                role: routine.role,
                cpu_address: format!("0x{:04X}", routine.cpu_address),
                len: routine.bytes.len(),
            })
            .collect(),
        prg_writer_count: SOURCE_PRG_BANK_WRITERS.len() + 1,
        central_chr_writer_count: CENTRAL_CHR_WRITERS.len(),
        direct_chr_writer_count: DIRECT_CHR_WRITERS.len(),
        source_indexed_mapper_alias_safety: assembled.source_indexed_mapper_alias_safety,
        source_mapper_write_audit: assembled.source_mapper_write_audit,
        absolute_chr_writer_census: assembled.absolute_chr_writer_census,
        tracked_writes: assembled.tracked_writes,
        unresolved_boundaries: vec![
            "Observed central PPU $1000 pairs use generated trigger-plane variants; unobserved pairs still require visible parity measurement.",
            "All sixteen exact source STA $7FEE,X sites in banks 06 and 0B are redirected through bank-local bounded routines; executable ownership for every other source effective write remains incomplete, and the final mapper165 image still requires its own $8000-$FFFF denominator.",
            "Converted direct-writer values are source-bound, but their complete runtime FD/FE co-lifetime population still requires battle-phase validation.",
            "The probe preserves and relocates the source CHR but does not add Korean glyphs or translation assets.",
            "Runtime parity covers suspend persistence, one adverse game-over path, and the chapter-one completion/save/cold-load transition, not whole-game regression.",
        ],
        release_eligible: false,
    };
    let report_bytes =
        serde_json::to_vec_pretty(&report).context("serialize mapper 165 parity report")?;
    let report_sha1 = sha1_hex(&report_bytes);
    let tracked_write_count = report.tracked_writes.len();

    write_file(output_path, &assembled.output)?;
    write_file(report_path, &report_bytes)?;
    Ok(BuildSummary {
        output_sha1,
        report_sha1,
        tracked_write_count,
    })
}

fn assemble_mapper165_parity_image(source_rom: &Rom) -> Result<AssembledParityImage> {
    verify_complete_prg_writer_inventory(source_rom)?;
    let source_mapper_write_audit = audit_source_mapper_writes(source_rom)?;
    let source_indexed_mapper_alias_safety = bind_source_indexed_mapper_aliases(source_rom)?;
    let absolute_chr_writer_census = bind_absolute_chr_writer_census(source_rom)?;
    selector_safety::bind_source_contract(source_rom)?;

    let (base, trigger_variant_plan) = create_chr_relocated_image(source_rom)?;
    let cave_file_start = fixed_bank_file_offset(CODE_CAVE_START_ADDRESS)?;
    let cave_file_end = cave_file_start
        .checked_add(CODE_CAVE_LEN)
        .ok_or_else(|| anyhow::anyhow!("mapper 165 code cave range overflow"))?;
    ensure!(
        base[cave_file_start..cave_file_end]
            .iter()
            .all(|byte| *byte == 0xFF),
        "mapper 165 code cave is no longer all FF"
    );
    let direct_code_cave_transfer_count = count_direct_transfers_to_range(
        source_rom.prg(),
        CODE_CAVE_START_ADDRESS,
        CODE_CAVE_START_ADDRESS + CODE_CAVE_LEN as u16,
    )?;
    ensure!(
        direct_code_cave_transfer_count == 0,
        "mapper 165 code cave has {direct_code_cave_transfer_count} direct JSR or JMP references"
    );

    let routines = build_routines(&trigger_variant_plan.selector_entries)?;
    validate_routine_placements(&routines)?;
    let mut image = TrackedImage::new(base.clone());
    image.write_expected("iNES mapper low nibble 10 to 165", 6, &[0xA2], &[0x52])?;
    image.write_expected("iNES mapper high nibble 10 to 165", 7, &[0x00], &[0xA0])?;
    image.write_expected(
        "reserve two CHR pages before the source CHR",
        5,
        &[0x10],
        &[OUTPUT_CHR_BANK_COUNT],
    )?;

    for routine in &routines {
        image.write_expected(
            format!("mapper 165 {} routine", routine.role),
            fixed_bank_file_offset(routine.cpu_address)?,
            &vec![0xFF; routine.bytes.len()],
            &routine.bytes,
        )?;
    }

    install_guarded_indexed_menu_stores(&mut image)?;

    replace_central_prg_writer(&mut image)?;
    selector_safety::install_source_hooks(&mut image)?;
    for writer in SOURCE_PRG_BANK_WRITERS {
        replace_direct_writer(&mut image, *writer)?;
    }
    for writer in CENTRAL_CHR_WRITERS {
        replace_central_chr_writer(&mut image, *writer)?;
    }
    for writer in DIRECT_CHR_WRITERS {
        replace_direct_writer(&mut image, *writer)?;
    }
    replace_mirroring_writer(
        &mut image,
        "horizontal mirroring selector",
        SOURCE_SELECT_HORIZONTAL_MIRRORING_ADDRESS,
        1,
    )?;
    replace_mirroring_writer(
        &mut image,
        "vertical mirroring selector",
        SOURCE_SELECT_VERTICAL_MIRRORING_ADDRESS,
        0,
    )?;
    image.write_expected(
        "reset vector to mapper 165 initializer",
        fixed_bank_file_offset(0xFFFC)?,
        &SOURCE_RESET_ADDRESS.to_le_bytes(),
        &RESET_INITIALIZER_ADDRESS.to_le_bytes(),
    )?;

    image.verify_all_changes_tracked(&base)?;
    let tracked_writes = image
        .writes()
        .iter()
        .map(|write| TrackedWrite {
            label: write.label.clone(),
            file_offset: format!("0x{:06X}", write.offset),
            len: write.len,
        })
        .collect::<Vec<_>>();
    let output = image.into_data();
    let output_rom = Rom::parse(output.clone()).context("parse mapper 165 parity image")?;
    verify_output(source_rom, &output_rom, &output, &trigger_variant_plan)?;
    selector_safety::verify_installed_contract(&output_rom)?;
    selector_safety::verify_parity_nonindexed_absolute_mapper_select_store(&output_rom)?;
    verify_installed_guarded_indexed_menu_stores(&output_rom)?;

    Ok(AssembledParityImage {
        output,
        trigger_variant_plan,
        cave_file_start,
        direct_code_cave_transfer_count,
        routines,
        tracked_writes,
        source_mapper_write_audit,
        source_indexed_mapper_alias_safety,
        absolute_chr_writer_census,
    })
}

pub(super) fn assemble_mapper165_parity_bytes(source_rom: &Rom) -> Result<Vec<u8>> {
    Ok(assemble_mapper165_parity_image(source_rom)?.output)
}

fn create_chr_relocated_image(source_rom: &Rom) -> Result<(Vec<u8>, TriggerVariantPlan)> {
    let output_len = source_rom
        .data()
        .len()
        .checked_add(OUTPUT_CHR_PADDING_SIZE)
        .ok_or_else(|| anyhow::anyhow!("mapper 165 output size overflow"))?;
    let mut output = Vec::with_capacity(output_len);
    output.extend_from_slice(&source_rom.data()[..CHR_FILE_OFFSET]);
    output.resize(output.len() + OUTPUT_CHR_PADDING_SIZE, 0);
    output.extend_from_slice(source_rom.chr());
    ensure!(
        output.len() == output_len,
        "mapper 165 CHR relocation size mismatch"
    );
    let trigger_variant_plan = install_observed_trigger_variants(
        source_rom.chr(),
        &mut output[CHR_FILE_OFFSET..CHR_FILE_OFFSET + OUTPUT_CHR_PADDING_SIZE],
    )?;
    Ok((output, trigger_variant_plan))
}

fn verify_complete_prg_writer_inventory(source_rom: &Rom) -> Result<()> {
    let candidates = find_absolute_write_candidates(source_rom.prg(), 0xA000);
    ensure!(
        candidates.len() == SOURCE_PRG_BANK_WRITERS.len() + 1,
        "source $A000 write inventory changed: expected {}, found {}",
        SOURCE_PRG_BANK_WRITERS.len() + 1,
        candidates.len()
    );
    ensure!(
        candidates.iter().all(|candidate| candidate.opcode == 0x8D),
        "source $A000 inventory contains a non-STA writer"
    );
    let mut actual = candidates
        .iter()
        .map(|candidate| candidate.cpu_address)
        .collect::<Vec<_>>();
    actual.sort_unstable();
    let mut expected = SOURCE_PRG_BANK_WRITERS
        .iter()
        .map(|writer| writer.source_address)
        .chain(std::iter::once(0xC9AA))
        .collect::<Vec<_>>();
    expected.sort_unstable();
    ensure!(actual == expected, "source $A000 writer addresses changed");
    Ok(())
}

fn verify_output(
    source_rom: &Rom,
    output_rom: &Rom,
    output: &[u8],
    trigger_variant_plan: &TriggerVariantPlan,
) -> Result<()> {
    ensure!(
        output_rom.mapper() == OUTPUT_MAPPER,
        "output mapper is not 165"
    );
    ensure!(
        output_rom.prg().len() == PRG_SIZE,
        "mapper 165 changed PRG size"
    );
    ensure!(
        output_rom.chr().len() == source_rom.chr().len() + OUTPUT_CHR_PADDING_SIZE,
        "mapper 165 output CHR size is incorrect"
    );
    verify_installed_trigger_variants(
        source_rom.chr(),
        &output_rom.chr()[..OUTPUT_CHR_PADDING_SIZE],
        trigger_variant_plan,
    )?;
    ensure!(
        output_rom.chr()[OUTPUT_CHR_PADDING_SIZE..] == *source_rom.chr(),
        "mapper 165 relocated source CHR changed"
    );
    ensure!(
        output[6] & 0x02 == source_rom.data()[6] & 0x02,
        "mapper 165 changed the iNES battery flag"
    );
    Ok(())
}

fn write_file(path: &Path, data: &[u8]) -> Result<()> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)
            .with_context(|| format!("create output directory {}", parent.display()))?;
    }
    fs::write(path, data).with_context(|| format!("write {}", path.display()))
}
