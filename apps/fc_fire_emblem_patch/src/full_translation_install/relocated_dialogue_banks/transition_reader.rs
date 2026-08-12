use anyhow::{Context, Result, ensure};

use crate::rp2a03::{Instruction, assemble_at};

pub(super) const TRANSITION_BANK_MARKER: u8 = 0x80;
pub(super) const SOURCE_POINTER_RESOLVER: u16 = 0xE6B2;
const SOURCE_PRG_SELECTOR: u16 = 0xFA20;
const ACTIVE_PRG_BANK_SHADOW: u8 = 0x29;
const TRANSITION_BANK_STATE: u16 = 0x77F2;
pub(super) const SELECTOR_CAVE_START: u16 = 0xF558;
pub(super) const TRANSITION_POINTER_RESOLVER: u16 = SELECTOR_CAVE_START;
pub(super) const TRANSITION_BANK_SELECTOR: u16 = 0xF568;
pub(super) const TRANSITION_BANK_RESTORE: u16 = 0xF5D0;
pub(super) const NMI_PRG_BANK_RESTORE_SELECTOR: u16 = 0xF5E0;
pub(super) const SELECTOR_CAVE_END_EXCLUSIVE: u16 = 0xF700;
const TRANSITION_MIRROR_PAGES: [(u8, u8); 5] = [
    (0x04, 0x22),
    (0x07, 0x24),
    (0x08, 0x26),
    (0x0B, 0x28),
    (0x0C, 0x2A),
];

pub(super) struct TransitionReaderRoutines {
    pub(super) pointer_resolver: Vec<u8>,
    pub(super) bank_selector: Vec<u8>,
    pub(super) bank_restore: Vec<u8>,
    pub(super) nmi_bank_restore_selector: Vec<u8>,
}

pub(super) fn assemble_transition_reader_routines() -> Result<TransitionReaderRoutines> {
    Ok(TransitionReaderRoutines {
        pointer_resolver: assemble_transition_pointer_resolver()?,
        bank_selector: assemble_transition_bank_selector()?,
        bank_restore: assemble_transition_bank_restore()?,
        nmi_bank_restore_selector: assemble_nmi_bank_restore_selector()?,
    })
}

fn assemble_transition_pointer_resolver() -> Result<Vec<u8>> {
    assemble_at(
        TRANSITION_POINTER_RESOLVER,
        &[
            Instruction::JsrAbsolute(SOURCE_POINTER_RESOLVER),
            Instruction::Php,
            Instruction::Pha,
            Instruction::LdaAbsolute(TRANSITION_BANK_STATE),
            Instruction::OraImmediate(TRANSITION_BANK_MARKER),
            Instruction::StaAbsolute(TRANSITION_BANK_STATE),
            Instruction::Pla,
            Instruction::Plp,
            Instruction::Rts,
        ],
    )
}

fn assemble_transition_bank_selector() -> Result<Vec<u8>> {
    let marked = |source_bank| TRANSITION_BANK_MARKER | source_bank;
    let mut instructions = vec![Instruction::Php, Instruction::Pha];
    let mut route_jumps = Vec::new();
    for (source_bank, _) in TRANSITION_MIRROR_PAGES {
        route_jumps.push(append_match_jump_at(
            &mut instructions,
            marked(source_bank),
            TRANSITION_BANK_SELECTOR,
        )?);
    }
    instructions.extend([
        Instruction::Pla,
        Instruction::Plp,
        Instruction::JmpAbsolute(SOURCE_PRG_SELECTOR),
    ]);

    let mut routes = Vec::new();
    let mut writer_branches = Vec::new();
    for (_, first_page) in TRANSITION_MIRROR_PAGES {
        routes.push(next_address(TRANSITION_BANK_SELECTOR, &instructions)?);
        instructions.push(Instruction::LdaImmediate(first_page));
        writer_branches.push(push_local_bne_placeholder_at(
            &mut instructions,
            TRANSITION_BANK_SELECTOR,
        )?);
    }
    let write_mirror = next_address(TRANSITION_BANK_SELECTOR, &instructions)?;
    for (jump, route) in route_jumps.into_iter().zip(routes) {
        instructions[jump] = Instruction::JmpAbsolute(route);
    }
    for branch in writer_branches {
        instructions[branch] = Instruction::BneAbsolute(write_mirror);
    }
    instructions.extend(mirror_pair_writer());

    let bytes = assemble_at(TRANSITION_BANK_SELECTOR, &instructions)?;
    ensure!(
        TRANSITION_BANK_SELECTOR as usize + bytes.len() <= TRANSITION_BANK_RESTORE as usize,
        "transition bank selector exceeds its fixed cave partition"
    );
    Ok(bytes)
}

