use std::{
    collections::{BTreeMap, BTreeSet},
    ops::RangeInclusive,
};

use anyhow::{Context, Result, ensure};
use retro_rp2a03::{AddressingMode, Mnemonic, Operand, decode_bytes};

use crate::{
    fixed_string_consumers::bind_composite_state_dispatch_source,
    front_end_menu::SAVE_SLOT_SELECTION_COMPOSITE_STATE,
    mapper165::battle_codebook_plan::IndirectWriteDestinationBounds, rom::Rom, sha1_hex,
    typed_source::decode_rp2a03_sequence,
};

const FIXED_PRG_BANK: u8 = 0x0F;
const RECORD_SLOT_MENU_HANDLER: u16 = 0x8CE8;
const RECORD_POINTER_TABLE: u16 = 0xA8DC;
const RECORD_POINTERS: [u16; 2] = [0x6A88, 0x6A8C];
const RECORD_BYTE_COUNT: u16 = 4;

const DELETE_RECORD_CLEAR: (u8, u16, u8) = (0x02, 0xA7E1, 0x00);
const COPY_DESTINATION_CLEAR: (u8, u16, u8) = (0x02, 0xA85D, 0x00);
const WRITER_SITES: [(u8, u16, u8); 2] = [DELETE_RECORD_CLEAR, COPY_DESTINATION_CLEAR];

#[derive(Clone, Copy)]
struct TypedRegion {
    bank: u8,
    start: u16,
    end: u16,
    sha1: &'static str,
    role: &'static str,
}

const TYPED_REGIONS: &[TypedRegion] = &[
    region(
        0x0B,
        0x8CE8,
        0x8D4B,
        "6b9ca9beef610a839ed0d2fb94407919a5342962",
        "build the two-slot front-end record menu",
    ),
    region(
        0x0B,
        0x8434,
        0x8457,
        "af1b6bbda5fb1211b7842805ddd760fbe45ccb1a",
        "publish one shared-menu choice mask",
    ),
    region(
        0x0B,
        0x9333,
        0x93AC,
        "ae003b21ace9212154d7616e38f2d542893c9c47",
        "commit or cancel one shared-menu selection",
    ),
    region(
        0x0B,
        0x9840,
        0x9858,
        "14de03d771684fdd3c61a885ecb70fb9ddc86eff",
        "count choices and map an ordinal to a record index",
    ),
    region(
        FIXED_PRG_BANK,
        0xE65C,
        0xE684,
        "b881345587ec77f648acdbe959acb263d095514b",
        "suspend the caller while a shared-menu request is active",
    ),
    region(
        0x02,
        0xA7C9,
        0xA7EF,
        "e4c9014a878b019419c6214ab3aa83f86d20846a",
        "clear the selected record after delete confirmation",
    ),
    region(
        0x02,
        0xA7FF,
        0xA8D4,
        "125ccd1fd71c33a86c228cf4953aa4a615a48651",
        "compare and copy one front-end record into another slot",
    ),
];

const fn region(
    bank: u8,
    start: u16,
    end: u16,
    sha1: &'static str,
    role: &'static str,
) -> TypedRegion {
    TypedRegion {
        bank,
        start,
        end,
        sha1,
        role,
    }
}

pub(super) fn bind_front_end_record_storage_destinations(
    source: &Rom,
) -> Result<BTreeMap<(u8, u16, u8), IndirectWriteDestinationBounds>> {
    source.verify_supported_japanese()?;

    let composite = bind_composite_state_dispatch_source(source)?;
    ensure!(
        composite.handler_target(SAVE_SLOT_SELECTION_COMPOSITE_STATE)
            == Some(RECORD_SLOT_MENU_HANDLER),
        "front-end record-selection state no longer reaches its two-slot menu handler"
    );
    bind_source_protocol(source)?;

    let pointers = source_bytes(source, 0x02, RECORD_POINTER_TABLE, 4)?
        .chunks_exact(2)
        .map(|bytes| u16::from_le_bytes([bytes[0], bytes[1]]))
        .collect::<Vec<_>>();
    ensure!(
        pointers == RECORD_POINTERS,
        "front-end record-storage pointer table changed"
    );

    let committed_indices = (1_u8..=3)
        .flat_map(committed_record_indices)
        .collect::<BTreeSet<_>>();
    ensure!(
        committed_indices == BTreeSet::from([1, 2]),
        "two-slot record menu can commit an unexpected record index: {committed_indices:?}"
    );

    let ranges = record_storage_ranges(&pointers)?;
    let mut destinations = BTreeMap::new();
    for site in WRITER_SITES {
        ensure_indirect_store(source, site)?;
        ensure!(
            destinations
                .insert(
                    site,
                    IndirectWriteDestinationBounds::from_source_ranges(
                        "one of two four-byte front-end record slots",
                        ranges.clone(),
                    )?,
                )
                .is_none(),
            "front-end record-storage writer is duplicated at {:02X}:${:04X}",
            site.0,
            site.1,
        );
    }
    ensure!(
        destinations.keys().copied().collect::<BTreeSet<_>>() == WRITER_SITES.into_iter().collect(),
        "front-end record-storage owner omitted or invented an indirect writer"
    );
    Ok(destinations)
}

