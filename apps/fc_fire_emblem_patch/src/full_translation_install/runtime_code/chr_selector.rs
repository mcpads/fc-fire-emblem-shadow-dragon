//! 합성이 끝난 뒤에만 CHR RAM을 고르는 selector다.
//!
//! 전송이 타일을 올려도 화면이 CHR RAM을 보지 않으면 아무것도 바뀌지 않는다.
//! 탐침에서 `$2007` 쓰기가 버려진 것이 그 증거였다.
//!
//! 자리는 원본 CHR selector 사슬의 `$FF40`이다. 지금 값은 폐기된 표본 selector로
//! 넘기는 `JMP $F990`이다. 전역 selector는 그 표본을 대체하므로 준비되지 않았을
//! 때는 표본 뒤의 실제 기존 소비자 `$FB80`으로 직접 넘긴다.
//!
//! 준비되지 않았을 때 CHR RAM을 고르면 아직 올라가지 않은 타일이 화면에 나온다.
//! `cold_requested`는 한글 슬롯만 빈 전용 CHR-ROM 페이지를 고르고, 그 밖의 알 수 없는
//! 상태는 기존 사슬로 넘긴다.
//!
//! 실행에서 맵 타일 자리에 한글이 나온 원인은 CHR RAM의 내용이 아니라 **표시 선택이
//! FD와 FE를 모두 RAM으로 바꾼 것**이었다. mapper165의 레지스터 2는 오른쪽 FD,
//! 레지스터 4는 오른쪽 FE다. 원본 대사 글꼴은 FD 페이지 0에 있고 맵 배경은 FE에
//! 남으므로, 둘을 같은 RAM 페이지로 합치면 같은 타일 코드의 서로 다른 두 패턴을
//! 표현할 수 없다.
//!
//! 전송 중에는 현재 래치와 관계없이 `$2007` 쓰기가 RAM에 닿도록 두 레지스터를 모두
//! 잠시 RAM으로 건다. 전송 이탈은 둘을 원천 상태로 되돌린다. 이 selector가 맡는 것은
//! 그 뒤의 **표시 상태**뿐이다. 마지막 전송 프레임이 FD를 한 번 직접 게시하고, 이후
//! 중앙 FD 공급자가 다시 불릴 때 이 selector가 같은 선택을 유지한다. 준비 완료이고
//! 원본 대사 상태가 종단 전이며 중앙 FD 원천이 페이지 0일 때 레지스터 2만 RAM으로
//! 바꾸고, 레지스터 4는 방금 복원한 원본 FE 페이지에 둔다.
//!
//! `$29`는 이 수명의 판정값이 아니다. NMI가 임시 PRG 매핑 뒤 되돌릴 16 KiB 뱅크의
//! 그림자이므로 실제 1장 완성 대사에서도 `06`이었다. 주 대사 코드는 뱅크 `0A`에
//! 있지만 표시 중 주 루프가 어느 뱅크를 소유하는지는 별개다. 수명은 요청 상태와 원본
//! 대사 종단 상태로 판정한다.
//!
//! **사슬은 누산기에 값을 싣고 다닌다.** `$FF1D`가 `PHP PHA`로 시작해 그 값을 페이지
//! 번호로 쓰므로, 끼어드는 쪽이 `LDA`로 누산기를 덮으면 뒤따르는 소비자가 남의 값을
//! 페이지로 읽는다. 실행하면 화면 전체가 깨진다 — 그렇게 확인했다. 그래서 이 selector는
//! 판정하기 전에 누산기와 상태를 밀어 두고 넘기기 전에 되돌린다.

use anyhow::{Context, Result, ensure};