fn assemble_transition_bank_restore() -> Result<Vec<u8>> {
    assemble_at(
        TRANSITION_BANK_RESTORE,
        &[
            Instruction::StaZeroPage(ACTIVE_PRG_BANK_SHADOW),
            Instruction::JmpAbsolute(SOURCE_PRG_SELECTOR),
        ],
    )
}

fn assemble_nmi_bank_restore_selector() -> Result<Vec<u8>> {
    let marked = |source_bank| TRANSITION_BANK_MARKER | source_bank;
    let mut instructions = vec![Instruction::Php, Instruction::Pha];
    let mut physical_route_jumps = Vec::new();
    for (_, first_page) in TRANSITION_MIRROR_PAGES {
        physical_route_jumps.push(append_match_jump_at(
            &mut instructions,
            first_page,
            NMI_PRG_BANK_RESTORE_SELECTOR,
        )?);
    }
    let source_fallback = next_address(NMI_PRG_BANK_RESTORE_SELECTOR, &instructions)?;
    instructions.extend([
        Instruction::Pla,
        Instruction::Plp,
        Instruction::JmpAbsolute(SOURCE_PRG_SELECTOR),
    ]);

    let mut physical_routes = Vec::new();
    let mut invalid_shadow_branches = Vec::new();
    let mut writer_branches = Vec::new();
    for (source_bank, first_page) in TRANSITION_MIRROR_PAGES {
        physical_routes.push(next_address(NMI_PRG_BANK_RESTORE_SELECTOR, &instructions)?);
        instructions.extend([
            Instruction::LdaAbsolute(TRANSITION_BANK_STATE),
            Instruction::CmpImmediate(marked(source_bank)),
        ]);
        invalid_shadow_branches.push(push_local_bne_placeholder_at(
            &mut instructions,
            NMI_PRG_BANK_RESTORE_SELECTOR,
        )?);
        instructions.push(Instruction::LdaImmediate(first_page));
        writer_branches.push(push_local_bne_placeholder_at(
            &mut instructions,
            NMI_PRG_BANK_RESTORE_SELECTOR,
        )?);
    }

    for branch in invalid_shadow_branches {
        instructions[branch] = Instruction::BneAbsolute(source_fallback);
    }

    let write_mirror = next_address(NMI_PRG_BANK_RESTORE_SELECTOR, &instructions)?;
    for branch in writer_branches {
        instructions[branch] = Instruction::BneAbsolute(write_mirror);
    }
    for (jump, route) in physical_route_jumps.into_iter().zip(physical_routes) {
        instructions[jump] = Instruction::JmpAbsolute(route);
    }
    instructions.extend(mirror_pair_writer());

    let bytes = assemble_at(NMI_PRG_BANK_RESTORE_SELECTOR, &instructions)?;
    ensure!(
        NMI_PRG_BANK_RESTORE_SELECTOR as usize + bytes.len()
            <= SELECTOR_CAVE_END_EXCLUSIVE as usize,
        "NMI PRG bank restore selector exceeds its fixed cave"
    );
    Ok(bytes)
}

fn mirror_pair_writer() -> [Instruction; 16] {
    [
        Instruction::StaZeroPage(ACTIVE_PRG_BANK_SHADOW),
        Instruction::Pha,
        Instruction::LdaImmediate(0x06),
        Instruction::StaAbsolute(0x8000),
        Instruction::Pla,
        Instruction::StaAbsolute(0x8001),
        Instruction::Clc,
        Instruction::AdcImmediate(1),
        Instruction::Pha,
        Instruction::LdaImmediate(0x07),
        Instruction::StaAbsolute(0x8000),
        Instruction::Pla,
        Instruction::StaAbsolute(0x8001),
        Instruction::Pla,
        Instruction::Plp,
        Instruction::Rts,
    ]
}

