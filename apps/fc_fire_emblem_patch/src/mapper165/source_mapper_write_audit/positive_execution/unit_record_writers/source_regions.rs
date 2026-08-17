use anyhow::{Context, Result, ensure};
use retro_rp2a03::{AddressingMode, Mnemonic, Operand, decode_bytes};

use crate::{rom::Rom, sha1_hex, typed_source::decode_rp2a03_sequence};

const SOURCE_PRG_BANK_BYTE_COUNT: usize = 16 * 1024;
const FIXED_PRG_BANK: u8 = 0x0F;
const RUNTIME_ROW_POINTER_TABLE: u16 = 0xED3D;
const RUNTIME_ROW_POINTER_TABLE_SHA1: &str = "bbbdd2a6b72e5eeab51a8606d8f4f59309a19305";
const MAP_LAYER_ROW_POINTER_TABLE: u16 = 0xED01;
const MAP_LAYER_ROW_POINTER_TABLE_SHA1: &str = "a5e8e50d82f281dc2307e403458e7c7c8f2a279e";
const MAP_LAYER_ROW_POINTERS: [u16; 30] = [
    0x7730, 0x7750, 0x7770, 0x7790, 0x77B0, 0x77D0, 0x77F0, 0x7810, 0x7830, 0x7850, 0x7870, 0x7890,
    0x78B0, 0x78D0, 0x78F0, 0x7910, 0x7930, 0x7950, 0x7970, 0x7990, 0x79B0, 0x79D0, 0x79F0, 0x7A10,
    0x7A30, 0x7A50, 0x7A70, 0x7A90, 0x7AB0, 0x7AD0,
];
const RUNTIME_ROW_POINTERS: [u16; 32] = [
    0x72AF, 0x72CF, 0x72EF, 0x730F, 0x732F, 0x734F, 0x736F, 0x738F, 0x73AF, 0x73CF, 0x73EF, 0x740F,
    0x742F, 0x744F, 0x746F, 0x748F, 0x74AF, 0x74CF, 0x74EF, 0x750F, 0x752F, 0x754F, 0x756F, 0x758F,
    0x75AF, 0x75CF, 0x75EF, 0x760F, 0x762F, 0x764F, 0x7AF0, 0x7B10,
];

pub(super) struct UnitRecordWriterSource {
    pub(super) runtime_row_pointers: Vec<u16>,
    pub(super) map_layer_row_pointers: Vec<u16>,
}

struct TypedRegion {
    bank: u8,
    start: u16,
    end: u16,
    sha1: &'static str,
    role: &'static str,
}

impl TypedRegion {
    const fn new(bank: u8, start: u16, end: u16, sha1: &'static str, role: &'static str) -> Self {
        Self {
            bank,
            start,
            end,
            sha1,
            role,
        }
    }
}

struct SourceInstruction {
    bank: u8,
    address: u16,
    mnemonic: Mnemonic,
    mode: AddressingMode,
    operand: Operand,
}

impl SourceInstruction {
    const fn immediate(bank: u8, address: u16, mnemonic: Mnemonic, value: u8) -> Self {
        Self::new(
            bank,
            address,
            mnemonic,
            AddressingMode::Immediate,
            Operand::Byte(value),
        )
    }

    const fn zero_page(bank: u8, address: u16, mnemonic: Mnemonic, value: u8) -> Self {
        Self::new(
            bank,
            address,
            mnemonic,
            AddressingMode::ZeroPage,
            Operand::Byte(value),
        )
    }

    const fn indirect_indexed_y(bank: u8, address: u16, mnemonic: Mnemonic, pointer: u8) -> Self {
        Self::new(
            bank,
            address,
            mnemonic,
            AddressingMode::ZeroPageIndirectIndexedY,
            Operand::Byte(pointer),
        )
    }

    const fn absolute(bank: u8, address: u16, mnemonic: Mnemonic, operand: u16) -> Self {
        Self::new(
            bank,
            address,
            mnemonic,
            AddressingMode::Absolute,
            Operand::Word(operand),
        )
    }

    const fn absolute_indexed_y(bank: u8, address: u16, mnemonic: Mnemonic, operand: u16) -> Self {
        Self::new(
            bank,
            address,
            mnemonic,
            AddressingMode::AbsoluteY,
            Operand::Word(operand),
        )
    }

