use std::{fs, path::Path};

use anyhow::{Context, Result, ensure};
use serde::Serialize;

use crate::{
    dialogue_inventory::inspect_chapter_intro_contexts,
    rom::{EXPECTED_SOURCE_SHA1, HEADER_SIZE, Rom},
    sha1_hex,
    typed_source::{TypedInstructionBinding, decode_rp2a03_sequence},
};

const PRG_BANK_SIZE: usize = 16 * 1024;
const SWITCHABLE_CPU_START: u16 = 0x8000;
const FIXED_CPU_START: u16 = 0xC000;

const CHAPTER_INDEX_ADDRESS: u16 = 0x781D;
const CHAPTER_TITLE_POINTER_TABLE_ADDRESS: u16 = 0xEE08;
const CHAPTER_TITLE_COUNT: usize = 25;
const CHAPTER_TITLE_DATA_START: usize = 0x3EE4A;
const CHAPTER_TITLE_DATA_END_EXCLUSIVE: usize = 0x3EFC7;
const CHAPTER_TITLE_TERMINATOR: u8 = 0xED;
const CHAPTER_TITLE_DIGIT_COUNT: usize = 41;
const CHAPTER_INTRO_SHARED_PAYLOAD: [u8; 4] = [0x30, 0x10, 0x14, 0x04];

const CHAPTER_TITLE_POINTER_TABLE_BYTES: &[u8] = &[
    0x3A, 0xEE, 0x49, 0xEE, 0x59, 0xEE, 0x67, 0xEE, 0x76, 0xEE, 0x85, 0xEE, 0x94, 0xEE, 0xA3, 0xEE,
    0xB1, 0xEE, 0xC0, 0xEE, 0xD0, 0xEE, 0xE1, 0xEE, 0xF0, 0xEE, 0x00, 0xEF, 0x11, 0xEF, 0x21, 0xEF,
    0x2F, 0xEF, 0x3E, 0xEF, 0x4E, 0xEF, 0x5E, 0xEF, 0x6E, 0xEF, 0x7D, 0xEF, 0x8B, 0xEF, 0x9A, 0xEF,
    0xA8, 0xEF,
];

const NEXT_STORY_COMPOSER_BYTES: &[u8] = &[
    0xA9, 0x04, 0x8D, 0xD0, 0x05, 0xA9, 0x0E, 0x8D, 0xCF, 0x05, 0xA9, 0x40, 0x85, 0x71, 0xA9, 0x60,
    0x85, 0x70, 0x20, 0x3C, 0x8E, 0xA9, 0x10, 0x8D, 0xF5, 0x06, 0xA9, 0x3E, 0x20, 0xEE, 0x8E, 0xA9,
    0xEF, 0x9D, 0x51, 0x04, 0x4C, 0x39, 0x8F,
];
const CHAPTER_TITLE_COMPOSER_BYTES: &[u8] = &[
    0x20, 0x3C, 0x8E, 0xAD, 0x1D, 0x78, 0x20, 0xE0, 0x8E, 0xA9, 0xEF, 0x9D, 0x51, 0x04, 0x4C, 0x39,
    0x8F,
];
const SAVE_OFFER_COMPOSER_BYTES: &[u8] = &[
    0xA9, 0x0C, 0x8D, 0xCF, 0x05, 0xA9, 0x04, 0x8D, 0xD0, 0x05, 0xA9, 0x60, 0x85, 0x70, 0xA9, 0x50,
    0x85, 0x71, 0x20, 0x3C, 0x8E, 0xA9, 0x32, 0x20, 0xEE, 0x8E, 0xA9, 0xEF, 0x9D, 0x51, 0x04, 0x4C,
    0x39, 0x8F,
];
const NEXT_STORY_POINTER_BYTES: &[u8] = &[0xFB, 0x91];
const NEXT_STORY_LABEL_BYTES: &[u8] = &[
    0x77, 0x6E, 0x81, 0x7D, 0xFF, 0x7C, 0x7D, 0x78, 0x7B, 0x82, 0xED,
];
const SAVE_OFFER_POINTER_BYTES: &[u8] = &[0xAA, 0x91];
const SAVE_OFFER_LABEL_BYTES: &[u8] = &[
    0x3D, 0x3F, 0x4C, 0x0F, 0x0B, 0x20, 0x0C, 0x05, 0xFF, 0x9C, 0xED,
];
const REGULAR_SAVE_CHECKSUM_BYTES: &[u8] = &[
    0x38, 0xA5, 0x02, 0xE5, 0x00, 0x85, 0x02, 0xA5, 0x03, 0xE5, 0x01, 0x85, 0x03, 0xA9, 0x00, 0x85,
    0x04, 0x85, 0x05, 0xA8, 0xA6, 0x02, 0xF0, 0x02, 0xE6, 0x03, 0xB1, 0x00, 0x18, 0x65, 0x04, 0x85,
    0x04, 0x90, 0x02, 0xE6, 0x05, 0xC8, 0xD0, 0x02, 0xE6, 0x01, 0xC6, 0x02, 0xD0, 0xEC, 0xC6, 0x03,
    0xD0, 0xE8, 0x60,
];
const WRITE_REGULAR_FILE_ONE_CHECKSUM_BYTES: &[u8] = &[
    0xA9, 0x00, 0x85, 0x00, 0xA9, 0x60, 0x85, 0x01, 0xA9, 0x42, 0x85, 0x02, 0xA9, 0x65, 0x85, 0x03,
    0x20, 0x52, 0x9D, 0xA5, 0x04, 0x8D, 0x42, 0x65, 0xA5, 0x05, 0x8D, 0x43, 0x65, 0xA2, 0x03, 0xBD,
    0x4E, 0x9D, 0x9D, 0x88, 0x6A, 0xCA, 0x10, 0xF7, 0xEE, 0xEE, 0x05, 0x60,
];
const VALIDATE_REGULAR_SAVE_CHECKSUM_BYTES: &[u8] = &[
    0xA5, 0x67, 0xD0, 0x23, 0xA9, 0x00, 0x85, 0x00, 0xA9, 0x60, 0x85, 0x01, 0xA9, 0x42, 0x85, 0x02,
    0xA9, 0x65, 0x85, 0x03, 0x20, 0x52, 0x9D, 0xA5, 0x04, 0xCD, 0x42, 0x65, 0xD0, 0x31, 0xA5, 0x05,
    0xCD, 0x43, 0x65, 0xD0, 0x2A, 0xF0, 0x21, 0xA9, 0x44, 0x85, 0x00, 0xA9, 0x65, 0x85, 0x01, 0xA9,
    0x86, 0x85, 0x02, 0xA9, 0x6A, 0x85, 0x03, 0x20, 0x52, 0x9D, 0xA5, 0x04, 0xCD, 0x86, 0x6A, 0xD0,
    0x0E, 0xA5, 0x05, 0xCD, 0x87, 0x6A, 0xD0, 0x07, 0x20, 0x2D, 0xC7, 0xEE, 0xEE, 0x05, 0x60, 0xA9,
    0x06, 0x8D, 0xEE, 0x05, 0x60,
];
const DISPATCH_OUTER_SCREEN_STATE_BYTES: &[u8] = &[0xA5, 0x24, 0x20, 0x4C, 0xC3];
const OUTER_SCREEN_0D_HANDLER_POINTER_BYTES: &[u8] = &[0xAC, 0xB5];
const DISPATCH_CHAPTER_TRANSITION_STATE_BYTES: &[u8] =
    &[0x20, 0x88, 0xC2, 0xA5, 0x84, 0x20, 0x4C, 0xC3];
