use std::collections::{BTreeMap, BTreeSet};

use anyhow::{Context, Result, ensure};

use crate::{
    dialogue_runtime_state::MAIN_DIALOGUE_RUNTIME_STATE,
    mapper165::{
        battle_codebook_plan::IndirectWriteDestinationBounds,
        inline_pointer_dispatch::bind_inline_pointer_dispatch,
    },
    rom::Rom,
    typed_source::decode_rp2a03_sequence,
};

use super::super::{
    chapter_map_loader::BoundChapterMapDimensions,
    unit_record_writers::BoundUnitRecordAddressDomain,
};
use super::state_transition_evidence::{
    StateWriteStep, StateWriterSource, TransitionPath, bind_state_transition_closure,
};

mod map_preparation;

use map_preparation::bind_map_preparation_dispatches;

const SOURCE_PRG_BANK_BYTE_COUNT: usize = 16 * 1024;
const OUTER_SCREEN_BANK: u8 = 0x06;
const MAP_DIALOGUE_STATE_ADDRESS: u16 =
    MAIN_DIALOGUE_RUNTIME_STATE.map_dialogue_outer_state_address;

const MAIN_STATE_TWO_TARGETS: [u16; 3] = [0x8A76, 0x8A8A, 0x8A92];
const MAIN_STATE_FORTY_TWO_TARGETS: [u16; 4] = [0xA733, 0xA725, 0x9F1C, 0xA746];
const MAIN_STATE_THIRTY_EIGHT_TARGETS: [u16; 3] = [0xB358, 0xB360, 0xB3BF];
const MAIN_STATE_THIRTY_SIX_TARGETS: [u16; 3] = [0xB406, 0xB40E, 0xB416];
const FOUR_WAY_CONTROLLER_TARGETS: [u16; 4] = [0xBC1A, 0xBC4D, 0xBC81, 0xBCB4];

const LOAD_MAP_DIALOGUE_STATE_AND_DISPATCH: [u8; 6] = [0xAD, 0xDB, 0x05, 0x20, 0x4C, 0xC3];
const FOUR_WAY_CONTROLLER_SELECTOR_START: u16 = 0xBB78;
const FOUR_WAY_CONTROLLER_DISPATCH_CALL: u16 = 0xBB96;
const FOUR_WAY_CONTROLLER_TABLE_END: u16 = 0xBBA1;
const FOUR_WAY_CONTROLLER_SELECTOR: [u8; 33] = [
    0xA5, 0x18, 0x29, 0x0F, 0xD0, 0x09, 0xAE, 0x0E, 0x05, 0xF0, 0x1F, 0xCE, 0x0E, 0x05, 0x60, 0xA2,
    0x0A, 0x8E, 0x0E, 0x05, 0xA2, 0x04, 0x4A, 0xB0, 0x03, 0xCA, 0xD0, 0xFA, 0xCA, 0x8A, 0x20, 0x4C,
    0xC3,
];

#[derive(Clone, Copy)]
struct StatefulDispatchSpec {
    call_address: u16,
    table_end: u16,
    targets: &'static [u16],
    role: &'static str,
    transitions: Option<&'static [TransitionPath]>,
}

const INC_8A82: StateWriteStep = StateWriteStep::increment(0x8A82);
const INC_8A8A: StateWriteStep = StateWriteStep::increment(0x8A8A);
const SET_00_8ACF: StateWriteStep = StateWriteStep::store_index_y_constant(0x8ACA, 0x8ACF, 0x00);
const MAIN_STATE_TWO_TRANSITIONS: &[TransitionPath] = &[
    TransitionPath::new(0, 0x8A76, &[INC_8A82]),
    TransitionPath::new(1, 0x8A8A, &[INC_8A8A]),
    TransitionPath::new(2, 0x8A92, &[SET_00_8ACF]),
];

const INC_A72F: StateWriteStep = StateWriteStep::increment(0xA72F);
const INC_A733: StateWriteStep = StateWriteStep::increment(0xA733);
const INC_9F95: StateWriteStep = StateWriteStep::increment(0x9F95);
const DEC_A784: StateWriteStep = StateWriteStep::decrement(0xA784);
const MAIN_STATE_FORTY_TWO_TRANSITIONS: &[TransitionPath] = &[
    TransitionPath::new(0, 0xA733, &[INC_A733]),
    TransitionPath::new(1, 0xA725, &[INC_A72F]),
    TransitionPath::new(2, 0x9F1C, &[INC_9F95]),
    TransitionPath::new(3, 0xA746, &[DEC_A784]),
];

const INC_B358: StateWriteStep = StateWriteStep::increment(0xB358);
const SET_00_B37F: StateWriteStep =
    StateWriteStep::store_accumulator_constant(0xB37B, 0xB37F, 0x00);
const INC_B383: StateWriteStep = StateWriteStep::increment(0xB383);
const INC_B397: StateWriteStep = StateWriteStep::increment(0xB397);
const INC_B3AB: StateWriteStep = StateWriteStep::increment(0xB3AB);
const SET_00_B3F1: StateWriteStep =
    StateWriteStep::store_accumulator_constant(0xB3ED, 0xB3F1, 0x00);