    const fn absolute_indexed_x(bank: u8, address: u16, mnemonic: Mnemonic, operand: u16) -> Self {
        Self::new(
            bank,
            address,
            mnemonic,
            AddressingMode::AbsoluteX,
            Operand::Word(operand),
        )
    }

    const fn accumulator(bank: u8, address: u16, mnemonic: Mnemonic) -> Self {
        Self::new(
            bank,
            address,
            mnemonic,
            AddressingMode::Accumulator,
            Operand::None,
        )
    }

    const fn implied(bank: u8, address: u16, mnemonic: Mnemonic) -> Self {
        Self::new(
            bank,
            address,
            mnemonic,
            AddressingMode::Implied,
            Operand::None,
        )
    }

    const fn new(
        bank: u8,
        address: u16,
        mnemonic: Mnemonic,
        mode: AddressingMode,
        operand: Operand,
    ) -> Self {
        Self {
            bank,
            address,
            mnemonic,
            mode,
            operand,
        }
    }
}

const TYPED_REGIONS: &[TypedRegion] = &[
    TypedRegion::new(
        0x06,
        0x845B,
        0x8470,
        "834c77a711f69d974738ab8fac354612c1c4d535",
        "clear the allied and enemy unit-record workspace",
    ),
    TypedRegion::new(
        FIXED_PRG_BANK,
        0xC225,
        0xC23D,
        "817ae96f8f6ac071de35ff0dfa6fbccceae8f022",
        "fill a bounded RAM range",
    ),
    TypedRegion::new(
        FIXED_PRG_BANK,
        0xF09E,
        0xF123,
        "edc5a9bd0adb8c018e81ba10c440aae4de1bcff0",
        "select an allied or enemy unit record by identity",
    ),
    TypedRegion::new(
        FIXED_PRG_BANK,
        0xF146,
        0xF181,
        "dec24bb87ed1732436421b0c3dcc4344f72d5a53",
        "select an inactive allied unit record within the roster domain",
    ),
    TypedRegion::new(
        0x02,
        0xAA0C,
        0xAA8F,
        "a651cc49ec23515ca00ed4f92c43bca7c3f86fe3",
        "shift fields after selecting an inactive allied unit record",
    ),
    TypedRegion::new(
        0x06,
        0x8641,
        0x867B,
        "ac179f0a79493f851c7e97ff95808a1a5864741f",
        "mark the first and identity-matched allied unit records as acted",
    ),
    TypedRegion::new(
        0x06,
        0x8890,
        0x8923,
        "7c0f561b42bac866c92379e598c9c37eff3f614a",
        "update selected allied unit action bytes",
    ),
    TypedRegion::new(
        0x06,
        0xA1D8,
        0xA228,
        "f92ca0902dd36c12c992be1e6aab80893aed8890",
        "refresh allied unit occupancy and action bytes",
    ),
    TypedRegion::new(
        0x06,
        0xA228,
        0xA253,
        "403389cb48c7e9db0d9bdb91d30da34690edb17c",
        "advance allied records and decrement the turn counter",
    ),
    TypedRegion::new(
        0x06,
        0xA253,
        0xA2B9,
        "34e7a759024f5a1aee66edda58a0bd8c03ffb41f",
        "rebuild an inactive allied unit record",
    ),
    TypedRegion::new(
        0x06,
        0xA91B,
        0xA937,
        "7db79ea87b3c4ee6a60453487782565a2342030f",
        "select a runtime occupancy cell from unit coordinates",
    ),
    TypedRegion::new(
        0x06,
        0xAD33,
        0xAD48,
        "290b1a2ea7bf6ed094238b5931db21373f46a983",
        "derive the runtime occupancy byte from one unit record",
    ),
    TypedRegion::new(
        0x06,
        0xB878,
        0xB8B3,
        "a9d213984dd75427b1f3d769063fcdf510bedc77",
        "publish selected unit fields into runtime map occupancy",
    ),
    TypedRegion::new(
        0x06,
        0xBB27,
        0xBB58,
        "2f5cd7035ed5df47f2377a5e305b110930ed917b",
        "clear source-bound map-layer rows",
    ),
    TypedRegion::new(
        0x06,
        0xBD48,
        0xBD5F,
        "94288f8f90d4b1a3b7397b766381898d28f3ff9e",
        "select one source-bound map-layer row",
    ),
    TypedRegion::new(
        0x08,
        0xBA93,
        0xBAA8,
        "a9cc2cf3448f0b0cea85f3e0f5885d240de93fcf",
        "copy derived map coordinates into a selected allied record",
    ),
    TypedRegion::new(
        0x08,
        0xBACC,
        0xBB10,
        "8baf51816d4747eb74c0627ee8eb48c6d4310d19",
        "select the allied unit-record destination",
    ),
    TypedRegion::new(
        0x08,
        0xBB10,
        0xBB85,
        "b8617a64b5880512bc3ab4255b3911439c78aed8",
        "copy one unit into the first available record",
    ),
    TypedRegion::new(
        0x08,
        0xBBC1,
        0xBBE8,
        "f6e9e80a2dcd1034cccc5a01e72055950966027a",
        "select the enemy unit-record destination",
    ),
];

