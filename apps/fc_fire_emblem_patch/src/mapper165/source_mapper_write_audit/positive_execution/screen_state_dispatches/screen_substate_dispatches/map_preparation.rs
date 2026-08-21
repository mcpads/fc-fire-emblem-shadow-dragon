use std::collections::BTreeSet;

use anyhow::{Context, Result, ensure};
use retro_rp2a03::{AddressingMode, Mnemonic, Operand, decode_bytes};

use crate::{
    rom::Rom,
    sha1_hex,
    source_direct_memory_writers::{DirectMemoryWriter, scan_direct_memory_writers},
    typed_source::decode_rp2a03_sequence,
};

use super::super::super::{
    chapter_map_loader::BoundChapterMapDimensions,
    unit_record_writers::BoundUnitRecordAddressDomain,
};
use super::{
    super::selector_transition_graph::{StateTransition, reachable_selectors},
    ScreenSubstateDispatch,
};

mod indirect_write_destinations;
mod nested_dispatches;

use indirect_write_destinations::bind_indirect_write_destinations;
#[cfg(test)]
use indirect_write_destinations::{
    DISPLAY_ROW_BASE, DISPLAY_ROW_POINTER_COUNT, DISPLAY_ROW_STRIDE,
    indexed_pointer_destination_ranges,
};
use nested_dispatches::bind_nested_map_preparation_dispatches;

const PRG_BANK_BYTE_COUNT: usize = 16 * 1024;
const MAP_PREPARATION_BANK: u8 = 0x03;
const FIXED_PRG_BANK: u8 = 0x0F;
const MAP_PREPARATION_STATE_ADDRESS: u16 = 0x053F;
const DISPATCH_ENTRY: u16 = 0x800C;
const DISPATCH_CALL: u16 = 0x800F;
const DISPATCH_TARGETS: [u16; 8] = [
    0x8022, 0x9346, 0x8026, 0x8270, 0x939A, 0x93EE, 0x802D, 0xC73D,
];
const DISPATCH_PREFIX: [u8; 6] = [0xAD, 0x3F, 0x05, 0x20, 0x4C, 0xC3];

#[derive(Clone, Copy)]
struct CodeRegion {
    start: u16,
    end: u16,
    sha1: &'static str,
    role: &'static str,
}

const CODE_REGIONS: [CodeRegion; 23] = [
    region(
        0x800C,
        0x8037,
        "f38a933f176a742d766756566df965a941194785",
        "map-preparation dispatcher and states zero, two, and six",
    ),
    region(
        0x8270,
        0x8334,
        "e81b41023166d3f36ede40be2299f65a04da40fa",
        "map-preparation state three",
    ),
    region(
        0x9346,
        0x939A,
        "52ee433d7a546ab8d796325f46ce5b88ffa0473c",
        "map-preparation state one",
    ),
    region(
        0x939A,
        0x93EE,
        "fedda5ed7e00f2e5296fff0a87cf7ce5ba602de0",
        "map-preparation state four",
    ),
    region(
        0x93EE,
        0x942B,
        "e8a9cb6df2e81776bb01b44383ae855ba7edd5f9",
        "map-preparation state five",
    ),
    region(
        0x8037,
        0x80A8,
        "4a902e003735a5d95997560c49215a3883df91a8",
        "compose the source-bound map-layer border",
    ),
    region(
        0x80A8,
        0x80CA,
        "47f7f83df5f80f9d5ca6b12b73ad9eca0eb1c4ca",
        "clear source-bound map-preparation display rows",
    ),
    region(
        0x80CA,
        0x8239,
        "9ca226b72cd1c55ba07b91faef759ae5af82cee2",
        "rewrite neighboring cells in source-bound map-preparation display rows",
    ),
    region(
        0x8222,
        0x8239,
        "d315396935b13504d86d663bf48fff69fb457c85",
        "select one source-bound map-layer row",
    ),
    region(
        0x8239,
        0x8270,
        "2c5f3630a8b27f59d01da8e4d8d0877481d5aa39",
        "select map-preparation display and runtime rows",
    ),
    region(
        0x8334,
        0x8398,
        "0353888ed6806af584d75826dd08a02ef033502d",
        "scan the first twenty enemy records during map preparation",
    ),
    region(
        0x8D6D,
        0x8DC0,
        "f59c4cb86488e705bdff01ecebe21969f0af24af",
        "rewrite the selected allied record shifted workspace",
    ),
    region(
        0x8FD2,
        0x8FDB,
        "ca0d65506c25bfd70d8425aff53ccf4bcabbd03c",
        "select the enemy record base during map preparation",
    ),
    region(
        0x91BC,
        0x91D0,
        "a8a3ce38c5fcffc63b6b1e937ea5a00e540a28e8",
        "advance allied and enemy unit-record pointers",
    ),
    region(
        0x91D0,
        0x9271,
        "055988d54cadf98073021f476234d6f003112dec",
        "select and normalize one map-preparation enemy event",
    ),
    region(
        0x9271,
        0x932A,
        "65f06ab741b8c5d2f75ba433742059517d2aae12",
        "compose the bounded enemy-record staging source",
    ),
    region(
        0x932A,
        0x9346,
        "08b64dd7901b1ff500845cf7ffc98970799c07d8",
        "copy one staged enemy record into the selected record",
    ),
    region(
        0x942B,
        0x9466,
        "6fd46ba08d1c77ccadcccdece2dda25a4f092708",
        "publish a selected terrain marker into a runtime row",
    ),
    region(
        0x8A82,
        0x8AD8,
        "0ba430ae9fa07e73218dc12dd668afa01a61438b",
        "rewrite neighboring cells in a selected map-layer row",
    ),
    region(
        0x8BD8,
        0x8C83,
        "2e68e6f992c2d1b92bc80fc784e146be22cf4fef",
        "search connected cells across source-bound map-layer rows",
    ),
    region(
        0x8C83,
        0x8CCE,
        "a7d01147ef86260f43144b88a3bed060d7826117",
        "normalize source-bound map-layer cells",
    ),
    region(
        0x8FE4,
        0x9080,
        "2d35e98f1ddd5fbfd706eca7cc080a8b258bd3e4",
        "seed a source-bound map-layer traversal",
    ),
    region(
        0x9080,
        0x913C,
        "2f16abaad220a8fd38f89d1d56ff9f50bf803c0f",
        "rewrite traversed cells in source-bound map-layer rows",
    ),
];

