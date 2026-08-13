//! 조용한 프레임 게이트가 보는 표시 넷과, 소비자가 밀어내는 호출을 결속한다.
//!
//! 게이트의 전제는 «표시가 전부 0이면 원본은 이 프레임 vblank에 PPU 자료를 쓰지
//! 않는다»이다. 그 전제를 지키는 것은 아래 세 갈래의 첫 분기이므로, 분기가 바뀌면
//! 게이트가 무효가 된다. 의사결정 64번을 따른다.
//!
//! 이 전제를 실측으로도 확인해 두었다. 렌더링이 켜진 706프레임 표본에서 표시가
//! 전부 0인 프레임의 `$2007` 쓰기는 0건이었다.

use anyhow::{Result, ensure};

use crate::{rom::Rom, typed_source::decode_rp2a03_sequence};

/// 소비자가 들어갈 자리다. 원본은 여기서 `JSR $C3A5`를 한다.
pub(super) const CONSUMER_HOOK: u16 = 0xC179;
/// 소비자가 밀어내고 자신이 다시 넘겨줘야 하는 호출이다.
pub(super) const DISPLACED_CALL: u16 = 0xC3A5;
/// 원본의 vblank PPU 자료 작업을 여는 대기열 표시들이다.
pub(super) const QUEUE_FLAGS: [u8; 4] = [0x21, 0x22, 0x89, 0x8A];
/// `$2000` 그림자다. 소비자는 순차 증가를 강제하면서 이 값을 함께 고친다.
pub(super) const PPU_CONTROL_SHADOW: u8 = 0xCD;

/// `$C179`: `JSR $C3A5`.
const HOOK_SITE: [u8; 3] = [0x20, 0xA5, 0xC3];
/// `$C3A5`: `LDA $21; BEQ $C3BE`.
const BLOCK_INTERPRETER_GATE: [u8; 4] = [0xA5, 0x21, 0xF0, 0x15];
/// `$C296`: `LDY $22; BEQ $C295`. 뒤이은 `BMI $C2CC`는 부호 갈래라 게이트가 아니다.
const PALETTE_QUEUE_GATE: [u8; 4] = [0xA4, 0x22, 0xF0, 0xFB];
/// `$D4AD`: `LDA $89; BNE $D4B6; LDA $8A; BNE $D4CE; RTS`.
const ROW_UPLOAD_GATE: [u8; 9] = [0xA5, 0x89, 0xD0, 0x05, 0xA5, 0x8A, 0xD0, 0x19, 0x60];
/// `$C733`: `LDA $CD; STA $2000; LDA $CC; STA $2001; RTS`.
/// 소비자가 쓰는 증가 비트의 그림자가 `$CD`라는 근거다.
const CONTROL_RESTORE: [u8; 11] = [
    0xA5, 0xCD, 0x8D, 0x00, 0x20, 0xA5, 0xCC, 0x8D, 0x01, 0x20, 0x60,
];

const BLOCK_INTERPRETER_ADDRESS: u16 = 0xC3A5;
const PALETTE_QUEUE_ADDRESS: u16 = 0xC296;
const ROW_UPLOAD_ADDRESS: u16 = 0xD4AD;
const CONTROL_RESTORE_ADDRESS: u16 = 0xC733;

#[derive(Debug, Clone, Copy)]
pub(super) struct QuietFrameGateContract {
    /// 표시가 여는 PPU 자료 갈래의 개수다.
    pub(super) gated_branch_count: usize,
}

