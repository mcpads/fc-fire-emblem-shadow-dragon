use std::collections::{BTreeMap, BTreeSet};

use anyhow::{Context, Result, ensure};

use crate::{
    chapter_transition::bind_outer_screen_state_dispatch_source,
    dialogue_inventory::bind_caller_handoff_state_dispatch_sources,
    fixed_string_consumers::bind_composite_state_dispatch_source,
    map_dialogue_lifecycle::bind_outer_screen_map_dialogue_lifecycle,
    mapper165::{
        battle_codebook_plan::IndirectWriteDestinationBounds,
        inline_pointer_dispatch::bind_inline_pointer_dispatch,
    },
    rom::Rom,
    sha1_hex,
    typed_source::decode_rp2a03_sequence,
};

use super::{
    chapter_map_loader::BoundChapterMapDimensions, control_state::MAIN_STATE,
    unit_record_writers::BoundUnitRecordAddressDomain,
};

mod battle_animation_test_lifecycle;
mod dialogue_or_sound_lifecycles;
mod main_state_lifecycles;
mod screen_substate_dispatches;
mod selector_transition_graph;
mod state_transition_evidence;

use battle_animation_test_lifecycle::bind_battle_animation_test_phase_lifecycle;
use dialogue_or_sound_lifecycles::bind_dialogue_or_sound_state_lifecycles;
use main_state_lifecycles::bind_outer_screen_main_state_lifecycles;
use screen_substate_dispatches::bind_screen_substate_dispatches;

const SOURCE_PRG_BANK_BYTE_COUNT: usize = 16 * 1024;
const FRONT_END_RECORD_BANK: u8 = 0x02;
const FRONT_END_RECORD_RESULT_DISPATCH_ENTRY: u16 = 0xA7F5;
const FRONT_END_RECORD_RESULT_DISPATCH_CALL: u16 = 0xA7F8;
const FRONT_END_RECORD_RESULT_ENTRY: [u8; 6] = [0xAD, 0xDB, 0x05, 0x20, 0x4C, 0xC3];
const FRONT_END_RECORD_RESULT_TARGETS: [u16; 2] = [0xA7FF, 0xA961];
pub(super) const GAMEPLAY_MAIN_STATE_DISPATCH_CALL: u16 = 0x8964;
const GAMEPLAY_MAIN_STATE_COUNT: u8 = 0x45;
const GAMEPLAY_MAIN_STATE_TABLE_SHA1: &str = "e14021109603c577f529bda132c8e21fed8f3333";

pub(super) struct SourceScreenStateDispatches {
    selector_domains: BTreeMap<(u8, u16), BTreeSet<u8>>,
    source_producer_domains: BTreeMap<(u8, u16), BTreeSet<u8>>,
    selector_memory_addresses: BTreeMap<(u8, u16), u16>,
    indirect_write_destinations: BTreeMap<(u8, u16, u8), IndirectWriteDestinationBounds>,
    gameplay_main_state_seed_selectors: BTreeSet<u8>,
    gameplay_deferred_main_state_selectors: BTreeSet<u8>,
}

impl SourceScreenStateDispatches {
    pub(super) fn selector_domains(&self) -> &BTreeMap<(u8, u16), BTreeSet<u8>> {
        &self.selector_domains
    }

    pub(super) fn selector_memory_addresses(&self) -> &BTreeMap<(u8, u16), u16> {
        &self.selector_memory_addresses
    }

    pub(super) fn source_producer_domains(&self) -> &BTreeMap<(u8, u16), BTreeSet<u8>> {
        &self.source_producer_domains
    }

    pub(super) fn indirect_write_destinations(
        &self,
    ) -> &BTreeMap<(u8, u16, u8), IndirectWriteDestinationBounds> {
        &self.indirect_write_destinations
    }

    pub(super) fn gameplay_main_state_seed_selectors(&self) -> &BTreeSet<u8> {
        &self.gameplay_main_state_seed_selectors
    }

    pub(super) fn gameplay_deferred_main_state_selectors(&self) -> &BTreeSet<u8> {
        &self.gameplay_deferred_main_state_selectors
    }
}

