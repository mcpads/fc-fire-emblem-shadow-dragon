//! 대사 진입에서 «현재 가시 페이지에 필요한 어느 레시피를 올려야 하는가»를 찾아
//! 커서를 세운다.
//!
//! 이 루틴은 NMI가 아니라 주 흐름에서 돈다. 생산자가 대사에 들어가는 순간 한 번만
//! 불리므로 vblank 예산과 무관하다. 그래서 여기서는 사이클보다 정확성을 본다.
//!
//! 찾는 순서는 넷이다.
//!
//! 1. `$77F4`(디렉터리 선택자)와 `$77F1`(엔트리 색인)로 런타임 식별표에서 레코드
//!    색인을 얻는다.
//! 2. 레코드 색인으로 스캔 재료의 레코드 디렉터리에서 그 레코드의 페이지 레시피
//!    구간을 얻고, 현재 가시 페이지의 레시피 덩이 오프셋을 읽는다.
//! 3. 그 오프셋으로 레시피 덩이의 자리를 얻는다.
//! 4. 덩이의 첫 바이트가 항목 수다. 그것과 항목 시작 주소·페이지를 커서에 쓴다.
//!
//! 어느 단계든 범위를 벗어나면 커서를 세우지 않고 실패로 돌아간다. 생산자는 그때
//! 요청을 발행하지 않으므로 원본 일본어 경로가 그대로 돈다. 설계의 «모든 실패는
//! 원본 동작으로 되돌아감»이 여기서 성립한다.

use anyhow::{Context, Result};

use super::super::dynamic_composition::{
    PAGE_RECIPE_HEADER_BYTE_COUNT, REUSE_RESIDENT_PAGE_RECIPE_REFERENCE,
};
use super::super::runtime_cursor_storage::{
    CURSOR_ENTRY_HIGH, CURSOR_ENTRY_LOW, CURSOR_OVERLAY_TILES, CURSOR_PHASE, CURSOR_RECIPE_PAGE,
    CURSOR_REMAINING_TILES, PUBLISHED_SOURCE_DIRECTORY_SELECTOR, PUBLISHED_SOURCE_ENTRY_INDEX,
    REQUEST_SOURCE_DIRECTORY_SELECTOR, REQUEST_SOURCE_ENTRY_INDEX,
};
use super::super::runtime_state_storage::{
    CANDIDATE_START, CURRENT_PAGE_RESIDENCY, DIALOGUE_RUNTIME_STATE_END, RECORD_INDEX_HIGH,
    RECORD_INDEX_LOW, REQUEST_STATE, VISIBLE_PAGE_INDEX,
};
use super::resolved_page_publication::NO_RESIDENT_PAGE_RECIPE;
use super::transport::{
    PHASE_RESTORE, RESTORE_CHUNK_COUNT, STATE_COMPLETED_PAGE_SUSPENDED, STATE_READY,
};
use super::{RuntimeRoutine, next_address};
use crate::rp2a03::{Instruction, assemble_at};

pub(in crate::full_translation_install) const INITIAL_PAGE_REQUEST_RESOLVER_ROLE: &str =
    "dialogue initial-page request resolver";
pub(in crate::full_translation_install) const NEXT_PAGE_REQUEST_RESOLVER_ROLE: &str =
    "dialogue next-page request resolver";

/// 원본이 디렉터리 선택자를 담아 두는 자리다.
pub(super) const SOURCE_DIRECTORY_SELECTOR: u16 = 0x77F4;
/// 원본이 엔트리 색인을 담아 두는 자리다.
pub(super) const SOURCE_ENTRY_INDEX: u16 = 0x77F1;
/// 식별표에서 «없는 선택자»를 뜻하는 값이다.
const MISSING_TABLE: u8 = 0xFF;
/// 새 대사 수명에서 살아 있는 원본 selector/index를 현재 레코드로 해석한다.
#[cfg(test)]
pub(super) const LOOKUP_LIVE_SOURCE_IDENTITY: u8 = 1;
/// 독립 수명은 살아 있는 원문 정체성을 쓰되 이전 상주 그룹은 재사용하지 않는다.
pub(super) const LOOKUP_INITIAL_SOURCE_IDENTITY: u8 = 2;
/// 연속 대사에서 직전에 게시한 선행 조회값을 현재 레코드로 승격한다.
pub(super) const LOOKUP_PUBLISHED_SOURCE_IDENTITY: u8 = 0;

const BANK_VALUE_REGISTER: u16 = 0x8001;
const PRG_8000_REGISTER: u8 = 6;
/// 자료 창의 시작이다. 재료 페이지가 여기 걸린다.
const DATA_WINDOW_BASE: u16 = 0x8000;
/// MMC3 페이지 하나가 자료 창에서 차지하는 크기다.
const DATA_WINDOW_SIZE: u16 = 0x2000;

/// 원본 대사 상태기가 쓰는 여섯 32바이트 물리 줄 버퍼다. 새 레코드는 이 범위
/// 전체를 `FF`로 비우고, 같은 레코드의 다음 페이지는 현재 줄 수명을 이어 간다.
const LINE_BUFFER_START: u16 = 0x7832;
const LINE_BUFFER_BYTE_COUNT: u8 = 6 * 0x20;
const LINE_BUFFER_BLANK: u8 = 0xFF;

/// 빌드가 아는 재료 배치다. 런타임은 이 값들을 상수로 받는다.
#[derive(Debug, Clone, Copy)]
pub(in crate::full_translation_install) struct MaterialLayout {
    /// 런타임 식별표가 들어 있는 MMC3 페이지다.
    pub(in crate::full_translation_install) identity_page: u8,
    /// 런타임 식별 자료가 그 페이지의 `$8000` 창 안에서 시작하는 주소다. 식별표
    /// 서술자의 엔트리 오프셋은 이 기준점에 상대적이다.
    pub(in crate::full_translation_install) identity_material_base: u16,
    /// 그 페이지 안에서 selector 디렉터리가 시작하는 CPU 주소다.
    pub(in crate::full_translation_install) identity_selector_directory: u16,
    /// 표 서술자가 시작하는 CPU 주소다.
    pub(in crate::full_translation_install) identity_table_descriptors: u16,
    /// 레시피 참조와 레코드 디렉터리가 들어 있는 MMC3 페이지다.
    pub(in crate::full_translation_install) scan_index_page: u8,
    /// 페이지 작업집합마다 하나씩인 16비트 레시피 덩이 오프셋 배열이다.
    pub(in crate::full_translation_install) page_recipe_references: u16,
    /// 레코드마다 두 바이트인 페이지 구간 표의 CPU 주소다.
    pub(in crate::full_translation_install) record_directory: u16,
    /// 레시피 덩이들이 재료 용기 안에서 시작하는 자리다.
    pub(in crate::full_translation_install) page_recipe_block_container_base: u16,
    /// 재료 용기가 시작하는 MMC3 페이지다.
    pub(in crate::full_translation_install) container_first_page: u8,
    /// `{EC}` 생산자 전용 정규 문자열이 들어 있는 MMC3 페이지다.
    pub(in crate::full_translation_install) producer_encoding_page: u8,
    /// 항목별 문자열 오프셋 표와 그 기준점이다.
    pub(in crate::full_translation_install) producer_item_directory: u16,
    pub(in crate::full_translation_install) producer_unit_directory: u16,
    pub(in crate::full_translation_install) producer_location_directory: u16,
    pub(in crate::full_translation_install) producer_encoding_base: u16,
}

