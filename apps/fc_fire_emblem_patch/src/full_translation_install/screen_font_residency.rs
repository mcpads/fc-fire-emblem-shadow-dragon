//! 동시에 보이는 비대사 표면을 한 CHR 페이지 경로에 결속한다.
//!
//! 문자열 재료와 카탈로그 페이지는 어느 화면에서 쓰이는지 결정하지 않는다. 이 모듈이
//! 원천 합성 상태와 가시 표면을 합쳐 화면별 경로를 고르고, 런타임 코드는 그 결과를
//! 그대로 방출한다. 이전 화면이 남긴 `$07FD`를 암묵적인 입력으로 쓰지 않는다.

mod dialogue_surfaces;
mod selector_forwarders;
mod surface_requirements;
mod transition_surfaces;

use std::collections::{BTreeMap, BTreeSet};

use anyhow::{Context, Result, ensure};
use serde::Serialize;

use crate::{
    chapter_transition::{ChapterTitlePlan, TransitionTranslationPlans},
    choice_labels::ChoiceLabelPlan,
    front_end_menu::{
        FRONT_END_FONT_STATES, RECORD_ACTION_COMPOSITE_STATE, RECORD_LIST_COMPOSITE_STATE,
        SAVE_SLOT_SELECTION_COMPOSITE_STATE, START_MENU_COMPOSITE_STATE,
    },
    mapper165::font_pair_projection::{TRANSLATED_FE_PAGE_FLAG, mapper_register_from_route},
    rom::Rom,
    semantic_translation::SemanticTranslationPlan,
    text_inventory::FixedTextPlan,
    unit_names::UnitNamePlan,
};

use super::{consumer_catalog::ConsumerCatalogPlan, consumer_codebook::ConsumerCodebookPlan};
pub(super) use dialogue_surfaces::DialogueSurfaceInputs;
use dialogue_surfaces::{DialogueSurfacePlan, plan_dialogue_surfaces};
pub(in crate::full_translation_install) use selector_forwarders::FontPageSelectorForwarderPlan;
use selector_forwarders::plan_font_page_selector_forwarders;
use surface_requirements::{
    ScreenFontSurfaceInputs, ScreenFontSurfacePlan, plan_screen_font_surfaces,
};
use transition_surfaces::{
    TransitionSurfaceInputs, TransitionSurfacePlan, plan_transition_surfaces,
};

pub(super) const FRONT_END_SAVE_SUMMARY_UNIT_SOURCE_INDEX: usize = 0;
pub(super) const FRONT_END_SAVE_SUMMARY_CLASS_SOURCE_INDEX: usize = 20;

