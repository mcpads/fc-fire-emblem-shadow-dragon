//! mapper165가 소유하는 오른쪽 CHR 원천 상태를 대사 소비자에 묶는다.
//!
//! `$5B`와 `$5C`는 각각 중앙 `$1000` FD·FE 기록기가 보존하는 원천 페이지다.
//! 직접 CHR 기록기는 의도적으로 이 상태를 바꾸지 않는다. 따라서 `$FA80`·`$FAA0`에
//! 관측 훅을 달아 둘을 갱신하면 일시적인 직접 기록을 중앙 selector 상태로 오염시킨다.
//!
//! 대사 전송은 이 상태를 읽기만 한다. `$52`의 상위 원천 비트를 다시 합친 값을
//! stateless 설정기에 넘기면, CHR RAM을 빌린 프레임이 끝난 뒤 FD와 FE 창을 서로
//! 다른 원천 페이지로 되돌릴 수 있다.

use anyhow::{Context, Result, ensure};

use crate::{
    rom::Rom,
    rp2a03::{Instruction, assemble_at},
    typed_source::decode_rp2a03_sequence,
};

use super::fixed_cfg_cycles::worst_case_fixed_subroutine_cycles;

/// 중앙 `$1000` FD 기록기가 보존하는 원천 페이지다.
pub(in crate::full_translation_install) const RIGHT_FD_SOURCE_SHADOW: u8 = 0x5B;
/// 중앙 `$1000` FE 기록기가 보존하는 원천 페이지다.
pub(in crate::full_translation_install) const RIGHT_FE_SOURCE_SHADOW: u8 = 0x5C;
/// 중앙 기록기가 설정기 호출 직전에 합치는 상위 원천 비트다.
pub(in crate::full_translation_install) const CHR_SOURCE_HIGH_BITS: u8 = 0x52;

/// stateless 오른쪽 FD 설정기다.
pub(in crate::full_translation_install) const RIGHT_FD_HELPER: u16 = 0xFA80;
/// stateless 오른쪽 FE 설정기다.
pub(in crate::full_translation_install) const RIGHT_FE_HELPER: u16 = 0xFAA0;

const RIGHT_FD_HELPER_TARGET: u16 = 0xFEEE;
const RIGHT_FE_HELPER_TARGET: u16 = 0xFF43;
const CENTRAL_RIGHT_FD_WRITER: u16 = 0xC9BE;
const CENTRAL_RIGHT_FE_WRITER: u16 = 0xC9C6;
const CENTRAL_RIGHT_FD_SELECTOR: u16 = 0xFF1D;
const CENTRAL_RIGHT_FE_SELECTOR: u16 = 0xFAB8;

/// 전송 이탈이 부르는 후보 helper의 source-bound 사이클 상한이다. 호출자의 `JSR`
/// 6사이클은 이 값 바깥에서 방출 명령과 함께 센다.
#[derive(Clone, Copy, Debug)]
pub(super) struct ChrSourceStateContract {
    fd_restore_callee_cycles: u32,
    fe_restore_callee_cycles: u32,
}

impl ChrSourceStateContract {
    pub(super) fn restore_callee_cycles(self) -> [(u16, u32); 2] {
        [
            (RIGHT_FD_HELPER, self.fd_restore_callee_cycles),
            (RIGHT_FE_HELPER, self.fe_restore_callee_cycles),
        ]
    }
}