/// 주 흐름에서 빌려 쓰는 제로 페이지다. 밀고 되돌린다.
const PREVIOUS_RESIDENT_PAGE_RECIPE: u8 = 0x06;
const BORROWED_SCRATCH: [u8; 7] = [
    0x00,
    0x01,
    0x02,
    0x03,
    0x04,
    0x05,
    PREVIOUS_RESIDENT_PAGE_RECIPE,
];

/// 빌린 제로 페이지를 민 순서의 반대로 되돌린다. 캐리는 건드리지 않는다.
fn restore_scratch(instructions: &mut Vec<Instruction>) {
    for address in BORROWED_SCRATCH.iter().rev() {
        instructions.extend([Instruction::Pla, Instruction::StaZeroPage(*address)]);
    }
}

/// 실패로 빠지는 분기를 붙인다.
///
/// 루틴이 200바이트 남짓이라 앞쪽 분기가 꼬리까지 상대 주소로 닿지 못한다. 그래서
/// 조건을 뒤집어 `JMP` 하나를 건너뛰는 형태를 쓴다. 되메울 `JMP`의 색인을 돌려준다.
fn branch_to_failure(
    instructions: &mut Vec<Instruction>,
    origin: u16,
    inverse: fn(u16) -> Instruction,
) -> Result<usize> {
    let branch = next_address(origin, instructions)?;
    let skip = branch
        .checked_add(2 + 3)
        .context("resolver failure branch address overflow")?;
    instructions.push(inverse(skip));
    let jump = instructions.len();
    instructions.push(Instruction::JmpAbsolute(origin));
    Ok(jump)
}

/// `$8000` 창에 페이지 하나를 건다.
fn map_page(page: Instruction) -> [Instruction; 4] {
    [
        Instruction::LdaImmediate(PRG_8000_REGISTER),
        crate::mapper165::selector_safety::select_register_instruction(),
        page,
        Instruction::StaAbsolute(BANK_VALUE_REGISTER),
    ]
}

fn save_scratch(instructions: &mut Vec<Instruction>) {
    for address in BORROWED_SCRATCH {
        instructions.extend([Instruction::LdaZeroPage(address), Instruction::Pha]);
    }
}

fn clear_runtime_state(instructions: &mut Vec<Instruction>) {
    instructions.push(Instruction::LdaImmediate(0));
    for address in CANDIDATE_START..=DIALOGUE_RUNTIME_STATE_END {
        instructions.push(Instruction::StaAbsolute(address));
    }
}

fn new_record_line_buffer_reset(origin: u16) -> Result<Vec<Instruction>> {
    let mut instructions = vec![
        Instruction::LdaImmediate(LINE_BUFFER_BLANK),
        Instruction::LdyImmediate(LINE_BUFFER_BYTE_COUNT),
    ];
    let loop_address = next_address(origin, &instructions)?;
    instructions.extend([
        Instruction::Dey,
        Instruction::StaAbsoluteY(LINE_BUFFER_START),
        Instruction::BneAbsolute(loop_address),
    ]);
    Ok(instructions)
}

fn append_new_record_line_buffer_reset(
    instructions: &mut Vec<Instruction>,
    routine_origin: u16,
) -> Result<()> {
    let reset_origin = next_address(routine_origin, instructions)?;
    instructions.extend(new_record_line_buffer_reset(reset_origin)?);
    Ok(())
}

struct PageRequestResolutionBranches {
    page_exhausted: Vec<usize>,
    resident_recipe_reuse: Vec<usize>,
}

pub(super) fn contains_new_record_line_buffer_reset(routine: &RuntimeRoutine) -> Result<bool> {
    let origin = 0x8000;
    let reset = assemble_at(origin, &new_record_line_buffer_reset(origin)?)?;
    Ok(routine
        .bytes
        .windows(reset.len())
        .any(|window| window == reset))
}

