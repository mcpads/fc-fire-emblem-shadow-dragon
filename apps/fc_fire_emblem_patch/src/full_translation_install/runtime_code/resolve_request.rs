//! 대사 진입에서 «어느 그룹을 올려야 하는가»를 찾아 커서를 세운다.
//!
//! 이 루틴은 NMI가 아니라 주 흐름에서 돈다. 생산자가 대사에 들어가는 순간 한 번만
//! 불리므로 vblank 예산과 무관하다. 그래서 여기서는 사이클보다 정확성을 본다.
//!
//! 찾는 순서는 넷이다.
//!
//! 1. `$77F4`(디렉터리 선택자)와 `$77F1`(엔트리 색인)로 런타임 식별표에서 레코드
//!    색인을 얻는다.
//! 2. 레코드 색인으로 스캔 재료의 레코드 디렉터리에서 그 레코드의 페이지 선택자
//!    구간을 얻고, 첫 가시 페이지의 그룹 선택자를 읽는다.
//! 3. 그룹 선택자로 그룹 덩이 오프셋 표에서 덩이의 자리를 얻는다.
//! 4. 덩이의 첫 바이트가 항목 수다. 그것과 항목 시작 주소·페이지를 커서에 쓴다.
//!
//! 어느 단계든 범위를 벗어나면 커서를 세우지 않고 실패로 돌아간다. 생산자는 그때
//! 요청을 발행하지 않으므로 원본 일본어 경로가 그대로 돈다. 설계의 «모든 실패는
//! 원본 동작으로 되돌아감»이 여기서 성립한다.

use anyhow::{Context, Result};

use super::super::runtime_cursor_storage::{
    CURSOR_ENTRY_HIGH, CURSOR_ENTRY_LOW, CURSOR_GROUP_PAGE, CURSOR_OVERLAY_TILES, CURSOR_PHASE,
    CURSOR_REMAINING_TILES, PUBLISHED_SOURCE_DIRECTORY_SELECTOR, PUBLISHED_SOURCE_ENTRY_INDEX,
    REQUEST_SOURCE_DIRECTORY_SELECTOR, REQUEST_SOURCE_ENTRY_INDEX,
};
use super::super::runtime_state_storage::{
    CANDIDATE_END, CANDIDATE_START, CURRENT_PAGE_GROUP, RECORD_INDEX_HIGH, RECORD_INDEX_LOW,
    REQUEST_STATE, VISIBLE_PAGE_INDEX,
};
use super::transport::{PHASE_RESTORE, RESTORE_CHUNK_COUNT};
use super::{RuntimeRoutine, next_address};
use crate::rp2a03::{Instruction, assemble_at};

pub(in crate::full_translation_install) const INITIAL_PAGE_REQUEST_RESOLVER_ROLE: &str =
    "dialogue initial-page request resolver";

/// 원본이 디렉터리 선택자를 담아 두는 자리다.
pub(super) const SOURCE_DIRECTORY_SELECTOR: u16 = 0x77F4;
/// 원본이 엔트리 색인을 담아 두는 자리다.
pub(super) const SOURCE_ENTRY_INDEX: u16 = 0x77F1;
/// 식별표에서 «없는 선택자»를 뜻하는 값이다.
const MISSING_TABLE: u8 = 0xFF;
/// 새 대사 수명에서 살아 있는 원본 selector/index를 현재 레코드로 해석한다.
pub(super) const LOOKUP_LIVE_SOURCE_IDENTITY: u8 = 0;
/// 연속 대사에서 직전에 게시한 선행 조회값을 현재 레코드로 승격한다.
pub(super) const LOOKUP_PUBLISHED_SOURCE_IDENTITY: u8 = 1;

const BANK_SELECT_REGISTER: u16 = 0x8000;
const BANK_VALUE_REGISTER: u16 = 0x8001;
const PRG_8000_REGISTER: u8 = 6;
/// 자료 창의 시작이다. 재료 페이지가 여기 걸린다.
const DATA_WINDOW_BASE: u16 = 0x8000;
/// MMC3 페이지 하나가 자료 창에서 차지하는 크기다.
const DATA_WINDOW_SIZE: u16 = 0x2000;

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
    /// 스캔 조회표가 들어 있는 MMC3 페이지다.
    pub(in crate::full_translation_install) scan_page: u8,
    /// 페이지 작업집합마다 하나씩인 그룹 선택자 배열의 CPU 주소다.
    pub(in crate::full_translation_install) page_selectors: u16,
    /// 레코드마다 두 바이트인 선택자 구간 표의 CPU 주소다.
    pub(in crate::full_translation_install) record_directory: u16,
    /// 그룹마다 두 바이트인 덩이 오프셋 표의 CPU 주소다.
    pub(in crate::full_translation_install) group_directory: u16,
    /// 그룹 덩이들이 재료 용기 안에서 시작하는 자리다.
    pub(in crate::full_translation_install) group_block_container_base: u16,
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
const BORROWED_SCRATCH: [u8; 6] = [0x00, 0x01, 0x02, 0x03, 0x04, 0x05];

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
        Instruction::StaAbsolute(BANK_SELECT_REGISTER),
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
    for address in CANDIDATE_START..=CANDIDATE_END {
        instructions.push(Instruction::StaAbsolute(address));
    }
}

