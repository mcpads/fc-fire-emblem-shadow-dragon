use std::collections::{BTreeMap, BTreeSet, VecDeque};

use anyhow::{Context, Result, ensure};
use retro_rp2a03::{
    AddressingMode, Location, MemoryAddress, Mnemonic, Operand, RegisterLocation, Rp2A03,
    decode_bytes,
};
use typed_isa_core::{AccessKind, StaticSemantics};

use crate::{
    rom::Rom,
    typed_source::{Rp2a03DirectControlFlow, decode_rp2a03_sequence, rp2a03_direct_control_flow},
};

use super::selector_transition_graph::{StateTransition, reachable_selectors};

const SOURCE_PRG_BANK_BYTE_COUNT: usize = 16 * 1024;
const FIXED_PRG_BANK: u8 = 0x0F;

#[derive(Clone, Copy)]
pub(super) enum StateWriteStep {
    Increment {
        address: u16,
    },
    Decrement {
        address: u16,
    },
    StoreConstant {
        load_address: u16,
        store_address: u16,
        value: u8,
    },
    StoreAccumulatorConstant {
        load_address: u16,
        store_address: u16,
        value: u8,
    },
    StoreIndexYConstant {
        load_address: u16,
        store_address: u16,
        value: u8,
    },
}

impl StateWriteStep {
    pub(super) const fn increment(address: u16) -> Self {
        Self::Increment { address }
    }

    pub(super) const fn decrement(address: u16) -> Self {
        Self::Decrement { address }
    }

    pub(super) const fn store_constant(load_address: u16, store_address: u16, value: u8) -> Self {
        Self::StoreConstant {
            load_address,
            store_address,
            value,
        }
    }

    pub(super) const fn store_accumulator_constant(
        load_address: u16,
        store_address: u16,
        value: u8,
    ) -> Self {
        Self::StoreAccumulatorConstant {
            load_address,
            store_address,
            value,
        }
    }

    pub(super) const fn store_index_y_constant(
        load_address: u16,
        store_address: u16,
        value: u8,
    ) -> Self {
        Self::StoreIndexYConstant {
            load_address,
            store_address,
            value,
        }
    }

    fn writer_address(self) -> u16 {
        match self {
            Self::Increment { address } | Self::Decrement { address } => address,
            Self::StoreConstant { store_address, .. }
            | Self::StoreAccumulatorConstant { store_address, .. }
            | Self::StoreIndexYConstant { store_address, .. } => store_address,
        }
    }

    fn evidence_start(self) -> u16 {
        match self {
            Self::Increment { address } | Self::Decrement { address } => address,
            Self::StoreConstant { load_address, .. }
            | Self::StoreAccumulatorConstant { load_address, .. }
            | Self::StoreIndexYConstant { load_address, .. } => load_address,
        }
    }

    fn next_address(self) -> u16 {
        match self {
            Self::Increment { address } | Self::Decrement { address } => address + 3,
            Self::StoreConstant { store_address, .. }
            | Self::StoreAccumulatorConstant { store_address, .. }
            | Self::StoreIndexYConstant { store_address, .. } => store_address + 3,
        }
    }

    fn apply(self, state: u8) -> u8 {
        match self {
            Self::Increment { .. } => state.wrapping_add(1),
            Self::Decrement { .. } => state.wrapping_sub(1),
            Self::StoreConstant { value, .. }
            | Self::StoreAccumulatorConstant { value, .. }
            | Self::StoreIndexYConstant { value, .. } => value,
        }
    }
}

#[derive(Clone, Copy)]
pub(super) struct TransitionPath {
    selector: u8,
    handler: u16,
    steps: &'static [StateWriteStep],
}

impl TransitionPath {
    pub(super) const fn new(selector: u8, handler: u16, steps: &'static [StateWriteStep]) -> Self {
        Self {
            selector,
            handler,
            steps,
        }
    }
}