const SOURCE_INSTRUCTIONS: &[SourceInstruction] = &[
    SourceInstruction::immediate(0x06, 0x845B, Mnemonic::Lda, 0x90),
    SourceInstruction::immediate(0x06, 0x845F, Mnemonic::Lda, 0x6A),
    SourceInstruction::immediate(0x06, 0x8463, Mnemonic::Lda, 0x1F),
    SourceInstruction::immediate(0x06, 0x8467, Mnemonic::Lda, 0x08),
    SourceInstruction::absolute(0x06, 0x846D, Mnemonic::Jsr, 0xC225),
    SourceInstruction::indirect_indexed_y(FIXED_PRG_BANK, 0xC22D, Mnemonic::Sta, 0x00),
    SourceInstruction::zero_page(FIXED_PRG_BANK, 0xF09E, Mnemonic::Sta, 0x02),
    SourceInstruction::immediate(FIXED_PRG_BANK, 0xF0A3, Mnemonic::Lda, 0x1B),
    SourceInstruction::immediate(FIXED_PRG_BANK, 0xF0A8, Mnemonic::Ldy, 0x00),
    SourceInstruction::immediate(FIXED_PRG_BANK, 0xF0B0, Mnemonic::Ldy, 0x12),
    SourceInstruction::immediate(FIXED_PRG_BANK, 0xF111, Mnemonic::Lda, 0x90),
    SourceInstruction::zero_page(FIXED_PRG_BANK, 0xF113, Mnemonic::Sta, 0x00),
    SourceInstruction::immediate(FIXED_PRG_BANK, 0xF115, Mnemonic::Lda, 0x6A),
    SourceInstruction::zero_page(FIXED_PRG_BANK, 0xF117, Mnemonic::Sta, 0x01),
    SourceInstruction::immediate(FIXED_PRG_BANK, 0xF11A, Mnemonic::Lda, 0x78),
    SourceInstruction::immediate(FIXED_PRG_BANK, 0xF11E, Mnemonic::Lda, 0x70),
    SourceInstruction::immediate(FIXED_PRG_BANK, 0xF146, Mnemonic::Lda, 0x1B),
    SourceInstruction::immediate(FIXED_PRG_BANK, 0xF151, Mnemonic::Ldy, 0x00),
    SourceInstruction::immediate(FIXED_PRG_BANK, 0xF159, Mnemonic::Ldy, 0x12),
    SourceInstruction::immediate(FIXED_PRG_BANK, 0xF169, Mnemonic::Ldx, 0x00),
    SourceInstruction::immediate(FIXED_PRG_BANK, 0xF16E, Mnemonic::Cpx, 0x36),
    SourceInstruction::absolute(0x06, 0x8641, Mnemonic::Jsr, 0xF111),
    SourceInstruction::immediate(0x06, 0x8644, Mnemonic::Ldy, 0x12),
    SourceInstruction::immediate(0x06, 0x8646, Mnemonic::Lda, 0x01),
    SourceInstruction::indirect_indexed_y(0x06, 0x8648, Mnemonic::Sta, 0x00),
    SourceInstruction::absolute(0x06, 0x865D, Mnemonic::Jsr, 0xF111),
    SourceInstruction::absolute(0x06, 0x8660, Mnemonic::Ldx, 0x05EA),
    SourceInstruction::absolute_indexed_x(0x06, 0x8663, Mnemonic::Lda, 0x7730),
    SourceInstruction::absolute(0x06, 0x8666, Mnemonic::Jsr, 0xF09E),
    SourceInstruction::immediate(0x06, 0x8669, Mnemonic::Ldy, 0x12),
    SourceInstruction::immediate(0x06, 0x866B, Mnemonic::Lda, 0x01),
    SourceInstruction::indirect_indexed_y(0x06, 0x866D, Mnemonic::Sta, 0x00),
    SourceInstruction::immediate(0x02, 0xAA0C, Mnemonic::Lda, 0x90),
    SourceInstruction::immediate(0x02, 0xAA10, Mnemonic::Lda, 0x6A),
    SourceInstruction::absolute(0x02, 0xAA14, Mnemonic::Jsr, 0xF151),
    SourceInstruction::zero_page(0x02, 0xAA20, Mnemonic::Sta, 0x65),
    SourceInstruction::immediate(0x02, 0xAA53, Mnemonic::Ldy, 0x47),
    SourceInstruction::indirect_indexed_y(0x02, 0xAA55, Mnemonic::Sta, 0x65),
    SourceInstruction::immediate(0x02, 0xAA59, Mnemonic::Ldy, 0x36),
    SourceInstruction::indirect_indexed_y(0x02, 0xAA5B, Mnemonic::Sta, 0x65),
    SourceInstruction::absolute(0x06, 0x88B6, Mnemonic::Jsr, 0xF111),
    SourceInstruction::absolute_indexed_y(0x06, 0x88B9, Mnemonic::Lda, 0x7731),
    SourceInstruction::absolute(0x06, 0x88BE, Mnemonic::Jsr, 0xF09E),
    SourceInstruction::immediate(0x06, 0x88C1, Mnemonic::Ldy, 0x12),
    SourceInstruction::indirect_indexed_y(0x06, 0x88C9, Mnemonic::Sta, 0x00),
    SourceInstruction::zero_page(0x06, 0x88D3, Mnemonic::Sta, 0x75),
    SourceInstruction::immediate(0x06, 0x88D5, Mnemonic::Ldy, 0x12),
    SourceInstruction::indirect_indexed_y(0x06, 0x88D9, Mnemonic::Sta, 0x74),
    SourceInstruction::immediate(0x06, 0x88DB, Mnemonic::Lda, 0x01),
    SourceInstruction::absolute(0x06, 0x88E1, Mnemonic::Jsr, 0xC9FA),
    SourceInstruction::immediate(0x06, 0x88E7, Mnemonic::Ldy, 0x06),
    SourceInstruction::indirect_indexed_y(0x06, 0x88E9, Mnemonic::Sta, 0x74),
    SourceInstruction::immediate(0x06, 0x88EB, Mnemonic::Ldy, 0x01),
    SourceInstruction::indirect_indexed_y(0x06, 0x88ED, Mnemonic::Lda, 0x74),
    SourceInstruction::accumulator(0x06, 0x88EF, Mnemonic::Asl),
    SourceInstruction::immediate(0x06, 0x88F0, Mnemonic::Ldy, 0x00),
    SourceInstruction::indirect_indexed_y(0x06, 0x88F2, Mnemonic::Sta, 0x00),
    SourceInstruction::absolute(0x06, 0xA1FD, Mnemonic::Jsr, 0xA91B),
    SourceInstruction::absolute(0x06, 0xA200, Mnemonic::Jsr, 0xAD33),
    SourceInstruction::indirect_indexed_y(0x06, 0xA205, Mnemonic::Sta, 0x00),
    SourceInstruction::indirect_indexed_y(0x06, 0xA20B, Mnemonic::Sta, 0x74),
    SourceInstruction::immediate(0x06, 0xA234, Mnemonic::Lda, 0x90),
    SourceInstruction::zero_page(0x06, 0xA236, Mnemonic::Sta, 0x74),
    SourceInstruction::immediate(0x06, 0xA238, Mnemonic::Lda, 0x6A),
    SourceInstruction::zero_page(0x06, 0xA23A, Mnemonic::Sta, 0x75),
    SourceInstruction::immediate(0x06, 0xA23C, Mnemonic::Ldy, 0x0F),
    SourceInstruction::indirect_indexed_y(0x06, 0xA247, Mnemonic::Sta, 0x74),
    SourceInstruction::absolute(0x06, 0xA26D, Mnemonic::Jsr, 0xF167),
    SourceInstruction::indirect_indexed_y(0x06, 0xA27C, Mnemonic::Sta, 0x00),
    SourceInstruction::indirect_indexed_y(0x06, 0xA2A7, Mnemonic::Sta, 0x00),
    SourceInstruction::absolute(0x06, 0xB87B, Mnemonic::Jsr, 0xA91B),
    SourceInstruction::indirect_indexed_y(0x06, 0xB884, Mnemonic::Sta, 0x00),
    SourceInstruction::absolute(0x06, 0xB8A0, Mnemonic::Jsr, 0xAD33),
    SourceInstruction::indirect_indexed_y(0x06, 0xB8A5, Mnemonic::Sta, 0x00),
    SourceInstruction::absolute(0x06, 0xBB2B, Mnemonic::Jsr, 0xBD48),
    SourceInstruction::indirect_indexed_y(0x06, 0xBB48, Mnemonic::Sta, 0x6C),
    SourceInstruction::absolute_indexed_x(0x06, 0xBD50, Mnemonic::Lda, 0xED01),
    SourceInstruction::zero_page(0x06, 0xBD53, Mnemonic::Sta, 0x6C),
    SourceInstruction::absolute_indexed_x(0x06, 0xBD55, Mnemonic::Lda, 0xED02),
    SourceInstruction::zero_page(0x06, 0xBD58, Mnemonic::Sta, 0x6D),
    SourceInstruction::immediate(0x08, 0xBA93, Mnemonic::Ldy, 0x00),
    SourceInstruction::immediate(0x08, 0xBA97, Mnemonic::Ldy, 0x10),
    SourceInstruction::indirect_indexed_y(0x08, 0xBA99, Mnemonic::Sta, 0x74),
    SourceInstruction::immediate(0x08, 0xBAA1, Mnemonic::Ldy, 0x11),
    SourceInstruction::indirect_indexed_y(0x08, 0xBAA3, Mnemonic::Sta, 0x74),
    SourceInstruction::immediate(0x08, 0xBACC, Mnemonic::Lda, 0x90),
    SourceInstruction::immediate(0x08, 0xBAD0, Mnemonic::Lda, 0x6A),
    SourceInstruction::absolute(0x08, 0xBAFB, Mnemonic::Jsr, 0xBB10),
    SourceInstruction::immediate(0x08, 0xBB1C, Mnemonic::Ldx, 0x00),
    SourceInstruction::immediate(0x08, 0xBB21, Mnemonic::Cpx, 0x36),
    SourceInstruction::immediate(0x08, 0xBB44, Mnemonic::Ldy, 0x00),
    SourceInstruction::absolute_indexed_y(0x08, 0xBB46, Mnemonic::Lda, 0x76F4),
    SourceInstruction::indirect_indexed_y(0x08, 0xBB49, Mnemonic::Sta, 0x74),
    SourceInstruction::immediate(0x08, 0xBB4C, Mnemonic::Cpy, 0x1B),
    SourceInstruction::indirect_indexed_y(0x08, 0xBB58, Mnemonic::Lda, 0x74),
    SourceInstruction::accumulator(0x08, 0xBB5A, Mnemonic::Asl),
    SourceInstruction::implied(0x08, 0xBB5B, Mnemonic::Tay),
    SourceInstruction::absolute_indexed_y(0x08, 0xBB5C, Mnemonic::Lda, 0xED3D),
    SourceInstruction::zero_page(0x08, 0xBB5F, Mnemonic::Sta, 0x00),
    SourceInstruction::absolute_indexed_y(0x08, 0xBB61, Mnemonic::Lda, 0xED3E),
    SourceInstruction::zero_page(0x08, 0xBB64, Mnemonic::Sta, 0x01),
    SourceInstruction::zero_page(0x08, 0xBB66, Mnemonic::Lda, 0x05),
    SourceInstruction::absolute(0x08, 0xBB68, Mnemonic::Jsr, 0xC379),
    SourceInstruction::immediate(0x08, 0xBB6B, Mnemonic::Ldy, 0x00),
    SourceInstruction::indirect_indexed_y(0x08, 0xBB6D, Mnemonic::Lda, 0x00),
    SourceInstruction::immediate(0x08, 0xBB6F, Mnemonic::Ldy, 0x06),
    SourceInstruction::indirect_indexed_y(0x08, 0xBB71, Mnemonic::Sta, 0x74),
    SourceInstruction::immediate(0x08, 0xBB73, Mnemonic::Ldy, 0x01),
    SourceInstruction::indirect_indexed_y(0x08, 0xBB75, Mnemonic::Lda, 0x74),
    SourceInstruction::accumulator(0x08, 0xBB77, Mnemonic::Asl),
    SourceInstruction::absolute(0x08, 0xBB78, Mnemonic::Ora, 0x76ED),
    SourceInstruction::immediate(0x08, 0xBB7B, Mnemonic::Ldy, 0x00),
    SourceInstruction::indirect_indexed_y(0x08, 0xBB7D, Mnemonic::Sta, 0x00),
    SourceInstruction::immediate(0x08, 0xBBC1, Mnemonic::Lda, 0x78),
    SourceInstruction::immediate(0x08, 0xBBC5, Mnemonic::Lda, 0x70),
    SourceInstruction::absolute(0x08, 0xBBD4, Mnemonic::Jsr, 0xBB10),
];

