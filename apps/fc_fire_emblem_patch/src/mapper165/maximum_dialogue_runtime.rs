use anyhow::{Context, Result, ensure};
use retro_rp2a03::{AddressingMode, Mnemonic, Operand, decode_bytes};

use crate::{
    dialogue_runtime_state::MAIN_DIALOGUE_RUNTIME_STATE,
    rp2a03::{Instruction, assemble_at},
    typed_source::decode_rp2a03_sequence,
};

pub(super) const FONT_GROUP_SELECTOR_ADDRESS: u16 = 0xF341;
pub(super) const FONT_GROUP_SELECTOR_END: u16 = 0xF378;
pub(super) const MAXIMUM_DIALOGUE_PAGE_RELOAD_ADDRESS: u16 = 0xBDF2;
pub(super) const MAXIMUM_DIALOGUE_PAGE_RELOAD_END: u16 = 0xBE14;
pub(super) const INITIAL_PAGE_SELECTOR_ADDRESS: u16 = 0xF990;
pub(super) const INITIAL_PAGE_SELECTOR_CAVE_END: u16 = 0xFA00;
pub(super) const MAIN_DIALOGUE_PRG_BANK: u8 = 0x0A;
pub(super) const INITIAL_FONT_SUPPLY_POINTER_ADVANCE: u16 = 4;
pub(super) const COMPLETED_PAGE_CONTINUE_ADDRESS: u16 = 0x85C9;
pub(super) const COMPLETED_PAGE_CONTINUE_SOURCE: [u8; 29] = [
    0xAD, 0x02, 0x78, 0xF0, 0x04, 0xA9, 0x0F, 0xD0, 0x10, 0xA9, 0x00, 0x8D, 0x04, 0x78, 0xAD, 0x0A,
    0x78, 0xF0, 0x04, 0xA9, 0x10, 0xD0, 0x02, 0xA9, 0x09, 0x8D, 0xF7, 0x77, 0x60,
];

const CURRENT_POINTER_LOW: u16 = 0x7812;
const CURRENT_POINTER_HIGH: u16 = 0x7814;
const DIALOGUE_STATE: u16 = MAIN_DIALOGUE_RUNTIME_STATE.state_address;
const CONTINUE_DECODE_STATE: u8 = 0x09;
const MAXIMUM_DIALOGUE_RUNTIME_IDENTITY: [(u16, u8); 4] = [
    (0x7674, 0x07),
    (MAIN_DIALOGUE_RUNTIME_STATE.entry_index_address, 0x18),
    (0x77F2, 0x0C),
    (MAIN_DIALOGUE_RUNTIME_STATE.directory_selector_address, 0xC0),
];