/// 영속 레코드 색인 `$07F0/1`과 가시 페이지 색인 `$07F2`를 사용해 페이지 그룹과
/// 전송 커서를 세운다. 레코드 디렉터리의 다음 항목이 현재 레코드의 끝이므로, 선택할
/// 페이지 오프셋이 그 끝보다 작은지도 함께 확인한다.
fn append_page_request_resolution(
    instructions: &mut Vec<Instruction>,
    failure_branches: &mut Vec<usize>,
    origin: u16,
    layout: MaterialLayout,
    allow_resident_recipe_reuse: bool,
) -> Result<PageRequestResolutionBranches> {
    let mut page_exhausted_branches = Vec::new();
    let mut resident_recipe_reuse_branches = Vec::new();
    instructions.extend(map_page(Instruction::LdaImmediate(layout.scan_index_page)));
    instructions.extend([
        // 레코드 색인 × 2가 디렉터리 안의 자리다.
        Instruction::LdaZeroPage(0x02),
        Instruction::AslAccumulator,
        Instruction::StaZeroPage(0x04),
        Instruction::LdaZeroPage(0x03),
        Instruction::RolZeroPage(0x03),
        Instruction::LdaZeroPage(0x04),
        Instruction::Clc,
        Instruction::AdcImmediate(layout.record_directory as u8),
        Instruction::StaZeroPage(0x00),
        Instruction::LdaZeroPage(0x03),
        Instruction::AdcImmediate((layout.record_directory >> 8) as u8),
        Instruction::StaZeroPage(0x01),
        // 현재 레코드의 페이지 선택자 시작 오프셋이다.
        Instruction::LdyImmediate(0),
        Instruction::LdaIndirectY(0x00),
        Instruction::StaZeroPage(0x04),
        Instruction::Iny,
        Instruction::LdaIndirectY(0x00),
        Instruction::StaZeroPage(0x05),
        // 현재 가시 페이지를 더한다.
        Instruction::Clc,
        Instruction::LdaZeroPage(0x04),
        Instruction::AdcAbsolute(VISIBLE_PAGE_INDEX),
        Instruction::StaZeroPage(0x04),
        Instruction::LdaZeroPage(0x05),
        Instruction::AdcImmediate(0),
        Instruction::StaZeroPage(0x05),
        // 다음 디렉터리 항목은 현재 레코드의 끝 오프셋이다.
        Instruction::LdyImmediate(2),
        Instruction::LdaIndirectY(0x00),
        Instruction::StaZeroPage(0x02),
        Instruction::Iny,
        Instruction::LdaIndirectY(0x00),
        Instruction::StaZeroPage(0x03),
        Instruction::LdaZeroPage(0x05),
        Instruction::CmpZeroPage(0x03),
    ]);
    let lower_high_byte = instructions.len();
    let lower_high_byte_placeholder = next_address(origin, instructions)?;
    instructions.push(Instruction::BccAbsolute(lower_high_byte_placeholder));
    // 상위 바이트가 같지 않으면서 작지도 않으면 선택 오프셋이 끝을 넘었다.
    page_exhausted_branches.push(branch_to_failure(
        instructions,
        origin,
        Instruction::BeqAbsolute,
    )?);
    instructions.extend([
        Instruction::LdaZeroPage(0x04),
        Instruction::CmpZeroPage(0x02),
    ]);
    // 같은 상위 바이트에서 하위 바이트가 끝 이상이어도 범위 밖이다.
    page_exhausted_branches.push(branch_to_failure(
        instructions,
        origin,
        Instruction::BccAbsolute,
    )?);
    let selected_page_is_bounded = next_address(origin, instructions)?;
    instructions[lower_high_byte] = Instruction::BccAbsolute(selected_page_is_bounded);

    instructions.extend([
        // 페이지 참조는 16비트 레시피 덩이 오프셋이므로 색인을 두 배로 만든다.
        Instruction::AslZeroPage(0x04),
        Instruction::RolZeroPage(0x05),
        Instruction::Clc,
        Instruction::LdaZeroPage(0x04),
        Instruction::AdcImmediate(layout.page_recipe_references as u8),
        Instruction::StaZeroPage(0x00),
        Instruction::LdaZeroPage(0x05),
        Instruction::AdcImmediate((layout.page_recipe_references >> 8) as u8),
        Instruction::StaZeroPage(0x01),
        // 참조 배열에서 레시피 덩이의 상대 오프셋을 읽는다.
        Instruction::LdyImmediate(0),
        Instruction::LdaIndirectY(0x00),
        Instruction::StaZeroPage(0x02),
        Instruction::Iny,
        Instruction::LdaIndirectY(0x00),
        Instruction::StaZeroPage(0x03),
    ]);

    // 같은 레코드의 직전 페이지와 레시피가 같으면 생성기가 FFFF를 쓴다. 최초
    // 페이지에는 이 표식이 금지되어 있고, 다음 페이지 resolver만 현재 CHR-RAM을
    // 그대로 준비 상태로 승격할 수 있다.
    instructions.extend([
        Instruction::LdaZeroPage(0x02),
        Instruction::CmpImmediate(REUSE_RESIDENT_PAGE_RECIPE_REFERENCE as u8),
    ]);
    let concrete_low = instructions.len();
    let concrete_low_branch = next_address(origin, instructions)?;
    instructions.push(Instruction::BneAbsolute(
        concrete_low_branch
            .checked_add(2)
            .context("resident recipe branch address overflow")?,
    ));
    instructions.extend([
        Instruction::LdaZeroPage(0x03),
        Instruction::CmpImmediate((REUSE_RESIDENT_PAGE_RECIPE_REFERENCE >> 8) as u8),
    ]);
    let reuse_branch = branch_to_failure(instructions, origin, Instruction::BneAbsolute)?;
    if allow_resident_recipe_reuse {
        resident_recipe_reuse_branches.push(reuse_branch);
    } else {
        failure_branches.push(reuse_branch);
    }
    let concrete_recipe = next_address(origin, instructions)?;
    instructions[concrete_low] = Instruction::BneAbsolute(concrete_recipe);

    instructions.extend([
        // 덩이의 용기 안 자리를 만든다.
        Instruction::Clc,
        Instruction::LdaZeroPage(0x02),
        Instruction::AdcImmediate(layout.page_recipe_block_container_base as u8),
        Instruction::StaZeroPage(0x02),
        Instruction::LdaZeroPage(0x03),
        Instruction::AdcImmediate((layout.page_recipe_block_container_base >> 8) as u8),
        Instruction::StaZeroPage(0x03),
        // 페이지는 상위 바이트의 위 세 비트에서, 창 안 주소는 나머지에서 나온다.
        Instruction::LsrAccumulator,
        Instruction::LsrAccumulator,
        Instruction::LsrAccumulator,
        Instruction::LsrAccumulator,
        Instruction::LsrAccumulator,
        Instruction::Clc,
        Instruction::AdcImmediate(layout.container_first_page),
        Instruction::StaAbsolute(CURSOR_RECIPE_PAGE),
        Instruction::LdaZeroPage(0x03),
        Instruction::AndImmediate(((DATA_WINDOW_SIZE - 1) >> 8) as u8),
        Instruction::OraImmediate((DATA_WINDOW_BASE >> 8) as u8),
        Instruction::StaZeroPage(0x03),
    ]);

    // 덩이의 첫 바이트가 항목 수다. 항목은 그다음부터다.
    instructions.extend(map_page(Instruction::LdaAbsolute(CURSOR_RECIPE_PAGE)));
    instructions.extend([
        Instruction::LdaZeroPage(0x02),
        Instruction::StaZeroPage(0x00),
        Instruction::LdaZeroPage(0x03),
        Instruction::StaZeroPage(0x01),
        Instruction::LdyImmediate(0),
        Instruction::LdaIndirectY(0x00),
        // 덩이 머리의 상주 그룹을 먼저 게시하고 직전 그룹과 비교한다.
        Instruction::StaAbsolute(CURRENT_PAGE_RESIDENCY),
        Instruction::CmpZeroPage(PREVIOUS_RESIDENT_PAGE_RECIPE),
    ]);
    let same_resident_group = branch_to_failure(instructions, origin, Instruction::BneAbsolute)?;
    if allow_resident_recipe_reuse {
        resident_recipe_reuse_branches.push(same_resident_group);
    } else {
        failure_branches.push(same_resident_group);
    }
    instructions.extend([
        Instruction::Iny,
        Instruction::LdaIndirectY(0x00),
        // 덮기 몫은 보관해 둔다. 먼저 도는 것은 복원 단계다.
        Instruction::StaAbsolute(CURSOR_OVERLAY_TILES),
    ]);
    failure_branches.push(branch_to_failure(
        instructions,
        origin,
        Instruction::BneAbsolute,
    )?);
    instructions.extend([
        Instruction::Clc,
        Instruction::LdaZeroPage(0x02),
        Instruction::AdcImmediate(PAGE_RECIPE_HEADER_BYTE_COUNT as u8),
        Instruction::StaAbsolute(CURSOR_ENTRY_LOW),
        Instruction::LdaZeroPage(0x03),
        Instruction::AdcImmediate(0),
        Instruction::StaAbsolute(CURSOR_ENTRY_HIGH),
        Instruction::LdaImmediate(PHASE_RESTORE),
        Instruction::StaAbsolute(CURSOR_PHASE),
        Instruction::LdaImmediate(RESTORE_CHUNK_COUNT),
        Instruction::StaAbsolute(CURSOR_REMAINING_TILES),
    ]);
    Ok(PageRequestResolutionBranches {
        page_exhausted: page_exhausted_branches,
        resident_recipe_reuse: resident_recipe_reuse_branches,
    })
}