fn append_match_jump_at(
    instructions: &mut Vec<Instruction>,
    selector: u8,
    origin: u16,
) -> Result<usize> {
    instructions.push(Instruction::CmpImmediate(selector));
    let mismatch_branch = instructions.len();
    instructions.push(Instruction::BneAbsolute(origin));
    let matching_jump = instructions.len();
    instructions.push(Instruction::JmpAbsolute(origin));
    let next_match = next_address(origin, instructions)?;
    instructions[mismatch_branch] = Instruction::BneAbsolute(next_match);
    Ok(matching_jump)
}

fn push_local_bne_placeholder_at(
    instructions: &mut Vec<Instruction>,
    origin: u16,
) -> Result<usize> {
    let branch_address = next_address(origin, instructions)?;
    let index = instructions.len();
    instructions.push(Instruction::BneAbsolute(branch_address));
    Ok(index)
}

fn next_address(origin: u16, instructions: &[Instruction]) -> Result<u16> {
    origin
        .checked_add(
            u16::try_from(assemble_at(origin, instructions)?.len())
                .context("transition reader routine length does not fit u16")?,
        )
        .context("transition reader routine address overflow")
}

#[cfg(test)]
fn transition_first_page(selector: u8) -> Option<u8> {
    TRANSITION_MIRROR_PAGES
        .iter()
        .find_map(|(source_bank, first_page)| {
            (selector == (TRANSITION_BANK_MARKER | *source_bank)).then_some(*first_page)
        })
}

#[cfg(test)]
fn restored_first_page(shadow: u8, transition_state: u8) -> Option<u8> {
    TRANSITION_MIRROR_PAGES
        .iter()
        .find_map(|(source_bank, first_page)| {
            (shadow == *first_page && transition_state == (TRANSITION_BANK_MARKER | *source_bank))
                .then_some(*first_page)
        })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn transition_reader_routines_fit_disjoint_cave_partitions() {
        let routines = assemble_transition_reader_routines().unwrap();

        assert!(
            routines.pointer_resolver.len()
                <= usize::from(TRANSITION_BANK_SELECTOR - TRANSITION_POINTER_RESOLVER)
        );
        assert!(
            routines.bank_selector.len()
                <= usize::from(TRANSITION_BANK_RESTORE - TRANSITION_BANK_SELECTOR)
        );
        assert!(
            routines.bank_restore.len()
                <= usize::from(NMI_PRG_BANK_RESTORE_SELECTOR - TRANSITION_BANK_RESTORE)
        );
        assert!(
            routines.nmi_bank_restore_selector.len()
                <= usize::from(SELECTOR_CAVE_END_EXCLUSIVE - NMI_PRG_BANK_RESTORE_SELECTOR)
        );
    }

    #[test]
    fn dedicated_reader_selector_maps_only_marked_transition_banks() {
        let bytes = assemble_transition_bank_selector().unwrap();

        for (source_bank, first_page) in TRANSITION_MIRROR_PAGES {
            assert_eq!(
                transition_first_page(TRANSITION_BANK_MARKER | source_bank),
                Some(first_page)
            );
            assert!(bytes.windows(2).any(|pair| pair == [0xA9, first_page]));
        }
        assert_eq!(transition_first_page(0x22), None);
        assert_eq!(transition_first_page(0x0A), None);
    }

    #[test]
    fn nmi_audio_restore_requires_the_matching_live_transition_identity() {
        for (source_bank, first_page) in TRANSITION_MIRROR_PAGES {
            let marked = TRANSITION_BANK_MARKER | source_bank;
            let shadow = transition_first_page(marked).unwrap();

            assert_eq!(shadow, first_page);
            assert_eq!(restored_first_page(shadow, marked), Some(first_page));
            assert_eq!(restored_first_page(shadow, source_bank), None);
        }
        assert_eq!(restored_first_page(0x0A, 0x0A), None);
        assert_eq!(restored_first_page(0x0E, 0x0E), None);
    }
}
