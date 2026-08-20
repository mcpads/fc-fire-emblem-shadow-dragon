//! 화면 상태가 동시에 요구하는 문자열 표면을 실제 글꼴 페이지에 결속한다.

use std::collections::{BTreeMap, BTreeSet};

use anyhow::{Context, Result, ensure};
use serde::Serialize;

use crate::{
    front_end_menu::RECORD_ACTION_COMPOSITE_STATE,
    semantic_translation::SemanticTranslationPlan,
    text_inventory::{FixedTextPlan, FixedTextPlannedEntry},
    unit_names::UnitNamePlan,
    unit_ui_text::{command_menu_label_ids, summary_and_status_label_ids},
};

use super::{
    ATTACK_WEAPON_SELECTION_COMPOSITE_STATE, CLASS_NAME_ONLY_COMPOSITE_STATE,
    COMPOSITE_FONT_RESIDENCY_POLICIES, ITEM_ACTION_COMPOSITE_STATE,
    ITEM_NAME_APPENDER_PUBLISHED_COMPOSITE_STATES, ITEM_USE_RESULT_COMPOSITE_STATE,
    STORAGE_ITEM_DETAIL_COMPOSITE_STATE, ScreenFontPageRole, ScreenFontResidencyPolicy,
    UNIT_ACTION_ITEM_COMPOSITE_STATE, UNIT_ITEM_LIST_COMPOSITE_STATE,
    UNIT_NAME_DETAIL_COMPOSITE_STATE, UNIT_STATUS_COMPOSITE_STATE, UNIT_SUMMARY_COMPOSITE_STATE,
};
use crate::full_translation_install::{
    consumer_catalog::{ConsumerCatalogPage, ConsumerCatalogPlan},
    consumer_codebook::ConsumerCodebookPlan,
};

const ITEM_NAME_COUNT: usize = 91;
const CLASS_NAME_COUNT: usize = 22;
const ENEMY_NAME_COUNT: usize = 69;
#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
enum CatalogSurface {
    ItemNames,
    ClassNames,
    SummaryAndStatusLabels,
    ItemActionLabels,
    FrontEndMenu,
}

