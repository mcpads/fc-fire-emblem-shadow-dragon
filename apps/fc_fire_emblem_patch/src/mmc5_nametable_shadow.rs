use std::{fs, path::Path};

use anyhow::{Context, Result, ensure};
use serde::Serialize;

use crate::{
    mmc5_chr::{create_mmc5_chr_writer_probe_image, switchable_bank_file_offset},
    mmc5_prg::{SOURCE_RESET_ADDRESS, count_direct_transfers_to_range, fixed_bank_file_offset},
    rom::{EXPECTED_CHR_SHA1, EXPECTED_SOURCE_SHA1, PRG_SIZE, Rom},
    rp2a03::{Instruction, assemble_at},
    sha1_hex,
    static_analysis::find_absolute_write_candidates,
    tracked::TrackedImage,
};

const CHR_MODE_RESET_TAIL_ADDRESS: u16 = 0xFA65;
const PPU_ADDRESS_HIGH_HOOK_ADDRESS: u16 = 0xFA80;
const PPU_ADDRESS_LOW_HOOK_ADDRESS: u16 = 0xFA8D;
const PPU_DATA_HOOK_ADDRESS: u16 = 0xFA9A;
const PPU_STORE_HOOK_COMMON_ADDRESS: u16 = 0xFAA7;
const INSTALL_NAMETABLE_SHADOW_ADDRESS: u16 = 0xFAD0;
const RUNTIME_PAYLOAD_SOURCE_ADDRESS: u16 = 0xFB00;
const RUNTIME_PAYLOAD_LEN: usize = 0x200;

const RUNTIME_DISPATCH_ADDRESS: u16 = 0x6000;
const RUNTIME_ADDRESS_HIGH_ADDRESS: u16 = 0x6010;
const RUNTIME_ADDRESS_LOW_ADDRESS: u16 = 0x6020;
const RUNTIME_DATA_ADDRESS: u16 = 0x6030;
const RUNTIME_CHECK_NAMETABLE_ADDRESS: u16 = 0x6060;
const RUNTIME_VERTICAL_MIRROR_ADDRESS: u16 = 0x6084;
const RUNTIME_PHYSICAL_ADDRESS: u16 = 0x6090;
const RUNTIME_INCREMENT_ADDRESS: u16 = 0x60B0;
const RUNTIME_INCREMENT_ACROSS_ADDRESS: u16 = 0x60C8;
const RUNTIME_MASK_AND_RESTORE_ADDRESS: u16 = 0x60D8;
const RUNTIME_INITIALIZE_ADDRESS: u16 = 0x6100;

const PRG_RAM_BANK_REGISTER: u16 = 0x5113;
const SOURCE_PPU_CONTROL_SHADOW: u8 = 0xCD;
const SOURCE_MIRRORING_SHADOW: u8 = 0xC8;
const PPU_ADDRESS_REGISTER: u16 = 0x2006;
const PPU_DATA_REGISTER: u16 = 0x2007;

const RUNTIME_ADDRESS_HIGH_COUNT: u16 = 0x67E0;
const RUNTIME_ADDRESS_LOW_COUNT: u16 = 0x67E2;
const RUNTIME_DATA_COUNT: u16 = 0x67E4;
const RUNTIME_STATE_START: u16 = 0x67E0;
const RUNTIME_STATE_LEN: u8 = 0x20;
const RUNTIME_VALUE: u16 = 0x67F0;
const RUNTIME_SAVED_ZERO_PAGE_0: u16 = 0x67F2;
const RUNTIME_SAVED_ZERO_PAGE_1: u16 = 0x67F3;
const RUNTIME_PPU_ADDRESS_HIGH: u16 = 0x67F4;
const RUNTIME_PPU_ADDRESS_LOW: u16 = 0x67F5;
const RUNTIME_LOGICAL_NAMETABLE: u16 = 0x67F6;
const RUNTIME_MAGIC_START: u16 = 0x67F8;
const RUNTIME_MAGIC: &[u8; 4] = b"NTS1";
const PHYSICAL_NAMETABLE_START: u16 = 0x6800;
const PHYSICAL_NAMETABLE_END: u16 = 0x7000;

const OPERATION_ADDRESS_HIGH: u8 = 0;
const OPERATION_ADDRESS_LOW: u8 = 1;
const OPERATION_DATA: u8 = 2;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
enum PpuStoreKind {
    AddressHigh,
    AddressLow,
    Data,
}

impl PpuStoreKind {
    fn register(self) -> u16 {
        match self {
            Self::AddressHigh | Self::AddressLow => PPU_ADDRESS_REGISTER,
            Self::Data => PPU_DATA_REGISTER,
        }
    }

    fn hook_address(self) -> u16 {
        match self {
            Self::AddressHigh => PPU_ADDRESS_HIGH_HOOK_ADDRESS,
            Self::AddressLow => PPU_ADDRESS_LOW_HOOK_ADDRESS,
            Self::Data => PPU_DATA_HOOK_ADDRESS,
        }
    }
}

#[derive(Debug, Clone, Copy)]
enum PrgLocation {
    Switchable(u8),
    Fixed,
}

#[derive(Debug, Clone, Copy)]
struct PpuStoreSite {
    role: &'static str,
    location: PrgLocation,
    cpu_address: u16,
    kind: PpuStoreKind,
}