const fn region(start: u16, end: u16, sha1: &'static str, role: &'static str) -> CodeRegion {
    CodeRegion {
        start,
        end,
        sha1,
        role,
    }
}

const DIRECT_STATE_WRITERS: [DirectMemoryWriter; 7] = [
    writer(0x8022, 0xEE),
    writer(0x8029, 0xEE),
    writer(0x8033, 0x8D),
    writer(0x832C, 0xEE),
    writer(0x9396, 0xEE),
    writer(0x93EA, 0xEE),
    writer(0x9427, 0xEE),
];

const fn writer(cpu_address: u16, opcode: u8) -> DirectMemoryWriter {
    DirectMemoryWriter::new(
        MAP_PREPARATION_BANK,
        cpu_address,
        opcode,
        MAP_PREPARATION_STATE_ADDRESS,
    )
}

#[derive(Clone, Copy)]
struct ExpectedSourceInstruction {
    address: u16,
    mnemonic: Mnemonic,
    mode: AddressingMode,
    operand: Operand,
}

impl ExpectedSourceInstruction {
    const fn immediate(address: u16, mnemonic: Mnemonic, value: u8) -> Self {
        Self::new(
            address,
            mnemonic,
            AddressingMode::Immediate,
            Operand::Byte(value),
        )
    }

    const fn zero_page(address: u16, mnemonic: Mnemonic, value: u8) -> Self {
        Self::new(
            address,
            mnemonic,
            AddressingMode::ZeroPage,
            Operand::Byte(value),
        )
    }

    const fn indirect_indexed_y(address: u16, mnemonic: Mnemonic, pointer: u8) -> Self {
        Self::new(
            address,
            mnemonic,
            AddressingMode::ZeroPageIndirectIndexedY,
            Operand::Byte(pointer),
        )
    }

    const fn absolute(address: u16, mnemonic: Mnemonic, operand: u16) -> Self {
        Self::new(
            address,
            mnemonic,
            AddressingMode::Absolute,
            Operand::Word(operand),
        )
    }

    const fn absolute_indexed_x(address: u16, mnemonic: Mnemonic, operand: u16) -> Self {
        Self::new(
            address,
            mnemonic,
            AddressingMode::AbsoluteX,
            Operand::Word(operand),
        )
    }

    const fn absolute_indexed_y(address: u16, mnemonic: Mnemonic, operand: u16) -> Self {
        Self::new(
            address,
            mnemonic,
            AddressingMode::AbsoluteY,
            Operand::Word(operand),
        )
    }

