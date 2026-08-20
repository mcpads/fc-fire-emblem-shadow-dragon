//! 중앙 거주 정책으로 이관이 끝난 화면의 누적 선택기를 순수 전달자로 바꾼다.

use anyhow::{Context, Result, ensure};
use serde::Serialize;

use crate::{
    front_end_menu::{
        FRONT_END_FONT_STATES, RECORD_ACTION_COMPOSITE_STATE, RECORD_LIST_COMPOSITE_STATE,
        SAVE_SLOT_SELECTION_COMPOSITE_STATE, START_MENU_COMPOSITE_STATE,
    },
    mapper165::{
        BoundFontPageFallbackGraph, FontPageFallbackNodeRole, OPTIONS_FONT_PAGE_COMPOSITE_STATES,
        ROSTER_FONT_PAGE_COMPOSITE_STATE, bind_cumulative_font_page_fallback_graph,
        build_front_end_font_page_forwarder, build_unit_name_font_page_forwarder,
    },
    rom::{HEADER_SIZE, Rom},
};

use super::{
    DelegatedFontPageOwner, ScreenFontPageRole, ScreenFontPageRoutes, ScreenFontResidencyPolicy,
    UNIT_STATUS_COMPOSITE_STATE, UNIT_SUMMARY_COMPOSITE_STATE, composite_font_residency_policy,
};

const FIXED_BANK_BYTE_COUNT: usize = 16 * 1024;
const FRONT_END_DOMAINS: &[&str] = &["front_end_menu_labels"];
const UNIT_NAME_PAGE_DOMAINS: &[&str] = &[
    "unit_names",
    "enemy_names",
    "class_names",
    "item_names",
    "unit_ui_labels",
];
const DELEGATED_DYNAMIC_SELECTOR_OWNERS: &[(
    DelegatedFontPageOwner,
    FontPageFallbackNodeRole,
    &[u8],
)] = &[
    (
        DelegatedFontPageOwner::OptionsSelector,
        FontPageFallbackNodeRole::OptionsMenu,
        &OPTIONS_FONT_PAGE_COMPOSITE_STATES,
    ),
    (
        DelegatedFontPageOwner::UnitRosterSelector,
        FontPageFallbackNodeRole::UnitRoster,
        &[ROSTER_FONT_PAGE_COMPOSITE_STATE],
    ),
];

#[derive(Serialize)]
pub(in crate::full_translation_install) struct FontPageSelectorForwarderPlan {
    schema: u8,
    strategy: &'static str,
    centralized_selector_count: usize,
    centrally_owned_composite_state_count: usize,
    centrally_owned_translation_domain_count: usize,
    direct_predecessor_count: usize,
    installed_forwarder_byte_count: usize,
    retained_dynamic_selector_count: usize,
    delegated_dynamic_selector_state_count: usize,
    integrated_runtime_rebound_selector_count: usize,
    central_policy_owns_every_removed_decision: bool,
    source_selector_structure_bound: bool,
    direct_entry_census_bound: bool,
    source_fallback_graph: FontPageFallbackGraphMigrationReport,
    selectors: Vec<FontPageSelectorForwarderReport>,
    #[serde(skip)]
    writes: Vec<FontPageSelectorExpectedWrite>,
    #[serde(skip)]
    bound_source_graph: BoundFontPageFallbackGraph,
}

#[derive(Serialize)]
struct FontPageSelectorForwarderReport {
    role: &'static str,
    centrally_owned_composite_state_count: usize,
    translation_domains: &'static [&'static str],
    source_selector_mapper_register: u8,
    source_selector_cpu_range_hex: String,
    forward_target_cpu_address_hex: String,
    direct_predecessor_cpu_address_hex: String,
    installed_forwarder_byte_count: usize,
}

#[derive(Serialize)]
struct FontPageFallbackGraphMigrationReport {
    schema: u8,
    source_node_count: usize,
    source_route_count: usize,
    source_direct_entry_candidate_count: usize,
    source_conditional_entry_count: usize,
    source_terminal_fallback_count: usize,
    central_policy_forwarder_count: usize,
    retained_dynamic_selector_count: usize,
    integrated_runtime_rebound_selector_count: usize,
    source_graph_is_branching: bool,
    nodes: Vec<FontPageFallbackNodeMigrationReport>,
    routes: Vec<FontPageFallbackRouteMigrationReport>,
}

#[derive(Serialize)]
struct FontPageFallbackNodeMigrationReport {
    role: &'static str,
    source_cpu_range_hex: String,
    source_mapper_registers_hex: Vec<String>,
    final_owner: &'static str,
}

