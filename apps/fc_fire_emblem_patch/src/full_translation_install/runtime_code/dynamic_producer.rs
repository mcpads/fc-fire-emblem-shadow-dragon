//! `{EC}` RAM 슬롯에 전용 정규 코드 문자열을 공급한다.
//!
//! 원본 생산자는 전투·메뉴와 공유하는 표의 바이트를 그대로 복사한다. 그 표는 화면별
//! 색칠 코드북이라 같은 바이트가 여러 글리프를 뜻할 수 있다. 이 모듈은 원본 표를
//! 바꾸지 않고 다섯 생산자만 전용 injective 저장소로 돌린다.
//!
//! 고정 뱅크는 다섯 진입 스텁과 매핑 브리지만 둔다. 실제 정규화기는 대사 런타임의
//! 전용 `$A000` 코드 페이지에 둔다. 생산자별로 상태 저장과 복사를 복제하면 고정
//! 동굴을 넘고, CHR selector 동굴과 겹치므로 실행 코드 페이지가 이 역할의 소유자다.

use anyhow::{Context, Result, ensure};

use super::{
    DialogueRuntimeHook, DialogueRuntimeHookRole, DialogueRuntimeHookSite, RuntimeRoutine,
    ensure_disjoint, next_address, resolve_request::MaterialLayout,
};
use crate::{
    dialogue_inventory::switchable_cpu_to_file_offset,
    rom::Rom,
    rp2a03::{Instruction, assemble_at},
};

const FIXED_BRIDGE_ORIGIN: u16 = 0xF79A;
const FIXED_BRIDGE_END: u16 = 0xF807;
const PPU_CONTROL: u16 = 0x2000;
const PPU_CONTROL_SHADOW: u8 = 0xCD;
const NMI_ENABLE_MASK: u8 = 0x80;
const BANK_VALUE_REGISTER: u16 = 0x8001;
const PRG_8000_REGISTER: u8 = 6;
const PRG_A000_REGISTER: u8 = 7;
const PRG_BANK_SHADOW: u8 = 0x29;
const PAIRED_BANK_HELPER: u16 = 0xFA20;
const STRING_TERMINATOR: u8 = 0xEF;
const SLOT_ZERO: u16 = 0x78F2;
const EPILOGUE_LOCATION_SLOT: u16 = 0x7902;
const EPILOGUE_UNIT_INDEX: u16 = 0x773B;
const ITEM_ENTRY_COUNT: u8 = 91;
const UNIT_ENTRY_COUNT: u8 = 53;
const LOCATION_ENTRY_COUNT: u8 = 24;

const KIND_GENERIC_ITEM: u8 = 0;
const KIND_GENERIC_UNIT: u8 = 1;
const KIND_VILLAGE_ITEM: u8 = 2;
const KIND_EPILOGUE_UNIT: u8 = 3;
const KIND_EPILOGUE_LOCATION: u8 = 4;
const KIND_COUNT: u8 = 5;

#[derive(Clone, Copy)]
struct HookSite {
    role: DialogueRuntimeHookRole,
    write_role: &'static str,
    bank: u8,
    address: u16,
    expected: [u8; 3],
    kind: u8,
}

const HOOK_SITES: [HookSite; 5] = [
    HookSite {
        role: DialogueRuntimeHookRole::DynamicItemSlotProducer,
        write_role: "dynamic item-slot canonical producer hook",
        bank: 0x06,
        address: 0x9AEC,
        expected: [0x48, 0xA9, 0xF2],
        kind: KIND_GENERIC_ITEM,
    },
    HookSite {
        role: DialogueRuntimeHookRole::DynamicUnitSlotProducer,
        write_role: "dynamic unit-slot canonical producer hook",
        bank: 0x06,
        address: 0x9B1D,
        expected: [0x48, 0xA9, 0xF2],
        kind: KIND_GENERIC_UNIT,
    },
    HookSite {
        role: DialogueRuntimeHookRole::DynamicVillageItemProducer,
        write_role: "dynamic village-item canonical producer hook",
        bank: 0x03,
        address: 0x9C50,
        expected: [0x8A, 0x0A, 0xA8],
        kind: KIND_VILLAGE_ITEM,
    },
    HookSite {
        role: DialogueRuntimeHookRole::DynamicEpilogueUnitProducer,
        write_role: "dynamic epilogue-unit canonical producer hook",
        bank: 0x04,
        address: 0xA366,
        expected: [0xAD, 0x3B, 0x77],
        kind: KIND_EPILOGUE_UNIT,
    },
    HookSite {
        role: DialogueRuntimeHookRole::DynamicEpilogueLocationProducer,
        write_role: "dynamic epilogue-location canonical producer hook",
        bank: 0x04,
        address: 0xA1CA,
        expected: [0xA0, 0x00, 0xB1],
        kind: KIND_EPILOGUE_LOCATION,
    },
];

