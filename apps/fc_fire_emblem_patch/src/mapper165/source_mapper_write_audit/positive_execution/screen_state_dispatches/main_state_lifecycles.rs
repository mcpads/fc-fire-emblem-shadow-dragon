use std::collections::BTreeSet;

use anyhow::{Context, Result, ensure};

use crate::{
    mapper165::inline_pointer_dispatch::bind_inline_pointer_dispatch, rom::Rom, sha1_hex,
    typed_source::decode_rp2a03_sequence,
};

mod chapter_save;

use chapter_save::bind_chapter_save_main_state_lifecycles;

const SOURCE_PRG_BANK_BYTE_COUNT: usize = 16 * 1024;
const FIXED_PRG_BANK: u8 = 0x0F;
const OUTER_SCREEN_BANK: u8 = 0x06;
const OUTER_SCREEN_STATE_ADDRESS: u16 = 0x0024;
const MAIN_STATE_ADDRESS: u16 = 0x0084;

const INITIALIZE_OUTER_AND_MAIN_STATE: (u16, &[u8]) = (
    0xF302,
    &[0xA9, 0x00, 0x85, 0x23, 0x85, 0x24, 0x85, 0x84, 0x85, 0x26],
);

const FIRST_LIFECYCLE_START: u16 = 0x8425;
const FIRST_LIFECYCLE_END: u16 = 0x84F6;
const FIRST_LIFECYCLE_SHA1: &str = "71af7c2a62bed101d623debd09e2af159c603410";
const SECOND_LIFECYCLE_START: u16 = 0x850B;
const SECOND_LIFECYCLE_END: u16 = 0x85BD;
const SECOND_LIFECYCLE_SHA1: &str = "d50b34ff8767ff2d30381e51e3e5471af520b1ef";

const STATE_TRANSITION_SEQUENCES: [(u16, &[u8]); 9] = [
    (0x8425, &[0x20, 0x8C, 0xB5, 0xE6, 0x24, 0x60]),
    (
        0x8447,
        &[0xA9, 0x09, 0x85, 0x24, 0xA9, 0x00, 0x85, 0x84, 0xF0, 0x4B],
    ),
    (0x8497, &[0x20, 0x7D, 0xC7, 0xE6, 0x24, 0x60]),
    (0x84BF, &[0xE6, 0x84, 0xD0, 0x02, 0xE6, 0x24, 0x60]),
    (0x850B, &[0x20, 0x85, 0xD3, 0xE6, 0x24, 0x60]),
    (
        0x8511,
        &[
            0xA9, 0x01, 0x8D, 0xED, 0x76, 0xA9, 0x03, 0x85, 0x44, 0xA9, 0x08, 0x20, 0xFA, 0xC9,
            0xA9, 0x00, 0x8D, 0xED, 0x76, 0xE6, 0x24, 0x60,
        ],
    ),
    (
        0x8530,
        &[
            0xA5, 0x63, 0x8D, 0x6F, 0x76, 0xA5, 0x64, 0x8D, 0x70, 0x76, 0xA0, 0x00, 0x84, 0x63,
            0x84, 0x64, 0x8C, 0x04, 0x05, 0xA9, 0x03, 0x85, 0x2F, 0xA9, 0x00, 0x8D, 0xE1, 0x05,
            0x20, 0x8B, 0xA7, 0xA9, 0x01, 0x8D, 0xF5, 0x06, 0xE6, 0x84, 0x60,
        ],
    ),
    (
        0x8594,
        &[
            0xA9, 0x06, 0x85, 0x24, 0xA9, 0x08, 0x85, 0x84, 0x4C, 0xB9, 0x87,
        ],
    ),
    (
        0x85AD,
        &[
            0xA9, 0x00, 0x85, 0x84, 0xE6, 0x24, 0xD0, 0x03, 0x20, 0x6B, 0xAA, 0x60,
        ],
    ),
];

