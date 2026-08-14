//! 조용한 프레임 게이트가 보는 vblank busy 상태와, 소비자가 밀어내는 호출을 결속한다.
//!
//! 게이트의 전제는 «표시가 전부 0이면 원본은 이 프레임 vblank에 PPU 자료를 쓰지
//! 않는다»이다. 그 전제를 지키는 것은 아래 세 갈래의 첫 분기이므로, 분기가 바뀌면
//! 게이트가 무효가 된다. 의사결정 64번을 따른다.
//!
//! 이 전제를 실측으로도 확인해 두었다. 렌더링이 켜진 706프레임 표본에서 표시가
//! 전부 0인 프레임의 `$2007` 쓰기는 0건이었다.

use anyhow::{Result, ensure};

use crate::{
    rom::Rom,
    rp2a03::{Instruction, assemble_at},
    typed_source::decode_rp2a03_sequence,
};

/// 소비자가 들어갈 자리다. 원본과 mapper165 후보 모두 첫째 호출 `$C3A5`를 보존한다.
pub(super) const CONSUMER_HOOK: u16 = 0xC179;
/// 소비자가 밀어내고 자신이 다시 불러야 하는 원본 호출이다.
pub(super) const DISPLACED_CALL: u16 = 0xC3A5;
/// 원본의 vblank PPU 자료 작업 또는 mapper CHR 복원을 여는 busy 상태들이다.
pub(super) const VBLANK_BUSY_FLAGS: [u8; 5] = [0x21, 0x22, 0x89, 0x8A, 0x5D];
/// `$2000` 그림자다. 소비자는 순차 증가를 강제하면서 이 값을 함께 고친다.
pub(super) const PPU_CONTROL_SHADOW: u8 = 0xCD;

/// 원본과 mapper165 후보의 `$C179`: `JSR $C3A5`.
const HOOK_SITE: [u8; 3] = [0x20, 0xA5, 0xC3];
/// `$C3A5`: `LDA $21; BEQ $C3BE`.
const BLOCK_INTERPRETER_GATE: [u8; 4] = [0xA5, 0x21, 0xF0, 0x15];
/// `$C296`: `LDY $22; BEQ $C295`. 뒤이은 `BMI $C2CC`는 부호 갈래라 게이트가 아니다.
const PALETTE_QUEUE_GATE: [u8; 4] = [0xA4, 0x22, 0xF0, 0xFB];
/// `$D4AD`: `LDA $89; BNE $D4B6; LDA $8A; BNE $D4CE; RTS`.
const ROW_UPLOAD_GATE: [u8; 9] = [0xA5, 0x89, 0xD0, 0x05, 0xA5, 0x8A, 0xD0, 0x19, 0x60];
/// `$C1EC`: `$5D != 0`이면 `$5E/$5F` CHR 원천을 복원한다. mapper165 후보에서는
/// 같은 두 write 자리가 `$FA80/$FAA0` helper 호출로 바뀐다.
const CHR_RESTORE_GATE_ADDRESS: u16 = 0xC1EC;
const CHR_RESTORE_SKIP_ADDRESS: u16 = 0xC1FA;
const CANDIDATE_RIGHT_FD_HELPER: u16 = 0xFA80;
const CANDIDATE_RIGHT_FE_HELPER: u16 = 0xFAA0;
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
    ensure!(
        fixed_bytes(source, CONSUMER_HOOK, HOOK_SITE.len())? == HOOK_SITE,
        "the source dialogue consumer hook site at $C179 no longer calls $C3A5"
    );
    ensure!(
        fixed_bytes(candidate, CONSUMER_HOOK, HOOK_SITE.len())? == HOOK_SITE,
        "the mapper165 dialogue consumer hook site at $C179 no longer calls $C3A5"
    );
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
    bind_chr_restore_gate(source, candidate)?;
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
        gated_branch_count: gates.len() + 1,
    })
}

