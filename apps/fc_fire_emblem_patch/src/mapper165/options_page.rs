use anyhow::Result;

use crate::rp2a03::{Instruction, assemble_at};

mod source_lifetime;

pub(crate) use source_lifetime::{BoundOptionsCompositeLifetime, bind_options_composite_lifetime};

pub(super) const ROW_PRG_BANK: u8 = 0x0B;
pub(super) const ROW_HOOK_ADDRESS: u16 = 0x93B7;
pub(super) const ROW_HOOK_LEN: usize = 11;
pub(super) const PAGE_ROUTINE_ADDRESS: u16 = 0xFB20;
pub(super) const PAGE_ROUTINE_END: u16 = 0xFB68;
pub(super) const ROW_OWNER_GATE_ADDRESS: u16 = PAGE_ROUTINE_END;
pub(super) const ROW_OWNER_GATE_END: u16 = 0xFB80;
pub(super) const PAGE_A_REGISTER: u8 = 0x88;
pub(super) const PAGE_B_REGISTER: u8 = 0x8C;
pub(super) const OPTIONS_COMPOSITE_STATE_ADDRESS: u16 = 0x05E8;
pub(super) const OPTIONS_COMPOSITE_STATE: u8 = 0x1B;
pub(super) const OPTIONS_RESULT_COMPOSITE_STATE: u8 = 0x19;
pub(super) const OPTIONS_COMPOSITE_LIFETIME_STATES: [u8; 2] =
    [OPTIONS_RESULT_COMPOSITE_STATE, OPTIONS_COMPOSITE_STATE];
pub(super) const OPTIONS_MAIN_STATE_ADDRESS: u16 = 0x0084;
pub(super) const OPTIONS_MAIN_STATE: u8 = 0x38;

const OPTIONS_PAGE_OWNER_FIELDS: [(u8, u8); 5] = [
    (0x52, 0x00),
    (0x59, 0x1A),
    (0x5A, 0x1A),
    (0x5B, 0x00),
    (OPTIONS_MAIN_STATE_ADDRESS as u8, OPTIONS_MAIN_STATE),
];

pub(super) fn row_calculation() -> Result<Vec<u8>> {
    assemble_at(
        ROW_HOOK_ADDRESS,
        &[
            Instruction::LdyImmediate(4),
            Instruction::LdaIndirectY(0x6E),
            Instruction::Clc,
            Instruction::AdcAbsoluteX(0x93D8),
            Instruction::StaZeroPage(0x34),
            Instruction::Iny,
        ],
    )
}

pub(super) fn row_hook() -> Result<Vec<u8>> {
    let mut instructions = vec![Instruction::JsrAbsolute(ROW_OWNER_GATE_ADDRESS)];
    instructions.extend(std::iter::repeat_n(Instruction::Nop, ROW_HOOK_LEN - 3));
    assemble_at(ROW_HOOK_ADDRESS, &instructions)
}

/// 공유 문자열 행 계산기가 설정 화면 밖에서도 호출되므로, 그림자 CHR 쌍만 보고
/// 설정 페이지를 고르면 맵 메뉴와 유닛 UI의 더 구체적인 소비자 페이지를 덮는다.
/// 설정 합성 상태일 때만 기존 페이지 선택기로 들어가고, 그 밖에는 훅이 밀어낸 원래
/// 행 계산만 실행한다.
pub(super) fn build_row_owner_gate() -> Result<Vec<u8>> {
    let mut instructions = vec![
        Instruction::LdaAbsolute(OPTIONS_COMPOSITE_STATE_ADDRESS),
        Instruction::CmpImmediate(OPTIONS_COMPOSITE_STATE),
        Instruction::BeqAbsolute(PAGE_ROUTINE_ADDRESS),
    ];
    instructions.extend([
        Instruction::LdyImmediate(4),
        Instruction::LdaIndirectY(0x6E),
        Instruction::Clc,
        Instruction::AdcAbsoluteX(0x93D8),
        Instruction::StaZeroPage(0x34),
        Instruction::Iny,
        Instruction::Rts,
    ]);
    assemble_at(ROW_OWNER_GATE_ADDRESS, &instructions)
}

