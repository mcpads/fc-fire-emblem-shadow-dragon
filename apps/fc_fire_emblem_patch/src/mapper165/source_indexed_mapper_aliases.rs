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

mod menu_selection_increment;

pub(super) use menu_selection_increment::source_indexed_menu_selection_increment_sites;
use menu_selection_increment::{
    bind_guarded_menu_selection_increments, install_guarded_menu_selection_increments,
    verify_installed_guarded_menu_selection_increments,
};

const MENU_MASK_COUNT_ADDRESS: u16 = 0x05CE;
const MENU_MASK_BASE_ADDRESS: u16 = 0x7FEE;
const MENU_SELECTION_BASE_ADDRESS: u16 = 0x7FF3;
const FIRST_MAPPER165_REGISTER_ADDRESS: u16 = 0x8000;
const SAFE_MENU_MASK_INDEX_LIMIT: u8 = 0x12;
const SAFE_MENU_SELECTION_INDEX_LIMIT: u8 = 0x0D;

const MAP_MENU_PRG_BANK: u8 = 0x06;
const MAP_MENU_GUARD_ADDRESS: u16 = 0xBEDE;
const MAP_MENU_GUARD_END: u16 = 0xBEE8;
const MAP_MENU_SELECTION_GUARD_ADDRESS: u16 = MAP_MENU_GUARD_END;
const MAP_MENU_SELECTION_GUARD_END: u16 = 0xBEF2;
const MAP_MENU_MASK_Y_GUARD_ADDRESS: u16 = MAP_MENU_SELECTION_GUARD_END;
const MAP_MENU_MASK_Y_GUARD_END: u16 = 0xBEFC;
const FRONT_END_PRG_BANK: u8 = 0x0B;
const FRONT_END_SELECTION_GUARD_ADDRESS: u16 = unit_name_table::CAVE_END_ADDRESS;
const FRONT_END_SELECTION_GUARD_END: u16 = 0xBF90;
const FRONT_END_GUARD_ADDRESS: u16 = FRONT_END_SELECTION_GUARD_END;
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
const INDEXED_MENU_SELECTION_STORE_SITES: [(u8, u16); 13] = [
    (MAP_MENU_PRG_BANK, 0xB38C),
    (MAP_MENU_PRG_BANK, 0xB3A0),
    (MAP_MENU_PRG_BANK, 0xB3B4),
    (FRONT_END_PRG_BANK, 0x81A1),
    (FRONT_END_PRG_BANK, 0x8451),
    (FRONT_END_PRG_BANK, 0x85B8),
    (FRONT_END_PRG_BANK, 0x87BE),
    (FRONT_END_PRG_BANK, 0x89D5),
    (FRONT_END_PRG_BANK, 0x8ABB),
    (FRONT_END_PRG_BANK, 0x8B34),
    (FRONT_END_PRG_BANK, 0x8DC2),
    (FRONT_END_PRG_BANK, 0x9364),
    (FRONT_END_PRG_BANK, 0x9376),
];
const INDEXED_MENU_MASK_Y_STORE_SITES: [(u8, u16); 2] =
    [(MAP_MENU_PRG_BANK, 0xB719), (MAP_MENU_PRG_BANK, 0xB8E6)];

pub(super) const fn source_indexed_menu_mask_store_sites() -> [(u8, u16); 16] {
    INDEXED_MENU_MASK_STORE_SITES
}

pub(super) const fn source_indexed_menu_selection_store_sites() -> [(u8, u16); 13] {
    INDEXED_MENU_SELECTION_STORE_SITES
}

pub(super) const fn source_indexed_menu_mask_y_store_sites() -> [(u8, u16); 2] {
    INDEXED_MENU_MASK_Y_STORE_SITES
}

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
    unguarded_indexed_increment_site_count: usize,
    rewritten_indexed_increment_site_count: usize,
    guarded_routines: Vec<GuardedRoutineEvidence>,
    safe_menu_mask_index_limit: u8,
    safe_menu_selection_index_limit: u8,
    all_index_and_status_values_preserve_source_effects: bool,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum IndexRegister {
    X,
    Y,
}