fn bind_chr_restore_gate(source: &Rom, candidate: &Rom) -> Result<()> {
    let source_expected = assemble_at(
        CHR_RESTORE_GATE_ADDRESS,
        &[
            Instruction::LdaZeroPage(VBLANK_BUSY_FLAGS[4]),
            Instruction::BeqAbsolute(CHR_RESTORE_SKIP_ADDRESS),
            Instruction::LdaZeroPage(0x5E),
            Instruction::StaAbsolute(0xD000),
            Instruction::LdaZeroPage(0x5F),
            Instruction::StaAbsolute(0xE000),
            Instruction::Rts,
        ],
    )?;
    ensure!(
        fixed_bytes(source, CHR_RESTORE_GATE_ADDRESS, source_expected.len())? == source_expected,
        "the source CHR-restore gate at C1EC changed; the vblank busy precondition is void"
    );

    let candidate_expected = assemble_at(
        CHR_RESTORE_GATE_ADDRESS,
        &[
            Instruction::LdaZeroPage(VBLANK_BUSY_FLAGS[4]),
            Instruction::BeqAbsolute(CHR_RESTORE_SKIP_ADDRESS),
            Instruction::LdaZeroPage(0x5E),
            Instruction::JsrAbsolute(CANDIDATE_RIGHT_FD_HELPER),
            Instruction::LdaZeroPage(0x5F),
            Instruction::JsrAbsolute(CANDIDATE_RIGHT_FE_HELPER),
            Instruction::Rts,
        ],
    )?;
    ensure!(
        fixed_bytes(
            candidate,
            CHR_RESTORE_GATE_ADDRESS,
            candidate_expected.len(),
        )? == candidate_expected,
        "the mapper165 CHR-restore gate at C1EC changed; the vblank busy precondition is void"
    );
    Ok(())
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

    /// 훅 자리가 다른 호출로 바뀌면 소비자가 밀어낼 대상이 사라진다.
    #[test]
    fn a_changed_hook_site_refuses_installation() {
        let (source, candidate) = quiet_frame_roms();
        let mutated = with_fixed_byte_replaced(&candidate, CONSUMER_HOOK, 0xEA);

        let error = bind_quiet_frame_gate(&source, &mutated).unwrap_err();

        assert!(error.to_string().contains("no longer calls $C3A5"));
    }

    /// 갈래의 첫 분기가 바뀌면 게이트가 지키던 전제가 사라지므로 설치를 막는다.
    #[test]
    fn a_changed_data_branch_voids_the_quiet_frame_precondition() {
        let (source, candidate) = quiet_frame_roms();
        let mutated = with_fixed_byte_replaced(&candidate, ROW_UPLOAD_ADDRESS, 0xEA);

        let error = bind_quiet_frame_gate(&source, &mutated).unwrap_err();

        assert!(error.to_string().contains("precondition is void"));
    }

    #[test]
    fn a_changed_chr_restore_gate_voids_the_busy_frame_precondition() {
        let (source, candidate) = quiet_frame_roms();
        let mutated = with_fixed_byte_replaced(&candidate, CHR_RESTORE_GATE_ADDRESS, 0xEA);

        let error = bind_quiet_frame_gate(&source, &mutated).unwrap_err();

        assert!(error.to_string().contains("CHR-restore gate"));
    }

    fn quiet_frame_roms() -> (Rom, Rom) {
        let mut source_bytes = crate::test_support::synthetic_mapper165_rom_bytes(0xFF);
        for (address, expected) in [
            (CONSUMER_HOOK, HOOK_SITE.as_slice()),
            (BLOCK_INTERPRETER_ADDRESS, BLOCK_INTERPRETER_GATE.as_slice()),
            (PALETTE_QUEUE_ADDRESS, PALETTE_QUEUE_GATE.as_slice()),
            (ROW_UPLOAD_ADDRESS, ROW_UPLOAD_GATE.as_slice()),
            (CONTROL_RESTORE_ADDRESS, CONTROL_RESTORE.as_slice()),
        ] {
            let offset = crate::test_support::synthetic_fixed_bank_file_offset(address);
            source_bytes[offset..offset + expected.len()].copy_from_slice(expected);
        }
        let source_gate = assemble_at(
            CHR_RESTORE_GATE_ADDRESS,
            &[
                Instruction::LdaZeroPage(VBLANK_BUSY_FLAGS[4]),
                Instruction::BeqAbsolute(CHR_RESTORE_SKIP_ADDRESS),
                Instruction::LdaZeroPage(0x5E),
                Instruction::StaAbsolute(0xD000),
                Instruction::LdaZeroPage(0x5F),
                Instruction::StaAbsolute(0xE000),
                Instruction::Rts,
            ],
        )
        .unwrap();
        let gate_offset =
            crate::test_support::synthetic_fixed_bank_file_offset(CHR_RESTORE_GATE_ADDRESS);
        source_bytes[gate_offset..gate_offset + source_gate.len()].copy_from_slice(&source_gate);

        let mut candidate_bytes = source_bytes.clone();
        let candidate_gate = assemble_at(
            CHR_RESTORE_GATE_ADDRESS,
            &[
                Instruction::LdaZeroPage(VBLANK_BUSY_FLAGS[4]),
                Instruction::BeqAbsolute(CHR_RESTORE_SKIP_ADDRESS),
                Instruction::LdaZeroPage(0x5E),
                Instruction::JsrAbsolute(CANDIDATE_RIGHT_FD_HELPER),
                Instruction::LdaZeroPage(0x5F),
                Instruction::JsrAbsolute(CANDIDATE_RIGHT_FE_HELPER),
                Instruction::Rts,
            ],
        )
        .unwrap();
        candidate_bytes[gate_offset..gate_offset + candidate_gate.len()]
            .copy_from_slice(&candidate_gate);

        (
            Rom::parse(source_bytes).expect("quiet-frame source fixture parses"),
            Rom::parse(candidate_bytes).expect("quiet-frame candidate fixture parses"),
        )
    }

    fn with_fixed_byte_replaced(rom: &Rom, address: u16, value: u8) -> Rom {
        let mut bytes = rom.data().to_vec();
        let offset = crate::test_support::synthetic_fixed_bank_file_offset(address);
        bytes[offset] = value;
        Rom::parse(bytes).expect("mutated image still parses")
    }
}