pub(super) struct DynamicProducerRuntime {
    pub(super) fixed_routines: Vec<RuntimeRoutine>,
    pub(super) code_routines: Vec<RuntimeRoutine>,
    pub(super) hooks: Vec<DialogueRuntimeHook>,
}

pub(super) fn bind_hook_sites(source: &Rom, candidate: &Rom) -> Result<()> {
    for site in HOOK_SITES {
        for (image_role, rom) in [("source", source), ("candidate", candidate)] {
            let offset = switchable_cpu_to_file_offset(site.bank, site.address)?;
            ensure!(
                rom.data().get(offset..offset + site.expected.len())
                    == Some(site.expected.as_slice()),
                "{image_role} {} changed at {:02X}:{:04X}",
                site.write_role,
                site.bank,
                site.address
            );
        }
    }
    let fixed = fixed_bytes(
        candidate,
        FIXED_BRIDGE_ORIGIN,
        FIXED_BRIDGE_END - FIXED_BRIDGE_ORIGIN,
    )?;
    ensure!(
        fixed.iter().all(|byte| *byte == 0xFF),
        "dynamic producer fixed bridge is not exact FF"
    );
    Ok(())
}

pub(super) fn build_dynamic_producer_runtime(
    code_origin: u16,
    code_page: u8,
    layout: MaterialLayout,
) -> Result<DynamicProducerRuntime> {
    let code_routine = build_canonical_copy_runtime(code_origin, layout)?;
    let mut next = FIXED_BRIDGE_ORIGIN;
    let mut fixed_routines = Vec::new();
    for site in HOOK_SITES {
        let bytes = build_entry_stub(next, FIXED_BRIDGE_END, site.kind)?;
        fixed_routines.push(RuntimeRoutine {
            role: site.write_role,
            address: next,
            bytes,
        });
        next = routine_end(fixed_routines.last().expect("producer stub was appended"))?;
    }
    let bridge_address = next;
    for (routine, site) in fixed_routines.iter_mut().zip(HOOK_SITES) {
        routine.bytes = build_entry_stub(routine.address, bridge_address, site.kind)?;
    }
    fixed_routines.push(RuntimeRoutine {
        role: "dynamic producer runtime-page bridge",
        address: bridge_address,
        bytes: build_fixed_bridge(bridge_address, code_routine.address, code_page)?,
    });
    ensure_disjoint(&fixed_routines.iter().collect::<Vec<_>>(), FIXED_BRIDGE_END)?;

    let hooks = HOOK_SITES
        .into_iter()
        .zip(fixed_routines.iter().take(HOOK_SITES.len()))
        .map(|(site, routine)| {
            Ok(DialogueRuntimeHook {
                role: site.role,
                write_role: site.write_role,
                site: DialogueRuntimeHookSite::Switchable {
                    bank: site.bank,
                    address: site.address,
                },
                bytes: assemble_at(site.address, &[Instruction::JmpAbsolute(routine.address)])?,
            })
        })
        .collect::<Result<Vec<_>>>()?;

    Ok(DynamicProducerRuntime {
        fixed_routines,
        code_routines: vec![code_routine],
        hooks,
    })
}

/// 각 원본 writer는 `JSR`로 불리고 자체 `RTS`로 끝난다. 세 바이트 훅은 writer의
/// 나머지를 건너뛰므로 스텁도 호출자에게 직접 돌아간다. 원본 A/X/Y/P는 스택에 두고
/// kind만 브리지로 운반한다.
fn build_entry_stub(origin: u16, bridge: u16, kind: u8) -> Result<Vec<u8>> {
    assemble_at(
        origin,
        &[
            Instruction::Php,
            Instruction::Pha,
            Instruction::Txa,
            Instruction::Pha,
            Instruction::Tya,
            Instruction::Pha,
            Instruction::LdaImmediate(kind),
            Instruction::JmpAbsolute(bridge),
        ],
    )
}

