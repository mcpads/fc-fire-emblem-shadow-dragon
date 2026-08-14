//! 유닛 UI와 아이템 화면의 공유 원문 표를 카탈로그 전용 문자열로 우회한다.
//!
//! 전투 런타임은 아이템·병종·이름 원문 표를 자기 화면 코드북으로 사용한다. 그 표를
//! 다시 UI 코드로 덮으면 한쪽 화면은 반드시 깨진다. 이 모듈은 유닛 요약과 그 아이템
//! 흐름이 실제로 호출하는 세 `JSR`만 고정 뱅크 브리지로 보내고, 별도 PRG 페이지의
//! 카탈로그 코드를 `0x0451,X`에 직접 합성한다. 같은 함수를 쓰는 명단·별도 프로필과
//! 원문 표·전투 소비자는 그대로 둔다.

use anyhow::{Context, Result, ensure};

use super::{
    DialogueRuntimeHook, DialogueRuntimeHookRole, DialogueRuntimeHookSite, RuntimeRoutine,
    ensure_disjoint, next_address,
};
use crate::{
    dialogue_inventory::switchable_cpu_to_file_offset,
    full_translation_install::{
        consumer_catalog::ConsumerCatalogRuntimeLayout,
        runtime_state_storage::CONSUMER_CATALOG_PAGE,
    },
    rom::Rom,
    rp2a03::{Instruction, assemble_at},
};

const UNIT_UI_BANK: u8 = 0x0B;
const ENTRY_STUB_CAVE_END: u16 = 0xF807;
const FIXED_BRIDGE_ORIGIN: u16 = 0xFAF3;
const FIXED_BRIDGE_END: u16 = 0xFB20;

const PPU_CONTROL: u16 = 0x2000;
const PPU_CONTROL_SHADOW: u8 = 0xCD;
const NMI_ENABLE_MASK: u8 = 0x80;
const BANK_VALUE_REGISTER: u16 = 0x8001;
const PRG_8000_REGISTER: u8 = 6;
const PRG_A000_REGISTER: u8 = 7;
const PRG_BANK_SHADOW: u8 = 0x29;
const PAIRED_BANK_HELPER: u16 = 0xFA20;

const CURRENT_RECORD_POINTER: u8 = 0x74;
const CURRENT_ITEM_OFFSET: u8 = 0x12;
const CURRENT_UNIT_RECORD: u16 = 0x76F4;
const ENEMY_RECORD_FLAG: u8 = 0x80;
const COMPOSITE_BUFFER: u16 = 0x0451;
const STRING_TERMINATOR: u8 = 0xEF;
const SEGMENT_SEPARATOR: u8 = 0xED;

const ITEM_ENTRY_COUNT: u8 = 91;
const CLASS_ENTRY_COUNT: u8 = 22;
const UNIT_ENTRY_COUNT: u8 = 53;
const ENEMY_ENTRY_COUNT: u8 = 69;

const KIND_ITEM: u8 = 0;
const KIND_UNIT_OR_ENEMY: u8 = 1;
const KIND_CLASS: u8 = 2;
const KIND_COUNT: u8 = 3;

#[derive(Clone, Copy)]
struct HookSite {
    role: DialogueRuntimeHookRole,
    write_role: &'static str,
    address: u16,
    expected_call: [u8; 3],
    kind: u8,
}

const HOOK_SITES: [HookSite; 3] = [
    HookSite {
        role: DialogueRuntimeHookRole::ConsumerCatalogItemAppender,
        write_role: "consumer catalog item appender hook",
        address: 0x875F,
        expected_call: [0x20, 0x6B, 0x8E],
        kind: KIND_ITEM,
    },
    HookSite {
        role: DialogueRuntimeHookRole::ConsumerCatalogUnitAppender,
        write_role: "consumer catalog unit-or-enemy appender hook",
        address: 0x8284,
        expected_call: [0x20, 0x88, 0x8E],
        kind: KIND_UNIT_OR_ENEMY,
    },
    HookSite {
        role: DialogueRuntimeHookRole::ConsumerCatalogClassAppender,
        write_role: "consumer catalog class appender hook",
        address: 0x82A7,
        expected_call: [0x20, 0xBA, 0x8E],
        kind: KIND_CLASS,
    },
];

