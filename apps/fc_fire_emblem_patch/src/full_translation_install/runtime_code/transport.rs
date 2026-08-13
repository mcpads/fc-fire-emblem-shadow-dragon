//! 페이지 `2E`에 놓이는 전송 루틴이다.
//!
//! 한 프레임에 정해진 수의 타일만 CHR RAM으로 올리고 커서를 남긴다. 그 수는
//! `$C179`의 vblank 잔여 1,704사이클에서 안전 여유 20%를 뺀 값 안에 실제 코드의
//! 최악 사이클이 들어가도록 정한다. 의사결정 64번을 따른다.
//!
//! atlas는 타일당 8바이트 1bpp다. CHR에는 16바이트 2bpp로 펼치고 상위 bitplane은
//! 0으로 채운다. 상위 bitplane이 전부 0이라는 것은 직렬화 시점에 검사돼 있다.

use anyhow::{Context, Result, ensure};

use super::super::runtime_cursor_storage::{
    CURSOR_NEXT_TILE_INDEX, CURSOR_REMAINING_TILES, CURSOR_SOURCE_HIGH, CURSOR_SOURCE_LOW,
};
use super::{RuntimeRoutine, next_address, worst_case_cycles};
use crate::rp2a03::{Instruction, assemble_at};

/// 한 프레임에 올리는 타일 수다. 사이클 예산에서 유도한 값이므로 늘리려면
/// 아래 `budget` 테스트가 먼저 통과해야 한다.
pub(super) const TILES_PER_FRAME: u8 = 8;
/// 타일 하나가 CHR에서 차지하는 바이트다. 2bpp 8×8.
pub(super) const CHR_TILE_BYTE_COUNT: u8 = 16;
/// atlas가 타일 하나에 쓰는 바이트다. 1bpp 8×8.
pub(super) const ATLAS_TILE_BYTE_COUNT: u8 = 8;

/// 요청 상태 바이트다. 생산자가 쓰고 소비자가 지운다.
pub(super) const REQUEST_STATE: u16 = 0x07F4;
/// 합성이 끝나 출력해도 되는 상태다.
pub(super) const STATE_READY: u8 = 3;

/// CHR RAM이 PPU 주소 공간에서 시작하는 자리다.
const CHR_RAM_BASE: u16 = 0x1000;
const PPU_STATUS: u16 = 0x2002;
const PPU_ADDRESS: u16 = 0x2006;
const PPU_DATA: u16 = 0x2007;
/// NMI 프롤로그 `$C173`~`$C178`이 스택에 밀어 둔 제로 페이지다. 소비자가 써도 된다.
const SCRATCH_POINTER_LOW: u8 = 0x00;
const SCRATCH_POINTER_HIGH: u8 = 0x01;

