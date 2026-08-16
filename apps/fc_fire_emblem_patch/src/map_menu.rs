use std::{collections::BTreeSet, fs, path::Path};

use anyhow::{Context, Result, ensure};
use serde::Deserialize;

use crate::{
    font_slots::active_hangul_codes,
    japanese_encoding::{is_japanese_text_code, japanese_text_glyph},
    rom::{EXPECTED_SOURCE_SHA1, HEADER_SIZE, Rom},
    sha1_hex,
    text_inventory::{FixedTextLogicalByte, encode_target_markup, is_japanese_character},
    translation_consumer::{
        ScreenConsumerSourceBinding, TranslationConsumerSourceEvidence,
        qualified_source_binding_id, source_binding_id,
    },
    typed_source::decode_rp2a03_sequence,
    unit_ui_text::terminated_composite_display_cell_count,
};

const PRG_BANK_SIZE: usize = 16 * 1024;
const PRG_BANK: u8 = 0x0B;
const CPU_WINDOW_START: u16 = 0x8000;
const COMPOSITE_DISPATCH_TABLE_ADDRESS: u16 = 0x8006;
const MAP_MENU_COMPOSER_STATE: u8 = 0x03;
const MAP_MENU_COMPOSER_ADDRESS: u16 = 0x8187;
const MAP_FUNDS_COMPOSER_STATE: u8 = 0x13;
const MAP_FUNDS_COMPOSER_ADDRESS: u16 = 0x88D5;
const MAP_SUMMARY_COMPOSER_STATE: u8 = 0x14;
const MAP_SUMMARY_COMPOSER_ADDRESS: u16 = 0x8923;
const FIXED_STRING_POINTER_TABLE_ADDRESS: u16 = 0x8FC2;
const COMPOSITE_POINTER_ROLE: &str = "composite_pointer";
const COMPOSER_ROLE: &str = "compose_map_menu";
const LABEL_BLOCK_ROLE: &str = "map_menu_label_block";
const MAP_MENU_COMPOSER: &[u8] = &[
    0xA9, 0x0E, 0x8D, 0xD0, 0x05, 0xA9, 0x0C, 0x8D, 0xCF, 0x05, 0x20, 0xC8, 0x97, 0x20, 0x3C, 0x8E,
    0xAE, 0xCE, 0x05, 0xA9, 0x3F, 0x9D, 0xEE, 0x7F, 0xA9, 0x01, 0x9D, 0xF3, 0x7F, 0xBD, 0xB2, 0x81,
    0x9D, 0x51, 0x04, 0xE8, 0xC9, 0xEF, 0xD0, 0xF5, 0x4C, 0x39, 0x8F,
];
const MAP_FUNDS_COMPOSER: &[u8] = &[
    0xA9, 0x0A, 0x8D, 0xCF, 0x05, 0xA9, 0x04, 0x8D, 0xD0, 0x05, 0xA9, 0x10, 0x85, 0x70, 0xA9, 0xA0,
    0x85, 0x71, 0x20, 0x3C, 0x8E, 0xAD, 0x78, 0x76, 0x0D, 0x79, 0x76, 0xF0, 0x15, 0xAD, 0x78, 0x76,
    0x85, 0x00, 0xAD, 0x79, 0x76, 0x85, 0x01, 0xA9, 0x51, 0x85, 0x08, 0xA9, 0x04, 0x85, 0x09, 0x20,
    0xBA, 0xC7, 0xA2, 0x05, 0xA9, 0x60, 0x9D, 0x51, 0x04, 0xE8, 0xA9, 0x70, 0x9D, 0x51, 0x04, 0xE8,
    0xA9, 0xED, 0x9D, 0x51, 0x04, 0xE8, 0xA9, 0xEF, 0x9D, 0x51, 0x04, 0x4C, 0x39, 0x8F,
];
const MAP_SUMMARY_COMPOSER: &[u8] = &[
    0xA9, 0x0A, 0x8D, 0xCF, 0x05, 0xA9, 0x06, 0x8D, 0xD0, 0x05, 0xA9, 0x50, 0x85, 0x70, 0x85, 0x71,
    0x20, 0x3C, 0x8E, 0xA9, 0x2D, 0x20, 0xEE, 0x8E, 0xE8, 0xAD, 0x74, 0x76, 0x85, 0x00, 0x20, 0x0E,
    0x8F, 0xA9, 0xED, 0x9D, 0x51, 0x04, 0xE8, 0xA9, 0x0A, 0x20, 0xEE, 0x8E, 0xAD, 0x75, 0x76, 0x85,
    0x00, 0x20, 0x23, 0x8F, 0xA9, 0xED, 0x9D, 0x51, 0x04, 0xE8, 0xA9, 0xEF, 0x9D, 0x51, 0x04, 0x4C,
    0x39, 0x8F,
];
const LABEL_BLOCK_ADDRESS: u16 = 0x81B2;
const LABEL_BLOCK_END: u8 = 0xEF;

