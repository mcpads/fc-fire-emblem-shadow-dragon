use anyhow::{Context, Result, ensure};
use serde::Serialize;

use crate::{
    mmc5_chr::switchable_bank_file_offset,
    mmc5_prg::count_direct_transfers_to_range,
    rom::Rom,
    rp2a03::{Instruction, assemble_at},
    tracked::TrackedImage,
};

use super::unit_name_table;

const MENU_MASK_COUNT_ADDRESS: u16 = 0x05CE;
const MENU_MASK_BASE_ADDRESS: u16 = 0x7FEE;
const FIRST_MAPPER165_REGISTER_ADDRESS: u16 = 0x8000;
const SAFE_MENU_MASK_INDEX_LIMIT: u8 = 0x12;

const MAP_MENU_PRG_BANK: u8 = 0x06;
const MAP_MENU_GUARD_ADDRESS: u16 = 0xBEDE;
const MAP_MENU_GUARD_END: u16 = 0xBEE8;
const FRONT_END_PRG_BANK: u8 = 0x0B;
const FRONT_END_GUARD_ADDRESS: u16 = unit_name_table::CAVE_END_ADDRESS;
const FRONT_END_GUARD_END: u16 = 0xBF9A;

const MAP_MENU_CLEAR_SEQUENCE_SITES: [u16; 6] = [0x940C, 0x9A14, 0x9C91, 0x9CDA, 0xB233, 0xB360];
const INDEXED_MENU_MASK_STORE_SITES: [(u8, u16); 16] = [
    (MAP_MENU_PRG_BANK, 0x9412),
    (MAP_MENU_PRG_BANK, 0x9A1A),
    (MAP_MENU_PRG_BANK, 0x9C97),
    (MAP_MENU_PRG_BANK, 0x9CE0),
    (MAP_MENU_PRG_BANK, 0xB239),
    (MAP_MENU_PRG_BANK, 0xB366),
    (FRONT_END_PRG_BANK, 0x819C),
    (FRONT_END_PRG_BANK, 0x844C),
    (FRONT_END_PRG_BANK, 0x85B3),
    (FRONT_END_PRG_BANK, 0x87B9),
    (FRONT_END_PRG_BANK, 0x87EC),
    (FRONT_END_PRG_BANK, 0x89D0),
    (FRONT_END_PRG_BANK, 0x8A96),
    (FRONT_END_PRG_BANK, 0x8AB6),
    (FRONT_END_PRG_BANK, 0x8B2F),
    (FRONT_END_PRG_BANK, 0x8E5B),
];

#[derive(Clone, Copy)]
enum AccumulatorProducer {
    Immediate(u8),
    Absolute(u16),
}

#[derive(Clone, Copy)]
struct FrontEndStoreBinding {
    store_address: u16,
    accumulator: AccumulatorProducer,
}

const FRONT_END_STORE_BINDINGS: [FrontEndStoreBinding; 10] = [
    FrontEndStoreBinding {
        store_address: 0x819C,
        accumulator: AccumulatorProducer::Immediate(0x3F),
    },
    FrontEndStoreBinding {
        store_address: 0x844C,
        accumulator: AccumulatorProducer::Absolute(0x05EB),
    },
    FrontEndStoreBinding {
        store_address: 0x85B3,
        accumulator: AccumulatorProducer::Absolute(0x05EB),
    },
    FrontEndStoreBinding {
        store_address: 0x87B9,
        accumulator: AccumulatorProducer::Immediate(0x03),
    },
    FrontEndStoreBinding {
        store_address: 0x87EC,
        accumulator: AccumulatorProducer::Immediate(0x03),
    },
    FrontEndStoreBinding {
        store_address: 0x89D0,
        accumulator: AccumulatorProducer::Absolute(0x05EB),
    },
    FrontEndStoreBinding {
        store_address: 0x8A96,
        accumulator: AccumulatorProducer::Immediate(0x1F),
    },
    FrontEndStoreBinding {
        store_address: 0x8AB6,
        accumulator: AccumulatorProducer::Immediate(0x07),
    },
    FrontEndStoreBinding {
        store_address: 0x8B2F,
        accumulator: AccumulatorProducer::Immediate(0x03),
    },
    FrontEndStoreBinding {
        store_address: 0x8E5B,
        accumulator: AccumulatorProducer::Immediate(0x00),
    },
];

