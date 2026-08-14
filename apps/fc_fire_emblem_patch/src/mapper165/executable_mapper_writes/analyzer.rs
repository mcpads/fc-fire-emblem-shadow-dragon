use std::collections::{BTreeSet, VecDeque};

use anyhow::{Result, ensure};
use retro_rp2a03::{
    AddressingMode, Instruction, Location, MemoryAddress, Operand, Rp2A03, decode_bytes,
};
use typed_isa_core::{AccessKind, ControlAction, ControlBoundary, ControlTarget, StaticSemantics};

use super::{
    hardware_decode::{MapperHardware, MapperRegister},
    mapped_program::{CodeLocation, ExecutableProgram},
};

/// One direct mapper write reached from the declared roots.
#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct DirectMapperWrite {
    pub(crate) instruction: CodeLocation,
    pub(crate) address: u16,
    pub(crate) register: MapperRegister,
    pub(crate) opcode: u8,
    pub(crate) opcode_is_documented: bool,
}

/// Why a control edge could not be bound to the declared physical mapping.
#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) enum UnresolvedControlEdge {
    MissingSequentialMapping { next_cpu_address: u16 },
    UnboundDirectTarget { target_cpu_address: u16 },
    IndirectTarget,
    RecursiveCall { target: CodeLocation },
    SynchronousTrap,
    UnsupportedBoundary,
    UnexpectedDelaySlot,
}

/// A fact that prevents the declared rooted analysis from passing closed.
#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) enum UnresolvedExecutableFact {
    InstructionBytesUnavailable {
        instruction: CodeLocation,
        next_cpu_address: u16,
    },
    ControlEdge {
        instruction: CodeLocation,
        edge: UnresolvedControlEdge,
    },
    EffectiveMapperWrite {
        instruction: CodeLocation,
        mode: AddressingMode,
        operand: Operand,
    },
}

/// Architectural endpoint reached without another instruction in this declared program view.
#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) enum ControlExit {
    SubroutineReturn(CodeLocation),
    InterruptReturn(CodeLocation),
    Stop(CodeLocation),
}

/// Result for the caller-declared roots and mapped executable extents only.
#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct ExecutableMapperWriteAnalysis {
    pub(crate) program_role: String,
    pub(crate) reachable_instruction_locations: BTreeSet<CodeLocation>,
    pub(crate) direct_mapper_writes: Vec<DirectMapperWrite>,
    pub(crate) control_exits: Vec<ControlExit>,
    pub(crate) unresolved_facts: Vec<UnresolvedExecutableFact>,
}

impl ExecutableMapperWriteAnalysis {
    /// Reject use as a closed declared-route result while any edge or effective mapper write is
    /// unresolved. Zero unresolved facts still establishes neither a whole-ROM executable census
    /// nor bank-state transitions caused by the reported mapper writes.
    pub(crate) fn require_declared_routes_resolved(&self) -> Result<()> {
        ensure!(
            self.unresolved_facts.is_empty(),
            "executable program {} has unresolved rooted-analysis facts: {:?}",
            self.program_role,
            self.unresolved_facts
        );
        Ok(())
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
struct CallFrame {
    callee_entry: CodeLocation,
    return_site: CodeLocation,
}

#[derive(Clone, Debug, Eq, PartialEq)]
struct AnalysisState {
    location: CodeLocation,
    calls: Vec<CallFrame>,
}

/// Rooted static analyzer for mapper writes in one explicitly mapped executable program view.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct ExecutableMapperWriteAnalyzer {
    hardware: MapperHardware,
}

impl ExecutableMapperWriteAnalyzer {
    pub(crate) const fn new(hardware: MapperHardware) -> Self {
        Self { hardware }
    }

