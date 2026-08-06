use anyhow::{Result, ensure};

use crate::{
    mmc5_chr::switchable_bank_file_offset,
    mmc5_prg::fixed_bank_file_offset,
    rp2a03::{Instruction, assemble_at},
    tracked::TrackedImage,
};

use super::{
    CODE_CAVE_LEN, CODE_CAVE_START_ADDRESS, RESET_INITIALIZER_ADDRESS,
    SELECT_LEFT_FD_CHR_BANK_ADDRESS, SELECT_LEFT_FE_CHR_BANK_ADDRESS, SELECT_PRG_BANK_ADDRESS,
    SELECT_RIGHT_FD_CHR_BANK_ADDRESS, SELECT_RIGHT_FE_CHR_BANK_ADDRESS, SOURCE_RESET_ADDRESS,
    SOURCE_SELECT_PRG_BANK_AND_SAVE_ADDRESS,
    writer_sites::{CentralChrWriter, DirectWriter, WriterLocation},
};

#[derive(Debug)]
pub(super) struct AssembledRoutine {
    pub(super) role: &'static str,
    pub(super) cpu_address: u16,
    pub(super) bytes: Vec<u8>,
}

pub(super) fn build_routines() -> Result<Vec<AssembledRoutine>> {
    Ok(vec![
        assemble_routine(
            "reset initialization",
            RESET_INITIALIZER_ADDRESS,
            &[
                Instruction::LdaImmediate(0),
                Instruction::StaAbsolute(0xE000),
                Instruction::LdaImmediate(0x80),
                Instruction::StaAbsolute(0xA001),
                Instruction::LdaImmediate(0),
                Instruction::JsrAbsolute(SELECT_PRG_BANK_ADDRESS),
                Instruction::JsrAbsolute(SELECT_LEFT_FD_CHR_BANK_ADDRESS),
                Instruction::JsrAbsolute(SELECT_LEFT_FE_CHR_BANK_ADDRESS),
                Instruction::JsrAbsolute(SELECT_RIGHT_FD_CHR_BANK_ADDRESS),
                Instruction::JsrAbsolute(SELECT_RIGHT_FE_CHR_BANK_ADDRESS),
                Instruction::JmpAbsolute(SOURCE_RESET_ADDRESS),
            ],
        )?,
        assemble_routine(
            "16 KiB PRG bank selection",
            SELECT_PRG_BANK_ADDRESS,
            &[
                Instruction::Php,
                Instruction::Pha,
                Instruction::AndImmediate(0x0F),
                Instruction::AslAccumulator,
                Instruction::Pha,
                Instruction::LdaImmediate(6),
                Instruction::StaAbsolute(0x8000),
                Instruction::Pla,
                Instruction::StaAbsolute(0x8001),
                Instruction::Pha,
                Instruction::LdaImmediate(7),
                Instruction::StaAbsolute(0x8000),
                Instruction::Pla,
                Instruction::OraImmediate(1),
                Instruction::StaAbsolute(0x8001),
                Instruction::Pla,
                Instruction::Plp,
                Instruction::Rts,
            ],
        )?,
        build_chr_routine(
            "PPU $0000 FD CHR bank selection",
            SELECT_LEFT_FD_CHR_BANK_ADDRESS,
            0,
        )?,
        build_chr_routine(
            "PPU $0000 FE CHR bank selection",
            SELECT_LEFT_FE_CHR_BANK_ADDRESS,
            1,
        )?,
        build_chr_routine(
            "PPU $1000 FD CHR bank selection",
            SELECT_RIGHT_FD_CHR_BANK_ADDRESS,
            2,
        )?,
        build_chr_routine(
            "PPU $1000 FE CHR bank selection",
            SELECT_RIGHT_FE_CHR_BANK_ADDRESS,
            4,
        )?,
    ])
}

fn build_chr_routine(
    role: &'static str,
    cpu_address: u16,
    mapper_register: u8,
) -> Result<AssembledRoutine> {
    assemble_routine(
        role,
        cpu_address,
        &[
            Instruction::Php,
            Instruction::Pha,
            Instruction::AndImmediate(0x1F),
            Instruction::AslAccumulator,
            Instruction::AslAccumulator,
            Instruction::Clc,
            Instruction::AdcImmediate(8),
            Instruction::Pha,
            Instruction::LdaImmediate(mapper_register),
            Instruction::StaAbsolute(0x8000),
            Instruction::Pla,
            Instruction::StaAbsolute(0x8001),
            Instruction::Pla,
            Instruction::Plp,
            Instruction::Rts,
        ],
    )
}