pub(super) struct ConsumerCatalogRuntime {
    pub(super) fixed_routines: Vec<RuntimeRoutine>,
    pub(super) code_routine: RuntimeRoutine,
    pub(super) hooks: Vec<DialogueRuntimeHook>,
}

pub(super) fn bind_consumer_catalog_sites(source: &Rom, candidate: &Rom) -> Result<()> {
    for site in HOOK_SITES {
        for (image_role, rom) in [("source", source), ("candidate", candidate)] {
            let offset = switchable_cpu_to_file_offset(UNIT_UI_BANK, site.address)?;
            ensure!(
                rom.data().get(offset..offset + site.expected_call.len())
                    == Some(site.expected_call.as_slice()),
                "{image_role} {} changed at {UNIT_UI_BANK:02X}:{:04X}",
                site.write_role,
                site.address
            );
        }
    }
    ensure!(
        fixed_bytes(
            candidate,
            FIXED_BRIDGE_ORIGIN,
            FIXED_BRIDGE_END - FIXED_BRIDGE_ORIGIN,
        )?
        .iter()
        .all(|byte| *byte == 0xFF),
        "consumer catalog fixed bridge cave is not exact FF"
    );
    Ok(())
}

pub(super) fn build_consumer_catalog_runtime(
    code_origin: u16,
    code_page: u8,
    entry_stub_origin: u16,
    layout: ConsumerCatalogRuntimeLayout,
) -> Result<ConsumerCatalogRuntime> {
    let code_routine = build_catalog_append_runtime(code_origin, layout)?;
    let mut next = entry_stub_origin;
    let mut fixed_routines = Vec::new();
    for site in HOOK_SITES {
        let routine = RuntimeRoutine {
            role: site.write_role,
            address: next,
            bytes: build_entry_stub(next, site.kind)?,
        };
        next = routine_end(&routine)?;
        fixed_routines.push(routine);
    }
    ensure_disjoint(
        &fixed_routines.iter().collect::<Vec<_>>(),
        ENTRY_STUB_CAVE_END,
    )?;
    fixed_routines.push(RuntimeRoutine {
        role: "consumer catalog runtime-page bridge",
        address: FIXED_BRIDGE_ORIGIN,
        bytes: build_fixed_bridge(FIXED_BRIDGE_ORIGIN, code_routine.address, code_page)?,
    });
    ensure!(
        routine_end(fixed_routines.last().expect("catalog bridge was appended"))?
            <= FIXED_BRIDGE_END,
        "consumer catalog bridge exceeds its fixed cave"
    );

    let hooks = HOOK_SITES
        .into_iter()
        .zip(fixed_routines.iter().take(HOOK_SITES.len()))
        .map(|(site, routine)| {
            Ok(DialogueRuntimeHook {
                role: site.role,
                write_role: site.write_role,
                site: DialogueRuntimeHookSite::Switchable {
                    bank: UNIT_UI_BANK,
                    address: site.address,
                },
                bytes: assemble_at(site.address, &[Instruction::JsrAbsolute(routine.address)])?,
            })
        })
        .collect::<Result<Vec<_>>>()?;

    Ok(ConsumerCatalogRuntime {
        fixed_routines,
        code_routine,
        hooks,
    })
}

/// 세 호출점 스텁은 kind만 다르고 정확히 다섯 바이트다. 기존 생산자 브리지 뒤의
/// 15바이트를 남김없이 사용하므로 어느 하나가 커지면 계획 단계에서 거부된다.
fn build_entry_stub(origin: u16, kind: u8) -> Result<Vec<u8>> {
    assemble_at(
        origin,
        &[
            Instruction::LdaImmediate(kind),
            Instruction::JmpAbsolute(FIXED_BRIDGE_ORIGIN),
        ],
    )
}

