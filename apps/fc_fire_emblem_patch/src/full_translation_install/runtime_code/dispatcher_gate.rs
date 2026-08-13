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
pub(super) const DISPATCHER_ENTRY: u16 = 0x8000;
/// 원본이 입구에서 읽는 상태 바이트다.
pub(super) const DISPATCHER_STATE: u16 = 0x77F7;
/// 표 분기 호출이다. 게이트는 통과할 때 이 자리로 되돌린다.
pub(super) const DISPATCHER_TABLE_CALL: u16 = 0x8003;
/// 대사 초기 진입이다. 콜드 초기화가 이 세 바이트를 가져간다.
pub(super) const COLD_ENTRY: u16 = 0x809B;
/// 초기 진입이 부르는 원본 포인터 resolver다.
pub(super) const SOURCE_POINTER_RESOLVER: u16 = 0xE6B2;

use super::super::runtime_cursor_storage::{
    CURSOR_NEXT_TILE_INDEX, CURSOR_REMAINING_TILES, CURSOR_SOURCE_HIGH, CURSOR_SOURCE_LOW,
};
use super::transport::{REQUEST_STATE, STATE_READY};

/// 합성을 기다리는 중이라는 요청이다.
pub(super) const STATE_COLD_REQUESTED: u8 = 1;

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

/// 커서를 전부 세운 뒤에 요청을 발행한다. 순서가 요구사항이다.
pub(super) fn build_cold_initializer(
    origin: u16,
    atlas_base: u16,
    tile_count: u8,
) -> Result<RuntimeRoutine> {
    ensure!(
        tile_count > 0,
        "a cold request with no tiles never completes and the dialogue never resumes"
    );
    let instructions = vec![
        Instruction::LdaImmediate(atlas_base as u8),
        Instruction::StaAbsolute(CURSOR_SOURCE_LOW),
        Instruction::LdaImmediate((atlas_base >> 8) as u8),
        Instruction::StaAbsolute(CURSOR_SOURCE_HIGH),
        Instruction::LdaImmediate(0),
        Instruction::StaAbsolute(CURSOR_NEXT_TILE_INDEX),
        Instruction::LdaImmediate(tile_count),
        Instruction::StaAbsolute(CURSOR_REMAINING_TILES),
        // 요청은 마지막에 발행한다. 그 전에 소비자가 깨어나면 반쯤 세워진 커서를 읽는다.
        Instruction::LdaImmediate(STATE_COLD_REQUESTED),
        Instruction::StaAbsolute(REQUEST_STATE),
        // 밀어낸 원본 호출로 넘긴다.
        Instruction::JmpAbsolute(SOURCE_POINTER_RESOLVER),
    ];
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
        let load_state = [
            0xAD,
            DISPATCHER_STATE as u8,
            (DISPATCHER_STATE >> 8) as u8,
        ];

        assert!(
            routine
                .bytes
                .windows(3)
                .any(|window| window == load_state),
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

        assert!(error.to_string().contains("dispatcher entry at 0A:8000 changed"));
    }

    /// 요청 발행이 커서 설정보다 앞서면 소비자가 반쯤 세워진 커서를 읽는다.
    #[test]
    fn the_request_is_published_after_every_cursor_byte() {
        let routine = build_cold_initializer(0xF480, 0xA100, 40).unwrap();
        let request_at = store_position(&routine.bytes, REQUEST_STATE)
            .expect("the initializer publishes a request");

        for cursor in [
            CURSOR_SOURCE_LOW,
            CURSOR_SOURCE_HIGH,
            CURSOR_NEXT_TILE_INDEX,
            CURSOR_REMAINING_TILES,
        ] {
            let at = store_position(&routine.bytes, cursor)
                .unwrap_or_else(|| panic!("cursor {cursor:04X} is never written"));
            assert!(at < request_at, "cursor {cursor:04X} is written too late");
        }
    }

    /// 타일이 0인 요청은 영원히 끝나지 않아 대사가 멈춘 채로 남는다.
    #[test]
    fn a_zero_tile_request_is_refused() {
        let error = build_cold_initializer(0xF480, 0xA100, 0).unwrap_err();

        assert!(error.to_string().contains("never completes"));
    }

    /// 초기화도 밀어낸 원본 호출로 끝나야 대사가 이어진다.
    #[test]
    fn the_initializer_reaches_the_displaced_source_resolver() {
        let routine = build_cold_initializer(0xF480, 0xA100, 40).unwrap();

        assert_eq!(
            &routine.bytes[routine.bytes.len() - 3..],
            [
                0x4C,
                SOURCE_POINTER_RESOLVER as u8,
                (SOURCE_POINTER_RESOLVER >> 8) as u8
            ]
        );
    }

    fn store_position(bytes: &[u8], address: u16) -> Option<usize> {
        let store = [0x8D, address as u8, (address >> 8) as u8];
        bytes.windows(3).position(|window| window == store)
    }
}
