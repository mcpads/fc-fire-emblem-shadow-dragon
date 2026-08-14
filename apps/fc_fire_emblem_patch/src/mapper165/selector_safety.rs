mod installed_verification;
mod nmi_protocol;
mod shadow_operand_backstop;

#[cfg(test)]
mod tests;

use anyhow::{Context, Result, ensure};

use crate::{
    rom::Rom,
    rp2a03::{Instruction, assemble_at},
    tracked::TrackedImage,
};

/// The only routine allowed to write MMC3's selected-register port.
pub(crate) const SELECT_REGISTER_ROUTINE_ADDRESS: u16 = 0xFA58;
/// The selected MMC3 register, saved so an NMI can restore an interrupted pair write.
pub(crate) const SELECTED_REGISTER_SHADOW: u8 = 0x51;
/// Callee cost only; a caller's six-cycle JSR is accounted separately.
pub(crate) const SELECT_REGISTER_CALLEE_CYCLES: u32 = 13;

pub(super) const NMI_ENTRY_CONTINUATION_ADDRESS: u16 = 0xFA76;
pub(super) const NMI_EXIT_TRAMPOLINE_ADDRESS: u16 = 0xFA96;

pub(super) const SOURCE_NMI_VECTOR_ADDRESS: u16 = 0xFFFA;
pub(super) const SOURCE_NMI_ENTRY: u16 = 0xC163;
pub(super) const SOURCE_NMI_STACK_EXTENSION: u16 = 0xC173;
pub(crate) const SOURCE_NMI_FIRST_CALL: u16 = 0xC179;
pub(crate) const SOURCE_NMI_DISPLACED_CALL: u16 = 0xC3A5;
pub(crate) const SOURCE_NMI_SECOND_CALL: u16 = 0xC296;
pub(super) const SOURCE_NMI_SKIP_BRANCHES: u16 = 0xC19E;
pub(super) const SOURCE_NMI_UNIVERSAL_EPILOGUE: u16 = 0xC1BD;
pub(super) const SOURCE_NMI_END_EXCLUSIVE: u16 = 0xC1C4;
pub(super) const SOURCE_PRG_SHADOW_READER: u16 = 0xD385;
pub(super) const CANONICAL_PRG_BANK_SHADOW: u8 = 0x29;

pub(crate) const fn select_register_instruction() -> Instruction {
    Instruction::JsrAbsolute(SELECT_REGISTER_ROUTINE_ADDRESS)
}

pub(super) fn bind_source_contract(source: &Rom) -> Result<()> {
    nmi_protocol::bind_source_contract(source)?;
    shadow_operand_backstop::bind_source_contract(source)
}

pub(super) fn install_source_hooks(image: &mut TrackedImage) -> Result<()> {
    nmi_protocol::install_source_hooks(image)
}

pub(crate) fn verify_installed_contract(candidate: &Rom) -> Result<()> {
    installed_verification::verify_installed_contract(candidate)
}

/// Final full-translation images replace only the first preserved NMI call with their typed
/// trampoline. The selector stack boundary and the second source call remain identical.
pub(crate) fn verify_final_installed_contract(
    candidate: &Rom,
    first_nmi_call_target: u16,
) -> Result<()> {
    installed_verification::verify_final_installed_contract(candidate, first_nmi_call_target)
}

pub(crate) fn verify_parity_nonindexed_absolute_mapper_select_store(candidate: &Rom) -> Result<()> {
    installed_verification::verify_parity_nonindexed_absolute_mapper_select_store(candidate)
}

pub(crate) fn verify_active_fixed_bank_nonindexed_absolute_mapper_select_store(
    candidate: &Rom,
) -> Result<()> {
    installed_verification::verify_active_fixed_bank_nonindexed_absolute_mapper_select_store(
        candidate,
    )
}

pub(super) fn bind_fixed_instructions(
    rom: &Rom,
    address: u16,
    instructions: &[Instruction],
    role: &str,
) -> Result<()> {
    let expected = assemble_at(address, instructions)?;
    ensure!(
        fixed_bytes(rom, address, expected.len())? == expected,
        "{role} at ${address:04X} changed"
    );
    Ok(())
}

pub(super) fn fixed_bytes(rom: &Rom, address: u16, len: usize) -> Result<&[u8]> {
    let fixed_start = rom
        .prg()
        .len()
        .checked_sub(0x4000)
        .context("PRG is smaller than the fixed bank")?;
    let offset = fixed_start + usize::from(address - 0xC000);
    rom.prg()
        .get(offset..offset + len)
        .context("fixed-bank selector-safety region is outside the ROM")
}
