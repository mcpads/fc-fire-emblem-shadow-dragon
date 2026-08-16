use anyhow::{Result, ensure};
use retro_rp2a03::{AddressingMode, Operand};
use serde::Serialize;

use crate::{
    mapper165::{
        executable_mapper_writes::{
            AllByteMapperWriteScan, DeclaredExecutableStart, MappedPrgProjection,
            Mapper165Register, MapperWriteAccess, MapperWriteCandidate, PhysicalPrgPage,
            ProjectionLedgerCompleteness, decode_mapper165_write,
            scan_all_byte_mapper_write_candidates,
        },
        source_indexed_mapper_aliases::source_indexed_menu_mask_store_sites,
    },
    rom::Rom,
};

use super::{
    FIXED_PRG_BANK, SourceWriterDeclaration, mapper_write_candidate_digest, source_mapped_location,
};

const EXPECTED_TARGET_MAPPER_CANDIDATE_COUNT: usize = 51_100;
const EXPECTED_TARGET_MAPPER_CANDIDATE_DIGEST_SHA1: &str =
    "11ac9401a9ac58748433afb08aebfbb39ab2a664";
const SOURCE_BOUND_INDIRECT_WRITER_COUNT: usize = 32;

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
    source_slice: &'static str,
    candidate: String,
}

pub(super) fn audit_target_mapper_migration(
    pages: &[PhysicalPrgPage<'_>],
    projections: &[MappedPrgProjection],
    canonical_writers: &[SourceWriterDeclaration],
    source: &Rom,
) -> Result<TargetMapperMigrationAudit> {
    let scan = scan_all_byte_mapper_write_candidates(
        pages,
        projections,
        ProjectionLedgerCompleteness::Complete,
        decode_mapper165_write,
    )?;
    let candidate_digest_sha1 = mapper_write_candidate_digest(&scan);
    ensure!(
        scan.candidates.len() == EXPECTED_TARGET_MAPPER_CANDIDATE_COUNT,
        "supported source target-mapper possible-start denominator changed: expected {EXPECTED_TARGET_MAPPER_CANDIDATE_COUNT}, found {}",
        scan.candidates.len()
    );
    ensure!(
        candidate_digest_sha1 == EXPECTED_TARGET_MAPPER_CANDIDATE_DIGEST_SHA1,
        "supported source target-mapper possible-start identity changed: expected {EXPECTED_TARGET_MAPPER_CANDIDATE_DIGEST_SHA1}, found {candidate_digest_sha1}"
    );
    let converted = bind_target_canonical_writers(&scan, canonical_writers)?;
    let guarded = bind_target_indexed_writers(&scan)?;
    let battle_indirect =
        crate::mapper165::battle_codebook_plan::bind_indirect_write_sites_below_mapper_space(
            source,
        )?;
    let dialogue_interrupt_audio =
        crate::full_translation_install::bind_dialogue_interrupt_audio_mapper_write_slice(source)?;
    let indirect_sites = battle_indirect
        .union(&dialogue_interrupt_audio.indirect_write_sites_below_mapper_space)
        .copied()
        .collect::<std::collections::BTreeSet<_>>();
    let source_bound_indirect =
        bind_target_indirect_writes_below_mapper_space(&scan, &indirect_sites)?;
    ensure!(
        source_bound_indirect.len() == SOURCE_BOUND_INDIRECT_WRITER_COUNT,
        "source-bound indirect mapper-safe writer count changed: expected {SOURCE_BOUND_INDIRECT_WRITER_COUNT}, found {}",
        source_bound_indirect.len()
    );
    let known = converted
        .iter()
        .chain(&guarded)
        .chain(&source_bound_indirect)
        .map(|declaration| declaration.candidate.clone())
        .collect::<std::collections::BTreeSet<_>>();

    let battle =
        crate::mapper165::battle_codebook_plan::phase_cooccurrence::battle_phase_reachable_instruction_starts(
            source,
        )?;
    let declared_reachable_slice_instruction_count = battle
        .union(&dialogue_interrupt_audio.reachable_instruction_starts)
        .map(|&(bank, address)| {
            let physical_bank = if address >= 0xC000 {
                FIXED_PRG_BANK
            } else {
                bank
            };
            (physical_bank, address)
        })
        .collect::<std::collections::BTreeSet<_>>()
        .len();

    let mut reachable_candidates = Vec::new();
    let mut unclassified = Vec::new();
    for (source_slice, starts) in [
        ("battle_phase_catalog", &battle),
        (
            "main_dialogue_nmi_and_audio_positive_graph",
            &dialogue_interrupt_audio.reachable_instruction_starts,
        ),
    ] {
        for &(bank, address) in starts {
            let actual_bank = if address >= 0xC000 {
                FIXED_PRG_BANK
            } else {
                bank
            };
            let location = source_mapped_location(actual_bank, address)?;
            for candidate in scan
                .candidates
                .iter()
                .filter(|candidate| candidate.start() == &location)
            {
                reachable_candidates.push(candidate.id().clone());
                if !known.contains(candidate.id()) {
                    unclassified.push(ReachableTargetMapperCandidate {
                        source_prg_bank_hex: format!("0x{actual_bank:02X}"),
                        cpu_address_hex: format!("0x{address:04X}"),
                        source_slice,
                        candidate: format!("{candidate:?}"),
                    });
                }
            }
        }
    }
    reachable_candidates.sort();
    reachable_candidates.dedup();
    unclassified.sort_by(|left, right| {
        left.source_prg_bank_hex
            .cmp(&right.source_prg_bank_hex)
            .then(left.cpu_address_hex.cmp(&right.cpu_address_hex))
            .then(left.source_slice.cmp(right.source_slice))
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
        closure_claim: "partial: this root-independent denominator includes source writes that MMC4 ignored but mapper165 decodes; the positive battle, main-dialogue, NMI, and audio graphs are crossed against it, while missing execution roots and computed target bounds remain unresolved",
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
    source_indexed_menu_mask_store_sites()
        .into_iter()
        .map(|(bank, address)| {
            let location = source_mapped_location(bank, address)?;
            let matches = scan
                .candidates
                .iter()
                .filter(|candidate| {
                    candidate.start() == &location
                        && matches!(
                            candidate,
                            MapperWriteCandidate::Decoded { opcode: 0x9D, accesses, .. }
                                if accesses.iter().any(|access| matches!(
                                    access,
                                    MapperWriteAccess::Effective {
                                        mode: AddressingMode::AbsoluteX,
                                        operand: Operand::Word(0x7FEE),
                                        ..
                                    }
                                ))
                        )
                })
                .collect::<Vec<_>>();
            ensure!(
                matches.len() == 1,
                "guarded indexed source writer bank {bank:02X}:${address:04X} matched {} target-mapper candidates",
                matches.len()
            );
            Ok(DeclaredExecutableStart {
                role: format!(
                    "guarded indexed menu-mask writer at bank {bank:02X}:${address:04X}"
                ),
                candidate: matches[0].id().clone(),
            })
        })
        .collect()
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
