//! 중앙 거주 정책으로 이관이 끝난 화면의 누적 선택기를 순수 전달자로 바꾼다.

use anyhow::{Context, Result, ensure};
use serde::Serialize;

use crate::{
    front_end_menu::{
        FRONT_END_FONT_STATES, RECORD_ACTION_COMPOSITE_STATE, RECORD_LIST_COMPOSITE_STATE,
        SAVE_SLOT_SELECTION_COMPOSITE_STATE, START_MENU_COMPOSITE_STATE,
    },
    mapper165::{bind_front_end_font_page_selector, build_front_end_font_page_forwarder},
    rom::{HEADER_SIZE, Rom},
};

use super::{
    COMPOSITE_FONT_RESIDENCY_POLICIES, ScreenFontPageRole, ScreenFontPageRoutes,
    ScreenFontResidencyPolicy,
};

const FIXED_BANK_BYTE_COUNT: usize = 16 * 1024;
const FRONT_END_DOMAIN: &str = "front_end_menu_labels";

#[derive(Serialize)]
pub(in crate::full_translation_install) struct FontPageSelectorForwarderPlan {
    schema: u8,
    strategy: &'static str,
    centralized_selector_count: usize,
    centrally_owned_composite_state_count: usize,
    direct_predecessor_count: usize,
    source_selector_mapper_register: u8,
    source_selector_cpu_range_hex: String,
    forward_target_cpu_address_hex: String,
    direct_predecessor_cpu_address_hex: String,
    installed_forwarder_byte_count: usize,
    central_policy_owns_every_removed_decision: bool,
    source_selector_structure_bound: bool,
    direct_entry_census_bound: bool,
    #[serde(skip)]
    writes: Vec<FontPageSelectorExpectedWrite>,
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
            .filter(|write| write.domain == domain)
            .count()
    }
}

pub(in crate::full_translation_install) struct FontPageSelectorExpectedWrite {
    pub(in crate::full_translation_install) domain: &'static str,
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
    let selector = bind_front_end_font_page_selector(candidate)?;
    let replacement = build_front_end_font_page_forwarder(&selector)?;
    let file_offset = active_fixed_file_offset(candidate, selector.cpu_address)?;

    Ok(FontPageSelectorForwarderPlan {
        schema: 1,
        strategy: "replace a cumulative screen selector only after the central residency policy owns every state it used to decide; retain the next unowned selector as an explicit full-span forwarder target",
        centralized_selector_count: 1,
        centrally_owned_composite_state_count: FRONT_END_FONT_STATES.len(),
        direct_predecessor_count: 1,
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
        installed_forwarder_byte_count: replacement.len(),
        central_policy_owns_every_removed_decision: true,
        source_selector_structure_bound: true,
        direct_entry_census_bound: true,
        writes: vec![FontPageSelectorExpectedWrite {
            domain: FRONT_END_DOMAIN,
            role: "replace the cumulative front-end font selector with a central-policy fallback forwarder",
            file_offset,
            cpu_address: selector.cpu_address,
            expected: selector.expected_bytes,
            replacement,
        }],
    })
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
    use crate::mapper165::BoundFrontEndFontPageSelector;

    fn binding() -> BoundFrontEndFontPageSelector {
        BoundFrontEndFontPageSelector {
            cpu_address: 0xFC60,
            cpu_end_exclusive: 0xFC99,
            fallback_target: 0xFBD4,
            mapper_register: 0xA8,
            direct_predecessor_address: 0xF797,
            expected_bytes: vec![0xCC; 0x39],
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
    fn the_complete_old_selector_span_only_forwards_to_the_unowned_chain() {
        let forwarder = build_front_end_font_page_forwarder(&binding()).unwrap();

        assert_eq!(&forwarder[..3], &[0x4C, 0xD4, 0xFB]);
        assert!(forwarder[3..].iter().all(|byte| *byte == 0xEA));
        assert_eq!(forwarder.len(), 0x39);
        assert!(
            !forwarder
                .windows(3)
                .any(|bytes| bytes == [0x8D, 0x01, 0x80])
        );
    }
}
