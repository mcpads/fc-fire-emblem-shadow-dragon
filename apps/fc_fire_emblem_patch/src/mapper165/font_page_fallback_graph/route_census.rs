use std::collections::{BTreeMap, BTreeSet};

use anyhow::{Result, ensure};

use super::super::{
    SELECT_RIGHT_FD_CHR_BANK_FOR_PAIR_ADDRESS,
    roster_page::{
        CENTRAL_RIGHT_FD_SELECTOR_CALL_ADDRESS, CENTRAL_RIGHT_FE_COMPANION_FD_REFRESH_CALL_ADDRESS,
        central_right_fd_selector_call, central_right_fe_companion_fd_refresh_call,
    },
};
use super::{
    BoundFontPageFallbackNode, BoundFontPageFallbackRoute, FontPageFallbackNodeRole,
    FontPageFallbackTransferKind, fixed_slice,
};

const JSR_ABSOLUTE: u8 = 0x20;
const JMP_ABSOLUTE: u8 = 0x4C;

pub(super) fn bind_routes(
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
            .filter(|route| {
                route.target_cpu_address == target.cpu_address
                    && matches!(
                        route.transfer_kind,
                        FontPageFallbackTransferKind::Call | FontPageFallbackTransferKind::Jump
                    )
            })
            .map(|route| {
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

pub(super) fn validate_nonoverlapping_nodes(nodes: &[BoundFontPageFallbackNode]) -> Result<()> {
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

pub(super) fn external_direct_transfer_candidates(
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
