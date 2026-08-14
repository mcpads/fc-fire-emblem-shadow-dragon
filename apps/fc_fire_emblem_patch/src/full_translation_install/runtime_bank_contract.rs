//! 실행 코드 페이지를 `$A000`에 잠깐 걸었다 되돌리는 계약을 원본 바이트에 결속한다.
//!
//! 소비자는 NMI 안에서 재료 용기의 마지막 MMC3 페이지를 `$A000`에 걸고 전송 루틴을 부른 뒤 원래대로
//! 돌려놓아야 한다. 되돌릴 값의 출처는 원본이 정한 것이므로 여기서 고르지 않고
//! 확인만 한다.
//!
//! 확인 결과 원본의 뱅크 구조는 이렇다.
//!
//! - `$FA20`은 16 KiB 뱅크 하나를 받아 MMC3 레지스터 6에 `n×2`, 7에 `n×2+1`을 쓴다.
//!   즉 `$8000`과 `$A000`은 항상 짝으로 움직이고, 입력은 `AND #$0F`로 잘리므로
//!   이 도우미로 닿을 수 있는 것은 8 KiB 페이지 0..31뿐이다. 실행 코드가 있는
//!   현재 실행 코드 페이지 `30`은 이 도우미로 못 부른다. 그래서 소비자는 레지스터 7만 직접 쓴다.
//! - `$29`가 현재 16 KiB 뱅크의 그림자다. `$C1FB`가 매 프레임 이 값으로 되돌린다.
//! - `$FA80`·`$FAA0`은 PRG가 아니라 CHR 설정기다. `$C1EC`가 `$5E`·`$5F`로 부른다.
//!   소비자는 PRG만 건드리므로 이 둘과 무관하다.

use anyhow::{Result, ensure};

use crate::{
    mapper165::selector_safety::{self, select_register_instruction},
    rom::Rom,
    rp2a03::{Instruction, assemble_at},
};

/// PRG `$8000` 창을 고르는 MMC3 레지스터 번호다. 소비자는 여기에 읽을 자료를 건다.
pub(super) const PRG_8000_REGISTER: u8 = 6;
/// PRG `$A000` 창을 고르는 MMC3 레지스터 번호다. 소비자는 여기에 실행 코드를 건다.
pub(super) const PRG_A000_REGISTER: u8 = 7;
/// 현재 16 KiB PRG 뱅크의 제로 페이지 그림자다.
pub(super) const PRG_BANK_SHADOW: u8 = 0x29;
/// `$FA20`이 입력에 씌우는 마스크다. 되돌릴 때 같은 계산을 해야 한다.
pub(super) const BANK_INDEX_MASK: u8 = 0x0F;

/// `$FA20`: 16 KiB 뱅크 하나를 레지스터 6·7 짝으로 펼친다. 선택 포트는
/// `$FA58` 공용 writer만 거치며 값 포트는 이어서 직접 쓴다.
fn paired_bank_setter() -> Result<Vec<u8>> {
    assemble_at(
        PAIRED_BANK_SETTER_ADDRESS,
        &[
            Instruction::Php,
            Instruction::Pha,
            Instruction::AndImmediate(BANK_INDEX_MASK),
            Instruction::AslAccumulator,
            Instruction::Pha,
            Instruction::LdaImmediate(PRG_8000_REGISTER),
            select_register_instruction(),
            Instruction::Pla,
            Instruction::StaAbsolute(0x8001),
            Instruction::Pha,
            Instruction::LdaImmediate(PRG_A000_REGISTER),
            select_register_instruction(),
            Instruction::Pla,
            Instruction::OraImmediate(1),
            Instruction::StaAbsolute(0x8001),
            Instruction::Pla,
            Instruction::Plp,
            Instruction::Rts,
        ],
    )
}

/// `$C1FB`: 뱅크를 바꿔 논리를 부르고 `$29`로 되돌린다. `$29`가 그림자라는 근거다.
/// `LDA #$0E JSR $FA20 JSR $8000 LDA $29 JSR $FA20 RTS`
const BANK_SHADOW_RESTORE: [u8; 14] = [
    0xA9, 0x0E, 0x20, 0x20, 0xFA, 0x20, 0x00, 0x80, 0xA5, 0x29, 0x20, 0x20, 0xFA, 0x60,
];