/// 매핑 중 NMI가 `$29`를 보고 두 PRG 창을 되돌리면 `$A000` 코드가 사라진다. 그래서
/// 고정 뱅크에서 먼저 하드웨어 NMI만 끄고, 실행 코드가 돌아온 뒤 두 창과 PPUCTRL을
/// 원래 그림자로 복구한다.
fn build_fixed_bridge(origin: u16, canonical_copy: u16, code_page: u8) -> Result<Vec<u8>> {
    assemble_at(
        origin,
        &[
            Instruction::Tax,
            Instruction::LdaZeroPage(PPU_CONTROL_SHADOW),
            Instruction::AndImmediate(!NMI_ENABLE_MASK),
            Instruction::StaAbsolute(PPU_CONTROL),
            Instruction::LdaImmediate(PRG_A000_REGISTER),
            crate::mapper165::selector_safety::select_register_instruction(),
            Instruction::LdaImmediate(code_page),
            Instruction::StaAbsolute(BANK_VALUE_REGISTER),
            Instruction::Txa,
            Instruction::JsrAbsolute(canonical_copy),
            Instruction::LdaZeroPage(PRG_BANK_SHADOW),
            Instruction::JsrAbsolute(PAIRED_BANK_HELPER),
            Instruction::LdaZeroPage(PPU_CONTROL_SHADOW),
            Instruction::StaAbsolute(PPU_CONTROL),
            Instruction::Pla,
            Instruction::Tay,
            Instruction::Pla,
            Instruction::Tax,
            Instruction::Pla,
            Instruction::Plp,
            Instruction::Rts,
        ],
    )
}

