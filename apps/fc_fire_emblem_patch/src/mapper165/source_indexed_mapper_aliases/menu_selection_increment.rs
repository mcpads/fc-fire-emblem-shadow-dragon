use anyhow::{Result, ensure};

use crate::{
    mmc5_chr::switchable_bank_file_offset,
    mmc5_prg::count_direct_transfers_to_range,
    rom::Rom,
    rp2a03::{Instruction, assemble_at},
    tracked::TrackedImage,
};

use super::{
    GuardedRoutineEvidence, MAP_MENU_PRG_BANK, MENU_SELECTION_BASE_ADDRESS,
    SAFE_MENU_SELECTION_INDEX_LIMIT, find_bank_local_sequence,
};

const GUARD_ADDRESS: u16 = 0xBF06;
const GUARD_END: u16 = 0xBF10;
const SITES: [(u8, u16, u8); 3] = [
    (MAP_MENU_PRG_BANK, 0xB38F, 0x19),
    (MAP_MENU_PRG_BANK, 0xB3A3, 0x19),
    (MAP_MENU_PRG_BANK, 0xB3B7, 0x1A),
];

pub(super) struct BoundMenuSelectionIncrements {
    pub(super) source_site_count: usize,
    pub(super) guard_evidence: GuardedRoutineEvidence,
}

pub(in crate::mapper165) const fn source_indexed_menu_selection_increment_sites()
-> [(u8, u16, u8); 3] {
    SITES
}

pub(super) fn bind_guarded_menu_selection_increments(
    source: &Rom,
) -> Result<BoundMenuSelectionIncrements> {
    let instruction = indexed_increment_bytes()?;
    let actual_sites = find_bank_local_sequence(source.prg(), &instruction)?;
    let expected_sites = SITES
        .iter()
        .map(|&(bank, address, _)| (bank, address))
        .collect::<Vec<_>>();
    ensure!(
        actual_sites == expected_sites,
        "source indexed menu-selection increment sites changed: expected {expected_sites:?}, found {actual_sites:?}"
    );
    for &(bank, address, next_value) in &SITES {
        let expected = assemble_at(
            address,
            &[
                Instruction::IncAbsoluteX(MENU_SELECTION_BASE_ADDRESS),
                Instruction::LdaImmediate(next_value),
                Instruction::JmpAbsolute(0xE690),
            ],
        )?;
        let file_offset = switchable_bank_file_offset(bank, address)?;
        ensure!(
            source.data()[file_offset..file_offset + expected.len()] == expected,
            "source indexed menu-selection increment tail at bank {bank:02X}:${address:04X} changed"
        );
    }

    let guard = guard_routine()?;
    let guard_file_offset = switchable_bank_file_offset(MAP_MENU_PRG_BANK, GUARD_ADDRESS)?;
    ensure!(
        source.data()[guard_file_offset..guard_file_offset + guard.len()]
            .iter()
            .all(|byte| *byte == 0xFF),
        "source guarded menu-selection increment range is no longer all FF"
    );
    let source_direct_transfer_count =
        count_direct_transfers_to_range(source.prg(), GUARD_ADDRESS, GUARD_END)?;
    ensure!(
        source_direct_transfer_count == 0,
        "source guarded menu-selection increment range has {source_direct_transfer_count} direct JSR or JMP references"
    );

    Ok(BoundMenuSelectionIncrements {
        source_site_count: actual_sites.len(),
        guard_evidence: GuardedRoutineEvidence {
            source_prg_bank: MAP_MENU_PRG_BANK,
            address: format!("0x{GUARD_ADDRESS:04X}"),
            len: guard.len(),
            source_direct_transfer_count,
        },
    })
}

pub(super) fn install_guarded_menu_selection_increments(image: &mut TrackedImage) -> Result<()> {
    let guard = guard_routine()?;
    image.write_expected(
        "guard bank 06 indexed menu-selection increments from mapper165 register aliases",
        switchable_bank_file_offset(MAP_MENU_PRG_BANK, GUARD_ADDRESS)?,
        &vec![0xFF; guard.len()],
        &guard,
    )?;
    for &(bank, cpu_address, _) in &SITES {
        let expected = assemble_at(
            cpu_address,
            &[Instruction::IncAbsoluteX(MENU_SELECTION_BASE_ADDRESS)],
        )?;
        let replacement = assemble_at(cpu_address, &[Instruction::JsrAbsolute(GUARD_ADDRESS)])?;
        ensure!(
            replacement.len() == expected.len(),
            "guarded indexed menu-selection increment replacement length changed"
        );
        image.write_expected(
            format!(
                "route bank {bank:02X}:${cpu_address:04X} indexed menu-selection increment through bounded effective address"
            ),
            switchable_bank_file_offset(bank, cpu_address)?,
            &expected,
            &replacement,
        )?;
    }
    Ok(())
}

