//! 번역 글꼴 페이지와 원본 MMC4 FD/FE 쌍을 한 실행 규칙으로 투영한다.
//!
//! 번역 페이지 번호만 고르면 래치가 FD에서 FE로 바뀌는 순간 원본 페이지가 다시
//! 나타나거나, 반대로 FE 배경까지 번역 페이지로 덮인다. `$07FD`는 그래서 물리
//! 페이지와 FE 소유 비트를 함께 보관한다. 이 모듈은 화면별 호출자가 아니라 중앙
//! selector가 그 규칙을 매번 다시 적용하도록 한다.

use anyhow::{Context, Result, ensure};

use super::{RuntimeRoutine, chr_selector, chr_source_state, next_address};
use crate::{
    full_translation_install::runtime_state_storage::CONSUMER_FONT_PAGE,
    mapper165::font_pair_projection::{TRANSLATED_FE_PAGE_FLAG, WRITE_TRANSLATED_CHR_PAGE_ADDRESS},
    rp2a03::{Instruction, assemble_at},
};

pub(super) const ROUTE_CAVE_ORIGIN: u16 = chr_selector::SELECTOR_CAVE_ORIGIN;

const TRANSLATED_PAGE_MASK: u8 = 0xFC;
const RIGHT_FD_CHR_REGISTER: u8 = 2;
const RIGHT_FE_CHR_REGISTER: u8 = 4;
const STACKED_ROUTE_OFFSET_FROM_TSX: u16 = 0x0102;

pub(super) struct FontPageRouteRuntime {
    pub(super) routine: RuntimeRoutine,
    pub(super) apply_route: u16,
    pub(super) project_dialogue_page: u16,
    pub(super) select_active_page: u16,
    pub(super) dialogue_selector: u16,
}

pub(super) fn build_font_page_route_runtime() -> Result<FontPageRouteRuntime> {
    let apply_route = ROUTE_CAVE_ORIGIN;
    let apply = build_apply_route(apply_route)?;
    let project_dialogue_page = apply.address
        + u16::try_from(apply.bytes.len()).context("font-page route length overflow")?;
    let dialogue = build_dialogue_page_projection(project_dialogue_page, apply_route)?;
    let select_active_page = dialogue.address
        + u16::try_from(dialogue.bytes.len()).context("dialogue page projection overflow")?;
    let provisional_active = build_active_page_selector(select_active_page, apply_route, 0)?;
    let dialogue_selector = provisional_active.address
        + u16::try_from(provisional_active.bytes.len())
            .context("active font-page selector length overflow")?;
    let active = build_active_page_selector(select_active_page, apply_route, dialogue_selector)?;

    let mut bytes = Vec::new();
    for routine in [&apply, &dialogue, &active] {
        ensure!(
            usize::from(routine.address) == usize::from(ROUTE_CAVE_ORIGIN) + bytes.len(),
            "font-page route routines are not contiguous"
        );
        bytes.extend_from_slice(&routine.bytes);
    }
    let executable_byte_count = bytes.len();
    ensure!(
        usize::from(ROUTE_CAVE_ORIGIN) + executable_byte_count == usize::from(dialogue_selector),
        "font-page route runtime no longer hands directly to the dialogue selector"
    );

    Ok(FontPageRouteRuntime {
        routine: RuntimeRoutine {
            role: "translated font page route selector",
            address: ROUTE_CAVE_ORIGIN,
            bytes,
        },
        apply_route,
        project_dialogue_page,
        select_active_page,
        dialogue_selector,
    })
}

