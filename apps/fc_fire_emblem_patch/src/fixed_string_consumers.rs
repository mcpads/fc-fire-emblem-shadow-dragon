//! bank 0B 고정 문자열 표와 공용 appender의 직접 소비 분모를 결속한다.
//!
//! 원문 문자열이 저장돼 있다는 사실과 실제 화면 상태가 그 문자열을 소비한다는
//! 사실은 다르다. 이 모듈은 72개 저장 엔트리, 49개 직접 appender 호출, 39개
//! 합성 상태 handler, `$E690`의 직접 상태 생산자를 서로 대조한다. 직접 생산자가
//! 없는 상태 00/01은 원천에 남아 있는 handler로 보고하되 실행 소유가 확인된
//! population에 섞지 않는다.

use std::collections::BTreeSet;

use anyhow::{Context, Result, ensure};
use serde::Serialize;

use crate::{rom::Rom, sha1_hex, typed_source::decode_rp2a03_sequence};

const PRG_BANK_BYTE_COUNT: usize = 16 * 1024;
const SOURCE_PRG_BANK_COUNT: usize = 16;
const FIXED_STRING_BANK: u8 = 0x0B;
const FIXED_STRING_POINTER_TABLE: u16 = 0x8FC2;
const FIXED_STRING_STORAGE_START: u16 = 0x9052;
const FIXED_STRING_STORAGE_END_EXCLUSIVE: u16 = 0x9251;
const FIXED_STRING_ENTRY_COUNT: usize = 72;
const APPEND_FIXED_STRING: u16 = 0x8EEE;
const COMPOSITE_DISPATCH_TABLE: u16 = 0x8006;
const COMPOSITE_STATE_COUNT: usize = 0x27;
const COMPOSITE_PAGE_ENTRY: u16 = 0xE690;

const EXPECTED_POINTER_TABLE_SHA1: &str = "abd2bda45d0e8b77efcd92175acaf12014105809";
const EXPECTED_RECORD_IDENTITY_SHA1: &str = "00db8ffe21dac8f7416856a4e32673919f4dadce";
const EXPECTED_CALL_SITE_SHA1: &str = "d3a5152272261cf455e883010c935c7d12a9cd5e";
pub(crate) const EXPECTED_DIRECT_COMPOSITE_PRODUCER_COUNT: usize = 50;
pub(crate) const EXPECTED_DIRECT_COMPOSITE_PRODUCER_SHA1: &str =
    "eba4ee041d3af03bd5c2d71cc443e81fb01590a1";

const EXPECTED_COMPOSITE_HANDLERS: [u16; COMPOSITE_STATE_COUNT] = [
    0x8054, 0x8088, 0x80F6, 0x8187, 0x826C, 0x82E3, 0x84F4, 0x85BE, 0x85E5, 0x8613, 0x86C1, 0x867D,
    0x8785, 0x8BE6, 0x8C8F, 0x87F2, 0x886A, 0x8891, 0x88C4, 0x88D5, 0x8923, 0x8965, 0x89DB, 0x89FD,
    0x8A25, 0x87C4, 0x8A47, 0x8AA1, 0x8AE6, 0x8B08, 0x8B3A, 0x8B80, 0x8BB9, 0x8CE8, 0x8D4B, 0x8D98,
    0x8DC6, 0x81DB, 0x8E0F,
];

const EXPECTED_CALL_SITES: [(u16, u8); 49] = [
    (0x8078, 0x00),
    (0x807D, 0x00),
    (0x80E7, 0x01),
    (0x82AC, 0x04),
    (0x82C0, 0x04),
    (0x8316, 0x05),
    (0x8334, 0x05),
    (0x835B, 0x05),
    (0x83A7, 0x05),
    (0x83FC, 0x05),
    (0x8413, 0x05),
    (0x8424, 0x05),
    (0x842D, 0x05),
    (0x8601, 0x08),
    (0x8608, 0x08),
    (0x8638, 0x09),
    (0x8650, 0x09),
    (0x8662, 0x09),
    (0x866F, 0x09),
    (0x8702, 0x0C),
    (0x87A7, 0x0C),
    (0x87AC, 0x0C),
    (0x87DA, 0x19),
    (0x87DF, 0x19),
    (0x881D, 0x0F),
    (0x883A, 0x0F),
    (0x8850, 0x0F),
    (0x8886, 0x10),
    (0x8938, 0x14),
    (0x894C, 0x14),
    (0x89F2, 0x16),
    (0x8A3C, 0x18),
    (0x8A6D, 0x1A),
    (0x8A7A, 0x1A),
    (0x8AFD, 0x1C),
    (0x8B1D, 0x1D),
    (0x8B22, 0x1D),
    (0x8C0F, 0x0D),
    (0x8C1E, 0x0D),
    (0x8C30, 0x0D),
    (0x8C41, 0x0D),
    (0x8C53, 0x0D),
    (0x8CA8, 0x0E),
    (0x8CC6, 0x0E),
    (0x8D01, 0x21),
    (0x8D24, 0x21),
    (0x8DA8, 0x23),
    (0x8DAD, 0x23),
    (0x8E31, 0x26),
];

