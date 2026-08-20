//! 원본의 여덟 main-dialogue selector 계열을 통합 설치와 직접 결속한다.
//!
//! 화면 표본만 세면 영입, 마을, 집, 일반 승패처럼 같은 dispatcher를 쓰는 계열이
//! 보고서 분모에서 사라질 수 있다. 여기서는 화면 이름을 추측하지 않는다. 원본
//! selector 디렉터리의 전체 레코드 모집단을 기준으로 번역 레코드, 포인터 재설치,
//! 페이지 작업집합, E4/E6 전이, 동적 문자열 생산자와 공통 실행 훅을 함께 검사한다.

use std::collections::{BTreeMap, BTreeSet};

use anyhow::{Context, Result, ensure};
use serde::Serialize;

use super::{
    dynamic_input_producers::DynamicInputProducerPlan, runtime_code::DialogueRuntimeHookRole,
};
use crate::{
    dialogue_assets::{
        EncodedMainDialogueBundle, MainDialogueDisplayPlan, MainDialoguePageWorkset,
    },
    dialogue_inventory::{
        MainDialogueGraphReport, MainDialogueRuntimeIdentityBinding, MainDialogueStorageRecord,
        inspect_main_dialogue_runtime_identities, inspect_main_dialogue_storage,
    },
    rom::Rom,
};

const EXPECTED_CANONICAL_RECORD_COUNT: usize = 504;
const EXPECTED_POINTER_SLOT_COUNT: usize = 523;
const EXPECTED_ENTRY_BINDING_COUNT: usize = 517;
const EXPECTED_HANDLER_ENTRY_COUNT: usize = 6;
const EXPECTED_TRANSITION_EDGE_COUNT: usize = 213;
// This is the source execution population, not the count of translated literal lines. The old
// value 37 dropped 105 EC controls from untranslated control-only epilogue records (53 direct
// character records and 52 routing records), even though those records still publish names.
const EXPECTED_DYNAMIC_CONTROL_COUNT: usize = 142;
const EXPECTED_E4_LOOKAHEAD_RECORD_COUNT: usize = 92;
const EXPECTED_E6_LOOKAHEAD_RECORD_COUNT: usize = 121;
const EXPECTED_E7_CALLER_RESUME_RECORD_COUNT: usize = 84;
const EXPECTED_TERMINAL_RECORD_COUNT: usize = 207;

#[derive(Clone, Copy)]
struct ExpectedRouteFamily {
    table_id: &'static str,
    selector: u8,
    pointer_slot_count: usize,
    canonical_record_count: usize,
    handler_entry_count: usize,
}

const EXPECTED_ROUTE_FAMILIES: [ExpectedRouteFamily; 8] = [
    family("chapter-intro-dialogue", 0x80, 51, 47, 0),
    family("village-and-outro-dialogue", 0xC0, 94, 86, 0),
    family("recruitment-dialogue", 0x71, 109, 104, 4),
    family("victory-and-defeat-dialogue", 0xB0, 11, 11, 0),
    family("shop-and-item-dialogue", 0xB1, 88, 88, 0),
    family("house-dialogue", 0x30, 50, 50, 0),
    family("epilogue-dialogue", 0x40, 66, 66, 0),
    family("epilogue-routing-dialogue", 0x41, 54, 52, 2),
];

const REQUIRED_COMMON_HOOKS: [DialogueRuntimeHookRole; 4] = [
    DialogueRuntimeHookRole::InitialDirectEntryRequest,
    DialogueRuntimeHookRole::E4TransitionEntryRequest,
    DialogueRuntimeHookRole::E6TransitionEntryRequest,
    DialogueRuntimeHookRole::E7CallerResumeRequest,
];

const fn family(
    table_id: &'static str,
    selector: u8,
    pointer_slot_count: usize,
    canonical_record_count: usize,
    handler_entry_count: usize,
) -> ExpectedRouteFamily {
    ExpectedRouteFamily {
        table_id,
        selector,
        pointer_slot_count,
        canonical_record_count,
        handler_entry_count,
    }
}

