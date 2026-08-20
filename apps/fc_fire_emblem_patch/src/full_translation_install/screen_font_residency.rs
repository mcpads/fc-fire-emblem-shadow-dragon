//! 동시에 보이는 비대사 표면을 한 CHR 페이지 경로에 결속한다.
//!
//! 문자열 재료와 카탈로그 페이지는 어느 화면에서 쓰이는지 결정하지 않는다. 이 모듈이
//! 원천 합성 상태와 가시 표면을 합쳐 화면별 경로를 고르고, 런타임 코드는 그 결과를
//! 그대로 방출한다. 이전 화면이 남긴 `$07FD`를 암묵적인 입력으로 쓰지 않는다.

mod composite_policies;
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
    fixed_string_consumers::FixedStringConsumerInspection,
    front_end_menu::FRONT_END_FONT_STATES,
    mapper165::font_pair_projection::{TRANSLATED_FE_PAGE_FLAG, mapper_register_from_route},
    mapper165::{
        BoundOptionsCompositeLifetime, OPTIONS_FONT_PAGE_COMPOSITE_STATES,
        bind_options_composite_lifetime,
    },
    rom::Rom,
    semantic_translation::SemanticTranslationPlan,
    text_inventory::FixedTextPlan,
    unit_names::UnitNamePlan,
};

use super::{
    choice_residency::ChoiceResidencyPlan, consumer_catalog::ConsumerCatalogPlan,
    consumer_codebook::ConsumerCodebookPlan,
    storage_residency::STORAGE_DIALOGUE_OVERLAY_COMPOSITE_STATES,
};
use composite_policies::validate_composite_state_policies;
pub(in crate::full_translation_install) use composite_policies::{
    ATTACK_WEAPON_SELECTION_COMPOSITE_STATE, CHAPTER_SAVE_OFFER_COMPOSITE_STATE,
    COMPOSITE_FONT_RESIDENCY_POLICIES, DelegatedFontPageOwner, ITEM_ACTION_COMPOSITE_STATE,
    MAP_FUNDS_COMPOSITE_STATE, MAP_SUMMARY_COMPOSITE_STATE, ScreenFontPageRole,
    ScreenFontResidencyPolicy, UNIT_ITEM_LIST_COMPOSITE_STATE, UNIT_STATUS_COMPOSITE_STATE,
    UNIT_SUMMARY_COMPOSITE_STATE, composite_font_residency_policy,
};
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
    pub(super) source: &'a Rom,
    pub(super) fixed_string_consumers: &'a FixedStringConsumerInspection,
    pub(super) front_end_menu_route: u8,
    pub(super) map_menu_route: u8,
    pub(super) consumer_catalog: &'a ConsumerCatalogPlan,
    pub(super) consumer_codebook: &'a ConsumerCodebookPlan,
    pub(super) chapter_titles: &'a ChapterTitlePlan,
    pub(super) choices: &'a ChoiceLabelPlan,
    pub(super) choice_residency: &'a ChoiceResidencyPlan,
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
    delegated_page_owner_state_count: usize,
    unresolved_page_owner_state_count: usize,
    static_state_route_count: usize,
    unit_or_enemy_name_published_state_count: usize,
    unit_or_enemy_name_retained_state_count: usize,
    completed_dialogue_page_retained_state_count: usize,
    storage_dialogue_or_static_state_count: usize,
    front_end_composite_state_count: usize,
    front_end_record_action_catalog_page_index: usize,
    front_end_record_action_mapper_route: u8,
    retained_menu_glyph_count: usize,
    retained_summary_glyph_count: usize,
    retained_record_action_glyph_count: usize,
    every_static_state_selects_an_explicit_route: bool,
    delegated_page_owners: Vec<DelegatedCompositeStateReport>,
    unresolved_page_owner_states_hex: Vec<String>,
    record_action_page_contains_every_menu_glyph: bool,
    record_action_page_contains_the_protagonist_name_and_class: bool,
    surface_requirements: ScreenFontSurfacePlan,
    transition_surfaces: TransitionSurfacePlan,
    #[serde(skip)]
    routes: ScreenFontPageRoutes,
    #[serde(skip)]
    record_action_summary_glyph_codes: BTreeMap<char, u8>,
}

