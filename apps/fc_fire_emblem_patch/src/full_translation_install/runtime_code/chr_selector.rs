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
//! 그것이 설계의 안전 성질 위반이므로 `ready`가 아닌 모든 값은 기존 사슬로 넘긴다.
//!
//! 실행에서 맵 타일 자리에 한글이 나온 원인은 CHR RAM의 내용이 아니라 **표시 선택이
//! FD와 FE를 모두 RAM으로 바꾼 것**이었다. mapper165의 레지스터 2는 오른쪽 FD,
//! 레지스터 4는 오른쪽 FE다. 원본 대사 글꼴은 FD 페이지 0에 있고 맵 배경은 FE에
//! 남으므로, 둘을 같은 RAM 페이지로 합치면 같은 타일 코드의 서로 다른 두 패턴을
//! 표현할 수 없다.
//!
//! 전송 중에는 현재 래치와 관계없이 `$2007` 쓰기가 RAM에 닿도록 두 레지스터를 모두
//! 잠시 RAM으로 건다. 전송 이탈은 둘을 원천 상태로 되돌린다. 이 selector가 맡는 것은
//! 그 뒤의 **표시 상태**뿐이다. 준비된 주 대사이며 중앙 FD 원천이 페이지 0일 때
//! 레지스터 2만 RAM으로 바꾸고, 레지스터 4는 방금 복원한 원본 FE 페이지에 둔다.
//!
//! **사슬은 누산기에 값을 싣고 다닌다.** `$FF1D`가 `PHP PHA`로 시작해 그 값을 페이지
//! 번호로 쓰므로, 끼어드는 쪽이 `LDA`로 누산기를 덮으면 뒤따르는 소비자가 남의 값을
//! 페이지로 읽는다. 실행하면 화면 전체가 깨진다 — 그렇게 확인했다. 그래서 이 selector는
//! 판정하기 전에 누산기와 상태를 밀어 두고 넘기기 전에 되돌린다.

use anyhow::{Context, Result, ensure};