#[derive(Serialize)]
struct FontPageFallbackRouteMigrationReport {
    source_role: &'static str,
    source_cpu_address_hex: String,
    transfer_kind: &'static str,
    target_role: &'static str,
    target_cpu_address_hex: String,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum FinalFontPageSelectorOwner {
    CentralScreenResidency,
    RetainedDynamicSelector,
    IntegratedRuntime,
}

impl FinalFontPageSelectorOwner {
    const fn for_role(role: FontPageFallbackNodeRole) -> Self {
        match role {
            FontPageFallbackNodeRole::UnitSummaryAndStatus
            | FontPageFallbackNodeRole::FrontEndMenu => Self::CentralScreenResidency,
            FontPageFallbackNodeRole::OptionsMenu
            | FontPageFallbackNodeRole::UnitRoster
            | FontPageFallbackNodeRole::WeaponShopDialogue
            | FontPageFallbackNodeRole::ChapterIntroDialogue => Self::RetainedDynamicSelector,
            FontPageFallbackNodeRole::BattleComposition
            | FontPageFallbackNodeRole::MaximumDialogue => Self::IntegratedRuntime,
        }
    }

    const fn id(self) -> &'static str {
        match self {
            Self::CentralScreenResidency => "central_screen_residency_forwarder",
            Self::RetainedDynamicSelector => "retained_dynamic_selector",
            Self::IntegratedRuntime => "integrated_runtime_rebound",
        }
    }
}

impl FontPageSelectorForwarderPlan {
    pub(in crate::full_translation_install) fn writes(&self) -> &[FontPageSelectorExpectedWrite] {
        &self.writes
    }

    pub(in crate::full_translation_install) fn write_count(&self) -> usize {
        self.writes.len()
    }

    pub(in crate::full_translation_install) fn write_count_for_domain(
        &self,
        domain: &str,
    ) -> usize {
        self.writes
            .iter()
            .filter(|write| write.domains.contains(&domain))
            .count()
    }

    pub(in crate::full_translation_install) fn verify_retained_dynamic_selectors(
        &self,
        installed: &[u8],
        candidate: &Rom,
    ) -> Result<()> {
        let retained = self
            .bound_source_graph
            .nodes
            .iter()
            .filter(|node| {
                FinalFontPageSelectorOwner::for_role(node.role)
                    == FinalFontPageSelectorOwner::RetainedDynamicSelector
            })
            .collect::<Vec<_>>();
        ensure!(
            retained.len() == 4,
            "final font-page plan lost a retained dynamic selector"
        );
        for node in retained {
            let offset = active_fixed_file_offset(candidate, node.cpu_address)?;
            ensure!(
                installed.get(offset..offset + node.expected_bytes.len())
                    == Some(node.expected_bytes.as_slice()),
                "final installation changed retained {} selector bytes",
                node.role.id()
            );
        }
        Ok(())
    }
}

pub(in crate::full_translation_install) struct FontPageSelectorExpectedWrite {
    pub(in crate::full_translation_install) domains: &'static [&'static str],
    pub(in crate::full_translation_install) role: &'static str,
    pub(in crate::full_translation_install) file_offset: usize,
    pub(in crate::full_translation_install) cpu_address: u16,
    pub(in crate::full_translation_install) expected: Vec<u8>,
    pub(in crate::full_translation_install) replacement: Vec<u8>,
}