/// 원본 appender는 A·Y·플래그를 보존하지 않고 X만 출력 끝으로 전진시킨다. 브리지는
/// 그 관측 계약을 그대로 따른다. kind는 Y에, 출력 X는 스택에 잠시 두고 코드 페이지를
/// 매핑한다. X를 복원하면 PLA가 A를 덮으므로 TYA로 kind를 다시 전달한 뒤 appender를
/// 호출한다. 실행 뒤 갱신된 X만 mapper 복원 호출 너머로 보존하고 A는 원본처럼 ED다.
fn build_fixed_bridge(origin: u16, appender: u16, code_page: u8) -> Result<Vec<u8>> {
    assemble_at(
        origin,
        &[
            Instruction::Tay,
            Instruction::Txa,
            Instruction::Pha,
            Instruction::LdaZeroPage(PPU_CONTROL_SHADOW),
            Instruction::AndImmediate(!NMI_ENABLE_MASK),
            Instruction::StaAbsolute(PPU_CONTROL),
            Instruction::LdaImmediate(PRG_A000_REGISTER),
            crate::mapper165::selector_safety::select_register_instruction(),
            Instruction::LdaImmediate(code_page),
            Instruction::StaAbsolute(BANK_VALUE_REGISTER),
            Instruction::Tya,
            Instruction::Pla,
            Instruction::Tax,
            Instruction::Tya,
            Instruction::JsrAbsolute(appender),
            Instruction::Txa,
            Instruction::Pha,
            Instruction::LdaZeroPage(PRG_BANK_SHADOW),
            Instruction::JsrAbsolute(PAIRED_BANK_HELPER),
            Instruction::LdaZeroPage(PPU_CONTROL_SHADOW),
            Instruction::StaAbsolute(PPU_CONTROL),
            Instruction::Pla,
            Instruction::Tax,
            Instruction::LdaImmediate(SEGMENT_SEPARATOR),
            Instruction::Rts,
        ],
    )
}

