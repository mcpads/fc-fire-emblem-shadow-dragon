use std::collections::{BTreeMap, BTreeSet};

use anyhow::{Context, Result, ensure};
use retro_rp2a03::decode_bytes;

use crate::{
    mapper165::inline_pointer_dispatch::{
        INLINE_POINTER_DISPATCH_ADDRESS, bind_inline_pointer_dispatch,
    },
    rom::Rom,
    sha1_hex,
    typed_source::{Rp2a03DirectControlFlow, decode_rp2a03_sequence, rp2a03_direct_control_flow},
};

const SOURCE_PRG_BANK: u8 = 0x0D;
const FIXED_PRG_BANK: u8 = 0x0F;
const SOURCE_PRG_BANK_BYTE_COUNT: usize = 16 * 1024;
const SWITCHABLE_CPU_START: u16 = 0x8000;
const FIXED_CPU_START: u16 = 0xC000;

const TITLE_SEQUENCE_RESET_HANDLER: u16 = 0xA580;
const TITLE_SEQUENCE_RESET_HANDLER_BYTE_COUNT: usize = 0x54;
const TITLE_SEQUENCE_RESET_HANDLER_SHA1: &str = "bd7ec1b958fd47a270d204128c7e15bf982d65f1";
const TITLE_SEQUENCE_FRAME_AND_DISPATCH: u16 = 0xA5D4;
const TITLE_SEQUENCE_FRAME_AND_DISPATCH_BYTE_COUNT: usize = 0x44;
const TITLE_SEQUENCE_FRAME_AND_DISPATCH_SHA1: &str = "2e5eb2dc91a72759c8d448c0b89141a9fe4ab1ac";
const TITLE_STATE_DISPATCH_CALL: u16 = 0xA615;
const TITLE_STATE_HANDLER_POINTERS: u16 = 0xA618;
const TITLE_STATE_HANDLER_POINTER_BYTE_COUNT: usize = 0x16;
const TITLE_STATE_HANDLER_POINTER_SHA1: &str = "ef19cb138a959722d55cf8064857bf59f6987272";
const TITLE_PHASE_SCHEDULE: u16 = 0xA62E;
const TITLE_PHASE_SCHEDULE_BYTE_COUNT: usize = 0x24;
const TITLE_PHASE_SCHEDULE_SHA1: &str = "e6cdd46e092157d4217ae718261d3e79e09261f2";
const TITLE_INPUT_OVERRIDE: u16 = 0xA652;
const TITLE_INPUT_OVERRIDE_BYTE_COUNT: usize = 0x17;
const TITLE_INPUT_OVERRIDE_SHA1: &str = "a09f1c4ddc79cd8a843cf870743666645c370bdb";
const TITLE_INPUT_OVERRIDE_SELECTOR: u8 = 0x06;
const TITLE_SEQUENCE_RESET_SELECTOR: u8 = 0x04;
const TITLE_SEQUENCE_RESET_WRITES: [u8; 10] =
    [0xA9, 0x00, 0x85, 0x25, 0x8D, 0x7B, 0x05, 0x8D, 0x7C, 0x05];
const MAXIMUM_TITLE_HANDLER_INSTRUCTIONS: usize = 8_192;

#[derive(Clone, Debug)]
pub(crate) struct TitleStateExecution {
    dispatch_call: u16,
    selector_domain: BTreeSet<u8>,
    selector_targets: BTreeMap<u8, u16>,
    reachable_instruction_starts: BTreeSet<(u8, u16)>,
    open_control_facts: BTreeSet<String>,
}

impl TitleStateExecution {
    pub(crate) fn dispatch_call(&self) -> u16 {
        self.dispatch_call
    }

    pub(crate) fn selector_domain(&self) -> &BTreeSet<u8> {
        &self.selector_domain
    }

    pub(crate) fn selector_targets(&self) -> &BTreeMap<u8, u16> {
        &self.selector_targets
    }

    pub(crate) fn reachable_instruction_starts(&self) -> &BTreeSet<(u8, u16)> {
        &self.reachable_instruction_starts
    }

    pub(crate) fn open_control_fact_descriptions(&self) -> Vec<String> {
        self.open_control_facts.iter().cloned().collect()
    }
}

