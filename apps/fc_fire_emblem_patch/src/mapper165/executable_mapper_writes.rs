mod all_byte_candidates;
#[cfg(test)]
mod analyzer;
mod candidate_partition;
mod hardware_decode;
#[cfg(test)]
mod mapped_program;
mod rooted_instruction_layout;

pub(crate) use all_byte_candidates::{
    AllByteMapperWriteScan, BoundarySuccessorCoverage, MappedPrgLocation, MappedPrgProjection,
    MapperWriteAccess, MapperWriteCandidate, PhysicalPrgPage, ProjectionLedgerCompleteness,
    scan_all_byte_mapper_write_candidates,
};
#[cfg(test)]
pub(crate) use all_byte_candidates::{CandidateDecodeVariant, MapperWriteCandidateId};
#[cfg(test)]
pub(crate) use analyzer::{
    DirectMapperWrite, ExecutableMapperWriteAnalyzer, UnresolvedControlEdge,
    UnresolvedExecutableFact,
};
pub(crate) use candidate_partition::{
    DeclaredExecutableStart, ExactBoundData, MapperWriteCandidatePartition,
    partition_mapper_write_candidates,
};
pub(crate) use hardware_decode::{Mapper165Register, decode_mapper165_write};
#[cfg(test)]
pub(crate) use hardware_decode::{MapperHardware, MapperRegister};
pub(crate) use hardware_decode::{SourceMmc4Register, decode_source_mmc4_write};
#[cfg(test)]
pub(crate) use mapped_program::{
    CodeLocation, DirectCodeBinding, ExecutableProgram, ExecutableRegion, SequentialCodeBoundary,
};
#[cfg(test)]
pub(crate) use rooted_instruction_layout::RootedInstructionLayout;
pub(crate) use rooted_instruction_layout::bind_rooted_instruction_layout;

#[cfg(test)]
mod tests;