use super::super::runtime_bank_contract::{BANK_INDEX_MASK, PRG_BANK_SHADOW};
use super::{
    RuntimeRoutine,
    chr_source_state::{CHR_SOURCE_HIGH_BITS, RIGHT_FD_SOURCE_SHADOW},
    next_address,
    transport::{REQUEST_STATE, STATE_READY},
};
use crate::{
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
const MAIN_DIALOGUE_BANK: u8 = 0x0A;
/// CHR RAM을 고르는 뱅크 값이다. 매퍼 165는 값 0이 보드의 CHR RAM이고, CHR ROM의
/// 물리 페이지 0은 `encode_chr_page_register`가 1로 인코딩해 이 값과 구분한다.
/// 그러므로 여기서는 그 함수를 쓰지 않는다.
const CHR_RAM_BANK_VALUE: u8 = 0;
/// 주 대사 글꼴이 있는 원천 FD 페이지다. 다른 원천 페이지에서 같은 RAM을 보여 주면
/// 그 화면의 FD 배경을 보존했다는 증명이 없으므로 기존 selector 사슬로 넘긴다.
const DIALOGUE_SOURCE_FD_PAGE: u8 = 0;
/// 매퍼 165의 오른쪽 FD 레지스터다. FE 레지스터 4는 원본 배경을 계속 본다.
const RIGHT_FD_CHR_REGISTER: u8 = 2;

/// `$FF40`: `JMP $F990`.
const SELECTOR_CHAIN_CODE: [u8; 3] = [
    0x4C,
    SELECTOR_CHAIN_SOURCE_TARGET as u8,
    (SELECTOR_CHAIN_SOURCE_TARGET >> 8) as u8,
];

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

/// `$FF40`에 쓸 세 바이트다.
pub(super) fn selector_hook_bytes(selector: u16) -> [u8; 3] {
    [0x4C, selector as u8, (selector >> 8) as u8]
}

pub(super) fn build_chr_selector(
    origin: u16,
    bank_select_register: u16,
    bank_value_register: u16,
) -> Result<RuntimeRoutine> {
    let mut instructions = vec![
        // 사슬이 나르는 누산기와 상태를 밀어 둔다.
        Instruction::Php,
        Instruction::Pha,
        // 예약 RAM은 주 대사 수명 밖에서 덮여도 된다. 현재 16 KiB 뱅크가 주 대사일
        // 때만 그 안의 요청 바이트를 읽어, 비활성 화면에서는 다섯 바이트를 전부
        // 무시한다.
        Instruction::LdaZeroPage(PRG_BANK_SHADOW),
        Instruction::AndImmediate(BANK_INDEX_MASK),
        Instruction::CmpImmediate(MAIN_DIALOGUE_BANK),
    ];
    let inactive_lifetime_placeholder = instructions.len();
    instructions.push(Instruction::BneAbsolute(origin));
    instructions.extend([
        Instruction::LdaAbsolute(REQUEST_STATE),
        Instruction::CmpImmediate(STATE_READY),
    ]);
    let not_ready_placeholder = instructions.len();
    instructions.push(Instruction::BneAbsolute(origin));
    instructions.extend([
        Instruction::LdaZeroPage(RIGHT_FD_SOURCE_SHADOW),
        Instruction::OraZeroPage(CHR_SOURCE_HIGH_BITS),
        Instruction::AndImmediate(0x1F),
        Instruction::CmpImmediate(DIALOGUE_SOURCE_FD_PAGE),
    ]);
    let wrong_fd_source_placeholder = instructions.len();
    instructions.push(Instruction::BneAbsolute(origin));
    instructions.extend([
        Instruction::Pla,
        Instruction::Plp,
        Instruction::LdaImmediate(RIGHT_FD_CHR_REGISTER),
        Instruction::StaAbsolute(bank_select_register),
        Instruction::LdaImmediate(CHR_RAM_BANK_VALUE),
        Instruction::StaAbsolute(bank_value_register),
        Instruction::Rts,
    ]);
    let not_ready = next_address(origin, &instructions)?;
    instructions[inactive_lifetime_placeholder] = Instruction::BneAbsolute(not_ready);
    instructions[not_ready_placeholder] = Instruction::BneAbsolute(not_ready);
    instructions[wrong_fd_source_placeholder] = Instruction::BneAbsolute(not_ready);
    instructions.extend([
        Instruction::Pla,
        Instruction::Plp,
        Instruction::JmpAbsolute(SELECTOR_CHAIN_FALLBACK),
    ]);

    Ok(RuntimeRoutine {
        role: "dialogue CHR RAM selector",
        address: origin,
        bytes: assemble_at(origin, &instructions)?,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 준비되지 않았는데 CHR RAM을 고르면 아직 올라가지 않은 타일이 화면에 나온다.
    /// 그러므로 `ready`가 아닌 모든 값은 기존 사슬로 넘겨야 한다.
    #[test]
    fn an_unready_state_hands_the_existing_chain_control() {
        let routine = build_chr_selector(0xF4A0, 0x8000, 0x8001).unwrap();

        assert_eq!(
            &routine.bytes[routine.bytes.len() - 3..],
            [
                0x4C,
                SELECTOR_CHAIN_FALLBACK as u8,
                (SELECTOR_CHAIN_FALLBACK >> 8) as u8
            ]
        );
    }

    /// 비활성 화면은 예약 RAM이 덮였을 수 있으므로 요청 바이트 자체를 읽으면 안 된다.
    /// 주 대사 뱅크 판정이 먼저 실패 경로로 나가야 우연한 `ready`를 신뢰하지 않는다.
    #[test]
    fn an_inactive_lifetime_never_reaches_the_request_read() {
        let routine = build_chr_selector(0xF4A0, 0x8000, 0x8001).unwrap();
        let bank_read = [0xA5, PRG_BANK_SHADOW];
        let request_read = [0xAD, REQUEST_STATE as u8, (REQUEST_STATE >> 8) as u8];
        let bank_position = routine
            .bytes
            .windows(bank_read.len())
            .position(|window| window == bank_read)
            .expect("the selector reads the active bank");
        let request_position = routine
            .bytes
            .windows(request_read.len())
            .position(|window| window == request_read)
            .expect("the selector reads the request");

        assert!(bank_position < request_position);
        assert!(
            routine.bytes[bank_position..request_position].contains(&0xD0),
            "the inactive bank has no branch around the request read"
        );
    }

    /// 표시 중에는 FD만 CHR RAM을 보고 FE는 원본 배경을 계속 봐야 한다. 둘 다 RAM으로
    /// 바꾸면 같은 타일 코드의 FD 글꼴과 FE 맵 패턴이 한 페이지에서 충돌한다.
    /// MMC3는 «레지스터를 고른 뒤 값을 쓴다»는 순서도 함께 지킨다.
    #[test]
    fn the_ready_path_selects_only_fd_chr_ram_and_keeps_fe_on_source_rom() {
        let routine = build_chr_selector(0xF4A0, 0x8000, 0x8001).unwrap();

        let mut selections = Vec::new();
        let mut index = 0;
        while index + 9 < routine.bytes.len() {
            if routine.bytes[index] == 0xA9
                && routine.bytes[index + 2..index + 5] == [0x8D, 0x00, 0x80]
                && routine.bytes[index + 5] == 0xA9
                && routine.bytes[index + 7..index + 10] == [0x8D, 0x01, 0x80]
            {
                selections.push((routine.bytes[index + 1], routine.bytes[index + 6]));
                index += 10;
                continue;
            }
            index += 1;
        }

        assert_eq!(selections, [(RIGHT_FD_CHR_REGISTER, CHR_RAM_BANK_VALUE)]);
        assert!(!selections.iter().any(|(register, _)| *register == 4));
    }

    /// RAM 내용은 원본 FD 페이지 0의 복제본 위에 만들어진다. 중앙 원천이 다른
    /// 페이지면 준비 표식만 믿지 않고 기존 selector 사슬로 넘겨야 한다.
    #[test]
    fn the_ready_path_is_guarded_by_the_dialogue_fd_source_page() {
        let routine = build_chr_selector(0xF4A0, 0x8000, 0x8001).unwrap();

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
                    DIALOGUE_SOURCE_FD_PAGE,
                ]
        }));
    }

    /// 사슬은 누산기에 페이지 값을 싣고 다닌다. 끼어드는 쪽이 그것을 덮으면 뒤따르는
    /// 소비자가 남의 값을 페이지로 읽어 화면이 깨진다.
    #[test]
    fn the_chain_accumulator_survives_both_paths() {
        let routine = build_chr_selector(0xF4A0, 0x8000, 0x8001).unwrap();

        // 진입에서 `PHP PHA`, 두 갈래 모두 `PLA PLP`로 되돌린다.
        assert_eq!(&routine.bytes[..2], [0x08, 0x48]);
        let restores = routine
            .bytes
            .windows(2)
            .filter(|window| *window == [0x68, 0x28])
            .count();
        assert_eq!(restores, 2, "both paths must hand the accumulator back");
    }

    /// 사슬 자리가 이미 바뀌었으면 이 selector가 무엇 앞에 끼어드는지 알 수 없다.
    #[test]
    fn a_changed_chain_site_refuses_installation() {
        let rom = crate::test_support::release_rom();
        let mut bytes = rom.data().to_vec();
        let fixed_base = 16 + rom.prg().len() - 16 * 1024;
        bytes[fixed_base + usize::from(SELECTOR_CHAIN_SITE) - 0xC000] = 0xEA;
        let mutated = Rom::parse(bytes).unwrap();

        let error = bind_selector_chain_site(&mutated).unwrap_err();

        assert!(
            error
                .to_string()
                .contains("hands the existing chain control")
        );
    }

    /// 원본 사슬 자리는 아직 표본 selector로 넘긴다. 그 사실이 바뀌면 이 계층이
    /// 대체하려는 대상이 달라진 것이다.
    #[test]
    fn the_source_chain_site_is_still_where_the_selector_expects_it() {
        let rom = crate::test_support::release_rom();

        bind_selector_chain_site(&rom).unwrap();
    }
}