pub(super) fn bind_state_transition_closure(
    source: &Rom,
    bank: u8,
    state_address: u16,
    handler_domain: &BTreeSet<u8>,
    handler_target: impl Fn(u8) -> Option<u16>,
    initial: impl IntoIterator<Item = u8>,
    paths: &[TransitionPath],
    role: &str,
) -> Result<BTreeSet<u8>> {
    ensure!(!role.is_empty(), "state-transition evidence role is empty");
    let writer_sites = raw_state_writer_sites(source, bank, state_address)?;
    let mut paths_by_selector = BTreeMap::<u8, Vec<&TransitionPath>>::new();
    for path in paths {
        ensure!(
            handler_target(path.selector) == Some(path.handler),
            "{role} handler changed for selector {:02X}",
            path.selector
        );
        ensure!(
            !path.steps.is_empty(),
            "{role} transition has no state write"
        );
        bind_transition_path(source, bank, state_address, path, &writer_sites, role)?;
        paths_by_selector
            .entry(path.selector)
            .or_default()
            .push(path);
    }

    for (&selector, selector_paths) in &paths_by_selector {
        let handler =
            handler_target(selector).context("state-transition selector has no handler")?;
        let expected_writers = selector_paths
            .iter()
            .flat_map(|path| path.steps.iter().map(|step| step.writer_address()))
            .collect::<BTreeSet<_>>();
        let reachable_writers =
            collect_reachable_state_writers(source, bank, handler, &writer_sites, role)?;
        ensure!(
            reachable_writers == expected_writers,
            "{role} selector {selector:02X} writer ownership changed: reached {reachable_writers:04X?}, expected {expected_writers:04X?}"
        );
    }

    let transitions = paths.iter().map(|path| {
        let output = path
            .steps
            .iter()
            .fold(path.selector, |state, step| step.apply(state));
        StateTransition::new(path.selector, output)
    });
    reachable_selectors(role, handler_domain, initial, transitions)
}

fn bind_transition_path(
    source: &Rom,
    bank: u8,
    state_address: u16,
    path: &TransitionPath,
    writer_sites: &BTreeSet<u16>,
    role: &str,
) -> Result<()> {
    let mut cursor = path.handler;
    for &step in path.steps {
        ensure!(
            reaches_without_another_state_write(
                source,
                bank,
                cursor,
                step.evidence_start(),
                writer_sites,
                role,
            )?,
            "{role} handler ${:04X} no longer reaches state evidence at ${:04X}",
            path.handler,
            step.evidence_start()
        );
        match step {
            StateWriteStep::Increment { address } => ensure_instruction(
                source,
                bank,
                address,
                Mnemonic::Inc,
                AddressingMode::Absolute,
                Operand::Word(state_address),
                role,
            )?,
            StateWriteStep::Decrement { address } => ensure_instruction(
                source,
                bank,
                address,
                Mnemonic::Dec,
                AddressingMode::Absolute,
                Operand::Word(state_address),
                role,
            )?,
            StateWriteStep::StoreConstant {
                load_address,
                store_address,
                value,
            } => bind_constant_store(
                source,
                bank,
                load_address,
                store_address,
                state_address,
                value,
                role,
            )?,
            StateWriteStep::StoreAccumulatorConstant {
                load_address,
                store_address,
                value,
            } => bind_preserved_register_constant_store(
                source,
                bank,
                load_address,
                store_address,
                state_address,
                value,
                RegisterLocation::Accumulator,
                role,
            )?,
            StateWriteStep::StoreIndexYConstant {
                load_address,
                store_address,
                value,
            } => bind_preserved_register_constant_store(
                source,
                bank,
                load_address,
                store_address,
                state_address,
                value,
                RegisterLocation::Y,
                role,
            )?,
        }
        cursor = step.next_address();
    }
    Ok(())
}

fn reaches_without_another_state_write(
    source: &Rom,
    bank: u8,
    start: u16,
    target: u16,
    writer_sites: &BTreeSet<u16>,
    role: &str,
) -> Result<bool> {
    let mut pending = VecDeque::from([start]);
    let mut visited = BTreeSet::new();
    while let Some(address) = pending.pop_front() {
        if address == target {
            return Ok(true);
        }
        if !visited.insert(address) || writer_sites.contains(&address) {
            continue;
        }
        ensure!(visited.len() <= 512, "{role} path exceeded its bounded CFG");
        pending.extend(local_successors(source, bank, address, role)?);
    }
    Ok(false)
}