impl PpuStoreSite {
    fn file_offset(self) -> Result<usize> {
        match self.location {
            PrgLocation::Switchable(bank) => switchable_bank_file_offset(bank, self.cpu_address),
            PrgLocation::Fixed => fixed_bank_file_offset(self.cpu_address),
        }
    }

    fn prg_bank(self) -> u8 {
        match self.location {
            PrgLocation::Switchable(bank) => bank,
            PrgLocation::Fixed => 0x0F,
        }
    }
}

const PPU_STORE_SITES: &[PpuStoreSite] = &[
    PpuStoreSite {
        role: "bank 0D visible-tile clear address high",
        location: PrgLocation::Switchable(0x0D),
        cpu_address: 0x8499,
        kind: PpuStoreKind::AddressHigh,
    },
    PpuStoreSite {
        role: "bank 0D visible-tile clear address low",
        location: PrgLocation::Switchable(0x0D),
        cpu_address: 0x849E,
        kind: PpuStoreKind::AddressLow,
    },
    PpuStoreSite {
        role: "bank 0D visible-tile clear data",
        location: PrgLocation::Switchable(0x0D),
        cpu_address: 0x84AB,
        kind: PpuStoreKind::Data,
    },
    PpuStoreSite {
        role: "bank 0D attribute clear address high",
        location: PrgLocation::Switchable(0x0D),
        cpu_address: 0x84BB,
        kind: PpuStoreKind::AddressHigh,
    },
    PpuStoreSite {
        role: "bank 0D attribute clear address low",
        location: PrgLocation::Switchable(0x0D),
        cpu_address: 0x84C0,
        kind: PpuStoreKind::AddressLow,
    },
    PpuStoreSite {
        role: "bank 0D attribute clear data",
        location: PrgLocation::Switchable(0x0D),
        cpu_address: 0x84C7,
        kind: PpuStoreKind::Data,
    },
    PpuStoreSite {
        role: "bank 0D composite text address high",
        location: PrgLocation::Switchable(0x0D),
        cpu_address: 0x84F7,
        kind: PpuStoreKind::AddressHigh,
    },
    PpuStoreSite {
        role: "bank 0D composite text address low",
        location: PrgLocation::Switchable(0x0D),
        cpu_address: 0x84FC,
        kind: PpuStoreKind::AddressLow,
    },
    PpuStoreSite {
        role: "bank 0D composite text data",
        location: PrgLocation::Switchable(0x0D),
        cpu_address: 0x850D,
        kind: PpuStoreKind::Data,
    },
    PpuStoreSite {
        role: "bank 0D composite mark address high",
        location: PrgLocation::Switchable(0x0D),
        cpu_address: 0x8544,
        kind: PpuStoreKind::AddressHigh,
    },
    PpuStoreSite {
        role: "bank 0D composite mark address low",
        location: PrgLocation::Switchable(0x0D),
        cpu_address: 0x8549,
        kind: PpuStoreKind::AddressLow,
    },
    PpuStoreSite {
        role: "bank 0D composite mark data",
        location: PrgLocation::Switchable(0x0D),
        cpu_address: 0x854D,
        kind: PpuStoreKind::Data,
    },
    PpuStoreSite {
        role: "bank 0D composite text resume address high",
        location: PrgLocation::Switchable(0x0D),
        cpu_address: 0x8563,
        kind: PpuStoreKind::AddressHigh,
    },
    PpuStoreSite {
        role: "bank 0D composite text resume address low",
        location: PrgLocation::Switchable(0x0D),
        cpu_address: 0x8568,
        kind: PpuStoreKind::AddressLow,
    },
    PpuStoreSite {
        role: "fixed nametable clear address high",
        location: PrgLocation::Fixed,
        cpu_address: 0xC26C,
        kind: PpuStoreKind::AddressHigh,
    },
    PpuStoreSite {
        role: "fixed nametable clear address low",
        location: PrgLocation::Fixed,
        cpu_address: 0xC271,
        kind: PpuStoreKind::AddressLow,
    },
    PpuStoreSite {
        role: "fixed nametable clear data",
        location: PrgLocation::Fixed,
        cpu_address: 0xC27A,
        kind: PpuStoreKind::Data,
    },
    PpuStoreSite {
        role: "fixed palette address high",
        location: PrgLocation::Fixed,
        cpu_address: 0xC2BD,
        kind: PpuStoreKind::AddressHigh,
    },
    PpuStoreSite {
        role: "fixed palette address low",
        location: PrgLocation::Fixed,
        cpu_address: 0xC2C2,
        kind: PpuStoreKind::AddressLow,
    },
    PpuStoreSite {
        role: "fixed palette latch reset address high",
        location: PrgLocation::Fixed,
        cpu_address: 0xC2C5,
        kind: PpuStoreKind::AddressHigh,
    },
    PpuStoreSite {
        role: "fixed palette latch reset address low",
        location: PrgLocation::Fixed,
        cpu_address: 0xC2C8,
        kind: PpuStoreKind::AddressLow,
    },
    PpuStoreSite {
        role: "fixed queued transfer address high",
        location: PrgLocation::Fixed,
        cpu_address: 0xC3BF,
        kind: PpuStoreKind::AddressHigh,
    },
    PpuStoreSite {
        role: "fixed queued transfer address low",
        location: PrgLocation::Fixed,
        cpu_address: 0xC3C5,
        kind: PpuStoreKind::AddressLow,
    },
    PpuStoreSite {
        role: "fixed queued transfer data",
        location: PrgLocation::Fixed,
        cpu_address: 0xC3DD,
        kind: PpuStoreKind::Data,
    },
    PpuStoreSite {
        role: "fixed buffer transfer address high",
        location: PrgLocation::Fixed,
        cpu_address: 0xD4EC,
        kind: PpuStoreKind::AddressHigh,
    },
    PpuStoreSite {
        role: "fixed buffer transfer address low",
        location: PrgLocation::Fixed,
        cpu_address: 0xD4F2,
        kind: PpuStoreKind::AddressLow,
    },
    PpuStoreSite {
        role: "fixed buffer transfer data",
        location: PrgLocation::Fixed,
        cpu_address: 0xD501,
        kind: PpuStoreKind::Data,
    },
];

