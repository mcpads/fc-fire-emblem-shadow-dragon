use std::collections::BTreeSet;

use anyhow::{Context, Result, ensure};
use serde::Serialize;

use crate::{
    rom::{HEADER_SIZE, Rom},
    sha1_hex,
};

use super::access_trace::{AccessDirection, AccessForm, AccessSite, RuntimeAccessTrace};
use super::{CANDIDATE_END, CANDIDATE_START};

const PRG_BANK_SIZE: usize = 16 * 1024;
const FIXED_BANK: u8 = 0x0F;

mod direct_operand_backstop;
mod indexed_queue;

use direct_operand_backstop::{
    WholePrgDirectOperandBackstop, bind_whole_prg_direct_operand_backstop,
};
use indexed_queue::{IndexedQueueContract, bind_indexed_queue_contract};

#[derive(Serialize)]
pub(super) struct RuntimeStateSourceAccessContract {
    strategy: &'static str,
    whole_prg_direct_operand_backstop: WholePrgDirectOperandBackstop,
    indexed_queue_contract: IndexedQueueContract,
    indirect_pointer_roles: Vec<PointerRoleContract>,
    indirect_pointer_role_count: usize,
    every_indexed_site_classified_once: bool,
    every_indirect_site_classified_once: bool,
    every_role_excludes_candidate: bool,
    source_lifetime_accesses_exclude_candidate: bool,
}

impl RuntimeStateSourceAccessContract {
    pub(super) fn source_lifetime_accesses_exclude_candidate(&self) -> bool {
        self.source_lifetime_accesses_exclude_candidate
    }

    pub(super) fn queue_bound_proven(&self) -> bool {
        self.indexed_queue_contract
            .candidate_begins_after_hard_queue_limit()
    }

    pub(super) fn indirect_access_ranges_proven(&self) -> bool {
        self.every_indirect_site_classified_once && self.every_role_excludes_candidate
    }
}

#[derive(Serialize)]
struct PointerRoleContract {
    role: &'static str,
    pointer_pair_hex: Vec<String>,
    address_basis: &'static str,
    sites: Vec<String>,
    possible_cpu_ranges_hex: Vec<String>,
    source_regions: Vec<SourceRegionBinding>,
    computed_pointer_domain: Option<ComputedPointerDomain>,
    candidate_excluded: bool,
}

#[derive(Serialize)]
struct ComputedPointerDomain {
    strategy: &'static str,
    selector_count: usize,
    pointer_value_count: usize,
    pointer_catalog_sha1: String,
    minimum_pointer_hex: String,
    maximum_pointer_hex: String,
    every_pointer_inside_switchable_prg: bool,
}

#[derive(Clone, Copy)]
struct PointerRoleSpec {
    role: &'static str,
    address_basis: &'static str,
    sites: &'static [AccessSite],
    possible_ranges: &'static [CpuRange],
    source_regions: &'static [SourceRegionSpec],
}

#[derive(Clone, Copy)]
struct CpuRange {
    start: u16,
    end: u16,
}

#[derive(Clone, Copy)]
struct SourceRegionSpec {
    bank: u8,
    address: u16,
    byte_count: usize,
    role: &'static str,
}

#[derive(Serialize)]
struct SourceRegionBinding {
    role: &'static str,
    prg_bank_hex: String,
    cpu_range_hex: String,
    byte_count: usize,
    source_sha1: String,
}

const fn read(bank: u8, address: u16, pointer: u16) -> AccessSite {
    AccessSite {
        bank,
        address,
        access: AccessDirection::Read,
        form: AccessForm::IndirectIndexedY,
        operand: pointer,
    }
}

const fn write(bank: u8, address: u16, pointer: u16) -> AccessSite {
    AccessSite {
        bank,
        address,
        access: AccessDirection::Write,
        form: AccessForm::IndirectIndexedY,
        operand: pointer,
    }
}

const fn region(bank: u8, address: u16, byte_count: usize, role: &'static str) -> SourceRegionSpec {
    SourceRegionSpec {
        bank,
        address,
        byte_count,
        role,
    }
}

const fn cpu_range(start: u16, end: u16) -> CpuRange {
    CpuRange { start, end }
}

