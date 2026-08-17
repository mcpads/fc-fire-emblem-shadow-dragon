use std::collections::{BTreeMap, BTreeSet};

use anyhow::{Context, Result, ensure};

use crate::{
    chapter_transition::bind_outer_screen_state_dispatch_source,
    dialogue_inventory::bind_caller_handoff_state_dispatch_sources,
    fixed_string_consumers::bind_composite_state_dispatch_source,
    mapper165::inline_pointer_dispatch::bind_inline_pointer_dispatch, rom::Rom,
    typed_source::decode_rp2a03_sequence,
};

const SOURCE_PRG_BANK_BYTE_COUNT: usize = 16 * 1024;
const MAP_DIALOGUE_BANK: u8 = 0x02;
const MAP_DIALOGUE_PRIMARY_DISPATCH_CALL: u16 = 0xA783;
const MAP_DIALOGUE_SECONDARY_DISPATCH_ENTRY: u16 = 0xA7F5;
const MAP_DIALOGUE_SECONDARY_DISPATCH_CALL: u16 = 0xA7F8;
const MAP_DIALOGUE_SECONDARY_ENTRY: [u8; 6] = [0xAD, 0xDB, 0x05, 0x20, 0x4C, 0xC3];
const MAP_DIALOGUE_SECONDARY_TARGETS: [u16; 2] = [0xA7FF, 0xA961];
const SAVE_COMPLETE_DIALOGUE_DISPATCH_CALL: u16 = 0xB36C;

pub(super) struct SourceScreenStateDispatches {
    selector_domains: BTreeMap<(u8, u16), BTreeSet<u8>>,
    selector_memory_addresses: BTreeMap<(u8, u16), u16>,
}

impl SourceScreenStateDispatches {
    pub(super) fn selector_domains(&self) -> &BTreeMap<(u8, u16), BTreeSet<u8>> {
        &self.selector_domains
    }

    pub(super) fn selector_memory_addresses(&self) -> &BTreeMap<(u8, u16), u16> {
        &self.selector_memory_addresses
    }
}

pub(super) fn bind_source_screen_state_dispatches(
    source: &Rom,
) -> Result<SourceScreenStateDispatches> {
    source.verify_supported_japanese()?;
    let mut selector_domains = BTreeMap::new();
    let mut selector_memory_addresses = BTreeMap::new();

    let caller_handoffs = bind_caller_handoff_state_dispatch_sources(source)?;
    for (bank, call, role) in [
        (
            MAP_DIALOGUE_BANK,
            MAP_DIALOGUE_PRIMARY_DISPATCH_CALL,
            "map-dialogue primary outer-state dispatch",
        ),
        (
            0x0B,
            SAVE_COMPLETE_DIALOGUE_DISPATCH_CALL,
            "save-complete dialogue-substate dispatch",
        ),
    ] {
        let matching = caller_handoffs
            .iter()
            .filter(|dispatch| dispatch.prg_bank() == bank && dispatch.call_address() == call)
            .collect::<Vec<_>>();
        ensure!(
            matching.len() == 1,
            "{role} is not owned by exactly one caller-handoff source table"
        );
        insert_domain(
            &mut selector_domains,
            bank,
            call,
            matching[0].selector_domain().clone(),
            role,
        )?;
    }

    let secondary_entry = switchable_bytes(
        source,
        MAP_DIALOGUE_BANK,
        MAP_DIALOGUE_SECONDARY_DISPATCH_ENTRY,
        MAP_DIALOGUE_SECONDARY_ENTRY.len(),
    )?;
    ensure!(
        secondary_entry == MAP_DIALOGUE_SECONDARY_ENTRY,
        "map-dialogue secondary outer-state dispatcher changed"
    );
    decode_rp2a03_sequence(
        secondary_entry,
        MAP_DIALOGUE_SECONDARY_DISPATCH_ENTRY,
        "dispatch secondary map-dialogue outer state",
    )?;
    let secondary_domain =
        (0..u8::try_from(MAP_DIALOGUE_SECONDARY_TARGETS.len())?).collect::<BTreeSet<_>>();
    let secondary = bind_inline_pointer_dispatch(
        source,
        MAP_DIALOGUE_BANK,
        MAP_DIALOGUE_SECONDARY_DISPATCH_CALL,
        secondary_domain.iter().copied(),
        "map-dialogue secondary outer-state dispatch",
    )?;
    ensure!(
        secondary.targets_in_selector_order() == MAP_DIALOGUE_SECONDARY_TARGETS,
        "map-dialogue secondary outer-state handlers changed"
    );
    insert_domain(
        &mut selector_domains,
        MAP_DIALOGUE_BANK,
        MAP_DIALOGUE_SECONDARY_DISPATCH_CALL,
        secondary_domain,
        "map-dialogue secondary outer-state dispatch",
    )?;
    insert_selector_memory_address(
        &mut selector_memory_addresses,
        MAP_DIALOGUE_BANK,
        MAP_DIALOGUE_SECONDARY_DISPATCH_CALL,
        0x05DB,
        "map-dialogue secondary outer-state dispatch",
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
        selector_memory_addresses,
    })
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
