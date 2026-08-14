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
//! 프레임 시작에서 CHR RAM을 두 CHR 창에 걸고 끝에서 되돌린다. 걸지 않으면 `$2007`
//! 쓰기가 CHR ROM으로 가서 버려진다 — 실행으로 확인했다. 되돌리지 않으면 아직 다
//! 올라가지 않은 CHR RAM이 화면에 나와 안전 성질이 깨진다. 둘 다 vblank 안이라
//! 렌더링은 그 사이를 보지 못한다.
//!
//! 합성은 두 단계다. 먼저 원본 배경 페이지 4 KiB를 CHR RAM으로 복원하고, 그 위에
//! 그 그룹의 한글 타일을 덮는다. 복원이 필요한 이유는 맵 타일과 대사 글꼴이 같은
//! 4 KiB 페이지 안에 함께 있기 때문이다 — 덮기만 하면 맵이 사라진다. 실행으로
//! 확인했다.
//!
//! 복원의 원본은 PRG 페이지 `21`이다. CHR ROM은 CPU가 읽을 수 없으므로 빌드가 원본
//! 페이지를 PRG에 복제해 두었고, 그것이 원본 글꼴과 바이트가 같다는 것은 설치가
//! 매번 확인한다.
//!
//! PRG `21`의 4 KiB는 원본 대사 글꼴이 있는 **FD 원천 페이지 0**과 같다. 실행에서
//! 맵 타일이 한글 조각으로 바뀐 원인은 이 복원 페이지가 아니라, 표시 selector가 FD와
//! FE를 모두 같은 CHR RAM 페이지로 바꿔 래치 두 네임스페이스를 합친 것이었다.
//! 전송 중에는 현재 래치와 무관하게 쓰기가 RAM에 닿도록 두 레지스터를 잠시 RAM으로
//! 고르지만, 이탈에서 둘을 원천으로 되돌린다. 준비 완료 뒤 표시 selector는 FD만 RAM을
//! 보게 하고 FE 배경은 원본 CHR ROM에 남긴다.
//!
//! 되돌릴 원천 페이지는 mapper165 중앙 기록기가 이미 `$5B`(FD)와 `$5C`(FE)에 따로
//! 보존한다. 직접 기록기는 의도적으로 그 상태를 바꾸지 않으므로 설정기 훅으로
//! 관측하지 않는다. 소비자는 두 값을 읽기만 하고 `$52`의 상위 비트를 합친 뒤 원본
//! stateless 설정기 `$FA80`·`$FAA0`으로 각 창을 되돌린다.
//!
//! atlas는 타일당 8바이트 1bpp다. CHR에는 16바이트 2bpp로 펼치고 상위 bitplane은
//! 0으로 채운다. 상위 bitplane이 전부 0이라는 것은 직렬화 시점에 검사돼 있다.

use anyhow::{Context, Result, ensure};

use super::super::runtime_cursor_storage::{
    CURSOR_ENTRY_HIGH, CURSOR_ENTRY_LOW, CURSOR_GROUP_PAGE, CURSOR_OVERLAY_TILES, CURSOR_PHASE,
    CURSOR_REMAINING_TILES, PUBLISHED_SOURCE_DIRECTORY_SELECTOR, PUBLISHED_SOURCE_ENTRY_INDEX,
    REQUEST_SOURCE_DIRECTORY_SELECTOR, REQUEST_SOURCE_ENTRY_INDEX,
};
use super::{RuntimeRoutine, next_address, worst_case_cycles, worst_case_cycles_with_calls};
use crate::rp2a03::{Instruction, assemble_at};

