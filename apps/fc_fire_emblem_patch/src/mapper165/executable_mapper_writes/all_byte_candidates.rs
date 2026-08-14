use std::{
    collections::{BTreeMap, BTreeSet},
    fmt::Debug,
};

use anyhow::{Context, Result, ensure};
use retro_rp2a03::{AddressingMode, Location, MemoryAddress, Operand, Rp2A03, decode_bytes};
use typed_isa_core::{AccessKind, StaticSemantics};

pub(crate) const PHYSICAL_PRG_PAGE_BYTE_COUNT: usize = 0x2000;

#[derive(Clone, Copy, Debug)]
pub(crate) struct PhysicalPrgPage<'a> {
    pub(crate) physical_page_8k: u16,
    pub(crate) bytes: &'a [u8],
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum ProjectionLedgerCompleteness {
    Complete,
    Incomplete,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) enum BoundarySuccessorCoverage {
    Complete(Vec<String>),
    Unresolved,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct MappedPrgProjection {
    pub(crate) role: String,
    pub(crate) physical_page_8k: u16,
    pub(crate) cpu_start: u16,
    pub(crate) boundary_successors: BoundarySuccessorCoverage,
}

#[derive(Clone, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub(crate) struct MappedPrgLocation {
    pub(crate) projection_role: String,
    pub(crate) physical_page_8k: u16,
    pub(crate) cpu_address: u16,
}

#[derive(Clone, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub(crate) enum CandidateDecodeVariant {
    WithinProjection,
    MappedSuccessor { projection_role: String },
    UnresolvedBoundary,
}

#[derive(Clone, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub(crate) struct MapperWriteCandidateId {
    pub(crate) start: MappedPrgLocation,
    pub(crate) decode_variant: CandidateDecodeVariant,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) enum MapperWriteAccess<R> {
    Direct {
        address: u16,
        register: R,
    },
    Effective {
        mode: AddressingMode,
        operand: Operand,
        possible_registers: Vec<R>,
    },
    RuntimeDerived {
        possible_registers: Vec<R>,
    },
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) enum MapperWriteCandidate<R> {
    Decoded {
        id: MapperWriteCandidateId,
        byte_locations: Vec<MappedPrgLocation>,
        opcode: u8,
        opcode_is_documented: bool,
        accesses: Vec<MapperWriteAccess<R>>,
    },
    BoundaryBytesUnresolved {
        id: MapperWriteCandidateId,
        opcode: u8,
        expected_byte_count: usize,
        available_bytes: Vec<u8>,
    },
}

impl<R> MapperWriteCandidate<R> {
    pub(crate) fn id(&self) -> &MapperWriteCandidateId {
        match self {
            Self::Decoded { id, .. } | Self::BoundaryBytesUnresolved { id, .. } => id,
        }
    }

    pub(crate) fn start(&self) -> &MappedPrgLocation {
        &self.id().start
    }

    pub(crate) fn byte_locations(&self) -> &[MappedPrgLocation] {
        match self {
            Self::Decoded { byte_locations, .. } => byte_locations,
            Self::BoundaryBytesUnresolved { .. } => std::slice::from_ref(self.start()),
        }
    }
}

#[derive(Clone, Debug)]
pub(crate) struct AllByteMapperWriteScan<R> {
    pub(crate) projection_ledger_completeness: ProjectionLedgerCompleteness,
    pub(crate) candidates: Vec<MapperWriteCandidate<R>>,
    pub(super) page_snapshots: BTreeMap<u16, Vec<u8>>,
    pub(super) projections: BTreeMap<String, MappedPrgProjection>,
}

