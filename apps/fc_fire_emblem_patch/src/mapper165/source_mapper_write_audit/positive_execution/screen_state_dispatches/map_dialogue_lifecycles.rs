use std::collections::BTreeSet;

use anyhow::{Context, Result, ensure};

use crate::{
    mapper165::inline_pointer_dispatch::bind_inline_pointer_dispatch, rom::Rom, sha1_hex,
    typed_source::decode_rp2a03_sequence,
};

const SOURCE_PRG_BANK_BYTE_COUNT: usize = 16 * 1024;
const FIXED_PRG_BANK: u8 = 0x0F;
const OUTER_SCREEN_BANK: u8 = 0x06;
const MAP_DIALOGUE_STATE_ADDRESS: u16 = 0x05DB;
const DISPATCH_CALL: u16 = 0x85F7;
const DISPATCH_TARGETS: [u16; 3] = [0x8600, 0x8608, 0xA122];
const LIFECYCLE_START: u16 = 0x85F4;
const LIFECYCLE_END: u16 = 0x8627;
const LIFECYCLE_SHA1: &str = "67c2a68c949db1221deda429ada98545f0c1ae9b";
const INITIALIZE_SCREEN_STATES: &[u8] = &[
    0xA9, 0x00, 0x85, 0x23, 0x85, 0x24, 0x85, 0x84, 0x85, 0x26, 0x8D, 0xDB, 0x05,
];
const DISPATCH_PREFIX: &[u8] = &[0xAD, 0xDB, 0x05, 0x20, 0x4C, 0xC3];
const ADVANCE_TO_DIALOGUE_STATE_ONE: &[u8] = &[0xEE, 0xDB, 0x05, 0xA9, 0x25, 0x4C, 0x90, 0xE6];
const ADVANCE_TO_DIALOGUE_STATE_TWO: &[u8] = &[
    0x20, 0x5C, 0xE6, 0xA9, 0x00, 0x8D, 0xF0, 0x77, 0xA9, 0xB1, 0x8D, 0xF4, 0x77, 0xA9, 0x01, 0x8D,
    0xF7, 0x77, 0xA9, 0x04, 0x85, 0x26, 0xA9, 0x52, 0x8D, 0xF1, 0x77, 0xEE, 0xDB, 0x05, 0x60,
];

pub(super) struct MapDialogueLifecycle {
    pub(super) dispatch_call: u16,
    pub(super) handler_domain: BTreeSet<u8>,
    pub(super) produced_selectors: BTreeSet<u8>,
}

pub(super) fn bind_outer_screen_map_dialogue_lifecycle(
    source: &Rom,
) -> Result<MapDialogueLifecycle> {
    source.verify_supported_japanese()?;
    bind_exact_code(
        source,
        FIXED_PRG_BANK,
        0xF302,
        INITIALIZE_SCREEN_STATES,
        "initialize map-dialogue state with the outer screen",
    )?;
    let lifecycle = source_bytes(
        source,
        OUTER_SCREEN_BANK,
        LIFECYCLE_START,
        usize::from(LIFECYCLE_END - LIFECYCLE_START),
    )?;
    ensure!(
        sha1_hex(lifecycle) == LIFECYCLE_SHA1,
        "outer-screen map-dialogue lifecycle digest changed"
    );
    bind_exact_code(
        source,
        OUTER_SCREEN_BANK,
        LIFECYCLE_START,
        DISPATCH_PREFIX,
        "dispatch outer-screen map-dialogue state",
    )?;
    bind_exact_code(
        source,
        OUTER_SCREEN_BANK,
        0x8600,
        ADVANCE_TO_DIALOGUE_STATE_ONE,
        "advance outer-screen map dialogue to state one",
    )?;
    bind_exact_code(
        source,
        OUTER_SCREEN_BANK,
        0x8608,
        ADVANCE_TO_DIALOGUE_STATE_TWO,
        "advance outer-screen map dialogue to caller handoff",
    )?;
    ensure!(
        scan_absolute_state_writers(lifecycle, LIFECYCLE_START)?
            == BTreeSet::from([(0x8600, 0xEE), (0x8623, 0xEE)]),
        "outer-screen map-dialogue lifecycle changed its direct writer census"
    );

    let handler_domain = (0..u8::try_from(DISPATCH_TARGETS.len())?).collect::<BTreeSet<_>>();
    let dispatch = bind_inline_pointer_dispatch(
        source,
        OUTER_SCREEN_BANK,
        DISPATCH_CALL,
        handler_domain.iter().copied(),
        "outer-screen map-dialogue state dispatch",
    )?;
    ensure!(
        dispatch.targets_in_selector_order() == DISPATCH_TARGETS,
        "outer-screen map-dialogue handlers changed"
    );
    let produced_selectors = BTreeSet::from([0x00, 0x01, 0x02]);
    ensure!(
        produced_selectors == handler_domain,
        "outer-screen map-dialogue producer closure left its handler table"
    );
    Ok(MapDialogueLifecycle {
        dispatch_call: DISPATCH_CALL,
        handler_domain,
        produced_selectors,
    })
}

fn scan_absolute_state_writers(bytes: &[u8], start: u16) -> Result<BTreeSet<(u16, u8)>> {
    let direct_write_opcodes = [0x0E, 0x2E, 0x4E, 0x6E, 0x8C, 0x8D, 0x8E, 0xCE, 0xEE];
    bytes
        .windows(3)
        .enumerate()
        .filter_map(|(offset, window)| {
            (direct_write_opcodes.contains(&window[0])
                && u16::from_le_bytes([window[1], window[2]]) == MAP_DIALOGUE_STATE_ADDRESS)
                .then(|| {
                    Ok((
                        start
                            .checked_add(u16::try_from(offset)?)
                            .context("map-dialogue writer address overflow")?,
                        window[0],
                    ))
                })
        })
        .collect()
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
        "map-dialogue source address is outside its PRG window"
    );
    let start = usize::from(bank)
        .checked_mul(SOURCE_PRG_BANK_BYTE_COUNT)
        .and_then(|base| base.checked_add(usize::from(address - window_start)))
        .context("map-dialogue source offset overflow")?;
    source
        .prg()
        .get(start..start + byte_count)
        .context("map-dialogue source range exceeds PRG")
}