/// A의 encoded route를 오른쪽 FD에 적용하고, low bit가 켜진 경우 FE에도 같은
/// 페이지를 적용한다. X와 A는 호출 전 값/route로 복원한다.
fn build_apply_route(origin: u16) -> Result<RuntimeRoutine> {
    let mut instructions = vec![
        Instruction::Pha,
        Instruction::Txa,
        Instruction::Pha,
        Instruction::Tsx,
        Instruction::LdaAbsoluteX(STACKED_ROUTE_OFFSET_FROM_TSX),
        Instruction::AndImmediate(TRANSLATED_PAGE_MASK),
        Instruction::LdxImmediate(RIGHT_FD_CHR_REGISTER),
        Instruction::JsrAbsolute(WRITE_TRANSLATED_CHR_PAGE_ADDRESS),
        Instruction::Tsx,
        Instruction::LdaAbsoluteX(STACKED_ROUTE_OFFSET_FROM_TSX),
        Instruction::AndImmediate(TRANSLATED_FE_PAGE_FLAG),
    ];
    let preserve_fe = instructions.len();
    instructions.push(Instruction::BeqAbsolute(origin));
    instructions.extend([
        Instruction::LdaAbsoluteX(STACKED_ROUTE_OFFSET_FROM_TSX),
        Instruction::AndImmediate(TRANSLATED_PAGE_MASK),
        Instruction::LdxImmediate(RIGHT_FE_CHR_REGISTER),
        Instruction::JsrAbsolute(WRITE_TRANSLATED_CHR_PAGE_ADDRESS),
    ]);
    let restore = next_address(origin, &instructions)?;
    instructions[preserve_fe] = Instruction::BeqAbsolute(restore);
    instructions.extend([
        Instruction::Pla,
        Instruction::Tax,
        Instruction::Pla,
        Instruction::Rts,
    ]);
    Ok(RuntimeRoutine {
        role: "apply translated font page route",
        address: origin,
        bytes: assemble_at(origin, &instructions)?,
    })
}

/// 대사 페이지는 원본 FE가 FD와 같은 페이지일 때만 두 래치를 함께 번역 페이지로
/// 보낸다. 서로 다른 원본 FE 배경은 그대로 보존한다.
fn build_dialogue_page_projection(origin: u16, apply_route: u16) -> Result<RuntimeRoutine> {
    let mut instructions = vec![
        Instruction::Pha,
        Instruction::LdaZeroPage(chr_source_state::RIGHT_FE_SOURCE_SHADOW),
        Instruction::OraZeroPage(chr_source_state::CHR_SOURCE_HIGH_BITS),
        Instruction::AndImmediate(0x1F),
        Instruction::CmpImmediate(chr_source_state::DIALOGUE_FD_SOURCE_PAGE),
    ];
    let preserve_fe = instructions.len();
    instructions.push(Instruction::BneAbsolute(origin));
    instructions.extend([
        Instruction::Pla,
        Instruction::OraImmediate(TRANSLATED_FE_PAGE_FLAG),
        Instruction::JmpAbsolute(apply_route),
    ]);
    let preserve_fe_target = next_address(origin, &instructions)?;
    instructions[preserve_fe] = Instruction::BneAbsolute(preserve_fe_target);
    instructions.extend([Instruction::Pla, Instruction::JmpAbsolute(apply_route)]);
    Ok(RuntimeRoutine {
        role: "project dialogue font page onto source FD and FE",
        address: origin,
        bytes: assemble_at(origin, &instructions)?,
    })
}