fn finish_resolver(
    origin: u16,
    mut instructions: Vec<Instruction>,
    failure_branches: Vec<usize>,
    page_exhausted_branches: Vec<usize>,
    resident_recipe_reuse_branches: Vec<usize>,
    role: &'static str,
) -> Result<RuntimeRoutine> {
    instructions.push(Instruction::Sec);
    restore_scratch(&mut instructions);
    instructions.push(Instruction::Rts);

    let resident_recipe_reuse = next_address(origin, &instructions)?;
    if !resident_recipe_reuse_branches.is_empty() {
        instructions.extend([
            // 레시피가 동일하면 기존 CHR-RAM이 이미 완성 결과다. `carry clear`로
            // 공통 발행기의 합성 경로를 건너뛰되, ready 상태는 먼저 게시한다.
            Instruction::LdaImmediate(STATE_READY),
            Instruction::StaAbsolute(REQUEST_STATE),
            Instruction::Clc,
        ]);
        restore_scratch(&mut instructions);
        instructions.push(Instruction::Rts);
    }

    let page_exhausted = next_address(origin, &instructions)?;
    if !page_exhausted_branches.is_empty() {
        instructions.extend([
            // 레코드의 마지막 번역 페이지 다음은 손상된 레시피와 다르다. 전송은
            // 끝났지만 원천 상태기가 곧 같은 완료 페이지 위에 선택 UI를 얹을 수
            // 있으므로, 그 UI만 다시 고를 수 있는 별도 상태로 내린다.
            Instruction::LdaImmediate(STATE_COMPLETED_PAGE_SUSPENDED),
            Instruction::StaAbsolute(REQUEST_STATE),
        ]);
    }

    let failure = next_address(origin, &instructions)?;
    for index in failure_branches {
        instructions[index] = Instruction::JmpAbsolute(failure);
    }
    for index in page_exhausted_branches {
        instructions[index] = Instruction::JmpAbsolute(page_exhausted);
    }
    for index in resident_recipe_reuse_branches {
        instructions[index] = Instruction::JmpAbsolute(resident_recipe_reuse);
    }
    instructions.push(Instruction::Clc);
    restore_scratch(&mut instructions);
    instructions.push(Instruction::Rts);

    let bytes =
        assemble_at(origin, &instructions).with_context(|| format!("cannot assemble {role}"))?;
    Ok(RuntimeRoutine {
        role,
        address: origin,
        bytes,
    })
}

