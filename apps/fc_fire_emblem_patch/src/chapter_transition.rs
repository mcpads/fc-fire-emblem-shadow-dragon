use std::{fs, path::Path};

use anyhow::{Context, Result, ensure};
use serde::Serialize;

use crate::{
    dialogue_inventory::inspect_chapter_intro_contexts,
    rom::{EXPECTED_SOURCE_SHA1, HEADER_SIZE, Rom},
    sha1_hex,
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

const SOURCE_REGIONS: &[SourceRegionSpec] = &[
    SourceRegionSpec::new(
        "compose_next_story_banner",
        0x0B,
        0x886A,
        NEXT_STORY_COMPOSER_BYTES,
    ),
    SourceRegionSpec::new(
        "compose_chapter_title",
        0x0B,
        0x88C4,
        CHAPTER_TITLE_COMPOSER_BYTES,
    ),
    SourceRegionSpec::new(
        "compose_chapter_save_offer",
        0x0B,
        0x8AE6,
        SAVE_OFFER_COMPOSER_BYTES,
    ),
    SourceRegionSpec::new(
        "chapter_title_pointer_table",
        0x0F,
        CHAPTER_TITLE_POINTER_TABLE_ADDRESS,
        CHAPTER_TITLE_POINTER_TABLE_BYTES,
    ),
    SourceRegionSpec::new("next_story_pointer", 0x0B, 0x903E, NEXT_STORY_POINTER_BYTES),
    SourceRegionSpec::new("next_story_label", 0x0B, 0x91FB, NEXT_STORY_LABEL_BYTES),
    SourceRegionSpec::new(
        "chapter_save_offer_pointer",
        0x0B,
        0x9026,
        SAVE_OFFER_POINTER_BYTES,
    ),
    SourceRegionSpec::new(
        "chapter_save_offer_label",
        0x0B,
        0x91AA,
        SAVE_OFFER_LABEL_BYTES,
    ),
    SourceRegionSpec::new(
        "calculate_regular_save_checksum",
        0x0B,
        0x9D52,
        REGULAR_SAVE_CHECKSUM_BYTES,
    ),
    SourceRegionSpec::new(
        "write_regular_file_one_checksum",
        0x0B,
        0x9AD0,
        WRITE_REGULAR_FILE_ONE_CHECKSUM_BYTES,
    ),
    SourceRegionSpec::new(
        "validate_regular_save_checksum",
        0x0B,
        0x9FA8,
        VALIDATE_REGULAR_SAVE_CHECKSUM_BYTES,
    ),
];

#[derive(Clone, Copy)]
struct SourceRegionSpec {
    role: &'static str,
    prg_bank: u8,
    cpu_address: u16,
    bytes: &'static [u8],
}

impl SourceRegionSpec {
    const fn new(role: &'static str, prg_bank: u8, cpu_address: u16, bytes: &'static [u8]) -> Self {
        Self {
            role,
            prg_bank,
            cpu_address,
            bytes,
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
    temporal_behavior: &'static str,
    input_actions: &'static [InputAction],
    unresolved_focus: &'static [&'static str],
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
struct ChapterIntroRuntimeSample {
    sample_role: &'static str,
    chapter_number_one_based: u8,
    chapter_index_zero_based: u8,
    entry_method: &'static str,
    left_fd_chr_page: u8,
    left_fe_chr_page: u8,
    right_fd_chr_page: u8,
    right_fe_chr_page: u8,
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
    prg_bank: u8,
    prg_bank_hex: String,
    cpu_address: u16,
    cpu_address_hex: String,
    file_offset: usize,
    file_offset_hex: String,
    byte_count: usize,
    source_sha1: String,
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
    pub next_screen_role: &'static str,
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
        next_screen_role: report.next_universalization_gate,
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
            proof_boundary: "source-bound chapter context, title, NEXT STORY, save-offer, and regular-save checksum producers plus the runtime-observed chapter-one-to-two sequence and chapter-twelve intro sample; no dialogue source, translation, or ROM mutation",
        },
        observed_sequence: transition_screens(),
        chapter_intro_contexts,
        chapter_titles,
        regular_save_reachability: regular_save_reachability(),
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
        next_universalization_gate: "later_chapter_transition",
        unresolved: vec![
            "The chapter-one epilogue and save-complete dialogue use the main dialogue engine, but their dialogue source content is intentionally outside this public report.",
            "The exact CHR pairs for chapter_clear_epilogue_dialogue, next_story_banner, chapter_save_offer, and chapter_save_complete_continue_prompt are not yet bound to each individual lifetime.",
            "Chapter twelve is observed only through a checksummed regular-save chapter intervention; chapter eleven's epilogue, save choices, and transition into chapter twelve are not observed.",
            "Chapter-two and chapter-twelve intro samples do not generalize the remaining twenty-three chapters, alternate save choices, or title lifetimes.",
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
            completion_marker_phase_union_observed: false,
            proof_limit: "binds the chapter-two composite only",
        },
        ChapterIntroRuntimeSample {
            sample_role: "chapter_twelve_intro",
            chapter_number_one_based: 12,
            chapter_index_zero_based: 11,
            entry_method: "chapter-one regular save with file-one chapter and checksum changed in a frozen isolated run",
            left_fd_chr_page: 0x0F,
            left_fe_chr_page: 0x0F,
            right_fd_chr_page: 0x00,
            right_fe_chr_page: 0x18,
            completion_marker_phase_union_observed: true,
            proof_limit: "proves later-chapter intro reachability and a distinct left CHR pair, not chapter-eleven completion or the full transition sequence",
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
            temporal_behavior: "text draws automatically; completed pages wait for input",
            input_actions: &[InputAction {
                input: "A on a completed page",
                immediate_effect: "advance the epilogue page; the terminal page enters next_story_banner",
                may_cause_persistent_gameplay_mutation: false,
                next_role: "chapter_clear_epilogue_dialogue or next_story_banner",
            }],
            unresolved_focus: &[
                "exact CHR pair and flashing-marker phase union",
                "terminal dialogue-state owner",
            ],
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
            temporal_behavior: "the banner remained visible for 1,200 input-free frames",
            input_actions: &[InputAction {
                input: "A",
                immediate_effect: "close the banner and open the chapter save offer",
                may_cause_persistent_gameplay_mutation: false,
                next_role: "chapter_save_offer",
            }],
            unresolved_focus: &["exact CHR pair", "font-page exit lifetime"],
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
            temporal_behavior: "the selection cursor may flash",
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
            ],
            unresolved_focus: &[
                "exact CHR pair and cursor phase union",
                "the unobserved no-choice route",
            ],
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
            temporal_behavior: "the selection cursor and map sprites may animate independently",
            input_actions: &[InputAction {
                input: "A on the observed default yes choice",
                immediate_effect: "continue from the completed save into the next chapter introduction",
                may_cause_persistent_gameplay_mutation: false,
                next_role: "chapter_intro_title_dialogue_composite",
            }],
            unresolved_focus: &[
                "which observed 1C/1C plus 00/15 or 00/18 CHR phase belongs to this exact lifetime",
                "the unobserved no-choice route",
            ],
        },
        TransitionScreen {
            sequence_order: 5,
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

    Ok(SourceRegionBinding {
        role: spec.role,
        prg_bank: spec.prg_bank,
        prg_bank_hex: format!("0x{:02X}", spec.prg_bank),
        cpu_address: spec.cpu_address,
        cpu_address_hex: format!("0x{:04X}", spec.cpu_address),
        file_offset,
        file_offset_hex: format!("0x{file_offset:05X}"),
        byte_count: spec.bytes.len(),
        source_sha1: sha1_hex(actual),
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
        assert_eq!(
            source_file_offset(0x0F, CHAPTER_TITLE_POINTER_TABLE_ADDRESS).unwrap(),
            0x3EE18
        );
    }

    #[test]
    fn later_intro_sample_does_not_claim_the_preceding_transition() {
        let samples = chapter_intro_runtime_samples();
        let chapter_twelve = samples
            .iter()
            .find(|sample| sample.chapter_number_one_based == 12)
            .unwrap();

        assert_eq!(chapter_twelve.chapter_index_zero_based, 11);
        assert_eq!(
            [
                chapter_twelve.left_fd_chr_page,
                chapter_twelve.left_fe_chr_page,
                chapter_twelve.right_fd_chr_page,
                chapter_twelve.right_fe_chr_page,
            ],
            [0x0F, 0x0F, 0x00, 0x18]
        );
        assert!(chapter_twelve.proof_limit.contains("not chapter-eleven"));
        assert!(!regular_save_reachability().natural_progression_claimed);
    }
}
