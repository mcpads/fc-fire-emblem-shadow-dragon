use std::collections::{BTreeMap, BTreeSet};

use anyhow::{Context, Result, ensure};
use retro_rp2a03::decode_bytes;

use super::all_byte_candidates::{
    AllByteMapperWriteScan, BoundarySuccessorCoverage, MappedPrgLocation,
    PHYSICAL_PRG_PAGE_BYTE_COUNT,
};

/// Exact instruction spans recovered from a positive, but not necessarily complete, execution
/// graph. This type never asserts that every executable root has been found.
#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct RootedInstructionLayout {
    instruction_spans: BTreeMap<MappedPrgLocation, Vec<MappedPrgLocation>>,
    instruction_interiors: BTreeSet<MappedPrgLocation>,
    start_interior_conflicts: BTreeSet<MappedPrgLocation>,
}

impl RootedInstructionLayout {
    pub(crate) fn instruction_count(&self) -> usize {
        self.instruction_spans.len()
    }

    pub(crate) fn instruction_interiors(&self) -> &BTreeSet<MappedPrgLocation> {
        &self.instruction_interiors
    }

    pub(crate) fn start_interior_conflicts(&self) -> &BTreeSet<MappedPrgLocation> {
        &self.start_interior_conflicts
    }

    pub(super) fn every_instruction_byte(&self) -> BTreeSet<MappedPrgLocation> {
        self.instruction_spans.values().flatten().cloned().collect()
    }
}

/// Re-decode every traced instruction start against the exact mapped-page snapshots used by the
/// possible-start scan. Instruction interiors are derived from those decoded spans; callers cannot
/// name an arbitrary byte as an interior.
pub(crate) fn bind_rooted_instruction_layout<R>(
    scan: &AllByteMapperWriteScan<R>,
    starts: &BTreeSet<MappedPrgLocation>,
) -> Result<RootedInstructionLayout> {
    let instruction_spans = starts
        .iter()
        .map(|start| Ok((start.clone(), decode_instruction_span(scan, start)?)))
        .collect::<Result<BTreeMap<_, _>>>()?;
    ensure!(
        instruction_spans.len() == starts.len(),
        "rooted instruction layout lost a traced instruction start"
    );

    let mut instruction_interiors = BTreeSet::new();
    let mut start_interior_conflicts = BTreeSet::new();
    for span in instruction_spans.values() {
        for location in span.iter().skip(1) {
            if starts.contains(location) {
                start_interior_conflicts.insert(location.clone());
            } else {
                instruction_interiors.insert(location.clone());
            }
        }
    }
    for conflict in &start_interior_conflicts {
        instruction_interiors.remove(conflict);
    }

    Ok(RootedInstructionLayout {
        instruction_spans,
        instruction_interiors,
        start_interior_conflicts,
    })
}

fn decode_instruction_span<R>(
    scan: &AllByteMapperWriteScan<R>,
    start: &MappedPrgLocation,
) -> Result<Vec<MappedPrgLocation>> {
    let projection = scan
        .projections
        .get(&start.projection_role)
        .with_context(|| {
            format!(
                "rooted instruction names unknown projection {}",
                start.projection_role
            )
        })?;
    ensure!(
        projection.physical_page_8k == start.physical_page_8k,
        "rooted instruction physical page does not match its projection"
    );
    ensure!(
        start.cpu_address >= projection.cpu_start,
        "rooted instruction starts before its projection"
    );
    let page_offset = usize::from(start.cpu_address - projection.cpu_start);
    ensure!(
        page_offset < PHYSICAL_PRG_PAGE_BYTE_COUNT,
        "rooted instruction starts after its projection"
    );
    let page = scan
        .page_snapshots
        .get(&projection.physical_page_8k)
        .expect("validated projection page exists");
    let available_end = (page_offset + 3).min(PHYSICAL_PRG_PAGE_BYTE_COUNT);
    let available = &page[page_offset..available_end];

    match decode_bytes(available) {
        Ok(instruction) => within_projection_locations(start, instruction.encoded_len()),
        Err(retro_rp2a03::DecodeError::Truncated { expected, .. }) => {
            let BoundarySuccessorCoverage::Complete(successor_roles) =
                &projection.boundary_successors
            else {
                anyhow::bail!(
                    "rooted instruction at {}:${:04X} crosses an unresolved mapping boundary",
                    start.projection_role,
                    start.cpu_address
                );
            };
            ensure!(
                successor_roles.len() == 1,
                "rooted instruction at {}:${:04X} has {} possible mapped successors; bind its bank state before admitting the span",
                start.projection_role,
                start.cpu_address,
                successor_roles.len()
            );
            let successor = scan
                .projections
                .get(&successor_roles[0])
                .expect("validated successor projection exists");
            let successor_page = scan
                .page_snapshots
                .get(&successor.physical_page_8k)
                .expect("validated successor page exists");
            let current_count = PHYSICAL_PRG_PAGE_BYTE_COUNT - page_offset;
            ensure!(
                current_count < expected,
                "rooted boundary instruction was not truncated"
            );
            let successor_count = expected - current_count;
            let mut bytes = page[page_offset..].to_vec();
            bytes.extend_from_slice(
                successor_page
                    .get(..successor_count)
                    .context("rooted instruction exceeds one successor page")?,
            );
            let instruction = decode_bytes(&bytes).context("decode rooted boundary instruction")?;
            ensure!(
                instruction.encoded_len() == expected,
                "rooted boundary instruction length changed while completing its bytes"
            );
            let mut locations = within_projection_locations(start, current_count)?;
            locations.extend((0..successor_count).map(|offset| MappedPrgLocation {
                projection_role: successor.role.clone(),
                physical_page_8k: successor.physical_page_8k,
                cpu_address: successor.cpu_start.wrapping_add(
                    u16::try_from(offset).expect("RP2A03 instruction length fits u16"),
                ),
            }));
            Ok(locations)
        }
        Err(retro_rp2a03::DecodeError::Empty) => {
            unreachable!("rooted instruction always has an opcode byte")
        }
    }
}

fn within_projection_locations(
    start: &MappedPrgLocation,
    byte_count: usize,
) -> Result<Vec<MappedPrgLocation>> {
    (0..byte_count)
        .map(|offset| {
            Ok(MappedPrgLocation {
                projection_role: start.projection_role.clone(),
                physical_page_8k: start.physical_page_8k,
                cpu_address: start
                    .cpu_address
                    .checked_add(u16::try_from(offset)?)
                    .context("rooted instruction CPU range overflow")?,
            })
        })
        .collect()
}