pub(in crate::full_translation_install) const MAP_MENU_COMPOSITE_STATE: u8 = 0x03;
pub(in crate::full_translation_install) const UNIT_SUMMARY_COMPOSITE_STATE: u8 = 0x04;
pub(in crate::full_translation_install) const UNIT_COMMAND_COMPOSITE_STATE: u8 = 0x05;
pub(in crate::full_translation_install) const ATTACK_WEAPON_SELECTION_COMPOSITE_STATE: u8 = 0x06;
pub(in crate::full_translation_install) const UNIT_ITEM_LIST_COMPOSITE_STATE: u8 = 0x07;
pub(in crate::full_translation_install) const ITEM_ACTION_COMPOSITE_STATE: u8 = 0x09;
pub(in crate::full_translation_install) const UNIT_STATUS_COMPOSITE_STATE: u8 = 0x0F;
pub(in crate::full_translation_install) const MAP_FUNDS_COMPOSITE_STATE: u8 = 0x13;
pub(in crate::full_translation_install) const MAP_SUMMARY_COMPOSITE_STATE: u8 = 0x14;
pub(in crate::full_translation_install) const CHAPTER_SAVE_OFFER_COMPOSITE_STATE: u8 = 0x1C;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(in crate::full_translation_install) enum ScreenFontPageRole {
    FrontEndMenu,
    FrontEndRecordAction,
    UnitCommand,
    MapMenu,
    ChapterSaveOffer,
    CatalogDefault,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(in crate::full_translation_install) enum ScreenFontResidencyPolicy {
    Static(ScreenFontPageRole),
    UnitOrEnemyNameSelected,
}

/// 번역 글꼴을 쓰는 원본 합성 상태의 전체 정책 집합이다. 고정 표면은 진입 즉시
/// 페이지를 게시하고, 이름에 따라 페이지가 달라지는 요약·상태 화면은 이름 appender가
/// 선택한 카탈로그 페이지를 게시한다.
pub(in crate::full_translation_install) const COMPOSITE_FONT_RESIDENCY_POLICIES: [(
    u8,
    ScreenFontResidencyPolicy,
); 14] = [
    (
        MAP_MENU_COMPOSITE_STATE,
        ScreenFontResidencyPolicy::Static(ScreenFontPageRole::MapMenu),
    ),
    (
        MAP_FUNDS_COMPOSITE_STATE,
        ScreenFontResidencyPolicy::Static(ScreenFontPageRole::MapMenu),
    ),
    (
        MAP_SUMMARY_COMPOSITE_STATE,
        ScreenFontResidencyPolicy::Static(ScreenFontPageRole::MapMenu),
    ),
    (
        UNIT_COMMAND_COMPOSITE_STATE,
        ScreenFontResidencyPolicy::Static(ScreenFontPageRole::UnitCommand),
    ),
    (
        ATTACK_WEAPON_SELECTION_COMPOSITE_STATE,
        ScreenFontResidencyPolicy::Static(ScreenFontPageRole::CatalogDefault),
    ),
    (
        UNIT_ITEM_LIST_COMPOSITE_STATE,
        ScreenFontResidencyPolicy::Static(ScreenFontPageRole::CatalogDefault),
    ),
    (
        ITEM_ACTION_COMPOSITE_STATE,
        ScreenFontResidencyPolicy::Static(ScreenFontPageRole::CatalogDefault),
    ),
    (
        CHAPTER_SAVE_OFFER_COMPOSITE_STATE,
        ScreenFontResidencyPolicy::Static(ScreenFontPageRole::ChapterSaveOffer),
    ),
    (
        START_MENU_COMPOSITE_STATE,
        ScreenFontResidencyPolicy::Static(ScreenFontPageRole::FrontEndMenu),
    ),
    (
        RECORD_LIST_COMPOSITE_STATE,
        ScreenFontResidencyPolicy::Static(ScreenFontPageRole::FrontEndMenu),
    ),
    (
        SAVE_SLOT_SELECTION_COMPOSITE_STATE,
        ScreenFontResidencyPolicy::Static(ScreenFontPageRole::FrontEndMenu),
    ),
    (
        RECORD_ACTION_COMPOSITE_STATE,
        ScreenFontResidencyPolicy::Static(ScreenFontPageRole::FrontEndRecordAction),
    ),
    (
        UNIT_SUMMARY_COMPOSITE_STATE,
        ScreenFontResidencyPolicy::UnitOrEnemyNameSelected,
    ),
    (
        UNIT_STATUS_COMPOSITE_STATE,
        ScreenFontResidencyPolicy::UnitOrEnemyNameSelected,
    ),
];

impl ScreenFontResidencyPolicy {
    pub(in crate::full_translation_install) fn static_page(self) -> Option<ScreenFontPageRole> {
        match self {
            Self::Static(page) => Some(page),
            Self::UnitOrEnemyNameSelected => None,
        }
    }
}

fn validate_composite_state_policies() -> Result<()> {
    let states = COMPOSITE_FONT_RESIDENCY_POLICIES
        .iter()
        .map(|(state, _)| *state)
        .collect::<BTreeSet<_>>();
    ensure!(
        states.len() == COMPOSITE_FONT_RESIDENCY_POLICIES.len(),
        "screen font residency assigns more than one policy to a composite state"
    );
    let dynamic_states = COMPOSITE_FONT_RESIDENCY_POLICIES
        .iter()
        .filter_map(|(state, policy)| {
            (*policy == ScreenFontResidencyPolicy::UnitOrEnemyNameSelected).then_some(*state)
        })
        .collect::<BTreeSet<_>>();
    ensure!(
        dynamic_states
            == BTreeSet::from([UNIT_SUMMARY_COMPOSITE_STATE, UNIT_STATUS_COMPOSITE_STATE]),
        "unit-name-selected screen font residency states changed"
    );
    Ok(())
}

#[derive(Clone, Copy)]
pub(in crate::full_translation_install) struct ScreenFontPageRoutes {
    pub(in crate::full_translation_install) front_end_menu: u8,
    pub(in crate::full_translation_install) front_end_record_action: u8,
    pub(in crate::full_translation_install) unit_command: u8,
    pub(in crate::full_translation_install) map_menu: u8,
    pub(in crate::full_translation_install) ending_record: u8,
    pub(in crate::full_translation_install) chapter_save_offer: u8,
    pub(in crate::full_translation_install) catalog: [u8; 2],
}

impl ScreenFontPageRole {
    pub(in crate::full_translation_install) fn mapper_route(
        self,
        routes: ScreenFontPageRoutes,
    ) -> u8 {
        match self {
            Self::FrontEndMenu => routes.front_end_menu,
            Self::FrontEndRecordAction => routes.front_end_record_action,
            Self::UnitCommand => routes.unit_command,
            Self::MapMenu => routes.map_menu,
            Self::ChapterSaveOffer => routes.chapter_save_offer,
            Self::CatalogDefault => routes.catalog[0],
        }
    }
}

impl ScreenFontPageRoutes {
    fn distinct_pages(self) -> [u8; 7] {
        [
            self.front_end_menu,
            self.unit_command,
            self.map_menu,
            self.ending_record,
            self.chapter_save_offer,
            self.catalog[0],
            self.catalog[1],
        ]
    }

    pub(in crate::full_translation_install) fn validate(self) -> Result<()> {
        let distinct_pages = self.distinct_pages();
        ensure!(
            distinct_pages.iter().all(|route| *route != 0),
            "screen font residency uses the empty sentinel as a mapper route"
        );
        let mapper_registers = distinct_pages.map(mapper_register_from_route);
        ensure!(
            mapper_registers.into_iter().collect::<BTreeSet<_>>().len() == mapper_registers.len(),
            "screen font residency maps two independent roles to the same translated page"
        );
        let record_action_register = mapper_register_from_route(self.front_end_record_action);
        ensure!(
            self.front_end_record_action & TRANSLATED_FE_PAGE_FLAG != 0
                && self
                    .catalog
                    .iter()
                    .any(|route| mapper_register_from_route(*route) == record_action_register),
            "front-end record action does not select both latches of one catalog page"
        );
        ensure!(
            distinct_pages
                .into_iter()
                .chain([self.front_end_record_action])
                .all(|route| {
                    let mapper_register = mapper_register_from_route(route);
                    mapper_register != 0 && mapper_register & 0x03 == 0 && route & !0xFD == 0
                }),
            "screen font residency contains an invalid FD/FE page route"
        );
        Ok(())
    }
}

pub(super) struct ScreenFontResidencyInputs<'a> {
    pub(super) front_end_menu_route: u8,
    pub(super) map_menu_route: u8,
    pub(super) consumer_catalog: &'a ConsumerCatalogPlan,
    pub(super) consumer_codebook: &'a ConsumerCodebookPlan,
    pub(super) chapter_titles: &'a ChapterTitlePlan,
    pub(super) choices: &'a ChoiceLabelPlan,
    pub(super) transitions: &'a TransitionTranslationPlans,
    pub(super) fixed: &'a FixedTextPlan,
    pub(super) unit_names: &'a UnitNamePlan,
    pub(super) unit_ui: &'a SemanticTranslationPlan,
    pub(super) item_actions: &'a SemanticTranslationPlan,
    pub(super) fixed_menu_labels: &'a SemanticTranslationPlan,
    pub(super) installed_front_end_glyph_codes: &'a BTreeMap<char, u8>,
    pub(super) options_glyph_codes: &'a BTreeMap<char, u8>,
}

