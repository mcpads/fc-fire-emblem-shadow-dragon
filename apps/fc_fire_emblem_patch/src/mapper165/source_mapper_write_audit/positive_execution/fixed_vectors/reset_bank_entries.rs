use std::collections::{BTreeMap, BTreeSet, VecDeque};

use anyhow::{Context, Result, ensure};
use retro_rp2a03::{AddressingMode, Mnemonic, Operand, decode_bytes};

use crate::{
    mapper165::banked_call_dispatch::{
        BANKED_CALL_DISPATCH_ADDRESS, BankedCallTransfer, bind_banked_call_dispatch,
    },
    mapper165::battle_codebook_plan::IndirectWriteDestinationBounds,
    mapper165::inline_pointer_dispatch::{
        INLINE_POINTER_DISPATCH_ADDRESS, bind_inline_pointer_dispatch,
    },
    rom::Rom,
    typed_source::{Rp2a03DirectControlFlow, rp2a03_direct_control_flow},
};

use super::{FIXED_CPU_START, FIXED_PRG_BANK, RESET_RAM_CLEAR_CODE, RESET_RAM_CLEAR_START};

const MAXIMUM_RESET_TRACE_STATES: usize = 50_000;
const SOURCE_PRG_BANK_COUNT: u8 = 16;

#[derive(Clone, Debug, Eq, Ord, PartialEq, PartialOrd)]
enum ReturnFrame {
    Direct(u16),
    Banked {
        continuation: Box<ReturnFrame>,
        restore_bank: Option<u8>,
    },
}

#[derive(Clone, Debug, Eq, Ord, PartialEq, PartialOrd)]
struct ResetTraceState {
    address: u16,
    accumulator: Option<u8>,
    index_x: Option<u8>,
    index_y: Option<u8>,
    zero: Option<bool>,
    negative: Option<bool>,
    carry: Option<bool>,
    pointer_low_00: Option<u8>,
    pointer_high_01: Option<u8>,
    outer_screen_state_24: Option<u8>,
    scheduler_state_25: Option<u8>,
    prg_bank_shadow_29: Option<u8>,
    far_selector_44: Option<u8>,
    main_state_84: Option<u8>,
    state_057a: Option<u8>,
    sound_test_state_05ee: Option<u8>,
    mapped_prg_bank: Option<u8>,
    return_stack: Vec<ReturnFrame>,
}

#[derive(Clone, Debug, Eq, Ord, PartialEq, PartialOrd)]
struct ResetTraceLocation {
    address: u16,
    mapped_prg_bank: Option<u8>,
    outer_screen_state_24: Option<u8>,
    scheduler_state_25: Option<u8>,
    main_state_84: Option<u8>,
    return_stack: Vec<ReturnFrame>,
}

impl ResetTraceState {
    fn at(address: u16) -> Self {
        Self {
            address,
            accumulator: None,
            index_x: None,
            index_y: None,
            zero: None,
            negative: None,
            carry: None,
            pointer_low_00: None,
            pointer_high_01: None,
            outer_screen_state_24: None,
            scheduler_state_25: None,
            prg_bank_shadow_29: None,
            far_selector_44: None,
            main_state_84: None,
            state_057a: None,
            sound_test_state_05ee: None,
            mapped_prg_bank: None,
            return_stack: Vec::new(),
        }
    }

    fn set_accumulator(&mut self, value: Option<u8>) {
        self.set_zero_negative(value);
        self.accumulator = value;
    }

    fn set_index_x(&mut self, value: Option<u8>) {
        self.set_zero_negative(value);
        self.index_x = value;
    }

    fn set_index_y(&mut self, value: Option<u8>) {
        self.set_zero_negative(value);
        self.index_y = value;
    }

    fn set_zero_negative(&mut self, value: Option<u8>) {
        self.zero = value.map(|value| value == 0);
        self.negative = value.map(|value| value & 0x80 != 0);
    }

    fn tracked_memory(&self, address: u16) -> Option<u8> {
        match address {
            0x00 => self.pointer_low_00,
            0x01 => self.pointer_high_01,
            0x24 => self.outer_screen_state_24,
            0x25 => self.scheduler_state_25,
            0x29 => self.prg_bank_shadow_29,
            0x44 => self.far_selector_44,
            0x84 => self.main_state_84,
            0x057A => self.state_057a,
            0x05EE => self.sound_test_state_05ee,
            _ => None,
        }
    }

    fn write_tracked_memory(&mut self, address: u16, value: Option<u8>) {
        match address {
            0x00 => self.pointer_low_00 = value,
            0x01 => self.pointer_high_01 = value,
            0x24 => self.outer_screen_state_24 = value,
            0x25 => self.scheduler_state_25 = value,
            0x29 => self.prg_bank_shadow_29 = value,
            0x44 => self.far_selector_44 = value,
            0x84 => self.main_state_84 = value,
            0x057A => self.state_057a = value,
            0x05EE => self.sound_test_state_05ee = value,
            _ => {}
        }
    }

    fn clobber_tracked_memory_and_bank(&mut self) {
        self.pointer_low_00 = None;
        self.pointer_high_01 = None;
        self.outer_screen_state_24 = None;
        self.scheduler_state_25 = None;
        self.prg_bank_shadow_29 = None;
        self.far_selector_44 = None;
        self.main_state_84 = None;
        self.state_057a = None;
        self.sound_test_state_05ee = None;
        self.mapped_prg_bank = None;
    }

    fn clobber_tracked_memory_in_ranges(
        &mut self,
        destination_ranges: &[std::ops::RangeInclusive<u16>],
    ) {
        for address in [
            0x0000, 0x0001, 0x0024, 0x0025, 0x0029, 0x0044, 0x0084, 0x057A, 0x05EE,
        ] {
            if destination_ranges
                .iter()
                .any(|range| range.contains(&address))
            {
                self.write_tracked_memory(address, None);
            }
        }
    }

    fn location(&self) -> ResetTraceLocation {
        ResetTraceLocation {
            address: self.address,
            mapped_prg_bank: self.mapped_prg_bank,
            outer_screen_state_24: self.outer_screen_state_24,
            scheduler_state_25: self.scheduler_state_25,
            main_state_84: self.main_state_84,
            return_stack: self.return_stack.clone(),
        }
    }