const SAVE_OFFER_STATE_POINTERS_BYTES: &[u8] = &[0xF3, 0xB6, 0x26, 0xB7, 0x37, 0xB7];
const RUN_SAVE_OFFER_CHOICE_STATE_BYTES: &[u8] = &[
    0xAD, 0xCC, 0x05, 0xC9, 0x03, 0xF0, 0x15, 0x20, 0x5C, 0xE6, 0xAD, 0xEB, 0x05, 0xC9, 0x02, 0xF0,
    0x1F, 0xA9, 0x0E, 0x85, 0x24, 0xA9, 0x02, 0x85, 0x84, 0x4C, 0xA1, 0xB7, 0xA9, 0x05, 0x8D, 0xCC,
    0x05, 0xAC, 0xCE, 0x05, 0xA9, 0x03, 0x99, 0xEE, 0x7F, 0xA9, 0x00, 0x8D, 0xD3, 0x05, 0xF0, 0x02,
    0xE6, 0x84, 0x60,
];
const CLOSE_SAVE_OFFER_NO_CHOICE_BYTES: &[u8] = &[
    0x20, 0x5C, 0xE6, 0x20, 0x6E, 0xE6, 0xAD, 0xCE, 0x05, 0xD0, 0x05, 0x20, 0x8C, 0xB5, 0xE6, 0x84,
    0x60,
];
const ENTER_NEXT_CHAPTER_WITHOUT_SAVE_BYTES: &[u8] = &[
    0x20, 0x5C, 0xE6, 0xAD, 0xF4, 0x05, 0xF0, 0x09, 0xA9, 0x02, 0x85, 0x44, 0xA9, 0x04, 0x4C, 0xFA,
    0xC9, 0xA9, 0x01, 0x8D, 0xF0, 0x06, 0xAD, 0xE0, 0x05, 0xF0, 0x05, 0xE6, 0x23, 0x4C, 0xC0, 0xFF,
    0xA9, 0x00, 0x8D, 0xEC, 0x05, 0x8D, 0x7D, 0x76, 0x8D, 0x7F, 0x76, 0x85, 0x84, 0x85, 0x60, 0x8D,
    0xF4, 0x05, 0xA9, 0x01, 0x8D, 0x75, 0x76, 0x85, 0x24, 0x60,
];
const READ_ACTIVE_MENU_SELECTION_BYTES: &[u8] = &[
    0xAE, 0xCE, 0x05, 0xCA, 0xBD, 0xEE, 0x7F, 0x20, 0x40, 0x98, 0x85, 0x0B, 0xAE, 0xCE, 0x05, 0xCA,
    0xBC, 0xF3, 0x7F, 0xA5, 0x14, 0xC9, 0x80, 0xF0, 0x45,
];
const ADVANCE_MENU_SELECTION_BYTES: &[u8] = &[
    0xC4, 0x0B, 0xD0, 0x04, 0xA9, 0x01, 0xD0, 0x02, 0xC8, 0x98, 0x9D, 0xF3, 0x7F, 0x20, 0x8D, 0xF1,
    0xD0, 0x2E,
];
const COMMIT_MENU_SELECTION_BYTES: &[u8] = &[
    0x20, 0x87, 0xF1, 0xBD, 0xF3, 0x7F, 0xAA, 0xAC, 0xCE, 0x05, 0x88, 0xB9, 0xEE, 0x7F, 0x20, 0x4D,
    0x98, 0x8D, 0xEB, 0x05,
];

const SOURCE_REGIONS: &[SourceRegionSpec] = &[
    SourceRegionSpec::code(
        "compose_next_story_banner",
        0x0B,
        0x886A,
        NEXT_STORY_COMPOSER_BYTES,
    ),
    SourceRegionSpec::code(
        "compose_chapter_title",
        0x0B,
        0x88C4,
        CHAPTER_TITLE_COMPOSER_BYTES,
    ),
    SourceRegionSpec::code(
        "compose_chapter_save_offer",
        0x0B,
        0x8AE6,
        SAVE_OFFER_COMPOSER_BYTES,
    ),
    SourceRegionSpec::data(
        "chapter_title_pointer_table",
        0x0F,
        CHAPTER_TITLE_POINTER_TABLE_ADDRESS,
        CHAPTER_TITLE_POINTER_TABLE_BYTES,
    ),
    SourceRegionSpec::data("next_story_pointer", 0x0B, 0x903E, NEXT_STORY_POINTER_BYTES),
    SourceRegionSpec::data("next_story_label", 0x0B, 0x91FB, NEXT_STORY_LABEL_BYTES),
    SourceRegionSpec::data(
        "chapter_save_offer_pointer",
        0x0B,
        0x9026,
        SAVE_OFFER_POINTER_BYTES,
    ),
    SourceRegionSpec::data(
        "chapter_save_offer_label",
        0x0B,
        0x91AA,
        SAVE_OFFER_LABEL_BYTES,
    ),
    SourceRegionSpec::code(
        "calculate_regular_save_checksum",
        0x0B,
        0x9D52,
        REGULAR_SAVE_CHECKSUM_BYTES,
    ),
    SourceRegionSpec::code(
        "write_regular_file_one_checksum",
        0x0B,
        0x9AD0,
        WRITE_REGULAR_FILE_ONE_CHECKSUM_BYTES,
    ),
    SourceRegionSpec::code(
        "validate_regular_save_checksum",
        0x0B,
        0x9FA8,
        VALIDATE_REGULAR_SAVE_CHECKSUM_BYTES,
    ),
    SourceRegionSpec::code(
        "dispatch_outer_screen_state",
        0x06,
        0x8400,
        DISPATCH_OUTER_SCREEN_STATE_BYTES,
    ),
    SourceRegionSpec::data(
        "outer_screen_0d_handler_pointer",
        0x06,
        0x841F,
        OUTER_SCREEN_0D_HANDLER_POINTER_BYTES,
    ),
    SourceRegionSpec::code(
        "dispatch_chapter_transition_state",
        0x06,
        0xB5AC,
        DISPATCH_CHAPTER_TRANSITION_STATE_BYTES,
    ),
    SourceRegionSpec::data(
        "save_offer_state_pointers",
        0x06,
        0xB5C2,
        SAVE_OFFER_STATE_POINTERS_BYTES,
    ),
    SourceRegionSpec::code(
        "run_save_offer_choice_state",
        0x06,
        0xB6F3,
        RUN_SAVE_OFFER_CHOICE_STATE_BYTES,
    ),
    SourceRegionSpec::code(
        "close_save_offer_no_choice",
        0x06,
        0xB726,
        CLOSE_SAVE_OFFER_NO_CHOICE_BYTES,
    ),
    SourceRegionSpec::code(
        "enter_next_chapter_without_save",
        0x06,
        0xB737,
        ENTER_NEXT_CHAPTER_WITHOUT_SAVE_BYTES,
    ),
    SourceRegionSpec::code(
        "read_active_menu_selection",
        0x0B,
        0x9333,
        READ_ACTIVE_MENU_SELECTION_BYTES,
    ),
    SourceRegionSpec::code(
        "advance_menu_selection",
        0x0B,
        0x936C,
        ADVANCE_MENU_SELECTION_BYTES,
    ),
    SourceRegionSpec::code(
        "commit_menu_selection",
        0x0B,
        0x9391,
        COMMIT_MENU_SELECTION_BYTES,
    ),
];

