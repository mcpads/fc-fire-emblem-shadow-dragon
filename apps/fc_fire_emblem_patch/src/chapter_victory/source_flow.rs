use anyhow::{Context, Result, ensure};
use retro_rp2a03::decode_bytes;
use serde::Serialize;

use crate::{rom::HEADER_SIZE, sha1_hex};

const PRG_BANK_SIZE: usize = 16 * 1024;
const SWITCHABLE_CPU_START: u16 = 0x8000;
const FIXED_CPU_START: u16 = 0xC000;
const FIXED_PRG_BANK: u8 = 0x0F;

const CASTLE_TILE_CODE: u8 = 0x4B;

const SELECT_COMMAND_ACTION: &[u8] = &[
    0x20, 0x5C, 0xE6, 0xAE, 0xEB, 0x05, 0x8E, 0xB3, 0x77, 0xCA, 0x8A, 0x20, 0x4C, 0xC3,
];
const COMMAND_ACTION_POINTER_TABLE: &[u8] = &[
    0xAD, 0x90, 0xAD, 0x90, 0xA1, 0x90, 0x7B, 0x90, 0x9B, 0x90, 0xB6, 0x90, 0xBF, 0x90, 0xC3, 0x90,
];
const SELECT_TERRAIN_ACTION_STATE: &[u8] = &[
    0xAD, 0x01, 0x05, 0x0A, 0xA8, 0xB9, 0x3D, 0xED, 0x85, 0x00, 0xB9, 0x3E, 0xED, 0x85, 0x01, 0xAC,
    0x00, 0x05, 0xB1, 0x00, 0xC9, 0x46, 0xD0, 0x04, 0xA9, 0x37, 0xD0, 0x2E, 0xA9, 0x3C, 0xD0, 0x2A,
];
const WRITE_TERRAIN_ACTION_STATE: &[u8] = &[0x85, 0x84, 0x60];
const VICTORY_MAIN_STATE_POINTER: &[u8] = &[0x90, 0x93];
const RUN_VICTORY_MAIN_STATE: &[u8] = &[0x20, 0x5C, 0xE6, 0x20, 0x27, 0xC0, 0x60];
const CALL_VICTORY_BANK: &[u8] = &[
    0xA9, 0x03, 0x20, 0xA6, 0xC9, 0x20, 0x06, 0x80, 0xA9, 0x06, 0x4C, 0xA6, 0xC9,
];
const ENTER_VICTORY_ROUTINE: &[u8] = &[0x4C, 0xE6, 0x99];
const SELECT_OUTER_SCREEN_ROUTE: &[u8] = &[0xA5, 0x24, 0xC9, 0x0B, 0xF0, 0x03, 0x4C, 0xBA, 0x9A];
const DISPATCH_OUTER_SCREEN_0C_STAGE: &[u8] = &[0xAD, 0x3E, 0x05, 0x20, 0x4C, 0xC3];
const OUTER_SCREEN_0C_STAGE_POINTERS: &[u8] = &[0xCC, 0x9A, 0x46, 0x9B, 0x8C, 0x9D, 0x3D, 0xC7];
const ADVANCE_OUTER_SCREEN_0C_STAGE: &[u8] = &[0xEE, 0x3E, 0x05, 0x60];
const RUN_OUTER_SCREEN_0C_STAGE_ZERO: &[u8] = &[
    0xA9, 0x00, 0x85, 0xA4, 0x85, 0xA5, 0x8D, 0x3B, 0x05, 0x8D, 0x41, 0x05, 0xAD, 0xE0, 0x05,
];
const RUN_OUTER_SCREEN_0C_STAGE_ONE: &[u8] = &[
    0xAD, 0x3B, 0x05, 0xC9, 0x80, 0x90, 0x62, 0x29, 0x7F, 0xF0, 0x5B, 0xA2, 0x27, 0xC9, 0x01,
];
const RUN_OUTER_SCREEN_0C_STAGE_TWO: &[u8] = &[
    0xA9, 0xC0, 0x20, 0x81, 0x9A, 0xF0, 0x7B, 0xAD, 0x7A, 0x76, 0xD0, 0x18, 0xAD, 0x74, 0x76,
];
const TERRAIN_ACTION_TABLES: &[u8] = &[
    0x00, 0x29, 0x33, 0x34, 0x3A, 0x3D, 0xA5, 0x4B, 0xAE, 0xAB, 0x2A, 0x38, 0x39, 0x37,
];
const TERRAIN_TILE_PREDICATE: &[u8] = &[
    0x48, 0xAD, 0x01, 0x05, 0x0A, 0xA8, 0xB9, 0x3D, 0xED, 0x85, 0x00, 0xB9, 0x3E, 0xED, 0x85, 0x01,
    0xAC, 0x00, 0x05, 0x68, 0xD1, 0x00, 0xF0, 0x01, 0x18, 0x60,
];
const CASTLE_LABEL_POINTER: &[u8] = &[0xD8, 0x91];
const CASTLE_LABEL_BYTES: &[u8] = &[0x0B, 0x2C, 0xED];

