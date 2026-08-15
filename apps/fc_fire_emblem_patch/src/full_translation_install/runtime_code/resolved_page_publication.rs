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
    runtime_cursor_storage::{
        CURSOR_OVERLAY_TILES, CURSOR_PHASE, CURSOR_REMAINING_TILES,
        PUBLISHED_SOURCE_DIRECTORY_SELECTOR, PUBLISHED_SOURCE_ENTRY_INDEX,
        REQUEST_SOURCE_DIRECTORY_SELECTOR, REQUEST_SOURCE_ENTRY_INDEX,
    },
    runtime_nmi_contract::PPU_CONTROL_SHADOW,
    runtime_state_storage::{CONSUMER_FONT_PAGE, CURRENT_PAGE_GROUP},
};
use super::{
    dispatcher_gate::{STATE_COLD_REQUESTED, STATE_RESIDENT_GROUP_OVERLAY_REQUESTED},
    transport::{PHASE_OVERLAY, REQUEST_STATE, STATE_READY},
};

const PPU_CONTROL: u16 = 0x2000;

/// 준비된 상주 그룹이 없었다는 생산자 입력이다. 페이지 그룹의 상위 비트는 동적
/// remap 표식이고 하위 일곱 비트가 실제 그룹이라 `FF`는 유효한 선택자가 아니다.
pub(super) const NO_RESIDENT_PAGE_GROUP: u8 = 0xFF;

/// 서로 다른 그룹을 해석한 뒤 이전 상주 그룹의 유무에 따라 요청 종류를 고른다.
///
/// 입력 A는 해석 전 상주 그룹이다. `FF`면 완성 기반이 없으므로 원본 4 KiB 복원부터
/// 시작하는 cold를 반환한다. 그 밖에는 resolver가 이미 세운 대상 그룹 항목 커서를
/// 그대로 두고 덮기 단계부터 시작한다. 두 경로 모두 상태가 게시되기 전에 냉간 표시
/// 페이지를 먼저 고른다.
pub(super) fn build_changed_group_request_initializer(
    origin: u16,
    cold_request_presentation_selector: u16,
) -> Result<RuntimeRoutine> {
    let mut instructions = vec![Instruction::CmpImmediate(NO_RESIDENT_PAGE_GROUP)];
    let cold_placeholder = instructions.len();
    instructions.push(Instruction::BeqAbsolute(origin));
    instructions.extend([
        Instruction::LdaImmediate(PHASE_OVERLAY),
        Instruction::StaAbsolute(CURSOR_PHASE),
        Instruction::LdaAbsolute(CURSOR_OVERLAY_TILES),
        Instruction::StaAbsolute(CURSOR_REMAINING_TILES),
        Instruction::JsrAbsolute(cold_request_presentation_selector),
        Instruction::LdaImmediate(STATE_RESIDENT_GROUP_OVERLAY_REQUESTED),
        Instruction::Rts,
    ]);

    let cold = next_address(origin, &instructions)?;
    instructions[cold_placeholder] = Instruction::BeqAbsolute(cold);
    instructions.extend([
        Instruction::JsrAbsolute(cold_request_presentation_selector),
        Instruction::LdaImmediate(STATE_COLD_REQUESTED),
        Instruction::Rts,
    ]);

    Ok(RuntimeRoutine {
        role: "changed dialogue group request initializer",
        address: origin,
        bytes: assemble_at(origin, &instructions)?,
    })
}