#[derive(Clone, Copy)]
enum RegionKind {
    Code,
    Data,
}

#[derive(Clone, Copy)]
struct SourceRegionSpec {
    role: &'static str,
    prg_bank: u8,
    cpu_address: u16,
    bytes: &'static [u8],
    kind: RegionKind,
}

impl SourceRegionSpec {
    const fn code(
        role: &'static str,
        prg_bank: u8,
        cpu_address: u16,
        bytes: &'static [u8],
    ) -> Self {
        Self {
            role,
            prg_bank,
            cpu_address,
            bytes,
            kind: RegionKind::Code,
        }
    }

    const fn data(
        role: &'static str,
        prg_bank: u8,
        cpu_address: u16,
        bytes: &'static [u8],
    ) -> Self {
        Self {
            role,
            prg_bank,
            cpu_address,
            bytes,
            kind: RegionKind::Data,
        }
    }
}

#[derive(Debug, Serialize)]
struct ChapterTransitionReport {
    schema: u8,
    source_sha1: &'static str,
    scope: Scope,
    observed_sequence: Vec<TransitionScreen>,
    chapter_intro_contexts: ChapterIntroContextSummary,
    chapter_titles: ChapterTitleSummary,
    regular_save_reachability: RegularSaveReachability,
    save_offer_no_branch: SaveOfferNoBranchContract,
    chapter_intro_runtime_samples: Vec<ChapterIntroRuntimeSample>,
    fixed_labels: Vec<FixedLabelBinding>,
    source_regions: Vec<SourceRegionBinding>,
    next_universalization_gate: &'static str,
    unresolved: Vec<&'static str>,
    release_eligible: bool,
}

#[derive(Debug, Serialize)]
struct Scope {
    translation_direction: &'static str,
    preserve_existing_english_and_digits: bool,
    dialogue_content_emitted: bool,
    proof_boundary: &'static str,
}

#[derive(Debug, Serialize)]
struct TransitionScreen {
    sequence_order: u8,
    screen_role: &'static str,
    entry_condition: &'static str,
    runtime_observed: bool,
    input_behavior: &'static str,
    visible_components: &'static [&'static str],
    translation_target: &'static str,
    preserved_original: &'static [&'static str],
    runtime_state: RuntimeScreenState,
    observed_chr_pair: ChrPair,
    temporal_behavior: &'static str,
    input_actions: &'static [InputAction],
    unresolved_focus: &'static [&'static str],
}

#[derive(Debug, Serialize)]
struct RuntimeScreenState {
    outer_screen_state: u8,
    outer_screen_state_hex: &'static str,
    main_state: u8,
    main_state_hex: &'static str,
    victory_stage: Option<u8>,
    dialogue_state: Option<u8>,
}

#[derive(Debug, Serialize)]
struct ChrPair {
    left_fd: u8,
    left_fe: u8,
    right_fd: u8,
    right_fe: u8,
}

#[derive(Debug, Serialize)]
struct InputAction {
    input: &'static str,
    immediate_effect: &'static str,
    may_cause_persistent_gameplay_mutation: bool,
    next_role: &'static str,
}