pub(super) fn bind_unit_record_writer_source(source: &Rom) -> Result<UnitRecordWriterSource> {
    source.verify_supported_japanese()?;
    for region in TYPED_REGIONS {
        let bytes = source_bytes(source, region.bank, region.start, region.end - region.start)?;
        ensure!(
            sha1_hex(bytes) == region.sha1,
            "{} source bytes changed",
            region.role
        );
        decode_rp2a03_sequence(bytes, region.start, region.role)?;
    }
    for instruction in SOURCE_INSTRUCTIONS {
        ensure_instruction(source, instruction)?;
    }

    let table = source_bytes(
        source,
        FIXED_PRG_BANK,
        RUNTIME_ROW_POINTER_TABLE,
        u16::try_from(RUNTIME_ROW_POINTERS.len() * 2)?,
    )?;
    ensure!(
        sha1_hex(table) == RUNTIME_ROW_POINTER_TABLE_SHA1,
        "runtime row pointer table changed"
    );
    let runtime_row_pointers = table
        .chunks_exact(2)
        .map(|bytes| u16::from_le_bytes([bytes[0], bytes[1]]))
        .collect::<Vec<_>>();
    ensure!(
        runtime_row_pointers == RUNTIME_ROW_POINTERS,
        "runtime row pointer values changed"
    );
    let map_layer_table = source_bytes(
        source,
        FIXED_PRG_BANK,
        MAP_LAYER_ROW_POINTER_TABLE,
        u16::try_from(MAP_LAYER_ROW_POINTERS.len() * 2)?,
    )?;
    ensure!(
        sha1_hex(map_layer_table) == MAP_LAYER_ROW_POINTER_TABLE_SHA1,
        "map-layer row pointer table changed"
    );
    let map_layer_row_pointers = map_layer_table
        .chunks_exact(2)
        .map(|bytes| u16::from_le_bytes([bytes[0], bytes[1]]))
        .collect::<Vec<_>>();
    ensure!(
        map_layer_row_pointers == MAP_LAYER_ROW_POINTERS,
        "map-layer row pointer values changed"
    );
    Ok(UnitRecordWriterSource {
        runtime_row_pointers,
        map_layer_row_pointers,
    })
}