const EXPECTED_DIRECT_PRODUCER_BOUND_INDICES: [u8; 56] = [
    0x00, 0x01, 0x02, 0x03, 0x04, 0x05, 0x06, 0x08, 0x09, 0x0A, 0x0B, 0x0E, 0x0F, 0x10, 0x11, 0x12,
    0x13, 0x14, 0x15, 0x16, 0x17, 0x22, 0x23, 0x27, 0x28, 0x29, 0x2A, 0x2B, 0x2C, 0x2D, 0x2E, 0x2F,
    0x30, 0x31, 0x32, 0x33, 0x34, 0x35, 0x36, 0x37, 0x38, 0x39, 0x3A, 0x3B, 0x3C, 0x3D, 0x3E, 0x3F,
    0x40, 0x41, 0x42, 0x43, 0x44, 0x45, 0x46, 0x47,
];

#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub(crate) struct CompositeStateProducer {
    pub(crate) prg_bank: u8,
    pub(crate) cpu_address: u16,
    pub(crate) transfer_opcode: u8,
    pub(crate) state: u8,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct CompositeStateDispatchSource {
    prg_bank: u8,
    call_address: u16,
    handler_selector_domain: BTreeSet<u8>,
    direct_producer_selector_domain: BTreeSet<u8>,
}

impl CompositeStateDispatchSource {
    pub(crate) fn prg_bank(&self) -> u8 {
        self.prg_bank
    }

    pub(crate) fn call_address(&self) -> u16 {
        self.call_address
    }

    pub(crate) fn handler_selector_domain(&self) -> &BTreeSet<u8> {
        &self.handler_selector_domain
    }

    pub(crate) fn direct_producer_selector_domain(&self) -> &BTreeSet<u8> {
        &self.direct_producer_selector_domain
    }

