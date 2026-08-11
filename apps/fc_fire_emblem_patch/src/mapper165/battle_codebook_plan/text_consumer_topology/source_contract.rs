use std::collections::BTreeSet;

use anyhow::{Context, Result, ensure};
use retro_rp2a03::{ControlFlow, decode_bytes};

use crate::{rom::Rom, sha1_hex, typed_source::decode_rp2a03_sequence};

use super::super::{
    phase_cooccurrence::UNIT_PANEL_PHASE_POINTERS,
    source_window::{prg_bank, source_bytes},
};

const COMMON_TEXT_RENDERER_BANK: u8 = 0x0F;
const COMMON_TEXT_RENDERER_ADDRESS: u16 = 0xE56C;
const COMMON_TEXT_RENDERER_BYTE_COUNT: usize = 0x63;
const COMMON_TEXT_RENDERER_SHA1: &str = "c3f7246aff5669e0ac537f20796932d53cd817f3";
const ROW_BUFFER_STARTS: [u16; 2] = [0x03E1, 0x03FF];
const ROW_BUFFER_BYTE_CAPACITY: usize = 30;
const QUEUE_COMMAND_HEADER_BYTE_COUNT: usize = 3;
const QUEUE_TERMINATOR_BYTE_COUNT: usize = 1;
const GLYPH_READ_ADDRESS: u16 = 0xE57F;
const GLYPH_READ_BYTES: [u8; 4] = [0xB1, 0x00, 0xC9, 0xEF];
const GLYPH_READ_SHA1: &str = "3276988cd7930e7ccb8906a0afabc0629db1df3e";
const DIRECT_CALL_BYTES: [u8; 3] = [0x20, 0x6C, 0xE5];

pub(in crate::mapper165::battle_codebook_plan) const BATTLE_DIALOGUE_STATE_HANDLERS: [u16; 9] = [
    0xC73D, 0x8063, 0x80C2, 0x8237, 0x827D, 0x83B8, 0x8309, 0x8369, 0x8049,
];
const DIALOGUE_BOX_PHASE_POINTERS: [u16; 6] = [0xC73D, 0x8012, 0x8012, 0x8012, 0x8012, 0x80D8];
pub(in crate::mapper165::battle_codebook_plan) const DIALOGUE_BOX_INNER_STATE_POINTERS: [u16; 11] = [
    0x8278, 0x81DD, 0x80F4, 0x819D, 0x8204, 0x8211, 0x81AD, 0x81BD, 0x81E5, 0x8193, 0x8234,
];
pub(in crate::mapper165::battle_codebook_plan) const BATTLE_TERRAIN_BANK_HANDLER_POINTER: [u16; 1] =
    [0x8472];
pub(super) const ENDING_SEQUENCE_PHASE_POINTERS: [u16; 30] = [
    0xA3A5, 0xA3E0, 0x9FED, 0xA054, 0xA0E9, 0x9FFA, 0xA011, 0xA02D, 0xA054, 0xA071, 0x9F64, 0x9F83,
    0xA054, 0x9F57, 0xA123, 0xA165, 0xA233, 0xA252, 0xA25D, 0xA269, 0xA27E, 0xA294, 0xA384, 0x9FCA,
    0xA02D, 0xA054, 0xA0D3, 0xA508, 0xA535, 0xC73D,
];
pub(super) const ENDING_SCROLL_INNER_STATE_POINTERS: [u16; 3] = [0xC73D, 0xA3EC, 0xA440];

#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub(super) struct CallerKey {
    pub(super) bank: u8,
    pub(super) address: u16,
}

#[derive(Clone, Copy)]
pub(super) struct CallerSpec {
    pub(super) key: CallerKey,
    pub(super) role: &'static str,
    pub(super) lifetime: &'static str,
}

