use std::collections::BTreeSet;

use anyhow::{Context, Result, ensure};
use serde::Serialize;

use crate::{
    rom::{HEADER_SIZE, Rom},
    sha1_hex,
    typed_source::decode_rp2a03_sequence,
};

use super::access_trace::{RuntimeAccessTrace, trace_switchable_accesses};

pub(super) const SOURCE_AUDIO_BANK: u8 = 0x0E;
pub(super) const SOURCE_AUDIO_ENTRY: u16 = 0x8000;
const AUDIO_INDIRECT_DISPATCH_SITE: u16 = 0x8A7C;
const AUDIO_HANDLER_TABLE_START: u16 = 0x882D;
const AUDIO_HANDLER_TABLE_BYTE_COUNT: usize = 12 * 16;
const AUDIO_HANDLER_POINTER_COUNT: usize = AUDIO_HANDLER_TABLE_BYTE_COUNT / 2;
const EXPECTED_AUDIO_HANDLER_TABLE_SHA1: &str = "7c8c5418559136a21402a2f4832dcab49abc3670";
const AUDIO_INDIRECT_DISPATCH_CODE: [u8; 3] = [0x6C, 0xF4, 0x00];
const PRG_BANK_SIZE: usize = 16 * 1024;

pub(super) struct SourceAudioExecution {
    pub(super) trace: RuntimeAccessTrace,
    pub(super) indirect_dispatch: SourceAudioIndirectDispatch,
}

#[derive(Serialize)]
pub(super) struct SourceAudioIndirectDispatch {
    strategy: &'static str,
    dispatch_site_hex: String,
    pointer_pair_hex: &'static str,
    handler_table_cpu_range_hex: String,
    handler_table_sha1: String,
    handler_pointer_count: usize,
    unique_handler_target_count: usize,
    handler_target_minimum_hex: String,
    handler_target_maximum_hex: String,
    every_handler_target_in_audio_prg_window: bool,
    every_indirect_control_site_bound: bool,
}

pub(super) fn trace_source_audio_execution(source: &Rom) -> Result<SourceAudioExecution> {
    let table = source_bytes(
        source,
        SOURCE_AUDIO_BANK,
        AUDIO_HANDLER_TABLE_START,
        AUDIO_HANDLER_TABLE_BYTE_COUNT,
    )?;
    ensure!(
        sha1_hex(table) == EXPECTED_AUDIO_HANDLER_TABLE_SHA1,
        "source audio indirect handler table changed"
    );
    let handler_targets = parse_audio_handler_targets(table)?;
    ensure!(
        handler_targets.len() == 88,
        "source audio unique indirect handler target count changed"
    );

    let dispatch = source_bytes(
        source,
        SOURCE_AUDIO_BANK,
        AUDIO_INDIRECT_DISPATCH_SITE,
        AUDIO_INDIRECT_DISPATCH_CODE.len(),
    )?;
    ensure!(
        dispatch == AUDIO_INDIRECT_DISPATCH_CODE,
        "source audio indirect dispatch instruction changed"
    );
    decode_rp2a03_sequence(
        dispatch,
        AUDIO_INDIRECT_DISPATCH_SITE,
        "source audio indirect handler dispatch",
    )?;

    let mut roots = Vec::with_capacity(handler_targets.len() + 1);
    roots.push(SOURCE_AUDIO_ENTRY);
    roots.extend(handler_targets.iter().copied());
    let trace = trace_switchable_accesses(source, SOURCE_AUDIO_BANK, &roots)?;
    let expected_indirect_control_sites =
        BTreeSet::from([(SOURCE_AUDIO_BANK, AUDIO_INDIRECT_DISPATCH_SITE)]);
    ensure!(
        trace.indirect_control_sites == expected_indirect_control_sites,
        "source audio indirect control-flow census changed: expected {expected_indirect_control_sites:?}, traced {:?}",
        trace.indirect_control_sites
    );
    ensure!(
        handler_targets
            .iter()
            .all(|target| trace.visited.contains(&(SOURCE_AUDIO_BANK, *target))),
        "source audio trace did not admit every source-bound indirect handler target"
    );

    let minimum = handler_targets
        .first()
        .context("source audio handler target set is empty")?;
    let maximum = handler_targets
        .last()
        .context("source audio handler target set is empty")?;
    Ok(SourceAudioExecution {
        trace,
        indirect_dispatch: SourceAudioIndirectDispatch {
            strategy: "bind the one reachable JMP-indirect site to all 96 entries of the twelve source audio handler tables, then trace the 88 unique targets as conservative roots",
            dispatch_site_hex: format!("0x{AUDIO_INDIRECT_DISPATCH_SITE:04X}"),
            pointer_pair_hex: "0xF4/0xF5",
            handler_table_cpu_range_hex: format!(
                "0x{AUDIO_HANDLER_TABLE_START:04X}..0x{:04X}",
                AUDIO_HANDLER_TABLE_START + AUDIO_HANDLER_TABLE_BYTE_COUNT as u16
            ),
            handler_table_sha1: sha1_hex(table),
            handler_pointer_count: AUDIO_HANDLER_POINTER_COUNT,
            unique_handler_target_count: handler_targets.len(),
            handler_target_minimum_hex: format!("0x{minimum:04X}"),
            handler_target_maximum_hex: format!("0x{maximum:04X}"),
            every_handler_target_in_audio_prg_window: true,
            every_indirect_control_site_bound: true,
        },
    })
}

fn parse_audio_handler_targets(table: &[u8]) -> Result<BTreeSet<u16>> {
    ensure!(
        table.len() == AUDIO_HANDLER_TABLE_BYTE_COUNT,
        "source audio indirect handler table length changed"
    );
    let mut targets = BTreeSet::new();
    for pointer in table.chunks_exact(2) {
        let target = u16::from_le_bytes([pointer[0], pointer[1]]);
        ensure!(
            (0x8000..0xC000).contains(&target),
            "source audio indirect handler target 0x{target:04X} escaped bank 0E"
        );
        targets.insert(target);
    }
    Ok(targets)
}

fn source_bytes(source: &Rom, bank: u8, address: u16, byte_count: usize) -> Result<&[u8]> {
    ensure!(
        address >= 0x8000 && address < 0xC000,
        "source audio binding escaped its switchable PRG window"
    );
    let offset = HEADER_SIZE + usize::from(bank) * PRG_BANK_SIZE + usize::from(address - 0x8000);
    source
        .data()
        .get(offset..offset + byte_count)
        .context("source audio binding is outside the source ROM")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn table_with(targets: impl IntoIterator<Item = u16>) -> Vec<u8> {
        targets.into_iter().flat_map(u16::to_le_bytes).collect()
    }

    #[test]
    fn handler_table_keeps_duplicate_entries_but_returns_unique_targets() {
        let targets = (0..AUDIO_HANDLER_POINTER_COUNT)
            .map(|index| 0x8800 + u16::try_from(index % 8).unwrap() * 2);
        let parsed = parse_audio_handler_targets(&table_with(targets)).unwrap();

        assert_eq!(parsed.len(), 8);
        assert_eq!(parsed.first(), Some(&0x8800));
        assert_eq!(parsed.last(), Some(&0x880E));
    }

    #[test]
    fn handler_table_rejects_targets_outside_the_audio_prg_window() {
        let mut targets = vec![0x8800; AUDIO_HANDLER_POINTER_COUNT];
        targets[37] = 0xC000;

        let error = parse_audio_handler_targets(&table_with(targets)).unwrap_err();

        assert!(error.to_string().contains("escaped bank 0E"));
    }
}
