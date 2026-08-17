use std::collections::BTreeSet;

use anyhow::{Context, Result, ensure};
use retro_rp2a03::decode_bytes;

use crate::{
    rom::Rom,
    typed_source::{Rp2a03DirectControlFlow, decode_rp2a03_sequence, rp2a03_direct_control_flow},
};

const SOURCE_PRG_BANK_BYTE_COUNT: usize = 16 * 1024;
const FIXED_PRG_BANK: u8 = 0x0F;
const FIXED_CPU_START: u16 = 0xC000;
const INLINE_POINTER_DISPATCH_ADDRESS: u16 = 0xC34C;
const HARDWARE_VECTOR_SLOTS: [u16; 3] = [0xFFFA, 0xFFFC, 0xFFFE];
const RESET_RAM_CLEAR_START: u16 = 0xC095;
const RESET_RAM_CLEAR_WRITER: u16 = 0xC09E;
const RESET_RAM_CLEAR_POINTER: u8 = 0x00;
const RESET_RAM_CLEAR_CODE: [u8; 18] = [
    0xA0, 0x07, 0x84, 0x01, 0xA0, 0x00, 0x84, 0x00, 0x98, 0x91, 0x00, 0xC8, 0xD0, 0xFB, 0xC6, 0x01,
    0x10, 0xF7,
];

#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub(super) enum FixedVectorOpenControlEdge {
    SwitchableTarget { instruction: u16, target: u16 },
    IndirectTarget { instruction: u16 },
    InlinePointerDispatch { instruction: u16, table_start: u16 },
}

#[derive(Debug)]
pub(super) struct FixedVectorExecution {
    vector_bindings: Vec<(u16, u16)>,
    reachable_instruction_starts: BTreeSet<(u8, u16)>,
    open_control_edges: BTreeSet<FixedVectorOpenControlEdge>,
    indirect_write_sites_below_mapper_space: BTreeSet<(u8, u16, u8)>,
}

impl FixedVectorExecution {
    pub(super) fn vector_slot_count(&self) -> usize {
        self.vector_bindings.len()
    }

    pub(super) fn unique_vector_root_count(&self) -> usize {
        self.vector_bindings
            .iter()
            .map(|(_, target)| *target)
            .collect::<BTreeSet<_>>()
            .len()
    }

    pub(super) fn reachable_instruction_starts(&self) -> &BTreeSet<(u8, u16)> {
        &self.reachable_instruction_starts
    }

    pub(super) fn open_control_edges(&self) -> &BTreeSet<FixedVectorOpenControlEdge> {
        &self.open_control_edges
    }

    pub(super) fn indirect_write_sites_below_mapper_space(&self) -> &BTreeSet<(u8, u16, u8)> {
        &self.indirect_write_sites_below_mapper_space
    }
}