    fn join(&self, other: &Self) -> Self {
        debug_assert_eq!(self.location(), other.location());
        Self {
            address: self.address,
            accumulator: join_value(self.accumulator, other.accumulator),
            index_x: join_value(self.index_x, other.index_x),
            index_y: join_value(self.index_y, other.index_y),
            zero: join_value(self.zero, other.zero),
            negative: join_value(self.negative, other.negative),
            carry: join_value(self.carry, other.carry),
            pointer_low_00: join_value(self.pointer_low_00, other.pointer_low_00),
            pointer_high_01: join_value(self.pointer_high_01, other.pointer_high_01),
            outer_screen_state_24: join_value(
                self.outer_screen_state_24,
                other.outer_screen_state_24,
            ),
            scheduler_state_25: join_value(self.scheduler_state_25, other.scheduler_state_25),
            prg_bank_shadow_29: join_value(self.prg_bank_shadow_29, other.prg_bank_shadow_29),
            far_selector_44: join_value(self.far_selector_44, other.far_selector_44),
            main_state_84: join_value(self.main_state_84, other.main_state_84),
            state_057a: join_value(self.state_057a, other.state_057a),
            sound_test_state_05ee: join_value(
                self.sound_test_state_05ee,
                other.sound_test_state_05ee,
            ),
            mapped_prg_bank: self.mapped_prg_bank,
            return_stack: self.return_stack.clone(),
        }
    }
}

fn join_value<T: Copy + Eq>(left: Option<T>, right: Option<T>) -> Option<T> {
    (left == right).then_some(left).flatten()
}

#[derive(Debug)]
pub(in super::super) struct StatefulBankExecution {
    switchable_roots: BTreeSet<(u8, u16)>,
    reachable_instruction_starts: BTreeSet<(u8, u16)>,
    open_facts: BTreeSet<String>,
    inline_dispatch_selectors: BTreeMap<(u8, u16), BTreeSet<u8>>,
    inline_dispatch_entry_banks: BTreeMap<(u8, u16, u8), BTreeSet<u8>>,
}

impl StatefulBankExecution {
    pub(in super::super) fn switchable_roots(&self) -> &BTreeSet<(u8, u16)> {
        &self.switchable_roots
    }

    pub(in super::super) fn reachable_instruction_starts(&self) -> &BTreeSet<(u8, u16)> {
        &self.reachable_instruction_starts
    }

    pub(in super::super) fn open_fact_descriptions(&self) -> Vec<String> {
        self.open_facts.iter().cloned().collect()
    }

    pub(in super::super) fn inline_dispatch_selectors(&self) -> &BTreeMap<(u8, u16), BTreeSet<u8>> {
        &self.inline_dispatch_selectors
    }

    pub(in super::super) fn inline_dispatch_entry_banks(
        &self,
    ) -> &BTreeMap<(u8, u16, u8), BTreeSet<u8>> {
        &self.inline_dispatch_entry_banks
    }

    pub(in super::super) fn inline_dispatch_contexts(
        &self,
        bank: u8,
        address: u16,
    ) -> BTreeSet<(u8, u8)> {
        self.inline_dispatch_entry_banks
            .iter()
            .filter_map(|(&(actual_bank, actual_address, selector), entry_banks)| {
                (actual_bank == bank && actual_address == address).then_some(
                    entry_banks
                        .iter()
                        .map(move |entry_bank| (selector, *entry_bank)),
                )
            })
            .flatten()
            .collect()
    }
}

pub(super) fn bind_reset_bank_entries(
    source: &Rom,
    reset_root: u16,
    indirect_write_destination_bounds: &BTreeMap<(u8, u16, u8), IndirectWriteDestinationBounds>,
) -> Result<StatefulBankExecution> {
    ensure!(
        reset_root >= FIXED_CPU_START,
        "source reset vector does not enter the fixed PRG window"
    );
    trace_bank_state_entries(
        source,
        VecDeque::from([ResetTraceState::at(reset_root)]),
        &BTreeMap::new(),
        indirect_write_destination_bounds,
    )
}

pub(in super::super) fn trace_fixed_scheduler_contexts(
    source: &Rom,
    dispatch_address: u16,
    return_address: u16,
    entry_contexts: impl IntoIterator<Item = (u8, u8)>,
    owned_inline_selector_domains: &BTreeMap<(u8, u16), BTreeSet<u8>>,
    indirect_write_destination_bounds: &BTreeMap<(u8, u16, u8), IndirectWriteDestinationBounds>,
) -> Result<StatefulBankExecution> {
    let entry_contexts = entry_contexts.into_iter().collect::<BTreeSet<_>>();
    ensure!(
        !entry_contexts.is_empty(),
        "fixed scheduler trace has no selector and entry-bank contexts"
    );
    ensure!(
        entry_contexts
            .iter()
            .all(|(_, bank)| *bank <= FIXED_PRG_BANK),
        "fixed scheduler selector trace has an entry bank outside the MMC4 selector domain"
    );
    let mut pending = VecDeque::new();
    for (selector, mapped_prg_bank) in entry_contexts {
        let mut state = ResetTraceState::at(dispatch_address);
        state.scheduler_state_25 = Some(selector);
        state.prg_bank_shadow_29 = Some(mapped_prg_bank);
        state.mapped_prg_bank = Some(mapped_prg_bank);
        state.return_stack.push(ReturnFrame::Direct(return_address));
        pending.push_back(state);
    }
    trace_bank_state_entries(
        source,
        pending,
        owned_inline_selector_domains,
        indirect_write_destination_bounds,
    )
}

