use crate::{
    rom::Rom,
    rp2a03::{Instruction, assemble_at},
};

use super::{shadow_operand_backstop::documented_memory_operand, *};

fn write_fixed(bytes: &mut [u8], address: u16, instructions: &[Instruction]) {
    let encoded = assemble_at(address, instructions).unwrap();
    let offset = crate::test_support::synthetic_fixed_bank_file_offset(address);
    bytes[offset..offset + encoded.len()].copy_from_slice(&encoded);
}

fn synthetic_installed_candidate(first_nmi_call_target: u16) -> Rom {
    let mut bytes = crate::test_support::synthetic_mapper165_rom_bytes(0xFF);
    let nmi_start = crate::test_support::synthetic_fixed_bank_file_offset(SOURCE_NMI_ENTRY);
    let nmi_end = crate::test_support::synthetic_fixed_bank_file_offset(SOURCE_NMI_END_EXCLUSIVE);
    bytes[nmi_start..nmi_end].fill(0xEA);
    write_fixed(
        &mut bytes,
        super::super::RESET_INITIALIZER_ADDRESS,
        &[
            Instruction::LdaImmediate(0),
            Instruction::StaAbsolute(0xE000),
            Instruction::LdaImmediate(0x80),
            Instruction::StaAbsolute(0xA001),
            Instruction::LdaImmediate(0),
            Instruction::JsrAbsolute(super::super::SELECT_PRG_BANK_ADDRESS),
            Instruction::JsrAbsolute(super::super::SELECT_LEFT_FD_CHR_BANK_ADDRESS),
            Instruction::JsrAbsolute(super::super::SELECT_LEFT_FE_CHR_BANK_ADDRESS),
            Instruction::JsrAbsolute(super::super::SELECT_RIGHT_FD_CHR_BANK_ADDRESS),
            Instruction::JsrAbsolute(super::super::SELECT_RIGHT_FE_CHR_BANK_ADDRESS),
            Instruction::JmpAbsolute(super::super::SOURCE_RESET_ADDRESS),
        ],
    );
    let reset_vector = crate::test_support::synthetic_fixed_bank_file_offset(0xFFFC);
    bytes[reset_vector..reset_vector + 2]
        .copy_from_slice(&super::super::RESET_INITIALIZER_ADDRESS.to_le_bytes());
    write_fixed(
        &mut bytes,
        SELECT_REGISTER_ROUTINE_ADDRESS,
        &[
            Instruction::StaZeroPage(SELECTED_REGISTER_SHADOW),
            Instruction::StaAbsolute(0x8000),
            Instruction::Rts,
        ],
    );
    write_fixed(
        &mut bytes,
        NMI_ENTRY_CONTINUATION_ADDRESS,
        &[
            Instruction::LdaZeroPage(0x00),
            Instruction::Pha,
            Instruction::LdaZeroPage(0x01),
            Instruction::Pha,
            Instruction::JmpAbsolute(SOURCE_NMI_FIRST_CALL),
        ],
    );
    write_fixed(
        &mut bytes,
        NMI_EXIT_TRAMPOLINE_ADDRESS,
        &[
            Instruction::Pla,
            select_register_instruction(),
            Instruction::Pla,
            Instruction::Tay,
            Instruction::Pla,
            Instruction::JmpAbsolute(0xC1C0),
        ],
    );
    write_fixed(
        &mut bytes,
        SOURCE_NMI_STACK_EXTENSION,
        &[
            Instruction::LdaZeroPage(SELECTED_REGISTER_SHADOW),
            Instruction::Pha,
            Instruction::JmpAbsolute(NMI_ENTRY_CONTINUATION_ADDRESS),
        ],
    );
    write_fixed(
        &mut bytes,
        SOURCE_NMI_FIRST_CALL,
        &[
            Instruction::JsrAbsolute(first_nmi_call_target),
            Instruction::JsrAbsolute(SOURCE_NMI_SECOND_CALL),
        ],
    );
    write_fixed(
        &mut bytes,
        SOURCE_NMI_UNIVERSAL_EPILOGUE,
        &[Instruction::JmpAbsolute(NMI_EXIT_TRAMPOLINE_ADDRESS)],
    );
    write_fixed(
        &mut bytes,
        0xC1C0,
        &[
            Instruction::Tax,
            Instruction::Pla,
            Instruction::Plp,
            Instruction::Rti,
        ],
    );
    write_fixed(
        &mut bytes,
        SOURCE_PRG_SHADOW_READER,
        &[
            Instruction::LdaZeroPage(CANONICAL_PRG_BANK_SHADOW),
            Instruction::StaZeroPage(0x08),
        ],
    );
    write_fixed(
        &mut bytes,
        super::super::SOURCE_SELECT_PRG_BANK_AND_SAVE_ADDRESS,
        &[
            Instruction::StaZeroPage(CANONICAL_PRG_BANK_SHADOW),
            Instruction::Nop,
            Instruction::Nop,
            Instruction::JsrAbsolute(super::super::SELECT_PRG_BANK_ADDRESS),
            Instruction::Rts,
        ],
    );
    Rom::parse(bytes).unwrap()
}

#[derive(Clone)]
struct AbstractMapper {
    selected: u8,
    shadow: u8,
    values: [u8; 8],
}

impl AbstractMapper {
    fn interrupt(&mut self) {
        let saved = self.shadow;
        for (register, value) in [(4, 0x44), (2, 0x22), (7, 0x77)] {
            self.shadow = register;
            self.selected = register;
            self.values[usize::from(register)] = value;
        }
        self.shadow = saved;
        self.selected = saved;
    }
}

