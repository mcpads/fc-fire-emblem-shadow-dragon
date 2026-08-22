//! 대사 CHR 페이지를 NMI 밖의 한 연속 렌더-off 구간에서 완성한다.
//!
//! 요청 발행기는 resolver를 실행하기 전에 NMI를 끈다. 이 루틴은 그 보호 구간에서
//! 다음 vblank를 기다려 렌더링을 끄고, 실행 코드 페이지의 전송기를 `ready`가 될
//! 때까지 연속 호출한다. 전송기가 끝나면 원본 PRG 뱅크와 호출자 레지스터를 되돌리고
//! 다시 다음 vblank까지 기다린다. 호출자는 그 자리에서 NMI를 되살리며, 다음 원본
//! NMI의 `$C733/$C36A`가 PPUMASK와 스크롤을 한 번에 복원한다.
//!
//! 이 경계의 목적은 속도보다 위상 보존이다. 전송을 `$C179` NMI에 끼워 넣으면 한
//! 프레임 몫 자체는 vblank에 들어가도 뒤쪽 원본 raster 경로가 밀려 화면 상단이
//! 주기적으로 검게 변한다. 여기서는 화면별 상태를 보지 않고 모든 cold/resident
//! 요청을 같은 주기에서 끝낸다.

use anyhow::{Result, ensure};

use super::super::{
    runtime_bank_contract::{BankRestoreContract, PRG_A000_REGISTER},
    runtime_nmi_contract::PPU_CONTROL_SHADOW,
};
use super::{RuntimeRoutine, next_address, transport::REQUEST_STATE};
use crate::rp2a03::{Instruction, assemble_at};

/// 이전 NMI 전송 트램폴린이 쓰던 exact-FF 동굴을 그대로 소유한다.
pub(in crate::full_translation_install) const COMPOSER_ORIGIN: u16 = 0xF400;
pub(super) const COMPOSER_CAVE_END: u16 = 0xF4B0;

const PPU_CONTROL: u16 = 0x2000;
const PPU_MASK: u16 = 0x2001;
const PPU_STATUS: u16 = 0x2002;
const VBLANK_FLAG: u8 = 0x80;
const NMI_ENABLE_MASK: u8 = 0x80;
const SEQUENTIAL_INCREMENT_MASK: u8 = 0x04;
const BANK_VALUE_REGISTER: u16 = 0x8001;
const PAIRED_BANK_HELPER: u16 = 0xFA20;

fn append_wait_for_next_vblank(instructions: &mut Vec<Instruction>, origin: u16) -> Result<()> {
    // 이미 선 vblank flag를 지운 뒤 다음 set을 기다려, 호출 시점이 vblank 안인지
    // 밖인지에 따라 렌더-off 경계가 흔들리지 않게 한다.
    instructions.push(Instruction::LdaAbsolute(PPU_STATUS));
    let wait = next_address(origin, instructions)?;
    instructions.extend([
        Instruction::LdaAbsolute(PPU_STATUS),
        Instruction::AndImmediate(VBLANK_FLAG),
        Instruction::BeqAbsolute(wait),
    ]);
    Ok(())
}

fn instructions(
    contract: BankRestoreContract,
    transport_entry: u16,
    runtime_code_page: u8,
) -> Result<Vec<Instruction>> {
    let origin = COMPOSER_ORIGIN;
    let mut instructions = vec![
        // 전송기는 NMI 프롤로그가 보존하던 A/X/Y/P와 $00/$01을 사용한다. 이제
        // mainline에서 부르므로 같은 상태를 이 루틴이 직접 보존한다.
        Instruction::Php,
        Instruction::Pha,
        Instruction::Txa,
        Instruction::Pha,
        Instruction::Tya,
        Instruction::Pha,
        Instruction::LdaZeroPage(0x00),
        Instruction::Pha,
        Instruction::LdaZeroPage(0x01),
        Instruction::Pha,
    ];

    append_wait_for_next_vblank(&mut instructions, origin)?;
    instructions.extend([
        // 화면은 clean vblank에서 한 번만 끈다. PPUMASK shadow는 건드리지 않아
        // 다음 원본 NMI가 원래 값을 복원할 수 있게 한다.
        Instruction::LdaImmediate(0),
        Instruction::StaAbsolute(PPU_MASK),
        // NMI는 계속 끄고 PPU data increment를 1로 강제한다. shadow는 호출자가
        // 이미 원래 값으로 되돌려 두었으므로 하드웨어에만 쓴다.
        Instruction::LdaZeroPage(PPU_CONTROL_SHADOW),
        Instruction::AndImmediate(!(NMI_ENABLE_MASK | SEQUENTIAL_INCREMENT_MASK)),
        Instruction::StaAbsolute(PPU_CONTROL),
        // 전송 실행 페이지를 A000에 건다. 전송기는 8000 자료 창만 바꾼다.
        Instruction::LdaImmediate(PRG_A000_REGISTER),
        crate::mapper165::selector_safety::select_register_instruction(),
        Instruction::LdaImmediate(runtime_code_page),
        Instruction::StaAbsolute(BANK_VALUE_REGISTER),
    ]);

    let run_transport = next_address(origin, &instructions)?;
    instructions.extend([
        Instruction::JsrAbsolute(transport_entry),
        Instruction::LdaAbsolute(REQUEST_STATE),
        Instruction::CmpImmediate(super::transport::STATE_READY),
        // cold는 복원과 overlay 두 호출, resident는 overlay 한 호출로 끝난다.
        // 알 수 없는 상위 상태는 무한 루프 대신 호출자에게 돌려보낸다.
        Instruction::BccAbsolute(run_transport),
        Instruction::LdaZeroPage(contract.prg_bank_shadow),
        Instruction::JsrAbsolute(PAIRED_BANK_HELPER),
    ]);

    append_wait_for_next_vblank(&mut instructions, origin)?;
    instructions.extend([
        // 민 순서의 반대로 호출자 상태를 되돌린다. 렌더링은 계속 꺼 둔다.
        // $00/$01과 X/Y를 먼저 되돌린 뒤 NMI를 되살린다. 저장해 둔 A/P는 마지막에
        // 복구하므로 PPU control 쓰기가 호출자 상태를 바꾸지 않는다. 다음 원본 NMI가
        // mask/scroll을 복원한다.
        Instruction::Pla,
        Instruction::StaZeroPage(0x01),
        Instruction::Pla,
        Instruction::StaZeroPage(0x00),
        Instruction::Pla,
        Instruction::Tay,
        Instruction::Pla,
        Instruction::Tax,
        Instruction::LdaZeroPage(PPU_CONTROL_SHADOW),
        Instruction::StaAbsolute(PPU_CONTROL),
        Instruction::Pla,
        Instruction::Plp,
        Instruction::Rts,
    ]);
    Ok(instructions)
}