impl CatalogSurface {
    const fn id(self) -> &'static str {
        match self {
            Self::ItemNames => "item_names",
            Self::ClassNames => "class_names",
            Self::SummaryAndStatusLabels => "summary_and_status_labels",
            Self::ItemActionLabels => "item_action_labels",
            Self::FrontEndMenu => "front_end_menu",
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum CatalogPageSelection {
    DefaultCatalog,
    SelectedUnitOrEnemy,
    ProtagonistRecordAction,
}

impl CatalogPageSelection {
    const fn id(self) -> &'static str {
        match self {
            Self::DefaultCatalog => "default_catalog_page",
            Self::SelectedUnitOrEnemy => "selected_unit_or_enemy_page",
            Self::ProtagonistRecordAction => "protagonist_record_action_page",
        }
    }
}

struct CatalogScreenRequirement {
    state: u8,
    screen_role: &'static str,
    page_selection: CatalogPageSelection,
    surfaces: &'static [CatalogSurface],
}

const CATALOG_SCREEN_REQUIREMENTS: &[CatalogScreenRequirement] = &[
    CatalogScreenRequirement {
        state: UNIT_NAME_DETAIL_COMPOSITE_STATE,
        screen_role: "unit_name_detail",
        page_selection: CatalogPageSelection::SelectedUnitOrEnemy,
        surfaces: &[CatalogSurface::SummaryAndStatusLabels],
    },
    CatalogScreenRequirement {
        state: UNIT_SUMMARY_COMPOSITE_STATE,
        screen_role: "unit_summary",
        page_selection: CatalogPageSelection::SelectedUnitOrEnemy,
        surfaces: &[
            CatalogSurface::ItemNames,
            CatalogSurface::ClassNames,
            CatalogSurface::SummaryAndStatusLabels,
        ],
    },
    CatalogScreenRequirement {
        state: ATTACK_WEAPON_SELECTION_COMPOSITE_STATE,
        screen_role: "attack_weapon_selection",
        page_selection: CatalogPageSelection::DefaultCatalog,
        surfaces: &[CatalogSurface::ItemNames],
    },
    CatalogScreenRequirement {
        state: UNIT_ITEM_LIST_COMPOSITE_STATE,
        screen_role: "item_inventory_list",
        page_selection: CatalogPageSelection::DefaultCatalog,
        surfaces: &[CatalogSurface::ItemNames],
    },
    CatalogScreenRequirement {
        state: ITEM_ACTION_COMPOSITE_STATE,
        screen_role: "item_action_menu",
        page_selection: CatalogPageSelection::DefaultCatalog,
        surfaces: &[CatalogSurface::ItemNames, CatalogSurface::ItemActionLabels],
    },
    CatalogScreenRequirement {
        state: UNIT_ACTION_ITEM_COMPOSITE_STATE,
        screen_role: "unit_action_item_names",
        page_selection: CatalogPageSelection::DefaultCatalog,
        surfaces: &[CatalogSurface::ItemNames],
    },
    CatalogScreenRequirement {
        state: CLASS_NAME_ONLY_COMPOSITE_STATE,
        screen_role: "class_name_only",
        page_selection: CatalogPageSelection::DefaultCatalog,
        surfaces: &[CatalogSurface::ClassNames],
    },
    CatalogScreenRequirement {
        state: ITEM_USE_RESULT_COMPOSITE_STATE,
        screen_role: "item_use_result_item_name",
        page_selection: CatalogPageSelection::DefaultCatalog,
        surfaces: &[CatalogSurface::ItemNames],
    },
    CatalogScreenRequirement {
        state: STORAGE_ITEM_DETAIL_COMPOSITE_STATE,
        screen_role: "storage_item_detail_name",
        page_selection: CatalogPageSelection::DefaultCatalog,
        surfaces: &[CatalogSurface::ItemNames],
    },
    CatalogScreenRequirement {
        state: UNIT_STATUS_COMPOSITE_STATE,
        screen_role: "unit_status",
        page_selection: CatalogPageSelection::SelectedUnitOrEnemy,
        surfaces: &[
            CatalogSurface::ClassNames,
            CatalogSurface::SummaryAndStatusLabels,
        ],
    },
    CatalogScreenRequirement {
        state: RECORD_ACTION_COMPOSITE_STATE,
        screen_role: "front_end_record_action",
        page_selection: CatalogPageSelection::ProtagonistRecordAction,
        surfaces: &[CatalogSurface::ClassNames, CatalogSurface::FrontEndMenu],
    },
];

pub(super) struct ScreenFontSurfaceInputs<'a> {
    pub(super) consumer_catalog: &'a ConsumerCatalogPlan,
    pub(super) consumer_codebook: &'a ConsumerCodebookPlan,
    pub(super) fixed: &'a FixedTextPlan,
    pub(super) unit_names: &'a UnitNamePlan,
    pub(super) unit_ui: &'a SemanticTranslationPlan,
    pub(super) item_actions: &'a SemanticTranslationPlan,
    pub(super) fixed_menu_labels: &'a SemanticTranslationPlan,
    pub(super) installed_front_end_glyph_codes: &'a BTreeMap<char, u8>,
    pub(super) options_glyph_codes: &'a BTreeMap<char, u8>,
}

#[derive(Serialize)]
pub(super) struct ScreenFontSurfacePlan {
    schema: u8,
    strategy: &'static str,
    catalog_screen_requirement_count: usize,
    shared_catalog_surface_count: usize,
    playable_name_identity_count: usize,
    enemy_name_identity_count: usize,
    unit_command_required_glyph_count: usize,
    requirements: Vec<CatalogScreenRequirementReport>,
    every_requirement_matches_the_state_policy: bool,
    every_selected_catalog_page_contains_its_shared_surfaces: bool,
    every_name_identity_selects_a_page_containing_its_name: bool,
    unit_command_page_contains_commands_fixed_menus_and_options_parent: bool,
}