pub(super) fn build_font_group_selector(
    mapper_registers: [u8; 3],
    transition_pointers: [u16; 2],
) -> Result<Vec<u8>> {
    ensure!(
        mapper_registers.iter().all(|register| *register != 0),
        "maximum dialogue mapper register cannot be zero"
    );
    ensure!(
        transition_pointers[0] < transition_pointers[1],
        "maximum dialogue transition pointers are not increasing"
    );
    let [first_transition, second_transition] = transition_pointers;
    let mut instructions = vec![
        Instruction::LdaAbsolute(CURRENT_POINTER_HIGH),
        Instruction::CmpImmediate((first_transition >> 8) as u8),
        Instruction::BccAbsolute(FONT_GROUP_SELECTOR_ADDRESS),
        Instruction::BneAbsolute(FONT_GROUP_SELECTOR_ADDRESS),
        Instruction::LdaAbsolute(CURRENT_POINTER_LOW),
        Instruction::CmpImmediate(first_transition as u8),
        Instruction::BccAbsolute(FONT_GROUP_SELECTOR_ADDRESS),
    ];
    let first_high_less = 2;
    let first_high_greater = 3;
    let first_low_less = 6;
    let check_second = next_address(FONT_GROUP_SELECTOR_ADDRESS, &instructions)?;
    instructions.extend([
        Instruction::LdaAbsolute(CURRENT_POINTER_HIGH),
        Instruction::CmpImmediate((second_transition >> 8) as u8),
        Instruction::BccAbsolute(FONT_GROUP_SELECTOR_ADDRESS),
        Instruction::BneAbsolute(FONT_GROUP_SELECTOR_ADDRESS),
        Instruction::LdaAbsolute(CURRENT_POINTER_LOW),
        Instruction::CmpImmediate(second_transition as u8),
        Instruction::BccAbsolute(FONT_GROUP_SELECTOR_ADDRESS),
    ]);
    let second_high_less = 9;
    let second_high_greater = 10;
    let second_low_less = 13;
    let group_two = next_address(FONT_GROUP_SELECTOR_ADDRESS, &instructions)?;
    instructions.extend([
        Instruction::LdaImmediate(mapper_registers[2]),
        Instruction::BneAbsolute(FONT_GROUP_SELECTOR_ADDRESS),
    ]);
    let group_two_to_write = instructions.len() - 1;
    let group_one = next_address(FONT_GROUP_SELECTOR_ADDRESS, &instructions)?;
    instructions.extend([
        Instruction::LdaImmediate(mapper_registers[1]),
        Instruction::BneAbsolute(FONT_GROUP_SELECTOR_ADDRESS),
    ]);
    let group_one_to_write = instructions.len() - 1;
    let group_zero = next_address(FONT_GROUP_SELECTOR_ADDRESS, &instructions)?;
    instructions.push(Instruction::LdaImmediate(mapper_registers[0]));
    let write_mapper = next_address(FONT_GROUP_SELECTOR_ADDRESS, &instructions)?;
    instructions.extend([
        Instruction::Pha,
        Instruction::LdaImmediate(2),
        crate::mapper165::selector_safety::select_register_instruction(),
        Instruction::Pla,
        Instruction::StaAbsolute(0x8001),
        Instruction::LdaImmediate(CONTINUE_DECODE_STATE),
        Instruction::Rts,
    ]);

    instructions[first_high_less] = Instruction::BccAbsolute(group_zero);
    instructions[first_high_greater] = Instruction::BneAbsolute(check_second);
    instructions[first_low_less] = Instruction::BccAbsolute(group_zero);
    instructions[second_high_less] = Instruction::BccAbsolute(group_one);
    instructions[second_high_greater] = Instruction::BneAbsolute(group_two);
    instructions[second_low_less] = Instruction::BccAbsolute(group_one);
    instructions[group_two_to_write] = Instruction::BneAbsolute(write_mapper);
    instructions[group_one_to_write] = Instruction::BneAbsolute(write_mapper);

    let bytes = assemble_at(FONT_GROUP_SELECTOR_ADDRESS, &instructions)?;
    ensure!(
        FONT_GROUP_SELECTOR_ADDRESS as usize + bytes.len() <= FONT_GROUP_SELECTOR_END as usize,
        "maximum dialogue font-group selector exceeds its cave partition"
    );
    decode_rp2a03_sequence(
        &bytes,
        FONT_GROUP_SELECTOR_ADDRESS,
        "maximum dialogue font-group selector",
    )?;
    Ok(bytes)
}

