use anyhow::{Context, Result, ensure};

use crate::{
    rom::{HEADER_SIZE, Rom},
    sha1_hex,
    typed_source::decode_rp2a03_sequence,
};

const PRG_BANK_SIZE: usize = 16 * 1024;
const PRG_BANK: u8 = 0x0B;
const CPU_WINDOW_START: u16 = 0x8000;

const STATE_DISPATCH_ADDRESS: u16 = 0x995F;
const STATE_DISPATCH_PREFIX: &[u8] = &[0xAD, 0xEE, 0x05, 0x20, 0x4C, 0xC3];
const STATE_HANDLER_POINTERS: [u16; 16] = [
    0xC73D, 0x9985, 0x9A33, 0x9A99, 0x9AFC, 0x9B14, 0x9B2B, 0x9B35, 0x9B8A, 0x9B14, 0x9BA0, 0x9BCF,
    0x9C17, 0x9C09, 0x9CF0, 0x9D0C,
];
const SUSPEND_STATE_INDEX: usize = 8;
const SUSPEND_HANDLER_ADDRESS: u16 = 0x9B8A;
const SUSPEND_HANDLER: &[u8] = &[
    0x20, 0x81, 0x9B, 0xA9, 0x00, 0x8D, 0xF0, 0x77, 0xA9, 0xB0, 0x8D, 0xF4, 0x77, 0xA9, 0x01, 0x8D,
    0xF1, 0x77, 0xEE, 0xEE, 0x05, 0x60,
];
const DIALOGUE_DIRECTORY_ADDRESS: u16 = 0xBFE0;
const VICTORY_AND_DEFEAT_POINTER_TABLE: u16 = 0x9D85;
const SUSPEND_DIALOGUE_ENTRY_ADDRESS: u16 = 0x9D87;
const SUSPEND_DIALOGUE_POINTER: u16 = 0x9DCB;

pub(crate) fn bind_suspend_message_to_main_dialogue(rom: &Rom) -> Result<()> {
    let dispatch_prefix = source_slice(rom, STATE_DISPATCH_ADDRESS, STATE_DISPATCH_PREFIX.len())?;
    ensure!(
        dispatch_prefix == STATE_DISPATCH_PREFIX,
        "map-message state dispatcher changed"
    );
    decode_rp2a03_sequence(
        dispatch_prefix,
        STATE_DISPATCH_ADDRESS,
        "dispatch map-message state",
    )?;

    let pointer_bytes = source_slice(
        rom,
        STATE_DISPATCH_ADDRESS + STATE_DISPATCH_PREFIX.len() as u16,
        STATE_HANDLER_POINTERS.len() * 2,
    )?;
    let pointers = pointer_bytes
        .chunks_exact(2)
        .map(|bytes| u16::from_le_bytes([bytes[0], bytes[1]]))
        .collect::<Vec<_>>();
    ensure!(
        pointers == STATE_HANDLER_POINTERS,
        "map-message state handler table changed"
    );
    ensure!(
        pointers[SUSPEND_STATE_INDEX] == SUSPEND_HANDLER_ADDRESS,
        "suspend state no longer selects its message handler"
    );

    let handler = source_slice(rom, SUSPEND_HANDLER_ADDRESS, SUSPEND_HANDLER.len())?;
    ensure!(
        handler == SUSPEND_HANDLER,
        "suspend message selector changed"
    );
    ensure!(
        sha1_hex(handler) == "5f6d63f7f8e8dd833e431fd40b3872d55fa7af80",
        "suspend message selector hash changed"
    );
    decode_rp2a03_sequence(handler, SUSPEND_HANDLER_ADDRESS, "select suspend dialogue")?;

    ensure!(
        read_u16(rom, DIALOGUE_DIRECTORY_ADDRESS)? == VICTORY_AND_DEFEAT_POINTER_TABLE,
        "victory-and-defeat dialogue directory changed"
    );
    ensure!(
        read_u16(rom, SUSPEND_DIALOGUE_ENTRY_ADDRESS)? == SUSPEND_DIALOGUE_POINTER,
        "suspend dialogue entry pointer changed"
    );
    Ok(())
}

fn read_u16(rom: &Rom, cpu_address: u16) -> Result<u16> {
    let bytes = source_slice(rom, cpu_address, 2)?;
    Ok(u16::from_le_bytes([bytes[0], bytes[1]]))
}

fn source_slice(rom: &Rom, cpu_address: u16, len: usize) -> Result<&[u8]> {
    ensure!(
        (CPU_WINDOW_START..0xC000).contains(&cpu_address),
        "suspend source address {cpu_address:04X} is outside bank 0B"
    );
    let offset = HEADER_SIZE
        + usize::from(PRG_BANK) * PRG_BANK_SIZE
        + usize::from(cpu_address - CPU_WINDOW_START);
    let end = offset.checked_add(len).context("suspend source overflow")?;
    rom.data()
        .get(offset..end)
        .with_context(|| format!("suspend source exceeds ROM at {offset:05X}"))
}
