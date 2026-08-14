//! 대사 상태 머신을 붙잡는 게이트와, 원문 정체성별 요청 발행기다.
//!
//! 게이트가 처리기를 붙잡고 있는 동안 원본은 PPU 큐에 아무것도 넣지 않는다.
//! 그래서 그 프레임들이 조용해지고 전송이 굶지 않는다. «올라가기 전에 출력하지
//! 않는다»는 안전 성질이 구조적으로 성립하는 이유가 이것이다.
//!
//! 원본 디스패처는 이렇다.
//!
//! ```text
//! 0A:$8000  LDA $77F7
//! 0A:$8003  JSR $C34C     ; 처리기 표가 이 호출 뒤에 인라인으로 붙어 있다
//! 0A:$8006  <표>
//! ```
//!
//! `$C34C`는 스택의 복귀 주소로 표를 찾는다. 그래서 게이트는 `$8003`을 그대로
//! 실행시켜야 하고, `$8000`의 세 바이트만 `JMP`로 바꿔 앞에 끼어든다.

use anyhow::{Context, Result, ensure};

use super::{RuntimeRoutine, next_address};
use crate::{
    dialogue_inventory::switchable_cpu_to_file_offset,
    rom::Rom,
    rp2a03::{Instruction, assemble_at},
    typed_source::decode_rp2a03_sequence,
};

/// 대사 디스패처 입구다. 게이트가 이 세 바이트를 가져간다.
pub(in crate::full_translation_install) const DISPATCHER_ENTRY: u16 = 0x8000;
/// 원본이 입구에서 읽는 상태 바이트다.
pub(in crate::full_translation_install) const DISPATCHER_STATE: u16 = 0x77F7;
/// 표 분기 호출이다. 게이트는 통과할 때 이 자리로 되돌린다.
pub(in crate::full_translation_install) const DISPATCHER_TABLE_CALL: u16 = 0x8003;
/// 대사 초기 진입이다. 요청 발행기가 이 세 바이트를 가져간다.
pub(in crate::full_translation_install) const COLD_ENTRY: u16 = 0x809B;
/// 초기 진입이 부르는 원본 포인터 resolver다.
pub(in crate::full_translation_install) const SOURCE_POINTER_RESOLVER: u16 = 0xE6B2;

use super::super::runtime_bank_contract::PRG_A000_REGISTER;
use super::super::runtime_cursor_storage::{
    PUBLISHED_SOURCE_DIRECTORY_SELECTOR, PUBLISHED_SOURCE_ENTRY_INDEX,
};
use super::super::runtime_nmi_contract::PPU_CONTROL_SHADOW;
use super::super::runtime_state_storage::CURRENT_PAGE_GROUP;
use super::resolve_request::{LOOKUP_LIVE_SOURCE_IDENTITY, LOOKUP_PUBLISHED_SOURCE_IDENTITY};
use super::resolve_request::{SOURCE_DIRECTORY_SELECTOR, SOURCE_ENTRY_INDEX};
use super::resolved_page_publication::NO_RESIDENT_PAGE_GROUP;
use super::transport::{REQUEST_STATE, STATE_READY};

/// 폐기된 표본 그룹 selector가 차지한 동굴이다. 전역 런타임에서는 그 selector를
/// 호출하지 않으므로 디스패처 게이트가 구간 전체를 digest에 묶어 되찾아 쓴다.
pub(super) const RECLAIMED_GATE_CAVE_ORIGIN: u16 = 0xF341;
pub(super) const RECLAIMED_GATE_CAVE_END: u16 = 0xF378;
pub(super) const EXPECTED_RECLAIMED_GATE_CAVE_SHA1: &str =
    "cea25e67f4399e422e8747046c13a959f5669ac1";

