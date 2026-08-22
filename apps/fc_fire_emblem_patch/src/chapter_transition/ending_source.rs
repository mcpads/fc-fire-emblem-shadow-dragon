use std::collections::BTreeSet;

use anyhow::{Result, ensure};

use crate::rom::Rom;

use super::{SourceRegionSpec, bind_source_region};

const ENDING_SEQUENCE_HANDLER_POINTER_BYTES: &[u8] = &[0xC6, 0x9E];
const RUN_ENDING_SEQUENCE_BYTES: &[u8] =
    &[0x20, 0x85, 0x9E, 0x20, 0xD0, 0x9E, 0x20, 0x15, 0x9F, 0x60];
const INITIALIZE_ENDING_SEQUENCE_BYTES: &[u8] = &[
    0xAD, 0x30, 0x77, 0xD0, 0x3B, 0x20, 0xCE, 0xC9, 0x20, 0x1F, 0xC7, 0x20, 0x3D, 0xC2, 0x20, 0x4E,
    0xC2, 0x20, 0x0D, 0xC7, 0xA9, 0xAC, 0x85, 0x01, 0xA9, 0xC8, 0x85, 0x00, 0x20, 0xE7, 0xC3, 0x20,
    0x2D, 0xC7, 0xA9, 0x00, 0x20, 0xBE, 0xC9, 0x20, 0xC6, 0xC9, 0xA2, 0x01, 0x8E, 0x30, 0x77, 0xCA,
    0x86, 0xCB, 0x86, 0xCA, 0x8E, 0x31, 0x77, 0x8E, 0x32, 0x77, 0xA5, 0xCD, 0x29, 0xFC, 0x85, 0xCD,
    0x60,
];
const UPDATE_ENDING_SEQUENCE_TEMPORAL_STATE_BYTES: &[u8] = &[
    0xA5, 0x30, 0x29, 0x07, 0xD0, 0x2C, 0xAD, 0x32, 0x77, 0xF0, 0x39, 0xE6, 0xCA, 0xA5, 0xCA, 0xC9,
    0xF0, 0xD0, 0x0A, 0xA5, 0xCD, 0x49, 0x02, 0x85, 0xCD, 0xA9, 0x00, 0x85, 0xCA, 0xA5, 0xCA, 0x4A,
    0xB0, 0x10, 0x4A, 0xB0, 0x0D, 0x4A, 0xB0, 0x0A, 0x4A, 0xB0, 0x07, 0xA9, 0x01, 0x8D, 0x33, 0x77,
    0xD0, 0x12, 0xAD, 0x33, 0x77, 0xC9, 0x01, 0xD0, 0x06, 0xEE, 0x33, 0x77, 0x4C, 0x14, 0x9F, 0xA9,
    0x00, 0x8D, 0x33, 0x77, 0x60,
];
const DISPATCH_ENDING_SEQUENCE_PHASE_BYTES: &[u8] = &[0xAD, 0x31, 0x77, 0x20, 0x4C, 0xC3];
pub(crate) const ENDING_SEQUENCE_INNER_STATE_ADDRESS: u16 = 0x7733;
pub(super) const ENDING_SEQUENCE_PHASE_POINTERS_BYTES: &[u8] = &[
    0xA5, 0xA3, 0xE0, 0xA3, 0xED, 0x9F, 0x54, 0xA0, 0xE9, 0xA0, 0xFA, 0x9F, 0x11, 0xA0, 0x2D, 0xA0,
    0x54, 0xA0, 0x71, 0xA0, 0x64, 0x9F, 0x83, 0x9F, 0x54, 0xA0, 0x57, 0x9F, 0x23, 0xA1, 0x65, 0xA1,
    0x33, 0xA2, 0x52, 0xA2, 0x5D, 0xA2, 0x69, 0xA2, 0x7E, 0xA2, 0x94, 0xA2, 0x84, 0xA3, 0xCA, 0x9F,
    0x2D, 0xA0, 0x54, 0xA0, 0xD3, 0xA0, 0x08, 0xA5, 0x35, 0xA5, 0x3D, 0xC7,
];

