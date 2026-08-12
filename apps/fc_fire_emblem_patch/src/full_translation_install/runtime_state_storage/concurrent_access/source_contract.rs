use std::collections::BTreeSet;

use anyhow::{Context, Result, ensure};
use serde::Serialize;

use crate::{
    dialogue_assets::MainDialogueTransitionMirror,
    rom::{HEADER_SIZE, Rom},
    sha1_hex,
};

use super::super::{
    CANDIDATE_END, CANDIDATE_START,
    access_trace::{AccessDirection, AccessForm, AccessSite, RuntimeAccessTrace},
};

const PRG_BANK_SIZE: usize = 16 * 1024;
const FIXED_BANK: u8 = 0x0F;
const BANK_DIRECTORY_POINTER: u16 = 0xBFC0;

#[derive(Serialize)]
pub(super) struct ConcurrentComputedAccessContract {
    strategy: &'static str,
    source_regions: Vec<SourceRegionBinding>,
    pointer_roles: Vec<ConcurrentPointerRole>,
    pointer_role_count: usize,
    bank_directory_domain: BankDirectoryDomain,
    every_nmi_indirect_site_classified_once: bool,
    every_audio_indirect_site_classified_once: bool,
    every_computed_access_excludes_candidate: bool,
}

impl ConcurrentComputedAccessContract {
    pub(super) fn every_computed_access_excludes_candidate(&self) -> bool {
        self.every_computed_access_excludes_candidate
    }
}

#[derive(Serialize)]
struct ConcurrentPointerRole {
    role: &'static str,
    execution_domain: &'static str,
    pointer_pair_hex: &'static str,
    address_basis: &'static str,
    sites: Vec<String>,
    possible_cpu_ranges_hex: Vec<&'static str>,
    candidate_excluded: bool,
}