fn assemble_routine(
    role: &'static str,
    cpu_address: u16,
    instructions: &[Instruction],
) -> Result<AssembledRoutine> {
    Ok(AssembledRoutine {
        role,
        cpu_address,
        bytes: assemble_at(cpu_address, instructions)?,
    })
}

pub(super) fn validate_routine_placements(routines: &[AssembledRoutine]) -> Result<()> {
    let cave_end = CODE_CAVE_START_ADDRESS as usize + CODE_CAVE_LEN;
    for (index, routine) in routines.iter().enumerate() {
        let routine_end = routine.cpu_address as usize + routine.bytes.len();
        ensure!(
            routine.cpu_address >= CODE_CAVE_START_ADDRESS && routine_end <= cave_end,
            "mapper 165 {} routine is outside the proven code cave",
            routine.role
        );
        if let Some(next) = routines.get(index + 1) {
            ensure!(
                routine_end <= next.cpu_address as usize,
                "mapper 165 {} routine overlaps {}",
                routine.role,
                next.role
            );
        }
    }
    Ok(())
}

pub(super) fn replace_central_prg_writer(image: &mut TrackedImage) -> Result<()> {
    let source = [
        Instruction::StaZeroPage(0x29),
        Instruction::StaZeroPage(0x51),
        Instruction::StaAbsolute(0xA000),
        Instruction::Rts,
    ];
    let replacement = [
        Instruction::StaZeroPage(0x29),
        Instruction::StaZeroPage(0x51),
        Instruction::JsrAbsolute(SELECT_PRG_BANK_ADDRESS),
        Instruction::Rts,
    ];
    replace_same_length_routine(
        image,
        "central PRG bank selector",
        SOURCE_SELECT_PRG_BANK_AND_SAVE_ADDRESS,
        &source,
        &replacement,
    )
}

pub(super) fn replace_central_chr_writer(
    image: &mut TrackedImage,
    writer: CentralChrWriter,
) -> Result<()> {
    let source = [
        Instruction::StaZeroPage(writer.shadow_address),
        Instruction::OraZeroPage(0x52),
        Instruction::StaAbsolute(writer.source_register),
        Instruction::Rts,
    ];
    let replacement = [
        Instruction::StaZeroPage(writer.shadow_address),
        Instruction::OraZeroPage(0x52),
        Instruction::JsrAbsolute(writer.target_routine),
        Instruction::Rts,
    ];
    replace_same_length_routine(
        image,
        writer.role,
        writer.source_address,
        &source,
        &replacement,
    )
}

fn replace_same_length_routine(
    image: &mut TrackedImage,
    label: &str,
    source_address: u16,
    source: &[Instruction],
    replacement: &[Instruction],
) -> Result<()> {
    let expected = assemble_at(source_address, source)?;
    let replacement = assemble_at(source_address, replacement)?;
    ensure!(
        expected.len() == replacement.len(),
        "mapper 165 {label} replacement changed routine length"
    );
    image.write_expected(
        format!("redirect {label} to mapper 165"),
        fixed_bank_file_offset(source_address)?,
        &expected,
        &replacement,
    )
}

pub(super) fn replace_direct_writer(image: &mut TrackedImage, writer: DirectWriter) -> Result<()> {
    let file_offset = match writer.location {
        WriterLocation::Fixed => fixed_bank_file_offset(writer.source_address)?,
        WriterLocation::Switchable { prg_bank } => {
            switchable_bank_file_offset(prg_bank, writer.source_address)?
        }
    };
    image.write_expected(
        format!("redirect {} to mapper 165", writer.role),
        file_offset,
        &assemble_at(
            writer.source_address,
            &[Instruction::StaAbsolute(writer.source_register)],
        )?,
        &assemble_at(
            writer.source_address,
            &[Instruction::JsrAbsolute(writer.target_routine)],
        )?,
    )
}

pub(super) fn replace_mirroring_writer(
    image: &mut TrackedImage,
    role: &str,
    source_address: u16,
    value: u8,
) -> Result<()> {
    let source = [
        Instruction::LdaImmediate(value),
        Instruction::StaZeroPage(0xC8),
        Instruction::StaAbsolute(0xF000),
        Instruction::Rts,
    ];
    let replacement = [
        Instruction::LdaImmediate(value),
        Instruction::StaZeroPage(0xC8),
        Instruction::StaAbsolute(0xA000),
        Instruction::Rts,
    ];
    replace_same_length_routine(image, role, source_address, &source, &replacement)
}