fn ensure_instruction(source: &Rom, expected: &SourceInstruction) -> Result<()> {
    let bytes = source_bytes(source, expected.bank, expected.address, 3)?;
    let instruction = decode_bytes(bytes).with_context(|| {
        format!(
            "decode unit-record source instruction at {:02X}:${:04X}",
            expected.bank, expected.address
        )
    })?;
    ensure!(
        instruction.mnemonic() == expected.mnemonic
            && instruction.addressing_mode() == expected.mode
            && instruction.operand() == expected.operand,
        "unit-record source instruction changed at {:02X}:${:04X}",
        expected.bank,
        expected.address,
    );
    Ok(())
}

fn source_bytes(source: &Rom, bank: u8, address: u16, byte_count: u16) -> Result<&[u8]> {
    let relative = if address >= 0xC000 {
        ensure!(
            bank == FIXED_PRG_BANK,
            "fixed unit-record source region uses a non-fixed physical bank"
        );
        usize::from(address - 0xC000)
    } else {
        ensure!(
            bank < FIXED_PRG_BANK && address >= 0x8000,
            "switchable unit-record source region is outside source PRG space"
        );
        usize::from(address - 0x8000)
    };
    let physical_bank = if address >= 0xC000 {
        FIXED_PRG_BANK
    } else {
        bank
    };
    let start = usize::from(physical_bank)
        .checked_mul(SOURCE_PRG_BANK_BYTE_COUNT)
        .and_then(|base| base.checked_add(relative))
        .context("unit-record source offset overflow")?;
    let end = start
        .checked_add(usize::from(byte_count))
        .context("unit-record source range overflow")?;
    source.prg().get(start..end).with_context(|| {
        format!("unit-record source range exceeds PRG at {bank:02X}:${address:04X}")
    })
}