fn trace_bank_state_entries(
    source: &Rom,
    mut pending: VecDeque<ResetTraceState>,
    owned_inline_selector_domains: &BTreeMap<(u8, u16), BTreeSet<u8>>,
    indirect_write_destination_bounds: &BTreeMap<(u8, u16, u8), IndirectWriteDestinationBounds>,
) -> Result<StatefulBankExecution> {
    let mut visited = BTreeMap::<ResetTraceLocation, ResetTraceState>::new();
    let mut switchable_roots = BTreeSet::new();
    let mut reachable_instruction_starts = BTreeSet::new();
    let mut open_facts = BTreeSet::new();
    let mut inline_dispatch_selectors = BTreeMap::<_, BTreeSet<_>>::new();
    let mut inline_dispatch_entry_banks = BTreeMap::<_, BTreeSet<_>>::new();

    while let Some(mut state) = pending.pop_front() {
        let location = state.location();
        if let Some(previous) = visited.get(&location) {
            let joined = previous.join(&state);
            if joined == *previous {
                continue;
            }
            state = joined.clone();
            visited.insert(location, joined);
        } else {
            ensure!(
                visited.len() < MAXIMUM_RESET_TRACE_STATES,
                "source reset bank-state trace exceeded {MAXIMUM_RESET_TRACE_STATES} control locations"
            );
            visited.insert(location, state.clone());
        }

        if state.address == RESET_RAM_CLEAR_START {
            summarize_reset_ram_clear(source, &mut state, &mut reachable_instruction_starts)?;
            pending.push_back(state);
            continue;
        }
        let physical_bank = physical_bank_for_state(&state, &mut open_facts)?;
        let Some(physical_bank) = physical_bank else {
            continue;
        };
        let instruction = decode_bytes(&source_instruction_bytes(
            source,
            physical_bank,
            state.address,
            3,
        )?)
        .with_context(|| {
            format!(
                "decode reset bank-state instruction at {physical_bank:02X}:${:04X}",
                state.address,
            )
        })?;
        if !instruction.opcode_is_documented() {
            open_facts.insert(format!(
                "undocumented_opcode@{physical_bank:02X}:{:04X}",
                state.address,
            ));
            continue;
        }
        reachable_instruction_starts.insert((physical_bank, state.address));
        apply_data_effect(
            &instruction,
            &mut state,
            physical_bank,
            indirect_write_destination_bounds,
            &mut open_facts,
        )?;

        match rp2a03_direct_control_flow(&instruction, state.address)? {
            Rp2a03DirectControlFlow::FallThrough { next } => {
                state.address = next;
                pending.push_back(state);
            }
            Rp2a03DirectControlFlow::Branch {
                target,
                fallthrough,
            } => {
                let condition = branch_condition(instruction.mnemonic(), &state);
                if condition != Some(false) {
                    let mut taken = state.clone();
                    taken.address = target;
                    pending.push_back(taken);
                }
                if condition != Some(true) {
                    if let Some(fallthrough) = fallthrough {
                        state.address = fallthrough;
                        pending.push_back(state);
                    }
                }
            }
            Rp2a03DirectControlFlow::Call {
                target,
                return_address,
            } if target == BANKED_CALL_DISPATCH_ADDRESS => {
                route_banked_call(
                    source,
                    physical_bank,
                    state,
                    BankedCallTransfer::Call,
                    ReturnFrame::Direct(return_address),
                    &mut pending,
                    &mut switchable_roots,
                    &mut open_facts,
                )?;
            }
            Rp2a03DirectControlFlow::Call {
                target,
                return_address: _,
            } if target == INLINE_POINTER_DISPATCH_ADDRESS => {
                let selectors = match state.accumulator {
                    Some(selector) => BTreeSet::from([selector]),
                    None => {
                        let Some(selectors) =
                            owned_inline_selector_domains.get(&(physical_bank, state.address))
                        else {
                            open_facts.insert(format!(
                                "inline_dispatch@{physical_bank:02X}:{:04X}:selector_unknown",
                                state.address,
                            ));
                            continue;
                        };
                        ensure!(
                            !selectors.is_empty(),
                            "owned inline dispatch at {physical_bank:02X}:${:04X} has an empty selector domain",
                            state.address,
                        );
                        selectors.clone()
                    }
                };
                let Some(mapped_prg_bank) = state.mapped_prg_bank else {
                    open_facts.insert(format!(
                        "inline_dispatch@{physical_bank:02X}:{:04X}:entry_bank_unknown",
                        state.address,
                    ));
                    continue;
                };
                for selector in selectors {
                    inline_dispatch_selectors
                        .entry((physical_bank, state.address))
                        .or_default()
                        .insert(selector);
                    inline_dispatch_entry_banks
                        .entry((physical_bank, state.address, selector))
                        .or_default()
                        .insert(mapped_prg_bank);
                    let binding = bind_inline_pointer_dispatch(
                        source,
                        physical_bank,
                        state.address,
                        [selector],
                        "stateful bank execution inline dispatch",
                    )?;
                    let target = *binding
                        .targets_in_selector_order()
                        .first()
                        .context("single stateful inline selector did not bind a target")?;
                    let mut selected = state.clone();
                    selected.set_accumulator(Some(selector.wrapping_mul(2)));
                    route_direct_target(
                        selected,
                        target,
                        &mut pending,
                        &mut switchable_roots,
                        &mut open_facts,
                    );
                }
            }
            Rp2a03DirectControlFlow::Call {
                target,
                return_address,
            } => {
                route_call_target(
                    state,
                    target,
                    return_address,
                    &mut pending,
                    &mut switchable_roots,
                    &mut open_facts,
                );
            }
            Rp2a03DirectControlFlow::Jump {
                target: Some(target),
            } if target == BANKED_CALL_DISPATCH_ADDRESS => {
                let Some(continuation) = state.return_stack.pop() else {
                    open_facts.insert(format!(
                        "banked_tail_jump@{physical_bank:02X}:{:04X}:return_stack_empty",
                        state.address,
                    ));
                    continue;
                };
                route_banked_call(
                    source,
                    physical_bank,
                    state,
                    BankedCallTransfer::TailJump,
                    continuation,
                    &mut pending,
                    &mut switchable_roots,
                    &mut open_facts,
                )?;
            }
            Rp2a03DirectControlFlow::Jump {
                target: Some(target),
            } => route_direct_target(
                state,
                target,
                &mut pending,
                &mut switchable_roots,
                &mut open_facts,
            ),
            Rp2a03DirectControlFlow::Jump { target: None } => {
                open_facts.insert(format!(
                    "indirect_jump@{physical_bank:02X}:{:04X}",
                    state.address,
                ));
            }
            Rp2a03DirectControlFlow::Return => {
                if let Some(frame) = state.return_stack.pop() {
                    resume_return_frame(
                        state,
                        frame,
                        false,
                        &mut pending,
                        &mut switchable_roots,
                        &mut open_facts,
                    );
                }
            }
            Rp2a03DirectControlFlow::Interrupt | Rp2a03DirectControlFlow::Stop => {}
        }
    }

    Ok(StatefulBankExecution {
        switchable_roots,
        reachable_instruction_starts,
        open_facts,
        inline_dispatch_selectors,
        inline_dispatch_entry_banks,
    })
}

