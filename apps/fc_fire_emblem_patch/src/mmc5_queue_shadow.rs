use std::{fs, path::Path};

use anyhow::{Context, Result, ensure};
use serde::Serialize;

use crate::{
    mmc5_chr::{create_mmc5_chr_writer_probe_image, switchable_bank_file_offset},
    mmc5_prg::{SOURCE_RESET_ADDRESS, count_direct_transfers_to_range, fixed_bank_file_offset},
    mmc5_queue_runtime as queue_runtime,
    rom::{EXPECTED_CHR_SHA1, EXPECTED_SOURCE_SHA1, PRG_SIZE, Rom},
    rp2a03::{Instruction, assemble_at},
    sha1_hex,
    tracked::TrackedImage,
};

mod direct_clear;

const CHR_MODE_RESET_TAIL_ADDRESS: u16 = 0xFA65;
const QUEUE_ACCUMULATOR_PUBLISH_WRAPPER_ADDRESS: u16 = 0xFA80;
const QUEUE_Y_PUBLISH_WRAPPER_ADDRESS: u16 = 0xFAA0;
const QUEUE_PUBLISH_BATCH_ADDRESS: u16 = 0xFAC0;
const INSTALL_QUEUE_SHADOW_ADDRESS: u16 = 0xFAD0;
const RUNTIME_PAYLOAD_SOURCE_ADDRESS: u16 = 0xFB00;

const SOURCE_QUEUE_REPLAY_ADDRESS: u16 = 0xC3E7;
const SOURCE_QUEUE_READY_FLAG: u8 = 0x21;

const PRG_RAM_BANK_REGISTER: u16 = 0x5113;

#[derive(Debug, Clone, Copy)]
enum PrgLocation {
    Fixed,
    Switchable(u8),
}

#[derive(Debug, Clone, Copy)]
enum QueuePublishRegister {
    Accumulator,
    Y,
}

#[derive(Debug, Clone, Copy)]
struct QueuePublisher {
    role: &'static str,
    location: PrgLocation,
    cpu_address: u16,
    register: QueuePublishRegister,
}

impl QueuePublisher {
    fn file_offset(self) -> Result<usize> {
        match self.location {
            PrgLocation::Fixed => fixed_bank_file_offset(self.cpu_address),
            PrgLocation::Switchable(bank) => switchable_bank_file_offset(bank, self.cpu_address),
        }
    }

    fn prg_bank(self) -> u8 {
        match self.location {
            PrgLocation::Fixed => 0x0F,
            PrgLocation::Switchable(bank) => bank,
        }
    }
}

const QUEUE_PUBLISHERS: &[QueuePublisher] = &[
    QueuePublisher {
        role: "bank 0D streamed command publisher",
        location: PrgLocation::Switchable(0x0D),
        cpu_address: 0xA6B9,
        register: QueuePublishRegister::Accumulator,
    },
    QueuePublisher {
        role: "bank 0D repeated fill publisher",
        location: PrgLocation::Switchable(0x0D),
        cpu_address: 0xAD52,
        register: QueuePublishRegister::Accumulator,
    },
    QueuePublisher {
        role: "fixed serialized text publisher",
        location: PrgLocation::Fixed,
        cpu_address: 0xC8C8,
        register: QueuePublishRegister::Y,
    },
];

#[derive(Debug, Serialize)]
struct QueueShadowProbeReport {
    schema: u32,
    source_sha1: &'static str,
    base_chr_writer_probe_sha1: String,
    output_sha1: String,
    output_mapper: u16,
    prg_size: usize,
    chr_size: usize,
    chr_sha1: &'static str,
    queue_replay_cpu_address: String,
    queue_replay_direct_transfer_candidate_count: usize,
    publisher_boundaries: Vec<QueuePublisherReport>,
    direct_transfer_boundaries: Vec<direct_clear::DirectTransferBoundaryReport>,
    ppu_data_store_hooks: usize,
    runtime: RuntimeReport,
    direct_fixed_code_transfer_count: usize,
    direct_payload_transfer_candidate_count: usize,
    tracked_delta_writes: Vec<TrackedWrite>,
    unresolved_boundaries: Vec<&'static str>,
    release_eligible: bool,
}