struct LabelSpec {
    id: &'static str,
    pointer: u16,
    expected: &'static [u8],
    preserved_suffix: &'static [u8],
    pointer_table_index: Option<u8>,
    preserve_source_display_cell_count: bool,
}

const MAP_MENU_LABELS: &[LabelSpec] = &[
    label("map-menu:roster", 0x81B2, &[0x01, 0x11, 0x28, 0x2F, 0xED]),
    label(
        "map-menu:storage",
        0x81B7,
        &[0x00, 0x0C, 0x0F, 0x05, 0x29, 0x0B, 0x0F, 0x87, 0xED],
    ),
    label(
        "map-menu:funds",
        0x81C0,
        &[0x0B, 0x87, 0x0B, 0x0F, 0x06, 0x2F, 0xED],
    ),
    label(
        "map-menu:suspend",
        0x81C7,
        &[0x11, 0x86, 0x02, 0x10, 0x0F, 0x2F, 0xED],
    ),
    label("map-menu:switch", 0x81CE, &[0x3C, 0x31, 0x89, 0x41, 0xED]),
    label(
        "map-menu:end-turn",
        0x81D3,
        &[0x40, 0x3F, 0x5F, 0x04, 0x2D, 0x2A, 0xED],
    ),
];

const MAP_FUNDS_SUMMARY_LABELS: &[LabelSpec] = &[
    summary_label(
        "map-funds-summary:map",
        0x9195,
        &[0x50, 0x89, 0x4C, 0x1F, 0x8D, 0xEF],
        0x2D,
    ),
    summary_label(
        "map-funds-summary:turn",
        0x90AA,
        &[0x40, 0x3F, 0x5F, 0x8D, 0xEF],
        0x0A,
    ),
];

const fn label(id: &'static str, pointer: u16, expected: &'static [u8]) -> LabelSpec {
    LabelSpec {
        id,
        pointer,
        expected,
        preserved_suffix: &[0xED],
        pointer_table_index: None,
        preserve_source_display_cell_count: false,
    }
}

const fn summary_label(
    id: &'static str,
    pointer: u16,
    expected: &'static [u8],
    pointer_table_index: u8,
) -> LabelSpec {
    LabelSpec {
        id,
        pointer,
        expected,
        preserved_suffix: &[0x8D, 0xEF],
        pointer_table_index: Some(pointer_table_index),
        preserve_source_display_cell_count: true,
    }
}

fn labels() -> impl Iterator<Item = &'static LabelSpec> {
    MAP_MENU_LABELS.iter().chain(MAP_FUNDS_SUMMARY_LABELS)
}

#[derive(Debug, Deserialize)]
struct Workspace {
    format_version: u8,
    source_sha1: String,
    translate_from: String,
    translate_to: String,
    preserve_existing_english_and_digits: bool,
    entries: Vec<WorkspaceEntry>,
}

#[derive(Debug, Deserialize)]
struct WorkspaceEntry {
    id: String,
    source_cpu_address_hex: String,
    source_bytes_hex: String,
    source_sha1: String,
    japanese_markup: String,
    korean_markup: String,
    status: String,
}

pub(crate) struct MapMenuPlan {
    pub(crate) entry_count: usize,
    pub(crate) translated_entry_count: usize,
    pub(crate) review_complete: bool,
    pub(crate) workspace_sha1: String,
    pub(crate) target_glyphs: BTreeSet<char>,
    pub(crate) source_reclaimable_active_codes: BTreeSet<u8>,
    pub(crate) entries: Vec<MapMenuPlannedEntry>,
}