#[derive(Serialize)]
struct CatalogScreenRequirementReport {
    state_hex: String,
    screen_role: &'static str,
    page_selection: &'static str,
    required_surfaces: Vec<&'static str>,
    required_glyph_count: usize,
    checked_page_count: usize,
}

pub(super) fn plan_screen_font_surfaces(
    inputs: ScreenFontSurfaceInputs<'_>,
) -> Result<ScreenFontSurfacePlan> {
    validate_requirement_policies()?;
    let item_names = fixed_table_entries(inputs.fixed, "item-names", ITEM_NAME_COUNT)?;
    let class_names = fixed_table_entries(inputs.fixed, "class-names", CLASS_NAME_COUNT)?;
    let enemy_names = fixed_table_entries(inputs.fixed, "enemy-names", ENEMY_NAME_COUNT)?;

    let surface_glyphs = BTreeMap::from([
        (
            CatalogSurface::ItemNames,
            entry_glyphs(item_names.iter().copied()),
        ),
        (
            CatalogSurface::ClassNames,
            entry_glyphs(class_names.iter().copied()),
        ),
        (
            CatalogSurface::SummaryAndStatusLabels,
            semantic_entry_glyphs(inputs.unit_ui, &summary_and_status_label_ids())?,
        ),
        (
            CatalogSurface::ItemActionLabels,
            inputs.item_actions.unique_target_glyphs(),
        ),
        (
            CatalogSurface::FrontEndMenu,
            inputs
                .installed_front_end_glyph_codes
                .keys()
                .copied()
                .collect(),
        ),
    ]);
    ensure!(
        surface_glyphs.values().all(|glyphs| !glyphs.is_empty()),
        "screen font residency contains an empty shared catalog surface"
    );

    let mut requirements = Vec::with_capacity(CATALOG_SCREEN_REQUIREMENTS.len());
    for requirement in CATALOG_SCREEN_REQUIREMENTS {
        let required_glyphs = requirement
            .surfaces
            .iter()
            .flat_map(|surface| &surface_glyphs[surface])
            .copied()
            .collect::<BTreeSet<_>>();
        let pages = selected_pages(requirement.page_selection, &inputs)?;
        for page in &pages {
            validate_page_assignments(
                page.assignments(),
                inputs.consumer_catalog.base_assignments(),
                &required_glyphs,
                requirement.screen_role,
            )?;
        }
        requirements.push(CatalogScreenRequirementReport {
            state_hex: format!("0x{:02X}", requirement.state),
            screen_role: requirement.screen_role,
            page_selection: requirement.page_selection.id(),
            required_surfaces: requirement
                .surfaces
                .iter()
                .map(|surface| surface.id())
                .collect(),
            required_glyph_count: required_glyphs.len(),
            checked_page_count: pages.len(),
        });
    }

    validate_name_pages(
        inputs.consumer_catalog,
        "unit_names",
        inputs.unit_names.entries.iter(),
    )?;
    validate_name_pages(
        inputs.consumer_catalog,
        "enemy_names",
        enemy_names.iter().copied(),
    )?;

    let command_glyphs = semantic_entry_glyphs(inputs.unit_ui, &command_menu_label_ids())?;
    let unit_command_glyphs = command_glyphs
        .union(&inputs.fixed_menu_labels.unique_target_glyphs())
        .copied()
        .collect::<BTreeSet<_>>();
    inputs
        .consumer_codebook
        .validate_unit_command_residency(&unit_command_glyphs, inputs.options_glyph_codes)?;

    Ok(ScreenFontSurfacePlan {
        schema: 1,
        strategy: "bind every simultaneous unit, item, and fixed-menu surface to the page selected by the central screen residency policy",
        catalog_screen_requirement_count: requirements.len(),
        shared_catalog_surface_count: surface_glyphs.len(),
        playable_name_identity_count: inputs.unit_names.entries.len(),
        enemy_name_identity_count: enemy_names.len(),
        unit_command_required_glyph_count: unit_command_glyphs.len(),
        requirements,
        every_requirement_matches_the_state_policy: true,
        every_selected_catalog_page_contains_its_shared_surfaces: true,
        every_name_identity_selects_a_page_containing_its_name: true,
        unit_command_page_contains_commands_fixed_menus_and_options_parent: true,
    })
}