fn bind_source_protocol(source: &Rom) -> Result<()> {
    for region in TYPED_REGIONS {
        let bytes = source_bytes(source, region.bank, region.start, region.end - region.start)?;
        ensure!(
            sha1_hex(bytes) == region.sha1,
            "{} source bytes changed",
            region.role
        );
        decode_rp2a03_sequence(bytes, region.start, region.role)?;
    }

    for (bank, address, mnemonic, mode, operand) in [
        (
            0x0B,
            0x8D18,
            Mnemonic::Ror,
            AddressingMode::Absolute,
            Operand::Word(0x05EB),
        ),
        (
            0x0B,
            0x8D3B,
            Mnemonic::Ror,
            AddressingMode::Absolute,
            Operand::Word(0x05EB),
        ),
        (
            0x0B,
            0x8D41,
            Mnemonic::Jsr,
            AddressingMode::Absolute,
            Operand::Word(0xC399),
        ),
        (
            0x0B,
            0x8D45,
            Mnemonic::Sta,
            AddressingMode::Absolute,
            Operand::Word(0x05EB),
        ),
        (
            0x0B,
            0x844C,
            Mnemonic::Sta,
            AddressingMode::AbsoluteX,
            Operand::Word(0x7FEE),
        ),
        (
            0x0B,
            0x8451,
            Mnemonic::Sta,
            AddressingMode::AbsoluteX,
            Operand::Word(0x7FF3),
        ),
        (
            0x0B,
            0x939F,
            Mnemonic::Jsr,
            AddressingMode::Absolute,
            Operand::Word(0x984D),
        ),
        (
            0x0B,
            0x93A2,
            Mnemonic::Sta,
            AddressingMode::Absolute,
            Operand::Word(0x05EB),
        ),
        (
            0x0B,
            0x938B,
            Mnemonic::Sta,
            AddressingMode::Absolute,
            Operand::Word(0x05EB),
        ),
    ] {
        ensure_instruction(source, bank, address, mnemonic, mode, operand)?;
    }
    Ok(())
}

fn committed_record_indices(choice_mask: u8) -> impl Iterator<Item = u8> {
    let choice_mask = choice_mask & 0x03;
    let selection_count = choice_mask.count_ones() as u8;
    (1..=selection_count).filter_map(move |ordinal| {
        let mut remaining = ordinal;
        for bit in 0..2 {
            if choice_mask & (1 << bit) != 0 {
                remaining -= 1;
                if remaining == 0 {
                    return Some(bit + 1);
                }
            }
        }
        None
    })
}

fn record_storage_ranges(pointers: &[u16]) -> Result<Vec<RangeInclusive<u16>>> {
    ensure!(
        pointers.len() == RECORD_POINTERS.len(),
        "front-end record-storage pointer count changed"
    );
    ensure!(
        pointers.windows(2).all(|pair| pair[0] < pair[1]),
        "front-end record-storage pointers are not strictly ordered"
    );
    pointers
        .iter()
        .map(|&start| {
            let end = start
                .checked_add(RECORD_BYTE_COUNT - 1)
                .context("front-end record-storage range overflow")?;
            ensure!(
                end < 0x8000,
                "front-end record-storage range reaches mapper space"
            );
            Ok(start..=end)
        })
        .collect()
}

fn ensure_indirect_store(source: &Rom, site: (u8, u16, u8)) -> Result<()> {
    ensure_instruction(
        source,
        site.0,
        site.1,
        Mnemonic::Sta,
        AddressingMode::ZeroPageIndirectIndexedY,
        Operand::Byte(site.2),
    )
}

fn ensure_instruction(
    source: &Rom,
    bank: u8,
    address: u16,
    mnemonic: Mnemonic,
    mode: AddressingMode,
    operand: Operand,
) -> Result<()> {
    let instruction = decode_bytes(source_bytes(source, bank, address, 3)?).with_context(|| {
        format!("decode front-end record-storage source at {bank:02X}:${address:04X}")
    })?;
    ensure!(
        instruction.mnemonic() == mnemonic
            && instruction.addressing_mode() == mode
            && instruction.operand() == operand,
        "front-end record-storage source instruction changed at {bank:02X}:${address:04X}"
    );
    Ok(())
}

fn source_bytes(source: &Rom, bank: u8, address: u16, byte_count: u16) -> Result<&[u8]> {
    let physical_bank = if address >= 0xC000 {
        FIXED_PRG_BANK
    } else {
        ensure!(
            bank < FIXED_PRG_BANK,
            "front-end record source uses an unavailable bank"
        );
        bank
    };
    let cpu_base = if address >= 0xC000 { 0xC000 } else { 0x8000 };
    let start = usize::from(physical_bank) * 0x4000 + usize::from(address - cpu_base);
    let end = start
        .checked_add(usize::from(byte_count))
        .context("front-end record source range overflow")?;
    source.prg().get(start..end).with_context(|| {
        format!("front-end record source range is missing at {bank:02X}:${address:04X}")
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn two_slot_choice_masks_commit_only_existing_record_indices() {
        assert_eq!(
            committed_record_indices(0).collect::<Vec<_>>(),
            Vec::<u8>::new()
        );
        assert_eq!(committed_record_indices(1).collect::<Vec<_>>(), [1]);
        assert_eq!(committed_record_indices(2).collect::<Vec<_>>(), [2]);
        assert_eq!(committed_record_indices(3).collect::<Vec<_>>(), [1, 2]);
    }

    #[test]
    fn record_storage_ranges_are_two_disjoint_four_byte_slots() {
        assert_eq!(
            record_storage_ranges(&RECORD_POINTERS).unwrap(),
            [0x6A88..=0x6A8B, 0x6A8C..=0x6A8F]
        );
    }

    #[test]
    fn record_storage_rejects_a_pointer_that_reaches_mapper_space() {
        assert!(record_storage_ranges(&[0x6A88, 0x7FFD]).is_err());
    }
}
