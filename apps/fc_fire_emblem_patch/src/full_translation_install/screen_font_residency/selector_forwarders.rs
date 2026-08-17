//! 중앙 거주 정책으로 이관이 끝난 화면의 누적 선택기를 순수 전달자로 바꾼다.

use anyhow::{Context, Result, ensure};
use serde::Serialize;

use crate::{
    front_end_menu::{
        FRONT_END_FONT_STATES, RECORD_ACTION_COMPOSITE_STATE, RECORD_LIST_COMPOSITE_STATE,
        SAVE_SLOT_SELECTION_COMPOSITE_STATE, START_MENU_COMPOSITE_STATE,
    },
    mapper165::{
        bind_front_end_font_page_selector, bind_unit_name_font_page_selector,
        build_front_end_font_page_forwarder, build_unit_name_font_page_forwarder,
    },
    rom::{HEADER_SIZE, Rom},
};

use super::{
    COMPOSITE_FONT_RESIDENCY_POLICIES, ScreenFontPageRole, ScreenFontPageRoutes,
    ScreenFontResidencyPolicy, UNIT_STATUS_COMPOSITE_STATE, UNIT_SUMMARY_COMPOSITE_STATE,
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

#[derive(Serialize)]
pub(in crate::full_translation_install) struct FontPageSelectorForwarderPlan {
    schema: u8,
    strategy: &'static str,
    centralized_selector_count: usize,
    centrally_owned_composite_state_count: usize,
    centrally_owned_translation_domain_count: usize,
    direct_predecessor_count: usize,
    installed_forwarder_byte_count: usize,
    central_policy_owns_every_removed_decision: bool,
    source_selector_structure_bound: bool,
    direct_entry_census_bound: bool,
    selectors: Vec<FontPageSelectorForwarderReport>,
    #[serde(skip)]
    writes: Vec<FontPageSelectorExpectedWrite>,
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
    let unit_selector = bind_unit_name_font_page_selector(candidate)?;
    let unit_replacement = build_unit_name_font_page_forwarder(&unit_selector)?;
    let front_end_selector = bind_front_end_font_page_selector(candidate)?;
    let front_end_replacement = build_front_end_font_page_forwarder(&front_end_selector)?;
    ensure!(
        unit_selector.cpu_end_exclusive <= front_end_selector.cpu_address,
        "migrated unit-name and front-end selector spans overlap"
    );
    let unique_domains = FRONT_END_DOMAINS
        .iter()
        .chain(UNIT_NAME_PAGE_DOMAINS)
        .copied()
        .collect::<std::collections::BTreeSet<_>>();

    Ok(FontPageSelectorForwarderPlan {
        schema: 2,
        strategy: "replace a cumulative screen selector only after the central residency policy owns every state it used to decide; retain the next unowned selector as an explicit full-span forwarder target",
        centralized_selector_count: 2,
        centrally_owned_composite_state_count: FRONT_END_FONT_STATES.len() + 2,
        centrally_owned_translation_domain_count: unique_domains.len(),
        direct_predecessor_count: 2,
        installed_forwarder_byte_count: unit_replacement.len() + front_end_replacement.len(),
        central_policy_owns_every_removed_decision: true,
        source_selector_structure_bound: true,
        direct_entry_census_bound: true,
        selectors: vec![
            forwarder_report(
                "unit_summary_and_status",
                2,
                UNIT_NAME_PAGE_DOMAINS,
                &unit_selector,
                unit_replacement.len(),
            ),
            forwarder_report(
                "front_end_menu",
                FRONT_END_FONT_STATES.len(),
                FRONT_END_DOMAINS,
                &front_end_selector,
                front_end_replacement.len(),
            ),
        ],
        writes: vec![
            FontPageSelectorExpectedWrite {
                domains: UNIT_NAME_PAGE_DOMAINS,
                role: "replace the cumulative unit-name font selector with a central-policy fallback forwarder",
                file_offset: active_fixed_file_offset(candidate, unit_selector.cpu_address)?,
                cpu_address: unit_selector.cpu_address,
                expected: unit_selector.expected_bytes,
                replacement: unit_replacement,
            },
            FontPageSelectorExpectedWrite {
                domains: FRONT_END_DOMAINS,
                role: "replace the cumulative front-end font selector with a central-policy fallback forwarder",
                file_offset: active_fixed_file_offset(candidate, front_end_selector.cpu_address)?,
                cpu_address: front_end_selector.cpu_address,
                expected: front_end_selector.expected_bytes,
                replacement: front_end_replacement,
            },
        ],
    })
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
        let policies = COMPOSITE_FONT_RESIDENCY_POLICIES
            .iter()
            .filter(|(candidate_state, _)| *candidate_state == state)
            .map(|(_, policy)| *policy)
            .collect::<Vec<_>>();
        ensure!(
            policies == vec![required_policy] && route != 0,
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
        let policies = COMPOSITE_FONT_RESIDENCY_POLICIES
            .iter()
            .filter(|(candidate_state, _)| *candidate_state == state)
            .map(|(_, policy)| *policy)
            .collect::<Vec<_>>();
        ensure!(
            policies == vec![required_policy] && routes.catalog.iter().all(|route| *route != 0),
            "unit-name state {state:02X} is not exclusively owned by a nonempty central font route"
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