#[derive(Debug, Serialize)]
struct NametableShadowProbeReport {
    schema: u32,
    source_sha1: &'static str,
    base_chr_writer_probe_sha1: String,
    output_sha1: String,
    output_mapper: u16,
    prg_size: usize,
    chr_size: usize,
    chr_sha1: &'static str,
    direct_ppu_address_write_count: usize,
    direct_ppu_data_write_count: usize,
    all_direct_ppu_store_candidates_hooked: bool,
    per_byte_hook_design_eligible: bool,
    direct_fixed_code_transfer_count: usize,
    direct_payload_transfer_candidate_count: usize,
    runtime: RuntimeReport,
    store_sites: Vec<StoreSiteReport>,
    tracked_delta_writes: Vec<TrackedWrite>,
    unresolved_boundaries: Vec<&'static str>,
    release_eligible: bool,
}

#[derive(Debug, Serialize)]
struct RuntimeReport {
    prg_ram_bank: u8,
    payload_source_cpu_start: String,
    payload_cpu_start: String,
    payload_len: usize,
    state_cpu_start: String,
    state_len: usize,
    physical_nametable_cpu_start: String,
    physical_nametable_len: usize,
    initial_nametable_byte: String,
    mirroring_source: &'static str,
    address_increment_source: &'static str,
    initialization_magic: String,
}

#[derive(Debug, Serialize)]
struct StoreSiteReport {
    role: &'static str,
    prg_bank: String,
    cpu_address: String,
    file_offset: String,
    kind: PpuStoreKind,
    hook_cpu_address: String,
}

#[derive(Debug, Serialize)]
struct TrackedWrite {
    label: String,
    file_offset: String,
    len: usize,
}

pub struct BuildSummary {
    pub output_sha1: String,
    pub report_sha1: String,
    pub tracked_write_count: usize,
    pub hooked_store_count: usize,
}