/// 프레임 시작에서 커서를 읽고 PPU 주소를 세우는 부분이다.
///
/// 두 번째 값은 «할 일이 없을 때 뛰어넘는 `JMP`»의 색인이다. 상대 분기로는 루틴
/// 끝까지 닿지 못하므로 짧은 `BNE`로 `JMP` 하나를 건너뛰는 형태를 쓴다.
fn frame_prologue(origin: u16) -> Result<(Vec<Instruction>, usize)> {
    let mut instructions = vec![
        // 남은 타일이 없으면 할 일이 없다.
        Instruction::LdaAbsolute(CURSOR_REMAINING_TILES),
    ];
    let has_work_placeholder = instructions.len();
    instructions.push(Instruction::BneAbsolute(origin));
    let finished_jump = instructions.len();
    instructions.push(Instruction::JmpAbsolute(origin));
    let has_work = next_address(origin, &instructions)?;
    instructions[has_work_placeholder] = Instruction::BneAbsolute(has_work);

    // 이번 프레임 몫은 남은 것과 예산 중 작은 쪽이다.
    instructions.push(Instruction::CmpImmediate(TILES_PER_FRAME));
    let use_remaining_placeholder = instructions.len();
    instructions.push(Instruction::BccAbsolute(origin));
    instructions.push(Instruction::LdaImmediate(TILES_PER_FRAME));
    let batch_selected = next_address(origin, &instructions)?;
    instructions[use_remaining_placeholder] = Instruction::BccAbsolute(batch_selected);
    instructions.push(Instruction::Tax);

    // atlas 포인터를 제로 페이지에 세운다.
    instructions.extend([
        Instruction::LdaAbsolute(CURSOR_SOURCE_LOW),
        Instruction::StaZeroPage(SCRATCH_POINTER_LOW),
        Instruction::LdaAbsolute(CURSOR_SOURCE_HIGH),
        Instruction::StaZeroPage(SCRATCH_POINTER_HIGH),
    ]);

    // 목적지 PPU 주소를 타일 색인에서 만든다. `$1000 + index × 16`이다.
    // `$2002` 읽기로 주소 래치를 초기화한다.
    instructions.extend([
        Instruction::LdaAbsolute(PPU_STATUS),
        Instruction::LdaAbsolute(CURSOR_NEXT_TILE_INDEX),
        Instruction::LsrAccumulator,
        Instruction::LsrAccumulator,
        Instruction::LsrAccumulator,
        Instruction::LsrAccumulator,
        Instruction::Clc,
        Instruction::AdcImmediate((CHR_RAM_BASE >> 8) as u8),
        Instruction::StaAbsolute(PPU_ADDRESS),
        Instruction::LdaAbsolute(CURSOR_NEXT_TILE_INDEX),
        Instruction::AslAccumulator,
        Instruction::AslAccumulator,
        Instruction::AslAccumulator,
        Instruction::AslAccumulator,
        Instruction::StaAbsolute(PPU_ADDRESS),
        // Y는 이번 프레임이 읽은 atlas 바이트 수를 센다. 최대 `8 × 예산`이라
        // 한 프레임 안에서 절대 넘치지 않는다.
        Instruction::LdyImmediate(0),
    ]);
    Ok((instructions, finished_jump))
}

/// 타일 하나를 올리는 몸통이다. 길이가 고정이라 자료가 반복 횟수를 늘릴 수 없다.
fn tile_body(loop_start: u16) -> Vec<Instruction> {
    let mut instructions = Vec::new();
    for _ in 0..ATLAS_TILE_BYTE_COUNT {
        instructions.extend([
            Instruction::LdaIndirectY(SCRATCH_POINTER_LOW),
            Instruction::StaAbsolute(PPU_DATA),
            Instruction::Iny,
        ]);
    }
    // 상위 bitplane은 전부 0이다.
    instructions.push(Instruction::LdaImmediate(0));
    for _ in 0..ATLAS_TILE_BYTE_COUNT {
        instructions.push(Instruction::StaAbsolute(PPU_DATA));
    }
    instructions.extend([Instruction::Dex, Instruction::BneAbsolute(loop_start)]);
    instructions
}

