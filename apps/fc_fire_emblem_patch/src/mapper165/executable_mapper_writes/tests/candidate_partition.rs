use retro_rp2a03::{AddressingMode, Mnemonic, Operand};

use super::{
    super::{
        DeclaredExecutableStart, ExactBoundData, MapperWriteCandidateId,
        ProjectionLedgerCompleteness, partition_mapper_write_candidates,
    },
    support::{one_projection, page_with, source_scan, typed_bytes},
};

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
    let declared_id = candidate_at(&scan, 0x8100);
    let data_id = candidate_at(&scan, 0x8300);
    let partition = partition_mapper_write_candidates(
        &scan,
        &[DeclaredExecutableStart {
            role: "declared writer".to_owned(),
            candidate: declared_id.clone(),
        }],
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
    let partition = partition_mapper_write_candidates(&scan, &[], &[]).unwrap();

    assert_eq!(partition.unresolved.len(), candidate_count);
    assert!(!partition.is_global_closed());
    assert!(partition.require_global_closed().is_err());
}

#[test]
fn exact_data_mutation_and_cross_category_overlap_are_rejected() {
    let scan = three_candidate_scan();
    let declared_id = candidate_at(&scan, 0x8100);
    assert!(
        partition_mapper_write_candidates(
            &scan,
            &[],
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
    let declared_id = candidate_at(&scan, 0x8100);
    let partition = partition_mapper_write_candidates(
        &scan,
        &[DeclaredExecutableStart {
            role: "declared writer".to_owned(),
            candidate: declared_id,
        }],
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

    let partition = partition_mapper_write_candidates(&scan, &declared, &[]).unwrap();

    assert!(partition.unresolved.is_empty());
    assert!(!partition.is_global_closed());
    assert!(partition.require_global_closed().is_err());
}

#[test]
fn structural_owners_cannot_forge_a_complete_root_ledger() {
    let scan = three_candidate_scan();
    let declared = scan
        .candidates
        .iter()
        .enumerate()
        .map(|(index, candidate)| DeclaredExecutableStart {
            role: format!("structural declaration {index}"),
            candidate: candidate.id().clone(),
        })
        .collect::<Vec<_>>();

    let partition = partition_mapper_write_candidates(&scan, &declared, &[]).unwrap();

    assert!(partition.unresolved.is_empty());
    assert!(!partition.executable_root_ledger_complete);
    assert!(partition.require_global_closed().is_err());
}