pub fn build_mmc5_nametable_shadow_probe(
    source_path: &Path,
    output_path: &Path,
    report_path: &Path,
) -> Result<BuildSummary> {
    let source_rom = Rom::from_path(source_path)?;
    source_rom.verify_supported_japanese()?;
    validate_direct_ppu_store_census(&source_rom)?;

    let direct_fixed_code_transfer_count = count_direct_transfers_to_range(
        source_rom.prg(),
        PPU_ADDRESS_HIGH_HOOK_ADDRESS,
        RUNTIME_PAYLOAD_SOURCE_ADDRESS,
    )?;
    ensure!(
        direct_fixed_code_transfer_count == 0,
        "source has {direct_fixed_code_transfer_count} direct JSR or JMP references into the nametable-shadow fixed code range"
    );
    let direct_payload_transfer_candidate_count = count_direct_transfers_to_range(
        source_rom.prg(),
        RUNTIME_PAYLOAD_SOURCE_ADDRESS,
        RUNTIME_PAYLOAD_SOURCE_ADDRESS + RUNTIME_PAYLOAD_LEN as u16,
    )?;

    let chr_writer_probe = create_mmc5_chr_writer_probe_image(&source_rom)?;
    let base = chr_writer_probe.data().to_vec();
    let base_chr_writer_probe_sha1 = sha1_hex(&base);
    let mut image = TrackedImage::new(base.clone());

    redirect_chr_initializer_to_shadow_install(&mut image)?;
    install_fixed_routines(&mut image)?;
    install_runtime_payload(&mut image)?;
    for site in PPU_STORE_SITES {
        install_ppu_store_hook(&mut image, *site)?;
    }

    image.verify_all_changes_tracked(&base)?;
    let tracked_delta_writes = image
        .writes()
        .iter()
        .map(|write| TrackedWrite {
            label: write.label.clone(),
            file_offset: format!("0x{:06X}", write.offset),
            len: write.len,
        })
        .collect::<Vec<_>>();
    let output = image.into_data();
    let output_rom = Rom::parse(output.clone()).context("parse MMC5 nametable-shadow probe")?;
    ensure!(
        output_rom.mapper() == 5,
        "nametable-shadow probe mapper is not 5"
    );
    ensure!(
        output_rom.prg().len() == PRG_SIZE,
        "nametable-shadow probe changed PRG size"
    );
    ensure!(
        sha1_hex(output_rom.chr()) == EXPECTED_CHR_SHA1,
        "nametable-shadow probe changed source CHR"
    );

    let output_sha1 = sha1_hex(&output);
    let direct_ppu_address_write_count = PPU_STORE_SITES
        .iter()
        .filter(|site| site.kind != PpuStoreKind::Data)
        .count();
    let direct_ppu_data_write_count = PPU_STORE_SITES
        .iter()
        .filter(|site| site.kind == PpuStoreKind::Data)
        .count();
    let report = NametableShadowProbeReport {
        schema: 1,
        source_sha1: EXPECTED_SOURCE_SHA1,
        base_chr_writer_probe_sha1,
        output_sha1: output_sha1.clone(),
        output_mapper: output_rom.mapper(),
        prg_size: output_rom.prg().len(),
        chr_size: output_rom.chr().len(),
        chr_sha1: EXPECTED_CHR_SHA1,
        direct_ppu_address_write_count,
        direct_ppu_data_write_count,
        all_direct_ppu_store_candidates_hooked: true,
        per_byte_hook_design_eligible: false,
        direct_fixed_code_transfer_count,
        direct_payload_transfer_candidate_count,
        runtime: RuntimeReport {
            prg_ram_bank: 1,
            payload_source_cpu_start: format!("0x{RUNTIME_PAYLOAD_SOURCE_ADDRESS:04X}"),
            payload_cpu_start: format!("0x{RUNTIME_DISPATCH_ADDRESS:04X}"),
            payload_len: RUNTIME_PAYLOAD_LEN,
            state_cpu_start: format!("0x{RUNTIME_STATE_START:04X}"),
            state_len: usize::from(RUNTIME_STATE_LEN),
            physical_nametable_cpu_start: format!("0x{PHYSICAL_NAMETABLE_START:04X}"),
            physical_nametable_len: usize::from(PHYSICAL_NAMETABLE_END - PHYSICAL_NAMETABLE_START),
            initial_nametable_byte: "0xFF".to_owned(),
            mirroring_source: "source zero-page $C8: 0 vertical, nonzero horizontal",
            address_increment_source: "source PPUCTRL shadow $CD bit 2: 0 across, 1 down",
            initialization_magic: String::from_utf8_lossy(RUNTIME_MAGIC).into_owned(),
        },
        store_sites: PPU_STORE_SITES
            .iter()
            .map(|site| {
                Ok(StoreSiteReport {
                    role: site.role,
                    prg_bank: format!("0x{:02X}", site.prg_bank()),
                    cpu_address: format!("0x{:04X}", site.cpu_address),
                    file_offset: format!("0x{:06X}", site.file_offset()?),
                    kind: site.kind,
                    hook_cpu_address: format!("0x{:04X}", site.kind.hook_address()),
                })
            })
            .collect::<Result<Vec<_>>>()?,
        tracked_delta_writes,
        unresolved_boundaries: vec![
            "The shadow is a producer probe and is not yet connected to MMC5 ExRAM display attributes.",
            "The per-byte hooks preserve CPU-visible registers and flags but add enough cycles to break observed VBlank transfers; production work must batch at transfer boundaries.",
            "Indirectly addressed PPU register writes are not disproven; this probe closes the complete direct STA $2006/$2007 byte-pattern census only.",
            "The all-FF runtime payload source has direct JSR/JMP byte-pattern candidates; they are reported rather than interpreted as instruction-boundary references.",
            "PRG RAM bank 1 is isolated from the source save bank in Mesen, but save/load compatibility and other execution environments remain unverified.",
        ],
        release_eligible: false,
    };
    let report_bytes = serde_json::to_vec_pretty(&report)
        .context("serialize MMC5 nametable-shadow probe report")?;
    let report_sha1 = sha1_hex(&report_bytes);
    let tracked_write_count = report.tracked_delta_writes.len();

    write_file(output_path, &output)?;
    write_file(report_path, &report_bytes)?;
    Ok(BuildSummary {
        output_sha1,
        report_sha1,
        tracked_write_count,
        hooked_store_count: PPU_STORE_SITES.len(),
    })
}

fn validate_direct_ppu_store_census(source_rom: &Rom) -> Result<()> {
    let mut expected_address_offsets = PPU_STORE_SITES
        .iter()
        .filter(|site| site.kind != PpuStoreKind::Data)
        .map(|site| site.file_offset())
        .collect::<Result<Vec<_>>>()?;
    expected_address_offsets.sort_unstable();
    let mut expected_data_offsets = PPU_STORE_SITES
        .iter()
        .filter(|site| site.kind == PpuStoreKind::Data)
        .map(|site| site.file_offset())
        .collect::<Result<Vec<_>>>()?;
    expected_data_offsets.sort_unstable();

    let address_candidates = find_absolute_write_candidates(source_rom.prg(), PPU_ADDRESS_REGISTER);
    let data_candidates = find_absolute_write_candidates(source_rom.prg(), PPU_DATA_REGISTER);
    ensure!(
        address_candidates
            .iter()
            .all(|candidate| candidate.opcode == 0x8D),
        "source contains a direct non-STA write to $2006"
    );
    ensure!(
        data_candidates
            .iter()
            .all(|candidate| candidate.opcode == 0x8D),
        "source contains a direct non-STA write to $2007"
    );
    let mut actual_address_offsets = address_candidates
        .iter()
        .map(|candidate| candidate.file_offset)
        .collect::<Vec<_>>();
    actual_address_offsets.sort_unstable();
    let mut actual_data_offsets = data_candidates
        .iter()
        .map(|candidate| candidate.file_offset)
        .collect::<Vec<_>>();
    actual_data_offsets.sort_unstable();
    ensure!(
        actual_address_offsets == expected_address_offsets,
        "direct $2006 write census changed: expected {expected_address_offsets:02X?}, found {actual_address_offsets:02X?}"
    );
    ensure!(
        actual_data_offsets == expected_data_offsets,
        "direct $2007 write census changed: expected {expected_data_offsets:02X?}, found {actual_data_offsets:02X?}"
    );
    Ok(())
}

