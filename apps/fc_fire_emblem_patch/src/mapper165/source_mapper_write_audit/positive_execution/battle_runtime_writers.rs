use std::{
    collections::{BTreeMap, BTreeSet},
    ops::RangeInclusive,
};

use anyhow::{Context, Result, ensure};
use retro_rp2a03::{AddressingMode, Mnemonic, Operand, decode_bytes};

use crate::{
    mapper165::battle_codebook_plan::IndirectWriteDestinationBounds, rom::Rom, sha1_hex,
    typed_source::decode_rp2a03_sequence,
};

const PRG_BANK_BYTE_COUNT: usize = 0x4000;
const FIXED_PRG_BANK: u8 = 0x0F;

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
        0x06,
        0x95DB,
        0x9608,
        "bc1b38aef316fcc1d25cade47b5701d7f36d7e83",
        "write first battle participant fields",
    ),
    region(
        0x06,
        0x9633,
        0x9675,
        "1764f7c74b4dde6e1ca13fb5063e1f392a933455",
        "write second battle participant fields",
    ),
    region(
        0x06,
        0x978C,
        0x97D0,
        "943871503386e777d70ef12ac127123488024412",
        "rewrite battle participant status",
    ),
    region(
        0x06,
        0x97DA,
        0x989D,
        "ef92cbe3aa061633174edc096efec775e8865849",
        "initialize and normalize both battle participant records",
    ),
    region(
        0x06,
        0xA2AD,
        0xA2E8,
        "af6dd3249d2d63535c263c50e7daa97b81c6df37",
        "temporarily rewrite battle participants",
    ),
    region(
        0x06,
        0xA5CE,
        0xA5F7,
        "de1f6ff348cb45a0ad091d4ca52a54502c278d0e",
        "swap paired fields within one battle participant record",
    ),
    region(
        0x06,
        0xB125,
        0xB178,
        "601a426a964d48eac0973df07394a541a1c645bd",
        "rewrite the selected battle participant",
    ),
    region(
        0x06,
        0xB29F,
        0xB2C2,
        "41e17a12bbbc08c92d7e9bb216c249ee4dea2044",
        "select one of two battle participant records",
    ),
    region(
        0x06,
        0xBDF1,
        0xBE3A,
        "4d98bfae73b6df517d96ef67f2c0f680d1e43008",
        "publish battle participant result fields",
    ),
    region(
        0x06,
        0xB210,
        0xB26B,
        "d297bfef98752dbc5017ab264e2eb24774dad62b",
        "rewrite battle participant inventory fields",
    ),
    region(
        0x06,
        0xAF92,
        0xAFB8,
        "007aee5c56599f6c299e59d4a5880ccf2dc13821",
        "enter the participant inventory rewrite",
    ),
    region(
        0x06,
        0xB1F7,
        0xB210,
        "9b49cdc71aaf8508b613f95f024dde70579fa3d2",
        "enter the participant status rewrite",
    ),
    region(
        0x06,
        0x9AEC,
        0x9B1D,
        "3db3abdee89d6ec7941483a8e39a353a405dddbb",
        "copy the first battle message buffer",
    ),
    region(
        0x06,
        0x9B1D,
        0x9B4E,
        "20a8592625578b5dc7839e1286a8b44363562334",
        "copy the second battle message buffer",
    ),
    region(
        0x06,
        0xAD46,
        0xAE0C,
        "b7b3faafdd36a02ea13457f12636e2c8b59e896c",
        "project live unit records into battle staging fields",
    ),
    region(
        0x05,
        0x8DA1,
        0x8DC1,
        "069bcb02b2659f340965a88b0a545d96cf846020",
        "select the second animation reset record",
    ),
    region(
        0x05,
        0x92CE,
        0x92EA,
        "bb8216670ec2c5eb1385d56e5459db641699dd31",
        "clear one bounded animation reset record",
    ),
];

const ANIMATION_RESET_POINTER_TABLE: u16 = 0xE477;
const ANIMATION_RESET_POINTER_BYTES: [u8; 4] = [0xD2, 0x03, 0xD6, 0x03];
const ANIMATION_RESET_POINTER_SHA1: &str = "451d7bbd6ed386074bf5a3cd69a56abdc6092749";

