use std::collections::BTreeSet;

use retro_rp2a03::{AddressingMode, Instruction, Mnemonic, Operand, encode_bytes};

use super::*;

fn typed_bytes(mnemonic: Mnemonic, mode: AddressingMode, operand: Operand) -> Vec<u8> {
    encode_bytes(&Instruction::new(mnemonic, mode, operand).unwrap()).unwrap()
}

fn exact_selector_bytes(opcode: u8, operand: Operand) -> Vec<u8> {
    encode_bytes(&Instruction::from_opcode(opcode, operand).unwrap()).unwrap()
}

fn one_region_program<'a>(bytes: &'a [u8], executable_len: usize) -> ExecutableProgram<'a> {
    let region = ExecutableRegion::new("synthetic-root", 3, 0x8000, bytes, executable_len).unwrap();
    ExecutableProgram::new(
        "synthetic-program",
        vec![region.clone()],
        vec![region.location(0x8000)],
        Vec::new(),
        Vec::new(),
    )
    .unwrap()
}

#[test]
fn source_mmc4_decoder_covers_every_register_page_alias() {
    use SourceMmc4Register::*;
    for (page_start, expected) in [
        (0xA000, PrgBank),
        (0xB000, LeftFdChrBank),
        (0xC000, LeftFeChrBank),
        (0xD000, RightFdChrBank),
        (0xE000, RightFeChrBank),
        (0xF000, Mirroring),
    ] {
        assert_eq!(decode_source_mmc4_write(page_start), Some(expected));
        assert_eq!(
            decode_source_mmc4_write(page_start | 0x0FFF),
            Some(expected)
        );
    }
    assert_eq!(decode_source_mmc4_write(0x9FFF), None);
}

#[test]
fn mapper165_decoder_covers_all_eight_mmc3_mask_classes() {
    use Mapper165Register::*;
    for (canonical, alias, expected) in [
        (0x8000, 0x9FFE, BankSelect),
        (0x8001, 0x9FFF, BankData),
        (0xA000, 0xBFFE, Mirroring),
        (0xA001, 0xBFFF, PrgRamProtect),
        (0xC000, 0xDFFE, IrqLatch),
        (0xC001, 0xDFFF, IrqReload),
        (0xE000, 0xFFFE, IrqDisable),
        (0xE001, 0xFFFF, IrqEnable),
    ] {
        assert_eq!(decode_mapper165_write(canonical), Some(expected));
        assert_eq!(decode_mapper165_write(alias), Some(expected));
    }
    assert_eq!(decode_mapper165_write(0x7FFF), None);
}

#[test]
fn typed_semantics_finds_documented_and_unofficial_read_modify_writes() {
    let mut bytes = typed_bytes(
        Mnemonic::Inc,
        AddressingMode::Absolute,
        Operand::Word(0xA123),
    );
    bytes.extend(exact_selector_bytes(0xCF, Operand::Word(0xB456)));
    bytes.extend(typed_bytes(
        Mnemonic::Kil,
        AddressingMode::Implied,
        Operand::None,
    ));
    let program = one_region_program(&bytes, bytes.len());

    let analysis = ExecutableMapperWriteAnalyzer::new(MapperHardware::SourceMmc4).analyze(&program);

    assert_eq!(analysis.direct_mapper_writes.len(), 2);
    assert_eq!(
        analysis.direct_mapper_writes[0].register,
        MapperRegister::SourceMmc4(SourceMmc4Register::PrgBank)
    );
    assert!(analysis.direct_mapper_writes[0].opcode_is_documented);
    assert_eq!(
        analysis.direct_mapper_writes[1].register,
        MapperRegister::SourceMmc4(SourceMmc4Register::LeftFdChrBank)
    );
    assert!(!analysis.direct_mapper_writes[1].opcode_is_documented);
    analysis.require_declared_routes_resolved().unwrap();
}