#[derive(Clone, Debug, Serialize)]
struct GuardedRoutineEvidence {
    source_prg_bank: u8,
    address: String,
    len: usize,
    source_direct_transfer_count: usize,
}

#[derive(Clone, Debug, Serialize)]
pub(super) struct SourceIndexedMapperAliasSafety {
    scope: &'static str,
    closure_claim: &'static str,
    source_prg_banks: [u8; 2],
    unguarded_indexed_store_site_count: usize,
    rewritten_indexed_store_site_count: usize,
    guarded_routines: Vec<GuardedRoutineEvidence>,
    safe_menu_mask_index_limit: u8,
    all_index_and_status_values_preserve_source_effects: bool,
}

pub(super) fn bind_source_indexed_mapper_aliases(
    source: &Rom,
) -> Result<SourceIndexedMapperAliasSafety> {
    source.verify_supported_japanese()?;

    let indexed_store = indexed_menu_mask_store_bytes()?;
    let actual_sites = find_bank_local_sequence(source.prg(), &indexed_store)?;
    let expected_sites = INDEXED_MENU_MASK_STORE_SITES.to_vec();
    ensure!(
        actual_sites == expected_sites,
        "source indexed menu-mask store sites changed: expected {expected_sites:?}, found {actual_sites:?}"
    );

    let map_sequence = unguarded_menu_mask_clear_sequence();
    for cpu_address in MAP_MENU_CLEAR_SEQUENCE_SITES {
        let expected = assemble_at(cpu_address, &map_sequence)?;
        let file_offset = switchable_bank_file_offset(MAP_MENU_PRG_BANK, cpu_address)?;
        ensure!(
            source.data()[file_offset..file_offset + expected.len()] == expected,
            "source map-menu mask clear at bank {MAP_MENU_PRG_BANK:02X}:${cpu_address:04X} changed"
        );
    }

    for binding in FRONT_END_STORE_BINDINGS {
        let (sequence_address, expected) = front_end_store_sequence(binding)?;
        let file_offset = switchable_bank_file_offset(FRONT_END_PRG_BANK, sequence_address)?;
        ensure!(
            source.data()[file_offset..file_offset + expected.len()] == expected,
            "source front-end indexed menu-mask producer ending at bank {FRONT_END_PRG_BANK:02X}:${:04X} changed",
            binding.store_address
        );
    }

    let mut guarded_routines = Vec::new();
    for (bank, address, end) in guarded_routine_sites() {
        let routine = guarded_indexed_store_routine(address, end)?;
        let cave_file_offset = switchable_bank_file_offset(bank, address)?;
        ensure!(
            source.data()[cave_file_offset..cave_file_offset + routine.len()]
                .iter()
                .all(|byte| *byte == 0xFF),
            "source guarded indexed-store range at bank {bank:02X}:${address:04X} is no longer all FF"
        );
        let source_direct_transfer_count =
            count_direct_transfers_to_range(source.prg(), address, end)?;
        ensure!(
            source_direct_transfer_count == 0,
            "source guarded indexed-store range at bank {bank:02X}:${address:04X} has {source_direct_transfer_count} direct JSR or JMP references"
        );
        guarded_routines.push(GuardedRoutineEvidence {
            source_prg_bank: bank,
            address: format!("0x{address:04X}"),
            len: routine.len(),
            source_direct_transfer_count,
        });
    }

    ensure!(
        all_index_and_status_values_preserve_source_effects(),
        "guarded indexed menu-mask store does not preserve the source effect domain"
    );

    Ok(SourceIndexedMapperAliasSafety {
        scope: "all sixteen exact source STA $7FEE,X sites in banks 06 and 0B, including their typed index and accumulator producers",
        closure_claim: "complete for every source occurrence of this indexed store and its complete 0x00..0xFF index domain; other source indexed, indirect, or synthesized writes that can enter mapper165 registers remain in the global executable-write audit",
        source_prg_banks: [MAP_MENU_PRG_BANK, FRONT_END_PRG_BANK],
        unguarded_indexed_store_site_count: actual_sites.len(),
        rewritten_indexed_store_site_count: INDEXED_MENU_MASK_STORE_SITES.len(),
        guarded_routines,
        safe_menu_mask_index_limit: SAFE_MENU_MASK_INDEX_LIMIT,
        all_index_and_status_values_preserve_source_effects: true,
    })
}