pub(super) fn bind_source_screen_state_dispatches(
    source: &Rom,
    unit_record_domain: &BoundUnitRecordAddressDomain,
    chapter_map_dimensions: &BoundChapterMapDimensions,
) -> Result<SourceScreenStateDispatches> {
    source.verify_supported_japanese()?;
    let mut selector_domains = BTreeMap::new();
    let mut source_producer_domains = BTreeMap::new();
    let mut selector_memory_addresses = BTreeMap::new();
    let mut indirect_write_destinations = BTreeMap::new();

    let caller_handoffs = bind_caller_handoff_state_dispatch_sources(source)?;
    let dialogue_or_sound_lifecycles =
        bind_dialogue_or_sound_state_lifecycles(source, &caller_handoffs)?;
    let caller_handoff_keys = caller_handoffs
        .iter()
        .map(|dispatch| (dispatch.prg_bank(), dispatch.call_address()))
        .collect::<BTreeSet<_>>();
    ensure!(
        caller_handoff_keys.len() == caller_handoffs.len(),
        "caller-handoff source tables do not have unique dispatch sites"
    );
    for dispatch in &caller_handoffs {
        let bank = dispatch.prg_bank();
        let call = dispatch.call_address();
        let role = format!("caller-handoff state dispatch {bank:02X}:${call:04X}");
        insert_domain(
            &mut selector_domains,
            bank,
            call,
            dispatch.selector_domain().clone(),
            &role,
        )?;
        insert_selector_memory_address(
            &mut selector_memory_addresses,
            bank,
            call,
            dispatch.selector_address(),
            &role,
        )?;
        if let Some(produced) = dialogue_or_sound_lifecycles.producer_domain(bank, call) {
            insert_domain(
                &mut source_producer_domains,
                bank,
                call,
                produced.clone(),
                "dialogue-or-sound state lifecycle",
            )?;
        }
    }
    ensure!(
        caller_handoff_keys.iter().all(|key| {
            selector_domains.contains_key(key) && selector_memory_addresses.contains_key(key)
        }),
        "screen-state registry omitted a source-bound caller-handoff dispatch"
    );
    for (&site, destination) in dialogue_or_sound_lifecycles.indirect_write_destinations() {
        ensure!(
            indirect_write_destinations
                .insert(site, destination.clone())
                .is_none(),
            "dialogue-or-sound lifecycle duplicates indirect-write destination owner at {:02X}:${:04X}",
            site.0,
            site.1,
        );
    }

    let battle_animation_test = bind_battle_animation_test_phase_lifecycle(source)?;
    insert_domain(
        &mut selector_domains,
        battle_animation_test.prg_bank(),
        battle_animation_test.dispatch_call(),
        battle_animation_test.handler_domain().clone(),
        "battle-animation test phase dispatch",
    )?;
    insert_domain(
        &mut source_producer_domains,
        battle_animation_test.prg_bank(),
        battle_animation_test.dispatch_call(),
        battle_animation_test.produced_selectors().clone(),
        "battle-animation test phase producer",
    )?;
    insert_selector_memory_address(
        &mut selector_memory_addresses,
        battle_animation_test.prg_bank(),
        battle_animation_test.dispatch_call(),
        battle_animation_test.selector_address(),
        "battle-animation test phase dispatch",
    )?;

    let secondary_entry = switchable_bytes(
        source,
        FRONT_END_RECORD_BANK,
        FRONT_END_RECORD_RESULT_DISPATCH_ENTRY,
        FRONT_END_RECORD_RESULT_ENTRY.len(),
    )?;
    ensure!(
        secondary_entry == FRONT_END_RECORD_RESULT_ENTRY,
        "front-end record-result state dispatcher changed"
    );
    decode_rp2a03_sequence(
        secondary_entry,
        FRONT_END_RECORD_RESULT_DISPATCH_ENTRY,
        "dispatch front-end record-result state",
    )?;
    let secondary_domain =
        (0..u8::try_from(FRONT_END_RECORD_RESULT_TARGETS.len())?).collect::<BTreeSet<_>>();
    let secondary = bind_inline_pointer_dispatch(
        source,
        FRONT_END_RECORD_BANK,
        FRONT_END_RECORD_RESULT_DISPATCH_CALL,
        secondary_domain.iter().copied(),
        "front-end record-result state dispatch",
    )?;
    ensure!(
        secondary.targets_in_selector_order() == FRONT_END_RECORD_RESULT_TARGETS,
        "front-end record-result state handlers changed"
    );
    insert_domain(
        &mut selector_domains,
        FRONT_END_RECORD_BANK,
        FRONT_END_RECORD_RESULT_DISPATCH_CALL,
        secondary_domain.clone(),
        "front-end record-result state dispatch",
    )?;
    insert_domain(
        &mut source_producer_domains,
        FRONT_END_RECORD_BANK,
        FRONT_END_RECORD_RESULT_DISPATCH_CALL,
        bind_front_end_record_result_state_producers(source)?,
        "front-end record-result state producer",
    )?;
    insert_selector_memory_address(
        &mut selector_memory_addresses,
        FRONT_END_RECORD_BANK,
        FRONT_END_RECORD_RESULT_DISPATCH_CALL,
        0x05DB,
        "front-end record-result state dispatch",
    )?;

    let outer_screen = bind_outer_screen_state_dispatch_source(source)?;
    insert_domain(
        &mut selector_domains,
        outer_screen.prg_bank(),
        outer_screen.call_address(),
        outer_screen.selector_domain().clone(),
        "gameplay outer-screen dispatch",
    )?;
    insert_selector_memory_address(
        &mut selector_memory_addresses,
        outer_screen.prg_bank(),
        outer_screen.call_address(),
        outer_screen.selector_address(),
        "gameplay outer-screen dispatch",
    )?;

    let outer_screen_main_state_lifecycles = bind_outer_screen_main_state_lifecycles(source)?;
    for lifecycle in outer_screen_main_state_lifecycles.dispatches() {
        let call = lifecycle.dispatch_call();
        insert_domain(
            &mut selector_domains,
            0x06,
            call,
            lifecycle.handler_domain().clone(),
            "outer-screen nested main-state dispatch",
        )?;
        if let Some(produced) = lifecycle.produced_selectors() {
            insert_domain(
                &mut source_producer_domains,
                0x06,
                call,
                produced.clone(),
                "outer-screen nested main-state producer",
            )?;
        }
        insert_selector_memory_address(
            &mut selector_memory_addresses,
            0x06,
            call,
            MAIN_STATE,
            "outer-screen nested main-state dispatch",
        )?;
    }

    let map_dialogue_lifecycle = bind_outer_screen_map_dialogue_lifecycle(source)?;
    insert_domain(
        &mut selector_domains,
        0x06,
        map_dialogue_lifecycle.dispatch_call(),
        map_dialogue_lifecycle.handler_domain().clone(),
        "outer-screen map-dialogue state dispatch",
    )?;
    insert_domain(
        &mut source_producer_domains,
        0x06,
        map_dialogue_lifecycle.dispatch_call(),
        map_dialogue_lifecycle.produced_selectors().clone(),
        "outer-screen map-dialogue state producer",
    )?;
    insert_selector_memory_address(
        &mut selector_memory_addresses,
        0x06,
        map_dialogue_lifecycle.dispatch_call(),
        0x05DB,
        "outer-screen map-dialogue state dispatch",
    )?;

    for dispatch in
        bind_screen_substate_dispatches(source, unit_record_domain, chapter_map_dimensions)?
    {
        let bank = dispatch.prg_bank();
        let call = dispatch.call_address();
        insert_domain(
            &mut selector_domains,
            bank,
            call,
            dispatch.handler_domain().clone(),
            dispatch.role(),
        )?;
        if let Some(produced) = dispatch.source_bound_produced_selectors() {
            insert_domain(
                &mut source_producer_domains,
                bank,
                call,
                produced.clone(),
                dispatch.role(),
            )?;
        }
        if let Some(selector_address) = dispatch.selector_memory_address() {
            insert_selector_memory_address(
                &mut selector_memory_addresses,
                bank,
                call,
                selector_address,
                dispatch.role(),
            )?;
        }
        for (&site, destination) in dispatch.indirect_write_destinations() {
            ensure!(
                indirect_write_destinations
                    .insert(site, destination.clone())
                    .is_none(),
                "{} duplicates indirect-write destination owner at {:02X}:${:04X}",
                dispatch.role(),
                site.0,
                site.1,
            );
        }
    }

    let gameplay_main_state_domain = (0..GAMEPLAY_MAIN_STATE_COUNT).collect::<BTreeSet<_>>();
    let gameplay_main_state = bind_inline_pointer_dispatch(
        source,
        0x06,
        GAMEPLAY_MAIN_STATE_DISPATCH_CALL,
        gameplay_main_state_domain.iter().copied(),
        "gameplay main-state dispatch",
    )?;
    let gameplay_table_bytes = switchable_bytes(
        source,
        0x06,
        gameplay_main_state.table_start(),
        usize::from(GAMEPLAY_MAIN_STATE_COUNT) * 2,
    )?;
    ensure!(
        gameplay_main_state.table_start() == 0x8967
            && sha1_hex(gameplay_table_bytes) == GAMEPLAY_MAIN_STATE_TABLE_SHA1,
        "gameplay main-state pointer table boundary changed"
    );
    insert_domain(
        &mut selector_domains,
        0x06,
        GAMEPLAY_MAIN_STATE_DISPATCH_CALL,
        gameplay_main_state_domain,
        "gameplay main-state dispatch",
    )?;
    insert_selector_memory_address(
        &mut selector_memory_addresses,
        0x06,
        GAMEPLAY_MAIN_STATE_DISPATCH_CALL,
        MAIN_STATE,
        "gameplay main-state dispatch",
    )?;

    let composite = bind_composite_state_dispatch_source(source)?;
    insert_domain(
        &mut selector_domains,
        composite.prg_bank(),
        composite.call_address(),
        composite.handler_selector_domain().clone(),
        "composite screen dispatch",
    )?;
    ensure!(
        composite
            .direct_producer_selector_domain()
            .is_subset(composite.handler_selector_domain()),
        "composite direct producers escape the source-bound handler table"
    );

    Ok(SourceScreenStateDispatches {
        selector_domains,
        source_producer_domains,
        selector_memory_addresses,
        indirect_write_destinations,
        gameplay_main_state_seed_selectors: outer_screen_main_state_lifecycles
            .gameplay_main_state_seed_selectors()
            .clone(),
        gameplay_deferred_main_state_selectors: outer_screen_main_state_lifecycles
            .gameplay_deferred_main_state_selectors()
            .clone(),
    })
}