/// 한 프레임에 올리는 타일 수다. 사이클 예산에서 유도한 값이므로 늘리려면
/// 아래 예산 시험이 먼저 통과해야 한다.
pub(in crate::full_translation_install) const TILES_PER_FRAME: u8 = 2;
/// 복원 단계가 한 번에 옮기는 덩어리의 크기다.
const RESTORE_CHUNK_BYTE_COUNT: u8 = 32;
/// 4 KiB 페이지를 그 크기로 나눈 덩어리 수다.
pub(in crate::full_translation_install) const RESTORE_CHUNK_COUNT: u8 = 128;
/// 한 프레임에 옮기는 덩어리 수다. 타일 수와 같은 예산에서 따로 유도한다.
const RESTORE_CHUNKS_PER_FRAME: u8 = 1;
/// 원본 배경 페이지를 복제해 둔 PRG 페이지다.
pub(in crate::full_translation_install) const SOURCE_PAGE_MMC3_PAGE: u8 = 0x21;
/// 복원 단계를 뜻하는 값이다.
pub(in crate::full_translation_install) const PHASE_RESTORE: u8 = 0;
/// 덮기 단계를 뜻하는 값이다.
pub(in crate::full_translation_install) const PHASE_OVERLAY: u8 = 1;
/// 그룹 덩이 항목 하나의 크기다. 코드 하나와 atlas CPU 주소 둘이다.
const GROUP_BLOCK_ENTRY_BYTE_COUNT: u8 = 3;
/// atlas가 타일 하나에 쓰는 바이트다. 1bpp 8×8.
pub(super) const ATLAS_TILE_BYTE_COUNT: u8 = 8;
/// 타일 하나가 CHR에서 차지하는 바이트다. 2bpp 8×8.
pub(super) const CHR_TILE_BYTE_COUNT: u8 = ATLAS_TILE_BYTE_COUNT * 2;

/// 요청 상태 바이트다. 생산자가 쓰고 소비자가 지운다.
pub(in crate::full_translation_install) use super::super::runtime_state_storage::REQUEST_STATE;
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
/// 매퍼 165가 4 KiB CHR 창 둘에 쓰는 MMC3 레지스터다.
const CHR_BANK_REGISTERS: [u8; 2] = [2, 4];
/// CHR RAM을 고르는 뱅크 값이다. CHR ROM의 물리 페이지 0은 다른 값으로 인코딩된다.
const CHR_RAM_BANK_VALUE: u8 = super::chr_source_state::CHR_RAM_BANK_VALUE;
/// 되돌릴 중앙 원천 상태와 stateless 설정기의 짝이다. FD와 FE는 같은 값이라고
/// 가정하지 않는다.
const CHR_RESTORE_PATHS: [(u8, u16); 2] = [
    (
        super::chr_source_state::RIGHT_FD_SOURCE_SHADOW,
        super::chr_source_state::RIGHT_FD_HELPER,
    ),
    (
        super::chr_source_state::RIGHT_FE_SOURCE_SHADOW,
        super::chr_source_state::RIGHT_FE_HELPER,
    ),
];
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

    // 이번 프레임 몫은 남은 것과 단계별 예산 중 작은 쪽이다.
    instructions.extend([
        Instruction::LdaAbsolute(CURSOR_PHASE),
        Instruction::BneAbsolute(origin),
    ]);
    let overlay_budget_placeholder = instructions.len() - 1;
    instructions.push(Instruction::LdaImmediate(RESTORE_CHUNKS_PER_FRAME));
    let budget_chosen_placeholder = instructions.len();
    instructions.push(Instruction::JmpAbsolute(origin));
    let overlay_budget = next_address(origin, &instructions)?;
    instructions[overlay_budget_placeholder] = Instruction::BneAbsolute(overlay_budget);
    instructions.push(Instruction::LdaImmediate(TILES_PER_FRAME));
    let budget_chosen = next_address(origin, &instructions)?;
    instructions[budget_chosen_placeholder] = Instruction::JmpAbsolute(budget_chosen);
    instructions.extend([
        Instruction::StaZeroPage(BATCH_SIZE),
        Instruction::LdaAbsolute(CURSOR_REMAINING_TILES),
        Instruction::CmpZeroPage(BATCH_SIZE),
    ]);
    let use_remaining_placeholder = instructions.len();
    instructions.push(Instruction::BccAbsolute(origin));
    instructions.push(Instruction::LdaZeroPage(BATCH_SIZE));
    let batch_selected = next_address(origin, &instructions)?;
    instructions[use_remaining_placeholder] = Instruction::BccAbsolute(batch_selected);
    instructions.extend([
        Instruction::Tax,
        Instruction::StxZeroPage(BATCH_SIZE),
        // 항목 포인터를 제로 페이지에 세운다. 복원 단계는 쓰지 않지만 세워 두어도
        // 해가 없고, 두 단계가 같은 프롤로그를 쓰면 진입이 하나로 남는다.
        Instruction::LdaAbsolute(CURSOR_ENTRY_LOW),
        Instruction::StaZeroPage(ENTRY_POINTER_LOW),
        Instruction::LdaAbsolute(CURSOR_ENTRY_HIGH),
        Instruction::StaZeroPage(ENTRY_POINTER_HIGH),
    ]);
    // CHR RAM을 두 창에 건다. 걸지 않으면 `$2007` 쓰기가 CHR ROM으로 간다.
    for register in CHR_BANK_REGISTERS {
        instructions.extend([
            Instruction::LdaImmediate(register),
            Instruction::StaAbsolute(BANK_SELECT_REGISTER),
            Instruction::LdaImmediate(CHR_RAM_BANK_VALUE),
            Instruction::StaAbsolute(BANK_VALUE_REGISTER),
        ]);
    }
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
    instructions.extend([
        Instruction::DecAbsolute(CURSOR_REMAINING_TILES),
        Instruction::Dex,
    ]);
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

