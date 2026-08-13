//! 대사 런타임이 ROM에 넣는 실행 코드다.
//!
//! 갈래를 나눈 기준은 «무엇이 바뀌면 이 파일이 바뀌는가»다. 전송 루프는 프레임
//! 예산이 바뀌면 바뀌고, 트램폴린은 원본 NMI 계약이 바뀌면 바뀐다.

use anyhow::{Context, Result, ensure};

use crate::rp2a03::{Instruction, assemble_at};

pub(super) mod transport;

/// ROM의 한 자리에 놓이는 실행 코드 조각이다.
pub(super) struct RuntimeRoutine {
    pub(super) role: &'static str,
    pub(super) address: u16,
    pub(super) bytes: Vec<u8>,
}

/// 같은 동굴에 놓이는 조각들이 서로 겹치거나 동굴을 넘지 않아야 한다.
/// 겹치면 조용히 잘못된 코드가 실행되고, 넘으면 원본 자료를 덮는다.
pub(super) fn ensure_disjoint(routines: &[&RuntimeRoutine], cave_end: u16) -> Result<()> {
    let mut ordered: Vec<&RuntimeRoutine> = routines.to_vec();
    ordered.sort_by_key(|routine| routine.address);
    for pair in ordered.windows(2) {
        ensure!(
            usize::from(pair[0].address) + pair[0].bytes.len() <= usize::from(pair[1].address),
            "{} ends at {:04X} and overlaps {} at {:04X}",
            pair[0].role,
            usize::from(pair[0].address) + pair[0].bytes.len(),
            pair[1].role,
            pair[1].address
        );
    }
    if let Some(last) = ordered.last() {
        ensure!(
            usize::from(last.address) + last.bytes.len() <= usize::from(cave_end),
            "{} reaches past the reserved cave end {cave_end:04X}",
            last.role
        );
    }
    Ok(())
}

/// 명령 목록을 이어 붙였을 때 다음 명령이 놓일 주소다. 분기 대상을 되메울 때 쓴다.
fn next_address(origin: u16, instructions: &[Instruction]) -> Result<u16> {
    let length = assemble_at(origin, instructions)
        .context("cannot measure a dialogue runtime routine")?
        .len();
    u16::try_from(usize::from(origin) + length)
        .context("dialogue runtime routine crosses the CPU address space")
}

/// 명령 목록이 최악의 경우 쓰는 사이클이다.
fn worst_case_cycles(instructions: &[Instruction]) -> u32 {
    instructions
        .iter()
        .map(|instruction| u32::from(instruction.worst_case_cycles()))
        .sum()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn overlapping_routines_are_refused() {
        let first = RuntimeRoutine {
            role: "first",
            address: 0xF400,
            bytes: vec![0; 16],
        };
        let second = RuntimeRoutine {
            role: "second",
            address: 0xF408,
            bytes: vec![0; 4],
        };

        let error = ensure_disjoint(&[&first, &second], 0xF4B0).unwrap_err();

        assert!(error.to_string().contains("overlaps"));
    }

    #[test]
    fn a_routine_past_the_cave_end_is_refused() {
        let only = RuntimeRoutine {
            role: "only",
            address: 0xF4A0,
            bytes: vec![0; 32],
        };

        let error = ensure_disjoint(&[&only], 0xF4B0).unwrap_err();

        assert!(error.to_string().contains("past the reserved cave end"));
    }
}