pub(super) fn verify_installed_guarded_menu_selection_increments(candidate: &Rom) -> Result<()> {
    let guard = guard_routine()?;
    let guard_file_offset = switchable_bank_file_offset(MAP_MENU_PRG_BANK, GUARD_ADDRESS)?;
    ensure!(
        candidate.data()[guard_file_offset..guard_file_offset + guard.len()] == guard,
        "installed guarded menu-selection increment routine changed"
    );
    for &(bank, cpu_address, _) in &SITES {
        let expected = assemble_at(cpu_address, &[Instruction::JsrAbsolute(GUARD_ADDRESS)])?;
        let file_offset = switchable_bank_file_offset(bank, cpu_address)?;
        ensure!(
            candidate.data()[file_offset..file_offset + expected.len()] == expected,
            "installed guarded indexed menu-selection increment at bank {bank:02X}:${cpu_address:04X} changed"
        );
    }
    let remaining = find_bank_local_sequence(candidate.prg(), &indexed_increment_bytes()?)?;
    ensure!(
        remaining == [(MAP_MENU_PRG_BANK, GUARD_ADDRESS + 0x05)],
        "installed indexed menu-selection increments are not confined to the guarded routine body: found {remaining:?}"
    );
    let installed_transfers =
        count_direct_transfers_to_range(candidate.prg(), GUARD_ADDRESS, GUARD_END)?;
    ensure!(
        installed_transfers == SITES.len(),
        "installed menu-selection increment guard transfer count changed: expected {}, found {installed_transfers}",
        SITES.len(),
    );
    Ok(())
}

fn indexed_increment_bytes() -> Result<Vec<u8>> {
    assemble_at(
        0x8000,
        &[Instruction::IncAbsoluteX(MENU_SELECTION_BASE_ADDRESS)],
    )
}

fn guard_routine() -> Result<Vec<u8>> {
    let bytes = assemble_at(
        GUARD_ADDRESS,
        &[
            Instruction::Php,
            Instruction::CpxImmediate(SAFE_MENU_SELECTION_INDEX_LIMIT),
            Instruction::BcsAbsolute(GUARD_ADDRESS + 0x08),
            Instruction::IncAbsoluteX(MENU_SELECTION_BASE_ADDRESS),
            Instruction::Plp,
            Instruction::Rts,
        ],
    )?;
    ensure!(
        usize::from(GUARD_ADDRESS) + bytes.len() == usize::from(GUARD_END),
        "guarded menu-selection increment routine extent changed"
    );
    Ok(bytes)
}

#[cfg(test)]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct IncrementTailEffect {
    accumulator: u8,
    index_value: u8,
    status: u8,
    prg_ram_write: Option<(u16, u8)>,
}

#[cfg(test)]
fn source_tail_effect(
    index: u8,
    previous_value: u8,
    next_accumulator: u8,
    incoming_status: u8,
) -> IncrementTailEffect {
    let effective_address = MENU_SELECTION_BASE_ADDRESS.wrapping_add(u16::from(index));
    IncrementTailEffect {
        accumulator: next_accumulator,
        index_value: index,
        status: status_after_load(incoming_status, next_accumulator),
        prg_ram_write: (effective_address < super::FIRST_MAPPER165_REGISTER_ADDRESS)
            .then_some((effective_address, previous_value.wrapping_add(1))),
    }
}

#[cfg(test)]
fn guarded_tail_effect(
    index: u8,
    previous_value: u8,
    next_accumulator: u8,
    incoming_status: u8,
) -> IncrementTailEffect {
    IncrementTailEffect {
        accumulator: next_accumulator,
        index_value: index,
        status: status_after_load(incoming_status, next_accumulator),
        prg_ram_write: (index < SAFE_MENU_SELECTION_INDEX_LIMIT).then_some((
            MENU_SELECTION_BASE_ADDRESS + u16::from(index),
            previous_value.wrapping_add(1),
        )),
    }
}

#[cfg(test)]
fn status_after_load(incoming_status: u8, value: u8) -> u8 {
    const NEGATIVE_AND_ZERO: u8 = 0x82;
    (incoming_status & !NEGATIVE_AND_ZERO) | (value & 0x80) | if value == 0 { 0x02 } else { 0 }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn guard_fits_its_owned_cave() {
        assert_eq!(
            guard_routine().unwrap().len(),
            usize::from(GUARD_END - GUARD_ADDRESS)
        );
    }

    #[test]
    fn guarded_increment_preserves_each_post_tail_effect_without_mapper_writes() {
        for index in u8::MIN..=u8::MAX {
            for incoming_status in u8::MIN..=u8::MAX {
                for previous_value in [0x00, 0x01, 0x7F, 0x80, 0xFE, 0xFF] {
                    for next_accumulator in [0x19, 0x1A] {
                        assert_eq!(
                            guarded_tail_effect(
                                index,
                                previous_value,
                                next_accumulator,
                                incoming_status,
                            ),
                            source_tail_effect(
                                index,
                                previous_value,
                                next_accumulator,
                                incoming_status,
                            )
                        );
                    }
                }
            }
        }
    }
}