fn build_catalog_append_runtime(
    origin: u16,
    layout: ConsumerCatalogRuntimeLayout,
) -> Result<RuntimeRoutine> {
    let mut instructions = vec![Instruction::Tay];
    for address in 0x00..=0x05 {
        instructions.extend([Instruction::LdaZeroPage(address), Instruction::Pha]);
    }
    instructions.extend([
        Instruction::Tya,
        Instruction::StaZeroPage(0x05),
        Instruction::CmpImmediate(KIND_COUNT),
    ]);
    let valid_kind = append_jump_if_carry_clear(origin, &mut instructions)?;
    let invalid_kind = push_jump(&mut instructions, origin);
    let valid_kind_target = next_address(origin, &instructions)?;
    patch_jump(&mut instructions, valid_kind, valid_kind_target);
    instructions.extend([
        Instruction::LdaZeroPage(0x05),
        Instruction::CmpImmediate(KIND_ITEM),
    ]);
    let item_kind = append_jump_if_equal(origin, &mut instructions)?;
    instructions.push(Instruction::CmpImmediate(KIND_UNIT_OR_ENEMY));
    let unit_kind = append_jump_if_equal(origin, &mut instructions)?;
    let class_kind = push_jump(&mut instructions, origin);

    let item = next_address(origin, &instructions)?;
    patch_jump(&mut instructions, item_kind, item);
    instructions.extend([
        Instruction::LdyZeroPage(CURRENT_ITEM_OFFSET),
        Instruction::LdaIndirectY(CURRENT_RECORD_POINTER),
        Instruction::CmpImmediate(1),
    ]);
    let item_minimum = append_jump_if_carry_set(origin, &mut instructions)?;
    let invalid_item_minimum = push_jump(&mut instructions, origin);
    let item_minimum_target = next_address(origin, &instructions)?;
    patch_jump(&mut instructions, item_minimum, item_minimum_target);
    instructions.push(Instruction::CmpImmediate(ITEM_ENTRY_COUNT + 1));
    let item_bounded = append_jump_if_carry_clear(origin, &mut instructions)?;
    let invalid_item_maximum = push_jump(&mut instructions, origin);
    let item_bounded_target = next_address(origin, &instructions)?;
    patch_jump(&mut instructions, item_bounded, item_bounded_target);
    instructions.extend([
        Instruction::Sec,
        Instruction::SbcImmediate(1),
        Instruction::StaZeroPage(0x04),
    ]);
    set_pointer(&mut instructions, layout.item_directory);
    let directory_ready_from_item = push_jump(&mut instructions, origin);

    let unit = next_address(origin, &instructions)?;
    patch_jump(&mut instructions, unit_kind, unit);
    instructions.extend([
        Instruction::LdyImmediate(0),
        Instruction::LdaIndirectY(CURRENT_RECORD_POINTER),
        Instruction::AndImmediate(0x7F),
        Instruction::CmpImmediate(1),
    ]);
    let unit_minimum = append_jump_if_carry_set(origin, &mut instructions)?;
    let invalid_unit_minimum = push_jump(&mut instructions, origin);
    let unit_minimum_target = next_address(origin, &instructions)?;
    patch_jump(&mut instructions, unit_minimum, unit_minimum_target);
    instructions.extend([
        Instruction::Sec,
        Instruction::SbcImmediate(1),
        Instruction::StaZeroPage(0x04),
        Instruction::LdaAbsolute(CURRENT_UNIT_RECORD),
        Instruction::AndImmediate(ENEMY_RECORD_FLAG),
    ]);
    let enemy = append_jump_if_not_equal(origin, &mut instructions)?;
    instructions.extend([
        Instruction::LdaZeroPage(0x04),
        Instruction::CmpImmediate(UNIT_ENTRY_COUNT),
    ]);
    let unit_bounded = append_jump_if_carry_clear(origin, &mut instructions)?;
    let invalid_unit_maximum = push_jump(&mut instructions, origin);
    let unit_bounded_target = next_address(origin, &instructions)?;
    patch_jump(&mut instructions, unit_bounded, unit_bounded_target);
    set_pointer(&mut instructions, layout.unit_directory);
    let directory_ready_from_unit = push_jump(&mut instructions, origin);

    let enemy_target = next_address(origin, &instructions)?;
    patch_jump(&mut instructions, enemy, enemy_target);
    instructions.extend([
        Instruction::LdaZeroPage(0x04),
        Instruction::CmpImmediate(ENEMY_ENTRY_COUNT),
    ]);
    let enemy_bounded = append_jump_if_carry_clear(origin, &mut instructions)?;
    let invalid_enemy_maximum = push_jump(&mut instructions, origin);
    let enemy_bounded_target = next_address(origin, &instructions)?;
    patch_jump(&mut instructions, enemy_bounded, enemy_bounded_target);
    set_pointer(&mut instructions, layout.enemy_directory);
    let directory_ready_from_enemy = push_jump(&mut instructions, origin);

    let class = next_address(origin, &instructions)?;
    patch_jump(&mut instructions, class_kind, class);
    instructions.extend([
        Instruction::LdyImmediate(1),
        Instruction::LdaIndirectY(CURRENT_RECORD_POINTER),
        Instruction::CmpImmediate(1),
    ]);
    let class_minimum = append_jump_if_carry_set(origin, &mut instructions)?;
    let invalid_class_minimum = push_jump(&mut instructions, origin);
    let class_minimum_target = next_address(origin, &instructions)?;
    patch_jump(&mut instructions, class_minimum, class_minimum_target);
    instructions.push(Instruction::CmpImmediate(CLASS_ENTRY_COUNT + 1));
    let class_bounded = append_jump_if_carry_clear(origin, &mut instructions)?;
    let invalid_class_maximum = push_jump(&mut instructions, origin);
    let class_bounded_target = next_address(origin, &instructions)?;
    patch_jump(&mut instructions, class_bounded, class_bounded_target);
    instructions.extend([
        Instruction::Sec,
        Instruction::SbcImmediate(1),
        Instruction::StaZeroPage(0x04),
    ]);
    set_pointer(&mut instructions, layout.class_directory);

    let directory_ready = next_address(origin, &instructions)?;
    for jump in [
        directory_ready_from_item,
        directory_ready_from_unit,
        directory_ready_from_enemy,
    ] {
        patch_jump(&mut instructions, jump, directory_ready);
    }
    instructions.extend([
        Instruction::LdaZeroPage(0x04),
        Instruction::AslAccumulator,
        Instruction::Clc,
        Instruction::AdcZeroPage(0x00),
        Instruction::StaZeroPage(0x00),
        Instruction::LdaZeroPage(0x01),
        Instruction::AdcImmediate(0),
        Instruction::StaZeroPage(0x01),
        Instruction::LdaImmediate(PRG_8000_REGISTER),
        crate::mapper165::selector_safety::select_register_instruction(),
        Instruction::LdaImmediate(layout.material_page),
        Instruction::StaAbsolute(BANK_VALUE_REGISTER),
        Instruction::LdyImmediate(0),
        Instruction::LdaIndirectY(0x00),
        Instruction::StaZeroPage(0x04),
        Instruction::Iny,
        Instruction::LdaIndirectY(0x00),
        Instruction::StaZeroPage(0x01),
        Instruction::Clc,
        Instruction::LdaZeroPage(0x04),
        Instruction::AdcImmediate(layout.material_base as u8),
        Instruction::StaZeroPage(0x00),
        Instruction::LdaZeroPage(0x01),
        Instruction::AdcImmediate((layout.material_base >> 8) as u8),
        Instruction::StaZeroPage(0x01),
        Instruction::LdyImmediate(0),
        Instruction::LdaZeroPage(0x05),
        Instruction::CmpImmediate(KIND_UNIT_OR_ENEMY),
    ]);
    let name_prefix = append_jump_if_equal(origin, &mut instructions)?;
    let copy_without_prefix = push_jump(&mut instructions, origin);
    let name_prefix_target = next_address(origin, &instructions)?;
    patch_jump(&mut instructions, name_prefix, name_prefix_target);
    instructions.extend([
        Instruction::LdaIndirectY(0x00),
        Instruction::StaAbsolute(CONSUMER_CATALOG_PAGE),
        Instruction::Iny,
    ]);
    let copy_loop = next_address(origin, &instructions)?;
    patch_jump(&mut instructions, copy_without_prefix, copy_loop);
    instructions.extend([
        Instruction::LdaIndirectY(0x00),
        Instruction::CmpImmediate(STRING_TERMINATOR),
    ]);
    let copy_finished = append_jump_if_equal(origin, &mut instructions)?;
    instructions.extend([
        Instruction::StaAbsoluteX(COMPOSITE_BUFFER),
        Instruction::Inx,
        Instruction::Iny,
        Instruction::JmpAbsolute(copy_loop),
    ]);
    let copy_finished_target = next_address(origin, &instructions)?;
    patch_jump(&mut instructions, copy_finished, copy_finished_target);
    instructions.extend([
        Instruction::LdaImmediate(SEGMENT_SEPARATOR),
        Instruction::StaAbsoluteX(COMPOSITE_BUFFER),
        Instruction::Inx,
        Instruction::LdaImmediate(PRG_8000_REGISTER),
        crate::mapper165::selector_safety::select_register_instruction(),
        Instruction::LdaZeroPage(PRG_BANK_SHADOW),
        Instruction::AndImmediate(0x0F),
        Instruction::AslAccumulator,
        Instruction::StaAbsolute(BANK_VALUE_REGISTER),
    ]);

    let cleanup = next_address(origin, &instructions)?;
    for jump in [
        invalid_kind,
        invalid_item_minimum,
        invalid_item_maximum,
        invalid_unit_minimum,
        invalid_unit_maximum,
        invalid_enemy_maximum,
        invalid_class_minimum,
        invalid_class_maximum,
    ] {
        patch_jump(&mut instructions, jump, cleanup);
    }
    for address in (0x00..=0x05).rev() {
        instructions.extend([Instruction::Pla, Instruction::StaZeroPage(address)]);
    }
    instructions.push(Instruction::Rts);

    Ok(RuntimeRoutine {
        role: "consumer catalog indexed string appender",
        address: origin,
        bytes: assemble_at(origin, &instructions)?,
    })
}