/// 대사 소비자가 기대하는 mapper165 생산자 구조를 후보 ROM에 묶는다.
///
/// helper 두 곳만 확인하면 `$5B/$5C`가 무엇인지 증명되지 않는다. 중앙 기록기가
/// 각 값을 먼저 보존한 뒤 현재 selector 사슬로 넘기는 바이트까지 함께 확인한다.
pub(super) fn bind_chr_source_state(candidate: &Rom) -> Result<ChrSourceStateContract> {
    for (address, expected, role) in [
        (
            RIGHT_FD_HELPER,
            assemble_at(
                RIGHT_FD_HELPER,
                &[Instruction::JmpAbsolute(RIGHT_FD_HELPER_TARGET)],
            )?,
            "stateless right FD CHR helper",
        ),
        (
            RIGHT_FE_HELPER,
            assemble_at(
                RIGHT_FE_HELPER,
                &[Instruction::JmpAbsolute(RIGHT_FE_HELPER_TARGET)],
            )?,
            "stateless right FE CHR helper",
        ),
        (
            CENTRAL_RIGHT_FD_WRITER,
            assemble_at(
                CENTRAL_RIGHT_FD_WRITER,
                &[
                    Instruction::StaZeroPage(RIGHT_FD_SOURCE_SHADOW),
                    Instruction::OraZeroPage(CHR_SOURCE_HIGH_BITS),
                    Instruction::JsrAbsolute(CENTRAL_RIGHT_FD_SELECTOR),
                    Instruction::Rts,
                ],
            )?,
            "central right FD source writer",
        ),
        (
            CENTRAL_RIGHT_FE_WRITER,
            assemble_at(
                CENTRAL_RIGHT_FE_WRITER,
                &[
                    Instruction::StaZeroPage(RIGHT_FE_SOURCE_SHADOW),
                    Instruction::OraZeroPage(CHR_SOURCE_HIGH_BITS),
                    Instruction::JsrAbsolute(CENTRAL_RIGHT_FE_SELECTOR),
                    Instruction::Rts,
                ],
            )?,
            "central right FE source writer",
        ),
    ] {
        let actual = fixed_bytes(candidate, address, expected.len())?;
        ensure!(
            actual == expected,
            "mapper165 {role} at {address:04X} changed"
        );
        decode_rp2a03_sequence(&expected, address, role)?;
    }
    const BATTLE_ACTIVE_RANGE: (u16, u16) = (0xFE90, 0xFEC3);
    const RIGHT_FD_ENTRY_RANGE: (u16, u16) = (RIGHT_FD_HELPER, RIGHT_FD_HELPER + 3);
    const RIGHT_FD_RANGE: (u16, u16) = (0xFEEE, 0xFF2D);
    const RIGHT_FE_ENTRY_RANGE: (u16, u16) = (RIGHT_FE_HELPER, RIGHT_FE_HELPER + 3);
    const RIGHT_FE_RANGE: (u16, u16) = (0xFF43, 0xFF80);
    Ok(ChrSourceStateContract {
        fd_restore_callee_cycles: worst_case_fixed_subroutine_cycles(
            candidate,
            RIGHT_FD_HELPER,
            &[RIGHT_FD_ENTRY_RANGE, BATTLE_ACTIVE_RANGE, RIGHT_FD_RANGE],
        )?,
        fe_restore_callee_cycles: worst_case_fixed_subroutine_cycles(
            candidate,
            RIGHT_FE_HELPER,
            &[RIGHT_FE_ENTRY_RANGE, BATTLE_ACTIVE_RANGE, RIGHT_FE_RANGE],
        )?,
    })
}

fn fixed_bytes(rom: &Rom, address: u16, length: usize) -> Result<&[u8]> {
    let base = rom
        .prg()
        .len()
        .checked_sub(16 * 1024)
        .context("PRG is smaller than one fixed bank")?;
    let offset = base + usize::from(address) - 0xC000;
    rom.prg()
        .get(offset..offset + length)
        .context("CHR source-state contract is outside ROM")
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 후보가 유지하는 중앙 그림자와 stateless helper 구조가 계약의 원천이다.
    #[test]
    fn the_release_candidate_keeps_the_bound_chr_source_state() {
        bind_chr_source_state(&crate::test_support::release_rom()).unwrap();
    }

    /// helper만 그대로여도 중앙 기록기가 다른 상태를 쓰기 시작하면 이 소비자는 그
    /// 값을 현재 페이지로 해석할 수 없다.
    #[test]
    fn a_changed_central_shadow_writer_refuses_installation() {
        let rom = crate::test_support::release_rom();
        let mut bytes = rom.data().to_vec();
        let fixed_base = 16 + rom.prg().len() - 16 * 1024;
        bytes[fixed_base + usize::from(CENTRAL_RIGHT_FD_WRITER) - 0xC000 + 1] = 0x5A;
        let mutated = Rom::parse(bytes).unwrap();

        let error = bind_chr_source_state(&mutated).unwrap_err();

        assert!(error.to_string().contains("central right FD source writer"));
    }

    /// 두 창은 같은 페이지라는 가정 없이 별도 상태와 별도 helper를 가져야 한다.
    #[test]
    fn fd_and_fe_have_distinct_source_state_and_helpers() {
        assert_ne!(RIGHT_FD_SOURCE_SHADOW, RIGHT_FE_SOURCE_SHADOW);
        assert_ne!(RIGHT_FD_HELPER, RIGHT_FE_HELPER);
    }

    /// 상한은 손으로 적은 공용 숫자가 아니라 후보의 서로 다른 두 CFG에서 나온다.
    #[test]
    fn restore_cycle_bounds_come_from_each_candidate_helper_cfg() {
        let contract = bind_chr_source_state(&crate::test_support::release_rom()).unwrap();
        let [(fd_helper, fd_cycles), (fe_helper, fe_cycles)] = contract.restore_callee_cycles();

        assert_eq!(fd_helper, RIGHT_FD_HELPER);
        assert_eq!(fe_helper, RIGHT_FE_HELPER);
        assert!(fd_cycles > 0 && fe_cycles > 0);
    }
}