#[derive(Serialize)]
pub(super) struct ScreenFontResidencyDraft {
    schema: u8,
    strategy: &'static str,
    composite_state_policy_count: usize,
    static_state_route_count: usize,
    unit_or_enemy_name_selected_state_count: usize,
    front_end_composite_state_count: usize,
    front_end_record_action_catalog_page_index: usize,
    front_end_record_action_mapper_route: u8,
    retained_menu_glyph_count: usize,
    retained_summary_glyph_count: usize,
    retained_record_action_glyph_count: usize,
    every_static_state_selects_an_explicit_route: bool,
    record_action_page_contains_every_menu_glyph: bool,
    record_action_page_contains_the_protagonist_name_and_class: bool,
    surface_requirements: ScreenFontSurfacePlan,
    transition_surfaces: TransitionSurfacePlan,
    #[serde(skip)]
    routes: ScreenFontPageRoutes,
    #[serde(skip)]
    record_action_summary_glyph_codes: BTreeMap<char, u8>,
}

impl ScreenFontResidencyDraft {
    pub(super) fn record_action_summary_glyph_codes(&self) -> &BTreeMap<char, u8> {
        &self.record_action_summary_glyph_codes
    }
}

#[derive(Serialize)]
pub(super) struct ScreenFontResidencyPlan {
    #[serde(flatten)]
    draft: ScreenFontResidencyDraft,
    dialogue_surfaces: DialogueSurfacePlan,
    selector_forwarders: FontPageSelectorForwarderPlan,
}