fn set_pointer(instructions: &mut Vec<Instruction>, address: u16) {
    instructions.extend([
        Instruction::LdaImmediate(address as u8),
        Instruction::StaZeroPage(0x00),
        Instruction::LdaImmediate((address >> 8) as u8),
        Instruction::StaZeroPage(0x01),
    ]);
}

fn push_jump(instructions: &mut Vec<Instruction>, placeholder: u16) -> usize {
    let index = instructions.len();
    instructions.push(Instruction::JmpAbsolute(placeholder));
    index
}

fn patch_jump(instructions: &mut [Instruction], index: usize, target: u16) {
    instructions[index] = Instruction::JmpAbsolute(target);
}

fn append_jump_if_equal(origin: u16, instructions: &mut Vec<Instruction>) -> Result<usize> {
    append_conditional_jump(origin, instructions, Instruction::BneAbsolute)
}

fn append_jump_if_not_equal(origin: u16, instructions: &mut Vec<Instruction>) -> Result<usize> {
    append_conditional_jump(origin, instructions, Instruction::BeqAbsolute)
}

fn append_jump_if_carry_clear(origin: u16, instructions: &mut Vec<Instruction>) -> Result<usize> {
    append_conditional_jump(origin, instructions, Instruction::BcsAbsolute)
}