const MAIN_STATE_THIRTY_EIGHT_TRANSITIONS: &[TransitionPath] = &[
    TransitionPath::new(0, 0xB358, &[INC_B358]),
    TransitionPath::new(1, 0xB360, &[SET_00_B37F]),
    TransitionPath::new(1, 0xB360, &[INC_B383]),
    TransitionPath::new(1, 0xB360, &[INC_B397]),
    TransitionPath::new(1, 0xB360, &[INC_B3AB]),
    TransitionPath::new(2, 0xB3BF, &[SET_00_B3F1]),
];

const INC_B406: StateWriteStep = StateWriteStep::increment(0xB406);
const INC_B40E: StateWriteStep = StateWriteStep::increment(0xB40E);
const SET_00_B421: StateWriteStep =
    StateWriteStep::store_accumulator_constant(0xB41D, 0xB421, 0x00);
const MAIN_STATE_THIRTY_SIX_TRANSITIONS: &[TransitionPath] = &[
    TransitionPath::new(0, 0xB406, &[INC_B406]),
    TransitionPath::new(1, 0xB40E, &[INC_B40E]),
    TransitionPath::new(2, 0xB416, &[SET_00_B421]),
];

const STATEFUL_DISPATCHES: [StatefulDispatchSpec; 4] = [
    StatefulDispatchSpec {
        call_address: 0x8A6D,
        table_end: 0x8A76,
        targets: &MAIN_STATE_TWO_TARGETS,
        role: "main-state-two screen-substate dispatch",
        transitions: Some(MAIN_STATE_TWO_TRANSITIONS),
    },
    StatefulDispatchSpec {
        call_address: 0xA71A,
        table_end: 0xA725,
        targets: &MAIN_STATE_FORTY_TWO_TARGETS,
        role: "main-state-forty-two screen-substate dispatch",
        transitions: Some(MAIN_STATE_FORTY_TWO_TRANSITIONS),
    },
    StatefulDispatchSpec {
        call_address: 0xB34F,
        table_end: 0xB358,
        targets: &MAIN_STATE_THIRTY_EIGHT_TARGETS,
        role: "main-state-thirty-eight screen-substate dispatch",
        transitions: Some(MAIN_STATE_THIRTY_EIGHT_TRANSITIONS),
    },
    StatefulDispatchSpec {
        call_address: 0xB3FD,
        table_end: 0xB406,
        targets: &MAIN_STATE_THIRTY_SIX_TARGETS,
        role: "main-state-thirty-six screen-substate dispatch",
        transitions: Some(MAIN_STATE_THIRTY_SIX_TRANSITIONS),
    },
];

pub(super) struct ScreenSubstateDispatch {
    prg_bank: u8,
    call_address: u16,
    handler_domain: BTreeSet<u8>,
    selector_memory_address: Option<u16>,
    source_bound_produced_selectors: Option<BTreeSet<u8>>,
    indirect_write_destinations: BTreeMap<(u8, u16, u8), IndirectWriteDestinationBounds>,
    role: &'static str,
}

impl ScreenSubstateDispatch {
    pub(super) fn prg_bank(&self) -> u8 {
        self.prg_bank
    }

    pub(super) fn call_address(&self) -> u16 {
        self.call_address
    }

    pub(super) fn handler_domain(&self) -> &BTreeSet<u8> {
        &self.handler_domain
    }

    pub(super) fn selector_memory_address(&self) -> Option<u16> {
        self.selector_memory_address
    }

    pub(super) fn source_bound_produced_selectors(&self) -> Option<&BTreeSet<u8>> {
        self.source_bound_produced_selectors.as_ref()
    }

    pub(super) fn indirect_write_destinations(
        &self,
    ) -> &BTreeMap<(u8, u16, u8), IndirectWriteDestinationBounds> {
        &self.indirect_write_destinations
    }

    pub(super) fn role(&self) -> &'static str {
        self.role
    }
}

