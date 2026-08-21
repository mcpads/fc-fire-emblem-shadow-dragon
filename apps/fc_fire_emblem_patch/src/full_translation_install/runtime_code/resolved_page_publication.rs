//! 해석기가 세운 가시 페이지 레시피를 현재 상주권과 결합해 다음 요청 상태를 게시한다.
//!
//! 초기 레코드 진입과 같은 레코드의 다음 페이지는 서로 다른 생산자지만, 해석이 끝난
//! 뒤 해야 할 일은 같다. resolver가 직전 상주 그룹과 새 그룹을 먼저 비교하고, 같으면
//! 이미 완성된 CHR-RAM을 그대로 `ready`로 게시한다. 그룹이 달라 실제 합성이 필요할
//! 때만 완성 기반이 없으면 원본 4 KiB 복원부터, 기반이 있으면 새 그룹 오버레이부터
//! 요청한다. 해석 실패는 생산자가 미리 써 둔 `inactive`를 유지한다.
//!
//! 입력 계약은 둘이다. 캐리는 해석 성공 여부이고 A는 해석 전 상주 그룹 색인이다.
//! 독립 수명에서 들어왔으면 A에 `NO_RESIDENT_PAGE_RECIPE`를 넣는다. 유효 그룹 색인은
//! `0xFF`보다 작으므로 없음 표식과 충돌하지 않는다.

use anyhow::Result;

use super::{RuntimeRoutine, next_address};
use crate::rp2a03::{Instruction, assemble_at};

use super::super::{
    runtime_cursor_storage::{CURSOR_OVERLAY_TILES, CURSOR_PHASE, CURSOR_REMAINING_TILES},
    runtime_nmi_contract::CONTROL_RESTORE_ADDRESS,
};
use super::{
    dispatcher_gate::{STATE_COLD_REQUESTED, STATE_RESIDENT_PAGE_OVERLAY_REQUESTED},
    transport::{PHASE_OVERLAY, REQUEST_STATE},
};

/// 준비된 가시 페이지가 없었다는 생산자 입력이다.
pub(super) const NO_RESIDENT_PAGE_RECIPE: u8 = 0xFF;

/// 새 상주 그룹을 해석한 뒤 이전 상주 그룹의 유무에 따라 요청 종류를 고른다.
///
/// 입력 A는 해석 전 상주 그룹 색인이다. `FF`면 완성 기반이 없으므로 원본 4 KiB
/// 복원부터 시작하는 cold를 반환한다. 그 밖에는 resolver가 이미 세운 대상 그룹
/// 레시피 항목 커서를 그대로 두고 덮기 단계부터 시작한다. 같은 그룹 재사용은 이
/// 루틴에 오기 전에 resolver가 `ready`로 끝낸다. 두 합성 경로 모두 상태가 게시되기
/// 전에 냉간 표시 페이지를 먼저 고른다.
pub(super) fn build_page_recipe_request_initializer(
    origin: u16,
    cold_request_presentation_selector: u16,
) -> Result<RuntimeRoutine> {
    let mut instructions = vec![Instruction::CmpImmediate(NO_RESIDENT_PAGE_RECIPE)];
    let cold_placeholder = instructions.len();
    instructions.push(Instruction::BeqAbsolute(origin));
    instructions.extend([
        Instruction::LdaImmediate(PHASE_OVERLAY),
        Instruction::StaAbsolute(CURSOR_PHASE),
        Instruction::LdaAbsolute(CURSOR_OVERLAY_TILES),
        Instruction::StaAbsolute(CURSOR_REMAINING_TILES),
        Instruction::JsrAbsolute(cold_request_presentation_selector),
        Instruction::LdaImmediate(STATE_RESIDENT_PAGE_OVERLAY_REQUESTED),
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
        role: "dialogue page-recipe request initializer",
        address: origin,
        bytes: assemble_at(origin, &instructions)?,
    })
}