fn collect_reachable_state_writers(
    source: &Rom,
    bank: u8,
    start: u16,
    writer_sites: &BTreeSet<u16>,
    role: &str,
) -> Result<BTreeSet<u16>> {
    let mut pending = VecDeque::from([start]);
    let mut visited = BTreeSet::new();
    let mut writers = BTreeSet::new();
    while let Some(address) = pending.pop_front() {
        if !visited.insert(address) {
            continue;
        }
        ensure!(
            visited.len() <= 512,
            "{role} handler exceeded its bounded CFG"
        );
        if writer_sites.contains(&address) {
            writers.insert(address);
        }
        pending.extend(local_successors(source, bank, address, role)?);
    }
    Ok(writers)
}

fn local_successors(source: &Rom, bank: u8, address: u16, role: &str) -> Result<Vec<u16>> {
    if !(0x8000..0xC000).contains(&address) {
        return Ok(Vec::new());
    }
    let instruction = decode_bytes(source_bytes(source, bank, address, 3)?)
        .with_context(|| format!("decode {role} at {bank:02X}:${address:04X}"))?;
    ensure!(
        instruction.opcode_is_documented(),
        "{role} reached undocumented code at {bank:02X}:${address:04X}"
    );
    let local = |candidate: u16| (0x8000..0xC000).contains(&candidate).then_some(candidate);
    Ok(match rp2a03_direct_control_flow(&instruction, address)? {
        Rp2a03DirectControlFlow::FallThrough { next } => local(next).into_iter().collect(),
        Rp2a03DirectControlFlow::Branch {
            target,
            fallthrough,
        } => [local(target), fallthrough.and_then(local)]
            .into_iter()
            .flatten()
            .collect(),
        Rp2a03DirectControlFlow::Jump {
            target: Some(target),
        } => local(target).into_iter().collect(),
        Rp2a03DirectControlFlow::Call { return_address, .. } => {
            local(return_address).into_iter().collect()
        }
        Rp2a03DirectControlFlow::Jump { target: None }
        | Rp2a03DirectControlFlow::Return
        | Rp2a03DirectControlFlow::Interrupt
        | Rp2a03DirectControlFlow::Stop => Vec::new(),
    })
}

fn raw_state_writer_sites(source: &Rom, bank: u8, state_address: u16) -> Result<BTreeSet<u16>> {
    ensure!(
        bank < FIXED_PRG_BANK,
        "state-transition writer scan requires a switchable source bank"
    );
    let bytes = source
        .prg()
        .get(
            usize::from(bank) * SOURCE_PRG_BANK_BYTE_COUNT
                ..usize::from(bank + 1) * SOURCE_PRG_BANK_BYTE_COUNT,
        )
        .context("state-transition bank is outside source PRG")?;
    Ok(bytes
        .windows(3)
        .enumerate()
        .filter_map(|(offset, candidate)| {
            let address = 0x8000 + u16::try_from(offset).expect("bank offset fits u16");
            let instruction = decode_bytes(candidate).ok()?;
            if !instruction.opcode_is_documented() {
                return None;
            }
            Rp2A03::semantics(&instruction, &address)
                .expect("RP2A03 static semantics are infallible")
                .location_accesses
                .into_iter()
                .any(|access| {
                    access.kind == AccessKind::Write
                        && access.location == Location::Memory(MemoryAddress::Direct(state_address))
                })
                .then_some(address)
        })
        .collect())
}

