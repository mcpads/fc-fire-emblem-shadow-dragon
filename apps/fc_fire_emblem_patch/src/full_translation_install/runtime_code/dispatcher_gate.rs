//! 대사 상태 머신을 붙잡는 게이트와, 진입에서 요청을 발행하는 콜드 초기화다.
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
/// 대사 초기 진입이다. 콜드 초기화가 이 세 바이트를 가져간다.
pub(in crate::full_translation_install) const COLD_ENTRY: u16 = 0x809B;
/// 초기 진입이 부르는 원본 포인터 resolver다.
pub(in crate::full_translation_install) const SOURCE_POINTER_RESOLVER: u16 = 0xE6B2;

use super::super::runtime_bank_contract::{PRG_A000_REGISTER, PRG_BANK_SHADOW};
use super::super::runtime_nmi_contract::PPU_CONTROL_SHADOW;
use super::transport::{REQUEST_STATE, STATE_READY};

const BANK_SELECT_REGISTER: u16 = 0x8000;
const BANK_VALUE_REGISTER: u16 = 0x8001;
const PPU_CONTROL: u16 = 0x2000;
const NMI_ENABLE_MASK: u8 = 0x80;
/// 16 KiB 뱅크 짝을 되돌리는 원본 도우미다.
const PAIRED_BANK_HELPER: u16 = 0xFA20;

