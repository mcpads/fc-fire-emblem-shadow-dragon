use anyhow::{Context, Result, ensure};
use serde::Serialize;

use crate::chapter_victory::validate_chapter_clear_command_route;

use super::report::MaximumTransitionChainReport;

mod chapter_events;
mod runtime_lifetime;
mod source_regions;
#[cfg(test)]
mod tests;

pub(super) use runtime_lifetime::bind_runtime_lifetime;
pub(crate) use runtime_lifetime::{RuntimeEvidence, load_runtime_evidence};

use chapter_events::{
    ChapterEventRecord, DataRegionBinding, bind_chapter_event_directory, bind_chapter_map_pointers,
    bind_chapter_map_sample,
};
use source_regions::{SourceRegionBinding, bind_source_regions};

const TABLE_ID: &str = "village-and-outro-dialogue";
const ENTRY_INDEX: usize = 24;
const ENTRY_SELECTOR: u8 = 0xC0;
const SOURCE_PRG_BANK: u8 = 0x0C;

const MAXIMUM_PRODUCER_CHAPTER: u8 = 7;
const MAXIMUM_PRODUCER_ROW: u8 = 27;
const MAXIMUM_PRODUCER_COLUMN: u8 = 10;
const MAXIMUM_PRODUCER_MAP_POINTER: u16 = 0x8F12;
const MAXIMUM_PRODUCER_MAP_HEADER: [u8; 4] = [0x1D, 0x0F, 0x0E, 0x00];
const CASTLE_TILE_CODE: u8 = 0x4B;
const CHAPTER_CLEAR_MAIN_STATE: u8 = 0x3C;
const CHAPTER_CLEAR_OUTER_SCREEN_STATE: u8 = 0x0C;
const CHAPTER_CLEAR_DIALOGUE_STAGE: u8 = 2;

const OTHER_SELECTOR_CHAPTER: u8 = 11;
const OTHER_SELECTOR_ROW: u8 = 17;
const OTHER_SELECTOR_COLUMN: u8 = 22;
const OTHER_SELECTOR_MAP_POINTER: u16 = 0x9C42;
const OTHER_SELECTOR_MAP_HEADER: [u8; 4] = [0x18, 0x1F, 0x0A, 0x00];
const OTHER_SELECTOR_TILE_CODE: u8 = 0x46;
const OTHER_MAIN_STATE: u8 = 0x37;
const OTHER_DIRECTORY_SELECTOR: u8 = 0x30;

#[derive(Debug, Serialize)]
pub(super) struct MaximumDialogueSourceBinding {
    pub(super) binding_status: &'static str,
    table_id: &'static str,
    canonical_entry_index: usize,
    source_prg_bank: u8,
    source_prg_bank_hex: &'static str,
    pub(super) runtime_directory_selector: u8,
    runtime_directory_selector_hex: &'static str,
    current_chapter_address: u16,
    current_chapter_address_hex: &'static str,
    chapter_event_directory: DataRegionBinding,
    chapter_map_pointer_table: DataRegionBinding,
    pub(super) producer: DialogueProducerBinding,
    pub(super) same_entry_other_selector: OtherSelectorBinding,
    source_regions: Vec<SourceRegionBinding>,
    pub(super) screen_lifetime_bound: bool,
    runtime_screen_lifetime: Option<runtime_lifetime::MaximumDialogueRuntimeLifetimeBinding>,
    pub(super) next_gate: &'static str,
}

impl MaximumDialogueSourceBinding {
    pub(super) fn observed_screen_lifetime_report(
        &self,
        active_slot_count: usize,
        review_complete: bool,
    ) -> Option<super::report::ObservedScreenLifetimeReport> {
        self.runtime_screen_lifetime.as_ref().map(|lifetime| {
            lifetime.observed_screen_lifetime_report(active_slot_count, review_complete)
        })
    }
}