    const fn new(address: u16, mnemonic: Mnemonic, mode: AddressingMode, operand: Operand) -> Self {
        Self {
            address,
            mnemonic,
            mode,
            operand,
        }
    }
}

const POINTER_DOMAIN_INSTRUCTIONS: &[ExpectedSourceInstruction] = &[
    ExpectedSourceInstruction::absolute(0x8045, Mnemonic::Jsr, 0x8222),
    ExpectedSourceInstruction::indirect_indexed_y(0x8062, Mnemonic::Sta, 0x6C),
    ExpectedSourceInstruction::absolute(0x80B6, Mnemonic::Jsr, 0x8239),
    ExpectedSourceInstruction::indirect_indexed_y(0x80BD, Mnemonic::Sta, 0x9B),
    ExpectedSourceInstruction::absolute(0x8117, Mnemonic::Jsr, 0x8239),
    ExpectedSourceInstruction::indirect_indexed_y(0x811E, Mnemonic::Sta, 0x9B),
    ExpectedSourceInstruction::absolute(0x8121, Mnemonic::Jsr, 0x8239),
    ExpectedSourceInstruction::indirect_indexed_y(0x8128, Mnemonic::Sta, 0x9B),
    ExpectedSourceInstruction::indirect_indexed_y(0x821F, Mnemonic::Sta, 0x9B),
    ExpectedSourceInstruction::immediate(0x8224, Mnemonic::Cmp, 0x1E),
    ExpectedSourceInstruction::absolute_indexed_x(0x822C, Mnemonic::Lda, 0xED01),
    ExpectedSourceInstruction::zero_page(0x822F, Mnemonic::Sta, 0x6C),
    ExpectedSourceInstruction::absolute_indexed_x(0x8231, Mnemonic::Lda, 0xED02),
    ExpectedSourceInstruction::zero_page(0x8234, Mnemonic::Sta, 0x6D),
    ExpectedSourceInstruction::immediate(0x823B, Mnemonic::Cmp, 0x1E),
    ExpectedSourceInstruction::absolute_indexed_x(0x8243, Mnemonic::Lda, 0xED79),
    ExpectedSourceInstruction::zero_page(0x8246, Mnemonic::Sta, 0x9B),
    ExpectedSourceInstruction::absolute_indexed_x(0x8248, Mnemonic::Lda, 0xED7A),
    ExpectedSourceInstruction::zero_page(0x824B, Mnemonic::Sta, 0x9C),
    ExpectedSourceInstruction::absolute(0x8334, Mnemonic::Jsr, 0x8FD2),
    ExpectedSourceInstruction::absolute(0x834B, Mnemonic::Jsr, 0x91D0),
    ExpectedSourceInstruction::immediate(0x8362, Mnemonic::Ldy, 0x16),
    ExpectedSourceInstruction::indirect_indexed_y(0x8364, Mnemonic::Sta, 0x9D),
    ExpectedSourceInstruction::immediate(0x8389, Mnemonic::Cpx, 0x14),
    ExpectedSourceInstruction::immediate(0x838F, Mnemonic::Lda, 0x1B),
    ExpectedSourceInstruction::absolute(0x8391, Mnemonic::Jsr, 0x91C6),
    ExpectedSourceInstruction::absolute(0x8A86, Mnemonic::Jsr, 0x8222),
    ExpectedSourceInstruction::absolute(0x8AAD, Mnemonic::Jsr, 0x8222),
    ExpectedSourceInstruction::indirect_indexed_y(0x8ACB, Mnemonic::Sta, 0x6C),
    ExpectedSourceInstruction::indirect_indexed_y(0x8AD5, Mnemonic::Sta, 0x6C),
    ExpectedSourceInstruction::absolute(0x8BF7, Mnemonic::Jsr, 0x8222),
    ExpectedSourceInstruction::indirect_indexed_y(0x8BFC, Mnemonic::Sta, 0x6C),
    ExpectedSourceInstruction::absolute_indexed_x(0x8C13, Mnemonic::Lda, 0xED01),
    ExpectedSourceInstruction::zero_page(0x8C16, Mnemonic::Sta, 0x6C),
    ExpectedSourceInstruction::absolute_indexed_x(0x8C18, Mnemonic::Lda, 0xED02),
    ExpectedSourceInstruction::zero_page(0x8C1B, Mnemonic::Sta, 0x6D),
    ExpectedSourceInstruction::indirect_indexed_y(0x8CC1, Mnemonic::Sta, 0x6C),
    ExpectedSourceInstruction::zero_page(0x8D6D, Mnemonic::Lda, 0x66),
    ExpectedSourceInstruction::absolute(0x8D71, Mnemonic::Jsr, 0x8DB4),
    ExpectedSourceInstruction::indirect_indexed_y(0x8D7A, Mnemonic::Sta, 0x9F),
    ExpectedSourceInstruction::absolute(0x8DA2, Mnemonic::Jsr, 0x8DB4),
    ExpectedSourceInstruction::indirect_indexed_y(0x8DA8, Mnemonic::Sta, 0x9F),
    ExpectedSourceInstruction::zero_page(0x8DB6, Mnemonic::Lda, 0x65),
    ExpectedSourceInstruction::zero_page(0x8DB8, Mnemonic::Sta, 0x9F),
    ExpectedSourceInstruction::immediate(0x8DBA, Mnemonic::Lda, 0x36),
    ExpectedSourceInstruction::absolute(0x8DBC, Mnemonic::Jsr, 0x91BC),
    ExpectedSourceInstruction::immediate(0x8FD2, Mnemonic::Lda, 0x78),
    ExpectedSourceInstruction::zero_page(0x8FD4, Mnemonic::Sta, 0x9D),
    ExpectedSourceInstruction::immediate(0x8FD6, Mnemonic::Lda, 0x70),
    ExpectedSourceInstruction::zero_page(0x8FD8, Mnemonic::Sta, 0x9E),
    ExpectedSourceInstruction::absolute(0x8FEA, Mnemonic::Jsr, 0x8222),
    ExpectedSourceInstruction::indirect_indexed_y(0x8FEF, Mnemonic::Sta, 0x6C),
    ExpectedSourceInstruction::absolute_indexed_y(0x9107, Mnemonic::Lda, 0xED01),
    ExpectedSourceInstruction::zero_page(0x910A, Mnemonic::Sta, 0x6C),
    ExpectedSourceInstruction::absolute_indexed_y(0x910C, Mnemonic::Lda, 0xED02),
    ExpectedSourceInstruction::zero_page(0x910F, Mnemonic::Sta, 0x6D),
    ExpectedSourceInstruction::indirect_indexed_y(0x912D, Mnemonic::Sta, 0x6C),
    ExpectedSourceInstruction::zero_page(0x91BD, Mnemonic::Adc, 0x9F),
    ExpectedSourceInstruction::zero_page(0x91BF, Mnemonic::Sta, 0x9F),
    ExpectedSourceInstruction::zero_page(0x91C7, Mnemonic::Adc, 0x9D),
    ExpectedSourceInstruction::zero_page(0x91C9, Mnemonic::Sta, 0x9D),
    ExpectedSourceInstruction::absolute(0x926B, Mnemonic::Jsr, 0x932A),
    ExpectedSourceInstruction::immediate(0x932E, Mnemonic::Lda, 0xF4),
    ExpectedSourceInstruction::zero_page(0x9330, Mnemonic::Sta, 0x02),
    ExpectedSourceInstruction::immediate(0x9332, Mnemonic::Lda, 0x76),
    ExpectedSourceInstruction::zero_page(0x9334, Mnemonic::Sta, 0x03),
    ExpectedSourceInstruction::immediate(0x9336, Mnemonic::Ldy, 0x00),
    ExpectedSourceInstruction::indirect_indexed_y(0x933A, Mnemonic::Sta, 0x9D),
    ExpectedSourceInstruction::immediate(0x933D, Mnemonic::Cpy, 0x1B),
    ExpectedSourceInstruction::absolute(0x9346, Mnemonic::Jsr, 0x8FD2),
    ExpectedSourceInstruction::indirect_indexed_y(0x9387, Mnemonic::Sta, 0x9D),
    ExpectedSourceInstruction::immediate(0x938E, Mnemonic::Lda, 0x1B),
    ExpectedSourceInstruction::absolute(0x9390, Mnemonic::Jsr, 0x91C6),
    ExpectedSourceInstruction::absolute(0x939A, Mnemonic::Jsr, 0x8267),
    ExpectedSourceInstruction::indirect_indexed_y(0x93DB, Mnemonic::Sta, 0x9F),
    ExpectedSourceInstruction::immediate(0x93E2, Mnemonic::Lda, 0x1B),
    ExpectedSourceInstruction::absolute(0x93E4, Mnemonic::Jsr, 0x91BC),
    ExpectedSourceInstruction::zero_page(0x9430, Mnemonic::Ldx, 0xB3),
    ExpectedSourceInstruction::absolute(0x9432, Mnemonic::Jsr, 0x8250),
    ExpectedSourceInstruction::zero_page(0x9435, Mnemonic::Ldy, 0xB2),
    ExpectedSourceInstruction::indirect_indexed_y(0x9439, Mnemonic::Sta, 0x04),
];