pub(super) fn plan_font_page_selector_forwarders(
    candidate: &Rom,
    routes: ScreenFontPageRoutes,
) -> Result<FontPageSelectorForwarderPlan> {
    routes.validate()?;
    bind_front_end_central_ownership(routes)?;
    bind_unit_name_central_ownership(routes)?;
    let bound_source_graph = bind_cumulative_font_page_fallback_graph(candidate)?;
    bind_delegated_dynamic_selector_ownership(&bound_source_graph)?;
    let unit_selector = bound_source_graph.unit_name_selector();
    let unit_replacement = build_unit_name_font_page_forwarder(unit_selector)?;
    let front_end_selector = bound_source_graph.front_end_selector();
    let front_end_replacement = build_front_end_font_page_forwarder(front_end_selector)?;
    ensure!(
        unit_selector.cpu_end_exclusive <= front_end_selector.cpu_address,
        "migrated unit-name and front-end selector spans overlap"
    );
    let unique_domains = FRONT_END_DOMAINS
        .iter()
        .chain(UNIT_NAME_PAGE_DOMAINS)
        .copied()
        .collect::<std::collections::BTreeSet<_>>();
    let source_fallback_graph = fallback_graph_migration_report(&bound_source_graph);

    Ok(FontPageSelectorForwarderPlan {
        schema: 5,
        strategy: "bind the complete branching cumulative fallback graph before replacing a screen selector; centralize only decisions fully owned by the screen residency plan, preserve dynamic options, roster, shop, and chapter-dialogue selectors, and leave battle/maximum-dialogue rebinding to the integrated runtime owner",
        centralized_selector_count: source_fallback_graph.central_policy_forwarder_count,
        centrally_owned_composite_state_count: FRONT_END_FONT_STATES.len() + 2,
        centrally_owned_translation_domain_count: unique_domains.len(),
        direct_predecessor_count: 2,
        installed_forwarder_byte_count: unit_replacement.len() + front_end_replacement.len(),
        retained_dynamic_selector_count: source_fallback_graph.retained_dynamic_selector_count,
        delegated_dynamic_selector_state_count: DELEGATED_DYNAMIC_SELECTOR_OWNERS
            .iter()
            .map(|(_, _, states)| states.len())
            .sum(),
        integrated_runtime_rebound_selector_count: source_fallback_graph
            .integrated_runtime_rebound_selector_count,
        central_policy_owns_every_removed_decision: true,
        source_selector_structure_bound: true,
        direct_entry_census_bound: true,
        source_fallback_graph,
        selectors: vec![
            forwarder_report(
                "unit_summary_and_status",
                2,
                UNIT_NAME_PAGE_DOMAINS,
                unit_selector,
                unit_replacement.len(),
            ),
            forwarder_report(
                "front_end_menu",
                FRONT_END_FONT_STATES.len(),
                FRONT_END_DOMAINS,
                front_end_selector,
                front_end_replacement.len(),
            ),
        ],
        writes: vec![
            FontPageSelectorExpectedWrite {
                domains: UNIT_NAME_PAGE_DOMAINS,
                role: "replace the cumulative unit-name font selector with a central-policy fallback forwarder",
                file_offset: active_fixed_file_offset(candidate, unit_selector.cpu_address)?,
                cpu_address: unit_selector.cpu_address,
                expected: unit_selector.expected_bytes.clone(),
                replacement: unit_replacement,
            },
            FontPageSelectorExpectedWrite {
                domains: FRONT_END_DOMAINS,
                role: "replace the cumulative front-end font selector with a central-policy fallback forwarder",
                file_offset: active_fixed_file_offset(candidate, front_end_selector.cpu_address)?,
                cpu_address: front_end_selector.cpu_address,
                expected: front_end_selector.expected_bytes.clone(),
                replacement: front_end_replacement,
            },
        ],
        bound_source_graph,
    })
}

fn fallback_graph_migration_report(
    graph: &BoundFontPageFallbackGraph,
) -> FontPageFallbackGraphMigrationReport {
    let nodes = graph
        .nodes
        .iter()
        .map(|node| FontPageFallbackNodeMigrationReport {
            role: node.role.id(),
            source_cpu_range_hex: format!(
                "0x{:04X}..0x{:04X}",
                node.cpu_address, node.cpu_end_exclusive
            ),
            source_mapper_registers_hex: node
                .mapper_registers
                .iter()
                .map(|register| format!("0x{register:02X}"))
                .collect(),
            final_owner: FinalFontPageSelectorOwner::for_role(node.role).id(),
        })
        .collect::<Vec<_>>();
    FontPageFallbackGraphMigrationReport {
        schema: 1,
        source_node_count: nodes.len(),
        source_route_count: graph.routes.len(),
        source_direct_entry_candidate_count: graph.direct_entry_candidate_count,
        source_conditional_entry_count: graph.conditional_entry_count,
        source_terminal_fallback_count: graph.terminal_fallback_count,
        central_policy_forwarder_count: nodes
            .iter()
            .filter(|node| node.final_owner == "central_screen_residency_forwarder")
            .count(),
        retained_dynamic_selector_count: nodes
            .iter()
            .filter(|node| node.final_owner == "retained_dynamic_selector")
            .count(),
        integrated_runtime_rebound_selector_count: nodes
            .iter()
            .filter(|node| node.final_owner == "integrated_runtime_rebound")
            .count(),
        source_graph_is_branching: graph
            .routes
            .iter()
            .filter(|route| route.target_role == FontPageFallbackNodeRole::UnitRoster.id())
            .count()
            == 2,
        nodes,
        routes: graph
            .routes
            .iter()
            .map(|route| FontPageFallbackRouteMigrationReport {
                source_role: route.source_role,
                source_cpu_address_hex: format!("0x{:04X}", route.source_cpu_address),
                transfer_kind: route.transfer_kind.id(),
                target_role: route.target_role,
                target_cpu_address_hex: format!("0x{:04X}", route.target_cpu_address),
            })
            .collect(),
    }
}

