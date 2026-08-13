//! 실행 코드 페이지에 놓이는 전송 루틴이다.
//!
//! 한 프레임에 정해진 수의 타일만 CHR RAM으로 올리고 커서를 남긴다. 그 수는
//! `$C179`의 vblank 잔여 1,704사이클에서 안전 여유 20%를 뺀 값 안에 실제 코드의
//! 최악 사이클이 들어가도록 정한다. 의사결정 64번을 따른다.
//!
//! 옮기는 것은 그룹 덩이의 항목이다. 항목은 `[코드][atlas 주소 하위][atlas 주소 상위]`
//! 세 바이트이고, 빌드가 주소를 미리 더해 두어 소비자는 계산을 하지 않는다.
//!
//! 한 타일에 `$8000` 창을 두 번 바꾼다. 항목은 그룹 덩이 페이지에, 타일 자료는
//! atlas 페이지에 있고 두 페이지를 동시에 걸 수 없기 때문이다.
//!
//! **아직 닫히지 않은 것.** 이 루틴이 `$2007`에 쓰는 동안 CHR RAM이 두 CHR 창에
//! 걸려 있어야 하는데 지금은 걸지 않는다. 그래서 실행하면 쓰기가 CHR ROM으로 가
//! 버려진다. 실행으로 확인했다 — 206항목을 정확히 걷고 `ready`까지 갔지만 CHR RAM은
//! 그대로 0이었다.
//!
//! 거는 것 자체는 레지스터 네 번 쓰기로 끝난다. 문제는 **되돌리기**다. 되돌리지
//! 않으면 반쯤 합성된 CHR RAM이 화면에 나와 안전 성질이 깨지는데, 되돌릴 값을 아는
//! 것은 원본 도우미 `$FA80`·`$FAA0`뿐이고 그 비용이 아직 측정되지 않았다. 모르는
//! 비용을 예산에 넣지 않는다는 것이 의사결정 62번이므로, 측정 전까지는 걸지 않는다.
//!
//! atlas는 타일당 8바이트 1bpp다. CHR에는 16바이트 2bpp로 펼치고 상위 bitplane은
//! 0으로 채운다. 상위 bitplane이 전부 0이라는 것은 직렬화 시점에 검사돼 있다.

use anyhow::{Context, Result, ensure};

use super::super::runtime_cursor_storage::{
    CURSOR_ENTRY_HIGH, CURSOR_ENTRY_LOW, CURSOR_GROUP_PAGE, CURSOR_REMAINING_TILES,
};
use super::{RuntimeRoutine, next_address, worst_case_cycles};
use crate::rp2a03::{Instruction, assemble_at};

/// 한 프레임에 올리는 타일 수다. 사이클 예산에서 유도한 값이므로 늘리려면
/// 아래 예산 시험이 먼저 통과해야 한다.
pub(in crate::full_translation_install) const TILES_PER_FRAME: u8 = 4;
/// 그룹 덩이 항목 하나의 크기다. 코드 하나와 atlas CPU 주소 둘이다.
const GROUP_BLOCK_ENTRY_BYTE_COUNT: u8 = 3;
/// atlas가 타일 하나에 쓰는 바이트다. 1bpp 8×8.
pub(super) const ATLAS_TILE_BYTE_COUNT: u8 = 8;
/// 타일 하나가 CHR에서 차지하는 바이트다. 2bpp 8×8.
pub(super) const CHR_TILE_BYTE_COUNT: u8 = ATLAS_TILE_BYTE_COUNT * 2;

/// 요청 상태 바이트다. 생산자가 쓰고 소비자가 지운다.
pub(in crate::full_translation_install) const REQUEST_STATE: u16 = 0x07F4;
/// 합성이 끝나 출력해도 되는 상태다.
pub(super) const STATE_READY: u8 = 3;

