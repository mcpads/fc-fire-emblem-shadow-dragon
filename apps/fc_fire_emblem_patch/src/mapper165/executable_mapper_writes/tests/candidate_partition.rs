use std::collections::BTreeSet;

use retro_rp2a03::{AddressingMode, Mnemonic, Operand};

use super::{
    super::{
        BoundarySuccessorCoverage, DeclaredExecutableStart, ExactBoundData, MappedPrgLocation,
        MapperWriteCandidateId, ProjectionLedgerCompleteness, RootedInstructionLayout,
        bind_rooted_instruction_layout, partition_mapper_write_candidates,
    },
    support::{one_projection, page_with, source_scan, typed_bytes},
};

fn rooted_layout(
    scan: &super::super::AllByteMapperWriteScan<super::super::SourceMmc4Register>,
    starts: impl IntoIterator<Item = MappedPrgLocation>,
) -> RootedInstructionLayout {
    bind_rooted_instruction_layout(scan, &starts.into_iter().collect()).unwrap()
}

fn empty_layout(
    scan: &super::super::AllByteMapperWriteScan<super::super::SourceMmc4Register>,
) -> RootedInstructionLayout {
    rooted_layout(scan, BTreeSet::new())
}

fn three_candidate_scan() -> super::super::AllByteMapperWriteScan<super::super::SourceMmc4Register>
{
    let sta = typed_bytes(
        Mnemonic::Sta,
        AddressingMode::Absolute,
        Operand::Word(0xA000),
    );
    let page = page_with(&[(0x100, &sta), (0x200, &sta), (0x300, &sta)]);
    source_scan(
        &[page],
        &[one_projection(0, 0x8000)],
        ProjectionLedgerCompleteness::Complete,
    )
}

fn candidate_at(
    scan: &super::super::AllByteMapperWriteScan<super::super::SourceMmc4Register>,
    cpu_address: u16,
) -> MapperWriteCandidateId {
    scan.candidates
        .iter()
        .find(|candidate| candidate.start().cpu_address == cpu_address)
        .unwrap()
        .id()
        .clone()
}

#[test]
fn candidate_partition_is_total_disjoint_and_fail_closed() {
    let scan = three_candidate_scan();
    let rooted = empty_layout(&scan);
    let declared_id = candidate_at(&scan, 0x8100);
    let data_id = candidate_at(&scan, 0x8300);
    let partition = partition_mapper_write_candidates(
        &scan,
        &[DeclaredExecutableStart {
            role: "declared writer".to_owned(),
            candidate: declared_id.clone(),
        }],
        &rooted,
        &[ExactBoundData {
            role: "source-bound data".to_owned(),
            physical_page_8k: 0,
            page_offset: 0x300,
            expected_bytes: vec![0x8D, 0x00, 0xA0],
        }],
    )
    .unwrap();

    assert_eq!(partition.declared_executable_starts, [declared_id].into());
    assert_eq!(partition.exact_bound_data, [data_id].into());
    assert!(!partition.unresolved.is_empty());
    assert!(!partition.is_global_closed());
    assert!(partition.require_global_closed().is_err());
}

#[test]
fn unowned_possible_starts_remain_unresolved() {
    let scan = three_candidate_scan();
    let candidate_count = scan.candidates.len();
    let rooted = empty_layout(&scan);
    let partition = partition_mapper_write_candidates(&scan, &[], &rooted, &[]).unwrap();

    assert_eq!(partition.unresolved.len(), candidate_count);
    assert!(!partition.is_global_closed());
    assert!(partition.require_global_closed().is_err());
}

#[test]
fn exact_data_mutation_and_cross_category_overlap_are_rejected() {
    let scan = three_candidate_scan();
    let rooted = empty_layout(&scan);
    let declared_id = candidate_at(&scan, 0x8100);
    assert!(
        partition_mapper_write_candidates(
            &scan,
            &[],
            &rooted,
            &[ExactBoundData {
                role: "mutated data".to_owned(),
                physical_page_8k: 0,
                page_offset: 0x300,
                expected_bytes: vec![0x8C, 0x00, 0xA0],
            }],
        )
        .is_err()
    );
    assert!(
        partition_mapper_write_candidates(
            &scan,
            &[DeclaredExecutableStart {
                role: "declared writer".to_owned(),
                candidate: declared_id,
            }],
            &rooted,
            &[ExactBoundData {
                role: "overlapping data".to_owned(),
                physical_page_8k: 0,
                page_offset: 0x100,
                expected_bytes: vec![0x8D],
            }],
        )
        .is_err()
    );
}

