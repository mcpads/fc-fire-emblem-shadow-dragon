//! 전투 텍스트와 같은 화면 수명에서 쓰는 지형·효과 스프라이트 CHR 공급을 재결속한다.

use std::collections::BTreeSet;

use anyhow::{Context, Result, ensure};
use serde::Serialize;

use crate::{
    font_slots::FONT_PAGE_SIZE,
    mmc5_chr::switchable_bank_file_offset,
    rom::{HEADER_SIZE, Rom},
    sha1_hex,
};

use super::super::{
    direct_chr_pairs::{ImmediateLeftFdWriterSpec, immediate_left_fd_writer_specs},
    runtime::build_left_fd_chr_bank_selection_routine,
};

const BATTLE_PRG_BANK: u8 = 0x05;
const OUTPUT_SOURCE_CHR_PAGE_OFFSET: usize = 2;
const TERRAIN_PREVIEW_SOURCE_PAGE: u8 = 0x16;

#[derive(Debug, Serialize)]
pub(super) struct BattleChrSupplyPlan {
    strategy: &'static str,
    immediate_left_fd_writer_count: usize,
    terrain_preview_writer_count: usize,
    effect_object_writer_count: usize,
    distinct_source_page_count: usize,
    source_pages_hex: Vec<String>,
    selector_cpu_address_hex: String,
    selector_byte_count: usize,
    writer_binding_sha1: String,
    source_page_binding_sha1: String,
    every_source_writer_redirected_in_cumulative_candidate: bool,
    every_redirect_preserved_in_integrated_image: bool,
    selector_rebound_in_both_artifacts: bool,
    every_referenced_sprite_page_matches_the_source: bool,
}

pub(super) fn bind_battle_chr_supply(
    source: &Rom,
    cumulative: &Rom,
    integrated: &Rom,
) -> Result<BattleChrSupplyPlan> {
    let specs = immediate_left_fd_writer_specs();
    ensure!(
        specs.len() == 22,
        "battle immediate left-FD writer population changed"
    );
    let terrain_preview_writer_count = specs
        .iter()
        .filter(|writer| writer.source_page == TERRAIN_PREVIEW_SOURCE_PAGE)
        .count();
    ensure!(
        terrain_preview_writer_count == 1,
        "battle terrain-preview CHR writer population changed"
    );

    let mut writer_binding_bytes = Vec::new();
    for writer in &specs {
        let writer_offset = switchable_bank_file_offset(BATTLE_PRG_BANK, writer.writer_address)?;
        let sequence_offset = writer_offset
            .checked_sub(2)
            .context("battle CHR writer has no immediate page producer")?;
        let source_sequence = source
            .data()
            .get(sequence_offset..sequence_offset + 5)
            .with_context(|| format!("source {} is outside the ROM", writer.role))?;
        let cumulative_sequence = cumulative
            .data()
            .get(sequence_offset..sequence_offset + 5)
            .with_context(|| format!("cumulative {} is outside the ROM", writer.role))?;
        let integrated_sequence = integrated
            .data()
            .get(sequence_offset..sequence_offset + 5)
            .with_context(|| format!("integrated {} is outside the ROM", writer.role))?;
        verify_writer_sequences(
            writer,
            source_sequence,
            cumulative_sequence,
            integrated_sequence,
        )?;
        writer_binding_bytes.extend_from_slice(&writer.writer_address.to_le_bytes());
        writer_binding_bytes.push(writer.source_page);
        writer_binding_bytes.extend_from_slice(integrated_sequence);
    }

    let selector = build_left_fd_chr_bank_selection_routine()?;
    let cumulative_selector =
        active_fixed_bytes(cumulative, selector.cpu_address, selector.bytes.len())?;
    let integrated_selector =
        active_fixed_bytes(integrated, selector.cpu_address, selector.bytes.len())?;
    ensure!(
        cumulative_selector == selector.bytes && integrated_selector == selector.bytes,
        "battle left-FD CHR selector changed in the cumulative or integrated artifact"
    );

    let source_pages = specs
        .iter()
        .map(|writer| writer.source_page)
        .collect::<BTreeSet<_>>();
    let mut source_page_binding_bytes = Vec::new();
    for source_page in &source_pages {
        let source_page_bytes = chr_page(source, usize::from(*source_page))?;
        let output_page = usize::from(*source_page)
            .checked_add(OUTPUT_SOURCE_CHR_PAGE_OFFSET)
            .context("battle output CHR page overflow")?;
        let cumulative_page_bytes = chr_page(cumulative, output_page)?;
        let integrated_page_bytes = chr_page(integrated, output_page)?;
        ensure!(
            cumulative_page_bytes == source_page_bytes
                && integrated_page_bytes == source_page_bytes,
            "battle sprite CHR source page {source_page:02X} changed after mapper relocation or integration"
        );
        source_page_binding_bytes.push(*source_page);
        source_page_binding_bytes.extend_from_slice(source_page_bytes);
    }

    Ok(BattleChrSupplyPlan {
        strategy: "rebind every immediate battle left-FD producer, its NMI-safe mapper selector, and every referenced terrain/effect sprite page on the exact cumulative and integrated artifacts",
        immediate_left_fd_writer_count: specs.len(),
        terrain_preview_writer_count,
        effect_object_writer_count: specs.len() - terrain_preview_writer_count,
        distinct_source_page_count: source_pages.len(),
        source_pages_hex: source_pages
            .into_iter()
            .map(|page| format!("0x{page:02X}"))
            .collect(),
        selector_cpu_address_hex: format!("0x{:04X}", selector.cpu_address),
        selector_byte_count: selector.bytes.len(),
        writer_binding_sha1: sha1_hex(&writer_binding_bytes),
        source_page_binding_sha1: sha1_hex(&source_page_binding_bytes),
        every_source_writer_redirected_in_cumulative_candidate: true,
        every_redirect_preserved_in_integrated_image: true,
        selector_rebound_in_both_artifacts: true,
        every_referenced_sprite_page_matches_the_source: true,
    })
}

