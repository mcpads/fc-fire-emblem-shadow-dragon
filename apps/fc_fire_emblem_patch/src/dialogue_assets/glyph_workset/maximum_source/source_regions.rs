use anyhow::{Context, Result, ensure};
use serde::Serialize;

use crate::{
    rom::HEADER_SIZE,
    sha1_hex,
    typed_source::{TypedInstructionBinding, decode_rp2a03_sequence},
};

pub(super) const PRG_BANK_SIZE: usize = 16 * 1024;
const SWITCHABLE_CPU_START: u16 = 0x8000;
const FIXED_CPU_START: u16 = 0xC000;
const FIXED_PRG_BANK: u8 = 0x0F;

const LOOK_UP_CHAPTER_EVENT: &[u8] = &[
    0xAD, 0x01, 0x05, 0x0A, 0xA8, 0xB9, 0x3D, 0xED, 0x85, 0x00, 0xB9, 0x3E, 0xED, 0x85, 0x01, 0xAC,
    0x00, 0x05, 0xB1, 0x00, 0xA6, 0xA5, 0xD0, 0x11, 0xC9, 0x4B, 0xF0, 0x11, 0xC9, 0xAE, 0xF0, 0x0D,
    0xC9, 0xA5, 0xF0, 0x09, 0xAC, 0x7E, 0x76, 0xF0, 0x1B, 0xC9, 0xAB, 0xD0, 0x17, 0x85, 0xA4, 0xAD,
    0x01, 0x05, 0x8D, 0x39, 0x05, 0xAD, 0x00, 0x05, 0x8D, 0x38, 0x05, 0x20, 0xAC, 0x9E, 0x90, 0x04,
    0xEE, 0x3E, 0x05, 0x60,
];
const READ_CHAPTER_EVENT_RECORD: &[u8] = &[
    0xAC, 0x74, 0x76, 0x88, 0x98, 0x0A, 0xA8, 0xB9, 0xF1, 0xA0, 0x85, 0x00, 0xB9, 0xF2, 0xA0, 0x85,
    0x01, 0xA0, 0x00, 0xB1, 0x00, 0xC9, 0x01, 0x90, 0x20, 0xCD, 0x39, 0x05, 0xD0, 0x1C, 0xC8, 0xB1,
    0x00, 0xCD, 0x38, 0x05, 0xD0, 0x14, 0xC8, 0xB1, 0x00, 0x8D, 0x3B, 0x05, 0xC8, 0xB1, 0x00, 0x8D,
    0x3C, 0x05, 0xC8, 0xB1, 0x00, 0x8D, 0x3D, 0x05, 0x38, 0x60,
];
const ROUTE_ZERO_EVENT_TO_DIALOGUE: &[u8] =
    &[0xAD, 0x3B, 0x05, 0xC9, 0x00, 0xD0, 0x4C, 0x4C, 0x6D, 0x9D];
const WRITE_DIALOGUE_ENTRY: &[u8] = &[
    0xAD, 0x7A, 0x76, 0xD0, 0x06, 0xAD, 0x3D, 0x05, 0x8D, 0xF2, 0x06, 0xAD, 0x3C, 0x05, 0x8D, 0xF1,
    0x77, 0xA9, 0x00, 0x8D, 0xF0, 0x77, 0xA9, 0x01, 0x8D, 0xF7, 0x77, 0xEE, 0x3E, 0x05, 0x60,
];
const SELECT_CHAPTER_CLEAR_DIALOGUE: &[u8] = &[0xA9, 0xC0, 0x20, 0x81, 0x9A];
const WRITE_DIALOGUE_SELECTOR: &[u8] = &[
    0x8D, 0xF4, 0x77, 0xA9, 0x00, 0x85, 0x44, 0xA9, 0x0A, 0x20, 0xFA, 0xC9, 0xAD, 0x03, 0x78, 0x60,
];
const OTHER_MAIN_STATE_POINTER: &[u8] = &[0xC2, 0xB2];
const RUN_OTHER_MAIN_STATE: &[u8] = &[0x4C, 0x41, 0xC0];
const CALL_OTHER_PRG_ENTRY: &[u8] = &[
    0xA9, 0x03, 0x20, 0xA6, 0xC9, 0x20, 0x09, 0x80, 0xA9, 0x06, 0x4C, 0xA6, 0xC9,
];
const ENTER_OTHER_ROUTE: &[u8] = &[0x4C, 0xEE, 0x9E];
const DISPATCH_OTHER_ROUTE_STAGE: &[u8] = &[0xAD, 0x3E, 0x05, 0x20, 0x4C, 0xC3];
const OTHER_ROUTE_STAGE_POINTERS: &[u8] = &[0xFC, 0x9E, 0x78, 0x9D, 0x1B, 0x9F, 0x3D, 0xC7];
const SELECT_OTHER_DIALOGUE: &[u8] = &[0xA9, 0x30, 0x20, 0x81, 0x9A];