const LINE_BUFFER_SITES: [AccessSite; 10] = [
    write(0x0A, 0x81FF, 0x06),
    write(0x0A, 0x8202, 0x06),
    write(0x0A, 0x8219, 0x06),
    write(0x0A, 0x821E, 0x06),
    write(0x0A, 0x8299, 0x06),
    write(0x0A, 0x82E6, 0x06),
    write(0x0A, 0x838A, 0x06),
    read(0x0A, 0x83C7, 0x06),
    read(0x0A, 0x8406, 0x06),
    write(0x0A, 0x8524, 0x06),
];
const DYNAMIC_STRING_SITE: [AccessSite; 1] = [read(0x0A, 0x8381, 0x08)];
const CHARACTER_ANIMATION_SITES: [AccessSite; 3] = [
    read(0x0A, 0x86DD, 0x04),
    read(0x0A, 0x86FD, 0x04),
    read(0x0A, 0x8716, 0x04),
];
const GENERIC_COPY_SOURCE_SITE: [AccessSite; 1] = [read(0x0F, 0xC211, 0x00)];
const GENERIC_COPY_DESTINATION_SITE: [AccessSite; 1] = [write(0x0F, 0xC213, 0x02)];
const LINE_BUFFER_FILL_SITE: [AccessSite; 1] = [write(0x0F, 0xC22D, 0x00)];
const STAGE_SERIALIZER_SITES: [AccessSite; 3] = [
    read(0x0F, 0xC850, 0x02),
    read(0x0F, 0xC856, 0x02),
    read(0x0F, 0xC88F, 0x02),
];
const SCRIPT_READER_SITE: [AccessSite; 1] = [read(0x0F, 0xE6A4, 0x76)];
const SCRIPT_POINTER_TABLE_SITES: [AccessSite; 2] =
    [read(0x0F, 0xE6E1, 0x04), read(0x0F, 0xE6EA, 0x04)];
const CHARACTER_SPRITE_SITES: [AccessSite; 9] = [
    read(0x0F, 0xE788, 0x42),
    read(0x0F, 0xE78D, 0x42),
    read(0x0F, 0xE792, 0x06),
    read(0x0F, 0xE797, 0x06),
    read(0x0F, 0xE7A2, 0x42),
    read(0x0F, 0xE7C4, 0x42),
    read(0x0F, 0xE7D4, 0x06),
    read(0x0F, 0xE7DD, 0x42),
    read(0x0F, 0xE804, 0x42),
];

const LINE_CODE: [SourceRegionSpec; 1] = [region(
    0x0A,
    0x81EA,
    0x0345,
    "line decoder, buffer directory, renderer, and line-to-line copy callers",
)];
const DYNAMIC_CODE: [SourceRegionSpec; 1] = [region(
    0x0A,
    0x8374,
    0x0032,
    "four-entry dynamic-string destination directory and append loop",
)];
const ANIMATION_CODE: [SourceRegionSpec; 1] = [region(
    0x0A,
    0x86D3,
    0x02F2,
    "character animation pointer directories and readers",
)];
const COPY_CODE: [SourceRegionSpec; 3] = [
    region(0x0F, 0xC209, 28, "generic bounded copy routine"),
    region(0x0A, 0x8170, 0x0057, "fixed-record copy caller"),
    region(0x0A, 0x84DA, 0x0055, "line-buffer copy caller"),
];
const FILL_CODE: [SourceRegionSpec; 2] = [
    region(0x0F, 0xC225, 24, "bounded fill routine"),
    region(0x0A, 0x802A, 0x0078, "main-dialogue cold-entry fill caller"),
];
const SERIALIZER_CODE: [SourceRegionSpec; 1] = [region(
    0x0F,
    0xC842,
    0x008B,
    "stage-buffer binding and bounded PPU queue serializer",
)];
const SCRIPT_READER_CODE: [SourceRegionSpec; 1] = [region(
    0x0F,
    0xE69C,
    0x0068,
    "banked script reader and directory pointer resolver",
)];
const SPRITE_CODE: [SourceRegionSpec; 1] = [region(
    0x0F,
    0xE759,
    0x00DA,
    "main-dialogue character sprite record consumer",
)];

