use std::{
    collections::{BTreeMap, BTreeSet},
    ops::RangeInclusive,
};

use anyhow::{Context, Result, ensure};
use retro_rp2a03::{AddressingMode, Mnemonic, Operand};

use crate::{
    dialogue_inventory::CallerHandoffStateDispatchSource,
    dialogue_runtime_state::MAIN_DIALOGUE_RUNTIME_STATE,
    mapper165::{
        banked_call_dispatch::{BankedCallTransfer, bind_banked_call_dispatch},
        battle_codebook_plan::IndirectWriteDestinationBounds,
    },
    rom::Rom,
    sha1_hex,
    typed_source::decode_rp2a03_sequence,
};

use super::state_transition_evidence::{
    StateWriteStep, StateWriterSource, TransitionPath, bind_constant_store,
    bind_state_transition_closure, ensure_instruction, source_bytes,
};

const FIXED_PRG_BANK: u8 = 0x0F;
const DIALOGUE_BANK: u8 = 0x0B;
const DIALOGUE_OR_SOUND_STATE: u16 = MAIN_DIALOGUE_RUNTIME_STATE.dialogue_or_sound_state_address;

const RESET_ZERO_FILL_START: u16 = 0xC08B;
const RESET_ZERO_FILL_END: u16 = 0xC0A7;
const RESET_ZERO_FILL_SHA1: &str = "26bca1bff2b494b095072649d86ea784fc26b4e7";

const SAVE_COMPLETE_COPY_REGIONS: &[(u16, u16, &str, &str)] = &[
    (
        0x9985,
        0x99BF,
        "7cd66bf3ea49c9c79a29cc42642e530aa2d4fda0",
        "copy one save header into persistent or working storage",
    ),
    (
        0x99D7,
        0x9A33,
        "d9f3738a6a9a097cfb2b5dd30b48069e4d9660bb",
        "select save-complete copy destinations",
    ),
    (
        0x9A33,
        0x9A79,
        "5dc106bb2518894f2daf0f9bb2516bc362df4f8f",
        "copy save chapter, body, and checksum fields",
    ),
];

#[derive(Clone, Copy)]
struct LifecycleSpec {
    dispatch_call: u16,
    caller_bank: u8,
    caller_address: u16,
    caller_transfer: BankedCallTransfer,
    banked_selector: u8,
    state_one_writer_bank: u8,
    state_one_load: u16,
    state_one_store: u16,
    transitions: &'static [TransitionPath],
}

const INC_99BB: StateWriteStep = StateWriteStep::increment(0x99BB);
const INC_9A75: StateWriteStep = StateWriteStep::increment(0x9A75);
const INC_9AF8: StateWriteStep = StateWriteStep::increment(0x9AF8);
const INC_9B10: StateWriteStep = StateWriteStep::increment(0x9B10);
const INC_9B27: StateWriteStep = StateWriteStep::increment(0x9B27);
const INC_9B31: StateWriteStep = StateWriteStep::increment(0x9B31);
const SET_FF_9B46: StateWriteStep = StateWriteStep::store_constant(0x9B44, 0x9B46, 0xFF);
const INC_9B53: StateWriteStep = StateWriteStep::increment(0x9B53);
const INC_9B9C: StateWriteStep = StateWriteStep::increment(0x9B9C);
const INC_9BC5: StateWriteStep = StateWriteStep::increment(0x9BC5);
const INC_9BFE: StateWriteStep = StateWriteStep::increment(0x9BFE);
const INC_9C02: StateWriteStep = StateWriteStep::increment(0x9C02);
const INC_9C05: StateWriteStep = StateWriteStep::increment(0x9C05);
const INC_9D08: StateWriteStep = StateWriteStep::increment(0x9D08);
const INC_A063: StateWriteStep = StateWriteStep::increment(0xA063);
const INC_A06F: StateWriteStep = StateWriteStep::increment(0xA06F);
const INC_A084: StateWriteStep = StateWriteStep::increment(0xA084);
const INC_A09B: StateWriteStep = StateWriteStep::increment(0xA09B);
const SET_08_A0AD: StateWriteStep = StateWriteStep::store_constant(0xA0AB, 0xA0AD, 0x08);
const INC_A0FD: StateWriteStep = StateWriteStep::increment(0xA0FD);
const INC_B3BC: StateWriteStep = StateWriteStep::increment(0xB3BC);
const INC_B3CF: StateWriteStep = StateWriteStep::increment(0xB3CF);
const SET_07_B3D8: StateWriteStep = StateWriteStep::store_constant(0xB3D6, 0xB3D8, 0x07);
const INC_B3F4: StateWriteStep = StateWriteStep::increment(0xB3F4);
const INC_B40A: StateWriteStep = StateWriteStep::increment(0xB40A);
const INC_B428: StateWriteStep = StateWriteStep::increment(0xB428);

