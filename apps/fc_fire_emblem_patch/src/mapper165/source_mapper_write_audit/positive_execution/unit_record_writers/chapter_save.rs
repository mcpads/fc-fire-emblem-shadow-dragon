use std::collections::{BTreeMap, BTreeSet};

use anyhow::{Context, Result, ensure};
use retro_rp2a03::{AddressingMode, Mnemonic, Operand, decode_bytes};

use crate::{
    mapper165::battle_codebook_plan::IndirectWriteDestinationBounds, rom::Rom, sha1_hex,
    typed_source::decode_rp2a03_sequence,
};

use super::{
    ACTION_BYTE_OFFSET, ALLIED_RECORD_BASE, RECORD_SCAN_CAPACITY, SHIFTED_RECORD_IDENTITY_OFFSET,
    insert_destination, record_address, record_field_destination_ranges,
};

const PRG_BANK_BYTE_COUNT: usize = 0x4000;
const CHAPTER_SAVE_BANK: u8 = 0x06;
const RECORD_FIELD_THREE_OFFSET: u16 = 0x03;
const RECORD_LOCATION_OFFSET: u16 = 0x0E;
const ROSTER_SENTINEL_INDEX: usize = RECORD_SCAN_CAPACITY - 1;

const SHIFT_INACTIVE_RECORD_IDENTITY: (u8, u16, u8) = (CHAPTER_SAVE_BANK, 0xB5E3, 0x65);
const CLEAR_RECORD_ACTION: (u8, u16, u8) = (CHAPTER_SAVE_BANK, 0xB658, 0x74);
const COPY_RECORD_FIELD_THREE: (u8, u16, u8) = (CHAPTER_SAVE_BANK, 0xB660, 0x74);
const RECORD_INACTIVE_LOCATION: (u8, u16, u8) = (CHAPTER_SAVE_BANK, 0xB698, 0x00);
const WRITER_SITES: [(u8, u16, u8); 4] = [
    SHIFT_INACTIVE_RECORD_IDENTITY,
    CLEAR_RECORD_ACTION,
    COPY_RECORD_FIELD_THREE,
    RECORD_INACTIVE_LOCATION,
];

struct TypedRegion {
    start: u16,
    end: u16,
    sha1: &'static str,
    role: &'static str,
}

const TYPED_REGIONS: [TypedRegion; 2] = [
    TypedRegion {
        start: 0xB5C8,
        end: 0xB67B,
        sha1: "37c70a01a184d607d4cac39726267a9c3e684695",
        role: "prepare and normalize chapter-save unit records",
    },
    TypedRegion {
        start: 0xB67B,
        end: 0xB69D,
        sha1: "7f89c168cb33e2425af50f7f68cab47ac2e67c4b",
        role: "record inactive unit locations during chapter save",
    },
];

struct ExpectedInstruction {
    address: u16,
    mnemonic: Mnemonic,
    mode: AddressingMode,
    operand: Operand,
}

impl ExpectedInstruction {
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