/// 원본 페이지에서 한 덩어리를 옮긴다.
///
/// 원본 주소도 목적지 주소도 «몇 덩어리 남았나»에서 나오므로 커서에 포인터를 담지
/// 않는다. 덮기 단계의 커서를 건드리지 않아야 복원이 끝난 뒤 그대로 이어받는다.
fn restore_body(loop_start: u16) -> Result<Vec<Instruction>> {
    let mut instructions = vec![
        // 이미 옮긴 덩어리 수를 만든다.
        Instruction::LdaImmediate(RESTORE_CHUNK_COUNT),
        Instruction::Sec,
        Instruction::SbcAbsolute(CURSOR_REMAINING_TILES),
        Instruction::StaZeroPage(CURRENT_CODE),
        // 원본 주소는 `$8000 + done × 32`다.
        Instruction::AslAccumulator,
        Instruction::AslAccumulator,
        Instruction::AslAccumulator,
        Instruction::AslAccumulator,
        Instruction::AslAccumulator,
        Instruction::StaZeroPage(ATLAS_POINTER_LOW),
        Instruction::LdaZeroPage(CURRENT_CODE),
        Instruction::LsrAccumulator,
        Instruction::LsrAccumulator,
        Instruction::LsrAccumulator,
        Instruction::Clc,
        Instruction::AdcImmediate(0x80),
        Instruction::StaZeroPage(ATLAS_POINTER_HIGH),
        // 목적지는 `$1000 + done × 32`다. 상위 바이트만 다르다.
        Instruction::LdaAbsolute(PPU_STATUS),
        Instruction::LdaZeroPage(ATLAS_POINTER_HIGH),
        Instruction::Sec,
        Instruction::SbcImmediate(0x80 - (CHR_RAM_BASE >> 8) as u8),
        Instruction::StaAbsolute(PPU_ADDRESS),
        Instruction::LdaZeroPage(ATLAS_POINTER_LOW),
        Instruction::StaAbsolute(PPU_ADDRESS),
        Instruction::LdyImmediate(0),
    ];
    for _ in 0..RESTORE_CHUNK_BYTE_COUNT {
        instructions.extend([
            Instruction::LdaIndirectY(ATLAS_POINTER_LOW),
            Instruction::StaAbsolute(PPU_DATA),
            Instruction::Iny,
        ]);
    }
    instructions.extend([
        Instruction::DecAbsolute(CURSOR_REMAINING_TILES),
        Instruction::Dex,
    ]);
    let branch = next_address(loop_start, &instructions)?;
    let exit = branch
        .checked_add(2 + 3)
        .context("transport restore body exit address overflow")?;
    instructions.extend([
        Instruction::BeqAbsolute(exit),
        Instruction::JmpAbsolute(loop_start),
    ]);
    Ok(instructions)
}

