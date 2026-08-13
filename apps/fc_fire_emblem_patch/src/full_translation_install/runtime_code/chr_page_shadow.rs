//! 원본이 고르는 CHR 페이지를 관측해 기록한다.
//!
//! 소비자가 CHR RAM을 잠깐 빌려 쓰려면 끝나고 원래 페이지로 돌려놓아야 하는데,
//! 원본에는 «지금 걸려 있는 페이지»를 담아 두는 변수가 없다. 호출부 전수 조사로
//! 확인했다 — CHR 설정기 `$FA60`·`$FA80`·`$FAA0`은 모두 호출자가 누산기에 실어 온
//! 값을 그대로 쓰고, 화면이 바뀔 때마다 호출자가 다시 고를 뿐이다.
//!
//! `$5D`·`$5E`·`$5F`는 그 변수가 아니다. 맵이 그려지는 중에도 셋 다 0이었다. 그것은
//! «바꿔 달라»는 요청이고 `$C1EC`가 `$5D != 0`일 때만 쓴다.
//!
//! 그래서 없는 변수를 만든다. `$FA80`은 `JMP $FEEE` 세 바이트라 그 자리를 가져가
//! 페이지를 적어 두고 원래 목적지로 넘긴다. 열세 곳의 호출부가 전부 이 자리를
//! 지나므로 기록은 항상 최신이다.

use anyhow::{Context, Result, ensure};

use super::{RuntimeRoutine, next_address};
use crate::{
    rom::Rom,
    rp2a03::{Instruction, assemble_at},
    typed_source::decode_rp2a03_sequence,
};

/// 관측 지점이다. 원본은 여기서 `JMP $FEEE`를 한다.
pub(in crate::full_translation_install) const CHR_HELPER_SITE: u16 = 0xFA80;
/// 그 자리가 넘기는 곳이다. 관측 뒤 그대로 넘긴다.
const CHR_HELPER_TARGET: u16 = 0xFEEE;
/// 관측한 페이지를 적어 두는 자리다.
///
/// 원본 PPU 블록 큐의 하드 상한 `$07DF`보다 위이고, PRG 전체에서 이 주소를 직접
/// 피연산자로 쓰는 명령이 없다. 대사 예약 `$07F0..$07F8`과 달리 이 값은 대사가
/// 활성이 아닐 때도 살아 있어야 하므로 그 범위 밖에 둔다.
pub(in crate::full_translation_install) const CHR_PAGE_SHADOW: u16 = 0x07EB;
/// 페이지 번호가 쓰는 비트다. `$FEEE`가 `AND #$1F`로 자르는 것과 같다.
const CHR_PAGE_MASK: u8 = 0x1F;

/// `$FA80`: `JMP $FEEE`.
const CHR_HELPER_CODE: [u8; 3] = [
    0x4C,
    CHR_HELPER_TARGET as u8,
    (CHR_HELPER_TARGET >> 8) as u8,
];

/// 관측 지점이 아직 그대로인지 확인한다. 후보만 본다 — 이 도우미는 매퍼 165 변환이
/// 세운 구조물이라 원본 일본어 ROM에는 없다.
pub(super) fn bind_chr_helper_site(candidate: &Rom) -> Result<()> {
    let prg = candidate.prg();
    let base = prg
        .len()
        .checked_sub(16 * 1024)
        .context("PRG is smaller than one fixed bank")?;
    let offset = base + usize::from(CHR_HELPER_SITE) - 0xC000;
    let bytes = prg
        .get(offset..offset + CHR_HELPER_CODE.len())
        .context("CHR helper site is outside ROM")?;
    ensure!(
        bytes == CHR_HELPER_CODE,
        "the CHR helper at {CHR_HELPER_SITE:04X} no longer jumps to {CHR_HELPER_TARGET:04X}"
    );
    decode_rp2a03_sequence(&CHR_HELPER_CODE, CHR_HELPER_SITE, "CHR helper site")?;
    Ok(())
}

/// `$FA80`에 쓸 세 바이트다.
pub(super) fn helper_hook_bytes(observer: u16) -> [u8; 3] {
    [0x4C, observer as u8, (observer >> 8) as u8]
}

/// 페이지를 적어 두고 원래 목적지로 넘긴다.
///
/// 누산기와 상태를 그대로 돌려준다. `$FEEE`는 진입 시점의 상태를 `PHP`로 잡아
/// 이탈에서 되돌리므로, 여기서 상태를 바꿔 두면 호출자가 남의 플래그를 받는다.
pub(super) fn build_chr_page_observer(origin: u16) -> Result<RuntimeRoutine> {
    let instructions = vec![
        Instruction::Php,
        Instruction::Pha,
        Instruction::AndImmediate(CHR_PAGE_MASK),
        Instruction::StaAbsolute(CHR_PAGE_SHADOW),
        Instruction::Pla,
        Instruction::Plp,
        Instruction::JmpAbsolute(CHR_HELPER_TARGET),
    ];
    let _ = next_address(origin, &instructions)?;
    Ok(RuntimeRoutine {
        role: "CHR page observer",
        address: origin,
        bytes: assemble_at(origin, &instructions)?,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 관측은 호출자가 보는 것을 하나도 바꾸지 않아야 한다. 누산기는 페이지 값이고
    /// 상태는 `$FEEE`가 잡아서 되돌리는 값이다.
    #[test]
    fn observing_changes_nothing_the_caller_can_see() {
        let routine = build_chr_page_observer(0xF5A0).unwrap();

        assert_eq!(&routine.bytes[..2], [0x08, 0x48]);
        assert_eq!(
            &routine.bytes[routine.bytes.len() - 5..],
            [
                0x68,
                0x28,
                0x4C,
                CHR_HELPER_TARGET as u8,
                (CHR_HELPER_TARGET >> 8) as u8
            ]
        );
    }

    /// 적어 두는 값은 `$FEEE`가 실제로 쓰는 비트여야 한다. 더 담으면 되돌릴 때
    /// 다른 페이지를 고른다.
    #[test]
    fn the_shadow_holds_the_bits_the_helper_uses() {
        let routine = build_chr_page_observer(0xF5A0).unwrap();
        let mask = routine
            .bytes
            .windows(2)
            .find(|window| window[0] == 0x29)
            .expect("the observer masks the page");

        assert_eq!(mask[1], CHR_PAGE_MASK);
    }

    /// 관측 지점이 바뀌면 어디를 가로채는지 알 수 없다.
    #[test]
    fn a_changed_helper_site_refuses_installation() {
        let rom = crate::test_support::release_rom();
        let mut bytes = rom.data().to_vec();
        let fixed_base = 16 + rom.prg().len() - 16 * 1024;
        bytes[fixed_base + usize::from(CHR_HELPER_SITE) - 0xC000] = 0xEA;
        let mutated = Rom::parse(bytes).unwrap();

        let error = bind_chr_helper_site(&mutated).unwrap_err();

        assert!(error.to_string().contains("no longer jumps"));
    }

    /// 그림자는 대사 예약 밖에 있어야 한다. 예약은 대사가 활성일 때만 살아 있는데
    /// 이 값은 그때가 아니어도 최신이어야 한다.
    #[test]
    fn the_shadow_lives_outside_the_dialogue_reservation() {
        assert!(CHR_PAGE_SHADOW < 0x07F0);
        // 원본 PPU 블록 큐가 닿는 곳보다 위다.
        assert!(CHR_PAGE_SHADOW > 0x07DF);
    }
}
