//! 누적 빌드의 오른쪽 FD 글꼴 선택 경로를 하나의 검증된 그래프로 묶는다.
//!
//! 화면별 선택기는 주소 순서대로 이어진 단순 사슬이 아니다. 전투 중앙 경로와
//! 설정 화면 경로가 서로 다른 곳에서 명단 선택기로 합류한다. 이 그래프를 거치지
//! 않고 선택기 하나만 교체하면 다른 단계가 만든 fallback을 잃을 수 있다.

use std::collections::{BTreeMap, BTreeSet};

use anyhow::{Context, Result, ensure};

use crate::{rom::Rom, typed_source::decode_rp2a03_sequence};

use super::{
    MAXIMUM_CHR_PAGE_COUNT, SELECT_RIGHT_FD_CHR_BANK_FOR_PAIR_ADDRESS,
    battle_composition_loader_probe::{
        CUMULATIVE_RUNTIME_LAYOUT, cumulative_battle_central_right_fd_selector,
    },
    chapter_page_selector::{ChapterPageSequence, build_chapter_page_selector},
    cumulative_patch::{DIALOGUE_SELECTOR_ADDRESS, DIALOGUE_SELECTOR_CAVE_END},
    final_font_page_forwarders::{
        BoundFontPageSelector, bind_front_end_font_page_selector, bind_unit_name_font_page_selector,
    },
    front_end_page::PAGE_ROUTINE_ADDRESS as FRONT_END_SELECTOR_ADDRESS,
    maximum_dialogue_runtime::{
        INITIAL_PAGE_SELECTOR_ADDRESS, INITIAL_PAGE_SELECTOR_CAVE_END,
        bind_installed_initial_page_selector,
    },
    options_page::{
        PAGE_A_REGISTER as OPTIONS_PAGE_A_REGISTER, PAGE_B_REGISTER as OPTIONS_PAGE_B_REGISTER,
        PAGE_ROUTINE_ADDRESS as OPTIONS_SELECTOR_ADDRESS, PAGE_ROUTINE_END as OPTIONS_SELECTOR_END,
        ROW_OWNER_GATE_ADDRESS, ROW_OWNER_GATE_END,
        build_page_routine_with_fallback as build_options_selector, build_row_owner_gate,
    },
    roster_page::{
        CENTRAL_RIGHT_FD_SELECTOR_CALL_ADDRESS, CENTRAL_RIGHT_FE_COMPANION_FD_REFRESH_CALL_ADDRESS,
        PAGE_REGISTERS as ROSTER_PAGE_REGISTERS, PAGE_ROUTINE_ADDRESS as ROSTER_SELECTOR_ADDRESS,
        PAGE_ROUTINE_END as ROSTER_SELECTOR_END,
        build_page_routine_with_fallback as build_roster_selector, central_right_fd_selector_call,
        central_right_fe_companion_fd_refresh_call,
    },
    shop_dialogue_page::{
        PAGE_ROUTINE_ADDRESS as SHOP_SELECTOR_ADDRESS, PAGE_ROUTINE_END as SHOP_SELECTOR_END,
        build_page_selector as build_shop_selector,
    },
    unit_name_page::PAGE_ROUTINE_ADDRESS as UNIT_SELECTOR_ADDRESS,
};

const FIXED_BANK_BYTE_COUNT: usize = 16 * 1024;
const JSR_ABSOLUTE: u8 = 0x20;
const JMP_ABSOLUTE: u8 = 0x4C;
const BRANCH_IF_EQUAL: u8 = 0xF0;
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

impl BoundFontPageFallbackGraph {
    pub(crate) fn unit_name_selector(&self) -> &BoundFontPageSelector {
        &self.unit_name_selector
    }