#[derive(Debug, Serialize)]
pub(super) struct DialogueProducerBinding {
    pub(super) chapter_number: u8,
    chapter_event_record_index: usize,
    chapter_event_record_file_offset: usize,
    chapter_event_record_file_offset_hex: String,
    pub(super) row: u8,
    pub(super) column: u8,
    event_code: u8,
    event_code_hex: String,
    event_value: u8,
    event_value_hex: String,
    map_pointer: u16,
    map_pointer_hex: String,
    map_sha1: String,
    pub(super) terrain_tile_code: u8,
    terrain_tile_code_hex: String,
    pub(super) selected_main_state: u8,
    selected_main_state_hex: String,
    selected_outer_screen_state: u8,
    selected_outer_screen_state_hex: String,
    pub(super) selected_stage: u8,
    dialogue_entry_address: u16,
    dialogue_entry_address_hex: &'static str,
    dialogue_selector_address: u16,
    dialogue_selector_address_hex: &'static str,
}

#[derive(Debug, Serialize)]
pub(super) struct OtherSelectorBinding {
    pub(super) chapter_number: u8,
    chapter_event_record_index: usize,
    row: u8,
    column: u8,
    terrain_tile_code: u8,
    terrain_tile_code_hex: String,
    selected_main_state: u8,
    selected_main_state_hex: String,
    pub(super) selected_directory_selector: u8,
    selected_directory_selector_hex: String,
    exclusion_reason: &'static str,
}

pub(super) fn bind_maximum_dialogue_source(
    prg: &[u8],
    maximum: &MaximumTransitionChainReport,
) -> Result<MaximumDialogueSourceBinding> {
    validate_chapter_clear_command_route(prg)?;
    bind_maximum_dialogue_source_records(prg, maximum)
}

fn bind_maximum_dialogue_source_records(
    prg: &[u8],
    maximum: &MaximumTransitionChainReport,
) -> Result<MaximumDialogueSourceBinding> {
    ensure!(
        maximum.start_table_id == TABLE_ID
            && maximum.start_canonical_entry_index == ENTRY_INDEX
            && maximum.record_count == 1
            && maximum.table_ids == [TABLE_ID],
        "maximum main-dialogue chain is no longer the source-bound C0:18 record"
    );

    let source_regions = bind_source_regions(prg)?;
    let (event_records, chapter_event_directory) = bind_chapter_event_directory(prg)?;
    let (map_pointers, chapter_map_pointer_table) = bind_chapter_map_pointers(prg)?;
    let matching = event_records
        .iter()
        .filter(|record| usize::from(record.dialogue_entry) == ENTRY_INDEX)
        .collect::<Vec<_>>();
    ensure!(
        matching.len() == 2,
        "expected exactly two chapter-event records with entry 0x18, found {}",
        matching.len()
    );

    let producer_record = find_matching_record(&matching, MAXIMUM_PRODUCER_CHAPTER)
        .context("chapter-seven C0:18 producer is absent")?;
    ensure!(
        producer_record.record_index == 1
            && producer_record.row == MAXIMUM_PRODUCER_ROW
            && producer_record.column == MAXIMUM_PRODUCER_COLUMN
            && producer_record.event_code == 0
            && producer_record.value == 0x20,
        "chapter-seven C0:18 event record changed"
    );
    let producer_map = bind_chapter_map_sample(
        prg,
        &map_pointers,
        producer_record,
        MAXIMUM_PRODUCER_MAP_POINTER,
        MAXIMUM_PRODUCER_MAP_HEADER,
    )?;
    ensure!(
        producer_map.tile_code == CASTLE_TILE_CODE,
        "chapter-seven C0:18 producer is no longer on castle tile 0x4B"
    );

    let other_record = find_matching_record(&matching, OTHER_SELECTOR_CHAPTER)
        .context("chapter-eleven same-entry record is absent")?;
    ensure!(
        other_record.record_index == 6
            && other_record.row == OTHER_SELECTOR_ROW
            && other_record.column == OTHER_SELECTOR_COLUMN
            && other_record.event_code == 0
            && other_record.value == 0,
        "chapter-eleven same-entry event record changed"
    );
    let other_map = bind_chapter_map_sample(
        prg,
        &map_pointers,
        other_record,
        OTHER_SELECTOR_MAP_POINTER,
        OTHER_SELECTOR_MAP_HEADER,
    )?;
    ensure!(
        other_map.tile_code == OTHER_SELECTOR_TILE_CODE,
        "chapter-eleven same-entry record is no longer on terrain 0x46"
    );

    Ok(MaximumDialogueSourceBinding {
        binding_status: "source_bound_runtime_screen_unobserved",
        table_id: TABLE_ID,
        canonical_entry_index: ENTRY_INDEX,
        source_prg_bank: SOURCE_PRG_BANK,
        source_prg_bank_hex: "0x0C",
        runtime_directory_selector: ENTRY_SELECTOR,
        runtime_directory_selector_hex: "0xC0",
        current_chapter_address: 0x7674,
        current_chapter_address_hex: "0x7674",
        chapter_event_directory,
        chapter_map_pointer_table,
        producer: producer_binding(producer_record, producer_map),
        same_entry_other_selector: OtherSelectorBinding {
            chapter_number: other_record.chapter_number,
            chapter_event_record_index: other_record.record_index,
            row: other_record.row,
            column: other_record.column,
            terrain_tile_code: other_map.tile_code,
            terrain_tile_code_hex: format!("0x{:02X}", other_map.tile_code),
            selected_main_state: OTHER_MAIN_STATE,
            selected_main_state_hex: format!("0x{OTHER_MAIN_STATE:02X}"),
            selected_directory_selector: OTHER_DIRECTORY_SELECTOR,
            selected_directory_selector_hex: format!("0x{OTHER_DIRECTORY_SELECTOR:02X}"),
            exclusion_reason: "the same numeric entry on terrain 0x46 runs the separate 0x30 directory-selector route, so it is not a producer of C0:18",
        },
        source_regions,
        screen_lifetime_bound: false,
        runtime_screen_lifetime: None,
        next_gate: "observe the chapter-seven castle-clear C0:18 pages and bind their simultaneous non-dialogue active codes",
    })
}