pub(super) fn build_completed_page_continue_hook() -> Result<Vec<u8>> {
    let mut instructions = vec![
        Instruction::LdaAbsolute(0x7802),
        Instruction::BneAbsolute(COMPLETED_PAGE_CONTINUE_ADDRESS),
        Instruction::StaAbsolute(0x7804),
        Instruction::LdaAbsolute(0x780A),
        Instruction::BneAbsolute(COMPLETED_PAGE_CONTINUE_ADDRESS),
        Instruction::JsrAbsolute(MAXIMUM_DIALOGUE_PAGE_RELOAD_ADDRESS),
        Instruction::BneAbsolute(COMPLETED_PAGE_CONTINUE_ADDRESS),
        Instruction::LdaImmediate(0x10),
        Instruction::BneAbsolute(COMPLETED_PAGE_CONTINUE_ADDRESS),
        Instruction::LdaImmediate(0x0F),
        Instruction::StaAbsolute(DIALOGUE_STATE),
        Instruction::Rts,
        Instruction::Rts,
    ];
    let state_ten = next_address(COMPLETED_PAGE_CONTINUE_ADDRESS, &instructions[..7])?;
    let state_fifteen = next_address(COMPLETED_PAGE_CONTINUE_ADDRESS, &instructions[..9])?;
    let store_state = next_address(COMPLETED_PAGE_CONTINUE_ADDRESS, &instructions[..10])?;
    instructions[1] = Instruction::BneAbsolute(state_fifteen);
    instructions[4] = Instruction::BneAbsolute(state_ten);
    instructions[6] = Instruction::BneAbsolute(store_state);
    instructions[8] = Instruction::BneAbsolute(store_state);
    let bytes = assemble_at(COMPLETED_PAGE_CONTINUE_ADDRESS, &instructions)?;
    ensure!(
        bytes.len() == COMPLETED_PAGE_CONTINUE_SOURCE.len(),
        "maximum dialogue completed-page transition changed source span"
    );
    decode_rp2a03_sequence(
        &bytes,
        COMPLETED_PAGE_CONTINUE_ADDRESS,
        "maximum dialogue completed-page transition",
    )?;
    Ok(bytes)
}

/// 공용 완료 페이지 훅에서 최대 대사 수명만 글꼴 그룹을 갱신한다.
///
/// 같은 `$85C9` 상태 전이는 모든 주 대사가 공유하므로 현재 포인터의 숫자 범위만으로
/// 최대 대사를 판정할 수 없다. 원천 생산자가 결속한 장·entry·바깥 화면·directory
/// selector가 모두 일치할 때만 고정 뱅크의 그룹 선택기로 이어진다. 다른 대사에서는
/// 기존 CHR 페이지를 건드리지 않고 원본 continue 상태만 반환한다.
pub(super) fn build_maximum_dialogue_page_reload() -> Result<Vec<u8>> {
    let mut instructions = Vec::new();
    let mut mismatch_branches = Vec::new();
    for (address, expected) in MAXIMUM_DIALOGUE_RUNTIME_IDENTITY {
        instructions.extend([
            Instruction::LdaAbsolute(address),
            Instruction::CmpImmediate(expected),
            Instruction::BneAbsolute(MAXIMUM_DIALOGUE_PAGE_RELOAD_ADDRESS),
        ]);
        mismatch_branches.push(instructions.len() - 1);
    }
    instructions.push(Instruction::JmpAbsolute(FONT_GROUP_SELECTOR_ADDRESS));
    let preserve_current_page = next_address(MAXIMUM_DIALOGUE_PAGE_RELOAD_ADDRESS, &instructions)?;
    instructions.extend([
        Instruction::LdaImmediate(CONTINUE_DECODE_STATE),
        Instruction::Rts,
    ]);
    for branch in mismatch_branches {
        instructions[branch] = Instruction::BneAbsolute(preserve_current_page);
    }

    let bytes = assemble_at(MAXIMUM_DIALOGUE_PAGE_RELOAD_ADDRESS, &instructions)?;
    ensure!(
        MAXIMUM_DIALOGUE_PAGE_RELOAD_ADDRESS as usize + bytes.len()
            == MAXIMUM_DIALOGUE_PAGE_RELOAD_END as usize,
        "maximum dialogue page reload changed its exact bank-local cave span"
    );
    decode_rp2a03_sequence(
        &bytes,
        MAXIMUM_DIALOGUE_PAGE_RELOAD_ADDRESS,
        "maximum dialogue owned completed-page reload",
    )?;
    Ok(bytes)
}