fn redirect_chr_initializer_to_shadow_install(image: &mut TrackedImage) -> Result<()> {
    image.write_expected(
        "redirect CHR initializer to nametable-shadow install",
        fixed_bank_file_offset(CHR_MODE_RESET_TAIL_ADDRESS)?,
        &assemble_at(
            CHR_MODE_RESET_TAIL_ADDRESS,
            &[Instruction::JmpAbsolute(SOURCE_RESET_ADDRESS)],
        )?,
        &assemble_at(
            CHR_MODE_RESET_TAIL_ADDRESS,
            &[Instruction::JmpAbsolute(INSTALL_NAMETABLE_SHADOW_ADDRESS)],
        )?,
    )
}

fn install_fixed_routines(image: &mut TrackedImage) -> Result<()> {
    for (role, address, instructions) in [
        (
            "PPU address-high store hook",
            PPU_ADDRESS_HIGH_HOOK_ADDRESS,
            ppu_store_hook(PPU_ADDRESS_REGISTER, OPERATION_ADDRESS_HIGH)?,
        ),
        (
            "PPU address-low store hook",
            PPU_ADDRESS_LOW_HOOK_ADDRESS,
            ppu_store_hook(PPU_ADDRESS_REGISTER, OPERATION_ADDRESS_LOW)?,
        ),
        (
            "PPU data store hook",
            PPU_DATA_HOOK_ADDRESS,
            ppu_store_hook(PPU_DATA_REGISTER, OPERATION_DATA)?,
        ),
        (
            "PPU store hook common",
            PPU_STORE_HOOK_COMMON_ADDRESS,
            ppu_store_hook_common()?,
        ),
        (
            "nametable-shadow installer",
            INSTALL_NAMETABLE_SHADOW_ADDRESS,
            install_nametable_shadow()?,
        ),
    ] {
        image.write_expected(
            format!("MMC5 {role}"),
            fixed_bank_file_offset(address)?,
            &vec![0xFF; instructions.len()],
            &instructions,
        )?;
    }
    Ok(())
}

fn ppu_store_hook(register: u16, operation: u8) -> Result<Vec<u8>> {
    assemble_at(
        match operation {
            OPERATION_ADDRESS_HIGH => PPU_ADDRESS_HIGH_HOOK_ADDRESS,
            OPERATION_ADDRESS_LOW => PPU_ADDRESS_LOW_HOOK_ADDRESS,
            OPERATION_DATA => PPU_DATA_HOOK_ADDRESS,
            _ => unreachable!(),
        },
        &[
            Instruction::StaAbsolute(register),
            Instruction::Php,
            Instruction::Pha,
            Instruction::LdaImmediate(operation),
            Instruction::JsrAbsolute(PPU_STORE_HOOK_COMMON_ADDRESS),
            Instruction::Pla,
            Instruction::Plp,
            Instruction::Rts,
        ],
    )
}

fn ppu_store_hook_common() -> Result<Vec<u8>> {
    assemble_at(
        PPU_STORE_HOOK_COMMON_ADDRESS,
        &[
            Instruction::Pha,
            Instruction::Txa,
            Instruction::Pha,
            Instruction::Tya,
            Instruction::Pha,
            Instruction::LdaImmediate(1),
            Instruction::StaAbsolute(PRG_RAM_BANK_REGISTER),
            Instruction::Tsx,
            Instruction::LdyAbsoluteX(0x0103),
            Instruction::LdaAbsoluteX(0x0106),
            Instruction::JsrAbsolute(RUNTIME_DISPATCH_ADDRESS),
            Instruction::LdaImmediate(0),
            Instruction::StaAbsolute(PRG_RAM_BANK_REGISTER),
            Instruction::Pla,
            Instruction::Tay,
            Instruction::Pla,
            Instruction::Tax,
            Instruction::Pla,
            Instruction::Rts,
        ],
    )
}

fn install_nametable_shadow() -> Result<Vec<u8>> {
    assemble_at(
        INSTALL_NAMETABLE_SHADOW_ADDRESS,
        &[
            Instruction::Php,
            Instruction::Pha,
            Instruction::Txa,
            Instruction::Pha,
            Instruction::Tya,
            Instruction::Pha,
            Instruction::LdaImmediate(1),
            Instruction::StaAbsolute(PRG_RAM_BANK_REGISTER),
            Instruction::LdxImmediate(0),
            Instruction::LdaAbsoluteX(RUNTIME_PAYLOAD_SOURCE_ADDRESS),
            Instruction::StaAbsoluteX(RUNTIME_DISPATCH_ADDRESS),
            Instruction::LdaAbsoluteX(RUNTIME_PAYLOAD_SOURCE_ADDRESS + 0x100),
            Instruction::StaAbsoluteX(RUNTIME_DISPATCH_ADDRESS + 0x100),
            Instruction::Inx,
            Instruction::BneAbsolute(INSTALL_NAMETABLE_SHADOW_ADDRESS + 0x0D),
            Instruction::JsrAbsolute(RUNTIME_INITIALIZE_ADDRESS),
            Instruction::LdaImmediate(0),
            Instruction::StaAbsolute(PRG_RAM_BANK_REGISTER),
            Instruction::Pla,
            Instruction::Tay,
            Instruction::Pla,
            Instruction::Tax,
            Instruction::Pla,
            Instruction::Plp,
            Instruction::JmpAbsolute(SOURCE_RESET_ADDRESS),
        ],
    )
}