use super::{
    RuntimeRoutine,
    chr_source_state::{
        CHR_RAM_BANK_VALUE, CHR_SOURCE_HIGH_BITS, DIALOGUE_FD_SOURCE_PAGE, RIGHT_FD_SOURCE_SHADOW,
    },
    dispatcher_gate::{
        DISPATCHER_STATE, STATE_COLD_REQUESTED, STATE_RESIDENT_GROUP_OVERLAY_REQUESTED,
    },
    lifecycle::TERMINAL_STATE,
    next_address,
    transport::{REQUEST_STATE, STATE_READY},
};
use crate::{
    chapter_transition::{ENDING_CHARACTER_EPILOGUE_VISIBLE_PHASE, ENDING_RECORD_PHASE_ADDRESS},
    rom::Rom,
    rp2a03::{Instruction, assemble_at},
    typed_source::decode_rp2a03_sequence,
};

/// selector 사슬에서 이 런타임이 가져가는 자리다.
pub(in crate::full_translation_install) const SELECTOR_CHAIN_SITE: u16 = 0xFF40;
/// 후보의 사슬 자리가 지금 넘기는 표본 selector다. 설치 선행 조건으로만 쓴다.
const SELECTOR_CHAIN_SOURCE_TARGET: u16 = 0xF990;
/// 표본 selector 뒤의 기존 소비자다. 전역 selector의 비활성 경로는 여기로 간다.
pub(in crate::full_translation_install) const SELECTOR_CHAIN_FALLBACK: u16 = 0xFB80;
/// `$FF40`: `JMP $F990`.
const SELECTOR_CHAIN_CODE: [u8; 3] = [
    0x4C,
    SELECTOR_CHAIN_SOURCE_TARGET as u8,
    (SELECTOR_CHAIN_SOURCE_TARGET >> 8) as u8,
];

/// 준비된 FD 표시 selector가 쓰는 별도 고정 뱅크 동굴이다. 요청 발행기와 한 동굴에
/// 이어 붙이면 반복 요청 판정이 커질 때 서로를 침범하므로 역할별로 분리한다.
pub(super) const SELECTOR_CAVE_ORIGIN: u16 = 0xF558;
pub(super) const SELECTOR_CAVE_END: u16 = 0xF700;
/// selector가 그 자리를 가져가기 전에 아직 그대로인지 확인한다.
///
/// 후보만 본다. `$FF40`의 사슬은 매퍼 165 변환이 세운 구조물이라 원본 일본어 ROM에는
/// 없다. 원본까지 보게 하면 없는 것을 있다고 요구하게 된다.
pub(super) fn bind_selector_chain_site(candidate: &Rom) -> Result<()> {
    {
        let prg = candidate.prg();
        let base = prg
            .len()
            .checked_sub(16 * 1024)
            .context("PRG is smaller than one fixed bank")?;
        let offset = base + usize::from(SELECTOR_CHAIN_SITE) - 0xC000;
        let bytes = prg
            .get(offset..offset + SELECTOR_CHAIN_CODE.len())
            .context("CHR selector chain site is outside ROM")?;
        ensure!(
            bytes == SELECTOR_CHAIN_CODE,
            "the CHR selector chain site at {SELECTOR_CHAIN_SITE:04X} no longer hands the existing chain control"
        );
    }
    decode_rp2a03_sequence(
        &SELECTOR_CHAIN_CODE,
        SELECTOR_CHAIN_SITE,
        "CHR selector chain site",
    )?;
    Ok(())
}

/// selector 전용 동굴 전체가 아직 예약된 `FF`인지 묶는다.
///
/// 설치자가 실제 쓰는 바이트도 다시 검사하지만, 여기서는 이 모듈이 소유한다고
/// 선언한 구간의 경계 자체가 다른 생산자와 드리프트하지 않았음을 확인한다.
pub(super) fn bind_selector_cave(candidate: &Rom) -> Result<()> {
    let prg = candidate.prg();
    let base = prg
        .len()
        .checked_sub(16 * 1024)
        .context("PRG is smaller than one fixed bank")?;
    let start = base + usize::from(SELECTOR_CAVE_ORIGIN) - 0xC000;
    let end = base + usize::from(SELECTOR_CAVE_END) - 0xC000;
    let bytes = prg
        .get(start..end)
        .context("CHR selector cave is outside ROM")?;
    ensure!(
        bytes.iter().all(|byte| *byte == 0xFF),
        "the CHR selector cave at {SELECTOR_CAVE_ORIGIN:04X}..{SELECTOR_CAVE_END:04X} is not exact FF"
    );
    Ok(())
}