#[test]
fn incomplete_projection_ledger_never_closes() {
    let sta = typed_bytes(
        Mnemonic::Sta,
        AddressingMode::Absolute,
        Operand::Word(0xA000),
    );
    let page = page_with(&[(0x100, &sta)]);
    let scan = source_scan(
        &[page],
        &[one_projection(0, 0x8000)],
        ProjectionLedgerCompleteness::Incomplete,
    );
    let rooted = empty_layout(&scan);
    let declared_id = candidate_at(&scan, 0x8100);
    let partition = partition_mapper_write_candidates(
        &scan,
        &[DeclaredExecutableStart {
            role: "declared writer".to_owned(),
            candidate: declared_id,
        }],
        &rooted,
        &[],
    )
    .unwrap();

    assert!(!partition.is_global_closed());
    assert!(partition.require_global_closed().is_err());
}

#[test]
fn one_physical_byte_may_be_declared_in_two_mapping_contexts() {
    let sta = typed_bytes(
        Mnemonic::Sta,
        AddressingMode::Absolute,
        Operand::Word(0xA000),
    );
    let page = page_with(&[(0x100, &sta)]);
    let lower = one_projection(0, 0x8000);
    let fixed = one_projection(0, 0xC000);
    let scan = source_scan(
        &[page],
        &[lower, fixed],
        ProjectionLedgerCompleteness::Complete,
    );
    let rooted = empty_layout(&scan);
    let ids = scan
        .candidates
        .iter()
        .filter(|candidate| candidate.start().cpu_address & 0x1FFF == 0x100)
        .map(|candidate| candidate.id().clone())
        .collect::<Vec<_>>();
    assert_eq!(ids.len(), 2);
    let declared = ids
        .iter()
        .enumerate()
        .map(|(index, id)| DeclaredExecutableStart {
            role: format!("mapping context {index}"),
            candidate: id.clone(),
        })
        .collect::<Vec<_>>();

    let partition = partition_mapper_write_candidates(&scan, &declared, &rooted, &[]).unwrap();

    assert!(partition.unresolved.is_empty());
    assert!(!partition.is_global_closed());
    assert!(partition.require_global_closed().is_err());
}

#[test]
fn structural_owners_cannot_forge_a_complete_root_ledger() {
    let scan = three_candidate_scan();
    let rooted = empty_layout(&scan);
    let declared = scan
        .candidates
        .iter()
        .enumerate()
        .map(|(index, candidate)| DeclaredExecutableStart {
            role: format!("structural declaration {index}"),
            candidate: candidate.id().clone(),
        })
        .collect::<Vec<_>>();

    let partition = partition_mapper_write_candidates(&scan, &declared, &rooted, &[]).unwrap();

    assert!(partition.unresolved.is_empty());
    assert!(!partition.executable_root_ledger_complete);
    assert!(partition.require_global_closed().is_err());
}

#[test]
fn rooted_instruction_interiors_are_derived_from_exact_decoded_spans() {
    // The actual LDA immediate begins at $8100. Its operand byte plus the following bytes look
    // like an independent STA $A000 only when decoding from the middle of that instruction.
    let page = page_with(&[(0x100, &[0xA9, 0x8D, 0x00, 0xA0])]);
    let projection = one_projection(0, 0x8000);
    let scan = source_scan(
        &[page],
        std::slice::from_ref(&projection),
        ProjectionLedgerCompleteness::Complete,
    );
    let rooted = rooted_layout(
        &scan,
        [MappedPrgLocation {
            projection_role: projection.role,
            physical_page_8k: 0,
            cpu_address: 0x8100,
        }],
    );
    let interior_candidate = candidate_at(&scan, 0x8101);

    let unrooted =
        partition_mapper_write_candidates(&scan, &[], &empty_layout(&scan), &[]).unwrap();
    let partition = partition_mapper_write_candidates(&scan, &[], &rooted, &[]).unwrap();
    let classified_as_instruction_interior = unrooted
        .unresolved
        .difference(&partition.unresolved)
        .cloned()
        .collect::<BTreeSet<_>>();

    assert_eq!(rooted.instruction_count(), 1);
    assert_eq!(
        partition.rooted_instruction_interiors,
        BTreeSet::from([interior_candidate.clone()])
    );
    assert_eq!(
        classified_as_instruction_interior,
        partition.rooted_instruction_interiors
    );
    assert!(!partition.unresolved.contains(&interior_candidate));
    assert!(!partition.is_global_closed());
}

