use std::collections::BTreeSet;

use anyhow::{Result, ensure};

use crate::{
    mapper165::inline_pointer_dispatch::bind_inline_pointer_dispatch, rom::Rom, sha1_hex,
    typed_source::decode_rp2a03_sequence,
};

use super::super::control_state::BATTLE_ANIMATION_TEST_PHASE;
use super::state_transition_evidence::{
    StateWriteStep, TransitionPath, bind_constant_store, bind_state_transition_closure,
    source_bytes,
};

const PHASE_BANK: u8 = 0x07;
const DISPATCH_CALL: u16 = 0xAA4D;
const HANDLERS: [u16; 6] = [0xAA5F, 0xAA82, 0xAB0D, 0xABB8, 0xAC0A, 0xAC1E];

const SOURCE_REGIONS: &[(u8, u16, u16, &str, &str)] = &[
    (
        0x0B,
        0x9C09,
        0x9C17,
        "f03c41439c7cd8f41e9c31bac14897f14b2139d0",
        "enter the battle-animation test",
    ),
    (
        PHASE_BANK,
        0xAA5F,
        0xAA82,
        "ec74a0ef9c91d41ee9eab5884b5fea87bac026f5",
        "initialize the battle-animation test phase",
    ),
    (
        PHASE_BANK,
        0xAA82,
        0xAA89,
        "29c43339ec7cac6b98982a09c43fa7b753d27645",
        "prepare random battle-animation test inputs",
    ),
    (
        PHASE_BANK,
        0xAB0D,
        0xABA1,
        "85f547dbf304136c6156478c5da45ea293cd67dc",
        "select one battle-animation test pairing",
    ),
    (
        PHASE_BANK,
        0xABB8,
        0xAC0A,
        "41f213966ffb1e4de01c840b2560b6f24702ae2f",
        "advance or restart a battle-animation test pairing",
    ),
    (
        PHASE_BANK,
        0xAC0A,
        0xAC1E,
        "56b01df51d0c279888f615a591ee6340b39befd4",
        "enter the shared battle engine from the sound test",
    ),
    (
        PHASE_BANK,
        0xAC1E,
        0xAC44,
        "27075559ba7defcd24dc61cd28ebf6e99ff88e7a",
        "wait for and recycle the battle-animation test",
    ),
];

const INC_AA7E: StateWriteStep = StateWriteStep::increment(0xAA7E);
const INC_AA85: StateWriteStep = StateWriteStep::increment(0xAA85);
const INC_AB9D: StateWriteStep = StateWriteStep::increment(0xAB9D);
const SET_00_AC00: StateWriteStep = StateWriteStep::store_constant(0xABFE, 0xAC00, 0x00);
const INC_AC06: StateWriteStep = StateWriteStep::increment(0xAC06);
const INC_AC1A: StateWriteStep = StateWriteStep::increment(0xAC1A);
const SET_03_AC30: StateWriteStep = StateWriteStep::store_constant(0xAC2E, 0xAC30, 0x03);

const TRANSITIONS: &[TransitionPath] = &[
    TransitionPath::new(0, 0xAA5F, &[INC_AA7E]),
    TransitionPath::new(1, 0xAA82, &[INC_AA85]),
    TransitionPath::new(2, 0xAB0D, &[INC_AB9D]),
    TransitionPath::new(3, 0xABB8, &[SET_00_AC00, INC_AC06]),
    TransitionPath::new(3, 0xABB8, &[INC_AC06]),
    TransitionPath::new(4, 0xAC0A, &[INC_AC1A]),
    TransitionPath::new(5, 0xAC1E, &[SET_03_AC30]),
];

pub(super) struct BattleAnimationTestPhaseLifecycle {
    handler_domain: BTreeSet<u8>,
    produced_selectors: BTreeSet<u8>,
}

impl BattleAnimationTestPhaseLifecycle {
    pub(super) fn prg_bank(&self) -> u8 {
        PHASE_BANK
    }

    pub(super) fn dispatch_call(&self) -> u16 {
        DISPATCH_CALL
    }

    pub(super) fn selector_address(&self) -> u16 {
        BATTLE_ANIMATION_TEST_PHASE
    }

    pub(super) fn handler_domain(&self) -> &BTreeSet<u8> {
        &self.handler_domain
    }

    pub(super) fn produced_selectors(&self) -> &BTreeSet<u8> {
        &self.produced_selectors
    }
}

pub(super) fn bind_battle_animation_test_phase_lifecycle(
    source: &Rom,
) -> Result<BattleAnimationTestPhaseLifecycle> {
    source.verify_supported_japanese()?;
    let handler_domain = (0..u8::try_from(HANDLERS.len())?).collect::<BTreeSet<_>>();
    let dispatch = bind_inline_pointer_dispatch(
        source,
        PHASE_BANK,
        DISPATCH_CALL,
        handler_domain.iter().copied(),
        "battle-animation test phase dispatch",
    )?;
    ensure!(
        dispatch.targets_in_selector_order() == HANDLERS,
        "battle-animation test phase handlers changed"
    );

    for &(bank, start, end, sha1, role) in SOURCE_REGIONS {
        let bytes = source_bytes(source, bank, start, usize::from(end - start))?;
        ensure!(sha1_hex(bytes) == sha1, "{role} source bytes changed");
        decode_rp2a03_sequence(bytes, start, role)?;
    }
    bind_constant_store(
        source,
        0x0B,
        0x9C09,
        0x9C0B,
        BATTLE_ANIMATION_TEST_PHASE,
        0,
        "initialize the battle-animation test phase",
    )?;

    let produced_selectors = bind_state_transition_closure(
        source,
        PHASE_BANK,
        BATTLE_ANIMATION_TEST_PHASE,
        &handler_domain,
        |selector| HANDLERS.get(usize::from(selector)).copied(),
        [0],
        TRANSITIONS,
        "battle-animation test phase lifecycle",
    )?;
    ensure!(
        produced_selectors == handler_domain,
        "battle-animation test phase lifecycle no longer reaches every source handler"
    );

    Ok(BattleAnimationTestPhaseLifecycle {
        handler_domain,
        produced_selectors,
    })
}