const POINTER_ROLE_SPECS: [PointerRoleSpec; 10] = [
    PointerRoleSpec {
        role: "main_dialogue_line_buffers",
        address_basis: "six source-bound 0x20-byte SRAM rows selected by 0x77F8",
        sites: &LINE_BUFFER_SITES,
        possible_ranges: &[cpu_range(0x7832, 0x79D1)],
        source_regions: &LINE_CODE,
    },
    PointerRoleSpec {
        role: "dynamic_string_slots",
        address_basis: "four source-bound slot bases 0x78F2, 0x7902, 0x7912, and 0x7922",
        sites: &DYNAMIC_STRING_SITE,
        possible_ranges: &[cpu_range(0x78F2, 0x7A21)],
        source_regions: &DYNAMIC_CODE,
    },
    PointerRoleSpec {
        role: "character_animation_tables",
        address_basis: "source-bound bank-0A animation pointer tables in executable PRG space",
        sites: &CHARACTER_ANIMATION_SITES,
        possible_ranges: &[cpu_range(0x8000, 0xBFFF)],
        source_regions: &ANIMATION_CODE,
    },
    PointerRoleSpec {
        role: "generic_copy_sources",
        address_basis: "the two reachable callers select either source PRG records or an SRAM dialogue row",
        sites: &GENERIC_COPY_SOURCE_SITE,
        possible_ranges: &[cpu_range(0x7832, 0x79D1), cpu_range(0x8000, 0xBFFF)],
        source_regions: &COPY_CODE,
    },
    PointerRoleSpec {
        role: "generic_copy_destinations",
        address_basis: "the two reachable callers select literal 0x04D8 or one of the SRAM dialogue rows with bounded lengths",
        sites: &GENERIC_COPY_DESTINATION_SITE,
        possible_ranges: &[cpu_range(0x04D8, 0x04EB), cpu_range(0x7832, 0x78F1)],
        source_regions: &COPY_CODE,
    },
    PointerRoleSpec {
        role: "cold_entry_line_buffer_fill",
        address_basis: "cold entry selects the first SRAM row and fills exactly 0x00C0 bytes",
        sites: &LINE_BUFFER_FILL_SITE,
        possible_ranges: &[cpu_range(0x7832, 0x78F1)],
        source_regions: &FILL_CODE,
    },
    PointerRoleSpec {
        role: "stage_serializer_source",
        address_basis: "0xC842 binds pointer 0x02/0x03 to the 0x0310 stage descriptor and payload",
        sites: &STAGE_SERIALIZER_SITES,
        possible_ranges: &[cpu_range(0x0310, 0x0350)],
        source_regions: &SERIALIZER_CODE,
    },
    PointerRoleSpec {
        role: "banked_script_stream",
        address_basis: "the canonical dialogue directory and per-record pointers resolve inside the 0x8000..0xBFFF bank window",
        sites: &SCRIPT_READER_SITE,
        possible_ranges: &[cpu_range(0x8000, 0xBFFF)],
        source_regions: &SCRIPT_READER_CODE,
    },
    PointerRoleSpec {
        role: "script_pointer_tables",
        address_basis: "0xE6B2 resolves a directory entry and indexed record pointer in the switchable PRG window",
        sites: &SCRIPT_POINTER_TABLE_SITES,
        possible_ranges: &[cpu_range(0x8000, 0xBFFF)],
        source_regions: &SCRIPT_READER_CODE,
    },
    PointerRoleSpec {
        role: "character_sprite_records",
        address_basis: "the fixed character compositor follows source-bank record pointers and only reads their PRG-resident sprite records",
        sites: &CHARACTER_SPRITE_SITES,
        possible_ranges: &[cpu_range(0x8000, 0xBFFF)],
        source_regions: &SPRITE_CODE,
    },
];

