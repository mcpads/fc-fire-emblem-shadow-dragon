//! 원본 주 대사 상태 머신의 수명 경계에서 요청을 만들거나 폐기한다.
//!
//! `$85C9`의 세 결과는 서로 다른 의미다. `09`만 같은 레코드의 다음 가시 페이지를
//! 찾고, `0F`와 `10`은 대사 수명을 끝낸다. E7 외부 호출도 화면 소유자가 바뀌므로
//! 넘기기 전에 요청을 폐기한다. 이 구분 없이 매번 초기 페이지를 해석하면 여러 쪽
//! 대사가 영원히 0번 페이지로 돌아간다.

use anyhow::{Context, Result, ensure};

use super::super::{
    runtime_bank_contract::{PRG_A000_REGISTER, PRG_BANK_SHADOW},
    runtime_nmi_contract::PPU_CONTROL_SHADOW,
};
use super::{RuntimeRoutine, next_address};
use super::{dispatcher_gate::STATE_COLD_REQUESTED, transport::REQUEST_STATE};
use crate::{
    dialogue_inventory::switchable_cpu_to_file_offset,
    rom::Rom,
    rp2a03::{Instruction, assemble_at},
    sha1_hex,
    typed_source::decode_rp2a03_sequence,
};

pub(super) const LIFECYCLE_ORIGIN: u16 = 0xF990;
pub(super) const LIFECYCLE_CAVE_END: u16 = 0xFA00;
pub(super) const EXPECTED_SAMPLE_LIFECYCLE_SHA1: &str = "67856cd2b7a26ef43649181f5e86ffe2741eb8b3";

pub(super) const COMPLETED_PAGE_SITE: u16 = 0x85C9;
const COMPLETED_PAGE_SPAN: usize = 29;
pub(super) const E7_HANDOFF_SITE: u16 = 0x8556;
pub(super) const E4_TRANSITION_SITE: u16 = 0x85F8;
pub(super) const E6_TRANSITION_SITE: u16 = 0x865F;
pub(super) const E7_RESUME_SITE: u16 = 0x871C;

const MAIN_DIALOGUE_BANK: u8 = 0x0A;
const DIALOGUE_STATE: u16 = 0x77F7;
pub(super) const TERMINAL_STATE: u8 = 0x0F;
const IDLE_STATE: u8 = 0x10;
const CONTINUE_STATE: u8 = 0x09;
const FIRST_COMPLETION_FLAG: u16 = 0x7802;
const SECOND_COMPLETION_FLAG: u16 = 0x780A;
const PAGE_ADVANCE_CLEAR: u16 = 0x7804;
const E7_DECODER_FLAG: u16 = 0x7808;

const BANK_SELECT_REGISTER: u16 = 0x8000;
const BANK_VALUE_REGISTER: u16 = 0x8001;
const PAIRED_BANK_HELPER: u16 = 0xFA20;
const PPU_CONTROL: u16 = 0x2000;
const NMI_ENABLE_MASK: u8 = 0x80;

const SOURCE_POINTER_RESOLVER: u16 = 0xE6B2;
const SOURCE_POINTER_CALL: [u8; 3] = [
    0x20,
    SOURCE_POINTER_RESOLVER as u8,
    (SOURCE_POINTER_RESOLVER >> 8) as u8,
];
const COMPLETED_PAGE_SOURCE: [u8; COMPLETED_PAGE_SPAN] = [
    0xAD, 0x02, 0x78, 0xF0, 0x04, 0xA9, 0x0F, 0xD0, 0x10, 0xA9, 0x00, 0x8D, 0x04, 0x78, 0xAD, 0x0A,
    0x78, 0xF0, 0x04, 0xA9, 0x10, 0xD0, 0x02, 0xA9, 0x09, 0x8D, 0xF7, 0x77, 0x60,
];
const EXPECTED_SAMPLE_COMPLETED_PAGE_SHA1: &str = "965de5bfca83263ac587e5c7c316ed6324d95ca8";
const E7_HANDOFF_SOURCE: [u8; 17] = [
    0xAD, 0x08, 0x78, 0xF0, 0x0C, 0xA9, 0x01, 0x8D, 0x31, 0x78, 0xEE, 0x09, 0x78, 0xA9, 0x11, 0xD0,
    0x18,
];

pub(super) struct LifecycleSuite {
    pub(super) routine: RuntimeRoutine,
    pub(super) completed_page_entry: u16,
    pub(super) handoff_invalidation_entry: u16,
}

