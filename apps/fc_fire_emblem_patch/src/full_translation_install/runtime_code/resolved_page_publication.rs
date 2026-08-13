//! 해석기가 세운 페이지 그룹을 현재 상주 그룹과 비교해 다음 요청 상태를 게시한다.
//!
//! 초기 레코드 진입과 같은 레코드의 다음 페이지는 서로 다른 생산자지만, 해석이 끝난
//! 뒤 해야 할 일은 같다. 이전 그룹이 그대로면 CHR RAM은 이미 완성돼 있으므로 새 원문
//! 정체성만 게시하고 곧바로 `ready`로 둔다. 그룹이 달라지면 전송 소비자가 처리할
//! `cold_requested`를 게시한다. 해석 실패는 생산자가 미리 써 둔 `inactive`를 유지한다.
//!
//! 입력 계약은 둘이다. 캐리는 해석 성공 여부이고 A는 해석 전 상주 그룹이다. 초기
//! 생산자가 준비 상태 밖에서 들어왔으면 A에 `NO_RESIDENT_PAGE_GROUP`을 넣는다. 현재
//! 그룹 선택자는 동적 remap 표식까지 포함해 `0x00..0xFE`만 쓰므로 이 값과 충돌하지
//! 않는다.

use anyhow::Result;

use super::{RuntimeRoutine, next_address};
use crate::rp2a03::{Instruction, assemble_at};

use super::super::{
    runtime_cursor_storage::{PUBLISHED_SOURCE_DIRECTORY_SELECTOR, PUBLISHED_SOURCE_ENTRY_INDEX},
    runtime_nmi_contract::PPU_CONTROL_SHADOW,
    runtime_state_storage::CURRENT_PAGE_GROUP,
};
use super::{
    dispatcher_gate::STATE_COLD_REQUESTED,
    resolve_request::{SOURCE_DIRECTORY_SELECTOR, SOURCE_ENTRY_INDEX},
    transport::{REQUEST_STATE, STATE_READY},
};

const PPU_CONTROL: u16 = 0x2000;

/// 준비된 상주 그룹이 없었다는 생산자 입력이다. 페이지 그룹의 상위 비트는 동적
/// remap 표식이고 하위 일곱 비트가 실제 그룹이라 `FF`는 유효한 선택자가 아니다.
pub(super) const NO_RESIDENT_PAGE_GROUP: u8 = 0xFF;

/// 해석 결과를 게시하고 마지막에만 NMI 제어값을 하드웨어로 되돌린다.
pub(super) fn build_resolved_page_publication(origin: u16) -> Result<RuntimeRoutine> {
    let mut instructions = Vec::new();

    // 실패면 생산자가 미리 써 둔 inactive를 유지한다.
    let restore_after_failure = instructions.len();
    instructions.push(Instruction::BccAbsolute(origin));
    instructions.push(Instruction::CmpAbsolute(CURRENT_PAGE_GROUP));
    let cold_for_different_group = instructions.len();
    instructions.push(Instruction::BneAbsolute(origin));

    // 같은 그룹은 실제 CHR 쓰기 없이 새 원문 정체성을 phase union에 게시한다.
    instructions.extend([
        Instruction::LdaAbsolute(SOURCE_DIRECTORY_SELECTOR),
        Instruction::StaAbsolute(PUBLISHED_SOURCE_DIRECTORY_SELECTOR),
        Instruction::LdaAbsolute(SOURCE_ENTRY_INDEX),
        Instruction::StaAbsolute(PUBLISHED_SOURCE_ENTRY_INDEX),
        Instruction::LdaImmediate(STATE_READY),
    ]);
    let publish_ready = instructions.len();
    instructions.push(Instruction::BneAbsolute(origin));

    let cold = next_address(origin, &instructions)?;
    instructions[cold_for_different_group] = Instruction::BneAbsolute(cold);
    instructions.push(Instruction::LdaImmediate(STATE_COLD_REQUESTED));

    let publish = next_address(origin, &instructions)?;
    instructions[publish_ready] = Instruction::BneAbsolute(publish);
    instructions.push(Instruction::StaAbsolute(REQUEST_STATE));

    let restore_ppu = next_address(origin, &instructions)?;
    instructions[restore_after_failure] = Instruction::BccAbsolute(restore_ppu);
    instructions.extend([
        // 요청 상태가 정해진 뒤에만 NMI를 다시 켠다. 그러면 selector가 해석 중간의
        // 커서나 이전 수명의 ready를 볼 수 없다.
        Instruction::LdaZeroPage(PPU_CONTROL_SHADOW),
        Instruction::StaAbsolute(PPU_CONTROL),
        Instruction::Rts,
    ]);

    Ok(RuntimeRoutine {
        role: "resolved dialogue page publication",
        address: origin,
        bytes: assemble_at(origin, &instructions)?,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn readiness_is_published_before_nmi_is_restored() {
        let routine = build_resolved_page_publication(0xF354).unwrap();
        let publish = routine
            .bytes
            .windows(3)
            .position(|window| window == [0x8D, REQUEST_STATE as u8, (REQUEST_STATE >> 8) as u8])
            .expect("the resolved state is published");
        let restore_ppu = routine
            .bytes
            .windows(3)
            .position(|window| window == [0x8D, 0x00, 0x20])
            .expect("PPU control is restored");

        assert!(publish < restore_ppu);
    }

    #[test]
    fn same_group_reuse_republishes_both_source_identity_bytes() {
        let routine = build_resolved_page_publication(0xF354).unwrap();
        for (source, published) in [
            (
                SOURCE_DIRECTORY_SELECTOR,
                PUBLISHED_SOURCE_DIRECTORY_SELECTOR,
            ),
            (SOURCE_ENTRY_INDEX, PUBLISHED_SOURCE_ENTRY_INDEX),
        ] {
            let transfer = [
                0xAD,
                source as u8,
                (source >> 8) as u8,
                0x8D,
                published as u8,
                (published >> 8) as u8,
            ];
            assert!(
                routine
                    .bytes
                    .windows(transfer.len())
                    .any(|window| window == transfer),
                "source identity {source:04X} is not republished"
            );
        }
    }

    #[test]
    fn resolved_group_comparison_can_publish_ready_or_cold() {
        let routine = build_resolved_page_publication(0xF354).unwrap();

        assert!(
            routine
                .bytes
                .windows(2)
                .any(|window| window == [0xA9, STATE_READY])
        );
        assert!(
            routine
                .bytes
                .windows(2)
                .any(|window| window == [0xA9, STATE_COLD_REQUESTED])
        );
        assert!(routine.bytes.windows(3).any(|window| {
            window
                == [
                    0xCD,
                    CURRENT_PAGE_GROUP as u8,
                    (CURRENT_PAGE_GROUP >> 8) as u8,
                ]
        }));
    }
}
