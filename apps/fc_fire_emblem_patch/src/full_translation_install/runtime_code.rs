//! 대사 런타임이 ROM에 넣는 실행 코드다.
//!
//! 갈래를 나눈 기준은 «무엇이 바뀌면 이 파일이 바뀌는가»다. 전송 루프는 프레임
//! 예산이 바뀌면 바뀌고, 트램폴린은 원본 NMI 계약이 바뀌면 바뀐다.

use anyhow::{Context, Result, ensure};

use super::{
    runtime_bank_contract::bind_bank_restore_contract, runtime_nmi_contract::bind_quiet_frame_gate,
};
use crate::{
    rom::Rom,
    rp2a03::{Instruction, assemble_at},
};

pub(super) mod dispatcher_gate;
pub(super) mod trampoline;
pub(super) mod transport;

/// `$C179` 진입 시점에 남아 있는 vblank다. 앞에 NMI 진입 오버헤드와 OAM DMA밖에
/// 없고 둘 다 고정 비용이라 이 값은 표본이 아니라 상수다. 에뮬레이터 실측으로
/// 확인했고 계산값 `2,273 − 566`과 3사이클 차이다. 의사결정 64번을 따른다.
const MEASURED_VBLANK_REMAINDER: u32 = 1_704;
/// 실기 여유다. 남은 vblank를 전부 쓰지 않는다.
const SAFETY_MARGIN_PERCENT: u32 = 20;
/// `$C179`의 `JSR`가 쓰는 몫이다.
const CONSUMER_HOOK_CALL_CYCLES: u32 = 6;
/// 전송 루틴이 한 프레임에 쓸 수 있는 사이클이다.
///
/// `trampoline_reserve`는 훅 호출과 트램폴린이 실제로 쓰는 최악 사이클이고 방출한
/// 명령에서 센 값이다. 임의의 여백을 따로 두지 않는다. 안전 여유는 위의 20% 하나뿐이고,
/// 여백을 두 겹으로 쌓으면 어느 쪽이 실제 근거인지 알 수 없게 된다.
fn budgeted_transport_cycles(trampoline_reserve: u32) -> u32 {
    MEASURED_VBLANK_REMAINDER * (100 - SAFETY_MARGIN_PERCENT) / 100 - trampoline_reserve
}

/// 대사 런타임이 ROM에 넣는 실행 코드와 훅 전체다.
pub(super) struct DialogueRuntimeCodePlan {
    /// 페이지 `2E` 꼬리에 놓이는 전송 루틴이다.
    pub(super) transport: RuntimeRoutine,
    /// 고정 뱅크 동굴에 놓이는 조각들이다.
    pub(super) fixed_routines: Vec<RuntimeRoutine>,
    /// `$C179`에 쓸 소비자 훅이다.
    pub(super) consumer_hook: [u8; 3],
    /// `0A:$8000`에 쓸 디스패처 훅이다.
    pub(super) dispatcher_hook: [u8; 3],
    /// `0A:$809B`에 쓸 콜드 초기화 훅이다.
    pub(super) cold_hook: [u8; 3],
}

