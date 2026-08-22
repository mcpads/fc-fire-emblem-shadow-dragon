use retro_rp2a03::{AddressingMode, Instruction, Mnemonic, Operand, encode_bytes};

use super::super::{
    AllByteMapperWriteScan, BoundarySuccessorCoverage, MappedPrgProjection, PhysicalPrgPage,
    ProjectionLedgerCompleteness, SourceMmc4Register, decode_source_mmc4_write,
    scan_all_byte_mapper_write_candidates,
};

pub(super) const PAGE_LEN: usize = 0x2000;
pub(super) const KIL: u8 = 0x02;

pub(super) fn page_with(writes: &[(usize, &[u8])]) -> Vec<u8> {
    let mut page = vec![KIL; PAGE_LEN];
    for (offset, bytes) in writes {
        page[*offset..*offset + bytes.len()].copy_from_slice(bytes);
    }
    page
}

pub(super) fn source_scan(
    pages: &[Vec<u8>],
    projections: &[MappedPrgProjection],
    completeness: ProjectionLedgerCompleteness,
) -> AllByteMapperWriteScan<SourceMmc4Register> {
    let physical_pages = pages
        .iter()
        .enumerate()
        .map(|(index, bytes)| PhysicalPrgPage {
            physical_page_8k: u16::try_from(index).unwrap(),
            bytes,
        })
        .collect::<Vec<_>>();
    scan_all_byte_mapper_write_candidates(
        &physical_pages,
        projections,
        completeness,
        decode_source_mmc4_write,
    )
    .unwrap()
}

pub(super) fn one_projection(page: u16, cpu_start: u16) -> MappedPrgProjection {
    MappedPrgProjection {
        role: format!("page-{page}-at-{cpu_start:04X}"),
        physical_page_8k: page,
        cpu_start,
        boundary_successors: BoundarySuccessorCoverage::Unresolved,
    }
}

pub(super) fn typed_bytes(mnemonic: Mnemonic, mode: AddressingMode, operand: Operand) -> Vec<u8> {
    encode_bytes(&Instruction::new(mnemonic, mode, operand).unwrap()).unwrap()
}

pub(super) fn exact_bytes(opcode: u8, operand: Operand) -> Vec<u8> {
    encode_bytes(&Instruction::from_opcode(opcode, operand).unwrap()).unwrap()
}
