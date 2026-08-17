use std::collections::{BTreeMap, BTreeSet};

use anyhow::{Result, ensure};
use retro_rp2a03::{AddressingMode, Operand};
use serde::Serialize;

use crate::mapper165::{
    executable_mapper_writes::{
        AllByteMapperWriteScan, DeclaredExecutableStart, MappedPrgProjection, Mapper165Register,
        MapperWriteAccess, MapperWriteCandidate, PhysicalPrgPage, ProjectionLedgerCompleteness,
        decode_mapper165_write, scan_all_byte_mapper_write_candidates,
    },
    source_indexed_mapper_aliases::{
        source_indexed_menu_mask_store_sites, source_indexed_menu_mask_y_store_sites,
        source_indexed_menu_selection_store_sites,
    },
};

use super::{
    SourceWriterDeclaration, mapper_write_candidate_digest,
    positive_execution::SourcePositiveExecutionGraph, source_mapped_location,
};

#[derive(Clone, Debug, Serialize)]
pub(super) struct TargetMapperMigrationAudit {
    candidate_scope: &'static str,
    mapper_write_candidate_count: usize,
    candidate_digest_sha1: String,
    converted_canonical_writer_count: usize,
    guarded_indexed_writer_count: usize,
    source_bound_indirect_writer_count: usize,
    declared_reachable_slice_instruction_count: usize,
    declared_reachable_slice_candidate_count: usize,
    declared_reachable_slice_unclassified_candidate_count: usize,
    declared_reachable_slice_unclassified_candidates: Vec<ReachableTargetMapperCandidate>,
    closure_claim: &'static str,
}

#[derive(Clone, Debug, Serialize)]
struct ReachableTargetMapperCandidate {
    source_prg_bank_hex: String,
    cpu_address_hex: String,
    source_slice: String,
    candidate: String,
}