fn verify_writer_sequences(
    writer: &ImmediateLeftFdWriterSpec,
    source: &[u8],
    cumulative: &[u8],
    integrated: &[u8],
) -> Result<()> {
    let expected_source = [0xA9, writer.source_page, 0x8D, 0x00, 0xB0];
    let expected_redirect = [0xA9, writer.source_page, 0x20, 0x40, 0xFA];
    ensure!(
        source == expected_source,
        "source {} immediate CHR producer changed",
        writer.role
    );
    ensure!(
        cumulative == expected_redirect,
        "cumulative {} was not redirected to the left-FD selector",
        writer.role
    );
    ensure!(
        integrated == expected_redirect,
        "integrated {} no longer preserves the left-FD redirect",
        writer.role
    );
    Ok(())
}

fn chr_page(rom: &Rom, page: usize) -> Result<&[u8]> {
    let start = page
        .checked_mul(FONT_PAGE_SIZE)
        .context("battle CHR page offset overflow")?;
    rom.chr()
        .get(start..start + FONT_PAGE_SIZE)
        .with_context(|| format!("battle CHR page {page:02X} is outside the ROM"))
}

fn active_fixed_bytes(rom: &Rom, address: u16, byte_count: usize) -> Result<&[u8]> {
    ensure!(
        rom.prg().len() >= 0x4000 && (0xC000..=0xFFFF).contains(&address),
        "battle selector is outside the active fixed bank"
    );
    let offset = HEADER_SIZE + rom.prg().len() - 0x4000 + usize::from(address - 0xC000);
    rom.data()
        .get(offset..offset + byte_count)
        .context("battle selector extends outside the active fixed bank")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn writer() -> ImmediateLeftFdWriterSpec {
        ImmediateLeftFdWriterSpec {
            role: "terrain preview",
            writer_address: 0x962F,
            source_page: TERRAIN_PREVIEW_SOURCE_PAGE,
        }
    }

    #[test]
    fn writer_redirect_keeps_the_immediate_page_and_changes_only_the_store() {
        verify_writer_sequences(
            &writer(),
            &[0xA9, 0x16, 0x8D, 0x00, 0xB0],
            &[0xA9, 0x16, 0x20, 0x40, 0xFA],
            &[0xA9, 0x16, 0x20, 0x40, 0xFA],
        )
        .unwrap();
    }

    #[test]
    fn later_integration_cannot_restore_the_raw_mmc4_write() {
        let error = verify_writer_sequences(
            &writer(),
            &[0xA9, 0x16, 0x8D, 0x00, 0xB0],
            &[0xA9, 0x16, 0x20, 0x40, 0xFA],
            &[0xA9, 0x16, 0x8D, 0x00, 0xB0],
        )
        .unwrap_err();

        assert!(error.to_string().contains("no longer preserves"));
    }
}