#[derive(Clone, Debug)]
pub(crate) struct MapMenuPlannedEntry {
    pub(crate) id: String,
    pub(crate) source_cpu_address: u16,
    pub(crate) source_file_offset: usize,
    pub(crate) source_storage: Vec<u8>,
    pub(crate) preserved_suffix: Vec<u8>,
    pub(crate) source_display_cell_count: Option<usize>,
    logical_bytes: Vec<FixedTextLogicalByte>,
}

impl MapMenuPlannedEntry {
    pub(crate) fn logical_bytes(&self) -> &[FixedTextLogicalByte] {
        &self.logical_bytes
    }
}

/// 지도 메뉴 여섯 라벨과 소지금 화면의 맵·턴 라벨을 각 소유 화면에 결속한다.
/// 번역 작업공간은 이 원천 census의 일부가 아니다.
pub(crate) fn inspect_map_menu_translation_consumers(
    rom: &Rom,
) -> Result<TranslationConsumerSourceEvidence> {
    bind_source(rom)?;
    let population_ids = labels().map(|spec| spec.id.to_owned()).collect::<Vec<_>>();
    Ok(TranslationConsumerSourceEvidence {
        population_ids,
        screen_bindings: vec![
            ScreenConsumerSourceBinding {
                screen_role: "map_menu",
                population_ids: MAP_MENU_LABELS
                    .iter()
                    .map(|spec| spec.id.to_owned())
                    .collect(),
                source_binding_ids: vec![
                    qualified_source_binding_id(
                        usize::from(PRG_BANK),
                        COMPOSITE_DISPATCH_TABLE_ADDRESS + u16::from(MAP_MENU_COMPOSER_STATE) * 2,
                        COMPOSITE_POINTER_ROLE,
                        &format!(
                            "state={MAP_MENU_COMPOSER_STATE:02X},composer={MAP_MENU_COMPOSER_ADDRESS:04X}"
                        ),
                    ),
                    source_binding_id(
                        usize::from(PRG_BANK),
                        MAP_MENU_COMPOSER_ADDRESS,
                        COMPOSER_ROLE,
                    ),
                    source_binding_id(usize::from(PRG_BANK), LABEL_BLOCK_ADDRESS, LABEL_BLOCK_ROLE),
                ],
            },
            ScreenConsumerSourceBinding {
                screen_role: "map_funds_summary",
                population_ids: MAP_FUNDS_SUMMARY_LABELS
                    .iter()
                    .map(|spec| spec.id.to_owned())
                    .collect(),
                source_binding_ids: vec![
                    qualified_source_binding_id(
                        usize::from(PRG_BANK),
                        COMPOSITE_DISPATCH_TABLE_ADDRESS + u16::from(MAP_FUNDS_COMPOSER_STATE) * 2,
                        COMPOSITE_POINTER_ROLE,
                        &format!(
                            "state={MAP_FUNDS_COMPOSER_STATE:02X},composer={MAP_FUNDS_COMPOSER_ADDRESS:04X}"
                        ),
                    ),
                    qualified_source_binding_id(
                        usize::from(PRG_BANK),
                        COMPOSITE_DISPATCH_TABLE_ADDRESS
                            + u16::from(MAP_SUMMARY_COMPOSER_STATE) * 2,
                        COMPOSITE_POINTER_ROLE,
                        &format!(
                            "state={MAP_SUMMARY_COMPOSER_STATE:02X},composer={MAP_SUMMARY_COMPOSER_ADDRESS:04X}"
                        ),
                    ),
                    source_binding_id(
                        usize::from(PRG_BANK),
                        MAP_FUNDS_COMPOSER_ADDRESS,
                        "compose_map_funds",
                    ),
                    source_binding_id(
                        usize::from(PRG_BANK),
                        MAP_SUMMARY_COMPOSER_ADDRESS,
                        "compose_map_and_turn_summary",
                    ),
                    source_binding_id(
                        usize::from(PRG_BANK),
                        FIXED_STRING_POINTER_TABLE_ADDRESS,
                        "map_and_turn_fixed_string_pointers",
                    ),
                ],
            },
        ],
    })
}