#[test]
fn rooted_decode_does_not_interpret_padding_after_executable_extent() {
    let mut bytes = typed_bytes(Mnemonic::Kil, AddressingMode::Implied, Operand::None);
    bytes.extend(typed_bytes(
        Mnemonic::Sta,
        AddressingMode::Absolute,
        Operand::Word(0x8000),
    ));
    let program = one_region_program(&bytes, 1);

    let analysis = ExecutableMapperWriteAnalyzer::new(MapperHardware::Mapper165).analyze(&program);

    assert!(analysis.direct_mapper_writes.is_empty());
    assert_eq!(
        analysis.reachable_instruction_locations,
        BTreeSet::from([CodeLocation::new("synthetic-root", 3, 0x8000)])
    );
    analysis.require_declared_routes_resolved().unwrap();
}

#[test]
fn indirect_jump_is_an_unresolved_control_edge() {
    let mut bytes = typed_bytes(
        Mnemonic::Jmp,
        AddressingMode::AbsoluteIndirect,
        Operand::Word(0x1234),
    );
    bytes.extend(typed_bytes(
        Mnemonic::Kil,
        AddressingMode::Implied,
        Operand::None,
    ));
    let program = one_region_program(&bytes, bytes.len());

    let analysis = ExecutableMapperWriteAnalyzer::new(MapperHardware::Mapper165).analyze(&program);

    assert!(
        analysis
            .unresolved_facts
            .contains(&UnresolvedExecutableFact::ControlEdge {
                instruction: CodeLocation::new("synthetic-root", 3, 0x8000),
                edge: UnresolvedControlEdge::IndirectTarget,
            })
    );
    assert!(analysis.require_declared_routes_resolved().is_err());
}

#[test]
fn direct_branch_taken_and_fallthrough_paths_and_direct_call_are_all_reached() {
    let mut bytes = typed_bytes(
        Mnemonic::Jsr,
        AddressingMode::Absolute,
        Operand::Word(0x8008),
    );
    bytes.extend(typed_bytes(
        Mnemonic::Beq,
        AddressingMode::Relative,
        Operand::Relative(2),
    ));
    bytes.extend(typed_bytes(
        Mnemonic::Kil,
        AddressingMode::Implied,
        Operand::None,
    ));
    bytes.push(
        typed_bytes(
            Mnemonic::Sta,
            AddressingMode::Absolute,
            Operand::Word(0x8000),
        )[0],
    ); // unreachable byte inside the declared extent
    bytes.extend(typed_bytes(
        Mnemonic::Kil,
        AddressingMode::Implied,
        Operand::None,
    ));
    bytes.extend(typed_bytes(
        Mnemonic::Nop,
        AddressingMode::Implied,
        Operand::None,
    ));
    bytes.extend(typed_bytes(
        Mnemonic::Rts,
        AddressingMode::Implied,
        Operand::None,
    ));
    let program = one_region_program(&bytes, bytes.len());

    let analysis = ExecutableMapperWriteAnalyzer::new(MapperHardware::Mapper165).analyze(&program);

    assert_eq!(
        analysis.reachable_instruction_locations,
        [0x8000, 0x8003, 0x8005, 0x8007, 0x8008, 0x8009]
            .into_iter()
            .map(|address| CodeLocation::new("synthetic-root", 3, address))
            .collect()
    );
    assert!(analysis.direct_mapper_writes.is_empty());
    analysis.require_declared_routes_resolved().unwrap();
}

#[test]
fn explicitly_bound_cross_region_call_returns_to_its_physical_caller() {
    let mut caller_bytes = typed_bytes(
        Mnemonic::Jsr,
        AddressingMode::Absolute,
        Operand::Word(0xC000),
    );
    caller_bytes.extend(typed_bytes(
        Mnemonic::Kil,
        AddressingMode::Implied,
        Operand::None,
    ));
    let callee_bytes = typed_bytes(Mnemonic::Rts, AddressingMode::Implied, Operand::None);
    let caller =
        ExecutableRegion::new("caller-page", 5, 0x8000, &caller_bytes, caller_bytes.len()).unwrap();
    let callee = ExecutableRegion::new(
        "fixed-callee",
        63,
        0xC000,
        &callee_bytes,
        callee_bytes.len(),
    )
    .unwrap();
    let program = ExecutableProgram::new(
        "cross-region-call",
        vec![caller.clone(), callee.clone()],
        vec![caller.location(0x8000)],
        Vec::new(),
        vec![DirectCodeBinding::new(
            "caller-page",
            0xC000,
            callee.location(0xC000),
        )],
    )
    .unwrap();

    let analysis = ExecutableMapperWriteAnalyzer::new(MapperHardware::Mapper165).analyze(&program);

    assert_eq!(
        analysis.reachable_instruction_locations,
        BTreeSet::from([
            caller.location(0x8000),
            caller.location(0x8003),
            callee.location(0xC000),
        ])
    );
    analysis.require_declared_routes_resolved().unwrap();
}

