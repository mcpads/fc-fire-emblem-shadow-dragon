//! `$C179`에 들어가는 고정 뱅크 트램폴린이다.
//!
//! 원본의 `JSR $C3A5`를 밀어내고 대신 불린다. 조용한 프레임이 아니거나 올릴 것이
//! 없으면 아무것도 하지 않고 곧바로 원본 호출로 넘긴다. 의사결정 64번을 따른다.
//!
//! 고정 뱅크에 있는 이유는 전송 루틴이 재료 용기의 마지막 MMC3 페이지에 있기 때문이다. 그 페이지를
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
    runtime_bank_contract::{BANK_INDEX_MASK, BankRestoreContract},
    runtime_nmi_contract::{DISPLACED_CALL, PPU_CONTROL_SHADOW, VBLANK_BUSY_FLAGS},
};
use super::{
    CONSUMER_HOOK_CALL_CYCLES, RuntimeRoutine, next_address,
    transport::{REQUEST_STATE, STATE_READY},
    worst_case_cycles_with_calls,
};
use crate::mapper165::battle_composition_loader_probe::{
    DIALOGUE_SUBSTATE_ADDRESS, ENEMY_INITIATED_BATTLE_STATE, MAIN_STATE_ADDRESS,
    PLAYER_INITIATED_BATTLE_STATE, SOUND_TEST_BATTLE_PHASE_ADDRESS, SOUND_TEST_BATTLE_SUBSTATE,
    SOUND_TEST_MAIN_STATE, SOUND_TEST_SHARED_BATTLE_PHASE,
};
use crate::rp2a03::{Instruction, assemble_at};

