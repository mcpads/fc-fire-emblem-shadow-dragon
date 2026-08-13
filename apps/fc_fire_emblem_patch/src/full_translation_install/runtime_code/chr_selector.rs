//! 합성이 끝난 뒤에만 CHR RAM을 고르는 selector다.
//!
//! 전송이 타일을 올려도 화면이 CHR RAM을 보지 않으면 아무것도 바뀌지 않는다.
//! 탐침에서 `$2007` 쓰기가 버려진 것이 그 증거였다.
//!
//! 자리는 원본 CHR selector 사슬의 `$FF40`이다. 지금 값은 폐기된 표본 selector로
//! 넘기는 `JMP $F990`이고, 그 뒤로 기존 소비자들이 이어진다. selector는 그 앞에
//! 끼어들어 «준비됐으면 CHR RAM, 아니면 하던 대로»만 고른다.
//!
//! 준비되지 않았을 때 CHR RAM을 고르면 아직 올라가지 않은 타일이 화면에 나온다.
//! 그것이 설계의 안전 성질 위반이므로 `ready`가 아닌 모든 값은 기존 사슬로 넘긴다.
//!
//! **사슬은 누산기에 값을 싣고 다닌다.** `$FF1D`가 `PHP PHA`로 시작해 그 값을 페이지
//! 번호로 쓰므로, 끼어드는 쪽이 `LDA`로 누산기를 덮으면 뒤따르는 소비자가 남의 값을
//! 페이지로 읽는다. 실행하면 화면 전체가 깨진다 — 그렇게 확인했다. 그래서 이 selector는
//! 판정하기 전에 누산기와 상태를 밀어 두고 넘기기 전에 되돌린다.

use anyhow::{Context, Result, ensure};

use super::{RuntimeRoutine, next_address, transport::{REQUEST_STATE, STATE_READY}};
use crate::{
    rom::Rom,
    rp2a03::{Instruction, assemble_at},
    typed_source::decode_rp2a03_sequence,
};

/// selector 사슬에서 이 런타임이 가져가는 자리다.
pub(in crate::full_translation_install) const SELECTOR_CHAIN_SITE: u16 = 0xFF40;
/// 그 자리가 지금 넘기는 곳이다. selector는 통과할 때 여기로 그대로 넘긴다.
pub(in crate::full_translation_install) const SELECTOR_CHAIN_FALLBACK: u16 = 0xF990;
/// CHR RAM을 고르는 뱅크 값이다. 매퍼 165는 값 0이 보드의 CHR RAM이고, CHR ROM의
/// 물리 페이지 0은 `encode_chr_page_register`가 1로 인코딩해 이 값과 구분한다.
/// 그러므로 여기서는 그 함수를 쓰지 않는다.
const CHR_RAM_BANK_VALUE: u8 = 0;
/// 매퍼 165가 4 KiB CHR 창 둘에 쓰는 MMC3 레지스터다. 전투 합성이 CHR RAM을 고를 때
/// 쓰는 것과 같다. 하나만 쓰면 나머지 창이 CHR ROM을 계속 본다.
const CHR_BANK_REGISTERS: [u8; 2] = [2, 4];

/// `$FF40`: `JMP $F990`.
const SELECTOR_CHAIN_CODE: [u8; 3] = [
    0x4C,
    SELECTOR_CHAIN_FALLBACK as u8,
    (SELECTOR_CHAIN_FALLBACK >> 8) as u8,
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
        Instruction::LdaAbsolute(REQUEST_STATE),
        Instruction::CmpImmediate(STATE_READY),
    ];
    let not_ready_placeholder = instructions.len();
    instructions.push(Instruction::BneAbsolute(origin));
    instructions.extend([Instruction::Pla, Instruction::Plp]);
    for register in CHR_BANK_REGISTERS {
        instructions.extend([
            Instruction::LdaImmediate(register),
            Instruction::StaAbsolute(bank_select_register),
            Instruction::LdaImmediate(CHR_RAM_BANK_VALUE),
            Instruction::StaAbsolute(bank_value_register),
        ]);
    }
    instructions.push(Instruction::Rts);
    let not_ready = next_address(origin, &instructions)?;
    instructions[not_ready_placeholder] = Instruction::BneAbsolute(not_ready);
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

    /// CHR 창이 둘이므로 레지스터도 둘 다 골라야 한다. 하나만 쓰면 나머지 창이
    /// CHR ROM을 계속 봐서 화면 절반이 원본 글꼴로 남는다.
    ///
    /// MMC3는 «레지스터를 고른 뒤 값을 쓴다». 값만 쓰면 직전에 누가 골라 둔
    /// 레지스터가 바뀌므로 무엇이 망가질지 알 수 없다.
    #[test]
    fn the_ready_path_selects_both_chr_windows_register_first() {
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

        assert_eq!(
            selections,
            CHR_BANK_REGISTERS
                .iter()
                .map(|register| (*register, CHR_RAM_BANK_VALUE))
                .collect::<Vec<_>>()
        );
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

        assert!(error.to_string().contains("hands the existing chain control"));
    }

    /// 원본 사슬 자리는 아직 표본 selector로 넘긴다. 그 사실이 바뀌면 이 계층이
    /// 대체하려는 대상이 달라진 것이다.
    #[test]
    fn the_source_chain_site_is_still_where_the_selector_expects_it() {
        let rom = crate::test_support::release_rom();

        bind_selector_chain_site(&rom).unwrap();
    }
}
