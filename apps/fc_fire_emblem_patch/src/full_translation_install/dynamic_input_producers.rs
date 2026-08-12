use std::collections::{BTreeMap, BTreeSet};

use anyhow::{Context, Result, ensure};
use serde::Serialize;

use super::dynamic_inputs::{DynamicStringDomain, classified_dynamic_string_bindings};
use crate::{
    dialogue_inventory::switchable_cpu_to_file_offset, item_flow::validate_item_lifetime_source,
    rom::Rom, sha1_hex,
};

const DYNAMIC_STRING_END_CODE: u8 = 0xEF;
const SELECTOR_DIRECTORY_PRG_BANK: u8 = 0x0A;
const SELECTOR_DIRECTORY_CPU_ADDRESS: u16 = 0x8397;
const SELECTOR_DESTINATIONS: [u16; 4] = [0x78F2, 0x7902, 0x7912, 0x7922];

#[derive(Serialize)]
pub(in crate::full_translation_install) struct DynamicInputProducerPlan {
    selector_directory: SourceRegionBinding,
    selector_destinations: [u16; 4],
    used_selectors: Vec<u8>,
    unused_selectors: Vec<u8>,
    used_selector_destinations: Vec<u16>,
    unused_selector_destinations: Vec<u16>,
    source_writers: Vec<SourceRegionBinding>,
    source_writer_count: usize,
    generic_slot_selecting_writer_count: usize,
    direct_absolute_writer_count: usize,
    every_dynamic_domain_has_a_source_writer: bool,
    item_action_record_routes_bound: bool,
    epilogue_location_record_route_bound: bool,
    classified_record_selector_count: usize,
    bound_record_selector_count: usize,
    remaining_record_selector_count: usize,
    remaining_record_selector_counts_by_domain: Vec<RemainingDomainRouteCount>,
    every_record_selector_route_bound: bool,
}

impl DynamicInputProducerPlan {
    pub(super) fn every_record_selector_route_bound(&self) -> bool {
        self.every_record_selector_route_bound
    }
}

#[derive(Serialize)]
struct SourceRegionBinding {
    role: &'static str,
    source_prg_bank: u8,
    source_prg_bank_hex: String,
    cpu_address: u16,
    cpu_address_hex: String,
    file_offset: usize,
    file_offset_hex: String,
    byte_count: usize,
    source_sha1: String,
}

#[derive(Serialize)]
struct RemainingDomainRouteCount {
    domain: DynamicStringDomain,
    count: usize,
}

#[derive(Clone, Copy)]
struct SourceRegionSpec {
    role: &'static str,
    source_prg_bank: u8,
    cpu_address: u16,
    byte_count: usize,
    expected_sha1: &'static str,
}

const SOURCE_WRITERS: [SourceRegionSpec; 7] = [
    source_region(
        "copy_item_name_to_selected_dynamic_slot",
        0x06,
        0x9AEC,
        49,
        "3db3abdee89d6ec7941483a8e39a353a405dddbb",
    ),
    source_region(
        "copy_playable_unit_name_to_selected_dynamic_slot",
        0x06,
        0x9B1D,
        49,
        "20a8592625578b5dc7839e1286a8b44363562334",
    ),
    source_region(
        "format_number_in_selected_dynamic_slot",
        0x06,
        0x9B4E,
        44,
        "b8a791aae3e8c45b6a05b2cced6338ea8ba075f2",
    ),
    source_region(
        "copy_village_item_name_to_dynamic_slot_zero",
        0x03,
        0x9C50,
        30,
        "b605a19b8605d6d32e8ea33e62c2c58b2a94797f",
    ),
    source_region(
        "copy_village_numeric_text_to_dynamic_slot_zero",
        0x03,
        0x9C6E,
        143,
        "53a47fbdba03a138f7463b10ccb71af4534d2c94",
    ),
    source_region(
        "copy_epilogue_location_name_to_dynamic_slot_one",
        0x04,
        0xA1CA,
        20,
        "42a7518454f95a16b93bbdf1eb55324450fa8aec",
    ),
    source_region(
        "write_item_result_number_to_dynamic_slot_two",
        0x06,
        0x97C1,
        15,
        "9826aa1f43a8958a6761b26216aa68f73ab6df5a",
    ),
];

const SELECTOR_DIRECTORY: SourceRegionSpec = source_region(
    "decode_dynamic_selector_destination",
    SELECTOR_DIRECTORY_PRG_BANK,
    SELECTOR_DIRECTORY_CPU_ADDRESS,
    8,
    "18a98567b249fa71252e7bb9b2572919d982db12",
);