pub(super) fn bind_quiet_frame_gate(
    source: &Rom,
    candidate: &Rom,
) -> Result<QuietFrameGateContract> {
    for rom in [source, candidate] {
        ensure!(
            fixed_bytes(rom, CONSUMER_HOOK, HOOK_SITE.len())? == HOOK_SITE,
            "the dialogue consumer hook site at $C179 no longer calls $C3A5"
        );
    }
    let gates: [(&str, u16, &[u8]); 3] = [
        (
            "block interpreter",
            BLOCK_INTERPRETER_ADDRESS,
            &BLOCK_INTERPRETER_GATE,
        ),
        ("palette queue", PALETTE_QUEUE_ADDRESS, &PALETTE_QUEUE_GATE),
        ("row upload", ROW_UPLOAD_ADDRESS, &ROW_UPLOAD_GATE),
    ];
    for (role, address, expected) in gates {
        ensure!(
            fixed_bytes(candidate, address, expected.len())? == expected,
            "the {role} gate at {address:04X} changed; the quiet-frame precondition is void"
        );
    }
    // 게이트 바이트 자체를 디코드해 형식이 여전히 «표시 적재 후 분기»인지 본다.
    decode_rp2a03_sequence(
        &BLOCK_INTERPRETER_GATE,
        BLOCK_INTERPRETER_ADDRESS,
        "block interpreter gate",
    )?;
    decode_rp2a03_sequence(&ROW_UPLOAD_GATE, ROW_UPLOAD_ADDRESS, "row upload gate")?;
    ensure!(
        fixed_bytes(candidate, CONTROL_RESTORE_ADDRESS, CONTROL_RESTORE.len())? == CONTROL_RESTORE,
        "the PPU control restore at $C733 changed; the increment shadow is unproven"
    );
    decode_rp2a03_sequence(
        &CONTROL_RESTORE,
        CONTROL_RESTORE_ADDRESS,
        "PPU control restore",
    )?;
    Ok(QuietFrameGateContract {
        gated_branch_count: gates.len(),
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

    /// 게이트가 보는 표시는 각각 원본의 PPU 자료 갈래를 열고 닫는다. 그 대응이
    /// 깨지면 «조용한 프레임»의 뜻이 달라져 소비자가 남의 vblank를 쓴다.
    #[test]
    fn every_queue_flag_still_guards_a_ppu_data_branch() {
        let rom = crate::test_support::release_rom();

        let contract = bind_quiet_frame_gate(&rom, &rom).unwrap();

        assert_eq!(contract.gated_branch_count, 3);
        assert_eq!(QUEUE_FLAGS.len(), 4);
        for flag in [
            BLOCK_INTERPRETER_GATE[1],
            PALETTE_QUEUE_GATE[1],
            ROW_UPLOAD_GATE[1],
            ROW_UPLOAD_GATE[5],
        ] {
            assert!(
                QUEUE_FLAGS.contains(&flag),
                "flag {flag:02X} guards a branch but the gate does not read it"
            );
        }
    }

    /// 훅 자리가 다른 호출로 바뀌면 소비자가 밀어낼 대상이 사라진다.
    #[test]
    fn a_changed_hook_site_refuses_installation() {
        let rom = crate::test_support::release_rom();
        let mutated = with_fixed_byte_replaced(&rom, CONSUMER_HOOK, 0xEA);

        let error = bind_quiet_frame_gate(&mutated, &mutated).unwrap_err();

        assert!(error.to_string().contains("no longer calls $C3A5"));
    }

    /// 갈래의 첫 분기가 바뀌면 게이트가 지키던 전제가 사라지므로 설치를 막는다.
    #[test]
    fn a_changed_data_branch_voids_the_quiet_frame_precondition() {
        let rom = crate::test_support::release_rom();
        let mutated = with_fixed_byte_replaced(&rom, ROW_UPLOAD_ADDRESS, 0xEA);

        let error = bind_quiet_frame_gate(&rom, &mutated).unwrap_err();

        assert!(error.to_string().contains("precondition is void"));
    }

    fn with_fixed_byte_replaced(rom: &Rom, address: u16, value: u8) -> Rom {
        let mut bytes = rom.data().to_vec();
        let fixed_base = 16 + rom.prg().len() - 16 * 1024;
        bytes[fixed_base + usize::from(address) - 0xC000] = value;
        Rom::parse(bytes).expect("mutated image still parses")
    }
}