const SOURCE_SPECS: &[SourceRegionSpec] = &[
    SourceRegionSpec::code("look_up_chapter_event", 0x03, 0x9AFF, LOOK_UP_CHAPTER_EVENT),
    SourceRegionSpec::code(
        "read_chapter_event_record",
        0x03,
        0x9EAC,
        READ_CHAPTER_EVENT_RECORD,
    ),
    SourceRegionSpec::code(
        "route_zero_event_to_dialogue",
        0x03,
        0x9BCA,
        ROUTE_ZERO_EVENT_TO_DIALOGUE,
    ),
    SourceRegionSpec::code("write_dialogue_entry", 0x03, 0x9D6D, WRITE_DIALOGUE_ENTRY),
    SourceRegionSpec::code(
        "select_chapter_clear_dialogue",
        0x03,
        0x9D8C,
        SELECT_CHAPTER_CLEAR_DIALOGUE,
    ),
    SourceRegionSpec::code(
        "write_dialogue_selector",
        0x03,
        0x9A81,
        WRITE_DIALOGUE_SELECTOR,
    ),
    SourceRegionSpec::data(
        "other_main_state_pointer",
        0x06,
        0x89D5,
        OTHER_MAIN_STATE_POINTER,
    ),
    SourceRegionSpec::code("run_other_main_state", 0x06, 0xB2C2, RUN_OTHER_MAIN_STATE),
    SourceRegionSpec::code("call_other_prg_entry", 0x0F, 0xC041, CALL_OTHER_PRG_ENTRY),
    SourceRegionSpec::code("enter_other_route", 0x03, 0x8009, ENTER_OTHER_ROUTE),
    SourceRegionSpec::code(
        "dispatch_other_route_stage",
        0x03,
        0x9EEE,
        DISPATCH_OTHER_ROUTE_STAGE,
    ),
    SourceRegionSpec::data(
        "other_route_stage_pointer_table",
        0x03,
        0x9EF4,
        OTHER_ROUTE_STAGE_POINTERS,
    ),
    SourceRegionSpec::code("select_other_dialogue", 0x03, 0x9F1B, SELECT_OTHER_DIALOGUE),
];

#[derive(Clone, Copy)]
enum RegionKind {
    Code,
    Data,
}

#[derive(Clone, Copy)]
struct SourceRegionSpec {
    role: &'static str,
    prg_bank: u8,
    cpu_address: u16,
    bytes: &'static [u8],
    kind: RegionKind,
}

impl SourceRegionSpec {
    const fn code(
        role: &'static str,
        prg_bank: u8,
        cpu_address: u16,
        bytes: &'static [u8],
    ) -> Self {
        Self {
            role,
            prg_bank,
            cpu_address,
            bytes,
            kind: RegionKind::Code,
        }
    }

    const fn data(
        role: &'static str,
        prg_bank: u8,
        cpu_address: u16,
        bytes: &'static [u8],
    ) -> Self {
        Self {
            role,
            prg_bank,
            cpu_address,
            bytes,
            kind: RegionKind::Data,
        }
    }
}

#[derive(Debug, Serialize)]
pub(super) struct SourceRegionBinding {
    role: &'static str,
    pub(super) region_kind: &'static str,
    prg_bank: u8,
    prg_bank_hex: String,
    cpu_address: u16,
    cpu_address_hex: String,
    file_offset: usize,
    file_offset_hex: String,
    byte_count: usize,
    source_sha1: String,
    pub(super) typed_instructions: Vec<TypedInstructionBinding>,
}

pub(super) fn bind_source_regions(prg: &[u8]) -> Result<Vec<SourceRegionBinding>> {
    SOURCE_SPECS
        .iter()
        .copied()
        .map(|spec| bind_source_region(prg, spec))
        .collect()
}

fn bind_source_region(prg: &[u8], spec: SourceRegionSpec) -> Result<SourceRegionBinding> {
    let offset = prg_offset(spec.prg_bank, spec.cpu_address)?;
    let actual = prg
        .get(offset..offset + spec.bytes.len())
        .with_context(|| format!("{} is outside PRG", spec.role))?;
    ensure!(actual == spec.bytes, "{} source bytes changed", spec.role);
    let typed_instructions = match spec.kind {
        RegionKind::Code => decode_rp2a03_sequence(actual, spec.cpu_address, spec.role)?,
        RegionKind::Data => Vec::new(),
    };
    Ok(SourceRegionBinding {
        role: spec.role,
        region_kind: match spec.kind {
            RegionKind::Code => "rp2a03_code",
            RegionKind::Data => "data",
        },
        prg_bank: spec.prg_bank,
        prg_bank_hex: format!("0x{:02X}", spec.prg_bank),
        cpu_address: spec.cpu_address,
        cpu_address_hex: format!("0x{:04X}", spec.cpu_address),
        file_offset: HEADER_SIZE + offset,
        file_offset_hex: format!("0x{:05X}", HEADER_SIZE + offset),
        byte_count: actual.len(),
        source_sha1: sha1_hex(actual),
        typed_instructions,
    })
}

pub(super) fn prg_offset(prg_bank: u8, cpu_address: u16) -> Result<usize> {
    let bank_offset = if prg_bank == FIXED_PRG_BANK {
        ensure!(
            cpu_address >= FIXED_CPU_START,
            "fixed-bank address is below 0xC000"
        );
        usize::from(cpu_address - FIXED_CPU_START)
    } else {
        ensure!(
            (SWITCHABLE_CPU_START..FIXED_CPU_START).contains(&cpu_address),
            "switchable-bank address is outside 0x8000..0xBFFF"
        );
        usize::from(cpu_address - SWITCHABLE_CPU_START)
    };
    Ok(usize::from(prg_bank) * PRG_BANK_SIZE + bank_offset)
}

#[cfg(test)]
pub(super) fn install_source_fixture(prg: &mut [u8]) {
    for spec in SOURCE_SPECS {
        let offset = prg_offset(spec.prg_bank, spec.cpu_address).unwrap();
        prg[offset..offset + spec.bytes.len()].copy_from_slice(spec.bytes);
    }
}