pub(super) fn bind_screen_substate_dispatches(
    source: &Rom,
    unit_record_domain: &BoundUnitRecordAddressDomain,
    chapter_map_dimensions: &BoundChapterMapDimensions,
) -> Result<Vec<ScreenSubstateDispatch>> {
    source.verify_supported_japanese()?;
    let mut dispatches =
        bind_map_preparation_dispatches(source, unit_record_domain, chapter_map_dimensions)?;

    for spec in STATEFUL_DISPATCHES {
        let prefix_start = spec
            .call_address
            .checked_sub(3)
            .context("screen-substate dispatch prefix underflow")?;
        bind_exact_code(
            source,
            prefix_start,
            &LOAD_MAP_DIALOGUE_STATE_AND_DISPATCH,
            spec.role,
        )?;
        let handler_domain = (0..u8::try_from(spec.targets.len())?).collect::<BTreeSet<_>>();
        let binding = bind_inline_pointer_dispatch(
            source,
            OUTER_SCREEN_BANK,
            spec.call_address,
            handler_domain.iter().copied(),
            spec.role,
        )?;
        ensure!(
            binding.targets_in_selector_order() == spec.targets,
            "{} handlers changed",
            spec.role
        );
        let table_end = binding
            .table_start()
            .checked_add(u16::try_from(spec.targets.len() * 2)?)
            .context("screen-substate pointer-table end overflow")?;
        ensure!(
            table_end == spec.table_end && spec.targets.contains(&table_end),
            "{} pointer table no longer ends at its first handler boundary",
            spec.role
        );
        let source_bound_produced_selectors = spec
            .transitions
            .map(|transitions| {
                bind_state_transition_closure(
                    StateWriterSource {
                        source,
                        bank: OUTER_SCREEN_BANK,
                        state_address: MAP_DIALOGUE_STATE_ADDRESS,
                    },
                    &handler_domain,
                    |selector| spec.targets.get(usize::from(selector)).copied(),
                    [0],
                    transitions,
                    spec.role,
                )
            })
            .transpose()?;
        ensure!(
            source_bound_produced_selectors
                .as_ref()
                .is_none_or(|produced| produced == &handler_domain),
            "{} producer closure left its handler table",
            spec.role
        );
        dispatches.push(ScreenSubstateDispatch {
            prg_bank: OUTER_SCREEN_BANK,
            call_address: spec.call_address,
            handler_domain,
            selector_memory_address: Some(MAP_DIALOGUE_STATE_ADDRESS),
            source_bound_produced_selectors,
            indirect_write_destinations: BTreeMap::new(),
            role: spec.role,
        });
    }

    bind_exact_code(
        source,
        FOUR_WAY_CONTROLLER_SELECTOR_START,
        &FOUR_WAY_CONTROLLER_SELECTOR,
        "four-way controller-bit selector",
    )?;
    let produced_selectors = (0..=u8::MAX)
        .filter_map(four_way_controller_selector)
        .collect::<BTreeSet<_>>();
    let binding = bind_inline_pointer_dispatch(
        source,
        OUTER_SCREEN_BANK,
        FOUR_WAY_CONTROLLER_DISPATCH_CALL,
        produced_selectors.iter().copied(),
        "four-way controller-bit dispatch",
    )?;
    ensure!(
        binding.targets_in_selector_order() == FOUR_WAY_CONTROLLER_TARGETS,
        "four-way controller-bit handlers changed"
    );
    ensure!(
        binding
            .table_start()
            .checked_add(u16::try_from(FOUR_WAY_CONTROLLER_TARGETS.len() * 2)?)
            == Some(FOUR_WAY_CONTROLLER_TABLE_END),
        "four-way controller-bit table boundary changed"
    );
    dispatches.push(ScreenSubstateDispatch {
        prg_bank: OUTER_SCREEN_BANK,
        call_address: FOUR_WAY_CONTROLLER_DISPATCH_CALL,
        handler_domain: produced_selectors.clone(),
        selector_memory_address: None,
        source_bound_produced_selectors: Some(produced_selectors),
        indirect_write_destinations: BTreeMap::new(),
        role: "four-way controller-bit dispatch",
    });

    Ok(dispatches)
}

fn four_way_controller_selector(input: u8) -> Option<u8> {
    let mut shifted = input & 0x0F;
    if shifted == 0 {
        return None;
    }
    let mut selector = 4_u8;
    loop {
        let carry = shifted & 1 != 0;
        shifted >>= 1;
        if carry {
            break;
        }
        selector -= 1;
    }
    Some(selector - 1)
}

fn bind_exact_code(source: &Rom, address: u16, expected: &[u8], role: &str) -> Result<()> {
    let actual = source_bytes(source, address, expected.len())?;
    ensure!(actual == expected, "{role} source bytes changed");
    decode_rp2a03_sequence(actual, address, role)?;
    Ok(())
}

fn source_bytes(source: &Rom, address: u16, byte_count: usize) -> Result<&[u8]> {
    ensure!(
        (0x8000..0xC000).contains(&address)
            && usize::from(address - 0x8000)
                .checked_add(byte_count)
                .is_some_and(|end| end <= SOURCE_PRG_BANK_BYTE_COUNT),
        "screen-substate source range is outside the switchable PRG window"
    );
    let start = usize::from(OUTER_SCREEN_BANK)
        .checked_mul(SOURCE_PRG_BANK_BYTE_COUNT)
        .and_then(|base| base.checked_add(usize::from(address - 0x8000)))
        .context("screen-substate source offset overflow")?;
    source
        .prg()
        .get(start..start + byte_count)
        .context("screen-substate source range exceeds PRG")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn controller_low_nibble_selects_exactly_four_handlers() {
        assert_eq!(four_way_controller_selector(0), None);
        assert_eq!(
            (1..=0x0F)
                .filter_map(four_way_controller_selector)
                .collect::<BTreeSet<_>>(),
            BTreeSet::from([0, 1, 2, 3])
        );
    }

    #[test]
    fn controller_selector_ignores_upper_input_bits() {
        for low in 0..=0x0F {
            assert_eq!(
                four_way_controller_selector(low),
                four_way_controller_selector(low | 0xF0)
            );
        }
    }
}