pub(super) fn install_guarded_menu_mask_clears(image: &mut TrackedImage) -> Result<()> {
    for (bank, address, end) in guarded_routine_sites() {
        let routine = guarded_indexed_store_routine(address, end)?;
        image.write_expected(
            format!(
                "guard bank {bank:02X} indexed menu-mask stores from mapper165 register aliases"
            ),
            switchable_bank_file_offset(bank, address)?,
            &vec![0xFF; routine.len()],
            &routine,
        )?;
    }

    for (bank, cpu_address) in INDEXED_MENU_MASK_STORE_SITES {
        let expected = assemble_at(
            cpu_address,
            &[Instruction::StaAbsoluteX(MENU_MASK_BASE_ADDRESS)],
        )?;
        let replacement = assemble_at(
            cpu_address,
            &[Instruction::JsrAbsolute(guard_address_for_bank(bank)?)],
        )?;
        ensure!(
            replacement.len() == expected.len(),
            "guarded indexed menu-mask store replacement length changed"
        );
        image.write_expected(
            format!("route bank {bank:02X}:${cpu_address:04X} indexed menu-mask store through bounded effective address"),
            switchable_bank_file_offset(bank, cpu_address)?,
            &expected,
            &replacement,
        )?;
    }
    Ok(())
}

pub(super) fn verify_installed_guarded_menu_mask_clears(candidate: &Rom) -> Result<()> {
    for (bank, address, end) in guarded_routine_sites() {
        let routine = guarded_indexed_store_routine(address, end)?;
        let cave_file_offset = switchable_bank_file_offset(bank, address)?;
        ensure!(
            candidate.data()[cave_file_offset..cave_file_offset + routine.len()] == routine,
            "installed guarded indexed-store routine at bank {bank:02X}:${address:04X} changed"
        );
    }

    for (bank, cpu_address) in INDEXED_MENU_MASK_STORE_SITES {
        let expected_installed = assemble_at(
            cpu_address,
            &[Instruction::JsrAbsolute(guard_address_for_bank(bank)?)],
        )?;
        let file_offset = switchable_bank_file_offset(bank, cpu_address)?;
        ensure!(
            candidate.data()[file_offset..file_offset + expected_installed.len()]
                == expected_installed,
            "installed guarded indexed menu-mask store at bank {bank:02X}:${cpu_address:04X} changed"
        );
    }

    let remaining_direct_stores =
        find_bank_local_sequence(candidate.prg(), &indexed_menu_mask_store_bytes()?)?;
    let expected_guarded_store_bodies = guarded_routine_sites()
        .into_iter()
        .map(|(bank, address, _)| (bank, address + 0x05))
        .collect::<Vec<_>>();
    ensure!(
        remaining_direct_stores == expected_guarded_store_bodies,
        "installed indexed menu-mask stores are not confined to the two guarded routine bodies: expected {expected_guarded_store_bodies:?}, found {remaining_direct_stores:?}"
    );
    for (bank, address, end) in guarded_routine_sites() {
        let expected_count = INDEXED_MENU_MASK_STORE_SITES
            .iter()
            .filter(|(site_bank, _)| *site_bank == bank)
            .count();
        let installed_direct_transfers =
            count_direct_transfers_to_range(candidate.prg(), address, end)?;
        ensure!(
            installed_direct_transfers == expected_count,
            "installed bank {bank:02X} indexed-store guard transfer count changed: expected {expected_count}, found {installed_direct_transfers}"
        );
    }
    Ok(())
}