const SAVE_COMPLETE_TRANSITIONS: &[TransitionPath] = &[
    TransitionPath::new(1, 0x9985, &[INC_99BB]),
    TransitionPath::new(2, 0x9A33, &[INC_9A75]),
    TransitionPath::new(3, 0x9A99, &[INC_9AF8]),
    TransitionPath::new(4, 0x9AFC, &[INC_9B10]),
    TransitionPath::new(5, 0x9B14, &[INC_9B27]),
    TransitionPath::new(6, 0x9B2B, &[INC_9B31]),
    TransitionPath::new(7, 0x9B35, &[INC_9B53]),
    TransitionPath::new(7, 0x9B35, &[SET_FF_9B46, INC_9B53]),
    TransitionPath::new(8, 0x9B8A, &[INC_9B9C]),
    TransitionPath::new(9, 0x9B14, &[INC_9B27]),
    TransitionPath::new(10, 0x9BA0, &[INC_9BC5]),
    TransitionPath::new(11, 0x9BCF, &[INC_9BFE]),
    TransitionPath::new(12, 0x9C17, &[INC_9C05]),
    TransitionPath::new(12, 0x9C17, &[INC_9C02, INC_9C05]),
    TransitionPath::new(14, 0x9CF0, &[INC_9D08]),
];

const AUXILIARY_SAVE_TRANSITIONS: &[TransitionPath] = &[
    TransitionPath::new(1, 0xA09F, &[INC_A0FD]),
    TransitionPath::new(1, 0xA09F, &[SET_08_A0AD, INC_A0FD]),
    TransitionPath::new(2, 0x9B14, &[INC_9B27]),
    TransitionPath::new(3, 0x9B2B, &[INC_9B31]),
    TransitionPath::new(4, 0x9B35, &[INC_9B53]),
    TransitionPath::new(4, 0x9B35, &[SET_FF_9B46, INC_9B53]),
    TransitionPath::new(5, 0xA03E, &[INC_A063]),
    TransitionPath::new(6, 0xA067, &[INC_A06F]),
    TransitionPath::new(7, 0xA073, &[INC_A084]),
    TransitionPath::new(9, 0xA088, &[INC_A09B]),
    TransitionPath::new(10, 0xA03E, &[INC_A063]),
    TransitionPath::new(11, 0xA06A, &[INC_A06F]),
    TransitionPath::new(12, 0xA076, &[INC_A084]),
];

const SECONDARY_DIALOGUE_TRANSITIONS: &[TransitionPath] = &[
    TransitionPath::new(1, 0xB383, &[INC_B3BC]),
    TransitionPath::new(2, 0x9B14, &[INC_9B27]),
    TransitionPath::new(3, 0x9B2B, &[INC_9B31]),
    TransitionPath::new(4, 0xB3C0, &[INC_B3CF]),
    TransitionPath::new(4, 0xB3C0, &[SET_07_B3D8]),
    TransitionPath::new(5, 0xB3DC, &[INC_B3F4]),
    TransitionPath::new(6, 0xB3F8, &[INC_B40A]),
    TransitionPath::new(7, 0x9B14, &[INC_9B27]),
    TransitionPath::new(8, 0xB421, &[INC_B428]),
];

