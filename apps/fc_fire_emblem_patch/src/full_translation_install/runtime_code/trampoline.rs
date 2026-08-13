//! `$C179`에 들어가는 고정 뱅크 트램폴린이다.
//!
//! 원본의 `JSR $C3A5`를 밀어내고 대신 불린다. 조용한 프레임이 아니거나 올릴 것이
//! 없으면 아무것도 하지 않고 곧바로 원본 호출로 넘긴다. 의사결정 64번을 따른다.
//!
//! 고정 뱅크에 있는 이유는 전송 루틴이 페이지 `2E`에 있기 때문이다. 그 페이지를
//! `$A000`에 걸려면 걸고 되돌리는 코드가 뱅크 전환에 영향받지 않는 자리에 있어야
//! 한다.
//!
//! 창을 둘 쓴다. 실행 코드는 `$A000`(레지스터 7), 읽을 atlas는 `$8000`(레지스터 6)에
//! 건다. 한 창에 둘 다 담을 수 없기 때문이다.
//!
//! 두 창을 모두 되돌리는 이유는 `$C296`이 `$BFC0`에서 포인터를 읽기 때문이다. 그
//! 주소는 `$A000` 창 안이라 레지스터 7이 어긋난 채로 넘기면 원본이 남의 자료를
//! 포인터로 읽는다. 지금은 게이트가 `$22 == 0`을 보장해 그 경로가 돌지 않지만,
//! 되돌리지 않는 설계는 그 보장에 기대게 되므로 그렇게 두지 않는다.

use anyhow::{Result, ensure};

use super::super::{
    runtime_bank_contract::{BankRestoreContract, BANK_INDEX_MASK},
    runtime_nmi_contract::{DISPLACED_CALL, PPU_CONTROL_SHADOW, QUEUE_FLAGS},
};
use super::{
    CONSUMER_HOOK_CALL_CYCLES, RuntimeRoutine, next_address, worst_case_cycles_with_calls,
    transport::{REQUEST_STATE, STATE_READY},
};
use crate::rp2a03::{Instruction, assemble_at};

/// 고정 뱅크에 비워 둔 동굴의 시작이다.
pub(super) const TRAMPOLINE_ORIGIN: u16 = 0xF400;
/// 그 동굴의 끝이다. 넘으면 원본 자료를 덮는다.
pub(super) const TRAMPOLINE_CAVE_END: u16 = 0xF4B0;
pub(super) use super::super::runtime_material::RUNTIME_CODE_MMC3_PAGE;

const BANK_SELECT_REGISTER: u16 = 0x8000;
const BANK_VALUE_REGISTER: u16 = 0x8001;
const PPU_CONTROL: u16 = 0x2000;
/// `$2000`의 증가 비트를 끄는 마스크다. `$D4E7`이 쓰는 값과 같다.
const SEQUENTIAL_INCREMENT_MASK: u8 = 0xFB;

/// `$C179`에 쓸 세 바이트다.
pub(super) fn hook_bytes() -> [u8; 3] {
    [
        0x20,
        TRAMPOLINE_ORIGIN as u8,
        (TRAMPOLINE_ORIGIN >> 8) as u8,
    ]
}