const BANK_SELECT_REGISTER: u16 = 0x8000;
const BANK_VALUE_REGISTER: u16 = 0x8001;
const PPU_CONTROL: u16 = 0x2000;
const NMI_ENABLE_MASK: u8 = 0x80;
/// 16 KiB 뱅크 짝을 되돌리는 원본 도우미다.
const PAIRED_BANK_HELPER: u16 = 0xFA20;

/// 합성을 기다리는 중이라는 요청이다.
pub(in crate::full_translation_install) const STATE_COLD_REQUESTED: u8 = 1;
/// 완성된 이전 그룹 위에 새 그룹 전체를 덮는 요청이다. 원본 4 KiB 복원은 생략하지만
/// 대상 그룹의 모든 글리프를 다시 써 같은 그룹의 후속 페이지까지 상주시킨다.
pub(in crate::full_translation_install) const STATE_RESIDENT_GROUP_OVERLAY_REQUESTED: u8 = 2;

/// 대사 뱅크다.
const MAIN_DIALOGUE_BANK: u8 = 0x0A;
/// `0A:$8000`: `LDA $77F7; JSR $C34C`. 게이트는 앞의 세 바이트만 가져가고 뒤의
/// 표 분기는 그대로 실행시킨다.
const DISPATCHER_ENTRY_CODE: [u8; 6] = [0xAD, 0xF7, 0x77, 0x20, 0x4C, 0xC3];

/// 게이트가 입구를 가져가기 전에 그 자리가 아직 원본인지 확인한다.
///
/// 표 분기 `$C34C`는 스택의 복귀 주소로 인라인 표를 찾는다. 그래서 `$8003`의 호출과
/// 그 길이가 바뀌면 게이트가 되돌릴 자리도 표의 시작도 함께 어긋난다.
pub(super) fn bind_dispatcher_entry(source: &Rom, candidate: &Rom) -> Result<()> {
    for rom in [source, candidate] {
        let offset = switchable_cpu_to_file_offset(MAIN_DIALOGUE_BANK, DISPATCHER_ENTRY)?;
        let bytes = rom
            .data()
            .get(offset..offset + DISPATCHER_ENTRY_CODE.len())
            .context("main-dialogue dispatcher entry is outside ROM")?;
        ensure!(
            bytes == DISPATCHER_ENTRY_CODE,
            "the main-dialogue dispatcher entry at 0A:{DISPATCHER_ENTRY:04X} changed"
        );
    }
    decode_rp2a03_sequence(
        &DISPATCHER_ENTRY_CODE,
        DISPATCHER_ENTRY,
        "main-dialogue dispatcher entry",
    )?;
    Ok(())
}

/// `0A:$8000`에 쓸 세 바이트다.
pub(super) fn dispatcher_hook_bytes(gate: u16) -> [u8; 3] {
    [0x4C, gate as u8, (gate >> 8) as u8]
}

/// `0A:$809B`에 쓸 세 바이트다.
pub(super) fn request_hook_bytes(publisher: u16) -> [u8; 3] {
    [0x20, publisher as u8, (publisher >> 8) as u8]
}

/// 요청이 걸려 있으면 처리기를 돌리지 않고 그대로 돌아간다.
pub(super) fn build_dispatcher_gate(origin: u16) -> Result<RuntimeRoutine> {
    let mut instructions = vec![Instruction::LdaAbsolute(REQUEST_STATE)];
    let inactive_placeholder = instructions.len();
    instructions.push(Instruction::BeqAbsolute(origin));
    // `ready` 이상이면 합성이 끝났거나 알 수 없는 값이다. 둘 다 원본을 돌린다.
    instructions.push(Instruction::CmpImmediate(STATE_READY));
    let settled_placeholder = instructions.len();
    instructions.push(Instruction::BcsAbsolute(origin));
    // 요청이 살아 있다. 이번 프레임에는 대사를 진행시키지 않는다.
    instructions.push(Instruction::Rts);

    let run_handler = next_address(origin, &instructions)?;
    instructions[inactive_placeholder] = Instruction::BeqAbsolute(run_handler);
    instructions[settled_placeholder] = Instruction::BcsAbsolute(run_handler);
    // 원본이 입구에서 하던 적재를 대신 하고 표 분기로 넘긴다.
    instructions.extend([
        Instruction::LdaAbsolute(DISPATCHER_STATE),
        Instruction::JmpAbsolute(DISPATCHER_TABLE_CALL),
    ]);

    Ok(RuntimeRoutine {
        role: "dialogue dispatcher gate",
        address: origin,
        bytes: assemble_at(origin, &instructions)?,
    })
}