pub(crate) fn bind_title_state_execution(source: &Rom) -> Result<TitleStateExecution> {
    source.verify_supported_japanese()?;
    let reset_handler = bind_source_region(
        source,
        TITLE_SEQUENCE_RESET_HANDLER,
        TITLE_SEQUENCE_RESET_HANDLER_BYTE_COUNT,
        TITLE_SEQUENCE_RESET_HANDLER_SHA1,
        "title sequence reset handler",
    )?;
    decode_rp2a03_sequence(
        reset_handler,
        TITLE_SEQUENCE_RESET_HANDLER,
        "title sequence reset handler",
    )?;
    ensure!(
        reset_handler
            .windows(TITLE_SEQUENCE_RESET_WRITES.len())
            .filter(|candidate| *candidate == TITLE_SEQUENCE_RESET_WRITES)
            .count()
            == 1,
        "title sequence reset handler no longer resets the scheduler and phase index together"
    );

    let frame = bind_source_region(
        source,
        TITLE_SEQUENCE_FRAME_AND_DISPATCH,
        TITLE_SEQUENCE_FRAME_AND_DISPATCH_BYTE_COUNT,
        TITLE_SEQUENCE_FRAME_AND_DISPATCH_SHA1,
        "title sequence frame and state dispatch",
    )?;
    decode_rp2a03_sequence(
        frame,
        TITLE_SEQUENCE_FRAME_AND_DISPATCH,
        "title sequence frame and state dispatch",
    )?;

    bind_source_region(
        source,
        TITLE_STATE_HANDLER_POINTERS,
        TITLE_STATE_HANDLER_POINTER_BYTE_COUNT,
        TITLE_STATE_HANDLER_POINTER_SHA1,
        "title state handler pointer table",
    )?;
    let phase_schedule = bind_source_region(
        source,
        TITLE_PHASE_SCHEDULE,
        TITLE_PHASE_SCHEDULE_BYTE_COUNT,
        TITLE_PHASE_SCHEDULE_SHA1,
        "title phase schedule",
    )?;
    let override_code = bind_source_region(
        source,
        TITLE_INPUT_OVERRIDE,
        TITLE_INPUT_OVERRIDE_BYTE_COUNT,
        TITLE_INPUT_OVERRIDE_SHA1,
        "title input state override",
    )?;
    decode_rp2a03_sequence(
        override_code,
        TITLE_INPUT_OVERRIDE,
        "title input state override",
    )?;

    let pointer_count = TITLE_STATE_HANDLER_POINTER_BYTE_COUNT / 2;
    let selector_domain = bind_phase_selector_domain(
        phase_schedule,
        TITLE_INPUT_OVERRIDE_SELECTOR,
        TITLE_SEQUENCE_RESET_SELECTOR,
        pointer_count,
    )?;
    let dispatch = bind_inline_pointer_dispatch(
        source,
        SOURCE_PRG_BANK,
        TITLE_STATE_DISPATCH_CALL,
        selector_domain.iter().copied(),
        "title sequence state dispatch",
    )?;
    ensure!(
        dispatch.table_start() == TITLE_STATE_HANDLER_POINTERS,
        "title state dispatch table boundary changed"
    );
    let selector_targets = selector_domain
        .iter()
        .copied()
        .zip(dispatch.targets_in_selector_order())
        .collect::<BTreeMap<_, _>>();
    ensure!(
        selector_targets
            .values()
            .all(|target| (SWITCHABLE_CPU_START..FIXED_CPU_START).contains(target)),
        "title state dispatch reaches RAM or a different PRG window"
    );
    ensure!(
        selector_targets.get(&TITLE_SEQUENCE_RESET_SELECTOR) == Some(&TITLE_SEQUENCE_RESET_HANDLER),
        "the terminal title phase no longer re-enters the phase-index reset handler"
    );

    let trace = trace_title_handler_graph(source, selector_targets.values().copied())?;
    Ok(TitleStateExecution {
        dispatch_call: TITLE_STATE_DISPATCH_CALL,
        selector_domain,
        selector_targets,
        reachable_instruction_starts: trace.reachable_instruction_starts,
        open_control_facts: trace.open_control_facts,
    })
}

fn bind_phase_selector_domain(
    phase_schedule: &[u8],
    input_override_selector: u8,
    reset_selector: u8,
    pointer_count: usize,
) -> Result<BTreeSet<u8>> {
    ensure!(
        pointer_count > 0 && pointer_count <= usize::from(u8::MAX) + 1,
        "title state handler table capacity is invalid"
    );
    let rows = phase_schedule.chunks_exact(2);
    ensure!(
        rows.remainder().is_empty(),
        "title phase schedule is truncated"
    );
    let rows = rows.collect::<Vec<_>>();
    ensure!(!rows.is_empty(), "title phase schedule is empty");
    ensure!(
        rows.last().map(|row| row[0]) == Some(reset_selector),
        "title phase schedule no longer terminates by resetting its phase index"
    );
    let mut domain = rows.iter().map(|row| row[0]).collect::<BTreeSet<_>>();
    domain.insert(input_override_selector);
    ensure!(
        domain
            .iter()
            .all(|selector| usize::from(*selector) < pointer_count),
        "title phase producer can select beyond its handler pointer table"
    );
    Ok(domain)
}