const FIRST_RAW_STATE_OPERANDS: [(u16, u16, u8); 7] = [
    (0x8428, OUTER_SCREEN_STATE_ADDRESS, 0xE6),
    (0x8449, OUTER_SCREEN_STATE_ADDRESS, 0x85),
    (0x844D, MAIN_STATE_ADDRESS, 0x85),
    (0x849A, OUTER_SCREEN_STATE_ADDRESS, 0xE6),
    (0x84A4, MAIN_STATE_ADDRESS, 0xC6),
    (0x84BF, MAIN_STATE_ADDRESS, 0xE6),
    (0x84C3, OUTER_SCREEN_STATE_ADDRESS, 0xE6),
];
const SECOND_RAW_STATE_OPERANDS: [(u16, u16, u8); 7] = [
    (0x850E, OUTER_SCREEN_STATE_ADDRESS, 0xE6),
    (0x8524, OUTER_SCREEN_STATE_ADDRESS, 0xE6),
    (0x8554, MAIN_STATE_ADDRESS, 0xE6),
    (0x8596, OUTER_SCREEN_STATE_ADDRESS, 0x85),
    (0x859A, MAIN_STATE_ADDRESS, 0x85),
    (0x85AF, MAIN_STATE_ADDRESS, 0x85),
    (0x85B1, OUTER_SCREEN_STATE_ADDRESS, 0xE6),
];

const MAIN_STATE_DISPATCHES: [(u16, [u16; 2], &str); 2] = [
    (
        0x849F,
        [0x84A6, 0x84C6],
        "outer-screen state two main-state dispatch",
    ),
    (
        0x8529,
        [0x8530, 0x8557],
        "outer-screen state five main-state dispatch",
    ),
];
const OUTER_SCREEN_SIX_MAIN_STATE_DISPATCH_CALL: u16 = 0x85C9;
const OUTER_SCREEN_SIX_MAIN_STATE_TARGETS: [u16; 20] = [
    0x8A92, 0x8627, 0x85F4, 0x867B, 0x86A8, 0x86BB, 0x86C5, 0x86CF, 0x86D5, 0x87E1, 0x8810, 0xAF66,
    0x8824, 0x8829, 0x8839, 0x8846, 0xAEF6, 0xAF1E, 0x886F, 0x8887,
];

pub(super) struct NestedMainStateLifecycle {
    dispatch_call: u16,
    handler_domain: BTreeSet<u8>,
    produced_selectors: Option<BTreeSet<u8>>,
}

impl NestedMainStateLifecycle {
    pub(super) fn dispatch_call(&self) -> u16 {
        self.dispatch_call
    }

    pub(super) fn handler_domain(&self) -> &BTreeSet<u8> {
        &self.handler_domain
    }

    pub(super) fn produced_selectors(&self) -> Option<&BTreeSet<u8>> {
        self.produced_selectors.as_ref()
    }
}

pub(super) fn bind_outer_screen_main_state_lifecycles(
    source: &Rom,
) -> Result<Vec<NestedMainStateLifecycle>> {
    source.verify_supported_japanese()?;
    bind_exact_code(
        source,
        FIXED_PRG_BANK,
        INITIALIZE_OUTER_AND_MAIN_STATE.0,
        INITIALIZE_OUTER_AND_MAIN_STATE.1,
        "initialize outer-screen and nested main states",
    )?;
    bind_hashed_region(
        source,
        OUTER_SCREEN_BANK,
        FIRST_LIFECYCLE_START,
        FIRST_LIFECYCLE_END,
        FIRST_LIFECYCLE_SHA1,
        "outer-screen states zero through two",
    )?;
    bind_hashed_region(
        source,
        OUTER_SCREEN_BANK,
        SECOND_LIFECYCLE_START,
        SECOND_LIFECYCLE_END,
        SECOND_LIFECYCLE_SHA1,
        "outer-screen states three through five",
    )?;
    for &(address, bytes) in &STATE_TRANSITION_SEQUENCES {
        bind_exact_code(
            source,
            OUTER_SCREEN_BANK,
            address,
            bytes,
            "outer-screen and nested main-state transition",
        )?;
    }

    ensure!(
        scan_raw_direct_state_operands(
            source,
            OUTER_SCREEN_BANK,
            FIRST_LIFECYCLE_START,
            FIRST_LIFECYCLE_END,
        )? == BTreeSet::from(FIRST_RAW_STATE_OPERANDS),
        "outer-screen states zero through two changed their direct state-operand census"
    );
    ensure!(
        scan_raw_direct_state_operands(
            source,
            OUTER_SCREEN_BANK,
            SECOND_LIFECYCLE_START,
            SECOND_LIFECYCLE_END,
        )? == BTreeSet::from(SECOND_RAW_STATE_OPERANDS),
        "outer-screen states three through five changed their direct state-operand census"
    );

    let produced_selectors = BTreeSet::from([0x00, 0x01]);
    let mut lifecycles = Vec::new();
    for (dispatch_call, targets, role) in MAIN_STATE_DISPATCHES {
        let handler_domain = (0..u8::try_from(targets.len())?).collect::<BTreeSet<_>>();
        let dispatch = bind_inline_pointer_dispatch(
            source,
            OUTER_SCREEN_BANK,
            dispatch_call,
            handler_domain.iter().copied(),
            role,
        )?;
        ensure!(
            dispatch.targets_in_selector_order() == targets,
            "{role} handlers changed"
        );
        ensure!(
            produced_selectors.is_subset(&handler_domain),
            "{role} source producer escaped its handler table"
        );
        if dispatch_call == 0x849F {
            ensure!(
                dispatch.table_start() == 0x84A2
                    && FIRST_RAW_STATE_OPERANDS.contains(&(0x84A4, MAIN_STATE_ADDRESS, 0xC6)),
                "outer-screen state two table no longer owns its instruction-shaped data window"
            );
        }
        lifecycles.push(NestedMainStateLifecycle {
            dispatch_call,
            handler_domain,
            produced_selectors: Some(produced_selectors.clone()),
        });
    }
    let handler_domain =
        (0..u8::try_from(OUTER_SCREEN_SIX_MAIN_STATE_TARGETS.len())?).collect::<BTreeSet<_>>();
    let dispatch = bind_inline_pointer_dispatch(
        source,
        OUTER_SCREEN_BANK,
        OUTER_SCREEN_SIX_MAIN_STATE_DISPATCH_CALL,
        handler_domain.iter().copied(),
        "outer-screen state six main-state dispatch",
    )?;
    ensure!(
        dispatch.targets_in_selector_order() == OUTER_SCREEN_SIX_MAIN_STATE_TARGETS,
        "outer-screen state six main-state handlers changed"
    );
    lifecycles.push(NestedMainStateLifecycle {
        dispatch_call: OUTER_SCREEN_SIX_MAIN_STATE_DISPATCH_CALL,
        handler_domain,
        produced_selectors: None,
    });
    lifecycles.extend(bind_chapter_save_main_state_lifecycles(source)?);
    Ok(lifecycles)
}

