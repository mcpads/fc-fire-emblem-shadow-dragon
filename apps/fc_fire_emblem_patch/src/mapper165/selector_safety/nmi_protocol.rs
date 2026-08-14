use std::collections::{BTreeMap, BTreeSet, VecDeque};

use anyhow::{Context, Result, ensure};
use retro_rp2a03::decode_bytes;

use crate::{
    mmc5_prg::fixed_bank_file_offset,
    rom::Rom,
    rp2a03::{Instruction, assemble_at},
    tracked::TrackedImage,
    typed_source::{Rp2a03DirectControlFlow, rp2a03_direct_control_flow},
};

use super::{
    CANONICAL_PRG_BANK_SHADOW, NMI_ENTRY_CONTINUATION_ADDRESS, NMI_EXIT_TRAMPOLINE_ADDRESS,
    SELECTED_REGISTER_SHADOW, SOURCE_NMI_DISPLACED_CALL, SOURCE_NMI_END_EXCLUSIVE,
    SOURCE_NMI_ENTRY, SOURCE_NMI_SECOND_CALL, SOURCE_NMI_SKIP_BRANCHES, SOURCE_NMI_STACK_EXTENSION,
    SOURCE_NMI_UNIVERSAL_EPILOGUE, SOURCE_NMI_VECTOR_ADDRESS, SOURCE_PRG_SHADOW_READER,
    bind_fixed_instructions, fixed_bytes,
};

pub(super) fn bind_source_contract(source: &Rom) -> Result<()> {
    ensure!(
        fixed_bytes(source, SOURCE_NMI_VECTOR_ADDRESS, 2)? == SOURCE_NMI_ENTRY.to_le_bytes(),
        "source NMI vector changed"
    );
    bind_fixed_instructions(
        source,
        SOURCE_NMI_ENTRY,
        &[
            Instruction::Php,
            Instruction::Pha,
            Instruction::Txa,
            Instruction::Pha,
            Instruction::Tya,
            Instruction::Pha,
        ],
        "source NMI register prologue",
    )?;
    bind_fixed_instructions(
        source,
        SOURCE_NMI_STACK_EXTENSION,
        &[
            Instruction::LdaZeroPage(0x00),
            Instruction::Pha,
            Instruction::LdaZeroPage(0x01),
            Instruction::Pha,
            Instruction::JsrAbsolute(SOURCE_NMI_DISPLACED_CALL),
            Instruction::JsrAbsolute(SOURCE_NMI_SECOND_CALL),
        ],
        "source NMI zero-page save and first two calls",
    )?;
    bind_fixed_instructions(
        source,
        SOURCE_NMI_SKIP_BRANCHES,
        &[
            Instruction::LdaZeroPage(0xD0),
            Instruction::BeqAbsolute(SOURCE_NMI_UNIVERSAL_EPILOGUE),
            Instruction::LdaAbsolute(0x047B),
            Instruction::BeqAbsolute(SOURCE_NMI_UNIVERSAL_EPILOGUE),
        ],
        "source NMI universal-epilogue branches",
    )?;
    bind_fixed_instructions(
        source,
        SOURCE_NMI_UNIVERSAL_EPILOGUE,
        &[
            Instruction::Pla,
            Instruction::Tay,
            Instruction::Pla,
            Instruction::Tax,
            Instruction::Pla,
            Instruction::Plp,
            Instruction::Rti,
        ],
        "source NMI universal epilogue",
    )?;
    bind_fixed_instructions(
        source,
        SOURCE_PRG_SHADOW_READER,
        &[
            Instruction::LdaZeroPage(SELECTED_REGISTER_SHADOW),
            Instruction::StaZeroPage(0x08),
        ],
        "source duplicate PRG-bank shadow reader",
    )?;
    bind_fixed_instructions(
        source,
        0xD3F0,
        &[
            Instruction::LdaZeroPage(0x08),
            Instruction::JmpAbsolute(0xC9A6),
        ],
        "source duplicate PRG-bank shadow restore tail",
    )?;
    bind_fixed_instructions(
        source,
        super::super::SOURCE_SELECT_PRG_BANK_AND_SAVE_ADDRESS,
        &[
            Instruction::StaZeroPage(CANONICAL_PRG_BANK_SHADOW),
            Instruction::StaZeroPage(SELECTED_REGISTER_SHADOW),
            Instruction::StaAbsolute(0xA000),
            Instruction::Rts,
        ],
        "source duplicate PRG-bank shadow writer",
    )?;
    bind_nmi_caller_routes_to_universal_epilogue(source, &[])
}

pub(super) fn install_source_hooks(image: &mut TrackedImage) -> Result<()> {
    replace_fixed_instructions(
        image,
        "extend the NMI stack with the selected MMC3 register",
        SOURCE_NMI_STACK_EXTENSION,
        &[
            Instruction::LdaZeroPage(0x00),
            Instruction::Pha,
            Instruction::LdaZeroPage(0x01),
            Instruction::Pha,
        ],
        &[
            Instruction::LdaZeroPage(SELECTED_REGISTER_SHADOW),
            Instruction::Pha,
            Instruction::JmpAbsolute(NMI_ENTRY_CONTINUATION_ADDRESS),
        ],
    )?;
    replace_fixed_instructions(
        image,
        "restore selected MMC3 register at universal NMI exit",
        SOURCE_NMI_UNIVERSAL_EPILOGUE,
        &[Instruction::Pla, Instruction::Tay, Instruction::Pla],
        &[Instruction::JmpAbsolute(NMI_EXIT_TRAMPOLINE_ADDRESS)],
    )?;
    replace_fixed_instructions(
        image,
        "migrate duplicate PRG-bank shadow reader to canonical $29",
        SOURCE_PRG_SHADOW_READER,
        &[Instruction::LdaZeroPage(SELECTED_REGISTER_SHADOW)],
        &[Instruction::LdaZeroPage(CANONICAL_PRG_BANK_SHADOW)],
    )
}