    const fn relative(address: u16, mnemonic: Mnemonic, displacement: i8) -> Self {
        Self::new(
            address,
            mnemonic,
            AddressingMode::Relative,
            Operand::Relative(displacement),
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

const SOURCE_INSTRUCTIONS: [ExpectedInstruction; 31] = [
    ExpectedInstruction::zero_page(0xB5D9, Mnemonic::Lda, 0x66),
    ExpectedInstruction::relative(0xB5DB, Mnemonic::Beq, 0x69),
    ExpectedInstruction::immediate(0xB5DD, Mnemonic::Ldy, 0x47),
    ExpectedInstruction::indirect_indexed_y(0xB5DF, Mnemonic::Lda, 0x65),
    ExpectedInstruction::immediate(0xB5E1, Mnemonic::Ldy, 0x36),
    ExpectedInstruction::indirect_indexed_y(0xB5E3, Mnemonic::Sta, 0x65),
    ExpectedInstruction::immediate(0xB646, Mnemonic::Lda, 0x90),
    ExpectedInstruction::zero_page(0xB648, Mnemonic::Sta, 0x74),
    ExpectedInstruction::immediate(0xB64A, Mnemonic::Lda, 0x6A),
    ExpectedInstruction::zero_page(0xB64C, Mnemonic::Sta, 0x75),
    ExpectedInstruction::immediate(0xB64E, Mnemonic::Ldy, 0x12),
    ExpectedInstruction::indirect_indexed_y(0xB650, Mnemonic::Lda, 0x74),
    ExpectedInstruction::immediate(0xB656, Mnemonic::Lda, 0x00),
    ExpectedInstruction::indirect_indexed_y(0xB658, Mnemonic::Sta, 0x74),
    ExpectedInstruction::immediate(0xB65A, Mnemonic::Ldy, 0x04),
    ExpectedInstruction::indirect_indexed_y(0xB65C, Mnemonic::Lda, 0x74),
    ExpectedInstruction::immediate(0xB65E, Mnemonic::Ldy, 0x03),
    ExpectedInstruction::indirect_indexed_y(0xB660, Mnemonic::Sta, 0x74),
    ExpectedInstruction::immediate(0xB665, Mnemonic::Adc, 0x1B),
    ExpectedInstruction::immediate(0xB66D, Mnemonic::Ldy, 0x00),
    ExpectedInstruction::indirect_indexed_y(0xB66F, Mnemonic::Lda, 0x74),
    ExpectedInstruction::relative(0xB671, Mnemonic::Bne, -0x25),
    ExpectedInstruction::absolute(0xB67B, Mnemonic::Jsr, 0xF111),
    ExpectedInstruction::immediate(0xB685, Mnemonic::Ldy, 0x00),
    ExpectedInstruction::indirect_indexed_y(0xB687, Mnemonic::Lda, 0x00),
    ExpectedInstruction::relative(0xB689, Mnemonic::Beq, 0x11),
    ExpectedInstruction::immediate(0xB68B, Mnemonic::Ldy, 0x12),
    ExpectedInstruction::indirect_indexed_y(0xB68D, Mnemonic::Lda, 0x00),
    ExpectedInstruction::absolute(0xB693, Mnemonic::Lda, 0x7674),
    ExpectedInstruction::immediate(0xB696, Mnemonic::Ldy, 0x0E),
    ExpectedInstruction::indirect_indexed_y(0xB698, Mnemonic::Sta, 0x00),
];

pub(super) fn bind_chapter_save_path_destinations(
    source: &Rom,
) -> Result<BTreeMap<(u8, u16, u8), IndirectWriteDestinationBounds>> {
    source.verify_supported_japanese()?;
    bind_source_protocol(source)?;

    let shifted_identity_targets = record_field_destination_ranges(
        ALLIED_RECORD_BASE,
        RECORD_SCAN_CAPACITY,
        SHIFTED_RECORD_IDENTITY_OFFSET,
    )?;
    let action_targets = record_field_destination_ranges(
        ALLIED_RECORD_BASE,
        RECORD_SCAN_CAPACITY,
        ACTION_BYTE_OFFSET,
    )?;
    let field_three_targets = record_field_destination_ranges(
        ALLIED_RECORD_BASE,
        RECORD_SCAN_CAPACITY,
        RECORD_FIELD_THREE_OFFSET,
    )?;
    let location_targets = record_field_destination_ranges(
        ALLIED_RECORD_BASE,
        RECORD_SCAN_CAPACITY,
        RECORD_LOCATION_OFFSET,
    )?;

    let mut destinations = BTreeMap::new();
    insert_destination(
        &mut destinations,
        SHIFT_INACTIVE_RECORD_IDENTITY,
        "identity field two records after a source-bound inactive allied record",
        shifted_identity_targets,
    )?;
    insert_destination(
        &mut destinations,
        CLEAR_RECORD_ACTION,
        "action byte of one allied record before the identity sentinel",
        action_targets,
    )?;
    insert_destination(
        &mut destinations,
        COPY_RECORD_FIELD_THREE,
        "field three of one allied record before the identity sentinel",
        field_three_targets,
    )?;
    insert_destination(
        &mut destinations,
        RECORD_INACTIVE_LOCATION,
        "location byte of one inactive allied record before the identity sentinel",
        location_targets,
    )?;
    ensure!(
        destinations.keys().copied().collect::<BTreeSet<_>>() == WRITER_SITES.into_iter().collect(),
        "chapter-save destination owner omitted or invented an indirect writer"
    );
    Ok(destinations)
}

fn bind_source_protocol(source: &Rom) -> Result<()> {
    for region in &TYPED_REGIONS {
        let bytes = source_bytes(source, region.start, region.end - region.start)?;
        ensure!(
            sha1_hex(bytes) == region.sha1,
            "{} source bytes changed",
            region.role
        );
        decode_rp2a03_sequence(bytes, region.start, region.role)?;
    }
    for instruction in &SOURCE_INSTRUCTIONS {
        let actual =
            decode_bytes(source_bytes(source, instruction.address, 3)?).with_context(|| {
                format!("decode chapter-save source at ${:04X}", instruction.address)
            })?;
        ensure!(
            actual.mnemonic() == instruction.mnemonic
                && actual.addressing_mode() == instruction.mode
                && actual.operand() == instruction.operand,
            "chapter-save source instruction changed at 06:${:04X}",
            instruction.address
        );
    }
    ensure!(
        record_address(ALLIED_RECORD_BASE, ROSTER_SENTINEL_INDEX)? == 0x7027,
        "allied roster sentinel no longer follows 53 candidate records"
    );
    ensure!(
        record_address(ALLIED_RECORD_BASE, RECORD_SCAN_CAPACITY)? == 0x7042,
        "allied roster domain no longer has its source-bound 54-record span"
    );
    Ok(())
}

fn source_bytes(source: &Rom, address: u16, byte_count: u16) -> Result<&[u8]> {
    ensure!(
        (0x8000..0xC000).contains(&address),
        "chapter-save source region is outside switchable PRG space"
    );
    let start = usize::from(CHAPTER_SAVE_BANK)
        .checked_mul(PRG_BANK_BYTE_COUNT)
        .and_then(|base| base.checked_add(usize::from(address - 0x8000)))
        .context("chapter-save source offset overflow")?;
    source
        .prg()
        .get(start..start + usize::from(byte_count))
        .with_context(|| format!("chapter-save source range exceeds PRG at 06:${address:04X}"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn chapter_save_writers_stay_inside_the_allied_roster_domain() {
        let shifted_identity = record_field_destination_ranges(
            ALLIED_RECORD_BASE,
            RECORD_SCAN_CAPACITY,
            SHIFTED_RECORD_IDENTITY_OFFSET,
        )
        .unwrap();
        let action = record_field_destination_ranges(
            ALLIED_RECORD_BASE,
            RECORD_SCAN_CAPACITY,
            ACTION_BYTE_OFFSET,
        )
        .unwrap();
        let field_three = record_field_destination_ranges(
            ALLIED_RECORD_BASE,
            RECORD_SCAN_CAPACITY,
            RECORD_FIELD_THREE_OFFSET,
        )
        .unwrap();
        let location = record_field_destination_ranges(
            ALLIED_RECORD_BASE,
            RECORD_SCAN_CAPACITY,
            RECORD_LOCATION_OFFSET,
        )
        .unwrap();

        for ranges in [&shifted_identity, &action, &field_three, &location] {
            assert_eq!(ranges.len(), RECORD_SCAN_CAPACITY);
            assert!(ranges.iter().all(|range| *range.end() < 0x8000));
        }
        assert_eq!(action.first(), Some(&(0x6AA2..=0x6AA2)));
        assert_eq!(action.last(), Some(&(0x7039..=0x7039)));
        assert_eq!(location.first(), Some(&(0x6A9E..=0x6A9E)));
        assert_eq!(location.last(), Some(&(0x7035..=0x7035)));
    }

    #[test]
    fn sentinel_follows_every_candidate_record_before_mapper_space() {
        let sentinel = record_address(ALLIED_RECORD_BASE, ROSTER_SENTINEL_INDEX).unwrap();
        let end = record_address(ALLIED_RECORD_BASE, RECORD_SCAN_CAPACITY).unwrap();

        assert_eq!(ROSTER_SENTINEL_INDEX, 53);
        assert_eq!(sentinel, 0x7027);
        assert_eq!(end, 0x7042);
        assert_eq!(end - sentinel, super::super::UNIT_RECORD_STRIDE);
        assert!(end < 0x8000);
    }
}