pub(super) fn build_initial_page_selector(
    fallback_target: u16,
    initial_supply_pointer: u16,
) -> Result<Vec<u8>> {
    let mut instructions = vec![Instruction::Php, Instruction::Pha];
    let mut mismatch_branches = Vec::new();
    for (address, expected) in [
        (0x24, 0x0C),
        (0x84, 0x3C),
        (0x59, 0x1B),
        (0x5A, 0x1B),
        (0x5B, 0x00),
        (0x5C, 0x18),
    ] {
        instructions.extend([
            Instruction::LdaZeroPage(address),
            Instruction::CmpImmediate(expected),
            Instruction::BneAbsolute(INITIAL_PAGE_SELECTOR_ADDRESS),
        ]);
        mismatch_branches.push(instructions.len() - 1);
    }
    for (address, expected) in MAXIMUM_DIALOGUE_RUNTIME_IDENTITY.into_iter().chain([
        (MAIN_DIALOGUE_RUNTIME_STATE.state_address, 0x05),
        (CURRENT_POINTER_LOW, initial_supply_pointer as u8),
        (CURRENT_POINTER_HIGH, (initial_supply_pointer >> 8) as u8),
    ]) {
        instructions.extend([
            Instruction::LdaAbsolute(address),
            Instruction::CmpImmediate(expected),
            Instruction::BneAbsolute(INITIAL_PAGE_SELECTOR_ADDRESS),
        ]);
        mismatch_branches.push(instructions.len() - 1);
    }
    instructions.extend([
        Instruction::JsrAbsolute(FONT_GROUP_SELECTOR_ADDRESS),
        Instruction::Pla,
        Instruction::Plp,
        Instruction::Rts,
    ]);
    let mismatch = next_address(INITIAL_PAGE_SELECTOR_ADDRESS, &instructions)?;
    instructions.extend([
        Instruction::Pla,
        Instruction::Plp,
        Instruction::JmpAbsolute(fallback_target),
    ]);
    for branch in mismatch_branches {
        instructions[branch] = Instruction::BneAbsolute(mismatch);
    }
    let bytes = assemble_at(INITIAL_PAGE_SELECTOR_ADDRESS, &instructions)?;
    ensure!(
        INITIAL_PAGE_SELECTOR_ADDRESS as usize + bytes.len()
            <= INITIAL_PAGE_SELECTOR_CAVE_END as usize,
        "maximum dialogue initial selector exceeds its cave"
    );
    decode_rp2a03_sequence(
        &bytes,
        INITIAL_PAGE_SELECTOR_ADDRESS,
        "maximum dialogue initial page selector",
    )?;
    Ok(bytes)
}

pub(super) fn bind_installed_initial_page_selector(
    bytes: &[u8],
    fallback_target: u16,
) -> Result<u16> {
    let mut offset = 0_usize;
    let mut pointer_low = None;
    let mut pointer_high = None;
    while offset < bytes.len() {
        let instruction = decode_bytes(&bytes[offset..]).with_context(|| {
            format!("decode installed maximum-dialogue selector at +0x{offset:X}")
        })?;
        ensure!(
            instruction.opcode_is_documented(),
            "installed maximum-dialogue selector contains an undocumented instruction"
        );
        if instruction.mnemonic() == Mnemonic::Lda
            && instruction.addressing_mode() == AddressingMode::Absolute
        {
            let destination = match instruction.operand() {
                Operand::Word(destination) => destination,
                _ => unreachable!("absolute LDA must have a word operand"),
            };
            if destination == CURRENT_POINTER_LOW || destination == CURRENT_POINTER_HIGH {
                let compare_offset = offset + instruction.encoded_len();
                let compare = decode_bytes(
                    bytes
                        .get(compare_offset..)
                        .context("installed maximum-dialogue pointer compare is truncated")?,
                )?;
                ensure!(
                    compare.opcode_is_documented()
                        && compare.mnemonic() == Mnemonic::Cmp
                        && compare.addressing_mode() == AddressingMode::Immediate,
                    "installed maximum-dialogue pointer load is not followed by CMP immediate"
                );
                let value = match compare.operand() {
                    Operand::Byte(value) => value,
                    _ => unreachable!("immediate CMP must have a byte operand"),
                };
                let slot = if destination == CURRENT_POINTER_LOW {
                    &mut pointer_low
                } else {
                    &mut pointer_high
                };
                ensure!(
                    slot.replace(value).is_none(),
                    "installed maximum-dialogue selector repeats a pointer comparison"
                );
            }
        }
        offset += instruction.encoded_len();
    }
    ensure!(
        offset == bytes.len(),
        "installed maximum-dialogue selector decode did not consume its region"
    );
    let pointer = u16::from(pointer_low.context("maximum-dialogue pointer low compare missing")?)
        | (u16::from(pointer_high.context("maximum-dialogue pointer high compare missing")?) << 8);
    ensure!(
        build_initial_page_selector(fallback_target, pointer)? == bytes,
        "installed maximum-dialogue selector changed outside its bound initial pointer"
    );
    Ok(pointer)
}