fn forwarder_report(
    role: &'static str,
    state_count: usize,
    translation_domains: &'static [&'static str],
    selector: &crate::mapper165::BoundFontPageSelector,
    installed_byte_count: usize,
) -> FontPageSelectorForwarderReport {
    FontPageSelectorForwarderReport {
        role,
        centrally_owned_composite_state_count: state_count,
        translation_domains,
        source_selector_mapper_register: selector.mapper_register,
        source_selector_cpu_range_hex: format!(
            "0x{:04X}..0x{:04X}",
            selector.cpu_address, selector.cpu_end_exclusive
        ),
        forward_target_cpu_address_hex: format!("0x{:04X}", selector.fallback_target),
        direct_predecessor_cpu_address_hex: format!(
            "0x{:04X}",
            selector.direct_predecessor_address
        ),
        installed_forwarder_byte_count: installed_byte_count,
    }
}

fn bind_front_end_central_ownership(routes: ScreenFontPageRoutes) -> Result<()> {
    let expected = [
        (
            START_MENU_COMPOSITE_STATE,
            ScreenFontResidencyPolicy::Static(ScreenFontPageRole::FrontEndMenu),
            routes.front_end_menu,
        ),
        (
            RECORD_LIST_COMPOSITE_STATE,
            ScreenFontResidencyPolicy::Static(ScreenFontPageRole::FrontEndMenu),
            routes.front_end_menu,
        ),
        (
            SAVE_SLOT_SELECTION_COMPOSITE_STATE,
            ScreenFontResidencyPolicy::Static(ScreenFontPageRole::FrontEndMenu),
            routes.front_end_menu,
        ),
        (
            RECORD_ACTION_COMPOSITE_STATE,
            ScreenFontResidencyPolicy::Static(ScreenFontPageRole::FrontEndRecordAction),
            routes.front_end_record_action,
        ),
    ];
    ensure!(
        expected.map(|(state, _, _)| state) == FRONT_END_FONT_STATES,
        "front-end selector migration state population changed"
    );
    for (state, required_policy, route) in expected {
        ensure!(
            composite_font_residency_policy(state) == Some(required_policy) && route != 0,
            "front-end state {state:02X} is not exclusively owned by a nonempty central font route"
        );
    }
    Ok(())
}

fn bind_unit_name_central_ownership(routes: ScreenFontPageRoutes) -> Result<()> {
    let expected = [
        (
            UNIT_SUMMARY_COMPOSITE_STATE,
            ScreenFontResidencyPolicy::UnitOrEnemyNamePublishedByAppender,
        ),
        (
            UNIT_STATUS_COMPOSITE_STATE,
            ScreenFontResidencyPolicy::UnitOrEnemyNameRetainedFromSummary,
        ),
    ];
    for (state, required_policy) in expected {
        ensure!(
            composite_font_residency_policy(state) == Some(required_policy)
                && routes.catalog.iter().all(|route| *route != 0),
            "unit-name state {state:02X} is not exclusively owned by a nonempty central font route"
        );
    }
    Ok(())
}

fn bind_delegated_dynamic_selector_ownership(graph: &BoundFontPageFallbackGraph) -> Result<()> {
    for &(owner, role, states) in DELEGATED_DYNAMIC_SELECTOR_OWNERS {
        ensure!(
            !states.is_empty(),
            "{} owns no composite states",
            owner.id()
        );
        for &state in states {
            ensure!(
                composite_font_residency_policy(state)
                    == Some(ScreenFontResidencyPolicy::Delegated(owner)),
                "composite state {state:02X} is not delegated to {}",
                owner.id()
            );
        }
        let nodes = graph
            .nodes
            .iter()
            .filter(|node| node.role == role)
            .collect::<Vec<_>>();
        ensure!(
            nodes.len() == 1 && !nodes[0].mapper_registers.is_empty(),
            "delegated {} has no unique generated selector",
            owner.id()
        );
    }
    Ok(())
}

