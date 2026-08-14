use retro_rp2a03::{AddressingMode, Mnemonic, Operand};

use super::{
    super::{
        BoundarySuccessorCoverage, CandidateDecodeVariant, MappedPrgProjection, MapperWriteAccess,
        MapperWriteCandidate, PhysicalPrgPage, ProjectionLedgerCompleteness, SourceMmc4Register,
        scan_all_byte_mapper_write_candidates,
    },
    support::{KIL, PAGE_LEN, exact_bytes, one_projection, page_with, source_scan, typed_bytes},
};

#[test]
fn hidden_possible_start_is_root_independent() {
    let sta = typed_bytes(
        Mnemonic::Sta,
        AddressingMode::Absolute,
        Operand::Word(0xA000),
    );
    let page = page_with(&[(0x100, &sta)]);
    let projection = one_projection(0, 0x8000);

    let scan = source_scan(
        &[page],
        &[projection],
        ProjectionLedgerCompleteness::Complete,
    );

    assert!(scan.candidates.iter().any(|candidate| {
        candidate.start().cpu_address == 0x8100
            && matches!(
                candidate,
                MapperWriteCandidate::Decoded { accesses, .. }
                    if accesses.iter().any(|access| matches!(
                        access,
                        MapperWriteAccess::Direct {
                            register: SourceMmc4Register::PrgBank,
                            ..
                        }
                    ))
            )
    }));
}

#[test]
fn mapped_page_boundary_uses_exact_successor_bytes() {
    let lower = page_with(&[(PAGE_LEN - 1, &[0x8D])]);
    let fixed = page_with(&[(0, &[0x00, 0xA0])]);
    let lower_role = "lower-A000".to_owned();
    let fixed_role = "fixed-C000".to_owned();
    let projections = vec![
        MappedPrgProjection {
            role: lower_role.clone(),
            physical_page_8k: 0,
            cpu_start: 0xA000,
            boundary_successors: BoundarySuccessorCoverage::Complete(vec![fixed_role.clone()]),
        },
        MappedPrgProjection {
            role: fixed_role.clone(),
            physical_page_8k: 1,
            cpu_start: 0xC000,
            boundary_successors: BoundarySuccessorCoverage::Unresolved,
        },
    ];

    let scan = source_scan(
        &[lower, fixed],
        &projections,
        ProjectionLedgerCompleteness::Complete,
    );

    let candidate = scan
        .candidates
        .iter()
        .find(|candidate| candidate.start().cpu_address == 0xBFFF)
        .unwrap();
    assert_eq!(candidate.start().projection_role, lower_role);
    assert!(matches!(
        candidate.id().decode_variant,
        CandidateDecodeVariant::MappedSuccessor { ref projection_role }
            if projection_role == &fixed_role
    ));
    assert_eq!(candidate.byte_locations().len(), 3);
    assert_eq!(candidate.byte_locations()[1].projection_role, fixed_role);
}

#[test]
fn same_physical_page_keeps_lower_and_fixed_contexts_distinct() {
    let shared = page_with(&[(PAGE_LEN - 1, &[0x8D])]);
    let fixed_c = page_with(&[(0, &[0x00, 0xA0])]);
    let projections = vec![
        MappedPrgProjection {
            role: "bank0f-lower-A000".to_owned(),
            physical_page_8k: 0,
            cpu_start: 0xA000,
            boundary_successors: BoundarySuccessorCoverage::Complete(vec!["fixed-C000".to_owned()]),
        },
        MappedPrgProjection {
            role: "bank0f-fixed-E000".to_owned(),
            physical_page_8k: 0,
            cpu_start: 0xE000,
            boundary_successors: BoundarySuccessorCoverage::Unresolved,
        },
        MappedPrgProjection {
            role: "fixed-C000".to_owned(),
            physical_page_8k: 1,
            cpu_start: 0xC000,
            boundary_successors: BoundarySuccessorCoverage::Unresolved,
        },
    ];

    let scan = source_scan(
        &[shared, fixed_c],
        &projections,
        ProjectionLedgerCompleteness::Complete,
    );

    assert!(scan.candidates.iter().any(|candidate| {
        candidate.start().projection_role == "bank0f-lower-A000"
            && candidate.start().cpu_address == 0xBFFF
            && matches!(
                candidate.id().decode_variant,
                CandidateDecodeVariant::MappedSuccessor { .. }
            )
    }));
    assert!(scan.candidates.iter().any(|candidate| {
        candidate.start().projection_role == "bank0f-fixed-E000"
            && candidate.start().cpu_address == 0xFFFF
            && matches!(
                candidate.id().decode_variant,
                CandidateDecodeVariant::UnresolvedBoundary
            )
    }));
}

