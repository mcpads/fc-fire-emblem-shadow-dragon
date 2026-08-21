//! 동기 대사 합성이 렌더링을 끈 뒤 제어를 돌려줄 원본 NMI 복원 경계를 결속한다.
//!
//! 합성기는 PPUMASK를 0으로 둔 채 다음 vblank에서 NMI만 되살린다. 이어지는 원본
//! NMI가 `$C733`에서 PPUCTRL/PPUMASK shadow를 쓰고 `$C36A`에서 PPU 주소 latch와
//! scroll을 복원해야 다음 가시 프레임이 완전하다. 둘 중 하나라도 바뀌면 renderer를
//! 다시 켜는 책임이 사라지므로 설치를 거부한다.

use anyhow::{Result, ensure};

use crate::{rom::Rom, typed_source::decode_rp2a03_sequence};

/// `$2000` 그림자다. 요청 발행기와 합성기가 NMI enable/increment를 제어한다.
pub(super) const PPU_CONTROL_SHADOW: u8 = 0xCD;
/// `$2001` 그림자다. 합성기는 이 값은 보존하고 하드웨어 mask만 0으로 만든다.
pub(super) const PPU_MASK_SHADOW: u8 = 0xCC;

pub(super) const CONTROL_RESTORE_ADDRESS: u16 = 0xC733;
const SCROLL_RESTORE_ADDRESS: u16 = 0xC36A;

/// `LDA $CD; STA $2000; LDA $CC; STA $2001; RTS`.
const CONTROL_RESTORE: [u8; 11] = [
    0xA5,
    PPU_CONTROL_SHADOW,
    0x8D,
    0x00,
    0x20,
    0xA5,
    PPU_MASK_SHADOW,
    0x8D,
    0x01,
    0x20,
    0x60,
];
/// `LDA $2002; LDA $CB; STA $2005; LDA $CA; STA $2005; RTS`.
const SCROLL_RESTORE: [u8; 14] = [
    0xAD, 0x02, 0x20, 0xA5, 0xCB, 0x8D, 0x05, 0x20, 0xA5, 0xCA, 0x8D, 0x05, 0x20, 0x60,
];

#[derive(Debug, Clone, Copy)]
pub(super) struct SynchronousComposerResumeContract {
    pub(super) restore_site_count: usize,
}

pub(super) fn bind_synchronous_composer_resume(
    source: &Rom,
    candidate: &Rom,
) -> Result<SynchronousComposerResumeContract> {
    for (role, address, expected) in [
        (
            "PPU control/mask restore",
            CONTROL_RESTORE_ADDRESS,
            CONTROL_RESTORE.as_slice(),
        ),
        (
            "PPU scroll restore",
            SCROLL_RESTORE_ADDRESS,
            SCROLL_RESTORE.as_slice(),
        ),
    ] {
        for (image_role, rom) in [("source", source), ("candidate", candidate)] {
            ensure!(
                fixed_bytes(rom, address, expected.len())? == expected,
                "the {image_role} {role} at ${address:04X} changed; synchronous dialogue composition cannot resume rendering"
            );
        }
        decode_rp2a03_sequence(expected, address, role)?;
    }

    Ok(SynchronousComposerResumeContract {
        restore_site_count: 2,
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

    #[test]
    fn both_render_resume_sites_are_required() {
        let source = resume_rom();
        let candidate = resume_rom();
        assert_eq!(
            bind_synchronous_composer_resume(&source, &candidate)
                .unwrap()
                .restore_site_count,
            2
        );

        for address in [CONTROL_RESTORE_ADDRESS, SCROLL_RESTORE_ADDRESS] {
            let mut bytes = candidate.data().to_vec();
            let offset = crate::test_support::synthetic_fixed_bank_file_offset(address);
            bytes[offset] ^= 0x01;
            let mutated = Rom::parse(bytes).unwrap();
            assert!(
                bind_synchronous_composer_resume(&source, &mutated)
                    .unwrap_err()
                    .to_string()
                    .contains("cannot resume rendering")
            );
        }
    }

    fn resume_rom() -> Rom {
        let mut bytes = crate::test_support::synthetic_mapper165_rom_bytes(0xFF);
        for (address, expected) in [
            (CONTROL_RESTORE_ADDRESS, CONTROL_RESTORE.as_slice()),
            (SCROLL_RESTORE_ADDRESS, SCROLL_RESTORE.as_slice()),
        ] {
            let offset = crate::test_support::synthetic_fixed_bank_file_offset(address);
            bytes[offset..offset + expected.len()].copy_from_slice(expected);
        }
        Rom::parse(bytes).unwrap()
    }
}