fn unguarded_menu_mask_clear_sequence() -> [Instruction; 4] {
    [
        Instruction::LdxAbsolute(MENU_MASK_COUNT_ADDRESS),
        Instruction::Dex,
        Instruction::LdaImmediate(0),
        Instruction::StaAbsoluteX(MENU_MASK_BASE_ADDRESS),
    ]
}

fn indexed_menu_mask_store_bytes() -> Result<Vec<u8>> {
    assemble_at(0x8000, &[Instruction::StaAbsoluteX(MENU_MASK_BASE_ADDRESS)])
}

fn front_end_store_sequence(binding: FrontEndStoreBinding) -> Result<(u16, Vec<u8>)> {
    let mut instructions = vec![Instruction::LdxAbsolute(MENU_MASK_COUNT_ADDRESS)];
    instructions.push(match binding.accumulator {
        AccumulatorProducer::Immediate(value) => Instruction::LdaImmediate(value),
        AccumulatorProducer::Absolute(address) => Instruction::LdaAbsolute(address),
    });
    instructions.push(Instruction::StaAbsoluteX(MENU_MASK_BASE_ADDRESS));
    let prototype = assemble_at(0x8000, &instructions)?;
    let prefix_len = u16::try_from(prototype.len() - indexed_menu_mask_store_bytes()?.len())?;
    let sequence_address = binding
        .store_address
        .checked_sub(prefix_len)
        .context("front-end indexed-store producer address underflow")?;
    Ok((
        sequence_address,
        assemble_at(sequence_address, &instructions)?,
    ))
}

fn guarded_routine_sites() -> [(u8, u16, u16); 2] {
    [
        (
            MAP_MENU_PRG_BANK,
            MAP_MENU_GUARD_ADDRESS,
            MAP_MENU_GUARD_END,
        ),
        (
            FRONT_END_PRG_BANK,
            FRONT_END_GUARD_ADDRESS,
            FRONT_END_GUARD_END,
        ),
    ]
}

fn guard_address_for_bank(bank: u8) -> Result<u16> {
    match bank {
        MAP_MENU_PRG_BANK => Ok(MAP_MENU_GUARD_ADDRESS),
        FRONT_END_PRG_BANK => Ok(FRONT_END_GUARD_ADDRESS),
        _ => anyhow::bail!("bank {bank:02X} has no indexed menu-mask store guard"),
    }
}

fn guarded_indexed_store_routine(address: u16, end: u16) -> Result<Vec<u8>> {
    let no_write = address + 0x08;
    let bytes = assemble_at(
        address,
        &[
            Instruction::Php,
            Instruction::CpxImmediate(SAFE_MENU_MASK_INDEX_LIMIT),
            Instruction::BcsAbsolute(no_write),
            Instruction::StaAbsoluteX(MENU_MASK_BASE_ADDRESS),
            Instruction::Plp,
            Instruction::Rts,
        ],
    )?;
    ensure!(
        usize::from(address) + bytes.len() == usize::from(end),
        "guarded indexed menu-mask store routine extent changed"
    );
    Ok(bytes)
}

fn find_bank_local_sequence(prg: &[u8], sequence: &[u8]) -> Result<Vec<(u8, u16)>> {
    ensure!(!sequence.is_empty(), "cannot scan an empty source sequence");
    let mut matches = Vec::new();
    for (bank, bytes) in prg.chunks_exact(0x4000).enumerate() {
        for (offset, window) in bytes.windows(sequence.len()).enumerate() {
            if window == sequence {
                matches.push((
                    u8::try_from(bank).context("source PRG bank index overflow")?,
                    0x8000u16
                        .checked_add(u16::try_from(offset)?)
                        .context("source CPU address overflow")?,
                ));
            }
        }
    }
    Ok(matches)
}

