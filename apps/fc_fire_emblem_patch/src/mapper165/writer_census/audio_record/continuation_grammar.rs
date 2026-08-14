use std::collections::BTreeSet;

use anyhow::{Context, Result, bail, ensure};

const MAX_STRUCTURAL_COMMANDS: usize = 4096;

#[derive(Clone, Copy)]
pub(super) struct AddressedBytes<'a> {
    base: u16,
    bytes: &'a [u8],
}

impl<'a> AddressedBytes<'a> {
    pub(super) const fn new(base: u16, bytes: &'a [u8]) -> Self {
        Self { base, bytes }
    }

    fn byte(self, address: u16) -> Result<u8> {
        let relative = address
            .checked_sub(self.base)
            .context("audio address precedes the supplied byte region")?;
        self.bytes
            .get(usize::from(relative))
            .copied()
            .context("audio address exceeds the supplied byte region")
    }

    pub(super) fn word(self, address: u16) -> Result<u16> {
        let high_address = address
            .checked_add(1)
            .context("audio pointer operand address overflow")?;
        Ok(u16::from_le_bytes([
            self.byte(address)?,
            self.byte(high_address)?,
        ]))
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct DeferredFdFrame {
    parent_address: u16,
    saved_operand_index: u16,
    nested_stream_address: u16,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct NestedReturnTrace {
    return_address: u16,
    nested_fd_call_count: usize,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) struct StructuralCommandBoundary {
    pub(super) deferred_fd_address: u16,
    pub(super) nested_stream_address: u16,
    pub(super) nested_stream_return_address: u16,
    pub(super) nested_fd_call_count: usize,
    pub(super) command_address: u16,
    pub(super) command: u8,
}

pub(super) fn locate_command_after_nested_return(
    data: &AddressedBytes<'_>,
    stream_address: u16,
) -> Result<StructuralCommandBoundary> {
    let frame = deferred_fd_frame_after_first_note(data, stream_address)?;
    let nested = trace_nested_fe_return(data, frame.nested_stream_address)?;
    let command_address = frame
        .parent_address
        .checked_add(frame.saved_operand_index)
        .and_then(|address| address.checked_add(2))
        .context("audio parent continuation address overflow")?;
    Ok(StructuralCommandBoundary {
        deferred_fd_address: frame.parent_address,
        nested_stream_address: frame.nested_stream_address,
        nested_stream_return_address: nested.return_address,
        nested_fd_call_count: nested.nested_fd_call_count,
        command_address,
        command: data.byte(command_address)?,
    })
}

fn deferred_fd_frame_after_first_note(
    data: &AddressedBytes<'_>,
    stream_address: u16,
) -> Result<DeferredFdFrame> {
    let mut cursor = stream_address;
    for _ in 0..MAX_STRUCTURAL_COMMANDS {
        let opcode = data.byte(cursor)?;
        match opcode {
            0x7E => cursor = checked_audio_advance(cursor, 1)?,
            0x00..=0x7F => {
                let parent_address = checked_audio_advance(cursor, 1)?;
                ensure!(
                    data.byte(parent_address)? == 0xFD,
                    "the first source audio note does not defer to an FD continuation"
                );
                let pointer_address = checked_audio_advance(parent_address, 1)?;
                return Ok(DeferredFdFrame {
                    parent_address,
                    saved_operand_index: 1,
                    nested_stream_address: data.word(pointer_address)?,
                });
            }
            0x80..=0xBD => cursor = checked_audio_advance(cursor, 1)?,
            0xBE | 0xBF => {
                data.byte(checked_audio_advance(cursor, 1)?)?;
                cursor = checked_audio_advance(cursor, 2)?;
            }
            _ => bail!(
                "unsupported audio opcode 0x{opcode:02X} before the first deferred continuation"
            ),
        }
    }
    bail!("audio stream did not reach a deferred FD continuation")
}

fn trace_nested_fe_return(
    data: &AddressedBytes<'_>,
    stream_address: u16,
) -> Result<NestedReturnTrace> {
    let mut cursor = stream_address;
    let mut return_stack = Vec::new();
    let mut visited = BTreeSet::new();
    let mut nested_fd_call_count = 0_usize;
    for _ in 0..MAX_STRUCTURAL_COMMANDS {
        ensure!(
            visited.insert((cursor, return_stack.clone())),
            "nested audio continuation contains a structural cycle"
        );
        let opcode = data.byte(cursor)?;
        match opcode {
            0x00..=0xBD => cursor = checked_audio_advance(cursor, 1)?,
            0xBE | 0xBF => {
                data.byte(checked_audio_advance(cursor, 1)?)?;
                cursor = checked_audio_advance(cursor, 2)?;
            }
            0xC0..=0xDF => {
                data.word(checked_audio_advance(cursor, 1)?)?;
                cursor = checked_audio_advance(cursor, 3)?;
            }
            0xFD => {
                let pointer_address = checked_audio_advance(cursor, 1)?;
                let target = data.word(pointer_address)?;
                return_stack.push(checked_audio_advance(cursor, 3)?);
                nested_fd_call_count = nested_fd_call_count
                    .checked_add(1)
                    .context("nested audio FD call count overflow")?;
                cursor = target;
            }
            0xFE => {
                if let Some(return_address) = return_stack.pop() {
                    cursor = return_address;
                } else {
                    return Ok(NestedReturnTrace {
                        return_address: cursor,
                        nested_fd_call_count,
                    });
                }
            }
            0xFF => bail!("nested audio stream terminates instead of returning with FE"),
            _ => bail!("unsupported high audio opcode 0x{opcode:02X} in the bounded FD/FE grammar"),
        }
    }
    bail!("nested audio stream did not return within the structural command bound")
}

fn checked_audio_advance(address: u16, byte_count: u16) -> Result<u16> {
    address
        .checked_add(byte_count)
        .context("audio stream address overflow")
}