    pub(crate) fn analyze(self, program: &ExecutableProgram<'_>) -> ExecutableMapperWriteAnalysis {
        let mut analysis = ExecutableMapperWriteAnalysis {
            program_role: program.role.clone(),
            reachable_instruction_locations: BTreeSet::new(),
            direct_mapper_writes: Vec::new(),
            control_exits: Vec::new(),
            unresolved_facts: Vec::new(),
        };
        let mut queue = VecDeque::new();
        let mut visited = Vec::new();
        for root in &program.roots {
            queue.push_back(AnalysisState {
                location: root.clone(),
                calls: Vec::new(),
            });
        }

        while let Some(state) = queue.pop_front() {
            if visited.contains(&state) {
                continue;
            }
            visited.push(state.clone());
            analysis
                .reachable_instruction_locations
                .insert(state.location.clone());

            let decoded = match decode_instruction(program, &state.location) {
                Ok(decoded) => decoded,
                Err(unresolved) => {
                    push_unique(&mut analysis.unresolved_facts, unresolved);
                    continue;
                }
            };
            self.collect_mapper_writes(&state.location, &decoded.instruction, &mut analysis);

            let semantics = Rp2A03::semantics(&decoded.instruction, &state.location.cpu_address)
                .expect("RP2A03 static semantics are infallible");
            if semantics.control_flow.delay_slot.is_some() {
                push_unique(
                    &mut analysis.unresolved_facts,
                    UnresolvedExecutableFact::ControlEdge {
                        instruction: state.location.clone(),
                        edge: UnresolvedControlEdge::UnexpectedDelaySlot,
                    },
                );
                continue;
            }

            match semantics.control_flow.action {
                ControlAction::Continue => enqueue_sequential(
                    &mut queue,
                    &mut analysis,
                    &state,
                    decoded.next,
                    semantics.control_flow.fallthrough,
                ),
                ControlAction::Transfer { target } => {
                    if semantics.control_flow.fallthrough.is_some() {
                        enqueue_sequential(
                            &mut queue,
                            &mut analysis,
                            &state,
                            decoded.next,
                            semantics.control_flow.fallthrough,
                        );
                    }
                    match target {
                        ControlTarget::Direct(target) => {
                            enqueue_direct(program, &mut queue, &mut analysis, &state, target)
                        }
                        ControlTarget::Indirect(_) => push_unique(
                            &mut analysis.unresolved_facts,
                            UnresolvedExecutableFact::ControlEdge {
                                instruction: state.location,
                                edge: UnresolvedControlEdge::IndirectTarget,
                            },
                        ),
                    }
                }
                ControlAction::LinkedTransfer {
                    target,
                    return_site,
                } => {
                    let Some(return_location) = require_sequential_location(
                        &mut analysis,
                        &state.location,
                        decoded.next,
                        return_site,
                    ) else {
                        continue;
                    };
                    let ControlTarget::Direct(target_cpu_address) = target else {
                        push_unique(
                            &mut analysis.unresolved_facts,
                            UnresolvedExecutableFact::ControlEdge {
                                instruction: state.location,
                                edge: UnresolvedControlEdge::IndirectTarget,
                            },
                        );
                        continue;
                    };
                    let Some(target) =
                        program.resolve_direct_target(&state.location, target_cpu_address)
                    else {
                        push_unique(
                            &mut analysis.unresolved_facts,
                            UnresolvedExecutableFact::ControlEdge {
                                instruction: state.location,
                                edge: UnresolvedControlEdge::UnboundDirectTarget {
                                    target_cpu_address,
                                },
                            },
                        );
                        continue;
                    };
                    if state.calls.iter().any(|frame| frame.callee_entry == target) {
                        push_unique(
                            &mut analysis.unresolved_facts,
                            UnresolvedExecutableFact::ControlEdge {
                                instruction: state.location,
                                edge: UnresolvedControlEdge::RecursiveCall { target },
                            },
                        );
                        continue;
                    }
                    let mut calls = state.calls;
                    calls.push(CallFrame {
                        callee_entry: target.clone(),
                        return_site: return_location,
                    });
                    queue.push_back(AnalysisState {
                        location: target,
                        calls,
                    });
                }
                ControlAction::Return { .. } => {
                    let mut calls = state.calls;
                    if let Some(frame) = calls.pop() {
                        queue.push_back(AnalysisState {
                            location: frame.return_site,
                            calls,
                        });
                    } else {
                        push_unique(
                            &mut analysis.control_exits,
                            ControlExit::SubroutineReturn(state.location),
                        );
                    }
                }
                ControlAction::ExceptionReturn { .. } => push_unique(
                    &mut analysis.control_exits,
                    ControlExit::InterruptReturn(state.location),
                ),
                ControlAction::Boundary(boundary) => match boundary {
                    ControlBoundary::Stop { .. } => push_unique(
                        &mut analysis.control_exits,
                        ControlExit::Stop(state.location),
                    ),
                    ControlBoundary::Trap { .. } => push_unique(
                        &mut analysis.unresolved_facts,
                        UnresolvedExecutableFact::ControlEdge {
                            instruction: state.location,
                            edge: UnresolvedControlEdge::SynchronousTrap,
                        },
                    ),
                    ControlBoundary::Wait { .. } | ControlBoundary::ProfileExit { .. } => {
                        push_unique(
                            &mut analysis.unresolved_facts,
                            UnresolvedExecutableFact::ControlEdge {
                                instruction: state.location,
                                edge: UnresolvedControlEdge::UnsupportedBoundary,
                            },
                        )
                    }
                },
            }
        }

        analysis.direct_mapper_writes.sort_by(|left, right| {
            left.instruction
                .cmp(&right.instruction)
                .then(left.address.cmp(&right.address))
                .then(left.register.cmp(&right.register))
        });
        analysis
    }

