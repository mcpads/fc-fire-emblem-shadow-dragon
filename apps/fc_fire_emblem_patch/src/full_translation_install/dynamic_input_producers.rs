use std::collections::{BTreeMap, BTreeSet};

use anyhow::{Context, Result, ensure};
use serde::Serialize;

use super::dynamic_inputs::{DynamicStringDomain, classified_dynamic_string_bindings};
use crate::{dialogue_inventory::switchable_cpu_to_file_offset, rom::Rom, sha1_hex};

mod arena_routes;
mod epilogue_routes;
mod item_routes;
mod shop_routes;
mod village_routes;

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
    producer_families: Vec<ProducerFamilySummary>,
    consumer_demand_count: usize,
    resolved_supply_count: usize,
    missing_consumer_demands: Vec<ProducerRouteReport>,
    mismatched_consumer_demands: Vec<ProducerRouteMismatch>,
    unexpected_producer_supplies: Vec<ProducerRouteSupply>,
    exact_consumer_producer_match: bool,
}

impl DynamicInputProducerPlan {
    pub(super) fn every_record_selector_route_bound(&self) -> bool {
        self.exact_consumer_producer_match
    }
}

#[derive(Serialize)]
pub(super) struct SourceRegionBinding {
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

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) struct ResolvedProducerRoute {
    pub(super) record_id: &'static str,
    pub(super) selector: u8,
    pub(super) domain: DynamicStringDomain,
    pub(super) family: &'static str,
}

#[derive(Serialize)]
struct ProducerFamilySummary {
    family: &'static str,
    resolved_route_count: usize,
}

#[derive(Serialize)]
struct ProducerRouteReport {
    record_id: &'static str,
    selector: u8,
    domain: DynamicStringDomain,
}

#[derive(Serialize)]
struct ProducerRouteMismatch {
    record_id: &'static str,
    selector: u8,
    demanded_domain: DynamicStringDomain,
    supplied_domain: DynamicStringDomain,
    producer_family: &'static str,
}

#[derive(Serialize)]
struct ProducerRouteSupply {
    record_id: &'static str,
    selector: u8,
    supplied_domain: DynamicStringDomain,
    producer_family: &'static str,
}

struct ProducerRouteResolution {
    producer_families: Vec<ProducerFamilySummary>,
    resolved_supply_count: usize,
    missing_consumer_demands: Vec<ProducerRouteReport>,
    mismatched_consumer_demands: Vec<ProducerRouteMismatch>,
    unexpected_producer_supplies: Vec<ProducerRouteSupply>,
    exact_consumer_producer_match: bool,
}

#[derive(Clone, Copy)]
pub(super) struct SourceRegionSpec {
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

    let route_resolution = resolve_producer_routes(rom, &classified)?;

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
        producer_families: route_resolution.producer_families,
        consumer_demand_count: classified.len(),
        resolved_supply_count: route_resolution.resolved_supply_count,
        missing_consumer_demands: route_resolution.missing_consumer_demands,
        mismatched_consumer_demands: route_resolution.mismatched_consumer_demands,
        unexpected_producer_supplies: route_resolution.unexpected_producer_supplies,
        exact_consumer_producer_match: route_resolution.exact_consumer_producer_match,
    })
}