#[derive(Debug, Serialize)]
struct RuntimeReport {
    prg_ram_bank: u8,
    queue_cpu_start: String,
    queue_format: &'static str,
    payload_source_cpu_start: String,
    payload_cpu_start: String,
    payload_len: usize,
    state_cpu_start: String,
    state_len: usize,
    physical_nametable_cpu_start: String,
    physical_nametable_len: usize,
    initial_tile_byte: String,
    initial_attribute_byte: String,
    mirroring_source: &'static str,
    initialization_magic: String,
}

#[derive(Debug, Serialize)]
struct QueuePublisherReport {
    role: &'static str,
    prg_bank: String,
    cpu_address: String,
    file_offset: String,
    source_register: &'static str,
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
}

pub fn build_mmc5_queue_shadow_probe(
    source_path: &Path,
    output_path: &Path,
    report_path: &Path,
) -> Result<BuildSummary> {
    let source_rom = Rom::from_path(source_path)?;
    source_rom.verify_supported_japanese()?;
    validate_queue_publisher_contracts(&source_rom)?;
    direct_clear::validate_source(&source_rom)?;

    let direct_fixed_code_transfer_count = count_direct_transfers_to_range(
        source_rom.prg(),
        QUEUE_ACCUMULATOR_PUBLISH_WRAPPER_ADDRESS,
        RUNTIME_PAYLOAD_SOURCE_ADDRESS,
    )? + count_direct_transfers_to_range(
        source_rom.prg(),
        direct_clear::WRAPPER_ADDRESS,
        direct_clear::wrapper_end()?,
    )?;
    ensure!(
        direct_fixed_code_transfer_count == 0,
        "source has {direct_fixed_code_transfer_count} direct JSR or JMP references into the queue-shadow fixed code range"
    );
    let direct_payload_transfer_candidate_count = count_direct_transfers_to_range(
        source_rom.prg(),
        RUNTIME_PAYLOAD_SOURCE_ADDRESS,
        RUNTIME_PAYLOAD_SOURCE_ADDRESS + queue_runtime::PAYLOAD_LEN as u16,
    )?;

    let chr_writer_probe = create_mmc5_chr_writer_probe_image(&source_rom)?;
    let base = chr_writer_probe.data().to_vec();
    let base_chr_writer_probe_sha1 = sha1_hex(&base);
    let mut image = TrackedImage::new(base.clone());

    redirect_chr_initializer_to_queue_shadow_install(&mut image)?;
    install_fixed_routines(&mut image)?;
    install_runtime_payload(&mut image)?;
    for publisher in QUEUE_PUBLISHERS {
        redirect_queue_publisher(&mut image, *publisher)?;
    }
    direct_clear::redirect_source_clear(&mut image)?;

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
    let output_rom = Rom::parse(output.clone()).context("parse MMC5 queue-shadow probe")?;
    ensure!(
        output_rom.mapper() == 5,
        "queue-shadow probe mapper is not 5"
    );
    ensure!(
        output_rom.prg().len() == PRG_SIZE,
        "queue-shadow probe changed PRG size"
    );
    ensure!(
        sha1_hex(output_rom.chr()) == EXPECTED_CHR_SHA1,
        "queue-shadow probe changed source CHR"
    );

    let output_sha1 = sha1_hex(&output);
    let queue_replay_direct_transfer_candidate_count = count_direct_transfers_to_range(
        source_rom.prg(),
        SOURCE_QUEUE_REPLAY_ADDRESS,
        SOURCE_QUEUE_REPLAY_ADDRESS + 1,
    )?;
    let report = QueueShadowProbeReport {
        schema: 2,
        source_sha1: EXPECTED_SOURCE_SHA1,
        base_chr_writer_probe_sha1,
        output_sha1: output_sha1.clone(),
        output_mapper: output_rom.mapper(),
        prg_size: output_rom.prg().len(),
        chr_size: output_rom.chr().len(),
        chr_sha1: EXPECTED_CHR_SHA1,
        queue_replay_cpu_address: format!("0x{SOURCE_QUEUE_REPLAY_ADDRESS:04X}"),
        queue_replay_direct_transfer_candidate_count,
        publisher_boundaries: QUEUE_PUBLISHERS
            .iter()
            .map(|publisher| {
                Ok(QueuePublisherReport {
                    role: publisher.role,
                    prg_bank: format!("0x{:02X}", publisher.prg_bank()),
                    cpu_address: format!("0x{:04X}", publisher.cpu_address),
                    file_offset: format!("0x{:06X}", publisher.file_offset()?),
                    source_register: match publisher.register {
                        QueuePublishRegister::Accumulator => "A",
                        QueuePublishRegister::Y => "Y",
                    },
                })
            })
            .collect::<Result<Vec<_>>>()?,
        direct_transfer_boundaries: vec![direct_clear::report()?],
        ppu_data_store_hooks: 0,
        runtime: RuntimeReport {
            prg_ram_bank: 1,
            queue_cpu_start: format!("0x{:04X}", queue_runtime::SOURCE_QUEUE_START),
            queue_format: "address_high, address_low, descriptor, data; descriptor bit 7 selects increment 32, bit 6 repeats one data byte, and bits 0-5 are the output length; zero address_high terminates",
            payload_source_cpu_start: format!("0x{RUNTIME_PAYLOAD_SOURCE_ADDRESS:04X}"),
            payload_cpu_start: format!("0x{:04X}", queue_runtime::REPLAY_QUEUE_ADDRESS),
            payload_len: queue_runtime::PAYLOAD_LEN,
            state_cpu_start: format!("0x{:04X}", queue_runtime::STATE_START),
            state_len: usize::from(queue_runtime::STATE_LEN),
            physical_nametable_cpu_start: format!(
                "0x{:04X}",
                queue_runtime::PHYSICAL_NAMETABLE_START
            ),
            physical_nametable_len: usize::from(
                queue_runtime::PHYSICAL_NAMETABLE_END - queue_runtime::PHYSICAL_NAMETABLE_START,
            ),
            initial_tile_byte: "0xFF".to_owned(),
            initial_attribute_byte: "0xFF until a confirmed direct clear or queue write".to_owned(),
            mirroring_source: "source zero-page $C8: 0 vertical, nonzero horizontal",
            initialization_magic: String::from_utf8_lossy(queue_runtime::MAGIC).into_owned(),
        },
        direct_fixed_code_transfer_count,
        direct_payload_transfer_candidate_count,
        tracked_delta_writes,
        unresolved_boundaries: vec![
            "This probe mirrors the three confirmed queue publishers before they set $21 and the confirmed bank 0D page-zero clear; other publishers and direct PPU transfer loops remain unowned.",
            "The runtime shadow is not yet connected to MMC5 ExRAM display attributes.",
            "The all-FF runtime payload source has direct JSR/JMP byte-pattern candidates; they are reported rather than interpreted as instruction-boundary references.",
            "PRG RAM bank 1 is isolated from the source save bank in Mesen, but save/load compatibility and other execution environments remain unverified.",
        ],
        release_eligible: false,
    };
    let report_bytes =
        serde_json::to_vec_pretty(&report).context("serialize MMC5 queue-shadow report")?;
    let report_sha1 = sha1_hex(&report_bytes);
    let tracked_write_count = report.tracked_delta_writes.len();

    write_file(output_path, &output)?;
    write_file(report_path, &report_bytes)?;
    Ok(BuildSummary {
        output_sha1,
        report_sha1,
        tracked_write_count,
    })
}