fn instructions(contract: BankRestoreContract, transport_entry: u16) -> Result<Vec<Instruction>> {
    let origin = TRAMPOLINE_ORIGIN;
    let mut instructions = vec![
        // 원본이 이 프레임에 PPU 자료를 쓸 예정이면 비켜난다.
        Instruction::LdaZeroPage(QUEUE_FLAGS[0]),
        Instruction::OraZeroPage(QUEUE_FLAGS[1]),
        Instruction::OraZeroPage(QUEUE_FLAGS[2]),
        Instruction::OraZeroPage(QUEUE_FLAGS[3]),
    ];
    let busy_placeholder = instructions.len();
    instructions.push(Instruction::BneAbsolute(origin));

    // 요청이 없으면(`inactive`) 할 일이 없다.
    instructions.push(Instruction::LdaAbsolute(REQUEST_STATE));
    let inactive_placeholder = instructions.len();
    instructions.push(Instruction::BeqAbsolute(origin));
    // `ready` 이상이면 이미 끝났거나 알 수 없는 값이다. 둘 다 비켜난다.
    instructions.push(Instruction::CmpImmediate(STATE_READY));
    let settled_placeholder = instructions.len();
    instructions.push(Instruction::BcsAbsolute(origin));

    instructions.extend([
        // 순차 증가를 강제하고 그림자도 함께 고친다. `$C185`의 `$C733`이 이 그림자를
        // 다시 쓰므로 여기서 어긋나면 원본이 잘못된 증가 모드로 돌아간다.
        Instruction::LdaZeroPage(PPU_CONTROL_SHADOW),
        Instruction::AndImmediate(SEQUENTIAL_INCREMENT_MASK),
        Instruction::StaZeroPage(PPU_CONTROL_SHADOW),
        Instruction::StaAbsolute(PPU_CONTROL),
        // 실행 코드 페이지를 `$A000`에 건다. 원본 도우미 `$FA20`은 입력을 네 비트로
        // 잘라 이 페이지에 닿지 못하므로 레지스터를 직접 쓴다. `$8000`은 전송 루틴이
        // 타일마다 스스로 바꾸므로 여기서 걸지 않는다.
        Instruction::LdaImmediate(contract.prg_a000_register),
        Instruction::StaAbsolute(BANK_SELECT_REGISTER),
        Instruction::LdaImmediate(RUNTIME_CODE_MMC3_PAGE),
        Instruction::StaAbsolute(BANK_VALUE_REGISTER),
        Instruction::JsrAbsolute(transport_entry),
        // 원본이 기대하는 짝으로 되돌린다. `$29`가 현재 16 KiB 뱅크의 그림자이고
        // 짝은 `(shadow & 0x0F) × 2`와 그 홀수 쪽이다.
        Instruction::LdaZeroPage(contract.prg_bank_shadow),
        Instruction::AndImmediate(BANK_INDEX_MASK),
        Instruction::AslAccumulator,
        Instruction::Tax,
        Instruction::LdaImmediate(contract.prg_8000_register),
        Instruction::StaAbsolute(BANK_SELECT_REGISTER),
        Instruction::Txa,
        Instruction::StaAbsolute(BANK_VALUE_REGISTER),
        Instruction::LdaImmediate(contract.prg_a000_register),
        Instruction::StaAbsolute(BANK_SELECT_REGISTER),
        Instruction::Txa,
        Instruction::OraImmediate(1),
        Instruction::StaAbsolute(BANK_VALUE_REGISTER),
    ]);

    let done = next_address(origin, &instructions)?;
    instructions[busy_placeholder] = Instruction::BneAbsolute(done);
    instructions[inactive_placeholder] = Instruction::BeqAbsolute(done);
    instructions[settled_placeholder] = Instruction::BcsAbsolute(done);
    // 밀어낸 원본 호출로 넘긴다. `$C3A5`의 `RTS`가 `$C17C`로 돌아간다.
    instructions.push(Instruction::JmpAbsolute(DISPLACED_CALL));
    Ok(instructions)
}

pub(super) fn build_trampoline(
    contract: BankRestoreContract,
    transport_entry: u16,
) -> Result<RuntimeRoutine> {
    let instructions = instructions(contract, transport_entry)?;
    let bytes = assemble_at(TRAMPOLINE_ORIGIN, &instructions)?;
    ensure!(
        usize::from(TRAMPOLINE_ORIGIN) + bytes.len() <= usize::from(TRAMPOLINE_CAVE_END),
        "the dialogue trampoline is {} bytes and overruns the {}-byte fixed cave",
        bytes.len(),
        TRAMPOLINE_CAVE_END - TRAMPOLINE_ORIGIN
    );
    Ok(RuntimeRoutine {
        role: "dialogue trampoline",
        address: TRAMPOLINE_ORIGIN,
        bytes,
    })
}