fn append_jump_if_carry_set(origin: u16, instructions: &mut Vec<Instruction>) -> Result<usize> {
    append_conditional_jump(origin, instructions, Instruction::BccAbsolute)
}

fn append_conditional_jump(
    origin: u16,
    instructions: &mut Vec<Instruction>,
    inverse: fn(u16) -> Instruction,
) -> Result<usize> {
    let branch_address = next_address(origin, instructions)?;
    let after = branch_address
        .checked_add(5)
        .context("consumer catalog conditional jump address overflow")?;
    instructions.push(inverse(after));
    Ok(push_jump(instructions, origin))
}

fn routine_end(routine: &RuntimeRoutine) -> Result<u16> {
    u16::try_from(usize::from(routine.address) + routine.bytes.len())
        .context("consumer catalog routine address overflow")
}

fn fixed_bytes(rom: &Rom, start: u16, length: u16) -> Result<&[u8]> {
    let base = rom
        .prg()
        .len()
        .checked_sub(16 * 1024)
        .context("candidate PRG has no fixed bank")?;
    let offset = base + usize::from(start - 0xC000);
    rom.prg()
        .get(offset..offset + usize::from(length))
        .context("consumer catalog fixed bridge is outside candidate")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn layout() -> ConsumerCatalogRuntimeLayout {
        ConsumerCatalogRuntimeLayout {
            material_page: 0x32,
            material_base: 0x8000,
            item_directory: 0x8010,
            class_directory: 0x80C6,
            unit_directory: 0x80F2,
            enemy_directory: 0x815C,
        }
    }

    #[test]
    fn three_five_byte_stubs_fill_the_remaining_producer_cave() {
        let runtime = build_consumer_catalog_runtime(0xA600, 0x30, 0xF7F8, layout()).unwrap();

        assert_eq!(runtime.fixed_routines.len(), 4);
        assert!(
            runtime.fixed_routines[..3]
                .iter()
                .all(|routine| routine.bytes.len() == 5)
        );
        assert_eq!(
            routine_end(&runtime.fixed_routines[2]).unwrap(),
            ENTRY_STUB_CAVE_END
        );
    }

    #[test]
    fn fixed_bridge_fits_the_independent_forty_five_byte_cave() {
        let bytes = build_fixed_bridge(FIXED_BRIDGE_ORIGIN, 0xA600, 0x30).unwrap();

        assert!(usize::from(FIXED_BRIDGE_ORIGIN) + bytes.len() <= usize::from(FIXED_BRIDGE_END));
        assert_eq!(bytes.last(), Some(&0x60));
    }

    #[test]
    fn fixed_bridge_restores_the_catalog_kind_after_restoring_output_x() {
        let bytes = build_fixed_bridge(FIXED_BRIDGE_ORIGIN, 0xA600, 0x30).unwrap();

        assert!(
            bytes
                .windows(6)
                .any(|window| { window == [0x98, 0x68, 0xAA, 0x98, 0x20, 0x00] })
        );
        assert_eq!(
            bytes.len(),
            usize::from(FIXED_BRIDGE_END - FIXED_BRIDGE_ORIGIN) - 1
        );
    }

    #[test]
    fn all_catalog_hooks_are_typed_three_byte_calls() {
        let runtime = build_consumer_catalog_runtime(0xA600, 0x30, 0xF7F8, layout()).unwrap();

        assert_eq!(runtime.hooks.len(), HOOK_SITES.len());
        assert!(runtime.hooks.iter().all(|hook| hook.bytes.len() == 3));
    }
}
