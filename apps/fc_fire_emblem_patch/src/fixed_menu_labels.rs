//! 직접 생산되는 bank 0B 고정 메뉴 중 아직 일본어인 일곱 라벨을 소유한다.
//!
//! 고정 문자열 표 전체 census는 `fixed_string_consumers`가 담당한다. 이 모듈은 그
//! 분모에서 번역 대상 일곱 레코드와 다섯 화면의 handler, appender 호출, 상태
//! 생산자를 함께 고정하고 번역 작업공간을 계획한다.

use std::{collections::BTreeSet, path::Path};

use anyhow::{Context, Result, ensure};

use crate::{
    fixed_string_consumers::{
        FixedStringConsumerInspection, bind_direct_composite_state_producer_catalog,
        inspect_fixed_string_consumers,
    },
    rom::Rom,
    semantic_translation::{
        ExpectedSemanticEntry, SemanticTranslationPlan, plan_semantic_translation,
    },
    translation_consumer::{
        ScreenConsumerSourceBinding, TranslationConsumerSourceEvidence,
        qualified_source_binding_id, source_binding_id,
    },
};

const FIXED_STRING_BANK: usize = 0x0B;
const FIXED_STRING_POINTER_TABLE: u16 = 0x8FC2;

pub(crate) const UNIT_SELECTION_COMPOSITE_STATE: u8 = 0x18;
pub(crate) const GAME_SPEED_SELECTION_COMPOSITE_STATE: u8 = 0x1A;
pub(crate) const STORAGE_CAPACITY_NOTICE_COMPOSITE_STATE: u8 = 0x26;
pub(crate) const STATIC_FONT_PAGE_APPENDER_COMPOSITE_STATES: [u8; 3] = [
    UNIT_SELECTION_COMPOSITE_STATE,
    GAME_SPEED_SELECTION_COMPOSITE_STATE,
    STORAGE_CAPACITY_NOTICE_COMPOSITE_STATE,
];

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct FixedMenuLabelSpec {
    pub(crate) index: u8,
    pub(crate) pointer: u16,
    pub(crate) expected: &'static [u8],
    pub(crate) japanese_markup: &'static str,
    pub(crate) max_visible_cells: usize,
    pub(crate) terminator: u8,
}

pub(crate) const FIXED_MENU_LABEL_SPECS: [FixedMenuLabelSpec; 7] = [
    label(
        0x2C,
        0x9188,
        &[
            0x56, 0x46, 0x89, 0x44, 0x3D, 0x5B, 0x37, 0x44, 0xFF, 0x19, 0x09, 0x29, 0xED,
        ],
        "ユニットセレクト{FF}のこり",
        12,
    ),
    label(0x30, 0x91A2, &[0x1A, 0x25, 0x01, 0xEF], "はやい", 3),
    label(0x31, 0x91A6, &[0x04, 0x0E, 0x01, 0xEF], "おそい", 3),
    label(
        0x35,
        0x91C5,
        &[0x00, 0x0C, 0x0F, 0x08, 0x2A, 0xED],
        "あずける",
        5,
    ),
    label(
        0x36,
        0x91CB,
        &[0x1B, 0x06, 0x10, 0x0F, 0x0C, 0xED],
        "ひきだす",
        5,
    ),
    label(
        0x46,
        0x923F,
        &[0x15, 0x16, 0x05, 0x0C, 0x13, 0x2A, 0xED],
        "なにかすてる",
        6,
    ),
    label(
        0x47,
        0x9246,
        &[
            0x09, 0xFF, 0x00, 0x0C, 0x0F, 0x08, 0x28, 0x2B, 0x20, 0x0C, 0xED,
        ],
        "こ{FF}あずけられます",
        10,
    ),
];

pub(crate) fn translated_fixed_string_indices() -> BTreeSet<u8> {
    FIXED_MENU_LABEL_SPECS
        .iter()
        .map(|spec| spec.index)
        .collect()
}