fn summarize_reset_ram_clear(
    source: &Rom,
    state: &mut ResetTraceState,
    reachable_instruction_starts: &mut BTreeSet<(u8, u16)>,
) -> Result<()> {
    let bytes = source_instruction_bytes(
        source,
        FIXED_PRG_BANK,
        RESET_RAM_CLEAR_START,
        RESET_RAM_CLEAR_CODE.len(),
    )?;
    ensure!(
        bytes == RESET_RAM_CLEAR_CODE,
        "source reset RAM-clear loop changed before state summarization"
    );
    let mut offset = 0_usize;
    while offset < bytes.len() {
        let instruction = decode_bytes(&bytes[offset..])
            .context("decode source reset RAM-clear summarized instruction")?;
        ensure!(
            instruction.opcode_is_documented(),
            "source reset RAM-clear summary reached undocumented opcode"
        );
        reachable_instruction_starts.insert((
            FIXED_PRG_BANK,
            RESET_RAM_CLEAR_START + u16::try_from(offset)?,
        ));
        offset += instruction.encoded_len();
    }
    ensure!(
        offset == bytes.len(),
        "source reset RAM-clear summary did not consume the exact loop"
    );

    state.pointer_low_00 = Some(0);
    state.pointer_high_01 = Some(0xFF);
    state.outer_screen_state_24 = Some(0);
    state.scheduler_state_25 = Some(0);
    state.prg_bank_shadow_29 = Some(0);
    state.far_selector_44 = Some(0);
    state.main_state_84 = Some(0);
    state.state_057a = Some(0);
    state.sound_test_state_05ee = Some(0);
    state.set_accumulator(Some(0));
    state.set_index_y(Some(0));
    state.zero = Some(false);
    state.negative = Some(true);
    state.address = RESET_RAM_CLEAR_START + u16::try_from(RESET_RAM_CLEAR_CODE.len())?;
    Ok(())
}

fn route_banked_call(
    source: &Rom,
    caller_bank: u8,
    mut state: ResetTraceState,
    transfer: BankedCallTransfer,
    continuation: ReturnFrame,
    pending: &mut VecDeque<ResetTraceState>,
    switchable_roots: &mut BTreeSet<(u8, u16)>,
    open_facts: &mut BTreeSet<String>,
) -> Result<()> {
    let Some(requested_bank) = state.accumulator.map(|value| value & 0x0F) else {
        open_facts.insert(format!(
            "banked_call@{caller_bank:02X}:{:04X}:requested_bank_unknown",
            state.address,
        ));
        return Ok(());
    };
    let Some(selector) = state.far_selector_44 else {
        open_facts.insert(format!(
            "banked_call@{caller_bank:02X}:{:04X}:selector_44_unknown[requested_bank={requested_bank:02X}]",
            state.address,
        ));
        return Ok(());
    };
    let binding = bind_banked_call_dispatch(
        source,
        caller_bank,
        state.address,
        transfer,
        requested_bank,
        selector,
        "reset-rooted banked call",
    )?;
    ensure!(
        binding.call_address() == state.address
            && binding.requested_bank() == requested_bank
            && binding.selector() == selector,
        "reset-rooted banked call binding changed its caller-owned inputs"
    );
    let target = binding.target();
    if target < 0x8000 {
        open_facts.insert(format!(
            "banked_call@{caller_bank:02X}:{:04X}->${target:04X}:ram_target",
            state.address,
        ));
        return Ok(());
    }

    let restore_bank = state.mapped_prg_bank;
    state.mapped_prg_bank = Some(requested_bank);
    state.prg_bank_shadow_29 = Some(requested_bank);
    state.set_accumulator(Some((target >> 8) as u8));
    state.set_index_x(Some(selector.wrapping_mul(2)));
    state.return_stack.push(ReturnFrame::Banked {
        continuation: Box::new(continuation),
        restore_bank,
    });
    if target < FIXED_CPU_START {
        switchable_roots.insert((requested_bank, target));
    }
    state.address = target;
    pending.push_back(state);
    Ok(())
}

fn resume_return_frame(
    mut state: ResetTraceState,
    frame: ReturnFrame,
    restored_bank_boundary: bool,
    pending: &mut VecDeque<ResetTraceState>,
    switchable_roots: &mut BTreeSet<(u8, u16)>,
    open_facts: &mut BTreeSet<String>,
) {
    match frame {
        ReturnFrame::Direct(return_address) => {
            if restored_bank_boundary && return_address < FIXED_CPU_START {
                let Some(bank) = state.mapped_prg_bank else {
                    open_facts.insert(format!("banked_return->${return_address:04X}:bank_unknown"));
                    return;
                };
                switchable_roots.insert((bank, return_address));
            } else {
                record_fixed_to_switchable_entry(
                    &state,
                    return_address,
                    switchable_roots,
                    open_facts,
                );
            }
            state.address = return_address;
            pending.push_back(state);
        }
        ReturnFrame::Banked {
            continuation,
            restore_bank,
        } => {
            state.mapped_prg_bank = restore_bank;
            state.prg_bank_shadow_29 = restore_bank;
            state.set_accumulator(restore_bank);
            resume_return_frame(
                state,
                *continuation,
                true,
                pending,
                switchable_roots,
                open_facts,
            );
        }
    }
}

