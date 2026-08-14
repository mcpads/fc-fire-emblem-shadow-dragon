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
};

const PRG_BANK_SIZE: usize = 16 * 1024;
const PRG_BANK: u8 = 0x0B;
const CPU_WINDOW_START: u16 = 0x8000;
const COMPOSITE_DISPATCH_TABLE_ADDRESS: u16 = 0x8006;
const COMPOSER_STATE: u8 = 3;
const COMPOSER_ADDRESS: u16 = 0x8187;
const COMPOSITE_POINTER_ROLE: &str = "composite_pointer";
const COMPOSER_ROLE: &str = "compose_map_menu";
const LABEL_BLOCK_ROLE: &str = "map_menu_label_block";
const COMPOSER: &[u8] = &[
    0xA9, 0x0E, 0x8D, 0xD0, 0x05, 0xA9, 0x0C, 0x8D, 0xCF, 0x05, 0x20, 0xC8, 0x97, 0x20, 0x3C, 0x8E,
    0xAE, 0xCE, 0x05, 0xA9, 0x3F, 0x9D, 0xEE, 0x7F, 0xA9, 0x01, 0x9D, 0xF3, 0x7F, 0xBD, 0xB2, 0x81,
    0x9D, 0x51, 0x04, 0xE8, 0xC9, 0xEF, 0xD0, 0xF5, 0x4C, 0x39, 0x8F,
];
const LABEL_BLOCK_ADDRESS: u16 = 0x81B2;
const LABEL_BLOCK_END: u8 = 0xEF;

struct LabelSpec {
    id: &'static str,
    pointer: u16,
    expected: &'static [u8],
}

const LABELS: &[LabelSpec] = &[
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

const fn label(id: &'static str, pointer: u16, expected: &'static [u8]) -> LabelSpec {
    LabelSpec {
        id,
        pointer,
        expected,
    }
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
    logical_bytes: Vec<FixedTextLogicalByte>,
}

impl MapMenuPlannedEntry {
    pub(crate) fn logical_bytes(&self) -> &[FixedTextLogicalByte] {
        &self.logical_bytes
    }
}

/// 여섯 지도 메뉴 라벨의 원천 블록과 composite-state 생산자를 한 소비자 증거로
/// 반환한다. 번역 작업공간은 이 원천 census의 일부가 아니다.
pub(crate) fn inspect_map_menu_translation_consumers(
    rom: &Rom,
) -> Result<TranslationConsumerSourceEvidence> {
    bind_source(rom)?;
    let population_ids = LABELS
        .iter()
        .map(|spec| spec.id.to_owned())
        .collect::<Vec<_>>();
    Ok(TranslationConsumerSourceEvidence {
        population_ids: population_ids.clone(),
        screen_bindings: vec![ScreenConsumerSourceBinding {
            screen_role: "map_menu",
            population_ids,
            source_binding_ids: vec![
                qualified_source_binding_id(
                    usize::from(PRG_BANK),
                    COMPOSITE_DISPATCH_TABLE_ADDRESS + u16::from(COMPOSER_STATE) * 2,
                    COMPOSITE_POINTER_ROLE,
                    &format!("state={COMPOSER_STATE:02X},composer={COMPOSER_ADDRESS:04X}"),
                ),
                source_binding_id(usize::from(PRG_BANK), COMPOSER_ADDRESS, COMPOSER_ROLE),
                source_binding_id(usize::from(PRG_BANK), LABEL_BLOCK_ADDRESS, LABEL_BLOCK_ROLE),
            ],
        }],
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
        workspace.entries.len() == LABELS.len(),
        "map menu workspace must contain all six labels"
    );

    let mut translated_entry_count = 0;
    let mut target_glyphs = BTreeSet::new();
    let mut entries = Vec::with_capacity(LABELS.len());
    for (entry, spec) in workspace.entries.iter().zip(LABELS) {
        let source_markup = decode_source_markup(&spec.expected[..spec.expected.len() - 1]);
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
            ensure!(
                logical_bytes.len() <= 6,
                "map menu translation exceeds six visible cells for {}",
                spec.id
            );
            translated_entry_count += 1;
            target_glyphs.extend(entry.korean_markup.chars());
            entries.push(MapMenuPlannedEntry {
                id: entry.id.clone(),
                source_cpu_address: spec.pointer,
                source_file_offset: source_file_offset(spec.pointer)?,
                source_storage: spec.expected.to_vec(),
                logical_bytes,
            });
        }
    }
    let active_codes = active_hangul_codes().into_iter().collect::<BTreeSet<_>>();
    let source_reclaimable_active_codes = LABELS
        .iter()
        .flat_map(|spec| spec.expected[..spec.expected.len() - 1].iter().copied())
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
    let dispatch_pointer = read_u16(
        rom,
        COMPOSITE_DISPATCH_TABLE_ADDRESS + u16::from(COMPOSER_STATE) * 2,
    )?;
    ensure!(
        dispatch_pointer == COMPOSER_ADDRESS,
        "map menu composite state no longer selects its composer"
    );
    let composer = source_slice(rom, COMPOSER_ADDRESS, COMPOSER.len())?;
    ensure!(composer == COMPOSER, "map menu composer changed");
    ensure!(
        sha1_hex(composer) == "606e2dd311cce69732a44278b9dfd019ecb977e5",
        "map menu composer hash changed"
    );
    decode_rp2a03_sequence(composer, COMPOSER_ADDRESS, "compose map menu")?;

    let mut cursor = LABEL_BLOCK_ADDRESS;
    for spec in LABELS {
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
