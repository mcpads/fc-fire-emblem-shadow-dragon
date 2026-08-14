use anyhow::{Result, ensure};

use crate::{
    rom::Rom,
    rp2a03::{Instruction, assemble_at},
    sha1_hex,
    typed_source::decode_rp2a03_sequence,
};

use super::{AUDIO_BANK, EVENT_DIRECTORY_ADDRESS};
use crate::mapper165::writer_census::source_bytes;

const EVENT_DIRECTORY_LOADER_ADDRESS: u16 = 0x8104;
const EVENT_DESCRIPTOR_READER_ADDRESS: u16 = 0x8110;
const EVENT_DESCRIPTOR_READER: [u8; 21] = [
    0xA0, 0x00, 0xB1, 0xF4, 0x99, 0xE0, 0x00, 0xC9, 0x00, 0xF0, 0x36, 0xC8, 0xC0, 0x08, 0xD0, 0xF2,
    0xB1, 0xF4, 0x8D, 0x10, 0x06,
];
const EVENT_STREAM_SLOT_CALL_ADDRESS: u16 = 0x8211;
const EVENT_STREAM_SLOT_CALL: [u8; 16] = [
    0xA2, 0x02, 0xB5, 0xE1, 0xF0, 0x7C, 0x20, 0x07, 0x85, 0x10, 0x77, 0x20, 0x7E, 0x85, 0xB0, 0x79,
];
const EVENT_STREAM_READER_ADDRESS: u16 = 0x857E;
const EVENT_STREAM_READER: [u8; 16] = [
    0xB5, 0xE0, 0x85, 0xF4, 0xB5, 0xE1, 0x85, 0xF5, 0xA0, 0x00, 0xB1, 0xF4, 0x85, 0xF8, 0x10, 0xA1,
];
const RECORD_POINTER_DISPATCH_ADDRESS: u16 = 0x8692;
const RECORD_READER_ADDRESS: u16 = 0x86AD;
const RECORD_READER_BYTES: [u8; 41] = [
    0x84, 0xF9, 0xA0, 0x00, 0xB1, 0xF2, 0x9D, 0x13, 0x06, 0xC8, 0xB1, 0xF2, 0x9D, 0x16, 0x06, 0xC8,
    0xB1, 0xF2, 0x9D, 0x12, 0x06, 0xC8, 0xB1, 0xF2, 0x95, 0xE8, 0xC8, 0xB1, 0xF2, 0x95, 0xE9, 0xC8,
    0xB1, 0xF2, 0x95, 0xEC, 0xC8, 0xB1, 0xF2, 0x95, 0xED,
];

#[derive(Clone, Copy)]
struct TypedParserSlice {
    address: u16,
    byte_count: usize,
    sha1: &'static str,
    role: &'static str,
}

const CONTINUATION_PARSER_SLICES: [TypedParserSlice; 5] = [
    TypedParserSlice {
        address: 0x852F,
        byte_count: 79,
        sha1: "42791b3ae900ac7eec83276e6141c45063dc06ed",
        role: "audio note and nonnegative-command handlers",
    },
    TypedParserSlice {
        address: 0x8588,
        byte_count: 48,
        sha1: "70ebd7cc7a98f5d871be59dbd0e3d7ab016698d0",
        role: "audio opcode-class and FD/FE dispatch",
    },
    TypedParserSlice {
        address: 0x85FF,
        byte_count: 50,
        sha1: "c695a49f57ba66c7e7853c5f15eb0cb4e27cc2e9",
        role: "audio FD nested-stream frame push and transfer",
    },
    TypedParserSlice {
        address: 0x8631,
        byte_count: 31,
        sha1: "005f2a3644a8e55c93520f9f62afd761e083a260",
        role: "audio FE parent-stream restore and continuation",
    },
    TypedParserSlice {
        address: 0x8650,
        byte_count: 40,
        sha1: "d32ed1366476170a2e4fce9e70f787ccdc79934d",
        role: "audio nested-stream frame push and pop helpers",
    },
];

pub(super) fn bind_continuation_parser(source: &Rom) -> Result<()> {
    for slice in CONTINUATION_PARSER_SLICES {
        let bytes = source_bytes(source, AUDIO_BANK, slice.address, slice.byte_count)?;
        ensure!(
            sha1_hex(bytes) == slice.sha1,
            "source {} changed",
            slice.role
        );
        decode_rp2a03_sequence(bytes, slice.address, slice.role)?;
    }
    Ok(())
}