/// 합성을 기다리는 중이라는 요청이다.
pub(in crate::full_translation_install) const STATE_COLD_REQUESTED: u8 = 1;

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
pub(super) fn cold_hook_bytes(initializer: u16) -> [u8; 3] {
    [0x20, initializer as u8, (initializer >> 8) as u8]
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

/// 해석기를 불러 커서를 세우고, 성공했을 때만 요청을 발행한다.
///
/// 해석기는 실행 코드 페이지에 있으므로 그 페이지를 `$A000`에 걸어야 부를 수 있다.
/// 걸면 원본이 기대하던 뱅크가 사라지므로 돌아오기 전에 되돌린다. 되돌리는 값은
/// `$29` 그림자와 원본 도우미 `$FA20`이 준다 — `$C1FB`가 매 프레임 쓰는 방식이다.
///
/// 실패하면 커서도 요청도 남기지 않는다. 그 경우 원본 일본어 경로가 그대로 돈다.
pub(super) fn build_cold_initializer(
    origin: u16,
    resolver: u16,
    code_page: u8,
) -> Result<RuntimeRoutine> {
    let mut instructions = vec![
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
        Instruction::LdaZeroPage(PRG_BANK_SHADOW),
        Instruction::JsrAbsolute(PAIRED_BANK_HELPER),
        Instruction::Plp,
        // 뱅크가 원래대로 돌아온 뒤에만 NMI를 원래 상태로 되돌린다. `PLA`와
        // `STA`는 resolver의 캐리를 바꾸지 않는다.
        Instruction::Pla,
        Instruction::StaZeroPage(PPU_CONTROL_SHADOW),
        Instruction::StaAbsolute(PPU_CONTROL),
    ];
    let failed_placeholder = instructions.len();
    instructions.push(Instruction::BccAbsolute(origin));
    instructions.extend([
        // 요청은 커서가 다 선 뒤에만 발행한다.
        Instruction::LdaImmediate(STATE_COLD_REQUESTED),
        Instruction::StaAbsolute(REQUEST_STATE),
    ]);
    let failed = next_address(origin, &instructions)?;
    instructions[failed_placeholder] = Instruction::BccAbsolute(failed);
    // 밀어낸 원본 호출로 넘긴다.
    instructions.push(Instruction::JmpAbsolute(SOURCE_POINTER_RESOLVER));

    Ok(RuntimeRoutine {
        role: "dialogue cold initializer",
        address: origin,
        bytes: assemble_at(origin, &instructions)?,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

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

    /// 입구가 아직 원본이어야 게이트가 그 자리를 가져갈 수 있다.
    #[test]
    fn the_source_dispatcher_entry_is_still_where_the_gate_expects_it() {
        let rom = crate::test_support::release_rom();

        bind_dispatcher_entry(&rom, &rom).unwrap();
    }

    /// 입구가 바뀌면 표 분기의 복귀 주소가 어긋나므로 설치를 막는다.
    #[test]
    fn a_changed_dispatcher_entry_refuses_installation() {
        let rom = crate::test_support::release_rom();
        let offset = switchable_cpu_to_file_offset(MAIN_DIALOGUE_BANK, DISPATCHER_ENTRY).unwrap();
        let mut bytes = rom.data().to_vec();
        bytes[offset + 3] = 0xEA;
        let mutated = Rom::parse(bytes).unwrap();

        let error = bind_dispatcher_entry(&rom, &mutated).unwrap_err();

        assert!(
            error
                .to_string()
                .contains("dispatcher entry at 0A:8000 changed")
        );
    }

    /// 해석기가 실패하면 요청을 발행하지 않아야 한다. 발행하면 소비자가 세워지지
    /// 않은 커서를 읽고 남의 자료를 CHR RAM에 올린다.
    #[test]
    fn a_failed_resolve_publishes_no_request() {
        let routine = build_cold_initializer(0xF558, 0xA400, 0x30).unwrap();
        let publish = publish_position(&routine.bytes, REQUEST_STATE)
            .expect("the initializer publishes a request");
        let branch = routine
            .bytes
            .iter()
            .position(|byte| *byte == 0x90)
            .expect("the initializer branches on the resolver's carry");

        assert!(
            branch < publish,
            "the request is published before the carry is read"
        );
    }

    /// 빌린 뱅크를 되돌리지 않으면 원본이 남의 코드를 실행한다. 되돌리기는 캐리를
    /// 읽기 전에 끝나야 하므로 그 사이에 `PHP`/`PLP`가 있어야 한다.
    #[test]
    fn the_borrowed_banks_are_handed_back_before_the_carry_decides() {
        let routine = build_cold_initializer(0xF558, 0xA400, 0x30).unwrap();
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
        let branch = routine
            .bytes
            .iter()
            .position(|byte| *byte == 0x90)
            .expect("the initializer branches on the resolver's carry");

        assert!(restore < branch);
        assert!(
            routine.bytes.contains(&0x08),
            "the carry is never saved across the restore"
        );
        assert!(routine.bytes.contains(&0x28), "the carry is never restored");
    }

    /// 주 흐름이 `$A000`의 임시 코드를 실행할 때 NMI가 뱅크를 되돌리면 복귀 주소의
    /// 코드가 바뀐다. NMI는 매핑 전에 꺼지고 뱅크 복원 뒤에만 돌아와야 한다.
    #[test]
    fn the_banked_resolver_cannot_be_interrupted_by_bank_restoring_nmi() {
        let routine = build_cold_initializer(0xF558, 0xA400, 0x30).unwrap();
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
        let restore_ppu = routine
            .bytes
            .windows(3)
            .rposition(|window| window == [0x8D, 0x00, 0x20])
            .expect("the initializer restores PPU control");

        assert!(disable < map_code_page);
        assert!(restore_bank < restore_ppu);
    }

    #[test]
    fn cold_entry_invalidates_the_previous_page_before_disabling_nmi() {
        let routine = build_cold_initializer(0xF558, 0xA400, 0x30).unwrap();
        let invalidate = [
            0xA9,
            0x00,
            0x8D,
            REQUEST_STATE as u8,
            (REQUEST_STATE >> 8) as u8,
        ];
        let disable = [0x29, !NMI_ENABLE_MASK];

        assert_eq!(&routine.bytes[..invalidate.len()], invalidate);
        assert!(
            routine
                .bytes
                .windows(disable.len())
                .position(|window| window == disable)
                .is_some_and(|position| position >= invalidate.len())
        );
    }

    /// 초기화도 밀어낸 원본 호출로 끝나야 대사가 이어진다.
    #[test]
    fn the_initializer_reaches_the_displaced_source_resolver() {
        let routine = build_cold_initializer(0xF558, 0xA400, 0x30).unwrap();

        assert_eq!(
            &routine.bytes[routine.bytes.len() - 3..],
            [
                0x4C,
                SOURCE_POINTER_RESOLVER as u8,
                (SOURCE_POINTER_RESOLVER >> 8) as u8
            ]
        );
    }

    fn publish_position(bytes: &[u8], address: u16) -> Option<usize> {
        let publish = [
            0xA9,
            STATE_COLD_REQUESTED,
            0x8D,
            address as u8,
            (address >> 8) as u8,
        ];
        bytes.windows(publish.len()).position(|window| window == publish)
    }
}