/// Follows the NMI caller's local edges while treating each JSR as returning to its encoded
/// continuation. Callee termination remains a separate source/runtime contract.
pub(super) fn bind_nmi_caller_routes_to_universal_epilogue(
    rom: &Rom,
    additional_ranges: &[(u16, u16)],
) -> Result<()> {
    let mut allowed_ranges = vec![(SOURCE_NMI_ENTRY, SOURCE_NMI_END_EXCLUSIVE)];
    allowed_ranges.extend_from_slice(additional_ranges);
    let mut pending = VecDeque::from([SOURCE_NMI_ENTRY]);
    let mut visited = BTreeSet::new();
    let mut edges = BTreeMap::<u16, BTreeSet<u16>>::new();
    let mut interrupt_exits = BTreeSet::new();

    while let Some(address) = pending.pop_front() {
        if !visited.insert(address) {
            continue;
        }
        ensure!(
            allowed_ranges
                .iter()
                .any(|&(start, end)| (start..end).contains(&address)),
            "NMI caller-local control flow escaped the source-bound ranges at ${address:04X}"
        );
        let bytes = fixed_bytes(rom, address, 3)?;
        let instruction = decode_bytes(bytes)
            .with_context(|| format!("decode NMI local instruction at ${address:04X}"))?;
        ensure!(
            instruction.opcode_is_documented(),
            "NMI caller-local control flow reached undocumented opcode at ${address:04X}"
        );
        let successors = match rp2a03_direct_control_flow(&instruction, address)? {
            Rp2a03DirectControlFlow::FallThrough { next } => vec![next],
            Rp2a03DirectControlFlow::Branch {
                target,
                fallthrough: Some(fallthrough),
            } => vec![target, fallthrough],
            Rp2a03DirectControlFlow::Call { return_address, .. } => vec![return_address],
            Rp2a03DirectControlFlow::Jump {
                target: Some(target),
            } => vec![target],
            Rp2a03DirectControlFlow::Interrupt => {
                interrupt_exits.insert(address);
                Vec::new()
            }
            Rp2a03DirectControlFlow::Branch {
                fallthrough: None, ..
            }
            | Rp2a03DirectControlFlow::Jump { target: None }
            | Rp2a03DirectControlFlow::Return
            | Rp2a03DirectControlFlow::Stop => {
                anyhow::bail!(
                    "NMI caller-local control flow has an unbound terminal or indirect edge at ${address:04X}"
                )
            }
        };
        for successor in &successors {
            pending.push_back(*successor);
        }
        edges.insert(address, successors.into_iter().collect());
    }

    ensure!(
        interrupt_exits == BTreeSet::from([SOURCE_NMI_END_EXCLUSIVE - 1]),
        "NMI caller-local control flow no longer has exactly the source RTI exit: {interrupt_exits:04X?}"
    );
    ensure!(
        visited.contains(&SOURCE_NMI_UNIVERSAL_EPILOGUE),
        "NMI universal epilogue is no longer reachable"
    );

    let mut can_reach_exit = interrupt_exits.clone();
    loop {
        let before = can_reach_exit.len();
        for (address, successors) in &edges {
            if successors
                .iter()
                .any(|successor| can_reach_exit.contains(successor))
            {
                can_reach_exit.insert(*address);
            }
        }
        if can_reach_exit.len() == before {
            break;
        }
    }
    ensure!(
        visited.is_subset(&can_reach_exit),
        "an NMI caller-local route cannot reach the source RTI under the returning-callee premise: {:?}",
        visited.difference(&can_reach_exit).collect::<Vec<_>>()
    );

    let mut bypass_pending = VecDeque::from([SOURCE_NMI_ENTRY]);
    let mut bypass_visited = BTreeSet::new();
    while let Some(address) = bypass_pending.pop_front() {
        if address == SOURCE_NMI_UNIVERSAL_EPILOGUE || !bypass_visited.insert(address) {
            continue;
        }
        ensure!(
            !interrupt_exits.contains(&address),
            "an NMI route reaches RTI without passing the universal epilogue"
        );
        if let Some(successors) = edges.get(&address) {
            bypass_pending.extend(successors);
        }
    }
    Ok(())
}

fn replace_fixed_instructions(
    image: &mut TrackedImage,
    role: &str,
    address: u16,
    expected: &[Instruction],
    replacement: &[Instruction],
) -> Result<()> {
    let expected = assemble_at(address, expected)?;
    let replacement = assemble_at(address, replacement)?;
    ensure!(
        expected.len() == replacement.len(),
        "{role} changes the source instruction footprint"
    );
    image.write_expected(
        role,
        fixed_bank_file_offset(address)?,
        &expected,
        &replacement,
    )
}