#[allow(clippy::too_many_arguments)]
fn bind_preserved_register_constant_store(
    source: &Rom,
    bank: u8,
    load_address: u16,
    store_address: u16,
    state_address: u16,
    value: u8,
    register: RegisterLocation,
    role: &str,
) -> Result<()> {
    ensure!(
        load_address < store_address,
        "{role} constant register load no longer precedes its state store"
    );
    let (load_mnemonic, store_mnemonic) = match register {
        RegisterLocation::Accumulator => (Mnemonic::Lda, Mnemonic::Sta),
        RegisterLocation::X => (Mnemonic::Ldx, Mnemonic::Stx),
        RegisterLocation::Y => (Mnemonic::Ldy, Mnemonic::Sty),
        RegisterLocation::StackPointer => {
            anyhow::bail!("{role} cannot bind a stack-pointer constant store")
        }
    };
    ensure_instruction(
        source,
        bank,
        load_address,
        load_mnemonic,
        AddressingMode::Immediate,
        Operand::Byte(value),
        role,
    )?;
    let load = decode_bytes(source_bytes(source, bank, load_address, 3)?)
        .with_context(|| format!("decode {role} constant load"))?;
    let mut cursor = load_address + u16::try_from(load.encoded_len())?;
    let mut intervening_instruction_count = 0_u8;
    while cursor < store_address {
        ensure!(
            intervening_instruction_count < 16,
            "{role} constant register path exceeded its bound"
        );
        let instruction = decode_bytes(source_bytes(source, bank, cursor, 3)?)
            .with_context(|| format!("decode {role} at {bank:02X}:${cursor:04X}"))?;
        ensure!(
            instruction.opcode_is_documented(),
            "{role} constant register path reached undocumented code"
        );
        ensure!(
            matches!(
                rp2a03_direct_control_flow(&instruction, cursor)?,
                Rp2a03DirectControlFlow::FallThrough { .. }
            ),
            "{role} constant register path gained control flow before its store"
        );
        let register_was_overwritten = Rp2A03::semantics(&instruction, &cursor)
            .expect("RP2A03 static semantics are infallible")
            .location_accesses
            .into_iter()
            .any(|access| {
                access.kind == AccessKind::Write && access.location == Location::Register(register)
            });
        ensure!(
            !register_was_overwritten,
            "{role} constant register value is overwritten before its state store"
        );
        cursor = cursor
            .checked_add(u16::try_from(instruction.encoded_len())?)
            .context("constant register path address overflow")?;
        intervening_instruction_count += 1;
    }
    ensure!(
        cursor == store_address,
        "{role} state store is no longer aligned"
    );
    ensure_instruction(
        source,
        bank,
        store_address,
        store_mnemonic,
        AddressingMode::Absolute,
        Operand::Word(state_address),
        role,
    )
}

pub(super) fn bind_constant_store(
    source: &Rom,
    bank: u8,
    load_address: u16,
    store_address: u16,
    state_address: u16,
    value: u8,
    role: &str,
) -> Result<()> {
    ensure!(
        store_address == load_address + 2,
        "{role} no longer uses adjacent load and store instructions"
    );
    let [low, high] = state_address.to_le_bytes();
    let bytes = source_bytes(source, bank, load_address, 5)?;
    ensure!(
        bytes == [0xA9, value, 0x8D, low, high],
        "{role} source bytes changed"
    );
    decode_rp2a03_sequence(bytes, load_address, role)?;
    Ok(())
}

pub(super) fn ensure_instruction(
    source: &Rom,
    bank: u8,
    address: u16,
    mnemonic: Mnemonic,
    mode: AddressingMode,
    operand: Operand,
    role: &str,
) -> Result<()> {
    let instruction = decode_bytes(source_bytes(source, bank, address, 3)?)
        .with_context(|| format!("decode {role} at {bank:02X}:${address:04X}"))?;
    ensure!(
        instruction.mnemonic() == mnemonic
            && instruction.addressing_mode() == mode
            && instruction.operand() == operand,
        "{role} source instruction changed at {bank:02X}:${address:04X}"
    );
    Ok(())
}