fn validate_requirement_policies() -> Result<()> {
    for requirement in CATALOG_SCREEN_REQUIREMENTS {
        let policy = COMPOSITE_FONT_RESIDENCY_POLICIES
            .iter()
            .find_map(|(state, policy)| (*state == requirement.state).then_some(*policy))
            .with_context(|| {
                format!(
                    "catalog screen {} has no font residency policy",
                    requirement.screen_role
                )
            })?;
        let expected = match requirement.page_selection {
            CatalogPageSelection::DefaultCatalog
                if requirement.state == UNIT_ITEM_LIST_COMPOSITE_STATE =>
            {
                ScreenFontResidencyPolicy::StorageDialogueOrStatic(
                    ScreenFontPageRole::CatalogDefault,
                )
            }
            CatalogPageSelection::DefaultCatalog
                if ITEM_NAME_APPENDER_PUBLISHED_COMPOSITE_STATES.contains(&requirement.state) =>
            {
                ScreenFontResidencyPolicy::ItemNamePublishedByAppender
            }
            CatalogPageSelection::DefaultCatalog
                if requirement.state == CLASS_NAME_ONLY_COMPOSITE_STATE =>
            {
                ScreenFontResidencyPolicy::ClassNamePublishedByAppender
            }
            CatalogPageSelection::DefaultCatalog => {
                ScreenFontResidencyPolicy::Static(ScreenFontPageRole::CatalogDefault)
            }
            CatalogPageSelection::SelectedUnitOrEnemy => match requirement.state {
                UNIT_NAME_DETAIL_COMPOSITE_STATE | UNIT_SUMMARY_COMPOSITE_STATE => {
                    ScreenFontResidencyPolicy::UnitOrEnemyNamePublishedByAppender
                }
                UNIT_STATUS_COMPOSITE_STATE => {
                    ScreenFontResidencyPolicy::UnitOrEnemyNameRetainedFromSummary
                }
                state => anyhow::bail!(
                    "catalog screen {} uses selected unit-or-enemy page in unsupported state {state:02X}",
                    requirement.screen_role
                ),
            },
            CatalogPageSelection::ProtagonistRecordAction => {
                ScreenFontResidencyPolicy::Static(ScreenFontPageRole::FrontEndRecordAction)
            }
        };
        ensure!(
            policy == expected,
            "catalog screen {} page selection disagrees with its residency policy",
            requirement.screen_role
        );
    }
    Ok(())
}

fn selected_pages<'a>(
    selection: CatalogPageSelection,
    inputs: &'a ScreenFontSurfaceInputs<'_>,
) -> Result<Vec<&'a ConsumerCatalogPage>> {
    match selection {
        CatalogPageSelection::DefaultCatalog => Ok(vec![
            inputs
                .consumer_catalog
                .pages()
                .first()
                .context("consumer catalog has no default page")?,
        ]),
        CatalogPageSelection::SelectedUnitOrEnemy => {
            ensure!(
                !inputs.consumer_catalog.pages().is_empty(),
                "consumer catalog has no selectable name pages"
            );
            Ok(inputs.consumer_catalog.pages().iter().collect())
        }
        CatalogPageSelection::ProtagonistRecordAction => Ok(vec![
            inputs.consumer_catalog.page_for_name("unit_names", 0)?,
        ]),
    }
}

fn fixed_table_entries<'a>(
    fixed: &'a FixedTextPlan,
    table_id: &str,
    expected_count: usize,
) -> Result<Vec<&'a FixedTextPlannedEntry>> {
    let entries = fixed
        .entries
        .iter()
        .filter(|entry| entry.table_id == table_id)
        .collect::<Vec<_>>();
    ensure!(
        entries.len() == expected_count,
        "screen font residency {table_id} population changed"
    );
    Ok(entries)
}