#[test]
fn indirect_effective_write_keeps_the_declared_route_open() {
    let mut bytes = typed_bytes(
        Mnemonic::Sta,
        AddressingMode::ZeroPageIndirectIndexedY,
        Operand::Byte(0x10),
    );
    bytes.extend(typed_bytes(
        Mnemonic::Kil,
        AddressingMode::Implied,
        Operand::None,
    ));
    let program = one_region_program(&bytes, bytes.len());

    let analysis = ExecutableMapperWriteAnalyzer::new(MapperHardware::Mapper165).analyze(&program);

    assert!(
        analysis
            .unresolved_facts
            .contains(&UnresolvedExecutableFact::EffectiveMapperWrite {
                instruction: CodeLocation::new("synthetic-root", 3, 0x8000),
                mode: AddressingMode::ZeroPageIndirectIndexedY,
                operand: Operand::Byte(0x10),
            })
    );
    assert!(analysis.require_declared_routes_resolved().is_err());
}

#[test]
fn instruction_fetch_crosses_an_explicit_physical_page_boundary() {
    let write = Instruction::new(
        Mnemonic::Sta,
        AddressingMode::Absolute,
        Operand::Word(0x8000),
    )
    .unwrap();
    let write_bytes = encode_bytes(&write).unwrap();
    let first_bytes = &write_bytes[..2];
    let mut second_bytes = vec![write_bytes[2]];
    second_bytes.extend(typed_bytes(
        Mnemonic::Kil,
        AddressingMode::Implied,
        Operand::None,
    ));
    let first = ExecutableRegion::new("switchable-page", 5, 0xBFFE, first_bytes, 2).unwrap();
    let second = ExecutableRegion::new("fixed-page", 63, 0xC000, &second_bytes, 2).unwrap();
    let program = ExecutableProgram::new(
        "mapped-boundary",
        vec![first.clone(), second.clone()],
        vec![first.location(0xBFFE)],
        vec![SequentialCodeBoundary::new(
            "switchable-page",
            second.location(0xC000),
        )],
        Vec::new(),
    )
    .unwrap();

    let analysis = ExecutableMapperWriteAnalyzer::new(MapperHardware::Mapper165).analyze(&program);

    assert_eq!(
        analysis.direct_mapper_writes,
        vec![DirectMapperWrite {
            instruction: first.location(0xBFFE),
            address: 0x8000,
            register: MapperRegister::Mapper165(Mapper165Register::BankSelect),
            opcode: write.opcode(),
            opcode_is_documented: true,
        }]
    );
    assert!(
        analysis
            .reachable_instruction_locations
            .contains(&second.location(0xC001))
    );
    analysis.require_declared_routes_resolved().unwrap();
}

#[test]
fn missing_physical_page_boundary_does_not_silently_truncate_an_instruction() {
    let write_bytes = typed_bytes(
        Mnemonic::Sta,
        AddressingMode::Absolute,
        Operand::Word(0x8000),
    );
    let bytes = &write_bytes[..2];
    let region = ExecutableRegion::new("switchable-page", 5, 0xBFFE, bytes, 2).unwrap();
    let program = ExecutableProgram::new(
        "unmapped-boundary",
        vec![region.clone()],
        vec![region.location(0xBFFE)],
        Vec::new(),
        Vec::new(),
    )
    .unwrap();

    let analysis = ExecutableMapperWriteAnalyzer::new(MapperHardware::Mapper165).analyze(&program);

    assert_eq!(
        analysis.unresolved_facts,
        vec![UnresolvedExecutableFact::InstructionBytesUnavailable {
            instruction: region.location(0xBFFE),
            next_cpu_address: 0xC000,
        }]
    );
    assert!(analysis.require_declared_routes_resolved().is_err());
}
