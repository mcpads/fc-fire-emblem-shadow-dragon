//! 실행 코드 페이지를 `$A000`에 잠깐 걸었다 되돌리는 계약을 원본 바이트에 결속한다.
//!
//! 소비자는 NMI 안에서 페이지 `2E`를 `$A000`에 걸고 전송 루틴을 부른 뒤 원래대로
//! 돌려놓아야 한다. 되돌릴 값의 출처는 원본이 정한 것이므로 여기서 고르지 않고
//! 확인만 한다.
//!
//! 확인 결과 원본의 뱅크 구조는 이렇다.
//!
//! - `$FA20`은 16 KiB 뱅크 하나를 받아 MMC3 레지스터 6에 `n×2`, 7에 `n×2+1`을 쓴다.
//!   즉 `$8000`과 `$A000`은 항상 짝으로 움직이고, 입력은 `AND #$0F`로 잘리므로
//!   이 도우미로 닿을 수 있는 것은 8 KiB 페이지 0..31뿐이다. 실행 코드가 있는
//!   페이지 `2E`는 이 도우미로 못 부른다. 그래서 소비자는 레지스터 7만 직접 쓴다.
//! - `$29`가 현재 16 KiB 뱅크의 그림자다. `$C1FB`가 매 프레임 이 값으로 되돌린다.
//! - `$FA80`·`$FAA0`은 PRG가 아니라 CHR 설정기다. `$C1EC`가 `$5E`·`$5F`로 부른다.
//!   소비자는 PRG만 건드리므로 이 둘과 무관하다.

use anyhow::{Result, ensure};

use crate::rom::Rom;

/// PRG `$8000` 창을 고르는 MMC3 레지스터 번호다. 소비자는 여기에 읽을 자료를 건다.
pub(super) const PRG_8000_REGISTER: u8 = 6;
/// PRG `$A000` 창을 고르는 MMC3 레지스터 번호다. 소비자는 여기에 실행 코드를 건다.
pub(super) const PRG_A000_REGISTER: u8 = 7;
/// 현재 16 KiB PRG 뱅크의 제로 페이지 그림자다.
pub(super) const PRG_BANK_SHADOW: u8 = 0x29;
/// `$FA20`이 입력에 씌우는 마스크다. 되돌릴 때 같은 계산을 해야 한다.
pub(super) const BANK_INDEX_MASK: u8 = 0x0F;

/// `$FA20`: 16 KiB 뱅크 하나를 레지스터 6·7 짝으로 펼친다.
/// `PHP PHA AND#$0F ASL PHA LDA#6 STA$8000 PLA STA$8001 PHA LDA#7 STA$8000
///  PLA ORA#$01 STA$8001 PLA PLP RTS`
const PAIRED_BANK_SETTER: [u8; 30] = [
    0x08, 0x48, 0x29, 0x0F, 0x0A, 0x48, 0xA9, 0x06, 0x8D, 0x00, 0x80, 0x68, 0x8D, 0x01, 0x80, 0x48,
    0xA9, 0x07, 0x8D, 0x00, 0x80, 0x68, 0x09, 0x01, 0x8D, 0x01, 0x80, 0x68, 0x28, 0x60,
];

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
    ensure!(
        fixed_bytes(candidate, PAIRED_BANK_SETTER_ADDRESS, PAIRED_BANK_SETTER.len())?
            == PAIRED_BANK_SETTER,
        "the paired PRG bank setter at $FA20 changed"
    );
    ensure!(
        fixed_bytes(candidate, BANK_SHADOW_RESTORE_ADDRESS, BANK_SHADOW_RESTORE.len())?
            == BANK_SHADOW_RESTORE,
        "the NMI bank shadow restore at $C1FB changed"
    );
    Ok(BankRestoreContract {
        prg_8000_register: PRG_8000_REGISTER,
        prg_a000_register: PRG_A000_REGISTER,
        prg_bank_shadow: PRG_BANK_SHADOW,
        helper_reachable_page_count: u16::from(BANK_INDEX_MASK) .wrapping_add(1) * 2,
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

    /// 되돌릴 뱅크 값의 출처가 원본에 있어야 소비자가 뱅크를 빌려 쓸 수 있다.
    #[test]
    fn the_nmi_restores_its_prg_bank_from_a_zero_page_shadow() {
        let rom = crate::test_support::release_rom();

        let contract = bind_bank_restore_contract(&rom).unwrap();

        assert_eq!(contract.prg_bank_shadow, BANK_SHADOW_RESTORE[9]);
        assert_eq!(contract.prg_a000_register, PAIRED_BANK_SETTER[17]);
        assert_eq!(contract.prg_8000_register, PAIRED_BANK_SETTER[7]);
    }

    /// 원본 도우미로는 실행 코드 페이지에 닿지 못한다. 소비자가 레지스터를 직접
    /// 쓰는 이유가 이것이므로, 도우미의 도달 범위가 넓어지면 그 선택을 다시 본다.
    #[test]
    fn the_source_helper_cannot_reach_the_runtime_code_page() {
        let rom = crate::test_support::release_rom();

        let contract = bind_bank_restore_contract(&rom).unwrap();

        assert!(contract.helper_reachable_page_count <= 0x2E);
    }

    /// 도우미가 바뀌면 되돌리기 계산이 달라지므로 설치를 막아야 한다.
    #[test]
    fn a_changed_bank_setter_refuses_installation() {
        let rom = crate::test_support::release_rom();
        let mut bytes = rom.data().to_vec();
        let fixed_base = 16 + rom.prg().len() - 16 * 1024;
        bytes[fixed_base + usize::from(PAIRED_BANK_SETTER_ADDRESS) - 0xC000] = 0xEA;
        let mutated = Rom::parse(bytes).unwrap();

        let error = bind_bank_restore_contract(&mutated).unwrap_err();

        assert!(error.to_string().contains("$FA20 changed"));
    }
}