#[derive(Clone)]
struct IndexedStoreFamily {
    role: &'static str,
    base_address: u16,
    index_register: IndexRegister,
    sites: Vec<(u8, u16)>,
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
    let indexed_selection_store = indexed_menu_selection_store_bytes()?;
    let actual_selection_sites = find_bank_local_sequence(source.prg(), &indexed_selection_store)?;
    let expected_selection_sites = INDEXED_MENU_SELECTION_STORE_SITES.to_vec();
    ensure!(
        actual_selection_sites == expected_selection_sites,
        "source indexed menu-selection store sites changed: expected {expected_selection_sites:?}, found {actual_selection_sites:?}"
    );
    let indexed_mask_y_store = indexed_menu_mask_y_store_bytes()?;
    let actual_mask_y_sites = find_bank_local_sequence(source.prg(), &indexed_mask_y_store)?;
    let expected_mask_y_sites = INDEXED_MENU_MASK_Y_STORE_SITES.to_vec();
    ensure!(
        actual_mask_y_sites == expected_mask_y_sites,
        "source indexed menu-mask Y-store sites changed: expected {expected_mask_y_sites:?}, found {actual_mask_y_sites:?}"
    );
    let indexed_increments = bind_guarded_menu_selection_increments(source)?;

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
    for guard in guarded_routine_sites() {
        let routine = guarded_indexed_store_routine(guard)?;
        let bank = guard.bank;
        let address = guard.address;
        let end = guard.end;
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
    guarded_routines.push(indexed_increments.guard_evidence);

    ensure!(
        all_index_and_status_values_preserve_source_effects(
            MENU_MASK_BASE_ADDRESS,
            SAFE_MENU_MASK_INDEX_LIMIT,
        ) && all_index_and_status_values_preserve_source_effects(
            MENU_SELECTION_BASE_ADDRESS,
            SAFE_MENU_SELECTION_INDEX_LIMIT,
        ),
        "guarded indexed menu store does not preserve the source effect domain"
    );

    Ok(SourceIndexedMapperAliasSafety {
        scope: "all exact source STA $7FEE,X, STA $7FF3,X, STA $7FEE,Y, and INC $7FF3,X sites in banks 06 and 0B, including typed producer or post-write tails and their complete 0x00..0xFF effective-address domains",
        closure_claim: "complete for every source occurrence of these four indexed write forms; other source indexed, indirect, or synthesized writes that can enter mapper165 registers remain in the global executable-write audit",
        source_prg_banks: [MAP_MENU_PRG_BANK, FRONT_END_PRG_BANK],
        unguarded_indexed_store_site_count: actual_sites.len()
            + actual_selection_sites.len()
            + actual_mask_y_sites.len(),
        rewritten_indexed_store_site_count: INDEXED_MENU_MASK_STORE_SITES.len()
            + INDEXED_MENU_SELECTION_STORE_SITES.len()
            + INDEXED_MENU_MASK_Y_STORE_SITES.len(),
        unguarded_indexed_increment_site_count: indexed_increments.source_site_count,
        rewritten_indexed_increment_site_count: source_indexed_menu_selection_increment_sites()
            .len(),
        guarded_routines,
        safe_menu_mask_index_limit: SAFE_MENU_MASK_INDEX_LIMIT,
        safe_menu_selection_index_limit: SAFE_MENU_SELECTION_INDEX_LIMIT,
        all_index_and_status_values_preserve_source_effects: true,
    })
}

pub(super) fn install_guarded_indexed_menu_stores(image: &mut TrackedImage) -> Result<()> {
    for guard in guarded_routine_sites() {
        let routine = guarded_indexed_store_routine(guard)?;
        image.write_expected(
            format!(
                "guard bank {:02X} indexed {} stores from mapper165 register aliases",
                guard.bank, guard.role,
            ),
            switchable_bank_file_offset(guard.bank, guard.address)?,
            &vec![0xFF; routine.len()],
            &routine,
        )?;
    }

    for family in indexed_store_families() {
        for (bank, cpu_address) in family.sites {
            let expected = assemble_at(
                cpu_address,
                &[indexed_store_instruction(
                    family.index_register,
                    family.base_address,
                )],
            )?;
            let replacement = assemble_at(
                cpu_address,
                &[Instruction::JsrAbsolute(guard_address_for_store(
                    bank,
                    family.base_address,
                    family.index_register,
                )?)],
            )?;
            ensure!(
                replacement.len() == expected.len(),
                "guarded indexed {} store replacement length changed",
                family.role,
            );
            image.write_expected(
                format!("route bank {bank:02X}:${cpu_address:04X} indexed {} store through bounded effective address", family.role),
                switchable_bank_file_offset(bank, cpu_address)?,
                &expected,
                &replacement,
            )?;
        }
    }
    install_guarded_menu_selection_increments(image)?;
    Ok(())
}

pub(super) fn verify_installed_guarded_indexed_menu_stores(candidate: &Rom) -> Result<()> {
    for guard in guarded_routine_sites() {
        let routine = guarded_indexed_store_routine(guard)?;
        let cave_file_offset = switchable_bank_file_offset(guard.bank, guard.address)?;
        ensure!(
            candidate.data()[cave_file_offset..cave_file_offset + routine.len()] == routine,
            "installed guarded indexed-store routine at bank {:02X}:${:04X} changed",
            guard.bank,
            guard.address,
        );
    }
    verify_installed_guarded_menu_selection_increments(candidate)?;

    for family in indexed_store_families() {
        for &(bank, cpu_address) in &family.sites {
            let expected_installed = assemble_at(
                cpu_address,
                &[Instruction::JsrAbsolute(guard_address_for_store(
                    bank,
                    family.base_address,
                    family.index_register,
                )?)],
            )?;
            let file_offset = switchable_bank_file_offset(bank, cpu_address)?;
            ensure!(
                candidate.data()[file_offset..file_offset + expected_installed.len()]
                    == expected_installed,
                "installed guarded indexed {} store at bank {bank:02X}:${cpu_address:04X} changed",
                family.role,
            );
        }

        let remaining_direct_stores = find_bank_local_sequence(
            candidate.prg(),
            &assemble_at(
                0x8000,
                &[indexed_store_instruction(
                    family.index_register,
                    family.base_address,
                )],
            )?,
        )?;
        let expected_guarded_store_bodies = guarded_routine_sites()
            .into_iter()
            .filter(|guard| {
                guard.base_address == family.base_address
                    && guard.index_register == family.index_register
            })
            .map(|guard| (guard.bank, guard.address + 0x05))
            .collect::<Vec<_>>();
        ensure!(
            remaining_direct_stores == expected_guarded_store_bodies,
            "installed indexed {} stores are not confined to their guarded routine bodies: expected {expected_guarded_store_bodies:?}, found {remaining_direct_stores:?}",
            family.role,
        );
    }
    for guard in guarded_routine_sites() {
        let expected_count = indexed_store_families()
            .into_iter()
            .find_map(|family| {
                (family.base_address == guard.base_address
                    && family.index_register == guard.index_register)
                    .then_some(
                        family
                            .sites
                            .iter()
                            .filter(|(site_bank, _)| *site_bank == guard.bank)
                            .count(),
                    )
            })
            .context("guard has no indexed store family")?;
        let installed_direct_transfers =
            count_direct_transfers_to_range(candidate.prg(), guard.address, guard.end)?;
        ensure!(
            installed_direct_transfers == expected_count,
            "installed bank {:02X} {} guard transfer count changed: expected {expected_count}, found {installed_direct_transfers}",
            guard.bank,
            guard.role,
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

fn indexed_menu_selection_store_bytes() -> Result<Vec<u8>> {
    assemble_at(
        0x8000,
        &[Instruction::StaAbsoluteX(MENU_SELECTION_BASE_ADDRESS)],
    )
}

fn indexed_menu_mask_y_store_bytes() -> Result<Vec<u8>> {
    assemble_at(0x8000, &[Instruction::StaAbsoluteY(MENU_MASK_BASE_ADDRESS)])
}

fn indexed_store_families() -> [IndexedStoreFamily; 3] {
    [
        IndexedStoreFamily {
            role: "menu-mask-x",
            base_address: MENU_MASK_BASE_ADDRESS,
            index_register: IndexRegister::X,
            sites: INDEXED_MENU_MASK_STORE_SITES.to_vec(),
        },
        IndexedStoreFamily {
            role: "menu-selection-x",
            base_address: MENU_SELECTION_BASE_ADDRESS,
            index_register: IndexRegister::X,
            sites: INDEXED_MENU_SELECTION_STORE_SITES.to_vec(),
        },
        IndexedStoreFamily {
            role: "menu-mask-y",
            base_address: MENU_MASK_BASE_ADDRESS,
            index_register: IndexRegister::Y,
            sites: INDEXED_MENU_MASK_Y_STORE_SITES.to_vec(),
        },
    ]
}

fn indexed_store_instruction(index_register: IndexRegister, base_address: u16) -> Instruction {
    match index_register {
        IndexRegister::X => Instruction::StaAbsoluteX(base_address),
        IndexRegister::Y => Instruction::StaAbsoluteY(base_address),
    }
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

#[derive(Clone, Copy)]
struct IndexedStoreGuard {
    role: &'static str,
    bank: u8,
    address: u16,
    end: u16,
    base_address: u16,
    index_limit: u8,
    index_register: IndexRegister,
}

fn guarded_routine_sites() -> [IndexedStoreGuard; 5] {
    [
        IndexedStoreGuard {
            role: "menu-mask",
            bank: MAP_MENU_PRG_BANK,
            address: MAP_MENU_GUARD_ADDRESS,
            end: MAP_MENU_GUARD_END,
            base_address: MENU_MASK_BASE_ADDRESS,
            index_limit: SAFE_MENU_MASK_INDEX_LIMIT,
            index_register: IndexRegister::X,
        },
        IndexedStoreGuard {
            role: "menu-selection",
            bank: MAP_MENU_PRG_BANK,
            address: MAP_MENU_SELECTION_GUARD_ADDRESS,
            end: MAP_MENU_SELECTION_GUARD_END,
            base_address: MENU_SELECTION_BASE_ADDRESS,
            index_limit: SAFE_MENU_SELECTION_INDEX_LIMIT,
            index_register: IndexRegister::X,
        },
        IndexedStoreGuard {
            role: "menu-mask-y",
            bank: MAP_MENU_PRG_BANK,
            address: MAP_MENU_MASK_Y_GUARD_ADDRESS,
            end: MAP_MENU_MASK_Y_GUARD_END,
            base_address: MENU_MASK_BASE_ADDRESS,
            index_limit: SAFE_MENU_MASK_INDEX_LIMIT,
            index_register: IndexRegister::Y,
        },
        IndexedStoreGuard {
            role: "menu-selection",
            bank: FRONT_END_PRG_BANK,
            address: FRONT_END_SELECTION_GUARD_ADDRESS,
            end: FRONT_END_SELECTION_GUARD_END,
            base_address: MENU_SELECTION_BASE_ADDRESS,
            index_limit: SAFE_MENU_SELECTION_INDEX_LIMIT,
            index_register: IndexRegister::X,
        },
        IndexedStoreGuard {
            role: "menu-mask",
            bank: FRONT_END_PRG_BANK,
            address: FRONT_END_GUARD_ADDRESS,
            end: FRONT_END_GUARD_END,
            base_address: MENU_MASK_BASE_ADDRESS,
            index_limit: SAFE_MENU_MASK_INDEX_LIMIT,
            index_register: IndexRegister::X,
        },
    ]
}

fn guard_address_for_store(
    bank: u8,
    base_address: u16,
    index_register: IndexRegister,
) -> Result<u16> {
    guarded_routine_sites()
        .into_iter()
        .find(|guard| {
            guard.bank == bank
                && guard.base_address == base_address
                && guard.index_register == index_register
        })
        .map(|guard| guard.address)
        .with_context(|| {
            format!(
                "bank {bank:02X} has no indexed store guard for ${base_address:04X},{index_register:?}"
            )
        })
}

fn guarded_indexed_store_routine(guard: IndexedStoreGuard) -> Result<Vec<u8>> {
    let no_write = guard.address + 0x08;
    let compare = match guard.index_register {
        IndexRegister::X => Instruction::CpxImmediate(guard.index_limit),
        IndexRegister::Y => Instruction::CpyImmediate(guard.index_limit),
    };
    let store = indexed_store_instruction(guard.index_register, guard.base_address);
    let bytes = assemble_at(
        guard.address,
        &[
            Instruction::Php,
            compare,
            Instruction::BcsAbsolute(no_write),
            store,
            Instruction::Plp,
            Instruction::Rts,
        ],
    )?;
    ensure!(
        usize::from(guard.address) + bytes.len() == usize::from(guard.end),
        "guarded indexed {} store routine extent changed",
        guard.role,
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

fn all_index_and_status_values_preserve_source_effects(
    base_address: u16,
    safe_index_limit: u8,
) -> bool {
    (u8::MIN..=u8::MAX).all(|index| {
        (u8::MIN..=u8::MAX).all(|incoming_status| {
            let accumulator = index.wrapping_add(incoming_status);
            source_indexed_store_effect(base_address, index, accumulator, incoming_status)
                == guarded_indexed_store_effect(
                    base_address,
                    safe_index_limit,
                    index,
                    accumulator,
                    incoming_status,
                )
        })
    })
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct IndexedStoreEffect {
    accumulator: u8,
    index_value: u8,
    status: u8,
    prg_ram_write: Option<(u16, u8)>,
}

fn source_indexed_store_effect(
    base_address: u16,
    index: u8,
    accumulator: u8,
    incoming_status: u8,
) -> IndexedStoreEffect {
    let effective_address = base_address.wrapping_add(u16::from(index));
    IndexedStoreEffect {
        accumulator,
        index_value: index,
        status: incoming_status,
        prg_ram_write: (effective_address < FIRST_MAPPER165_REGISTER_ADDRESS)
            .then_some((effective_address, accumulator)),
    }
}

fn guarded_indexed_store_effect(
    base_address: u16,
    safe_index_limit: u8,
    index: u8,
    accumulator: u8,
    incoming_status: u8,
) -> IndexedStoreEffect {
    IndexedStoreEffect {
        accumulator,
        index_value: index,
        status: incoming_status,
        prg_ram_write: (index < safe_index_limit)
            .then(|| (base_address + u16::from(index), accumulator)),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn guarded_routines_preserve_every_index_and_status_without_mapper_writes() {
        for (base_address, index_limit) in [
            (MENU_MASK_BASE_ADDRESS, SAFE_MENU_MASK_INDEX_LIMIT),
            (MENU_SELECTION_BASE_ADDRESS, SAFE_MENU_SELECTION_INDEX_LIMIT),
        ] {
            assert!(all_index_and_status_values_preserve_source_effects(
                base_address,
                index_limit,
            ));
        }
        for guard in guarded_routine_sites() {
            assert_eq!(
                guarded_indexed_store_routine(guard).unwrap().len(),
                usize::from(guard.end - guard.address)
            );
        }
    }

    #[test]
    fn out_of_range_indices_do_not_reach_mapper165_registers() {
        for (base_address, index_limit) in [
            (MENU_MASK_BASE_ADDRESS, SAFE_MENU_MASK_INDEX_LIMIT),
            (MENU_SELECTION_BASE_ADDRESS, SAFE_MENU_SELECTION_INDEX_LIMIT),
        ] {
            for index in [index_limit, index_limit + 1, 0x80, 0xFF] {
                let effect =
                    guarded_indexed_store_effect(base_address, index_limit, index, 0xA5, 0xFF);
                let source_effective_address = base_address + u16::from(index);
                assert!(source_effective_address >= FIRST_MAPPER165_REGISTER_ADDRESS);
                assert_eq!(effect.prg_ram_write, None);
                assert_eq!(effect.accumulator, 0xA5);
                assert_eq!(effect.index_value, index);
                assert_eq!(effect.status, 0xFF);
            }
        }
    }

    #[test]
    fn valid_indices_keep_the_original_prg_ram_cell_and_value() {
        for (base_address, index_limit) in [
            (MENU_MASK_BASE_ADDRESS, SAFE_MENU_MASK_INDEX_LIMIT),
            (MENU_SELECTION_BASE_ADDRESS, SAFE_MENU_SELECTION_INDEX_LIMIT),
        ] {
            for index in 0..index_limit {
                let expected = base_address + u16::from(index);
                assert!(expected < FIRST_MAPPER165_REGISTER_ADDRESS);
                assert_eq!(
                    guarded_indexed_store_effect(base_address, index_limit, index, 0x5A, 0xA5,)
                        .prg_ram_write,
                    Some((expected, 0x5A))
                );
            }
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
