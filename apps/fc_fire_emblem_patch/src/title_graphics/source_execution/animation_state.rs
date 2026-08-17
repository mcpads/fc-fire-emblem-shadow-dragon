use std::collections::{BTreeMap, BTreeSet};

use anyhow::{Context, Result, ensure};
use retro_rp2a03::{Location, MemoryAddress, Rp2A03, decode_bytes};
use typed_isa_core::{AccessKind, StaticSemantics};

use crate::{
    mapper165::inline_pointer_dispatch::bind_inline_pointer_dispatch, rom::Rom, sha1_hex,
    typed_source::decode_rp2a03_sequence,
};

use super::{SOURCE_PRG_BANK, SOURCE_PRG_BANK_BYTE_COUNT, SWITCHABLE_CPU_START, source_cpu_bytes};

const TITLE_ANIMATION_STATE_ADDRESS: u16 = 0x0587;
const TITLE_ANIMATION_STAGING_ADDRESS: u8 = 0x0A;
const TITLE_ANIMATION_DISPATCH_CALL: u16 = 0xA757;
const TITLE_ANIMATION_TARGETS: [u16; 5] = [0xA798, 0xA83A, 0xA7DA, 0xA7F6, 0xA871];

const TITLE_ANIMATION_TICK_START: u16 = 0xA766;
const TITLE_ANIMATION_TICK_BYTE_COUNT: usize = 0x32;
const TITLE_ANIMATION_TICK_SHA1: &str = "89bfd9d380126c0b20b174b64dd0058769671651";
const TITLE_ANIMATION_HANDLERS_START: u16 = 0xA798;
const TITLE_ANIMATION_HANDLERS_BYTE_COUNT: usize = 0xF4;
const TITLE_ANIMATION_HANDLERS_SHA1: &str = "23fbce3b72baaaf54772c551669b38ba51f7582c";
const TITLE_ANIMATION_INITIALIZER_START: u16 = 0xA890;
const TITLE_ANIMATION_INITIALIZER_BYTE_COUNT: usize = 0x43;
const TITLE_ANIMATION_INITIALIZER_SHA1: &str = "df0506aa7ac239b9f2b00fcea39130eb2c8498f1";

#[derive(Clone, Debug)]
pub(super) struct TitleAnimationStateExecution {
    dispatch_call: u16,
    selector_domain: BTreeSet<u8>,
    selector_targets: BTreeMap<u8, u16>,
    documented_direct_writer_starts: BTreeSet<(u8, u16)>,
}

impl TitleAnimationStateExecution {
    pub(super) fn dispatch_call(&self) -> u16 {
        self.dispatch_call
    }

    pub(super) fn selector_domain(&self) -> &BTreeSet<u8> {
        &self.selector_domain
    }

    pub(super) fn selector_targets(&self) -> &BTreeMap<u8, u16> {
        &self.selector_targets
    }

    pub(super) fn documented_direct_writer_starts(&self) -> &BTreeSet<(u8, u16)> {
        &self.documented_direct_writer_starts
    }
}