pub(super) const CALLER_SPECS: [CallerSpec; 10] = [
    caller(0x04, 0x8263, "publish_battle_dialogue_row", "battle"),
    caller(0x04, 0x8392, "publish_next_battle_dialogue_row", "battle"),
    caller(0x04, 0x9F9C, "publish_ending_sequence_text", "ending"),
    caller(0x04, 0xA43C, "publish_ending_scroll_heading", "ending"),
    caller(0x04, 0xA478, "publish_ending_scroll_record", "ending"),
    caller(0x05, 0x89D0, "publish_battle_unit_name", "battle"),
    caller(0x05, 0x8A5D, "publish_battle_class_name", "battle"),
    caller(0x05, 0x8A8D, "publish_battle_item_name", "battle"),
    caller(0x07, 0x82C3, "publish_battle_message_template", "battle"),
    caller(0x07, 0x84A2, "publish_battle_terrain_name", "battle"),
];

const fn caller(bank: u8, address: u16, role: &'static str, lifetime: &'static str) -> CallerSpec {
    CallerSpec {
        key: CallerKey { bank, address },
        role,
        lifetime,
    }
}

pub(super) struct TextConsumerSourceBinding {
    pub(super) renderer_bank: u8,
    pub(super) renderer_address: u16,
    pub(super) renderer_byte_count: usize,
    pub(super) renderer_source_sha1: String,
    pub(super) renderer_typed_instruction_count: usize,
    pub(super) glyph_read_address: u16,
    pub(super) glyph_read_source_bytes: Vec<u8>,
    pub(super) glyph_read_source_sha1: String,
    pub(super) row_buffer_count: usize,
    pub(super) row_buffer_byte_capacity: usize,
    pub(super) maximum_queue_byte_count: usize,
}

pub(super) fn bind_text_consumer_source(rom: &Rom) -> Result<TextConsumerSourceBinding> {
    bind_declared_state_roots(rom)?;

    let renderer = source_bytes(
        rom,
        COMMON_TEXT_RENDERER_BANK,
        COMMON_TEXT_RENDERER_ADDRESS,
        COMMON_TEXT_RENDERER_BYTE_COUNT,
    )?;
    let renderer_source_sha1 = sha1_hex(renderer);
    ensure!(
        renderer_source_sha1 == COMMON_TEXT_RENDERER_SHA1,
        "common text renderer changed: expected {COMMON_TEXT_RENDERER_SHA1}, found {renderer_source_sha1}"
    );
    let renderer_instructions = decode_rp2a03_sequence(
        renderer,
        COMMON_TEXT_RENDERER_ADDRESS,
        "common text renderer",
    )?;

    let glyph_read = source_bytes(
        rom,
        COMMON_TEXT_RENDERER_BANK,
        GLYPH_READ_ADDRESS,
        GLYPH_READ_BYTES.len(),
    )?;
    ensure!(
        glyph_read == GLYPH_READ_BYTES,
        "renderer glyph read changed"
    );
    let glyph_read_source_sha1 = sha1_hex(glyph_read);
    ensure!(
        glyph_read_source_sha1 == GLYPH_READ_SHA1,
        "renderer glyph-read hash changed"
    );
    decode_rp2a03_sequence(glyph_read, GLYPH_READ_ADDRESS, "renderer glyph read")?;

    ensure!(
        usize::from(ROW_BUFFER_STARTS[1] - ROW_BUFFER_STARTS[0]) == ROW_BUFFER_BYTE_CAPACITY,
        "common renderer row-buffer stride changed"
    );
    let maximum_queue_byte_count = ROW_BUFFER_STARTS
        .len()
        .checked_mul(QUEUE_COMMAND_HEADER_BYTE_COUNT + ROW_BUFFER_BYTE_CAPACITY)
        .and_then(|count| count.checked_add(QUEUE_TERMINATOR_BYTE_COUNT))
        .context("common renderer queue bound overflow")?;
    ensure!(
        maximum_queue_byte_count == 67,
        "common renderer queue bound changed"
    );

    let actual_callers = scan_direct_callers(rom)?;
    let expected_callers = CALLER_SPECS
        .iter()
        .map(|spec| spec.key)
        .collect::<BTreeSet<_>>();
    ensure!(
        actual_callers == expected_callers,
        "common text renderer direct caller census changed: expected {expected_callers:?}, found {actual_callers:?}"
    );
    for spec in CALLER_SPECS {
        bind_typed_direct_call(rom, spec.key)?;
    }

    Ok(TextConsumerSourceBinding {
        renderer_bank: COMMON_TEXT_RENDERER_BANK,
        renderer_address: COMMON_TEXT_RENDERER_ADDRESS,
        renderer_byte_count: renderer.len(),
        renderer_source_sha1,
        renderer_typed_instruction_count: renderer_instructions.len(),
        glyph_read_address: GLYPH_READ_ADDRESS,
        glyph_read_source_bytes: glyph_read.to_vec(),
        glyph_read_source_sha1,
        row_buffer_count: ROW_BUFFER_STARTS.len(),
        row_buffer_byte_capacity: ROW_BUFFER_BYTE_CAPACITY,
        maximum_queue_byte_count,
    })
}