pub(super) fn bind_runtime_state_source_accesses(
    source: &Rom,
    trace: &RuntimeAccessTrace,
) -> Result<RuntimeStateSourceAccessContract> {
    let whole_prg_direct_operand_backstop = bind_whole_prg_direct_operand_backstop(source)?;
    let indexed_queue_contract = bind_indexed_queue_contract(source, trace)?;
    let mut classified_indirect_sites = BTreeSet::new();
    let mut indirect_pointer_roles = Vec::new();

    for spec in POINTER_ROLE_SPECS {
        let sites = spec.sites.iter().copied().collect::<BTreeSet<_>>();
        ensure!(
            sites.len() == spec.sites.len(),
            "runtime-state pointer role {} repeats an access site",
            spec.role
        );
        ensure!(
            sites.is_subset(&trace.indirect_sites),
            "runtime-state pointer role {} contains a site outside the main-dialogue trace",
            spec.role
        );
        for site in &sites {
            ensure!(
                classified_indirect_sites.insert(*site),
                "main-dialogue indirect access site is assigned to more than one pointer role"
            );
        }
        let candidate_excluded = spec
            .possible_ranges
            .iter()
            .all(|range| !ranges_overlap(range.start, range.end, CANDIDATE_START, CANDIDATE_END));
        ensure!(
            candidate_excluded,
            "runtime-state pointer role {} reaches the candidate range",
            spec.role
        );
        let pointer_pair_hex = sites
            .iter()
            .map(|site| format!("0x{:02X}/0x{:02X}", site.operand, site.operand + 1))
            .collect::<BTreeSet<_>>()
            .into_iter()
            .collect();
        indirect_pointer_roles.push(PointerRoleContract {
            role: spec.role,
            pointer_pair_hex,
            address_basis: spec.address_basis,
            sites: report_site_keys(&sites),
            possible_cpu_ranges_hex: spec
                .possible_ranges
                .iter()
                .map(|range| format!("0x{:04X}..0x{:04X}", range.start, range.end))
                .collect(),
            source_regions: spec
                .source_regions
                .iter()
                .copied()
                .map(|region| bind_source_region(source, region))
                .collect::<Result<Vec<_>>>()?,
            computed_pointer_domain: (spec.role == "character_sprite_records")
                .then(|| bind_character_sprite_pointer_domain(source))
                .transpose()?,
            candidate_excluded,
        });
    }

    ensure!(
        classified_indirect_sites == trace.indirect_sites,
        "main-dialogue indirect access role census changed: classified {classified_indirect_sites:?}, traced {:?}",
        trace.indirect_sites
    );
    let every_role_excludes_candidate = indirect_pointer_roles
        .iter()
        .all(|role| role.candidate_excluded);
    let source_lifetime_accesses_exclude_candidate = trace.direct_overlaps.is_empty()
        && whole_prg_direct_operand_backstop.excludes_candidate()
        && indexed_queue_contract.candidate_begins_after_hard_queue_limit()
        && every_role_excludes_candidate;

    Ok(RuntimeStateSourceAccessContract {
        strategy: "partition every reachable computed access by stable producer role, bind each producer to canonical source, and reject any unclassified or overlapping site",
        whole_prg_direct_operand_backstop,
        indexed_queue_contract,
        indirect_pointer_role_count: indirect_pointer_roles.len(),
        indirect_pointer_roles,
        every_indexed_site_classified_once: true,
        every_indirect_site_classified_once: true,
        every_role_excludes_candidate,
        source_lifetime_accesses_exclude_candidate,
    })
}

fn bind_character_sprite_pointer_domain(source: &Rom) -> Result<ComputedPointerDomain> {
    const MAIN_DIALOGUE_SPRITE_DIRECTORY: u16 = 0x8AB3;
    const SELECTOR_END_EXCLUSIVE: u16 = 0x00FF;

    let animation_selectors = source_bytes(source, 0x0A, 0x8833, 0x00AF)?;
    let character_selectors = source_bytes(source, 0x0A, 0x89C5, 0x0060)?;
    ensure!(
        animation_selectors
            .iter()
            .chain(character_selectors)
            .all(|selector| *selector != 0xFF),
        "main-dialogue character compositor admits reserved selector FF"
    );
    let cold_entry = source_bytes(source, 0x0A, 0x806D, 5)?;
    ensure!(
        cold_entry == [0xA9, 0xFF, 0x8D, 0xC6, 0x05],
        "main-dialogue cold entry no longer selects its own sprite source bank"
    );
    let directory_pointer = source_bytes(source, 0x0A, 0xBFD0, 2)?;
    ensure!(
        u16::from_le_bytes([directory_pointer[0], directory_pointer[1]])
            == MAIN_DIALOGUE_SPRITE_DIRECTORY,
        "main-dialogue sprite directory pointer changed"
    );

    let mut pointer_values = Vec::new();
    for selector in 0..SELECTOR_END_EXCLUSIVE {
        let directory_entry = MAIN_DIALOGUE_SPRITE_DIRECTORY
            .checked_add(selector * 2)
            .context("main-dialogue sprite directory entry overflow")?;
        let bytes = source_bytes(source, 0x0A, directory_entry, 2)?;
        let record_pointer = u16::from_le_bytes([bytes[0], bytes[1]]);
        ensure!(
            (0x8000..=0xBFFF).contains(&record_pointer),
            "main-dialogue sprite selector {selector:02X} leaves switchable PRG"
        );
        pointer_values.push(record_pointer);

        let bytes = source_bytes(source, 0x0A, record_pointer, 2)?;
        let sprite_pointer = u16::from_le_bytes([bytes[0], bytes[1]]);
        ensure!(
            (0x8000..=0xBFFF).contains(&sprite_pointer),
            "main-dialogue sprite record {record_pointer:04X} leaves switchable PRG"
        );
        pointer_values.push(sprite_pointer);
    }
    let minimum_pointer = *pointer_values
        .iter()
        .min()
        .context("main-dialogue sprite pointer catalog is empty")?;
    let maximum_pointer = *pointer_values
        .iter()
        .max()
        .context("main-dialogue sprite pointer catalog is empty")?;
    let pointer_catalog = pointer_values
        .iter()
        .flat_map(|pointer| pointer.to_le_bytes())
        .collect::<Vec<_>>();

    Ok(ComputedPointerDomain {
        strategy: "cold entry owns bank 0A, every reachable selector source excludes reserved FF, and both pointer levels are enumerated for all 0x00..0xFE selectors",
        selector_count: usize::from(SELECTOR_END_EXCLUSIVE),
        pointer_value_count: pointer_values.len(),
        pointer_catalog_sha1: sha1_hex(&pointer_catalog),
        minimum_pointer_hex: format!("0x{minimum_pointer:04X}"),
        maximum_pointer_hex: format!("0x{maximum_pointer:04X}"),
        every_pointer_inside_switchable_prg: true,
    })
}