const TRANSITIONS: [StateTransition; 7] = [
    StateTransition::new(0, 1),
    StateTransition::new(1, 2),
    StateTransition::new(2, 3),
    StateTransition::new(3, 4),
    StateTransition::new(4, 5),
    StateTransition::new(5, 6),
    StateTransition::new(6, 0),
];

pub(super) fn bind_map_preparation_dispatches(
    source: &Rom,
    unit_record_domain: &BoundUnitRecordAddressDomain,
    chapter_map_dimensions: &BoundChapterMapDimensions,
) -> Result<Vec<ScreenSubstateDispatch>> {
    let mut dispatches = vec![bind_map_preparation_lifecycle(
        source,
        unit_record_domain,
        chapter_map_dimensions,
    )?];
    dispatches.extend(bind_nested_map_preparation_dispatches(source)?);
    Ok(dispatches)
}

fn bind_map_preparation_lifecycle(
    source: &Rom,
    unit_record_domain: &BoundUnitRecordAddressDomain,
    chapter_map_dimensions: &BoundChapterMapDimensions,
) -> Result<ScreenSubstateDispatch> {
    source.verify_supported_japanese()?;
    for region in CODE_REGIONS {
        let bytes = source_bytes(source, region.start, usize::from(region.end - region.start))?;
        ensure!(
            sha1_hex(bytes) == region.sha1,
            "{} source bytes changed",
            region.role
        );
    }
    for instruction in POINTER_DOMAIN_INSTRUCTIONS {
        ensure_source_instruction(source, instruction)?;
    }
    let dispatch_prefix = source_bytes(source, DISPATCH_ENTRY, DISPATCH_PREFIX.len())?;
    ensure!(
        dispatch_prefix == DISPATCH_PREFIX,
        "map-preparation dispatch prefix changed"
    );
    decode_rp2a03_sequence(
        dispatch_prefix,
        DISPATCH_ENTRY,
        "load and dispatch map-preparation state",
    )?;

    let handler_domain = (0..u8::try_from(DISPATCH_TARGETS.len())?).collect::<BTreeSet<_>>();
    let dispatch = crate::mapper165::inline_pointer_dispatch::bind_inline_pointer_dispatch(
        source,
        MAP_PREPARATION_BANK,
        DISPATCH_CALL,
        handler_domain.iter().copied(),
        "map-preparation state dispatch",
    )?;
    ensure!(
        dispatch.targets_in_selector_order() == DISPATCH_TARGETS,
        "map-preparation state handlers changed"
    );

    let direct_writers =
        scan_direct_memory_writers(source.prg(), &[MAP_PREPARATION_STATE_ADDRESS])?;
    ensure!(
        direct_writers == BTreeSet::from(DIRECT_STATE_WRITERS),
        "map-preparation direct state-writer census changed: expected {:?}, found {direct_writers:?}",
        BTreeSet::from(DIRECT_STATE_WRITERS),
    );
    for transition in TRANSITIONS {
        bind_transition(source, transition)?;
    }
    let produced_selectors =
        reachable_selectors("map-preparation state", &handler_domain, [0], TRANSITIONS)?;
    ensure!(
        produced_selectors == (0..=6).collect::<BTreeSet<_>>() && !produced_selectors.contains(&7),
        "map-preparation producer closure no longer owns states zero through six while excluding dormant state seven"
    );
    let indirect_write_destinations =
        bind_indirect_write_destinations(source, unit_record_domain, chapter_map_dimensions)?;

    Ok(ScreenSubstateDispatch {
        prg_bank: MAP_PREPARATION_BANK,
        call_address: DISPATCH_CALL,
        handler_domain,
        selector_memory_address: Some(MAP_PREPARATION_STATE_ADDRESS),
        source_bound_produced_selectors: Some(produced_selectors),
        indirect_write_destinations,
        role: "map-preparation state dispatch",
    })
}