#[derive(Serialize)]
struct DelegatedCompositeStateReport {
    state_hex: String,
    owner: &'static str,
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
    let options_lifetime: BoundOptionsCompositeLifetime =
        bind_options_composite_lifetime(inputs.source, inputs.fixed_string_consumers)?;
    ensure!(
        options_lifetime.delegated_states() == OPTIONS_FONT_PAGE_COMPOSITE_STATES
            && options_lifetime.result_fixed_string_indices() == [0x2E, 0x2F]
            && options_lifetime.result_producer_count() == 2,
        "options composite lifetime source ownership changed"
    );
    ensure!(
        composite_font_residency_policy(inputs.choice_residency.composite_state())
            == Some(ScreenFontResidencyPolicy::Delegated(
                DelegatedFontPageOwner::ChoiceDialogueResidency,
            )),
        "choice-label residency is not delegated from its source composite state"
    );
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

    let delegated_page_owners = COMPOSITE_FONT_RESIDENCY_POLICIES
        .iter()
        .filter_map(|(state, policy)| match policy {
            ScreenFontResidencyPolicy::Delegated(owner) => Some(DelegatedCompositeStateReport {
                state_hex: format!("0x{state:02X}"),
                owner: owner.id(),
            }),
            _ => None,
        })
        .collect::<Vec<_>>();
    let unresolved_page_owner_states_hex = COMPOSITE_FONT_RESIDENCY_POLICIES
        .iter()
        .filter(|(_, policy)| *policy == ScreenFontResidencyPolicy::UnresolvedPageOwner)
        .map(|(state, _)| format!("0x{state:02X}"))
        .collect::<Vec<_>>();