/// 직접 진입과 E7 재개에서 같은 원문의 준비된 현재 페이지는 재사용하고, 그 밖에는
/// 해석기로 새 요청을 발행한다.
///
/// 해석기는 실행 코드 페이지에 있으므로 그 페이지를 `$A000`에 걸어야 부를 수 있다.
/// 걸면 원본이 기대하던 뱅크가 사라지므로 돌아오기 전에 되돌린다. 되돌리는 값은
/// `$29` 그림자와 원본 도우미 `$FA20`이 준다 — `$C1FB`가 매 프레임 쓰는 방식이다.
///
/// 같은 정체성의 반복 생산은 준비된 한글 페이지를 그대로 쓰되, 훅이 가져간 원본
/// resolver에는 제어를 넘긴다. 가시 페이지 전환은 각 줄의 원본 포인터 전진 경계에서
/// 처리하고, E4/E6가 미리 결속한 다음 레코드는 현재 화면의 완료 상태가 10이 된 뒤
/// 별도 생산자가 맡는다.
///
/// 실패하면 커서도 요청도 남기지 않는다. 그 경우 원본 일본어 경로가 그대로 돈다.
pub(super) fn build_source_identity_request_publisher(
    origin: u16,
    resolver: u16,
    code_page: u8,
    resolved_page_publication: u16,
) -> Result<RuntimeRoutine> {
    let mut instructions = vec![
        Instruction::LdaAbsolute(REQUEST_STATE),
        Instruction::CmpImmediate(STATE_READY),
    ];
    let completed_page = instructions.len();
    instructions.push(Instruction::BeqAbsolute(origin));

    instructions.extend([
        Instruction::LdaImmediate(NO_RESIDENT_PAGE_GROUP),
        // 새 수명에는 게시된 선행 조회값이 없으므로 살아 있는 원본 정체성이 현재
        // 레코드다.
        Instruction::LdxImmediate(LOOKUP_LIVE_SOURCE_IDENTITY),
    ]);
    let preserve_resident_group = instructions.len();
    instructions.push(Instruction::JmpAbsolute(origin));

    let ready_identity_comparison = next_address(origin, &instructions)?;
    instructions[completed_page] = Instruction::BeqAbsolute(ready_identity_comparison);
    instructions.push(Instruction::LdxImmediate(LOOKUP_PUBLISHED_SOURCE_IDENTITY));
    instructions.extend([
        Instruction::LdaAbsolute(SOURCE_DIRECTORY_SELECTOR),
        Instruction::CmpAbsolute(PUBLISHED_SOURCE_DIRECTORY_SELECTOR),
    ]);
    let rebuild_for_selector = instructions.len();
    instructions.push(Instruction::BneAbsolute(origin));
    instructions.extend([
        Instruction::LdaAbsolute(SOURCE_ENTRY_INDEX),
        Instruction::CmpAbsolute(PUBLISHED_SOURCE_ENTRY_INDEX),
    ]);
    let rebuild_for_entry = instructions.len();
    instructions.push(Instruction::BneAbsolute(origin));
    // 같은 전이 생산자가 반복돼도 한글 페이지는 그대로 재사용한다. 하지만 훅이
    // 가져간 원본 resolver 호출까지 생략하면 안 된다. E6 전이에서는 그 호출의
    // 복귀 A/플래그가 바깥 렌더러를 제어하고, 뒤이은 상태 처리가 새 더블버퍼의
    // `$7825+X` 메타데이터를 채울 수 있게 한다. 이를 RTS로 줄이면 길이 0인 새
    // 버퍼를 즉시 소비해 256바이트를 덮는다.
    instructions.push(Instruction::JmpAbsolute(SOURCE_POINTER_RESOLVER));

    let ready_rebuild = next_address(origin, &instructions)?;
    for index in [rebuild_for_selector, rebuild_for_entry] {
        instructions[index] = Instruction::BneAbsolute(ready_rebuild);
    }
    instructions.extend([
        Instruction::LdaAbsolute(CURRENT_PAGE_GROUP),
        // 준비된 수명의 전이는 직전에 게시한 선행 조회값을 현재 레코드로 승격한다.
    ]);

    let preserve_group = next_address(origin, &instructions)?;
    instructions[preserve_resident_group] = Instruction::JmpAbsolute(preserve_group);
    append_guarded_resolver_publication(
        &mut instructions,
        resolver,
        code_page,
        resolved_page_publication,
    );
    // 밀어낸 원본 호출로 넘긴다.
    instructions.push(Instruction::JmpAbsolute(SOURCE_POINTER_RESOLVER));

    Ok(RuntimeRoutine {
        role: "dialogue source-identity request publisher",
        address: origin,
        bytes: assemble_at(origin, &instructions)?,
    })
}