pub(crate) fn scan_all_byte_mapper_write_candidates<R, F>(
    pages: &[PhysicalPrgPage<'_>],
    projections: &[MappedPrgProjection],
    projection_ledger_completeness: ProjectionLedgerCompleteness,
    decode_mapper_write: F,
) -> Result<AllByteMapperWriteScan<R>>
where
    R: Clone + Debug + Eq + Ord,
    F: Fn(u16) -> Option<R>,
{
    let page_snapshots = validate_pages(pages)?;
    let projection_map = validate_projections(&page_snapshots, projections)?;
    let register_universe = (0..=u16::MAX)
        .filter_map(&decode_mapper_write)
        .collect::<BTreeSet<_>>()
        .into_iter()
        .collect::<Vec<_>>();
    ensure!(
        !register_universe.is_empty(),
        "mapper write decoder recognizes no CPU register addresses"
    );

    let mut candidates = Vec::new();
    for projection in projections {
        let page = page_snapshots
            .get(&projection.physical_page_8k)
            .expect("validated projection page exists");
        for page_offset in 0..PHYSICAL_PRG_PAGE_BYTE_COUNT {
            let cpu_address = projection
                .cpu_start
                .checked_add(u16::try_from(page_offset)?)
                .context("mapped PRG CPU address overflow")?;
            let start = MappedPrgLocation {
                projection_role: projection.role.clone(),
                physical_page_8k: projection.physical_page_8k,
                cpu_address,
            };
            let available_end = (page_offset + 3).min(PHYSICAL_PRG_PAGE_BYTE_COUNT);
            let available = &page[page_offset..available_end];
            match decode_bytes(available) {
                Ok(instruction) => {
                    if let Some(candidate) = decoded_candidate(
                        MapperWriteCandidateId {
                            start: start.clone(),
                            decode_variant: CandidateDecodeVariant::WithinProjection,
                        },
                        &instruction,
                        instruction_locations_within_projection(
                            projection,
                            cpu_address,
                            instruction.encoded_len(),
                        )?,
                        &register_universe,
                        &decode_mapper_write,
                    )? {
                        candidates.push(candidate);
                    }
                }
                Err(retro_rp2a03::DecodeError::Truncated { expected, .. }) => {
                    match &projection.boundary_successors {
                        BoundarySuccessorCoverage::Complete(successor_roles) => {
                            for successor_role in successor_roles {
                                let successor = projection_map
                                    .get(successor_role)
                                    .expect("validated successor projection exists");
                                let successor_page = page_snapshots
                                    .get(&successor.physical_page_8k)
                                    .expect("validated successor page exists");
                                let (bytes, locations) = boundary_instruction_bytes(
                                    projection,
                                    page,
                                    page_offset,
                                    successor,
                                    successor_page,
                                    expected,
                                )?;
                                let instruction = decode_bytes(&bytes).with_context(|| {
                                    format!(
                                        "decode mapped boundary instruction at {} ${cpu_address:04X}",
                                        projection.role
                                    )
                                })?;
                                if let Some(candidate) = decoded_candidate(
                                    MapperWriteCandidateId {
                                        start: start.clone(),
                                        decode_variant: CandidateDecodeVariant::MappedSuccessor {
                                            projection_role: successor.role.clone(),
                                        },
                                    },
                                    &instruction,
                                    locations,
                                    &register_universe,
                                    &decode_mapper_write,
                                )? {
                                    candidates.push(candidate);
                                }
                            }
                        }
                        BoundarySuccessorCoverage::Unresolved => {
                            if unresolved_completion_may_write_mapper(
                                available,
                                expected,
                                &register_universe,
                                &decode_mapper_write,
                            )? {
                                candidates.push(MapperWriteCandidate::BoundaryBytesUnresolved {
                                    id: MapperWriteCandidateId {
                                        start,
                                        decode_variant: CandidateDecodeVariant::UnresolvedBoundary,
                                    },
                                    opcode: available[0],
                                    expected_byte_count: expected,
                                    available_bytes: available.to_vec(),
                                });
                            }
                        }
                    }
                }
                Err(retro_rp2a03::DecodeError::Empty) => {
                    unreachable!("every mapped page offset contains an opcode byte")
                }
            }
        }
    }

    candidates.sort_by(|left, right| left.id().cmp(right.id()));
    ensure!(
        candidates
            .windows(2)
            .all(|pair| pair[0].id() != pair[1].id()),
        "root-independent mapper write scan produced duplicate candidate identities"
    );
    Ok(AllByteMapperWriteScan {
        projection_ledger_completeness,
        candidates,
        page_snapshots,
        projections: projection_map,
    })
}

fn validate_pages(pages: &[PhysicalPrgPage<'_>]) -> Result<BTreeMap<u16, Vec<u8>>> {
    ensure!(
        !pages.is_empty(),
        "mapped PRG image contains no physical pages"
    );
    let mut snapshots = BTreeMap::new();
    for page in pages {
        ensure!(
            page.bytes.len() == PHYSICAL_PRG_PAGE_BYTE_COUNT,
            "physical PRG page {} contains {} bytes, expected {}",
            page.physical_page_8k,
            page.bytes.len(),
            PHYSICAL_PRG_PAGE_BYTE_COUNT
        );
        ensure!(
            snapshots
                .insert(page.physical_page_8k, page.bytes.to_vec())
                .is_none(),
            "physical PRG page {} is duplicated",
            page.physical_page_8k
        );
    }
    Ok(snapshots)
}

fn validate_projections(
    pages: &BTreeMap<u16, Vec<u8>>,
    projections: &[MappedPrgProjection],
) -> Result<BTreeMap<String, MappedPrgProjection>> {
    ensure!(
        !projections.is_empty(),
        "mapped PRG image has no projections"
    );
    let mut projection_map = BTreeMap::new();
    let mut projected_pages = BTreeSet::new();
    for projection in projections {
        ensure!(
            !projection.role.is_empty(),
            "mapped PRG projection role is empty"
        );
        ensure!(
            projection.cpu_start & 0x1FFF == 0,
            "mapped PRG projection {} starts at unaligned CPU address ${:04X}",
            projection.role,
            projection.cpu_start
        );
        ensure!(
            pages.contains_key(&projection.physical_page_8k),
            "mapped PRG projection {} names missing physical page {}",
            projection.role,
            projection.physical_page_8k
        );
        projected_pages.insert(projection.physical_page_8k);
        ensure!(
            projection_map
                .insert(projection.role.clone(), projection.clone())
                .is_none(),
            "mapped PRG projection role {} is duplicated",
            projection.role
        );
    }
    ensure!(
        projected_pages.len() == pages.len(),
        "at least one physical PRG page has no mapped projection"
    );

    for projection in projections {
        if let BoundarySuccessorCoverage::Complete(successor_roles) =
            &projection.boundary_successors
        {
            ensure!(
                !successor_roles.is_empty(),
                "projection {} claims complete boundary coverage without a successor; use unresolved coverage until non-execution is rooted",
                projection.role
            );
            let mut unique = BTreeSet::new();
            for successor_role in successor_roles {
                ensure!(
                    unique.insert(successor_role),
                    "projection {} repeats boundary successor {}",
                    projection.role,
                    successor_role
                );
                let successor = projection_map.get(successor_role).ok_or_else(|| {
                    anyhow::anyhow!(
                        "projection {} names unknown boundary successor {}",
                        projection.role,
                        successor_role
                    )
                })?;
                ensure!(
                    successor.cpu_start == projection.cpu_start.wrapping_add(0x2000),
                    "projection {} boundary successor {} starts at ${:04X}, expected ${:04X}",
                    projection.role,
                    successor.role,
                    successor.cpu_start,
                    projection.cpu_start.wrapping_add(0x2000)
                );
            }
        }
    }
    Ok(projection_map)
}

fn instruction_locations_within_projection(
    projection: &MappedPrgProjection,
    cpu_address: u16,
    byte_count: usize,
) -> Result<Vec<MappedPrgLocation>> {
    (0..byte_count)
        .map(|offset| {
            Ok(MappedPrgLocation {
                projection_role: projection.role.clone(),
                physical_page_8k: projection.physical_page_8k,
                cpu_address: cpu_address
                    .checked_add(u16::try_from(offset)?)
                    .context("instruction CPU range overflow")?,
            })
        })
        .collect()
}

fn boundary_instruction_bytes(
    projection: &MappedPrgProjection,
    page: &[u8],
    page_offset: usize,
    successor: &MappedPrgProjection,
    successor_page: &[u8],
    expected: usize,
) -> Result<(Vec<u8>, Vec<MappedPrgLocation>)> {
    let current_count = PHYSICAL_PRG_PAGE_BYTE_COUNT - page_offset;
    ensure!(current_count < expected, "boundary decode is not truncated");
    let successor_count = expected - current_count;
    ensure!(
        successor_count <= successor_page.len(),
        "boundary instruction exceeds one successor page"
    );
    let mut bytes = page[page_offset..].to_vec();
    bytes.extend_from_slice(&successor_page[..successor_count]);
    let mut locations = (0..current_count)
        .map(|offset| MappedPrgLocation {
            projection_role: projection.role.clone(),
            physical_page_8k: projection.physical_page_8k,
            cpu_address: projection
                .cpu_start
                .wrapping_add(u16::try_from(page_offset + offset).unwrap()),
        })
        .collect::<Vec<_>>();
    locations.extend((0..successor_count).map(|offset| {
        MappedPrgLocation {
            projection_role: successor.role.clone(),
            physical_page_8k: successor.physical_page_8k,
            cpu_address: successor
                .cpu_start
                .wrapping_add(u16::try_from(offset).unwrap()),
        }
    }));
    Ok((bytes, locations))
}

fn decoded_candidate<R, F>(
    id: MapperWriteCandidateId,
    instruction: &retro_rp2a03::Instruction,
    byte_locations: Vec<MappedPrgLocation>,
    register_universe: &[R],
    decode_mapper_write: &F,
) -> Result<Option<MapperWriteCandidate<R>>>
where
    R: Clone + Debug + Eq + Ord,
    F: Fn(u16) -> Option<R>,
{
    let accesses = mapper_write_accesses(
        instruction,
        id.start.cpu_address,
        register_universe,
        decode_mapper_write,
    )?;
    Ok(
        (!accesses.is_empty()).then(|| MapperWriteCandidate::Decoded {
            id,
            byte_locations,
            opcode: instruction.opcode(),
            opcode_is_documented: instruction.opcode_is_documented(),
            accesses,
        }),
    )
}

fn mapper_write_accesses<R, F>(
    instruction: &retro_rp2a03::Instruction,
    cpu_address: u16,
    register_universe: &[R],
    decode_mapper_write: &F,
) -> Result<Vec<MapperWriteAccess<R>>>
where
    R: Clone + Debug + Eq + Ord,
    F: Fn(u16) -> Option<R>,
{
    let semantics = Rp2A03::semantics(instruction, &cpu_address)
        .expect("RP2A03 static semantics are infallible");
    let mut accesses = Vec::new();
    for access in semantics.location_accesses {
        if access.kind != AccessKind::Write {
            continue;
        }
        let Location::Memory(memory) = access.location else {
            continue;
        };
        match memory {
            MemoryAddress::Direct(address) => {
                if let Some(register) = decode_mapper_write(address) {
                    accesses.push(MapperWriteAccess::Direct { address, register });
                }
            }
            MemoryAddress::Effective { mode, operand } => {
                let possible_registers = possible_effective_registers(
                    mode,
                    operand,
                    register_universe,
                    decode_mapper_write,
                );
                if !possible_registers.is_empty() {
                    accesses.push(MapperWriteAccess::Effective {
                        mode,
                        operand,
                        possible_registers,
                    });
                }
            }
            MemoryAddress::Stack => {
                let possible_registers = (0x0100..=0x01FF)
                    .filter_map(decode_mapper_write)
                    .collect::<BTreeSet<_>>()
                    .into_iter()
                    .collect::<Vec<_>>();
                if !possible_registers.is_empty() {
                    accesses.push(MapperWriteAccess::RuntimeDerived { possible_registers });
                }
            }
            MemoryAddress::Pointer { .. } | MemoryAddress::InterruptVector => {
                accesses.push(MapperWriteAccess::RuntimeDerived {
                    possible_registers: register_universe.to_vec(),
                });
            }
        }
    }
    Ok(accesses)
}

fn possible_effective_registers<R, F>(
    mode: AddressingMode,
    operand: Operand,
    register_universe: &[R],
    decode_mapper_write: &F,
) -> Vec<R>
where
    R: Clone + Eq + Ord,
    F: Fn(u16) -> Option<R>,
{
    let addresses: Option<Box<dyn Iterator<Item = u16>>> = match (mode, operand) {
        (AddressingMode::AbsoluteX | AddressingMode::AbsoluteY, Operand::Word(base)) => Some(
            Box::new((0..=u8::MAX).map(move |index| base.wrapping_add(u16::from(index)))),
        ),
        (AddressingMode::ZeroPageX | AddressingMode::ZeroPageY, Operand::Byte(base)) => Some(
            Box::new((0..=u8::MAX).map(move |index| u16::from(base.wrapping_add(index)))),
        ),
        (
            AddressingMode::ZeroPageIndexedIndirectX | AddressingMode::ZeroPageIndirectIndexedY,
            _,
        ) => return register_universe.to_vec(),
        _ => return register_universe.to_vec(),
    };
    addresses
        .expect("effective address iterator exists")
        .filter_map(decode_mapper_write)
        .collect::<BTreeSet<_>>()
        .into_iter()
        .collect()
}

fn unresolved_completion_may_write_mapper<R, F>(
    available: &[u8],
    expected: usize,
    register_universe: &[R],
    decode_mapper_write: &F,
) -> Result<bool>
where
    R: Clone + Debug + Eq + Ord,
    F: Fn(u16) -> Option<R>,
{
    ensure!(
        available.len() < expected,
        "boundary completion is already complete"
    );
    let missing = expected - available.len();
    ensure!(
        missing <= 2,
        "RP2A03 instruction needs more than two boundary bytes"
    );
    let completion_count = 1_usize << (missing * 8);
    for completion in 0..completion_count {
        let mut bytes = available.to_vec();
        for index in 0..missing {
            bytes.push(((completion >> (index * 8)) & 0xFF) as u8);
        }
        let instruction = decode_bytes(&bytes).context("decode completed boundary instruction")?;
        if !mapper_write_accesses(&instruction, 0, register_universe, decode_mapper_write)?
            .is_empty()
        {
            return Ok(true);
        }
    }
    Ok(false)
}