fn active_fixed_file_offset(candidate: &Rom, address: u16) -> Result<usize> {
    ensure!(address >= 0xC000, "fixed forwarder address is below $C000");
    HEADER_SIZE
        .checked_add(
            candidate
                .prg()
                .len()
                .checked_sub(FIXED_BANK_BYTE_COUNT)
                .context("candidate PRG is smaller than its active fixed bank")?,
        )
        .and_then(|base| base.checked_add(usize::from(address - 0xC000)))
        .context("front-end forwarder file offset overflow")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::mapper165::BoundFontPageSelector;

    fn binding(
        cpu_address: u16,
        cpu_end_exclusive: u16,
        fallback_target: u16,
    ) -> BoundFontPageSelector {
        BoundFontPageSelector {
            cpu_address,
            cpu_end_exclusive,
            fallback_target,
            mapper_register: 0xA8,
            direct_predecessor_address: 0xF797,
            expected_bytes: vec![0xCC; usize::from(cpu_end_exclusive - cpu_address)],
        }
    }

    fn routes() -> ScreenFontPageRoutes {
        ScreenFontPageRoutes {
            front_end_menu: 0xA9,
            front_end_record_action: 0xDD,
            unit_command: 0xCC,
            map_menu: 0xD0,
            ending_record: 0xD9,
            chapter_save_offer: 0xD4,
            catalog: [0xDC, 0xE0],
        }
    }

    #[test]
    fn every_removed_front_end_decision_has_one_central_owner() {
        bind_front_end_central_ownership(routes()).unwrap();
    }

    #[test]
    fn unit_summary_and_status_have_distinct_central_owners() {
        bind_unit_name_central_ownership(routes()).unwrap();
    }

    #[test]
    fn every_fallback_node_has_one_explicit_final_owner() {
        let roles = [
            FontPageFallbackNodeRole::BattleComposition,
            FontPageFallbackNodeRole::MaximumDialogue,
            FontPageFallbackNodeRole::OptionsMenu,
            FontPageFallbackNodeRole::UnitRoster,
            FontPageFallbackNodeRole::UnitSummaryAndStatus,
            FontPageFallbackNodeRole::WeaponShopDialogue,
            FontPageFallbackNodeRole::FrontEndMenu,
            FontPageFallbackNodeRole::ChapterIntroDialogue,
        ];
        let owned_by = |owner| {
            roles
                .into_iter()
                .filter(|role| FinalFontPageSelectorOwner::for_role(*role) == owner)
                .collect::<std::collections::BTreeSet<_>>()
        };

        assert_eq!(
            owned_by(FinalFontPageSelectorOwner::CentralScreenResidency),
            std::collections::BTreeSet::from([
                FontPageFallbackNodeRole::UnitSummaryAndStatus,
                FontPageFallbackNodeRole::FrontEndMenu,
            ])
        );
        assert_eq!(
            owned_by(FinalFontPageSelectorOwner::RetainedDynamicSelector),
            std::collections::BTreeSet::from([
                FontPageFallbackNodeRole::OptionsMenu,
                FontPageFallbackNodeRole::UnitRoster,
                FontPageFallbackNodeRole::WeaponShopDialogue,
                FontPageFallbackNodeRole::ChapterIntroDialogue,
            ])
        );
        assert_eq!(
            owned_by(FinalFontPageSelectorOwner::IntegratedRuntime),
            std::collections::BTreeSet::from([
                FontPageFallbackNodeRole::BattleComposition,
                FontPageFallbackNodeRole::MaximumDialogue,
            ])
        );
    }

    #[test]
    fn the_complete_old_selector_span_only_forwards_to_the_unowned_chain() {
        let forwarder =
            build_front_end_font_page_forwarder(&binding(0xFC60, 0xFC99, 0xFBD4)).unwrap();

        assert_eq!(&forwarder[..3], &[0x4C, 0xD4, 0xFB]);
        assert!(forwarder[3..].iter().all(|byte| *byte == 0xEA));
        assert_eq!(forwarder.len(), 0x39);
        assert!(
            !forwarder
                .windows(3)
                .any(|bytes| bytes == [0x8D, 0x01, 0x80])
        );
    }

    #[test]
    fn the_unit_name_selector_only_forwards_to_the_shop_chain() {
        let forwarder =
            build_unit_name_font_page_forwarder(&binding(0xF700, 0xF748, 0xF748)).unwrap();

        assert_eq!(&forwarder[..3], &[0x4C, 0x48, 0xF7]);
        assert!(forwarder[3..].iter().all(|byte| *byte == 0xEA));
        assert_eq!(forwarder.len(), 0x48);
    }
}
