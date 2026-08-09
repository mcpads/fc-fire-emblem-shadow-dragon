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

mod runtime_payload;
mod store_sites;
#[cfg(test)]
mod tests;

use runtime_payload::*;
use store_sites::PPU_STORE_SITES;

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
