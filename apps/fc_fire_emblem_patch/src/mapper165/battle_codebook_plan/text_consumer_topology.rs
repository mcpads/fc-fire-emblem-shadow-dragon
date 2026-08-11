use std::collections::BTreeSet;

use anyhow::{Context, Result, ensure};
use serde::Serialize;

use crate::{rom::Rom, sha1_hex};

use super::{phase_cooccurrence::trace_switchable_control_flow, source_window::prg_bank};

pub(super) mod source_contract;

use source_contract::{
    BATTLE_TERRAIN_BANK_HANDLER_POINTER, CALLER_SPECS, CallerKey,
    DIALOGUE_BOX_INNER_STATE_POINTERS, ENDING_SCROLL_INNER_STATE_POINTERS,
    ENDING_SEQUENCE_PHASE_POINTERS, bind_text_consumer_source,
};

#[derive(Clone, Debug, Serialize)]
pub(super) struct BattleTextConsumerTopology {
    renderer_bank_hex: String,
    renderer_address_hex: String,
    renderer_byte_count: usize,
    renderer_source_sha1: String,
    renderer_typed_instruction_count: usize,
    glyph_read_address_hex: String,
    glyph_read_source_bytes_hex: String,
    glyph_read_source_sha1: String,
    row_buffer_count: usize,
    row_buffer_byte_capacity: usize,
    maximum_published_queue_byte_count: usize,
    direct_caller_count: usize,
    battle_caller_count: usize,
    reached_battle_caller_count: usize,
    unresolved_battle_caller_count: usize,
    non_battle_caller_count: usize,
    unresolved_battle_caller_addresses_hex: Vec<String>,
    caller_catalog_sha1: String,
    callers: Vec<TextConsumerCaller>,
    reachability_groups: Vec<TextConsumerReachability>,
    every_direct_caller_typed_as_call: bool,
    every_direct_caller_classified: bool,
    every_battle_caller_reached_from_declared_battle_state: bool,
    every_non_battle_caller_reached_from_declared_ending_state: bool,
    shared_renderer_requires_battle_conditional_projection: bool,
    hook_must_preserve_registers: [&'static str; 3],
    hook_final_flags_contract: &'static str,
    projection_hook_installed: bool,
    runtime_verified: bool,
}

#[derive(Clone, Debug, Serialize)]
struct TextConsumerCaller {
    role: &'static str,
    lifetime: &'static str,
    bank_hex: String,
    address_hex: String,
}

#[derive(Clone, Debug, Serialize)]
struct TextConsumerReachability {
    role: &'static str,
    lifetime: &'static str,
    #[serde(skip)]
    bank: u8,
    bank_hex: String,
    root_addresses_hex: Vec<String>,
    traced_instruction_count: usize,
    #[serde(skip)]
    reached_caller_addresses: BTreeSet<u16>,
    reached_caller_addresses_hex: Vec<String>,
}

pub(super) fn bind_battle_text_consumer_topology(rom: &Rom) -> Result<BattleTextConsumerTopology> {
    let source = bind_text_consumer_source(rom)?;

    let reachability_groups = vec![
        trace_consumer_group(
            rom,
            "battle dialogue row publishers",
            "battle",
            0x04,
            &[0x8237, 0x8369],
            &[0x8263, 0x8392],
        )?,
        trace_consumer_group(
            rom,
            "battle unit panel text publishers",
            "battle",
            0x05,
            &[0x89AA, 0x8A39, 0x8A64],
            &[0x89D0, 0x8A5D, 0x8A8D],
        )?,
        trace_consumer_group(
            rom,
            "battle message publisher",
            "battle",
            0x07,
            &DIALOGUE_BOX_INNER_STATE_POINTERS,
            &[0x82C3],
        )?,
        trace_consumer_group(
            rom,
            "battle terrain publisher",
            "battle",
            0x07,
            &BATTLE_TERRAIN_BANK_HANDLER_POINTER,
            &[0x84A2],
        )?,
        trace_consumer_group(
            rom,
            "ending sequence text publisher",
            "ending",
            0x04,
            &ENDING_SEQUENCE_PHASE_POINTERS,
            &[0x9F9C],
        )?,
        trace_consumer_group(
            rom,
            "ending scroll text publishers",
            "ending",
            0x04,
            &ENDING_SCROLL_INNER_STATE_POINTERS,
            &[0xA43C, 0xA478],
        )?,
    ];
    let reached_battle_callers = reached_callers(&reachability_groups, "battle");
    let reached_ending_callers = reached_callers(&reachability_groups, "ending");
    let expected_battle_callers = CALLER_SPECS
        .iter()
        .filter(|spec| spec.lifetime == "battle")
        .map(|spec| spec.key)
        .collect::<BTreeSet<_>>();
    let unresolved_battle_callers = expected_battle_callers
        .difference(&reached_battle_callers)
        .copied()
        .collect::<BTreeSet<_>>();
    let every_battle_caller_reached = unresolved_battle_callers.is_empty();
    ensure!(
        reached_battle_callers.is_subset(&expected_battle_callers),
        "declared battle roots reached a caller outside the battle catalog"
    );
    ensure!(
        unresolved_battle_callers.is_empty(),
        "battle text reverse-flow boundary is incomplete: unresolved {unresolved_battle_callers:?}"
    );
    ensure!(
        reached_ending_callers
            == CALLER_SPECS
                .iter()
                .filter(|spec| spec.lifetime == "ending")
                .map(|spec| spec.key)
                .collect(),
        "declared ending roots do not reach every non-battle text caller"
    );

    let callers = CALLER_SPECS
        .iter()
        .map(|spec| TextConsumerCaller {
            role: spec.role,
            lifetime: spec.lifetime,
            bank_hex: format!("0x{:02X}", spec.key.bank),
            address_hex: format!("0x{:04X}", spec.key.address),
        })
        .collect::<Vec<_>>();
    let mut caller_catalog = Vec::new();
    for caller in &callers {
        caller_catalog.extend_from_slice(caller.role.as_bytes());
        caller_catalog.push(0);
        caller_catalog.extend_from_slice(caller.lifetime.as_bytes());
        caller_catalog.push(0);
        caller_catalog.extend_from_slice(caller.bank_hex.as_bytes());
        caller_catalog.push(0);
        caller_catalog.extend_from_slice(caller.address_hex.as_bytes());
        caller_catalog.push(0);
    }

    Ok(BattleTextConsumerTopology {
        renderer_bank_hex: format!("0x{:02X}", source.renderer_bank),
        renderer_address_hex: format!("0x{:04X}", source.renderer_address),
        renderer_byte_count: source.renderer_byte_count,
        renderer_source_sha1: source.renderer_source_sha1,
        renderer_typed_instruction_count: source.renderer_typed_instruction_count,
        glyph_read_address_hex: format!("0x{:04X}", source.glyph_read_address),
        glyph_read_source_bytes_hex: source
            .glyph_read_source_bytes
            .iter()
            .map(|byte| format!("{byte:02X}"))
            .collect::<Vec<_>>()
            .join(" "),
        glyph_read_source_sha1: source.glyph_read_source_sha1,
        row_buffer_count: source.row_buffer_count,
        row_buffer_byte_capacity: source.row_buffer_byte_capacity,
        maximum_published_queue_byte_count: source.maximum_queue_byte_count,
        direct_caller_count: callers.len(),
        battle_caller_count: expected_battle_callers.len(),
        reached_battle_caller_count: reached_battle_callers.len(),
        unresolved_battle_caller_count: unresolved_battle_callers.len(),
        non_battle_caller_count: reached_ending_callers.len(),
        unresolved_battle_caller_addresses_hex: unresolved_battle_callers
            .into_iter()
            .map(|key| format!("0x{:02X}:0x{:04X}", key.bank, key.address))
            .collect(),
        caller_catalog_sha1: sha1_hex(&caller_catalog),
        callers,
        reachability_groups,
        every_direct_caller_typed_as_call: true,
        every_direct_caller_classified: true,
        every_battle_caller_reached_from_declared_battle_state: every_battle_caller_reached,
        every_non_battle_caller_reached_from_declared_ending_state: true,
        shared_renderer_requires_battle_conditional_projection: true,
        hook_must_preserve_registers: ["A", "X", "Y"],
        hook_final_flags_contract: "match CMP #$EF on the returned projected-or-original glyph code before the source BEQ",
        projection_hook_installed: false,
        runtime_verified: false,
    })
}

impl BattleTextConsumerTopology {
    pub(super) fn maximum_published_queue_byte_count(&self) -> usize {
        self.maximum_published_queue_byte_count
    }
}

fn trace_consumer_group(
    rom: &Rom,
    role: &'static str,
    lifetime: &'static str,
    bank_number: u8,
    roots: &[u16],
    expected_callers: &[u16],
) -> Result<TextConsumerReachability> {
    let bank = prg_bank(rom, bank_number)?;
    let target_addresses = expected_callers.iter().copied().collect::<BTreeSet<_>>();
    let mut visited_instructions = BTreeSet::new();
    let mut reached_target_addresses = BTreeSet::new();
    for root in roots.iter().copied().collect::<BTreeSet<_>>() {
        let trace = trace_switchable_control_flow(bank, root, &target_addresses)
            .with_context(|| format!("trace {role} root ${root:04X}"))?;
        visited_instructions.extend(trace.visited_instructions);
        reached_target_addresses.extend(trace.reached_target_addresses);
    }
    ensure!(
        reached_target_addresses == target_addresses,
        "{role} reaches {reached_target_addresses:?}, expected {target_addresses:?}"
    );
    Ok(TextConsumerReachability {
        role,
        lifetime,
        bank: bank_number,
        bank_hex: format!("0x{bank_number:02X}"),
        root_addresses_hex: roots
            .iter()
            .copied()
            .collect::<BTreeSet<_>>()
            .into_iter()
            .map(|address| format!("0x{address:04X}"))
            .collect(),
        traced_instruction_count: visited_instructions.len(),
        reached_caller_addresses: reached_target_addresses.clone(),
        reached_caller_addresses_hex: reached_target_addresses
            .into_iter()
            .map(|address| format!("0x{address:04X}"))
            .collect(),
    })
}

fn reached_callers(groups: &[TextConsumerReachability], lifetime: &str) -> BTreeSet<CallerKey> {
    groups
        .iter()
        .filter(|group| group.lifetime == lifetime)
        .flat_map(|group| {
            group
                .reached_caller_addresses
                .iter()
                .copied()
                .map(|address| CallerKey {
                    bank: group.bank,
                    address,
                })
        })
        .collect()
}

impl BattleTextConsumerTopology {
    #[cfg(test)]
    pub(super) fn test_model() -> Self {
        Self {
            renderer_bank_hex: String::new(),
            renderer_address_hex: String::new(),
            renderer_byte_count: 0,
            renderer_source_sha1: String::new(),
            renderer_typed_instruction_count: 0,
            glyph_read_address_hex: String::new(),
            glyph_read_source_bytes_hex: String::new(),
            glyph_read_source_sha1: String::new(),
            row_buffer_count: 2,
            row_buffer_byte_capacity: 30,
            maximum_published_queue_byte_count: 67,
            direct_caller_count: 0,
            battle_caller_count: 0,
            reached_battle_caller_count: 0,
            unresolved_battle_caller_count: 0,
            non_battle_caller_count: 0,
            unresolved_battle_caller_addresses_hex: Vec::new(),
            caller_catalog_sha1: String::new(),
            callers: Vec::new(),
            reachability_groups: Vec::new(),
            every_direct_caller_typed_as_call: false,
            every_direct_caller_classified: false,
            every_battle_caller_reached_from_declared_battle_state: false,
            every_non_battle_caller_reached_from_declared_ending_state: false,
            shared_renderer_requires_battle_conditional_projection: false,
            hook_must_preserve_registers: ["A", "X", "Y"],
            hook_final_flags_contract: "test",
            projection_hook_installed: false,
            runtime_verified: false,
        }
    }
}