/// 커서를 저장하고, 빌린 제로 페이지를 되돌리고, 단계를 넘기거나 준비 완료를 알린다.
///
/// 돌려주는 색인들은 «루틴 끝»을 가리켜야 하는 분기들이다. 끝 주소는 이 조각의 길이가
/// 정해진 뒤에야 나오므로 부르는 쪽이 되메운다.
fn frame_epilogue(
    origin: u16,
    cold_request_mapper_register: u8,
) -> Result<(Vec<Instruction>, Vec<usize>)> {
    let mut instructions = vec![
        Instruction::LdaZeroPage(ENTRY_POINTER_LOW),
        Instruction::StaAbsolute(CURSOR_ENTRY_LOW),
        Instruction::LdaZeroPage(ENTRY_POINTER_HIGH),
        Instruction::StaAbsolute(CURSOR_ENTRY_HIGH),
    ];
    // CHR 뱅크를 원본이 기대하는 값으로 되돌린다. 되돌리지 않으면 아직 다 올라가지
    // 않은 CHR RAM이 다음 프레임 렌더링에 그대로 나온다.
    for (source_shadow, helper) in CHR_RESTORE_PATHS {
        instructions.extend([
            Instruction::LdaZeroPage(source_shadow),
            Instruction::OraZeroPage(super::chr_source_state::CHR_SOURCE_HIGH_BITS),
            Instruction::JsrAbsolute(helper),
        ]);
    }
    // 민 순서의 반대로 되돌린다.
    for address in BORROWED_SCRATCH.iter().rev() {
        instructions.extend([Instruction::Pla, Instruction::StaZeroPage(*address)]);
    }

    // 전송 시작에서 잠시 RAM으로 바꾼 창을 각 원천 그림자대로 복원한 뒤 같은 vblank
    // 안에서 냉간 표시 페이지를 다시 고르므로 렌더링은 중간 상태를 보지 않는다.
    // 그림자는 이 프레임의 복귀값이지 전송 자격 조건이 아니다. 중앙 selector가 같은
    // 프레임 뒤쪽에서 다음 표시 원천을 갱신할 수 있기 때문이다.
    instructions.extend([
        Instruction::LdaImmediate(super::chr_source_state::RIGHT_FD_CHR_REGISTER),
        Instruction::StaAbsolute(BANK_SELECT_REGISTER),
        Instruction::LdaImmediate(cold_request_mapper_register),
        Instruction::StaAbsolute(BANK_VALUE_REGISTER),
    ]);

    let mut needs_done = Vec::new();
    instructions.push(Instruction::LdaAbsolute(CURSOR_REMAINING_TILES));
    needs_done.push(instructions.len());
    instructions.push(Instruction::BneAbsolute(origin));

    // 이번 단계가 끝났다. 복원이었으면 덮기로 넘어가고, 덮기였으면 준비 완료다.
    instructions.push(Instruction::LdaAbsolute(CURSOR_PHASE));
    let publish_placeholder = instructions.len();
    instructions.push(Instruction::BneAbsolute(origin));
    instructions.extend([
        Instruction::LdaImmediate(PHASE_OVERLAY),
        Instruction::StaAbsolute(CURSOR_PHASE),
        Instruction::LdaAbsolute(CURSOR_OVERLAY_TILES),
        Instruction::StaAbsolute(CURSOR_REMAINING_TILES),
    ]);
    needs_done.push(instructions.len());
    instructions.push(Instruction::JmpAbsolute(origin));

    let publish = next_address(origin, &instructions)?;
    instructions[publish_placeholder] = Instruction::BneAbsolute(publish);
    // FE는 위에서 원본으로 복원한 채 두고, FD만 완성된 RAM으로 게시한다. 같은 NMI
    // 안에서 하드웨어 선택이 끝난 뒤에만 `ready`를 공개한다.
    instructions.extend([
        // 전송 커서는 이제 필요 없다. `ready`를 게시하기 전에 같은 두 칸을 이
        // 완성 페이지의 원문 정체성으로 바꿔 반복 생산자가 재사용할 수 있게 한다.
        Instruction::LdaAbsolute(REQUEST_SOURCE_DIRECTORY_SELECTOR),
        Instruction::StaAbsolute(PUBLISHED_SOURCE_DIRECTORY_SELECTOR),
        Instruction::LdaAbsolute(REQUEST_SOURCE_ENTRY_INDEX),
        Instruction::StaAbsolute(PUBLISHED_SOURCE_ENTRY_INDEX),
        Instruction::LdaImmediate(super::chr_source_state::RIGHT_FD_CHR_REGISTER),
        Instruction::StaAbsolute(BANK_SELECT_REGISTER),
        Instruction::LdaImmediate(CHR_RAM_BANK_VALUE),
        Instruction::StaAbsolute(BANK_VALUE_REGISTER),
        Instruction::LdaImmediate(STATE_READY),
        Instruction::StaAbsolute(REQUEST_STATE),
    ]);
    needs_done.push(instructions.len());
    instructions.push(Instruction::JmpAbsolute(origin));

    Ok((instructions, needs_done))
}

