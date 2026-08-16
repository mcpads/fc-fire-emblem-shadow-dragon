use anyhow::{Context, Result, ensure};

use crate::{
    mmc5_chr::switchable_bank_file_offset, rom::Rom, text_inventory::decode_source_markup,
};

pub(super) const PROFILE_COUNT: usize = 22;
pub(super) const TITLE_TERMINATOR: u8 = 0xED;
pub(super) const DESCRIPTION_LINE_BREAK: u8 = 0xED;
pub(super) const DESCRIPTION_TERMINATOR: u8 = 0xEF;
const SOURCE_PRG_BANK: u8 = 0x0D;
const TITLE_POINTER_TABLE_ADDRESS: u16 = 0x8C98;
const DESCRIPTION_POINTER_TABLE_ADDRESS: u16 = 0x8DCC;
const TITLE_CONSUMER_ADDRESS: u16 = 0x82F5;
const DESCRIPTION_CONSUMER_ADDRESS: u16 = 0x830B;
const TITLE_CONSUMER: [u8; 22] = [
    0xAD, 0x59, 0x05, 0x0A, 0xA8, 0xB9, 0x98, 0x8C, 0x85, 0x00, 0xB9, 0x99, 0x8C, 0x85, 0x01, 0xA9,
    0xFF, 0x85, 0x04, 0x20, 0xD0, 0x84,
];
const DESCRIPTION_CONSUMER: [u8; 19] = [
    0xAD, 0x59, 0x05, 0x0A, 0xA8, 0xB9, 0xCC, 0x8D, 0x85, 0x00, 0xB9, 0xCD, 0x8D, 0x85, 0x01, 0xA9,
    0xFF, 0x85, 0x04,
];

pub(crate) fn bind_installed_consumers(rom: &Rom) -> Result<Vec<&'static str>> {
    bind_consumer(rom, TITLE_CONSUMER_ADDRESS, &TITLE_CONSUMER, "title")?;
    bind_consumer(
        rom,
        DESCRIPTION_CONSUMER_ADDRESS,
        &DESCRIPTION_CONSUMER,
        "description",
    )?;
    ensure!(
        read_pointer_table(rom, TITLE_POINTER_TABLE_ADDRESS)? == TITLE_POINTERS
            && read_pointer_table(rom, DESCRIPTION_POINTER_TABLE_ADDRESS)? == DESCRIPTION_POINTERS,
        "installed class-profile pointer tables changed"
    );
    Ok(vec![
        "0D:82F5:class_profile_title_consumer",
        "0D:830B:class_profile_description_consumer",
        "0D:8C98:class_profile_title_pointer_table",
        "0D:8DCC:class_profile_description_pointer_table",
    ])
}
const TITLE_POINTERS: [u16; PROFILE_COUNT] = [
    0x8CC4, 0x8CD5, 0x8CE6, 0x8CF9, 0x8D06, 0x8D19, 0x8D23, 0x8D2C, 0x8D37, 0x8D42, 0x8D4C, 0x8D57,
    0x8D61, 0x8D6C, 0x8D77, 0x8D83, 0x8D8E, 0x8D99, 0x8DA4, 0x8DAE, 0x8DB7, 0x8DC1,
];
const DESCRIPTION_POINTERS: [u16; PROFILE_COUNT] = [
    0x8DF8, 0x8E4F, 0x8E9A, 0x8ED3, 0x8F1F, 0x8F6E, 0x8FB7, 0x8FF4, 0x9026, 0x9066, 0x90B4, 0x90F4,
    0x9138, 0x918F, 0x91D4, 0x9210, 0x924A, 0x9297, 0x92DA, 0x9317, 0x934F, 0x939A,
];

pub(super) struct ClassProfileSourceEntry {
    pub(super) index: usize,
    pub(super) title_pointer: u16,
    pub(super) title_file_offset: usize,
    pub(super) title_storage_byte_count: usize,
    pub(super) title_bytes: Vec<u8>,
    pub(super) title_markup: String,
    pub(super) description_pointer: u16,
    pub(super) description_file_offset: usize,
    pub(super) description_storage_byte_count: usize,
    pub(super) description_bytes: Vec<u8>,
    pub(super) description_lines: Vec<String>,
}

