use std::collections::BTreeSet;

use anyhow::{Context, Result, ensure};
use serde::Serialize;

use crate::{rom::Rom, sha1_hex};

use super::{
    SOURCE_SELECT_HORIZONTAL_MIRRORING_ADDRESS, SOURCE_SELECT_PRG_BANK_AND_SAVE_ADDRESS,
    SOURCE_SELECT_VERTICAL_MIRRORING_ADDRESS,
    executable_mapper_writes::{
        AllByteMapperWriteScan, BoundarySuccessorCoverage, DeclaredExecutableStart, ExactBoundData,
        MappedPrgLocation, MappedPrgProjection, MapperWriteAccess, MapperWriteCandidate,
        MapperWriteCandidatePartition, PhysicalPrgPage, ProjectionLedgerCompleteness,
        SourceMmc4Register, bind_rooted_instruction_layout, decode_source_mmc4_write,
        partition_mapper_write_candidates, scan_all_byte_mapper_write_candidates,
    },
    writer_census::{bind_audio_record_data_region, legacy_canonical_chr_write_candidates},
    writer_sites::{
        CENTRAL_CHR_WRITERS, DIRECT_CHR_WRITERS, SOURCE_PRG_BANK_WRITERS, WriterLocation,
    },
};

mod positive_execution;
mod target_mapper_migration;

use positive_execution::bind_source_positive_execution_graph;
use target_mapper_migration::{TargetMapperMigrationAudit, audit_target_mapper_migration};

const SOURCE_PRG_8K_PAGE_COUNT: usize = 32;
const SOURCE_PRG_8K_PAGE_LEN: usize = 0x2000;
const SOURCE_PRG_BANK_COUNT: u8 = 16;
const FIXED_PRG_BANK: u8 = 0x0F;
const FIXED_C000_PAGE: u16 = 0x1E;
const FIXED_E000_PAGE: u16 = 0x1F;
const FIXED_C000_PROJECTION: &str = "source-fixed-C000";
const FIXED_E000_PROJECTION: &str = "source-fixed-E000";
#[derive(Clone, Debug, Serialize)]
pub(super) struct SourceMapperWriteAudit {
    candidate_scope: &'static str,
    closure_claim: &'static str,
    physical_prg_page_count: usize,
    mapped_projection_count: usize,
    mapper_write_candidate_count: usize,
    declared_source_writer_count: usize,
    positive_execution_instruction_count: usize,
    rooted_instruction_interior_candidate_count: usize,
    rooted_start_interior_conflict_count: usize,
    unresolved_before_positive_execution_count: usize,
    exact_bound_data_candidate_count: usize,
    unresolved_candidate_count: usize,
    boundary_unresolved_candidate_count: usize,
    legacy_canonical_candidate_count: usize,
    every_legacy_canonical_candidate_present: bool,
    projection_ledger_complete: bool,
    executable_root_ledger_complete: bool,
    global_complete: bool,
    candidate_digest_sha1: String,
    target_mapper_migration: TargetMapperMigrationAudit,
}

#[derive(Clone, Debug)]
struct SourceWriterDeclaration {
    role: String,
    prg_bank: u8,
    cpu_address: u16,
    register_address: u16,
}