fn physical_bank_for_state(
    state: &ResetTraceState,
    open_facts: &mut BTreeSet<String>,
) -> Result<Option<u8>> {
    ensure!(
        state.address >= 0x8000,
        "source reset bank-state trace reached RAM at ${:04X}",
        state.address
    );
    if state.address >= FIXED_CPU_START {
        return Ok(Some(FIXED_PRG_BANK));
    }
    match state.mapped_prg_bank {
        Some(bank) if bank < SOURCE_PRG_BANK_COUNT => Ok(Some(bank)),
        _ => {
            open_facts.insert(format!(
                "instruction_fetch@${:04X}:bank_unknown",
                state.address
            ));
            Ok(None)
        }
    }
}

fn source_instruction_bytes(
    source: &Rom,
    physical_bank: u8,
    address: u16,
    byte_count: usize,
) -> Result<Vec<u8>> {
    ensure!(
        physical_bank < SOURCE_PRG_BANK_COUNT,
        "source reset physical bank is outside the MMC4 selector range"
    );
    (0..byte_count)
        .map(|offset| {
            let cpu_address = address.wrapping_add(u16::try_from(offset)?);
            ensure!(
                cpu_address >= 0x8000,
                "source reset instruction fetch wrapped into RAM"
            );
            let (bank, relative) = if cpu_address >= FIXED_CPU_START {
                (FIXED_PRG_BANK, usize::from(cpu_address - FIXED_CPU_START))
            } else {
                (physical_bank, usize::from(cpu_address - 0x8000))
            };
            let prg_offset = usize::from(bank)
                .checked_mul(16 * 1024)
                .and_then(|base| base.checked_add(relative))
                .context("source reset instruction offset overflow")?;
            source.prg().get(prg_offset).copied().with_context(|| {
                format!(
                    "source reset instruction fetch exceeds bank {bank:02X} at ${cpu_address:04X}"
                )
            })
        })
        .collect()
}

fn route_call_target(
    mut state: ResetTraceState,
    target: u16,
    return_address: u16,
    pending: &mut VecDeque<ResetTraceState>,
    switchable_roots: &mut BTreeSet<(u8, u16)>,
    open_facts: &mut BTreeSet<String>,
) {
    if target < 0x8000 {
        open_facts.insert(format!(
            "call@{:04X}->${target:04X}:ram_target",
            state.address
        ));
        return;
    }
    record_fixed_to_switchable_entry(&state, target, switchable_roots, open_facts);
    if target < FIXED_CPU_START && state.mapped_prg_bank.is_none() {
        return;
    }
    state.return_stack.push(ReturnFrame::Direct(return_address));
    state.address = target;
    pending.push_back(state);
}

fn route_direct_target(
    mut state: ResetTraceState,
    target: u16,
    pending: &mut VecDeque<ResetTraceState>,
    switchable_roots: &mut BTreeSet<(u8, u16)>,
    open_facts: &mut BTreeSet<String>,
) {
    if target < 0x8000 {
        open_facts.insert(format!(
            "jump@{:04X}->${target:04X}:ram_target",
            state.address
        ));
        return;
    }
    record_fixed_to_switchable_entry(&state, target, switchable_roots, open_facts);
    if target >= FIXED_CPU_START {
        state.address = target;
        pending.push_back(state);
    } else if state.mapped_prg_bank.is_some() {
        state.address = target;
        pending.push_back(state);
    } else {
        open_facts.insert(format!(
            "switchable_target@{:04X}->${target:04X}:bank_unknown",
            state.address
        ));
    }
}

fn record_fixed_to_switchable_entry(
    state: &ResetTraceState,
    target: u16,
    switchable_roots: &mut BTreeSet<(u8, u16)>,
    open_facts: &mut BTreeSet<String>,
) {
    if state.address < FIXED_CPU_START || target >= FIXED_CPU_START {
        return;
    }
    match state.mapped_prg_bank {
        Some(bank) if bank < SOURCE_PRG_BANK_COUNT => {
            switchable_roots.insert((bank, target));
        }
        _ => {
            open_facts.insert(format!(
                "switchable_target@0F:{:04X}->${target:04X}:bank_unknown",
                state.address
            ));
        }
    }
}

