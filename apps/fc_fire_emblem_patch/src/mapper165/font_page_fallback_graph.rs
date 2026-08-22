//! 누적 빌드의 오른쪽 FD 글꼴 선택 경로를 하나의 검증된 그래프로 묶는다.
//!
//! 화면별 선택기는 주소 순서대로 이어진 단순 사슬이 아니다. 전투 중앙 경로와
//! 설정 화면 경로가 서로 다른 곳에서 명단 선택기로 합류한다. 이 그래프를 거치지
//! 않고 선택기 하나만 교체하면 다른 단계가 만든 fallback을 잃을 수 있다.

use anyhow::{Context, Result, ensure};

use crate::rom::Rom;

mod route_census;
mod source_binding;
#[cfg(test)]
mod tests;

use route_census::{bind_routes, validate_nonoverlapping_nodes};
use source_binding::{
    bind_exact_node, bind_generated_register, bind_options_owner_gate,
    maximum_dialogue_selector_end, node_from_single_page_binding,
};

use super::{
    SELECT_RIGHT_FD_CHR_BANK_FOR_PAIR_ADDRESS,
    battle_composition_runtime::{
        CUMULATIVE_RUNTIME_LAYOUT, cumulative_battle_central_right_fd_selector,
        cumulative_battle_central_right_fe_resupply_natural_tail,
    },
    chapter_page_selector::{ChapterPageSequence, build_chapter_page_selector},
    cumulative_patch::{DIALOGUE_FONT_PAGE_SELECTOR_ADDRESS, DIALOGUE_FONT_PAGE_SELECTOR_CAVE_END},
    final_font_page_forwarders::{
        BoundFontPageSelector, bind_front_end_font_page_selector, bind_unit_name_font_page_selector,
    },
    front_end_page::PAGE_ROUTINE_ADDRESS as FRONT_END_SELECTOR_ADDRESS,
    maximum_dialogue_runtime::{
        INITIAL_PAGE_SELECTOR_ADDRESS, bind_installed_initial_page_selector,
    },
    options_page::{
        PAGE_A_REGISTER as OPTIONS_PAGE_A_REGISTER, PAGE_B_REGISTER as OPTIONS_PAGE_B_REGISTER,
        PAGE_ROUTINE_ADDRESS as OPTIONS_SELECTOR_ADDRESS, PAGE_ROUTINE_END as OPTIONS_SELECTOR_END,
        build_page_routine_with_fallback as build_options_selector,
    },
    roster_page::{
        PAGE_REGISTERS as ROSTER_PAGE_REGISTERS, PAGE_ROUTINE_ADDRESS as ROSTER_SELECTOR_ADDRESS,
        PAGE_ROUTINE_END as ROSTER_SELECTOR_END,
        build_page_routine_with_fallback as build_roster_selector,
    },
    shop_dialogue_page::{
        PAGE_ROUTINE_ADDRESS as SHOP_SELECTOR_ADDRESS, PAGE_ROUTINE_END as SHOP_SELECTOR_END,
        build_page_selector as build_shop_selector,
    },
    unit_name_page::PAGE_ROUTINE_ADDRESS as UNIT_SELECTOR_ADDRESS,
};

const FIXED_BANK_BYTE_COUNT: usize = 16 * 1024;
const CUMULATIVE_DIALOGUE_CHAPTER_COUNT: u8 = 2;

#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub(crate) enum FontPageFallbackNodeRole {
    BattleComposition,
    MaximumDialogue,
    OptionsMenu,
    UnitRoster,
    UnitSummaryAndStatus,
    WeaponShopDialogue,
    FrontEndMenu,
    ChapterIntroDialogue,
}