/// 영속 레코드 색인 `$07F0/1`과 가시 페이지 색인 `$07F2`를 사용해 페이지 그룹과
/// 전송 커서를 세운다. 레코드 디렉터리의 다음 항목이 현재 레코드의 끝이므로, 선택할
/// 페이지 오프셋이 그 끝보다 작은지도 함께 확인한다.
fn append_page_request_resolution(
    instructions: &mut Vec<Instruction>,
    failure_branches: &mut Vec<usize>,
    origin: u16,
    layout: MaterialLayout,
) -> Result<()> {
    instructions.extend(map_page(Instruction::LdaImmediate(layout.scan_page)));
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
    failure_branches.push(branch_to_failure(
        instructions,
        origin,
        Instruction::BeqAbsolute,
    )?);
    instructions.extend([
        Instruction::LdaZeroPage(0x04),
        Instruction::CmpZeroPage(0x02),
    ]);
    // 같은 상위 바이트에서 하위 바이트가 끝 이상이어도 범위 밖이다.
    failure_branches.push(branch_to_failure(
        instructions,
        origin,
        Instruction::BccAbsolute,
    )?);
    let selected_page_is_bounded = next_address(origin, instructions)?;
    instructions[lower_high_byte] = Instruction::BccAbsolute(selected_page_is_bounded);

    instructions.extend([
        // 페이지 선택자 배열 안의 정확한 한 바이트를 읽는다.
        Instruction::Clc,
        Instruction::LdaZeroPage(0x04),
        Instruction::AdcImmediate(layout.page_selectors as u8),
        Instruction::StaZeroPage(0x00),
        Instruction::LdaZeroPage(0x05),
        Instruction::AdcImmediate((layout.page_selectors >> 8) as u8),
        Instruction::StaZeroPage(0x01),
        Instruction::LdyImmediate(0),
        Instruction::LdaIndirectY(0x00),
        Instruction::StaAbsolute(CURRENT_PAGE_GROUP),
        // 그룹 선택자 × 2가 덩이 오프셋 표의 자리다.
        Instruction::AslAccumulator,
        Instruction::Tax,
        Instruction::LdaAbsoluteX(layout.group_directory),
        Instruction::StaZeroPage(0x02),
        Instruction::LdaAbsoluteX(layout.group_directory + 1),
        Instruction::StaZeroPage(0x03),
        // 덩이의 용기 안 자리를 만든다.
        Instruction::Clc,
        Instruction::LdaZeroPage(0x02),
        Instruction::AdcImmediate(layout.group_block_container_base as u8),
        Instruction::StaZeroPage(0x02),
        Instruction::LdaZeroPage(0x03),
        Instruction::AdcImmediate((layout.group_block_container_base >> 8) as u8),
        Instruction::StaZeroPage(0x03),
        // 페이지는 상위 바이트의 위 세 비트에서, 창 안 주소는 나머지에서 나온다.
        Instruction::LsrAccumulator,
        Instruction::LsrAccumulator,
        Instruction::LsrAccumulator,
        Instruction::LsrAccumulator,
        Instruction::LsrAccumulator,
        Instruction::Clc,
        Instruction::AdcImmediate(layout.container_first_page),
        Instruction::StaAbsolute(CURSOR_GROUP_PAGE),
        Instruction::LdaZeroPage(0x03),
        Instruction::AndImmediate(((DATA_WINDOW_SIZE - 1) >> 8) as u8),
        Instruction::OraImmediate((DATA_WINDOW_BASE >> 8) as u8),
        Instruction::StaZeroPage(0x03),
    ]);

    // 덩이의 첫 바이트가 항목 수다. 항목은 그다음부터다.
    instructions.extend(map_page(Instruction::LdaAbsolute(CURSOR_GROUP_PAGE)));
    instructions.extend([
        Instruction::LdaZeroPage(0x02),
        Instruction::StaZeroPage(0x00),
        Instruction::LdaZeroPage(0x03),
        Instruction::StaZeroPage(0x01),
        Instruction::LdyImmediate(0),
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
        Instruction::AdcImmediate(1),
        Instruction::StaAbsolute(CURSOR_ENTRY_LOW),
        Instruction::LdaZeroPage(0x03),
        Instruction::AdcImmediate(0),
        Instruction::StaAbsolute(CURSOR_ENTRY_HIGH),
        Instruction::LdaImmediate(PHASE_RESTORE),
        Instruction::StaAbsolute(CURSOR_PHASE),
        Instruction::LdaImmediate(RESTORE_CHUNK_COUNT),
        Instruction::StaAbsolute(CURSOR_REMAINING_TILES),
    ]);
    Ok(())
}

