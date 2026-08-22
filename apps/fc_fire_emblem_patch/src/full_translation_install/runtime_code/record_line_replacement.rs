//! 새 대사 레코드가 직전 물리 줄을 대체하는 정확한 시점을 소유한다.
//!
//! 직접 진입·E7 호출자 인계·E4/E6 게시 전이는 모두 레코드 디렉터리의 명시적
//! 정책 비트를 따른다. 어느 경로든 글꼴 합성이 끝났거나 같은 상주 그룹을 재사용할
//! 수 있다고 판정된 뒤에만 여섯 줄을 `FF`로 지운다. 저장 완료 안내와 아이템 결과처럼
//! 앞줄을 유지하는 경로는 이 루틴을 지나도 아무것도 바꾸지 않는다.
//!
//! 이 정책은 `$A000` 실행 페이지에 있고 그 페이지가 이미 걸린 두 완료 경계에서만
//! 호출된다. resolver는 전송 없는 완료와 실패를, 동기 합성기는 전송 완료를 맡는다.
//! 따라서 고정 뱅크에서 실행 페이지로 되돌아가는 별도 브리지가 필요하지 않다.

use anyhow::Result;

use super::super::runtime_state_storage::{REQUEST_STATE, VISIBLE_PAGE_INDEX};
use super::{RuntimeRoutine, next_address};
use super::{resolve_request::REPLACE_PREVIOUS_ROWS_PENDING, transport::STATE_READY};
use crate::rp2a03::{Instruction, assemble_at};

const LINE_BUFFER_START: u16 = 0x7832;
const LINE_BUFFER_BYTE_COUNT: u8 = 6 * 0x20;
const LINE_BUFFER_BLANK: u8 = 0xFF;
const VISIBLE_PAGE_INDEX_MASK: u8 = !REPLACE_PREVIOUS_ROWS_PENDING;