    fn collect_mapper_writes(
        self,
        location: &CodeLocation,
        instruction: &Instruction,
        analysis: &mut ExecutableMapperWriteAnalysis,
    ) {
        let semantics = Rp2A03::semantics(instruction, &location.cpu_address)
            .expect("RP2A03 static semantics are infallible");
        for access in semantics.location_accesses {
            if access.kind != AccessKind::Write {
                continue;
            }
            let Location::Memory(memory) = access.location else {
                continue;
            };
            match memory {
                MemoryAddress::Direct(address) => {
                    let Some(register) = self.hardware.decode_write(address) else {
                        continue;
                    };
                    push_unique(
                        &mut analysis.direct_mapper_writes,
                        DirectMapperWrite {
                            instruction: location.clone(),
                            address,
                            register,
                            opcode: instruction.opcode(),
                            opcode_is_documented: instruction.opcode_is_documented(),
                        },
                    );
                }
                MemoryAddress::Effective { mode, operand } => {
                    if effective_write_may_reach_mapper(self.hardware, mode, operand) {
                        push_unique(
                            &mut analysis.unresolved_facts,
                            UnresolvedExecutableFact::EffectiveMapperWrite {
                                instruction: location.clone(),
                                mode,
                                operand,
                            },
                        );
                    }
                }
                MemoryAddress::Stack => {}
                MemoryAddress::Pointer { .. } | MemoryAddress::InterruptVector => push_unique(
                    &mut analysis.unresolved_facts,
                    UnresolvedExecutableFact::EffectiveMapperWrite {
                        instruction: location.clone(),
                        mode: instruction.addressing_mode(),
                        operand: instruction.operand(),
                    },
                ),
            }
        }
    }
}

#[derive(Debug)]
struct DecodedInstruction {
    instruction: Instruction,
    next: Option<CodeLocation>,
}

fn decode_instruction(
    program: &ExecutableProgram<'_>,
    start: &CodeLocation,
) -> std::result::Result<DecodedInstruction, UnresolvedExecutableFact> {
    let mut bytes = Vec::with_capacity(3);
    let mut cursor = start.clone();
    loop {
        let Some(byte) = program.byte_at(&cursor) else {
            return Err(UnresolvedExecutableFact::InstructionBytesUnavailable {
                instruction: start.clone(),
                next_cpu_address: cursor.cpu_address,
            });
        };
        bytes.push(byte);
        match decode_bytes(&bytes) {
            Ok(instruction) => {
                return Ok(DecodedInstruction {
                    instruction,
                    next: program.sequential_location_after(&cursor),
                });
            }
            Err(retro_rp2a03::DecodeError::Truncated { .. }) => {
                let Some(next) = program.sequential_location_after(&cursor) else {
                    return Err(UnresolvedExecutableFact::InstructionBytesUnavailable {
                        instruction: start.clone(),
                        next_cpu_address: cursor.cpu_address.wrapping_add(1),
                    });
                };
                cursor = next;
            }
            Err(retro_rp2a03::DecodeError::Empty) => {
                unreachable!("one opcode byte was supplied to the RP2A03 decoder")
            }
        }
    }
}

fn enqueue_sequential(
    queue: &mut VecDeque<AnalysisState>,
    analysis: &mut ExecutableMapperWriteAnalysis,
    state: &AnalysisState,
    next: Option<CodeLocation>,
    semantic_fallthrough: Option<u16>,
) {
    let Some(expected_cpu_address) = semantic_fallthrough else {
        let next_cpu_address = state.location.cpu_address;
        push_unique(
            &mut analysis.unresolved_facts,
            UnresolvedExecutableFact::ControlEdge {
                instruction: state.location.clone(),
                edge: UnresolvedControlEdge::MissingSequentialMapping { next_cpu_address },
            },
        );
        return;
    };
    let Some(location) =
        require_sequential_location(analysis, &state.location, next, expected_cpu_address)
    else {
        return;
    };
    queue.push_back(AnalysisState {
        location,
        calls: state.calls.clone(),
    });
}

fn require_sequential_location(
    analysis: &mut ExecutableMapperWriteAnalysis,
    instruction: &CodeLocation,
    next: Option<CodeLocation>,
    expected_cpu_address: u16,
) -> Option<CodeLocation> {
    let Some(next) = next else {
        push_unique(
            &mut analysis.unresolved_facts,
            UnresolvedExecutableFact::ControlEdge {
                instruction: instruction.clone(),
                edge: UnresolvedControlEdge::MissingSequentialMapping {
                    next_cpu_address: expected_cpu_address,
                },
            },
        );
        return None;
    };
    if next.cpu_address != expected_cpu_address {
        push_unique(
            &mut analysis.unresolved_facts,
            UnresolvedExecutableFact::ControlEdge {
                instruction: instruction.clone(),
                edge: UnresolvedControlEdge::MissingSequentialMapping {
                    next_cpu_address: expected_cpu_address,
                },
            },
        );
        return None;
    }
    Some(next)
}

fn enqueue_direct(
    program: &ExecutableProgram<'_>,
    queue: &mut VecDeque<AnalysisState>,
    analysis: &mut ExecutableMapperWriteAnalysis,
    state: &AnalysisState,
    target_cpu_address: u16,
) {
    let Some(location) = program.resolve_direct_target(&state.location, target_cpu_address) else {
        push_unique(
            &mut analysis.unresolved_facts,
            UnresolvedExecutableFact::ControlEdge {
                instruction: state.location.clone(),
                edge: UnresolvedControlEdge::UnboundDirectTarget { target_cpu_address },
            },
        );
        return;
    };
    queue.push_back(AnalysisState {
        location,
        calls: state.calls.clone(),
    });
}

fn effective_write_may_reach_mapper(
    hardware: MapperHardware,
    mode: AddressingMode,
    operand: Operand,
) -> bool {
    match (mode, operand) {
        // The RP2A03 wraps zero-page indexed addressing inside page zero.
        (AddressingMode::ZeroPageX | AddressingMode::ZeroPageY, Operand::Byte(_)) => false,
        (AddressingMode::AbsoluteX | AddressingMode::AbsoluteY, Operand::Word(base_address)) => {
            (0..=u8::MAX).any(|index| {
                hardware
                    .decode_write(base_address.wrapping_add(u16::from(index)))
                    .is_some()
            })
        }
        // A runtime pointer can name any 16-bit CPU address until a separate value-range proof is
        // attached by the caller. No pointer-range assumption is made here.
        (
            AddressingMode::ZeroPageIndexedIndirectX | AddressingMode::ZeroPageIndirectIndexedY,
            Operand::Byte(_),
        ) => true,
        // StaticSemantics should not produce another effective write form for this profile. If it
        // ever does, treating it as possibly mapped keeps the analysis fail-closed.
        _ => true,
    }
}

fn push_unique<T: PartialEq>(items: &mut Vec<T>, item: T) {
    if !items.contains(&item) {
        items.push(item);
    }
}