/// CHR RAM이 PPU 주소 공간에서 시작하는 자리다.
const CHR_RAM_BASE: u16 = 0x1000;
const PPU_STATUS: u16 = 0x2002;
const PPU_ADDRESS: u16 = 0x2006;
const PPU_DATA: u16 = 0x2007;
const BANK_SELECT_REGISTER: u16 = 0x8000;
const BANK_VALUE_REGISTER: u16 = 0x8001;
const PRG_8000_REGISTER: u8 = 6;

/// NMI 프롤로그 `$C173`~`$C178`이 스택에 밀어 둔 제로 페이지다. 소비자가 써도 된다.
const ENTRY_POINTER_LOW: u8 = 0x00;
const ENTRY_POINTER_HIGH: u8 = 0x01;
/// 소비자가 진입에서 밀고 이탈에서 되돌려 빌려 쓰는 제로 페이지다. 전투 합성이
/// 쓰는 방식과 같다. 밀고 되돌리면 안전이 증명이 아니라 구조로 성립한다.
const BORROWED_SCRATCH: [u8; 4] = [0x02, 0x03, 0x04, 0x05];
const ATLAS_POINTER_LOW: u8 = BORROWED_SCRATCH[0];
const ATLAS_POINTER_HIGH: u8 = BORROWED_SCRATCH[1];
const CURRENT_CODE: u8 = BORROWED_SCRATCH[2];
const BATCH_SIZE: u8 = BORROWED_SCRATCH[3];

/// `$8000` 창에 페이지 하나를 건다.
fn map_data_page(page: Instruction) -> [Instruction; 4] {
    [
        Instruction::LdaImmediate(PRG_8000_REGISTER),
        Instruction::StaAbsolute(BANK_SELECT_REGISTER),
        page,
        Instruction::StaAbsolute(BANK_VALUE_REGISTER),
    ]
}

/// 프레임 시작에서 커서를 읽고 이번 몫을 정한다.
///
/// 두 번째 값은 «할 일이 없을 때 뛰어넘는 `JMP`»의 색인이다. 상대 분기로는 루틴
/// 끝까지 닿지 못하므로 짧은 분기로 `JMP` 하나를 건너뛰는 형태를 쓴다.
fn frame_prologue(origin: u16) -> Result<(Vec<Instruction>, usize)> {
    let mut instructions = vec![Instruction::LdaAbsolute(CURSOR_REMAINING_TILES)];
    let has_work_placeholder = instructions.len();
    instructions.push(Instruction::BneAbsolute(origin));
    let finished_jump = instructions.len();
    instructions.push(Instruction::JmpAbsolute(origin));
    let has_work = next_address(origin, &instructions)?;
    instructions[has_work_placeholder] = Instruction::BneAbsolute(has_work);

    // 빌린 제로 페이지를 밀어 둔다.
    for address in BORROWED_SCRATCH {
        instructions.extend([Instruction::LdaZeroPage(address), Instruction::Pha]);
    }

    // 이번 프레임 몫은 남은 것과 예산 중 작은 쪽이다.
    instructions.extend([
        Instruction::LdaAbsolute(CURSOR_REMAINING_TILES),
        Instruction::CmpImmediate(TILES_PER_FRAME),
    ]);
    let use_remaining_placeholder = instructions.len();
    instructions.push(Instruction::BccAbsolute(origin));
    instructions.push(Instruction::LdaImmediate(TILES_PER_FRAME));
    let batch_selected = next_address(origin, &instructions)?;
    instructions[use_remaining_placeholder] = Instruction::BccAbsolute(batch_selected);
    instructions.extend([
        Instruction::Tax,
        Instruction::StxZeroPage(BATCH_SIZE),
        // 남은 수는 지금 줄여 둔다. 루프는 이 값을 읽지 않으므로 나중에 다시 셀
        // 이유가 없고, 여기서 줄여 두면 커서 저장이 한 곳으로 모인다.
        Instruction::LdaAbsolute(CURSOR_REMAINING_TILES),
        Instruction::Sec,
        Instruction::SbcZeroPage(BATCH_SIZE),
        Instruction::StaAbsolute(CURSOR_REMAINING_TILES),
        // 항목 포인터를 제로 페이지에 세운다.
        Instruction::LdaAbsolute(CURSOR_ENTRY_LOW),
        Instruction::StaZeroPage(ENTRY_POINTER_LOW),
        Instruction::LdaAbsolute(CURSOR_ENTRY_HIGH),
        Instruction::StaZeroPage(ENTRY_POINTER_HIGH),
    ]);
    Ok((instructions, finished_jump))
}