/// 커서를 저장하고, 다 올렸으면 준비 완료를 알린다.
///
/// `pending_placeholder`는 이 조각이 놓이는 주소다. 아직 남았을 때 뛰어넘을 분기의
/// 자리표로 쓴다. 조각 안이라 상대 분기 범위 안이고, 실제 대상은 뒤에서 되메운다.
fn frame_epilogue(pending_placeholder: u16) -> Vec<Instruction> {
    let origin = pending_placeholder;
    vec![
        // atlas 포인터를 이번 프레임이 읽은 만큼 전진시킨다.
        Instruction::Tya,
        Instruction::Clc,
        Instruction::AdcZeroPage(SCRATCH_POINTER_LOW),
        Instruction::StaAbsolute(CURSOR_SOURCE_LOW),
        Instruction::LdaZeroPage(SCRATCH_POINTER_HIGH),
        Instruction::AdcImmediate(0),
        Instruction::StaAbsolute(CURSOR_SOURCE_HIGH),
        // 올린 타일 수는 읽은 바이트 수를 타일 크기로 나눈 값이다.
        Instruction::Tya,
        Instruction::LsrAccumulator,
        Instruction::LsrAccumulator,
        Instruction::LsrAccumulator,
        Instruction::StaZeroPage(SCRATCH_POINTER_LOW),
        Instruction::Clc,
        Instruction::AdcAbsolute(CURSOR_NEXT_TILE_INDEX),
        Instruction::StaAbsolute(CURSOR_NEXT_TILE_INDEX),
        Instruction::LdaAbsolute(CURSOR_REMAINING_TILES),
        Instruction::Sec,
        Instruction::SbcZeroPage(SCRATCH_POINTER_LOW),
        Instruction::StaAbsolute(CURSOR_REMAINING_TILES),
        Instruction::BneAbsolute(origin),
        Instruction::LdaImmediate(STATE_READY),
        Instruction::StaAbsolute(REQUEST_STATE),
    ]
}

pub(super) fn build_transport_routine(origin: u16) -> Result<RuntimeRoutine> {
    let (mut instructions, finished_jump) = frame_prologue(origin)?;
    let loop_start = next_address(origin, &instructions)?;
    instructions.extend(tile_body(loop_start));

    let epilogue_start_index = instructions.len();
    let epilogue_address = next_address(origin, &instructions)?;
    instructions.extend(frame_epilogue(epilogue_address));
    let done = next_address(origin, &instructions)?;
    // 아직 남았으면 준비 완료 표시를 건너뛴다.
    let pending_branch = instructions[epilogue_start_index..]
        .iter()
        .position(|instruction| matches!(instruction, Instruction::BneAbsolute(_)))
        .context("the transport epilogue lost its pending branch")?;
    instructions[epilogue_start_index + pending_branch] = Instruction::BneAbsolute(done);
    instructions[finished_jump] = Instruction::JmpAbsolute(done);
    instructions.push(Instruction::Rts);

    let bytes = assemble_at(origin, &instructions)?;
    ensure!(
        !bytes.is_empty(),
        "the dialogue transport routine assembled to nothing"
    );
    Ok(RuntimeRoutine {
        role: "dialogue transport",
        address: origin,
        bytes,
    })
}

/// 한 프레임이 최악의 경우 쓰는 사이클이다. 실제로 방출하는 명령에서 센다.
pub(super) fn worst_case_frame_cycles(origin: u16) -> Result<u32> {
    let (prologue, _) = frame_prologue(origin)?;
    let loop_start = next_address(origin, &prologue)?;
    Ok(worst_case_cycles(&prologue)
        + worst_case_cycles(&tile_body(loop_start)) * u32::from(TILES_PER_FRAME)
        + worst_case_cycles(&frame_epilogue(origin))
        + u32::from(Instruction::Rts.worst_case_cycles()))
}

#[cfg(test)]
mod tests {
    use super::*;
    use super::super::trampoline::worst_case_reserve_cycles;
    use super::super::super::runtime_bank_contract::BankRestoreContract;

    /// 훅 호출과 트램폴린이 실제로 쓰는 몫이다. 방출한 명령에서 센다.
    fn trampoline_reserve() -> u32 {
        worst_case_reserve_cycles(BankRestoreContract {
            prg_8000_register: 6,
            prg_a000_register: 7,
            prg_bank_shadow: 0x29,
            helper_reachable_page_count: 32,
        })
        .unwrap()
    }