fn install_runtime_payload(image: &mut TrackedImage) -> Result<()> {
    let payload = runtime_payload()?;
    image.write_expected(
        "MMC5 nametable-shadow runtime payload",
        fixed_bank_file_offset(RUNTIME_PAYLOAD_SOURCE_ADDRESS)?,
        &vec![0xFF; RUNTIME_PAYLOAD_LEN],
        &payload,
    )
}

fn runtime_payload() -> Result<Vec<u8>> {
    let mut payload = vec![0xFF; RUNTIME_PAYLOAD_LEN];
    for (address, instructions) in [
        (RUNTIME_DISPATCH_ADDRESS, runtime_dispatch()?),
        (RUNTIME_ADDRESS_HIGH_ADDRESS, runtime_address_high()?),
        (RUNTIME_ADDRESS_LOW_ADDRESS, runtime_address_low()?),
        (RUNTIME_DATA_ADDRESS, runtime_data_prepare()?),
        (RUNTIME_CHECK_NAMETABLE_ADDRESS, runtime_check_nametable()?),
        (RUNTIME_VERTICAL_MIRROR_ADDRESS, runtime_vertical_mirror()?),
        (RUNTIME_PHYSICAL_ADDRESS, runtime_physical_address()?),
        (RUNTIME_INCREMENT_ADDRESS, runtime_increment_address()?),
        (
            RUNTIME_INCREMENT_ACROSS_ADDRESS,
            runtime_increment_across()?,
        ),
        (
            RUNTIME_MASK_AND_RESTORE_ADDRESS,
            runtime_mask_and_restore()?,
        ),
        (RUNTIME_INITIALIZE_ADDRESS, runtime_initialize()?),
    ] {
        let start = usize::from(address - RUNTIME_DISPATCH_ADDRESS);
        let end = start
            .checked_add(instructions.len())
            .ok_or_else(|| anyhow::anyhow!("runtime payload range overflow"))?;
        ensure!(
            end <= payload.len(),
            "runtime routine at {address:04X} exceeds the payload"
        );
        ensure!(
            payload[start..end].iter().all(|byte| *byte == 0xFF),
            "runtime routine at {address:04X} overlaps another routine"
        );
        payload[start..end].copy_from_slice(&instructions);
    }
    Ok(payload)
}

fn runtime_dispatch() -> Result<Vec<u8>> {
    assemble_at(
        RUNTIME_DISPATCH_ADDRESS,
        &[
            Instruction::CpyImmediate(OPERATION_ADDRESS_HIGH),
            Instruction::BeqAbsolute(RUNTIME_ADDRESS_HIGH_ADDRESS),
            Instruction::CpyImmediate(OPERATION_ADDRESS_LOW),
            Instruction::BeqAbsolute(RUNTIME_ADDRESS_LOW_ADDRESS),
            Instruction::CpyImmediate(OPERATION_DATA),
            Instruction::BeqAbsolute(RUNTIME_DATA_ADDRESS),
            Instruction::Rts,
        ],
    )
}

fn runtime_address_high() -> Result<Vec<u8>> {
    assemble_at(
        RUNTIME_ADDRESS_HIGH_ADDRESS,
        &[
            Instruction::AndImmediate(0x3F),
            Instruction::StaAbsolute(RUNTIME_PPU_ADDRESS_HIGH),
            Instruction::IncAbsolute(RUNTIME_ADDRESS_HIGH_COUNT),
            Instruction::BneAbsolute(RUNTIME_ADDRESS_HIGH_ADDRESS + 0x0D),
            Instruction::IncAbsolute(RUNTIME_ADDRESS_HIGH_COUNT + 1),
            Instruction::Rts,
        ],
    )
}

fn runtime_address_low() -> Result<Vec<u8>> {
    assemble_at(
        RUNTIME_ADDRESS_LOW_ADDRESS,
        &[
            Instruction::StaAbsolute(RUNTIME_PPU_ADDRESS_LOW),
            Instruction::IncAbsolute(RUNTIME_ADDRESS_LOW_COUNT),
            Instruction::BneAbsolute(RUNTIME_ADDRESS_LOW_ADDRESS + 0x0B),
            Instruction::IncAbsolute(RUNTIME_ADDRESS_LOW_COUNT + 1),
            Instruction::Rts,
        ],
    )
}