/// 타일 하나를 올리는 몸통이다. 길이가 고정이라 자료가 반복 횟수를 늘릴 수 없다.
fn tile_body(loop_start: u16, atlas_page: u8) -> Result<Vec<Instruction>> {
    let mut instructions = Vec::new();
    // 항목을 읽으려면 그룹 덩이 페이지가 걸려 있어야 한다.
    instructions.extend(map_data_page(Instruction::LdaAbsolute(CURSOR_GROUP_PAGE)));
    instructions.extend([
        Instruction::LdyImmediate(0),
        Instruction::LdaIndirectY(ENTRY_POINTER_LOW),
        Instruction::StaZeroPage(CURRENT_CODE),
        Instruction::Iny,
        Instruction::LdaIndirectY(ENTRY_POINTER_LOW),
        Instruction::StaZeroPage(ATLAS_POINTER_LOW),
        Instruction::Iny,
        Instruction::LdaIndirectY(ENTRY_POINTER_LOW),
        Instruction::StaZeroPage(ATLAS_POINTER_HIGH),
        // 다음 항목으로 옮긴다.
        Instruction::Clc,
        Instruction::LdaZeroPage(ENTRY_POINTER_LOW),
        Instruction::AdcImmediate(GROUP_BLOCK_ENTRY_BYTE_COUNT),
        Instruction::StaZeroPage(ENTRY_POINTER_LOW),
        Instruction::LdaZeroPage(ENTRY_POINTER_HIGH),
        Instruction::AdcImmediate(0),
        Instruction::StaZeroPage(ENTRY_POINTER_HIGH),
        // 목적지 PPU 주소는 코드에서 나온다. `$1000 + code × 16`이다.
        Instruction::LdaAbsolute(PPU_STATUS),
        Instruction::LdaZeroPage(CURRENT_CODE),
        Instruction::LsrAccumulator,
        Instruction::LsrAccumulator,
        Instruction::LsrAccumulator,
        Instruction::LsrAccumulator,
        Instruction::Clc,
        Instruction::AdcImmediate((CHR_RAM_BASE >> 8) as u8),
        Instruction::StaAbsolute(PPU_ADDRESS),
        Instruction::LdaZeroPage(CURRENT_CODE),
        Instruction::AslAccumulator,
        Instruction::AslAccumulator,
        Instruction::AslAccumulator,
        Instruction::AslAccumulator,
        Instruction::StaAbsolute(PPU_ADDRESS),
    ]);
    // 타일 자료를 읽으려면 atlas 페이지가 걸려 있어야 한다.
    instructions.extend(map_data_page(Instruction::LdaImmediate(atlas_page)));
    instructions.push(Instruction::LdyImmediate(0));
    for _ in 0..ATLAS_TILE_BYTE_COUNT {
        instructions.extend([
            Instruction::LdaIndirectY(ATLAS_POINTER_LOW),
            Instruction::StaAbsolute(PPU_DATA),
            Instruction::Iny,
        ]);
    }
    // 상위 bitplane은 전부 0이다.
    instructions.push(Instruction::LdaImmediate(0));
    for _ in 0..ATLAS_TILE_BYTE_COUNT {
        instructions.push(Instruction::StaAbsolute(PPU_DATA));
    }
    // 몸통이 상대 분기 사거리보다 길다. 뒤로 돌아가는 분기를 쓸 수 없으므로 조건을
    // 뒤집어 `JMP` 하나를 건너뛴다. 탈출 자리는 그 `JMP` 바로 뒤다.
    instructions.push(Instruction::Dex);
    let branch = next_address(loop_start, &instructions)?;
    let exit = branch
        .checked_add(2 + 3)
        .context("transport tile body exit address overflow")?;
    instructions.extend([
        Instruction::BeqAbsolute(exit),
        Instruction::JmpAbsolute(loop_start),
    ]);
    Ok(instructions)
}