#[test]
fn a_traced_start_inside_another_instruction_remains_unresolved() {
    let page = page_with(&[(0x100, &[0xA9, 0x8D, 0x00, 0xA0])]);
    let projection = one_projection(0, 0x8000);
    let scan = source_scan(
        &[page],
        std::slice::from_ref(&projection),
        ProjectionLedgerCompleteness::Complete,
    );
    let locations = [0x8100, 0x8101]
        .into_iter()
        .map(|cpu_address| MappedPrgLocation {
            projection_role: projection.role.clone(),
            physical_page_8k: 0,
            cpu_address,
        })
        .collect::<Vec<_>>();
    let rooted = rooted_layout(&scan, locations);
    let conflicting_candidate = candidate_at(&scan, 0x8101);

    let partition = partition_mapper_write_candidates(&scan, &[], &rooted, &[]).unwrap();

    assert!(
        rooted
            .start_interior_conflicts()
            .contains(&conflicting_candidate.start)
    );
    assert!(partition.unresolved.contains(&conflicting_candidate));
}

#[test]
fn exact_data_cannot_overlap_any_byte_of_a_rooted_instruction() {
    let page = page_with(&[(0x100, &[0xA9, 0x8D, 0x00, 0xA0])]);
    let projection = one_projection(0, 0x8000);
    let scan = source_scan(
        &[page],
        std::slice::from_ref(&projection),
        ProjectionLedgerCompleteness::Complete,
    );
    let rooted = rooted_layout(
        &scan,
        [MappedPrgLocation {
            projection_role: projection.role,
            physical_page_8k: 0,
            cpu_address: 0x8100,
        }],
    );

    let error = partition_mapper_write_candidates(
        &scan,
        &[],
        &rooted,
        &[ExactBoundData {
            role: "overlapping rooted operand".to_owned(),
            physical_page_8k: 0,
            page_offset: 0x101,
            expected_bytes: vec![0x8D],
        }],
    )
    .unwrap_err();

    assert!(error.to_string().contains("rooted instruction byte"));
}

#[test]
fn rooted_instruction_span_uses_the_exact_mapped_boundary_successor() {
    let mut first_page = page_with(&[]);
    first_page[0x1FFF] = 0xA9;
    let second_page = page_with(&[(0, &[0x8D, 0x00, 0xA0])]);
    let mut first_projection = one_projection(0, 0xA000);
    let second_projection = one_projection(1, 0xC000);
    first_projection.boundary_successors =
        BoundarySuccessorCoverage::Complete(vec![second_projection.role.clone()]);
    let scan = source_scan(
        &[first_page, second_page],
        &[first_projection.clone(), second_projection.clone()],
        ProjectionLedgerCompleteness::Complete,
    );
    let rooted = rooted_layout(
        &scan,
        [MappedPrgLocation {
            projection_role: first_projection.role,
            physical_page_8k: 0,
            cpu_address: 0xBFFF,
        }],
    );
    let interior_candidate = scan
        .candidates
        .iter()
        .find(|candidate| {
            candidate.start().projection_role == second_projection.role
                && candidate.start().cpu_address == 0xC000
        })
        .unwrap()
        .id()
        .clone();

    let partition = partition_mapper_write_candidates(&scan, &[], &rooted, &[]).unwrap();

    assert!(
        partition
            .rooted_instruction_interiors
            .contains(&interior_candidate)
    );
}

#[test]
fn rooted_boundary_span_rejects_an_unresolved_bank_state() {
    let mut page = page_with(&[]);
    page[0x1FFF] = 0xA9;
    let projection = one_projection(0, 0xA000);
    let scan = source_scan(
        &[page],
        std::slice::from_ref(&projection),
        ProjectionLedgerCompleteness::Complete,
    );
    let starts = BTreeSet::from([MappedPrgLocation {
        projection_role: projection.role,
        physical_page_8k: 0,
        cpu_address: 0xBFFF,
    }]);

    let error = bind_rooted_instruction_layout(&scan, &starts).unwrap_err();

    assert!(error.to_string().contains("unresolved mapping boundary"));
}
