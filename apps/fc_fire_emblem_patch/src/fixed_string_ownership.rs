//! 직접 생산되는 bank 0B 고정 문자열의 번역·보존 소유권을 닫는다.
//!
//! `fixed_string_consumers`가 원천 표와 직접 생산자 분모를 제공한다. 이 모듈은 그
//! 분모의 각 인덱스를 정확히 한 번 번역 도메인 또는 비일본어 보존 도메인에
//! 배정한다. 직접 생산자가 없는 handler-only 경로는 별도 미해결 집합으로 남긴다.

use std::collections::{BTreeMap, BTreeSet};

use anyhow::{Result, ensure};
use serde::Serialize;

use crate::{
    chapter_transition::SAVE_OFFER_FIXED_STRING_INDEX,
    choice_labels, fixed_menu_labels,
    fixed_string_consumers::{
        FixedStringCallSite, FixedStringConsumerInspection, FixedStringRecord,
    },
    front_end_menu, item_flow,
    japanese_encoding::is_japanese_text_code,
    map_menu,
    mapper165::ROSTER_HEADER_FIXED_STRING_INDEX,
    unit_ui_text,
};

const PRESERVED_FIXED_STRING_INDICES: [u8; 7] = [0x09, 0x0B, 0x12, 0x17, 0x2E, 0x2F, 0x3E];
const EXPECTED_HANDLER_ONLY_INDICES: [u8; 6] = [0x18, 0x19, 0x1A, 0x1B, 0x1E, 0x1F];

#[derive(Clone, Debug)]
struct TranslatedOwnerGroup {
    domain: &'static str,
    indices: BTreeSet<u8>,
}

#[derive(Debug, Serialize)]
pub(crate) struct FixedStringOwnershipReport {
    translated_owner_count: usize,
    preserved_owner_count: usize,
    direct_producer_bound_count: usize,
    appender_possible_index_count: usize,
    handler_only_unresolved_index_count: usize,
    translated_owner_groups: Vec<TranslatedOwnerReport>,
    preserved_indices_hex: Vec<String>,
    handler_only_unresolved_routes: Vec<HandlerOnlyRouteReport>,
    direct_producer_bound_ownership_complete: bool,
    whole_program_reference_ownership_complete: bool,
}

#[derive(Debug, Serialize)]
struct TranslatedOwnerReport {
    domain: &'static str,
    indices_hex: Vec<String>,
}

#[derive(Debug, Serialize)]
struct HandlerOnlyRouteReport {
    composite_state_hex: String,
    indices_hex: Vec<String>,
}

pub(crate) fn inspect_fixed_string_ownership(
    inspection: &FixedStringConsumerInspection,
) -> Result<FixedStringOwnershipReport> {
    let owners = translated_owner_groups();
    let preserved = PRESERVED_FIXED_STRING_INDICES.into_iter().collect();
    let appender_possible_indices = inspection
        .call_sites
        .iter()
        .flat_map(|call| call.possible_indices.iter().copied())
        .collect::<BTreeSet<_>>();
    let handler_only = appender_possible_indices
        .difference(&inspection.direct_producer_bound_indices)
        .copied()
        .collect::<BTreeSet<_>>();
    let report = partition_fixed_string_ownership(
        &inspection.records,
        &inspection.call_sites,
        &inspection.direct_producer_bound_indices,
        &owners,
        &preserved,
    )?;

    ensure!(
        report.translated_owner_count == 49
            && report.preserved_owner_count == 7
            && report.direct_producer_bound_count == 56
            && report.appender_possible_index_count == 62
            && handler_only == EXPECTED_HANDLER_ONLY_INDICES.into_iter().collect(),
        "fixed-string ownership population changed"
    );
    ensure!(
        report
            .handler_only_unresolved_routes
            .iter()
            .map(|route| route.composite_state_hex.as_str())
            .collect::<BTreeSet<_>>()
            == BTreeSet::from(["00", "01"]),
        "handler-only fixed-string state population changed"
    );
    Ok(report)
}