#[derive(Serialize)]
pub(super) struct MainDialogueRoutePopulationPlan {
    strategy: &'static str,
    route_family_count: usize,
    canonical_record_count: usize,
    pointer_slot_count: usize,
    installed_pointer_binding_count: usize,
    handler_entry_count: usize,
    page_workset_count: usize,
    transition_edge_count: usize,
    cross_family_transition_edge_count: usize,
    dynamic_string_control_count: usize,
    common_runtime_hook_roles: Vec<DialogueRuntimeHookRole>,
    every_new_record_entry_clears_all_physical_line_buffers: bool,
    identity_lookup_boundary_partition: IdentityLookupBoundaryPartition,
    every_route_family_fully_installed: bool,
    every_transition_target_installed: bool,
    dynamic_string_producer_routes_bound: bool,
    natural_gameplay_branch_semantics_complete: bool,
    route_families: Vec<MainDialogueRouteFamilyPlan>,
    unresolved: [&'static str; 2],
}

#[derive(Serialize)]
struct IdentityLookupBoundaryPartition {
    e4_published_lookahead_record_count: usize,
    e6_published_lookahead_record_count: usize,
    e7_live_caller_resume_record_count: usize,
    terminal_without_followup_lookup_record_count: usize,
    transition_graph_sources_equal_e4_and_e6_records: bool,
    every_canonical_record_has_one_identity_lookup_boundary: bool,
}

#[derive(Serialize)]
struct MainDialogueRouteFamilyPlan {
    table_id: &'static str,
    directory_selector: u8,
    directory_selector_hex: String,
    pointer_slot_count: usize,
    canonical_record_count: usize,
    installed_record_count: usize,
    installed_pointer_binding_count: usize,
    handler_entry_count: usize,
    page_workset_count: usize,
    dynamic_string_control_count: usize,
    terminal_record_count: usize,
    caller_handoff_record_count: usize,
    transition_source_record_count: usize,
    incoming_transition_edge_count: usize,
    every_record_has_a_runtime_page: bool,
}

#[derive(Clone)]
struct JoinedRecord {
    record_id: String,
    table_id: &'static str,
    selector: u8,
    pointer_slot_count: usize,
    entry_indices: Vec<usize>,
    boundary_control: u8,
    page_workset_count: usize,
    dynamic_string_control_count: usize,
    installed_pointer_binding_count: usize,
}

pub(super) fn plan_main_dialogue_route_population(
    source: &Rom,
    display: &MainDialogueDisplayPlan,
    encoded: &EncodedMainDialogueBundle,
    graph: &MainDialogueGraphReport,
    dynamic_producers: &DynamicInputProducerPlan,
    assembled_hook_roles: &[DialogueRuntimeHookRole],
    new_record_line_buffer_reset_routes_bound: bool,
) -> Result<MainDialogueRoutePopulationPlan> {
    let identities = inspect_main_dialogue_runtime_identities(source.data())?;
    let storage = inspect_main_dialogue_storage(source.data())?;
    let joined = join_records(
        &identities,
        &storage.records,
        &display.record_ids,
        &display.page_worksets,
        &encoded.pointer_writes,
    )?;
    build_route_population(
        &EXPECTED_ROUTE_FAMILIES,
        &joined,
        graph,
        dynamic_producers.every_record_selector_route_bound(),
        assembled_hook_roles,
        new_record_line_buffer_reset_routes_bound,
    )
}

fn join_records(
    identities: &[MainDialogueRuntimeIdentityBinding],
    storage: &[MainDialogueStorageRecord],
    installed_record_ids: &[String],
    page_worksets: &[MainDialoguePageWorkset],
    pointer_writes: &[crate::dialogue_assets::MainDialoguePointerWrite],
) -> Result<Vec<JoinedRecord>> {
    let installed = exact_record_set(installed_record_ids, "installed main dialogue")?;
    let identity_by_id = identities
        .iter()
        .map(|identity| (identity.record_id.as_str(), identity))
        .collect::<BTreeMap<_, _>>();
    ensure!(
        identity_by_id.len() == identities.len(),
        "main-dialogue source identity population contains duplicate record IDs"
    );
    let storage_by_id = storage
        .iter()
        .map(|record| {
            (
                format!("{}:{:03}", record.table_id, record.canonical_entry_index),
                record,
            )
        })
        .collect::<BTreeMap<_, _>>();
    ensure!(
        storage_by_id.len() == storage.len(),
        "main-dialogue storage population contains duplicate record IDs"
    );
    let source_ids = identity_by_id.keys().copied().collect::<BTreeSet<_>>();
    ensure!(
        source_ids == installed.iter().map(String::as_str).collect(),
        "installed main-dialogue record population differs from the source selector population"
    );
    ensure!(
        storage_by_id
            .keys()
            .map(String::as_str)
            .collect::<BTreeSet<_>>()
            == source_ids,
        "main-dialogue storage and selector populations disagree"
    );

    let mut page_counts = BTreeMap::<&str, usize>::new();
    let mut dynamic_counts = BTreeMap::<&str, usize>::new();
    for workset in page_worksets {
        ensure!(
            source_ids.contains(workset.record_id.as_str()),
            "page workset references unknown main-dialogue record {}",
            workset.record_id
        );
        *page_counts.entry(workset.record_id.as_str()).or_default() += 1;
        *dynamic_counts
            .entry(workset.record_id.as_str())
            .or_default() += workset.dynamic_string_control_count;
    }
    let mut pointer_write_counts = BTreeMap::<&str, usize>::new();
    for write in pointer_writes {
        ensure!(
            source_ids.contains(write.record_id.as_str()),
            "pointer installation references unknown main-dialogue record {}",
            write.record_id
        );
        *pointer_write_counts
            .entry(write.record_id.as_str())
            .or_default() += 1;
    }

    identities
        .iter()
        .map(|identity| {
            let storage = storage_by_id
                .get(identity.record_id.as_str())
                .with_context(|| format!("{} has no source storage record", identity.record_id))?;
            let page_workset_count = page_counts
                .get(identity.record_id.as_str())
                .copied()
                .unwrap_or_default();
            ensure!(
                page_workset_count != 0,
                "{} has no installed runtime page workset",
                identity.record_id
            );
            let installed_pointer_binding_count = pointer_write_counts
                .get(identity.record_id.as_str())
                .copied()
                .unwrap_or_default();
            ensure!(
                installed_pointer_binding_count == identity.entry_indices.len(),
                "{} installs {installed_pointer_binding_count} pointer bindings but owns {} source entries",
                identity.record_id,
                identity.entry_indices.len()
            );
            Ok(JoinedRecord {
                record_id: identity.record_id.clone(),
                table_id: storage.table_id,
                selector: identity.directory_selector,
                pointer_slot_count: identity.pointer_count,
                entry_indices: identity.entry_indices.clone(),
                boundary_control: storage.boundary_control,
                page_workset_count,
                dynamic_string_control_count: dynamic_counts
                    .get(identity.record_id.as_str())
                    .copied()
                    .unwrap_or_default(),
                installed_pointer_binding_count,
            })
        })
        .collect()
}

fn build_route_population(
    expected_families: &[ExpectedRouteFamily],
    records: &[JoinedRecord],
    graph: &MainDialogueGraphReport,
    dynamic_producer_routes_bound: bool,
    assembled_hook_roles: &[DialogueRuntimeHookRole],
    new_record_line_buffer_reset_routes_bound: bool,
) -> Result<MainDialogueRoutePopulationPlan> {
    ensure!(
        !expected_families.is_empty(),
        "main-dialogue route population has no expected families"
    );
    let family_by_id = expected_families
        .iter()
        .map(|family| (family.table_id, *family))
        .collect::<BTreeMap<_, _>>();
    ensure!(
        family_by_id.len() == expected_families.len(),
        "main-dialogue route population repeats an expected table"
    );
    ensure!(
        expected_families
            .iter()
            .map(|family| family.selector)
            .collect::<BTreeSet<_>>()
            .len()
            == expected_families.len(),
        "main-dialogue route population repeats a selector"
    );

    let record_ids = records
        .iter()
        .map(|record| record.record_id.as_str())
        .collect::<BTreeSet<_>>();
    ensure!(
        record_ids.len() == records.len(),
        "main-dialogue route population repeats a record"
    );
    ensure!(
        records
            .iter()
            .all(|record| family_by_id.contains_key(record.table_id)),
        "main-dialogue route population contains an unexpected table"
    );
    ensure!(
        graph.cycle_count == 0
            && graph.unresolved_node_count == 0
            && graph.node_count == records.len(),
        "main-dialogue transition graph is not closed over the installed population"
    );

    let mut incoming_by_table = BTreeMap::<&str, usize>::new();
    let mut outgoing_by_table = BTreeMap::<&str, usize>::new();
    let mut cross_family_transition_edge_count = 0;
    for edge in &graph.transition_edges {
        let source_id = format!(
            "{}:{:03}",
            edge.source_table_id, edge.source_canonical_entry_index
        );
        let target_id = format!(
            "{}:{:03}",
            edge.target_table_id, edge.target_canonical_entry_index
        );
        ensure!(
            record_ids.contains(source_id.as_str()) && record_ids.contains(target_id.as_str()),
            "main-dialogue transition leaves the installed record population"
        );
        *outgoing_by_table.entry(edge.source_table_id).or_default() += 1;
        *incoming_by_table.entry(edge.target_table_id).or_default() += 1;
        cross_family_transition_edge_count +=
            usize::from(edge.source_table_id != edge.target_table_id);
    }
    let transition_graph_source_ids = graph
        .transition_edges
        .iter()
        .map(|edge| {
            format!(
                "{}:{:03}",
                edge.source_table_id, edge.source_canonical_entry_index
            )
        })
        .collect::<BTreeSet<_>>();
    let transition_boundary_record_ids = records
        .iter()
        .filter(|record| matches!(record.boundary_control, 0xE4 | 0xE6))
        .map(|record| record.record_id.clone())
        .collect::<BTreeSet<_>>();
    ensure!(
        transition_graph_source_ids == transition_boundary_record_ids
            && transition_graph_source_ids.len() == graph.transition_edges.len(),
        "E4/E6 identity-lookahead records and transition graph sources disagree"
    );
    let e4_lookahead_record_count = records
        .iter()
        .filter(|record| record.boundary_control == 0xE4)
        .count();
    let e6_lookahead_record_count = records
        .iter()
        .filter(|record| record.boundary_control == 0xE6)
        .count();
    let e7_caller_resume_record_count = records
        .iter()
        .filter(|record| record.boundary_control == 0xE7)
        .count();
    let terminal_record_count = records
        .iter()
        .filter(|record| record.boundary_control == 0xEF)
        .count();

    let actual_hook_roles = assembled_hook_roles
        .iter()
        .copied()
        .collect::<BTreeSet<_>>();
    ensure!(
        REQUIRED_COMMON_HOOKS
            .iter()
            .all(|role| actual_hook_roles.contains(role)),
        "main-dialogue route population lacks a common runtime entry hook"
    );
    ensure!(
        new_record_line_buffer_reset_routes_bound,
        "main-dialogue new-record routes do not share the physical line-buffer reset"
    );
    ensure!(
        dynamic_producer_routes_bound,
        "main-dialogue route population has unresolved dynamic-string producer routes"
    );

    let mut route_families = Vec::with_capacity(expected_families.len());
    for expected in expected_families {
        let family_records = records
            .iter()
            .filter(|record| record.table_id == expected.table_id)
            .collect::<Vec<_>>();
        ensure!(
            family_records.len() == expected.canonical_record_count,
            "{} canonical record population changed",
            expected.table_id
        );
        ensure!(
            family_records.iter().all(|record| {
                record.selector == expected.selector
                    && record.pointer_slot_count == expected.pointer_slot_count
            }),
            "{} selector or pointer-table extent changed",
            expected.table_id
        );
        let bound_entries = family_records
            .iter()
            .flat_map(|record| record.entry_indices.iter().copied())
            .collect::<BTreeSet<_>>();
        ensure!(
            bound_entries.len() + expected.handler_entry_count == expected.pointer_slot_count,
            "{} script entries and handler holes do not partition its pointer table",
            expected.table_id
        );
        ensure!(
            bound_entries
                .iter()
                .all(|entry| *entry < expected.pointer_slot_count),
            "{} has a script entry outside its pointer table",
            expected.table_id
        );
        let transition_source_record_count = family_records
            .iter()
            .filter(|record| matches!(record.boundary_control, 0xE4 | 0xE6))
            .count();
        ensure!(
            transition_source_record_count
                == outgoing_by_table
                    .get(expected.table_id)
                    .copied()
                    .unwrap_or_default(),
            "{} transition records and graph edges disagree",
            expected.table_id
        );
        let terminal_record_count = family_records
            .iter()
            .filter(|record| record.boundary_control == 0xEF)
            .count();
        let caller_handoff_record_count = family_records
            .iter()
            .filter(|record| record.boundary_control == 0xE7)
            .count();
        ensure!(
            terminal_record_count + caller_handoff_record_count + transition_source_record_count
                == family_records.len(),
            "{} contains an unclassified dialogue boundary",
            expected.table_id
        );
        route_families.push(MainDialogueRouteFamilyPlan {
            table_id: expected.table_id,
            directory_selector: expected.selector,
            directory_selector_hex: format!("0x{:02X}", expected.selector),
            pointer_slot_count: expected.pointer_slot_count,
            canonical_record_count: expected.canonical_record_count,
            installed_record_count: family_records.len(),
            installed_pointer_binding_count: family_records
                .iter()
                .map(|record| record.installed_pointer_binding_count)
                .sum(),
            handler_entry_count: expected.handler_entry_count,
            page_workset_count: family_records
                .iter()
                .map(|record| record.page_workset_count)
                .sum(),
            dynamic_string_control_count: family_records
                .iter()
                .map(|record| record.dynamic_string_control_count)
                .sum(),
            terminal_record_count,
            caller_handoff_record_count,
            transition_source_record_count,
            incoming_transition_edge_count: incoming_by_table
                .get(expected.table_id)
                .copied()
                .unwrap_or_default(),
            every_record_has_a_runtime_page: true,
        });
    }

    let canonical_record_count = records.len();
    let pointer_slot_count = expected_families
        .iter()
        .map(|family| family.pointer_slot_count)
        .sum::<usize>();
    let installed_pointer_binding_count = records
        .iter()
        .map(|record| record.installed_pointer_binding_count)
        .sum::<usize>();
    let handler_entry_count = expected_families
        .iter()
        .map(|family| family.handler_entry_count)
        .sum::<usize>();
    let dynamic_string_control_count = records
        .iter()
        .map(|record| record.dynamic_string_control_count)
        .sum::<usize>();
    ensure!(
        canonical_record_count == EXPECTED_CANONICAL_RECORD_COUNT
            && pointer_slot_count == EXPECTED_POINTER_SLOT_COUNT
            && installed_pointer_binding_count == EXPECTED_ENTRY_BINDING_COUNT
            && handler_entry_count == EXPECTED_HANDLER_ENTRY_COUNT
            && graph.transition_edges.len() == EXPECTED_TRANSITION_EDGE_COUNT
            && dynamic_string_control_count == EXPECTED_DYNAMIC_CONTROL_COUNT
            && e4_lookahead_record_count == EXPECTED_E4_LOOKAHEAD_RECORD_COUNT
            && e6_lookahead_record_count == EXPECTED_E6_LOOKAHEAD_RECORD_COUNT
            && e7_caller_resume_record_count == EXPECTED_E7_CALLER_RESUME_RECORD_COUNT
            && terminal_record_count == EXPECTED_TERMINAL_RECORD_COUNT
            && e4_lookahead_record_count
                + e6_lookahead_record_count
                + e7_caller_resume_record_count
                + terminal_record_count
                == canonical_record_count,
        "supported main-dialogue route population changed"
    );

    Ok(MainDialogueRoutePopulationPlan {
        strategy: "bind every source selector family to the canonical translated record, pointer, page, transition, dynamic producer, and shared dispatcher-hook populations without inferring gameplay semantics from a table name",
        route_family_count: route_families.len(),
        canonical_record_count,
        pointer_slot_count,
        installed_pointer_binding_count,
        handler_entry_count,
        page_workset_count: records.iter().map(|record| record.page_workset_count).sum(),
        transition_edge_count: graph.transition_edges.len(),
        cross_family_transition_edge_count,
        dynamic_string_control_count,
        common_runtime_hook_roles: REQUIRED_COMMON_HOOKS.to_vec(),
        every_new_record_entry_clears_all_physical_line_buffers: true,
        identity_lookup_boundary_partition: IdentityLookupBoundaryPartition {
            e4_published_lookahead_record_count: e4_lookahead_record_count,
            e6_published_lookahead_record_count: e6_lookahead_record_count,
            e7_live_caller_resume_record_count: e7_caller_resume_record_count,
            terminal_without_followup_lookup_record_count: terminal_record_count,
            transition_graph_sources_equal_e4_and_e6_records: true,
            every_canonical_record_has_one_identity_lookup_boundary: true,
        },
        every_route_family_fully_installed: true,
        every_transition_target_installed: true,
        dynamic_string_producer_routes_bound: true,
        natural_gameplay_branch_semantics_complete: false,
        route_families,
        unresolved: [
            "table-family closure does not prove which natural gameplay condition selects every record",
            "representative recruitment, village or house, defeat, and ending branches still require same-artifact runtime replay",
        ],
    })
}

fn exact_record_set(record_ids: &[String], role: &str) -> Result<BTreeSet<String>> {
    let records = record_ids.iter().cloned().collect::<BTreeSet<_>>();
    ensure!(
        records.len() == record_ids.len(),
        "{role} population contains duplicate record IDs"
    );
    Ok(records)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::dialogue_inventory::MainDialogueTransitionEdgeReport;

    fn record(
        record_id: &str,
        table_id: &'static str,
        selector: u8,
        pointer_slot_count: usize,
        entry_index: usize,
        boundary_control: u8,
    ) -> JoinedRecord {
        JoinedRecord {
            record_id: record_id.to_owned(),
            table_id,
            selector,
            pointer_slot_count,
            entry_indices: vec![entry_index],
            boundary_control,
            page_workset_count: 1,
            dynamic_string_control_count: 0,
            installed_pointer_binding_count: 1,
        }
    }

    fn edge(
        source_table: &'static str,
        target_table: &'static str,
    ) -> MainDialogueTransitionEdgeReport {
        MainDialogueTransitionEdgeReport {
            source_table_id: source_table,
            source_canonical_entry_index: 0,
            source_entry_indices: vec![0],
            source_pointer_cpu_address: 0x9000,
            source_pointer_cpu_address_hex: "0x9000".to_owned(),
            source_file_offset: 0,
            source_file_offset_hex: "0x00000".to_owned(),
            control: 0xE4,
            control_hex: "E4".to_owned(),
            target_table_id: target_table,
            target_entry_index: 0,
            target_canonical_entry_index: 0,
            target_pointer_cpu_address: 0x9000,
            target_pointer_cpu_address_hex: "0x9000".to_owned(),
            target_file_offset: 0,
            target_file_offset_hex: "0x00000".to_owned(),
        }
    }

    #[test]
    fn rejects_a_missing_common_runtime_entry_hook_before_counting_families() {
        let expected = [family("table", 0x20, 1, 1, 0)];
        let records = [record("table:000", "table", 0x20, 1, 0, 0xEF)];
        let error = build_route_population(
            &expected,
            &records,
            &MainDialogueGraphReport {
                node_count: 1,
                transition_edge_count: 0,
                terminal_reachable_node_count: 1,
                caller_handoff_boundary_reachable_node_count: 0,
                max_transition_edge_count_to_boundary: 0,
                cycle_count: 0,
                unresolved_node_count: 0,
                transition_edges: vec![],
            },
            true,
            &[],
            true,
        )
        .err()
        .expect("missing common runtime hook must fail");
        assert!(error.to_string().contains("common runtime entry hook"));
    }

    #[test]
    fn common_hooks_without_the_shared_physical_row_reset_are_not_complete_routes() {
        let expected = [family("table", 0x20, 1, 1, 0)];
        let records = [record("table:000", "table", 0x20, 1, 0, 0xEF)];
        let error = build_route_population(
            &expected,
            &records,
            &MainDialogueGraphReport {
                node_count: 1,
                transition_edge_count: 0,
                terminal_reachable_node_count: 1,
                caller_handoff_boundary_reachable_node_count: 0,
                max_transition_edge_count_to_boundary: 0,
                cycle_count: 0,
                unresolved_node_count: 0,
                transition_edges: vec![],
            },
            true,
            &REQUIRED_COMMON_HOOKS,
            false,
        )
        .err()
        .expect("missing shared physical row reset must fail");

        assert!(error.to_string().contains("physical line-buffer reset"));
    }

    #[test]
    fn transition_target_must_belong_to_the_installed_population() {
        let expected = [family("source", 0x20, 1, 1, 0)];
        let records = [record("source:000", "source", 0x20, 1, 0, 0xE4)];
        let graph = MainDialogueGraphReport {
            node_count: 1,
            transition_edge_count: 1,
            terminal_reachable_node_count: 1,
            caller_handoff_boundary_reachable_node_count: 0,
            max_transition_edge_count_to_boundary: 1,
            cycle_count: 0,
            unresolved_node_count: 0,
            transition_edges: vec![edge("source", "missing")],
        };
        let error = build_route_population(
            &expected,
            &records,
            &graph,
            true,
            &REQUIRED_COMMON_HOOKS,
            true,
        )
        .err()
        .expect("transition outside the installed population must fail");
        assert!(
            error
                .to_string()
                .contains("leaves the installed record population")
        );
    }

    #[test]
    fn handler_holes_cannot_be_reclassified_as_translated_records() {
        let expected = [family("recruitment", 0x71, 2, 1, 1)];
        let mut records = [record("recruitment:000", "recruitment", 0x71, 2, 0, 0xEF)];
        records[0].entry_indices.push(1);
        records[0].installed_pointer_binding_count = 2;
        let graph = MainDialogueGraphReport {
            node_count: 1,
            transition_edge_count: 0,
            terminal_reachable_node_count: 1,
            caller_handoff_boundary_reachable_node_count: 0,
            max_transition_edge_count_to_boundary: 0,
            cycle_count: 0,
            unresolved_node_count: 0,
            transition_edges: vec![],
        };
        let error = build_route_population(
            &expected,
            &records,
            &graph,
            true,
            &REQUIRED_COMMON_HOOKS,
            true,
        )
        .err()
        .expect("handler hole reclassification must fail");
        assert!(error.to_string().contains("handler holes"));
    }

    #[test]
    fn unresolved_dynamic_producer_routes_block_the_family_plan() {
        let expected = [family("table", 0x20, 1, 1, 0)];
        let records = [record("table:000", "table", 0x20, 1, 0, 0xEF)];
        let graph = MainDialogueGraphReport {
            node_count: 1,
            transition_edge_count: 0,
            terminal_reachable_node_count: 1,
            caller_handoff_boundary_reachable_node_count: 0,
            max_transition_edge_count_to_boundary: 0,
            cycle_count: 0,
            unresolved_node_count: 0,
            transition_edges: vec![],
        };
        let error = build_route_population(
            &expected,
            &records,
            &graph,
            false,
            &REQUIRED_COMMON_HOOKS,
            true,
        )
        .err()
        .expect("unresolved dynamic producer routes must fail");
        assert!(error.to_string().contains("dynamic-string producer"));
    }
}
