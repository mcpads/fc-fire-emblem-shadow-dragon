use anyhow::{Context, Result, ensure};
use serde::Serialize;

mod probe_plan;

use crate::{
    dialogue_inventory::{
        TranslationSurfaceDialogueTableBinding,
        aggregate_translation_surface_dialogue_literal_inventory,
    },
    rom::Rom,
    source_literals::TranslationSurfaceLiteralInventory,
};

use super::{
    CodeLocation, SourceRegionSpec, location,
    unit_record_history::{
        INACTIVE_ACTION_VALUE, ROSTER_ACTION_OFFSET, ROSTER_BUFFER_ADDRESS,
        ROSTER_BUFFER_ADDRESS_HEX, ROSTER_IDENTITY_OFFSET, ROSTER_LOCATION_OFFSET,
        ROSTER_RECORD_CAPACITY, ROSTER_RECORD_STRIDE,
    },
};
use probe_plan::{EndingEpilogueVariantObservationPlan, ending_epilogue_variant_observation_plan};

const INITIAL_CURSOR: u8 = 0x35;
const FIRST_CANDIDATE_ID: u8 = 0x35;
const LAST_CANDIDATE_ID: u8 = 0x01;
const CANDIDATE_COUNT: usize = 53;

pub(super) const SOURCE_REGIONS: &[SourceRegionSpec] = &[
    SourceRegionSpec::code_sha1(
        "initialize_ending_character_epilogue",
        0x04,
        0xA123,
        0x3D,
        "4b9e66d5b74ec0082cdd62d93616b1d1e4083088",
    ),
    SourceRegionSpec::code_sha1(
        "select_ending_character_epilogue",
        0x04,
        0xA165,
        0x52,
        "f45d86c0252e1a4b9194407be8bf1a8e23d40f07",
    ),
    SourceRegionSpec::code_sha1(
        "copy_ending_character_location_name",
        0x04,
        0xA1B7,
        0x27,
        "1adc381f1f3db32e2fbdb3c4a643df3842df9ea0",
    ),
    SourceRegionSpec::code_sha1(
        "classify_ending_character_roster_record",
        0x04,
        0xA1DE,
        0x31,
        "28b8e662e09eb380a9b9f66a81f6508b04c4ff74",
    ),
    SourceRegionSpec::code_sha1(
        "wait_ending_character_epilogue",
        0x04,
        0xA233,
        0x1F,
        "d41db20b99824edaff5fbc6ac30157394a6a2648",
    ),
    SourceRegionSpec::code_sha1(
        "compose_ending_character_name",
        0x04,
        0xA366,
        0x1E,
        "8d1714400a97d103cc7f03015e0ae95d5ab8ae77",
    ),
];

#[derive(Debug, Serialize)]
pub(super) struct EndingCharacterEpilogueTranslationSurface {
    screen_role: &'static str,
    selector_phase: u8,
    selector_phase_hex: &'static str,
    visible_dialogue_phase: u8,
    visible_dialogue_phase_hex: &'static str,
    table_selector_address: u16,
    table_selector_address_hex: &'static str,
    entry_selector_address: u16,
    entry_selector_address_hex: &'static str,
    direct_dialogue_table_id: &'static str,
    routing_dialogue_table_id: &'static str,
    direct_selector: u8,
    direct_selector_hex: &'static str,
    routing_selector: u8,
    routing_selector_hex: &'static str,
    selector_flow: EndingEpilogueSelectorFlow,
    variant_observation_plan: EndingEpilogueVariantObservationPlan,
    dialogue_literal_inventory: TranslationSurfaceLiteralInventory,
    dialogue_literal_inventory_scope: &'static str,
    selector_writer: CodeLocation,
    dialogue_wait_handler: CodeLocation,
    input_behavior: &'static str,
    translation_handling: &'static str,
    unresolved: &'static [&'static str],
}

