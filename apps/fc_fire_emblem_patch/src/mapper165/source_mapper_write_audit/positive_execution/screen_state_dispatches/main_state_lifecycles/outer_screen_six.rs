use std::collections::BTreeSet;

use anyhow::{Context, Result, ensure};

use crate::{rom::Rom, sha1_hex};

use super::super::selector_transition_graph::{StateTransition, reachable_selectors};
use super::{MAIN_STATE_ADDRESS, bind_exact_code, source_bytes};

const DEFERRED_MAIN_STATE_ADDRESS: u16 = 0x0026;
const ENTRY_SELECTOR: u8 = 0x08;
const GAMEPLAY_DEFERRED_MAIN_STATE_SELECTORS: [u8; 4] = [0x00, 0x04, 0x0C, 0x13];

pub(super) struct BoundOuterScreenSixMainStateProducers {
    main_state_selectors: BTreeSet<u8>,
    deferred_main_state_selectors: BTreeSet<u8>,
}

impl BoundOuterScreenSixMainStateProducers {
    pub(super) fn main_state_selectors(&self) -> &BTreeSet<u8> {
        &self.main_state_selectors
    }

    pub(super) fn deferred_main_state_selectors(&self) -> &BTreeSet<u8> {
        &self.deferred_main_state_selectors
    }
}

#[derive(Clone, Copy)]
struct SourceRegion {
    bank: u8,
    start: u16,
    end: u16,
    sha1: &'static str,
    role: &'static str,
}

const SOURCE_REGIONS: [SourceRegion; 5] = [
    region(
        0x06,
        0x85BD,
        0x8890,
        "537232e4739f6afc90693e2dca27f9b00f6cb326",
        "outer-screen state-six main-state dispatcher and handlers",
    ),
    region(
        0x06,
        0x8A92,
        0x8AED,
        "b2c78e74ee11844fc83fc039f3b2b78c01b84465",
        "outer-screen state-six unit-list transitions",
    ),
    region(
        0x06,
        0xAEF6,
        0xAF30,
        "cdbcc0beb1f13810d3cda809404755ef949bfb44",
        "outer-screen state-six late command transitions",
    ),
    region(
        0x06,
        0xAF66,
        0xAF88,
        "642edd5d663311024e5f9880aa942feafd04b553",
        "outer-screen state-six deferred return",
    ),
    region(
        0x0B,
        0x9383,
        0x9389,
        "fb6b84c1b6fcc6bec4b0128618e173f7ecd56821",
        "shared menu deferred return",
    ),
];

const fn region(
    bank: u8,
    start: u16,
    end: u16,
    sha1: &'static str,
    role: &'static str,
) -> SourceRegion {
    SourceRegion {
        bank,
        start,
        end,
        sha1,
        role,
    }
}

struct CodeBinding {
    bank: u8,
    address: u16,
    bytes: &'static [u8],
    role: &'static str,
}