/// 해석 결과를 게시하고 마지막에만 NMI 제어값을 하드웨어로 되돌린다.
pub(super) fn build_resolved_page_publication(
    origin: u16,
    changed_group_request_initializer: u16,
) -> Result<RuntimeRoutine> {
    let mut instructions = Vec::new();

    // 실패면 생산자가 미리 써 둔 inactive를 유지한다.
    let restore_after_failure = instructions.len();
    instructions.push(Instruction::BccAbsolute(origin));
    // 상위 비트는 동적 문자열 remap 여부이고 하위 일곱 비트만 물리 코드북이다.
    // 완성 기반이 없는 FF는 따로 거른 뒤, remap 비트만 다른 같은 코드북은 CHR을
    // 다시 쓰지 않고 새 selector와 원문 정체성만 게시한다.
    instructions.push(Instruction::CmpImmediate(NO_RESIDENT_PAGE_GROUP));
    let no_resident_group_placeholder = instructions.len();
    instructions.push(Instruction::BeqAbsolute(origin));
    instructions.extend([
        Instruction::AndImmediate(0x7F),
        Instruction::StaAbsolute(REQUEST_STATE),
        Instruction::LdaAbsolute(CURRENT_PAGE_GROUP),
        Instruction::AndImmediate(0x7F),
        Instruction::CmpAbsolute(REQUEST_STATE),
    ]);
    let changed_group_placeholder = instructions.len();
    instructions.push(Instruction::BneAbsolute(origin));

    // 같은 그룹은 실제 CHR 쓰기 없이 새 원문 정체성을 phase union에 게시한다.
    instructions.extend([
        Instruction::LdaAbsolute(REQUEST_SOURCE_DIRECTORY_SELECTOR),
        Instruction::StaAbsolute(PUBLISHED_SOURCE_DIRECTORY_SELECTOR),
        Instruction::LdaAbsolute(REQUEST_SOURCE_ENTRY_INDEX),
        Instruction::StaAbsolute(PUBLISHED_SOURCE_ENTRY_INDEX),
        Instruction::LdaImmediate(0),
        Instruction::StaAbsolute(CONSUMER_FONT_PAGE),
        Instruction::LdaImmediate(STATE_READY),
    ]);
    let publish_ready = instructions.len();
    instructions.push(Instruction::BneAbsolute(origin));

    let resident_group_changed = next_address(origin, &instructions)?;
    instructions[changed_group_placeholder] = Instruction::BneAbsolute(resident_group_changed);
    // 변경 초기화기는 이전 그룹의 정확한 번호가 아니라 상주 기반의 유무만 구분한다.
    // 유효한 상주 그룹임을 나타내는 0을 넘기면 스택에 이전 A와 비교 플래그를 함께
    // 보관했다가 뒤바꾸어 꺼낼 이유가 없다.
    instructions.push(Instruction::LdaImmediate(0));
    let initialize_changed_group = next_address(origin, &instructions)?;
    instructions.push(Instruction::JsrAbsolute(changed_group_request_initializer));
    let publish_changed_group = instructions.len();
    instructions.push(Instruction::BneAbsolute(origin));

    let no_resident_group = next_address(origin, &instructions)?;
    instructions[no_resident_group_placeholder] = Instruction::BeqAbsolute(no_resident_group);
    instructions.push(Instruction::LdaImmediate(NO_RESIDENT_PAGE_GROUP));
    instructions.push(Instruction::JmpAbsolute(initialize_changed_group));

    let publish = next_address(origin, &instructions)?;
    instructions[publish_ready] = Instruction::BneAbsolute(publish);
    instructions[publish_changed_group] = Instruction::BneAbsolute(publish);
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

    const PUBLICATION_ORIGIN: u16 = 0xF354;
    const COLD_REQUEST_PRESENTATION_SELECTOR: u16 = 0xF5A0;
    const CHANGED_GROUP_REQUEST_INITIALIZER: u16 = 0xF5B0;

    fn publication() -> RuntimeRoutine {
        build_resolved_page_publication(PUBLICATION_ORIGIN, CHANGED_GROUP_REQUEST_INITIALIZER)
            .unwrap()
    }

    #[test]
    fn readiness_is_published_before_nmi_is_restored() {
        let routine = publication();
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
    fn a_changed_group_is_initialized_before_nmi_resumes() {
        let routine = publication();
        let initialize = routine
            .bytes
            .windows(3)
            .position(|window| {
                window
                    == [
                        0x20,
                        CHANGED_GROUP_REQUEST_INITIALIZER as u8,
                        (CHANGED_GROUP_REQUEST_INITIALIZER >> 8) as u8,
                    ]
            })
            .expect("changed-group publication initializes its request");
        let restore_ppu = routine
            .bytes
            .windows(3)
            .position(|window| window == [0x8D, 0x00, 0x20])
            .expect("publication restores PPU control");

        assert!(initialize < restore_ppu);
    }

    #[test]
    fn a_resident_group_starts_at_the_overlay_phase_without_source_restore() {
        let routine = build_changed_group_request_initializer(
            CHANGED_GROUP_REQUEST_INITIALIZER,
            COLD_REQUEST_PRESENTATION_SELECTOR,
        )
        .unwrap();

        assert!(routine.bytes.windows(16).any(|window| {
            window
                == [
                    0xA9,
                    PHASE_OVERLAY,
                    0x8D,
                    CURSOR_PHASE as u8,
                    (CURSOR_PHASE >> 8) as u8,
                    0xAD,
                    CURSOR_OVERLAY_TILES as u8,
                    (CURSOR_OVERLAY_TILES >> 8) as u8,
                    0x8D,
                    CURSOR_REMAINING_TILES as u8,
                    (CURSOR_REMAINING_TILES >> 8) as u8,
                    0x20,
                    COLD_REQUEST_PRESENTATION_SELECTOR as u8,
                    (COLD_REQUEST_PRESENTATION_SELECTOR >> 8) as u8,
                    0xA9,
                    STATE_RESIDENT_GROUP_OVERLAY_REQUESTED,
                ]
        }));
    }

    #[test]
    fn no_resident_group_keeps_the_full_cold_request() {
        let routine = build_changed_group_request_initializer(
            CHANGED_GROUP_REQUEST_INITIALIZER,
            COLD_REQUEST_PRESENTATION_SELECTOR,
        )
        .unwrap();

        assert!(routine.bytes.windows(6).any(|window| {
            window
                == [
                    0x20,
                    COLD_REQUEST_PRESENTATION_SELECTOR as u8,
                    (COLD_REQUEST_PRESENTATION_SELECTOR >> 8) as u8,
                    0xA9,
                    STATE_COLD_REQUESTED,
                    0x60,
                ]
        }));
    }

    #[test]
    fn same_group_reuse_republishes_both_source_identity_bytes() {
        let routine = publication();
        for (source, published) in [
            (
                REQUEST_SOURCE_DIRECTORY_SELECTOR,
                PUBLISHED_SOURCE_DIRECTORY_SELECTOR,
            ),
            (REQUEST_SOURCE_ENTRY_INDEX, PUBLISHED_SOURCE_ENTRY_INDEX),
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
    fn resolved_group_comparison_can_publish_ready_or_initialize_a_change() {
        let routine = publication();

        assert!(
            routine
                .bytes
                .windows(2)
                .any(|window| window == [0xA9, STATE_READY])
        );
        assert!(routine.bytes.windows(3).any(|window| {
            window
                == [
                    0x20,
                    CHANGED_GROUP_REQUEST_INITIALIZER as u8,
                    (CHANGED_GROUP_REQUEST_INITIALIZER >> 8) as u8,
                ]
        }));
        assert!(routine.bytes.windows(13).any(|window| {
            window
                == [
                    0x29,
                    0x7F,
                    0x8D,
                    REQUEST_STATE as u8,
                    (REQUEST_STATE >> 8) as u8,
                    0xAD,
                    CURRENT_PAGE_GROUP as u8,
                    (CURRENT_PAGE_GROUP >> 8) as u8,
                    0x29,
                    0x7F,
                    0xCD,
                    REQUEST_STATE as u8,
                    (REQUEST_STATE >> 8) as u8,
                ]
        }));
    }

    /// 회귀: 비교 중 `PHA; PHP; PLA; PLP`로 이전 그룹과 플래그를 함께 보관하면
    /// 두 값을 반대 순서로 복원해 CPU 상태를 오염시켰고, 실제 실행은 피연산자
    /// `$8D0F`의 JAM opcode로 잘못 복귀했다. 게시 분류는 호출자의 스택을 전혀
    /// 빌리지 않아야 한다.
    #[test]
    fn resolved_group_classification_does_not_borrow_the_caller_stack() {
        let routine = publication();

        for stack_opcode in [0x48, 0x08, 0x68, 0x28] {
            assert!(
                !routine.bytes.contains(&stack_opcode),
                "resolved-page publication contains stack opcode {stack_opcode:02X}"
            );
        }
    }
}