#[test]
fn static_semantics_finds_documented_unofficial_indexed_and_indirect_writes() {
    let inc = typed_bytes(
        Mnemonic::Inc,
        AddressingMode::Absolute,
        Operand::Word(0xA123),
    );
    let unofficial = exact_bytes(0xCF, Operand::Word(0xB456));
    let indexed = typed_bytes(
        Mnemonic::Sta,
        AddressingMode::AbsoluteX,
        Operand::Word(0x9FFF),
    );
    let indirect = typed_bytes(
        Mnemonic::Sta,
        AddressingMode::ZeroPageIndirectIndexedY,
        Operand::Byte(0x10),
    );
    let page = page_with(&[
        (0x100, &inc),
        (0x200, &unofficial),
        (0x300, &indexed),
        (0x400, &indirect),
    ]);

    let scan = source_scan(
        &[page],
        &[one_projection(0, 0x8000)],
        ProjectionLedgerCompleteness::Complete,
    );

    for cpu in [0x8100, 0x8200, 0x8300, 0x8400] {
        assert!(
            scan.candidates
                .iter()
                .any(|candidate| candidate.start().cpu_address == cpu),
            "missing candidate at {cpu:04X}"
        );
    }
    assert!(scan.candidates.iter().any(|candidate| {
        matches!(
            candidate,
            MapperWriteCandidate::Decoded {
                opcode_is_documented: false,
                ..
            } if candidate.start().cpu_address == 0x8200
        )
    }));
    assert!(scan.candidates.iter().any(|candidate| {
        matches!(
            candidate,
            MapperWriteCandidate::Decoded { accesses, .. }
                if candidate.start().cpu_address == 0x8300
                    && accesses.iter().any(|access| matches!(
                        access,
                        MapperWriteAccess::Effective {
                            mode: AddressingMode::AbsoluteX,
                            ..
                        }
                    ))
        )
    }));
}

#[test]
fn unresolved_boundary_completion_is_preserved() {
    let page = page_with(&[(PAGE_LEN - 1, &[0x8D])]);
    let projection = MappedPrgProjection {
        role: "fixed-E000".to_owned(),
        physical_page_8k: 0,
        cpu_start: 0xE000,
        boundary_successors: BoundarySuccessorCoverage::Unresolved,
    };

    let scan = source_scan(
        &[page],
        &[projection],
        ProjectionLedgerCompleteness::Complete,
    );

    assert!(scan.candidates.iter().any(|candidate| matches!(
        candidate,
        MapperWriteCandidate::BoundaryBytesUnresolved { id, .. }
            if id.start.cpu_address == 0xFFFF
    )));
}

#[test]
fn a_physical_page_without_any_projection_is_rejected() {
    let pages = [vec![KIL; PAGE_LEN], vec![KIL; PAGE_LEN]];
    let physical_pages = pages
        .iter()
        .enumerate()
        .map(|(index, bytes)| PhysicalPrgPage {
            physical_page_8k: u16::try_from(index).unwrap(),
            bytes,
        })
        .collect::<Vec<_>>();

    assert!(
        scan_all_byte_mapper_write_candidates(
            &physical_pages,
            &[one_projection(0, 0x8000)],
            ProjectionLedgerCompleteness::Complete,
            super::super::decode_source_mmc4_write,
        )
        .is_err()
    );
}

#[test]
fn empty_complete_boundary_cannot_hide_a_possible_instruction_fetch() {
    let page = vec![KIL; PAGE_LEN];
    let projection = MappedPrgProjection {
        role: "unrooted-terminal".to_owned(),
        physical_page_8k: 0,
        cpu_start: 0xE000,
        boundary_successors: BoundarySuccessorCoverage::Complete(Vec::new()),
    };

    assert!(
        scan_all_byte_mapper_write_candidates(
            &[PhysicalPrgPage {
                physical_page_8k: 0,
                bytes: &page,
            }],
            &[projection],
            ProjectionLedgerCompleteness::Complete,
            super::super::decode_source_mmc4_write,
        )
        .is_err()
    );
}