const CODE_BINDINGS: [CodeBinding; 25] = [
    code(
        0x06,
        0x8594,
        &[0xA9, 0x06, 0x85, 0x24, 0xA9, 0x08, 0x85, 0x84],
        "enter outer-screen state six",
    ),
    code(0x06, 0x8AD2, &[0xE6, 0x84], "advance unit-list setup"),
    code(0x06, 0x865A, &[0xE6, 0x84], "advance selected-unit setup"),
    code(
        0x06,
        0x8674,
        &[0xA9, 0x08, 0x85, 0x84],
        "return selected-unit setup to input",
    ),
    code(
        0x06,
        0x861A,
        &[0xA9, 0x04, 0x85, 0x26],
        "defer the map-dialogue return state",
    ),
    code(
        0x0B,
        0x9383,
        &[0xA5, 0x26, 0xF0, 0x02, 0x85, 0x84],
        "apply a shared-menu deferred return state",
    ),
    code(
        0x06,
        0x869F,
        &[0xA9, 0x0D, 0x85, 0x84],
        "select the unit-toggle late state",
    ),
    code(0x06, 0x86A5, &[0xE6, 0x84], "advance unit-toggle setup"),
    code(
        0x06,
        0x86A8,
        &[0xE6, 0x84],
        "advance unit-toggle composition",
    ),
    code(
        0x06,
        0x86BE,
        &[0xE6, 0x84],
        "advance unit-toggle first dialogue",
    ),
    code(
        0x06,
        0x86C8,
        &[0xE6, 0x84],
        "advance unit-toggle second dialogue",
    ),
    code(
        0x06,
        0x8AEB,
        &[0xE6, 0x84],
        "return unit-toggle flow to input",
    ),
    code(
        0x06,
        0x8782,
        &[0xC6, 0x84],
        "return input to unit-toggle state",
    ),
    code(0x06, 0x87BC, &[0xE6, 0x84], "advance unit input to close"),
    code(
        0x06,
        0x8702,
        &[0xA9, 0x0C, 0x85, 0x26, 0xA9, 0x0B, 0x85, 0x84],
        "enter command dialogue with state 0C deferred",
    ),
    code(
        0x06,
        0x870B,
        &[0xA9, 0x0A, 0x85, 0x84],
        "select unit action state 0A",
    ),
    code(
        0x06,
        0x8806,
        &[0xA9, 0x00, 0x8D, 0xCE, 0x05, 0x85, 0x84, 0xE6, 0x24],
        "close outer-screen state six",
    ),
    code(
        0x06,
        0x881A,
        &[0xA9, 0x07, 0x85, 0x84],
        "return unit action to toggle state",
    ),
    code(
        0x06,
        0x8824,
        &[0xA9, 0x03, 0x85, 0x84],
        "return command dialogue to unit input setup",
    ),
    code(
        0x06,
        0x8829,
        &[0xE6, 0x84],
        "advance command animation state 0D",
    ),
    code(
        0x06,
        0x883C,
        &[0xE6, 0x84],
        "advance command animation state 0E",
    ),
    code(
        0x06,
        0x8855,
        &[0xE6, 0x84],
        "advance command animation state 0F",
    ),
    code(
        0x06,
        0x8866,
        &[0xA9, 0x13, 0x85, 0x26, 0xA9, 0x0B, 0x85, 0x84],
        "enter command dialogue with state 13 deferred",
    ),
    code(0x06, 0xAF1B, &[0xE6, 0x84], "advance late command state 10"),
    code(0x06, 0xAF2D, &[0xE6, 0x84], "advance late command state 11"),
];

const EXTRA_CODE_BINDINGS: [CodeBinding; 2] = [
    code(
        0x06,
        0x888A,
        &[0xA9, 0x03, 0x85, 0x84],
        "return deferred command state to input setup",
    ),
    code(
        0x06,
        0xAF79,
        &[
            0xA5, 0x26, 0x85, 0x84, 0xA9, 0x00, 0x8D, 0xED, 0x05, 0x85, 0x26,
        ],
        "consume and clear the deferred command state",
    ),
];

const fn code(bank: u8, address: u16, bytes: &'static [u8], role: &'static str) -> CodeBinding {
    CodeBinding {
        bank,
        address,
        bytes,
        role,
    }
}

const TRANSITIONS: [StateTransition; 27] = [
    edge(0x00, 0x01),
    edge(0x01, 0x02),
    edge(0x01, 0x08),
    edge(0x02, 0x04),
    edge(0x03, 0x04),
    edge(0x03, 0x0D),
    edge(0x04, 0x05),
    edge(0x05, 0x06),
    edge(0x06, 0x07),
    edge(0x07, 0x08),
    edge(0x08, 0x07),
    edge(0x08, 0x09),
    edge(0x08, 0x0A),
    edge(0x08, 0x0B),
    edge(0x09, 0x00),
    edge(0x0A, 0x07),
    edge(0x0B, 0x0C),
    edge(0x0B, 0x13),
    edge(0x0C, 0x03),
    edge(0x0D, 0x0E),
    edge(0x0E, 0x0F),
    edge(0x0F, 0x10),
    edge(0x0F, 0x0B),
    edge(0x10, 0x11),
    edge(0x11, 0x12),
    edge(0x12, 0x0B),
    edge(0x13, 0x03),
];

const fn edge(from: u8, to: u8) -> StateTransition {
    StateTransition::new(from, to)
}

