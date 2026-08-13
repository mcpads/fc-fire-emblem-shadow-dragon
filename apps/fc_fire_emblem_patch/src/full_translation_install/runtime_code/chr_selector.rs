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

use anyhow::{Context, Result, ensure};

use super::{RuntimeRoutine, next_address, transport::{REQUEST_STATE, STATE_READY}};
use crate::{
    mapper165::encode_chr_page_register,
    rom::Rom,
    rp2a03::{Instruction, assemble_at},
    typed_source::decode_rp2a03_sequence,
};

/// selector 사슬에서 이 런타임이 가져가는 자리다.
pub(in crate::full_translation_install) const SELECTOR_CHAIN_SITE: u16 = 0xFF40;
/// 그 자리가 지금 넘기는 곳이다. selector는 통과할 때 여기로 그대로 넘긴다.
pub(in crate::full_translation_install) const SELECTOR_CHAIN_FALLBACK: u16 = 0xF990;
/// CHR RAM을 고르는 물리 페이지 번호다. 매퍼 165는 값 0이 보드의 CHR RAM이다.
const CHR_RAM_PHYSICAL_PAGE: u8 = 0;

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

pub(super) fn build_chr_selector(origin: u16, chr_bank_register: u16) -> Result<RuntimeRoutine> {
    let mut instructions = vec![
        Instruction::LdaAbsolute(REQUEST_STATE),
        Instruction::CmpImmediate(STATE_READY),
    ];
    let not_ready_placeholder = instructions.len();
    instructions.push(Instruction::BneAbsolute(origin));
    instructions.extend([
        Instruction::LdaImmediate(encode_chr_page_register(CHR_RAM_PHYSICAL_PAGE)?),
        Instruction::StaAbsolute(chr_bank_register),
        Instruction::Rts,
    ]);
    let not_ready = next_address(origin, &instructions)?;
    instructions[not_ready_placeholder] = Instruction::BneAbsolute(not_ready);
    instructions.push(Instruction::JmpAbsolute(SELECTOR_CHAIN_FALLBACK));

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
        let routine = build_chr_selector(0xF4A0, 0x8001).unwrap();

        assert_eq!(
            &routine.bytes[routine.bytes.len() - 3..],
            [
                0x4C,
                SELECTOR_CHAIN_FALLBACK as u8,
                (SELECTOR_CHAIN_FALLBACK >> 8) as u8
            ]
        );
    }

    /// CHR RAM을 고르는 것은 매퍼 165에서 레지스터 값 0이다. 다른 값이면 CHR ROM의
    /// 어느 페이지를 골라 원본 글꼴이 그대로 나온다.
    #[test]
    fn the_ready_path_selects_the_boards_chr_ram() {
        let routine = build_chr_selector(0xF4A0, 0x8001).unwrap();
        let load_immediate = routine
            .bytes
            .windows(2)
            .position(|window| window[0] == 0xA9)
            .expect("the selector loads a bank value");

        assert_eq!(
            routine.bytes[load_immediate + 1],
            encode_chr_page_register(CHR_RAM_PHYSICAL_PAGE).unwrap()
        );
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