/// 직접 진입은 휘발 RAM의 이전 값을 상태로 해석하지 않고 반드시 새 수명을 연다.
/// `$07F4`가 우연히 `ready`인 채 시작해도 resolver가 열세 바이트 전체를 먼저 지우며,
/// 완성 기반이 없다는 `FF` 입력 때문에 첫 그룹은 항상 원본 4 KiB 복원부터 합성한다.
pub(super) fn build_initial_request_publisher(
    origin: u16,
    resolver: u16,
    code_page: u8,
    resolved_page_publication: u16,
) -> Result<RuntimeRoutine> {
    let mut instructions = vec![
        Instruction::LdaImmediate(NO_RESIDENT_PAGE_GROUP),
        Instruction::LdxImmediate(LOOKUP_LIVE_SOURCE_IDENTITY),
    ];
    append_guarded_resolver_publication(
        &mut instructions,
        resolver,
        code_page,
        resolved_page_publication,
    );
    instructions.push(Instruction::JmpAbsolute(SOURCE_POINTER_RESOLVER));

    Ok(RuntimeRoutine {
        role: "dialogue initial-entry request publisher",
        address: origin,
        bytes: assemble_at(origin, &instructions)?,
    })
}

/// A에 든 기존 상주 그룹을 보존하고, NMI가 `$A000` 실행 코드를 바꾸지 못하게 한
/// 채 resolver와 공통 발행기를 호출한다. 발행기가 요청 상태를 정한 뒤에만 NMI를
/// 되살린다.
fn append_guarded_resolver_publication(
    instructions: &mut Vec<Instruction>,
    resolver: u16,
    code_page: u8,
    resolved_page_publication: u16,
) {
    instructions.extend([
        Instruction::Pha,
        // NMI를 끄기 직전 한 프레임이 끼어도 이전 수명의 `ready`를 고르지 않는다.
        Instruction::LdaImmediate(0),
        Instruction::StaAbsolute(REQUEST_STATE),
        // `$A000`에서 해석기를 실행하는 동안 NMI가 원본 뱅크를 복원하면 복귀할
        // 코드가 사라진다. 현재 PPU 제어값을 보존하고 NMI만 잠깐 막는다.
        Instruction::LdaZeroPage(PPU_CONTROL_SHADOW),
        Instruction::Pha,
        Instruction::AndImmediate(!NMI_ENABLE_MASK),
        Instruction::StaZeroPage(PPU_CONTROL_SHADOW),
        Instruction::StaAbsolute(PPU_CONTROL),
        // 실행 코드 페이지를 `$A000`에 건다.
        Instruction::LdaImmediate(PRG_A000_REGISTER),
        Instruction::StaAbsolute(BANK_SELECT_REGISTER),
        Instruction::LdaImmediate(code_page),
        Instruction::StaAbsolute(BANK_VALUE_REGISTER),
        Instruction::JsrAbsolute(resolver),
        // 캐리를 뱅크 복원 너머로 나른다. `$FA20`은 상태를 보존하지만 그 사실에
        // 기대는 대신 여기서 명시적으로 밀어 둔다.
        Instruction::Php,
        // 이 루틴의 복귀 주소는 항상 주 대사 뱅크 0A에 있다. `$29`는 바깥 지도
        // 상태의 NMI 복귀 그림자일 수 있으므로 여기서 사용하면 다른 뱅크의 같은
        // CPU 주소로 돌아간다.
        Instruction::LdaImmediate(MAIN_DIALOGUE_BANK),
        Instruction::JsrAbsolute(PAIRED_BANK_HELPER),
        Instruction::Plp,
        // 발행기가 상태를 결정할 때까지 하드웨어 NMI는 꺼 둔다. `PLA`와 `STA`는
        // resolver의 캐리를 바꾸지 않는다.
        Instruction::Pla,
        Instruction::StaZeroPage(PPU_CONTROL_SHADOW),
        // 해석 전 상주 그룹을 A로 넘긴다. 캐리는 그대로 성공 여부다.
        Instruction::Pla,
        Instruction::JsrAbsolute(resolved_page_publication),
    ]);
}