#[derive(Debug, Serialize)]
struct ChapterIntroContextSummary {
    prefix_code: u8,
    prefix_code_hex: &'static str,
    payload_destinations: [u16; 5],
    payload_destination_hex: [&'static str; 5],
    unique_context_count: usize,
    first_chapter_index: u8,
    last_chapter_index: u8,
    chapter_index_address: u16,
    chapter_index_address_hex: &'static str,
    shared_non_index_payload_sha1: String,
    source_entry_indices: Vec<Vec<usize>>,
}

#[derive(Debug, Serialize)]
struct ChapterTitleSummary {
    pointer_table: CodeLocation,
    pointer_count: usize,
    data_file_start: usize,
    data_file_start_hex: String,
    data_file_end_exclusive: usize,
    data_file_end_exclusive_hex: String,
    source_terminator: u8,
    source_terminator_hex: &'static str,
    protected_original_digit_count: usize,
    composer: CodeLocation,
    selector_address: u16,
    selector_address_hex: &'static str,
    translation_target: &'static str,
}

#[derive(Debug, Serialize)]
struct RegularSaveReachability {
    file_one_data_start_address: u16,
    file_one_data_start_address_hex: &'static str,
    file_one_data_end_exclusive_address: u16,
    file_one_data_end_exclusive_address_hex: &'static str,
    file_one_chapter_address: u16,
    file_one_chapter_address_hex: &'static str,
    file_one_checksum_address: u16,
    file_one_checksum_address_hex: &'static str,
    checksum_byte_order: &'static str,
    checksum_algorithm: &'static str,
    chapter_number_basis: &'static str,
    runtime_use: &'static str,
    natural_progression_claimed: bool,
}

#[derive(Debug, Serialize)]
struct SaveOfferNoBranchContract {
    screen_role: &'static str,
    outer_screen_state_address: u16,
    outer_screen_state_address_hex: &'static str,
    offer_outer_screen_state: u8,
    offer_outer_screen_state_hex: &'static str,
    main_state_address: u16,
    main_state_address_hex: &'static str,
    owned_main_state_sequence: [u8; 4],
    owned_main_state_sequence_hex: [&'static str; 4],
    menu_depth_address: u16,
    menu_depth_address_hex: &'static str,
    observed_menu_depth: u8,
    active_selection_address: u16,
    active_selection_address_hex: &'static str,
    default_yes_selection: u8,
    no_selection: u8,
    committed_result_address: u16,
    committed_result_address_hex: &'static str,
    no_committed_result: u8,
    no_branch_exit_outer_state: u8,
    no_branch_exit_outer_state_hex: &'static str,
    no_branch_blackout_chr_pair: ChrPair,
    persistent_save_route_entered: bool,
    next_role: &'static str,
    stable_sample_offsets_frames: [u16; 8],
    stable_sample_screenshot_sha256: &'static str,
    runtime_evidence: &'static str,
}

#[derive(Debug, Serialize)]
struct ChapterIntroRuntimeSample {
    sample_role: &'static str,
    chapter_number_one_based: u8,
    chapter_index_zero_based: u8,
    entry_method: &'static str,
    left_fd_chr_page: u8,
    left_fe_chr_page: u8,
    right_fd_chr_page: u8,
    right_fe_chr_page: u8,
    portrait_visible_in_sample: bool,
    completion_marker_phase_union_observed: bool,
    proof_limit: &'static str,
}

#[derive(Debug, Serialize)]
struct FixedLabelBinding {
    screen_role: &'static str,
    index: u8,
    index_hex: String,
    source_text: &'static str,
    translation_handling: &'static str,
    pointer: u16,
    pointer_hex: String,
    composer: CodeLocation,
}

#[derive(Debug, Serialize)]
struct SourceRegionBinding {
    role: &'static str,
    region_kind: &'static str,
    prg_bank: u8,
    prg_bank_hex: String,
    cpu_address: u16,
    cpu_address_hex: String,
    file_offset: usize,
    file_offset_hex: String,
    byte_count: usize,
    source_sha1: String,
    typed_instructions: Vec<TypedInstructionBinding>,
}

#[derive(Debug, Serialize)]
struct CodeLocation {
    prg_bank: u8,
    prg_bank_hex: String,
    cpu_address: u16,
    cpu_address_hex: String,
}

pub struct ChapterTransitionSummary {
    pub report_sha1: String,
    pub screen_count: usize,
    pub chapter_context_count: usize,
    pub chapter_title_count: usize,
    pub chapter_intro_runtime_sample_count: usize,
    pub source_region_count: usize,
    pub next_observation_gate_role: &'static str,
}

pub fn analyze_chapter_transitions(
    source_path: &Path,
    report_path: &Path,
) -> Result<ChapterTransitionSummary> {
    let rom = Rom::from_path(source_path)?;
    rom.verify_supported_japanese()?;
    let report = build_report(&rom)?;
    let mut report_bytes =
        serde_json::to_vec_pretty(&report).context("serialize chapter-transition report")?;
    report_bytes.push(b'\n');
    let report_sha1 = sha1_hex(&report_bytes);

    if let Some(parent) = report_path.parent() {
        fs::create_dir_all(parent)
            .with_context(|| format!("create output directory {}", parent.display()))?;
    }
    fs::write(report_path, report_bytes)
        .with_context(|| format!("write {}", report_path.display()))?;

    Ok(ChapterTransitionSummary {
        report_sha1,
        screen_count: report.observed_sequence.len(),
        chapter_context_count: report.chapter_intro_contexts.unique_context_count,
        chapter_title_count: report.chapter_titles.pointer_count,
        chapter_intro_runtime_sample_count: report.chapter_intro_runtime_samples.len(),
        source_region_count: report.source_regions.len(),
        next_observation_gate_role: report.next_universalization_gate,
    })
}

fn build_report(rom: &Rom) -> Result<ChapterTransitionReport> {
    let source_regions = SOURCE_REGIONS
        .iter()
        .copied()
        .map(|spec| bind_source_region(rom, spec))
        .collect::<Result<Vec<_>>>()?;
    let chapter_intro_contexts = bind_chapter_intro_contexts(rom)?;
    let chapter_titles = bind_chapter_titles(rom)?;

    Ok(ChapterTransitionReport {
        schema: 1,
        source_sha1: EXPECTED_SOURCE_SHA1,
        scope: Scope {
            translation_direction: "Japanese to Korean",
            preserve_existing_english_and_digits: true,
            dialogue_content_emitted: false,
            proof_boundary: "source-bound chapter context, title, NEXT STORY, save-offer, and regular-save checksum producers plus the runtime-observed chapter-one-to-two sequence, chapter-eleven intro reachability, and continuous accelerated chapter-eleven-victory-to-chapter-twelve-intro route; no dialogue source, translation, or ROM mutation",
        },
        observed_sequence: transition_screens(),
        chapter_intro_contexts,
        chapter_titles,
        regular_save_reachability: regular_save_reachability(),
        save_offer_no_branch: save_offer_no_branch_contract(),
        chapter_intro_runtime_samples: chapter_intro_runtime_samples(),
        fixed_labels: vec![
            FixedLabelBinding {
                screen_role: "next_story_banner",
                index: 0x3E,
                index_hex: "0x3E".to_owned(),
                source_text: "NEXT STORY",
                translation_handling: "preserve original English",
                pointer: 0x91FB,
                pointer_hex: "0x91FB".to_owned(),
                composer: location(0x0B, 0x886A),
            },
            FixedLabelBinding {
                screen_role: "chapter_save_offer",
                index: 0x32,
                index_hex: "0x32".to_owned(),
                source_text: "セーブしますか?",
                translation_handling: "translate Japanese only",
                pointer: 0x91AA,
                pointer_hex: "0x91AA".to_owned(),
                composer: location(0x0B, 0x8AE6),
            },
        ],
        source_regions,
        next_universalization_gate: "chapter_transition_alternate_choice_and_failure_variants",
        unresolved: vec![
            "The chapter-one epilogue and save-complete dialogue use the main dialogue engine, but their dialogue source content is intentionally outside this public report.",
            "The save-offer no choice is source-bound and runtime-observed; the save-complete no-choice route remains unobserved.",
            "The accelerated continuous route establishes reachability but not baseline combat difficulty, defeat, or unfavorable branches.",
            "Chapter-two, chapter-eleven, and chapter-twelve intro samples do not generalize the remaining twenty-two chapters or all title lifetimes.",
        ],
        release_eligible: false,
    })
}

fn bind_chapter_intro_contexts(rom: &Rom) -> Result<ChapterIntroContextSummary> {
    let mut contexts = inspect_chapter_intro_contexts(rom.data())?;
    contexts.sort_by_key(|context| context.chapter_index);
    ensure!(
        contexts.len() == CHAPTER_TITLE_COUNT,
        "expected {CHAPTER_TITLE_COUNT} chapter-intro E5 contexts, found {}",
        contexts.len()
    );
    for (expected_index, context) in contexts.iter().enumerate() {
        ensure!(
            context.chapter_index == expected_index as u8,
            "chapter-intro E5 contexts are not a contiguous 00..18 sequence"
        );
        ensure!(
            context.prefix_payload[..4] == CHAPTER_INTRO_SHARED_PAYLOAD,
            "chapter-intro E5 shared payload changed at source file offset 0x{:05X}",
            context.file_offset
        );
    }

    Ok(ChapterIntroContextSummary {
        prefix_code: 0xE5,
        prefix_code_hex: "E5",
        payload_destinations: [0x0071, 0x0070, 0x05CF, 0x05D0, CHAPTER_INDEX_ADDRESS],
        payload_destination_hex: ["0x0071", "0x0070", "0x05CF", "0x05D0", "0x781D"],
        unique_context_count: contexts.len(),
        first_chapter_index: contexts
            .first()
            .context("no chapter contexts")?
            .chapter_index,
        last_chapter_index: contexts
            .last()
            .context("no chapter contexts")?
            .chapter_index,
        chapter_index_address: CHAPTER_INDEX_ADDRESS,
        chapter_index_address_hex: "0x781D",
        shared_non_index_payload_sha1: sha1_hex(&CHAPTER_INTRO_SHARED_PAYLOAD),
        source_entry_indices: contexts
            .into_iter()
            .map(|context| context.entry_indices)
            .collect(),
    })
}

fn bind_chapter_titles(rom: &Rom) -> Result<ChapterTitleSummary> {
    let pointer_table_file_offset = source_file_offset(0x0F, CHAPTER_TITLE_POINTER_TABLE_ADDRESS)?;
    let pointer_table_end = pointer_table_file_offset + CHAPTER_TITLE_POINTER_TABLE_BYTES.len();
    let pointers = rom.data()[pointer_table_file_offset..pointer_table_end]
        .chunks_exact(2)
        .map(|bytes| u16::from_le_bytes([bytes[0], bytes[1]]))
        .collect::<Vec<_>>();
    ensure!(
        pointers.len() == CHAPTER_TITLE_COUNT,
        "chapter-title pointer count changed"
    );
    ensure!(
        pointers.windows(2).all(|pair| pair[0] < pair[1]),
        "chapter-title pointers are not strictly increasing"
    );

    let mut data_end_exclusive = CHAPTER_TITLE_DATA_START;
    let mut protected_digit_count = 0;
    for (index, pointer) in pointers.iter().copied().enumerate() {
        let file_offset = source_file_offset(0x0F, pointer)?;
        if index == 0 {
            ensure!(
                file_offset == CHAPTER_TITLE_DATA_START,
                "chapter-title data start changed"
            );
        }
        let relative_end = rom.data()[file_offset..CHAPTER_TITLE_DATA_END_EXCLUSIVE]
            .iter()
            .position(|byte| *byte == CHAPTER_TITLE_TERMINATOR)
            .with_context(|| format!("chapter-title entry {index} has no ED terminator"))?;
        let entry_end_exclusive = file_offset + relative_end + 1;
        if let Some(next_pointer) = pointers.get(index + 1) {
            ensure!(
                entry_end_exclusive == source_file_offset(0x0F, *next_pointer)?,
                "chapter-title entry {index} does not end at the next pointer"
            );
        }
        protected_digit_count += rom.data()[file_offset..entry_end_exclusive]
            .iter()
            .filter(|byte| (0x60..=0x69).contains(*byte))
            .count();
        data_end_exclusive = entry_end_exclusive;
    }
    ensure!(
        data_end_exclusive == CHAPTER_TITLE_DATA_END_EXCLUSIVE,
        "chapter-title data does not end at the next text table"
    );
    ensure!(
        protected_digit_count == CHAPTER_TITLE_DIGIT_COUNT,
        "chapter-title protected digit count changed"
    );

    Ok(ChapterTitleSummary {
        pointer_table: location(0x0F, CHAPTER_TITLE_POINTER_TABLE_ADDRESS),
        pointer_count: pointers.len(),
        data_file_start: CHAPTER_TITLE_DATA_START,
        data_file_start_hex: format!("0x{CHAPTER_TITLE_DATA_START:05X}"),
        data_file_end_exclusive: data_end_exclusive,
        data_file_end_exclusive_hex: format!("0x{data_end_exclusive:05X}"),
        source_terminator: CHAPTER_TITLE_TERMINATOR,
        source_terminator_hex: "ED",
        protected_original_digit_count: protected_digit_count,
        composer: location(0x0B, 0x88C4),
        selector_address: CHAPTER_INDEX_ADDRESS,
        selector_address_hex: "0x781D",
        translation_target: "Japanese chapter-title glyphs only; preserve original chapter-number digits",
    })
}

fn regular_save_reachability() -> RegularSaveReachability {
    RegularSaveReachability {
        file_one_data_start_address: 0x6000,
        file_one_data_start_address_hex: "0x6000",
        file_one_data_end_exclusive_address: 0x6542,
        file_one_data_end_exclusive_address_hex: "0x6542",
        file_one_chapter_address: 0x6519,
        file_one_chapter_address_hex: "0x6519",
        file_one_checksum_address: 0x6542,
        file_one_checksum_address_hex: "0x6542",
        checksum_byte_order: "little-endian",
        checksum_algorithm: "16-bit wrapping sum of every byte in 0x6000..0x6542",
        chapter_number_basis: "one-based MAP number; the E5 intro context later writes the zero-based value to 0x781D",
        runtime_use: "reachability intervention only; change the chapter byte and recompute the checksum before selecting regular file one",
        natural_progression_claimed: false,
    }
}

fn save_offer_no_branch_contract() -> SaveOfferNoBranchContract {
    SaveOfferNoBranchContract {
        screen_role: "chapter_save_offer",
        outer_screen_state_address: 0x0024,
        outer_screen_state_address_hex: "0x0024",
        offer_outer_screen_state: 0x0D,
        offer_outer_screen_state_hex: "0x0D",
        main_state_address: 0x0084,
        main_state_address_hex: "0x0084",
        owned_main_state_sequence: [0x07, 0x08, 0x09, 0x00],
        owned_main_state_sequence_hex: ["0x07", "0x08", "0x09", "0x00"],
        menu_depth_address: 0x05CE,
        menu_depth_address_hex: "0x05CE",
        observed_menu_depth: 0x02,
        active_selection_address: 0x7FF4,
        active_selection_address_hex: "0x7FF4",
        default_yes_selection: 0x01,
        no_selection: 0x02,
        committed_result_address: 0x05EB,
        committed_result_address_hex: "0x05EB",
        no_committed_result: 0x02,
        no_branch_exit_outer_state: 0x01,
        no_branch_exit_outer_state_hex: "0x01",
        no_branch_blackout_chr_pair: chr_pair(0x1B, 0x1B, 0x18, 0x18),
        persistent_save_route_entered: false,
        next_role: "chapter_transition_blackout",
        stable_sample_offsets_frames: [0, 7, 19, 43, 82, 171, 308, 565],
        stable_sample_screenshot_sha256: "a05ea43bc1844701650cfee6d441862d16d60fc1f7f0035704ce15dbbcfb352c",
        runtime_evidence: "with menu depth 02, Down changed only active selection slot 0x7FF4 from 01 to 02; A committed 02 to 0x05EB, main state advanced 07->08->09->00, bank 06:B76E wrote outer state 01, the no-save blackout used CHR 1B/1B + 18/18, and the next chapter map loaded without visiting the save-complete prompt",
    }
}

fn chapter_intro_runtime_samples() -> Vec<ChapterIntroRuntimeSample> {
    vec![
        ChapterIntroRuntimeSample {
            sample_role: "chapter_two_intro",
            chapter_number_one_based: 2,
            chapter_index_zero_based: 1,
            entry_method: "natural chapter-one completion and regular-save cold load",
            left_fd_chr_page: 0x13,
            left_fe_chr_page: 0x13,
            right_fd_chr_page: 0x00,
            right_fe_chr_page: 0x18,
            portrait_visible_in_sample: true,
            completion_marker_phase_union_observed: false,
            proof_limit: "binds the chapter-two composite only",
        },
        ChapterIntroRuntimeSample {
            sample_role: "chapter_eleven_intro",
            chapter_number_one_based: 11,
            chapter_index_zero_based: 10,
            entry_method: "chapter-one regular save with file-one chapter and checksum changed in a frozen isolated run",
            left_fd_chr_page: 0x1A,
            left_fe_chr_page: 0x1A,
            right_fd_chr_page: 0x00,
            right_fe_chr_page: 0x18,
            portrait_visible_in_sample: false,
            completion_marker_phase_union_observed: true,
            proof_limit: "proves chapter-eleven intro reachability and a distinct left CHR pair, not chapter-ten completion or the full transition sequence",
        },
        ChapterIntroRuntimeSample {
            sample_role: "chapter_twelve_intro",
            chapter_number_one_based: 12,
            chapter_index_zero_based: 11,
            entry_method: "continuous chapter-eleven しろ, four-page epilogue, default-yes save, automatic blackout, and continue route with declared movement and HP progression accelerations; the same CHR pair was previously reached by a checksummed cold load",
            left_fd_chr_page: 0x0F,
            left_fe_chr_page: 0x0F,
            right_fd_chr_page: 0x00,
            right_fe_chr_page: 0x18,
            portrait_visible_in_sample: true,
            completion_marker_phase_union_observed: true,
            proof_limit: "proves the continuous accelerated route and a distinct left CHR pair, not baseline difficulty, unaccelerated combat equivalence, alternate save choices, or failure paths",
        },
    ]
}

fn transition_screens() -> Vec<TransitionScreen> {
    vec![
        TransitionScreen {
            sequence_order: 1,
            screen_role: "chapter_clear_epilogue_dialogue",
            entry_condition: "the chapter objective resolves and the chapter-clear epilogue begins over the retained map",
            runtime_observed: true,
            input_behavior: "mixed",
            visible_components: &[
                "retained chapter map and unit sprites",
                "portrait",
                "dialogue window and Japanese text",
                "possibly flashing completion marker",
            ],
            translation_target: "Japanese dialogue only",
            preserved_original: &[],
            runtime_state: runtime_state(0x0C, "0x0C", 0x3C, "0x3C", Some(0x02), Some(0x0E)),
            observed_chr_pair: chr_pair(0x11, 0x11, 0x00, 0x18),
            temporal_behavior: "four observed page variants draw automatically and then wait for A; portrait visibility was true, true, false, true",
            input_actions: &[InputAction {
                input: "A on a completed page",
                immediate_effect: "advance the epilogue page; the terminal page enters next_story_banner",
                may_cause_persistent_gameplay_mutation: false,
                next_role: "chapter_clear_epilogue_dialogue or next_story_banner",
            }],
            unresolved_focus: &["remaining chapter-specific epilogue page and portrait variants"],
        },
        TransitionScreen {
            sequence_order: 2,
            screen_role: "next_story_banner",
            entry_condition: "the chapter-clear epilogue dialogue reaches its terminal transition",
            runtime_observed: true,
            input_behavior: "input_wait",
            visible_components: &[
                "retained chapter map and unit sprites",
                "centered window",
                "original English NEXT STORY label",
            ],
            translation_target: "none",
            preserved_original: &["NEXT STORY"],
            runtime_state: runtime_state(0x0D, "0x0D", 0x03, "0x03", Some(0x00), Some(0x00)),
            observed_chr_pair: chr_pair(0x1B, 0x1B, 0x00, 0x18),
            temporal_behavior: "the banner remained visible for 1,200 input-free frames",
            input_actions: &[InputAction {
                input: "A",
                immediate_effect: "close the banner and open the chapter save offer",
                may_cause_persistent_gameplay_mutation: false,
                next_role: "chapter_save_offer",
            }],
            unresolved_focus: &["remaining chapter-specific retained-map variants"],
        },
        TransitionScreen {
            sequence_order: 3,
            screen_role: "chapter_save_offer",
            entry_condition: "NEXT STORY is dismissed",
            runtime_observed: true,
            input_behavior: "input_wait",
            visible_components: &[
                "retained chapter map and unit sprites",
                "small centered Japanese save question",
                "yes and no choice window",
                "selection cursor",
            ],
            translation_target: "Japanese question and choices only",
            preserved_original: &[],
            runtime_state: runtime_state(0x0D, "0x0D", 0x07, "0x07", None, None),
            observed_chr_pair: chr_pair(0x1B, 0x1B, 0x00, 0x18),
            temporal_behavior: "the selected-no composite was pixel-stable at irregular offsets through 565 input-free frames",
            input_actions: &[
                InputAction {
                    input: "up or down",
                    immediate_effect: "change the yes or no selection",
                    may_cause_persistent_gameplay_mutation: false,
                    next_role: "chapter_save_offer",
                },
                InputAction {
                    input: "A on the observed default yes choice",
                    immediate_effect: "write the chapter-clear save and open the save-complete continue prompt",
                    may_cause_persistent_gameplay_mutation: true,
                    next_role: "chapter_save_complete_continue_prompt",
                },
                InputAction {
                    input: "A on the observed no choice",
                    immediate_effect: "skip the save-complete prompt, close through main states 08 and 09, and enter the no-save blackout",
                    may_cause_persistent_gameplay_mutation: false,
                    next_role: "chapter_transition_blackout",
                },
            ],
            unresolved_focus: &["remaining chapter-specific retained-map variants"],
        },
        TransitionScreen {
            sequence_order: 4,
            screen_role: "chapter_save_complete_continue_prompt",
            entry_condition: "the observed chapter-clear save finishes",
            runtime_observed: true,
            input_behavior: "input_wait",
            visible_components: &[
                "retained chapter map and unit sprites",
                "portrait",
                "large dialogue window with Japanese save-complete and continue text",
                "yes and no choice window",
                "selection cursor",
            ],
            translation_target: "Japanese dialogue and choices only",
            preserved_original: &[],
            runtime_state: runtime_state(0x0E, "0x0E", 0x04, "0x04", None, Some(0x11)),
            observed_chr_pair: chr_pair(0x1C, 0x1C, 0x00, 0x18),
            temporal_behavior: "the selection cursor and map sprites may animate independently",
            input_actions: &[InputAction {
                input: "A on the observed default yes choice",
                immediate_effect: "continue from the completed save into the automatic black transition",
                may_cause_persistent_gameplay_mutation: false,
                next_role: "chapter_transition_blackout",
            }],
            unresolved_focus: &[
                "selection-cursor phase union",
                "the unobserved no-choice route",
            ],
        },
        TransitionScreen {
            sequence_order: 5,
            screen_role: "chapter_transition_blackout",
            entry_condition: "the observed default-yes continue choice leaves the save-complete prompt, or the observed save-offer no choice skips that prompt",
            runtime_observed: true,
            input_behavior: "automatic",
            visible_components: &["full black frame"],
            translation_target: "none",
            preserved_original: &[],
            runtime_state: runtime_state(0x09, "0x09", 0x00, "0x00", None, None),
            observed_chr_pair: chr_pair(0x1A, 0x1A, 0x18, 0x18),
            temporal_behavior: "both observed full-black routes advance without input; the save route used outer state 09 and the no-save route wrote outer state 01",
            input_actions: &[],
            unresolved_focus: &[
                "remaining chapter-specific transition timing",
                "the complete outer-state lifetime after the no-save route writes 01",
            ],
        },
        TransitionScreen {
            sequence_order: 6,
            screen_role: "chapter_intro_title_dialogue_composite",
            entry_condition: "chapter-clear continuation or a cold load enters a chapter introduction",
            runtime_observed: true,
            input_behavior: "mixed",
            visible_components: &[
                "new chapter map and unit sprites",
                "chapter title bar with protected original number and Japanese title",
                "portrait",
                "dialogue window and Japanese text layered below the title bar",
                "possibly flashing completion marker",
            ],
            translation_target: "Japanese chapter title and dialogue only",
            preserved_original: &["chapter-number digits"],
            runtime_state: runtime_state(0x0B, "0x0B", 0x00, "0x00", None, Some(0x0E)),
            observed_chr_pair: chr_pair(0x0F, 0x0F, 0x00, 0x18),
            temporal_behavior: "the title bar remains while dialogue draws automatically and completed pages wait for input",
            input_actions: &[InputAction {
                input: "A on a completed dialogue page",
                immediate_effect: "advance the chapter-intro dialogue without changing the retained title contract",
                may_cause_persistent_gameplay_mutation: false,
                next_role: "chapter_intro_title_dialogue_composite or the chapter map",
            }],
            unresolved_focus: &[
                "title-bar exit lifetime relative to the final dialogue page",
                "chapter-specific map, portrait, and CHR variants after chapter 2",
            ],
        },
    ]
}

const fn runtime_state(
    outer_screen_state: u8,
    outer_screen_state_hex: &'static str,
    main_state: u8,
    main_state_hex: &'static str,
    victory_stage: Option<u8>,
    dialogue_state: Option<u8>,
) -> RuntimeScreenState {
    RuntimeScreenState {
        outer_screen_state,
        outer_screen_state_hex,
        main_state,
        main_state_hex,
        victory_stage,
        dialogue_state,
    }
}

const fn chr_pair(left_fd: u8, left_fe: u8, right_fd: u8, right_fe: u8) -> ChrPair {
    ChrPair {
        left_fd,
        left_fe,
        right_fd,
        right_fe,
    }
}

fn bind_source_region(rom: &Rom, spec: SourceRegionSpec) -> Result<SourceRegionBinding> {
    let file_offset = source_file_offset(spec.prg_bank, spec.cpu_address)?;
    let end = file_offset
        .checked_add(spec.bytes.len())
        .context("chapter-transition source region overflow")?;
    let actual = rom
        .data()
        .get(file_offset..end)
        .with_context(|| format!("{} source region is outside the ROM", spec.role))?;
    ensure!(actual == spec.bytes, "{} source bytes changed", spec.role);
    let typed_instructions = match spec.kind {
        RegionKind::Code => decode_rp2a03_sequence(actual, spec.cpu_address, spec.role)?,
        RegionKind::Data => Vec::new(),
    };

    Ok(SourceRegionBinding {
        role: spec.role,
        region_kind: match spec.kind {
            RegionKind::Code => "rp2a03_code",
            RegionKind::Data => "data",
        },
        prg_bank: spec.prg_bank,
        prg_bank_hex: format!("0x{:02X}", spec.prg_bank),
        cpu_address: spec.cpu_address,
        cpu_address_hex: format!("0x{:04X}", spec.cpu_address),
        file_offset,
        file_offset_hex: format!("0x{file_offset:05X}"),
        byte_count: spec.bytes.len(),
        source_sha1: sha1_hex(actual),
        typed_instructions,
    })
}

fn source_file_offset(prg_bank: u8, cpu_address: u16) -> Result<usize> {
    let bank_offset = if prg_bank == 0x0F {
        ensure!(
            cpu_address >= FIXED_CPU_START,
            "fixed-bank address is below 0xC000"
        );
        usize::from(cpu_address - FIXED_CPU_START)
    } else {
        ensure!(
            (SWITCHABLE_CPU_START..FIXED_CPU_START).contains(&cpu_address),
            "switchable-bank address is outside 0x8000..0xBFFF"
        );
        usize::from(cpu_address - SWITCHABLE_CPU_START)
    };
    Ok(HEADER_SIZE + usize::from(prg_bank) * PRG_BANK_SIZE + bank_offset)
}

fn location(prg_bank: u8, cpu_address: u16) -> CodeLocation {
    CodeLocation {
        prg_bank,
        prg_bank_hex: format!("0x{prg_bank:02X}"),
        cpu_address,
        cpu_address_hex: format!("0x{cpu_address:04X}"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn chapter_title_table_includes_the_twenty_fifth_pointer() {
        assert_eq!(CHAPTER_TITLE_POINTER_TABLE_BYTES.len(), 50);
        let pointers = CHAPTER_TITLE_POINTER_TABLE_BYTES
            .chunks_exact(2)
            .map(|bytes| u16::from_le_bytes([bytes[0], bytes[1]]))
            .collect::<Vec<_>>();

        assert_eq!(pointers.len(), 25);
        assert_eq!(pointers.first(), Some(&0xEE3A));
        assert_eq!(pointers.last(), Some(&0xEFA8));
        assert!(pointers.windows(2).all(|pair| pair[0] < pair[1]));
    }

    #[test]
    fn transition_sequence_separates_each_observed_screen_lifetime() {
        let screens = transition_screens();
        let roles = screens
            .iter()
            .map(|screen| screen.screen_role)
            .collect::<Vec<_>>();

        assert_eq!(
            roles,
            [
                "chapter_clear_epilogue_dialogue",
                "next_story_banner",
                "chapter_save_offer",
                "chapter_save_complete_continue_prompt",
                "chapter_transition_blackout",
                "chapter_intro_title_dialogue_composite",
            ]
        );
        assert!(screens.iter().all(|screen| screen.runtime_observed));
        assert_eq!(screens[1].translation_target, "none");
        assert_eq!(screens[1].preserved_original, ["NEXT STORY"]);
        assert!(
            screens[2]
                .input_actions
                .iter()
                .any(|action| action.may_cause_persistent_gameplay_mutation)
        );
        assert!(screens[2].input_actions.iter().any(|action| {
            action.input.contains("no choice")
                && !action.may_cause_persistent_gameplay_mutation
                && action.next_role == "chapter_transition_blackout"
        }));
        assert_eq!(
            [
                screens[3].observed_chr_pair.left_fd,
                screens[3].observed_chr_pair.left_fe,
                screens[3].observed_chr_pair.right_fd,
                screens[3].observed_chr_pair.right_fe,
            ],
            [0x1C, 0x1C, 0x00, 0x18]
        );
        assert_eq!(screens[4].input_behavior, "automatic");
    }

    #[test]
    fn fixed_label_indices_match_their_pointer_table_cells() {
        let pointer_table_address = 0x8FC2_u16;

        assert_eq!(pointer_table_address + 2 * 0x3E, 0x903E);
        assert_eq!(pointer_table_address + 2 * 0x32, 0x9026);
        assert_eq!(u16::from_le_bytes([0xFB, 0x91]), 0x91FB);
        assert_eq!(u16::from_le_bytes([0xAA, 0x91]), 0x91AA);
    }

    #[test]
    fn source_region_addresses_map_to_the_verified_file_offsets() {
        assert_eq!(source_file_offset(0x0B, 0x886A).unwrap(), 0x2C87A);
        assert_eq!(source_file_offset(0x0B, 0x88C4).unwrap(), 0x2C8D4);
        assert_eq!(source_file_offset(0x0B, 0x8AE6).unwrap(), 0x2CAF6);
        assert_eq!(source_file_offset(0x0B, 0x9AD0).unwrap(), 0x2DAE0);
        assert_eq!(source_file_offset(0x0B, 0x9D52).unwrap(), 0x2DD62);
        assert_eq!(source_file_offset(0x0B, 0x9FA8).unwrap(), 0x2DFB8);
        assert_eq!(source_file_offset(0x06, 0x8400).unwrap(), 0x18410);
        assert_eq!(source_file_offset(0x06, 0xB6F3).unwrap(), 0x1B703);
        assert_eq!(source_file_offset(0x06, 0xB737).unwrap(), 0x1B747);
        assert_eq!(source_file_offset(0x0B, 0x9333).unwrap(), 0x2D343);
        assert_eq!(
            source_file_offset(0x0F, CHAPTER_TITLE_POINTER_TABLE_ADDRESS).unwrap(),
            0x3EE18
        );
    }

    #[test]
    fn save_offer_no_choice_owns_a_distinct_close_and_blackout_route() {
        let contract = save_offer_no_branch_contract();

        assert_eq!(contract.offer_outer_screen_state, 0x0D);
        assert_eq!(contract.owned_main_state_sequence, [0x07, 0x08, 0x09, 0x00]);
        assert_eq!(contract.observed_menu_depth, 2);
        assert_eq!(contract.active_selection_address, 0x7FF4);
        assert_eq!(contract.no_selection, 2);
        assert_eq!(contract.no_committed_result, 2);
        assert_eq!(contract.no_branch_exit_outer_state, 1);
        assert!(!contract.persistent_save_route_entered);
        assert_eq!(contract.next_role, "chapter_transition_blackout");
        assert_eq!(
            [
                contract.no_branch_blackout_chr_pair.left_fd,
                contract.no_branch_blackout_chr_pair.left_fe,
                contract.no_branch_blackout_chr_pair.right_fd,
                contract.no_branch_blackout_chr_pair.right_fe,
            ],
            [0x1B, 0x1B, 0x18, 0x18]
        );
        assert_eq!(contract.stable_sample_offsets_frames.last(), Some(&565));
    }

    #[test]
    fn chapter_transition_code_regions_use_typed_rp2a03_decode() {
        for spec in SOURCE_REGIONS {
            if matches!(spec.kind, RegionKind::Code) {
                let instructions =
                    decode_rp2a03_sequence(spec.bytes, spec.cpu_address, spec.role).unwrap();
                assert!(
                    !instructions.is_empty(),
                    "{} has no instructions",
                    spec.role
                );
            }
        }
    }

    #[test]
    fn later_intro_samples_keep_entry_methods_and_proof_limits_distinct() {
        let samples = chapter_intro_runtime_samples();
        let chapter_eleven = samples
            .iter()
            .find(|sample| sample.chapter_number_one_based == 11)
            .unwrap();
        let chapter_twelve = samples
            .iter()
            .find(|sample| sample.chapter_number_one_based == 12)
            .unwrap();

        assert_eq!(chapter_eleven.chapter_index_zero_based, 10);
        assert_eq!(
            [
                chapter_eleven.left_fd_chr_page,
                chapter_eleven.left_fe_chr_page,
                chapter_eleven.right_fd_chr_page,
                chapter_eleven.right_fe_chr_page,
            ],
            [0x1A, 0x1A, 0x00, 0x18]
        );
        assert!(chapter_eleven.proof_limit.contains("not chapter-ten"));
        assert!(!chapter_eleven.portrait_visible_in_sample);
        assert_eq!(chapter_twelve.chapter_index_zero_based, 11);
        assert!(chapter_twelve.portrait_visible_in_sample);
        assert_eq!(
            [
                chapter_twelve.left_fd_chr_page,
                chapter_twelve.left_fe_chr_page,
                chapter_twelve.right_fd_chr_page,
                chapter_twelve.right_fe_chr_page,
            ],
            [0x0F, 0x0F, 0x00, 0x18]
        );
        assert!(
            chapter_twelve
                .entry_method
                .contains("continuous chapter-eleven")
        );
        assert!(chapter_twelve.proof_limit.contains("baseline difficulty"));
        assert!(!regular_save_reachability().natural_progression_claimed);
    }
}