fn validate_queue_publisher_contracts(source_rom: &Rom) -> Result<()> {
    for publisher in QUEUE_PUBLISHERS {
        let expected = queue_publisher_source(*publisher)?;
        let offset = publisher.file_offset()?;
        ensure!(
            source_rom.data()[offset..offset + expected.len()] == expected,
            "queue publisher {:02X}:{:04X} changed",
            publisher.prg_bank(),
            publisher.cpu_address
        );
    }
    Ok(())
}

fn redirect_chr_initializer_to_queue_shadow_install(image: &mut TrackedImage) -> Result<()> {
    image.write_expected(
        "redirect CHR initializer to queue-shadow install",
        fixed_bank_file_offset(CHR_MODE_RESET_TAIL_ADDRESS)?,
        &assemble_at(
            CHR_MODE_RESET_TAIL_ADDRESS,
            &[Instruction::JmpAbsolute(SOURCE_RESET_ADDRESS)],
        )?,
        &assemble_at(
            CHR_MODE_RESET_TAIL_ADDRESS,
            &[Instruction::JmpAbsolute(INSTALL_QUEUE_SHADOW_ADDRESS)],
        )?,
    )
}

fn install_fixed_routines(image: &mut TrackedImage) -> Result<()> {
    for (role, address, instructions) in [
        (
            "accumulator queue-publish wrapper",
            QUEUE_ACCUMULATOR_PUBLISH_WRAPPER_ADDRESS,
            accumulator_queue_publish_wrapper()?,
        ),
        (
            "Y queue-publish wrapper",
            QUEUE_Y_PUBLISH_WRAPPER_ADDRESS,
            y_queue_publish_wrapper()?,
        ),
        (
            "queue-publish batch helper",
            QUEUE_PUBLISH_BATCH_ADDRESS,
            queue_publish_batch()?,
        ),
        (
            "queue-shadow installer",
            INSTALL_QUEUE_SHADOW_ADDRESS,
            install_queue_shadow()?,
        ),
    ] {
        image.write_expected(
            format!("MMC5 {role}"),
            fixed_bank_file_offset(address)?,
            &vec![0xFF; instructions.len()],
            &instructions,
        )?;
    }
    direct_clear::install_wrapper(image)?;
    Ok(())
}

