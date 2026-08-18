use std::collections::{BTreeMap, BTreeSet};

use anyhow::{Context, Result, ensure};
use retro_rp2a03::{AddressingMode, Mnemonic, Operand, decode_bytes};

use crate::{
    mapper165::battle_codebook_plan::IndirectWriteDestinationBounds, rom::Rom, sha1_hex,
    typed_source::decode_rp2a03_sequence,
};

use super::{
    ACTION_BYTE_OFFSET, ALLIED_RECORD_BASE, COPIED_CLASS_OFFSET, DERIVED_MAP_X_OFFSET,
    DERIVED_MAP_Y_OFFSET, ENEMY_RECORD_BASE, RECORD_SCAN_CAPACITY,
    allied_and_enemy_field_destinations, indexed_pointer_destination_ranges, insert_destination,
    record_copy_destination_range, record_field_destination_ranges,
};

const PRG_BANK_BYTE_COUNT: usize = 0x4000;

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
        0x03,
        0x8250,
        0x8267,
        "2b8d13dd12fc06765504035fbfe99e505c155a41",
        "select a runtime occupancy row from an enemy coordinate",
    ),
    region(
        0x03,
        0x8267,
        0x8270,
        "5d39962dca78807802ef1c74880221a24e81d8e9",
        "select the first allied unit-record pointer",
    ),
    region(
        0x06,
        0x874C,
        0x8784,
        "170c67f0d867b1ce49c5d18535bb4b3b17685a05",
        "write the selected allied unit action state",
    ),
    region(
        0x06,
        0x8E85,
        0x8EF1,
        "29348110f1c18ca214f91a79da8beb82e6ccbff7",
        "move selected unit coordinates and occupancy",
    ),
    region(
        0x06,
        0x9328,
        0x935E,
        "7465383eac2e6f3334c205d4316a127e92f013bd",
        "refresh selected map occupancy",
    ),
    region(
        0x06,
        0x9690,
        0x96F6,
        "87f86132e2b81da84492552f4173893a4d0dff5f",
        "scan and rewrite runtime occupancy",
    ),
    region(
        0x06,
        0x96FA,
        0x972D,
        "d5c0e98b1b5fb03b3b1a8104566a1777d3fd8c32",
        "advance the runtime occupancy search",
    ),
    region(
        0x03,
        0x8278,
        0x8334,
        "d5e70795c362793fcd2c80d629c463d15b5516f6",
        "scan bounded enemy records and rewrite field six",
    ),
    region(
        0x03,
        0x8334,
        0x8398,
        "0353888ed6806af584d75826dd08a02ef033502d",
        "finish the bounded enemy-record scan",
    ),
    region(
        0x03,
        0x8509,
        0x8524,
        "090c6e286dea7a2ffb95d27fc755dfac5e5c350f",
        "derive one selected unit-record status byte",
    ),
    region(
        0x03,
        0x8548,
        0x85A4,
        "b8d4874f5ed369aa7d741297386c9e0ff1ca3ec1",
        "update selected unit occupancy and action state",
    ),
    region(
        0x03,
        0x8FD2,
        0x8FDB,
        "ca0d65506c25bfd70d8425aff53ccf4bcabbd03c",
        "select the enemy record base",
    ),
    region(
        0x03,
        0x8E52,
        0x8EAF,
        "99f2eed7ec23e57f2206253efa80fec7c9f49481",
        "clear and rebuild one selected unit record",
    ),
    region(
        0x03,
        0x8EAF,
        0x8EC4,
        "a2d13b563e6dd2d47114041987b6b895772eccbb",
        "publish one selected unit class byte",
    ),
    region(
        0x03,
        0x9A91,
        0x9ABA,
        "d12b47d34a870dae62a9893f42d7dc7cd521acd5",
        "rewrite the first enemy record derived status byte",
    ),
    region(
        0x03,
        0x9E51,
        0x9EAC,
        "c304539e1d32acb1e9db03d5d8275f0936944ce9",
        "copy source-bound allied and enemy unit records",
    ),
    region(
        0x03,
        0x9E0F,
        0x9E4B,
        "2b4f966aead132cb4276c5614fbfc8d57b05b035",
        "publish selected unit fields and occupancy",
    ),
    region(
        0x06,
        0x9930,
        0x9965,
        "db4eb0533dd011605c78c16da2f3f4ba8190d0b2",
        "select an enemy record by identity",
    ),
    region(
        0x06,
        0x9975,
        0x999A,
        "28f7483dcee005d7b74f88297605d31201fa755a",
        "rewrite the selected enemy record field",
    ),
    region(
        0x06,
        0xB46F,
        0xB4AD,
        "55709ddee6783525855db286659e828a99be9e33",
        "rewrite allied or enemy record field three",
    ),
    region(
        0x02,
        0xAAAD,
        0xAAC9,
        "9b2d5aac8891d732b5dfab0566c588461ac6c61d",
        "write one map-layer cell",
    ),
    region(
        0x02,
        0xAB00,
        0xABBC,
        "ba241503605d797166ebf2bbaab8fb1f95c622b7",
        "compose a source-bound map-layer row",
    ),
    region(
        0x02,
        0xABC4,
        0xABDB,
        "94288f8f90d4b1a3b7397b766381898d28f3ff9e",
        "select a source-bound map-layer row",
    ),
    region(
        0x06,
        0xBCF0,
        0xBD40,
        "071bceb01077c6d07a8d6a487716ce4e693f99fa",
        "rewrite map-layer cells from coordinates",
    ),
];