pub(super) fn audit_source_mapper_writes(source: &Rom) -> Result<SourceMapperWriteAudit> {
    source.verify_supported_japanese()?;
    let pages = source
        .prg()
        .chunks_exact(SOURCE_PRG_8K_PAGE_LEN)
        .enumerate()
        .map(|(physical_page_8k, bytes)| {
            Ok(PhysicalPrgPage {
                physical_page_8k: u16::try_from(physical_page_8k)
                    .context("source physical PRG page index overflow")?,
                bytes,
            })
        })
        .collect::<Result<Vec<_>>>()?;
    ensure!(
        pages.len() == SOURCE_PRG_8K_PAGE_COUNT,
        "source PRG page count changed: expected {SOURCE_PRG_8K_PAGE_COUNT}, found {}",
        pages.len()
    );
    let projections = source_mmc4_projections();
    let expected_projection_count = usize::from(SOURCE_PRG_BANK_COUNT) * 2 + 2;
    ensure!(
        projections.len() == expected_projection_count,
        "source MMC4 projection ledger does not cover both lower pages for every selector plus both fixed pages"
    );
    let scan = scan_all_byte_mapper_write_candidates(
        &pages,
        &projections,
        ProjectionLedgerCompleteness::Complete,
        decode_source_mmc4_write,
    )?;
    let candidate_digest_sha1 = mapper_write_candidate_digest(&scan);
    let declarations = declared_source_writers()?;
    let positive_execution = bind_source_positive_execution_graph(source)?;
    let target_mapper_migration =
        audit_target_mapper_migration(&pages, &projections, &declarations, &positive_execution)?;
    let declared_starts = bind_declared_starts(&scan, &declarations)?;
    let audio = bind_audio_record_data_region(source)?;
    let (audio_page, audio_offset) = physical_page_and_offset(audio.prg_bank, audio.cpu_address)?;
    let exact_data = [ExactBoundData {
        role: audio.role.to_owned(),
        physical_page_8k: audio_page,
        page_offset: audio_offset,
        expected_bytes: audio.expected_bytes.to_vec(),
    }];
    let rooted_instructions =
        bind_rooted_instruction_layout(&scan, &positive_execution.mapped_instruction_starts()?)?;
    let unrooted_instructions = bind_rooted_instruction_layout(&scan, &BTreeSet::new())?;
    let unrooted_partition = partition_mapper_write_candidates(
        &scan,
        &declared_starts,
        &unrooted_instructions,
        &exact_data,
    )?;
    let partition: MapperWriteCandidatePartition = partition_mapper_write_candidates(
        &scan,
        &declared_starts,
        &rooted_instructions,
        &exact_data,
    )?;

    ensure!(
        partition.declared_executable_starts.len() == declarations.len(),
        "source mapper writer declaration partition lost or duplicated a bound writer"
    );
    ensure!(
        rooted_instructions.instruction_count() == positive_execution.instruction_count(),
        "source positive execution graph lost an instruction while rebinding exact spans"
    );
    ensure!(
        rooted_instructions.start_interior_conflicts().is_empty(),
        "source rooted execution graph contains instruction starts that are also interiors: {:?}",
        rooted_instructions.start_interior_conflicts()
    );
    let classified_as_positive_instruction_interiors = unrooted_partition
        .unresolved
        .difference(&partition.unresolved)
        .cloned()
        .collect::<BTreeSet<_>>();
    ensure!(
        classified_as_positive_instruction_interiors == partition.rooted_instruction_interiors,
        "source positive execution graph changed candidates outside exact instruction interiors"
    );
    ensure!(
        partition
            .unresolved
            .is_subset(&unrooted_partition.unresolved),
        "source positive execution graph introduced a new unresolved candidate"
    );
    ensure!(
        partition.exact_bound_data == unrooted_partition.exact_bound_data,
        "positive execution classification changed exact source-data ownership"
    );
    ensure!(
        !partition.is_global_closed(),
        "source mapper write audit must remain open until the executable-root ledger is complete"
    );
    ensure!(
        partition.require_global_closed().is_err(),
        "source mapper write audit unexpectedly produced a global closure proof"
    );

    let legacy = legacy_canonical_chr_write_candidates(source)?;
    for candidate in &legacy {
        let location = source_mapped_location(candidate.prg_bank, candidate.cpu_address)?;
        let matches = scan
            .candidates
            .iter()
            .filter(|scanned| {
                scanned.start() == &location
                    && decoded_candidate_matches(scanned, candidate.opcode, candidate.register)
            })
            .count();
        ensure!(
            matches == 1,
            "legacy canonical candidate bank {:02X}:${:04X} matched {matches} root-independent candidates",
            candidate.prg_bank,
            candidate.cpu_address
        );
    }

    let boundary_unresolved_candidate_count = scan
        .candidates
        .iter()
        .filter(|candidate| {
            matches!(
                candidate,
                MapperWriteCandidate::BoundaryBytesUnresolved { .. }
            )
        })
        .count();
    Ok(SourceMapperWriteAudit {
        candidate_scope: "every byte offset in every declared source MMC4 PRG projection, decoded with RP2A03 StaticSemantics against all MMC4 register aliases",
        closure_claim: "partial: the physical-page and projection denominator is complete; positive battle, main-dialogue, NMI, and audio instruction spans classify only their exact instruction interiors, while source-structural writer declarations do not by themselves establish reachability; counts and the candidate digest are diagnostic outputs rather than closure proofs; the whole-program executable-root ledger is incomplete and every remaining possible start stays unresolved",
        physical_prg_page_count: pages.len(),
        mapped_projection_count: projections.len(),
        mapper_write_candidate_count: scan.candidates.len(),
        declared_source_writer_count: partition.declared_executable_starts.len(),
        positive_execution_instruction_count: rooted_instructions.instruction_count(),
        rooted_instruction_interior_candidate_count: partition.rooted_instruction_interiors.len(),
        rooted_start_interior_conflict_count: rooted_instructions.start_interior_conflicts().len(),
        unresolved_before_positive_execution_count: unrooted_partition.unresolved.len(),
        exact_bound_data_candidate_count: partition.exact_bound_data.len(),
        unresolved_candidate_count: partition.unresolved.len(),
        boundary_unresolved_candidate_count,
        legacy_canonical_candidate_count: legacy.len(),
        every_legacy_canonical_candidate_present: true,
        projection_ledger_complete: partition.projection_ledger_complete,
        executable_root_ledger_complete: partition.executable_root_ledger_complete,
        global_complete: partition.is_global_closed(),
        candidate_digest_sha1,
        target_mapper_migration,
    })
}

