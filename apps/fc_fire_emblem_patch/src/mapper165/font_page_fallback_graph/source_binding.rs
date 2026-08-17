use anyhow::{Context, Result, ensure};

use crate::typed_source::decode_rp2a03_sequence;

use super::super::{
    MAXIMUM_CHR_PAGE_COUNT,
    final_font_page_forwarders::BoundFontPageSelector,
    maximum_dialogue_runtime::{INITIAL_PAGE_SELECTOR_ADDRESS, INITIAL_PAGE_SELECTOR_CAVE_END},
    options_page::{
        PAGE_ROUTINE_ADDRESS as OPTIONS_SELECTOR_ADDRESS, ROW_OWNER_GATE_ADDRESS,
        ROW_OWNER_GATE_END, build_row_owner_gate,
    },
    roster_page::{
        PAGE_ROUTINE_ADDRESS as ROSTER_SELECTOR_ADDRESS, PAGE_ROUTINE_END as ROSTER_SELECTOR_END,
    },
};
use super::{
    BoundFontPageFallbackNode, FontPageFallbackNodeRole, fixed_slice,
    route_census::external_direct_transfer_candidates,
};

const JMP_ABSOLUTE: u8 = 0x4C;
const BRANCH_IF_EQUAL: u8 = 0xF0;

pub(super) fn bind_exact_node(
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

pub(super) fn node_from_single_page_binding(
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

pub(super) fn maximum_dialogue_selector_end(fixed: &[u8]) -> Result<u16> {
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

pub(super) fn bind_options_owner_gate(fixed: &[u8]) -> Result<u16> {
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

pub(super) fn bind_generated_register(
    actual: &[u8],
    build: impl Fn(u8) -> Result<Vec<u8>>,
) -> Result<u8> {
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