const BOUND_RECORD_SELECTORS: [(&str, u8); 10] = [
    ("shop-and-item-dialogue:025", 0),
    ("shop-and-item-dialogue:025", 1),
    ("shop-and-item-dialogue:026", 0),
    ("shop-and-item-dialogue:026", 1),
    ("shop-and-item-dialogue:027", 0),
    ("shop-and-item-dialogue:027", 1),
    ("shop-and-item-dialogue:027", 2),
    ("shop-and-item-dialogue:028", 0),
    ("shop-and-item-dialogue:028", 1),
    ("epilogue-dialogue:000", 1),
];

pub(in crate::full_translation_install) fn inspect_dynamic_input_producers(
    rom: &Rom,
) -> Result<DynamicInputProducerPlan> {
    let source = rom.data();
    let selector_directory = bind_source_region(source, SELECTOR_DIRECTORY)?;
    let selector_bytes = source_region_bytes(source, SELECTOR_DIRECTORY)?;
    let selector_destinations = selector_bytes
        .chunks_exact(2)
        .map(|bytes| u16::from_le_bytes([bytes[0], bytes[1]]))
        .collect::<Vec<_>>();
    ensure!(
        selector_destinations == SELECTOR_DESTINATIONS,
        "dynamic selector destination directory changed"
    );

    let source_writers = SOURCE_WRITERS
        .iter()
        .copied()
        .map(|spec| bind_source_region(source, spec))
        .collect::<Result<Vec<_>>>()?;
    validate_writer_semantics(source)?;
    validate_item_action_routes(rom)?;

    let classified = classified_dynamic_string_bindings();
    let used_selectors = classified
        .keys()
        .map(|(_, selector)| *selector)
        .collect::<BTreeSet<_>>();
    ensure!(
        used_selectors == BTreeSet::from([0, 1, 2]),
        "current dialogue EC selector population changed"
    );
    let unused_selectors = (0..SELECTOR_DESTINATIONS.len() as u8)
        .filter(|selector| !used_selectors.contains(selector))
        .collect::<Vec<_>>();
    ensure!(
        unused_selectors == [3],
        "current dialogue selector-three lifetime changed"
    );

    let bound = BOUND_RECORD_SELECTORS.into_iter().collect::<BTreeSet<_>>();
    ensure!(
        bound.iter().all(|binding| classified.contains_key(binding)),
        "source-bound dynamic producer route is absent from classified EC inputs"
    );
    let remaining = classified
        .iter()
        .filter(|(binding, _)| !bound.contains(binding))
        .map(|(_, domain)| *domain)
        .fold(
            BTreeMap::<DynamicStringDomain, usize>::new(),
            |mut counts, domain| {
                *counts.entry(domain).or_default() += 1;
                counts
            },
        );
    let remaining_record_selector_count = remaining.values().sum::<usize>();
    let bound_record_selector_count = bound.len();
    ensure!(
        bound_record_selector_count + remaining_record_selector_count == classified.len(),
        "dynamic producer route accounting lost record-selector bindings"
    );

    let used_selectors = used_selectors.into_iter().collect::<Vec<_>>();
    let used_selector_destinations = used_selectors
        .iter()
        .map(|selector| SELECTOR_DESTINATIONS[usize::from(*selector)])
        .collect();
    let unused_selector_destinations = unused_selectors
        .iter()
        .map(|selector| SELECTOR_DESTINATIONS[usize::from(*selector)])
        .collect();

    Ok(DynamicInputProducerPlan {
        selector_directory,
        selector_destinations: SELECTOR_DESTINATIONS,
        used_selectors,
        unused_selectors,
        used_selector_destinations,
        unused_selector_destinations,
        source_writers,
        source_writer_count: SOURCE_WRITERS.len(),
        generic_slot_selecting_writer_count: 3,
        direct_absolute_writer_count: SOURCE_WRITERS.len() - 3,
        every_dynamic_domain_has_a_source_writer: true,
        item_action_record_routes_bound: true,
        epilogue_location_record_route_bound: true,
        classified_record_selector_count: classified.len(),
        bound_record_selector_count,
        remaining_record_selector_count,
        remaining_record_selector_counts_by_domain: remaining
            .into_iter()
            .map(|(domain, count)| RemainingDomainRouteCount { domain, count })
            .collect(),
        every_record_selector_route_bound: remaining_record_selector_count == 0,
    })
}

