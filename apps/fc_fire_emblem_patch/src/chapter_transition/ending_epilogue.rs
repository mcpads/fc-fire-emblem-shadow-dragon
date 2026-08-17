use std::collections::BTreeSet;

use anyhow::{Context, Result, ensure};
use retro_rp2a03::decode_bytes;
use serde::Serialize;

mod observed_variants;
mod probe_plan;

use crate::{
    dialogue_inventory::{
        TranslationSurfaceDialogueTableBinding,
        aggregate_translation_surface_dialogue_literal_inventory,
        bind_main_dialogue_progress_source,
    },
    rom::Rom,
    source_literals::TranslationSurfaceLiteralInventory,
    typed_source::decode_rp2a03_sequence,
};

use super::{
    CodeLocation, SourceRegionSpec, bind_source_region, location, source_file_offset,
    unit_record_history::{
        INACTIVE_ACTION_VALUE, ROSTER_ACTION_OFFSET, ROSTER_BUFFER_ADDRESS,
        ROSTER_BUFFER_ADDRESS_HEX, ROSTER_IDENTITY_OFFSET, ROSTER_LOCATION_OFFSET,
        ROSTER_RECORD_CAPACITY, ROSTER_RECORD_STRIDE,
    },
};
use observed_variants::{EndingEpilogueVariantObservation, ending_epilogue_variant_observation};
use probe_plan::{EndingEpilogueVariantObservationPlan, ending_epilogue_variant_observation_plan};

const INITIAL_CURSOR: u8 = 0x35;
const FIRST_CANDIDATE_ID: u8 = 0x35;
const LAST_CANDIDATE_ID: u8 = 0x01;
const CANDIDATE_COUNT: usize = 53;
const ENDING_DIALOGUE_BANK: u8 = 0x04;
const ENDING_CALLER_HANDOFF_PHASE: u8 = 0x10;
const ENDING_CALLER_HANDOFF_HANDLER: u16 = 0xA233;
const ENDING_CALLER_HANDOFF_PREFIX: [u8; 15] = [
    0xA9, 0x00, 0x85, 0x44, 0x85, 0x14, 0x85, 0x16, 0x85, 0x18, 0xA9, 0x0A, 0x20, 0xFA, 0xC9,
];
const ENDING_DIALOGUE_COMPLETION_PHASE: u8 = 0x15;
const ENDING_DIALOGUE_COMPLETION_HANDLER: u16 = 0xA294;
const ENDING_DIALOGUE_COMPLETION_PREFIX: [u8; 9] =
    [0xA9, 0x00, 0x85, 0x44, 0xA9, 0x0A, 0x20, 0xFA, 0xC9];
const ENDING_CHARACTER_ANIMATION_DISPATCH_CALL: u16 = 0xA2A9;
const ENDING_CHARACTER_ANIMATION_DISPATCH_BYTES: [u8; 6] = [0xAD, 0x5D, 0x77, 0x20, 0x4C, 0xC3];
const ENDING_CHARACTER_ANIMATION_HANDLER_POINTER_BYTES: [u8; 8] =
    [0xB4, 0xA2, 0xC4, 0xA2, 0x12, 0xA3, 0x3D, 0xC7];
const ENDING_CHARACTER_ANIMATION_INITIAL_STATE_BYTES: [u8; 8] =
    [0xA9, 0x00, 0x8D, 0xF0, 0x77, 0x8D, 0x5D, 0x77];
const ENDING_CHARACTER_ANIMATION_INITIAL_STATE_ADDRESS: u16 = 0xA19D;
const ENDING_CHARACTER_ANIMATION_ADVANCE_ADDRESSES: [u16; 3] = [0xA2C0, 0xA30E, 0xA31F];
const ENDING_CHARACTER_ANIMATION_ADVANCE_BYTES: [u8; 3] = [0xEE, 0x5D, 0x77];

pub(crate) const ENDING_CHARACTER_ANIMATION_STATE_ADDRESS: u16 = 0x775D;

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct EndingCharacterAnimationDispatchSource {
    prg_bank: u8,
    dispatch_call: u16,
    selector_address: u16,
    produced_selectors: BTreeSet<u8>,
    producer_instruction_starts: BTreeSet<u16>,
}