/// 해석 결과를 게시하고 성공·실패 경로를 각 복구 책임자에게 tail-call한다.
pub(super) fn build_resolved_page_publication(
    origin: u16,
    page_recipe_request_initializer: u16,
    synchronous_composer: u16,
) -> Result<RuntimeRoutine> {
    let mut instructions = Vec::new();

    // 실패면 생산자가 미리 써 둔 inactive를 유지하고, 원본의 PPU control/mask
    // 복구 루틴으로 곧장 돌아간다.
    let restore_after_failure = instructions.len();
    instructions.push(Instruction::BccAbsolute(origin));
    // 입력 A는 resolver 호출 전에 저장해 둔 상주권이다. FF면 cold, 그 밖이면 현재
    // 레시피만 덮는 resident 요청을 초기화한다.
    instructions.push(Instruction::JsrAbsolute(page_recipe_request_initializer));
    let publish_request = instructions.len();
    instructions.push(Instruction::BneAbsolute(origin));

    let restore_ppu = next_address(origin, &instructions)?;
    instructions[restore_after_failure] = Instruction::BccAbsolute(restore_ppu);
    instructions.push(Instruction::JmpAbsolute(CONTROL_RESTORE_ADDRESS));

    let publish = next_address(origin, &instructions)?;
    instructions[publish_request] = Instruction::BneAbsolute(publish);
    instructions.extend([
        Instruction::StaAbsolute(REQUEST_STATE),
        // NMI를 다시 켜기 전에 한 연속 render-off 구간에서 요청을 ready로 만든다.
        // cold와 resident, 초기 진입과 다음 페이지가 모두 이 한 소비자를 쓴다. 합성기는
        // PPU control과 호출자 상태를 복구한 뒤 이 routine의 호출자에게 직접 RTS한다.
        Instruction::JmpAbsolute(synchronous_composer),
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
    const PAGE_RECIPE_REQUEST_INITIALIZER: u16 = 0xF5B0;
    const SYNCHRONOUS_COMPOSER: u16 = 0xF400;

    fn publication() -> RuntimeRoutine {
        build_resolved_page_publication(
            PUBLICATION_ORIGIN,
            PAGE_RECIPE_REQUEST_INITIALIZER,
            SYNCHRONOUS_COMPOSER,
        )
        .unwrap()
    }

    #[test]
    fn failure_and_success_tail_call_their_distinct_resume_paths() {
        let routine = publication();
        assert!(routine.bytes.windows(3).any(|window| {
            window
                == [
                    0x4C,
                    CONTROL_RESTORE_ADDRESS as u8,
                    (CONTROL_RESTORE_ADDRESS >> 8) as u8,
                ]
        }));
        assert!(routine.bytes.windows(3).any(|window| {
            window
                == [
                    0x4C,
                    SYNCHRONOUS_COMPOSER as u8,
                    (SYNCHRONOUS_COMPOSER >> 8) as u8,
                ]
        }));
        assert!(!routine.bytes.contains(&0x60));
    }

    #[test]
    fn every_successful_request_is_composed_before_nmi_is_restored() {
        let routine = publication();
        let publish = routine
            .bytes
            .windows(3)
            .position(|window| window == [0x8D, REQUEST_STATE as u8, (REQUEST_STATE >> 8) as u8])
            .expect("the request state is published");
        let compose = routine
            .bytes
            .windows(3)
            .position(|window| {
                window
                    == [
                        0x4C,
                        SYNCHRONOUS_COMPOSER as u8,
                        (SYNCHRONOUS_COMPOSER >> 8) as u8,
                    ]
            })
            .expect("the synchronous composer is tail-called");

        assert!(publish < compose);
    }

    #[test]
    fn a_resolved_page_recipe_is_initialized_before_nmi_resumes() {
        let routine = publication();
        let initialize = routine
            .bytes
            .windows(3)
            .position(|window| {
                window
                    == [
                        0x20,
                        PAGE_RECIPE_REQUEST_INITIALIZER as u8,
                        (PAGE_RECIPE_REQUEST_INITIALIZER >> 8) as u8,
                    ]
            })
            .expect("page-recipe publication initializes its request");
        let compose = routine
            .bytes
            .windows(3)
            .position(|window| {
                window
                    == [
                        0x4C,
                        SYNCHRONOUS_COMPOSER as u8,
                        (SYNCHRONOUS_COMPOSER >> 8) as u8,
                    ]
            })
            .expect("publication tail-calls the synchronous composer");

        assert!(initialize < compose);
    }

    #[test]
    fn a_resident_page_starts_at_the_overlay_phase_without_source_restore() {
        let routine = build_page_recipe_request_initializer(
            PAGE_RECIPE_REQUEST_INITIALIZER,
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
                    STATE_RESIDENT_PAGE_OVERLAY_REQUESTED,
                ]
        }));
    }

    #[test]
    fn no_resident_page_keeps_the_full_cold_request() {
        let routine = build_page_recipe_request_initializer(
            PAGE_RECIPE_REQUEST_INITIALIZER,
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
    fn every_new_page_recipe_is_sent_to_the_transport() {
        let routine = publication();
        assert!(routine.bytes.windows(3).any(|window| {
            window
                == [
                    0x20,
                    PAGE_RECIPE_REQUEST_INITIALIZER as u8,
                    (PAGE_RECIPE_REQUEST_INITIALIZER >> 8) as u8,
                ]
        }));
        assert!(!routine.bytes.windows(2).any(|window| window == [0xA9, 3]));
    }

    /// 회귀: 비교 중 `PHA; PHP; PLA; PLP`로 이전 그룹과 플래그를 함께 보관하면
    /// 두 값을 반대 순서로 복원해 CPU 상태를 오염시켰고, 실제 실행은 피연산자
    /// `$8D0F`의 JAM opcode로 잘못 복귀했다. 게시 분류는 호출자의 스택을 전혀
    /// 빌리지 않아야 한다.
    #[test]
    fn resolved_page_classification_does_not_borrow_the_caller_stack() {
        let routine = publication();
        let mut remaining = routine.bytes.as_slice();

        while !remaining.is_empty() {
            let instruction = retro_rp2a03::decode_bytes(remaining).unwrap();
            assert!(
                !matches!(
                    instruction.mnemonic().to_string().as_str(),
                    "PHA" | "PHP" | "PLA" | "PLP"
                ),
                "resolved-page publication borrows the caller stack through {}",
                instruction.mnemonic()
            );
            remaining = &remaining[instruction.encoded_len()..];
        }
    }
}
