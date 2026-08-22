use anyhow::{Context, Result, ensure};

use crate::{rom::Rom, rp2a03::Instruction};

use super::{
    CANONICAL_PRG_BANK_SHADOW, NMI_ENTRY_CONTINUATION_ADDRESS, NMI_EXIT_TRAMPOLINE_ADDRESS,
    SELECT_REGISTER_ROUTINE_ADDRESS, SELECTED_REGISTER_SHADOW, SOURCE_NMI_DISPLACED_CALL,
    SOURCE_NMI_FIRST_CALL, SOURCE_NMI_SECOND_CALL, SOURCE_NMI_STACK_EXTENSION,
    SOURCE_NMI_UNIVERSAL_EPILOGUE, SOURCE_PRG_SHADOW_READER, bind_fixed_instructions, fixed_bytes,
    select_register_instruction,
};

pub(super) fn verify_installed_contract(candidate: &Rom) -> Result<()> {
    verify_installed_contract_body(candidate)
}

pub(super) fn verify_final_installed_contract(candidate: &Rom) -> Result<()> {
    verify_installed_contract_body(candidate)
}

fn verify_installed_contract_body(candidate: &Rom) -> Result<()> {
    bind_fixed_instructions(
        candidate,
        super::super::RESET_INITIALIZER_ADDRESS,
        &[
            Instruction::LdaImmediate(0),
            Instruction::StaAbsolute(0xE000),
            Instruction::LdaImmediate(0x80),
            Instruction::StaAbsolute(0xA001),
            Instruction::LdaImmediate(0),
            Instruction::JsrAbsolute(super::super::SELECT_PRG_BANK_ADDRESS),
            Instruction::JsrAbsolute(super::super::SELECT_LEFT_FD_CHR_BANK_ADDRESS),
            Instruction::JsrAbsolute(super::super::SELECT_LEFT_FE_CHR_BANK_ADDRESS),
            Instruction::JsrAbsolute(super::super::SELECT_RIGHT_FD_CHR_BANK_ADDRESS),
            Instruction::JsrAbsolute(super::super::SELECT_RIGHT_FE_CHR_BANK_ADDRESS),
            Instruction::JmpAbsolute(super::super::SOURCE_RESET_ADDRESS),
        ],
        "reset path through the first selected-register shadow writer",
    )?;
    ensure!(
        fixed_bytes(candidate, 0xFFFC, 2)? == super::super::RESET_INITIALIZER_ADDRESS.to_le_bytes(),
        "reset vector no longer reaches the selector initialization path"
    );
    bind_fixed_instructions(
        candidate,
        SELECT_REGISTER_ROUTINE_ADDRESS,
        &[
            Instruction::StaZeroPage(SELECTED_REGISTER_SHADOW),
            Instruction::StaAbsolute(0x8000),
            Instruction::Rts,
        ],
        "selected-register writer",
    )?;
    bind_fixed_instructions(
        candidate,
        NMI_ENTRY_CONTINUATION_ADDRESS,
        &[
            Instruction::LdaZeroPage(0x00),
            Instruction::Pha,
            Instruction::LdaZeroPage(0x01),
            Instruction::Pha,
            Instruction::JmpAbsolute(SOURCE_NMI_FIRST_CALL),
        ],
        "NMI zero-page save continuation",
    )?;
    bind_fixed_instructions(
        candidate,
        NMI_EXIT_TRAMPOLINE_ADDRESS,
        &[
            Instruction::Pla,
            select_register_instruction(),
            Instruction::Pla,
            Instruction::Tay,
            Instruction::Pla,
            Instruction::JmpAbsolute(0xC1C0),
        ],
        "NMI selected-register restore trampoline",
    )?;
    bind_fixed_instructions(
        candidate,
        SOURCE_NMI_STACK_EXTENSION,
        &[
            Instruction::LdaZeroPage(SELECTED_REGISTER_SHADOW),
            Instruction::Pha,
            Instruction::JmpAbsolute(NMI_ENTRY_CONTINUATION_ADDRESS),
        ],
        "NMI selected-register stack extension",
    )?;
    bind_fixed_instructions(
        candidate,
        SOURCE_NMI_FIRST_CALL,
        &[
            Instruction::JsrAbsolute(SOURCE_NMI_DISPLACED_CALL),
            Instruction::JsrAbsolute(SOURCE_NMI_SECOND_CALL),
        ],
        "preserved original NMI calls",
    )?;
    bind_fixed_instructions(
        candidate,
        SOURCE_NMI_UNIVERSAL_EPILOGUE,
        &[Instruction::JmpAbsolute(NMI_EXIT_TRAMPOLINE_ADDRESS)],
        "NMI selected-register restore hook",
    )?;
    bind_fixed_instructions(
        candidate,
        SOURCE_PRG_SHADOW_READER,
        &[Instruction::LdaZeroPage(CANONICAL_PRG_BANK_SHADOW)],
        "canonical PRG-bank shadow reader",
    )?;
    bind_fixed_instructions(
        candidate,
        super::super::SOURCE_SELECT_PRG_BANK_AND_SAVE_ADDRESS,
        &[
            Instruction::StaZeroPage(CANONICAL_PRG_BANK_SHADOW),
            Instruction::Nop,
            Instruction::Nop,
            Instruction::JsrAbsolute(super::super::SELECT_PRG_BANK_ADDRESS),
            Instruction::Rts,
        ],
        "canonical PRG-bank shadow writer",
    )?;
    super::nmi_protocol::bind_nmi_caller_routes_to_universal_epilogue(
        candidate,
        &[
            (NMI_ENTRY_CONTINUATION_ADDRESS, 0xFA7F),
            (NMI_EXIT_TRAMPOLINE_ADDRESS, 0xFAA0),
        ],
    )?;
    verify_active_fixed_bank_nonindexed_absolute_mapper_select_store(candidate)
}

pub(super) fn verify_parity_nonindexed_absolute_mapper_select_store(candidate: &Rom) -> Result<()> {
    let candidates = nonindexed_absolute_mapper_select_store_offsets(candidate.prg());
    let expected =
        candidate.prg().len() - 0x4000 + usize::from(SELECT_REGISTER_ROUTINE_ADDRESS - 0xC000) + 2;
    ensure!(
        candidates == [expected],
        "parity PRG non-indexed absolute mapper-select store census must contain only the common writer; found PRG offsets {candidates:06X?}"
    );
    Ok(())
}

pub(super) fn verify_active_fixed_bank_nonindexed_absolute_mapper_select_store(
    candidate: &Rom,
) -> Result<()> {
    let active_fixed_start = candidate
        .prg()
        .len()
        .checked_sub(0x4000)
        .context("PRG is smaller than the active fixed bank")?;
    let candidates =
        nonindexed_absolute_mapper_select_store_offsets(&candidate.prg()[active_fixed_start..]);
    let expected = usize::from(SELECT_REGISTER_ROUTINE_ADDRESS - 0xC000) + 2;
    ensure!(
        candidates == [expected],
        "active fixed-bank non-indexed absolute mapper-select store census must contain only the common writer; found fixed-bank offsets {candidates:04X?}"
    );
    Ok(())
}

fn nonindexed_absolute_mapper_select_store_offsets(bytes: &[u8]) -> Vec<usize> {
    bytes
        .windows(3)
        .enumerate()
        .filter(|(_, bytes)| matches!(bytes[0], 0x8C..=0x8E) && bytes[1..] == [0x00, 0x80])
        .map(|(offset, _)| offset)
        .collect()
}