#[derive(Debug, Serialize)]
struct EndingEpilogueSelectorFlow {
    initialization_phase: u8,
    initialization_phase_hex: &'static str,
    candidate_cursor_address: u16,
    candidate_cursor_address_hex: &'static str,
    initial_cursor: u8,
    initial_cursor_hex: &'static str,
    first_candidate_id: u8,
    first_candidate_id_hex: &'static str,
    last_candidate_id: u8,
    last_candidate_id_hex: &'static str,
    candidate_count: usize,
    scan_order: &'static str,
    roster_buffer_address: u16,
    roster_buffer_address_hex: &'static str,
    roster_record_capacity: usize,
    roster_record_stride: u8,
    roster_record_stride_hex: &'static str,
    identity_offset: u8,
    identity_offset_hex: &'static str,
    action_byte_offset: u8,
    action_byte_offset_hex: &'static str,
    inactive_action_value: u8,
    inactive_action_value_hex: &'static str,
    classification_result_address: u16,
    classification_result_address_hex: &'static str,
    classification_cases: &'static [EpilogueClassificationCase],
    entry_index_relation: &'static str,
    direct_candidate_entry_range: &'static str,
    routing_candidate_entry_range: &'static str,
    routing_script_entry_range: &'static str,
    routing_no_op_entries: &'static [u8],
    routing_transition_target: &'static str,
    direct_extension_entry_range: &'static str,
    character_name_pointer_table: CodeLocation,
    character_name_destination: u16,
    character_name_destination_hex: &'static str,
    alternate_location_index_offset: u8,
    alternate_location_index_offset_hex: &'static str,
    location_name_pointer_table: CodeLocation,
    location_name_destination: u16,
    location_name_destination_hex: &'static str,
    inactive_location_producer: CodeLocation,
    completion_condition: &'static str,
    semantic_boundary: &'static str,
}

#[derive(Debug, Serialize)]
struct EpilogueClassificationCase {
    result: u8,
    result_hex: &'static str,
    source_condition: &'static str,
    action: &'static str,
    visible_entry: bool,
}

const CLASSIFICATION_CASES: &[EpilogueClassificationCase] = &[
    EpilogueClassificationCase {
        result: 0x02,
        result_hex: "0x02",
        source_condition: "no roster record has identity byte equal to the candidate id",
        action: "decrement the candidate cursor again without opening a dialogue entry",
        visible_entry: false,
    },
    EpilogueClassificationCase {
        result: 0x01,
        result_hex: "0x01",
        source_condition: "matching roster record has inactive or defeated action byte 0xFF at offset 0x12",
        action: "select routing table 0x41 and copy the record's location-name field before opening the candidate-numbered entry",
        visible_entry: true,
    },
    EpilogueClassificationCase {
        result: 0x00,
        result_hex: "0x00",
        source_condition: "matching roster record action byte at offset 0x12 is not 0xFF",
        action: "keep direct table 0x40 and open the candidate-numbered entry",
        visible_entry: true,
    },
];