/// 설치가 전제로 삼는 원본 상태 전이와 표본 코드를 한 번에 결속한다.
pub(super) fn bind_lifecycle_sites(source: &Rom, candidate: &Rom) -> Result<()> {
    for address in [E4_TRANSITION_SITE, E6_TRANSITION_SITE, E7_RESUME_SITE] {
        for rom in [source, candidate] {
            ensure!(
                switchable_bytes(rom, address, SOURCE_POINTER_CALL.len())? == SOURCE_POINTER_CALL,
                "main-dialogue lifecycle producer changed at 0A:{address:04X}"
            );
        }
        decode_rp2a03_sequence(
            &SOURCE_POINTER_CALL,
            address,
            "main-dialogue lifecycle source pointer call",
        )?;
    }

    ensure!(
        switchable_bytes(source, COMPLETED_PAGE_SITE, COMPLETED_PAGE_SPAN)?
            == COMPLETED_PAGE_SOURCE,
        "main-dialogue completed-page source changed"
    );
    let candidate_completed =
        switchable_bytes(candidate, COMPLETED_PAGE_SITE, COMPLETED_PAGE_SPAN)?;
    ensure!(
        sha1_hex(candidate_completed) == EXPECTED_SAMPLE_COMPLETED_PAGE_SHA1,
        "sample completed-page hook changed"
    );
    decode_rp2a03_sequence(
        candidate_completed,
        COMPLETED_PAGE_SITE,
        "sample completed-page hook",
    )?;

    for rom in [source, candidate] {
        ensure!(
            switchable_bytes(rom, E7_HANDOFF_SITE, E7_HANDOFF_SOURCE.len())? == E7_HANDOFF_SOURCE,
            "main-dialogue E7 handoff changed"
        );
    }
    decode_rp2a03_sequence(
        &E7_HANDOFF_SOURCE,
        E7_HANDOFF_SITE,
        "main-dialogue E7 handoff",
    )?;

    ensure!(
        fixed_bytes(source, LIFECYCLE_ORIGIN, LIFECYCLE_CAVE_END)?
            .iter()
            .all(|byte| *byte == 0xFF),
        "source lifecycle cave is no longer exact FF"
    );
    ensure!(
        sha1_hex(fixed_bytes(
            candidate,
            LIFECYCLE_ORIGIN,
            LIFECYCLE_CAVE_END
        )?) == EXPECTED_SAMPLE_LIFECYCLE_SHA1,
        "sample lifecycle cave changed"
    );
    Ok(())
}

fn append_guarded_banked_resolver_call(
    instructions: &mut Vec<Instruction>,
    resolver: u16,
    code_page: u8,
) {
    instructions.extend([
        // NMI 비활성화 직전의 좁은 경계에서도 이전 페이지를 `ready`로 선택하지 않는다.
        Instruction::LdaImmediate(0),
        Instruction::StaAbsolute(REQUEST_STATE),
        // NMI가 `$A000`을 원래 뱅크로 되돌려 실행 중인 resolver를 바꾸지 못하게 한다.
        Instruction::LdaZeroPage(PPU_CONTROL_SHADOW),
        Instruction::Pha,
        Instruction::AndImmediate(!NMI_ENABLE_MASK),
        Instruction::StaZeroPage(PPU_CONTROL_SHADOW),
        Instruction::StaAbsolute(PPU_CONTROL),
        Instruction::LdaImmediate(PRG_A000_REGISTER),
        Instruction::StaAbsolute(BANK_SELECT_REGISTER),
        Instruction::LdaImmediate(code_page),
        Instruction::StaAbsolute(BANK_VALUE_REGISTER),
        Instruction::JsrAbsolute(resolver),
        Instruction::Php,
        Instruction::LdaZeroPage(PRG_BANK_SHADOW),
        Instruction::JsrAbsolute(PAIRED_BANK_HELPER),
        Instruction::Plp,
        // resolver의 캐리는 `PLA`와 `STA`를 지나도 그대로다.
        Instruction::Pla,
        Instruction::StaZeroPage(PPU_CONTROL_SHADOW),
        Instruction::StaAbsolute(PPU_CONTROL),
    ]);
}