const LIFECYCLES: &[LifecycleSpec] = &[
    LifecycleSpec {
        dispatch_call: 0x9962,
        caller_bank: 0x06,
        caller_address: 0xB7D1,
        caller_transfer: BankedCallTransfer::Call,
        banked_selector: 0x05,
        state_one_writer_bank: 0x06,
        state_one_load: 0xB7A1,
        state_one_store: 0xB7A3,
        transitions: SAVE_COMPLETE_TRANSITIONS,
    },
    LifecycleSpec {
        dispatch_call: 0xA01F,
        caller_bank: 0x06,
        caller_address: 0xB6AC,
        caller_transfer: BankedCallTransfer::Call,
        banked_selector: 0x0B,
        state_one_writer_bank: 0x06,
        state_one_load: 0xB675,
        state_one_store: 0xB677,
        transitions: AUXILIARY_SAVE_TRANSITIONS,
    },
    LifecycleSpec {
        dispatch_call: 0xB36C,
        caller_bank: FIXED_PRG_BANK,
        caller_address: 0xF33E,
        caller_transfer: BankedCallTransfer::TailJump,
        banked_selector: 0x0A,
        state_one_writer_bank: 0x06,
        state_one_load: 0xBA09,
        state_one_store: 0xBA0B,
        transitions: SECONDARY_DIALOGUE_TRANSITIONS,
    },
];

pub(super) struct DialogueOrSoundStateLifecycleBindings {
    producer_domains: BTreeMap<(u8, u16), BTreeSet<u8>>,
    indirect_write_destinations: BTreeMap<(u8, u16, u8), IndirectWriteDestinationBounds>,
}

impl DialogueOrSoundStateLifecycleBindings {
    pub(super) fn producer_domain(&self, bank: u8, call: u16) -> Option<&BTreeSet<u8>> {
        self.producer_domains.get(&(bank, call))
    }

    pub(super) fn indirect_write_destinations(
        &self,
    ) -> &BTreeMap<(u8, u16, u8), IndirectWriteDestinationBounds> {
        &self.indirect_write_destinations
    }
}

pub(super) fn bind_dialogue_or_sound_state_lifecycles(
    source: &Rom,
    dispatches: &[CallerHandoffStateDispatchSource],
) -> Result<DialogueOrSoundStateLifecycleBindings> {
    source.verify_supported_japanese()?;
    bind_reset_zero_seed(source)?;

    let by_call = dispatches
        .iter()
        .filter(|dispatch| dispatch.selector_address() == DIALOGUE_OR_SOUND_STATE)
        .map(|dispatch| ((dispatch.prg_bank(), dispatch.call_address()), dispatch))
        .collect::<BTreeMap<_, _>>();
    ensure!(
        by_call.keys().copied().collect::<BTreeSet<_>>()
            == LIFECYCLES
                .iter()
                .map(|spec| (DIALOGUE_BANK, spec.dispatch_call))
                .collect(),
        "dialogue-or-sound state dispatch population changed"
    );

    let mut producer_domains = BTreeMap::new();
    for spec in LIFECYCLES {
        let dispatch = by_call
            .get(&(DIALOGUE_BANK, spec.dispatch_call))
            .context("dialogue-or-sound lifecycle lost its dispatch table")?;
        bind_lifecycle_caller(source, spec, spec.dispatch_call - 3)?;
        bind_constant_store(
            source,
            spec.state_one_writer_bank,
            spec.state_one_load,
            spec.state_one_store,
            DIALOGUE_OR_SOUND_STATE,
            1,
            "enter dialogue-or-sound state one",
        )?;
        let domain = bind_state_transition_closure(
            StateWriterSource {
                source,
                bank: DIALOGUE_BANK,
                state_address: DIALOGUE_OR_SOUND_STATE,
            },
            dispatch.selector_domain(),
            |selector| dispatch.handler_target(selector),
            [0, 1],
            spec.transitions,
            "dialogue-or-sound state lifecycle",
        )?;
        ensure!(
            domain == *dispatch.selector_domain(),
            "dialogue-or-sound lifecycle reaches {domain:02X?}, but its source table owns {:02X?}",
            dispatch.selector_domain()
        );
        ensure!(
            producer_domains
                .insert((DIALOGUE_BANK, spec.dispatch_call), domain)
                .is_none(),
            "dialogue-or-sound lifecycle duplicated a dispatch site"
        );
    }

    Ok(DialogueOrSoundStateLifecycleBindings {
        producer_domains,
        indirect_write_destinations: bind_save_complete_copy_destinations(source)?,
    })
}