fn apply_data_effect(
    instruction: &retro_rp2a03::Instruction,
    state: &mut ResetTraceState,
    physical_bank: u8,
    indirect_write_destination_bounds: &BTreeMap<(u8, u16, u8), IndirectWriteDestinationBounds>,
    open_facts: &mut BTreeSet<String>,
) -> Result<()> {
    let mode = instruction.addressing_mode();
    let operand = instruction.operand();
    match (instruction.mnemonic(), mode, operand) {
        (Mnemonic::Lda, AddressingMode::Immediate, Operand::Byte(value)) => {
            state.set_accumulator(Some(value));
        }
        (Mnemonic::Ldx, AddressingMode::Immediate, Operand::Byte(value)) => {
            state.set_index_x(Some(value));
        }
        (Mnemonic::Ldy, AddressingMode::Immediate, Operand::Byte(value)) => {
            state.set_index_y(Some(value));
        }
        (Mnemonic::Lda, AddressingMode::ZeroPage, Operand::Byte(address)) => {
            state.set_accumulator(state.tracked_memory(u16::from(address)));
        }
        (Mnemonic::Ldx, AddressingMode::ZeroPage, Operand::Byte(address)) => {
            state.set_index_x(state.tracked_memory(u16::from(address)));
        }
        (Mnemonic::Ldy, AddressingMode::ZeroPage, Operand::Byte(address)) => {
            state.set_index_y(state.tracked_memory(u16::from(address)));
        }
        (Mnemonic::Lda, AddressingMode::Absolute, Operand::Word(address)) => {
            state.set_accumulator(state.tracked_memory(address));
        }
        (Mnemonic::Ldx, AddressingMode::Absolute, Operand::Word(address)) => {
            state.set_index_x(state.tracked_memory(address));
        }
        (Mnemonic::Ldy, AddressingMode::Absolute, Operand::Word(address)) => {
            state.set_index_y(state.tracked_memory(address));
        }
        (Mnemonic::Sta, AddressingMode::ZeroPage, Operand::Byte(address)) => {
            state.write_tracked_memory(u16::from(address), state.accumulator);
        }
        (Mnemonic::Stx, AddressingMode::ZeroPage, Operand::Byte(address)) => {
            state.write_tracked_memory(u16::from(address), state.index_x);
        }
        (Mnemonic::Sty, AddressingMode::ZeroPage, Operand::Byte(address)) => {
            state.write_tracked_memory(u16::from(address), state.index_y);
        }
        (Mnemonic::Sta, AddressingMode::Absolute, Operand::Word(0xA000..=0xAFFF)) => {
            state.mapped_prg_bank = state.accumulator.map(|value| value & 0x0F);
        }
        (Mnemonic::Stx, AddressingMode::Absolute, Operand::Word(0xA000..=0xAFFF)) => {
            state.mapped_prg_bank = state.index_x.map(|value| value & 0x0F);
        }
        (Mnemonic::Sty, AddressingMode::Absolute, Operand::Word(0xA000..=0xAFFF)) => {
            state.mapped_prg_bank = state.index_y.map(|value| value & 0x0F);
        }
        (Mnemonic::Sta, AddressingMode::Absolute, Operand::Word(address)) => {
            state.write_tracked_memory(address, state.accumulator);
        }
        (Mnemonic::Stx, AddressingMode::Absolute, Operand::Word(address)) => {
            state.write_tracked_memory(address, state.index_x);
        }
        (Mnemonic::Sty, AddressingMode::Absolute, Operand::Word(address)) => {
            state.write_tracked_memory(address, state.index_y);
        }
        (Mnemonic::Sta, AddressingMode::ZeroPageIndirectIndexedY, Operand::Byte(pointer)) => {
            if let (Some(low), Some(high), Some(index_y)) = (
                state.tracked_memory(u16::from(pointer)),
                state.tracked_memory(u16::from(pointer.wrapping_add(1))),
                state.index_y,
            ) {
                let base = u16::from_le_bytes([low, high]);
                let target = base.wrapping_add(u16::from(index_y));
                state.write_tracked_memory(target, state.accumulator);
                if (0xA000..=0xAFFF).contains(&target) {
                    state.mapped_prg_bank = state.accumulator.map(|value| value & 0x0F);
                }
            } else {
                if let Some(bounds) =
                    indirect_write_destination_bounds.get(&(physical_bank, state.address, pointer))
                {
                    ensure!(
                        bounds
                            .destination_ranges()
                            .iter()
                            .all(|range| { range.start() <= range.end() && *range.end() < 0x8000 }),
                        "{} indirect-write destination bounds can reach mapper space",
                        bounds.role(),
                    );
                    state.clobber_tracked_memory_in_ranges(bounds.destination_ranges());
                } else {
                    open_facts.insert(format!(
                        "effective_write@{physical_bank:02X}:{:04X}:indirect_target_unknown",
                        state.address,
                    ));
                    state.clobber_tracked_memory_and_bank();
                }
            }
        }
        (Mnemonic::Tax, AddressingMode::Implied, Operand::None) => {
            state.set_index_x(state.accumulator);
        }
        (Mnemonic::Tay, AddressingMode::Implied, Operand::None) => {
            state.set_index_y(state.accumulator);
        }
        (Mnemonic::Txa, AddressingMode::Implied, Operand::None) => {
            state.set_accumulator(state.index_x);
        }
        (Mnemonic::Tya, AddressingMode::Implied, Operand::None) => {
            state.set_accumulator(state.index_y);
        }
        (Mnemonic::Inx, AddressingMode::Implied, Operand::None) => {
            state.set_index_x(state.index_x.map(|value| value.wrapping_add(1)));
        }
        (Mnemonic::Dex, AddressingMode::Implied, Operand::None) => {
            state.set_index_x(state.index_x.map(|value| value.wrapping_sub(1)));
        }
        (Mnemonic::Iny, AddressingMode::Implied, Operand::None) => {
            state.set_index_y(state.index_y.map(|value| value.wrapping_add(1)));
        }
        (Mnemonic::Dey, AddressingMode::Implied, Operand::None) => {
            state.set_index_y(state.index_y.map(|value| value.wrapping_sub(1)));
        }
        (Mnemonic::Inc, AddressingMode::ZeroPage, Operand::Byte(address)) => {
            let value = state
                .tracked_memory(u16::from(address))
                .map(|value| value.wrapping_add(1));
            state.write_tracked_memory(u16::from(address), value);
            state.set_zero_negative(value);
        }
        (Mnemonic::Dec, AddressingMode::ZeroPage, Operand::Byte(address)) => {
            let value = state
                .tracked_memory(u16::from(address))
                .map(|value| value.wrapping_sub(1));
            state.write_tracked_memory(u16::from(address), value);
            state.set_zero_negative(value);
        }
        (Mnemonic::Inc, AddressingMode::Absolute, Operand::Word(address)) => {
            let value = state
                .tracked_memory(address)
                .map(|value| value.wrapping_add(1));
            state.write_tracked_memory(address, value);
            state.set_zero_negative(value);
        }
        (Mnemonic::Dec, AddressingMode::Absolute, Operand::Word(address)) => {
            let value = state
                .tracked_memory(address)
                .map(|value| value.wrapping_sub(1));
            state.write_tracked_memory(address, value);
            state.set_zero_negative(value);
        }
        (Mnemonic::Asl, AddressingMode::Accumulator, Operand::None) => {
            let value = state.accumulator;
            state.carry = value.map(|value| value & 0x80 != 0);
            state.set_accumulator(value.map(|value| value.wrapping_mul(2)));
        }
        (Mnemonic::And, AddressingMode::Immediate, Operand::Byte(mask)) => {
            state.set_accumulator(state.accumulator.map(|value| value & mask));
        }
        (Mnemonic::Ora, AddressingMode::Immediate, Operand::Byte(mask)) => {
            state.set_accumulator(state.accumulator.map(|value| value | mask));
        }
        (Mnemonic::Eor, AddressingMode::Immediate, Operand::Byte(mask)) => {
            state.set_accumulator(state.accumulator.map(|value| value ^ mask));
        }
        (Mnemonic::Cmp, AddressingMode::Immediate, Operand::Byte(value)) => {
            compare(state.accumulator, value, state);
        }
        (Mnemonic::Cpx, AddressingMode::Immediate, Operand::Byte(value)) => {
            compare(state.index_x, value, state);
        }
        (Mnemonic::Cpy, AddressingMode::Immediate, Operand::Byte(value)) => {
            compare(state.index_y, value, state);
        }
        (Mnemonic::Clc, AddressingMode::Implied, Operand::None) => state.carry = Some(false),
        (Mnemonic::Sec, AddressingMode::Implied, Operand::None) => state.carry = Some(true),
        (Mnemonic::Lda, _, _) => state.set_accumulator(None),
        (Mnemonic::Ldx, _, _) => state.set_index_x(None),
        (Mnemonic::Ldy, _, _) => state.set_index_y(None),
        (
            Mnemonic::Adc
            | Mnemonic::Sbc
            | Mnemonic::Ora
            | Mnemonic::And
            | Mnemonic::Eor
            | Mnemonic::Lsr
            | Mnemonic::Rol
            | Mnemonic::Ror
            | Mnemonic::Pla,
            _,
            _,
        ) => {
            state.set_accumulator(None);
            state.carry = None;
        }
        (Mnemonic::Bit | Mnemonic::Cmp | Mnemonic::Cpx | Mnemonic::Cpy, _, _) => {
            state.zero = None;
            state.negative = None;
            state.carry = None;
        }
        _ => {}
    }
    Ok(())
}