    /// 한 프레임이 vblank를 넘지 않아야 한다. 넘으면 렌더링 중에 `$2007`을 쓰게 되고
    /// 그것은 에뮬레이터에서는 대체로 보이지 않는 실기 손상이다.
    #[test]
    fn one_frame_of_transport_fits_the_measured_vblank_remainder() {
        let allowed = super::super::budgeted_transport_cycles(trampoline_reserve());

        let worst_case = worst_case_frame_cycles(0xB000).unwrap();

        assert!(
            worst_case <= allowed,
            "one frame costs {worst_case} cycles but only {allowed} are budgeted"
        );
    }

    /// 예산을 한 타일 더 늘리면 넘친다는 것이 지금 값이 상한이라는 근거다.
    /// 이 단언이 깨지면 예산을 늘릴 여지가 생긴 것이므로 다시 유도한다.
    #[test]
    fn the_budget_is_the_largest_batch_that_still_fits() {
        let allowed = super::super::budgeted_transport_cycles(trampoline_reserve());
        let prologue = frame_prologue(0xB000).unwrap().0;
        let loop_start = next_address(0xB000, &prologue).unwrap();
        let per_tile = worst_case_cycles(&tile_body(loop_start));

        let one_more = worst_case_frame_cycles(0xB000).unwrap() + per_tile;

        assert!(
            one_more > allowed,
            "another tile would still fit; the budget is understated"
        );
    }

    /// 한 타일이 CHR에서 차지하는 만큼 정확히 쓰지 않으면 다음 타일이 밀린다.
    #[test]
    fn a_tile_expands_from_one_bitplane_to_two() {
        let body = tile_body(0xB000);
        let ppu_data_writes = body
            .iter()
            .filter(|instruction| matches!(instruction, Instruction::StaAbsolute(PPU_DATA)))
            .count();

        assert_eq!(ppu_data_writes, usize::from(CHR_TILE_BYTE_COUNT));
        assert_eq!(CHR_TILE_BYTE_COUNT, ATLAS_TILE_BYTE_COUNT * 2);
    }

    /// 다 올리기 전에 준비 완료를 알리면 아직 없는 글자가 화면에 나온다.
    #[test]
    fn readiness_is_published_only_after_the_last_tile() {
        let routine = build_transport_routine(0xB000).unwrap();
        let ready_store = [0x8D, REQUEST_STATE as u8, (REQUEST_STATE >> 8) as u8];
        let ready_at = routine
            .bytes
            .windows(3)
            .position(|window| window == ready_store)
            .expect("the routine publishes readiness");
        let last_ppu_write = routine
            .bytes
            .windows(3)
            .rposition(|window| window == [0x8D, PPU_DATA as u8, (PPU_DATA >> 8) as u8])
            .expect("the routine writes PPU data");

        assert!(ready_at > last_ppu_write);
    }

    /// 전송 루틴은 페이지 `2E` 꼬리의 예약 안에 들어가야 한다. 넘으면 앞선
    /// 직렬화 자료를 덮는다.
    #[test]
    fn the_routine_fits_the_page_tail_reservation() {
        /// 자료 배치가 실행 코드에 남겨 두기로 한 하한이다.
        const MINIMUM_RUNTIME_CODE_RESERVATION: usize = 1_888;

        let routine = build_transport_routine(0xB000).unwrap();

        assert!(
            routine.bytes.len() <= MINIMUM_RUNTIME_CODE_RESERVATION,
            "the transport routine is {} bytes",
            routine.bytes.len()
        );
    }

    /// 남은 타일이 0인 프레임은 PPU를 건드리지 않고 곧바로 돌아가야 한다.
    #[test]
    fn an_empty_request_returns_before_touching_the_ppu() {
        let (prologue, finished_jump) = frame_prologue(0xB000).unwrap();

        let ppu_writes_before_branch = prologue[..finished_jump]
            .iter()
            .filter(|instruction| {
                matches!(
                    instruction,
                    Instruction::StaAbsolute(PPU_DATA) | Instruction::StaAbsolute(PPU_ADDRESS)
                )
            })
            .count();

        assert_eq!(ppu_writes_before_branch, 0);
    }
}