fn bind_lifecycle_caller(source: &Rom, spec: &LifecycleSpec, dispatch_entry: u16) -> Result<()> {
    let binding = bind_banked_call_dispatch(
        source,
        spec.caller_bank,
        spec.caller_address,
        spec.caller_transfer,
        DIALOGUE_BANK,
        spec.banked_selector,
        "enter one dialogue-or-sound state machine",
    )?;
    ensure!(
        binding.target() == dispatch_entry,
        "dialogue-or-sound caller no longer reaches its owned state dispatcher"
    );
    Ok(())
}

fn bind_reset_zero_seed(source: &Rom) -> Result<()> {
    let bytes = source_bytes(
        source,
        FIXED_PRG_BANK,
        RESET_ZERO_FILL_START,
        usize::from(RESET_ZERO_FILL_END - RESET_ZERO_FILL_START),
    )?;
    ensure!(
        sha1_hex(bytes) == RESET_ZERO_FILL_SHA1,
        "reset zero-fill source changed"
    );
    decode_rp2a03_sequence(bytes, RESET_ZERO_FILL_START, "zero source RAM at reset")?;
    ensure!(
        (0x0000..=0x07FF).contains(&DIALOGUE_OR_SOUND_STATE),
        "dialogue-or-sound state left the reset zero-fill range"
    );
    Ok(())
}

fn bind_save_complete_copy_destinations(
    source: &Rom,
) -> Result<BTreeMap<(u8, u16, u8), IndirectWriteDestinationBounds>> {
    for &(start, end, sha1, role) in SAVE_COMPLETE_COPY_REGIONS {
        let bytes = source_bytes(source, DIALOGUE_BANK, start, usize::from(end - start))?;
        ensure!(sha1_hex(bytes) == sha1, "{role} source bytes changed");
        decode_rp2a03_sequence(bytes, start, role)?;
    }

    let destinations = [
        (
            (DIALOGUE_BANK, 0x99A8, 0x02),
            vec![0x6000..=0x6017, 0x6544..=0x655B],
            "save header copy destination",
        ),
        (
            (DIALOGUE_BANK, 0x9A56, 0x02),
            vec![0x6519..=0x6528, 0x6A5D..=0x6A6C],
            "save chapter-field copy destination",
        ),
        (
            (DIALOGUE_BANK, 0x9A63, 0x02),
            vec![0x64C8..=0x6518, 0x6A0C..=0x6A5C],
            "save body copy destination",
        ),
        (
            (DIALOGUE_BANK, 0x9A70, 0x02),
            vec![0x6529..=0x6541, 0x6A6D..=0x6A85],
            "save checksum-field copy destination",
        ),
    ];
    let mut bounds = BTreeMap::new();
    for (site, ranges, role) in destinations {
        ensure_instruction(
            source,
            site.0,
            site.1,
            Mnemonic::Sta,
            AddressingMode::ZeroPageIndirectIndexedY,
            Operand::Byte(site.2),
            role,
        )?;
        ensure_destination_ranges_below_mapper_space(&ranges, role)?;
        ensure!(
            bounds
                .insert(
                    site,
                    IndirectWriteDestinationBounds::from_source_ranges(role, ranges)?,
                )
                .is_none(),
            "save-complete copy destination was registered twice"
        );
    }
    Ok(bounds)
}

fn ensure_destination_ranges_below_mapper_space(
    ranges: &[RangeInclusive<u16>],
    role: &str,
) -> Result<()> {
    ensure!(
        !ranges.is_empty()
            && ranges
                .iter()
                .all(|range| range.start() <= range.end() && *range.end() < 0x8000),
        "{role} is empty, inverted, or reaches mapper space"
    );
    Ok(())
}