pub(super) fn bind_title_animation_state_execution(
    source: &Rom,
) -> Result<TitleAnimationStateExecution> {
    let tick = bind_code_region(
        source,
        TITLE_ANIMATION_TICK_START,
        TITLE_ANIMATION_TICK_BYTE_COUNT,
        TITLE_ANIMATION_TICK_SHA1,
        "title animation tick",
    )?;
    ensure!(
        tick.windows(3)
            .any(|candidate| candidate == [0x20, 0x54, 0xA7]),
        "title animation tick no longer calls its state dispatch"
    );

    let handlers = bind_code_region(
        source,
        TITLE_ANIMATION_HANDLERS_START,
        TITLE_ANIMATION_HANDLERS_BYTE_COUNT,
        TITLE_ANIMATION_HANDLERS_SHA1,
        "title animation state handlers",
    )?;
    let initializer = bind_code_region(
        source,
        TITLE_ANIMATION_INITIALIZER_START,
        TITLE_ANIMATION_INITIALIZER_BYTE_COUNT,
        TITLE_ANIMATION_INITIALIZER_SHA1,
        "title animation state initializer",
    )?;

    let (handler_values, mut producer_write_starts) =
        immediate_state_writes(handlers, TITLE_ANIMATION_HANDLERS_START)?;
    let (initializer_values, initializer_write_starts) =
        immediate_state_writes(initializer, TITLE_ANIMATION_INITIALIZER_START)?;
    ensure!(
        initializer_values == BTreeSet::from([0x00]),
        "title animation initializer no longer establishes state zero"
    );
    producer_write_starts.extend(initializer_write_starts);

    let staging_values = immediate_staging_writes(handlers);
    ensure!(
        !staging_values.is_empty(),
        "title animation state has no source-bound staged transition values"
    );
    let staged_writer_starts = staged_state_writes(handlers, TITLE_ANIMATION_HANDLERS_START)?;
    ensure!(
        staged_writer_starts.len() == 1,
        "title animation state no longer has exactly one staged transition writer"
    );
    producer_write_starts.extend(staged_writer_starts);

    let selector_domain = handler_values
        .into_iter()
        .chain(initializer_values)
        .chain(staging_values)
        .collect::<BTreeSet<_>>();
    ensure_selector_domain_matches_handlers(&selector_domain, TITLE_ANIMATION_TARGETS.len())?;

    let documented_direct_writer_starts = documented_direct_state_writers_in_source(source)?;
    let source_bound_producer_write_starts = producer_write_starts
        .into_iter()
        .map(|address| (SOURCE_PRG_BANK, address))
        .collect::<BTreeSet<_>>();
    ensure_direct_writer_ownership(
        &documented_direct_writer_starts,
        &source_bound_producer_write_starts,
    )?;

    let dispatch = bind_inline_pointer_dispatch(
        source,
        SOURCE_PRG_BANK,
        TITLE_ANIMATION_DISPATCH_CALL,
        selector_domain.iter().copied(),
        "title animation state dispatch",
    )?;
    ensure!(
        dispatch.targets_in_selector_order() == TITLE_ANIMATION_TARGETS,
        "title animation state-to-handler mapping changed"
    );
    let selector_targets = selector_domain
        .iter()
        .copied()
        .zip(dispatch.targets_in_selector_order())
        .collect::<BTreeMap<_, _>>();

    Ok(TitleAnimationStateExecution {
        dispatch_call: TITLE_ANIMATION_DISPATCH_CALL,
        selector_domain,
        selector_targets,
        documented_direct_writer_starts,
    })
}

fn bind_code_region<'a>(
    source: &'a Rom,
    address: u16,
    byte_count: usize,
    expected_sha1: &str,
    role: &str,
) -> Result<&'a [u8]> {
    let bytes = source_cpu_bytes(source, SOURCE_PRG_BANK, address, byte_count)?;
    ensure!(sha1_hex(bytes) == expected_sha1, "source {role} changed");
    decode_rp2a03_sequence(bytes, address, role)?;
    Ok(bytes)
}

fn immediate_state_writes(bytes: &[u8], origin: u16) -> Result<(BTreeSet<u8>, BTreeSet<u16>)> {
    let mut values = BTreeSet::new();
    let mut writer_starts = BTreeSet::new();
    for (offset, candidate) in bytes.windows(5).enumerate() {
        if candidate[0] == 0xA9
            && candidate[2..]
                == [
                    0x8D,
                    TITLE_ANIMATION_STATE_ADDRESS as u8,
                    (TITLE_ANIMATION_STATE_ADDRESS >> 8) as u8,
                ]
        {
            values.insert(candidate[1]);
            let writer = origin
                .checked_add(u16::try_from(offset)?)
                .and_then(|address| address.checked_add(2))
                .context("title animation immediate writer address overflow")?;
            writer_starts.insert(writer);
        }
    }
    for (offset, candidate) in bytes.windows(8).enumerate() {
        if candidate[0] == 0xA9
            && candidate[2] == 0x8D
            && candidate[5..]
                == [
                    0x8D,
                    TITLE_ANIMATION_STATE_ADDRESS as u8,
                    (TITLE_ANIMATION_STATE_ADDRESS >> 8) as u8,
                ]
        {
            values.insert(candidate[1]);
            let writer = origin
                .checked_add(u16::try_from(offset)?)
                .and_then(|address| address.checked_add(5))
                .context("title animation propagated immediate writer address overflow")?;
            writer_starts.insert(writer);
        }
    }
    Ok((values, writer_starts))
}

fn immediate_staging_writes(bytes: &[u8]) -> BTreeSet<u8> {
    bytes
        .windows(4)
        .filter_map(|candidate| {
            (candidate[0] == 0xA9
                && candidate[2] == 0x85
                && candidate[3] == TITLE_ANIMATION_STAGING_ADDRESS)
                .then_some(candidate[1])
        })
        .collect()
}

