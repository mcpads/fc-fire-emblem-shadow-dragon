mod continuation_grammar;
mod source_binding;

#[cfg(test)]
mod tests;

use anyhow::{Context, Result, ensure};
use serde::Serialize;

use crate::{rom::Rom, sha1_hex};

use self::continuation_grammar::{AddressedBytes, locate_command_after_nested_return};
use super::source_bytes;

pub(super) const AUDIO_BANK: u8 = 0x0E;
pub(super) const EVENT_DIRECTORY_ADDRESS: u16 = 0x96CF;
const EVENT_DIRECTORY_INDEX: usize = 15;
const EVENT_DESCRIPTOR_ADDRESS: u16 = 0xB0C5;
const EVENT_DESCRIPTOR: [u8; 9] = [0x37, 0xB2, 0xCE, 0xB0, 0xAB, 0xB3, 0xA5, 0xB4, 0x0D];
const EVENT_STREAM_SLOT: usize = 1;
const EVENT_STREAM_ADDRESS: u16 = 0xB0CE;
const EVENT_STREAM_PREFIX: [u8; 10] = [0xBE, 0x00, 0x82, 0x7F, 0xFD, 0x36, 0xA4, 0xC1, 0x7A, 0x9A];
const DEFERRED_FD_ADDRESS: u16 = 0xB0D2;
const NESTED_STREAM_ADDRESS: u16 = 0xA436;
const NESTED_STREAM_RETURN_ADDRESS: u16 = 0xA4D2;
const NESTED_STREAM_LENGTH: usize = 0x9D;
const NESTED_STREAM_SHA1: &str = "cd726512de2c9e1250643fd585136b2331c80d5d";
const SHARED_NESTED_STREAM_ADDRESS: u16 = 0xA62A;
const SHARED_NESTED_STREAM_LENGTH: usize = 0x4D;
const SHARED_NESTED_STREAM_SHA1: &str = "c1e53842f5af4d27a24248ecc17b4687acfa4ae3";
const RECORD_COMMAND_OFFSET: usize = 7;
const RECORD_COMMAND: u8 = 0xC1;
const RECORD_ADDRESS: u16 = 0x9A7A;
pub(super) const RECORD_BYTES: [u8; 7] = [0x8C, 0x00, 0xC0, 0x81, 0x9A, 0x68, 0x97];
const AUDIO_PRG_ADDRESS: u16 = 0x8000;
const AUDIO_PRG_LENGTH: usize = 16 * 1024;

#[derive(Debug, Clone, Serialize)]
pub(super) struct AudioRecordCandidateBinding {
    prg_bank_hex: String,
    event_directory_address_hex: String,
    event_directory_index: usize,
    event_directory_entry_address_hex: String,
    event_descriptor_address_hex: String,
    event_stream_slot: usize,
    event_stream_address_hex: String,
    deferred_fd_address_hex: String,
    nested_stream_address_hex: String,
    nested_stream_return_address_hex: String,
    nested_stream_sha1: String,
    shared_nested_stream_sha1: String,
    nested_stream_internal_fd_call_count: usize,
    record_command_offset: usize,
    record_command_address_hex: String,
    record_command_boundary_structurally_bound: bool,
    record_command_boundary_proof_scope: &'static str,
    nested_continuation_grammar_scope: &'static str,
    record_pointer_operand_address_hex: String,
    record_cpu_range_hex: String,
    record_sha1: String,
    candidate_cpu_range_hex: String,
    candidate_within_record: bool,
    record_byte_count: usize,
}

