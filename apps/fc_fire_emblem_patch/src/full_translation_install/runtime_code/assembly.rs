//! 런타임 실행 조각의 배치와 사이클 계산 규칙이다.

use anyhow::{Context, Result, ensure};

use crate::rp2a03::{Instruction, assemble_at};

/// ROM의 한 자리에 놓이는 실행 코드 조각이다.
#[derive(Debug)]
pub(in crate::full_translation_install) struct RuntimeRoutine {
    pub(in crate::full_translation_install) role: &'static str,
    pub(in crate::full_translation_install) address: u16,
    pub(in crate::full_translation_install) bytes: Vec<u8>,
}

/// 같은 동굴에 놓이는 조각들이 동굴 안에 있고 서로 겹치지 않아야 한다.
/// 범위 밖이면 원본 자료나 다른 실행 소유자를 덮고, 겹치면 조용히 잘못된 코드가
/// 실행된다.
pub(super) fn ensure_routines_fit_cave(
    routines: &[&RuntimeRoutine],
    cave_start: u16,
    cave_end: u16,
) -> Result<()> {
    ensure!(
        cave_start <= cave_end,
        "reserved cave starts at {cave_start:04X} after its end {cave_end:04X}"
    );
    let mut ordered: Vec<&RuntimeRoutine> = routines.to_vec();
    ordered.sort_by_key(|routine| routine.address);
    if let Some(first) = ordered.first() {
        ensure!(
            first.address >= cave_start,
            "{} starts at {:04X} before the reserved cave start {cave_start:04X}",
            first.role,
            first.address
        );
    }
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
            "{} ends at {:04X} and reaches past the reserved cave end {cave_end:04X}",
            last.role,
            usize::from(last.address) + last.bytes.len()
        );
    }
    Ok(())
}

/// 명령 목록을 이어 붙였을 때 다음 명령이 놓일 주소다. 분기 대상을 되메울 때 쓴다.
pub(super) fn next_address(origin: u16, instructions: &[Instruction]) -> Result<u16> {
    let length = assemble_at(origin, instructions)
        .context("cannot measure a dialogue runtime routine")?
        .len();
    u16::try_from(usize::from(origin) + length)
        .context("dialogue runtime routine crosses the CPU address space")
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

        let error = ensure_routines_fit_cave(&[&first, &second], 0xF400, 0xF4B0).unwrap_err();

        assert!(error.to_string().contains("overlaps"));
    }

    #[test]
    fn a_routine_past_the_cave_end_is_refused() {
        let only = RuntimeRoutine {
            role: "only",
            address: 0xF4A0,
            bytes: vec![0; 32],
        };

        let error = ensure_routines_fit_cave(&[&only], 0xF400, 0xF4B0).unwrap_err();

        assert!(error.to_string().contains("past the reserved cave end"));
    }

    #[test]
    fn a_routine_before_the_cave_start_is_refused() {
        let only = RuntimeRoutine {
            role: "before",
            address: 0xF3FF,
            bytes: vec![0; 1],
        };

        let error = ensure_routines_fit_cave(&[&only], 0xF400, 0xF4B0).unwrap_err();

        assert!(error.to_string().contains("before the reserved cave start"));
    }

    #[test]
    fn an_inverted_cave_is_refused_even_when_empty() {
        let error = ensure_routines_fit_cave(&[], 0xF4B0, 0xF400).unwrap_err();

        assert!(error.to_string().contains("after its end"));
    }
}