const PARTICIPANT_WRITERS: &[(u8, u16, u8)] = &[
    (0x06, 0x95E1, 0x74),
    (0x06, 0x95F8, 0x74),
    (0x06, 0x9641, 0x74),
    (0x06, 0x9648, 0x74),
    (0x06, 0x9666, 0x74),
    (0x06, 0x97A9, 0x74),
    (0x06, 0x9832, 0x74),
    (0x06, 0x985E, 0x74),
    (0x06, 0xA2C3, 0x74),
    (0x06, 0xA2C7, 0x74),
    (0x06, 0xA5EF, 0x74),
    (0x06, 0xA5F4, 0x74),
    (0x06, 0xB162, 0x00),
    (0x06, 0xB16D, 0x00),
    (0x06, 0xB24B, 0x74),
    (0x06, 0xB251, 0x74),
    (0x06, 0xBE06, 0x00),
    (0x06, 0xBE1C, 0x74),
    (0x06, 0xBE29, 0x74),
    (0x06, 0xBE30, 0x74),
    (0x06, 0xBE37, 0x74),
];
const MESSAGE_WRITERS: &[(u8, u16, u8)] = &[(0x06, 0x9B15, 0x02), (0x06, 0x9B46, 0x02)];
const STAGING_WRITERS: &[(u8, u16, u8)] = &[
    (0x06, 0xAD95, 0x02),
    (0x06, 0xADAC, 0x02),
    (0x06, 0xADCF, 0x02),
    (0x06, 0xADD8, 0x02),
    (0x06, 0xADEC, 0x02),
    (0x06, 0xADF4, 0x02),
    (0x06, 0xAE09, 0x02),
];
const ANIMATION_WRITERS: &[(u8, u16, u8)] = &[(0x05, 0x92DF, 0x0A)];

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

pub(super) fn bind_battle_runtime_write_destinations(
    source: &Rom,
) -> Result<BTreeMap<(u8, u16, u8), IndirectWriteDestinationBounds>> {
    source.verify_supported_japanese()?;
    bind_source_regions(source)?;
    let expected_sites = all_writer_sites();
    for &(bank, address, pointer) in &expected_sites {
        ensure_indirect_store(source, bank, address, pointer)?;
    }

    let participant_records = vec![0x76F4..=0x770E, 0x7715..=0x772F];
    let message_buffer = vec![0x78F2..=0x79F1];
    let staging_fields = vec![0x0304..=0x0327];
    let animation_reset = vec![0x03D6..=0x03D9];
    let mut destinations = BTreeMap::new();
    for &site in PARTICIPANT_WRITERS {
        insert_destination(
            &mut destinations,
            site,
            "one of two battle participant records",
            participant_records.clone(),
        )?;
    }
    for &site in MESSAGE_WRITERS {
        insert_destination(
            &mut destinations,
            site,
            "battle message staging buffer",
            message_buffer.clone(),
        )?;
    }
    for &site in STAGING_WRITERS {
        insert_destination(
            &mut destinations,
            site,
            "battle field staging record",
            staging_fields.clone(),
        )?;
    }
    for &site in ANIMATION_WRITERS {
        insert_destination(
            &mut destinations,
            site,
            "second animation reset record",
            animation_reset.clone(),
        )?;
    }
    ensure!(
        destinations.keys().copied().collect::<BTreeSet<_>>()
            == expected_sites.iter().copied().collect(),
        "battle runtime destination owner omitted or invented an indirect writer"
    );
    Ok(destinations)
}

fn bind_source_regions(source: &Rom) -> Result<()> {
    for region in TYPED_REGIONS {
        let bytes = source_bytes(source, region.bank, region.start, region.end - region.start)?;
        ensure!(
            sha1_hex(bytes) == region.sha1,
            "{} source bytes changed",
            region.role
        );
        decode_rp2a03_sequence(bytes, region.start, region.role)?;
    }
    let pointer_table = source_bytes(
        source,
        FIXED_PRG_BANK,
        ANIMATION_RESET_POINTER_TABLE,
        u16::try_from(ANIMATION_RESET_POINTER_BYTES.len())?,
    )?;
    ensure!(
        pointer_table == ANIMATION_RESET_POINTER_BYTES
            && sha1_hex(pointer_table) == ANIMATION_RESET_POINTER_SHA1,
        "animation reset pointer table changed"
    );
    ensure_instruction(
        source,
        0x05,
        0x8DBC,
        Mnemonic::Ldx,
        AddressingMode::Immediate,
        Operand::Byte(1),
    )?;
    ensure_instruction(
        source,
        0x05,
        0x8DBE,
        Mnemonic::Jsr,
        AddressingMode::Absolute,
        Operand::Word(0x92CE),
    )?;
    Ok(())
}