pub(super) fn source_bytes(
    source: &Rom,
    selected_bank: u8,
    address: u16,
    byte_count: usize,
) -> Result<&[u8]> {
    ensure!(
        selected_bank < 0x10 && address >= 0x8000,
        "state-transition source address is outside PRG space"
    );
    let (physical_bank, relative) = if address >= 0xC000 {
        (FIXED_PRG_BANK, usize::from(address - 0xC000))
    } else {
        (selected_bank, usize::from(address - 0x8000))
    };
    let start = usize::from(physical_bank)
        .checked_mul(SOURCE_PRG_BANK_BYTE_COUNT)
        .and_then(|base| base.checked_add(relative))
        .context("state-transition source offset overflow")?;
    source
        .prg()
        .get(start..start + byte_count)
        .context("state-transition source range exceeds PRG")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{dialogue_runtime_state::MAIN_DIALOGUE_RUNTIME_STATE, rom::HEADER_SIZE};

    const TEST_BANK: u8 = 0;
    const TEST_STATE_ADDRESS: u16 = MAIN_DIALOGUE_RUNTIME_STATE.map_dialogue_outer_state_address;

    fn synthetic_source(programs: &[(u16, &[u8])]) -> Rom {
        let mut bytes = vec![0x60; HEADER_SIZE + 16 * SOURCE_PRG_BANK_BYTE_COUNT];
        bytes[..HEADER_SIZE].fill(0);
        bytes[..4].copy_from_slice(b"NES\x1A");
        bytes[4] = 16;
        bytes[6] = 0xA0;
        for &(address, program) in programs {
            let offset = HEADER_SIZE + usize::from(address - 0x8000);
            bytes[offset..offset + program.len()].copy_from_slice(program);
        }
        Rom::parse(bytes).unwrap()
    }

    #[test]
    fn preserved_constant_survives_unrelated_memory_writes() {
        const SET_ZERO: StateWriteStep =
            StateWriteStep::store_accumulator_constant(0x8000, 0x8004, 0);
        const PATHS: &[TransitionPath] = &[TransitionPath::new(0, 0x8000, &[SET_ZERO])];
        let source = synthetic_source(&[(
            0x8000,
            &[
                0xA9, 0x00, // LDA #0
                0x85, 0x20, // STA $20; does not replace A
                0x8D, 0xDB, 0x05, // STA $05DB
                0x60, // RTS
            ],
        )]);

        let produced = bind_state_transition_closure(
            &source,
            TEST_BANK,
            TEST_STATE_ADDRESS,
            &BTreeSet::from([0]),
            |selector| (selector == 0).then_some(0x8000),
            [0],
            PATHS,
            "test preserved constant",
        )
        .unwrap();

        assert_eq!(produced, BTreeSet::from([0]));
    }

    #[test]
    fn overwritten_constant_cannot_prove_a_state_transition() {
        const SET_ZERO: StateWriteStep =
            StateWriteStep::store_accumulator_constant(0x8000, 0x8004, 0);
        const PATHS: &[TransitionPath] = &[TransitionPath::new(0, 0x8000, &[SET_ZERO])];
        let source = synthetic_source(&[(
            0x8000,
            &[
                0xA9, 0x00, // LDA #0
                0xA9, 0x01, // LDA #1; replaces A
                0x8D, 0xDB, 0x05, // STA $05DB
                0x60, // RTS
            ],
        )]);

        let error = bind_state_transition_closure(
            &source,
            TEST_BANK,
            TEST_STATE_ADDRESS,
            &BTreeSet::from([0]),
            |selector| (selector == 0).then_some(0x8000),
            [0],
            PATHS,
            "test overwritten constant",
        )
        .unwrap_err();

        assert!(error.to_string().contains("overwritten"));
    }

    #[test]
    fn writer_census_uses_typed_write_semantics_across_mnemonics() {
        let source = synthetic_source(&[
            (0x8000, &[0xCE, 0xDB, 0x05, 0x60]), // DEC $05DB; RTS
            (0x8010, &[0x8C, 0xDB, 0x05, 0x60]), // STY $05DB; RTS
        ]);

        assert_eq!(
            raw_state_writer_sites(&source, TEST_BANK, TEST_STATE_ADDRESS).unwrap(),
            BTreeSet::from([0x8000, 0x8010])
        );
    }
}
