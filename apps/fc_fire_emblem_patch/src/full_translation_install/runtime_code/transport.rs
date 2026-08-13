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
//! **다만 그 페이지가 맞는 페이지가 아니다.** 실행해 보니 대사 글자는 제대로 나오는데
//! 맵 타일이 여전히 사라진다. 원인은 페이지 번호다. 게임이 `LDA #$00; JSR $FA80`으로
//! 고르는 «0번»은 `$FEEE`가 `(A & 0x1F) × 4 + 8`로 바꾸므로 물리 CHR 페이지 **2번**이다.
//! PRG `21`에 복제해 둔 것은 물리 0번, 즉 원본 글꼴 페이지다.
//!
//! 그러므로 복원이 되살리는 것은 맵이 쓰던 페이지가 아니라 글꼴 페이지다. 다음 과제는
//! 덮어쓸 수 있는 물리 CHR 페이지들을 PRG에도 복제해 두고, 관측해 둔 페이지 번호로
//! 그중 맞는 것을 고르는 것이다. PRG는 아직 100 KB 넘게 비어 있다.
//!
//! 되돌릴 페이지는 `chr_page_shadow`가 관측해 둔 값이다. 원본에는 «지금 걸려 있는
//! 페이지»를 담아 두는 변수가 없어서 만들어 두었다. 되돌리는 일 자체는 원본 설정기
//! `$FA80`·`$FAA0`이 하고, 그 비용은 아래에 세어 두었다.
//!
//! atlas는 타일당 8바이트 1bpp다. CHR에는 16바이트 2bpp로 펼치고 상위 bitplane은
//! 0으로 채운다. 상위 bitplane이 전부 0이라는 것은 직렬화 시점에 검사돼 있다.

use anyhow::{Context, Result, ensure};

use super::super::runtime_cursor_storage::{
    CURSOR_ENTRY_HIGH, CURSOR_ENTRY_LOW, CURSOR_GROUP_PAGE, CURSOR_OVERLAY_TILES, CURSOR_PHASE,
    CURSOR_REMAINING_TILES,
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
/// 매퍼 165가 4 KiB CHR 창 둘에 쓰는 MMC3 레지스터다.
const CHR_BANK_REGISTERS: [u8; 2] = [2, 4];
/// CHR RAM을 고르는 뱅크 값이다. CHR ROM의 물리 페이지 0은 다른 값으로 인코딩된다.
const CHR_RAM_BANK_VALUE: u8 = 0;
/// 되돌릴 때 부르는 원본 CHR 설정기들이다. 값은 관측해 둔 페이지 하나를 함께 쓴다.
/// 열세 곳의 호출부 중 열한 곳이 두 설정기에 같은 값을 넘기므로 그렇게 맞춘다.
const CHR_RESTORE_HELPERS: [u16; 2] = [0xFA80, 0xFAA0];
/// 도우미 하나가 최악의 경우 쓰는 사이클이다.
///
/// 방출된 바이트를 전수로 세어 얻었다. `$FA80`은 `JMP $FEEE`(3)이고, `$FEEE`는
/// `PHP PHA JSR $FE90` 뒤에 `$07DF` 오버라이드 분기 넷을 지나 `$FF10`의 레지스터
/// 쓰기로 모인다. `$FE90`의 최악 경로가 40, `$FEEE`의 최악 경로가 그것을 포함해
/// 122, 여기에 `JMP` 3과 호출한 `JSR` 6을 더해 131이다. 표본이 아니라 경로 전수라
/// 이 값은 상한이다.
const CHR_HELPER_WORST_CASE_CYCLES: u32 = 131;

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
    instructions.extend([Instruction::DecAbsolute(CURSOR_REMAINING_TILES), Instruction::Dex]);
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
fn frame_epilogue(origin: u16) -> Result<(Vec<Instruction>, Vec<usize>)> {
    let mut instructions = vec![
        Instruction::LdaZeroPage(ENTRY_POINTER_LOW),
        Instruction::StaAbsolute(CURSOR_ENTRY_LOW),
        Instruction::LdaZeroPage(ENTRY_POINTER_HIGH),
        Instruction::StaAbsolute(CURSOR_ENTRY_HIGH),
    ];
    // CHR 뱅크를 원본이 기대하는 값으로 되돌린다. 되돌리지 않으면 아직 다 올라가지
    // 않은 CHR RAM이 다음 프레임 렌더링에 그대로 나온다.
    for helper in CHR_RESTORE_HELPERS {
        instructions.extend([
            Instruction::LdaAbsolute(super::chr_page_shadow::CHR_PAGE_SHADOW),
            Instruction::JsrAbsolute(helper),
        ]);
    }
    // 민 순서의 반대로 되돌린다.
    for address in BORROWED_SCRATCH.iter().rev() {
        instructions.extend([Instruction::Pla, Instruction::StaZeroPage(*address)]);
    }

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
    instructions.extend([
        Instruction::LdaImmediate(STATE_READY),
        Instruction::StaAbsolute(REQUEST_STATE),
    ]);
    Ok((instructions, needs_done))
}

pub(super) fn build_transport_routine(origin: u16, atlas_page: u8) -> Result<RuntimeRoutine> {
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
    let (epilogue, needs_done) = frame_epilogue(epilogue_address)?;
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
pub(super) fn worst_case_frame_cycles(origin: u16, atlas_page: u8) -> Result<u32> {
    let (prologue, _) = frame_prologue(origin)?;
    let loop_start = next_address(origin, &prologue)?;
    let fixed = worst_case_cycles(&prologue)?
        + worst_case_cycles_with_calls(
            &frame_epilogue(origin)?.0,
            &CHR_RESTORE_HELPERS.map(|helper| (helper, CHR_HELPER_WORST_CASE_CYCLES)),
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
        let restored: Vec<u8> = frame_epilogue(0xA000)
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