pub(super) fn build_page_routine_with_fallback(
    page_a_register: u8,
    page_b_register: u8,
    fallback_target: u16,
) -> Result<Vec<u8>> {
    const PAGE_B_ADDRESS: u16 = 0xFB55;
    const WRITE_MAPPER_ADDRESS: u16 = 0xFB57;
    const FALLBACK_ADDRESS: u16 = 0xFB63;

    let mut instructions = vec![
        Instruction::LdyImmediate(4),
        Instruction::LdaIndirectY(0x6E),
        Instruction::Clc,
        Instruction::AdcAbsoluteX(0x93D8),
        Instruction::StaZeroPage(0x34),
        Instruction::Iny,
        Instruction::Php,
    ];
    for (address, expected) in OPTIONS_PAGE_OWNER_FIELDS {
        instructions.extend([
            Instruction::LdaZeroPage(address),
            Instruction::CmpImmediate(expected),
            Instruction::BneAbsolute(FALLBACK_ADDRESS),
        ]);
    }
    instructions.extend([
        Instruction::LdaZeroPage(0x34),
        Instruction::CmpImmediate(0x30),
        Instruction::BeqAbsolute(PAGE_B_ADDRESS),
        Instruction::LdaImmediate(page_a_register),
        Instruction::JmpAbsolute(WRITE_MAPPER_ADDRESS),
        Instruction::LdaImmediate(page_b_register),
        Instruction::Pha,
        Instruction::LdaImmediate(2),
        crate::mapper165::selector_safety::select_register_instruction(),
        Instruction::Pla,
        Instruction::StaAbsolute(0x8001),
        Instruction::Plp,
        Instruction::Rts,
        Instruction::JsrAbsolute(fallback_target),
        Instruction::Plp,
        Instruction::Rts,
    ]);
    assemble_at(PAGE_ROUTINE_ADDRESS, &instructions)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn row_hook_preserves_the_original_span_and_calls_the_fixed_routine() {
        let original = row_calculation().unwrap();
        let hook = row_hook().unwrap();

        assert_eq!(
            original,
            [
                0xA0, 0x04, 0xB1, 0x6E, 0x18, 0x7D, 0xD8, 0x93, 0x85, 0x34, 0xC8
            ]
        );
        assert_eq!(hook.len(), original.len());
        assert_eq!(&hook[..3], &[0x20, 0x68, 0xFB]);
        assert!(hook[3..].iter().all(|byte| *byte == 0xEA));
    }

    #[test]
    fn row_owner_gate_admits_only_the_evidenced_options_composite_state() {
        let gate = build_row_owner_gate().unwrap();

        assert!(ROW_OWNER_GATE_ADDRESS as usize + gate.len() <= ROW_OWNER_GATE_END as usize);
        assert_eq!(
            &gate[..7],
            &[
                0xAD,
                OPTIONS_COMPOSITE_STATE_ADDRESS as u8,
                (OPTIONS_COMPOSITE_STATE_ADDRESS >> 8) as u8,
                0xC9,
                OPTIONS_COMPOSITE_STATE,
                0xF0,
                0xB1,
            ]
        );
        let mut displaced_row = row_calculation().unwrap();
        displaced_row.push(0x60);
        assert_eq!(&gate[7..], displaced_row);
    }

    #[test]
    fn page_routine_fits_its_proven_cave_and_has_a_pair_aware_fallback() {
        let routine = build_page_routine_with_fallback(
            PAGE_A_REGISTER,
            PAGE_B_REGISTER,
            super::super::SELECT_RIGHT_FD_CHR_BANK_FOR_PAIR_ADDRESS,
        )
        .unwrap();

        assert_eq!(routine.len(), 0x48);
        assert_eq!(
            PAGE_ROUTINE_ADDRESS as usize + routine.len(),
            PAGE_ROUTINE_END as usize
        );
        assert!(routine.windows(2).any(|bytes| bytes == [0xA9, 0x88]));
        assert!(routine.windows(2).any(|bytes| bytes == [0xA9, 0x8C]));
        assert!(routine.windows(3).any(|bytes| bytes == [0x20, 0xC0, 0xFA]));
        assert!(routine.windows(4).any(|bytes| bytes
            == [
                0xA5,
                OPTIONS_MAIN_STATE_ADDRESS as u8,
                0xC9,
                OPTIONS_MAIN_STATE,
            ]));
        assert!(!routine.windows(2).any(|bytes| bytes == [0xA5, 0x5C]));
        assert_eq!(&routine[..11], row_calculation().unwrap());
    }

    #[test]
    fn owner_predicate_ignores_inherited_right_fe_but_requires_the_live_options_state() {
        let mut iram = [0_u8; 256];
        for (address, expected) in OPTIONS_PAGE_OWNER_FIELDS {
            iram[usize::from(address)] = expected;
        }
        let matches = |memory: &[u8; 256]| {
            OPTIONS_PAGE_OWNER_FIELDS
                .into_iter()
                .all(|(address, expected)| memory[usize::from(address)] == expected)
        };

        for inherited_right_fe in [0x15, 0x18, 0x19, 0xFF] {
            iram[0x5C] = inherited_right_fe;
            assert!(matches(&iram));
        }

        iram[usize::from(OPTIONS_MAIN_STATE_ADDRESS)] = OPTIONS_MAIN_STATE - 1;
        assert!(!matches(&iram));
        iram[usize::from(OPTIONS_MAIN_STATE_ADDRESS)] = OPTIONS_MAIN_STATE;
        iram[0x5B] = 1;
        assert!(!matches(&iram));
    }

    #[test]
    fn non_options_rows_can_continue_through_another_screen_lifetime_selector() {
        let routine = build_page_routine_with_fallback(
            PAGE_A_REGISTER,
            PAGE_B_REGISTER,
            super::super::roster_page::PAGE_ROUTINE_ADDRESS,
        )
        .unwrap();

        assert_eq!(routine.len(), 0x48);
        assert!(routine.windows(3).any(|bytes| bytes == [0x20, 0x80, 0xFB]));
        assert!(!routine.windows(3).any(|bytes| bytes == [0x20, 0xC0, 0xFA]));
    }
}