pub(super) fn bind_fixed_vector_execution(source: &Rom) -> Result<FixedVectorExecution> {
    let vector_bindings = HARDWARE_VECTOR_SLOTS
        .into_iter()
        .map(|slot| {
            let bytes = fixed_source_bytes(source, slot, 2)?;
            let target = u16::from_le_bytes([bytes[0], bytes[1]]);
            ensure!(
                target >= FIXED_CPU_START,
                "source hardware vector ${slot:04X} targets unbound switchable or RAM address ${target:04X}"
            );
            Ok((slot, target))
        })
        .collect::<Result<Vec<_>>>()?;

    let mut pending = vector_bindings
        .iter()
        .map(|(_, target)| *target)
        .collect::<Vec<_>>();
    let mut reachable_instruction_starts = BTreeSet::new();
    let mut open_control_edges = BTreeSet::new();
    while let Some(address) = pending.pop() {
        ensure!(
            address >= FIXED_CPU_START,
            "fixed-vector trace escaped the fixed CPU window at ${address:04X}"
        );
        if !reachable_instruction_starts.insert((FIXED_PRG_BANK, address)) {
            continue;
        }
        let instruction =
            decode_bytes(fixed_source_bytes(source, address, 3)?).with_context(|| {
                format!("decode fixed-vector instruction at {FIXED_PRG_BANK:02X}:${address:04X}")
            })?;
        ensure!(
            instruction.opcode_is_documented(),
            "fixed-vector graph reached undocumented opcode at {FIXED_PRG_BANK:02X}:${address:04X}"
        );

        match rp2a03_direct_control_flow(&instruction, address)? {
            Rp2a03DirectControlFlow::FallThrough { next } => {
                enqueue_fixed_target(&mut pending, &mut open_control_edges, address, next);
            }
            Rp2a03DirectControlFlow::Branch {
                target,
                fallthrough,
            } => {
                enqueue_fixed_target(&mut pending, &mut open_control_edges, address, target);
                if let Some(fallthrough) = fallthrough {
                    enqueue_fixed_target(
                        &mut pending,
                        &mut open_control_edges,
                        address,
                        fallthrough,
                    );
                }
            }
            Rp2a03DirectControlFlow::Jump {
                target: Some(target),
            } => enqueue_fixed_target(&mut pending, &mut open_control_edges, address, target),
            Rp2a03DirectControlFlow::Jump { target: None } => {
                open_control_edges.insert(FixedVectorOpenControlEdge::IndirectTarget {
                    instruction: address,
                });
            }
            Rp2a03DirectControlFlow::Call {
                target,
                return_address,
            } if target == INLINE_POINTER_DISPATCH_ADDRESS => {
                pending.push(target);
                open_control_edges.insert(FixedVectorOpenControlEdge::InlinePointerDispatch {
                    instruction: address,
                    table_start: return_address,
                });
            }
            Rp2a03DirectControlFlow::Call {
                target,
                return_address,
            } => {
                enqueue_fixed_target(
                    &mut pending,
                    &mut open_control_edges,
                    address,
                    return_address,
                );
                enqueue_fixed_target(&mut pending, &mut open_control_edges, address, target);
            }
            Rp2a03DirectControlFlow::Return
            | Rp2a03DirectControlFlow::Interrupt
            | Rp2a03DirectControlFlow::Stop => {}
        }
    }

    let indirect_write_sites_below_mapper_space =
        if reachable_instruction_starts.contains(&(FIXED_PRG_BANK, RESET_RAM_CLEAR_WRITER)) {
            BTreeSet::from([bind_reset_ram_clear(source)?])
        } else {
            BTreeSet::new()
        };

    Ok(FixedVectorExecution {
        vector_bindings,
        reachable_instruction_starts,
        open_control_edges,
        indirect_write_sites_below_mapper_space,
    })
}

fn bind_reset_ram_clear(source: &Rom) -> Result<(u8, u16, u8)> {
    let bytes = fixed_source_bytes(source, RESET_RAM_CLEAR_START, RESET_RAM_CLEAR_CODE.len())?;
    ensure!(
        bytes == RESET_RAM_CLEAR_CODE,
        "source reset RAM-clear pointer producer or loop changed"
    );
    decode_rp2a03_sequence(bytes, RESET_RAM_CLEAR_START, "source reset RAM clear")?;
    Ok((
        FIXED_PRG_BANK,
        RESET_RAM_CLEAR_WRITER,
        RESET_RAM_CLEAR_POINTER,
    ))
}

fn enqueue_fixed_target(
    pending: &mut Vec<u16>,
    open_control_edges: &mut BTreeSet<FixedVectorOpenControlEdge>,
    instruction: u16,
    target: u16,
) {
    if target >= FIXED_CPU_START {
        pending.push(target);
    } else {
        open_control_edges.insert(FixedVectorOpenControlEdge::SwitchableTarget {
            instruction,
            target,
        });
    }
}

