use anyhow::{Result, ensure};
use serde::Serialize;

use crate::{
    mmc5_chr::switchable_bank_file_offset,
    mmc5_prg::fixed_bank_file_offset,
    mmc5_queue_runtime as queue_runtime,
    rom::Rom,
    rp2a03::{Instruction, assemble_at},
    tracked::TrackedImage,
};

use super::PRG_RAM_BANK_REGISTER;

pub(super) const WRAPPER_ADDRESS: u16 = 0xFD00;
const SOURCE_PRG_BANK: u8 = 0x0D;
const SOURCE_ADDRESS: u16 = 0x848E;

#[derive(Debug, Serialize)]
pub(super) struct DirectTransferBoundaryReport {
    role: &'static str,
    prg_bank: String,
    cpu_address: String,
    file_offset: String,
    wrapper_cpu_address: String,
    final_shadow_state: &'static str,
}

pub(super) fn validate_source(source_rom: &Rom) -> Result<()> {
    let source = source_entry()?;
    let offset = switchable_bank_file_offset(SOURCE_PRG_BANK, SOURCE_ADDRESS)?;
    ensure!(
        source_rom.data()[offset..offset + source.len()] == source,
        "direct nametable clear at {SOURCE_PRG_BANK:02X}:{SOURCE_ADDRESS:04X} changed"
    );
    Ok(())
}

pub(super) fn wrapper_end() -> Result<u16> {
    Ok(WRAPPER_ADDRESS + shadow_clear_wrapper()?.len() as u16)
}

pub(super) fn install_wrapper(image: &mut TrackedImage) -> Result<()> {
    let wrapper = shadow_clear_wrapper()?;
    image.write_expected(
        "MMC5 direct nametable clear wrapper",
        fixed_bank_file_offset(WRAPPER_ADDRESS)?,
        &vec![0xFF; wrapper.len()],
        &wrapper,
    )
}

pub(super) fn redirect_source_clear(image: &mut TrackedImage) -> Result<()> {
    image.write_expected(
        "redirect bank 0D direct nametable clear to batch shadow",
        switchable_bank_file_offset(SOURCE_PRG_BANK, SOURCE_ADDRESS)?,
        &source_entry()?,
        &redirect_entry()?,
    )
}

pub(super) fn report() -> Result<DirectTransferBoundaryReport> {
    Ok(DirectTransferBoundaryReport {
        role: "bank 0D physical nametable zero clear",
        prg_bank: format!("0x{SOURCE_PRG_BANK:02X}"),
        cpu_address: format!("0x{SOURCE_ADDRESS:04X}"),
        file_offset: format!(
            "0x{:06X}",
            switchable_bank_file_offset(SOURCE_PRG_BANK, SOURCE_ADDRESS)?
        ),
        wrapper_cpu_address: format!("0x{WRAPPER_ADDRESS:04X}"),
        final_shadow_state: "physical page 0 tiles $FF and attributes $00",
    })
}

fn source_entry() -> Result<Vec<u8>> {
    assemble_at(
        SOURCE_ADDRESS,
        &[
            Instruction::LdaZeroPage(0xCD),
            Instruction::AndImmediate(0xFB),
        ],
    )
}

fn redirect_entry() -> Result<Vec<u8>> {
    assemble_at(
        SOURCE_ADDRESS,
        &[Instruction::JsrAbsolute(WRAPPER_ADDRESS), Instruction::Nop],
    )
}

fn shadow_clear_wrapper() -> Result<Vec<u8>> {
    assemble_at(
        WRAPPER_ADDRESS,
        &[
            Instruction::Txa,
            Instruction::Pha,
            Instruction::Tya,
            Instruction::Pha,
            Instruction::LdaImmediate(1),
            Instruction::StaAbsolute(PRG_RAM_BANK_REGISTER),
            Instruction::JsrAbsolute(queue_runtime::CLEAR_PHYSICAL_NAMETABLE_ZERO_ADDRESS),
            Instruction::LdaImmediate(0),
            Instruction::StaAbsolute(PRG_RAM_BANK_REGISTER),
            Instruction::Pla,
            Instruction::Tay,
            Instruction::Pla,
            Instruction::Tax,
            Instruction::LdaZeroPage(0xCD),
            Instruction::AndImmediate(0xFB),
            Instruction::Rts,
        ],
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn redirect_preserves_the_source_entry_span_and_resume_values() {
        let source = source_entry().unwrap();
        let replacement = redirect_entry().unwrap();
        let wrapper = shadow_clear_wrapper().unwrap();

        assert_eq!(source, [0xA5, 0xCD, 0x29, 0xFB]);
        assert_eq!(source.len(), replacement.len());
        assert!(wrapper.windows(3).any(|bytes| bytes == [0x20, 0x80, 0x61]));
        assert!(wrapper.ends_with(&[0xA5, 0xCD, 0x29, 0xFB, 0x60]));
    }
}