    pub(crate) fn handler_target(&self, state: u8) -> Option<u16> {
        self.handler_selector_domain
            .contains(&state)
            .then(|| EXPECTED_COMPOSITE_HANDLERS[usize::from(state)])
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct FixedStringRecord {
    pub(crate) index: u8,
    pub(crate) pointer: u16,
    pub(crate) source_bytes: Vec<u8>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct FixedStringCallSite {
    pub(crate) cpu_address: u16,
    pub(crate) composite_state: u8,
    pub(crate) possible_indices: Vec<u8>,
}

#[derive(Debug)]
pub(crate) struct FixedStringConsumerInspection {
    pub(crate) records: Vec<FixedStringRecord>,
    pub(crate) call_sites: Vec<FixedStringCallSite>,
    pub(crate) composite_state_producers: Vec<CompositeStateProducer>,
    pub(crate) direct_producer_bound_indices: BTreeSet<u8>,
    pub(crate) census: FixedStringConsumerCensus,
}

impl FixedStringConsumerInspection {
    pub(crate) fn composite_handler_target(&self, state: u8) -> Option<u16> {
        EXPECTED_COMPOSITE_HANDLERS.get(usize::from(state)).copied()
    }
}

#[derive(Debug, Serialize)]
pub(crate) struct FixedStringConsumerCensus {
    pointer_table_cpu_address_hex: &'static str,
    entry_count: usize,
    unique_pointer_count: usize,
    contiguous_storage_byte_count: usize,
    direct_appender_call_count: usize,
    direct_composite_state_producer_count: usize,
    direct_producer_state_count: usize,
    direct_producer_bound_call_count: usize,
    direct_producer_bound_index_count: usize,
    direct_producer_bound_indices_hex: Vec<String>,
    handlers_without_direct_producer_hex: Vec<String>,
    pointer_table_sha1: String,
    record_identity_sha1: String,
    call_site_sha1: String,
    population_complete_for_declared_table: bool,
    direct_appender_call_population_complete: bool,
    whole_program_indirect_or_computed_reference_census_complete: bool,
}

pub(crate) fn inspect_fixed_string_consumers(rom: &Rom) -> Result<FixedStringConsumerInspection> {
    rom.verify_supported_japanese()?;
    let bank = source_bank(rom, FIXED_STRING_BANK)?;
    let records = parse_fixed_string_records(bank)?;
    let calls = bind_fixed_string_call_sites(bank)?;
    let producers = bind_direct_composite_state_producer_catalog(rom)?;
    let produced_states = producers
        .iter()
        .map(|producer| producer.state)
        .collect::<BTreeSet<_>>();
    ensure!(
        produced_states == (0x02..=0x26).collect(),
        "direct composite-state producer population changed: {produced_states:?}"
    );

    let direct_producer_bound_indices = calls
        .iter()
        .filter(|call| produced_states.contains(&call.composite_state))
        .flat_map(|call| call.possible_indices.iter().copied())
        .collect::<BTreeSet<_>>();
    let handlers_without_direct_producer = (0..COMPOSITE_STATE_COUNT as u8)
        .filter(|state| !produced_states.contains(state))
        .collect::<Vec<_>>();
    ensure!(
        handlers_without_direct_producer == [0x00, 0x01],
        "composite handlers without direct producers changed"
    );
    ensure!(
        direct_producer_bound_indices
            == EXPECTED_DIRECT_PRODUCER_BOUND_INDICES.into_iter().collect(),
        "direct-producer fixed-string index population changed: {direct_producer_bound_indices:?}"
    );

    let pointer_table = bank_slice(
        bank,
        FIXED_STRING_POINTER_TABLE,
        FIXED_STRING_ENTRY_COUNT * 2,
    )?;
    let record_identity = records
        .iter()
        .flat_map(|record| {
            record
                .pointer
                .to_le_bytes()
                .into_iter()
                .chain(record.source_bytes.iter().copied())
        })
        .collect::<Vec<_>>();
    let call_identity = calls
        .iter()
        .flat_map(|call| call.cpu_address.to_le_bytes())
        .collect::<Vec<_>>();
    let pointer_table_sha1 = sha1_hex(pointer_table);
    let record_identity_sha1 = sha1_hex(&record_identity);
    let call_site_sha1 = sha1_hex(&call_identity);
    ensure!(
        pointer_table_sha1 == EXPECTED_POINTER_TABLE_SHA1
            && record_identity_sha1 == EXPECTED_RECORD_IDENTITY_SHA1
            && call_site_sha1 == EXPECTED_CALL_SITE_SHA1,
        "fixed-string source identity changed"
    );

    let census = FixedStringConsumerCensus {
        pointer_table_cpu_address_hex: "0x8FC2",
        entry_count: records.len(),
        unique_pointer_count: records
            .iter()
            .map(|record| record.pointer)
            .collect::<BTreeSet<_>>()
            .len(),
        contiguous_storage_byte_count: usize::from(
            FIXED_STRING_STORAGE_END_EXCLUSIVE - FIXED_STRING_STORAGE_START,
        ),
        direct_appender_call_count: calls.len(),
        direct_composite_state_producer_count: producers.len(),
        direct_producer_state_count: produced_states.len(),
        direct_producer_bound_call_count: calls
            .iter()
            .filter(|call| produced_states.contains(&call.composite_state))
            .count(),
        direct_producer_bound_index_count: direct_producer_bound_indices.len(),
        direct_producer_bound_indices_hex: direct_producer_bound_indices
            .iter()
            .copied()
            .map(|index| format!("{index:02X}"))
            .collect(),
        handlers_without_direct_producer_hex: handlers_without_direct_producer
            .into_iter()
            .map(|state| format!("{state:02X}"))
            .collect(),
        pointer_table_sha1,
        record_identity_sha1,
        call_site_sha1,
        population_complete_for_declared_table: true,
        direct_appender_call_population_complete: true,
        whole_program_indirect_or_computed_reference_census_complete: false,
    };

    Ok(FixedStringConsumerInspection {
        records,
        call_sites: calls,
        composite_state_producers: producers,
        direct_producer_bound_indices,
        census,
    })
}

pub(crate) fn bind_direct_composite_state_producer_catalog(
    rom: &Rom,
) -> Result<Vec<CompositeStateProducer>> {
    let producers = scan_direct_composite_state_producers(rom)?;
    ensure!(
        producers.len() == EXPECTED_DIRECT_COMPOSITE_PRODUCER_COUNT,
        "direct composite-state producer population changed"
    );
    let identity = producers
        .iter()
        .flat_map(|producer| {
            [
                producer.prg_bank,
                producer.cpu_address as u8,
                (producer.cpu_address >> 8) as u8,
                producer.transfer_opcode,
                producer.state,
            ]
        })
        .collect::<Vec<_>>();
    ensure!(
        sha1_hex(&identity) == EXPECTED_DIRECT_COMPOSITE_PRODUCER_SHA1,
        "direct composite-state producer catalog changed"
    );
    Ok(producers)
}

/// Binds both the common bank-0B handler-table domain and the states produced by its exact
/// direct-entry catalog. The latter is deliberately not a complete producer denominator:
/// states zero and one remain valid table entries even though the direct producer census does
/// not produce them.
pub(crate) fn bind_composite_state_dispatch_source(
    rom: &Rom,
) -> Result<CompositeStateDispatchSource> {
    rom.verify_supported_japanese()?;
    let bank = source_bank(rom, FIXED_STRING_BANK)?;
    bind_composite_dispatch_table(bank)?;
    let entry = bank_slice(bank, 0x8000, 6)?;
    ensure!(
        entry == [0xAD, 0xE8, 0x05, 0x20, 0x4C, 0xC3],
        "composite-state dispatcher entry changed"
    );
    decode_rp2a03_sequence(entry, 0x8000, "dispatch one composite screen state")?;

    let direct_producer_selector_domain = bind_direct_composite_state_producer_catalog(rom)?
        .into_iter()
        .map(|producer| producer.state)
        .collect::<BTreeSet<_>>();
    ensure!(
        !direct_producer_selector_domain.is_empty()
            && direct_producer_selector_domain
                .iter()
                .all(|state| usize::from(*state) < COMPOSITE_STATE_COUNT),
        "direct composite-state producer domain escapes the handler table"
    );
    let handler_selector_domain = (0..u8::try_from(COMPOSITE_STATE_COUNT)
        .context("composite-state count exceeds u8")?)
        .collect::<BTreeSet<_>>();
    ensure!(
        direct_producer_selector_domain.is_subset(&handler_selector_domain),
        "direct composite-state producers escape the bound handler domain"
    );

    Ok(CompositeStateDispatchSource {
        prg_bank: FIXED_STRING_BANK,
        call_address: 0x8003,
        handler_selector_domain,
        direct_producer_selector_domain,
    })
}

pub(crate) fn scan_direct_composite_state_producers(
    rom: &Rom,
) -> Result<Vec<CompositeStateProducer>> {
    let prg = rom
        .prg()
        .get(..SOURCE_PRG_BANK_COUNT * PRG_BANK_BYTE_COUNT)
        .context("image has fewer than sixteen source PRG banks")?;
    let target = COMPOSITE_PAGE_ENTRY.to_le_bytes();
    let mut producers = Vec::new();
    for (bank_index, bank) in prg.chunks_exact(PRG_BANK_BYTE_COUNT).enumerate() {
        for offset in 2..bank.len() - 2 {
            let opcode = bank[offset];
            if ![0x20, 0x4C].contains(&opcode) || bank[offset + 1..offset + 3] != target {
                continue;
            }
            ensure!(
                bank[offset - 2] == 0xA9,
                "direct composite-state transfer has no immediate state producer at bank {bank_index:02X} offset {offset:04X}"
            );
            let cpu_address = 0x8000_u16
                .checked_add(u16::try_from(offset).context("composite producer offset overflow")?)
                .context("composite producer CPU address overflow")?;
            decode_rp2a03_sequence(
                &bank[offset - 2..offset + 3],
                cpu_address - 2,
                "load one composite state and transfer to its fixed writer",
            )?;
            producers.push(CompositeStateProducer {
                prg_bank: u8::try_from(bank_index).context("composite producer bank overflow")?,
                cpu_address,
                transfer_opcode: opcode,
                state: bank[offset - 1],
            });
        }
    }
    producers.sort_unstable();
    Ok(producers)
}

fn parse_fixed_string_records(bank: &[u8]) -> Result<Vec<FixedStringRecord>> {
    let pointer_table = bank_slice(
        bank,
        FIXED_STRING_POINTER_TABLE,
        FIXED_STRING_ENTRY_COUNT * 2,
    )?;
    let pointers = pointer_table
        .chunks_exact(2)
        .map(|bytes| u16::from_le_bytes([bytes[0], bytes[1]]))
        .collect::<Vec<_>>();
    ensure!(
        pointers.first() == Some(&FIXED_STRING_STORAGE_START),
        "fixed-string table no longer ends at its first record"
    );
    ensure!(
        pointers.iter().copied().collect::<BTreeSet<_>>().len() == FIXED_STRING_ENTRY_COUNT,
        "fixed-string table repeats a source pointer"
    );

    let mut records = Vec::with_capacity(pointers.len());
    for (index, pointer) in pointers.iter().copied().enumerate() {
        let start = bank_offset(pointer)?;
        let relative_end = bank
            .get(start..)
            .context("fixed-string pointer exceeds bank 0B")?
            .iter()
            .position(|byte| [0xED, 0xEF].contains(byte))
            .context("fixed-string record has no terminator")?;
        let end = start
            .checked_add(relative_end + 1)
            .context("fixed-string record range overflow")?;
        let end_address = 0x8000_u16
            .checked_add(u16::try_from(end).context("fixed-string end offset overflow")?)
            .context("fixed-string end address overflow")?;
        if let Some(next) = pointers.get(index + 1) {
            ensure!(
                end_address == *next,
                "fixed-string record {index:02X} is not contiguous with the next pointer"
            );
        } else {
            ensure!(
                end_address == FIXED_STRING_STORAGE_END_EXCLUSIVE,
                "fixed-string storage end changed"
            );
        }
        records.push(FixedStringRecord {
            index: u8::try_from(index).context("fixed-string index overflow")?,
            pointer,
            source_bytes: bank[start..end].to_vec(),
        });
    }
    Ok(records)
}

fn bind_fixed_string_call_sites(bank: &[u8]) -> Result<Vec<FixedStringCallSite>> {
    let target = APPEND_FIXED_STRING.to_le_bytes();
    let actual = bank
        .windows(3)
        .enumerate()
        .filter_map(|(offset, bytes)| {
            (bytes == [0x20, target[0], target[1]])
                .then_some(0x8000_u16 + u16::try_from(offset).ok()?)
        })
        .collect::<Vec<_>>();
    let expected = EXPECTED_CALL_SITES
        .iter()
        .map(|(address, _)| *address)
        .collect::<Vec<_>>();
    ensure!(
        actual == expected,
        "fixed-string direct call population changed"
    );

    bind_composite_dispatch_table(bank)?;
    EXPECTED_CALL_SITES
        .iter()
        .map(|(cpu_address, composite_state)| {
            let possible_indices = classify_call_indices(bank, *cpu_address)?;
            ensure!(
                possible_indices
                    .iter()
                    .all(|index| usize::from(*index) < FIXED_STRING_ENTRY_COUNT),
                "fixed-string call {cpu_address:04X} can select outside the table"
            );
            Ok(FixedStringCallSite {
                cpu_address: *cpu_address,
                composite_state: *composite_state,
                possible_indices,
            })
        })
        .collect()
}

fn classify_call_indices(bank: &[u8], address: u16) -> Result<Vec<u8>> {
    match address {
        0x80E7 => {
            bind_code(
                bank,
                0x80E1,
                &[0xAC, 0xB2, 0x77, 0xB9, 0xF2, 0x80, 0x20, 0xEE, 0x8E],
            )?;
            bind_bytes(bank, 0x80F2, &[0x1B, 0x1A, 0x19, 0x18])?;
            Ok(vec![0x18, 0x19, 0x1A, 0x1B])
        }
        0x83A7 => {
            bind_code(
                bank,
                0x8374,
                &[
                    0xA4, 0x12, 0xB9, 0x5D, 0x84, 0x20, 0x65, 0x84, 0xB0, 0x06, 0xC6, 0x12, 0x10,
                    0xF2, 0x30, 0x07, 0xA4, 0x12, 0xB9, 0x61, 0x84, 0xD0, 0x1C, 0xA0, 0x01, 0xB1,
                    0x74, 0xC9, 0x09, 0xD0, 0x0B, 0xA9, 0xAB, 0x20, 0x65, 0x84, 0x90, 0x04, 0xA9,
                    0x37, 0xD0, 0x09, 0xA9, 0x46, 0x20, 0x65, 0x84, 0x90, 0x06, 0xA9, 0x2A, 0x20,
                    0xEE, 0x8E,
                ],
            )?;
            bind_bytes(bank, 0x8461, &[0x2A, 0x38, 0x39, 0x37])?;
            Ok(vec![0x2A, 0x37, 0x38, 0x39])
        }
        0x8424 => {
            bind_code(
                bank,
                0x841A,
                &[
                    0x18, 0xAD, 0xD0, 0x77, 0xF0, 0x08, 0xA8, 0xB9, 0x57, 0x84, 0x20, 0xEE, 0x8E,
                ],
            )?;
            bind_bytes(bank, 0x8457, &[0x00, 0x29, 0x33, 0x34, 0x3A, 0x3D])?;
            Ok(vec![0x00, 0x29, 0x33, 0x34, 0x3A, 0x3D])
        }
        0x881D => {
            bind_code(
                bank,
                0x8811,
                &[
                    0xA0, 0x07, 0x84, 0x12, 0xC0, 0x0E, 0xF0, 0x18, 0x98, 0x38, 0xE9, 0x07, 0x20,
                    0xEE, 0x8E,
                ],
            )?;
            Ok((0x00..=0x06).collect())
        }
        _ => {
            let start = address
                .checked_sub(2)
                .context("fixed-string immediate producer address underflow")?;
            let bytes = bank_slice(bank, start, 5)?;
            ensure!(
                bytes[0] == 0xA9 && bytes[2..] == [0x20, 0xEE, 0x8E],
                "fixed-string call {address:04X} lost its immediate index producer"
            );
            decode_rp2a03_sequence(bytes, start, "produce and append one fixed string")?;
            Ok(vec![bytes[1]])
        }
    }
}

fn bind_composite_dispatch_table(bank: &[u8]) -> Result<()> {
    let actual = bank_slice(
        bank,
        COMPOSITE_DISPATCH_TABLE,
        EXPECTED_COMPOSITE_HANDLERS.len() * 2,
    )?
    .chunks_exact(2)
    .map(|bytes| u16::from_le_bytes([bytes[0], bytes[1]]))
    .collect::<Vec<_>>();
    ensure!(
        actual == EXPECTED_COMPOSITE_HANDLERS,
        "composite handler table changed"
    );
    Ok(())
}

fn bind_bytes(bank: &[u8], address: u16, expected: &[u8]) -> Result<()> {
    ensure!(
        bank_slice(bank, address, expected.len())? == expected,
        "fixed-string source region changed at 0B:{address:04X}"
    );
    Ok(())
}

fn bind_code(bank: &[u8], address: u16, expected: &[u8]) -> Result<()> {
    bind_bytes(bank, address, expected)?;
    decode_rp2a03_sequence(expected, address, "fixed-string source producer")?;
    Ok(())
}

fn source_bank(rom: &Rom, bank: u8) -> Result<&[u8]> {
    let start = usize::from(bank) * PRG_BANK_BYTE_COUNT;
    rom.prg()
        .get(start..start + PRG_BANK_BYTE_COUNT)
        .with_context(|| format!("source PRG bank {bank:02X} is missing"))
}

fn bank_slice(bank: &[u8], address: u16, length: usize) -> Result<&[u8]> {
    let start = bank_offset(address)?;
    bank.get(start..start + length)
        .with_context(|| format!("bank 0B range {address:04X}+{length:X} is missing"))
}

fn bank_offset(address: u16) -> Result<usize> {
    ensure!(
        (0x8000..0xC000).contains(&address),
        "fixed-string address {address:04X} is outside a switchable bank"
    );
    Ok(usize::from(address - 0x8000))
}

#[cfg(test)]
mod tests;