fn bind_source_region(source: &Rom, spec: SourceRegionSpec) -> Result<SourceRegionBinding> {
    let bytes = source_bytes(source, spec.bank, spec.address, spec.byte_count)?;
    let end = spec
        .address
        .checked_add(u16::try_from(spec.byte_count).context("source region length overflow")?)
        .context("source region address overflow")?;
    Ok(SourceRegionBinding {
        role: spec.role,
        prg_bank_hex: format!("0x{:02X}", spec.bank),
        cpu_range_hex: format!("0x{:04X}..0x{end:04X}", spec.address),
        byte_count: spec.byte_count,
        source_sha1: sha1_hex(bytes),
    })
}

fn source_bytes(source: &Rom, bank: u8, address: u16, byte_count: usize) -> Result<&[u8]> {
    let relative = if address >= 0xC000 {
        ensure!(
            bank == FIXED_BANK,
            "fixed source region uses a non-fixed bank"
        );
        usize::from(address - 0xC000)
    } else {
        ensure!(
            address >= 0x8000,
            "switchable source region is below 0x8000"
        );
        usize::from(address - 0x8000)
    };
    let offset = HEADER_SIZE + usize::from(bank) * PRG_BANK_SIZE + relative;
    source
        .data()
        .get(offset..offset + byte_count)
        .context("runtime-state source region is outside the source ROM")
}

fn report_site_keys(sites: &BTreeSet<AccessSite>) -> Vec<String> {
    sites
        .iter()
        .map(|site| {
            let access = match site.access {
                AccessDirection::Read => "read",
                AccessDirection::Write => "write",
            };
            format!("{:02X}:{:04X}:{access}", site.bank, site.address)
        })
        .collect()
}

fn ranges_overlap(left_start: u16, left_end: u16, right_start: u16, right_end: u16) -> bool {
    left_start <= right_end && right_start <= left_end
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pointer_role_partition_covers_exactly_thirty_two_sites() {
        let sites = POINTER_ROLE_SPECS
            .iter()
            .flat_map(|spec| spec.sites.iter().copied())
            .collect::<BTreeSet<_>>();

        assert_eq!(sites.len(), 32);
        assert_eq!(
            POINTER_ROLE_SPECS
                .iter()
                .map(|spec| spec.sites.len())
                .sum::<usize>(),
            sites.len()
        );
    }

    #[test]
    fn every_declared_pointer_role_excludes_the_candidate() {
        assert!(POINTER_ROLE_SPECS.iter().all(|spec| {
            spec.possible_ranges.iter().all(|range| {
                !ranges_overlap(range.start, range.end, CANDIDATE_START, CANDIDATE_END)
            })
        }));
    }
}