    Ok(ScreenFontResidencyDraft {
        schema: 10,
        strategy: "bind every source-produced composite state to one explicit central residency action, a named upstream selector or appender, or an unresolved page owner; publish static pages at entry, retain the source-bound storage dialogue page when its item-list producer reuses state 07, let the unit-summary appender publish its name-dependent page, retain that page explicitly for unit status, delegate record-metadata composites 12/1F/20 to the active main-dialogue runtime selector, retain the options selector across the source-bound 1B-to-19 value-result overlay, and keep every state without an admitted owner visible instead of treating no central write as proof of safe inheritance",
        composite_state_policy_count: COMPOSITE_FONT_RESIDENCY_POLICIES.len(),
        delegated_page_owner_state_count: delegated_page_owners.len(),
        unresolved_page_owner_state_count: unresolved_page_owner_states_hex.len(),
        static_state_route_count: COMPOSITE_FONT_RESIDENCY_POLICIES
            .iter()
            .filter(|(_, policy)| policy.static_page().is_some())
            .count(),
        unit_or_enemy_name_published_state_count: COMPOSITE_FONT_RESIDENCY_POLICIES
            .iter()
            .filter(|(_, policy)| {
                *policy == ScreenFontResidencyPolicy::UnitOrEnemyNamePublishedByAppender
            })
            .count(),
        unit_or_enemy_name_retained_state_count: COMPOSITE_FONT_RESIDENCY_POLICIES
            .iter()
            .filter(|(_, policy)| {
                *policy == ScreenFontResidencyPolicy::UnitOrEnemyNameRetainedFromSummary
            })
            .count(),
        completed_dialogue_page_retained_state_count: COMPOSITE_FONT_RESIDENCY_POLICIES
            .iter()
            .filter(|(_, policy)| {
                *policy == ScreenFontResidencyPolicy::CompletedDialoguePageRetained
            })
            .count(),
        storage_dialogue_or_static_state_count: COMPOSITE_FONT_RESIDENCY_POLICIES
            .iter()
            .filter(|(_, policy)| {
                matches!(
                    policy,
                    ScreenFontResidencyPolicy::StorageDialogueOrStatic(_)
                )
            })
            .count(),
        front_end_composite_state_count: FRONT_END_FONT_STATES.len(),
        front_end_record_action_catalog_page_index: record_action_catalog_page_index,
        front_end_record_action_mapper_route: front_end_record_action_route,
        retained_menu_glyph_count: inputs.installed_front_end_glyph_codes.len(),
        retained_summary_glyph_count: record_action_summary_glyph_codes.len(),
        retained_record_action_glyph_count,
        every_static_state_selects_an_explicit_route: true,
        delegated_page_owners,
        unresolved_page_owner_states_hex,
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
    use crate::{
        choice_labels::CHOICE_LABEL_COMPOSITE_STATE,
        dialogue_inventory::MAIN_DIALOGUE_COMPOSITE_APPENDER_STATES,
        fixed_menu_labels::{
            GAME_SPEED_SELECTION_COMPOSITE_STATE, STORAGE_CAPACITY_NOTICE_COMPOSITE_STATE,
            UNIT_SELECTION_COMPOSITE_STATE,
        },
        front_end_menu::{
            RECORD_ACTION_COMPOSITE_STATE, RECORD_LIST_COMPOSITE_STATE,
            SAVE_SLOT_SELECTION_COMPOSITE_STATE, START_MENU_COMPOSITE_STATE,
        },
        mapper165::{OPTIONS_FONT_PAGE_COMPOSITE_STATES, ROSTER_FONT_PAGE_COMPOSITE_STATE},
    };

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
    fn unit_summary_publishes_and_unit_status_retains_one_name_page() {
        validate_composite_state_policies().unwrap();
        assert_eq!(
            COMPOSITE_FONT_RESIDENCY_POLICIES
                .iter()
                .find(|(state, _)| *state == UNIT_SUMMARY_COMPOSITE_STATE)
                .map(|(_, policy)| *policy),
            Some(ScreenFontResidencyPolicy::UnitOrEnemyNamePublishedByAppender)
        );
        assert_eq!(
            COMPOSITE_FONT_RESIDENCY_POLICIES
                .iter()
                .find(|(state, _)| *state == UNIT_STATUS_COMPOSITE_STATE)
                .map(|(_, policy)| *policy),
            Some(ScreenFontResidencyPolicy::UnitOrEnemyNameRetainedFromSummary)
        );
    }

    #[test]
    fn named_upstream_owners_are_distinct_from_unresolved_page_inheritance() {
        validate_composite_state_policies().unwrap();
        let delegated = COMPOSITE_FONT_RESIDENCY_POLICIES
            .iter()
            .filter_map(|(state, policy)| match policy {
                ScreenFontResidencyPolicy::Delegated(owner) => Some((*state, *owner)),
                _ => None,
            })
            .collect::<BTreeMap<_, _>>();

        assert_eq!(
            delegated,
            BTreeMap::from([
                (
                    CHOICE_LABEL_COMPOSITE_STATE,
                    DelegatedFontPageOwner::ChoiceDialogueResidency,
                ),
                (
                    MAIN_DIALOGUE_COMPOSITE_APPENDER_STATES[0],
                    DelegatedFontPageOwner::MainDialogueRuntimeSelector,
                ),
                (
                    ROSTER_FONT_PAGE_COMPOSITE_STATE,
                    DelegatedFontPageOwner::UnitRosterSelector,
                ),
                (
                    UNIT_SELECTION_COMPOSITE_STATE,
                    DelegatedFontPageOwner::UnitSelectionAppender,
                ),
                (
                    GAME_SPEED_SELECTION_COMPOSITE_STATE,
                    DelegatedFontPageOwner::GameSpeedAppender,
                ),
                (
                    OPTIONS_FONT_PAGE_COMPOSITE_STATES[0],
                    DelegatedFontPageOwner::OptionsSelector,
                ),
                (
                    MAIN_DIALOGUE_COMPOSITE_APPENDER_STATES[1],
                    DelegatedFontPageOwner::MainDialogueRuntimeSelector,
                ),
                (
                    OPTIONS_FONT_PAGE_COMPOSITE_STATES[1],
                    DelegatedFontPageOwner::OptionsSelector,
                ),
                (
                    MAIN_DIALOGUE_COMPOSITE_APPENDER_STATES[2],
                    DelegatedFontPageOwner::MainDialogueRuntimeSelector,
                ),
                (
                    STORAGE_CAPACITY_NOTICE_COMPOSITE_STATE,
                    DelegatedFontPageOwner::StorageCapacityAppender,
                ),
            ])
        );
        assert_eq!(
            COMPOSITE_FONT_RESIDENCY_POLICIES
                .iter()
                .filter(|(_, policy)| *policy == ScreenFontResidencyPolicy::UnresolvedPageOwner)
                .count(),
            10
        );
    }

    #[test]
    fn storage_overlays_retain_only_the_source_bound_completed_dialogue_states() {
        validate_composite_state_policies().unwrap();
        let actual = COMPOSITE_FONT_RESIDENCY_POLICIES
            .iter()
            .filter_map(|(state, policy)| {
                (*policy == ScreenFontResidencyPolicy::CompletedDialoguePageRetained)
                    .then_some(*state)
            })
            .collect::<Vec<_>>();

        assert_eq!(actual, STORAGE_DIALOGUE_OVERLAY_COMPOSITE_STATES);
    }

    #[test]
    fn shared_item_list_state_declares_its_storage_dialogue_override() {
        validate_composite_state_policies().unwrap();
        assert_eq!(
            COMPOSITE_FONT_RESIDENCY_POLICIES
                .iter()
                .find(|(state, _)| *state == UNIT_ITEM_LIST_COMPOSITE_STATE)
                .map(|(_, policy)| *policy),
            Some(ScreenFontResidencyPolicy::StorageDialogueOrStatic(
                ScreenFontPageRole::CatalogDefault,
            ))
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