/// `$FF40`에 쓸 세 바이트다.
pub(super) fn selector_hook_bytes(selector: u16) -> [u8; 3] {
    [0x4C, selector as u8, (selector >> 8) as u8]
}

pub(super) fn build_chr_selector(
    origin: u16,
    cold_request_mapper_register: u8,
    fallback: u16,
    project_dialogue_page: u16,
) -> Result<RuntimeRoutine> {
    let mut instructions = vec![
        // 사슬이 나르는 누산기와 상태를 밀어 둔다.
        Instruction::Php,
        Instruction::Pha,
        Instruction::LdaAbsolute(REQUEST_STATE),
        Instruction::CmpImmediate(STATE_READY),
    ];
    let ready_placeholder = instructions.len();
    instructions.push(Instruction::BeqAbsolute(origin));
    instructions.push(Instruction::CmpImmediate(STATE_COLD_REQUESTED));
    let cold_state_placeholder = instructions.len();
    instructions.push(Instruction::BeqAbsolute(origin));
    instructions.push(Instruction::CmpImmediate(
        STATE_RESIDENT_GROUP_OVERLAY_REQUESTED,
    ));
    let resident_overlay_state_placeholder = instructions.len();
    instructions.push(Instruction::BeqAbsolute(origin));
    let unsupported_state_placeholder = instructions.len();
    instructions.push(Instruction::BneAbsolute(origin));
    let eligible_state = next_address(origin, &instructions)?;
    instructions[ready_placeholder] = Instruction::BeqAbsolute(eligible_state);
    instructions[cold_state_placeholder] = Instruction::BeqAbsolute(eligible_state);
    instructions[resident_overlay_state_placeholder] = Instruction::BeqAbsolute(eligible_state);
    instructions.extend([
        Instruction::LdaAbsolute(DISPATCHER_STATE),
        Instruction::CmpImmediate(TERMINAL_STATE),
    ]);
    let active_dialogue_placeholder = instructions.len();
    instructions.push(Instruction::BccAbsolute(origin));
    let past_terminal_placeholder = instructions.len();
    instructions.push(Instruction::BneAbsolute(origin));
    instructions.extend([
        Instruction::LdaAbsolute(ENDING_RECORD_PHASE_ADDRESS),
        Instruction::CmpImmediate(ENDING_CHARACTER_EPILOGUE_VISIBLE_PHASE),
    ]);
    let terminal_outside_epilogue_placeholder = instructions.len();
    instructions.push(Instruction::BneAbsolute(origin));
    let visible_dialogue = next_address(origin, &instructions)?;
    instructions[active_dialogue_placeholder] = Instruction::BccAbsolute(visible_dialogue);
    instructions.extend([
        Instruction::LdaZeroPage(RIGHT_FD_SOURCE_SHADOW),
        Instruction::OraZeroPage(CHR_SOURCE_HIGH_BITS),
        Instruction::AndImmediate(0x1F),
        Instruction::CmpImmediate(DIALOGUE_FD_SOURCE_PAGE),
    ]);
    let wrong_fd_source_placeholder = instructions.len();
    instructions.push(Instruction::BneAbsolute(origin));
    instructions.extend([
        Instruction::LdaAbsolute(REQUEST_STATE),
        Instruction::CmpImmediate(STATE_READY),
    ]);
    let complete_request_placeholder = instructions.len();
    instructions.push(Instruction::BeqAbsolute(origin));
    let incomplete_request_placeholder = instructions.len();
    instructions.push(Instruction::BneAbsolute(origin));
    let complete_request = next_address(origin, &instructions)?;
    instructions[complete_request_placeholder] = Instruction::BeqAbsolute(complete_request);
    instructions.extend([
        Instruction::LdaImmediate(CHR_RAM_BANK_VALUE),
        Instruction::JsrAbsolute(project_dialogue_page),
        Instruction::Pla,
        Instruction::Plp,
        Instruction::Rts,
    ]);
    let incomplete_request = next_address(origin, &instructions)?;
    instructions[incomplete_request_placeholder] = Instruction::BneAbsolute(incomplete_request);
    instructions.extend([
        Instruction::LdaImmediate(cold_request_mapper_register),
        Instruction::JsrAbsolute(project_dialogue_page),
        Instruction::Pla,
        Instruction::Plp,
        Instruction::Rts,
    ]);
    let unsupported_state = next_address(origin, &instructions)?;
    instructions[unsupported_state_placeholder] = Instruction::BneAbsolute(unsupported_state);
    instructions[past_terminal_placeholder] = Instruction::BneAbsolute(unsupported_state);
    instructions[terminal_outside_epilogue_placeholder] =
        Instruction::BneAbsolute(unsupported_state);
    instructions[wrong_fd_source_placeholder] = Instruction::BneAbsolute(unsupported_state);
    instructions.extend([
        Instruction::Pla,
        Instruction::Plp,
        Instruction::JmpAbsolute(fallback),
    ]);

    Ok(RuntimeRoutine {
        role: "dialogue CHR RAM selector",
        address: origin,
        bytes: assemble_at(origin, &instructions)?,
    })
}

