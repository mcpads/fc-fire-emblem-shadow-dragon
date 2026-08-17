use std::collections::BTreeMap;

use super::*;

pub(super) fn selector_fallback_graph_report(
    graph: &BoundFontPageFallbackGraph,
) -> SelectorFallbackGraphReport {
    let mut incoming_route_counts = BTreeMap::<&str, usize>::new();
    for route in &graph.routes {
        *incoming_route_counts.entry(route.target_role).or_default() += 1;
    }
    SelectorFallbackGraphReport {
        schema: 1,
        node_count: graph.nodes.len(),
        route_count: graph.routes.len(),
        multi_entry_target_count: incoming_route_counts
            .values()
            .filter(|count| **count > 1)
            .count(),
        direct_entry_candidate_count: graph.direct_entry_candidate_count,
        conditional_entry_count: graph.conditional_entry_count,
        terminal_fallback_count: graph.terminal_fallback_count,
        generated_selector_structure_bound: true,
        active_fixed_direct_entry_candidates_partitioned: true,
        nodes: graph
            .nodes
            .iter()
            .map(|node| SelectorFallbackNodeReport {
                role: node.role.id(),
                cpu_range_hex: format!(
                    "0x{:04X}..0x{:04X}",
                    node.cpu_address, node.cpu_end_exclusive
                ),
                mapper_registers_hex: node
                    .mapper_registers
                    .iter()
                    .map(|register| format!("0x{register:02X}"))
                    .collect(),
                admitted_chapter_indices: if node.role
                    == FontPageFallbackNodeRole::ChapterIntroDialogue
                {
                    vec![CHAPTER_ONE_INDEX, CHAPTER_TWO_INDEX]
                } else {
                    Vec::new()
                },
            })
            .collect(),
        routes: graph
            .routes
            .iter()
            .map(|route| SelectorFallbackRouteReport {
                source_role: route.source_role,
                source_cpu_address_hex: format!("0x{:04X}", route.source_cpu_address),
                transfer_kind: route.transfer_kind.id(),
                target_role: route.target_role,
                target_cpu_address_hex: format!("0x{:04X}", route.target_cpu_address),
            })
            .collect(),
    }
}