fn validate_writer_semantics(source: &[u8]) -> Result<()> {
    let item_writer = source_region_bytes(source, SOURCE_WRITERS[0])?;
    let unit_writer = source_region_bytes(source, SOURCE_WRITERS[1])?;
    let numeric_writer = source_region_bytes(source, SOURCE_WRITERS[2])?;
    for (role, writer) in [
        ("item-name", item_writer),
        ("playable-unit-name", unit_writer),
        ("numeric", numeric_writer),
    ] {
        ensure!(
            writer.starts_with(&[0x48, 0xA9, 0xF2]),
            "{role} slot-selecting writer no longer starts at 78F2"
        );
        ensure!(
            writer.last() == Some(&0x60),
            "{role} slot-selecting writer no longer returns at its bound end"
        );
    }
    ensure!(
        item_writer
            .windows(2)
            .any(|bytes| bytes == [0xC9, DYNAMIC_STRING_END_CODE])
            && unit_writer
                .windows(2)
                .any(|bytes| bytes == [0xC9, DYNAMIC_STRING_END_CODE])
            && numeric_writer
                .windows(2)
                .any(|bytes| bytes == [0xA9, DYNAMIC_STRING_END_CODE]),
        "dynamic source writers no longer terminate their SRAM strings with EF"
    );
    Ok(())
}

fn validate_item_action_routes(rom: &Rom) -> Result<()> {
    validate_item_lifetime_source(rom)?;
    let file_offset = switchable_cpu_to_file_offset(0x06, 0x944C)?;
    let execute_item_action = rom
        .data()
        .get(file_offset..file_offset + 202)
        .context("item action producer source range is outside the ROM")?;
    for (role, sequence) in [
        (
            "transfer recipient in selector two",
            &[0xA0, 0x02, 0xAD, 0x15, 0x77, 0x20, 0x1D, 0x9B][..],
        ),
        (
            "acting unit in selector zero",
            &[0xA0, 0x00, 0xAD, 0xF4, 0x76, 0x20, 0x1D, 0x9B][..],
        ),
        (
            "selected item in selector one",
            &[0xA0, 0x01, 0xAD, 0xB0, 0x77, 0x20, 0xEC, 0x9A][..],
        ),
        (
            "action-indexed result dialogue",
            &[0xAC, 0xB2, 0x77, 0xB9, 0x16, 0x95, 0x8D, 0xF1, 0x77][..],
        ),
    ] {
        ensure!(
            execute_item_action
                .windows(sequence.len())
                .filter(|bytes| *bytes == sequence)
                .count()
                == 1,
            "item action dynamic producer lost unique {role} sequence"
        );
    }
    Ok(())
}

fn source_region_bytes(source: &[u8], spec: SourceRegionSpec) -> Result<&[u8]> {
    let file_offset = switchable_cpu_to_file_offset(spec.source_prg_bank, spec.cpu_address)?;
    source
        .get(file_offset..file_offset + spec.byte_count)
        .with_context(|| format!("{} source range is outside the ROM", spec.role))
}

fn bind_source_region(source: &[u8], spec: SourceRegionSpec) -> Result<SourceRegionBinding> {
    let file_offset = switchable_cpu_to_file_offset(spec.source_prg_bank, spec.cpu_address)?;
    let bytes = source_region_bytes(source, spec)?;
    let source_sha1 = sha1_hex(bytes);
    ensure!(
        source_sha1 == spec.expected_sha1,
        "{} source changed: expected {}, found {source_sha1}",
        spec.role,
        spec.expected_sha1
    );
    Ok(SourceRegionBinding {
        role: spec.role,
        source_prg_bank: spec.source_prg_bank,
        source_prg_bank_hex: format!("{:02X}", spec.source_prg_bank),
        cpu_address: spec.cpu_address,
        cpu_address_hex: format!("{:04X}", spec.cpu_address),
        file_offset,
        file_offset_hex: format!("{file_offset:05X}"),
        byte_count: spec.byte_count,
        source_sha1,
    })
}

const fn source_region(
    role: &'static str,
    source_prg_bank: u8,
    cpu_address: u16,
    byte_count: usize,
    expected_sha1: &'static str,
) -> SourceRegionSpec {
    SourceRegionSpec {
        role,
        source_prg_bank,
        cpu_address,
        byte_count,
        expected_sha1,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn exact_source_region_rejects_a_mutated_writer() {
        let mut source = vec![0; switchable_cpu_to_file_offset(0, 0x8000).unwrap() + 3];
        let file_offset = switchable_cpu_to_file_offset(0, 0x8000).unwrap();
        source[file_offset..].copy_from_slice(&[1, 2, 3]);
        let spec = source_region(
            "test_writer",
            0,
            0x8000,
            3,
            "7037807198c22a7d2b0807371d763779a84fdfcf",
        );
        assert!(bind_source_region(&source, spec).is_ok());

        source[file_offset + 1] ^= 0xFF;
        assert!(bind_source_region(&source, spec).is_err());
    }
}