fn entry_glyphs<'a>(entries: impl Iterator<Item = &'a FixedTextPlannedEntry>) -> BTreeSet<char> {
    entries
        .flat_map(FixedTextPlannedEntry::unique_glyphs)
        .collect()
}

fn semantic_entry_glyphs(plan: &SemanticTranslationPlan, ids: &[String]) -> Result<BTreeSet<char>> {
    ids.iter()
        .map(|id| {
            plan.entry_target_glyphs(id)
                .with_context(|| format!("screen font residency lost semantic entry {id}"))
        })
        .collect::<Result<Vec<_>>>()
        .map(|entries| entries.into_iter().flatten().copied().collect())
}

fn validate_page_assignments(
    page: &BTreeMap<char, u8>,
    base: &BTreeMap<char, u8>,
    required: &BTreeSet<char>,
    screen_role: &str,
) -> Result<()> {
    ensure!(
        required.iter().all(|glyph| base
            .get(glyph)
            .is_some_and(|code| page.get(glyph) == Some(code))),
        "screen font page lost or re-encoded a required {screen_role} glyph"
    );
    Ok(())
}

fn validate_name_pages<'a>(
    catalog: &ConsumerCatalogPlan,
    domain: &'static str,
    entries: impl Iterator<Item = &'a FixedTextPlannedEntry>,
) -> Result<()> {
    for entry in entries {
        let page = catalog.page_for_name(domain, entry.source_index)?;
        ensure!(
            entry
                .unique_glyphs()
                .iter()
                .all(|glyph| page.assignments().contains_key(glyph)),
            "catalog page for {domain} index {} lost a selected-name glyph",
            entry.source_index
        );
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn catalog_screen_requirements_match_the_central_state_policies() {
        validate_requirement_policies().unwrap();
        let published_item_states = CATALOG_SCREEN_REQUIREMENTS
            .iter()
            .filter_map(|requirement| {
                (requirement.page_selection == CatalogPageSelection::DefaultCatalog
                    && requirement.surfaces == [CatalogSurface::ItemNames]
                    && ITEM_NAME_APPENDER_PUBLISHED_COMPOSITE_STATES.contains(&requirement.state))
                .then_some(requirement.state)
            })
            .collect::<Vec<_>>();
        assert_eq!(
            published_item_states,
            ITEM_NAME_APPENDER_PUBLISHED_COMPOSITE_STATES
        );
        assert!(CATALOG_SCREEN_REQUIREMENTS.iter().any(|requirement| {
            requirement.state == UNIT_NAME_DETAIL_COMPOSITE_STATE
                && requirement.page_selection == CatalogPageSelection::SelectedUnitOrEnemy
        }));
        assert!(CATALOG_SCREEN_REQUIREMENTS.iter().any(|requirement| {
            requirement.state == CLASS_NAME_ONLY_COMPOSITE_STATE
                && requirement.page_selection == CatalogPageSelection::DefaultCatalog
                && requirement.surfaces == [CatalogSurface::ClassNames]
        }));
    }

    #[test]
    fn required_surface_must_keep_the_base_code_on_the_selected_page() {
        let base = BTreeMap::from([('가', 0x20), ('나', 0x21)]);
        let required = BTreeSet::from(['가', '나']);
        validate_page_assignments(&base, &base, &required, "fixture").unwrap();

        let changed = BTreeMap::from([('가', 0x20), ('나', 0x22)]);
        assert!(
            validate_page_assignments(&changed, &base, &required, "fixture")
                .unwrap_err()
                .to_string()
                .contains("lost or re-encoded")
        );
    }

    #[test]
    fn missing_required_surface_glyph_fails_closed() {
        let base = BTreeMap::from([('가', 0x20), ('나', 0x21)]);
        let page = BTreeMap::from([('가', 0x20)]);
        assert!(
            validate_page_assignments(&page, &base, &BTreeSet::from(['가', '나']), "fixture")
                .is_err()
        );
    }
}
