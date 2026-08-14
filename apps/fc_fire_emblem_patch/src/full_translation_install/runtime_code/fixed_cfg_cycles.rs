//! 후보 고정 뱅크의 방출 코드를 따라 최악 실행 사이클을 계산한다.
//!
//! 호출자 바깥의 `JSR` 비용은 포함하지 않는다. 진입점부터 최상위 `RTS`까지의 분기,
//! 직접 점프, 중첩 호출만 센다. 선언 범위 밖 전이, 간접 전이, 재귀·루프는 유한한
//! 상한을 증명하지 못하므로 실패한다.

use std::collections::{BTreeMap, BTreeSet};

use anyhow::{Context, Result, bail, ensure};
use retro_rp2a03::{AddressingMode, Mnemonic, decode_bytes};

use crate::{
    rom::Rom,
    typed_source::{Rp2a03DirectControlFlow, rp2a03_direct_control_flow},
};

const FIXED_BANK_ORIGIN: u16 = 0xC000;
const FIXED_BANK_BYTE_COUNT: usize = 16 * 1024;
const MAXIMUM_CALL_DEPTH: usize = 16;

#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord)]
struct ExecutionState {
    pc: u16,
    return_stack: Vec<u16>,
}

pub(super) fn worst_case_fixed_subroutine_cycles(
    rom: &Rom,
    entry: u16,
    allowed_ranges: &[(u16, u16)],
) -> Result<u32> {
    let fixed_start = rom
        .prg()
        .len()
        .checked_sub(FIXED_BANK_BYTE_COUNT)
        .context("PRG is smaller than one fixed bank")?;
    let fixed = rom
        .prg()
        .get(fixed_start..)
        .context("fixed PRG bank is outside ROM")?;
    worst_case_subroutine_cycles(fixed, FIXED_BANK_ORIGIN, entry, allowed_ranges)
}

fn worst_case_subroutine_cycles(
    bytes: &[u8],
    origin: u16,
    entry: u16,
    allowed_ranges: &[(u16, u16)],
) -> Result<u32> {
    ensure!(
        !allowed_ranges.is_empty(),
        "cycle analysis has no allowed code range"
    );
    for &(start, end) in allowed_ranges {
        ensure!(
            start < end,
            "cycle analysis range {start:04X}..{end:04X} is empty"
        );
    }
    let mut analyzer = Analyzer {
        bytes,
        origin,
        allowed_ranges,
        active: BTreeSet::new(),
        memoized: BTreeMap::new(),
    };
    analyzer.cycles_from(ExecutionState {
        pc: entry,
        return_stack: Vec::new(),
    })
}

struct Analyzer<'a> {
    bytes: &'a [u8],
    origin: u16,
    allowed_ranges: &'a [(u16, u16)],
    active: BTreeSet<ExecutionState>,
    memoized: BTreeMap<ExecutionState, u32>,
}

impl Analyzer<'_> {
    fn cycles_from(&mut self, state: ExecutionState) -> Result<u32> {
        if let Some(cycles) = self.memoized.get(&state) {
            return Ok(*cycles);
        }
        ensure!(
            state.return_stack.len() <= MAXIMUM_CALL_DEPTH,
            "fixed-code cycle analysis exceeded {MAXIMUM_CALL_DEPTH} nested calls at {:04X}",
            state.pc
        );
        ensure!(
            self.active.insert(state.clone()),
            "fixed-code cycle analysis found a loop or recursion at {:04X}",
            state.pc
        );

        let result = self.cycles_from_fresh_state(&state);
        self.active.remove(&state);
        if let Ok(cycles) = result {
            self.memoized.insert(state, cycles);
        }
        result
    }

    fn cycles_from_fresh_state(&mut self, state: &ExecutionState) -> Result<u32> {
        let instruction = self.decode_at(state.pc)?;
        let own_cycles = documented_worst_case_cycles(&instruction)?;
        let flow = rp2a03_direct_control_flow(&instruction, state.pc)?;
        let continuation = match flow {
            Rp2a03DirectControlFlow::FallThrough { next } => {
                let next = self.with_pc(state, next)?;
                self.cycles_from(next)?
            }
            Rp2a03DirectControlFlow::Branch {
                target,
                fallthrough: Some(fallthrough),
            } => {
                let taken = self.with_pc(state, target)?;
                let not_taken = self.with_pc(state, fallthrough)?;
                let taken_cycles = self.cycles_from(taken)?;
                let not_taken_cycles = self.cycles_from(not_taken)?;
                taken_cycles.max(not_taken_cycles)
            }
            Rp2a03DirectControlFlow::Branch {
                fallthrough: None, ..
            } => bail!("conditional branch at {:04X} has no fallthrough", state.pc),
            Rp2a03DirectControlFlow::Jump {
                target: Some(target),
            } => {
                let target = self.with_pc(state, target)?;
                self.cycles_from(target)?
            }
            Rp2a03DirectControlFlow::Jump { target: None } => {
                bail!(
                    "fixed-code cycle analysis reached an indirect jump at {:04X}",
                    state.pc
                )
            }
            Rp2a03DirectControlFlow::Call {
                target,
                return_address,
            } => {
                let mut called = state.clone();
                called.pc = target;
                called.return_stack.push(return_address);
                self.cycles_from(called)?
            }
            Rp2a03DirectControlFlow::Return => {
                let mut returned = state.clone();
                match returned.return_stack.pop() {
                    Some(return_address) => {
                        returned.pc = return_address;
                        self.cycles_from(returned)?
                    }
                    None => 0,
                }
            }
            Rp2a03DirectControlFlow::Interrupt | Rp2a03DirectControlFlow::Stop => bail!(
                "fixed-code cycle analysis reached a non-returning boundary at {:04X}",
                state.pc
            ),
        };
        own_cycles
            .checked_add(continuation)
            .context("fixed-code cycle count overflow")
    }

    fn with_pc(&self, state: &ExecutionState, pc: u16) -> Result<ExecutionState> {
        ensure!(
            self.allowed_ranges
                .iter()
                .any(|&(start, end)| (start..end).contains(&pc)),
            "fixed-code control flow left its allowed ranges at {pc:04X}"
        );
        let mut next = state.clone();
        next.pc = pc;
        Ok(next)
    }

    fn decode_at(&self, pc: u16) -> Result<retro_rp2a03::Instruction> {
        let offset = usize::from(
            pc.checked_sub(self.origin)
                .context("fixed-code address is below its byte origin")?,
        );
        let instruction = decode_bytes(
            self.bytes
                .get(offset..)
                .context("fixed-code address is outside supplied bytes")?,
        )
        .with_context(|| format!("decode fixed-code cycle instruction at {pc:04X}"))?;
        ensure!(
            instruction.opcode_is_documented(),
            "fixed-code cycle path reached undocumented selector at {pc:04X}"
        );
        let end = pc
            .checked_add(u16::try_from(instruction.encoded_len())?)
            .context("fixed-code instruction crosses the address space")?;
        ensure!(
            self.allowed_ranges
                .iter()
                .any(|&(start, range_end)| start <= pc && end <= range_end),
            "fixed-code instruction at {pc:04X} crosses its allowed range"
        );
        Ok(instruction)
    }
}