impl ScreenFontResidencyPlan {
    pub(in crate::full_translation_install) fn routes(&self) -> ScreenFontPageRoutes {
        self.draft.routes
    }

    pub(in crate::full_translation_install) fn selector_forwarders(
        &self,
    ) -> &FontPageSelectorForwarderPlan {
        &self.selector_forwarders
    }
}

pub(super) fn plan_screen_font_residency(
    inputs: ScreenFontResidencyInputs<'_>,
) -> Result<ScreenFontResidencyDraft> {
    validate_composite_state_policies()?;
    ensure!(
        !inputs.installed_front_end_glyph_codes.is_empty(),
        "screen font residency has no installed front-end glyphs"
    );
    let unit = inputs
        .unit_names
        .entries
        .get(FRONT_END_SAVE_SUMMARY_UNIT_SOURCE_INDEX)
        .context("front-end record action lost the protagonist unit name")?;
    ensure!(
        unit.table_id == "unit-names"
            && unit.source_index == FRONT_END_SAVE_SUMMARY_UNIT_SOURCE_INDEX,
        "front-end record-action protagonist identity changed"
    );
    let class = inputs
        .fixed
        .entry_for_source_index("class-names", FRONT_END_SAVE_SUMMARY_CLASS_SOURCE_INDEX)
        .context("front-end record action lost the protagonist class name")?;
    let summary_glyphs = unit
        .unique_glyphs()
        .union(&class.unique_glyphs())
        .copied()
        .collect::<BTreeSet<_>>();

    let record_action_page = inputs
        .consumer_catalog
        .page_for_name("unit_names", FRONT_END_SAVE_SUMMARY_UNIT_SOURCE_INDEX)?;
    let record_action_summary_glyph_codes = bind_required_glyph_codes(
        record_action_page.assignments(),
        inputs.installed_front_end_glyph_codes,
        &summary_glyphs,
    )?;
    let front_end_record_action_route = record_action_page.mapper_route() | TRANSLATED_FE_PAGE_FLAG;
    let surface_requirements = plan_screen_font_surfaces(ScreenFontSurfaceInputs {
        consumer_catalog: inputs.consumer_catalog,
        consumer_codebook: inputs.consumer_codebook,
        fixed: inputs.fixed,
        unit_names: inputs.unit_names,
        unit_ui: inputs.unit_ui,
        item_actions: inputs.item_actions,
        fixed_menu_labels: inputs.fixed_menu_labels,
        installed_front_end_glyph_codes: inputs.installed_front_end_glyph_codes,
        options_glyph_codes: inputs.options_glyph_codes,
    })?;
    let transition_surfaces = plan_transition_surfaces(TransitionSurfaceInputs {
        consumer_codebook: inputs.consumer_codebook,
        chapter_titles: inputs.chapter_titles,
        choices: inputs.choices,
        transitions: inputs.transitions,
    })?;
    let routes = ScreenFontPageRoutes {
        front_end_menu: inputs.front_end_menu_route,
        front_end_record_action: front_end_record_action_route,
        unit_command: inputs
            .consumer_codebook
            .mapper_route_for("unit_command_menu")?,
        map_menu: inputs.map_menu_route,
        ending_record: transition_surfaces.ending_record_route(),
        chapter_save_offer: transition_surfaces.chapter_save_route(),
        catalog: inputs.consumer_catalog.mapper_routes()?,
    };
    routes.validate()?;
    let record_action_catalog_page_index = inputs
        .consumer_catalog
        .pages()
        .iter()
        .position(|page| page.mapper_route() == record_action_page.mapper_route())
        .context("front-end record-action catalog page is outside the catalog")?;
    let retained_record_action_glyph_count = inputs
        .installed_front_end_glyph_codes
        .keys()
        .copied()
        .chain(summary_glyphs.iter().copied())
        .collect::<BTreeSet<_>>()
        .len();

    Ok(ScreenFontResidencyDraft {
        schema: 3,
        strategy: "derive every composite-screen font policy from one residency plan; publish static pages at entry, let unit-or-enemy-name appenders select name-dependent pages, and select the protagonist catalog page explicitly for the front-end record-action lifetime",
        composite_state_policy_count: COMPOSITE_FONT_RESIDENCY_POLICIES.len(),
        static_state_route_count: COMPOSITE_FONT_RESIDENCY_POLICIES
            .iter()
            .filter(|(_, policy)| policy.static_page().is_some())
            .count(),
        unit_or_enemy_name_selected_state_count: COMPOSITE_FONT_RESIDENCY_POLICIES
            .iter()
            .filter(|(_, policy)| *policy == ScreenFontResidencyPolicy::UnitOrEnemyNameSelected)
            .count(),
        front_end_composite_state_count: FRONT_END_FONT_STATES.len(),
        front_end_record_action_catalog_page_index: record_action_catalog_page_index,
        front_end_record_action_mapper_route: front_end_record_action_route,
        retained_menu_glyph_count: inputs.installed_front_end_glyph_codes.len(),
        retained_summary_glyph_count: record_action_summary_glyph_codes.len(),
        retained_record_action_glyph_count,
        every_static_state_selects_an_explicit_route: true,
        record_action_page_contains_every_menu_glyph: true,
        record_action_page_contains_the_protagonist_name_and_class: true,
        surface_requirements,
        transition_surfaces,
        routes,
        record_action_summary_glyph_codes,
    })
}