fn runtime_data_prepare() -> Result<Vec<u8>> {
    assemble_at(
        RUNTIME_DATA_ADDRESS,
        &[
            Instruction::IncAbsolute(RUNTIME_DATA_COUNT),
            Instruction::BneAbsolute(RUNTIME_DATA_ADDRESS + 0x08),
            Instruction::IncAbsolute(RUNTIME_DATA_COUNT + 1),
            Instruction::StaAbsolute(RUNTIME_VALUE),
            Instruction::LdaZeroPage(0x00),
            Instruction::StaAbsolute(RUNTIME_SAVED_ZERO_PAGE_0),
            Instruction::LdaZeroPage(0x01),
            Instruction::StaAbsolute(RUNTIME_SAVED_ZERO_PAGE_1),
            Instruction::LdaAbsolute(RUNTIME_PPU_ADDRESS_HIGH),
            Instruction::CmpImmediate(0x3F),
            Instruction::BcsAbsolute(RUNTIME_INCREMENT_ADDRESS),
            Instruction::CmpImmediate(0x30),
            Instruction::BccAbsolute(RUNTIME_CHECK_NAMETABLE_ADDRESS),
            Instruction::Sec,
            Instruction::SbcImmediate(0x10),
            Instruction::JmpAbsolute(RUNTIME_CHECK_NAMETABLE_ADDRESS),
        ],
    )
}

fn runtime_check_nametable() -> Result<Vec<u8>> {
    assemble_at(
        RUNTIME_CHECK_NAMETABLE_ADDRESS,
        &[
            Instruction::CmpImmediate(0x20),
            Instruction::BccAbsolute(RUNTIME_INCREMENT_ADDRESS),
            Instruction::Sec,
            Instruction::SbcImmediate(0x20),
            Instruction::Pha,
            Instruction::AndImmediate(0x03),
            Instruction::Clc,
            Instruction::AdcImmediate((PHYSICAL_NAMETABLE_START >> 8) as u8),
            Instruction::StaZeroPage(0x01),
            Instruction::Pla,
            Instruction::LsrAccumulator,
            Instruction::LsrAccumulator,
            Instruction::StaAbsolute(RUNTIME_LOGICAL_NAMETABLE),
            Instruction::LdaZeroPage(SOURCE_MIRRORING_SHADOW),
            Instruction::BeqAbsolute(RUNTIME_VERTICAL_MIRROR_ADDRESS),
            Instruction::LdaAbsolute(RUNTIME_LOGICAL_NAMETABLE),
            Instruction::LsrAccumulator,
            Instruction::JmpAbsolute(RUNTIME_PHYSICAL_ADDRESS),
        ],
    )
}

fn runtime_vertical_mirror() -> Result<Vec<u8>> {
    assemble_at(
        RUNTIME_VERTICAL_MIRROR_ADDRESS,
        &[
            Instruction::LdaAbsolute(RUNTIME_LOGICAL_NAMETABLE),
            Instruction::AndImmediate(0x01),
            Instruction::JmpAbsolute(RUNTIME_PHYSICAL_ADDRESS),
        ],
    )
}

fn runtime_physical_address() -> Result<Vec<u8>> {
    assemble_at(
        RUNTIME_PHYSICAL_ADDRESS,
        &[
            Instruction::AslAccumulator,
            Instruction::AslAccumulator,
            Instruction::Clc,
            Instruction::AdcZeroPage(0x01),
            Instruction::StaZeroPage(0x01),
            Instruction::LdaAbsolute(RUNTIME_PPU_ADDRESS_LOW),
            Instruction::StaZeroPage(0x00),
            Instruction::LdyImmediate(0),
            Instruction::LdaAbsolute(RUNTIME_VALUE),
            Instruction::StaIndirectY(0x00),
            Instruction::JmpAbsolute(RUNTIME_INCREMENT_ADDRESS),
        ],
    )
}

fn runtime_increment_address() -> Result<Vec<u8>> {
    assemble_at(
        RUNTIME_INCREMENT_ADDRESS,
        &[
            Instruction::LdaZeroPage(SOURCE_PPU_CONTROL_SHADOW),
            Instruction::AndImmediate(0x04),
            Instruction::BeqAbsolute(RUNTIME_INCREMENT_ACROSS_ADDRESS),
            Instruction::LdaAbsolute(RUNTIME_PPU_ADDRESS_LOW),
            Instruction::Clc,
            Instruction::AdcImmediate(32),
            Instruction::StaAbsolute(RUNTIME_PPU_ADDRESS_LOW),
            Instruction::BccAbsolute(RUNTIME_MASK_AND_RESTORE_ADDRESS),
            Instruction::IncAbsolute(RUNTIME_PPU_ADDRESS_HIGH),
            Instruction::JmpAbsolute(RUNTIME_MASK_AND_RESTORE_ADDRESS),
        ],
    )
}

fn runtime_increment_across() -> Result<Vec<u8>> {
    assemble_at(
        RUNTIME_INCREMENT_ACROSS_ADDRESS,
        &[
            Instruction::IncAbsolute(RUNTIME_PPU_ADDRESS_LOW),
            Instruction::BneAbsolute(RUNTIME_MASK_AND_RESTORE_ADDRESS),
            Instruction::IncAbsolute(RUNTIME_PPU_ADDRESS_HIGH),
            Instruction::JmpAbsolute(RUNTIME_MASK_AND_RESTORE_ADDRESS),
        ],
    )
}