fn accumulator_queue_publish_wrapper() -> Result<Vec<u8>> {
    assemble_at(
        QUEUE_ACCUMULATOR_PUBLISH_WRAPPER_ADDRESS,
        &[
            Instruction::Php,
            Instruction::Txa,
            Instruction::Pha,
            Instruction::Tya,
            Instruction::Pha,
            Instruction::LdaZeroPage(0x00),
            Instruction::Pha,
            Instruction::LdaZeroPage(0x01),
            Instruction::Pha,
            Instruction::JsrAbsolute(QUEUE_PUBLISH_BATCH_ADDRESS),
            Instruction::Pla,
            Instruction::StaZeroPage(0x01),
            Instruction::Pla,
            Instruction::StaZeroPage(0x00),
            Instruction::Pla,
            Instruction::Tay,
            Instruction::Pla,
            Instruction::Tax,
            Instruction::Plp,
            Instruction::LdaImmediate(1),
            Instruction::StaZeroPage(SOURCE_QUEUE_READY_FLAG),
            Instruction::Rts,
        ],
    )
}

fn y_queue_publish_wrapper() -> Result<Vec<u8>> {
    assemble_at(
        QUEUE_Y_PUBLISH_WRAPPER_ADDRESS,
        &[
            Instruction::Php,
            Instruction::Pha,
            Instruction::Txa,
            Instruction::Pha,
            Instruction::LdaZeroPage(0x00),
            Instruction::Pha,
            Instruction::LdaZeroPage(0x01),
            Instruction::Pha,
            Instruction::JsrAbsolute(QUEUE_PUBLISH_BATCH_ADDRESS),
            Instruction::Pla,
            Instruction::StaZeroPage(0x01),
            Instruction::Pla,
            Instruction::StaZeroPage(0x00),
            Instruction::Pla,
            Instruction::Tax,
            Instruction::Pla,
            Instruction::Plp,
            Instruction::LdyImmediate(1),
            Instruction::StyZeroPage(SOURCE_QUEUE_READY_FLAG),
            Instruction::Rts,
        ],
    )
}

fn queue_publish_batch() -> Result<Vec<u8>> {
    assemble_at(
        QUEUE_PUBLISH_BATCH_ADDRESS,
        &[
            Instruction::LdaImmediate(1),
            Instruction::StaAbsolute(PRG_RAM_BANK_REGISTER),
            Instruction::JsrAbsolute(queue_runtime::REPLAY_QUEUE_ADDRESS),
            Instruction::LdaImmediate(0),
            Instruction::StaAbsolute(PRG_RAM_BANK_REGISTER),
            Instruction::Rts,
        ],
    )
}