pub(crate) fn plan_map_menu(rom: &Rom, workspace_path: &Path) -> Result<MapMenuPlan> {
    bind_source(rom)?;
    let bytes = fs::read(workspace_path)
        .with_context(|| format!("read map menu workspace {}", workspace_path.display()))?;
    let workspace: Workspace = serde_json::from_slice(&bytes)
        .with_context(|| format!("parse map menu workspace {}", workspace_path.display()))?;
    ensure!(workspace.format_version == 1, "unsupported map menu format");
    ensure!(
        workspace.source_sha1 == EXPECTED_SOURCE_SHA1
            && workspace.translate_from == "ja"
            && workspace.translate_to == "ko"
            && workspace.preserve_existing_english_and_digits,
        "map menu workspace contract changed"
    );
    ensure!(
        workspace.entries.len() == MAP_MENU_LABELS.len() + MAP_FUNDS_SUMMARY_LABELS.len(),
        "map menu workspace must contain all menu and funds-summary labels"
    );

    let mut translated_entry_count = 0;
    let mut target_glyphs = BTreeSet::new();
    let mut entries = Vec::with_capacity(workspace.entries.len());
    for (entry, spec) in workspace.entries.iter().zip(labels()) {
        ensure!(
            spec.expected.ends_with(spec.preserved_suffix),
            "map menu structural suffix changed for {}",
            spec.id
        );
        let payload_len = spec.expected.len() - spec.preserved_suffix.len();
        let source_markup = decode_source_markup(&spec.expected[..payload_len]);
        ensure!(
            entry.id == spec.id
                && entry.source_cpu_address_hex == format!("0x{:04X}", spec.pointer)
                && entry.source_bytes_hex == hex(spec.expected)
                && entry.source_sha1 == sha1_hex(spec.expected)
                && entry.japanese_markup == source_markup,
            "map menu workspace binding changed for {}",
            spec.id
        );
        ensure!(
            matches!(
                entry.status.as_str(),
                "untranslated" | "needs_human_review" | "complete"
            ),
            "invalid map menu status for {}",
            spec.id
        );
        ensure!(
            (entry.status == "untranslated") == entry.korean_markup.is_empty(),
            "map menu text and status disagree for {}",
            spec.id
        );
        if !entry.korean_markup.is_empty() {
            ensure!(
                !entry.korean_markup.chars().any(is_japanese_character),
                "map menu Korean text still contains Japanese for {}",
                spec.id
            );
            ensure!(
                entry
                    .korean_markup
                    .chars()
                    .all(|character| ('가'..='힣').contains(&character)),
                "map menu translation introduced non-Hangul text for {}",
                spec.id
            );
            let logical_bytes = encode_target_markup(&entry.korean_markup)?;
            let source_display_cell_count = spec
                .preserve_source_display_cell_count
                .then(|| terminated_composite_display_cell_count(spec.expected, 0xEF))
                .transpose()?;
            if let Some(source_display_cell_count) = source_display_cell_count {
                ensure!(
                    logical_bytes.len() + 1 <= source_display_cell_count,
                    "map funds-summary translation exceeds its source display span for {}",
                    spec.id
                );
            } else {
                ensure!(
                    logical_bytes.len() <= 6,
                    "map menu translation exceeds six visible cells for {}",
                    spec.id
                );
            }
            translated_entry_count += 1;
            target_glyphs.extend(entry.korean_markup.chars());
            entries.push(MapMenuPlannedEntry {
                id: entry.id.clone(),
                source_cpu_address: spec.pointer,
                source_file_offset: source_file_offset(spec.pointer)?,
                source_storage: spec.expected.to_vec(),
                preserved_suffix: spec.preserved_suffix.to_vec(),
                source_display_cell_count,
                logical_bytes,
            });
        }
    }
    let active_codes = active_hangul_codes().into_iter().collect::<BTreeSet<_>>();
    let source_reclaimable_active_codes = labels()
        .flat_map(|spec| {
            spec.expected[..spec.expected.len() - spec.preserved_suffix.len()]
                .iter()
                .copied()
        })
        .filter(|code| is_japanese_text_code(*code) && active_codes.contains(code))
        .collect::<BTreeSet<_>>();
    ensure!(
        !source_reclaimable_active_codes.is_empty()
            && target_glyphs.len() <= source_reclaimable_active_codes.len(),
        "map menu cannot reclaim enough exact source label codes for its Korean glyphs"
    );
    Ok(MapMenuPlan {
        entry_count: workspace.entries.len(),
        translated_entry_count,
        review_complete: workspace
            .entries
            .iter()
            .all(|entry| entry.status == "complete"),
        workspace_sha1: sha1_hex(&bytes),
        target_glyphs,
        source_reclaimable_active_codes,
        entries,
    })
}