/// 요청 발행기가 NMI를 다시 켜기 전에 냉간 표시 페이지를 원자적으로 고른다.
pub(super) fn build_cold_request_presentation_selector(
    origin: u16,
    cold_request_mapper_register: u8,
    project_dialogue_page: u16,
) -> Result<RuntimeRoutine> {
    let instructions = [
        Instruction::LdaImmediate(cold_request_mapper_register),
        Instruction::JmpAbsolute(project_dialogue_page),
    ];
    Ok(RuntimeRoutine {
        role: "cold-request dialogue presentation selector",
        address: origin,
        bytes: assemble_at(origin, &instructions)?,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    const PROJECT_DIALOGUE_PAGE: u16 = 0xF480;

    /// 알 수 없는 상태에서 새 페이지를 고르면 원본의 다른 화면을 침범할 수 있다.
    #[test]
    fn an_unsupported_state_hands_the_existing_chain_control() {
        let routine =
            build_chr_selector(0xF4A0, 0xC8, SELECTOR_CHAIN_FALLBACK, PROJECT_DIALOGUE_PAGE)
                .unwrap();

        assert_eq!(
            &routine.bytes[routine.bytes.len() - 3..],
            [
                0x4C,
                SELECTOR_CHAIN_FALLBACK as u8,
                (SELECTOR_CHAIN_FALLBACK >> 8) as u8
            ]
        );
    }

    /// `$29`는 주 대사 실행 수명이 아니라 NMI 복원용 PRG 뱅크다. 실제 표시 중에도
    /// 다른 값이므로 selector가 그것을 읽으면 준비된 페이지를 영원히 고르지 못한다.
    #[test]
    fn prg_bank_shadow_is_not_used_as_the_dialogue_lifetime() {
        let routine =
            build_chr_selector(0xF4A0, 0xC8, SELECTOR_CHAIN_FALLBACK, PROJECT_DIALOGUE_PAGE)
                .unwrap();

        assert!(!routine.bytes.windows(2).any(|window| window
            == [
                0xA5,
                super::super::super::runtime_bank_contract::PRG_BANK_SHADOW
            ]));
        assert!(routine.bytes.windows(5).any(|window| {
            window
                == [
                    0xAD,
                    REQUEST_STATE as u8,
                    (REQUEST_STATE >> 8) as u8,
                    0xC9,
                    STATE_READY,
                ]
        }));
    }

    /// 일반 대사는 terminal에서 원본 사슬로 돌아가지만, 엔딩 후일담은 바깥 phase
    /// 0x10이 화면을 보존하는 동안 같은 준비된 페이지를 계속 선택한다.
    #[test]
    fn terminal_dialogue_state_is_retained_only_for_the_visible_epilogue_phase() {
        let routine =
            build_chr_selector(0xF4A0, 0xC8, SELECTOR_CHAIN_FALLBACK, PROJECT_DIALOGUE_PAGE)
                .unwrap();

        assert!(routine.bytes.windows(14).any(|window| {
            window
                == [
                    0xAD,
                    DISPATCHER_STATE as u8,
                    (DISPATCHER_STATE >> 8) as u8,
                    0xC9,
                    TERMINAL_STATE,
                    0x90,
                    window[6],
                    0xD0,
                    window[8],
                    0xAD,
                    ENDING_RECORD_PHASE_ADDRESS as u8,
                    (ENDING_RECORD_PHASE_ADDRESS >> 8) as u8,
                    0xC9,
                    ENDING_CHARACTER_EPILOGUE_VISIBLE_PHASE,
                ]
        }));
    }

    /// 표시 페이지 선택은 여기서 FD 하나를 하드코딩하지 않고, live FE 원천을 아는
    /// 공통 투영기에 ready/cold 페이지를 모두 넘긴다.
    #[test]
    fn ready_and_cold_paths_delegate_to_the_pair_projection() {
        let routine =
            build_chr_selector(0xF4A0, 0xC8, SELECTOR_CHAIN_FALLBACK, PROJECT_DIALOGUE_PAGE)
                .unwrap();

        assert_eq!(
            routine
                .bytes
                .windows(5)
                .filter(|window| {
                    window[0] == 0xA9
                        && window[2..]
                            == [
                                0x20,
                                PROJECT_DIALOGUE_PAGE as u8,
                                (PROJECT_DIALOGUE_PAGE >> 8) as u8,
                            ]
                })
                .map(|window| window[1])
                .collect::<Vec<_>>(),
            [CHR_RAM_BANK_VALUE, 0xC8]
        );
    }

    /// RAM 내용은 원본 FD 페이지 0의 복제본 위에 만들어진다. 중앙 원천이 다른
    /// 페이지면 준비 표식만 믿지 않고 기존 selector 사슬로 넘겨야 한다.
    #[test]
    fn the_ready_path_is_guarded_by_the_dialogue_fd_source_page() {
        let routine =
            build_chr_selector(0xF4A0, 0xC8, SELECTOR_CHAIN_FALLBACK, PROJECT_DIALOGUE_PAGE)
                .unwrap();

        assert!(routine.bytes.windows(8).any(|window| {
            window
                == [
                    0xA5,
                    RIGHT_FD_SOURCE_SHADOW,
                    0x05,
                    CHR_SOURCE_HIGH_BITS,
                    0x29,
                    0x1F,
                    0xC9,
                    DIALOGUE_FD_SOURCE_PAGE,
                ]
        }));
    }

    /// 사슬은 누산기에 원천 페이지 값을 싣고 다닌다. 성공 경로가 매퍼 페이지를 쓴 뒤
    /// 그 값을 A에 남기면 호출자가 그것을 자연 페이지 그림자에 저장해 화면이 깨진다.
    #[test]
    fn the_chain_accumulator_survives_both_paths() {
        let routine =
            build_chr_selector(0xF4A0, 0xC8, SELECTOR_CHAIN_FALLBACK, PROJECT_DIALOGUE_PAGE)
                .unwrap();

        assert_eq!(&routine.bytes[..2], [0x08, 0x48]);
        let selected_returns = routine
            .bytes
            .windows(6)
            .filter(|window| {
                *window
                    == [
                        0x20,
                        PROJECT_DIALOGUE_PAGE as u8,
                        (PROJECT_DIALOGUE_PAGE >> 8) as u8,
                        0x68,
                        0x28,
                        0x60,
                    ]
            })
            .count();
        assert_eq!(
            selected_returns, 2,
            "both selected pages must restore A and P after the mapper write"
        );
        assert_eq!(
            &routine.bytes[routine.bytes.len() - 5..],
            [
                0x68,
                0x28,
                0x4C,
                SELECTOR_CHAIN_FALLBACK as u8,
                (SELECTOR_CHAIN_FALLBACK >> 8) as u8,
            ]
        );
    }

    #[test]
    fn cold_request_selection_uses_chr_rom_instead_of_partial_chr_ram() {
        let routine =
            build_chr_selector(0xF4A0, 0xC8, SELECTOR_CHAIN_FALLBACK, PROJECT_DIALOGUE_PAGE)
                .unwrap();

        assert_eq!(
            &routine.bytes[2..7],
            [
                0xAD,
                REQUEST_STATE as u8,
                (REQUEST_STATE >> 8) as u8,
                0xC9,
                STATE_READY,
            ]
        );
        assert_eq!(routine.bytes[7], 0xF0, "ready must enter the guarded path");
        assert_eq!(
            &routine.bytes[9..11],
            [0xC9, STATE_COLD_REQUESTED],
            "the same state load must next admit cold_requested"
        );
        assert!(routine.bytes.windows(5).any(|window| {
            window
                == [
                    0xA9,
                    0xC8,
                    0x20,
                    PROJECT_DIALOGUE_PAGE as u8,
                    (PROJECT_DIALOGUE_PAGE >> 8) as u8,
                ]
        }));
    }

    #[test]
    fn resident_group_overlay_uses_the_same_safe_presentation() {
        let routine =
            build_chr_selector(0xF4A0, 0xC8, SELECTOR_CHAIN_FALLBACK, PROJECT_DIALOGUE_PAGE)
                .unwrap();

        assert!(
            routine
                .bytes
                .windows(3)
                .any(|window| { window == [0xC9, STATE_RESIDENT_GROUP_OVERLAY_REQUESTED, 0xF0] })
        );
        assert!(routine.bytes.windows(5).any(|window| {
            window
                == [
                    0xA9,
                    0xC8,
                    0x20,
                    PROJECT_DIALOGUE_PAGE as u8,
                    (PROJECT_DIALOGUE_PAGE >> 8) as u8,
                ]
        }));
    }

    #[test]
    fn atomic_cold_selector_tail_calls_the_pair_projection() {
        let routine =
            build_cold_request_presentation_selector(0xF5A0, 0xC8, PROJECT_DIALOGUE_PAGE).unwrap();

        assert_eq!(
            routine.bytes,
            [
                0xA9,
                0xC8,
                0x4C,
                PROJECT_DIALOGUE_PAGE as u8,
                (PROJECT_DIALOGUE_PAGE >> 8) as u8,
            ]
        );
    }

    /// 사슬 자리가 이미 바뀌었으면 이 selector가 무엇 앞에 끼어드는지 알 수 없다.
    #[test]
    fn a_changed_chain_site_refuses_installation() {
        let mut bytes = crate::test_support::synthetic_mapper165_rom_bytes(0xFF);
        let chain = crate::test_support::synthetic_fixed_bank_file_offset(SELECTOR_CHAIN_SITE);
        bytes[chain..chain + SELECTOR_CHAIN_CODE.len()].copy_from_slice(&SELECTOR_CHAIN_CODE);
        bytes[chain] = 0xEA;
        let mutated = Rom::parse(bytes).unwrap();

        let error = bind_selector_chain_site(&mutated).unwrap_err();

        assert!(
            error
                .to_string()
                .contains("hands the existing chain control")
        );
    }

    #[test]
    fn a_non_ff_byte_in_the_selector_cave_refuses_installation() {
        let mut bytes = crate::test_support::synthetic_mapper165_rom_bytes(0xFF);
        let cave = crate::test_support::synthetic_fixed_bank_file_offset(SELECTOR_CAVE_ORIGIN);
        bytes[cave] = 0xEA;
        let mutated = Rom::parse(bytes).unwrap();

        let error = bind_selector_cave(&mutated).unwrap_err();

        assert!(error.to_string().contains("is not exact FF"));
    }
}