fn source_mmc4_projections() -> Vec<MappedPrgProjection> {
    let mut projections = Vec::with_capacity(34);
    for bank in 0..SOURCE_PRG_BANK_COUNT {
        let lower_8000 = lower_projection_role(bank, 0x8000);
        let lower_a000 = lower_projection_role(bank, 0xA000);
        projections.push(MappedPrgProjection {
            role: lower_8000,
            physical_page_8k: u16::from(bank) * 2,
            cpu_start: 0x8000,
            boundary_successors: BoundarySuccessorCoverage::Complete(vec![lower_a000.clone()]),
        });
        projections.push(MappedPrgProjection {
            role: lower_a000,
            physical_page_8k: u16::from(bank) * 2 + 1,
            cpu_start: 0xA000,
            boundary_successors: BoundarySuccessorCoverage::Complete(vec![
                FIXED_C000_PROJECTION.to_owned(),
            ]),
        });
    }
    projections.push(MappedPrgProjection {
        role: FIXED_C000_PROJECTION.to_owned(),
        physical_page_8k: FIXED_C000_PAGE,
        cpu_start: 0xC000,
        boundary_successors: BoundarySuccessorCoverage::Complete(vec![
            FIXED_E000_PROJECTION.to_owned(),
        ]),
    });
    projections.push(MappedPrgProjection {
        role: FIXED_E000_PROJECTION.to_owned(),
        physical_page_8k: FIXED_E000_PAGE,
        cpu_start: 0xE000,
        boundary_successors: BoundarySuccessorCoverage::Unresolved,
    });
    projections
}

fn declared_source_writers() -> Result<Vec<SourceWriterDeclaration>> {
    let mut declarations = Vec::new();
    declarations.extend(SOURCE_PRG_BANK_WRITERS.iter().map(writer_declaration));
    declarations.push(SourceWriterDeclaration {
        role: "central PRG bank selector".to_owned(),
        prg_bank: FIXED_PRG_BANK,
        cpu_address: SOURCE_SELECT_PRG_BANK_AND_SAVE_ADDRESS + 4,
        register_address: 0xA000,
    });
    declarations.extend(DIRECT_CHR_WRITERS.iter().map(writer_declaration));
    for writer in CENTRAL_CHR_WRITERS {
        declarations.push(SourceWriterDeclaration {
            role: writer.role.to_owned(),
            prg_bank: FIXED_PRG_BANK,
            cpu_address: writer
                .source_address
                .checked_add(4)
                .context("central CHR writer start overflow")?,
            register_address: writer.source_register,
        });
    }
    for (role, routine_start) in [
        (
            "horizontal mirroring selector",
            SOURCE_SELECT_HORIZONTAL_MIRRORING_ADDRESS,
        ),
        (
            "vertical mirroring selector",
            SOURCE_SELECT_VERTICAL_MIRRORING_ADDRESS,
        ),
    ] {
        declarations.push(SourceWriterDeclaration {
            role: role.to_owned(),
            prg_bank: FIXED_PRG_BANK,
            cpu_address: routine_start + 4,
            register_address: 0xF000,
        });
    }
    Ok(declarations)
}