#[derive(Debug)]
struct TitleHandlerTrace {
    reachable_instruction_starts: BTreeSet<(u8, u16)>,
    open_control_facts: BTreeSet<String>,
}

fn trace_title_handler_graph(
    source: &Rom,
    roots: impl IntoIterator<Item = u16>,
) -> Result<TitleHandlerTrace> {
    let mut pending = roots.into_iter().collect::<Vec<_>>();
    let mut reachable_instruction_starts = BTreeSet::new();
    let mut fixed_roots = BTreeSet::new();
    let mut open_control_facts = BTreeSet::new();
    while let Some(address) = pending.pop() {
        let bank = if address >= FIXED_CPU_START {
            FIXED_PRG_BANK
        } else {
            SOURCE_PRG_BANK
        };
        ensure!(
            address >= SWITCHABLE_CPU_START,
            "title handler graph reached RAM at ${address:04X}"
        );
        if !reachable_instruction_starts.insert((bank, address)) {
            continue;
        }
        ensure!(
            reachable_instruction_starts.len() <= MAXIMUM_TITLE_HANDLER_INSTRUCTIONS,
            "title handler graph exceeded its instruction safety bound"
        );
        let instruction =
            decode_bytes(source_cpu_bytes(source, bank, address, 3)?).with_context(|| {
                format!("decode title handler instruction at {bank:02X}:${address:04X}")
            })?;
        if !instruction.opcode_is_documented() {
            open_control_facts.insert(format!(
                "title_handler_undocumented_opcode@{bank:02X}:{address:04X}"
            ));
            continue;
        }
        match rp2a03_direct_control_flow(&instruction, address)? {
            Rp2a03DirectControlFlow::FallThrough { next } => {
                enqueue_title_target(
                    next,
                    &mut pending,
                    &mut fixed_roots,
                    &mut open_control_facts,
                    address,
                );
            }
            Rp2a03DirectControlFlow::Branch {
                target,
                fallthrough,
            } => {
                enqueue_title_target(
                    target,
                    &mut pending,
                    &mut fixed_roots,
                    &mut open_control_facts,
                    address,
                );
                if let Some(fallthrough) = fallthrough {
                    enqueue_title_target(
                        fallthrough,
                        &mut pending,
                        &mut fixed_roots,
                        &mut open_control_facts,
                        address,
                    );
                }
            }
            Rp2a03DirectControlFlow::Call {
                target,
                return_address: _,
            } if target == INLINE_POINTER_DISPATCH_ADDRESS => {
                open_control_facts.insert(format!(
                    "title_handler_inline_dispatch@{bank:02X}:{address:04X}"
                ));
                // `$C34C` consumes this JSR frame and does not return to the inline data.
            }
            Rp2a03DirectControlFlow::Call {
                target,
                return_address,
            } => {
                enqueue_title_target(
                    return_address,
                    &mut pending,
                    &mut fixed_roots,
                    &mut open_control_facts,
                    address,
                );
                enqueue_title_target(
                    target,
                    &mut pending,
                    &mut fixed_roots,
                    &mut open_control_facts,
                    address,
                );
            }
            Rp2a03DirectControlFlow::Jump {
                target: Some(target),
            } => enqueue_title_target(
                target,
                &mut pending,
                &mut fixed_roots,
                &mut open_control_facts,
                address,
            ),
            Rp2a03DirectControlFlow::Jump { target: None } => {
                open_control_facts.insert(format!(
                    "title_handler_indirect_jump@{bank:02X}:{address:04X}"
                ));
            }
            Rp2a03DirectControlFlow::Return
            | Rp2a03DirectControlFlow::Interrupt
            | Rp2a03DirectControlFlow::Stop => {}
        }
    }
    reachable_instruction_starts.extend(fixed_roots);
    Ok(TitleHandlerTrace {
        reachable_instruction_starts,
        open_control_facts,
    })
}