pub(super) fn finalize_screen_font_residency(
    draft: ScreenFontResidencyDraft,
    dialogue_inputs: DialogueSurfaceInputs<'_>,
    candidate: &Rom,
) -> Result<ScreenFontResidencyPlan> {
    let dialogue_surfaces = plan_dialogue_surfaces(dialogue_inputs)?;
    let selector_forwarders = plan_font_page_selector_forwarders(candidate, draft.routes)?;
    Ok(ScreenFontResidencyPlan {
        draft,
        dialogue_surfaces,
        selector_forwarders,
    })
}

fn bind_required_glyph_codes(
    page_assignments: &BTreeMap<char, u8>,
    menu_glyph_codes: &BTreeMap<char, u8>,
    summary_glyphs: &BTreeSet<char>,
) -> Result<BTreeMap<char, u8>> {
    ensure!(
        menu_glyph_codes
            .iter()
            .all(|(glyph, code)| page_assignments.get(glyph) == Some(code)),
        "front-end record-action page changed an installed menu glyph code"
    );
    let summary_glyph_codes = summary_glyphs
        .iter()
        .map(|glyph| {
            Ok((
                *glyph,
                page_assignments.get(glyph).copied().with_context(|| {
                    format!("front-end record-action page lost summary glyph {glyph:?}")
                })?,
            ))
        })
        .collect::<Result<BTreeMap<_, _>>>()?;
    let mut glyph_by_code = BTreeMap::<u8, char>::new();
    for (glyph, code) in menu_glyph_codes.iter().chain(&summary_glyph_codes) {
        if let Some(other) = glyph_by_code.insert(*code, *glyph) {
            ensure!(
                other == *glyph,
                "front-end record-action code {code:02X} means both {other:?} and {glyph:?}"
            );
        }
    }
    Ok(summary_glyph_codes)
}