const fn label(
    index: u8,
    pointer: u16,
    expected: &'static [u8],
    japanese_markup: &'static str,
    max_visible_cells: usize,
) -> FixedMenuLabelSpec {
    FixedMenuLabelSpec {
        index,
        pointer,
        expected,
        japanese_markup,
        max_visible_cells,
        terminator: expected[expected.len() - 1],
    }
}

#[derive(Clone, Copy)]
struct ScreenRoute {
    screen_role: &'static str,
    composite_state: u8,
    handler: u16,
    producer_bank: usize,
    producer: u16,
    label_indices: &'static [u8],
}

const SCREEN_ROUTES: [ScreenRoute; 5] = [
    route(
        "unit_selection",
        UNIT_SELECTION_COMPOSITE_STATE,
        0x8A25,
        0x06,
        0x86B8,
        &[0x2C],
    ),
    route(
        "game_speed_selection",
        GAME_SPEED_SELECTION_COMPOSITE_STATE,
        0x8A47,
        0x06,
        0xB3BC,
        &[0x30, 0x31],
    ),
    route(
        "storage_action_menu",
        0x1D,
        0x8B08,
        0x06,
        0x9E12,
        &[0x35, 0x36],
    ),
    route(
        "storage_overflow_action",
        0x23,
        0x8D98,
        0x06,
        0xB17F,
        &[0x35, 0x46],
    ),
    route(
        "storage_capacity_notice",
        STORAGE_CAPACITY_NOTICE_COMPOSITE_STATE,
        0x8E0F,
        0x06,
        0xA743,
        &[0x47],
    ),
];

const fn route(
    screen_role: &'static str,
    composite_state: u8,
    handler: u16,
    producer_bank: usize,
    producer: u16,
    label_indices: &'static [u8],
) -> ScreenRoute {
    ScreenRoute {
        screen_role,
        composite_state,
        handler,
        producer_bank,
        producer,
        label_indices,
    }
}

pub(crate) fn plan_fixed_menu_labels(
    rom: &Rom,
    workspace_path: &Path,
) -> Result<SemanticTranslationPlan> {
    bind_source(rom)?;
    let expected = FIXED_MENU_LABEL_SPECS
        .iter()
        .map(|spec| ExpectedSemanticEntry {
            id: population_id(spec.index),
            japanese_markup: spec.japanese_markup.to_owned(),
            max_visible_cells: spec.max_visible_cells,
        })
        .collect::<Vec<_>>();
    plan_semantic_translation(workspace_path, &expected)
}

pub(crate) fn inspect_fixed_menu_translation_consumers(
    rom: &Rom,
) -> Result<TranslationConsumerSourceEvidence> {
    let inspection = bind_source(rom)?;
    let population_ids = FIXED_MENU_LABEL_SPECS
        .iter()
        .map(|spec| population_id(spec.index))
        .collect::<Vec<_>>();
    let screen_bindings = SCREEN_ROUTES
        .iter()
        .map(|route| {
            let call_sites = inspection
                .call_sites
                .iter()
                .filter(|call| {
                    call.composite_state == route.composite_state
                        && call
                            .possible_indices
                            .iter()
                            .any(|index| route.label_indices.contains(index))
                })
                .collect::<Vec<_>>();
            ensure!(
                !call_sites.is_empty(),
                "fixed-menu screen {} has no appender call",
                route.screen_role
            );
            let mut source_binding_ids = vec![
                qualified_source_binding_id(
                    FIXED_STRING_BANK,
                    0x8006 + u16::from(route.composite_state) * 2,
                    "composite_handler_pointer",
                    &format!("handler={:04X}", route.handler),
                ),
                source_binding_id(
                    FIXED_STRING_BANK,
                    route.handler,
                    "compose_fixed_menu_screen",
                ),
                qualified_source_binding_id(
                    route.producer_bank,
                    route.producer,
                    "produce_composite_state",
                    &format!("state={:02X}", route.composite_state),
                ),
            ];
            source_binding_ids.extend(
                route
                    .label_indices
                    .iter()
                    .map(|index| fixed_menu_pointer_binding_id(*index)),
            );
            source_binding_ids.extend(call_sites.into_iter().map(|call| {
                qualified_source_binding_id(
                    FIXED_STRING_BANK,
                    call.cpu_address,
                    "append_fixed_string",
                    &format!("indices={:02X?}", call.possible_indices),
                )
            }));
            Ok(ScreenConsumerSourceBinding {
                screen_role: route.screen_role,
                population_ids: route
                    .label_indices
                    .iter()
                    .map(|index| population_id(*index))
                    .collect(),
                source_binding_ids,
            })
        })
        .collect::<Result<Vec<_>>>()?;

    Ok(TranslationConsumerSourceEvidence {
        population_ids,
        screen_bindings,
    })
}