fn writer_declaration(writer: &super::writer_sites::DirectWriter) -> SourceWriterDeclaration {
    SourceWriterDeclaration {
        role: writer.role.to_owned(),
        prg_bank: match writer.location {
            WriterLocation::Fixed => FIXED_PRG_BANK,
            WriterLocation::Switchable { prg_bank } => prg_bank,
        },
        cpu_address: writer.source_address,
        register_address: writer.source_register,
    }
}

fn bind_declared_starts(
    scan: &AllByteMapperWriteScan<SourceMmc4Register>,
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
                        && decoded_candidate_matches(candidate, 0x8D, declaration.register_address)
                })
                .collect::<Vec<_>>();
            ensure!(
                matches.len() == 1,
                "declared source writer {} bank {:02X}:${:04X} matched {} root-independent candidates",
                declaration.role,
                declaration.prg_bank,
                declaration.cpu_address,
                matches.len()
            );
            Ok(DeclaredExecutableStart {
                role: format!(
                    "{} at bank {:02X}:${:04X}",
                    declaration.role, declaration.prg_bank, declaration.cpu_address
                ),
                candidate: matches[0].id().clone(),
            })
        })
        .collect()
}

fn decoded_candidate_matches(
    candidate: &MapperWriteCandidate<SourceMmc4Register>,
    opcode: u8,
    register_address: u16,
) -> bool {
    let expected_register = decode_source_mmc4_write(register_address);
    matches!(
        candidate,
        MapperWriteCandidate::Decoded {
            opcode: actual_opcode,
            accesses,
            ..
        } if *actual_opcode == opcode
            && accesses.iter().any(|access| matches!(
                access,
                MapperWriteAccess::Direct { address, register }
                    if *address == register_address && Some(*register) == expected_register
            ))
    )
}

fn source_mapped_location(prg_bank: u8, cpu_address: u16) -> Result<MappedPrgLocation> {
    ensure!(
        prg_bank < SOURCE_PRG_BANK_COUNT,
        "source PRG bank {prg_bank:02X} is outside the MMC4 bank selector range"
    );
    let (projection_role, physical_page_8k) = match cpu_address {
        0x8000..=0x9FFF => (
            lower_projection_role(prg_bank, 0x8000),
            u16::from(prg_bank) * 2,
        ),
        0xA000..=0xBFFF => (
            lower_projection_role(prg_bank, 0xA000),
            u16::from(prg_bank) * 2 + 1,
        ),
        0xC000..=0xDFFF => {
            ensure!(
                prg_bank == FIXED_PRG_BANK,
                "fixed C000 projection requires source bank 0F"
            );
            (FIXED_C000_PROJECTION.to_owned(), FIXED_C000_PAGE)
        }
        0xE000..=0xFFFF => {
            ensure!(
                prg_bank == FIXED_PRG_BANK,
                "fixed E000 projection requires source bank 0F"
            );
            (FIXED_E000_PROJECTION.to_owned(), FIXED_E000_PAGE)
        }
        _ => anyhow::bail!("source PRG address ${cpu_address:04X} is outside MMC4 PRG space"),
    };
    Ok(MappedPrgLocation {
        projection_role,
        physical_page_8k,
        cpu_address,
    })
}