/// 완료 페이지 처리와 E7 외부 호출 무효화를 표본 selector 동굴 하나에 넣는다.
pub(super) fn build_lifecycle_suite(
    next_page_resolver: u16,
    code_page: u8,
) -> Result<LifecycleSuite> {
    let origin = LIFECYCLE_ORIGIN;
    let mut instructions = vec![Instruction::LdaAbsolute(FIRST_COMPLETION_FLAG)];
    let first_flag_clear_placeholder = instructions.len();
    instructions.push(Instruction::BeqAbsolute(origin));
    instructions.push(Instruction::LdaImmediate(TERMINAL_STATE));
    let terminal_placeholder = instructions.len();
    instructions.push(Instruction::BneAbsolute(origin));

    let first_flag_clear = next_address(origin, &instructions)?;
    instructions[first_flag_clear_placeholder] = Instruction::BeqAbsolute(first_flag_clear);
    instructions.extend([
        Instruction::LdaImmediate(0),
        Instruction::StaAbsolute(PAGE_ADVANCE_CLEAR),
        Instruction::LdaAbsolute(SECOND_COMPLETION_FLAG),
    ]);
    let second_flag_clear_placeholder = instructions.len();
    instructions.push(Instruction::BeqAbsolute(origin));
    instructions.push(Instruction::LdaImmediate(IDLE_STATE));
    let idle_placeholder = instructions.len();
    instructions.push(Instruction::BneAbsolute(origin));

    let continue_page = next_address(origin, &instructions)?;
    instructions[second_flag_clear_placeholder] = Instruction::BeqAbsolute(continue_page);
    append_guarded_banked_resolver_call(&mut instructions, next_page_resolver, code_page);
    let no_page_placeholder = instructions.len();
    instructions.push(Instruction::BccAbsolute(origin));
    instructions.extend([
        Instruction::LdaImmediate(STATE_COLD_REQUESTED),
        Instruction::StaAbsolute(REQUEST_STATE),
    ]);
    let store_continue = next_address(origin, &instructions)?;
    instructions[no_page_placeholder] = Instruction::BccAbsolute(store_continue);
    instructions.extend([
        Instruction::LdaImmediate(CONTINUE_STATE),
        Instruction::StaAbsolute(DIALOGUE_STATE),
        Instruction::Rts,
    ]);

    let invalidate_and_store_state = next_address(origin, &instructions)?;
    instructions[terminal_placeholder] = Instruction::BneAbsolute(invalidate_and_store_state);
    instructions[idle_placeholder] = Instruction::BneAbsolute(invalidate_and_store_state);
    instructions.extend([
        Instruction::Pha,
        Instruction::LdaImmediate(0),
        Instruction::StaAbsolute(REQUEST_STATE),
        Instruction::Pla,
        Instruction::StaAbsolute(DIALOGUE_STATE),
        Instruction::Rts,
    ]);

    let handoff_invalidation_entry = next_address(origin, &instructions)?;
    instructions.extend([
        Instruction::LdaImmediate(0),
        Instruction::StaAbsolute(REQUEST_STATE),
        // 훅이 밀어낸 적재와 그 분기 플래그를 그대로 재현한다.
        Instruction::LdaAbsolute(E7_DECODER_FLAG),
        Instruction::Rts,
    ]);

    let mut bytes = assemble_at(origin, &instructions)
        .context("cannot assemble the dialogue lifecycle suite")?;
    let capacity = usize::from(LIFECYCLE_CAVE_END - LIFECYCLE_ORIGIN);
    ensure!(
        bytes.len() <= capacity,
        "dialogue lifecycle suite is {} bytes and exceeds its {capacity}-byte reclaimed cave",
        bytes.len()
    );
    bytes.resize(capacity, 0xFF);
    Ok(LifecycleSuite {
        routine: RuntimeRoutine {
            role: "dialogue lifecycle suite",
            address: origin,
            bytes,
        },
        completed_page_entry: origin,
        handoff_invalidation_entry,
    })
}

/// 표본이 바꾼 29바이트 전체를 가져간다. 첫 `JMP` 뒤는 실행되지 않는 `NOP`으로
/// 채워 표본 전용 분기가 새 전역 훅 뒤에 남지 않게 한다.
pub(super) fn completed_page_hook_bytes(entry: u16) -> Result<Vec<u8>> {
    let mut instructions = vec![Instruction::JmpAbsolute(entry)];
    instructions.resize(COMPLETED_PAGE_SPAN - 2, Instruction::Nop);
    let bytes = assemble_at(COMPLETED_PAGE_SITE, &instructions)?;
    ensure!(
        bytes.len() == COMPLETED_PAGE_SPAN,
        "completed-page hook changed its source span"
    );
    Ok(bytes)
}

pub(super) fn handoff_invalidation_hook_bytes(entry: u16) -> [u8; 3] {
    [0x20, entry as u8, (entry >> 8) as u8]
}

fn switchable_bytes(rom: &Rom, address: u16, length: usize) -> Result<&[u8]> {
    let offset = switchable_cpu_to_file_offset(MAIN_DIALOGUE_BANK, address)?;
    rom.data()
        .get(offset..offset + length)
        .context("main-dialogue lifecycle range is outside ROM")
}