impl FontPageFallbackNodeRole {
    pub(crate) const fn id(self) -> &'static str {
        match self {
            Self::BattleComposition => "battle_composition",
            Self::MaximumDialogue => "maximum_dialogue",
            Self::OptionsMenu => "options_menu",
            Self::UnitRoster => "unit_roster",
            Self::UnitSummaryAndStatus => "unit_summary_and_status",
            Self::WeaponShopDialogue => "weapon_shop_dialogue",
            Self::FrontEndMenu => "front_end_menu",
            Self::ChapterIntroDialogue => "chapter_intro_dialogue",
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum FontPageFallbackTransferKind {
    Call,
    Jump,
    ConditionalBranch,
}

impl FontPageFallbackTransferKind {
    pub(crate) const fn id(self) -> &'static str {
        match self {
            Self::Call => "call",
            Self::Jump => "jump",
            Self::ConditionalBranch => "conditional_branch",
        }
    }
}

#[derive(Debug)]
pub(crate) struct BoundFontPageFallbackNode {
    pub(crate) role: FontPageFallbackNodeRole,
    pub(crate) cpu_address: u16,
    pub(crate) cpu_end_exclusive: u16,
    pub(crate) fallback_target: u16,
    pub(crate) mapper_registers: Vec<u8>,
    pub(crate) expected_bytes: Vec<u8>,
}

#[derive(Debug)]
pub(crate) struct BoundFontPageFallbackRoute {
    pub(crate) source_role: &'static str,
    pub(crate) source_cpu_address: u16,
    pub(crate) transfer_kind: FontPageFallbackTransferKind,
    pub(crate) target_role: &'static str,
    pub(crate) target_cpu_address: u16,
}

#[derive(Debug)]
pub(crate) struct BoundFontPageFallbackGraph {
    pub(crate) nodes: Vec<BoundFontPageFallbackNode>,
    pub(crate) routes: Vec<BoundFontPageFallbackRoute>,
    pub(crate) direct_entry_candidate_count: usize,
    pub(crate) conditional_entry_count: usize,
    pub(crate) terminal_fallback_count: usize,
    unit_name_selector: BoundFontPageSelector,
    front_end_selector: BoundFontPageSelector,
}

/// 누적 선택기 그래프에서 최종 대사 런타임이 넘겨받는 정확한 한 갈래다.
///
/// 중앙 전투 선택기의 fallback 명령 위치와, 폐기되는 최대 대사 선택기 뒤에 남길
/// 비활성 경로를 한 번의 그래프 결속에서 함께 얻는다. 선택기 길이가 바뀌어 fallback
/// 명령이 이동해도 최종 통합기가 과거 주소를 별도 상수로 기억하지 않게 한다.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct BoundFontPageRuntimeTakeover {
    pub(crate) owner_cpu_address: u16,
    pub(crate) hook_cpu_address: u16,
    pub(crate) superseded_selector_cpu_address: u16,
    pub(crate) inactive_fallback_cpu_address: u16,
    pub(crate) expected_hook_bytes: [u8; 3],
}

impl BoundFontPageFallbackGraph {
    pub(crate) fn unit_name_selector(&self) -> &BoundFontPageSelector {
        &self.unit_name_selector
    }

    pub(crate) fn front_end_selector(&self) -> &BoundFontPageSelector {
        &self.front_end_selector
    }

    pub(crate) fn integrated_dialogue_runtime_takeover(
        &self,
    ) -> Result<BoundFontPageRuntimeTakeover> {
        let central = self
            .nodes
            .iter()
            .find(|node| node.role == FontPageFallbackNodeRole::BattleComposition)
            .context("font-page graph has no battle-composition owner")?;
        let maximum = self
            .nodes
            .iter()
            .find(|node| node.role == FontPageFallbackNodeRole::MaximumDialogue)
            .context("font-page graph has no maximum-dialogue selector")?;
        let routes = self
            .routes
            .iter()
            .filter(|route| {
                route.source_role == central.role.id()
                    && route.target_role == maximum.role.id()
                    && route.target_cpu_address == maximum.cpu_address
            })
            .collect::<Vec<_>>();
        ensure!(
            routes.len() == 1 && routes[0].transfer_kind == FontPageFallbackTransferKind::Jump,
            "font-page graph does not have one jump from battle composition to maximum dialogue"
        );
        let route = routes[0];
        let expected_hook_bytes = [
            0x4C,
            maximum.cpu_address as u8,
            (maximum.cpu_address >> 8) as u8,
        ];
        let offset = usize::from(
            route
                .source_cpu_address
                .checked_sub(central.cpu_address)
                .context("font-page takeover starts before its battle owner")?,
        );
        ensure!(
            route.source_cpu_address + 3 <= central.cpu_end_exclusive
                && central
                    .expected_bytes
                    .get(offset..offset + expected_hook_bytes.len())
                    == Some(expected_hook_bytes.as_slice()),
            "font-page takeover is not the bound battle fallback instruction"
        );
        Ok(BoundFontPageRuntimeTakeover {
            owner_cpu_address: central.cpu_address,
            hook_cpu_address: route.source_cpu_address,
            superseded_selector_cpu_address: maximum.cpu_address,
            inactive_fallback_cpu_address: maximum.fallback_target,
            expected_hook_bytes,
        })
    }
}