fn all_index_and_status_values_preserve_source_effects() -> bool {
    (u8::MIN..=u8::MAX).all(|index| {
        (u8::MIN..=u8::MAX).all(|incoming_status| {
            let accumulator = index.wrapping_add(incoming_status);
            source_indexed_store_effect(index, accumulator, incoming_status)
                == guarded_indexed_store_effect(index, accumulator, incoming_status)
        })
    })
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct IndexedStoreEffect {
    accumulator: u8,
    index_x: u8,
    status: u8,
    prg_ram_write: Option<(u16, u8)>,
}

fn source_indexed_store_effect(
    index: u8,
    accumulator: u8,
    incoming_status: u8,
) -> IndexedStoreEffect {
    let effective_address = MENU_MASK_BASE_ADDRESS.wrapping_add(u16::from(index));
    IndexedStoreEffect {
        accumulator,
        index_x: index,
        status: incoming_status,
        prg_ram_write: (effective_address < FIRST_MAPPER165_REGISTER_ADDRESS)
            .then_some((effective_address, accumulator)),
    }
}

fn guarded_indexed_store_effect(
    index: u8,
    accumulator: u8,
    incoming_status: u8,
) -> IndexedStoreEffect {
    IndexedStoreEffect {
        accumulator,
        index_x: index,
        status: incoming_status,
        prg_ram_write: (index < SAFE_MENU_MASK_INDEX_LIMIT)
            .then(|| (MENU_MASK_BASE_ADDRESS + u16::from(index), accumulator)),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn guarded_routines_preserve_every_index_and_status_without_mapper_writes() {
        assert!(all_index_and_status_values_preserve_source_effects());
        for (_, address, end) in guarded_routine_sites() {
            assert_eq!(
                guarded_indexed_store_routine(address, end).unwrap().len(),
                usize::from(end - address)
            );
        }
    }

    #[test]
    fn out_of_range_indices_do_not_reach_mapper165_registers() {
        for index in [0x12u8, 0x13, 0x80, 0xFF] {
            let effect = guarded_indexed_store_effect(index, 0xA5, 0xFF);
            let source_effective_address = MENU_MASK_BASE_ADDRESS + u16::from(index);
            assert!(source_effective_address >= FIRST_MAPPER165_REGISTER_ADDRESS);
            assert_eq!(effect.prg_ram_write, None);
            assert_eq!(effect.accumulator, 0xA5);
            assert_eq!(effect.index_x, index);
            assert_eq!(effect.status, 0xFF);
        }
    }

    #[test]
    fn valid_indices_keep_the_original_prg_ram_cell_and_value() {
        for index in 0..SAFE_MENU_MASK_INDEX_LIMIT {
            let expected = MENU_MASK_BASE_ADDRESS + u16::from(index);
            assert!(expected < FIRST_MAPPER165_REGISTER_ADDRESS);
            assert_eq!(
                guarded_indexed_store_effect(index, 0x5A, 0xA5).prg_ram_write,
                Some((expected, 0x5A))
            );
        }
    }

    #[test]
    fn front_end_producer_bindings_cover_every_front_end_store_once() {
        let mut sites = FRONT_END_STORE_BINDINGS
            .iter()
            .map(|binding| binding.store_address)
            .collect::<Vec<_>>();
        sites.sort_unstable();
        let mut expected = INDEXED_MENU_MASK_STORE_SITES
            .iter()
            .filter_map(|(bank, address)| (*bank == FRONT_END_PRG_BANK).then_some(*address))
            .collect::<Vec<_>>();
        expected.sort_unstable();
        assert_eq!(sites, expected);
    }
}