fn physical_page_and_offset(prg_bank: u8, cpu_address: u16) -> Result<(u16, u16)> {
    let location = source_mapped_location(prg_bank, cpu_address)?;
    let cpu_start = match cpu_address {
        0x8000..=0x9FFF => 0x8000,
        0xA000..=0xBFFF => 0xA000,
        0xC000..=0xDFFF => 0xC000,
        0xE000..=0xFFFF => 0xE000,
        _ => unreachable!("source_mapped_location rejected non-PRG address"),
    };
    Ok((location.physical_page_8k, cpu_address - cpu_start))
}

fn lower_projection_role(prg_bank: u8, cpu_start: u16) -> String {
    format!("source-bank-{prg_bank:02X}-{cpu_start:04X}")
}

fn mapper_write_candidate_digest<R: std::fmt::Debug>(scan: &AllByteMapperWriteScan<R>) -> String {
    let mut canonical = String::new();
    for candidate in &scan.candidates {
        canonical.push_str(&format!("{candidate:?}\n"));
    }
    sha1_hex(canonical.as_bytes())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::mapper165::executable_mapper_writes::{
        CandidateDecodeVariant, MapperWriteCandidateId,
    };

    #[test]
    fn source_projection_ledger_covers_every_lower_page_and_both_fixed_pages() {
        let projections = source_mmc4_projections();
        assert_eq!(projections.len(), 34);
        for page in 0..SOURCE_PRG_8K_PAGE_COUNT as u16 {
            assert!(
                projections
                    .iter()
                    .any(|projection| projection.physical_page_8k == page)
            );
        }
        let bank_0f_a000 = projections
            .iter()
            .find(|projection| projection.role == lower_projection_role(0x0F, 0xA000))
            .unwrap();
        assert_eq!(bank_0f_a000.physical_page_8k, FIXED_E000_PAGE);
        assert_eq!(
            bank_0f_a000.boundary_successors,
            BoundarySuccessorCoverage::Complete(vec![FIXED_C000_PROJECTION.to_owned()])
        );
        let fixed_e000 = projections
            .iter()
            .find(|projection| projection.role == FIXED_E000_PROJECTION)
            .unwrap();
        assert_eq!(
            fixed_e000.boundary_successors,
            BoundarySuccessorCoverage::Unresolved
        );
    }

    #[test]
    fn bank_0f_lower_and_fixed_views_keep_distinct_projection_identities() {
        let lower = source_mapped_location(0x0F, 0xA123).unwrap();
        let fixed = source_mapped_location(0x0F, 0xE123).unwrap();
        assert_eq!(lower.physical_page_8k, fixed.physical_page_8k);
        assert_ne!(lower.projection_role, fixed.projection_role);
        assert_eq!(lower.projection_role, lower_projection_role(0x0F, 0xA000));
        assert_eq!(fixed.projection_role, FIXED_E000_PROJECTION);
    }

    #[test]
    fn source_projection_rejects_a_bank_outside_the_four_bit_selector() {
        assert!(source_mapped_location(0x10, 0x8000).is_err());
    }

    #[test]
    fn source_structural_writer_ledger_has_one_owner_per_distinct_start() {
        let declarations = declared_source_writers().unwrap();
        assert!(!declarations.is_empty());
        let starts = declarations
            .iter()
            .map(|writer| (writer.prg_bank, writer.cpu_address))
            .collect::<std::collections::BTreeSet<_>>();
        assert_eq!(starts.len(), declarations.len());
    }

    #[test]
    fn audio_record_address_projects_to_bank_0e_first_physical_page() {
        assert_eq!(
            physical_page_and_offset(0x0E, 0x9A7A).unwrap(),
            (0x1C, 0x1A7A)
        );
    }

    #[test]
    fn mapped_boundary_writer_identity_names_the_fixed_successor_variant() {
        let lower_start = source_mapped_location(0x05, 0xBFFF).unwrap();
        let id = MapperWriteCandidateId {
            start: lower_start,
            decode_variant: CandidateDecodeVariant::MappedSuccessor {
                projection_role: FIXED_C000_PROJECTION.to_owned(),
            },
        };
        assert_eq!(id.start.physical_page_8k, 0x0B);
        assert_eq!(id.start.cpu_address, 0xBFFF);
    }
}