fn translated_owner_groups() -> Vec<TranslatedOwnerGroup> {
    vec![
        owner(
            "unit_ui_labels",
            unit_ui_text::translated_fixed_string_indices(),
        ),
        owner(
            "item_action_labels",
            item_flow::translated_fixed_string_indices(),
        ),
        owner(
            "fixed_menu_labels",
            fixed_menu_labels::translated_fixed_string_indices(),
        ),
        owner(
            "choice_labels",
            choice_labels::translated_fixed_string_indices(),
        ),
        owner(
            "front_end_menu_labels",
            front_end_menu::translated_fixed_string_indices(),
        ),
        owner(
            "map_menu_labels",
            map_menu::translated_fixed_string_indices(),
        ),
        owner(
            "chapter_save_offer_label",
            BTreeSet::from([SAVE_OFFER_FIXED_STRING_INDEX]),
        ),
        owner(
            "roster_header",
            BTreeSet::from([ROSTER_HEADER_FIXED_STRING_INDEX]),
        ),
    ]
}

fn owner(domain: &'static str, indices: BTreeSet<u8>) -> TranslatedOwnerGroup {
    TranslatedOwnerGroup { domain, indices }
}

fn partition_fixed_string_ownership(
    records: &[FixedStringRecord],
    call_sites: &[FixedStringCallSite],
    direct_producer_bound_indices: &BTreeSet<u8>,
    translated_owners: &[TranslatedOwnerGroup],
    preserved_indices: &BTreeSet<u8>,
) -> Result<FixedStringOwnershipReport> {
    let mut translated_indices = BTreeSet::new();
    let mut translated_owner_groups = Vec::with_capacity(translated_owners.len());
    let mut domains = BTreeSet::new();
    for group in translated_owners {
        ensure!(
            !group.indices.is_empty(),
            "fixed-string owner {} is empty",
            group.domain
        );
        ensure!(
            domains.insert(group.domain),
            "fixed-string owner domain repeats"
        );
        for index in &group.indices {
            ensure!(
                translated_indices.insert(*index),
                "fixed-string index {index:02X} has multiple translated owners"
            );
        }
        translated_owner_groups.push(TranslatedOwnerReport {
            domain: group.domain,
            indices_hex: hex_indices(&group.indices),
        });
    }

    ensure!(
        translated_indices.is_disjoint(preserved_indices),
        "fixed-string index is both translated and preserved"
    );
    let owned_direct_indices = translated_indices
        .union(preserved_indices)
        .copied()
        .collect::<BTreeSet<_>>();
    ensure!(
        &owned_direct_indices == direct_producer_bound_indices,
        "direct-producer fixed-string ownership is incomplete: expected {direct_producer_bound_indices:?}, got {owned_direct_indices:?}"
    );

    let records_by_index = records
        .iter()
        .map(|record| (record.index, record))
        .collect::<BTreeMap<_, _>>();
    ensure!(
        records_by_index.len() == records.len(),
        "fixed-string record index repeats"
    );
    for index in preserved_indices {
        let record = records_by_index.get(index).ok_or_else(|| {
            anyhow::anyhow!("preserved fixed-string index {index:02X} is missing")
        })?;
        ensure!(
            !record
                .source_bytes
                .iter()
                .copied()
                .any(is_japanese_text_code),
            "preserved fixed-string index {index:02X} contains Japanese text"
        );
    }

    let appender_possible_indices = call_sites
        .iter()
        .flat_map(|call| call.possible_indices.iter().copied())
        .collect::<BTreeSet<_>>();
    ensure!(
        direct_producer_bound_indices.is_subset(&appender_possible_indices),
        "direct-producer fixed-string index is absent from the appender population"
    );
    let handler_only_indices = appender_possible_indices
        .difference(direct_producer_bound_indices)
        .copied()
        .collect::<BTreeSet<_>>();
    let mut handler_only_by_state = BTreeMap::<u8, BTreeSet<u8>>::new();
    for call in call_sites {
        let route_indices = call
            .possible_indices
            .iter()
            .copied()
            .filter(|index| handler_only_indices.contains(index))
            .collect::<BTreeSet<_>>();
        if !route_indices.is_empty() {
            handler_only_by_state
                .entry(call.composite_state)
                .or_default()
                .extend(route_indices);
        }
    }
    ensure!(
        handler_only_by_state
            .values()
            .flatten()
            .copied()
            .collect::<BTreeSet<_>>()
            == handler_only_indices,
        "handler-only fixed-string route classification is incomplete"
    );

    Ok(FixedStringOwnershipReport {
        translated_owner_count: translated_indices.len(),
        preserved_owner_count: preserved_indices.len(),
        direct_producer_bound_count: direct_producer_bound_indices.len(),
        appender_possible_index_count: appender_possible_indices.len(),
        handler_only_unresolved_index_count: handler_only_indices.len(),
        translated_owner_groups,
        preserved_indices_hex: hex_indices(preserved_indices),
        handler_only_unresolved_routes: handler_only_by_state
            .into_iter()
            .map(|(state, indices)| HandlerOnlyRouteReport {
                composite_state_hex: format!("{state:02X}"),
                indices_hex: hex_indices(&indices),
            })
            .collect(),
        direct_producer_bound_ownership_complete: true,
        whole_program_reference_ownership_complete: false,
    })
}