fn fixed_bytes(rom: &Rom, start: u16, end: u16) -> Result<&[u8]> {
    ensure!(
        start >= 0xC000 && start <= end,
        "fixed lifecycle range is invalid"
    );
    let base = rom
        .prg()
        .len()
        .checked_sub(16 * 1024)
        .context("PRG is smaller than one fixed bank")?;
    let relative_start = base + usize::from(start - 0xC000);
    let relative_end = base + usize::from(end - 0xC000);
    rom.prg()
        .get(relative_start..relative_end)
        .context("fixed lifecycle range is outside ROM")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn page_completion_preserves_all_three_source_outcomes() {
        let suite = build_lifecycle_suite(0xB400, 0x2E).unwrap();
        for state in [TERMINAL_STATE, IDLE_STATE, CONTINUE_STATE] {
            assert!(
                suite
                    .routine
                    .bytes
                    .windows(2)
                    .any(|window| window == [0xA9, state]),
                "state {state:02X} disappeared from the lifecycle suite"
            );
        }
        let invalidate = [0x8D, REQUEST_STATE as u8, (REQUEST_STATE >> 8) as u8];
        assert!(
            suite
                .routine
                .bytes
                .windows(invalidate.len())
                .any(|window| window == invalidate)
        );
    }

    #[test]
    fn next_page_resolution_is_nmi_guarded_until_the_bank_is_restored() {
        let suite = build_lifecycle_suite(0xB400, 0x2E).unwrap();
        let bytes = &suite.routine.bytes;
        let disable = bytes
            .windows(2)
            .position(|window| window == [0x29, !NMI_ENABLE_MASK])
            .expect("the lifecycle disables NMI");
        let resolver = bytes
            .windows(3)
            .position(|window| window == [0x20, 0x00, 0xB4])
            .expect("the lifecycle calls the next-page resolver");
        let bank_restore = bytes
            .windows(3)
            .position(|window| window == [0x20, 0x20, 0xFA])
            .expect("the lifecycle restores the source bank");
        let ppu_restore = bytes
            .windows(3)
            .rposition(|window| window == [0x8D, 0x00, 0x20])
            .expect("the lifecycle restores PPU control");

        assert!(disable < resolver && resolver < bank_restore && bank_restore < ppu_restore);
    }

    #[test]
    fn the_completed_page_hook_replaces_the_whole_sample_span() {
        let hook = completed_page_hook_bytes(LIFECYCLE_ORIGIN).unwrap();

        assert_eq!(hook.len(), COMPLETED_PAGE_SPAN);
        assert_eq!(&hook[..3], &[0x4C, 0x90, 0xF9]);
        assert!(hook[3..].iter().all(|byte| *byte == 0xEA));
    }

    #[test]
    fn handoff_invalidation_replays_the_displaced_load_last() {
        let suite = build_lifecycle_suite(0xB400, 0x2E).unwrap();
        let start = usize::from(suite.handoff_invalidation_entry - LIFECYCLE_ORIGIN);
        let handoff = &suite.routine.bytes[start..start + 9];

        assert_eq!(&handoff[5..8], &[0xAD, 0x08, 0x78]);
        assert_eq!(handoff[8], 0x60);
    }

    #[test]
    fn lifecycle_installation_is_bound_to_the_source_and_sample_code() {
        let source_path = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("../../roms/Fire Emblem - Ankoku Ryuu to Hikari no Tsurugi (Japan).nes");
        let source = Rom::parse(std::fs::read(source_path).unwrap()).unwrap();
        let candidate = crate::test_support::release_rom();

        bind_lifecycle_sites(&source, &candidate).unwrap();
    }

    #[test]
    fn the_reclaimed_lifecycle_range_fails_closed_after_mutation() {
        let source_path = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("../../roms/Fire Emblem - Ankoku Ryuu to Hikari no Tsurugi (Japan).nes");
        let source = Rom::parse(std::fs::read(source_path).unwrap()).unwrap();
        let candidate = crate::test_support::release_rom();
        let mut bytes = candidate.data().to_vec();
        let fixed_base = 16 + candidate.prg().len() - 16 * 1024;
        bytes[fixed_base + usize::from(LIFECYCLE_ORIGIN - 0xC000)] ^= 1;
        let mutated = Rom::parse(bytes).unwrap();

        let error = bind_lifecycle_sites(&source, &mutated).unwrap_err();

        assert!(error.to_string().contains("sample lifecycle cave changed"));
    }

    #[test]
    fn source_pointer_call_constant_matches_the_displaced_target() {
        assert_eq!(
            SOURCE_POINTER_CALL,
            [
                0x20,
                SOURCE_POINTER_RESOLVER as u8,
                (SOURCE_POINTER_RESOLVER >> 8) as u8
            ]
        );
    }
}