fn bind_source(rom: &Rom) -> Result<()> {
    for (state, address, expected, expected_sha1, role) in [
        (
            MAP_MENU_COMPOSER_STATE,
            MAP_MENU_COMPOSER_ADDRESS,
            MAP_MENU_COMPOSER,
            "606e2dd311cce69732a44278b9dfd019ecb977e5",
            "compose map menu",
        ),
        (
            MAP_FUNDS_COMPOSER_STATE,
            MAP_FUNDS_COMPOSER_ADDRESS,
            MAP_FUNDS_COMPOSER,
            "d5536e7a7f51313dc5980f8b96e0dea32b314e1c",
            "compose map funds",
        ),
        (
            MAP_SUMMARY_COMPOSER_STATE,
            MAP_SUMMARY_COMPOSER_ADDRESS,
            MAP_SUMMARY_COMPOSER,
            "784c310665741f691847ad70d230a07e9fe32556",
            "compose map and turn summary",
        ),
    ] {
        ensure!(
            read_u16(rom, COMPOSITE_DISPATCH_TABLE_ADDRESS + u16::from(state) * 2,)? == address,
            "composite state {state:02X} no longer selects {role}"
        );
        let composer = source_slice(rom, address, expected.len())?;
        ensure!(composer == expected, "{role} changed");
        ensure!(sha1_hex(composer) == expected_sha1, "{role} hash changed");
        decode_rp2a03_sequence(composer, address, role)?;
    }

    let mut cursor = LABEL_BLOCK_ADDRESS;
    for spec in MAP_MENU_LABELS {
        ensure!(
            spec.pointer == cursor,
            "map menu labels are no longer contiguous"
        );
        ensure!(
            source_slice(rom, spec.pointer, spec.expected.len())? == spec.expected,
            "map menu source changed for {}",
            spec.id
        );
        cursor += spec.expected.len() as u16;
    }
    ensure!(
        source_slice(rom, cursor, 1)? == [LABEL_BLOCK_END],
        "map menu label block terminator changed"
    );
    for spec in MAP_FUNDS_SUMMARY_LABELS {
        ensure!(
            source_slice(rom, spec.pointer, spec.expected.len())? == spec.expected,
            "map funds-summary source changed for {}",
            spec.id
        );
        let pointer_index = spec
            .pointer_table_index
            .context("map funds-summary label lost its pointer-table index")?;
        ensure!(
            read_u16(
                rom,
                FIXED_STRING_POINTER_TABLE_ADDRESS + u16::from(pointer_index) * 2,
            )? == spec.pointer,
            "map funds-summary pointer changed for {}",
            spec.id
        );
    }
    Ok(())
}

fn read_u16(rom: &Rom, cpu_address: u16) -> Result<u16> {
    let bytes = source_slice(rom, cpu_address, 2)?;
    Ok(u16::from_le_bytes([bytes[0], bytes[1]]))
}

fn source_slice(rom: &Rom, cpu_address: u16, len: usize) -> Result<&[u8]> {
    ensure!(
        (CPU_WINDOW_START..0xC000).contains(&cpu_address),
        "map menu source address {cpu_address:04X} is outside bank 0B"
    );
    let offset = source_file_offset(cpu_address)?;
    let end = offset
        .checked_add(len)
        .context("map menu source overflow")?;
    rom.data()
        .get(offset..end)
        .with_context(|| format!("map menu source exceeds ROM at {offset:05X}"))
}

fn source_file_offset(cpu_address: u16) -> Result<usize> {
    ensure!(
        (CPU_WINDOW_START..0xC000).contains(&cpu_address),
        "map menu source address {cpu_address:04X} is outside bank 0B"
    );
    Ok(HEADER_SIZE
        + usize::from(PRG_BANK) * PRG_BANK_SIZE
        + usize::from(cpu_address - CPU_WINDOW_START))
}

fn decode_source_markup(raw: &[u8]) -> String {
    raw.iter()
        .map(|code| {
            japanese_text_glyph(*code)
                .map(str::to_owned)
                .unwrap_or_else(|| format!("{{{code:02X}}}"))
        })
        .collect()
}

fn hex(bytes: &[u8]) -> String {
    bytes
        .iter()
        .map(|byte| format!("{byte:02X}"))
        .collect::<Vec<_>>()
        .join(" ")
}