fn bind_front_end_record_result_state_producers(source: &Rom) -> Result<BTreeSet<u8>> {
    let advance = switchable_bytes(source, FRONT_END_RECORD_BANK, 0xA8D0, 3)?;
    ensure!(
        advance == [0xEE, 0xDB, 0x05],
        "front-end record-result advance changed"
    );
    decode_rp2a03_sequence(advance, 0xA8D0, "advance front-end record-result state")?;

    let finish = switchable_bytes(source, FRONT_END_RECORD_BANK, 0xA96D, 7)?;
    ensure!(
        finish == [0xA9, 0x00, 0x85, 0x26, 0x8D, 0xDB, 0x05],
        "front-end record-result completion changed"
    );
    decode_rp2a03_sequence(finish, 0xA96D, "complete front-end record-result state")?;

    Ok(BTreeSet::from([0x00, 0x01]))
}

fn insert_selector_memory_address(
    addresses: &mut BTreeMap<(u8, u16), u16>,
    bank: u8,
    call: u16,
    selector_address: u16,
    role: &str,
) -> Result<()> {
    ensure!(
        addresses.insert((bank, call), selector_address).is_none(),
        "{role} duplicates a selector-memory binding at {bank:02X}:${call:04X}"
    );
    Ok(())
}

fn insert_domain(
    domains: &mut BTreeMap<(u8, u16), BTreeSet<u8>>,
    bank: u8,
    call: u16,
    selectors: BTreeSet<u8>,
    role: &str,
) -> Result<()> {
    ensure!(!selectors.is_empty(), "{role} selector domain is empty");
    ensure!(
        domains.insert((bank, call), selectors).is_none(),
        "{role} duplicates an owned inline dispatch at {bank:02X}:${call:04X}"
    );
    Ok(())
}

fn switchable_bytes(source: &Rom, bank: u8, address: u16, byte_count: usize) -> Result<&[u8]> {
    ensure!(bank < 0x0F, "screen-state source uses the fixed PRG bank");
    ensure!(
        (0x8000..0xC000).contains(&address),
        "screen-state source address is outside the switchable window"
    );
    let start = usize::from(bank)
        .checked_mul(SOURCE_PRG_BANK_BYTE_COUNT)
        .and_then(|base| base.checked_add(usize::from(address - 0x8000)))
        .context("screen-state source offset overflow")?;
    source
        .prg()
        .get(start..start + byte_count)
        .context("screen-state source range exceeds PRG")
}
