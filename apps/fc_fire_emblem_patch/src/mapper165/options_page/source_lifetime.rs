//! 설정 목록과 값 선택 결과가 공유하는 원천 화면 수명을 결속한다.
//!
//! 합성 상태 `19`는 독립 화면이 아니다. 주 상태 `38`의 설정 상태기에서 상태 `1B`
//! 목록을 그린 뒤 첫째 또는 둘째 값을 고르면 같은 상태기가 `19` 결과창을 덧그린다.
//! 셋째 값은 별도 `1A` 게임 속도 appender로 간다. 이 경계를 잃으면 `19`를 단지
//! 보존 문자열 두 개를 쓴다는 이유로 이전 CHR 페이지에 묵시적으로 맡기게 된다.

use anyhow::{Context, Result, ensure};
use retro_rp2a03::{AddressingMode, Location, MemoryAddress, Operand, Rp2A03, decode_bytes};
use typed_isa_core::{AccessKind, StaticSemantics};

use crate::{
    fixed_string_consumers::FixedStringConsumerInspection,
    mapper165::inline_pointer_dispatch::bind_inline_pointer_dispatch, rom::Rom,
    typed_source::decode_rp2a03_sequence,
};

use super::{
    OPTIONS_COMPOSITE_LIFETIME_STATES, OPTIONS_COMPOSITE_STATE, OPTIONS_MAIN_STATE,
    OPTIONS_RESULT_COMPOSITE_STATE,
};

const SOURCE_PRG_BANK_BYTE_COUNT: usize = 16 * 1024;
const OPTIONS_PRG_BANK: u8 = 0x06;
const GAMEPLAY_MAIN_STATE_DISPATCH_CALL: u16 = 0x8964;
const OPTIONS_MAIN_HANDLER: u16 = 0xB349;
const OPTIONS_SUBSTATE_DISPATCH_CALL: u16 = 0xB34F;
const OPTIONS_SUBSTATE_TABLE_END: u16 = 0xB358;
const OPTIONS_SUBSTATE_HANDLERS: [u16; 3] = [0xB358, 0xB360, 0xB3BF];
const OPTIONS_RESULT_HANDLER: u16 = 0x87C4;

const OPTIONS_ENTRY: [u8; 9] = [
    0x20, 0x5C, 0xE6, // JSR $E65C
    0xAD, 0xDB, 0x05, // LDA $05DB
    0x20, 0x4C, 0xC3, // JSR $C34C
];
const OPTIONS_INITIAL_SCREEN: [u8; 8] = [
    0xEE,
    0xDB,
    0x05, // INC $05DB
    0xA9,
    OPTIONS_COMPOSITE_STATE, // LDA #$1B
    0x4C,
    0x90,
    0xE6, // JMP $E690
];
const OPTIONS_VALUE_SELECTION: [u8; 0x5F] = [
    0xAE,
    0xCE,
    0x05,
    0xCA,
    0xA9,
    0x00,
    0x9D,
    0xEE,
    0x7F,
    0xAE,
    0xEB,
    0x05,
    0x8E,
    0x30,
    0x77,
    0xE0,
    0x01,
    0xF0,
    0x10,
    0xE0,
    0x02,
    0xF0,
    0x20,
    0xE0,
    0x03,
    0xF0,
    0x30,
    0xA9,
    0x00,
    0x85,
    0x84,
    0x8D,
    0xDB,
    0x05,
    0x60,
    0xEE,
    0xDB,
    0x05,
    0xAE,
    0xCE,
    0x05,
    0xAD,
    0x7A,
    0x76,
    0x9D,
    0xF3,
    0x7F,
    0xFE,
    0xF3,
    0x7F,
    0xA9,
    OPTIONS_RESULT_COMPOSITE_STATE,
    0x4C,
    0x90,
    0xE6,
    0xEE,
    0xDB,
    0x05,
    0xAE,
    0xCE,
    0x05,
    0xAD,
    0x7B,
    0x76,
    0x9D,
    0xF3,
    0x7F,
    0xFE,
    0xF3,
    0x7F,
    0xA9,
    OPTIONS_RESULT_COMPOSITE_STATE,
    0x4C,
    0x90,
    0xE6,
    0xEE,
    0xDB,
    0x05,
    0xAE,
    0xCE,
    0x05,
    0xAD,
    0x7C,
    0x76,
    0x9D,
    0xF3,
    0x7F,
    0xFE,
    0xF3,
    0x7F,
    0xA9,
    0x1A,
    0x4C,
    0x90,
    0xE6,
];
const OPTIONS_FINISH: [u8; 0x38] = [
    0xAE, 0xEB, 0x05, 0xD0, 0x02, 0xF0, 0x27, 0xCA, 0xAD, 0x30, 0x77, 0xC9, 0x01, 0xF0, 0x08, 0xC9,
    0x02, 0xF0, 0x12, 0xC9, 0x03, 0xF0, 0x14, 0x8E, 0x7A, 0x76, 0x8E, 0xF0, 0x06, 0xD0, 0x0F, 0x20,
    0x7F, 0xB9, 0x4C, 0xED, 0xB3, 0x8E, 0x7B, 0x76, 0x4C, 0xED, 0xB3, 0x8E, 0x7C, 0x76, 0xA9, 0x00,
    0x85, 0x26, 0x8D, 0xDB, 0x05, 0x4C, 0xDE, 0xB8,
];