fn scan_raw_direct_state_operands(
    source: &Rom,
    bank: u8,
    start: u16,
    end: u16,
) -> Result<BTreeSet<(u16, u16, u8)>> {
    let bytes = source_bytes(source, bank, start, usize::from(end - start))?;
    let direct_write_opcodes = [0x06, 0x26, 0x46, 0x66, 0x84, 0x85, 0x86, 0xC6, 0xE6];
    Ok(bytes
        .windows(2)
        .enumerate()
        .filter_map(|(offset, window)| {
            let target = u16::from(window[1]);
            (direct_write_opcodes.contains(&window[0])
                && [OUTER_SCREEN_STATE_ADDRESS, MAIN_STATE_ADDRESS].contains(&target))
            .then(|| {
                Ok((
                    start
                        .checked_add(u16::try_from(offset)?)
                        .context("state-operand candidate address overflow")?,
                    target,
                    window[0],
                ))
            })
        })
        .collect::<Result<_>>()?)
}

fn bind_exact_code(
    source: &Rom,
    bank: u8,
    address: u16,
    expected: &[u8],
    role: &str,
) -> Result<()> {
    let actual = source_bytes(source, bank, address, expected.len())?;
    ensure!(actual == expected, "{role} source bytes changed");
    decode_rp2a03_sequence(actual, address, role)?;
    Ok(())
}

fn bind_hashed_region(
    source: &Rom,
    bank: u8,
    start: u16,
    end: u16,
    expected_sha1: &str,
    role: &str,
) -> Result<()> {
    ensure!(end > start, "{role} source range is empty");
    let bytes = source_bytes(source, bank, start, usize::from(end - start))?;
    ensure!(
        sha1_hex(bytes) == expected_sha1,
        "{role} source digest changed"
    );
    Ok(())
}

fn source_bytes(source: &Rom, bank: u8, address: u16, byte_count: usize) -> Result<&[u8]> {
    let window_start = if bank == FIXED_PRG_BANK {
        0xC000
    } else {
        0x8000
    };
    let within_window = address >= window_start
        && usize::from(address - window_start)
            .checked_add(byte_count)
            .is_some_and(|end| end <= SOURCE_PRG_BANK_BYTE_COUNT);
    ensure!(
        bank <= FIXED_PRG_BANK && within_window,
        "nested main-state source address is outside its PRG window"
    );
    let start = usize::from(bank)
        .checked_mul(SOURCE_PRG_BANK_BYTE_COUNT)
        .and_then(|base| base.checked_add(usize::from(address - window_start)))
        .context("nested main-state source offset overflow")?;
    source
        .prg()
        .get(start..start + byte_count)
        .context("nested main-state source range exceeds PRG")
}