fn compare(register: Option<u8>, operand: u8, state: &mut ResetTraceState) {
    state.zero = register.map(|value| value == operand);
    state.carry = register.map(|value| value >= operand);
    state.negative = register.map(|value| value.wrapping_sub(operand) & 0x80 != 0);
}

fn branch_condition(mnemonic: Mnemonic, state: &ResetTraceState) -> Option<bool> {
    match mnemonic {
        Mnemonic::Beq => state.zero,
        Mnemonic::Bne => state.zero.map(|value| !value),
        Mnemonic::Bmi => state.negative,
        Mnemonic::Bpl => state.negative.map(|value| !value),
        Mnemonic::Bcs => state.carry,
        Mnemonic::Bcc => state.carry.map(|value| !value),
        Mnemonic::Bvs | Mnemonic::Bvc => None,
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        mapper165::inline_pointer_dispatch::INLINE_POINTER_DISPATCH_CODE, rom::HEADER_SIZE,
    };

    fn synthetic_destination_bounds(
        site: (u8, u16, u8),
        destination_ranges: Vec<std::ops::RangeInclusive<u16>>,
    ) -> BTreeMap<(u8, u16, u8), IndirectWriteDestinationBounds> {
        BTreeMap::from([(
            site,
            IndirectWriteDestinationBounds::for_synthetic_test(
                "synthetic indirect write",
                destination_ranges,
            ),
        )])
    }

    fn synthetic_source(fixed_program: &[(u16, &[u8])], reset_root: u16) -> Rom {
        let mut bytes = vec![0x60; HEADER_SIZE + 16 * 16 * 1024];
        bytes[..HEADER_SIZE].fill(0);
        bytes[..4].copy_from_slice(b"NES\x1A");
        bytes[4] = 16;
        bytes[6] = 0xA0;
        let fixed = HEADER_SIZE + 15 * 16 * 1024;
        for &(address, program) in fixed_program {
            let offset = fixed + usize::from(address - FIXED_CPU_START);
            bytes[offset..offset + program.len()].copy_from_slice(program);
        }
        let reset_vector = fixed + usize::from(0xFFFC - FIXED_CPU_START);
        bytes[reset_vector..reset_vector + 2].copy_from_slice(&reset_root.to_le_bytes());
        Rom::parse(bytes).unwrap()
    }

    #[test]
    fn immediate_mapper_write_resolves_the_switchable_target_bank() {
        let source = synthetic_source(
            &[(0xC100, &[0xA9, 0x02, 0x8D, 0x00, 0xA0, 0x4C, 0x00, 0x84])],
            0xC100,
        );

        let trace = bind_reset_bank_entries(&source, 0xC100, &BTreeMap::new()).unwrap();

        assert_eq!(trace.switchable_roots(), &BTreeSet::from([(0x02, 0x8400)]));
        assert!(trace.open_facts.is_empty());
    }

    #[test]
    fn unknown_mapper_value_keeps_the_dynamic_target_open() {
        let source = synthetic_source(
            &[(
                0xC100,
                &[0xAD, 0x00, 0x04, 0x8D, 0x00, 0xA0, 0x4C, 0x00, 0x84],
            )],
            0xC100,
        );

        let trace = bind_reset_bank_entries(&source, 0xC100, &BTreeMap::new()).unwrap();

        assert!(trace.switchable_roots().is_empty());
        assert!(
            trace
                .open_fact_descriptions()
                .iter()
                .any(|fact| fact.contains("bank_unknown"))
        );
    }

    #[test]
    fn source_bound_ram_destination_preserves_the_selected_prg_bank() {
        let source = synthetic_source(
            &[(
                0xC100,
                &[
                    0xA9, 0x02, 0x8D, 0x00, 0xA0, 0xAD, 0x00, 0x04, 0x91, 0x02, 0x4C, 0x00, 0x84,
                ],
            )],
            0xC100,
        );
        let bounds = synthetic_destination_bounds(
            (FIXED_PRG_BANK, 0xC108, 0x02),
            vec![0x0781..=0x07A5, 0x7953..=0x79F2],
        );

        let trace = bind_reset_bank_entries(&source, 0xC100, &bounds).unwrap();

        assert_eq!(trace.switchable_roots(), &BTreeSet::from([(0x02, 0x8400)]));
        assert!(
            trace
                .open_fact_descriptions()
                .iter()
                .all(|fact| !fact.contains("effective_write"))
        );
    }

    #[test]
    fn source_bound_destination_clobbers_only_intersecting_tracked_state() {
        let instruction = decode_bytes(&[0x91, 0x02]).unwrap();
        let mut state = ResetTraceState::at(0xC100);
        state.pointer_low_00 = Some(0x34);
        state.pointer_high_01 = None;
        state.outer_screen_state_24 = Some(0x04);
        state.scheduler_state_25 = Some(0x05);
        state.prg_bank_shadow_29 = Some(0x06);
        state.far_selector_44 = Some(0x07);
        state.main_state_84 = Some(0x0F);
        state.state_057a = Some(0x08);
        state.sound_test_state_05ee = Some(0x09);
        state.mapped_prg_bank = Some(0x06);
        let bounds =
            synthetic_destination_bounds((FIXED_PRG_BANK, 0xC100, 0x02), vec![0x0025..=0x0025]);
        let mut open_facts = BTreeSet::new();

        apply_data_effect(
            &instruction,
            &mut state,
            FIXED_PRG_BANK,
            &bounds,
            &mut open_facts,
        )
        .unwrap();

        assert_eq!(state.pointer_low_00, Some(0x34));
        assert_eq!(state.outer_screen_state_24, Some(0x04));
        assert_eq!(state.scheduler_state_25, None);
        assert_eq!(state.prg_bank_shadow_29, Some(0x06));
        assert_eq!(state.far_selector_44, Some(0x07));
        assert_eq!(state.main_state_84, Some(0x0F));
        assert_eq!(state.state_057a, Some(0x08));
        assert_eq!(state.sound_test_state_05ee, Some(0x09));
        assert_eq!(state.mapped_prg_bank, Some(0x06));
        assert!(open_facts.is_empty());
    }

    #[test]
    fn unbound_unknown_indirect_write_remains_open_and_clobbers_bank_state() {
        let instruction = decode_bytes(&[0x91, 0x02]).unwrap();
        let mut state = ResetTraceState::at(0xC100);
        state.pointer_low_00 = Some(0x34);
        state.pointer_high_01 = None;
        state.scheduler_state_25 = Some(0x05);
        state.mapped_prg_bank = Some(0x06);
        let mut open_facts = BTreeSet::new();

        apply_data_effect(
            &instruction,
            &mut state,
            FIXED_PRG_BANK,
            &BTreeMap::new(),
            &mut open_facts,
        )
        .unwrap();

        assert_eq!(state.pointer_low_00, None);
        assert_eq!(state.scheduler_state_25, None);
        assert_eq!(state.mapped_prg_bank, None);
        assert!(
            open_facts
                .iter()
                .any(|fact| fact.contains("indirect_target_unknown"))
        );
    }

    #[test]
    fn undocumented_target_remains_open_and_is_not_admitted_as_executable() {
        let source = synthetic_source(&[(0xC100, &[0x4C, 0x10, 0xC1]), (0xC110, &[0xFF])], 0xC100);

        let trace = bind_reset_bank_entries(&source, 0xC100, &BTreeMap::new()).unwrap();

        assert!(
            trace
                .open_fact_descriptions()
                .iter()
                .any(|fact| fact == "undocumented_opcode@0F:C110")
        );
        assert!(
            !trace
                .reachable_instruction_starts()
                .contains(&(FIXED_PRG_BANK, 0xC110))
        );
    }

    #[test]
    fn scheduler_state_is_part_of_the_trace_location() {
        let mut state_zero = ResetTraceState::at(0xC100);
        state_zero.mapped_prg_bank = Some(0x06);
        state_zero.scheduler_state_25 = Some(0x00);
        let mut state_five = state_zero.clone();
        state_five.scheduler_state_25 = Some(0x05);

        assert_ne!(state_zero.location(), state_five.location());
    }

    #[test]
    fn outer_screen_state_is_part_of_the_trace_location() {
        let mut state_save_offer = ResetTraceState::at(0x8400);
        state_save_offer.mapped_prg_bank = Some(0x06);
        state_save_offer.outer_screen_state_24 = Some(0x0D);
        state_save_offer.scheduler_state_25 = Some(0x05);
        let mut state_save_complete = state_save_offer.clone();
        state_save_complete.outer_screen_state_24 = Some(0x0E);

        assert_ne!(state_save_offer.location(), state_save_complete.location());
    }

    #[test]
    fn main_state_is_part_of_the_trace_location() {
        let mut state_input = ResetTraceState::at(0x849D);
        state_input.mapped_prg_bank = Some(0x06);
        state_input.outer_screen_state_24 = Some(0x02);
        state_input.scheduler_state_25 = Some(0x05);
        state_input.main_state_84 = Some(0x00);
        let mut state_transition = state_input.clone();
        state_transition.main_state_84 = Some(0x01);

        assert_ne!(state_input.location(), state_transition.location());
    }

    #[test]
    fn owned_inline_domain_records_each_selector_in_the_actual_entry_bank() {
        let source = synthetic_source(
            &[
                (
                    0xC100,
                    &[0xAD, 0x00, 0x04, 0x20, 0x4C, 0xC3, 0x10, 0xC1, 0x20, 0xC1],
                ),
                (0xC110, &[0x60]),
                (0xC120, &[0x60]),
                (0xC130, &[0x60]),
                (
                    INLINE_POINTER_DISPATCH_ADDRESS,
                    &INLINE_POINTER_DISPATCH_CODE,
                ),
            ],
            0xC100,
        );
        let owned_domains =
            BTreeMap::from([((FIXED_PRG_BANK, 0xC103), BTreeSet::from([0x00, 0x01]))]);

        let trace = trace_fixed_scheduler_contexts(
            &source,
            0xC100,
            0xC130,
            [(0x05, 0x06)],
            &owned_domains,
            &BTreeMap::new(),
        )
        .unwrap();

        assert_eq!(
            trace.inline_dispatch_contexts(FIXED_PRG_BANK, 0xC103),
            BTreeSet::from([(0x00, 0x06), (0x01, 0x06)])
        );
        assert!(
            trace
                .reachable_instruction_starts()
                .contains(&(FIXED_PRG_BANK, 0xC110))
        );
        assert!(
            trace
                .reachable_instruction_starts()
                .contains(&(FIXED_PRG_BANK, 0xC120))
        );
    }
}