fn all_writer_sites() -> Vec<(u8, u16, u8)> {
    PARTICIPANT_WRITERS
        .iter()
        .chain(MESSAGE_WRITERS)
        .chain(STAGING_WRITERS)
        .chain(ANIMATION_WRITERS)
        .copied()
        .collect()
}

fn insert_destination(
    destinations: &mut BTreeMap<(u8, u16, u8), IndirectWriteDestinationBounds>,
    site: (u8, u16, u8),
    role: &'static str,
    ranges: Vec<RangeInclusive<u16>>,
) -> Result<()> {
    ensure!(
        destinations
            .insert(
                site,
                IndirectWriteDestinationBounds::from_source_ranges(role, ranges)?
            )
            .is_none(),
        "battle runtime indirect writer is duplicated at {:02X}:${:04X}",
        site.0,
        site.1,
    );
    Ok(())
}

fn ensure_indirect_store(source: &Rom, bank: u8, address: u16, pointer: u8) -> Result<()> {
    ensure_instruction(
        source,
        bank,
        address,
        Mnemonic::Sta,
        AddressingMode::ZeroPageIndirectIndexedY,
        Operand::Byte(pointer),
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
    let instruction = decode_bytes(source_bytes(source, bank, address, 3)?)
        .with_context(|| format!("decode battle runtime source at {bank:02X}:${address:04X}"))?;
    ensure!(
        instruction.mnemonic() == mnemonic
            && instruction.addressing_mode() == mode
            && instruction.operand() == operand,
        "battle runtime source instruction changed at {bank:02X}:${address:04X}"
    );
    Ok(())
}

fn source_bytes(source: &Rom, bank: u8, address: u16, byte_count: u16) -> Result<&[u8]> {
    let (physical_bank, relative) = if address >= 0xC000 {
        ensure!(
            bank == FIXED_PRG_BANK,
            "battle runtime fixed source uses a non-fixed bank"
        );
        (FIXED_PRG_BANK, usize::from(address - 0xC000))
    } else {
        ensure!(
            bank < FIXED_PRG_BANK && address >= 0x8000,
            "battle runtime source is outside PRG space"
        );
        (bank, usize::from(address - 0x8000))
    };
    let start = usize::from(physical_bank)
        .checked_mul(PRG_BANK_BYTE_COUNT)
        .and_then(|base| base.checked_add(relative))
        .context("battle runtime source offset overflow")?;
    source
        .prg()
        .get(start..start + usize::from(byte_count))
        .with_context(|| {
            format!("battle runtime source range exceeds PRG at {bank:02X}:${address:04X}")
        })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn battle_runtime_catalog_covers_whole_write_families() {
        let sites = all_writer_sites();
        assert_eq!(sites.len(), 31);
        assert_eq!(sites.iter().copied().collect::<BTreeSet<_>>().len(), 31);
        assert_eq!(PARTICIPANT_WRITERS.len(), 21);
        assert_eq!(MESSAGE_WRITERS.len(), 2);
        assert_eq!(STAGING_WRITERS.len(), 7);
        assert_eq!(ANIMATION_WRITERS.len(), 1);
    }

    #[test]
    fn battle_runtime_destination_families_are_disjoint_from_mapper_space() {
        for ranges in [
            vec![0x76F4..=0x770E, 0x7715..=0x772F],
            vec![0x78F2..=0x79F1],
            vec![0x0304..=0x0327],
            vec![0x03D6..=0x03D9],
        ] {
            let bounds =
                IndirectWriteDestinationBounds::from_source_ranges("test", ranges).unwrap();
            assert!(
                bounds
                    .destination_ranges()
                    .iter()
                    .all(|range| *range.end() < 0x8000)
            );
        }
        assert!(
            IndirectWriteDestinationBounds::from_source_ranges("bad", vec![0x7FFF..=0x8000])
                .is_err()
        );
    }
}