/// 새 레코드의 0번 가시 페이지를 찾는다. X는 현재 레코드를 식별할 원천을 고른다.
/// 새 수명은 살아 있는 원본 정체성을 쓰고, 연속 수명은 직전에 게시한 선행 조회값을
/// 현재 레코드로 승격한다. 게시 정체성은 커서와 같은 두 바이트를 공유하므로 지우기
/// 전에 X와 빌린 제로 페이지로 옮긴다. 그 뒤 모든 휘발 상태를 지우고 살아 있는 값은
/// 다음 호출의 승격 후보로 따로 고정한다. 따라서 실패해도 selector가 이전 수명의
/// `ready`나 게시 정체성을 다시 볼 수 없다.
pub(in crate::full_translation_install) fn build_resolve_request(
    origin: u16,
    layout: MaterialLayout,
) -> Result<RuntimeRoutine> {
    let mut instructions = Vec::new();
    let mut failure_branches = Vec::new();
    save_scratch(&mut instructions);

    // 독립 진입은 휘발 RAM의 과거 값을 절대 재사용하지 않는다. 연결 레코드 진입은
    // 현재 상주 그룹을 빌린 제로 페이지에 보존한 뒤 공통 상태 초기화를 거친다.
    instructions.push(Instruction::CpxImmediate(LOOKUP_INITIAL_SOURCE_IDENTITY));
    let continuing_lifetime = instructions.len();
    let continuing_lifetime_branch = next_address(origin, &instructions)?;
    instructions.push(Instruction::BneAbsolute(
        continuing_lifetime_branch
            .checked_add(2)
            .context("resident group selection branch overflow")?,
    ));
    instructions.push(Instruction::LdaImmediate(NO_RESIDENT_PAGE_RECIPE));
    let previous_group_selected = instructions.len();
    instructions.push(Instruction::JmpAbsolute(origin));

    let load_previous_group = next_address(origin, &instructions)?;
    instructions[continuing_lifetime] = Instruction::BneAbsolute(load_previous_group);
    instructions.push(Instruction::LdaAbsolute(CURRENT_PAGE_RESIDENCY));

    let save_previous_group = next_address(origin, &instructions)?;
    instructions[previous_group_selected] = Instruction::JmpAbsolute(save_previous_group);
    instructions.push(Instruction::StaZeroPage(PREVIOUS_RESIDENT_PAGE_RECIPE));

    // 연속 수명에서는 게시 정체성도 아래 clear 범위에 들어 있다. 어느 정체성을
    // 해석할지 먼저 고른 뒤에만 휘발 상태를 지운다.
    instructions.push(Instruction::CpxImmediate(LOOKUP_PUBLISHED_SOURCE_IDENTITY));
    let use_published_identity = instructions.len();
    instructions.push(Instruction::BeqAbsolute(origin));
    instructions.extend([
        Instruction::LdxAbsolute(SOURCE_DIRECTORY_SELECTOR),
        Instruction::LdaAbsolute(SOURCE_ENTRY_INDEX),
        Instruction::StaZeroPage(0x05),
    ]);
    let identity_selected = instructions.len();
    instructions.push(Instruction::JmpAbsolute(origin));

    let published_identity = next_address(origin, &instructions)?;
    instructions[use_published_identity] = Instruction::BeqAbsolute(published_identity);
    instructions.extend([
        Instruction::LdxAbsolute(PUBLISHED_SOURCE_DIRECTORY_SELECTOR),
        Instruction::LdaAbsolute(PUBLISHED_SOURCE_ENTRY_INDEX),
        Instruction::StaZeroPage(0x05),
    ]);

    let resolve_identity = next_address(origin, &instructions)?;
    instructions[identity_selected] = Instruction::JmpAbsolute(resolve_identity);
    clear_runtime_state(&mut instructions);
    // 최초 진입뿐 아니라 E4/E6 lookahead와 E7 caller-resume도 모두 여기서 새 레코드
    // 수명을 연다. 원본 state-1만 갖고 있던 0x00C0-byte fill을 이 공통 경계로 올려,
    // 어느 외부 상태기가 레코드를 골라도 직전 레코드의 물리 줄을 재해석하지 않는다.
    append_new_record_line_buffer_reset(&mut instructions, origin)?;
    instructions.extend([
        Instruction::LdaAbsolute(SOURCE_DIRECTORY_SELECTOR),
        Instruction::StaAbsolute(REQUEST_SOURCE_DIRECTORY_SELECTOR),
        Instruction::LdaAbsolute(SOURCE_ENTRY_INDEX),
        Instruction::StaAbsolute(REQUEST_SOURCE_ENTRY_INDEX),
    ]);

    // 1. 식별표에서 레코드 색인을 얻는다.
    instructions.extend(map_page(Instruction::LdaImmediate(layout.identity_page)));
    instructions.extend([
        Instruction::LdaAbsoluteX(layout.identity_selector_directory),
        Instruction::CmpImmediate(MISSING_TABLE),
    ]);
    failure_branches.push(branch_to_failure(
        &mut instructions,
        origin,
        Instruction::BneAbsolute,
    )?);
    instructions.extend([
        Instruction::AslAccumulator,
        Instruction::AslAccumulator,
        Instruction::Tax,
        Instruction::LdaAbsoluteX(layout.identity_table_descriptors + 1),
        Instruction::CmpZeroPage(0x05),
    ]);
    failure_branches.push(branch_to_failure(
        &mut instructions,
        origin,
        Instruction::BcsAbsolute,
    )?);
    failure_branches.push(branch_to_failure(
        &mut instructions,
        origin,
        Instruction::BneAbsolute,
    )?);
    instructions.extend([
        Instruction::LdaAbsoluteX(layout.identity_table_descriptors + 2),
        Instruction::Clc,
        Instruction::AdcImmediate(layout.identity_material_base as u8),
        Instruction::StaZeroPage(0x00),
        Instruction::LdaAbsoluteX(layout.identity_table_descriptors + 3),
        Instruction::AdcImmediate((layout.identity_material_base >> 8) as u8),
        Instruction::StaZeroPage(0x01),
        Instruction::LdaZeroPage(0x05),
        Instruction::AslAccumulator,
        Instruction::Tay,
        Instruction::LdaIndirectY(0x00),
        Instruction::StaZeroPage(0x02),
        Instruction::StaAbsolute(RECORD_INDEX_LOW),
        Instruction::Iny,
        Instruction::LdaIndirectY(0x00),
        Instruction::StaZeroPage(0x03),
        Instruction::StaAbsolute(RECORD_INDEX_HIGH),
        // 레코드 색인 `FFFF`는 «없음»이다.
        Instruction::AndZeroPage(0x02),
        Instruction::CmpImmediate(0xFF),
    ]);
    failure_branches.push(branch_to_failure(
        &mut instructions,
        origin,
        Instruction::BneAbsolute,
    )?);

    let page_resolution = append_page_request_resolution(
        &mut instructions,
        &mut failure_branches,
        origin,
        layout,
        false,
    )?;
    failure_branches.extend(page_resolution.page_exhausted);
    finish_resolver(
        origin,
        instructions,
        failure_branches,
        Vec::new(),
        page_resolution.resident_recipe_reuse,
        INITIAL_PAGE_REQUEST_RESOLVER_ROLE,
    )
}