/// 고정 뱅크에 비워 둔 동굴의 시작이다.
pub(in crate::full_translation_install) const TRAMPOLINE_ORIGIN: u16 = 0xF400;
/// 그 동굴의 끝이다. 넘으면 원본 자료를 덮는다.
pub(super) const TRAMPOLINE_CAVE_END: u16 = 0xF4B0;
pub(super) use super::super::runtime_material::RUNTIME_CODE_MMC3_PAGE;

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
    let main_state =
        u8::try_from(MAIN_STATE_ADDRESS).expect("the battle/main state must remain in zero page");
    let mut instructions = vec![
        // 원본 block interpreter는 한 프레임에 PPU command block을 여럿 비울 수 있다.
        // 이 경로는 실제 후일담 화면에서 vblank 끝까지 닿았으므로, 전투 화면 판정조차
        // 하기 전에 가장 짧은 경로로 원본 호출에 양보한다. 나머지 busy flag도 전투
        // 판정보다 먼저 합쳐 검사하되, `$21`의 taken path에는 OR 네 개를 얹지 않는다.
        Instruction::LdaZeroPage(VBLANK_BUSY_FLAGS[0]),
        Instruction::BneAbsolute(origin),
        Instruction::LdaZeroPage(VBLANK_BUSY_FLAGS[1]),
        Instruction::OraZeroPage(VBLANK_BUSY_FLAGS[2]),
        Instruction::OraZeroPage(VBLANK_BUSY_FLAGS[3]),
        Instruction::OraZeroPage(VBLANK_BUSY_FLAGS[4]),
        Instruction::BneAbsolute(origin),
        // 전투 합성기가 같은 NMI의 뒤쪽에서 4 KiB CHR 페이지를 쓸 수 있는 화면이면
        // 대사 전송은 먼저 물러난다. `$047D`는 비전투에서 FF일 수 있으므로 그것만
        // 보면 안 되고, 전투 디스패처가 쓰는 일반/사운드 화면 조건을 그대로 쓴다.
        Instruction::LdaZeroPage(main_state),
        Instruction::CmpImmediate(PLAYER_INITIATED_BATTLE_STATE),
        Instruction::BeqAbsolute(origin),
        Instruction::CmpImmediate(ENEMY_INITIATED_BATTLE_STATE),
        Instruction::BeqAbsolute(origin),
        Instruction::CmpImmediate(SOUND_TEST_MAIN_STATE),
        Instruction::BneAbsolute(origin),
        Instruction::LdaAbsolute(DIALOGUE_SUBSTATE_ADDRESS),
        Instruction::CmpImmediate(SOUND_TEST_BATTLE_SUBSTATE),
        Instruction::BneAbsolute(origin),
        Instruction::LdaAbsolute(SOUND_TEST_BATTLE_PHASE_ADDRESS),
        Instruction::CmpImmediate(SOUND_TEST_SHARED_BATTLE_PHASE),
        Instruction::BeqAbsolute(origin),
    ];
    let block_interpreter_busy_placeholder = 1;
    let other_busy_placeholder = 6;
    let surface_predicate_start = 7;
    let player_battle_placeholder = surface_predicate_start + 2;
    let enemy_battle_placeholder = surface_predicate_start + 4;
    let non_sound_test_placeholder = surface_predicate_start + 6;
    let inactive_sound_test_substate_placeholder = surface_predicate_start + 9;
    let sound_test_battle_placeholder = surface_predicate_start + 12;
    let transport_gate = next_address(origin, &instructions)?;
    instructions[non_sound_test_placeholder] = Instruction::BneAbsolute(transport_gate);
    instructions[inactive_sound_test_substate_placeholder] =
        Instruction::BneAbsolute(transport_gate);

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
        crate::mapper165::selector_safety::select_register_instruction(),
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
        crate::mapper165::selector_safety::select_register_instruction(),
        Instruction::Txa,
        Instruction::StaAbsolute(BANK_VALUE_REGISTER),
        Instruction::LdaImmediate(contract.prg_a000_register),
        crate::mapper165::selector_safety::select_register_instruction(),
        Instruction::Txa,
        Instruction::OraImmediate(1),
        Instruction::StaAbsolute(BANK_VALUE_REGISTER),
    ]);

    let done = next_address(origin, &instructions)?;
    instructions[player_battle_placeholder] = Instruction::BeqAbsolute(done);
    instructions[enemy_battle_placeholder] = Instruction::BeqAbsolute(done);
    instructions[sound_test_battle_placeholder] = Instruction::BeqAbsolute(done);
    instructions[block_interpreter_busy_placeholder] = Instruction::BneAbsolute(done);
    instructions[other_busy_placeholder] = Instruction::BneAbsolute(done);
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

    #[derive(Clone, Copy, Debug, Eq, PartialEq)]
    enum SurfaceRoute {
        QuietGate,
        Done,
    }

    fn run_emitted_surface_predicate(
        listing: &[Instruction],
        main_state: u8,
        dialogue_substate: u8,
        sound_test_phase: u8,
    ) -> SurfaceRoute {
        const BUSY_GATE_INSTRUCTION_COUNT: usize = 7;
        const PREDICATE_INSTRUCTION_COUNT: usize = 13;
        let predicate_start =
            next_address(TRAMPOLINE_ORIGIN, &listing[..BUSY_GATE_INSTRUCTION_COUNT]).unwrap();
        let predicate = &listing[BUSY_GATE_INSTRUCTION_COUNT
            ..BUSY_GATE_INSTRUCTION_COUNT + PREDICATE_INSTRUCTION_COUNT];
        let addresses = (0..=predicate.len())
            .map(|length| next_address(predicate_start, &predicate[..length]).unwrap())
            .collect::<Vec<_>>();
        let quiet_gate = addresses[PREDICATE_INSTRUCTION_COUNT];
        let done = next_address(TRAMPOLINE_ORIGIN, &listing[..listing.len() - 1]).unwrap();
        let mut pc = predicate_start;
        let mut accumulator = 0;
        let mut zero = false;

        loop {
            if pc == quiet_gate {
                return SurfaceRoute::QuietGate;
            }
            if pc == done {
                return SurfaceRoute::Done;
            }
            let index = addresses[..PREDICATE_INSTRUCTION_COUNT]
                .iter()
                .position(|address| *address == pc)
                .expect("surface predicate branched outside its emitted instructions");
            let fallthrough = addresses[index + 1];
            match predicate[index] {
                Instruction::LdaZeroPage(address) if u16::from(address) == MAIN_STATE_ADDRESS => {
                    accumulator = main_state;
                }
                Instruction::LdaAbsolute(DIALOGUE_SUBSTATE_ADDRESS) => {
                    accumulator = dialogue_substate;
                }
                Instruction::LdaAbsolute(SOUND_TEST_BATTLE_PHASE_ADDRESS) => {
                    accumulator = sound_test_phase;
                }
                Instruction::CmpImmediate(value) => zero = accumulator == value,
                Instruction::BeqAbsolute(target) => {
                    pc = if zero { target } else { fallthrough };
                    continue;
                }
                Instruction::BneAbsolute(target) => {
                    pc = if zero { fallthrough } else { target };
                    continue;
                }
                ref instruction => {
                    panic!("unexpected surface predicate instruction: {instruction:?}")
                }
            }
            pc = fallthrough;
        }
    }

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

        assert_eq!(reserve, 197);
        assert_eq!(budget, 1_156);
        assert_eq!(
            reserve + budget,
            super::super::MAPPER_VBLANK_REMAINDER * (100 - super::super::SAFETY_MARGIN_PERCENT)
                / 100
        );
    }

    /// 원본에 일이 있는 프레임에는 PPU도 뱅크도 건드리지 않아야 한다. 그러지 않으면
    /// 게이트가 아무것도 지키지 못한다.
    #[test]
    fn the_skip_path_touches_neither_the_ppu_nor_the_bank_registers() {
        let listing = instructions(contract(), 0xB000).unwrap();
        let skip_target = match listing[1] {
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

    #[test]
    fn the_chr_restore_flag_uses_the_same_busy_skip_path() {
        let listing = instructions(contract(), 0xB000).unwrap();

        assert_eq!(
            &listing[..7],
            &[
                Instruction::LdaZeroPage(0x21),
                Instruction::BneAbsolute(match listing[1] {
                    Instruction::BneAbsolute(target) => target,
                    _ => unreachable!(),
                }),
                Instruction::LdaZeroPage(0x22),
                Instruction::OraZeroPage(0x89),
                Instruction::OraZeroPage(0x8A),
                Instruction::OraZeroPage(0x5D),
                Instruction::BneAbsolute(match listing[6] {
                    Instruction::BneAbsolute(target) => target,
                    _ => unreachable!(),
                }),
            ]
        );
        assert_eq!(listing[1], listing[6]);
    }

    /// `$21` queue는 실제 후일담에서 vblank 끝까지 썼다. 이 flag가 켜진 경로는
    /// 전투 상태나 다른 busy flag를 읽기 전에 즉시 원본 interpreter로 돌아가야 한다.
    #[test]
    fn the_block_interpreter_queue_uses_the_shortest_busy_path() {
        let listing = instructions(contract(), 0xB000).unwrap();
        let done = next_address(TRAMPOLINE_ORIGIN, &listing[..listing.len() - 1]).unwrap();

        assert_eq!(listing[0], Instruction::LdaZeroPage(0x21));
        assert_eq!(listing[1], Instruction::BneAbsolute(done));
        assert_eq!(listing[2], Instruction::LdaZeroPage(0x22));
        assert_eq!(
            listing[7],
            Instruction::LdaZeroPage(
                u8::try_from(MAIN_STATE_ADDRESS).expect("main state is zero-page")
            )
        );
        assert_eq!(
            listing.last(),
            Some(&Instruction::JmpAbsolute(DISPLACED_CALL))
        );
        assert_eq!(done >> 8, TRAMPOLINE_ORIGIN >> 8);
    }

    #[test]
    fn battle_surface_states_preempt_dialogue_transport_in_the_same_nmi() {
        let listing = instructions(contract(), 0xB000).unwrap();
        let surface_start = 7;
        let transport_gate =
            next_address(TRAMPOLINE_ORIGIN, &listing[..surface_start + 13]).unwrap();

        assert_eq!(
            listing[surface_start],
            Instruction::LdaZeroPage(
                u8::try_from(MAIN_STATE_ADDRESS).expect("main state is zero-page")
            )
        );
        assert_eq!(
            listing[surface_start + 1],
            Instruction::CmpImmediate(PLAYER_INITIATED_BATTLE_STATE)
        );
        let player_skip_target = match listing[surface_start + 2] {
            Instruction::BeqAbsolute(target) => target,
            _ => unreachable!(),
        };
        let enemy_skip_target = match listing[surface_start + 4] {
            Instruction::BeqAbsolute(target) => target,
            _ => unreachable!(),
        };
        let sound_skip_target = match listing[surface_start + 12] {
            Instruction::BeqAbsolute(target) => target,
            _ => unreachable!(),
        };
        let non_sound_target = match listing[surface_start + 6] {
            Instruction::BneAbsolute(target) => target,
            _ => unreachable!(),
        };
        let inactive_sound_substate_target = match listing[surface_start + 9] {
            Instruction::BneAbsolute(target) => target,
            _ => unreachable!(),
        };
        assert_eq!(player_skip_target, enemy_skip_target);
        assert_eq!(player_skip_target, sound_skip_target);
        assert_eq!(non_sound_target, transport_gate);
        assert_eq!(inactive_sound_substate_target, transport_gate);
        assert_eq!(
            listing.last(),
            Some(&Instruction::JmpAbsolute(DISPLACED_CALL))
        );
    }

    #[test]
    fn emitted_battle_surface_predicate_matches_the_runtime_truth_table() {
        let listing = instructions(contract(), 0xB000).unwrap();

        for (main_state, dialogue_substate, sound_test_phase) in [
            (PLAYER_INITIATED_BATTLE_STATE, 0xFF, 0xFF),
            (ENEMY_INITIATED_BATTLE_STATE, 0xFF, 0xFF),
            (
                SOUND_TEST_MAIN_STATE,
                SOUND_TEST_BATTLE_SUBSTATE,
                SOUND_TEST_SHARED_BATTLE_PHASE,
            ),
        ] {
            assert_eq!(
                run_emitted_surface_predicate(
                    &listing,
                    main_state,
                    dialogue_substate,
                    sound_test_phase,
                ),
                SurfaceRoute::Done,
            );
        }

        for (main_state, dialogue_substate, sound_test_phase) in [
            (0xFF, 0xFF, 0xFF),
            (
                SOUND_TEST_MAIN_STATE,
                SOUND_TEST_BATTLE_SUBSTATE,
                SOUND_TEST_SHARED_BATTLE_PHASE - 1,
            ),
            (
                SOUND_TEST_MAIN_STATE,
                SOUND_TEST_BATTLE_SUBSTATE - 1,
                SOUND_TEST_SHARED_BATTLE_PHASE,
            ),
            (
                SOUND_TEST_MAIN_STATE - 1,
                SOUND_TEST_BATTLE_SUBSTATE,
                SOUND_TEST_SHARED_BATTLE_PHASE,
            ),
        ] {
            assert_eq!(
                run_emitted_surface_predicate(
                    &listing,
                    main_state,
                    dialogue_substate,
                    sound_test_phase,
                ),
                SurfaceRoute::QuietGate,
            );
        }
    }

    #[test]
    fn nonbattle_ff_active_sentinel_does_not_block_dialogue_transport() {
        let listing = instructions(contract(), 0xB000).unwrap();

        assert!(!listing.iter().any(|instruction| {
            matches!(
                instruction,
                Instruction::LdaAbsolute(address)
                    if *address
                        == crate::mapper165::battle_composition_loader_probe::BATTLE_ACTIVE_FLAG
            )
        }));
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
            listing[call..].iter().any(|instruction| matches!(
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
            usize::from(routine.address) + routine.bytes.len() <= usize::from(TRAMPOLINE_CAVE_END)
        );
    }
}