#[derive(Serialize)]
struct BankDirectoryDomain {
    source_bank_count: usize,
    transition_mirror_bank_count: usize,
    pointer_base_count: usize,
    selector_count_per_base: usize,
    effective_address_count: usize,
    pointer_base_catalog_sha1: String,
    effective_address_catalog_sha1: String,
    minimum_effective_address_hex: String,
    maximum_effective_address_hex: String,
    candidate_excluded: bool,
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

const NMI_BANK_DIRECTORY_SITES: [AccessSite; 2] =
    [read(0x0F, 0xC2AA, 0x00), read(0x0F, 0xC2AE, 0x00)];
const NMI_PPU_STREAM_SITES: [AccessSite; 5] = [
    read(0x0F, 0xC3C3, 0x00),
    read(0x0F, 0xC3C9, 0x00),
    read(0x0F, 0xC3D0, 0x00),
    read(0x0F, 0xC3DB, 0x00),
    read(0x0F, 0xC3EC, 0x00),
];
const NMI_BLOCK_TRANSFER_SITES: [AccessSite; 3] = [
    read(0x0F, 0xD4EA, 0x1C),
    read(0x0F, 0xD4F0, 0x1C),
    read(0x0F, 0xD4FF, 0x1C),
];
const AUDIO_EVENT_STREAM_SITES: [AccessSite; 9] = [
    read(0x0E, 0x8112, 0xF4),
    read(0x0E, 0x8120, 0xF4),
    read(0x0E, 0x856C, 0xF4),
    read(0x0E, 0x8575, 0xF4),
    read(0x0E, 0x8588, 0xF4),
    read(0x0E, 0x861F, 0xF4),
    read(0x0E, 0x8624, 0xF4),
    read(0x0E, 0x8692, 0xF4),
    read(0x0E, 0x8697, 0xF4),
];
const AUDIO_CHANNEL_STREAM_SITES: [AccessSite; 5] = [
    read(0x0E, 0x82C9, 0xE8),
    read(0x0E, 0x82D3, 0xEA),
    read(0x0E, 0x8332, 0xEC),
    read(0x0E, 0x833C, 0xEE),
    read(0x0E, 0x8346, 0xF0),
];
const AUDIO_RECORD_SITES: [AccessSite; 14] = [
    read(0x0E, 0x86B1, 0xF2),
    read(0x0E, 0x86B7, 0xF2),
    read(0x0E, 0x86BD, 0xF2),
    read(0x0E, 0x86C3, 0xF2),
    read(0x0E, 0x86C8, 0xF2),
    read(0x0E, 0x86CD, 0xF2),
    read(0x0E, 0x86D2, 0xF2),
    read(0x0E, 0x86E7, 0xF2),
    read(0x0E, 0x86ED, 0xF2),
    read(0x0E, 0x86F3, 0xF2),
    read(0x0E, 0x86F9, 0xF2),
    read(0x0E, 0x86FE, 0xF2),
    read(0x0E, 0x8821, 0xF2),
    read(0x0E, 0x8826, 0xF2),
];

pub(super) fn bind_concurrent_computed_accesses(
    source: &Rom,
    source_nmi: &RuntimeAccessTrace,
    source_audio: &RuntimeAccessTrace,
    transition_mirrors: &[MainDialogueTransitionMirror],
    main_dialogue_queue_bound_proven: bool,
) -> Result<ConcurrentComputedAccessContract> {
    ensure!(
        main_dialogue_queue_bound_proven,
        "concurrent NMI PPU-stream role lost the shared queue bound"
    );

    let nmi_roles = [
        NMI_BANK_DIRECTORY_SITES.as_slice(),
        NMI_PPU_STREAM_SITES.as_slice(),
        NMI_BLOCK_TRANSFER_SITES.as_slice(),
    ];
    let audio_roles = [
        AUDIO_EVENT_STREAM_SITES.as_slice(),
        AUDIO_CHANNEL_STREAM_SITES.as_slice(),
        AUDIO_RECORD_SITES.as_slice(),
    ];
    ensure_exact_partition("source NMI", &source_nmi.indirect_sites, &nmi_roles)?;
    ensure_exact_partition("source audio", &source_audio.indirect_sites, &audio_roles)?;

    let bank_directory_domain = bind_bank_directory_domain(source, transition_mirrors)?;
    let pointer_roles = vec![
        role(
            "bank_local_nmi_transfer_directory",
            "fixed NMI",
            "0x00/0x01",
            "the active bank's source-bound 0xBFC0 pointer plus the positive 0x22 selector domain is enumerated for every source bank and every planned transition mirror",
            &NMI_BANK_DIRECTORY_SITES,
            &["enumerated 16-bit domain"],
            bank_directory_domain.candidate_excluded,
        ),
        role(
            "bounded_nmi_ppu_command_streams",
            "fixed NMI",
            "0x00/0x01",
            "the negative 0x22 route selects 0x04D8 and the main route selects the shared 0x0781 queue whose source appender stops at 0x07DF",
            &NMI_PPU_STREAM_SITES,
            &["0x04D8..0x05D7", "0x0781..0x07DF"],
            true,
        ),
        role(
            "fixed_nmi_block_transfer",
            "fixed NMI",
            "0x1C/0x1D",
            "both producers write literal 0x0302 before the bounded 0x20-byte PPU transfer",
            &NMI_BLOCK_TRANSFER_SITES,
            &["0x0302..0x03FF"],
            true,
        ),
        role(
            "audio_event_command_streams",
            "bank 0E audio",
            "0xF4/0xF5",
            "the source audio event directory, saved loop frames, and nested command pointers remain in the selected bank-0E PRG data window",
            &AUDIO_EVENT_STREAM_SITES,
            &["0x8000..0xBFFF"],
            true,
        ),
        role(
            "audio_channel_streams",
            "bank 0E audio",
            "0xE8..0xF1",
            "the source audio record parser supplies five channel command pointers from bank-0E PRG records",
            &AUDIO_CHANNEL_STREAM_SITES,
            &["0x8000..0xBFFF"],
            true,
        ),
        role(
            "audio_nested_records",
            "bank 0E audio",
            "0xF2/0xF3",
            "source-bound event directories and nested record commands supply only bank-0E PRG record pointers at these read sites",
            &AUDIO_RECORD_SITES,
            &["0x8000..0xBFFF"],
            true,
        ),
    ];
    let every_computed_access_excludes_candidate =
        pointer_roles.iter().all(|role| role.candidate_excluded);
    ensure!(
        every_computed_access_excludes_candidate,
        "a concurrent computed-access role reaches the dialogue runtime-state candidate"
    );

    Ok(ConcurrentComputedAccessContract {
        strategy: "partition every concurrent NMI and audio indirect access by producer role; enumerate bank-local NMI directory targets and source-bind every remaining bounded RAM or PRG domain",
        source_regions: vec![
            bind_source_region(
                source,
                FIXED_BANK,
                0xC2A6,
                0x015B,
                "NMI bank directory and bounded PPU command stream consumers",
            )?,
            bind_source_region(
                source,
                FIXED_BANK,
                0xD4BD,
                0x005C,
                "NMI fixed block-transfer producers and consumer",
            )?,
            bind_source_region(
                source,
                0x0E,
                0x8000,
                PRG_BANK_SIZE,
                "complete source audio code and data domain",
            )?,
        ],
        pointer_role_count: pointer_roles.len(),
        pointer_roles,
        bank_directory_domain,
        every_nmi_indirect_site_classified_once: true,
        every_audio_indirect_site_classified_once: true,
        every_computed_access_excludes_candidate,
    })
}

fn bind_bank_directory_domain(
    source: &Rom,
    transition_mirrors: &[MainDialogueTransitionMirror],
) -> Result<BankDirectoryDomain> {
    const SOURCE_SWITCHABLE_BANK_COUNT: usize = 15;
    const POSITIVE_SELECTOR_COUNT: usize = 0x7F;

    let mut pointer_bases = Vec::new();
    for bank in 0..SOURCE_SWITCHABLE_BANK_COUNT {
        let bank = u8::try_from(bank).expect("source switchable bank count fits u8");
        let bytes = source_bytes(source, bank, BANK_DIRECTORY_POINTER, 2)?;
        pointer_bases.push(u16::from_le_bytes([bytes[0], bytes[1]]));
    }
    ensure!(
        transition_mirrors.len() == 5,
        "transition mirror population changed before NMI directory analysis"
    );
    for mirror in transition_mirrors {
        ensure!(
            mirror.material.len() == PRG_BANK_SIZE,
            "transition mirror is not one complete 16 KiB bank"
        );
        let offset = usize::from(BANK_DIRECTORY_POINTER - 0x8000);
        let bytes = mirror
            .material
            .get(offset..offset + 2)
            .context("transition mirror has no NMI directory pointer cell")?;
        pointer_bases.push(u16::from_le_bytes([bytes[0], bytes[1]]));
    }

    let mut effective_addresses = BTreeSet::new();
    for base in &pointer_bases {
        for selector in 1..=POSITIVE_SELECTOR_COUNT {
            let y =
                u16::try_from((selector - 1) * 2).expect("positive NMI selector offset fits u16");
            effective_addresses.insert(base.wrapping_add(y));
            effective_addresses.insert(base.wrapping_add(y + 1));
        }
    }
    let candidate_excluded = effective_addresses
        .iter()
        .all(|address| !(CANDIDATE_START..=CANDIDATE_END).contains(address));
    ensure!(
        candidate_excluded,
        "a source or transition-mirror NMI directory target reaches the dialogue runtime state"
    );
    let pointer_catalog = pointer_bases
        .iter()
        .flat_map(|pointer| pointer.to_le_bytes())
        .collect::<Vec<_>>();
    let effective_catalog = effective_addresses
        .iter()
        .flat_map(|address| address.to_le_bytes())
        .collect::<Vec<_>>();
    let minimum = effective_addresses
        .first()
        .context("NMI directory effective-address catalog is empty")?;
    let maximum = effective_addresses
        .last()
        .context("NMI directory effective-address catalog is empty")?;

    Ok(BankDirectoryDomain {
        source_bank_count: SOURCE_SWITCHABLE_BANK_COUNT,
        transition_mirror_bank_count: transition_mirrors.len(),
        pointer_base_count: pointer_bases.len(),
        selector_count_per_base: POSITIVE_SELECTOR_COUNT,
        effective_address_count: effective_addresses.len(),
        pointer_base_catalog_sha1: sha1_hex(&pointer_catalog),
        effective_address_catalog_sha1: sha1_hex(&effective_catalog),
        minimum_effective_address_hex: format!("0x{minimum:04X}"),
        maximum_effective_address_hex: format!("0x{maximum:04X}"),
        candidate_excluded,
    })
}

fn ensure_exact_partition(
    domain: &str,
    traced: &BTreeSet<AccessSite>,
    roles: &[&[AccessSite]],
) -> Result<()> {
    let mut classified = BTreeSet::new();
    for sites in roles {
        for site in *sites {
            ensure!(
                classified.insert(*site),
                "{domain} indirect site is classified more than once"
            );
        }
    }
    ensure!(
        &classified == traced,
        "{domain} indirect access role census changed: classified {classified:?}, traced {traced:?}"
    );
    Ok(())
}

fn role(
    role: &'static str,
    execution_domain: &'static str,
    pointer_pair_hex: &'static str,
    address_basis: &'static str,
    sites: &[AccessSite],
    possible_cpu_ranges_hex: &[&'static str],
    candidate_excluded: bool,
) -> ConcurrentPointerRole {
    ConcurrentPointerRole {
        role,
        execution_domain,
        pointer_pair_hex,
        address_basis,
        sites: sites
            .iter()
            .map(|site| format!("{:02X}:{:04X}:read", site.bank, site.address))
            .collect(),
        possible_cpu_ranges_hex: possible_cpu_ranges_hex.to_vec(),
        candidate_excluded,
    }
}

fn bind_source_region(
    source: &Rom,
    bank: u8,
    address: u16,
    byte_count: usize,
    role: &'static str,
) -> Result<SourceRegionBinding> {
    let bytes = source_bytes(source, bank, address, byte_count)?;
    let end = address
        .checked_add(u16::try_from(byte_count).context("concurrent source region overflow")?)
        .context("concurrent source region overflow")?;
    Ok(SourceRegionBinding {
        role,
        prg_bank_hex: format!("0x{bank:02X}"),
        cpu_range_hex: format!("0x{address:04X}..0x{end:04X}"),
        byte_count,
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
        .context("concurrent computed-access source region is outside the source ROM")
}