/// 실행 코드를 전부 조립한다.
///
/// 고정 뱅크 동굴의 배치는 여기서 한 번에 정한다. 조각마다 시작 주소를 따로 두면
/// 하나가 커졌을 때 다음 조각을 덮는다.
pub(super) fn plan_dialogue_runtime_code(
    source: &Rom,
    candidate: &Rom,
    runtime_code_cpu_start: u16,
    atlas_page: u8,
    atlas_cpu_base: u16,
    cold_tile_count: u8,
) -> Result<DialogueRuntimeCodePlan> {
    let bank_restore = bind_bank_restore_contract(candidate)?;
    bind_quiet_frame_gate(source, candidate)?;
    dispatcher_gate::bind_dispatcher_entry(source, candidate)?;

    let transport = transport::build_transport_routine(runtime_code_cpu_start)?;
    let trampoline_routine =
        trampoline::build_trampoline(bank_restore, atlas_page, transport.address)?;

    let gate_origin = trampoline_routine.address
        + u16::try_from(trampoline_routine.bytes.len())
            .context("dialogue trampoline length overflow")?;
    let gate = dispatcher_gate::build_dispatcher_gate(gate_origin)?;

    let initializer_origin = gate.address
        + u16::try_from(gate.bytes.len()).context("dispatcher gate length overflow")?;
    let initializer =
        dispatcher_gate::build_cold_initializer(initializer_origin, atlas_cpu_base, cold_tile_count)?;

    // 예산은 시험만이 아니라 빌드가 지킨다. vblank를 넘기는 코드는 ROM에 들어가면
    // 안 되므로, 여기서 막지 않으면 그 판정이 시험을 돌리는 사람에게 넘어간다.
    // 의사결정 62번을 따른다.
    let reserve = trampoline::worst_case_reserve_cycles(bank_restore)?;
    let budget = budgeted_transport_cycles(reserve);
    let frame_cycles = transport::worst_case_frame_cycles(runtime_code_cpu_start)?;
    ensure!(
        frame_cycles <= budget,
        "one transport frame costs {frame_cycles} cycles but only {budget} of the measured \
         {MEASURED_VBLANK_REMAINDER}-cycle vblank remainder are budgeted after the \
         {SAFETY_MARGIN_PERCENT}% margin and the {reserve}-cycle trampoline reserve"
    );

    let fixed_routines = vec![trampoline_routine, gate, initializer];
    ensure_disjoint(
        &fixed_routines.iter().collect::<Vec<_>>(),
        trampoline::TRAMPOLINE_CAVE_END,
    )?;

    Ok(DialogueRuntimeCodePlan {
        consumer_hook: trampoline::hook_bytes(),
        dispatcher_hook: dispatcher_gate::dispatcher_hook_bytes(fixed_routines[1].address),
        cold_hook: dispatcher_gate::cold_hook_bytes(fixed_routines[2].address),
        transport,
        fixed_routines,
    })
}

/// ROM의 한 자리에 놓이는 실행 코드 조각이다.
#[derive(Debug)]
pub(super) struct RuntimeRoutine {
    pub(super) role: &'static str,
    pub(super) address: u16,
    pub(super) bytes: Vec<u8>,
}

/// 같은 동굴에 놓이는 조각들이 서로 겹치거나 동굴을 넘지 않아야 한다.
/// 겹치면 조용히 잘못된 코드가 실행되고, 넘으면 원본 자료를 덮는다.
pub(super) fn ensure_disjoint(routines: &[&RuntimeRoutine], cave_end: u16) -> Result<()> {
    let mut ordered: Vec<&RuntimeRoutine> = routines.to_vec();
    ordered.sort_by_key(|routine| routine.address);
    for pair in ordered.windows(2) {
        ensure!(
            usize::from(pair[0].address) + pair[0].bytes.len() <= usize::from(pair[1].address),
            "{} ends at {:04X} and overlaps {} at {:04X}",
            pair[0].role,
            usize::from(pair[0].address) + pair[0].bytes.len(),
            pair[1].role,
            pair[1].address
        );
    }
    if let Some(last) = ordered.last() {
        ensure!(
            usize::from(last.address) + last.bytes.len() <= usize::from(cave_end),
            "{} reaches past the reserved cave end {cave_end:04X}",
            last.role
        );
    }
    Ok(())
}

/// 명령 목록을 이어 붙였을 때 다음 명령이 놓일 주소다. 분기 대상을 되메울 때 쓴다.
fn next_address(origin: u16, instructions: &[Instruction]) -> Result<u16> {
    let length = assemble_at(origin, instructions)
        .context("cannot measure a dialogue runtime routine")?
        .len();
    u16::try_from(usize::from(origin) + length)
        .context("dialogue runtime routine crosses the CPU address space")
}

/// 명령 목록이 최악의 경우 쓰는 사이클이다.
fn worst_case_cycles(instructions: &[Instruction]) -> u32 {
    instructions
        .iter()
        .map(|instruction| u32::from(instruction.worst_case_cycles()))
        .sum()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn overlapping_routines_are_refused() {
        let first = RuntimeRoutine {
            role: "first",
            address: 0xF400,
            bytes: vec![0; 16],
        };
        let second = RuntimeRoutine {
            role: "second",
            address: 0xF408,
            bytes: vec![0; 4],
        };

        let error = ensure_disjoint(&[&first, &second], 0xF4B0).unwrap_err();

        assert!(error.to_string().contains("overlaps"));
    }

    #[test]
    fn a_routine_past_the_cave_end_is_refused() {
        let only = RuntimeRoutine {
            role: "only",
            address: 0xF4A0,
            bytes: vec![0; 32],
        };

        let error = ensure_disjoint(&[&only], 0xF4B0).unwrap_err();

        assert!(error.to_string().contains("past the reserved cave end"));
    }
}