/// 진입 A는 생산자 kind다. 원본 레지스터는 고정 스텁 아래에 있고 JSR 반환 주소가
/// 두 바이트 더 쌓인다. kind와 ZP 0..5를 추가로 저장한 뒤의 TSX 기준으로 원본
/// Y/X/A는 각각 `+10/+11/+12`다.
fn build_canonical_copy_runtime(origin: u16, layout: MaterialLayout) -> Result<RuntimeRoutine> {
    let mut instructions = vec![Instruction::Pha];
    for address in 0x00..=0x05 {
        instructions.extend([Instruction::LdaZeroPage(address), Instruction::Pha]);
    }
    instructions.extend([
        Instruction::Tsx,
        Instruction::LdaAbsoluteX(0x0107),
        Instruction::StaZeroPage(0x05),
        Instruction::CmpImmediate(KIND_COUNT),
    ]);
    let valid_kind_jump = append_jump_if_carry_clear(origin, &mut instructions)?;
    let invalid_kind_jump = push_jump(&mut instructions, origin);

    let valid_kind = next_address(origin, &instructions)?;
    patch_jump(&mut instructions, valid_kind_jump, valid_kind);
    instructions.extend([
        Instruction::LdaZeroPage(0x05),
        Instruction::CmpImmediate(KIND_VILLAGE_ITEM),
    ]);
    let generic_jump = append_jump_if_carry_clear(origin, &mut instructions)?;
    instructions.push(Instruction::CmpImmediate(KIND_EPILOGUE_UNIT));
    let epilogue_unit_jump = append_jump_if_equal(origin, &mut instructions)?;

    // Village item and epilogue location both arrive with a zero-based source X.
    instructions.extend([
        Instruction::LdaAbsoluteX(0x010B),
        Instruction::StaZeroPage(0x04),
        Instruction::LdaZeroPage(0x05),
        Instruction::CmpImmediate(KIND_EPILOGUE_LOCATION),
    ]);
    let location_destination_jump = append_jump_if_equal(origin, &mut instructions)?;
    instructions.push(Instruction::LdaZeroPage(0x04));
    instructions.push(Instruction::CmpImmediate(ITEM_ENTRY_COUNT));
    let village_index_ok_jump = append_jump_if_carry_clear(origin, &mut instructions)?;
    let invalid_village_jump = push_jump(&mut instructions, origin);
    let village_index_ok = next_address(origin, &instructions)?;
    patch_jump(&mut instructions, village_index_ok_jump, village_index_ok);
    set_destination(&mut instructions, SLOT_ZERO);
    let selected_after_village = push_jump(&mut instructions, origin);

    let location_destination = next_address(origin, &instructions)?;
    patch_jump(
        &mut instructions,
        location_destination_jump,
        location_destination,
    );
    instructions.push(Instruction::LdaZeroPage(0x04));
    instructions.push(Instruction::CmpImmediate(LOCATION_ENTRY_COUNT));
    let location_index_ok_jump = append_jump_if_carry_clear(origin, &mut instructions)?;
    let invalid_location_jump = push_jump(&mut instructions, origin);
    let location_index_ok = next_address(origin, &instructions)?;
    patch_jump(&mut instructions, location_index_ok_jump, location_index_ok);
    set_destination(&mut instructions, EPILOGUE_LOCATION_SLOT);
    let selected_after_location = push_jump(&mut instructions, origin);

    let epilogue_unit = next_address(origin, &instructions)?;
    patch_jump(&mut instructions, epilogue_unit_jump, epilogue_unit);
    instructions.extend([
        Instruction::LdaAbsolute(EPILOGUE_UNIT_INDEX),
        Instruction::CmpImmediate(UNIT_ENTRY_COUNT),
    ]);
    let epilogue_unit_ok_jump = append_jump_if_carry_clear(origin, &mut instructions)?;
    let invalid_epilogue_unit_jump = push_jump(&mut instructions, origin);
    let epilogue_unit_ok = next_address(origin, &instructions)?;
    patch_jump(&mut instructions, epilogue_unit_ok_jump, epilogue_unit_ok);
    instructions.push(Instruction::StaZeroPage(0x04));
    set_destination(&mut instructions, SLOT_ZERO);
    let selected_after_epilogue_unit = push_jump(&mut instructions, origin);

    let generic = next_address(origin, &instructions)?;
    patch_jump(&mut instructions, generic_jump, generic);
    instructions.extend([
        Instruction::LdaAbsoluteX(0x010C),
        Instruction::CmpImmediate(1),
    ]);
    let generic_has_minimum_jump = append_jump_if_carry_set(origin, &mut instructions)?;
    let invalid_generic_minimum_jump = push_jump(&mut instructions, origin);
    let generic_has_minimum = next_address(origin, &instructions)?;
    patch_jump(
        &mut instructions,
        generic_has_minimum_jump,
        generic_has_minimum,
    );
    instructions.push(Instruction::LdaZeroPage(0x05));
    instructions.push(Instruction::CmpImmediate(KIND_GENERIC_ITEM));
    let generic_item_jump = append_jump_if_equal(origin, &mut instructions)?;

    instructions.extend([
        Instruction::LdaAbsoluteX(0x010C),
        Instruction::CmpImmediate(UNIT_ENTRY_COUNT + 1),
    ]);
    let generic_unit_ok_jump = append_jump_if_carry_clear(origin, &mut instructions)?;
    let invalid_generic_unit_jump = push_jump(&mut instructions, origin);
    let generic_unit_ok = next_address(origin, &instructions)?;
    patch_jump(&mut instructions, generic_unit_ok_jump, generic_unit_ok);
    let generic_bounds_done = push_jump(&mut instructions, origin);

    let generic_item = next_address(origin, &instructions)?;
    patch_jump(&mut instructions, generic_item_jump, generic_item);
    instructions.extend([
        Instruction::LdaAbsoluteX(0x010C),
        Instruction::CmpImmediate(ITEM_ENTRY_COUNT + 1),
    ]);
    let generic_item_ok_jump = append_jump_if_carry_clear(origin, &mut instructions)?;
    let invalid_generic_item_jump = push_jump(&mut instructions, origin);
    let generic_item_ok = next_address(origin, &instructions)?;
    patch_jump(&mut instructions, generic_item_ok_jump, generic_item_ok);

    let encode_generic = next_address(origin, &instructions)?;
    patch_jump(&mut instructions, generic_bounds_done, encode_generic);
    instructions.extend([
        Instruction::LdaAbsoluteX(0x010C),
        Instruction::Sec,
        Instruction::SbcImmediate(1),
        Instruction::StaZeroPage(0x04),
        Instruction::LdaAbsoluteX(0x010A),
        Instruction::AslAccumulator,
        Instruction::AslAccumulator,
        Instruction::AslAccumulator,
        Instruction::AslAccumulator,
        Instruction::Clc,
        Instruction::AdcImmediate(SLOT_ZERO as u8),
        Instruction::StaZeroPage(0x02),
        Instruction::LdaImmediate((SLOT_ZERO >> 8) as u8),
        Instruction::AdcImmediate(0),
        Instruction::StaZeroPage(0x03),
    ]);
    let selected_after_generic = push_jump(&mut instructions, origin);

    let selected = next_address(origin, &instructions)?;
    for jump in [
        selected_after_village,
        selected_after_location,
        selected_after_epilogue_unit,
        selected_after_generic,
    ] {
        patch_jump(&mut instructions, jump, selected);
    }
    instructions.extend([
        Instruction::LdaZeroPage(0x05),
        Instruction::CmpImmediate(KIND_GENERIC_ITEM),
    ]);
    let item_directory_jump_a = append_jump_if_equal(origin, &mut instructions)?;
    instructions.push(Instruction::CmpImmediate(KIND_VILLAGE_ITEM));
    let item_directory_jump_b = append_jump_if_equal(origin, &mut instructions)?;
    instructions.push(Instruction::CmpImmediate(KIND_GENERIC_UNIT));
    let unit_directory_jump_a = append_jump_if_equal(origin, &mut instructions)?;
    instructions.push(Instruction::CmpImmediate(KIND_EPILOGUE_UNIT));
    let unit_directory_jump_b = append_jump_if_equal(origin, &mut instructions)?;
    set_pointer(&mut instructions, layout.producer_location_directory);
    let directory_ready_from_location = push_jump(&mut instructions, origin);

    let item_directory = next_address(origin, &instructions)?;
    patch_jump(&mut instructions, item_directory_jump_a, item_directory);
    patch_jump(&mut instructions, item_directory_jump_b, item_directory);
    set_pointer(&mut instructions, layout.producer_item_directory);
    let directory_ready_from_item = push_jump(&mut instructions, origin);

    let unit_directory = next_address(origin, &instructions)?;
    patch_jump(&mut instructions, unit_directory_jump_a, unit_directory);
    patch_jump(&mut instructions, unit_directory_jump_b, unit_directory);
    set_pointer(&mut instructions, layout.producer_unit_directory);

    let directory_ready = next_address(origin, &instructions)?;
    patch_jump(
        &mut instructions,
        directory_ready_from_location,
        directory_ready,
    );
    patch_jump(
        &mut instructions,
        directory_ready_from_item,
        directory_ready,
    );
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
        Instruction::LdaImmediate(layout.producer_encoding_page),
        Instruction::StaAbsolute(BANK_VALUE_REGISTER),
        Instruction::LdyImmediate(0),
        Instruction::LdaIndirectY(0x00),
        Instruction::StaZeroPage(0x04),
        Instruction::Iny,
        Instruction::LdaIndirectY(0x00),
        Instruction::StaZeroPage(0x05),
        Instruction::Clc,
        Instruction::LdaZeroPage(0x04),
        Instruction::AdcImmediate(layout.producer_encoding_base as u8),
        Instruction::StaZeroPage(0x00),
        Instruction::LdaZeroPage(0x05),
        Instruction::AdcImmediate((layout.producer_encoding_base >> 8) as u8),
        Instruction::StaZeroPage(0x01),
        Instruction::LdyImmediate(0),
    ]);
    let copy_loop = next_address(origin, &instructions)?;
    instructions.extend([
        Instruction::LdaIndirectY(0x00),
        Instruction::StaIndirectY(0x02),
        Instruction::Iny,
        Instruction::CmpImmediate(STRING_TERMINATOR),
        Instruction::BneAbsolute(copy_loop),
        // `$A000`에서 실행 중이므로 `$8000`만 먼저 되돌린다. 고정 브리지로
        // 돌아간 뒤 `$FA20`이 두 창을 함께 원복한다.
        Instruction::LdaImmediate(PRG_8000_REGISTER),
        crate::mapper165::selector_safety::select_register_instruction(),
        Instruction::LdaZeroPage(PRG_BANK_SHADOW),
        Instruction::AndImmediate(0x0F),
        Instruction::AslAccumulator,
        Instruction::StaAbsolute(BANK_VALUE_REGISTER),
    ]);

    let cleanup = next_address(origin, &instructions)?;
    for jump in [
        invalid_kind_jump,
        invalid_village_jump,
        invalid_location_jump,
        invalid_epilogue_unit_jump,
        invalid_generic_minimum_jump,
        invalid_generic_unit_jump,
        invalid_generic_item_jump,
    ] {
        patch_jump(&mut instructions, jump, cleanup);
    }
    for address in (0x00..=0x05).rev() {
        instructions.extend([Instruction::Pla, Instruction::StaZeroPage(address)]);
    }
    instructions.extend([Instruction::Pla, Instruction::Rts]);

    Ok(RuntimeRoutine {
        role: "dynamic canonical string lookup and copy",
        address: origin,
        bytes: assemble_at(origin, &instructions)?,
    })
}