/// 현재 helper 경로에서 허용한 명령 형식의 안전한 최악값이다. 새 형식이 경로에
/// 나타나면 추정하지 않고 이 표를 의식적으로 확장할 때까지 실패한다.
fn documented_worst_case_cycles(instruction: &retro_rp2a03::Instruction) -> Result<u32> {
    use AddressingMode::{Absolute, Accumulator, Immediate, Implied, Relative, ZeroPage};
    use Mnemonic::{
        Adc, And, Asl, Bcc, Bcs, Beq, Bmi, Bne, Bpl, Bvc, Bvs, Clc, Cmp, Jmp, Jsr, Lda, Pha, Php,
        Pla, Plp, Rts, Sta,
    };

    let cycles = match (instruction.mnemonic(), instruction.addressing_mode()) {
        (Bcc | Bcs | Beq | Bmi | Bne | Bpl | Bvc | Bvs, Relative) => 4,
        (Jsr, Absolute) | (Rts, Implied) => 6,
        (Jmp, Absolute) => 3,
        (Pha | Php, Implied) => 3,
        (Pla | Plp, Implied) => 4,
        (Lda | Sta, Absolute) => 4,
        (Sta, ZeroPage) => 3,
        (Adc | And | Cmp | Lda, Immediate) => 2,
        (Asl, Accumulator) => 2,
        (Clc, Implied) => 2,
        _ => bail!(
            "fixed-code cycle path reached an unmodeled documented form {} {:?}",
            instruction.mnemonic(),
            instruction.addressing_mode()
        ),
    };
    Ok(cycles)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::rp2a03::{Instruction, assemble_at};

    #[test]
    fn a_branch_takes_the_more_expensive_complete_path() {
        let origin = 0xF000;
        let bytes = assemble_at(
            origin,
            &[
                Instruction::BeqAbsolute(0xF005),
                Instruction::LdaAbsolute(0x1234),
                Instruction::Rts,
            ],
        )
        .unwrap();

        let cycles =
            worst_case_subroutine_cycles(&bytes, origin, origin, &[(origin, 0xF006)]).unwrap();

        assert_eq!(cycles, 14);
    }

    #[test]
    fn a_nested_call_returns_to_its_caller_before_finishing() {
        let origin = 0xF100;
        let mut bytes = assemble_at(
            origin,
            &[Instruction::JsrAbsolute(0xF104), Instruction::Rts],
        )
        .unwrap();
        bytes.extend(
            assemble_at(
                0xF104,
                &[Instruction::LdaAbsolute(0x1234), Instruction::Rts],
            )
            .unwrap(),
        );

        let cycles =
            worst_case_subroutine_cycles(&bytes, origin, origin, &[(origin, 0xF108)]).unwrap();

        assert_eq!(cycles, 22);
    }

    #[test]
    fn a_loop_has_no_proven_finite_cycle_bound() {
        let origin = 0xF200;
        let bytes = assemble_at(origin, &[Instruction::JmpAbsolute(origin)]).unwrap();

        let error = worst_case_subroutine_cycles(&bytes, origin, origin, &[(origin, origin + 3)])
            .unwrap_err();

        assert!(error.to_string().contains("loop or recursion"));
    }

    #[test]
    fn control_flow_outside_the_declared_code_is_refused() {
        let origin = 0xF300;
        let bytes = assemble_at(origin, &[Instruction::JmpAbsolute(0xF400)]).unwrap();

        let error = worst_case_subroutine_cycles(&bytes, origin, origin, &[(origin, origin + 3)])
            .unwrap_err();

        assert!(error.to_string().contains("left its allowed ranges"));
    }
}