#[test]
fn an_nmi_at_every_selector_step_restores_the_interrupted_register() {
    for register in [0, 1, 2, 4, 6, 7] {
        for interrupt_boundary in 0..=3 {
            let mut mapper = AbstractMapper {
                selected: 3,
                shadow: 3,
                values: [0; 8],
            };
            if interrupt_boundary == 0 {
                mapper.interrupt();
            }
            mapper.shadow = register;
            if interrupt_boundary == 1 {
                mapper.interrupt();
            }
            mapper.selected = register;
            if interrupt_boundary == 2 {
                mapper.interrupt();
            }
            let pending_value = 0xA5;
            if interrupt_boundary == 3 {
                mapper.interrupt();
            }
            mapper.values[usize::from(mapper.selected)] = pending_value;
            assert_eq!(mapper.values[usize::from(register)], 0xA5);
        }
    }
}

#[test]
fn nmi_entry_and_universal_exit_restore_the_exact_stack_order() {
    let mut stack = vec![
        "return_hi",
        "return_lo",
        "interrupt_p",
        "saved_p",
        "saved_a",
        "saved_x",
        "saved_y",
    ];
    stack.extend(["selector", "zero_page_00", "zero_page_01"]);

    for call in ["C3A5", "C296"] {
        stack.extend(["call_return_hi", "call_return_lo"]);
        assert_eq!(stack.pop(), Some("call_return_lo"), "{call} RTS low");
        assert_eq!(stack.pop(), Some("call_return_hi"), "{call} RTS high");
    }
    assert_eq!(stack.pop(), Some("zero_page_01"));
    assert_eq!(stack.pop(), Some("zero_page_00"));
    assert_eq!(stack.pop(), Some("selector"));
    assert_eq!(stack.pop(), Some("saved_y"));
    assert_eq!(stack.pop(), Some("saved_x"));
    assert_eq!(stack.pop(), Some("saved_a"));
    assert_eq!(stack.pop(), Some("saved_p"));
    assert_eq!(stack.pop(), Some("interrupt_p"));
    assert_eq!(stack.pop(), Some("return_lo"));
    assert_eq!(stack.pop(), Some("return_hi"));
    assert!(stack.is_empty());
}

#[test]
fn nmi_trampolines_fit_their_exact_cross_stage_gaps() {
    let entry = assemble_at(
        NMI_ENTRY_CONTINUATION_ADDRESS,
        &[
            Instruction::LdaZeroPage(0x00),
            Instruction::Pha,
            Instruction::LdaZeroPage(0x01),
            Instruction::Pha,
            Instruction::JmpAbsolute(SOURCE_NMI_FIRST_CALL),
        ],
    )
    .unwrap();
    let exit = assemble_at(
        NMI_EXIT_TRAMPOLINE_ADDRESS,
        &[
            Instruction::Pla,
            select_register_instruction(),
            Instruction::Pla,
            Instruction::Tay,
            Instruction::Pla,
            Instruction::JmpAbsolute(0xC1C0),
        ],
    )
    .unwrap();

    assert_eq!(entry.len(), 9);
    assert!(usize::from(NMI_ENTRY_CONTINUATION_ADDRESS) + entry.len() <= 0xFA80);
    assert_eq!(exit.len(), 10);
    assert_eq!(
        usize::from(NMI_EXIT_TRAMPOLINE_ADDRESS) + exit.len(),
        0xFAA0
    );
}

#[test]
fn two_stage_prg_selection_survives_an_interrupt_before_each_value_write() {
    let mut mapper = AbstractMapper {
        selected: 2,
        shadow: 2,
        values: [0; 8],
    };
    for (register, value) in [(6, 0x1C), (7, 0x1D)] {
        mapper.shadow = register;
        mapper.selected = register;
        mapper.interrupt();
        mapper.values[usize::from(mapper.selected)] = value;
    }

    assert_eq!(mapper.values[6], 0x1C);
    assert_eq!(mapper.values[7], 0x1D);
}

#[test]
fn selector_writer_callee_cost_is_derived_from_its_typed_body() {
    let body = [
        Instruction::StaZeroPage(SELECTED_REGISTER_SHADOW),
        Instruction::StaAbsolute(0x8000),
        Instruction::Rts,
    ];
    let cycles = body
        .into_iter()
        .map(Instruction::worst_case_cycles)
        .map(u32::from)
        .sum::<u32>();

    assert_eq!(cycles, SELECT_REGISTER_CALLEE_CYCLES);
}

#[test]
fn shadow_operand_census_uses_typed_direct_indexed_and_pointer_semantics() {
    // LDA $51, LDA $0051,X, JMP ($0051) all depend on the operand as memory.
    assert!(documented_memory_operand(0xA5, &[0x51]));
    assert!(documented_memory_operand(0xBD, &[0x51, 0x00]));
    assert!(documented_memory_operand(0x6C, &[0x51, 0x00]));

    // Immediate and relative operands happen to carry the same byte but do not access it.
    assert!(!documented_memory_operand(0xA9, &[0x51]));
    assert!(!documented_memory_operand(0xD0, &[0x51]));
}

#[test]
fn final_contract_accepts_only_its_exact_nmi_trampoline_call() {
    let candidate = synthetic_installed_candidate(0xF400);

    verify_final_installed_contract(&candidate, 0xF400).unwrap();
    assert!(verify_final_installed_contract(&candidate, SOURCE_NMI_DISPLACED_CALL).is_err());
}