#[cfg(test)]
mod tests {
    use super::*;

    const RESOLVED_PAGE_PUBLICATION: u16 = 0xF354;

    fn publisher() -> RuntimeRoutine {
        build_source_identity_request_publisher(0xF446, 0xA400, 0x30, RESOLVED_PAGE_PUBLICATION)
            .unwrap()
    }

    /// 요청이 걸린 동안 처리기가 돌면 아직 CHR RAM에 없는 글자가 화면에 나온다.
    /// 그것이 0원칙 위반이므로 게이트는 반드시 먼저 되돌아가야 한다.
    #[test]
    fn a_pending_request_returns_before_the_handler_can_run() {
        let routine = build_dispatcher_gate(0xF460).unwrap();
        let table_jump = [
            0x4C,
            DISPATCHER_TABLE_CALL as u8,
            (DISPATCHER_TABLE_CALL >> 8) as u8,
        ];

        let early_return = routine
            .bytes
            .iter()
            .position(|byte| *byte == 0x60)
            .expect("the gate has an early return");
        let handler_jump = routine
            .bytes
            .windows(3)
            .position(|window| window == table_jump)
            .expect("the gate can reach the handler");

        assert!(early_return < handler_jump);
    }

    /// 통과할 때는 원본이 입구에서 하던 일을 그대로 해야 한다. 표 분기는 스택의
    /// 복귀 주소로 표를 찾으므로 `$8003`을 건너뛰면 안 된다.
    #[test]
    fn the_pass_path_reproduces_the_source_entry_and_returns_to_the_table_call() {
        let routine = build_dispatcher_gate(0xF460).unwrap();
        let load_state = [0xAD, DISPATCHER_STATE as u8, (DISPATCHER_STATE >> 8) as u8];

        assert!(
            routine.bytes.windows(3).any(|window| window == load_state),
            "the gate never loads the dispatcher state the source entry loaded"
        );
        assert_eq!(
            &routine.bytes[routine.bytes.len() - 3..],
            [
                0x4C,
                DISPATCHER_TABLE_CALL as u8,
                (DISPATCHER_TABLE_CALL >> 8) as u8
            ]
        );
    }

    /// 입구 훅은 세 바이트를 가져간다. 길이가 달라지면 표의 시작이 밀린다.
    #[test]
    fn the_dispatcher_hook_keeps_the_entry_three_bytes_long() {
        assert_eq!(dispatcher_hook_bytes(0xF460).len(), 3);
        assert_eq!(DISPATCHER_TABLE_CALL - DISPATCHER_ENTRY, 3);
    }