fn hex_indices(indices: &BTreeSet<u8>) -> Vec<String> {
    indices.iter().map(|index| format!("{index:02X}")).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn record(index: u8, source_bytes: &[u8]) -> FixedStringRecord {
        FixedStringRecord {
            index,
            pointer: 0x9000 + u16::from(index),
            source_bytes: source_bytes.to_vec(),
        }
    }

    fn call(state: u8, possible_indices: &[u8]) -> FixedStringCallSite {
        FixedStringCallSite {
            cpu_address: 0x8000 + u16::from(state),
            composite_state: state,
            possible_indices: possible_indices.to_vec(),
        }
    }

    fn fixture() -> (
        Vec<FixedStringRecord>,
        Vec<FixedStringCallSite>,
        BTreeSet<u8>,
        Vec<TranslatedOwnerGroup>,
        BTreeSet<u8>,
    ) {
        (
            vec![
                record(0x10, &[0x10, 0xED]),
                record(0x11, &[0x71, 0x79, 0xED]),
                record(0x18, &[0x0C, 0x13, 0x10, 0xED]),
            ],
            vec![call(0x02, &[0x10, 0x11]), call(0x00, &[0x18])],
            BTreeSet::from([0x10, 0x11]),
            vec![owner("translated", BTreeSet::from([0x10]))],
            BTreeSet::from([0x11]),
        )
    }

    #[test]
    fn handler_only_route_remains_explicitly_unresolved() {
        let (records, calls, direct, owners, preserved) = fixture();
        let report =
            partition_fixed_string_ownership(&records, &calls, &direct, &owners, &preserved)
                .unwrap();

        assert_eq!(report.handler_only_unresolved_index_count, 1);
        assert_eq!(report.handler_only_unresolved_routes[0].indices_hex, ["18"]);
        assert!(report.direct_producer_bound_ownership_complete);
        assert!(!report.whole_program_reference_ownership_complete);
    }

    #[test]
    fn missing_direct_translation_owner_fails() {
        let (records, calls, direct, mut owners, preserved) = fixture();
        owners[0].indices.clear();

        assert!(
            partition_fixed_string_ownership(&records, &calls, &direct, &owners, &preserved,)
                .is_err()
        );
    }

    #[test]
    fn translated_and_preserved_ownership_cannot_overlap() {
        let (records, calls, direct, mut owners, preserved) = fixture();
        owners[0].indices.insert(0x11);

        assert!(
            partition_fixed_string_ownership(&records, &calls, &direct, &owners, &preserved,)
                .is_err()
        );
    }

    #[test]
    fn preserved_owner_rejects_japanese_source_codes() {
        let (mut records, calls, direct, owners, preserved) = fixture();
        records[1].source_bytes = vec![0x01, 0xED];

        assert!(
            partition_fixed_string_ownership(&records, &calls, &direct, &owners, &preserved,)
                .is_err()
        );
    }
}