pub(super) fn audit_target_mapper_migration(
    pages: &[PhysicalPrgPage<'_>],
    projections: &[MappedPrgProjection],
    canonical_writers: &[SourceWriterDeclaration],
    positive_execution: &SourcePositiveExecutionGraph,
) -> Result<TargetMapperMigrationAudit> {
    let scan = scan_all_byte_mapper_write_candidates(
        pages,
        projections,
        ProjectionLedgerCompleteness::Complete,
        decode_mapper165_write,
    )?;
    let candidate_digest_sha1 = mapper_write_candidate_digest(&scan);
    let converted = bind_target_canonical_writers(&scan, canonical_writers)?;
    let guarded = bind_target_indexed_writers(&scan)?;
    let source_bound_indirect = bind_target_indirect_writes_below_mapper_space(
        &scan,
        positive_execution.indirect_write_sites_below_mapper_space(),
    )?;
    let known = converted
        .iter()
        .chain(&guarded)
        .chain(&source_bound_indirect)
        .map(|declaration| declaration.candidate.clone())
        .collect::<BTreeSet<_>>();

    let declared_reachable_slice_instruction_count = positive_execution.instruction_count();
    let mut candidates_by_start = BTreeMap::<_, Vec<_>>::new();
    for candidate in &scan.candidates {
        candidates_by_start
            .entry(candidate.start().clone())
            .or_default()
            .push(candidate);
    }

    let mut reachable_candidates = Vec::new();
    let mut unclassified = Vec::new();
    for (bank, address) in positive_execution.instruction_starts() {
        let location = source_mapped_location(bank, address)?;
        for candidate in candidates_by_start.get(&location).into_iter().flatten() {
            reachable_candidates.push(candidate.id().clone());
            if !known.contains(candidate.id()) {
                let source_slice = positive_execution
                    .roles_at(bank, address)
                    .expect("positive execution location has at least one role")
                    .iter()
                    .copied()
                    .collect::<Vec<_>>()
                    .join("+");
                unclassified.push(ReachableTargetMapperCandidate {
                    source_prg_bank_hex: format!("0x{bank:02X}"),
                    cpu_address_hex: format!("0x{address:04X}"),
                    source_slice,
                    candidate: format!("{candidate:?}"),
                });
            }
        }
    }
    reachable_candidates.sort();
    reachable_candidates.dedup();
    unclassified.sort_by(|left, right| {
        left.source_prg_bank_hex
            .cmp(&right.source_prg_bank_hex)
            .then(left.cpu_address_hex.cmp(&right.cpu_address_hex))
            .then(left.source_slice.cmp(&right.source_slice))
            .then(left.candidate.cmp(&right.candidate))
    });
    unclassified.dedup_by(|left, right| {
        left.source_prg_bank_hex == right.source_prg_bank_hex
            && left.cpu_address_hex == right.cpu_address_hex
            && left.candidate == right.candidate
    });
    ensure!(
        unclassified.is_empty(),
        "a source-reachable target-mapper write remains outside the converted, guarded, and source-bound indirect catalogs: {unclassified:?}"
    );

    Ok(TargetMapperMigrationAudit {
        candidate_scope: "every source PRG byte offset under the complete MMC4 projection ledger, decoded against all mapper165/MMC3 $8000..$FFFF register aliases",
        mapper_write_candidate_count: scan.candidates.len(),
        candidate_digest_sha1,
        converted_canonical_writer_count: converted.len(),
        guarded_indexed_writer_count: guarded.len(),
        source_bound_indirect_writer_count: source_bound_indirect.len(),
        declared_reachable_slice_instruction_count,
        declared_reachable_slice_candidate_count: reachable_candidates.len(),
        declared_reachable_slice_unclassified_candidate_count: unclassified.len(),
        declared_reachable_slice_unclassified_candidates: unclassified,
        closure_claim: "partial: this root-independent denominator includes source writes that MMC4 ignored but mapper165 decodes; the fixed hardware-vector, reset, battle, main-dialogue, NMI, audio, title-state, source-closed ending-sequence, and positive fixed-scheduler graphs are crossed against it, while other dynamic switchable-bank, outer-screen and nested selector domains, missing execution roots, and computed target bounds remain unresolved",
    })
}

fn bind_target_indirect_writes_below_mapper_space(
    scan: &AllByteMapperWriteScan<Mapper165Register>,
    sites: &std::collections::BTreeSet<(u8, u16, u8)>,
) -> Result<Vec<DeclaredExecutableStart>> {
    sites
        .iter()
        .map(|&(bank, address, pointer)| {
            let location = source_mapped_location(bank, address)?;
            let matches = scan
                .candidates
                .iter()
                .filter(|candidate| {
                    candidate.start() == &location
                        && matches!(
                            candidate,
                            MapperWriteCandidate::Decoded { opcode: 0x91, accesses, .. }
                                if accesses.iter().any(|access| matches!(
                                    access,
                                    MapperWriteAccess::Effective {
                                        mode: AddressingMode::ZeroPageIndirectIndexedY,
                                        operand: Operand::Byte(actual_pointer),
                                        ..
                                    } if *actual_pointer == pointer
                                ))
                        )
                })
                .collect::<Vec<_>>();
            ensure!(
                matches.len() == 1,
                "source-bound indirect writer bank {bank:02X}:${address:04X} through ${pointer:02X} matched {} target-mapper candidates",
                matches.len()
            );
            Ok(DeclaredExecutableStart {
                role: format!(
                    "source-bound indirect writer below mapper space at bank {bank:02X}:${address:04X}"
                ),
                candidate: matches[0].id().clone(),
            })
        })
        .collect()
}

fn bind_target_canonical_writers(
    scan: &AllByteMapperWriteScan<Mapper165Register>,
    declarations: &[SourceWriterDeclaration],
) -> Result<Vec<DeclaredExecutableStart>> {
    declarations
        .iter()
        .map(|declaration| {
            let location = source_mapped_location(declaration.prg_bank, declaration.cpu_address)?;
            let matches = scan
                .candidates
                .iter()
                .filter(|candidate| {
                    candidate.start() == &location
                        && decoded_target_direct_candidate_matches(
                            candidate,
                            0x8D,
                            declaration.register_address,
                        )
                })
                .collect::<Vec<_>>();
            ensure!(
                matches.len() == 1,
                "converted source writer {} bank {:02X}:${:04X} matched {} target-mapper candidates",
                declaration.role,
                declaration.prg_bank,
                declaration.cpu_address,
                matches.len()
            );
            Ok(DeclaredExecutableStart {
                role: format!(
                    "converted {} at bank {:02X}:${:04X}",
                    declaration.role, declaration.prg_bank, declaration.cpu_address
                ),
                candidate: matches[0].id().clone(),
            })
        })
        .collect()
}

fn bind_target_indexed_writers(
    scan: &AllByteMapperWriteScan<Mapper165Register>,
) -> Result<Vec<DeclaredExecutableStart>> {
    let mut declarations = Vec::new();
    for (role, opcode, mode, operand, sites) in [
        (
            "menu-mask-x",
            0x9D,
            AddressingMode::AbsoluteX,
            0x7FEE,
            source_indexed_menu_mask_store_sites().to_vec(),
        ),
        (
            "menu-selection-x",
            0x9D,
            AddressingMode::AbsoluteX,
            0x7FF3,
            source_indexed_menu_selection_store_sites().to_vec(),
        ),
        (
            "menu-mask-y",
            0x99,
            AddressingMode::AbsoluteY,
            0x7FEE,
            source_indexed_menu_mask_y_store_sites().to_vec(),
        ),
    ] {
        for (bank, address) in sites {
            let location = source_mapped_location(bank, address)?;
            let matches = scan
                .candidates
                .iter()
                .filter(|candidate| {
                    candidate.start() == &location
                        && matches!(
                            candidate,
                            MapperWriteCandidate::Decoded { opcode: actual_opcode, accesses, .. }
                                if *actual_opcode == opcode && accesses.iter().any(|access| matches!(
                                    access,
                                    MapperWriteAccess::Effective {
                                        mode: actual_mode,
                                        operand: Operand::Word(actual_operand),
                                        ..
                                    } if *actual_mode == mode && *actual_operand == operand
                                ))
                        )
                })
                .collect::<Vec<_>>();
            ensure!(
                matches.len() == 1,
                "guarded indexed {role} source writer bank {bank:02X}:${address:04X} matched {} target-mapper candidates",
                matches.len()
            );
            declarations.push(DeclaredExecutableStart {
                role: format!("guarded indexed {role} writer at bank {bank:02X}:${address:04X}"),
                candidate: matches[0].id().clone(),
            });
        }
    }
    Ok(declarations)
}

fn decoded_target_direct_candidate_matches(
    candidate: &MapperWriteCandidate<Mapper165Register>,
    opcode: u8,
    address: u16,
) -> bool {
    let expected_register = decode_mapper165_write(address);
    matches!(
        candidate,
        MapperWriteCandidate::Decoded { opcode: actual_opcode, accesses, .. }
            if *actual_opcode == opcode
                && accesses.iter().any(|access| matches!(
                    access,
                    MapperWriteAccess::Direct { address: actual_address, register }
                        if *actual_address == address && Some(*register) == expected_register
                ))
    )
}