pub(crate) fn bind_cumulative_font_page_fallback_graph(
    candidate: &Rom,
) -> Result<BoundFontPageFallbackGraph> {
    ensure!(
        candidate.mapper() == 165,
        "font-page fallback graph candidate is not mapper 165"
    );
    let fixed = active_fixed_bank(candidate)?;

    let central_bytes = cumulative_battle_central_right_fd_selector(INITIAL_PAGE_SELECTOR_ADDRESS)?;
    let central_end = CUMULATIVE_RUNTIME_LAYOUT
        .battle_central_right_fd_selector
        .checked_add(u16::try_from(central_bytes.len())?)
        .context("battle-composition selector end overflow")?;
    let central_resupply_tail = cumulative_battle_central_right_fe_resupply_natural_tail()?;
    let central_resupply_tail_start =
        CUMULATIVE_RUNTIME_LAYOUT.central_right_fe_resupply_natural_tail;
    let central_resupply_tail_end = central_resupply_tail_start
        .checked_add(u16::try_from(central_resupply_tail.len())?)
        .context("central FE resupply tail end overflow")?;
    ensure!(
        central_end == central_resupply_tail_start
            && central_resupply_tail_end <= CUMULATIVE_RUNTIME_LAYOUT.battle_right_fe_selector
            && fixed_slice(
                fixed,
                central_resupply_tail_start,
                central_resupply_tail.len(),
            )? == central_resupply_tail
            && fixed_slice(
                fixed,
                central_resupply_tail_end,
                usize::from(
                    CUMULATIVE_RUNTIME_LAYOUT.battle_right_fe_selector - central_resupply_tail_end,
                ),
            )?
            .iter()
            .all(|byte| *byte == 0xFF),
        "battle-composition selector or central FE resupply tail changed"
    );
    let central = bind_exact_node(
        fixed,
        FontPageFallbackNodeRole::BattleComposition,
        CUMULATIVE_RUNTIME_LAYOUT.battle_central_right_fd_selector,
        central_end,
        INITIAL_PAGE_SELECTOR_ADDRESS,
        Vec::new(),
        central_bytes,
    )?;

    let maximum_end = maximum_dialogue_selector_end(fixed)?;
    let maximum_bytes = fixed_slice(
        fixed,
        INITIAL_PAGE_SELECTOR_ADDRESS,
        usize::from(maximum_end - INITIAL_PAGE_SELECTOR_ADDRESS),
    )?;
    bind_installed_initial_page_selector(maximum_bytes, ROSTER_SELECTOR_ADDRESS)?;
    let maximum = bind_exact_node(
        fixed,
        FontPageFallbackNodeRole::MaximumDialogue,
        INITIAL_PAGE_SELECTOR_ADDRESS,
        maximum_end,
        ROSTER_SELECTOR_ADDRESS,
        Vec::new(),
        maximum_bytes.to_vec(),
    )?;

    let options_bytes = build_options_selector(
        OPTIONS_PAGE_A_REGISTER,
        OPTIONS_PAGE_B_REGISTER,
        ROSTER_SELECTOR_ADDRESS,
    )?;
    let options = bind_exact_node(
        fixed,
        FontPageFallbackNodeRole::OptionsMenu,
        OPTIONS_SELECTOR_ADDRESS,
        OPTIONS_SELECTOR_END,
        ROSTER_SELECTOR_ADDRESS,
        vec![OPTIONS_PAGE_A_REGISTER, OPTIONS_PAGE_B_REGISTER],
        options_bytes,
    )?;
    let options_gate = bind_options_owner_gate(fixed)?;

    let roster = bind_exact_node(
        fixed,
        FontPageFallbackNodeRole::UnitRoster,
        ROSTER_SELECTOR_ADDRESS,
        ROSTER_SELECTOR_END,
        UNIT_SELECTOR_ADDRESS,
        ROSTER_PAGE_REGISTERS.to_vec(),
        build_roster_selector(
            ROSTER_PAGE_REGISTERS[0],
            ROSTER_PAGE_REGISTERS[1],
            UNIT_SELECTOR_ADDRESS,
        )?,
    )?;

    let unit_name_selector = bind_unit_name_font_page_selector(candidate)?;
    let unit = node_from_single_page_binding(
        FontPageFallbackNodeRole::UnitSummaryAndStatus,
        &unit_name_selector,
    );

    let shop_actual = fixed_slice(
        fixed,
        SHOP_SELECTOR_ADDRESS,
        usize::from(SHOP_SELECTOR_END - SHOP_SELECTOR_ADDRESS),
    )?;
    let shop_register = bind_generated_register(shop_actual, |register| {
        build_shop_selector(register, FRONT_END_SELECTOR_ADDRESS)
    })?;
    let shop = bind_exact_node(
        fixed,
        FontPageFallbackNodeRole::WeaponShopDialogue,
        SHOP_SELECTOR_ADDRESS,
        SHOP_SELECTOR_END,
        FRONT_END_SELECTOR_ADDRESS,
        vec![shop_register],
        shop_actual.to_vec(),
    )?;

    let front_end_selector = bind_front_end_font_page_selector(candidate)?;
    let front_end =
        node_from_single_page_binding(FontPageFallbackNodeRole::FrontEndMenu, &front_end_selector);

    let dialogue_actual = fixed_slice(
        fixed,
        DIALOGUE_FONT_PAGE_SELECTOR_ADDRESS,
        usize::from(DIALOGUE_FONT_PAGE_SELECTOR_CAVE_END - DIALOGUE_FONT_PAGE_SELECTOR_ADDRESS),
    )?;
    let dialogue_first_register = bind_generated_register(dialogue_actual, |register| {
        build_chapter_page_selector(
            DIALOGUE_FONT_PAGE_SELECTOR_ADDRESS,
            ChapterPageSequence {
                admitted_chapter_count: CUMULATIVE_DIALOGUE_CHAPTER_COUNT,
                first_mapper_register: register,
            },
            SELECT_RIGHT_FD_CHR_BANK_FOR_PAIR_ADDRESS,
        )
    })?;
    let dialogue = bind_exact_node(
        fixed,
        FontPageFallbackNodeRole::ChapterIntroDialogue,
        DIALOGUE_FONT_PAGE_SELECTOR_ADDRESS,
        DIALOGUE_FONT_PAGE_SELECTOR_CAVE_END,
        SELECT_RIGHT_FD_CHR_BANK_FOR_PAIR_ADDRESS,
        (0..CUMULATIVE_DIALOGUE_CHAPTER_COUNT)
            .map(|index| dialogue_first_register + index * 8)
            .collect(),
        dialogue_actual.to_vec(),
    )?;

    let nodes = vec![
        central, maximum, options, roster, unit, shop, front_end, dialogue,
    ];
    validate_nonoverlapping_nodes(&nodes)?;

    let routes = bind_routes(fixed, &nodes, options_gate)?;
    let direct_entry_candidate_count = routes
        .iter()
        .filter(|route| {
            matches!(
                route.transfer_kind,
                FontPageFallbackTransferKind::Call | FontPageFallbackTransferKind::Jump
            ) && route.target_role != "original_pair_aware_selector"
        })
        .count();
    ensure!(
        nodes.len() == 8
            && routes.len() == 11
            && direct_entry_candidate_count == 9
            && routes
                .iter()
                .filter(|route| {
                    route.transfer_kind == FontPageFallbackTransferKind::ConditionalBranch
                })
                .count()
                == 1
            && routes
                .iter()
                .filter(|route| route.target_role == "original_pair_aware_selector")
                .count()
                == 1,
        "font-page fallback graph population changed"
    );

    Ok(BoundFontPageFallbackGraph {
        nodes,
        routes,
        direct_entry_candidate_count,
        conditional_entry_count: 1,
        terminal_fallback_count: 1,
        unit_name_selector,
        front_end_selector,
    })
}

fn active_fixed_bank(candidate: &Rom) -> Result<&[u8]> {
    candidate
        .prg()
        .get(candidate.prg().len().saturating_sub(FIXED_BANK_BYTE_COUNT)..)
        .context("mapper-165 candidate has no active fixed PRG bank")
}

fn fixed_slice(fixed: &[u8], address: u16, len: usize) -> Result<&[u8]> {
    ensure!(address >= 0xC000, "fixed selector address is below $C000");
    fixed
        .get(usize::from(address - 0xC000)..usize::from(address - 0xC000) + len)
        .context("fixed selector range is outside the active bank")
}