#[cfg(test)]
fn font_group_for_pointer(pointer: u16, transition_pointers: [u16; 2]) -> usize {
    if pointer < transition_pointers[0] {
        0
    } else if pointer < transition_pointers[1] {
        1
    } else {
        2
    }
}

fn next_address(origin: u16, instructions: &[Instruction]) -> Result<u16> {
    origin
        .checked_add(
            u16::try_from(assemble_at(origin, instructions)?.len())
                .context("maximum dialogue routine length does not fit u16")?,
        )
        .context("maximum dialogue routine address overflow")
}

#[cfg(test)]
mod tests {
    use super::*;

    const STATUS_CARRY: u8 = 0x01;
    const STATUS_ZERO: u8 = 0x02;

    struct CompletedPageResult {
        mapper_values: Vec<u8>,
        dialogue_state: u8,
    }

    struct TestCpu {
        memory: Box<[u8; 0x10000]>,
        a: u8,
        p: u8,
        sp: u8,
        pc: u16,
        mapper_values: Vec<u8>,
    }

    impl TestCpu {
        fn run_completed_page(
            identity: [(u16, u8); 4],
            pointer: u16,
            first_completion_flag: u8,
            second_completion_flag: u8,
        ) -> CompletedPageResult {
            let hook = build_completed_page_continue_hook().unwrap();
            let reload = build_maximum_dialogue_page_reload().unwrap();
            let group_selector =
                build_font_group_selector([0xC8, 0xCC, 0xD0], [0x90C0, 0x9192]).unwrap();
            let mut memory: Box<[u8; 0x10000]> =
                vec![0; 0x10000].into_boxed_slice().try_into().unwrap();
            for (address, value) in identity {
                memory[usize::from(address)] = value;
            }
            memory[usize::from(CURRENT_POINTER_LOW)] = pointer as u8;
            memory[usize::from(CURRENT_POINTER_HIGH)] = (pointer >> 8) as u8;
            memory[0x7802] = first_completion_flag;
            memory[0x780A] = second_completion_flag;
            Self::install(&mut memory, COMPLETED_PAGE_CONTINUE_ADDRESS, &hook);
            Self::install(&mut memory, MAXIMUM_DIALOGUE_PAGE_RELOAD_ADDRESS, &reload);
            Self::install(&mut memory, FONT_GROUP_SELECTOR_ADDRESS, &group_selector);

            let mut cpu = Self {
                memory,
                a: 0,
                p: 0,
                sp: 0xFD,
                pc: COMPLETED_PAGE_CONTINUE_ADDRESS,
                mapper_values: Vec::new(),
            };
            let mut returned = false;
            for _ in 0..256 {
                match cpu.read_pc() {
                    0x20 => {
                        let target = cpu.read_word_pc();
                        if target
                            == crate::mapper165::selector_safety::SELECT_REGISTER_ROUTINE_ADDRESS
                        {
                            continue;
                        }
                        let return_address = cpu.pc.wrapping_sub(1);
                        cpu.push((return_address >> 8) as u8);
                        cpu.push(return_address as u8);
                        cpu.pc = target;
                    }
                    0x48 => cpu.push(cpu.a),
                    0x4C => cpu.pc = cpu.read_word_pc(),
                    0x60 => {
                        if cpu.sp == 0xFD {
                            returned = true;
                            break;
                        }
                        let low = cpu.pop();
                        let high = cpu.pop();
                        cpu.pc = u16::from_le_bytes([low, high]).wrapping_add(1);
                    }
                    0x68 => {
                        cpu.a = cpu.pop();
                        cpu.set_zero(cpu.a == 0);
                    }
                    0x8D => {
                        let address = cpu.read_word_pc();
                        cpu.memory[usize::from(address)] = cpu.a;
                        if address == 0x8001 {
                            cpu.mapper_values.push(cpu.a);
                        }
                    }
                    0x90 => {
                        let displacement = cpu.read_pc() as i8;
                        if cpu.p & STATUS_CARRY == 0 {
                            cpu.pc = cpu.pc.wrapping_add_signed(i16::from(displacement));
                        }
                    }
                    0xA9 => {
                        cpu.a = cpu.read_pc();
                        cpu.set_zero(cpu.a == 0);
                    }
                    0xAD => {
                        let address = cpu.read_word_pc();
                        cpu.a = cpu.memory[usize::from(address)];
                        cpu.set_zero(cpu.a == 0);
                    }
                    0xC9 => {
                        let value = cpu.read_pc();
                        cpu.set_zero(cpu.a == value);
                        cpu.set_carry(cpu.a >= value);
                    }
                    0xD0 => {
                        let displacement = cpu.read_pc() as i8;
                        if cpu.p & STATUS_ZERO == 0 {
                            cpu.pc = cpu.pc.wrapping_add_signed(i16::from(displacement));
                        }
                    }
                    opcode => panic!(
                        "maximum-dialogue completed-page test reached unsupported opcode {opcode:02X}"
                    ),
                }
            }
            assert!(returned, "completed-page test routine did not return");
            CompletedPageResult {
                mapper_values: cpu.mapper_values,
                dialogue_state: cpu.memory[usize::from(DIALOGUE_STATE)],
            }
        }