    /// 입구가 바뀌면 표 분기의 복귀 주소가 어긋나므로 설치를 막는다.
    #[test]
    fn a_changed_dispatcher_entry_refuses_installation() {
        let offset = switchable_cpu_to_file_offset(MAIN_DIALOGUE_BANK, DISPATCHER_ENTRY).unwrap();
        let mut bytes = crate::test_support::synthetic_mapper165_rom_bytes(0xFF);
        bytes[offset..offset + DISPATCHER_ENTRY_CODE.len()].copy_from_slice(&DISPATCHER_ENTRY_CODE);
        let source = Rom::parse(bytes.clone()).unwrap();
        bytes[offset + 3] = 0xEA;
        let mutated = Rom::parse(bytes).unwrap();

        let error = bind_dispatcher_entry(&source, &mutated).unwrap_err();

        assert!(
            error
                .to_string()
                .contains("dispatcher entry at 0A:8000 changed")
        );
    }

    /// 해석 결과의 성공·실패와 상주 그룹 비교는 고정 발행기 하나가 맡아야 한다.
    /// 생산자가 그 판단 전에 cold를 직접 게시하면 실패한 커서를 소비자가 읽는다.
    #[test]
    fn resolver_result_is_delegated_without_direct_cold_publication() {
        let routine = publisher();
        let delegate = routine
            .bytes
            .windows(3)
            .position(|window| {
                window
                    == [
                        0x20,
                        RESOLVED_PAGE_PUBLICATION as u8,
                        (RESOLVED_PAGE_PUBLICATION >> 8) as u8,
                    ]
            })
            .expect("the initializer delegates resolved-page publication");
        let direct_cold = [
            0xA9,
            STATE_COLD_REQUESTED,
            0x8D,
            REQUEST_STATE as u8,
            (REQUEST_STATE >> 8) as u8,
        ];

        assert!(delegate > 0);
        assert!(
            !routine
                .bytes
                .windows(direct_cold.len())
                .any(|window| window == direct_cold)
        );
    }

    /// 빌린 뱅크를 되돌리지 않으면 원본이 남의 코드를 실행한다. 되돌리기는 캐리를
    /// 읽기 전에 끝나야 하므로 그 사이에 `PHP`/`PLP`가 있어야 한다.
    #[test]
    fn the_borrowed_banks_are_handed_back_before_the_carry_decides() {
        let routine = publisher();
        let restore = routine
            .bytes
            .windows(3)
            .position(|window| {
                window
                    == [
                        0x20,
                        PAIRED_BANK_HELPER as u8,
                        (PAIRED_BANK_HELPER >> 8) as u8,
                    ]
            })
            .expect("the initializer restores the bank pair");
        let publication = routine
            .bytes
            .windows(3)
            .position(|window| {
                window
                    == [
                        0x20,
                        RESOLVED_PAGE_PUBLICATION as u8,
                        (RESOLVED_PAGE_PUBLICATION >> 8) as u8,
                    ]
            })
            .expect("the initializer delegates publication");

        assert!(restore < publication);
        assert!(
            routine.bytes.contains(&0x08),
            "the carry is never saved across the restore"
        );
        assert!(routine.bytes.contains(&0x28), "the carry is never restored");
    }

    #[test]
    fn a_dialogue_producer_restores_its_call_site_bank_instead_of_the_outer_shadow() {
        let routine = publisher();
        let restore = [
            0xA9,
            MAIN_DIALOGUE_BANK,
            0x20,
            PAIRED_BANK_HELPER as u8,
            (PAIRED_BANK_HELPER >> 8) as u8,
        ];

        assert!(
            routine
                .bytes
                .windows(restore.len())
                .any(|window| window == restore)
        );
    }