const ROW_SELECTOR_DATA_START: u16 = 0xBD40;
const ROW_SELECTOR_DATA_END: u16 = 0xBD48;
const ROW_SELECTOR_DATA_SHA1: &str = "66170f5421151086501cc8e80e114e3259bacdce";

const WRITER_SITES: &[(u8, u16, u8)] = &[
    (0x06, 0x8767, 0x00),
    (0x06, 0x8773, 0x00),
    (0x06, 0x8E97, 0x00),
    (0x06, 0x8EA1, 0x00),
    (0x06, 0x8EB8, 0x00),
    (0x06, 0x8ECE, 0x00),
    (0x06, 0x8ED8, 0x00),
    (0x06, 0x8EEF, 0x00),
    (0x06, 0x933F, 0x00),
    (0x06, 0x935C, 0x00),
    (0x06, 0x96CC, 0x00),
    (0x03, 0x82D8, 0x9D),
    (0x03, 0x82E9, 0x04),
    (0x03, 0x8308, 0x9D),
    (0x03, 0x8521, 0x9D),
    (0x03, 0x856E, 0x04),
    (0x03, 0x859F, 0x9D),
    (0x03, 0x8E6B, 0x9F),
    (0x03, 0x8E7D, 0x9F),
    (0x03, 0x8E8C, 0x9F),
    (0x03, 0x8E96, 0x9F),
    (0x03, 0x8E9C, 0x9D),
    (0x03, 0x8EA0, 0x9F),
    (0x03, 0x8EA9, 0x9F),
    (0x03, 0x8EC1, 0x9F),
    (0x03, 0x9AB0, 0x9D),
    (0x03, 0x9E33, 0x9F),
    (0x03, 0x9E55, 0x9F),
    (0x03, 0x9E6F, 0x9F),
    (0x03, 0x9E7C, 0x9D),
    (0x03, 0x9EA4, 0x9F),
    (0x06, 0x998C, 0x04),
    (0x06, 0xB4A0, 0x02),
    (0x02, 0xAABA, 0x6C),
    (0x02, 0xABAD, 0x6C),
    (0x06, 0xBD32, 0x6C),
    (0x06, 0xBD39, 0x6C),
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

pub(super) fn bind_gameplay_path_destinations(
    source: &Rom,
    runtime_row_pointers: &[u16],
    map_layer_row_pointers: &[u16],
) -> Result<BTreeMap<(u8, u16, u8), IndirectWriteDestinationBounds>> {
    source.verify_supported_japanese()?;
    bind_source_regions(source)?;
    for &(bank, address, pointer) in WRITER_SITES {
        ensure_indirect_store(source, bank, address, pointer)?;
    }

    let allied_actions = record_field_destination_ranges(
        ALLIED_RECORD_BASE,
        RECORD_SCAN_CAPACITY,
        ACTION_BYTE_OFFSET,
    )?;
    let allied_map_x = record_field_destination_ranges(
        ALLIED_RECORD_BASE,
        RECORD_SCAN_CAPACITY,
        DERIVED_MAP_X_OFFSET,
    )?;
    let allied_map_y = record_field_destination_ranges(
        ALLIED_RECORD_BASE,
        RECORD_SCAN_CAPACITY,
        DERIVED_MAP_Y_OFFSET,
    )?;
    let occupancy = indexed_pointer_destination_ranges(runtime_row_pointers, u8::MAX)?;
    let enemy_field_six =
        record_field_destination_ranges(ENEMY_RECORD_BASE, 0x14, COPIED_CLASS_OFFSET)?;
    let enemy_field_three =
        record_field_destination_ranges(ENEMY_RECORD_BASE, RECORD_SCAN_CAPACITY, 0x03)?;
    let allied_and_enemy_field_three = allied_and_enemy_field_destinations(0x03)?;
    let allied_and_enemy_status = allied_and_enemy_field_destinations(0x16)?;
    let allied_and_enemy_actions = allied_and_enemy_field_destinations(ACTION_BYTE_OFFSET)?;
    let allied_and_enemy_records = vec![
        record_copy_destination_range(ALLIED_RECORD_BASE, RECORD_SCAN_CAPACITY)?,
        record_copy_destination_range(ENEMY_RECORD_BASE, RECORD_SCAN_CAPACITY)?,
    ];
    let map_layer = indexed_pointer_destination_ranges(map_layer_row_pointers, u8::MAX)?;

    let mut destinations = BTreeMap::new();
    for site in [(0x06, 0x8767, 0x00), (0x06, 0x8773, 0x00)] {
        insert_destination(
            &mut destinations,
            site,
            "selected allied unit action byte",
            allied_actions.clone(),
        )?;
    }
    for site in [(0x06, 0x8E97, 0x00), (0x06, 0x8ECE, 0x00)] {
        insert_destination(
            &mut destinations,
            site,
            "selected allied unit map-x byte",
            allied_map_x.clone(),
        )?;
    }
    for site in [(0x06, 0x8EA1, 0x00), (0x06, 0x8ED8, 0x00)] {
        insert_destination(
            &mut destinations,
            site,
            "selected allied unit map-y byte",
            allied_map_y.clone(),
        )?;
    }
    for site in [
        (0x06, 0x8EB8, 0x00),
        (0x06, 0x8EEF, 0x00),
        (0x06, 0x933F, 0x00),
        (0x06, 0x935C, 0x00),
        (0x06, 0x96CC, 0x00),
        (0x03, 0x82E9, 0x04),
    ] {
        insert_destination(
            &mut destinations,
            site,
            "runtime map occupancy cell",
            occupancy.clone(),
        )?;
    }
    for site in [(0x03, 0x82D8, 0x9D), (0x03, 0x8308, 0x9D)] {
        insert_destination(
            &mut destinations,
            site,
            "field six of one of the first twenty enemy records",
            enemy_field_six.clone(),
        )?;
    }
    insert_destination(
        &mut destinations,
        (0x03, 0x8521, 0x9D),
        "derived status byte of one allied or enemy unit record",
        allied_and_enemy_status,
    )?;
    insert_destination(
        &mut destinations,
        (0x03, 0x856E, 0x04),
        "runtime map occupancy cell",
        occupancy.clone(),
    )?;
    insert_destination(
        &mut destinations,
        (0x03, 0x859F, 0x9D),
        "action byte of one allied or enemy unit record",
        allied_and_enemy_actions,
    )?;
    for site in [
        (0x03, 0x8E6B, 0x9F),
        (0x03, 0x8E7D, 0x9F),
        (0x03, 0x8E8C, 0x9F),
        (0x03, 0x8E96, 0x9F),
        (0x03, 0x8E9C, 0x9D),
        (0x03, 0x8EA0, 0x9F),
        (0x03, 0x8EA9, 0x9F),
        (0x03, 0x8EC1, 0x9F),
        (0x03, 0x9E33, 0x9F),
        (0x03, 0x9E55, 0x9F),
        (0x03, 0x9E6F, 0x9F),
        (0x03, 0x9E7C, 0x9D),
        (0x03, 0x9EA4, 0x9F),
    ] {
        insert_destination(
            &mut destinations,
            site,
            "one complete allied or enemy unit record",
            allied_and_enemy_records.clone(),
        )?;
    }
    insert_destination(
        &mut destinations,
        (0x03, 0x9AB0, 0x9D),
        "derived status byte of the first enemy record",
        record_field_destination_ranges(ENEMY_RECORD_BASE, 1, 0x16)?,
    )?;
    insert_destination(
        &mut destinations,
        (0x06, 0x998C, 0x04),
        "field three of one enemy record",
        enemy_field_three,
    )?;
    insert_destination(
        &mut destinations,
        (0x06, 0xB4A0, 0x02),
        "field three of one allied or enemy record",
        allied_and_enemy_field_three,
    )?;
    for site in [
        (0x02, 0xAABA, 0x6C),
        (0x02, 0xABAD, 0x6C),
        (0x06, 0xBD32, 0x6C),
        (0x06, 0xBD39, 0x6C),
    ] {
        insert_destination(
            &mut destinations,
            site,
            "source-bound map-layer row",
            map_layer.clone(),
        )?;
    }

    ensure!(
        destinations.keys().copied().collect::<BTreeSet<_>>()
            == WRITER_SITES.iter().copied().collect(),
        "gameplay path destination owner omitted or invented an indirect writer"
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
    let row_selector = source_bytes(
        source,
        0x06,
        ROW_SELECTOR_DATA_START,
        ROW_SELECTOR_DATA_END - ROW_SELECTOR_DATA_START,
    )?;
    ensure!(
        sha1_hex(row_selector) == ROW_SELECTOR_DATA_SHA1,
        "map-layer row selector data changed"
    );
    Ok(())
}

fn ensure_indirect_store(source: &Rom, bank: u8, address: u16, pointer: u8) -> Result<()> {
    let instruction = decode_bytes(source_bytes(source, bank, address, 3)?)
        .with_context(|| format!("decode gameplay indirect writer at {bank:02X}:${address:04X}"))?;
    ensure!(
        instruction.mnemonic() == Mnemonic::Sta
            && instruction.addressing_mode() == AddressingMode::ZeroPageIndirectIndexedY
            && instruction.operand() == Operand::Byte(pointer),
        "gameplay indirect writer changed at {bank:02X}:${address:04X}"
    );
    Ok(())
}

fn source_bytes(source: &Rom, bank: u8, address: u16, byte_count: u16) -> Result<&[u8]> {
    ensure!(
        bank < 0x0F && (0x8000..0xC000).contains(&address),
        "gameplay source region is outside switchable PRG space"
    );
    let start = usize::from(bank)
        .checked_mul(PRG_BANK_BYTE_COUNT)
        .and_then(|base| base.checked_add(usize::from(address - 0x8000)))
        .context("gameplay source offset overflow")?;
    source
        .prg()
        .get(start..start + usize::from(byte_count))
        .with_context(|| format!("gameplay source range exceeds PRG at {bank:02X}:${address:04X}"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn gameplay_writer_catalog_is_grouped_by_complete_routine_families() {
        assert_eq!(
            WRITER_SITES.iter().copied().collect::<BTreeSet<_>>().len(),
            WRITER_SITES.len(),
            "a gameplay writer site must have exactly one semantic owner"
        );
        for sites in [
            &[(0x06, 0x8767, 0x00), (0x06, 0x8773, 0x00)][..],
            &[
                (0x06, 0x8E97, 0x00),
                (0x06, 0x8EA1, 0x00),
                (0x06, 0x8EB8, 0x00),
                (0x06, 0x8ECE, 0x00),
                (0x06, 0x8ED8, 0x00),
                (0x06, 0x8EEF, 0x00),
            ][..],
            &[(0x06, 0x933F, 0x00), (0x06, 0x935C, 0x00)][..],
            &[
                (0x03, 0x8521, 0x9D),
                (0x03, 0x856E, 0x04),
                (0x03, 0x859F, 0x9D),
            ][..],
            &[
                (0x03, 0x8E6B, 0x9F),
                (0x03, 0x8E7D, 0x9F),
                (0x03, 0x8E8C, 0x9F),
                (0x03, 0x8E96, 0x9F),
                (0x03, 0x8E9C, 0x9D),
                (0x03, 0x8EA0, 0x9F),
                (0x03, 0x8EA9, 0x9F),
                (0x03, 0x8EC1, 0x9F),
                (0x03, 0x9E33, 0x9F),
            ][..],
            &[
                (0x03, 0x9E55, 0x9F),
                (0x03, 0x9E6F, 0x9F),
                (0x03, 0x9E7C, 0x9D),
                (0x03, 0x9EA4, 0x9F),
            ][..],
            &[
                (0x02, 0xAABA, 0x6C),
                (0x02, 0xABAD, 0x6C),
                (0x06, 0xBD32, 0x6C),
                (0x06, 0xBD39, 0x6C),
            ][..],
        ] {
            assert!(sites.iter().all(|site| WRITER_SITES.contains(site)));
        }
    }

    #[test]
    fn conservative_map_and_record_domains_stay_below_mapper_space() {
        let runtime_rows = (0..30)
            .map(|row| 0x72AF + row * 0x20)
            .chain([0x7AF0, 0x7B10])
            .collect::<Vec<_>>();
        let map_rows = (0..30).map(|row| 0x7730 + row * 0x20).collect::<Vec<_>>();
        assert_eq!(
            indexed_pointer_destination_ranges(&map_rows, u8::MAX).unwrap(),
            vec![0x7730..=0x7BCF]
        );
        assert!(
            indexed_pointer_destination_ranges(&runtime_rows, u8::MAX)
                .unwrap()
                .iter()
                .all(|range| *range.end() < 0x8000)
        );
        assert_eq!(
            record_field_destination_ranges(ENEMY_RECORD_BASE, 1, 0x16).unwrap(),
            vec![0x708E..=0x708E]
        );
        assert!(indexed_pointer_destination_ranges(&[0x7F01], u8::MAX).is_err());
    }
}