        fn install(memory: &mut [u8; 0x10000], address: u16, bytes: &[u8]) {
            let start = usize::from(address);
            memory[start..start + bytes.len()].copy_from_slice(bytes);
        }

        fn read_pc(&mut self) -> u8 {
            let value = self.memory[usize::from(self.pc)];
            self.pc = self.pc.wrapping_add(1);
            value
        }

        fn read_word_pc(&mut self) -> u16 {
            let low = self.read_pc();
            let high = self.read_pc();
            u16::from_le_bytes([low, high])
        }

        fn push(&mut self, value: u8) {
            self.memory[0x100 + usize::from(self.sp)] = value;
            self.sp = self.sp.wrapping_sub(1);
        }

        fn pop(&mut self) -> u8 {
            self.sp = self.sp.wrapping_add(1);
            self.memory[0x100 + usize::from(self.sp)]
        }

        fn set_zero(&mut self, set: bool) {
            if set {
                self.p |= STATUS_ZERO;
            } else {
                self.p &= !STATUS_ZERO;
            }
        }

        fn set_carry(&mut self, set: bool) {
            if set {
                self.p |= STATUS_CARRY;
            } else {
                self.p &= !STATUS_CARRY;
            }
        }
    }

    #[test]
    fn pointer_boundaries_select_the_next_page_font_group() {
        let transitions = [0x9120, 0x9250];

        assert_eq!(font_group_for_pointer(0x8FFF, transitions), 0);
        assert_eq!(font_group_for_pointer(0x911F, transitions), 0);
        assert_eq!(font_group_for_pointer(0x9120, transitions), 1);
        assert_eq!(font_group_for_pointer(0x924F, transitions), 1);
        assert_eq!(font_group_for_pointer(0x9250, transitions), 2);
    }