    /// 주 흐름이 `$A000`의 임시 코드를 실행할 때 NMI가 뱅크를 되돌리면 복귀 주소의
    /// 코드가 바뀐다. NMI는 매핑 전에 꺼지고 뱅크 복원 뒤에만 돌아와야 한다.
    #[test]
    fn the_banked_resolver_cannot_be_interrupted_by_bank_restoring_nmi() {
        let routine = publisher();
        let disable = routine
            .bytes
            .windows(2)
            .position(|window| window == [0x29, !NMI_ENABLE_MASK])
            .expect("the initializer disables NMI");
        let map_code_page = routine
            .bytes
            .windows(2)
            .position(|window| window == [0xA9, PRG_A000_REGISTER])
            .expect("the initializer maps the code page");
        let restore_bank = routine
            .bytes
            .windows(3)
            .position(|window| {
                window
                    == [
                        0x20,
                        PAIRED_BANK_HELPER as u8,
                        (PAIRED_BANK_HELPER >> 8) as u8,
                    ]
            })
            .expect("the initializer restores the bank pair");
        let publication = routine
            .bytes
            .windows(3)
            .position(|window| {
                window
                    == [
                        0x20,
                        RESOLVED_PAGE_PUBLICATION as u8,
                        (RESOLVED_PAGE_PUBLICATION >> 8) as u8,
                    ]
            })
            .expect("the initializer delegates publication and PPU restore");

        assert!(disable < map_code_page);
        assert!(restore_bank < publication);
    }

    #[test]
    fn a_rebuild_invalidates_the_previous_page_before_disabling_nmi() {
        let routine = publisher();
        let invalidate = [
            0xA9,
            0x00,
            0x8D,
            REQUEST_STATE as u8,
            (REQUEST_STATE >> 8) as u8,
        ];
        let disable = [0x29, !NMI_ENABLE_MASK];

        let invalidate_at = routine
            .bytes
            .windows(invalidate.len())
            .position(|window| window == invalidate)
            .expect("the rebuild path invalidates the old page");
        let disable_at = routine
            .bytes
            .windows(disable.len())
            .position(|window| window == disable)
            .expect("the rebuild path disables NMI");

        assert!(invalidate_at + invalidate.len() <= disable_at);
    }

    /// 초기화도 밀어낸 원본 호출로 끝나야 대사가 이어진다.
    #[test]
    fn the_initializer_reaches_the_displaced_source_resolver() {
        let routine = publisher();

        assert_eq!(
            &routine.bytes[routine.bytes.len() - 3..],
            [
                0x4C,
                SOURCE_POINTER_RESOLVER as u8,
                (SOURCE_POINTER_RESOLVER >> 8) as u8
            ]
        );
    }