/// 같은 레코드의 다음 가시 페이지를 찾는다. 호출자는 원본 완료 상태가 실제로
/// `09` 계속을 선택한 경계이므로, 이 루틴은 페이지를 정확히 한 칸만 올린다.
/// 디렉터리의 끝에 닿으면 완료 페이지를 보류하고, 그 밖의 자료 실패는 요청을
/// `inactive`로 남긴다.
pub(in crate::full_translation_install) fn build_resolve_next_page_request(
    origin: u16,
    layout: MaterialLayout,
) -> Result<RuntimeRoutine> {
    let mut instructions = vec![
        Instruction::LdaAbsolute(PUBLISHED_SOURCE_DIRECTORY_SELECTOR),
        Instruction::StaAbsolute(REQUEST_SOURCE_DIRECTORY_SELECTOR),
        Instruction::LdaAbsolute(PUBLISHED_SOURCE_ENTRY_INDEX),
        Instruction::StaAbsolute(REQUEST_SOURCE_ENTRY_INDEX),
        Instruction::LdaImmediate(0),
        Instruction::StaAbsolute(REQUEST_STATE),
    ];
    let mut failure_branches = Vec::new();
    save_scratch(&mut instructions);
    instructions.extend([
        Instruction::LdaAbsolute(CURRENT_PAGE_RESIDENCY),
        Instruction::StaZeroPage(PREVIOUS_RESIDENT_PAGE_RECIPE),
        Instruction::IncAbsolute(VISIBLE_PAGE_INDEX),
        Instruction::LdaAbsolute(RECORD_INDEX_LOW),
        Instruction::StaZeroPage(0x02),
        Instruction::LdaAbsolute(RECORD_INDEX_HIGH),
        Instruction::StaZeroPage(0x03),
    ]);
    let page_resolution = append_page_request_resolution(
        &mut instructions,
        &mut failure_branches,
        origin,
        layout,
        true,
    )?;
    finish_resolver(
        origin,
        instructions,
        failure_branches,
        page_resolution.page_exhausted,
        page_resolution.resident_recipe_reuse,
        NEXT_PAGE_REQUEST_RESOLVER_ROLE,
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::full_translation_install::runtime_state_storage::CONSUMER_FONT_PAGE;

    fn layout() -> MaterialLayout {
        MaterialLayout {
            identity_page: 0x2F,
            identity_material_base: 0x9800,
            identity_selector_directory: 0x9424,
            identity_table_descriptors: 0x9524,
            scan_index_page: 0x2C,
            page_recipe_references: 0x9676,
            record_directory: 0x9A16,
            page_recipe_block_container_base: 7_758,
            container_first_page: 0x2C,
            producer_encoding_page: 0x2F,
            producer_item_directory: 0x9000,
            producer_unit_directory: 0x9100,
            producer_location_directory: 0x9200,
            producer_encoding_base: 0x8F00,
        }
    }

    fn runtime_state_clear_bytes() -> Vec<u8> {
        let mut instructions = vec![Instruction::LdaImmediate(0)];
        instructions
            .extend((CANDIDATE_START..=DIALOGUE_RUNTIME_STATE_END).map(Instruction::StaAbsolute));
        assemble_at(0x8000, &instructions).unwrap()
    }

    fn execute_line_buffer_reset(bytes: &[u8], origin: u16, memory: &mut [u8; 0x10000]) -> usize {
        let mut pc = origin;
        let mut a = 0;
        let mut y = 0;
        let mut write_count = 0;
        let end = origin + u16::try_from(bytes.len()).unwrap();
        for _ in 0..1_000 {
            if pc == end {
                return write_count;
            }
            let offset = usize::from(pc - origin);
            match bytes[offset] {
                0xA9 => {
                    a = bytes[offset + 1];
                    pc += 2;
                }
                0xA0 => {
                    y = bytes[offset + 1];
                    pc += 2;
                }
                0x88 => {
                    y = y.wrapping_sub(1);
                    pc += 1;
                }
                0x99 => {
                    let base = u16::from_le_bytes([bytes[offset + 1], bytes[offset + 2]]);
                    memory[usize::from(base.wrapping_add(u16::from(y)))] = a;
                    write_count += 1;
                    pc += 3;
                }
                0xD0 => {
                    let displacement = bytes[offset + 1] as i8;
                    pc += 2;
                    if y != 0 {
                        pc = pc.wrapping_add_signed(i16::from(displacement));
                    }
                }
                opcode => panic!("unsupported line-buffer reset opcode {opcode:02X}"),
            }
        }
        panic!("line-buffer reset did not terminate");
    }

    #[test]
    fn a_new_record_clears_all_six_physical_rows_without_touching_neighbors() {
        let origin = 0x9000;
        let instructions = new_record_line_buffer_reset(origin).unwrap();
        let bytes = assemble_at(origin, &instructions).unwrap();
        let mut memory = [0x5A; 0x10000];

        let write_count = execute_line_buffer_reset(&bytes, origin, &mut memory);

        let end = LINE_BUFFER_START + u16::from(LINE_BUFFER_BYTE_COUNT);
        assert_eq!(write_count, usize::from(LINE_BUFFER_BYTE_COUNT));
        assert!(
            memory[usize::from(LINE_BUFFER_START)..usize::from(end)]
                .iter()
                .all(|byte| *byte == LINE_BUFFER_BLANK)
        );
        assert_eq!(memory[usize::from(LINE_BUFFER_START - 1)], 0x5A);
        assert_eq!(memory[usize::from(end)], 0x5A);
    }

    #[test]
    fn only_new_record_resolution_owns_the_physical_row_reset() {
        let reset = assemble_at(0x9000, &new_record_line_buffer_reset(0x9000).unwrap()).unwrap();
        let initial = build_resolve_request(0xA400, layout()).unwrap();
        let next = build_resolve_next_page_request(0xA700, layout()).unwrap();

        assert!(
            initial
                .bytes
                .windows(reset.len())
                .any(|window| window == reset),
            "new-record resolution never clears the physical rows"
        );
        assert!(
            !next
                .bytes
                .windows(reset.len())
                .any(|window| window == reset),
            "same-record page advance must preserve the current row lifetime"
        );
    }

    /// 새 대사 수명은 조회 성공 여부와 관계없이 이전 정체성과 전송 커서를 모두
    /// 지운다. 단, 연속 수명의 게시 정체성은 그 전에 읽어야 한다.
    #[test]
    fn an_initial_request_selects_its_identity_before_clearing_dialogue_state() {
        let routine = build_resolve_request(0xA400, layout()).unwrap();
        let published = assemble_at(
            0x8000,
            &[
                Instruction::LdxAbsolute(PUBLISHED_SOURCE_DIRECTORY_SELECTOR),
                Instruction::LdaAbsolute(PUBLISHED_SOURCE_ENTRY_INDEX),
                Instruction::StaZeroPage(0x05),
            ],
        );
        let published = published.unwrap();
        let published_at = routine
            .bytes
            .windows(published.len())
            .position(|window| window == published)
            .expect("the continuing identity is never selected");
        let clear = runtime_state_clear_bytes();
        let clear_at = routine
            .bytes
            .windows(clear.len())
            .position(|window| window == clear)
            .expect("the previous dialogue state is never cleared");

        assert!(published_at + published.len() <= clear_at);
        assert!(!routine.bytes.windows(3).any(|window| {
            window
                == [
                    0x8D,
                    CONSUMER_FONT_PAGE as u8,
                    (CONSUMER_FONT_PAGE >> 8) as u8,
                ]
        }));
    }

    #[test]
    fn a_request_freezes_the_live_lookahead_identity_for_the_next_transition() {
        let routine = build_resolve_request(0xA400, layout()).unwrap();
        let capture = assemble_at(
            0x8000,
            &[
                Instruction::LdaAbsolute(SOURCE_DIRECTORY_SELECTOR),
                Instruction::StaAbsolute(REQUEST_SOURCE_DIRECTORY_SELECTOR),
                Instruction::LdaAbsolute(SOURCE_ENTRY_INDEX),
                Instruction::StaAbsolute(REQUEST_SOURCE_ENTRY_INDEX),
            ],
        )
        .unwrap();

        assert!(
            routine
                .bytes
                .windows(capture.len())
                .any(|window| window == capture)
        );
    }

    #[test]
    fn a_continuing_request_resolves_the_previously_published_identity() {
        let routine = build_resolve_request(0xA400, layout()).unwrap();
        let selection = assemble_at(
            0x8000,
            &[
                Instruction::CpxImmediate(LOOKUP_PUBLISHED_SOURCE_IDENTITY),
                Instruction::BeqAbsolute(0x8005),
            ],
        )
        .unwrap();
        let published = assemble_at(
            0x8000,
            &[
                Instruction::LdxAbsolute(PUBLISHED_SOURCE_DIRECTORY_SELECTOR),
                Instruction::LdaAbsolute(PUBLISHED_SOURCE_ENTRY_INDEX),
                Instruction::StaZeroPage(0x05),
            ],
        )
        .unwrap();

        assert!(routine.bytes.windows(selection.len()).any(|window| {
            window[0] == selection[0] && window[1] == selection[1] && window[2] == 0xF0
        }));
        let published_at = routine
            .bytes
            .windows(published.len())
            .position(|window| window == published)
            .expect("the continuing identity is never selected");
        let clear = runtime_state_clear_bytes();
        let clear_at = routine
            .bytes
            .windows(clear.len())
            .position(|window| window == clear)
            .expect("the previous dialogue state is never cleared");
        let capture = assemble_at(
            0x8000,
            &[
                Instruction::LdaAbsolute(SOURCE_DIRECTORY_SELECTOR),
                Instruction::StaAbsolute(REQUEST_SOURCE_DIRECTORY_SELECTOR),
                Instruction::LdaAbsolute(SOURCE_ENTRY_INDEX),
                Instruction::StaAbsolute(REQUEST_SOURCE_ENTRY_INDEX),
            ],
        )
        .unwrap();
        let capture_at = routine
            .bytes
            .windows(capture.len())
            .position(|window| window == capture)
            .expect("the live lookahead identity is never captured");

        assert!(published_at + published.len() <= clear_at);
        assert!(clear_at + clear.len() <= capture_at);
    }

    /// 식별 자료는 `B1` 같은 전체 원문 selector를 키로 직렬화한다. 여기서 상위
    /// 니블을 버리면 아이템 결과뿐 아니라 30/40/71/80/B0/C0 계열도 모두 실패한다.
    #[test]
    fn both_identity_paths_use_the_full_directory_selector_byte() {
        let routine = build_resolve_request(0xA400, layout()).unwrap();

        for selector in [
            SOURCE_DIRECTORY_SELECTOR,
            PUBLISHED_SOURCE_DIRECTORY_SELECTOR,
        ] {
            let load = assemble_at(0x8000, &[Instruction::LdxAbsolute(selector)]).unwrap();
            assert!(
                routine
                    .bytes
                    .windows(load.len())
                    .any(|window| window == load),
                "selector {selector:04X} is not used as the full identity-table key"
            );
        }
    }

    /// 원본 엔트리 0도 유효하다. 엔트리를 임시 저장한 직후 Z 플래그에 기대어
    /// `BNE`로 합류하면 0번만 게시 정체성 경로로 잘못 떨어진다.
    #[test]
    fn the_live_identity_path_joins_unconditionally_for_entry_zero() {
        let routine = build_resolve_request(0xA400, layout()).unwrap();
        let mut live_identity = assemble_at(
            0x8000,
            &[
                Instruction::LdxAbsolute(SOURCE_DIRECTORY_SELECTOR),
                Instruction::LdaAbsolute(SOURCE_ENTRY_INDEX),
                Instruction::StaZeroPage(0x05),
            ],
        )
        .unwrap();
        live_identity.push(0x4C);

        assert!(
            routine
                .bytes
                .windows(live_identity.len())
                .any(|window| window == live_identity),
            "the live identity path must not make entry zero control flow"
        );
    }

    #[test]
    fn identity_entry_offsets_are_relative_to_the_mapped_material_base() {
        let routine = build_resolve_request(0xA400, layout()).unwrap();
        let address = assemble_at(
            0x8000,
            &[
                Instruction::LdaAbsoluteX(layout().identity_table_descriptors + 2),
                Instruction::Clc,
                Instruction::AdcImmediate(layout().identity_material_base as u8),
                Instruction::StaZeroPage(0x00),
                Instruction::LdaAbsoluteX(layout().identity_table_descriptors + 3),
                Instruction::AdcImmediate((layout().identity_material_base >> 8) as u8),
                Instruction::StaZeroPage(0x01),
            ],
        )
        .unwrap();

        assert!(
            routine
                .bytes
                .windows(address.len())
                .any(|window| window == address)
        );
    }

    #[test]
    fn a_next_page_request_keeps_the_published_record_identity() {
        let routine = build_resolve_next_page_request(0xA700, layout()).unwrap();
        let capture = assemble_at(
            0x8000,
            &[
                Instruction::LdaAbsolute(PUBLISHED_SOURCE_DIRECTORY_SELECTOR),
                Instruction::StaAbsolute(REQUEST_SOURCE_DIRECTORY_SELECTOR),
                Instruction::LdaAbsolute(PUBLISHED_SOURCE_ENTRY_INDEX),
                Instruction::StaAbsolute(REQUEST_SOURCE_ENTRY_INDEX),
            ],
        )
        .unwrap();

        assert!(routine.bytes.starts_with(&capture));
    }

    /// 다음 페이지는 레코드 정체성을 새로 찾지 않고 현재 페이지 색인만 하나 올린다.
    /// 레코드 색인을 덮으면 디렉터리의 다른 레코드를 읽게 된다.
    #[test]
    fn a_next_page_request_advances_only_the_visible_page_identity() {
        let routine = build_resolve_next_page_request(0xA700, layout()).unwrap();
        let increment = [
            0xEE,
            VISIBLE_PAGE_INDEX as u8,
            (VISIBLE_PAGE_INDEX >> 8) as u8,
        ];

        assert!(routine.bytes.windows(3).any(|window| window == increment));
        for identity in [RECORD_INDEX_LOW, RECORD_INDEX_HIGH] {
            let store = [0x8D, identity as u8, (identity >> 8) as u8];
            assert!(
                !routine.bytes.windows(3).any(|window| window == store),
                "next-page resolution overwrites record identity {identity:04X}"
            );
        }
    }

    /// 생성 자료가 직전 페이지와 같은 레시피를 가리키면 다음 페이지는 이미 완성된
    /// CHR-RAM을 다시 쓰지 않는다. 최초 페이지에는 상주 기반이 없으므로 같은 표식을
    /// 받아도 ready로 승격해서는 안 된다.
    #[test]
    fn only_next_page_can_publish_resident_recipe_reuse_as_ready() {
        let initial = build_resolve_request(0xA400, layout()).unwrap();
        let next = build_resolve_next_page_request(0xA700, layout()).unwrap();
        let ready_without_transport = assemble_at(
            0x8000,
            &[
                Instruction::LdaImmediate(STATE_READY),
                Instruction::StaAbsolute(REQUEST_STATE),
                Instruction::Clc,
            ],
        )
        .unwrap();

        assert!(
            next.bytes
                .windows(ready_without_transport.len())
                .any(|window| window == ready_without_transport)
        );
        assert!(
            !initial
                .bytes
                .windows(ready_without_transport.len())
                .any(|window| window == ready_without_transport)
        );
    }

    /// 레코드 디렉터리의 다음 16비트 항목이 현재 레코드의 끝이다. 다음 페이지
    /// resolver도 그 끝을 읽고 비교해야 마지막 페이지 다음의 레코드로 새지 않는다.
    #[test]
    fn both_resolvers_bound_the_page_against_the_next_directory_entry() {
        for routine in [
            build_resolve_request(0xA400, layout()).unwrap(),
            build_resolve_next_page_request(0xA700, layout()).unwrap(),
        ] {
            assert!(
                routine
                    .bytes
                    .windows(2)
                    .any(|window| window == [0xA0, 0x02]),
                "{} never reads the next record-directory entry",
                routine.role
            );
            assert!(
                routine
                    .bytes
                    .windows(2)
                    .any(|window| window == [0xC5, 0x03])
                    && routine
                        .bytes
                        .windows(2)
                        .any(|window| window == [0xC5, 0x02]),
                "{} never compares the selected page offset with the 16-bit end",
                routine.role
            );
        }
    }

    #[test]
    fn only_next_page_exhaustion_suspends_the_completed_page() {
        let initial = build_resolve_request(0xA400, layout()).unwrap();
        let next = build_resolve_next_page_request(0xA700, layout()).unwrap();
        let suspension = [
            0xA9,
            STATE_COMPLETED_PAGE_SUSPENDED,
            0x8D,
            REQUEST_STATE as u8,
            (REQUEST_STATE >> 8) as u8,
        ];

        assert!(
            !initial
                .bytes
                .windows(suspension.len())
                .any(|window| window == suspension),
            "an initial lookup failure must not retain an unrelated old page"
        );

        let suspension_offset = next
            .bytes
            .windows(suspension.len())
            .position(|window| window == suspension)
            .expect("next-page exhaustion never suspends the completed page");
        let suspension_address = next.address + u16::try_from(suspension_offset).unwrap();
        let mut ordinary_failure_tail = vec![0x18];
        for address in BORROWED_SCRATCH.iter().rev() {
            ordinary_failure_tail.extend([0x68, 0x85, *address]);
        }
        ordinary_failure_tail.push(0x60);
        assert!(next.bytes.ends_with(&ordinary_failure_tail));
        let ordinary_failure_address =
            next.address + u16::try_from(next.bytes.len() - ordinary_failure_tail.len()).unwrap();
        let jump_targets = next
            .bytes
            .windows(3)
            .filter(|window| window[0] == 0x4C)
            .map(|window| u16::from_le_bytes([window[1], window[2]]))
            .collect::<Vec<_>>();

        assert_eq!(
            jump_targets
                .iter()
                .filter(|target| **target == suspension_address)
                .count(),
            2,
            "the high-byte and same-high-byte record-end checks must share the suspension exit"
        );
        assert!(
            jump_targets.contains(&ordinary_failure_address),
            "malformed or empty recipe failures must remain inactive instead of reusing stale material"
        );
    }

    /// 실패는 커서를 세우지 않고 캐리를 지워서 알려야 한다. 세워 버리면 생산자가
    /// 쓰레기 요청을 발행하고 소비자가 남의 자료를 CHR RAM에 올린다.
    #[test]
    fn every_failure_path_clears_carry_without_publishing_a_request() {
        let routine = build_resolve_request(0xA400, layout()).unwrap();

        // 실패 꼬리는 캐리를 지우고, 빌린 제로 페이지를 되돌리고, 돌아간다.
        let mut expected = vec![0x18];
        for address in BORROWED_SCRATCH.iter().rev() {
            expected.extend([0x68, 0x85, *address]);
        }
        expected.push(0x60);

        assert_eq!(
            &routine.bytes[routine.bytes.len() - expected.len()..],
            &expected[..]
        );
    }

    /// 성공은 캐리를 세우고 돌아간다. 생산자는 그 캐리만 보고 요청을 발행한다.
    #[test]
    fn the_success_path_sets_carry_after_writing_every_cursor_byte() {
        let routine = build_resolve_request(0xA400, layout()).unwrap();
        // `SEC` 뒤에 되돌리기가 오고 `RTS`로 끝난다. `PLA`는 캐리를 건드리지 않는다.
        let success_end = routine
            .bytes
            .iter()
            .position(|byte| *byte == 0x38)
            .expect("the resolver has a success tail");

        for cursor in [
            CURSOR_ENTRY_LOW,
            CURSOR_ENTRY_HIGH,
            CURSOR_RECIPE_PAGE,
            CURSOR_REMAINING_TILES,
            CURSOR_PHASE,
            CURSOR_OVERLAY_TILES,
        ] {
            let store = [0x8D, cursor as u8, (cursor >> 8) as u8];
            let at = routine
                .bytes
                .windows(3)
                .position(|window| window == store)
                .unwrap_or_else(|| panic!("cursor {cursor:04X} is never written"));
            assert!(
                at < success_end,
                "cursor {cursor:04X} is written after success"
            );
        }
    }

    /// 조회는 식별표 페이지와 스캔 페이지를 모두 걸어야 한다. 한쪽만 걸면 다른 쪽이
    /// 남의 자료를 표로 읽는다.
    #[test]
    fn the_resolver_maps_every_page_it_reads() {
        let initial = build_resolve_request(0xA400, layout()).unwrap();
        let next = build_resolve_next_page_request(0xA700, layout()).unwrap();

        for (routine, page) in [
            (&initial, layout().identity_page),
            (&initial, layout().scan_index_page),
            (&next, layout().scan_index_page),
        ] {
            let select = [0xA9, PRG_8000_REGISTER, 0x20, 0x58, 0xFA, 0xA9, page];
            assert!(
                routine.bytes.windows(7).any(|window| window == select),
                "the resolver never maps page {page:02X}"
            );
        }
    }
}