fn staged_state_writes(bytes: &[u8], origin: u16) -> Result<BTreeSet<u16>> {
    let mut writers = BTreeSet::new();
    for (offset, candidate) in bytes.windows(5).enumerate() {
        if candidate
            != [
                0xA5,
                TITLE_ANIMATION_STAGING_ADDRESS,
                0x8D,
                TITLE_ANIMATION_STATE_ADDRESS as u8,
                (TITLE_ANIMATION_STATE_ADDRESS >> 8) as u8,
            ]
        {
            continue;
        }
        let writer = origin
            .checked_add(u16::try_from(offset)?)
            .and_then(|address| address.checked_add(2))
            .context("title animation staged writer address overflow")?;
        writers.insert(writer);
    }
    Ok(writers)
}

fn ensure_direct_writer_ownership(
    actual: &BTreeSet<(u8, u16)>,
    supported: &BTreeSet<(u8, u16)>,
) -> Result<()> {
    ensure!(
        actual == supported,
        "title animation state has a documented direct writer outside its source-bound producers"
    );
    Ok(())
}

fn ensure_selector_domain_matches_handlers(
    selector_domain: &BTreeSet<u8>,
    handler_count: usize,
) -> Result<()> {
    ensure!(
        selector_domain == &(0..u8::try_from(handler_count)?).collect::<BTreeSet<_>>(),
        "title animation producers no longer cover exactly the state handler table"
    );
    Ok(())
}

fn documented_direct_state_writers_in_source(source: &Rom) -> Result<BTreeSet<(u8, u16)>> {
    let mut writers = BTreeSet::new();
    for physical_bank in 0..16_u8 {
        let bank_start = usize::from(physical_bank) * SOURCE_PRG_BANK_BYTE_COUNT;
        let bank = &source.prg()[bank_start..bank_start + SOURCE_PRG_BANK_BYTE_COUNT];
        let cpu_start = if physical_bank == 0x0F {
            0xC000
        } else {
            SWITCHABLE_CPU_START
        };
        for offset in 0..bank.len().saturating_sub(2) {
            let Ok(instruction) = decode_bytes(&bank[offset..offset + 3]) else {
                continue;
            };
            if !instruction.opcode_is_documented() {
                continue;
            }
            let address = cpu_start + u16::try_from(offset)?;
            let semantics = Rp2A03::semantics(&instruction, &address)
                .expect("RP2A03 static semantics are infallible");
            if semantics.location_accesses.into_iter().any(|access| {
                access.kind == AccessKind::Write
                    && matches!(
                        access.location,
                        Location::Memory(MemoryAddress::Direct(target))
                            if target == TITLE_ANIMATION_STATE_ADDRESS
                    )
            }) {
                writers.insert((physical_bank, address));
            }
        }
    }
    Ok(writers)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn state_domain_is_derived_from_direct_and_staged_transitions() {
        let handlers = [
            0xA9, 0x00, 0x8D, 0x87, 0x05, 0xA9, 0x04, 0x8D, 0x87, 0x05, 0xA9, 0x03, 0x85, 0x0A,
            0xA5, 0x0A, 0x8D, 0x87, 0x05, 0xA9, 0x02, 0x8D, 0x83, 0x05, 0x8D, 0x87, 0x05,
        ];
        let (direct, writers) = immediate_state_writes(&handlers, 0xA000).unwrap();
        let staged = immediate_staging_writes(&handlers);
        let staged_writers = staged_state_writes(&handlers, 0xA000).unwrap();

        assert_eq!(direct, BTreeSet::from([0x00, 0x02, 0x04]));
        assert_eq!(staged, BTreeSet::from([0x03]));
        assert_eq!(writers, BTreeSet::from([0xA002, 0xA007, 0xA018]));
        assert_eq!(staged_writers, BTreeSet::from([0xA010]));
    }

    #[test]
    fn a_direct_writer_without_a_supported_producer_is_not_owned() {
        let supported = BTreeSet::from([(0x0D, 0xA002), (0x0D, 0xA010)]);
        let actual = BTreeSet::from([(0x0D, 0xA002), (0x0D, 0xA010), (0x06, 0xA020)]);

        let error = ensure_direct_writer_ownership(&actual, &supported).unwrap_err();

        assert!(
            error
                .to_string()
                .contains("outside its source-bound producers")
        );
    }

    #[test]
    fn a_producer_value_without_a_handler_is_rejected() {
        let error = ensure_selector_domain_matches_handlers(&BTreeSet::from([0x00, 0x01, 0x05]), 5)
            .unwrap_err();

        assert!(error.to_string().contains("state handler table"));
    }
}