    /// 같은 원문의 반복 생산은 이미 결정된 한글 페이지를 재사용하되, 훅이 밀어낸
    /// 원본 포인터 resolver는 반드시 실행한다.
    #[test]
    fn a_repeated_ready_identity_reuses_the_page_and_executes_the_displaced_resolver() {
        let routine = publisher();
        let source_rebind = [
            0x4C,
            SOURCE_POINTER_RESOLVER as u8,
            (SOURCE_POINTER_RESOLVER >> 8) as u8,
        ];
        routine
            .bytes
            .windows(source_rebind.len())
            .position(|window| window == source_rebind)
            .expect("a changed identity still performs the displaced source rebind");

        assert!(routine.bytes.windows(5).any(|window| {
            window
                == [
                    0xAD,
                    REQUEST_STATE as u8,
                    (REQUEST_STATE >> 8) as u8,
                    0xC9,
                    STATE_READY,
                ]
        }));
        assert!(
            !routine.bytes.windows(3).any(|window| {
                window
                    == [
                        0xAD,
                        super::super::super::runtime_state_storage::VISIBLE_PAGE_INDEX as u8,
                        (super::super::super::runtime_state_storage::VISIBLE_PAGE_INDEX >> 8) as u8,
                    ]
            }),
            "the repeated producer must not reset a completed next page to page zero"
        );
        assert!(routine.bytes.windows(6).any(|window| {
            window
                == [
                    0xAD,
                    SOURCE_DIRECTORY_SELECTOR as u8,
                    (SOURCE_DIRECTORY_SELECTOR >> 8) as u8,
                    0xCD,
                    PUBLISHED_SOURCE_DIRECTORY_SELECTOR as u8,
                    (PUBLISHED_SOURCE_DIRECTORY_SELECTOR >> 8) as u8,
                ]
        }));
        assert!(
            routine.bytes.windows(11).any(|window| {
                window[..6]
                    == [
                        0xAD,
                        SOURCE_ENTRY_INDEX as u8,
                        (SOURCE_ENTRY_INDEX >> 8) as u8,
                        0xCD,
                        PUBLISHED_SOURCE_ENTRY_INDEX as u8,
                        (PUBLISHED_SOURCE_ENTRY_INDEX >> 8) as u8,
                    ]
                    && window[6] == 0xD0
                    && window[8..11] == source_rebind
            }),
            "a repeated identity must skip Korean page rebuilding but still execute the displaced source resolver"
        );
        assert!(routine.bytes.windows(6).any(|window| {
            window
                == [
                    0xAD,
                    SOURCE_ENTRY_INDEX as u8,
                    (SOURCE_ENTRY_INDEX >> 8) as u8,
                    0xCD,
                    PUBLISHED_SOURCE_ENTRY_INDEX as u8,
                    (PUBLISHED_SOURCE_ENTRY_INDEX >> 8) as u8,
                ]
        }));
        assert!(
            !routine
                .bytes
                .windows(3)
                .any(|window| window == [0x20, 0x00, 0xA7]),
            "the repeated producer must not decide a page after automatic line decoding"
        );
    }

    /// 새 수명은 살아 있는 정체성을 바로 해석하지만, 준비된 수명의 다음 선행 조회는
    /// 직전에 게시한 정체성을 현재 레코드로 승격해야 한다. 이 모드를 뒤집으면
    /// `80:03`을 보고 아직 표시 중인 레코드 002 대신 레코드 003의 코드북을 올린다.
    #[test]
    fn new_and_continuing_lifetimes_select_different_identity_sources() {
        let routine = publisher();

        let new_lifetime = [
            0xA9,
            NO_RESIDENT_PAGE_GROUP,
            0xA2,
            LOOKUP_LIVE_SOURCE_IDENTITY,
            0x4C,
        ];
        assert!(
            routine
                .bytes
                .windows(new_lifetime.len())
                .any(|window| window == new_lifetime),
            "new-lifetime mode must be followed by an unconditional JMP because LDX #0 sets Z"
        );
        assert!(
            routine
                .bytes
                .windows(2)
                .any(|window| { window == [0xA2, LOOKUP_PUBLISHED_SOURCE_IDENTITY] })
        );
        assert_eq!(
            routine
                .bytes
                .windows(2)
                .filter(|window| *window == [0xA2, LOOKUP_LIVE_SOURCE_IDENTITY])
                .count(),
            1,
            "only a new lifetime resolves the live identity"
        );
    }

    #[test]
    fn an_initial_entry_never_trusts_a_preexisting_ready_byte() {
        let routine =
            build_initial_request_publisher(0xF650, 0xA400, 0x30, RESOLVED_PAGE_PUBLICATION)
                .unwrap();

        assert_eq!(
            &routine.bytes[..4],
            [
                0xA9,
                NO_RESIDENT_PAGE_GROUP,
                0xA2,
                LOOKUP_LIVE_SOURCE_IDENTITY,
            ]
        );
        assert!(
            !routine.bytes.windows(3).any(|window| {
                window == [0xAD, REQUEST_STATE as u8, (REQUEST_STATE >> 8) as u8]
            }),
            "the initial entry must not branch on unowned RAM"
        );
    }
}
