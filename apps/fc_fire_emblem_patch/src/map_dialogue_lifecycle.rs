//! 지도 위 고정 오버레이에서 주 대사로 넘어가는 원본 수명을 결속한다.
//!
//! mapper 실행 그래프와 한글 글꼴 residency가 같은 상태기와 같은 대사 정체성을
//! 소비한다. 한쪽에서 주소를 다시 열거하면 화면 전환은 맞아도 두 글꼴 페이지가
//! 서로 다른 수명을 모델링할 수 있으므로 이 모듈이 원천 결속을 한 번만 소유한다.

use std::collections::BTreeSet;

use anyhow::{Context, Result, ensure};

use crate::{
    mapper165::inline_pointer_dispatch::bind_inline_pointer_dispatch,
    rom::Rom,
    sha1_hex,
    source_direct_memory_writers::{DirectMemoryWriter, scan_direct_memory_writers},
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
const ADVANCE_TO_HELP_OVERLAY: &[u8] = &[0xEE, 0xDB, 0x05, 0xA9, 0x25, 0x4C, 0x90, 0xE6];
const ADVANCE_TO_HELP_DIALOGUE: &[u8] = &[
    0x20, 0x5C, 0xE6, 0xA9, 0x00, 0x8D, 0xF0, 0x77, 0xA9, 0xB1, 0x8D, 0xF4, 0x77, 0xA9, 0x01, 0x8D,
    0xF7, 0x77, 0xA9, 0x04, 0x85, 0x26, 0xA9, 0x52, 0x8D, 0xF1, 0x77, 0xEE, 0xDB, 0x05, 0x60,
];
const HELP_DIALOGUE_DIRECTORY_SELECTOR: u8 = ADVANCE_TO_HELP_DIALOGUE[9];
const HELP_DIALOGUE_ENTRY_INDEX: u8 = ADVANCE_TO_HELP_DIALOGUE[23];

// The outer screen reaches the help/dialogue lifecycle through one source-owned
// deployment decision. Keep every producer that writes the reused $05EA byte in
// this owner: bank 06 first publishes a unit-status count, then bank 08 replaces
// it with the current chapter's deployment limit before bank 06 compares it with
// the selectable-unit count.
const UNIT_SELECTION_LIMIT_ADDRESS: u16 = 0x05EA;
const UNIT_STATUS_COUNT_SCAN_START: u16 = 0xB8B3;
const UNIT_STATUS_COUNT_SCAN_END: u16 = 0xB8DE;
const UNIT_STATUS_COUNT_SCAN_SHA1: &str = "9aa4938449a1a6e64f348e65d1b28d778ae9559d";
const UNIT_STATUS_COUNT_PUBLISH_START: u16 = 0x8905;
const UNIT_STATUS_COUNT_PUBLISH_END: u16 = 0x8919;
const UNIT_STATUS_COUNT_PUBLISH_SHA1: &str = "0651f581279713e041226fc28cb3956e89954c4e";
const CHAPTER_DEPLOYMENT_BANK: u8 = 0x08;
const CHAPTER_DEPLOYMENT_LIMIT_LOAD_START: u16 = 0xBA7A;
const CHAPTER_DEPLOYMENT_LIMIT_LOAD_END: u16 = 0xBA93;
const CHAPTER_DEPLOYMENT_LIMIT_LOAD_SHA1: &str = "dbc671678c8bd55982a49c3e5b42b35308ca23b1";
const CHAPTER_DEPLOYMENT_POINTER_TABLE: u16 = 0x8790;
const CHAPTER_COUNT: usize = 25;
const CHAPTER_DEPLOYMENT_POINTER_TABLE_SHA1: &str = "60e866d62625abf92c92365ac27cc8226a9a6960";
const CHAPTER_DEPLOYMENT_LIMITS: [u8; CHAPTER_COUNT] = [
    0x08, 0x08, 0x0E, 0x0E, 0x0A, 0x0E, 0x0E, 0x0D, 0x0F, 0x0E, 0x0E, 0x0B, 0x0E, 0x0E, 0x10, 0x0E,
    0x10, 0x0F, 0x0C, 0x0F, 0x10, 0x10, 0x0C, 0x0F, 0x0F,
];
const UNIT_SELECTION_LIST_START: u16 = 0x8A92;
const UNIT_SELECTION_LIST_END: u16 = 0x8AD5;
const UNIT_SELECTION_LIST_SHA1: &str = "a849935a36bdd48f4a0aa533ae0d17914585376d";
const UNIT_SELECTION_DECISION_START: u16 = 0x8627;
const UNIT_SELECTION_DECISION_END: u16 = 0x867B;
const UNIT_SELECTION_DECISION_SHA1: &str = "3abe708d3d1fd560b99487dfb60aa78dae7d3bd4";
const UNIT_SELECTION_LIMIT_WRITERS: [DirectMemoryWriter; 15] = [
    unit_selection_limit_writer(0x02, 0xAA07, 0xCE),
    unit_selection_limit_writer(0x06, 0x863E, 0x8D),
    unit_selection_limit_writer(0x06, 0x8657, 0xCE),
    unit_selection_limit_writer(0x06, 0x866F, 0xCE),
    unit_selection_limit_writer(0x06, 0x877A, 0xCE),
    unit_selection_limit_writer(0x06, 0x877F, 0xEE),
    unit_selection_limit_writer(0x06, 0x8916, 0x8D),
    unit_selection_limit_writer(0x07, 0x8B53, 0xEE),
    unit_selection_limit_writer(0x08, 0xA084, 0xEE),
    unit_selection_limit_writer(0x08, 0xB66F, 0xEE),
    unit_selection_limit_writer(0x08, 0xB738, 0xEE),
    unit_selection_limit_writer(0x08, 0xBA8F, 0x8D),
    unit_selection_limit_writer(0x0C, 0x85AF, 0xEE),
    unit_selection_limit_writer(0x0C, 0x90D3, 0xEE),
    unit_selection_limit_writer(0x0C, 0xB376, 0xEE),
];

const fn unit_selection_limit_writer(bank: u8, cpu_address: u16, opcode: u8) -> DirectMemoryWriter {
    DirectMemoryWriter::new(bank, cpu_address, opcode, UNIT_SELECTION_LIMIT_ADDRESS)
}

pub(crate) struct MapDialogueLifecycle {
    dispatch_call: u16,
    handler_domain: BTreeSet<u8>,
    produced_selectors: BTreeSet<u8>,
}

impl MapDialogueLifecycle {
    pub(crate) fn dispatch_call(&self) -> u16 {
        self.dispatch_call
    }

    pub(crate) fn handler_domain(&self) -> &BTreeSet<u8> {
        &self.handler_domain
    }

    pub(crate) fn produced_selectors(&self) -> &BTreeSet<u8> {
        &self.produced_selectors
    }

    pub(crate) fn help_dialogue_directory_selector(&self) -> u8 {
        HELP_DIALOGUE_DIRECTORY_SELECTOR
    }

    pub(crate) fn help_dialogue_entry_index(&self) -> usize {
        usize::from(HELP_DIALOGUE_ENTRY_INDEX)
    }
}

pub(crate) fn bind_outer_screen_map_dialogue_lifecycle(
    source: &Rom,
) -> Result<MapDialogueLifecycle> {
    source.verify_supported_japanese()?;
    bind_natural_unit_selection_entry(source)?;
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
        ADVANCE_TO_HELP_OVERLAY,
        "advance outer-screen map dialogue to the fixed help overlay",
    )?;
    bind_exact_code(
        source,
        OUTER_SCREEN_BANK,
        0x8608,
        ADVANCE_TO_HELP_DIALOGUE,
        "advance outer-screen map dialogue from the help overlay to its dialogue",
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

fn bind_natural_unit_selection_entry(source: &Rom) -> Result<()> {
    let actual_writers = scan_direct_memory_writers(source.prg(), &[UNIT_SELECTION_LIMIT_ADDRESS])?;
    let expected_writers = UNIT_SELECTION_LIMIT_WRITERS.into_iter().collect();
    ensure!(
        actual_writers == expected_writers,
        "unit-selection limit writer census changed: expected {expected_writers:?}, found {actual_writers:?}"
    );

    // 8905's unit-status count is an earlier use of $05EA, not the value consumed
    // by the decision. Bank 08:BA7A selects the chapter record through 8790 and
    // replaces $05EA with that record's first byte. State one at 8627 enters state
    // two only when $776C is strictly greater than this deployment limit; the
    // equal-or-smaller branch selects every candidate and advances to state eight.
    for (bank, start, end, expected_sha1, role) in [
        (
            OUTER_SCREEN_BANK,
            UNIT_STATUS_COUNT_SCAN_START,
            UNIT_STATUS_COUNT_SCAN_END,
            UNIT_STATUS_COUNT_SCAN_SHA1,
            "count source unit-status classes before loading the chapter deployment limit",
        ),
        (
            OUTER_SCREEN_BANK,
            UNIT_STATUS_COUNT_PUBLISH_START,
            UNIT_STATUS_COUNT_PUBLISH_END,
            UNIT_STATUS_COUNT_PUBLISH_SHA1,
            "publish the earlier unit-status count",
        ),
        (
            CHAPTER_DEPLOYMENT_BANK,
            CHAPTER_DEPLOYMENT_LIMIT_LOAD_START,
            CHAPTER_DEPLOYMENT_LIMIT_LOAD_END,
            CHAPTER_DEPLOYMENT_LIMIT_LOAD_SHA1,
            "load the current chapter deployment limit",
        ),
        (
            OUTER_SCREEN_BANK,
            UNIT_SELECTION_LIST_START,
            UNIT_SELECTION_LIST_END,
            UNIT_SELECTION_LIST_SHA1,
            "build and count the selectable-unit list",
        ),
        (
            OUTER_SCREEN_BANK,
            UNIT_SELECTION_DECISION_START,
            UNIT_SELECTION_DECISION_END,
            UNIT_SELECTION_DECISION_SHA1,
            "choose automatic or manual unit selection",
        ),
    ] {
        let bytes = source_bytes(source, bank, start, usize::from(end - start))?;
        ensure!(
            sha1_hex(bytes) == expected_sha1,
            "{role} source digest changed"
        );
        decode_rp2a03_sequence(bytes, start, role)?;
    }
    bind_chapter_deployment_limits(source)?;
    Ok(())
}

fn bind_chapter_deployment_limits(source: &Rom) -> Result<()> {
    let table = source_bytes(
        source,
        CHAPTER_DEPLOYMENT_BANK,
        CHAPTER_DEPLOYMENT_POINTER_TABLE,
        CHAPTER_COUNT * 2,
    )?;
    ensure!(
        sha1_hex(table) == CHAPTER_DEPLOYMENT_POINTER_TABLE_SHA1,
        "chapter deployment pointer table changed"
    );
    let pointers = table
        .chunks_exact(2)
        .map(|bytes| u16::from_le_bytes([bytes[0], bytes[1]]))
        .collect::<Vec<_>>();
    let table_end = CHAPTER_DEPLOYMENT_POINTER_TABLE
        .checked_add(u16::try_from(CHAPTER_COUNT * 2)?)
        .context("chapter deployment pointer table end overflow")?;
    ensure!(
        pointers
            .iter()
            .all(|pointer| *pointer >= table_end && *pointer < 0xC000),
        "chapter deployment pointer left bank 08 data"
    );
    let limits = pointers
        .iter()
        .map(|pointer| {
            source_bytes(source, CHAPTER_DEPLOYMENT_BANK, *pointer, 1).map(|bytes| bytes[0])
        })
        .collect::<Result<Vec<_>>>()?;
    ensure!(
        limits.as_slice() == CHAPTER_DEPLOYMENT_LIMITS,
        "chapter deployment limits changed"
    );
    Ok(())
}

fn scan_absolute_state_writers(bytes: &[u8], start: u16) -> Result<BTreeSet<(u16, u8)>> {
    let direct_write_opcodes = [0x0E, 0x2E, 0x4E, 0x6E, 0x8C, 0x8D, 0x8E, 0xCE, 0xEE];
    let mut writers = BTreeSet::new();
    for (offset, window) in bytes.windows(3).enumerate() {
        if direct_write_opcodes.contains(&window[0])
            && u16::from_le_bytes([window[1], window[2]]) == MAP_DIALOGUE_STATE_ADDRESS
        {
            let address = start
                .checked_add(u16::try_from(offset)?)
                .context("map-dialogue writer address overflow")?;
            writers.insert((address, window[0]));
        }
    }
    Ok(writers)
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