pub(super) fn bind_ending_character_epilogue(
    rom: &Rom,
    dialogue_tables: &[TranslationSurfaceDialogueTableBinding],
) -> Result<EndingCharacterEpilogueTranslationSurface> {
    let direct_epilogue = dialogue_tables
        .iter()
        .find(|table| table.table_id == "epilogue-dialogue")
        .context("epilogue-dialogue surface binding is absent")?;
    ensure!(
        direct_epilogue.directory_selector == Some(0x40)
            && direct_epilogue.pointer_count == 66
            && direct_epilogue.proven_record_count == Some(66),
        "direct epilogue-dialogue surface structure changed"
    );
    let routing_epilogue = dialogue_tables
        .iter()
        .find(|table| table.table_id == "epilogue-routing-dialogue")
        .context("epilogue-routing surface binding is absent")?;
    ensure!(
        routing_epilogue.directory_selector == Some(0x41)
            && routing_epilogue.pointer_count == 54
            && routing_epilogue.unique_target_count == 53
            && routing_epilogue.proven_record_count == Some(52),
        "routing epilogue-dialogue surface structure changed"
    );

    let dialogue_literal_inventory = aggregate_translation_surface_dialogue_literal_inventory(
        rom.data(),
        dialogue_tables,
        &["epilogue-dialogue", "epilogue-routing-dialogue"],
    )?;

    Ok(EndingCharacterEpilogueTranslationSurface {
        screen_role: "ending_character_epilogue",
        selector_phase: 0x0F,
        selector_phase_hex: "0x0F",
        visible_dialogue_phase: 0x10,
        visible_dialogue_phase_hex: "0x10",
        table_selector_address: 0x77F4,
        table_selector_address_hex: "0x77F4",
        entry_selector_address: 0x77F1,
        entry_selector_address_hex: "0x77F1",
        direct_dialogue_table_id: "epilogue-dialogue",
        routing_dialogue_table_id: "epilogue-routing-dialogue",
        direct_selector: 0x40,
        direct_selector_hex: "0x40",
        routing_selector: 0x41,
        routing_selector_hex: "0x41",
        selector_flow: EndingEpilogueSelectorFlow {
            initialization_phase: 0x0E,
            initialization_phase_hex: "0x0E",
            candidate_cursor_address: 0x773B,
            candidate_cursor_address_hex: "0x773B",
            initial_cursor: INITIAL_CURSOR,
            initial_cursor_hex: "0x35",
            first_candidate_id: FIRST_CANDIDATE_ID,
            first_candidate_id_hex: "0x35",
            last_candidate_id: LAST_CANDIDATE_ID,
            last_candidate_id_hex: "0x01",
            candidate_count: CANDIDATE_COUNT,
            scan_order: "phase 0x0F decrements 0x773B before classification, so candidate ids run from 0x35 down through 0x01",
            roster_buffer_address: ROSTER_BUFFER_ADDRESS,
            roster_buffer_address_hex: ROSTER_BUFFER_ADDRESS_HEX,
            roster_record_capacity: ROSTER_RECORD_CAPACITY,
            roster_record_stride: ROSTER_RECORD_STRIDE,
            roster_record_stride_hex: "0x1B",
            identity_offset: ROSTER_IDENTITY_OFFSET,
            identity_offset_hex: "0x00",
            action_byte_offset: ROSTER_ACTION_OFFSET,
            action_byte_offset_hex: "0x12",
            inactive_action_value: INACTIVE_ACTION_VALUE,
            inactive_action_value_hex: "0xFF",
            classification_result_address: 0x0004,
            classification_result_address_hex: "0x0004",
            classification_cases: CLASSIFICATION_CASES,
            entry_index_relation: "the selected dialogue entry index equals the classified candidate id",
            direct_candidate_entry_range: "0x01..0x35",
            routing_candidate_entry_range: "0x01..0x35",
            routing_script_entry_range: "0x02..0x35",
            routing_no_op_entries: &[0x00, 0x01],
            routing_transition_target: "all 52 routing scripts transition to direct table 0x40 entry 0x00",
            direct_extension_entry_range: "0x36..0x41; reached only through dialogue control flow, not by the outer candidate selector",
            character_name_pointer_table: location(0x0F, 0xDE2B),
            character_name_destination: 0x78F2,
            character_name_destination_hex: "0x78F2",
            alternate_location_index_offset: ROSTER_LOCATION_OFFSET,
            alternate_location_index_offset_hex: "0x0E",
            location_name_pointer_table: location(0x0F, 0xEFB7),
            location_name_destination: 0x7902,
            location_name_destination_hex: "0x7902",
            inactive_location_producer: location(0x06, 0xB67B),
            completion_condition: "after candidate 0x01, the next decrement makes 0x773B negative and phase 0x0F jumps to ending phase 0x17",
            semantic_boundary: "the producer and gameplay evidence prove absent, inactive-or-defeated, and active branches; exact causes inside the inactive class, including whether every record means character death, still require runtime variant evidence",
        },
        variant_observation_plan: ending_epilogue_variant_observation_plan(),
        dialogue_literal_inventory,
        dialogue_literal_inventory_scope: "all canonical first linear segments in selector tables 0x40 and 0x41; every routing-table transition targets the included direct epilogue table",
        selector_writer: location(0x04, 0xA17E),
        dialogue_wait_handler: location(0x04, 0xA233),
        input_behavior: "automatic; phase 0x0F scans and selects an entry, and phase 0x10 waits for the shared dialogue engine before advancing",
        translation_handling: "translate Japanese character names, location names, and epilogue lines only; preserve original Latin and digit codes",
        unresolved: &[
            "observe inactive or defeated character variants before narrowing the 0xFF branch to a specific outcome such as death",
            "complete portrait and CHR-page union across candidate and dialogue-control variants",
            "runtime coverage of direct, routing, and direct-extension epilogue entries",
        ],
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn candidate_scan_covers_each_nonzero_identity_once() {
        assert_eq!(usize::from(FIRST_CANDIDATE_ID), CANDIDATE_COUNT);
        assert_eq!(LAST_CANDIDATE_ID, 1);
        assert_eq!(INITIAL_CURSOR, FIRST_CANDIDATE_ID);
    }

    #[test]
    fn selector_cases_keep_skip_and_visible_variants_distinct() {
        assert_eq!(CLASSIFICATION_CASES.len(), 3);
        assert!(!CLASSIFICATION_CASES[0].visible_entry);
        assert!(CLASSIFICATION_CASES[1].visible_entry);
        assert!(CLASSIFICATION_CASES[2].visible_entry);
        assert_eq!(CLASSIFICATION_CASES[0].result, 2);
        assert_eq!(CLASSIFICATION_CASES[1].result, 1);
        assert_eq!(CLASSIFICATION_CASES[2].result, 0);
    }
}
