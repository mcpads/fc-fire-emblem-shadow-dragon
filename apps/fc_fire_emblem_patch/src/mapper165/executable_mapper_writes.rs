#[cfg(test)]
mod analyzer;
mod hardware_decode;
#[cfg(test)]
mod mapped_program;

#[cfg(test)]
pub(crate) use analyzer::{
    DirectMapperWrite, ExecutableMapperWriteAnalyzer, UnresolvedControlEdge,
    UnresolvedExecutableFact,
};
pub(crate) use hardware_decode::{Mapper165Register, decode_mapper165_write};
#[cfg(test)]
pub(crate) use hardware_decode::{
    MapperHardware, MapperRegister, SourceMmc4Register, decode_source_mmc4_write,
};
#[cfg(test)]
pub(crate) use mapped_program::{
    CodeLocation, DirectCodeBinding, ExecutableProgram, ExecutableRegion, SequentialCodeBoundary,
};

#[cfg(test)]
mod tests;
