use std::collections::BTreeSet;

use anyhow::{Context, Result, ensure};
use serde::Serialize;

use crate::{
    rom::{HEADER_SIZE, Rom},
    sha1_hex,
    typed_source::decode_rp2a03_sequence,
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
    audio_lookup_pointer_domain: AudioLookupPointerDomain,
    every_nmi_indirect_site_classified_once: bool,
    every_audio_indirect_site_classified_once: bool,
    every_computed_access_excludes_candidate: bool,
    #[serde(skip)]
    indirect_write_sites_below_mapper_space: BTreeSet<(u8, u16, u8)>,
}

impl ConcurrentComputedAccessContract {
    pub(super) fn every_computed_access_excludes_candidate(&self) -> bool {
        self.every_computed_access_excludes_candidate
    }

    pub(super) fn indirect_write_sites_below_mapper_space(&self) -> &BTreeSet<(u8, u16, u8)> {
        &self.indirect_write_sites_below_mapper_space
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
struct AudioLookupPointerDomain {
    lookup_reader_cpu_range_hex: &'static str,
    call_site_count: usize,
    call_sites_hex: Vec<String>,
    pointer_bases_hex: Vec<String>,
    index_domain_hex: &'static str,
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

const fn write(bank: u8, address: u16, pointer: u16) -> AccessSite {
    AccessSite {
        bank,
        address,
        access: AccessDirection::Write,
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
const AUDIO_APU_TEMPLATE_READ_SITES: [AccessSite; 1] = [read(0x0E, 0x87F4, 0xF4)];
const AUDIO_APU_REGISTER_WRITE_SITES: [AccessSite; 1] = [write(0x0E, 0x87F6, 0xF2)];
const AUDIO_LOOKUP_READ_SITES: [AccessSite; 1] = [read(0x0E, 0x8C1F, 0xF2)];
const AUDIO_APU_REGISTER_COPY_CODE: [u8; 39] = [
    0xA9, 0x00, 0xF0, 0x0A, 0xA9, 0x08, 0xD0, 0x06, 0xA9, 0x0C, 0xD0, 0x02, 0xA9, 0x04, 0x85, 0xF2,
    0xA9, 0x40, 0x85, 0xF3, 0x84, 0xF4, 0xA9, 0x89, 0x85, 0xF5, 0xA0, 0x00, 0xB1, 0xF4, 0x91, 0xF2,
    0xC8, 0x98, 0xC9, 0x04, 0xD0, 0xF6, 0x60,
];
const AUDIO_LOOKUP_READER_CODE: [u8; 14] = [
    0x86, 0xF2, 0x84, 0xF3, 0xAE, 0x55, 0x06, 0x8A, 0x4A, 0xA8, 0xB1, 0xF2, 0x85, 0xF6,
];
const AUDIO_LOOKUP_CALLS: [(u16, u16); 13] = [
    (0x8B43, 0x8C6C),
    (0x8B81, 0x8C3C),
    (0x8BEC, 0x8DE4),
    (0x8C08, 0x8E2F),
    (0x8CD5, 0x8DFF),
    (0x8CF7, 0x8DFF),
    (0x8D44, 0x8E17),
    (0x8D71, 0x8DC9),
    (0x8DBA, 0x8C3C),
    (0x904A, 0x8E8F),
    (0x906A, 0x8E8F),
    (0x939E, 0x8C9C),
    (0x9404, 0x8CBC),
];

pub(super) fn bind_concurrent_computed_accesses(
    source: &Rom,
    source_nmi: &RuntimeAccessTrace,
    source_audio: &RuntimeAccessTrace,
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
        AUDIO_APU_TEMPLATE_READ_SITES.as_slice(),
        AUDIO_APU_REGISTER_WRITE_SITES.as_slice(),
        AUDIO_LOOKUP_READ_SITES.as_slice(),
    ];
    ensure_exact_partition("source NMI", &source_nmi.indirect_sites, &nmi_roles)?;
    ensure_exact_partition("source audio", &source_audio.indirect_sites, &audio_roles)?;

    let bank_directory_domain = bind_bank_directory_domain(source)?;
    bind_audio_apu_register_copy(source)?;
    let audio_lookup_pointer_domain = bind_audio_lookup_pointer_domain(source)?;
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
        role(
            "audio_apu_register_templates",
            "bank 0E audio",
            "0xF4/0xF5",
            "the source-bound register-copy routine fixes the high byte to 0x89 and copies four bytes from a caller-selected low-byte base",
            &AUDIO_APU_TEMPLATE_READ_SITES,
            &["0x8900..0x8A02"],
            true,
        ),
        role(
            "audio_apu_register_writes",
            "bank 0E audio",
            "0xF2/0xF3",
            "four source entry points select low byte 0x00, 0x04, 0x08, or 0x0C, fix the high byte to 0x40, and copy exactly four bytes",
            &AUDIO_APU_REGISTER_WRITE_SITES,
            &["0x4000..0x400F"],
            true,
        ),
        role(
            "audio_indexed_lookup_tables",
            "bank 0E audio",
            "0xF2/0xF3",
            "thirteen typed callers load an immediate bank-0E table base before the shared reader derives a 0x00..0x7F index from 0x0655",
            &AUDIO_LOOKUP_READ_SITES,
            &["0x8C3C..0x8F0E"],
            audio_lookup_pointer_domain.candidate_excluded,
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
            bind_source_region(
                source,
                0x0E,
                0x87D8,
                AUDIO_APU_REGISTER_COPY_CODE.len(),
                "audio source-template to APU-register copy routine",
            )?,
            bind_source_region(
                source,
                0x0E,
                0x8C15,
                AUDIO_LOOKUP_READER_CODE.len(),
                "audio indexed lookup reader",
            )?,
        ],
        pointer_role_count: pointer_roles.len(),
        pointer_roles,
        bank_directory_domain,
        audio_lookup_pointer_domain,
        every_nmi_indirect_site_classified_once: true,
        every_audio_indirect_site_classified_once: true,
        every_computed_access_excludes_candidate,
        indirect_write_sites_below_mapper_space: BTreeSet::from([(0x0E, 0x87F6, 0xF2)]),
    })
}

fn bind_audio_apu_register_copy(source: &Rom) -> Result<()> {
    let bytes = source_bytes(source, 0x0E, 0x87D8, AUDIO_APU_REGISTER_COPY_CODE.len())?;
    ensure!(
        bytes == AUDIO_APU_REGISTER_COPY_CODE,
        "source audio APU-register copy routine changed"
    );
    decode_rp2a03_sequence(bytes, 0x87D8, "source audio APU-register copy")?;
    Ok(())
}

fn bind_audio_lookup_pointer_domain(source: &Rom) -> Result<AudioLookupPointerDomain> {
    let reader = source_bytes(source, 0x0E, 0x8C15, AUDIO_LOOKUP_READER_CODE.len())?;
    ensure!(
        reader == AUDIO_LOOKUP_READER_CODE,
        "source audio indexed lookup reader changed"
    );
    decode_rp2a03_sequence(reader, 0x8C15, "source audio indexed lookup reader")?;

    let mut pointer_bases = BTreeSet::new();
    for (call_site, pointer_base) in AUDIO_LOOKUP_CALLS {
        let sequence_start = call_site - 4;
        let expected = [
            0xA2,
            pointer_base as u8,
            0xA0,
            (pointer_base >> 8) as u8,
            0x20,
            0x15,
            0x8C,
        ];
        let actual = source_bytes(source, 0x0E, sequence_start, expected.len())?;
        ensure!(
            actual == expected,
            "source audio indexed lookup caller 0x{call_site:04X} changed"
        );
        decode_rp2a03_sequence(actual, sequence_start, "source audio indexed lookup caller")?;
        pointer_bases.insert(pointer_base);
    }

    let minimum = *pointer_bases
        .first()
        .context("source audio indexed lookup pointer set is empty")?;
    let maximum = pointer_bases
        .last()
        .copied()
        .context("source audio indexed lookup pointer set is empty")?
        .checked_add(0x7F)
        .context("source audio indexed lookup domain overflow")?;
    let candidate_excluded = maximum < CANDIDATE_START || minimum > CANDIDATE_END;
    ensure!(
        candidate_excluded,
        "source audio indexed lookup domain reaches the dialogue runtime state"
    );

    Ok(AudioLookupPointerDomain {
        lookup_reader_cpu_range_hex: "0x8C15..0x8C23",
        call_site_count: AUDIO_LOOKUP_CALLS.len(),
        call_sites_hex: AUDIO_LOOKUP_CALLS
            .iter()
            .map(|(site, _)| format!("0x{site:04X}"))
            .collect(),
        pointer_bases_hex: pointer_bases
            .iter()
            .map(|base| format!("0x{base:04X}"))
            .collect(),
        index_domain_hex: "0x00..0x7F",
        minimum_effective_address_hex: format!("0x{minimum:04X}"),
        maximum_effective_address_hex: format!("0x{maximum:04X}"),
        candidate_excluded,
    })
}

fn bind_bank_directory_domain(source: &Rom) -> Result<BankDirectoryDomain> {
    const SOURCE_SWITCHABLE_BANK_COUNT: usize = 15;
    const POSITIVE_SELECTOR_COUNT: usize = 0x7F;

    let mut pointer_bases = Vec::new();
    for bank in 0..SOURCE_SWITCHABLE_BANK_COUNT {
        let bank = u8::try_from(bank).expect("source switchable bank count fits u8");
        let bytes = source_bytes(source, bank, BANK_DIRECTORY_POINTER, 2)?;
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
        "a source NMI directory target reaches the dialogue runtime state"
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
            .map(|site| {
                let direction = match site.access {
                    AccessDirection::Read => "read",
                    AccessDirection::Write => "write",
                };
                format!("{:02X}:{:04X}:{direction}", site.bank, site.address)
            })
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn computed_access_role_reports_write_direction() {
        let role = role(
            "audio_apu_register_writes",
            "bank 0E audio",
            "0xF2/0xF3",
            "test",
            &AUDIO_APU_REGISTER_WRITE_SITES,
            &["0x4000..0x400F"],
            true,
        );

        assert_eq!(role.sites, vec!["0E:87F6:write"]);
    }
}