#[cfg(test)]
mod tests {
    use super::*;

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
    fn every_front_end_state_selects_an_explicit_page() {
        let routes = routes();
        routes.validate().unwrap();
        let actual = COMPOSITE_FONT_RESIDENCY_POLICIES
            .iter()
            .filter(|(state, _)| FRONT_END_FONT_STATES.contains(state))
            .map(|(state, policy)| {
                (
                    *state,
                    policy
                        .static_page()
                        .expect("front-end state must have a static page")
                        .mapper_route(routes),
                )
            })
            .collect::<BTreeMap<_, _>>();

        assert_eq!(
            actual,
            BTreeMap::from([
                (START_MENU_COMPOSITE_STATE, routes.front_end_menu),
                (RECORD_LIST_COMPOSITE_STATE, routes.front_end_menu),
                (SAVE_SLOT_SELECTION_COMPOSITE_STATE, routes.front_end_menu),
                (
                    RECORD_ACTION_COMPOSITE_STATE,
                    routes.front_end_record_action,
                ),
            ])
        );
    }

    #[test]
    fn unit_summary_and_status_select_the_page_published_by_the_name_appender() {
        validate_composite_state_policies().unwrap();
        let dynamic_states = COMPOSITE_FONT_RESIDENCY_POLICIES
            .iter()
            .filter_map(|(state, policy)| {
                (*policy == ScreenFontResidencyPolicy::UnitOrEnemyNameSelected).then_some(*state)
            })
            .collect::<BTreeSet<_>>();

        assert_eq!(
            dynamic_states,
            BTreeSet::from([UNIT_SUMMARY_COMPOSITE_STATE, UNIT_STATUS_COMPOSITE_STATE])
        );
    }

    #[test]
    fn record_action_requires_menu_name_and_class_on_one_page() {
        let page = BTreeMap::from([('기', 0x20), ('마', 0x21), ('로', 0x22)]);
        let menu = BTreeMap::from([('기', 0x20)]);
        let summary = BTreeSet::from(['마', '로']);

        assert_eq!(
            bind_required_glyph_codes(&page, &menu, &summary).unwrap(),
            BTreeMap::from([('마', 0x21), ('로', 0x22)])
        );
    }

    #[test]
    fn missing_or_aliased_record_action_glyph_fails_closed() {
        let menu = BTreeMap::from([('기', 0x20)]);
        assert!(
            bind_required_glyph_codes(
                &BTreeMap::from([('기', 0x20)]),
                &menu,
                &BTreeSet::from(['마']),
            )
            .unwrap_err()
            .to_string()
            .contains("lost summary glyph")
        );
        assert!(
            bind_required_glyph_codes(
                &BTreeMap::from([('기', 0x20), ('마', 0x20)]),
                &menu,
                &BTreeSet::from(['마']),
            )
            .unwrap_err()
            .to_string()
            .contains("means both")
        );
    }

    #[test]
    fn record_action_route_must_be_a_catalog_page_on_both_latches() {
        let mut invalid = routes();
        invalid.front_end_record_action = invalid.catalog[0];
        assert!(
            invalid
                .validate()
                .unwrap_err()
                .to_string()
                .contains("both latches")
        );
        invalid.front_end_record_action = 0xE5;
        assert!(
            invalid
                .validate()
                .unwrap_err()
                .to_string()
                .contains("catalog page")
        );
    }
}