/// `$07FD`가 열려 있으면 화면 수명의 route가 대사보다 우선한다. 없을 때만 기존
/// 대사 selector로 넘긴다. selector 사슬이 운반하는 A/P는 두 경로 모두 보존한다.
fn build_active_page_selector(
    origin: u16,
    apply_route: u16,
    dialogue_selector: u16,
) -> Result<RuntimeRoutine> {
    let mut instructions = vec![
        Instruction::Php,
        Instruction::Pha,
        Instruction::LdaAbsolute(CONSUMER_FONT_PAGE),
    ];
    let no_consumer = instructions.len();
    instructions.push(Instruction::BeqAbsolute(origin));
    instructions.extend([
        Instruction::JsrAbsolute(apply_route),
        Instruction::Pla,
        Instruction::Plp,
        Instruction::Rts,
    ]);
    let no_consumer_target = next_address(origin, &instructions)?;
    instructions[no_consumer] = Instruction::BeqAbsolute(no_consumer_target);
    instructions.extend([
        Instruction::Pla,
        Instruction::Plp,
        Instruction::JmpAbsolute(dialogue_selector),
    ]);
    Ok(RuntimeRoutine {
        role: "select active translated font page route",
        address: origin,
        bytes: assemble_at(origin, &instructions)?,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    const STATUS_ZERO: u8 = 0x02;

    #[derive(Default)]
    struct RunResult {
        writes: Vec<(u8, u8)>,
        fallback: Option<u16>,
        a: u8,
        x: u8,
        p: u8,
    }

    struct TestCpu {
        memory: Box<[u8; 0x10000]>,
        a: u8,
        x: u8,
        p: u8,
        sp: u8,
        pc: u16,
        writes: Vec<(u8, u8)>,
        fallback: Option<u16>,
    }

    impl TestCpu {
        fn run(
            runtime: &FontPageRouteRuntime,
            entry: u16,
            memory: Box<[u8; 0x10000]>,
            a: u8,
            x: u8,
            p: u8,
        ) -> RunResult {
            let mut cpu = Self {
                memory,
                a,
                x,
                p,
                sp: 0xFD,
                pc: entry,
                writes: Vec::new(),
                fallback: None,
            };
            let start = usize::from(runtime.routine.address);
            let end = start + runtime.routine.bytes.len();
            cpu.memory[start..end].copy_from_slice(&runtime.routine.bytes);
            for _ in 0..256 {
                match cpu.read_pc() {
                    0x05 => {
                        let address = cpu.read_pc();
                        cpu.a |= cpu.memory[usize::from(address)];
                        cpu.set_zero(cpu.a == 0);
                    }
                    0x08 => cpu.push(cpu.p),
                    0x09 => {
                        cpu.a |= cpu.read_pc();
                        cpu.set_zero(cpu.a == 0);
                    }
                    0x20 => {
                        let target = cpu.read_word_pc();
                        if target == WRITE_TRANSLATED_CHR_PAGE_ADDRESS {
                            cpu.writes.push((cpu.x, cpu.a));
                        } else {
                            let return_address = cpu.pc.wrapping_sub(1);
                            cpu.push((return_address >> 8) as u8);
                            cpu.push(return_address as u8);
                            cpu.pc = target;
                        }
                    }
                    0x28 => cpu.p = cpu.pop(),
                    0x29 => {
                        cpu.a &= cpu.read_pc();
                        cpu.set_zero(cpu.a == 0);
                    }
                    0x48 => cpu.push(cpu.a),
                    0x4C => {
                        let target = cpu.read_word_pc();
                        if target == runtime.dialogue_selector {
                            cpu.fallback = Some(target);
                            break;
                        }
                        cpu.pc = target;
                    }
                    0x60 => {
                        if cpu.sp == 0xFD {
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
                    0x8A => {
                        cpu.a = cpu.x;
                        cpu.set_zero(cpu.a == 0);
                    }
                    0xA2 => {
                        cpu.x = cpu.read_pc();
                        cpu.set_zero(cpu.x == 0);
                    }
                    0xA5 => {
                        let address = cpu.read_pc();
                        cpu.a = cpu.memory[usize::from(address)];
                        cpu.set_zero(cpu.a == 0);
                    }
                    0xAA => {
                        cpu.x = cpu.a;
                        cpu.set_zero(cpu.x == 0);
                    }
                    0xAD => {
                        let address = cpu.read_word_pc();
                        cpu.a = cpu.memory[usize::from(address)];
                        cpu.set_zero(cpu.a == 0);
                    }
                    0xBA => {
                        cpu.x = cpu.sp;
                        cpu.set_zero(cpu.x == 0);
                    }
                    0xBD => {
                        let base = cpu.read_word_pc();
                        cpu.a = cpu.memory[usize::from(base.wrapping_add(u16::from(cpu.x)))];
                        cpu.set_zero(cpu.a == 0);
                    }
                    0xC9 => {
                        let value = cpu.read_pc();
                        cpu.set_zero(cpu.a == value);
                    }
                    0xD0 => {
                        let displacement = cpu.read_pc() as i8;
                        if cpu.p & STATUS_ZERO == 0 {
                            cpu.pc = cpu.pc.wrapping_add_signed(i16::from(displacement));
                        }
                    }
                    0xF0 => {
                        let displacement = cpu.read_pc() as i8;
                        if cpu.p & STATUS_ZERO != 0 {
                            cpu.pc = cpu.pc.wrapping_add_signed(i16::from(displacement));
                        }
                    }
                    opcode => panic!("font-route test reached unsupported opcode {opcode:02X}"),
                }
            }
            RunResult {
                writes: cpu.writes,
                fallback: cpu.fallback,
                a: cpu.a,
                x: cpu.x,
                p: cpu.p,
            }
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
    }

    #[test]
    fn route_runtime_hands_directly_to_the_dialogue_selector_in_the_shared_cave() {
        let runtime = build_font_page_route_runtime().unwrap();
        assert_eq!(runtime.routine.address, ROUTE_CAVE_ORIGIN);
        assert_eq!(
            usize::from(runtime.routine.address) + runtime.routine.bytes.len(),
            usize::from(runtime.dialogue_selector)
        );
        assert!(runtime.dialogue_selector < chr_selector::SELECTOR_CAVE_END);
    }

    #[test]
    fn encoded_route_maps_fd_and_only_its_declared_fe_owner() {
        let runtime = build_font_page_route_runtime().unwrap();
        let memory = vec![0; 0x10000].into_boxed_slice().try_into().unwrap();
        let mirrored = TestCpu::run(&runtime, runtime.apply_route, memory, 0xA9, 0x37, 0x44);
        assert_eq!(mirrored.writes, [(2, 0xA8), (4, 0xA8)]);
        assert_eq!((mirrored.a, mirrored.x), (0xA9, 0x37));

        let memory = vec![0; 0x10000].into_boxed_slice().try_into().unwrap();
        let preserved = TestCpu::run(&runtime, runtime.apply_route, memory, 0xA8, 0x37, 0x44);
        assert_eq!(preserved.writes, [(2, 0xA8)]);
        assert_eq!((preserved.a, preserved.x), (0xA8, 0x37));
    }

    #[test]
    fn dialogue_projection_uses_the_live_source_fe_pair() {
        let runtime = build_font_page_route_runtime().unwrap();
        let mut memory: Box<[u8; 0x10000]> =
            vec![0; 0x10000].into_boxed_slice().try_into().unwrap();
        memory[usize::from(chr_source_state::RIGHT_FE_SOURCE_SHADOW)] = 0;
        memory[usize::from(chr_source_state::CHR_SOURCE_HIGH_BITS)] = 0;
        let mirrored = TestCpu::run(
            &runtime,
            runtime.project_dialogue_page,
            memory,
            0xC8,
            0x19,
            0x44,
        );
        assert_eq!(mirrored.writes, [(2, 0xC8), (4, 0xC8)]);

        let mut memory: Box<[u8; 0x10000]> =
            vec![0; 0x10000].into_boxed_slice().try_into().unwrap();
        memory[usize::from(chr_source_state::RIGHT_FE_SOURCE_SHADOW)] = 0x18;
        let preserved = TestCpu::run(
            &runtime,
            runtime.project_dialogue_page,
            memory,
            0xC8,
            0x19,
            0x44,
        );
        assert_eq!(preserved.writes, [(2, 0xC8)]);
    }

    #[test]
    fn active_consumer_route_survives_selector_refresh_and_empty_state_falls_through() {
        let runtime = build_font_page_route_runtime().unwrap();
        let mut memory: Box<[u8; 0x10000]> =
            vec![0; 0x10000].into_boxed_slice().try_into().unwrap();
        memory[usize::from(CONSUMER_FONT_PAGE)] = 0xD9;
        let active = TestCpu::run(
            &runtime,
            runtime.select_active_page,
            memory,
            0x42,
            0x37,
            0x45,
        );
        assert_eq!(active.writes, [(2, 0xD8), (4, 0xD8)]);
        assert_eq!((active.a, active.x, active.p), (0x42, 0x37, 0x45));
        assert_eq!(active.fallback, None);

        let memory = vec![0; 0x10000].into_boxed_slice().try_into().unwrap();
        let empty = TestCpu::run(
            &runtime,
            runtime.select_active_page,
            memory,
            0x42,
            0x37,
            0x45,
        );
        assert_eq!(empty.writes, []);
        assert_eq!(empty.fallback, Some(runtime.dialogue_selector));
        assert_eq!((empty.a, empty.x, empty.p), (0x42, 0x37, 0x45));
    }
}