fn resolve_producer_routes(
    rom: &Rom,
    classified: &BTreeMap<(&'static str, u8), DynamicStringDomain>,
) -> Result<ProducerRouteResolution> {
    let family_routes = [
        arena_routes::resolve(rom, classified)?,
        epilogue_routes::resolve(rom, classified)?,
        item_routes::resolve(rom, classified)?,
        shop_routes::resolve(rom, classified)?,
        village_routes::resolve(rom, classified)?,
    ];
    compare_producer_routes(classified, family_routes)
}

fn compare_producer_routes<const FAMILY_COUNT: usize>(
    classified: &BTreeMap<(&'static str, u8), DynamicStringDomain>,
    family_routes: [Vec<ResolvedProducerRoute>; FAMILY_COUNT],
) -> Result<ProducerRouteResolution> {
    let producer_families = family_routes
        .iter()
        .map(|routes| ProducerFamilySummary {
            family: routes.first().map_or("empty", |route| route.family),
            resolved_route_count: routes.len(),
        })
        .collect::<Vec<_>>();
    ensure!(
        producer_families
            .iter()
            .all(|family| family.family != "empty"),
        "dynamic producer resolver returned an empty family"
    );

    let mut supplied = BTreeMap::new();
    for route in family_routes.into_iter().flatten() {
        ensure!(
            supplied
                .insert((route.record_id, route.selector), route)
                .is_none(),
            "multiple producer families resolved {} selector {}",
            route.record_id,
            route.selector
        );
    }

    let missing_consumer_demands = classified
        .iter()
        .filter(|(binding, _)| !supplied.contains_key(binding))
        .map(|(&(record_id, selector), &domain)| ProducerRouteReport {
            record_id,
            selector,
            domain,
        })
        .collect::<Vec<_>>();
    let mismatched_consumer_demands = classified
        .iter()
        .filter_map(|(&(record_id, selector), &demanded_domain)| {
            let route = supplied.get(&(record_id, selector))?;
            (route.domain != demanded_domain).then_some(ProducerRouteMismatch {
                record_id,
                selector,
                demanded_domain,
                supplied_domain: route.domain,
                producer_family: route.family,
            })
        })
        .collect::<Vec<_>>();
    let unexpected_producer_supplies = supplied
        .iter()
        .filter(|(binding, _)| !classified.contains_key(binding))
        .map(|(&(record_id, selector), route)| ProducerRouteSupply {
            record_id,
            selector,
            supplied_domain: route.domain,
            producer_family: route.family,
        })
        .collect::<Vec<_>>();
    let exact_consumer_producer_match = missing_consumer_demands.is_empty()
        && mismatched_consumer_demands.is_empty()
        && unexpected_producer_supplies.is_empty()
        && supplied.len() == classified.len();

    Ok(ProducerRouteResolution {
        producer_families,
        resolved_supply_count: supplied.len(),
        missing_consumer_demands,
        mismatched_consumer_demands,
        unexpected_producer_supplies,
        exact_consumer_producer_match,
    })
}

pub(super) fn selected_record_routes(
    classified: &BTreeMap<(&'static str, u8), DynamicStringDomain>,
    table_id: &str,
    selected_record_indices: &BTreeSet<usize>,
    produced_domains_by_selector: &BTreeMap<u8, DynamicStringDomain>,
    family: &'static str,
) -> Vec<ResolvedProducerRoute> {
    classified
        .iter()
        .filter_map(|(&(record_id, selector), _)| {
            let (record_table_id, index) = parse_record_id(record_id)?;
            let produced_domain = produced_domains_by_selector.get(&selector)?;
            (record_table_id == table_id && selected_record_indices.contains(&index)).then_some(
                ResolvedProducerRoute {
                    record_id,
                    selector,
                    domain: *produced_domain,
                    family,
                },
            )
        })
        .collect()
}

fn parse_record_id(record_id: &str) -> Option<(&str, usize)> {
    let (table_id, index) = record_id.rsplit_once(':')?;
    Some((table_id, index.parse().ok()?))
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

pub(super) fn source_region_bytes(source: &[u8], spec: SourceRegionSpec) -> Result<&[u8]> {
    let file_offset = switchable_cpu_to_file_offset(spec.source_prg_bank, spec.cpu_address)?;
    source
        .get(file_offset..file_offset + spec.byte_count)
        .with_context(|| format!("{} source range is outside the ROM", spec.role))
}

pub(super) fn bind_source_region(
    source: &[u8],
    spec: SourceRegionSpec,
) -> Result<SourceRegionBinding> {
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

pub(super) const fn source_region(
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

    #[test]
    fn producer_supply_must_exactly_equal_consumer_demand() {
        let classified =
            BTreeMap::from([(("test-dialogue:000", 0), DynamicStringDomain::ItemName)]);
        let matching = ResolvedProducerRoute {
            record_id: "test-dialogue:000",
            selector: 0,
            domain: DynamicStringDomain::ItemName,
            family: "test_family",
        };
        let matched = compare_producer_routes(&classified, [vec![matching]]).unwrap();
        assert!(matched.exact_consumer_producer_match);

        let classified_with_missing = BTreeMap::from([
            (("test-dialogue:000", 0), DynamicStringDomain::ItemName),
            (("test-dialogue:001", 0), DynamicStringDomain::ItemName),
        ]);
        let missing = compare_producer_routes(&classified_with_missing, [vec![matching]]).unwrap();
        assert!(!missing.exact_consumer_producer_match);
        assert_eq!(missing.missing_consumer_demands.len(), 1);

        let mismatched = compare_producer_routes(
            &classified,
            [vec![ResolvedProducerRoute {
                domain: DynamicStringDomain::PreservedNumeric,
                ..matching
            }]],
        )
        .unwrap();
        assert!(!mismatched.exact_consumer_producer_match);
        assert_eq!(mismatched.mismatched_consumer_demands.len(), 1);

        let unexpected = compare_producer_routes(
            &classified,
            [vec![
                matching,
                ResolvedProducerRoute {
                    record_id: "test-dialogue:001",
                    ..matching
                },
            ]],
        )
        .unwrap();
        assert!(!unexpected.exact_consumer_producer_match);
        assert_eq!(unexpected.unexpected_producer_supplies.len(), 1);
    }
}