pub(super) fn extract_source_entries(rom: &Rom) -> Result<Vec<ClassProfileSourceEntry>> {
    rom.verify_supported_japanese()?;
    bind_consumer(rom, TITLE_CONSUMER_ADDRESS, &TITLE_CONSUMER, "title")?;
    bind_consumer(
        rom,
        DESCRIPTION_CONSUMER_ADDRESS,
        &DESCRIPTION_CONSUMER,
        "description",
    )?;
    ensure!(
        read_pointer_table(rom, TITLE_POINTER_TABLE_ADDRESS)? == TITLE_POINTERS,
        "class-profile title pointer table changed"
    );
    ensure!(
        read_pointer_table(rom, DESCRIPTION_POINTER_TABLE_ADDRESS)? == DESCRIPTION_POINTERS,
        "class-profile description pointer table changed"
    );

    let mut entries = Vec::with_capacity(PROFILE_COUNT);
    for index in 0..PROFILE_COUNT {
        let title_pointer = TITLE_POINTERS[index];
        let title_end = TITLE_POINTERS
            .get(index + 1)
            .copied()
            .unwrap_or(DESCRIPTION_POINTER_TABLE_ADDRESS);
        let title_bytes = read_cpu_range(rom, title_pointer, title_end)?;
        ensure!(
            title_bytes.last() == Some(&TITLE_TERMINATOR)
                && !title_bytes[..title_bytes.len() - 1].contains(&TITLE_TERMINATOR),
            "class-profile title {index} terminator changed"
        );

        let description_pointer = DESCRIPTION_POINTERS[index];
        let description_end = if let Some(next) = DESCRIPTION_POINTERS.get(index + 1) {
            *next
        } else {
            find_record_end(rom, description_pointer)?
        };
        let description_bytes = read_cpu_range(rom, description_pointer, description_end)?;
        ensure!(
            description_bytes.last() == Some(&DESCRIPTION_TERMINATOR)
                && !description_bytes[..description_bytes.len() - 1]
                    .contains(&DESCRIPTION_TERMINATOR),
            "class-profile description {index} terminator changed"
        );
        let description_body = &description_bytes[..description_bytes.len() - 1];
        ensure!(
            description_body.last() == Some(&DESCRIPTION_LINE_BREAK),
            "class-profile description {index} does not terminate its final line"
        );
        let description_lines = description_body[..description_body.len() - 1]
            .split(|byte| *byte == DESCRIPTION_LINE_BREAK)
            .map(decode_source_markup)
            .collect::<Vec<_>>();
        ensure!(
            (1..=4).contains(&description_lines.len()),
            "class-profile description {index} line count changed"
        );

        entries.push(ClassProfileSourceEntry {
            index,
            title_pointer,
            title_file_offset: source_file_offset(title_pointer)?,
            title_storage_byte_count: title_bytes.len(),
            title_markup: decode_source_markup(&title_bytes[..title_bytes.len() - 1]),
            title_bytes,
            description_pointer,
            description_file_offset: source_file_offset(description_pointer)?,
            description_storage_byte_count: description_bytes.len(),
            description_bytes,
            description_lines,
        });
    }
    Ok(entries)
}

fn bind_consumer(rom: &Rom, address: u16, expected: &[u8], role: &str) -> Result<()> {
    let offset = source_file_offset(address)?;
    let actual = rom
        .data()
        .get(offset..offset + expected.len())
        .with_context(|| format!("class-profile {role} consumer is outside the source"))?;
    ensure!(actual == expected, "class-profile {role} consumer changed");
    Ok(())
}

fn read_pointer_table(rom: &Rom, address: u16) -> Result<[u16; PROFILE_COUNT]> {
    let offset = source_file_offset(address)?;
    let bytes = rom
        .data()
        .get(offset..offset + PROFILE_COUNT * 2)
        .context("class-profile pointer table is outside the source")?;
    let pointers = bytes
        .chunks_exact(2)
        .map(|pair| u16::from_le_bytes([pair[0], pair[1]]))
        .collect::<Vec<_>>();
    pointers
        .try_into()
        .map_err(|_| anyhow::anyhow!("class-profile pointer count changed"))
}

fn read_cpu_range(rom: &Rom, start: u16, end: u16) -> Result<Vec<u8>> {
    ensure!(start < end, "class-profile source range is empty");
    let start_offset = source_file_offset(start)?;
    let end_offset = source_file_offset(end)?;
    Ok(rom
        .data()
        .get(start_offset..end_offset)
        .context("class-profile source range is outside the ROM")?
        .to_vec())
}

fn find_record_end(rom: &Rom, start: u16) -> Result<u16> {
    let start_offset = source_file_offset(start)?;
    let bank_end = switchable_bank_file_offset(SOURCE_PRG_BANK, 0xBFFF)? + 1;
    let relative_end = rom.data()[start_offset..bank_end]
        .iter()
        .position(|byte| *byte == DESCRIPTION_TERMINATOR)
        .context("last class-profile description has no terminator")?;
    start
        .checked_add(u16::try_from(relative_end + 1).context("description record is too large")?)
        .context("description end address overflow")
}

pub(super) fn source_file_offset(cpu_address: u16) -> Result<usize> {
    switchable_bank_file_offset(SOURCE_PRG_BANK, cpu_address)
}