    #[test]
    fn completed_page_transition_preserves_terminal_and_idle_branches() {
        let hook = build_completed_page_continue_hook().unwrap();

        assert_eq!(hook.len(), COMPLETED_PAGE_CONTINUE_SOURCE.len());
        assert_eq!(&hook[..3], &[0xAD, 0x02, 0x78]);
        assert_eq!(&hook[hook.len() - 2..], &[0x60, 0x60]);
        assert_eq!(
            COMPLETED_PAGE_CONTINUE_ADDRESS + 28,
            0x85E5,
            "the no-input BEQ target must remain the final RTS"
        );
    }

    #[test]
    fn chapter_one_completed_page_preserves_its_existing_font_page() {
        let result = TestCpu::run_completed_page(
            [
                (0x7674, 0x01),
                (MAIN_DIALOGUE_RUNTIME_STATE.entry_index_address, 0x03),
                (0x77F2, 0x08),
                (MAIN_DIALOGUE_RUNTIME_STATE.directory_selector_address, 0x80),
            ],
            0xE3A0,
            0,
            0,
        );

        assert_eq!(result.dialogue_state, CONTINUE_DECODE_STATE);
        assert!(result.mapper_values.is_empty());
    }

    #[test]
    fn maximum_dialogue_identity_alone_selects_completed_page_font_groups() {
        for (pointer, expected_mapper_value) in [(0x901C, 0xC8), (0x90F0, 0xCC), (0x91B3, 0xD0)] {
            let result =
                TestCpu::run_completed_page(MAXIMUM_DIALOGUE_RUNTIME_IDENTITY, pointer, 0, 0);
            assert_eq!(result.dialogue_state, CONTINUE_DECODE_STATE);
            assert_eq!(result.mapper_values, [expected_mapper_value]);
        }

        for index in 0..MAXIMUM_DIALOGUE_RUNTIME_IDENTITY.len() {
            let mut identity = MAXIMUM_DIALOGUE_RUNTIME_IDENTITY;
            identity[index].1 ^= 1;
            let result = TestCpu::run_completed_page(identity, 0x91B3, 0, 0);
            assert_eq!(result.dialogue_state, CONTINUE_DECODE_STATE);
            assert!(result.mapper_values.is_empty());
        }
    }

    #[test]
    fn terminal_and_idle_completed_page_states_do_not_reload_fonts() {
        let terminal = TestCpu::run_completed_page(MAXIMUM_DIALOGUE_RUNTIME_IDENTITY, 0x91B3, 1, 0);
        assert_eq!(terminal.dialogue_state, 0x0F);
        assert!(terminal.mapper_values.is_empty());

        let idle = TestCpu::run_completed_page(MAXIMUM_DIALOGUE_RUNTIME_IDENTITY, 0x91B3, 0, 1);
        assert_eq!(idle.dialogue_state, 0x10);
        assert!(idle.mapper_values.is_empty());
    }

    #[test]
    fn runtime_routines_fit_their_independent_checked_caves() {
        let group_selector =
            build_font_group_selector([0xC8, 0xCC, 0xD0], [0x9120, 0x9250]).unwrap();
        let page_reload = build_maximum_dialogue_page_reload().unwrap();
        let initial = build_initial_page_selector(0xFB80, 0x8FF1).unwrap();

        assert!(
            FONT_GROUP_SELECTOR_ADDRESS as usize + group_selector.len()
                <= FONT_GROUP_SELECTOR_END as usize
        );
        assert!(
            INITIAL_PAGE_SELECTOR_ADDRESS as usize + initial.len()
                <= INITIAL_PAGE_SELECTOR_CAVE_END as usize
        );
        assert_eq!(
            MAXIMUM_DIALOGUE_PAGE_RELOAD_ADDRESS as usize + page_reload.len(),
            MAXIMUM_DIALOGUE_PAGE_RELOAD_END as usize
        );
        assert_eq!(
            &group_selector[group_selector.len() - 3..],
            &[0xA9, 0x09, 0x60]
        );
        assert_eq!(&initial[initial.len() - 3..], &[0x4C, 0x80, 0xFB]);
    }
}