pub(super) fn bind_audio_record_candidate(
    source: &Rom,
    candidate_address: u16,
    candidate_len: usize,
) -> Result<AudioRecordCandidateBinding> {
    source_binding::bind_event_directory_loader(source)?;

    let directory_entry_address = EVENT_DIRECTORY_ADDRESS
        .checked_add(
            u16::try_from(EVENT_DIRECTORY_INDEX * 2)
                .context("audio event directory index overflow")?,
        )
        .context("audio event directory entry overflow")?;
    let directory_entry = source_bytes(source, AUDIO_BANK, directory_entry_address, 2)?;
    ensure!(
        u16::from_le_bytes([directory_entry[0], directory_entry[1]]) == EVENT_DESCRIPTOR_ADDRESS,
        "source audio event directory entry changed"
    );

    let descriptor = source_bytes(
        source,
        AUDIO_BANK,
        EVENT_DESCRIPTOR_ADDRESS,
        EVENT_DESCRIPTOR.len(),
    )?;
    ensure!(
        descriptor == EVENT_DESCRIPTOR,
        "source audio event descriptor changed"
    );
    source_binding::bind_event_descriptor_and_stream_readers(source)?;
    let stream_pointer_offset = EVENT_STREAM_SLOT
        .checked_mul(2)
        .context("audio stream slot offset overflow")?;
    let stream_address = u16::from_le_bytes([
        descriptor[stream_pointer_offset],
        descriptor[stream_pointer_offset + 1],
    ]);
    ensure!(
        stream_address == EVENT_STREAM_ADDRESS,
        "source audio event stream pointer changed"
    );

    let stream_prefix = source_bytes(
        source,
        AUDIO_BANK,
        stream_address,
        EVENT_STREAM_PREFIX.len(),
    )?;
    ensure!(
        stream_prefix == EVENT_STREAM_PREFIX,
        "source audio event stream continuation changed"
    );
    source_binding::bind_continuation_parser(source)?;
    let nested_stream_sha1 = source_binding::bind_source_data_slice(
        source,
        NESTED_STREAM_ADDRESS,
        NESTED_STREAM_LENGTH,
        NESTED_STREAM_SHA1,
        "source nested audio stream",
    )?;
    let shared_nested_stream_sha1 = source_binding::bind_source_data_slice(
        source,
        SHARED_NESTED_STREAM_ADDRESS,
        SHARED_NESTED_STREAM_LENGTH,
        SHARED_NESTED_STREAM_SHA1,
        "source shared nested audio stream",
    )?;
    let audio_prg = source_bytes(source, AUDIO_BANK, AUDIO_PRG_ADDRESS, AUDIO_PRG_LENGTH)?;
    let audio_view = AddressedBytes::new(AUDIO_PRG_ADDRESS, audio_prg);
    let boundary = locate_command_after_nested_return(&audio_view, stream_address)?;
    ensure!(
        boundary.deferred_fd_address == DEFERRED_FD_ADDRESS,
        "source audio deferred FD address changed"
    );
    ensure!(
        boundary.nested_stream_address == NESTED_STREAM_ADDRESS,
        "source audio nested stream entry changed"
    );
    ensure!(
        boundary.nested_stream_return_address == NESTED_STREAM_RETURN_ADDRESS,
        "source audio nested stream return changed"
    );
    ensure!(
        boundary.command_address
            == EVENT_STREAM_ADDRESS
                .checked_add(u16::try_from(RECORD_COMMAND_OFFSET)?)
                .context("audio record command address overflow")?,
        "source audio nested continuation no longer resumes at the record command"
    );
    ensure!(
        boundary.command == RECORD_COMMAND
            && stream_prefix[RECORD_COMMAND_OFFSET] == RECORD_COMMAND,
        "source audio record command changed"
    );
    let record_address = audio_view.word(
        boundary
            .command_address
            .checked_add(1)
            .context("audio record pointer operand address overflow")?,
    )?;
    ensure!(
        record_address == RECORD_ADDRESS,
        "source audio record pointer changed"
    );

    source_binding::bind_record_parser(source)?;
    let record = source_bytes(source, AUDIO_BANK, record_address, RECORD_BYTES.len())?;
    ensure!(record == RECORD_BYTES, "source C0/C1 audio record changed");
    ensure!(
        range_contains(
            record_address,
            RECORD_BYTES.len(),
            candidate_address,
            candidate_len,
        )?,
        "MMC4-looking audio candidate is outside the source-bound C0/C1 record"
    );

    let record_end = record_address
        .checked_add(u16::try_from(RECORD_BYTES.len())?)
        .context("audio record range overflow")?;
    let candidate_end = candidate_address
        .checked_add(u16::try_from(candidate_len)?)
        .context("audio candidate range overflow")?;
    Ok(AudioRecordCandidateBinding {
        prg_bank_hex: format!("0x{AUDIO_BANK:02X}"),
        event_directory_address_hex: format!("0x{EVENT_DIRECTORY_ADDRESS:04X}"),
        event_directory_index: EVENT_DIRECTORY_INDEX,
        event_directory_entry_address_hex: format!("0x{directory_entry_address:04X}"),
        event_descriptor_address_hex: format!("0x{EVENT_DESCRIPTOR_ADDRESS:04X}"),
        event_stream_slot: EVENT_STREAM_SLOT,
        event_stream_address_hex: format!("0x{EVENT_STREAM_ADDRESS:04X}"),
        deferred_fd_address_hex: format!("0x{DEFERRED_FD_ADDRESS:04X}"),
        nested_stream_address_hex: format!("0x{NESTED_STREAM_ADDRESS:04X}"),
        nested_stream_return_address_hex: format!("0x{NESTED_STREAM_RETURN_ADDRESS:04X}"),
        nested_stream_sha1,
        shared_nested_stream_sha1,
        nested_stream_internal_fd_call_count: boundary.nested_fd_call_count,
        record_command_offset: RECORD_COMMAND_OFFSET,
        record_command_address_hex: format!("0x{:04X}", boundary.command_address),
        record_command_boundary_structurally_bound: true,
        record_command_boundary_proof_scope: "source-bound static parser and FD/FE continuation grammar; natural execution is not claimed",
        nested_continuation_grammar_scope: "the exact B0CE to A436 route and its nested FD/FE calls; this is not a complete audio-bytecode grammar or runtime-coverage claim",
        record_pointer_operand_address_hex: format!(
            "0x{:04X}",
            EVENT_STREAM_ADDRESS + RECORD_COMMAND_OFFSET as u16 + 1
        ),
        record_cpu_range_hex: format!("0x{record_address:04X}..0x{record_end:04X}"),
        record_sha1: sha1_hex(record),
        candidate_cpu_range_hex: format!("0x{candidate_address:04X}..0x{candidate_end:04X}"),
        candidate_within_record: true,
        record_byte_count: RECORD_BYTES.len(),
    })
}

fn range_contains(
    container_start: u16,
    container_len: usize,
    candidate_start: u16,
    candidate_len: usize,
) -> Result<bool> {
    ensure!(container_len > 0, "audio record range is empty");
    ensure!(candidate_len > 0, "audio candidate range is empty");
    let container_end = container_start
        .checked_add(u16::try_from(container_len)?)
        .context("audio record range overflow")?;
    let candidate_end = candidate_start
        .checked_add(u16::try_from(candidate_len)?)
        .context("audio candidate range overflow")?;
    Ok(container_start <= candidate_start && candidate_end <= container_end)
}
