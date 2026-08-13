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
    CURSOR_ENTRY_HIGH, CURSOR_ENTRY_LOW, CURSOR_GROUP_PAGE, CURSOR_REMAINING_TILES,
};
use super::{RuntimeRoutine, next_address};
use crate::rp2a03::{Instruction, assemble_at};

/// 원본이 디렉터리 선택자를 담아 두는 자리다.
const SOURCE_DIRECTORY_SELECTOR: u16 = 0x77F4;
/// 원본이 엔트리 색인을 담아 두는 자리다.
const SOURCE_ENTRY_INDEX: u16 = 0x77F1;
/// 식별표에서 «없는 선택자»를 뜻하는 값이다.
const MISSING_TABLE: u8 = 0xFF;

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

/// 캐리를 세워 성공, 지워서 실패를 알린다.
pub(in crate::full_translation_install) fn build_resolve_request(
    origin: u16,
    layout: MaterialLayout,
) -> Result<RuntimeRoutine> {
    let mut instructions = Vec::new();
    let mut failure_branches = Vec::new();

    // 1. 식별표에서 레코드 색인을 얻는다.
    instructions.extend(map_page(Instruction::LdaImmediate(layout.identity_page)));
    instructions.extend([
        Instruction::LdxAbsolute(SOURCE_DIRECTORY_SELECTOR),
        Instruction::LdaAbsoluteX(layout.identity_selector_directory),
        Instruction::CmpImmediate(MISSING_TABLE),
    ]);
    failure_branches.push(branch_to_failure(
        &mut instructions,
        origin,
        Instruction::BneAbsolute,
    )?);

    // 표 서술자는 네 바이트씩이다. 표 번호를 네 배 해서 자리를 찾는다.
    instructions.extend([
        Instruction::AslAccumulator,
        Instruction::AslAccumulator,
        Instruction::Tax,
        // 서술자는 `[선택자][엔트리 수][엔트리 오프셋 하위][상위]`다.
        Instruction::LdaAbsoluteX(layout.identity_table_descriptors + 1),
        Instruction::CmpAbsolute(SOURCE_ENTRY_INDEX),
    ]);
    // 엔트리 색인이 엔트리 수 이상이면 그 표에 없는 항목이다.
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

    // 엔트리 오프셋에 색인×2를 더해 레코드 색인 두 바이트를 읽는다.
    instructions.extend([
        Instruction::LdaAbsoluteX(layout.identity_table_descriptors + 2),
        Instruction::StaZeroPage(0x00),
        Instruction::LdaAbsoluteX(layout.identity_table_descriptors + 3),
        Instruction::StaZeroPage(0x01),
        Instruction::LdaAbsolute(SOURCE_ENTRY_INDEX),
        Instruction::AslAccumulator,
        Instruction::Tay,
        Instruction::LdaIndirectY(0x00),
        Instruction::StaZeroPage(0x02),
        Instruction::Iny,
        Instruction::LdaIndirectY(0x00),
        Instruction::StaZeroPage(0x03),
        // 레코드 색인 `FFFF`는 «없음»이다.
        Instruction::AndZeroPage(0x02),
        Instruction::CmpImmediate(0xFF),
    ]);
    failure_branches.push(branch_to_failure(
        &mut instructions,
        origin,
        Instruction::BneAbsolute,
    )?);

    // 2. 레코드 디렉터리에서 그 레코드의 첫 페이지 선택자를 읽는다.
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
        Instruction::LdyImmediate(0),
        Instruction::LdaIndirectY(0x00),
        Instruction::StaZeroPage(0x04),
        Instruction::Iny,
        Instruction::LdaIndirectY(0x00),
        Instruction::StaZeroPage(0x05),
        // 첫 가시 페이지의 선택자를 읽는다.
        Instruction::Clc,
        Instruction::LdaZeroPage(0x04),
        Instruction::AdcImmediate(layout.page_selectors as u8),
        Instruction::StaZeroPage(0x00),
        Instruction::LdaZeroPage(0x05),
        Instruction::AdcImmediate((layout.page_selectors >> 8) as u8),
        Instruction::StaZeroPage(0x01),
        Instruction::LdyImmediate(0),
        Instruction::LdaIndirectY(0x00),
        // 3. 그룹 선택자 × 2가 덩이 오프셋 표의 자리다.
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

    // 4. 덩이의 첫 바이트가 항목 수다. 항목은 그다음부터다.
    instructions.extend(map_page(Instruction::LdaAbsolute(CURSOR_GROUP_PAGE)));
    instructions.extend([
        Instruction::LdaZeroPage(0x02),
        Instruction::StaZeroPage(0x00),
        Instruction::LdaZeroPage(0x03),
        Instruction::StaZeroPage(0x01),
        Instruction::LdyImmediate(0),
        Instruction::LdaIndirectY(0x00),
        Instruction::StaAbsolute(CURSOR_REMAINING_TILES),
    ]);
    // 항목 수가 0인 그룹은 올릴 것이 없다. 요청을 세우지 않는다.
    failure_branches.push(branch_to_failure(
        &mut instructions,
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
        Instruction::Sec,
        Instruction::Rts,
    ]);

    let failure = next_address(origin, &instructions)?;
    for index in failure_branches {
        instructions[index] = Instruction::JmpAbsolute(failure);
    }
    instructions.extend([Instruction::Clc, Instruction::Rts]);

    let bytes = assemble_at(origin, &instructions)
        .context("cannot assemble the dialogue cold request resolver")?;
    Ok(RuntimeRoutine {
        role: "dialogue cold request resolver",
        address: origin,
        bytes,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn layout() -> MaterialLayout {
        MaterialLayout {
            identity_page: 0x2F,
            identity_selector_directory: 0x9424,
            identity_table_descriptors: 0x9524,
            scan_page: 0x2C,
            page_selectors: 0x9676,
            record_directory: 0x9A16,
            group_directory: 0x9E08,
            group_block_container_base: 7_758,
            container_first_page: 0x2C,
        }
    }

    /// 실패는 커서를 세우지 않고 캐리를 지워서 알려야 한다. 세워 버리면 생산자가
    /// 쓰레기 요청을 발행하고 소비자가 남의 자료를 CHR RAM에 올린다.
    #[test]
    fn every_failure_path_clears_carry_without_publishing_a_request() {
        let routine = build_resolve_request(0xA400, layout()).unwrap();

        // 마지막 두 바이트가 실패 꼬리다.
        assert_eq!(&routine.bytes[routine.bytes.len() - 2..], [0x18, 0x60]);
    }

    /// 성공은 캐리를 세우고 돌아간다. 생산자는 그 캐리만 보고 요청을 발행한다.
    #[test]
    fn the_success_path_sets_carry_after_writing_every_cursor_byte() {
        let routine = build_resolve_request(0xA400, layout()).unwrap();
        let success_end = routine
            .bytes
            .windows(2)
            .position(|window| window == [0x38, 0x60])
            .expect("the resolver has a success tail");

        for cursor in [
            CURSOR_ENTRY_LOW,
            CURSOR_ENTRY_HIGH,
            CURSOR_GROUP_PAGE,
            CURSOR_REMAINING_TILES,
        ] {
            let store = [0x8D, cursor as u8, (cursor >> 8) as u8];
            let at = routine
                .bytes
                .windows(3)
                .position(|window| window == store)
                .unwrap_or_else(|| panic!("cursor {cursor:04X} is never written"));
            assert!(at < success_end, "cursor {cursor:04X} is written after success");
        }
    }

    /// 조회는 식별표 페이지와 스캔 페이지를 모두 걸어야 한다. 한쪽만 걸면 다른 쪽이
    /// 남의 자료를 표로 읽는다.
    #[test]
    fn the_resolver_maps_every_page_it_reads() {
        let routine = build_resolve_request(0xA400, layout()).unwrap();

        for page in [layout().identity_page, layout().scan_page] {
            let select = [0xA9, PRG_8000_REGISTER, 0x8D, 0x00, 0x80, 0xA9, page];
            assert!(
                routine.bytes.windows(7).any(|window| window == select),
                "the resolver never maps page {page:02X}"
            );
        }
    }
}