pub(super) const SOURCE_REGIONS: &[SourceRegionSpec] = &[
    SourceRegionSpec::data(
        "ending_sequence_handler_pointer",
        0x04,
        0xBFA8,
        ENDING_SEQUENCE_HANDLER_POINTER_BYTES,
    ),
    SourceRegionSpec::code(
        "run_ending_sequence",
        0x04,
        0x9EC6,
        RUN_ENDING_SEQUENCE_BYTES,
    ),
    SourceRegionSpec::code(
        "initialize_ending_sequence",
        0x04,
        0x9E85,
        INITIALIZE_ENDING_SEQUENCE_BYTES,
    ),
    SourceRegionSpec::code(
        "update_ending_sequence_temporal_state",
        0x04,
        0x9ED0,
        UPDATE_ENDING_SEQUENCE_TEMPORAL_STATE_BYTES,
    ),
    SourceRegionSpec::code(
        "dispatch_ending_sequence_phase",
        0x04,
        0x9F15,
        DISPATCH_ENDING_SEQUENCE_PHASE_BYTES,
    ),
    SourceRegionSpec::data(
        "ending_sequence_phase_pointers",
        0x04,
        0x9F1B,
        ENDING_SEQUENCE_PHASE_POINTERS_BYTES,
    ),
    SourceRegionSpec::code_sha1(
        "initialize_ending_scroll_stream",
        0x04,
        0xA3A5,
        0x3B,
        "ee45386f98c7ee65cab296e900b5dad9acb4bf0f",
    ),
    SourceRegionSpec::code_sha1(
        "dispatch_ending_scroll_inner_state",
        0x04,
        0xA3E0,
        0x06,
        "af87f3b42ff75fe77d4f423410e1776a08f2644c",
    ),
    SourceRegionSpec::data_sha1(
        "ending_scroll_inner_state_pointers",
        0x04,
        0xA3E6,
        0x06,
        "edcaf5913c7895f15e684f65577e95f8f92439e0",
    ),
    SourceRegionSpec::code_sha1(
        "update_ending_scroll_position",
        0x04,
        0xA3EC,
        0x54,
        "ebdc5cf629d1d977fa5d930eb59bfd5aeea6e88c",
    ),
    SourceRegionSpec::code_sha1(
        "write_ending_scroll_record",
        0x04,
        0xA440,
        0x53,
        "7b295e6926b702f2755420e0cf767c705277fdd5",
    ),
    SourceRegionSpec::code_sha1(
        "expand_ending_scroll_turn_value",
        0x04,
        0xA4A6,
        0x62,
        "844d7ea01828e3fbdb516c34a3056ae3b9b535b9",
    ),
    SourceRegionSpec::data_sha1(
        "ending_scroll_records",
        0x04,
        0xA826,
        0x4A2,
        "137f18180b51a86fac7a1f0c6eb9fa4269ec2504",
    ),
];

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct EndingSequencePhaseDispatchSource {
    prg_bank: u8,
    dispatch_call: u16,
    selector_address: u16,
    admitted_selectors: BTreeSet<u8>,
    inner_dispatch_call: u16,
    inner_selector_address: u16,
    inner_produced_selectors: BTreeSet<u8>,
}

impl EndingSequencePhaseDispatchSource {
    pub(crate) fn prg_bank(&self) -> u8 {
        self.prg_bank
    }

    pub(crate) fn dispatch_call(&self) -> u16 {
        self.dispatch_call
    }

    pub(crate) fn selector_address(&self) -> u16 {
        self.selector_address
    }

    pub(crate) fn admitted_selectors(&self) -> &BTreeSet<u8> {
        &self.admitted_selectors
    }

    pub(crate) fn inner_dispatch_call(&self) -> u16 {
        self.inner_dispatch_call
    }

    pub(crate) fn inner_selector_address(&self) -> u16 {
        self.inner_selector_address
    }

    pub(crate) fn inner_produced_selectors(&self) -> &BTreeSet<u8> {
        &self.inner_produced_selectors
    }
}

pub(crate) fn bind_ending_sequence_phase_dispatch_source(
    source: &Rom,
) -> Result<EndingSequencePhaseDispatchSource> {
    for role in [
        "update_ending_sequence_temporal_state",
        "dispatch_ending_sequence_phase",
        "ending_sequence_phase_pointers",
        "dispatch_ending_scroll_inner_state",
        "ending_scroll_inner_state_pointers",
    ] {
        let spec = SOURCE_REGIONS
            .iter()
            .find(|spec| spec.role == role)
            .copied()
            .expect("ending state-machine source region is declared");
        bind_source_region(source, spec)?;
    }

    ensure!(
        ENDING_SEQUENCE_PHASE_POINTERS_BYTES.len().is_multiple_of(2),
        "ending phase pointer table has a partial pointer"
    );
    let phase_count = u8::try_from(ENDING_SEQUENCE_PHASE_POINTERS_BYTES.len() / 2)?;
    ensure!(
        phase_count == 0x1E,
        "ending phase handler domain no longer has thirty selectors"
    );
    let inner_produced_selectors = BTreeSet::from([0x00, 0x01, 0x02]);
    for producer in [
        [0xA9, 0x01, 0x8D, 0x33, 0x77].as_slice(),
        [0xEE, 0x33, 0x77].as_slice(),
        [0xA9, 0x00, 0x8D, 0x33, 0x77].as_slice(),
    ] {
        ensure!(
            UPDATE_ENDING_SEQUENCE_TEMPORAL_STATE_BYTES
                .windows(producer.len())
                .any(|window| window == producer),
            "ending inner-state producer changed"
        );
    }

    Ok(EndingSequencePhaseDispatchSource {
        prg_bank: 0x04,
        dispatch_call: 0x9F18,
        selector_address: 0x7731,
        admitted_selectors: (0..phase_count).collect(),
        inner_dispatch_call: 0xA3E3,
        inner_selector_address: ENDING_SEQUENCE_INNER_STATE_ADDRESS,
        inner_produced_selectors,
    })
}