/// 커서를 저장하고, 빌린 제로 페이지를 되돌리고, 다 올렸으면 준비 완료를 알린다.
fn frame_epilogue(pending_placeholder: u16) -> Vec<Instruction> {
    let mut instructions = vec![
        Instruction::LdaZeroPage(ENTRY_POINTER_LOW),
        Instruction::StaAbsolute(CURSOR_ENTRY_LOW),
        Instruction::LdaZeroPage(ENTRY_POINTER_HIGH),
        Instruction::StaAbsolute(CURSOR_ENTRY_HIGH),
    ];
    // 민 순서의 반대로 되돌린다.
    for address in BORROWED_SCRATCH.iter().rev() {
        instructions.extend([Instruction::Pla, Instruction::StaZeroPage(*address)]);
    }
    instructions.extend([
        Instruction::LdaAbsolute(CURSOR_REMAINING_TILES),
        Instruction::BneAbsolute(pending_placeholder),
        Instruction::LdaImmediate(STATE_READY),
        Instruction::StaAbsolute(REQUEST_STATE),
    ]);
    instructions
}

pub(super) fn build_transport_routine(origin: u16, atlas_page: u8) -> Result<RuntimeRoutine> {
    let (mut instructions, finished_jump) = frame_prologue(origin)?;
    let loop_start = next_address(origin, &instructions)?;
    instructions.extend(tile_body(loop_start, atlas_page)?);

    let epilogue_start_index = instructions.len();
    let epilogue_address = next_address(origin, &instructions)?;
    instructions.extend(frame_epilogue(epilogue_address));
    let done = next_address(origin, &instructions)?;
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
pub(super) fn worst_case_frame_cycles(origin: u16, atlas_page: u8) -> Result<u32> {
    let (prologue, _) = frame_prologue(origin)?;
    let loop_start = next_address(origin, &prologue)?;
    Ok(worst_case_cycles(&prologue)?
        + worst_case_cycles(&tile_body(loop_start, atlas_page)?)? * u32::from(TILES_PER_FRAME)
        + worst_case_cycles(&frame_epilogue(origin))?
        + u32::from(Instruction::Rts.worst_case_cycles()))
}

#[cfg(test)]
mod tests {
    use super::super::super::runtime_bank_contract::BankRestoreContract;
    use super::super::trampoline::worst_case_reserve_cycles;
    use super::*;

    const ATLAS_PAGE: u8 = 0x2C;

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

        let worst_case = worst_case_frame_cycles(0xA000, ATLAS_PAGE).unwrap();

        assert!(
            worst_case <= allowed,
            "one frame costs {worst_case} cycles but only {allowed} are budgeted"
        );
    }

    /// 예산을 한 타일 더 늘리면 넘친다는 것이 지금 값이 상한이라는 근거다.
    #[test]
    fn the_budget_is_the_largest_batch_that_still_fits() {
        let allowed = super::super::budgeted_transport_cycles(trampoline_reserve());
        let prologue = frame_prologue(0xA000).unwrap().0;
        let loop_start = next_address(0xA000, &prologue).unwrap();
        let per_tile = worst_case_cycles(&tile_body(loop_start, ATLAS_PAGE).unwrap()).unwrap();

        let one_more = worst_case_frame_cycles(0xA000, ATLAS_PAGE).unwrap() + per_tile;

        assert!(
            one_more > allowed,
            "another tile would still fit; the budget is understated"
        );
    }

    /// 한 타일이 CHR에서 차지하는 만큼 정확히 쓰지 않으면 다음 타일이 밀린다.
    #[test]
    fn a_tile_expands_from_one_bitplane_to_two() {
        let body = tile_body(0xA000, ATLAS_PAGE).unwrap();
        let ppu_data_writes = body
            .iter()
            .filter(|instruction| matches!(instruction, Instruction::StaAbsolute(PPU_DATA)))
            .count();

        assert_eq!(ppu_data_writes, usize::from(CHR_TILE_BYTE_COUNT));
    }

    /// 빌린 제로 페이지는 민 순서의 반대로 되돌아가야 한다. 순서가 어긋나면 원본이
    /// 남의 값을 자기 것으로 읽는다.
    #[test]
    fn borrowed_zero_page_is_restored_in_reverse_order() {
        let (prologue, _) = frame_prologue(0xA000).unwrap();
        let pushed: Vec<u8> = prologue
            .iter()
            .filter_map(|instruction| match instruction {
                Instruction::LdaZeroPage(address) if BORROWED_SCRATCH.contains(address) => {
                    Some(*address)
                }
                _ => None,
            })
            .collect();
        let restored: Vec<u8> = frame_epilogue(0xA000)
            .iter()
            .filter_map(|instruction| match instruction {
                Instruction::StaZeroPage(address) if BORROWED_SCRATCH.contains(address) => {
                    Some(*address)
                }
                _ => None,
            })
            .collect();

        assert_eq!(pushed, BORROWED_SCRATCH);
        assert_eq!(
            restored,
            BORROWED_SCRATCH.iter().rev().copied().collect::<Vec<_>>()
        );
    }

    /// 항목은 그룹 덩이 페이지에서, 타일 자료는 atlas 페이지에서 읽어야 한다.
    /// 한쪽만 걸면 다른 쪽이 남의 자료를 읽는다.
    #[test]
    fn each_tile_maps_the_block_page_before_the_entry_and_the_atlas_page_before_the_copy() {
        let body = tile_body(0xA000, ATLAS_PAGE).unwrap();
        let position = |wanted: Instruction| {
            body.iter()
                .position(|instruction| *instruction == wanted)
                .unwrap_or_else(|| panic!("the body is missing {wanted:?}"))
        };

        let block_map = position(Instruction::LdaAbsolute(CURSOR_GROUP_PAGE));
        let entry_read = position(Instruction::LdaIndirectY(ENTRY_POINTER_LOW));
        let atlas_map = position(Instruction::LdaImmediate(ATLAS_PAGE));
        let atlas_read = position(Instruction::LdaIndirectY(ATLAS_POINTER_LOW));

        assert!(block_map < entry_read);
        assert!(entry_read < atlas_map);
        assert!(atlas_map < atlas_read);
    }

    /// 다 올리기 전에 준비 완료를 알리면 아직 없는 글자가 화면에 나온다.
    #[test]
    fn readiness_is_published_only_after_the_last_tile() {
        let routine = build_transport_routine(0xA000, ATLAS_PAGE).unwrap();
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

    /// 전송 루틴은 실행 코드 페이지 안에 들어가야 한다.
    #[test]
    fn the_routine_fits_the_runtime_code_page() {
        let routine = build_transport_routine(0xA000, ATLAS_PAGE).unwrap();

        assert!(
            routine.bytes.len() <= 8 * 1024,
            "the transport routine is {} bytes",
            routine.bytes.len()
        );
    }

    /// 예산은 불려 가는 코드의 비용을 모르면 세지 않는다. 그것이 «모르는 것을
    /// 6사이클이라고 세지 않는다»는 규칙이고, vblank에서 과소평가는 실기 손상이다.
    #[test]
    fn a_call_with_an_unmeasured_callee_is_refused_by_the_cycle_budget() {
        let error = super::super::worst_case_cycles(&[Instruction::JsrAbsolute(0xFA80)])
            .unwrap_err();

        assert!(error.to_string().contains("must be measured"));
    }

    /// 남은 타일이 0인 프레임은 PPU를 건드리지 않고 곧바로 돌아가야 한다.
    #[test]
    fn an_empty_request_returns_before_touching_the_ppu() {
        let (prologue, finished_jump) = frame_prologue(0xA000).unwrap();

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