fn finish_resolver(
    origin: u16,
    mut instructions: Vec<Instruction>,
    failure_branches: Vec<usize>,
    role: &'static str,
) -> Result<RuntimeRoutine> {
    instructions.push(Instruction::Sec);
    restore_scratch(&mut instructions);
    instructions.push(Instruction::Rts);

    let failure = next_address(origin, &instructions)?;
    for index in failure_branches {
        instructions[index] = Instruction::JmpAbsolute(failure);
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
/// 현재 레코드로 승격한다. 어느 쪽이든 살아 있는 값은 다음 호출의 승격 후보로 따로
/// 고정한다. 모든 휘발 상태를 먼저 지우므로 실패해도 selector가 이전 수명의
/// `ready`를 볼 수 없다.
pub(in crate::full_translation_install) fn build_resolve_request(
    origin: u16,
    layout: MaterialLayout,
) -> Result<RuntimeRoutine> {
    let mut instructions = Vec::new();
    let mut failure_branches = Vec::new();
    clear_runtime_state(&mut instructions);
    instructions.extend([
        Instruction::LdaAbsolute(SOURCE_DIRECTORY_SELECTOR),
        Instruction::StaAbsolute(REQUEST_SOURCE_DIRECTORY_SELECTOR),
        Instruction::LdaAbsolute(SOURCE_ENTRY_INDEX),
        Instruction::StaAbsolute(REQUEST_SOURCE_ENTRY_INDEX),
    ]);
    save_scratch(&mut instructions);

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

    append_page_request_resolution(&mut instructions, &mut failure_branches, origin, layout)?;
    finish_resolver(
        origin,
        instructions,
        failure_branches,
        INITIAL_PAGE_REQUEST_RESOLVER_ROLE,
    )
}

/// 같은 레코드의 다음 가시 페이지를 찾는다. 호출자는 원본 완료 상태가 실제로
/// `09` 계속을 선택한 경계이므로, 이 루틴은 페이지를 정확히 한 칸만 올린다.
/// 디렉터리의 끝을 넘으면 실패하며 요청은 `inactive`로 남는다.
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
        Instruction::IncAbsolute(VISIBLE_PAGE_INDEX),
        Instruction::LdaAbsolute(RECORD_INDEX_LOW),
        Instruction::StaZeroPage(0x02),
        Instruction::LdaAbsolute(RECORD_INDEX_HIGH),
        Instruction::StaZeroPage(0x03),
    ]);
    append_page_request_resolution(&mut instructions, &mut failure_branches, origin, layout)?;
    finish_resolver(
        origin,
        instructions,
        failure_branches,
        "dialogue next-page request resolver",
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    fn layout() -> MaterialLayout {
        MaterialLayout {
            identity_page: 0x2F,
            identity_material_base: 0x9800,
            identity_selector_directory: 0x9424,
            identity_table_descriptors: 0x9524,
            scan_page: 0x2C,
            page_selectors: 0x9676,
            record_directory: 0x9A16,
            group_directory: 0x9E08,
            group_block_container_base: 7_758,
            container_first_page: 0x2C,
            producer_encoding_page: 0x2F,
            producer_item_directory: 0x9000,
            producer_unit_directory: 0x9100,
            producer_location_directory: 0x9200,
            producer_encoding_base: 0x8F00,
        }
    }

    /// 새 대사 수명은 조회 성공 여부와 관계없이 이전 정체성과 전송 커서를 먼저
    /// 지운다. 하나라도 남으면 실패 경로가 이전 `ready`를 재사용할 수 있다.
    #[test]
    fn an_initial_request_clears_the_whole_volatile_reservation_first() {
        let routine = build_resolve_request(0xA400, layout()).unwrap();
        let mut expected = vec![0xA9, 0x00];
        for address in CANDIDATE_START..=CANDIDATE_END {
            expected.extend([0x8D, address as u8, (address >> 8) as u8]);
        }

        assert!(routine.bytes.starts_with(&expected));
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
        assert!(
            routine
                .bytes
                .windows(published.len())
                .any(|window| window == published)
        );
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
            CURSOR_GROUP_PAGE,
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
            (&initial, layout().scan_page),
            (&next, layout().scan_page),
        ] {
            let select = [0xA9, PRG_8000_REGISTER, 0x8D, 0x00, 0x80, 0xA9, page];
            assert!(
                routine.bytes.windows(7).any(|window| window == select),
                "the resolver never maps page {page:02X}"
            );
        }
    }
}