fn bind_declared_state_roots(rom: &Rom) -> Result<()> {
    for (bank, address, pointers, role) in [
        (
            0x04,
            0x8037,
            BATTLE_DIALOGUE_STATE_HANDLERS.as_slice(),
            "battle dialogue state",
        ),
        (
            0x05,
            0x8836,
            UNIT_PANEL_PHASE_POINTERS.as_slice(),
            "battle unit-panel phase",
        ),
        (
            0x07,
            0x8006,
            DIALOGUE_BOX_PHASE_POINTERS.as_slice(),
            "battle dialogue-box phase",
        ),
        (
            0x07,
            0x80DE,
            DIALOGUE_BOX_INNER_STATE_POINTERS.as_slice(),
            "battle dialogue-box inner state",
        ),
        (
            0x07,
            0xBFA2,
            BATTLE_TERRAIN_BANK_HANDLER_POINTER.as_slice(),
            "battle terrain bank-handler slot 1",
        ),
        (
            0x04,
            0x9F1B,
            ENDING_SEQUENCE_PHASE_POINTERS.as_slice(),
            "ending sequence phase",
        ),
        (
            0x04,
            0xA3E6,
            ENDING_SCROLL_INNER_STATE_POINTERS.as_slice(),
            "ending scroll inner state",
        ),
    ] {
        bind_pointer_table(rom, bank, address, pointers, role)?;
    }
    Ok(())
}

fn bind_pointer_table(
    rom: &Rom,
    bank: u8,
    address: u16,
    expected: &[u16],
    role: &str,
) -> Result<()> {
    let bytes = source_bytes(rom, bank, address, expected.len() * 2)?;
    let actual = bytes
        .chunks_exact(2)
        .map(|pair| u16::from_le_bytes([pair[0], pair[1]]))
        .collect::<Vec<_>>();
    ensure!(actual == expected, "{role} pointer table changed");
    Ok(())
}

fn scan_direct_callers(rom: &Rom) -> Result<BTreeSet<CallerKey>> {
    let mut callers = BTreeSet::new();
    for bank_number in 0_u8..=0x0F {
        let bank = prg_bank(rom, bank_number)?;
        let cpu_base: u16 = if bank_number == 0x0F { 0xC000 } else { 0x8000 };
        for (offset, bytes) in bank.windows(DIRECT_CALL_BYTES.len()).enumerate() {
            if bytes == DIRECT_CALL_BYTES {
                let address = cpu_base
                    .checked_add(u16::try_from(offset).context("direct caller offset overflow")?)
                    .context("direct caller address overflow")?;
                callers.insert(CallerKey {
                    bank: bank_number,
                    address,
                });
            }
        }
    }
    Ok(callers)
}

fn bind_typed_direct_call(rom: &Rom, key: CallerKey) -> Result<()> {
    let bytes = source_bytes(rom, key.bank, key.address, DIRECT_CALL_BYTES.len())?;
    ensure!(
        bytes == DIRECT_CALL_BYTES,
        "declared common-renderer caller changed at {:02X}:${:04X}",
        key.bank,
        key.address
    );
    let instruction = decode_bytes(bytes).with_context(|| {
        format!(
            "decode common-renderer caller at {:02X}:${:04X}",
            key.bank, key.address
        )
    })?;
    ensure!(
        matches!(
            instruction.control_flow(key.address),
            ControlFlow::Call {
                target: COMMON_TEXT_RENDERER_ADDRESS,
                ..
            }
        ),
        "declared common-renderer caller is not a typed call at {:02X}:${:04X}",
        key.bank,
        key.address
    );
    Ok(())
}