/// 훅 호출과 트램폴린 자신이 최악의 경우 쓰는 사이클이다. 전송 루틴 몸통은 빼고
/// 센다. 그쪽은 자기 예산을 따로 지킨다.
pub(super) fn worst_case_reserve_cycles(contract: BankRestoreContract) -> Result<u32> {
    let transport_entry = 0xB000;
    let instructions = instructions(contract, transport_entry)?;
    // 전송 루틴 호출은 트램폴린 몫에 넣지 않는다. 그쪽은 자기 예산을 따로 지키므로
    // 여기서는 `JSR` 명령 자체만 세면 된다.
    Ok(CONSUMER_HOOK_CALL_CYCLES
        + worst_case_cycles_with_calls(&instructions, &[(transport_entry, 0)])?)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn contract() -> BankRestoreContract {
        BankRestoreContract {
            prg_8000_register: 6,
            prg_a000_register: 7,
            prg_bank_shadow: 0x29,
            helper_reachable_page_count: 32,
        }
    }

    /// 트램폴린 몫과 전송 예산을 합쳐도 vblank 여유 안에 들어야 한다. 이 둘이
    /// 같은 vblank를 나눠 쓰는 관계라는 것을 여기서 고정한다.
    #[test]
    fn the_trampoline_reserve_and_the_transport_budget_share_one_vblank() {
        let reserve = worst_case_reserve_cycles(contract()).unwrap();

        let budget = super::super::budgeted_transport_cycles(reserve);

        assert_eq!(
            reserve + budget,
            super::super::MEASURED_VBLANK_REMAINDER
                * (100 - super::super::SAFETY_MARGIN_PERCENT)
                / 100
        );
    }

    /// 원본에 일이 있는 프레임에는 PPU도 뱅크도 건드리지 않아야 한다. 그러지 않으면
    /// 게이트가 아무것도 지키지 못한다.
    #[test]
    fn the_skip_path_touches_neither_the_ppu_nor_the_bank_registers() {
        let listing = instructions(contract(), 0xB000).unwrap();
        let gate_branch = listing
            .iter()
            .position(|instruction| matches!(instruction, Instruction::BneAbsolute(_)))
            .expect("the trampoline branches on the queue flags");
        let skip_target = match listing[gate_branch] {
            Instruction::BneAbsolute(target) => target,
            _ => unreachable!(),
        };

        // 건너뛴 자리부터 끝까지는 원본 호출로 넘기는 `JMP` 하나뿐이어야 한다.
        let mut address = TRAMPOLINE_ORIGIN;
        let mut after_skip = Vec::new();
        for instruction in &listing {
            if address >= skip_target {
                after_skip.push(*instruction);
            }
            address += u16::try_from(
                assemble_at(address, std::slice::from_ref(instruction))
                    .unwrap()
                    .len(),
            )
            .unwrap();
        }

        assert_eq!(after_skip, vec![Instruction::JmpAbsolute(DISPLACED_CALL)]);
    }

    /// 모든 경로가 밀어낸 원본 호출로 끝나야 한다. 그러지 않으면 원본의 블록 큐가
    /// 영원히 비워지지 않고 화면이 갱신되지 않는다.
    #[test]
    fn every_path_reaches_the_displaced_source_call() {
        let routine = build_trampoline(contract(), 0xB000).unwrap();
        let tail = &routine.bytes[routine.bytes.len() - 3..];

        assert_eq!(
            tail,
            [0x4C, DISPLACED_CALL as u8, (DISPLACED_CALL >> 8) as u8]
        );
    }

    /// 훅은 원본 호출을 트램폴린 호출로 바꾼다. 길이가 같아야 뒤 명령이 밀리지 않는다.
    #[test]
    fn the_hook_replaces_the_source_call_without_changing_its_length() {
        let source_call = [0x20, DISPLACED_CALL as u8, (DISPLACED_CALL >> 8) as u8];

        assert_eq!(hook_bytes().len(), source_call.len());
        assert_eq!(hook_bytes()[0], source_call[0]);
    }

    /// 뱅크를 되돌리지 않으면 NMI가 끝난 뒤 주 흐름이 남의 코드를 실행한다.
    #[test]
    fn the_bank_is_restored_from_the_source_shadow_before_returning() {
        let listing = instructions(contract(), 0xB000).unwrap();
        let call = listing
            .iter()
            .position(|instruction| matches!(instruction, Instruction::JsrAbsolute(_)))
            .expect("the trampoline calls the transport routine");

        assert!(
            listing[call..]
                .iter()
                .any(|instruction| matches!(
                    instruction,
                    Instruction::LdaZeroPage(shadow) if *shadow == contract().prg_bank_shadow
                )),
            "the trampoline never reads the bank shadow after the transport call"
        );
    }

    #[test]
    fn the_trampoline_fits_the_reserved_fixed_cave() {
        let routine = build_trampoline(contract(), 0xB000).unwrap();

        assert!(
            usize::from(routine.address) + routine.bytes.len()
                <= usize::from(TRAMPOLINE_CAVE_END)
        );
    }
}