impl EndingCharacterAnimationDispatchSource {
    pub(crate) fn prg_bank(&self) -> u8 {
        self.prg_bank
    }

    pub(crate) fn dispatch_call(&self) -> u16 {
        self.dispatch_call
    }

    pub(crate) fn selector_address(&self) -> u16 {
        self.selector_address
    }

    pub(crate) fn produced_selectors(&self) -> &BTreeSet<u8> {
        &self.produced_selectors
    }

    pub(crate) fn producer_instruction_starts(&self) -> &BTreeSet<u16> {
        &self.producer_instruction_starts
    }
}

pub(crate) fn bind_ending_character_animation_dispatch_source(
    rom: &Rom,
) -> Result<EndingCharacterAnimationDispatchSource> {
    rom.verify_supported_japanese()?;
    for role in [
        "select_ending_character_epilogue",
        "dispatch_ending_character_animation",
        "ending_character_animation_handler_pointers",
        "wait_for_ending_character_animation_dialogue",
        "initialize_ending_character_animation_tiles",
        "advance_ending_character_animation_tiles",
        "ending_character_animation_tile_steps",
    ] {
        let spec = SOURCE_REGIONS
            .iter()
            .find(|spec| spec.role == role)
            .copied()
            .with_context(|| {
                format!("ending character-animation source region {role} is absent")
            })?;
        bind_source_region(rom, spec)?;
    }

    let initial_offset = source_file_offset(
        ENDING_DIALOGUE_BANK,
        ENDING_CHARACTER_ANIMATION_INITIAL_STATE_ADDRESS,
    )?;
    let initial_end = initial_offset
        .checked_add(ENDING_CHARACTER_ANIMATION_INITIAL_STATE_BYTES.len())
        .context("ending character-animation initializer range overflow")?;
    let initial = rom
        .data()
        .get(initial_offset..initial_end)
        .context("ending character-animation initializer is outside the source")?;
    ensure!(
        initial == ENDING_CHARACTER_ANIMATION_INITIAL_STATE_BYTES,
        "ending character-animation state initializer changed"
    );
    decode_rp2a03_sequence(
        initial,
        ENDING_CHARACTER_ANIMATION_INITIAL_STATE_ADDRESS,
        "initialize ending character-animation state",
    )?;

    let handler_addresses = ENDING_CHARACTER_ANIMATION_HANDLER_POINTER_BYTES
        .chunks_exact(2)
        .map(|bytes| u16::from_le_bytes([bytes[0], bytes[1]]))
        .collect::<Vec<_>>();
    ensure!(
        handler_addresses == [0xA2B4, 0xA2C4, 0xA312, 0xC73D],
        "ending character-animation handler table changed"
    );
    ensure!(
        (handler_addresses[0]..handler_addresses[1])
            .contains(&ENDING_CHARACTER_ANIMATION_ADVANCE_ADDRESSES[0])
            && (handler_addresses[1]..handler_addresses[2])
                .contains(&ENDING_CHARACTER_ANIMATION_ADVANCE_ADDRESSES[1])
            && (handler_addresses[2]..0xA356)
                .contains(&ENDING_CHARACTER_ANIMATION_ADVANCE_ADDRESSES[2]),
        "ending character-animation state advances left their owning handlers"
    );

    let mut producer_instruction_starts =
        BTreeSet::from([ENDING_CHARACTER_ANIMATION_INITIAL_STATE_ADDRESS + 5]);
    let mut produced_selectors = BTreeSet::from([0x00]);
    let mut selector = 0_u8;
    for address in ENDING_CHARACTER_ANIMATION_ADVANCE_ADDRESSES {
        let offset = source_file_offset(ENDING_DIALOGUE_BANK, address)?;
        let end = offset
            .checked_add(ENDING_CHARACTER_ANIMATION_ADVANCE_BYTES.len())
            .context("ending character-animation advance range overflow")?;
        let actual = rom
            .data()
            .get(offset..end)
            .context("ending character-animation advance is outside the source")?;
        ensure!(
            actual == ENDING_CHARACTER_ANIMATION_ADVANCE_BYTES,
            "ending character-animation advance changed at {address:04X}"
        );
        decode_rp2a03_sequence(actual, address, "advance ending character-animation state")?;
        producer_instruction_starts.insert(address);
        selector = selector.wrapping_add(1);
        produced_selectors.insert(selector);
    }
    ensure!(
        produced_selectors.len() == handler_addresses.len()
            && produced_selectors.iter().copied().eq(0_u8..=3),
        "ending character-animation producer domain no longer covers its handler table exactly"
    );

    Ok(EndingCharacterAnimationDispatchSource {
        prg_bank: ENDING_DIALOGUE_BANK,
        dispatch_call: ENDING_CHARACTER_ANIMATION_DISPATCH_CALL,
        selector_address: ENDING_CHARACTER_ANIMATION_STATE_ADDRESS,
        produced_selectors,
        producer_instruction_starts,
    })
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct EndingDialogueProgressBoundary {
    phase: u8,
    prg_bank: u8,
    handler_address: u16,
    continuation_address: u16,
    progress_flag_address: u16,
    pending_value: u8,
    asserted_value: u8,
    prefix_instruction_starts: BTreeSet<u16>,
}

impl EndingDialogueProgressBoundary {
    pub(crate) fn phase(&self) -> u8 {
        self.phase
    }

    pub(crate) fn prg_bank(&self) -> u8 {
        self.prg_bank
    }

    pub(crate) fn handler_address(&self) -> u16 {
        self.handler_address
    }

    pub(crate) fn continuation_address(&self) -> u16 {
        self.continuation_address
    }

    pub(crate) fn progress_flag_address(&self) -> u16 {
        self.progress_flag_address
    }

    pub(crate) fn pending_value(&self) -> u8 {
        self.pending_value
    }

    pub(crate) fn asserted_value(&self) -> u8 {
        self.asserted_value
    }

    pub(crate) fn prefix_instruction_starts(&self) -> &BTreeSet<u16> {
        &self.prefix_instruction_starts
    }
}

pub(crate) fn bind_ending_dialogue_progress_boundaries(
    rom: &Rom,
) -> Result<[EndingDialogueProgressBoundary; 2]> {
    rom.verify_supported_japanese()?;
    for role in [
        "wait_ending_character_epilogue",
        "wait_between_ending_character_epilogues",
    ] {
        let spec = SOURCE_REGIONS
            .iter()
            .find(|spec| spec.role == role)
            .copied()
            .with_context(|| format!("ending dialogue progress source region {role} is absent"))?;
        bind_source_region(rom, spec)?;
    }
    let dialogue = bind_main_dialogue_progress_source(
        rom,
        ENDING_DIALOGUE_BANK,
        ENDING_CALLER_HANDOFF_HANDLER,
    )?;
    ensure!(
        dialogue.caller_prg_bank() == ENDING_DIALOGUE_BANK
            && dialogue.caller_handler_address() == ENDING_CALLER_HANDOFF_HANDLER
            && dialogue.caller_observer_address()
                == ENDING_CALLER_HANDOFF_HANDLER
                    + u16::try_from(ENDING_CALLER_HANDOFF_PREFIX.len())?
            && dialogue.dialogue_prg_bank() == 0x0A
            && dialogue.dialogue_dispatcher_address() == 0x8000,
        "ending caller handoff no longer targets the main-dialogue dispatcher and observer"
    );

    Ok([
        bind_ending_dialogue_progress_boundary(
            rom,
            ENDING_CALLER_HANDOFF_PHASE,
            ENDING_CALLER_HANDOFF_HANDLER,
            &ENDING_CALLER_HANDOFF_PREFIX,
            dialogue.caller_handoff_flag_address(),
            dialogue.pending_value(),
            dialogue.asserted_value(),
        )?,
        bind_ending_dialogue_progress_boundary(
            rom,
            ENDING_DIALOGUE_COMPLETION_PHASE,
            ENDING_DIALOGUE_COMPLETION_HANDLER,
            &ENDING_DIALOGUE_COMPLETION_PREFIX,
            dialogue.completion_flag_address(),
            dialogue.pending_value(),
            dialogue.asserted_value(),
        )?,
    ])
}

fn bind_ending_dialogue_progress_boundary(
    rom: &Rom,
    phase: u8,
    handler_address: u16,
    expected_prefix: &[u8],
    progress_flag_address: u16,
    pending_value: u8,
    asserted_value: u8,
) -> Result<EndingDialogueProgressBoundary> {
    let file_offset = source_file_offset(ENDING_DIALOGUE_BANK, handler_address)?;
    let end = file_offset
        .checked_add(expected_prefix.len())
        .context("ending dialogue progress prefix range overflow")?;
    let actual = rom
        .data()
        .get(file_offset..end)
        .context("ending dialogue progress prefix is outside the source")?;
    ensure!(
        actual == expected_prefix,
        "ending dialogue progress prefix changed for phase {phase:02X}"
    );
    decode_rp2a03_sequence(
        actual,
        handler_address,
        "ending dialogue asynchronous progress prefix",
    )?;
    let mut prefix_instruction_starts = BTreeSet::new();
    let mut offset = 0_usize;
    while offset < actual.len() {
        let instruction = decode_bytes(&actual[offset..])
            .context("decode ending dialogue asynchronous progress prefix")?;
        let address = handler_address
            .checked_add(u16::try_from(offset)?)
            .context("ending dialogue progress instruction address overflow")?;
        prefix_instruction_starts.insert(address);
        offset += instruction.encoded_len();
    }
    ensure!(
        offset == actual.len() && prefix_instruction_starts.contains(&handler_address),
        "ending dialogue progress prefix did not decode to its exact continuation"
    );

    Ok(EndingDialogueProgressBoundary {
        phase,
        prg_bank: ENDING_DIALOGUE_BANK,
        handler_address,
        continuation_address: handler_address + u16::try_from(actual.len())?,
        progress_flag_address,
        pending_value,
        asserted_value,
        prefix_instruction_starts,
    })
}

/// The ending phase that selects one surviving/dead character record.
pub(crate) const ENDING_CHARACTER_EPILOGUE_SELECTOR_PHASE: u8 = 0x0F;
/// The shared dialogue engine remains visible while the outer ending handler
/// waits for its terminal state.
pub(crate) const ENDING_CHARACTER_EPILOGUE_VISIBLE_PHASE_START: u8 = 0x10;
/// Once dialogue reaches terminal, the outer handler keeps the completed page
/// visible for its own timer in the following phase.
pub(crate) const ENDING_CHARACTER_EPILOGUE_VISIBLE_WAIT_PHASE: u8 = 0x11;
/// Phases 0x12 and 0x13 fade the completed page while its translated tile codes
/// remain visible. They therefore share the same font residency as 0x10/0x11.
pub(crate) const ENDING_CHARACTER_EPILOGUE_INTERPAGE_PHASE_START: u8 = 0x12;
/// Mask that recognizes exactly the four residency phases 0x10 through 0x13.
pub(crate) const ENDING_CHARACTER_EPILOGUE_FONT_RESIDENCY_PHASE_MASK: u8 = 0xFC;
/// Phase 0x14 begins preparing the next dialogue after the completed page has
/// faded away. Source CHR owns the pattern window from this boundary onward.
pub(crate) const ENDING_CHARACTER_EPILOGUE_REPEAT_PHASE: u8 = 0x16;
/// Only exhausting the candidate scan leaves the repeated character epilogue.
pub(crate) const ENDING_CHARACTER_EPILOGUE_COMPLETION_PHASE: u8 = 0x17;

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
        0x2A,
        "7f740b30d6e8411170e6496447149b42b7478e61",
    ),
    SourceRegionSpec::code_sha1(
        "transition_between_ending_character_epilogues",
        0x04,
        0xA25D,
        0x37,
        "18fc0c4dc3bc84e97c7b25c5d2d6804be27f1393",
    ),
    SourceRegionSpec::code_sha1(
        "wait_between_ending_character_epilogues",
        0x04,
        0xA294,
        0x12,
        "5f0206a48e3275d8dc2e1b24cd4901260fc7307f",
    ),
    SourceRegionSpec::code(
        "dispatch_ending_character_animation",
        0x04,
        0xA2A6,
        &ENDING_CHARACTER_ANIMATION_DISPATCH_BYTES,
    ),
    SourceRegionSpec::data(
        "ending_character_animation_handler_pointers",
        0x04,
        0xA2AC,
        &ENDING_CHARACTER_ANIMATION_HANDLER_POINTER_BYTES,
    ),
    SourceRegionSpec::code_sha1(
        "wait_for_ending_character_animation_dialogue",
        0x04,
        0xA2B4,
        0x10,
        "9e4134168ee6d82023b5219066e1f4328a2a9f3e",
    ),
    SourceRegionSpec::code_sha1(
        "initialize_ending_character_animation_tiles",
        0x04,
        0xA2C4,
        0x4E,
        "7a29f9545a26ba1a471d3acf02858d831f495f4a",
    ),
    SourceRegionSpec::code_sha1(
        "advance_ending_character_animation_tiles",
        0x04,
        0xA312,
        0x44,
        "7d2e7cad13a9b47349ad24e45a6da6e4452f6b4f",
    ),
    SourceRegionSpec::data_sha1(
        "ending_character_animation_tile_steps",
        0x04,
        0xA356,
        0x10,
        "3bc266b62af183c627a82397695f090e8f7eb39f",
    ),
    SourceRegionSpec::code_sha1(
        "repeat_ending_character_epilogue",
        0x04,
        0xA384,
        0x06,
        "34db719e957adfd0be88850ac9d919b3b0f78dc0",
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
    visible_wait_phase: u8,
    visible_wait_phase_hex: &'static str,
    interpage_phase_start: u8,
    interpage_phase_start_hex: &'static str,
    repeat_phase: u8,
    repeat_phase_hex: &'static str,
    completion_phase: u8,
    completion_phase_hex: &'static str,
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
    variant_observation: EndingEpilogueVariantObservation,
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
        selector_phase: ENDING_CHARACTER_EPILOGUE_SELECTOR_PHASE,
        selector_phase_hex: "0x0F",
        visible_dialogue_phase: ENDING_CHARACTER_EPILOGUE_VISIBLE_PHASE_START,
        visible_dialogue_phase_hex: "0x10",
        visible_wait_phase: ENDING_CHARACTER_EPILOGUE_VISIBLE_WAIT_PHASE,
        visible_wait_phase_hex: "0x11",
        interpage_phase_start: ENDING_CHARACTER_EPILOGUE_INTERPAGE_PHASE_START,
        interpage_phase_start_hex: "0x12",
        repeat_phase: ENDING_CHARACTER_EPILOGUE_REPEAT_PHASE,
        repeat_phase_hex: "0x16",
        completion_phase: ENDING_CHARACTER_EPILOGUE_COMPLETION_PHASE,
        completion_phase_hex: "0x17",
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
            semantic_boundary: "producer, natural execution, and controlled branch evidence distinguish absent, inactive-or-defeated, and active branches; the rendering union is closed, but exact causes inside the inactive class cannot all be labeled as death",
        },
        variant_observation_plan: ending_epilogue_variant_observation_plan(),
        variant_observation: ending_epilogue_variant_observation(),
        dialogue_literal_inventory,
        dialogue_literal_inventory_scope: "all canonical first linear segments in selector tables 0x40 and 0x41; every routing-table transition targets the included direct epilogue table",
        selector_writer: location(0x04, 0xA17E),
        dialogue_wait_handler: location(0x04, 0xA233),
        input_behavior: "automatic; phase 0x0F selects an entry, phases 0x10..0x11 display the completed translated page, phases 0x12..0x13 fade that same page, phase 0x14 releases it before preparing the next dialogue, phase 0x16 loops to 0x0F, and exhausted selection advances to 0x17",
        translation_handling: "translate Japanese character names, location names, and epilogue lines only; preserve original Latin and digit codes",
        unresolved: &[
            "separate causes inside inactive-or-defeated action 0xFF remain unresolved; synthetic branch coverage cannot label every cause as death",
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