fn runtime_mask_and_restore() -> Result<Vec<u8>> {
    assemble_at(
        RUNTIME_MASK_AND_RESTORE_ADDRESS,
        &[
            Instruction::LdaAbsolute(RUNTIME_PPU_ADDRESS_HIGH),
            Instruction::AndImmediate(0x3F),
            Instruction::StaAbsolute(RUNTIME_PPU_ADDRESS_HIGH),
            Instruction::LdaAbsolute(RUNTIME_SAVED_ZERO_PAGE_0),
            Instruction::StaZeroPage(0x00),
            Instruction::LdaAbsolute(RUNTIME_SAVED_ZERO_PAGE_1),
            Instruction::StaZeroPage(0x01),
            Instruction::Rts,
        ],
    )
}

fn runtime_initialize() -> Result<Vec<u8>> {
    let clear_loop_address = RUNTIME_INITIALIZE_ADDRESS + 0x04;
    let state_loop_address = RUNTIME_INITIALIZE_ADDRESS + 0x23;
    let mut instructions = vec![
        Instruction::LdaImmediate(0xFF),
        Instruction::LdxImmediate(0),
        Instruction::StaAbsoluteX(0x6800),
        Instruction::StaAbsoluteX(0x6900),
        Instruction::StaAbsoluteX(0x6A00),
        Instruction::StaAbsoluteX(0x6B00),
        Instruction::StaAbsoluteX(0x6C00),
        Instruction::StaAbsoluteX(0x6D00),
        Instruction::StaAbsoluteX(0x6E00),
        Instruction::StaAbsoluteX(0x6F00),
        Instruction::Inx,
        Instruction::BneAbsolute(clear_loop_address),
        Instruction::LdaImmediate(0),
        Instruction::LdxImmediate(0),
        Instruction::StaAbsoluteX(RUNTIME_STATE_START),
        Instruction::Inx,
        Instruction::CpxImmediate(RUNTIME_STATE_LEN),
        Instruction::BneAbsolute(state_loop_address),
    ];
    for (index, byte) in RUNTIME_MAGIC.iter().copied().enumerate() {
        instructions.push(Instruction::LdaImmediate(byte));
        instructions.push(Instruction::StaAbsolute(RUNTIME_MAGIC_START + index as u16));
    }
    instructions.push(Instruction::Rts);
    assemble_at(RUNTIME_INITIALIZE_ADDRESS, &instructions)
}

fn install_ppu_store_hook(image: &mut TrackedImage, site: PpuStoreSite) -> Result<()> {
    image.write_expected(
        format!("hook {}", site.role),
        site.file_offset()?,
        &assemble_at(
            site.cpu_address,
            &[Instruction::StaAbsolute(site.kind.register())],
        )?,
        &assemble_at(
            site.cpu_address,
            &[Instruction::JsrAbsolute(site.kind.hook_address())],
        )?,
    )
}

fn write_file(path: &Path, data: &[u8]) -> Result<()> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)
            .with_context(|| format!("create output directory {}", parent.display()))?;
    }
    fs::write(path, data).with_context(|| format!("write {}", path.display()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn direct_ppu_store_sites_cover_twenty_address_bytes_and_seven_data_consumers() {
        assert_eq!(
            PPU_STORE_SITES
                .iter()
                .filter(|site| site.kind != PpuStoreKind::Data)
                .count(),
            20
        );
        assert_eq!(
            PPU_STORE_SITES
                .iter()
                .filter(|site| site.kind == PpuStoreKind::Data)
                .count(),
            7
        );
        let mut offsets = PPU_STORE_SITES
            .iter()
            .map(|site| site.file_offset().unwrap())
            .collect::<Vec<_>>();
        offsets.sort_unstable();
        offsets.dedup();
        assert_eq!(offsets.len(), PPU_STORE_SITES.len());
    }

    #[test]
    fn fixed_hooks_preserve_the_original_store_before_recording_it() {
        for (register, operation) in [
            (PPU_ADDRESS_REGISTER, OPERATION_ADDRESS_HIGH),
            (PPU_ADDRESS_REGISTER, OPERATION_ADDRESS_LOW),
            (PPU_DATA_REGISTER, OPERATION_DATA),
        ] {
            let hook = ppu_store_hook(register, operation).unwrap();
            assert_eq!(&hook[..3], &[0x8D, register as u8, (register >> 8) as u8]);
            assert_eq!(hook.len(), 13);
        }
    }

    #[test]
    fn runtime_payload_routines_fit_the_two_page_install_image() {
        let payload = runtime_payload().unwrap();
        let initializer = runtime_initialize().unwrap();
        assert_eq!(payload.len(), RUNTIME_PAYLOAD_LEN);
        assert!(initializer.len() < 0x100);
        assert_eq!(
            &payload[usize::from(RUNTIME_INITIALIZE_ADDRESS - RUNTIME_DISPATCH_ADDRESS)
                ..usize::from(RUNTIME_INITIALIZE_ADDRESS - RUNTIME_DISPATCH_ADDRESS)
                    + initializer.len()],
            initializer
        );
    }

    #[test]
    fn physical_shadow_uses_two_kibibytes_outside_runtime_code_and_state() {
        assert_eq!(PHYSICAL_NAMETABLE_END - PHYSICAL_NAMETABLE_START, 0x0800);
        assert!(RUNTIME_DISPATCH_ADDRESS + RUNTIME_PAYLOAD_LEN as u16 <= RUNTIME_STATE_START);
        assert!(RUNTIME_STATE_START + u16::from(RUNTIME_STATE_LEN) <= PHYSICAL_NAMETABLE_START);
    }
}