const EXPECTED_DIRECT_STATE_OPERANDS: [(u8, u16, u16, u8); 29] = [
    (0x06, 0x861C, 0x0026, 0x85),
    (0x06, 0x865A, 0x0084, 0xE6),
    (0x06, 0x8676, 0x0084, 0x85),
    (0x06, 0x86A1, 0x0084, 0x85),
    (0x06, 0x86A5, 0x0084, 0xE6),
    (0x06, 0x86A8, 0x0084, 0xE6),
    (0x06, 0x86BE, 0x0084, 0xE6),
    (0x06, 0x86C8, 0x0084, 0xE6),
    (0x06, 0x8704, 0x0026, 0x85),
    (0x06, 0x8708, 0x0084, 0x85),
    (0x06, 0x870D, 0x0084, 0x85),
    (0x06, 0x8782, 0x0084, 0xC6),
    (0x06, 0x87BC, 0x0084, 0xE6),
    (0x06, 0x880B, 0x0084, 0x85),
    (0x06, 0x881C, 0x0084, 0x85),
    (0x06, 0x8826, 0x0084, 0x85),
    (0x06, 0x8829, 0x0084, 0xE6),
    (0x06, 0x883C, 0x0084, 0xE6),
    (0x06, 0x8855, 0x0084, 0xE6),
    (0x06, 0x8868, 0x0026, 0x85),
    (0x06, 0x886C, 0x0084, 0x85),
    (0x06, 0x888C, 0x0084, 0x85),
    (0x06, 0x8AD2, 0x0084, 0xE6),
    (0x06, 0x8AEB, 0x0084, 0xE6),
    (0x06, 0xAF1B, 0x0084, 0xE6),
    (0x06, 0xAF2D, 0x0084, 0xE6),
    (0x06, 0xAF7B, 0x0084, 0x85),
    (0x06, 0xAF82, 0x0026, 0x85),
    (0x0B, 0x9387, 0x0084, 0x85),
];

pub(super) fn bind_outer_screen_six_main_state_producers(
    source: &Rom,
    handler_domain: &BTreeSet<u8>,
) -> Result<BoundOuterScreenSixMainStateProducers> {
    source.verify_supported_japanese()?;
    for region in SOURCE_REGIONS {
        let bytes = source_bytes(
            source,
            region.bank,
            region.start,
            usize::from(region.end - region.start),
        )?;
        ensure!(
            sha1_hex(bytes) == region.sha1,
            "{} source bytes changed",
            region.role
        );
    }
    for binding in CODE_BINDINGS.iter().chain(EXTRA_CODE_BINDINGS.iter()) {
        bind_exact_code(
            source,
            binding.bank,
            binding.address,
            binding.bytes,
            binding.role,
        )?;
    }
    ensure!(
        scan_direct_state_operands(source)? == BTreeSet::from(EXPECTED_DIRECT_STATE_OPERANDS),
        "outer-screen state-six direct main/deferred-state operand census changed"
    );
    let produced = reachable_selectors(
        "outer-screen state-six main state",
        handler_domain,
        [ENTRY_SELECTOR],
        TRANSITIONS,
    )?;
    ensure!(
        produced == *handler_domain,
        "outer-screen state-six transition graph no longer reaches every owned handler"
    );
    Ok(BoundOuterScreenSixMainStateProducers {
        main_state_selectors: produced,
        deferred_main_state_selectors: BTreeSet::from(GAMEPLAY_DEFERRED_MAIN_STATE_SELECTORS),
    })
}

fn scan_direct_state_operands(source: &Rom) -> Result<BTreeSet<(u8, u16, u16, u8)>> {
    const DIRECT_WRITE_OPCODES: [u8; 9] = [0x06, 0x26, 0x46, 0x66, 0x84, 0x85, 0x86, 0xC6, 0xE6];
    let mut candidates = BTreeSet::new();
    for region in SOURCE_REGIONS {
        let bytes = source_bytes(
            source,
            region.bank,
            region.start,
            usize::from(region.end - region.start),
        )?;
        for (offset, window) in bytes.windows(2).enumerate() {
            let target = u16::from(window[1]);
            if !DIRECT_WRITE_OPCODES.contains(&window[0])
                || ![DEFERRED_MAIN_STATE_ADDRESS, MAIN_STATE_ADDRESS].contains(&target)
            {
                continue;
            }
            candidates.insert((
                region.bank,
                region
                    .start
                    .checked_add(u16::try_from(offset)?)
                    .context("outer-screen state-six writer address overflow")?,
                target,
                window[0],
            ));
        }
    }
    Ok(candidates)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn entry_and_transition_graph_cover_the_complete_owned_state_machine() {
        let handlers = (0..0x14).collect::<BTreeSet<_>>();
        assert_eq!(
            reachable_selectors(
                "outer-screen state-six main state",
                &handlers,
                [ENTRY_SELECTOR],
                TRANSITIONS,
            )
            .unwrap(),
            handlers
        );
    }
}