fn set_destination(instructions: &mut Vec<Instruction>, address: u16) {
    instructions.extend([
        Instruction::LdaImmediate(address as u8),
        Instruction::StaZeroPage(0x02),
        Instruction::LdaImmediate((address >> 8) as u8),
        Instruction::StaZeroPage(0x03),
    ]);
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
    let branch_address = next_address(origin, instructions)?;
    let after = branch_address
        .checked_add(5)
        .context("dynamic producer conditional jump address overflow")?;
    instructions.push(Instruction::BneAbsolute(after));
    let jump = push_jump(instructions, origin);
    Ok(jump)
}

fn append_jump_if_carry_clear(origin: u16, instructions: &mut Vec<Instruction>) -> Result<usize> {
    let branch_address = next_address(origin, instructions)?;
    let after = branch_address
        .checked_add(5)
        .context("dynamic producer conditional jump address overflow")?;
    instructions.push(Instruction::BcsAbsolute(after));
    let jump = push_jump(instructions, origin);
    Ok(jump)
}

fn append_jump_if_carry_set(origin: u16, instructions: &mut Vec<Instruction>) -> Result<usize> {
    let branch_address = next_address(origin, instructions)?;
    let after = branch_address
        .checked_add(5)
        .context("dynamic producer conditional jump address overflow")?;
    instructions.push(Instruction::BccAbsolute(after));
    let jump = push_jump(instructions, origin);
    Ok(jump)
}