const PAIRED_BANK_SETTER_ADDRESS: u16 = 0xFA20;
const BANK_SHADOW_RESTORE_ADDRESS: u16 = 0xC1FB;

/// 소비자가 확인해야 확정되는 값만 담는다. 레지스터 주소와 마스크는 위 상수가
/// 단일 출처이므로 여기서 다시 나르지 않는다.
#[derive(Debug, Clone, Copy)]
pub(super) struct BankRestoreContract {
    pub(super) prg_8000_register: u8,
    pub(super) prg_a000_register: u8,
    pub(super) prg_bank_shadow: u8,
    /// `$FA20`으로 닿을 수 있는 8 KiB 페이지의 개수다. 실행 코드 페이지가 이보다
    /// 크면 소비자는 레지스터를 직접 써야 한다.
    pub(super) helper_reachable_page_count: u16,
}

/// `candidate`는 매퍼 165로 변환한 누적 이미지다. 원본은 매퍼 10이라 `$8000`·`$8001`이
/// 뱅크 레지스터가 아니므로 여기서 볼 대상이 아니다.
pub(super) fn bind_bank_restore_contract(candidate: &Rom) -> Result<BankRestoreContract> {
    let paired_bank_setter = paired_bank_setter()?;
    ensure!(
        fixed_bytes(
            candidate,
            PAIRED_BANK_SETTER_ADDRESS,
            paired_bank_setter.len()
        )? == paired_bank_setter,
        "the paired PRG bank setter at $FA20 changed"
    );
    ensure!(
        fixed_bytes(
            candidate,
            BANK_SHADOW_RESTORE_ADDRESS,
            BANK_SHADOW_RESTORE.len()
        )? == BANK_SHADOW_RESTORE,
        "the NMI bank shadow restore at $C1FB changed"
    );
    selector_safety::verify_installed_contract(candidate)?;
    Ok(BankRestoreContract {
        prg_8000_register: PRG_8000_REGISTER,
        prg_a000_register: PRG_A000_REGISTER,
        prg_bank_shadow: PRG_BANK_SHADOW,
        helper_reachable_page_count: u16::from(BANK_INDEX_MASK).wrapping_add(1) * 2,
    })
}

fn fixed_bytes(rom: &Rom, address: u16, length: usize) -> Result<&[u8]> {
    let prg = rom.prg();
    let base = prg
        .len()
        .checked_sub(16 * 1024)
        .ok_or_else(|| anyhow::anyhow!("PRG is smaller than one fixed bank"))?;
    let offset = base + usize::from(address) - 0xC000;
    prg.get(offset..offset + length)
        .ok_or_else(|| anyhow::anyhow!("fixed-bank read at {address:04X} is out of range"))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 도우미가 바뀌면 되돌리기 계산이 달라지므로 설치를 막아야 한다.
    #[test]
    fn a_changed_bank_setter_refuses_installation() {
        let mut bytes = crate::test_support::synthetic_mapper165_rom_bytes(0xFF);
        let setter =
            crate::test_support::synthetic_fixed_bank_file_offset(PAIRED_BANK_SETTER_ADDRESS);
        let paired_bank_setter = paired_bank_setter().unwrap();
        bytes[setter..setter + paired_bank_setter.len()].copy_from_slice(&paired_bank_setter);
        let restore =
            crate::test_support::synthetic_fixed_bank_file_offset(BANK_SHADOW_RESTORE_ADDRESS);
        bytes[restore..restore + BANK_SHADOW_RESTORE.len()].copy_from_slice(&BANK_SHADOW_RESTORE);
        bytes[setter] = 0xEA;
        let mutated = Rom::parse(bytes).unwrap();

        let error = bind_bank_restore_contract(&mutated).unwrap_err();

        assert!(error.to_string().contains("$FA20 changed"));
    }
}
