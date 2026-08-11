use std::{fs, path::Path};

use anyhow::{Context, Result, ensure};
use serde::Deserialize;

use crate::{
    rom::{CHR_FILE_OFFSET, EXPECTED_SOURCE_SHA1, HEADER_SIZE, Rom},
    sha1_hex,
    text_inventory::is_japanese_character,
};

mod install;
mod logo_asset;

pub(crate) use install::install_title_logo_asset;
pub(crate) use logo_asset::build_title_logo_asset;

const PRG_BANK_SIZE: usize = 16 * 1024;
const SOURCE_PRG_BANK: u8 = 0x0D;
const CPU_WINDOW_START: u16 = 0x8000;
const TITLE_STREAM_ADDRESS: u16 = 0xB2B0;
pub(super) const TITLE_STREAM_BYTE_COUNT: usize = 180;
const TITLE_ROW_COUNT: usize = 5;
const TITLE_ROW_WIDTH: usize = 32;
const TITLE_FIRST_PPU_ADDRESS: u16 = 0x21A0;
pub(super) const TITLE_TRANSLATION_FIRST_COLUMN: usize = 2;
pub(super) const TITLE_TRANSLATION_END_COLUMN_EXCLUSIVE: usize = 29;
const PRESERVED_TM_COLUMN: usize = 29;
const PRESERVED_TM_TILE: u8 = 0xBB;
const TITLE_CHR_PAGE: usize = 0x14;
const CHR_PAGE_BYTES: usize = 4 * 1024;
const TITLE_CHR_PAGE_SHA1: &str = "dd382dfe729f44e3ee493fadde2394862828affd";
const TITLE_STREAM_SHA1: &str = "bdd564533623646556668eef5da652669f0c9382";

#[derive(Debug, Deserialize)]
struct Workspace {
    format_version: u8,
    source_sha1: String,
    translate_from: String,
    translate_to: String,
    preserve_existing_english_and_digits: bool,
    entry: WorkspaceEntry,
}

#[derive(Debug, Deserialize)]
struct WorkspaceEntry {
    id: String,
    source_prg_bank_hex: String,
    source_cpu_address_hex: String,
    source_stream_byte_count: usize,
    source_stream_sha1: String,
    source_chr_page_hex: String,
    source_chr_page_file_offset_hex: String,
    source_chr_page_sha1: String,
    japanese_markup: String,
    korean_markup: String,
    preserved_original_text: Vec<String>,
    status: String,
}

pub(crate) struct TitleGraphicsPlan {
    pub(crate) translated_surface_count: usize,
    pub(crate) review_complete: bool,
    pub(crate) workspace_sha1: String,
}

pub(crate) fn plan_title_graphics(rom: &Rom, workspace_path: &Path) -> Result<TitleGraphicsPlan> {
    bind_source(rom)?;
    let bytes = fs::read(workspace_path)
        .with_context(|| format!("read title graphics workspace {}", workspace_path.display()))?;
    let workspace: Workspace = serde_json::from_slice(&bytes).with_context(|| {
        format!(
            "parse title graphics workspace {}",
            workspace_path.display()
        )
    })?;
    ensure!(
        workspace.format_version == 1,
        "unsupported title graphics format"
    );
    ensure!(
        workspace.source_sha1 == EXPECTED_SOURCE_SHA1
            && workspace.translate_from == "ja"
            && workspace.translate_to == "ko"
            && workspace.preserve_existing_english_and_digits,
        "title graphics workspace contract changed"
    );
    let entry = &workspace.entry;
    ensure!(
        entry.id == "title-logo"
            && entry.source_prg_bank_hex == "0x0D"
            && entry.source_cpu_address_hex == "0xB2B0"
            && entry.source_stream_byte_count == TITLE_STREAM_BYTE_COUNT
            && entry.source_stream_sha1 == TITLE_STREAM_SHA1
            && entry.source_chr_page_hex == "0x14"
            && entry.source_chr_page_file_offset_hex == "0x54010"
            && entry.source_chr_page_sha1 == TITLE_CHR_PAGE_SHA1
            && entry.japanese_markup == "ファイアーエムブレム"
            && entry.preserved_original_text == ["TM", "©1990 Nintendo"],
        "title graphics workspace source binding changed"
    );
    ensure!(
        matches!(
            entry.status.as_str(),
            "untranslated" | "needs_human_review" | "complete"
        ),
        "invalid title graphics status"
    );
    ensure!(
        (entry.status == "untranslated") == entry.korean_markup.is_empty(),
        "title graphics text and status disagree"
    );
    if !entry.korean_markup.is_empty() {
        ensure!(
            !entry.korean_markup.chars().any(is_japanese_character)
                && entry
                    .korean_markup
                    .chars()
                    .all(|character| ('가'..='힣').contains(&character)),
            "title translation must contain Hangul only"
        );
    }
    Ok(TitleGraphicsPlan {
        translated_surface_count: usize::from(!entry.korean_markup.is_empty()),
        review_complete: entry.status == "complete",
        workspace_sha1: sha1_hex(&bytes),
    })
}