fn ensure_source_instruction(source: &Rom, expected: &ExpectedSourceInstruction) -> Result<()> {
    let instruction =
        decode_bytes(source_bytes(source, expected.address, 3)?).with_context(|| {
            format!(
                "decode map-preparation pointer-domain instruction at 03:${:04X}",
                expected.address
            )
        })?;
    ensure!(
        instruction.mnemonic() == expected.mnemonic
            && instruction.addressing_mode() == expected.mode
            && instruction.operand() == expected.operand,
        "map-preparation pointer-domain instruction changed at 03:${:04X}",
        expected.address,
    );
    Ok(())
}

fn bind_transition(source: &Rom, transition: StateTransition) -> Result<()> {
    let (address, expected): (u16, &[u8]) = match (transition.from, transition.to) {
        (0, 1) => (0x8022, &[0xEE, 0x3F, 0x05]),
        (1, 2) => (0x9396, &[0xEE, 0x3F, 0x05]),
        (2, 3) => (0x8029, &[0xEE, 0x3F, 0x05]),
        (3, 4) => (0x832C, &[0xEE, 0x3F, 0x05]),
        (4, 5) => (0x93EA, &[0xEE, 0x3F, 0x05]),
        (5, 6) => (0x9427, &[0xEE, 0x3F, 0x05]),
        (6, 0) => (0x8031, &[0xA9, 0x00, 0x8D, 0x3F, 0x05, 0x60]),
        _ => anyhow::bail!(
            "map-preparation transition {} -> {} has no source encoding",
            transition.from,
            transition.to,
        ),
    };
    let actual = source_bytes(source, address, expected.len())?;
    ensure!(
        actual == expected,
        "map-preparation transition {} -> {} changed",
        transition.from,
        transition.to,
    );
    decode_rp2a03_sequence(actual, address, "advance map-preparation state")?;
    Ok(())
}