const SOURCE_SPECS: &[SourceRegionSpec] = &[
    SourceRegionSpec::code(
        "select_unit_command_action",
        0x06,
        0x905D,
        SELECT_COMMAND_ACTION,
    ),
    SourceRegionSpec::data(
        "unit_command_action_pointer_table",
        0x06,
        0x906B,
        COMMAND_ACTION_POINTER_TABLE,
    ),
    SourceRegionSpec::code(
        "select_terrain_action_state",
        0x06,
        0x907B,
        SELECT_TERRAIN_ACTION_STATE,
    ),
    SourceRegionSpec::code(
        "write_terrain_action_state",
        0x06,
        0x90C5,
        WRITE_TERRAIN_ACTION_STATE,
    ),
    SourceRegionSpec::data(
        "victory_main_state_pointer",
        0x06,
        0x89DF,
        VICTORY_MAIN_STATE_POINTER,
    ),
    SourceRegionSpec::code(
        "run_victory_main_state",
        0x06,
        0x9390,
        RUN_VICTORY_MAIN_STATE,
    ),
    SourceRegionSpec::code("call_victory_prg_bank", 0x0F, 0xC027, CALL_VICTORY_BANK),
    SourceRegionSpec::code("enter_victory_routine", 0x03, 0x8006, ENTER_VICTORY_ROUTINE),
    SourceRegionSpec::code(
        "select_outer_screen_victory_route",
        0x03,
        0x99E6,
        SELECT_OUTER_SCREEN_ROUTE,
    ),
    SourceRegionSpec::code(
        "dispatch_outer_screen_0c_stage",
        0x03,
        0x9ABA,
        DISPATCH_OUTER_SCREEN_0C_STAGE,
    ),
    SourceRegionSpec::data(
        "outer_screen_0c_stage_pointer_table",
        0x03,
        0x9AC0,
        OUTER_SCREEN_0C_STAGE_POINTERS,
    ),
    SourceRegionSpec::code(
        "advance_outer_screen_0c_stage",
        0x03,
        0x9AC8,
        ADVANCE_OUTER_SCREEN_0C_STAGE,
    ),
    SourceRegionSpec::code(
        "run_outer_screen_0c_stage_zero",
        0x03,
        0x9ACC,
        RUN_OUTER_SCREEN_0C_STAGE_ZERO,
    ),
    SourceRegionSpec::code(
        "run_outer_screen_0c_stage_one",
        0x03,
        0x9B46,
        RUN_OUTER_SCREEN_0C_STAGE_ONE,
    ),
    SourceRegionSpec::code(
        "run_outer_screen_0c_stage_two",
        0x03,
        0x9D8C,
        RUN_OUTER_SCREEN_0C_STAGE_TWO,
    ),
    SourceRegionSpec::data(
        "terrain_action_label_tables",
        0x0B,
        0x8457,
        TERRAIN_ACTION_TABLES,
    ),
    SourceRegionSpec::code(
        "match_current_map_tile",
        0x0B,
        0x8465,
        TERRAIN_TILE_PREDICATE,
    ),
    SourceRegionSpec::data("castle_label_pointer", 0x0B, 0x9032, CASTLE_LABEL_POINTER),
    SourceRegionSpec::data("castle_label", 0x0B, 0x91D8, CASTLE_LABEL_BYTES),
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
pub(super) struct CommandRouteBinding {
    command_selection_mask_address: u16,
    command_selection_mask_address_hex: &'static str,
    selected_command_bit: u8,
    terrain_handler: Location,
    current_row_address: u16,
    current_row_address_hex: &'static str,
    current_column_address: u16,
    current_column_address_hex: &'static str,
    runtime_row_pointer_table_address: u16,
    runtime_row_pointer_table_address_hex: &'static str,
    castle_tile_code: u8,
    castle_tile_code_hex: &'static str,
    selected_main_state: u8,
    selected_main_state_hex: &'static str,
    main_state_address: u16,
    main_state_address_hex: &'static str,
    outer_screen_state_address: u16,
    outer_screen_state_address_hex: &'static str,
    observed_outer_screen_state: u8,
    observed_outer_screen_state_hex: &'static str,
    victory_stage_address: u16,
    victory_stage_address_hex: &'static str,
    outer_screen_0c_stage_handlers: Vec<Location>,
    input_side_effect: &'static str,
}

#[derive(Debug, Serialize)]
pub(super) struct SourceRegionBinding {
    role: &'static str,
    region_kind: &'static str,
    location: Location,
    file_offset: usize,
    file_offset_hex: String,
    byte_count: usize,
    source_sha1: String,
    typed_instructions: Vec<TypedInstructionBinding>,
}

#[derive(Debug, Serialize)]
struct TypedInstructionBinding {
    cpu_address: u16,
    cpu_address_hex: String,
    mnemonic: String,
    addressing_mode: String,
    operand: String,
    control_flow: String,
}

#[derive(Debug, Serialize, Clone, PartialEq, Eq)]
struct Location {
    prg_bank: u8,
    prg_bank_hex: String,
    cpu_address: u16,
    cpu_address_hex: String,
}

pub(super) fn bind_command_route(
    prg: &[u8],
) -> Result<(CommandRouteBinding, Vec<SourceRegionBinding>)> {
    let source_regions = SOURCE_SPECS
        .iter()
        .copied()
        .map(|spec| bind_source_region(prg, spec))
        .collect::<Result<Vec<_>>>()?;

    let command_handlers = COMMAND_ACTION_POINTER_TABLE
        .chunks_exact(2)
        .map(|bytes| u16::from_le_bytes([bytes[0], bytes[1]]))
        .collect::<Vec<_>>();
    ensure!(
        command_handlers[3] == 0x907B,
        "terrain command handler pointer changed"
    );
    let stage_handlers = OUTER_SCREEN_0C_STAGE_POINTERS
        .chunks_exact(2)
        .map(|bytes| u16::from_le_bytes([bytes[0], bytes[1]]))
        .collect::<Vec<_>>();
    ensure!(
        stage_handlers == [0x9ACC, 0x9B46, 0x9D8C, 0xC73D],
        "outer-screen 0x0C stage handlers changed"
    );

    Ok((
        CommandRouteBinding {
            command_selection_mask_address: 0x05EB,
            command_selection_mask_address_hex: "0x05EB",
            selected_command_bit: 3,
            terrain_handler: location(0x06, command_handlers[3]),
            current_row_address: 0x0501,
            current_row_address_hex: "0x0501",
            current_column_address: 0x0500,
            current_column_address_hex: "0x0500",
            runtime_row_pointer_table_address: 0xED3D,
            runtime_row_pointer_table_address_hex: "0xED3D",
            castle_tile_code: CASTLE_TILE_CODE,
            castle_tile_code_hex: "0x4B",
            selected_main_state: 0x3C,
            selected_main_state_hex: "0x3C",
            main_state_address: 0x0084,
            main_state_address_hex: "0x0084",
            outer_screen_state_address: 0x0024,
            outer_screen_state_address_hex: "0x0024",
            observed_outer_screen_state: 0x0C,
            observed_outer_screen_state_hex: "0x0C",
            victory_stage_address: 0x053E,
            victory_stage_address_hex: "0x053E",
            outer_screen_0c_stage_handlers: stage_handlers
                .into_iter()
                .map(|address| location(if address >= 0xC000 { 0x0F } else { 0x03 }, address))
                .collect(),
            input_side_effect: "A on しろ does not merely close the menu: it selects command bit 3, writes main state 0x3C, bank-switches into the victory routine, and starts the outer-screen 0x0C staged handler",
        },
        source_regions,
    ))
}

fn bind_source_region(prg: &[u8], spec: SourceRegionSpec) -> Result<SourceRegionBinding> {
    let offset = prg_offset(spec.prg_bank, spec.cpu_address)?;
    let actual = prg
        .get(offset..offset + spec.bytes.len())
        .with_context(|| format!("{} is outside PRG", spec.role))?;
    ensure!(actual == spec.bytes, "{} source bytes changed", spec.role);
    let typed_instructions = match spec.kind {
        RegionKind::Code => decode_typed_sequence(actual, spec.cpu_address, spec.role)?,
        RegionKind::Data => Vec::new(),
    };

    Ok(SourceRegionBinding {
        role: spec.role,
        region_kind: match spec.kind {
            RegionKind::Code => "rp2a03_code",
            RegionKind::Data => "data",
        },
        location: location(spec.prg_bank, spec.cpu_address),
        file_offset: HEADER_SIZE + offset,
        file_offset_hex: format!("0x{:05X}", HEADER_SIZE + offset),
        byte_count: actual.len(),
        source_sha1: sha1_hex(actual),
        typed_instructions,
    })
}

fn decode_typed_sequence(
    bytes: &[u8],
    origin: u16,
    role: &str,
) -> Result<Vec<TypedInstructionBinding>> {
    let mut reports = Vec::new();
    let mut offset = 0_usize;
    while offset < bytes.len() {
        let instruction = decode_bytes(&bytes[offset..])
            .with_context(|| format!("decode {role} at +0x{offset:X} through typed RP2A03 ISA"))?;
        ensure!(
            instruction.opcode_is_documented(),
            "{role} contains undocumented selector at +0x{offset:X}"
        );
        let address = origin
            .checked_add(offset as u16)
            .context("typed RP2A03 address overflow")?;
        reports.push(TypedInstructionBinding {
            cpu_address: address,
            cpu_address_hex: format!("0x{address:04X}"),
            mnemonic: instruction.mnemonic().to_string(),
            addressing_mode: format!("{:?}", instruction.addressing_mode()),
            operand: format!("{:?}", instruction.operand()),
            control_flow: format!("{:?}", instruction.control_flow(address)),
        });
        offset += instruction.encoded_len();
    }
    ensure!(
        offset == bytes.len(),
        "{role} typed decode did not consume the full region"
    );
    Ok(reports)
}

fn prg_offset(prg_bank: u8, cpu_address: u16) -> Result<usize> {
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

fn location(prg_bank: u8, cpu_address: u16) -> Location {
    Location {
        prg_bank,
        prg_bank_hex: format!("0x{prg_bank:02X}"),
        cpu_address,
        cpu_address_hex: format!("0x{cpu_address:04X}"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::rom::PRG_SIZE;

    fn fixture_prg() -> Vec<u8> {
        let mut prg = vec![0; PRG_SIZE];
        for spec in SOURCE_SPECS {
            let offset = prg_offset(spec.prg_bank, spec.cpu_address).unwrap();
            prg[offset..offset + spec.bytes.len()].copy_from_slice(spec.bytes);
        }
        prg
    }

    #[test]
    fn binds_command_bit_three_to_the_staged_victory_route() {
        let (binding, regions) = bind_command_route(&fixture_prg()).unwrap();
        assert_eq!(binding.selected_command_bit, 3);
        assert_eq!(binding.terrain_handler.cpu_address, 0x907B);
        assert_eq!(binding.selected_main_state, 0x3C);
        assert_eq!(
            binding
                .outer_screen_0c_stage_handlers
                .iter()
                .map(|location| location.cpu_address)
                .collect::<Vec<_>>(),
            [0x9ACC, 0x9B46, 0x9D8C, 0xC73D]
        );
        assert!(
            regions
                .iter()
                .filter(|region| region.region_kind == "rp2a03_code")
                .all(|region| !region.typed_instructions.is_empty())
        );
    }

    #[test]
    fn typed_decode_rejects_a_truncated_region() {
        let error = decode_typed_sequence(&[0x4C, 0x00], 0x8000, "truncated_test").unwrap_err();
        assert!(error.to_string().contains("typed RP2A03 ISA"));
    }
}
