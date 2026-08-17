use std::collections::{BTreeMap, BTreeSet};

use anyhow::{Context, Result, ensure};

use super::all_byte_candidates::{
    AllByteMapperWriteScan, CandidateDecodeVariant, MappedPrgLocation, MapperWriteCandidateId,
    ProjectionLedgerCompleteness,
};
use super::rooted_instruction_layout::RootedInstructionLayout;

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct DeclaredExecutableStart {
    pub(crate) role: String,
    pub(crate) candidate: MapperWriteCandidateId,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct ExactBoundData {
    pub(crate) role: String,
    pub(crate) physical_page_8k: u16,
    pub(crate) page_offset: u16,
    pub(crate) expected_bytes: Vec<u8>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct MapperWriteCandidatePartition {
    pub(crate) declared_executable_starts: BTreeSet<MapperWriteCandidateId>,
    pub(crate) rooted_instruction_interiors: BTreeSet<MapperWriteCandidateId>,
    pub(crate) exact_bound_data: BTreeSet<MapperWriteCandidateId>,
    pub(crate) unresolved: BTreeSet<MapperWriteCandidateId>,
    pub(crate) projection_ledger_complete: bool,
    pub(crate) executable_root_ledger_complete: bool,
}

impl MapperWriteCandidatePartition {
    pub(crate) fn is_global_closed(&self) -> bool {
        self.projection_ledger_complete
            && self.executable_root_ledger_complete
            && self.unresolved.is_empty()
    }

    pub(crate) fn require_global_closed(&self) -> Result<()> {
        ensure!(
            self.projection_ledger_complete,
            "mapper-write projection ledger is incomplete"
        );
        ensure!(
            self.executable_root_ledger_complete,
            "mapper-write executable-root ledger is incomplete"
        );
        ensure!(
            self.unresolved.is_empty(),
            "mapper-write possible-start ledger retains {} unresolved candidates",
            self.unresolved.len()
        );
        Ok(())
    }
}

pub(crate) fn partition_mapper_write_candidates<R>(
    scan: &AllByteMapperWriteScan<R>,
    declared_starts: &[DeclaredExecutableStart],
    rooted_instructions: &RootedInstructionLayout,
    bound_data: &[ExactBoundData],
) -> Result<MapperWriteCandidatePartition> {
    let candidates = scan
        .candidates
        .iter()
        .map(|candidate| (candidate.id().clone(), candidate))
        .collect::<BTreeMap<_, _>>();
    ensure!(
        candidates.len() == scan.candidates.len(),
        "mapper-write scan contains duplicate candidate identities"
    );

    let mut declared_by_id = BTreeMap::new();
    for declared in declared_starts {
        ensure!(
            !declared.role.is_empty(),
            "declared executable role is empty"
        );
        let candidate = candidates.get(&declared.candidate).ok_or_else(|| {
            anyhow::anyhow!(
                "declared executable {} names a missing mapper-write candidate {:?}",
                declared.role,
                declared.candidate
            )
        })?;
        ensure!(
            !matches!(
                declared.candidate.decode_variant,
                CandidateDecodeVariant::UnresolvedBoundary
            ),
            "declared executable {} starts at unresolved instruction bytes",
            declared.role
        );
        ensure!(
            declared_by_id
                .insert(declared.candidate.clone(), declared.role.as_str())
                .is_none(),
            "mapper-write candidate {:?} has duplicate executable owners",
            candidate.id()
        );
    }

    let data_bytes = validate_bound_data(scan, bound_data)?;
    reject_code_data_overlap(
        &candidates,
        &declared_by_id,
        rooted_instructions,
        &data_bytes,
    )?;

    let mut partition = MapperWriteCandidatePartition {
        declared_executable_starts: BTreeSet::new(),
        rooted_instruction_interiors: BTreeSet::new(),
        exact_bound_data: BTreeSet::new(),
        unresolved: BTreeSet::new(),
        projection_ledger_complete: scan.projection_ledger_completeness
            == ProjectionLedgerCompleteness::Complete,
        // No caller-supplied boolean may stand in for a rooted, bank-aware execution proof.
        // A later source-graph unit must supply sealed evidence before this can become true.
        executable_root_ledger_complete: false,
    };
    for (candidate_id, candidate) in &candidates {
        let declared = declared_by_id.contains_key(candidate_id);
        let rooted_interior = rooted_instructions
            .instruction_interiors()
            .contains(candidate.start());
        let page_offset = physical_page_offset(scan, candidate.start())?;
        let data = data_bytes.contains(&(candidate.start().physical_page_8k, page_offset));
        let category_count =
            usize::from(declared) + usize::from(rooted_interior) + usize::from(data);
        ensure!(
            category_count <= 1,
            "mapper-write candidate {candidate_id:?} belongs to multiple ownership categories"
        );
        if declared {
            partition
                .declared_executable_starts
                .insert(candidate_id.clone());
        } else if rooted_interior {
            partition
                .rooted_instruction_interiors
                .insert(candidate_id.clone());
        } else if data {
            partition.exact_bound_data.insert(candidate_id.clone());
        } else {
            partition.unresolved.insert(candidate_id.clone());
        }
    }
    let classified_count = partition.declared_executable_starts.len()
        + partition.rooted_instruction_interiors.len()
        + partition.exact_bound_data.len()
        + partition.unresolved.len();
    ensure!(
        classified_count == candidates.len(),
        "mapper-write possible-start partition is not total"
    );
    Ok(partition)
}

fn validate_bound_data<R>(
    scan: &AllByteMapperWriteScan<R>,
    bound_data: &[ExactBoundData],
) -> Result<BTreeSet<(u16, u16)>> {
    let mut occupied = BTreeSet::new();
    let mut roles = BTreeSet::new();
    for data in bound_data {
        ensure!(!data.role.is_empty(), "exact-bound data role is empty");
        ensure!(
            roles.insert(data.role.as_str()),
            "exact-bound data role {} is duplicated",
            data.role
        );
        ensure!(
            !data.expected_bytes.is_empty(),
            "exact-bound data {} has no expected bytes",
            data.role
        );
        let page = scan
            .page_snapshots
            .get(&data.physical_page_8k)
            .ok_or_else(|| {
                anyhow::anyhow!(
                    "exact-bound data {} names missing physical page {}",
                    data.role,
                    data.physical_page_8k
                )
            })?;
        let start = usize::from(data.page_offset);
        let end = start
            .checked_add(data.expected_bytes.len())
            .context("exact-bound data range overflow")?;
        ensure!(
            page.get(start..end) == Some(data.expected_bytes.as_slice()),
            "exact-bound data {} no longer matches physical page bytes",
            data.role
        );
        for offset in start..end {
            let key = (data.physical_page_8k, u16::try_from(offset)?);
            ensure!(
                occupied.insert(key),
                "exact-bound data regions overlap at physical page {} offset ${offset:04X}",
                data.physical_page_8k
            );
        }
    }
    Ok(occupied)
}

fn reject_code_data_overlap<R>(
    candidates: &BTreeMap<
        MapperWriteCandidateId,
        &super::all_byte_candidates::MapperWriteCandidate<R>,
    >,
    declared: &BTreeMap<MapperWriteCandidateId, &str>,
    rooted_instructions: &RootedInstructionLayout,
    data_bytes: &BTreeSet<(u16, u16)>,
) -> Result<()> {
    for candidate_id in declared.keys() {
        let candidate = candidates
            .get(candidate_id)
            .expect("declared candidate was validated");
        for location in candidate.byte_locations() {
            let page_offset = usize::from(location.cpu_address & 0x1FFF) as u16;
            ensure!(
                !data_bytes.contains(&(location.physical_page_8k, page_offset)),
                "declared executable candidate {candidate_id:?} overlaps exact-bound data"
            );
        }
    }
    for location in rooted_instructions.every_instruction_byte() {
        let page_offset = usize::from(location.cpu_address & 0x1FFF) as u16;
        ensure!(
            !data_bytes.contains(&(location.physical_page_8k, page_offset)),
            "rooted instruction byte {location:?} overlaps exact-bound data"
        );
    }
    Ok(())
}

fn physical_page_offset<R>(
    scan: &AllByteMapperWriteScan<R>,
    location: &MappedPrgLocation,
) -> Result<u16> {
    let projection = scan
        .projections
        .get(&location.projection_role)
        .ok_or_else(|| anyhow::anyhow!("candidate uses unknown projection"))?;
    ensure!(
        projection.physical_page_8k == location.physical_page_8k
            && location.cpu_address >= projection.cpu_start,
        "candidate location does not belong to its projection"
    );
    let offset = location.cpu_address - projection.cpu_start;
    ensure!(
        usize::from(offset) < super::all_byte_candidates::PHYSICAL_PRG_PAGE_BYTE_COUNT,
        "candidate location lies outside its projection page"
    );
    Ok(offset)
}