pub(super) fn build_transport_routine(
    origin: u16,
    atlas_page: u8,
    cold_request_mapper_register: u8,
) -> Result<RuntimeRoutine> {
    let (mut instructions, finished_jump) = frame_prologue(origin)?;
    // 단계에 따라 다른 루프로 간다. 복원 몸통이 상대 분기 사거리보다 길어 조건을
    // 뒤집어 `JMP` 하나를 건너뛴다.
    instructions.push(Instruction::LdaAbsolute(CURSOR_PHASE));
    let restore_branch = next_address(origin, &instructions)?;
    let restore_here = restore_branch
        .checked_add(2 + 3)
        .context("transport phase branch address overflow")?;
    instructions.push(Instruction::BeqAbsolute(restore_here));
    let overlay_placeholder = instructions.len();
    instructions.push(Instruction::JmpAbsolute(origin));

    instructions.extend(map_data_page(Instruction::LdaImmediate(
        SOURCE_PAGE_MMC3_PAGE,
    )));
    let restore_start = next_address(origin, &instructions)?;
    instructions.extend(restore_body(restore_start)?);
    let after_restore_placeholder = instructions.len();
    instructions.push(Instruction::JmpAbsolute(origin));

    let overlay_start = next_address(origin, &instructions)?;
    instructions[overlay_placeholder] = Instruction::JmpAbsolute(overlay_start);
    instructions.extend(tile_body(overlay_start, atlas_page)?);

    let epilogue_start_index = instructions.len();
    let epilogue_address = next_address(origin, &instructions)?;
    instructions[after_restore_placeholder] = Instruction::JmpAbsolute(epilogue_address);
    let (epilogue, needs_done) = frame_epilogue(epilogue_address, cold_request_mapper_register)?;
    instructions.extend(epilogue);
    let done = next_address(origin, &instructions)?;
    for index in needs_done {
        let slot = epilogue_start_index + index;
        instructions[slot] = match instructions[slot] {
            Instruction::BneAbsolute(_) => Instruction::BneAbsolute(done),
            _ => Instruction::JmpAbsolute(done),
        };
    }
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
/// 두 단계 중 비싼 쪽을 센다. 예산은 어느 단계가 돌든 지켜져야 한다.
pub(super) fn worst_case_frame_cycles(
    origin: u16,
    atlas_page: u8,
    chr_source_state: super::chr_source_state::ChrSourceStateContract,
    cold_request_mapper_register: u8,
) -> Result<u32> {
    let (prologue, _) = frame_prologue(origin)?;
    let loop_start = next_address(origin, &prologue)?;
    let fixed = worst_case_cycles(&prologue)?
        + worst_case_cycles_with_calls(
            &frame_epilogue(origin, cold_request_mapper_register)?.0,
            &chr_source_state.restore_callee_cycles(),
        )?
        + u32::from(Instruction::Rts.worst_case_cycles());
    let overlay =
        worst_case_cycles(&tile_body(loop_start, atlas_page)?)? * u32::from(TILES_PER_FRAME);
    let restore =
        worst_case_cycles(&restore_body(loop_start)?)? * u32::from(RESTORE_CHUNKS_PER_FRAME);
    Ok(fixed + overlay.max(restore))
}

#[cfg(test)]
mod tests {
    use super::*;

    const ATLAS_PAGE: u8 = 0x2C;
    const COLD_REQUEST_MAPPER_REGISTER: u8 = 0xC8;

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
        // 밀기는 «제로 페이지 적재 뒤에 곧바로 `PHA`»인 짝만 센다. 배치 계산이 같은
        // 바이트를 읽는 것과 섞이지 않게 한다.
        let pushed: Vec<u8> = prologue
            .windows(2)
            .filter_map(|window| match window {
                [Instruction::LdaZeroPage(address), Instruction::Pha]
                    if BORROWED_SCRATCH.contains(address) =>
                {
                    Some(*address)
                }
                _ => None,
            })
            .collect();
        let restored: Vec<u8> = frame_epilogue(0xA000, COLD_REQUEST_MAPPER_REGISTER)
            .unwrap()
            .0
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

    /// 중앙 selector가 소유하는 FD와 FE 원천은 서로 다를 수 있다. 전송 이탈이 한
    /// 그림자를 두 helper에 재사용하거나 두 값을 갱신하면 원래 래치 쌍을 잃는다.
    #[test]
    fn fd_and_fe_are_restored_from_distinct_read_only_source_state() {
        let epilogue = frame_epilogue(0xA000, COLD_REQUEST_MAPPER_REGISTER)
            .unwrap()
            .0;
        let fd_restore = [
            Instruction::LdaZeroPage(super::super::chr_source_state::RIGHT_FD_SOURCE_SHADOW),
            Instruction::OraZeroPage(super::super::chr_source_state::CHR_SOURCE_HIGH_BITS),
            Instruction::JsrAbsolute(super::super::chr_source_state::RIGHT_FD_HELPER),
        ];
        let fe_restore = [
            Instruction::LdaZeroPage(super::super::chr_source_state::RIGHT_FE_SOURCE_SHADOW),
            Instruction::OraZeroPage(super::super::chr_source_state::CHR_SOURCE_HIGH_BITS),
            Instruction::JsrAbsolute(super::super::chr_source_state::RIGHT_FE_HELPER),
        ];

        assert!(
            epilogue
                .windows(fd_restore.len())
                .any(|window| window == fd_restore)
        );
        assert!(
            epilogue
                .windows(fe_restore.len())
                .any(|window| window == fe_restore)
        );
        assert!(!epilogue.iter().any(|instruction| matches!(
            instruction,
            Instruction::StaZeroPage(address)
                if [
                    super::super::chr_source_state::RIGHT_FD_SOURCE_SHADOW,
                    super::super::chr_source_state::RIGHT_FE_SOURCE_SHADOW,
                ]
                .contains(address)
        )));
    }

    /// 전송 프레임이 끝날 때 원본 FD를 그대로 두면 직전 한글 코드가 원본 일본어
    /// 타일로 해석된다. 원본 복원 뒤 렌더링이 재개되기 전에 냉간 표시 페이지를 다시
    /// 골라야 한다.
    #[test]
    fn an_incomplete_frame_reselects_the_cold_presentation_after_source_restore() {
        let epilogue = frame_epilogue(0xA000, COLD_REQUEST_MAPPER_REGISTER)
            .unwrap()
            .0;
        let fd_restore = Instruction::JsrAbsolute(super::super::chr_source_state::RIGHT_FD_HELPER);
        let cold_selection = [
            Instruction::LdaImmediate(super::super::chr_source_state::RIGHT_FD_CHR_REGISTER),
            Instruction::StaAbsolute(BANK_SELECT_REGISTER),
            Instruction::LdaImmediate(COLD_REQUEST_MAPPER_REGISTER),
            Instruction::StaAbsolute(BANK_VALUE_REGISTER),
        ];
        let restore_at = epilogue
            .iter()
            .position(|instruction| *instruction == fd_restore)
            .expect("the epilogue restores source FD");
        let cold_at = epilogue
            .windows(cold_selection.len())
            .position(|window| window == cold_selection)
            .expect("the epilogue reselects cold presentation FD");

        assert!(restore_at < cold_at);
    }

    /// 표시 selector는 준비 완료를 보는 순간 FD만 RAM으로 바꾸고 FE는 이 이탈에서
    /// 복원한 원본을 그대로 쓴다. FE 복원보다 먼저 준비 완료를 게시하면 그 사이에
    /// 한 프레임이라도 전송용 RAM 페이지가 배경으로 보일 수 있다.
    #[test]
    fn native_fe_is_restored_before_readiness_is_published() {
        let epilogue = frame_epilogue(0xA000, COLD_REQUEST_MAPPER_REGISTER)
            .unwrap()
            .0;
        let fe_restore = [
            Instruction::LdaZeroPage(super::super::chr_source_state::RIGHT_FE_SOURCE_SHADOW),
            Instruction::OraZeroPage(super::super::chr_source_state::CHR_SOURCE_HIGH_BITS),
            Instruction::JsrAbsolute(super::super::chr_source_state::RIGHT_FE_HELPER),
        ];
        let fe_restore_at = epilogue
            .windows(fe_restore.len())
            .position(|window| window == fe_restore)
            .expect("the epilogue restores native FE");
        let ready_at = epilogue
            .iter()
            .position(|instruction| *instruction == Instruction::StaAbsolute(REQUEST_STATE))
            .expect("the epilogue publishes readiness");

        assert!(fe_restore_at + fe_restore.len() <= ready_at);
    }

    /// 마지막 전송 프레임은 중앙 selector가 나중에 다시 불리기를 기다리지 않는다.
    /// FE 원본 복원 뒤 FD만 RAM으로 게시하고, 그 하드웨어 선택이 끝난 뒤에야
    /// 준비 완료를 알려야 첫 완성 프레임부터 올바른 두 래치가 보인다.
    #[test]
    fn the_completed_page_publishes_fd_ram_before_readiness() {
        let epilogue = frame_epilogue(0xA000, COLD_REQUEST_MAPPER_REGISTER)
            .unwrap()
            .0;
        let fe_restore = Instruction::JsrAbsolute(super::super::chr_source_state::RIGHT_FE_HELPER);
        let select_fd = [
            Instruction::LdaImmediate(super::super::chr_source_state::RIGHT_FD_CHR_REGISTER),
            Instruction::StaAbsolute(BANK_SELECT_REGISTER),
            Instruction::LdaImmediate(CHR_RAM_BANK_VALUE),
            Instruction::StaAbsolute(BANK_VALUE_REGISTER),
        ];
        let fe_restore_at = epilogue
            .iter()
            .position(|instruction| *instruction == fe_restore)
            .expect("the epilogue restores native FE");
        let select_fd_at = epilogue
            .windows(select_fd.len())
            .position(|window| window == select_fd)
            .expect("the epilogue publishes FD RAM");
        let ready_at = epilogue
            .iter()
            .position(|instruction| *instruction == Instruction::StaAbsolute(REQUEST_STATE))
            .expect("the epilogue publishes readiness");

        assert!(fe_restore_at < select_fd_at && select_fd_at + select_fd.len() <= ready_at);
        assert!(!epilogue.windows(4).any(|window| {
            window
                == [
                    Instruction::LdaImmediate(4),
                    Instruction::StaAbsolute(BANK_SELECT_REGISTER),
                    Instruction::LdaImmediate(CHR_RAM_BANK_VALUE),
                    Instruction::StaAbsolute(BANK_VALUE_REGISTER),
                ]
        }));
    }

    /// 반복 생산자는 `ready`에서만 커서 두 칸을 게시 원문 정체성으로 읽는다. 전송
    /// 완료 시점의 원본 상태는 이미 다음 레코드를 가리킬 수 있으므로 요청 때 고정한
    /// 두 값을 모두 저장한 뒤에만 준비 완료를 알린다.
    #[test]
    fn delayed_completion_publishes_the_request_time_source_identity() {
        let epilogue = frame_epilogue(0xA000, COLD_REQUEST_MAPPER_REGISTER)
            .unwrap()
            .0;
        let directory = [
            Instruction::LdaAbsolute(REQUEST_SOURCE_DIRECTORY_SELECTOR),
            Instruction::StaAbsolute(PUBLISHED_SOURCE_DIRECTORY_SELECTOR),
        ];
        let entry = [
            Instruction::LdaAbsolute(REQUEST_SOURCE_ENTRY_INDEX),
            Instruction::StaAbsolute(PUBLISHED_SOURCE_ENTRY_INDEX),
        ];
        let directory_at = epilogue
            .windows(directory.len())
            .position(|window| window == directory)
            .expect("the epilogue publishes the source selector");
        let entry_at = epilogue
            .windows(entry.len())
            .position(|window| window == entry)
            .expect("the epilogue publishes the source entry");
        let ready_at = epilogue
            .iter()
            .position(|instruction| *instruction == Instruction::StaAbsolute(REQUEST_STATE))
            .expect("the epilogue publishes readiness");

        assert!(directory_at + directory.len() <= entry_at);
        assert!(entry_at + entry.len() <= ready_at);
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
        let routine =
            build_transport_routine(0xA000, ATLAS_PAGE, COLD_REQUEST_MAPPER_REGISTER).unwrap();
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
        let routine =
            build_transport_routine(0xA000, ATLAS_PAGE, COLD_REQUEST_MAPPER_REGISTER).unwrap();

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
        let error =
            super::super::worst_case_cycles(&[Instruction::JsrAbsolute(0xFA80)]).unwrap_err();

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