pub(super) fn bind_source_data_slice(
    source: &Rom,
    address: u16,
    byte_count: usize,
    expected_sha1: &str,
    role: &str,
) -> Result<String> {
    let bytes = source_bytes(source, AUDIO_BANK, address, byte_count)?;
    let actual_sha1 = sha1_hex(bytes);
    ensure!(actual_sha1 == expected_sha1, "{role} changed");
    Ok(actual_sha1)
}

pub(super) fn bind_event_directory_loader(source: &Rom) -> Result<()> {
    let expected = assemble_at(
        EVENT_DIRECTORY_LOADER_ADDRESS,
        &[
            Instruction::AslAccumulator,
            Instruction::Tax,
            Instruction::LdaAbsoluteX(EVENT_DIRECTORY_ADDRESS),
            Instruction::StaZeroPage(0xF4),
            Instruction::LdaAbsoluteX(EVENT_DIRECTORY_ADDRESS + 1),
            Instruction::StaZeroPage(0xF5),
        ],
    )?;
    ensure!(
        source_bytes(
            source,
            AUDIO_BANK,
            EVENT_DIRECTORY_LOADER_ADDRESS,
            expected.len(),
        )? == expected,
        "source audio event directory loader changed"
    );
    Ok(())
}

pub(super) fn bind_event_descriptor_and_stream_readers(source: &Rom) -> Result<()> {
    let descriptor_reader = source_bytes(
        source,
        AUDIO_BANK,
        EVENT_DESCRIPTOR_READER_ADDRESS,
        EVENT_DESCRIPTOR_READER.len(),
    )?;
    ensure!(
        descriptor_reader == EVENT_DESCRIPTOR_READER,
        "source audio event descriptor reader changed"
    );
    decode_rp2a03_sequence(
        descriptor_reader,
        EVENT_DESCRIPTOR_READER_ADDRESS,
        "source audio event descriptor reader",
    )?;

    let stream_slot_call = source_bytes(
        source,
        AUDIO_BANK,
        EVENT_STREAM_SLOT_CALL_ADDRESS,
        EVENT_STREAM_SLOT_CALL.len(),
    )?;
    ensure!(
        stream_slot_call == EVENT_STREAM_SLOT_CALL,
        "source audio event stream-slot call changed"
    );
    decode_rp2a03_sequence(
        stream_slot_call,
        EVENT_STREAM_SLOT_CALL_ADDRESS,
        "source audio event stream-slot call",
    )?;

    let stream_reader = source_bytes(
        source,
        AUDIO_BANK,
        EVENT_STREAM_READER_ADDRESS,
        EVENT_STREAM_READER.len(),
    )?;
    ensure!(
        stream_reader == EVENT_STREAM_READER,
        "source audio event stream reader changed"
    );
    decode_rp2a03_sequence(
        stream_reader,
        EVENT_STREAM_READER_ADDRESS,
        "source audio event stream reader",
    )?;
    Ok(())
}

pub(super) fn bind_record_parser(source: &Rom) -> Result<()> {
    let pointer_dispatch = assemble_at(
        RECORD_POINTER_DISPATCH_ADDRESS,
        &[
            Instruction::LdaIndirectY(0xF4),
            Instruction::StaZeroPage(0xF2),
            Instruction::Iny,
            Instruction::LdaIndirectY(0xF4),
            Instruction::StaZeroPage(0xF3),
            Instruction::Iny,
            Instruction::LdaZeroPage(0xF8),
            Instruction::CmpImmediate(0xC0),
            Instruction::BeqAbsolute(RECORD_READER_ADDRESS),
            Instruction::CmpImmediate(0xC1),
            Instruction::BeqAbsolute(RECORD_READER_ADDRESS),
            Instruction::CmpImmediate(0xC2),
            Instruction::BeqAbsolute(0x86E3),
            Instruction::JmpAbsolute(0x8588),
        ],
    )?;
    ensure!(
        source_bytes(
            source,
            AUDIO_BANK,
            RECORD_POINTER_DISPATCH_ADDRESS,
            pointer_dispatch.len(),
        )? == pointer_dispatch,
        "source audio record-pointer dispatch changed"
    );

    let record_reader = source_bytes(
        source,
        AUDIO_BANK,
        RECORD_READER_ADDRESS,
        RECORD_READER_BYTES.len(),
    )?;
    ensure!(
        record_reader == RECORD_READER_BYTES,
        "source C0/C1 seven-byte audio record reader changed"
    );
    decode_rp2a03_sequence(
        record_reader,
        RECORD_READER_ADDRESS,
        "source C0/C1 seven-byte audio record reader",
    )?;
    Ok(())
}