fn fixed_source_bytes(source: &Rom, address: u16, byte_count: usize) -> Result<&[u8]> {
    ensure!(
        address >= FIXED_CPU_START,
        "fixed source address ${address:04X} is below the fixed CPU window"
    );
    let start = usize::from(FIXED_PRG_BANK)
        .checked_mul(SOURCE_PRG_BANK_BYTE_COUNT)
        .and_then(|offset| offset.checked_add(usize::from(address - FIXED_CPU_START)))
        .context("fixed source PRG offset overflow")?;
    source
        .prg()
        .get(start..start + byte_count)
        .with_context(|| format!("fixed source ${address:04X} exceeds PRG storage"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::rom::HEADER_SIZE;

    fn synthetic_source(fixed_writes: &[(u16, &[u8])], vector_target: u16) -> Rom {
        let mut bytes = vec![0x60; HEADER_SIZE + 16 * SOURCE_PRG_BANK_BYTE_COUNT];
        bytes[..HEADER_SIZE].fill(0);
        bytes[..4].copy_from_slice(b"NES\x1A");
        bytes[4] = 16;
        bytes[6] = 0xA0;
        let fixed_start = HEADER_SIZE + 15 * SOURCE_PRG_BANK_BYTE_COUNT;
        for &(address, replacement) in fixed_writes {
            let offset = fixed_start + usize::from(address - FIXED_CPU_START);
            bytes[offset..offset + replacement.len()].copy_from_slice(replacement);
        }
        for slot in HARDWARE_VECTOR_SLOTS {
            let offset = fixed_start + usize::from(slot - FIXED_CPU_START);
            bytes[offset..offset + 2].copy_from_slice(&vector_target.to_le_bytes());
        }
        Rom::parse(bytes).unwrap()
    }

    #[test]
    fn hardware_vector_slots_derive_their_shared_fixed_root() {
        let source = synthetic_source(&[(0xC100, &[0x60])], 0xC100);

        let execution = bind_fixed_vector_execution(&source).unwrap();

        assert_eq!(execution.vector_slot_count(), HARDWARE_VECTOR_SLOTS.len());
        assert_eq!(execution.unique_vector_root_count(), 1);
        assert_eq!(
            execution.reachable_instruction_starts(),
            &BTreeSet::from([(FIXED_PRG_BANK, 0xC100)])
        );
        assert!(execution.open_control_edges().is_empty());
        assert!(
            execution
                .indirect_write_sites_below_mapper_space()
                .is_empty()
        );
    }

    #[test]
    fn direct_graph_preserves_switchable_and_indirect_edges() {
        let source = synthetic_source(
            &[
                (0xC100, &[0x20, 0x10, 0xC1]),
                (0xC103, &[0x20, 0x00, 0x80]),
                (0xC106, &[0x6C, 0x0C, 0x00]),
                (0xC110, &[0x60]),
            ],
            0xC100,
        );

        let execution = bind_fixed_vector_execution(&source).unwrap();

        for address in [0xC100, 0xC103, 0xC106, 0xC110] {
            assert!(
                execution
                    .reachable_instruction_starts()
                    .contains(&(FIXED_PRG_BANK, address))
            );
        }
        assert!(execution.open_control_edges().contains(
            &FixedVectorOpenControlEdge::SwitchableTarget {
                instruction: 0xC103,
                target: 0x8000,
            }
        ));
        assert!(execution.open_control_edges().contains(
            &FixedVectorOpenControlEdge::IndirectTarget {
                instruction: 0xC106,
            }
        ));
    }

    #[test]
    fn inline_pointer_dispatch_does_not_decode_its_table_as_return_code() {
        let source = synthetic_source(
            &[
                (0xC100, &[0x20, 0x4C, 0xC3]),
                (0xC103, &[0x8D, 0x00, 0xA0]),
                (INLINE_POINTER_DISPATCH_ADDRESS, &[0x6C, 0x0C, 0x00]),
            ],
            0xC100,
        );

        let execution = bind_fixed_vector_execution(&source).unwrap();

        assert!(
            !execution
                .reachable_instruction_starts()
                .contains(&(FIXED_PRG_BANK, 0xC103))
        );
        assert!(execution.open_control_edges().contains(
            &FixedVectorOpenControlEdge::InlinePointerDispatch {
                instruction: 0xC100,
                table_start: 0xC103,
            }
        ));
    }

    #[test]
    fn hardware_vector_outside_fixed_prg_fails_closed() {
        let source = synthetic_source(&[], 0x8000);

        let error = bind_fixed_vector_execution(&source).unwrap_err();

        assert!(error.to_string().contains("unbound switchable or RAM"));
    }

    #[test]
    fn reset_clear_binds_the_full_internal_ram_pointer_range() {
        let source = synthetic_source(
            &[(RESET_RAM_CLEAR_START, &RESET_RAM_CLEAR_CODE)],
            RESET_RAM_CLEAR_START,
        );

        let execution = bind_fixed_vector_execution(&source).unwrap();

        assert!(
            execution
                .reachable_instruction_starts()
                .contains(&(FIXED_PRG_BANK, RESET_RAM_CLEAR_WRITER))
        );
        assert_eq!(
            execution.indirect_write_sites_below_mapper_space(),
            &BTreeSet::from([(
                FIXED_PRG_BANK,
                RESET_RAM_CLEAR_WRITER,
                RESET_RAM_CLEAR_POINTER,
            )])
        );
    }
}