const EXPECTED_COMPOSITE_PRODUCERS: [(u16, u8); 4] = [
    (0xB35D, OPTIONS_COMPOSITE_STATE),
    (0xB394, OPTIONS_RESULT_COMPOSITE_STATE),
    (0xB3A8, OPTIONS_RESULT_COMPOSITE_STATE),
    (0xB3BC, 0x1A),
];

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct BoundOptionsCompositeLifetime {
    delegated_states: [u8; 2],
    result_fixed_string_indices: [u8; 2],
    result_producer_count: usize,
}

impl BoundOptionsCompositeLifetime {
    pub(crate) const fn delegated_states(&self) -> [u8; 2] {
        self.delegated_states
    }

    pub(crate) const fn result_fixed_string_indices(&self) -> [u8; 2] {
        self.result_fixed_string_indices
    }

    pub(crate) const fn result_producer_count(&self) -> usize {
        self.result_producer_count
    }
}

pub(crate) fn bind_options_composite_lifetime(
    source: &Rom,
    fixed_strings: &FixedStringConsumerInspection,
) -> Result<BoundOptionsCompositeLifetime> {
    source.verify_supported_japanese()?;

    let main_dispatch = bind_inline_pointer_dispatch(
        source,
        OPTIONS_PRG_BANK,
        GAMEPLAY_MAIN_STATE_DISPATCH_CALL,
        [OPTIONS_MAIN_STATE],
        "options main-state route",
    )?;
    ensure!(
        main_dispatch.targets_in_selector_order() == [OPTIONS_MAIN_HANDLER],
        "gameplay main state 38 no longer enters the options lifetime"
    );

    bind_code_without_source_mapper_writes(
        source,
        OPTIONS_MAIN_HANDLER,
        &OPTIONS_ENTRY,
        "options lifetime entry",
    )?;
    let substate_dispatch = bind_inline_pointer_dispatch(
        source,
        OPTIONS_PRG_BANK,
        OPTIONS_SUBSTATE_DISPATCH_CALL,
        0..=2,
        "options lifetime substate dispatch",
    )?;
    ensure!(
        substate_dispatch.table_start() == OPTIONS_SUBSTATE_DISPATCH_CALL + 3
            && substate_dispatch.targets_in_selector_order() == OPTIONS_SUBSTATE_HANDLERS
            && OPTIONS_SUBSTATE_TABLE_END == OPTIONS_SUBSTATE_HANDLERS[0],
        "options lifetime substate table changed"
    );

    bind_code_without_source_mapper_writes(
        source,
        OPTIONS_SUBSTATE_HANDLERS[0],
        &OPTIONS_INITIAL_SCREEN,
        "options initial screen producer",
    )?;
    bind_code_without_source_mapper_writes(
        source,
        OPTIONS_SUBSTATE_HANDLERS[1],
        &OPTIONS_VALUE_SELECTION,
        "options value-selection branches",
    )?;
    bind_code_without_source_mapper_writes(
        source,
        OPTIONS_SUBSTATE_HANDLERS[2],
        &OPTIONS_FINISH,
        "options lifetime completion",
    )?;

    let actual_producers = fixed_strings
        .composite_state_producers
        .iter()
        .copied()
        .filter(|producer| {
            producer.prg_bank == OPTIONS_PRG_BANK
                && EXPECTED_COMPOSITE_PRODUCERS
                    .iter()
                    .any(|(address, _)| *address == producer.cpu_address)
        })
        .collect::<Vec<_>>();
    let actual_producer_identity = actual_producers
        .iter()
        .map(|producer| (producer.cpu_address, producer.state))
        .collect::<Vec<_>>();
    ensure!(
        actual_producer_identity == EXPECTED_COMPOSITE_PRODUCERS,
        "options composite producer family changed: {actual_producer_identity:04X?}"
    );

    ensure!(
        fixed_strings.composite_handler_target(OPTIONS_RESULT_COMPOSITE_STATE)
            == Some(OPTIONS_RESULT_HANDLER),
        "options result composite handler changed"
    );
    let result_calls = fixed_strings
        .call_sites
        .iter()
        .filter(|call| call.composite_state == OPTIONS_RESULT_COMPOSITE_STATE)
        .map(|call| (call.cpu_address, call.possible_indices.as_slice()))
        .collect::<Vec<_>>();
    ensure!(
        result_calls == [(0x87DA, &[0x2E][..]), (0x87DF, &[0x2F][..])],
        "options result fixed-string consumers changed: {result_calls:?}"
    );

    Ok(BoundOptionsCompositeLifetime {
        delegated_states: OPTIONS_COMPOSITE_LIFETIME_STATES,
        result_fixed_string_indices: [0x2E, 0x2F],
        result_producer_count: actual_producers
            .iter()
            .filter(|producer| producer.state == OPTIONS_RESULT_COMPOSITE_STATE)
            .count(),
    })
}