pub(super) fn bind_source(rom: &Rom) -> Result<()> {
    let stream = source_stream(rom)?;
    ensure!(
        sha1_hex(stream) == TITLE_STREAM_SHA1,
        "title tilemap stream changed"
    );
    let mut cursor = 0;
    for row in 0..TITLE_ROW_COUNT {
        let expected_address = TITLE_FIRST_PPU_ADDRESS + (row * TITLE_ROW_WIDTH) as u16;
        ensure!(
            u16::from_be_bytes([stream[cursor], stream[cursor + 1]]) == expected_address,
            "title tilemap row {row} PPU address changed"
        );
        ensure!(
            stream[cursor + 2] == TITLE_ROW_WIDTH as u8,
            "title tilemap row {row} width changed"
        );
        let row_bytes = &stream[cursor + 3..cursor + 3 + TITLE_ROW_WIDTH];
        ensure!(
            row_bytes[TITLE_TRANSLATION_FIRST_COLUMN..TITLE_TRANSLATION_END_COLUMN_EXCLUSIVE]
                .iter()
                .any(|tile| *tile != 0xFF),
            "title Japanese surface row {row} disappeared"
        );
        if row == TITLE_ROW_COUNT - 1 {
            ensure!(
                row_bytes[PRESERVED_TM_COLUMN] == PRESERVED_TM_TILE,
                "preserved title TM tile changed"
            );
        }
        ensure!(
            stream[cursor + 3 + TITLE_ROW_WIDTH] == 0,
            "title tilemap row {row} command terminator changed"
        );
        cursor += 3 + TITLE_ROW_WIDTH + 1;
    }
    ensure!(
        cursor == stream.len(),
        "title tilemap stream has trailing data"
    );

    let chr_page = source_chr_page(rom)?;
    ensure!(
        sha1_hex(chr_page) == TITLE_CHR_PAGE_SHA1,
        "title CHR page changed"
    );
    ensure!(
        CHR_FILE_OFFSET + TITLE_CHR_PAGE * CHR_PAGE_BYTES == 0x54010,
        "title CHR page file location changed"
    );
    Ok(())
}

pub(super) fn source_stream(rom: &Rom) -> Result<&[u8]> {
    let offset = title_stream_file_offset();
    let end = offset
        .checked_add(TITLE_STREAM_BYTE_COUNT)
        .context("title stream overflow")?;
    rom.data()
        .get(offset..end)
        .with_context(|| format!("title stream exceeds ROM at {offset:05X}"))
}

pub(super) fn title_stream_file_offset() -> usize {
    HEADER_SIZE
        + usize::from(SOURCE_PRG_BANK) * PRG_BANK_SIZE
        + usize::from(TITLE_STREAM_ADDRESS - CPU_WINDOW_START)
}

pub(super) fn source_chr_page(rom: &Rom) -> Result<&[u8]> {
    let chr_start = TITLE_CHR_PAGE
        .checked_mul(CHR_PAGE_BYTES)
        .context("title CHR page offset overflow")?;
    rom.chr()
        .get(chr_start..chr_start + CHR_PAGE_BYTES)
        .context("title CHR page is outside the ROM")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn public_title_workspace_separates_logo_from_preserved_original_text() {
        let source = Path::new(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../../roms/Fire Emblem - Ankoku Ryuu to Hikari no Tsurugi (Japan).nes"
        ));
        if !source.exists() {
            return;
        }
        let workspace = Path::new(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../../assets/translation/title-logo.ko.json"
        ));
        let rom = Rom::from_path(source).unwrap();
        let plan = plan_title_graphics(&rom, workspace).unwrap();

        assert_eq!(plan.translated_surface_count, 1);
        assert!(!plan.review_complete);
    }
}