fn enqueue_title_target(
    target: u16,
    pending: &mut Vec<u16>,
    fixed_roots: &mut BTreeSet<(u8, u16)>,
    open_control_facts: &mut BTreeSet<String>,
    source: u16,
) {
    if target < SWITCHABLE_CPU_START {
        open_control_facts.insert(format!(
            "title_handler_control@{source:04X}->${target:04X}:ram_target"
        ));
    } else if target >= FIXED_CPU_START {
        // Fixed-bank helpers are already rooted by the hardware-vector graph. Preserve the
        // direct edge as a root without recursively duplicating that graph here.
        fixed_roots.insert((FIXED_PRG_BANK, target));
    } else {
        pending.push(target);
    }
}

fn bind_source_region<'a>(
    source: &'a Rom,
    address: u16,
    byte_count: usize,
    expected_sha1: &str,
    role: &str,
) -> Result<&'a [u8]> {
    let bytes = source_cpu_bytes(source, SOURCE_PRG_BANK, address, byte_count)?;
    ensure!(sha1_hex(bytes) == expected_sha1, "source {role} changed");
    Ok(bytes)
}

fn source_cpu_bytes(source: &Rom, bank: u8, address: u16, byte_count: usize) -> Result<&[u8]> {
    ensure!(
        bank <= FIXED_PRG_BANK && address >= SWITCHABLE_CPU_START,
        "title source address is outside PRG space"
    );
    let physical_bank = if address >= FIXED_CPU_START {
        FIXED_PRG_BANK
    } else {
        bank
    };
    let cpu_start = if address >= FIXED_CPU_START {
        FIXED_CPU_START
    } else {
        SWITCHABLE_CPU_START
    };
    let offset = usize::from(physical_bank)
        .checked_mul(SOURCE_PRG_BANK_BYTE_COUNT)
        .and_then(|base| base.checked_add(usize::from(address - cpu_start)))
        .context("title source PRG offset overflow")?;
    source
        .prg()
        .get(offset..offset + byte_count)
        .with_context(|| {
            format!("title source range at {physical_bank:02X}:${address:04X} exceeds PRG")
        })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::rom::HEADER_SIZE;

    fn synthetic_title_code(address: u16, bytes_at_address: &[u8]) -> Rom {
        let mut bytes = vec![0x60; HEADER_SIZE + 16 * SOURCE_PRG_BANK_BYTE_COUNT];
        bytes[..HEADER_SIZE].fill(0);
        bytes[..4].copy_from_slice(b"NES\x1A");
        bytes[4] = 16;
        bytes[6] = 0xA0;
        let offset = HEADER_SIZE
            + usize::from(SOURCE_PRG_BANK) * SOURCE_PRG_BANK_BYTE_COUNT
            + usize::from(address - SWITCHABLE_CPU_START);
        bytes[offset..offset + bytes_at_address.len()].copy_from_slice(bytes_at_address);
        Rom::parse(bytes).unwrap()
    }

    #[test]
    fn phase_schedule_and_input_override_stay_inside_the_handler_table() {
        let domain = bind_phase_selector_domain(&[0, 1, 2, 3, 4, 1], 3, 4, 5).unwrap();

        assert_eq!(domain, BTreeSet::from([0, 2, 3, 4]));
    }

    #[test]
    fn phase_schedule_rejects_a_selector_without_a_handler() {
        let error = bind_phase_selector_domain(&[0, 1, 5, 1, 4, 1], 3, 4, 5).unwrap_err();

        assert!(error.to_string().contains("beyond its handler"));
    }

    #[test]
    fn terminal_phase_must_return_to_the_index_reset_handler() {
        let error = bind_phase_selector_domain(&[0, 1, 2, 1], 1, 4, 5).unwrap_err();

        assert!(error.to_string().contains("resetting its phase index"));
    }

    #[test]
    fn nested_inline_dispatch_stays_open_without_decoding_its_pointer_table_as_code() {
        let source = synthetic_title_code(0xA000, &[0x20, 0x4C, 0xC3, 0x02, 0x80]);

        let trace = trace_title_handler_graph(&source, [0xA000]).unwrap();

        assert!(
            trace
                .open_control_facts
                .contains("title_handler_inline_dispatch@0D:A000")
        );
        assert!(!trace.reachable_instruction_starts.contains(&(0x0D, 0xA003)));
    }

    #[test]
    fn an_unbound_indirect_jump_remains_an_open_control_fact() {
        let source = synthetic_title_code(0xA000, &[0x6C, 0x00, 0x00]);

        let trace = trace_title_handler_graph(&source, [0xA000]).unwrap();

        assert!(
            trace
                .open_control_facts
                .contains("title_handler_indirect_jump@0D:A000")
        );
    }
}