fn bind_code_without_source_mapper_writes(
    source: &Rom,
    address: u16,
    expected: &[u8],
    role: &str,
) -> Result<()> {
    let actual = source_bytes(source, address, expected.len())?;
    ensure!(actual == expected, "{role} source bytes changed");
    decode_rp2a03_sequence(actual, address, role)?;

    let mut offset = 0_usize;
    while offset < actual.len() {
        let instruction = decode_bytes(&actual[offset..])
            .with_context(|| format!("decode {role} at +0x{offset:X}"))?;
        let instruction_address = address
            .checked_add(u16::try_from(offset)?)
            .context("options source instruction address overflow")?;
        for access in Rp2A03::semantics(&instruction, &instruction_address)
            .expect("RP2A03 static semantics are infallible")
            .location_accesses
        {
            if access.kind != AccessKind::Write {
                continue;
            }
            match access.location {
                Location::Memory(MemoryAddress::Direct(target)) => ensure!(
                    target < 0xA000,
                    "{role} gained a direct source mapper write at ${instruction_address:04X}"
                ),
                Location::Memory(MemoryAddress::Effective {
                    mode: AddressingMode::AbsoluteX | AddressingMode::AbsoluteY,
                    operand: Operand::Word(base),
                }) => ensure!(
                    base < 0xA000 && base.saturating_add(0xFF) < 0xA000,
                    "{role} gained an indexed source mapper-write envelope at ${instruction_address:04X}"
                ),
                Location::Memory(MemoryAddress::Effective { .. }) => anyhow::bail!(
                    "{role} gained an unbounded effective write at ${instruction_address:04X}"
                ),
                _ => {}
            }
        }
        offset += instruction.encoded_len();
    }
    ensure!(
        offset == actual.len(),
        "{role} typed decode ended mid-region"
    );
    Ok(())
}

fn source_bytes(source: &Rom, address: u16, byte_count: usize) -> Result<&[u8]> {
    ensure!(
        (0x8000..0xC000).contains(&address),
        "options source address is outside the switchable window"
    );
    let start = usize::from(OPTIONS_PRG_BANK)
        .checked_mul(SOURCE_PRG_BANK_BYTE_COUNT)
        .and_then(|base| base.checked_add(usize::from(address - 0x8000)))
        .context("options source range offset overflow")?;
    source
        .prg()
        .get(start..start + byte_count)
        .context("options source range exceeds PRG")
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeSet;

    use super::*;

    #[test]
    fn delegated_states_exclude_the_separately_owned_game_speed_branch() {
        assert_eq!(
            OPTIONS_COMPOSITE_LIFETIME_STATES,
            [OPTIONS_RESULT_COMPOSITE_STATE, OPTIONS_COMPOSITE_STATE]
        );
        assert!(!OPTIONS_COMPOSITE_LIFETIME_STATES.contains(&0x1A));
    }

    #[test]
    fn result_producers_are_two_branches_of_one_state_handler() {
        let result_producers = EXPECTED_COMPOSITE_PRODUCERS
            .iter()
            .filter(|(_, state)| *state == OPTIONS_RESULT_COMPOSITE_STATE)
            .map(|(address, _)| *address)
            .collect::<BTreeSet<_>>();
        assert_eq!(result_producers, BTreeSet::from([0xB394, 0xB3A8]));
    }
}
