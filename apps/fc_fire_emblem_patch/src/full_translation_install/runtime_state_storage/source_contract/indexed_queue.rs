use std::collections::BTreeSet;

use anyhow::{Context, Result, ensure};
use serde::Serialize;

use crate::{rom::Rom, typed_source::decode_rp2a03_sequence};

use super::super::{
    CANDIDATE_START,
    access_trace::{AccessDirection, AccessForm, AccessSite, RuntimeAccessTrace},
};
use super::{
    FIXED_BANK, SourceRegionBinding, bind_source_region, region, report_site_keys, source_bytes,
};

const QUEUE_START: u16 = 0x0781;
const QUEUE_APPEND_LIMIT_EXCLUSIVE: u16 = 0x005F;

const QUEUE_APPEND_CODE: [u8; 19] = [
    0x9D, 0x81, 0x07, 0xE8, 0xE0, 0x5F, 0x90, 0x0A, 0xAE, 0x80, 0x07, 0xA9, 0x00, 0x9D, 0x81, 0x07,
    0x68, 0x68, 0x60,
];
const QUEUE_SERIALIZER_TAIL_CODE: [u8; 20] = [
    0x8E, 0x80, 0x07, 0x20, 0x1C, 0xC8, 0xC6, 0x04, 0xD0, 0x9D, 0xA9, 0x00, 0x9D, 0x81, 0x07, 0xA0,
    0x01, 0x84, 0x21, 0x60,
];

#[derive(Serialize)]
pub(super) struct IndexedQueueContract {
    role: &'static str,
    sites: Vec<String>,
    source_regions: Vec<SourceRegionBinding>,
    queue_start_hex: String,
    hard_limit_exclusive_hex: String,
    highest_reachable_queue_address_hex: String,
    candidate_offset_from_queue_start: usize,
    candidate_begins_after_hard_queue_limit: bool,
    successful_append_returns_only_below_limit: bool,
    overflow_restores_previous_terminator_and_aborts_serializer: bool,
    serializer_terminal_store_inherits_append_bound: bool,
}

impl IndexedQueueContract {
    pub(super) fn candidate_begins_after_hard_queue_limit(&self) -> bool {
        self.candidate_begins_after_hard_queue_limit
    }
}

pub(super) fn bind_indexed_queue_contract(
    source: &Rom,
    trace: &RuntimeAccessTrace,
) -> Result<IndexedQueueContract> {
    let expected_sites = BTreeSet::from([
        AccessSite {
            bank: 0x0F,
            address: 0xC4A2,
            access: AccessDirection::Write,
            form: AccessForm::AbsoluteX,
            operand: QUEUE_START,
        },
        AccessSite {
            bank: 0x0F,
            address: 0xC4AF,
            access: AccessDirection::Write,
            form: AccessForm::AbsoluteX,
            operand: QUEUE_START,
        },
        AccessSite {
            bank: 0x0F,
            address: 0xC8C5,
            access: AccessDirection::Write,
            form: AccessForm::AbsoluteX,
            operand: QUEUE_START,
        },
    ]);
    ensure!(
        trace.indexed_potential_overlaps == expected_sites,
        "main-dialogue indexed queue access census changed"
    );
    let append = source_bytes(source, FIXED_BANK, 0xC4A2, QUEUE_APPEND_CODE.len())?;
    ensure!(
        append == QUEUE_APPEND_CODE,
        "PPU queue append contract changed"
    );
    decode_rp2a03_sequence(append, 0xC4A2, "bounded PPU queue append")?;
    let serializer_tail =
        source_bytes(source, FIXED_BANK, 0xC8B9, QUEUE_SERIALIZER_TAIL_CODE.len())?;
    ensure!(
        serializer_tail == QUEUE_SERIALIZER_TAIL_CODE,
        "PPU queue serializer tail changed"
    );
    decode_rp2a03_sequence(serializer_tail, 0xC8B9, "PPU queue serializer tail")?;

    let highest_reachable_queue_address = QUEUE_START
        .checked_add(QUEUE_APPEND_LIMIT_EXCLUSIVE - 1)
        .context("PPU queue hard-limit address overflow")?;
    let candidate_offset_from_queue_start = usize::from(CANDIDATE_START - QUEUE_START);
    let candidate_begins_after_hard_queue_limit = CANDIDATE_START > highest_reachable_queue_address;
    ensure!(
        candidate_begins_after_hard_queue_limit,
        "runtime-state candidate is inside the PPU queue hard limit"
    );

    Ok(IndexedQueueContract {
        role: "bounded_ppu_command_queue",
        sites: report_site_keys(&expected_sites),
        source_regions: vec![
            bind_source_region(
                source,
                region(
                    0x0F,
                    0xC4A2,
                    QUEUE_APPEND_CODE.len(),
                    "queue append and overflow abort",
                ),
            )?,
            bind_source_region(
                source,
                region(
                    0x0F,
                    0xC842,
                    0x008B,
                    "stage serializer and terminal publication",
                ),
            )?,
        ],
        queue_start_hex: format!("0x{QUEUE_START:04X}"),
        hard_limit_exclusive_hex: format!("0x{QUEUE_APPEND_LIMIT_EXCLUSIVE:02X}"),
        highest_reachable_queue_address_hex: format!("0x{highest_reachable_queue_address:04X}"),
        candidate_offset_from_queue_start,
        candidate_begins_after_hard_queue_limit,
        successful_append_returns_only_below_limit: true,
        overflow_restores_previous_terminator_and_aborts_serializer: true,
        serializer_terminal_store_inherits_append_bound: true,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn queue_hard_limit_stops_before_the_candidate() {
        let highest = QUEUE_START + QUEUE_APPEND_LIMIT_EXCLUSIVE - 1;
        assert_eq!(highest, 0x07DF);
        assert!(highest < CANDIDATE_START);
    }
}