fn install_queue_shadow() -> Result<Vec<u8>> {
    assemble_at(
        INSTALL_QUEUE_SHADOW_ADDRESS,
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
            Instruction::StaAbsoluteX(queue_runtime::REPLAY_QUEUE_ADDRESS),
            Instruction::LdaAbsoluteX(RUNTIME_PAYLOAD_SOURCE_ADDRESS + 0x100),
            Instruction::StaAbsoluteX(queue_runtime::REPLAY_QUEUE_ADDRESS + 0x100),
            Instruction::Inx,
            Instruction::BneAbsolute(INSTALL_QUEUE_SHADOW_ADDRESS + 0x0D),
            Instruction::JsrAbsolute(queue_runtime::INITIALIZE_ADDRESS),
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

fn redirect_queue_publisher(image: &mut TrackedImage, publisher: QueuePublisher) -> Result<()> {
    let expected = queue_publisher_source(publisher)?;
    let replacement = match publisher.register {
        QueuePublishRegister::Accumulator => assemble_at(
            publisher.cpu_address,
            &[
                Instruction::JsrAbsolute(QUEUE_ACCUMULATOR_PUBLISH_WRAPPER_ADDRESS),
                Instruction::Nop,
            ],
        )?,
        QueuePublishRegister::Y => assemble_at(
            publisher.cpu_address,
            &[
                Instruction::JmpAbsolute(QUEUE_Y_PUBLISH_WRAPPER_ADDRESS),
                Instruction::Nop,
                Instruction::Nop,
            ],
        )?,
    };
    ensure!(
        expected.len() == replacement.len(),
        "queue publisher redirect changed instruction span"
    );
    image.write_expected(
        format!("redirect {} to batch shadow", publisher.role),
        publisher.file_offset()?,
        &expected,
        &replacement,
    )
}

fn queue_publisher_source(publisher: QueuePublisher) -> Result<Vec<u8>> {
    match publisher.register {
        QueuePublishRegister::Accumulator => assemble_at(
            publisher.cpu_address,
            &[
                Instruction::LdaImmediate(1),
                Instruction::StaZeroPage(SOURCE_QUEUE_READY_FLAG),
            ],
        ),
        QueuePublishRegister::Y => assemble_at(
            publisher.cpu_address,
            &[
                Instruction::LdyImmediate(1),
                Instruction::StyZeroPage(SOURCE_QUEUE_READY_FLAG),
                Instruction::Rts,
            ],
        ),
    }
}

fn install_runtime_payload(image: &mut TrackedImage) -> Result<()> {
    let payload = queue_runtime::build_payload()?;
    image.write_expected(
        "MMC5 queue-shadow runtime payload",
        fixed_bank_file_offset(RUNTIME_PAYLOAD_SOURCE_ADDRESS)?,
        &vec![0xFF; queue_runtime::PAYLOAD_LEN],
        &payload,
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
    use crate::mmc4_latch::{Mmc4NametableShadow, NametableMirroring, PpuAddressIncrement};

    #[derive(Debug)]
    struct QueueTransfer {
        address: u16,
        increment: PpuAddressIncrement,
        data: Vec<u8>,
    }

    fn decode_queue(queue: &[u8]) -> Result<Vec<QueueTransfer>> {
        let mut cursor = 0;
        let mut transfers = Vec::new();
        loop {
            let Some(&address_high) = queue.get(cursor) else {
                anyhow::bail!("source PPU queue has no zero address-high terminator");
            };
            if address_high == 0 {
                return Ok(transfers);
            }
            ensure!(
                cursor + 3 <= queue.len(),
                "source PPU queue command header is truncated"
            );
            let address_low = queue[cursor + 1];
            let descriptor = queue[cursor + 2];
            let data_len = usize::from(descriptor & 0x3F);
            ensure!(
                data_len > 0,
                "zero-length source PPU queue command is unsupported"
            );
            let data_start = cursor + 3;
            let encoded_data_len = if descriptor & 0x40 == 0 { data_len } else { 1 };
            let data_end = data_start
                .checked_add(encoded_data_len)
                .ok_or_else(|| anyhow::anyhow!("source PPU queue length overflow"))?;
            ensure!(
                data_end <= queue.len(),
                "source PPU queue command data is truncated"
            );
            transfers.push(QueueTransfer {
                address: u16::from_be_bytes([address_high, address_low]),
                increment: if descriptor & 0x80 == 0 {
                    PpuAddressIncrement::Across
                } else {
                    PpuAddressIncrement::Down
                },
                data: if descriptor & 0x40 == 0 {
                    queue[data_start..data_end].to_vec()
                } else {
                    vec![queue[data_start]; data_len]
                },
            });
            cursor = data_end;
        }
    }

    #[test]
    fn queue_publisher_redirects_preserve_their_source_spans() {
        for publisher in QUEUE_PUBLISHERS {
            let source = queue_publisher_source(*publisher).unwrap();
            let replacement = match publisher.register {
                QueuePublishRegister::Accumulator => assemble_at(
                    publisher.cpu_address,
                    &[
                        Instruction::JsrAbsolute(QUEUE_ACCUMULATOR_PUBLISH_WRAPPER_ADDRESS),
                        Instruction::Nop,
                    ],
                )
                .unwrap(),
                QueuePublishRegister::Y => assemble_at(
                    publisher.cpu_address,
                    &[
                        Instruction::JmpAbsolute(QUEUE_Y_PUBLISH_WRAPPER_ADDRESS),
                        Instruction::Nop,
                        Instruction::Nop,
                    ],
                )
                .unwrap(),
            };
            assert_eq!(source.len(), replacement.len(), "{}", publisher.role);
        }
    }

    #[test]
    fn publisher_wrappers_replay_before_setting_the_ready_flag() {
        let accumulator_wrapper = accumulator_queue_publish_wrapper().unwrap();
        let y_wrapper = y_queue_publish_wrapper().unwrap();
        let batch = queue_publish_batch().unwrap();
        let replay = [0x20, 0x00, 0x60];
        let publish_batch = [0x20, 0xC0, 0xFA];
        let accumulator_ready = [0xA9, 0x01, 0x85, 0x21, 0x60];
        let y_ready = [0xA0, 0x01, 0x84, 0x21, 0x60];
        let replay_offset = batch
            .windows(replay.len())
            .position(|window| window == replay)
            .unwrap();
        assert!(replay_offset < batch.len());
        for (wrapper, ready) in [
            (&accumulator_wrapper, accumulator_ready.as_slice()),
            (&y_wrapper, y_ready.as_slice()),
        ] {
            let batch_offset = wrapper
                .windows(publish_batch.len())
                .position(|window| window == publish_batch)
                .unwrap();
            let ready_offset = wrapper
                .windows(ready.len())
                .position(|window| window == ready)
                .unwrap();
            assert!(batch_offset < ready_offset);
            assert!(wrapper.ends_with(ready));
        }
        assert!(
            accumulator_wrapper.len()
                <= usize::from(
                    QUEUE_Y_PUBLISH_WRAPPER_ADDRESS - QUEUE_ACCUMULATOR_PUBLISH_WRAPPER_ADDRESS
                )
        );
        assert!(
            y_wrapper.len()
                <= usize::from(QUEUE_PUBLISH_BATCH_ADDRESS - QUEUE_Y_PUBLISH_WRAPPER_ADDRESS)
        );
        assert!(
            batch.len() <= usize::from(INSTALL_QUEUE_SHADOW_ADDRESS - QUEUE_PUBLISH_BATCH_ADDRESS)
        );
    }

    #[test]
    fn source_queue_semantics_cover_across_down_and_mirroring() {
        let queue = [
            0x20, 0x00, 0x03, 0x11, 0x22, 0x33, 0x24, 0x00, 0x82, 0x44, 0x55, 0x00,
        ];
        let transfers = decode_queue(&queue).unwrap();
        let mut shadow = Mmc4NametableShadow::filled(0xFF);
        for transfer in transfers {
            shadow
                .apply_ppu_transfer(
                    transfer.address,
                    transfer.increment,
                    &transfer.data,
                    NametableMirroring::Vertical,
                )
                .unwrap();
        }
        let bytes = shadow.physical_bytes();
        assert_eq!(&bytes[..3], &[0x11, 0x22, 0x33]);
        assert_eq!(bytes[0x400], 0x44);
        assert_eq!(bytes[0x420], 0x55);
    }

    #[test]
    fn source_queue_repeat_flag_expands_one_encoded_byte() {
        let queue = [0x20, 0x10, 0x44, 0x7A, 0x00];
        let transfers = decode_queue(&queue).unwrap();
        assert_eq!(transfers.len(), 1);
        assert_eq!(transfers[0].data, [0x7A; 4]);
    }
}