fn bind_source(rom: &Rom) -> Result<FixedStringConsumerInspection> {
    let inspection = inspect_fixed_string_consumers(rom)?;
    for spec in FIXED_MENU_LABEL_SPECS {
        let record = inspection
            .records
            .iter()
            .find(|record| record.index == spec.index)
            .with_context(|| format!("fixed-menu record {:02X} disappeared", spec.index))?;
        ensure!(
            record.pointer == spec.pointer && record.source_bytes == spec.expected,
            "fixed-menu source record {:02X} changed",
            spec.index
        );
        ensure!(
            inspection
                .direct_producer_bound_indices
                .contains(&spec.index),
            "fixed-menu record {:02X} lost its direct producer route",
            spec.index
        );
    }

    let producers = bind_direct_composite_state_producer_catalog(rom)?;
    for route in SCREEN_ROUTES {
        ensure!(
            producers.iter().any(|producer| {
                usize::from(producer.prg_bank) == route.producer_bank
                    && producer.cpu_address == route.producer
                    && producer.transfer_opcode == 0x4C
                    && producer.state == route.composite_state
            }),
            "fixed-menu state {:02X} producer changed",
            route.composite_state
        );
    }

    let routed_indices = SCREEN_ROUTES
        .iter()
        .flat_map(|route| route.label_indices.iter().copied())
        .collect::<BTreeSet<_>>();
    ensure!(
        routed_indices
            == FIXED_MENU_LABEL_SPECS
                .iter()
                .map(|spec| spec.index)
                .collect(),
        "fixed-menu screen routes do not cover the target population"
    );
    Ok(inspection)
}

fn population_id(index: u8) -> String {
    format!("fixed-menu-label:{index:02X}")
}

pub(crate) fn fixed_menu_screen_roles() -> &'static [&'static str] {
    &[
        "unit_selection",
        "game_speed_selection",
        "storage_action_menu",
        "storage_overflow_action",
        "storage_capacity_notice",
    ]
}

pub(crate) fn fixed_menu_pointer_binding_id(index: u8) -> String {
    source_binding_id(
        FIXED_STRING_BANK,
        FIXED_STRING_POINTER_TABLE + u16::from(index) * 2,
        "fixed_menu_label_pointer",
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn screen_routes_cover_every_label_and_preserve_the_shared_storage_label() {
        let routed = SCREEN_ROUTES
            .iter()
            .flat_map(|route| route.label_indices.iter().copied())
            .collect::<Vec<_>>();
        assert_eq!(routed.iter().filter(|index| **index == 0x35).count(), 2);
        assert_eq!(
            routed.into_iter().collect::<BTreeSet<_>>(),
            FIXED_MENU_LABEL_SPECS
                .iter()
                .map(|spec| spec.index)
                .collect()
        );
    }

    #[test]
    fn every_label_has_one_structural_terminator_and_a_visible_payload() {
        for spec in FIXED_MENU_LABEL_SPECS {
            assert!([0xED, 0xEF].contains(&spec.terminator));
            assert_eq!(spec.expected.last(), Some(&spec.terminator));
            assert!(spec.expected.len() > 1);
            assert!(spec.max_visible_cells >= spec.expected.len() - 1);
        }
    }

    #[test]
    fn consumer_roles_are_unique_and_stable() {
        assert_eq!(fixed_menu_screen_roles().len(), 5);
        assert_eq!(
            fixed_menu_screen_roles()
                .iter()
                .copied()
                .collect::<BTreeSet<_>>()
                .len(),
            fixed_menu_screen_roles().len()
        );
    }
}