pub(super) fn build_synchronous_composer(
    contract: BankRestoreContract,
    transport_entry: u16,
    runtime_code_page: u8,
) -> Result<RuntimeRoutine> {
    let bytes = assemble_at(
        COMPOSER_ORIGIN,
        &instructions(contract, transport_entry, runtime_code_page)?,
    )?;
    ensure!(
        usize::from(COMPOSER_ORIGIN) + bytes.len() <= usize::from(COMPOSER_CAVE_END),
        "the synchronous dialogue composer is {} bytes and overruns the {}-byte fixed cave",
        bytes.len(),
        COMPOSER_CAVE_END - COMPOSER_ORIGIN
    );
    Ok(RuntimeRoutine {
        role: "synchronous dialogue page composer",
        address: COMPOSER_ORIGIN,
        bytes,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn contract() -> BankRestoreContract {
        BankRestoreContract {
            prg_bank_shadow: 0x29,
            helper_reachable_page_count: 32,
        }
    }

    #[test]
    fn one_clean_render_off_interval_contains_the_entire_transport() {
        let listing = instructions(contract(), 0xA000, 0x30).unwrap();
        let mask_off = listing
            .iter()
            .position(|instruction| *instruction == Instruction::StaAbsolute(PPU_MASK))
            .unwrap();
        let transport = listing
            .iter()
            .position(|instruction| *instruction == Instruction::JsrAbsolute(0xA000))
            .unwrap();
        let bank_restore = listing
            .iter()
            .position(|instruction| *instruction == Instruction::JsrAbsolute(PAIRED_BANK_HELPER))
            .unwrap();
        let resume_nmi = listing
            .iter()
            .rposition(|instruction| *instruction == Instruction::StaAbsolute(PPU_CONTROL))
            .unwrap();

        assert!(mask_off < transport && transport < bank_restore && bank_restore < resume_nmi);
        assert_eq!(
            listing
                .iter()
                .filter(|instruction| **instruction == Instruction::StaAbsolute(PPU_MASK))
                .count(),
            1
        );
        assert!(
            !listing.contains(&Instruction::LdaZeroPage(0xCC)),
            "the composer must not restore PPUMASK itself before scroll is restored"
        );
    }

    #[test]
    fn the_transport_repeats_until_ready_and_then_restores_the_source_bank() {
        let listing = instructions(contract(), 0xA000, 0x30).unwrap();
        let transport = listing
            .iter()
            .position(|instruction| *instruction == Instruction::JsrAbsolute(0xA000))
            .unwrap();

        assert_eq!(
            listing[transport + 1],
            Instruction::LdaAbsolute(REQUEST_STATE)
        );
        assert_eq!(
            listing[transport + 2],
            Instruction::CmpImmediate(super::super::transport::STATE_READY)
        );
        assert!(matches!(
            listing[transport + 3],
            Instruction::BccAbsolute(_)
        ));
        assert_eq!(
            listing[transport + 4],
            Instruction::LdaZeroPage(contract().prg_bank_shadow)
        );
        assert_eq!(
            listing[transport + 5],
            Instruction::JsrAbsolute(PAIRED_BANK_HELPER)
        );
    }

    #[test]
    fn caller_registers_flags_and_entry_pointer_are_balanced() {
        let listing = instructions(contract(), 0xA000, 0x30).unwrap();
        let pushes = listing
            .iter()
            .filter(|instruction| matches!(instruction, Instruction::Pha | Instruction::Php))
            .count();
        let pulls = listing
            .iter()
            .filter(|instruction| matches!(instruction, Instruction::Pla | Instruction::Plp))
            .count();

        assert_eq!(pushes, pulls);
        assert_eq!(listing.last(), Some(&Instruction::Rts));
        assert!(
            listing
                .windows(2)
                .any(|pair| { pair == [Instruction::Pla, Instruction::StaZeroPage(0x01)] })
        );
        assert!(
            listing
                .windows(2)
                .any(|pair| { pair == [Instruction::Pla, Instruction::StaZeroPage(0x00)] })
        );
    }

    #[test]
    fn the_composer_fits_with_the_shared_publisher_in_the_owned_cave() {
        let routine = build_synchronous_composer(contract(), 0xA000, 0x30).unwrap();
        assert!(
            usize::from(routine.address) + routine.bytes.len() <= usize::from(COMPOSER_CAVE_END)
        );
    }
}