fn find_matching_record<'a>(
    records: &[&'a ChapterEventRecord],
    chapter_number: u8,
) -> Option<&'a ChapterEventRecord> {
    records
        .iter()
        .copied()
        .find(|record| record.chapter_number == chapter_number)
}

fn producer_binding(
    record: &ChapterEventRecord,
    map: chapter_events::ChapterMapSample,
) -> DialogueProducerBinding {
    DialogueProducerBinding {
        chapter_number: record.chapter_number,
        chapter_event_record_index: record.record_index,
        chapter_event_record_file_offset: record.file_offset,
        chapter_event_record_file_offset_hex: format!("0x{:05X}", record.file_offset),
        row: record.row,
        column: record.column,
        event_code: record.event_code,
        event_code_hex: format!("0x{:02X}", record.event_code),
        event_value: record.value,
        event_value_hex: format!("0x{:02X}", record.value),
        map_pointer: map.pointer,
        map_pointer_hex: format!("0x{:04X}", map.pointer),
        map_sha1: map.map_sha1,
        terrain_tile_code: map.tile_code,
        terrain_tile_code_hex: format!("0x{:02X}", map.tile_code),
        selected_main_state: CHAPTER_CLEAR_MAIN_STATE,
        selected_main_state_hex: format!("0x{CHAPTER_CLEAR_MAIN_STATE:02X}"),
        selected_outer_screen_state: CHAPTER_CLEAR_OUTER_SCREEN_STATE,
        selected_outer_screen_state_hex: format!("0x{CHAPTER_CLEAR_OUTER_SCREEN_STATE:02X}"),
        selected_stage: CHAPTER_CLEAR_DIALOGUE_STAGE,
        dialogue_entry_address: 0x77F1,
        dialogue_entry_address_hex: "0x77F1",
        dialogue_selector_address: 0x77F4,
        dialogue_selector_address_hex: "0x77F4",
    }
}