pub(super) fn build_record_line_replacement(origin: u16) -> Result<RuntimeRoutine> {
    let mut instructions = Vec::new();
    // resolver의 carry와 호출자의 A/X/P를 바꾸지 않는다. 호출자가 어느 완료 경계로
    // 돌아갈지는 이 정책이 아니라 그 경계를 소유한 resolver/합성기가 결정한다.
    instructions.extend([
        Instruction::Php,
        Instruction::Pha,
        Instruction::Txa,
        Instruction::Pha,
        Instruction::LdaAbsolute(REQUEST_STATE),
        Instruction::CmpImmediate(STATE_READY),
    ]);
    let restore_without_replacement = instructions.len();
    instructions.push(Instruction::BneAbsolute(origin));
    instructions.push(Instruction::LdaAbsolute(VISIBLE_PAGE_INDEX));
    let replace_rows = instructions.len();
    instructions.push(Instruction::BmiAbsolute(origin));

    let restore = next_address(origin, &instructions)?;
    instructions[restore_without_replacement] = Instruction::BneAbsolute(restore);
    instructions.extend([
        Instruction::Pla,
        Instruction::Tax,
        Instruction::Pla,
        Instruction::Plp,
        Instruction::Rts,
    ]);

    let clear_rows = next_address(origin, &instructions)?;
    instructions[replace_rows] = Instruction::BmiAbsolute(clear_rows);
    instructions.extend([
        Instruction::AndImmediate(VISIBLE_PAGE_INDEX_MASK),
        Instruction::StaAbsolute(VISIBLE_PAGE_INDEX),
        Instruction::LdaImmediate(LINE_BUFFER_BLANK),
        Instruction::LdxImmediate(LINE_BUFFER_BYTE_COUNT),
    ]);
    let clear_loop = next_address(origin, &instructions)?;
    instructions.extend([
        Instruction::Dex,
        Instruction::StaAbsoluteX(LINE_BUFFER_START),
        Instruction::BneAbsolute(clear_loop),
        Instruction::JmpAbsolute(restore),
    ]);

    Ok(RuntimeRoutine {
        role: "completed-font record-line replacement",
        address: origin,
        bytes: assemble_at(origin, &instructions)?,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    struct Execution {
        a: u8,
        x: u8,
        carry: bool,
    }

    fn execute(
        runtime: &RuntimeRoutine,
        memory: &mut [u8; 0x10000],
        input_carry: bool,
    ) -> Execution {
        let mut pc = runtime.address;
        let mut stack = Vec::new();
        let mut a = 0x5A;
        let mut x = 0xA5;
        let mut negative = false;
        let mut zero = false;
        let mut carry = input_carry;
        for _ in 0..2_000 {
            let offset = usize::from(pc - runtime.address);
            match runtime.bytes[offset] {
                0x08 => {
                    stack.push((u8::from(negative) << 7) | (u8::from(zero) << 1) | u8::from(carry));
                    pc += 1;
                }
                0x28 => {
                    let status = stack.pop().unwrap();
                    negative = status & 0x80 != 0;
                    zero = status & 0x02 != 0;
                    carry = status & 0x01 != 0;
                    pc += 1;
                }
                0x48 => {
                    stack.push(a);
                    pc += 1;
                }
                0x68 => {
                    a = stack.pop().unwrap();
                    zero = a == 0;
                    negative = a & 0x80 != 0;
                    pc += 1;
                }
                0x8A => {
                    a = x;
                    zero = a == 0;
                    negative = a & 0x80 != 0;
                    pc += 1;
                }
                0xAA => {
                    x = a;
                    zero = x == 0;
                    negative = x & 0x80 != 0;
                    pc += 1;
                }
                0xA9 => {
                    a = runtime.bytes[offset + 1];
                    zero = a == 0;
                    negative = a & 0x80 != 0;
                    pc += 2;
                }
                0xA2 => {
                    x = runtime.bytes[offset + 1];
                    zero = x == 0;
                    negative = x & 0x80 != 0;
                    pc += 2;
                }
                0xAD => {
                    let address =
                        u16::from_le_bytes([runtime.bytes[offset + 1], runtime.bytes[offset + 2]]);
                    a = memory[usize::from(address)];
                    zero = a == 0;
                    negative = a & 0x80 != 0;
                    pc += 3;
                }
                0x8D => {
                    let address =
                        u16::from_le_bytes([runtime.bytes[offset + 1], runtime.bytes[offset + 2]]);
                    memory[usize::from(address)] = a;
                    pc += 3;
                }
                0x9D => {
                    let address =
                        u16::from_le_bytes([runtime.bytes[offset + 1], runtime.bytes[offset + 2]]);
                    memory[usize::from(address.wrapping_add(u16::from(x)))] = a;
                    pc += 3;
                }
                0x29 => {
                    a &= runtime.bytes[offset + 1];
                    zero = a == 0;
                    negative = a & 0x80 != 0;
                    pc += 2;
                }
                0xC9 => {
                    let value = runtime.bytes[offset + 1];
                    let result = a.wrapping_sub(value);
                    carry = a >= value;
                    zero = a == value;
                    negative = result & 0x80 != 0;
                    pc += 2;
                }
                0xCA => {
                    x = x.wrapping_sub(1);
                    zero = x == 0;
                    negative = x & 0x80 != 0;
                    pc += 1;
                }
                0xD0 => {
                    let displacement = runtime.bytes[offset + 1] as i8;
                    pc += 2;
                    if !zero {
                        pc = pc.wrapping_add_signed(i16::from(displacement));
                    }
                }
                0x30 => {
                    let displacement = runtime.bytes[offset + 1] as i8;
                    pc += 2;
                    if negative {
                        pc = pc.wrapping_add_signed(i16::from(displacement));
                    }
                }
                0x4C => {
                    pc = u16::from_le_bytes([runtime.bytes[offset + 1], runtime.bytes[offset + 2]]);
                }
                0x60 => {
                    assert!(stack.is_empty());
                    return Execution { a, x, carry };
                }
                opcode => panic!("unsupported record-line replacement opcode {opcode:02X}"),
            }
        }
        panic!("record-line replacement did not terminate")
    }

    #[test]
    fn only_a_ready_replacement_request_clears_all_six_rows() {
        let runtime = build_record_line_replacement(0xA800).unwrap();
        for (state, policy, should_clear) in [
            (0, REPLACE_PREVIOUS_ROWS_PENDING, false),
            (STATE_READY, 0, false),
            (STATE_READY, REPLACE_PREVIOUS_ROWS_PENDING, true),
        ] {
            let mut memory = [0x5A; 0x10000];
            memory[usize::from(REQUEST_STATE)] = state;
            memory[usize::from(VISIBLE_PAGE_INDEX)] = policy;

            let execution = execute(&runtime, &mut memory, true);

            let end = LINE_BUFFER_START + u16::from(LINE_BUFFER_BYTE_COUNT);
            assert_eq!(
                memory[usize::from(LINE_BUFFER_START)..usize::from(end)]
                    .iter()
                    .all(|byte| *byte == LINE_BUFFER_BLANK),
                should_clear
            );
            assert_eq!(memory[usize::from(LINE_BUFFER_START - 1)], 0x5A);
            assert_eq!(memory[usize::from(end)], 0x5A);
            if should_clear {
                assert_eq!(
                    memory[usize::from(VISIBLE_PAGE_INDEX)] & REPLACE_PREVIOUS_ROWS_PENDING,
                    0
                );
            }
            assert_eq!(
                (execution.a, execution.x, execution.carry),
                (0x5A, 0xA5, true)
            );
        }
    }

    #[test]
    fn resolver_carry_is_preserved_for_both_completion_outcomes() {
        let runtime = build_record_line_replacement(0xA800).unwrap();
        for carry in [false, true] {
            let mut memory = [0x5A; 0x10000];
            memory[usize::from(REQUEST_STATE)] = STATE_READY;
            memory[usize::from(VISIBLE_PAGE_INDEX)] = REPLACE_PREVIOUS_ROWS_PENDING;

            let execution = execute(&runtime, &mut memory, carry);

            assert_eq!(execution.carry, carry);
            assert_eq!((execution.a, execution.x), (0x5A, 0xA5));
        }
    }
}