fn routine_end(routine: &RuntimeRoutine) -> Result<u16> {
    u16::try_from(usize::from(routine.address) + routine.bytes.len())
        .context("dynamic producer routine address overflow")
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
        .context("dynamic producer fixed bridge is outside candidate")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn layout() -> MaterialLayout {
        MaterialLayout {
            identity_page: 0x2C,
            identity_material_base: 0x8000,
            identity_selector_directory: 0x8010,
            identity_table_descriptors: 0x8030,
            scan_index_page: 0x2D,
            page_recipe_references: 0x8000,
            record_directory: 0x8200,
            page_recipe_block_container_base: 0x1000,
            container_first_page: 0x2C,
            producer_encoding_page: 0x2F,
            producer_item_directory: 0x8100,
            producer_unit_directory: 0x81B6,
            producer_location_directory: 0x8220,
            producer_encoding_base: 0x8000,
        }
    }

    #[test]
    fn fixed_stubs_and_bridge_fit_their_independent_cave() {
        let runtime = build_dynamic_producer_runtime(0xA400, 0x30, layout()).unwrap();
        let last = runtime.fixed_routines.last().unwrap();
        assert!(usize::from(last.address) + last.bytes.len() <= usize::from(FIXED_BRIDGE_END));
        assert_eq!(runtime.fixed_routines.len(), HOOK_SITES.len() + 1);
    }

    #[test]
    fn all_five_hooks_are_three_byte_jumps() {
        let runtime = build_dynamic_producer_runtime(0xA400, 0x30, layout()).unwrap();
        assert_eq!(runtime.hooks.len(), HOOK_SITES.len());
        assert!(runtime.hooks.iter().all(|hook| hook.bytes.len() == 3));
    }

    #[test]
    fn canonical_copy_lives_in_the_runtime_code_page() {
        let runtime = build_dynamic_producer_runtime(0xA400, 0x30, layout()).unwrap();
        assert_eq!(runtime.code_routines.len(), 1);
        assert_eq!(runtime.code_routines[0].address, 0xA400);
        assert!(
            usize::from(runtime.code_routines[0].address) + runtime.code_routines[0].bytes.len()
                <= 0xC000
        );
    }
}