fn source_bytes(source: &Rom, address: u16, byte_count: usize) -> Result<&[u8]> {
    ensure!(
        (0x8000..0xC000).contains(&address)
            && usize::from(address - 0x8000)
                .checked_add(byte_count)
                .is_some_and(|end| end <= PRG_BANK_BYTE_COUNT),
        "map-preparation source range is outside bank 03"
    );
    let start = usize::from(MAP_PREPARATION_BANK)
        .checked_mul(PRG_BANK_BYTE_COUNT)
        .and_then(|base| base.checked_add(usize::from(address - 0x8000)))
        .context("map-preparation source offset overflow")?;
    source
        .prg()
        .get(start..start + byte_count)
        .context("map-preparation source range exceeds PRG")
}

fn fixed_source_bytes(source: &Rom, address: u16, byte_count: usize) -> Result<&[u8]> {
    ensure!(
        address >= 0xC000
            && usize::from(address - 0xC000)
                .checked_add(byte_count)
                .is_some_and(|end| end <= PRG_BANK_BYTE_COUNT),
        "map-preparation fixed source range is outside bank 0F"
    );
    let start = usize::from(FIXED_PRG_BANK)
        .checked_mul(PRG_BANK_BYTE_COUNT)
        .and_then(|base| base.checked_add(usize::from(address - 0xC000)))
        .context("map-preparation fixed source offset overflow")?;
    source
        .prg()
        .get(start..start + byte_count)
        .context("map-preparation fixed source range exceeds PRG")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn transition_closure_excludes_the_dormant_eighth_handler() {
        let handlers = (0..8).collect::<BTreeSet<_>>();
        assert_eq!(
            reachable_selectors("map preparation", &handlers, [0], TRANSITIONS).unwrap(),
            (0..=6).collect::<BTreeSet<_>>()
        );
    }

    #[test]
    fn chapter_dimensions_bound_every_display_row_destination() {
        let pointers = (0..DISPLAY_ROW_POINTER_COUNT)
            .map(|index| DISPLAY_ROW_BASE + u16::try_from(index).unwrap() * DISPLAY_ROW_STRIDE)
            .collect::<Vec<_>>();
        let ranges = indexed_pointer_destination_ranges(&pointers, 0x1F).unwrap();

        assert_eq!(ranges, vec![0x7AF0..=0x7EAF]);
        assert!(ranges.iter().all(|range| *range.end() < 0x8000));
    }

    #[test]
    fn display_row_destination_rejects_a_mapper_space_overlap() {
        let error = indexed_pointer_destination_ranges(&[0x7FF0], 0x1F).unwrap_err();

        assert!(error.to_string().contains("reaches mapper space"));
    }
}