    pub(crate) fn front_end_selector(&self) -> &BoundFontPageSelector {
        &self.front_end_selector
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
    let central = bind_exact_node(
        fixed,
        FontPageFallbackNodeRole::BattleComposition,
        CUMULATIVE_RUNTIME_LAYOUT.battle_central_right_fd_selector,
        CUMULATIVE_RUNTIME_LAYOUT.battle_right_fe_selector,
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
        DIALOGUE_SELECTOR_ADDRESS,
        usize::from(DIALOGUE_SELECTOR_CAVE_END - DIALOGUE_SELECTOR_ADDRESS),
    )?;
    let dialogue_first_register = bind_generated_register(dialogue_actual, |register| {
        build_chapter_page_selector(
            DIALOGUE_SELECTOR_ADDRESS,
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
        DIALOGUE_SELECTOR_ADDRESS,
        DIALOGUE_SELECTOR_CAVE_END,
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

fn bind_routes(
    fixed: &[u8],
    nodes: &[BoundFontPageFallbackNode],
    options_gate_branch_address: u16,
) -> Result<Vec<BoundFontPageFallbackRoute>> {
    let by_role = nodes
        .iter()
        .map(|node| (node.role, node))
        .collect::<BTreeMap<_, _>>();
    let central = by_role[&FontPageFallbackNodeRole::BattleComposition];
    let maximum = by_role[&FontPageFallbackNodeRole::MaximumDialogue];
    let options = by_role[&FontPageFallbackNodeRole::OptionsMenu];
    let roster = by_role[&FontPageFallbackNodeRole::UnitRoster];
    let unit = by_role[&FontPageFallbackNodeRole::UnitSummaryAndStatus];
    let shop = by_role[&FontPageFallbackNodeRole::WeaponShopDialogue];
    let front_end = by_role[&FontPageFallbackNodeRole::FrontEndMenu];
    let dialogue = by_role[&FontPageFallbackNodeRole::ChapterIntroDialogue];

    ensure!(
        fixed_slice(fixed, CENTRAL_RIGHT_FD_SELECTOR_CALL_ADDRESS, 3)?
            == central_right_fd_selector_call(central.cpu_address)?
            && fixed_slice(fixed, CENTRAL_RIGHT_FE_COMPANION_FD_REFRESH_CALL_ADDRESS, 3,)?
                == central_right_fe_companion_fd_refresh_call(central.cpu_address)?,
        "central font-page selector call sites changed"
    );

    let routes = vec![
        route(
            "central_right_fd_call",
            CENTRAL_RIGHT_FD_SELECTOR_CALL_ADDRESS,
            FontPageFallbackTransferKind::Call,
            central,
        ),
        route(
            "central_right_fe_companion_refresh",
            CENTRAL_RIGHT_FE_COMPANION_FD_REFRESH_CALL_ADDRESS,
            FontPageFallbackTransferKind::Call,
            central,
        ),
        fallback_route(central, maximum)?,
        route(
            "options_row_owner_gate",
            options_gate_branch_address,
            FontPageFallbackTransferKind::ConditionalBranch,
            options,
        ),
        fallback_route(maximum, roster)?,
        fallback_route(options, roster)?,
        fallback_route(roster, unit)?,
        fallback_route(unit, shop)?,
        fallback_route(shop, front_end)?,
        fallback_route(front_end, dialogue)?,
        BoundFontPageFallbackRoute {
            source_role: dialogue.role.id(),
            source_cpu_address: fallback_transfer(dialogue)?.0,
            transfer_kind: FontPageFallbackTransferKind::Jump,
            target_role: "original_pair_aware_selector",
            target_cpu_address: SELECT_RIGHT_FD_CHR_BANK_FOR_PAIR_ADDRESS,
        },
    ];

    for target in nodes {
        let expected = routes
            .iter()
            .filter_map(|route| {
                (route.target_cpu_address == target.cpu_address
                    && matches!(
                        route.transfer_kind,
                        FontPageFallbackTransferKind::Call | FontPageFallbackTransferKind::Jump
                    ))
                .then(|| {
                    (
                        route.source_cpu_address,
                        match route.transfer_kind {
                            FontPageFallbackTransferKind::Call => JSR_ABSOLUTE,
                            FontPageFallbackTransferKind::Jump => JMP_ABSOLUTE,
                            FontPageFallbackTransferKind::ConditionalBranch => unreachable!(),
                        },
                        route.target_cpu_address,
                    )
                })
            })
            .collect::<Vec<_>>();
        let actual = external_direct_transfer_candidates(
            fixed,
            target.cpu_address,
            target.cpu_end_exclusive,
        );
        ensure!(
            actual == expected,
            "{} direct-entry candidate census changed: {actual:?}",
            target.role.id()
        );
    }
    Ok(routes)
}

fn route(
    source_role: &'static str,
    source_cpu_address: u16,
    transfer_kind: FontPageFallbackTransferKind,
    target: &BoundFontPageFallbackNode,
) -> BoundFontPageFallbackRoute {
    BoundFontPageFallbackRoute {
        source_role,
        source_cpu_address,
        transfer_kind,
        target_role: target.role.id(),
        target_cpu_address: target.cpu_address,
    }
}

fn fallback_route(
    source: &BoundFontPageFallbackNode,
    target: &BoundFontPageFallbackNode,
) -> Result<BoundFontPageFallbackRoute> {
    let (source_cpu_address, opcode, actual_target) = fallback_transfer(source)?;
    ensure!(
        actual_target == target.cpu_address,
        "{} fallback no longer targets {}",
        source.role.id(),
        target.role.id()
    );
    let transfer_kind = match opcode {
        JSR_ABSOLUTE => FontPageFallbackTransferKind::Call,
        JMP_ABSOLUTE => FontPageFallbackTransferKind::Jump,
        _ => unreachable!("fallback transfer binder only returns JSR/JMP"),
    };
    Ok(BoundFontPageFallbackRoute {
        source_role: source.role.id(),
        source_cpu_address,
        transfer_kind,
        target_role: target.role.id(),
        target_cpu_address: target.cpu_address,
    })
}

fn fallback_transfer(node: &BoundFontPageFallbackNode) -> Result<(u16, u8, u16)> {
    let matches = node
        .expected_bytes
        .windows(3)
        .enumerate()
        .filter_map(|(offset, bytes)| {
            matches!(bytes[0], JSR_ABSOLUTE | JMP_ABSOLUTE)
                .then(|| {
                    (
                        node.cpu_address + u16::try_from(offset).expect("selector offset fits u16"),
                        bytes[0],
                        u16::from_le_bytes([bytes[1], bytes[2]]),
                    )
                })
                .filter(|(_, _, target)| *target == node.fallback_target)
        })
        .collect::<Vec<_>>();
    ensure!(
        matches.len() == 1,
        "{} does not contain exactly one generated fallback transfer: {matches:?}",
        node.role.id()
    );
    Ok(matches[0])
}

fn bind_exact_node(
    fixed: &[u8],
    role: FontPageFallbackNodeRole,
    cpu_address: u16,
    cpu_end_exclusive: u16,
    fallback_target: u16,
    mapper_registers: Vec<u8>,
    expected_bytes: Vec<u8>,
) -> Result<BoundFontPageFallbackNode> {
    ensure!(
        cpu_address < cpu_end_exclusive
            && expected_bytes.len() == usize::from(cpu_end_exclusive - cpu_address)
            && fixed_slice(fixed, cpu_address, expected_bytes.len())? == expected_bytes,
        "{} generated selector bytes changed",
        role.id()
    );
    decode_rp2a03_sequence(
        &expected_bytes,
        cpu_address,
        "cumulative font-page fallback selector",
    )?;
    ensure!(
        mapper_registers.iter().all(|register| *register != 0),
        "{} uses an empty mapper register",
        role.id()
    );
    Ok(BoundFontPageFallbackNode {
        role,
        cpu_address,
        cpu_end_exclusive,
        fallback_target,
        mapper_registers,
        expected_bytes,
    })
}

fn node_from_single_page_binding(
    role: FontPageFallbackNodeRole,
    binding: &BoundFontPageSelector,
) -> BoundFontPageFallbackNode {
    BoundFontPageFallbackNode {
        role,
        cpu_address: binding.cpu_address,
        cpu_end_exclusive: binding.cpu_end_exclusive,
        fallback_target: binding.fallback_target,
        mapper_registers: vec![binding.mapper_register],
        expected_bytes: binding.expected_bytes.clone(),
    }
}

fn maximum_dialogue_selector_end(fixed: &[u8]) -> Result<u16> {
    let matches =
        external_direct_transfer_candidates(fixed, ROSTER_SELECTOR_ADDRESS, ROSTER_SELECTOR_END)
            .into_iter()
            .filter(|(source, opcode, target)| {
                (INITIAL_PAGE_SELECTOR_ADDRESS..INITIAL_PAGE_SELECTOR_CAVE_END).contains(source)
                    && *opcode == JMP_ABSOLUTE
                    && *target == ROSTER_SELECTOR_ADDRESS
            })
            .collect::<Vec<_>>();
    ensure!(
        matches.len() == 1,
        "maximum-dialogue fallback route changed: {matches:?}"
    );
    matches[0]
        .0
        .checked_add(3)
        .context("maximum-dialogue selector end overflow")
}

fn bind_options_owner_gate(fixed: &[u8]) -> Result<u16> {
    let gate = build_row_owner_gate()?;
    let capacity = usize::from(ROW_OWNER_GATE_END - ROW_OWNER_GATE_ADDRESS);
    let actual = fixed_slice(fixed, ROW_OWNER_GATE_ADDRESS, capacity)?;
    ensure!(
        gate.len() <= capacity
            && actual[..gate.len()] == gate
            && actual[gate.len()..].iter().all(|byte| *byte == 0xFF),
        "options row-owner gate or its reserved suffix changed"
    );
    decode_rp2a03_sequence(&gate, ROW_OWNER_GATE_ADDRESS, "options row-owner gate")?;
    let branches = gate
        .windows(2)
        .enumerate()
        .filter_map(|(offset, bytes)| {
            (bytes[0] == BRANCH_IF_EQUAL).then(|| {
                let address = ROW_OWNER_GATE_ADDRESS
                    + u16::try_from(offset).expect("options gate offset fits u16");
                let next = address + 2;
                let target = next.wrapping_add_signed(i16::from(bytes[1] as i8));
                (address, target)
            })
        })
        .collect::<Vec<_>>();
    ensure!(
        branches == vec![(ROW_OWNER_GATE_ADDRESS + 5, OPTIONS_SELECTOR_ADDRESS)],
        "options row-owner gate no longer has one exact branch into its selector: {branches:?}"
    );
    Ok(branches[0].0)
}

fn bind_generated_register(actual: &[u8], build: impl Fn(u8) -> Result<Vec<u8>>) -> Result<u8> {
    let matching = (1_u8..MAXIMUM_CHR_PAGE_COUNT)
        .filter_map(|physical_page| {
            let register = physical_page.checked_mul(4)?;
            (build(register).ok()?.as_slice() == actual).then_some(register)
        })
        .collect::<Vec<_>>();
    ensure!(
        matching.len() == 1,
        "generated selector does not identify exactly one CHR page: {matching:?}"
    );
    Ok(matching[0])
}

fn validate_nonoverlapping_nodes(nodes: &[BoundFontPageFallbackNode]) -> Result<()> {
    let roles = nodes.iter().map(|node| node.role).collect::<BTreeSet<_>>();
    ensure!(
        roles.len() == nodes.len(),
        "font-page fallback graph repeats a selector role"
    );
    for (index, left) in nodes.iter().enumerate() {
        for right in &nodes[index + 1..] {
            ensure!(
                left.cpu_end_exclusive <= right.cpu_address
                    || right.cpu_end_exclusive <= left.cpu_address,
                "font-page fallback selectors {} and {} overlap",
                left.role.id(),
                right.role.id()
            );
        }
    }
    Ok(())
}

fn external_direct_transfer_candidates(
    fixed: &[u8],
    target_start: u16,
    target_end: u16,
) -> Vec<(u16, u8, u16)> {
    fixed
        .windows(3)
        .enumerate()
        .filter_map(|(offset, bytes)| {
            let opcode = bytes[0];
            if !matches!(opcode, JSR_ABSOLUTE | JMP_ABSOLUTE) {
                return None;
            }
            let source = 0xC000 + u16::try_from(offset).expect("16 KiB offset fits u16");
            let target = u16::from_le_bytes([bytes[1], bytes[2]]);
            (!(target_start..target_end).contains(&source)
                && (target_start..target_end).contains(&target))
            .then_some((source, opcode, target))
        })
        .collect()
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::rp2a03::{Instruction, assemble_at};

    const SYNTHETIC_CHR_BANK_COUNT: u8 = 32;
    const MAXIMUM_INITIAL_POINTER: u16 = 0x8FF1;

    fn installed_candidate() -> Rom {
        let mut bytes = crate::test_support::synthetic_mapper165_rom_bytes(0xFF);
        bytes[5] = SYNTHETIC_CHR_BANK_COUNT;
        bytes.resize(
            bytes.len() + usize::from(SYNTHETIC_CHR_BANK_COUNT) * 8 * 1024,
            0,
        );
        let central =
            cumulative_battle_central_right_fd_selector(INITIAL_PAGE_SELECTOR_ADDRESS).unwrap();
        install_fixed(
            &mut bytes,
            CUMULATIVE_RUNTIME_LAYOUT.battle_central_right_fd_selector,
            &central,
        );
        let maximum = super::super::maximum_dialogue_runtime::build_initial_page_selector(
            ROSTER_SELECTOR_ADDRESS,
            MAXIMUM_INITIAL_POINTER,
        )
        .unwrap();
        install_fixed(&mut bytes, INITIAL_PAGE_SELECTOR_ADDRESS, &maximum);
        let options = build_options_selector(
            OPTIONS_PAGE_A_REGISTER,
            OPTIONS_PAGE_B_REGISTER,
            ROSTER_SELECTOR_ADDRESS,
        )
        .unwrap();
        install_fixed(&mut bytes, OPTIONS_SELECTOR_ADDRESS, &options);
        install_fixed(
            &mut bytes,
            ROW_OWNER_GATE_ADDRESS,
            &build_row_owner_gate().unwrap(),
        );
        let roster = build_roster_selector(
            ROSTER_PAGE_REGISTERS[0],
            ROSTER_PAGE_REGISTERS[1],
            UNIT_SELECTOR_ADDRESS,
        )
        .unwrap();
        install_fixed(&mut bytes, ROSTER_SELECTOR_ADDRESS, &roster);
        let unit =
            super::super::unit_name_page::build_page_selector(0xB0, SHOP_SELECTOR_ADDRESS).unwrap();
        install_fixed(&mut bytes, UNIT_SELECTOR_ADDRESS, &unit);
        let shop = build_shop_selector(0xC0, FRONT_END_SELECTOR_ADDRESS).unwrap();
        install_fixed(&mut bytes, SHOP_SELECTOR_ADDRESS, &shop);
        let front =
            super::super::front_end_page::build_page_selector(0xA8, DIALOGUE_SELECTOR_ADDRESS)
                .unwrap();
        install_fixed(&mut bytes, FRONT_END_SELECTOR_ADDRESS, &front);
        let dialogue = build_chapter_page_selector(
            DIALOGUE_SELECTOR_ADDRESS,
            ChapterPageSequence {
                admitted_chapter_count: CUMULATIVE_DIALOGUE_CHAPTER_COUNT,
                first_mapper_register: 0x98,
            },
            SELECT_RIGHT_FD_CHR_BANK_FOR_PAIR_ADDRESS,
        )
        .unwrap();
        install_fixed(&mut bytes, DIALOGUE_SELECTOR_ADDRESS, &dialogue);
        install_fixed(
            &mut bytes,
            CENTRAL_RIGHT_FD_SELECTOR_CALL_ADDRESS,
            &assemble_at(
                CENTRAL_RIGHT_FD_SELECTOR_CALL_ADDRESS,
                &[Instruction::JsrAbsolute(
                    CUMULATIVE_RUNTIME_LAYOUT.battle_central_right_fd_selector,
                )],
            )
            .unwrap(),
        );
        install_fixed(
            &mut bytes,
            CENTRAL_RIGHT_FE_COMPANION_FD_REFRESH_CALL_ADDRESS,
            &assemble_at(
                CENTRAL_RIGHT_FE_COMPANION_FD_REFRESH_CALL_ADDRESS,
                &[Instruction::JsrAbsolute(
                    CUMULATIVE_RUNTIME_LAYOUT.battle_central_right_fd_selector,
                )],
            )
            .unwrap(),
        );
        Rom::parse(bytes).unwrap()
    }

    fn install_fixed(bytes: &mut [u8], address: u16, replacement: &[u8]) {
        let offset = crate::test_support::synthetic_fixed_bank_file_offset(address);
        bytes[offset..offset + replacement.len()].copy_from_slice(replacement);
    }

    #[test]
    fn binds_the_branching_cumulative_fallback_graph() {
        let graph = bind_cumulative_font_page_fallback_graph(&installed_candidate()).unwrap();

        assert_eq!(graph.nodes.len(), 8);
        assert_eq!(graph.routes.len(), 11);
        assert_eq!(graph.direct_entry_candidate_count, 9);
        assert_eq!(graph.conditional_entry_count, 1);
        assert_eq!(graph.terminal_fallback_count, 1);
        assert_eq!(graph.unit_name_selector().mapper_register, 0xB0);
        assert_eq!(graph.front_end_selector().mapper_register, 0xA8);
        assert_eq!(
            graph
                .routes
                .iter()
                .filter(|route| route.target_role == FontPageFallbackNodeRole::UnitRoster.id())
                .map(|route| route.source_role)
                .collect::<BTreeSet<_>>(),
            BTreeSet::from([
                FontPageFallbackNodeRole::MaximumDialogue.id(),
                FontPageFallbackNodeRole::OptionsMenu.id(),
            ])
        );
    }

    #[test]
    fn rejects_an_unclassified_direct_entry_into_any_node() {
        let mut bytes = installed_candidate().data().to_vec();
        install_fixed(
            &mut bytes,
            0xC100,
            &assemble_at(0xC100, &[Instruction::JmpAbsolute(SHOP_SELECTOR_ADDRESS)]).unwrap(),
        );

        let error = bind_cumulative_font_page_fallback_graph(&Rom::parse(bytes).unwrap())
            .unwrap_err()
            .to_string();
        assert!(error.contains("direct-entry candidate census changed"));
    }

    #[test]
    fn rejects_a_drifted_options_branch_before_reclassifying_the_graph() {
        let mut bytes = installed_candidate().data().to_vec();
        let offset = crate::test_support::synthetic_fixed_bank_file_offset(ROW_OWNER_GATE_ADDRESS);
        bytes[offset + 6] ^= 1;

        let error = bind_cumulative_font_page_fallback_graph(&Rom::parse(bytes).unwrap())
            .unwrap_err()
            .to_string();
        assert!(error.contains("options row-owner gate"));
    }
}
