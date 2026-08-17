use std::{collections::BTreeMap, ops::RangeInclusive};

use anyhow::{Context, Result, ensure};
use retro_rp2a03::{AddressingMode, Location, MemoryAddress, Operand, Rp2A03, decode_bytes};
use typed_isa_core::{AccessKind, StaticSemantics};

use crate::{rom::Rom, sha1_hex, typed_source::decode_rp2a03_sequence};

use super::control_state::PENDING_SHARED_MENU_REQUEST_STATE;

const FIXED_PRG_BANK: u8 = 0x0F;
const PRG_BANK_BYTE_COUNT: usize = 16 * 1024;

struct TypedRegion {
    bank: u8,
    start: u16,
    end: u16,
    expected_sha1: &'static str,
    role: &'static str,
}

const TYPED_REGIONS: [TypedRegion; 10] = [
    TypedRegion {
        bank: 0x04,
        start: 0x843F,
        end: 0x8456,
        expected_sha1: "0b3544c4e4abaa87c66fb184a840d2ac97e15763",
        role: "normalize thirty-two map-sprite cells",
    },
    TypedRegion {
        bank: 0x0D,
        start: 0x80B9,
        end: 0x80C6,
        expected_sha1: "39bb71be3eb7c58a1d120dbb2ea3e44a882ff542",
        role: "clear the bounded class-profile workspace",
    },
    TypedRegion {
        bank: 0x0D,
        start: 0x80E2,
        end: 0x80FF,
        expected_sha1: "aab610097dc296447ef439bbdfe63d580dfc1f87",
        role: "copy one bounded class-profile layout",
    },
    TypedRegion {
        bank: 0x0D,
        start: 0x8271,
        end: 0x82BB,
        expected_sha1: "ce3f7aeea3f6b52d904642183ac14539457d8533",
        role: "compose sixteen class-profile row cells",
    },
    TypedRegion {
        bank: 0x0D,
        start: 0x85F3,
        end: 0x8613,
        expected_sha1: "0c558b250b0b2aa628bc746e63c2bc76c296f696",
        role: "advance bounded class-profile animation cells",
    },
    TypedRegion {
        bank: 0x0D,
        start: 0x8628,
        end: 0x8662,
        expected_sha1: "7b26a1b1d4a0fe05ab54c1305ec1fbfb73cd8efd",
        role: "retreat bounded class-profile animation cells",
    },
    TypedRegion {
        bank: 0x0D,
        start: 0x8699,
        end: 0x86D9,
        expected_sha1: "c0881b5a33951b95a229081c9151610c6dddc994",
        role: "restore bounded class-profile animation cells",
    },
    TypedRegion {
        bank: 0x0D,
        start: 0x86D9,
        end: 0x86FE,
        expected_sha1: "ad9055437d0237ecf4f44aafb656a655e71fbb81",
        role: "copy four class-profile frame bytes",
    },
    TypedRegion {
        bank: 0x0D,
        start: 0xAC34,
        end: 0xAC3E,
        expected_sha1: "425051cfb962e7be86fb682503ca368b7b7e1e41",
        role: "initialize three title animation cells",
    },
    TypedRegion {
        bank: 0x0D,
        start: 0xAE0D,
        end: 0xAE3B,
        expected_sha1: "d721f1910bf6283c219dc779f1f9e9fc1b99ee59",
        role: "swap thirty-one title animation cells",
    },
];

struct WriterSpec {
    bank: u8,
    address: u16,
    mode: AddressingMode,
    base: u16,
    role: &'static str,
    destination_ranges: &'static [(u16, u16)],
}

const WRITERS: [WriterSpec; 16] = [
    writer(
        0x04,
        0x844E,
        AddressingMode::AbsoluteX,
        0x04DB,
        "map-sprite cells",
        &[(0x04DB, 0x04FA)],
    ),
    writer(
        0x0D,
        0x80BD,
        AddressingMode::AbsoluteY,
        0x0550,
        "class-profile workspace clear",
        &[(0x0550, 0x05B4)],
    ),
    writer(
        0x0D,
        0x80F2,
        AddressingMode::AbsoluteY,
        0x04D8,
        "class-profile layout copy",
        &[(0x04D8, 0x04FA)],
    ),
    writer(
        0x0D,
        0x8289,
        AddressingMode::AbsoluteX,
        0x0591,
        "class-profile row clear",
        &[(0x0591, 0x05A0)],
    ),
    writer(
        0x0D,
        0x8292,
        AddressingMode::AbsoluteX,
        0x0591,
        "class-profile row increment",
        &[(0x0591, 0x05A0)],
    ),
    writer(
        0x0D,
        0x82A5,
        AddressingMode::AbsoluteX,
        0x0591,
        "class-profile first row cell",
        &[(0x0591, 0x05A0)],
    ),
    writer(
        0x0D,
        0x82AD,
        AddressingMode::AbsoluteX,
        0x0591,
        "class-profile second row cell",
        &[(0x0591, 0x05A0)],
    ),
    writer(
        0x0D,
        0x85FB,
        AddressingMode::AbsoluteY,
        0x04DB,
        "class-profile leading animation cell",
        &[(0x04DC, 0x04DC)],
    ),
    writer(
        0x0D,
        0x860B,
        AddressingMode::AbsoluteY,
        0x04DB,
        "class-profile four-cell animation row",
        &[(0x04EB, 0x04EE)],
    ),
    writer(
        0x0D,
        0x865E,
        AddressingMode::AbsoluteY,
        0x04DB,
        "class-profile retreat cells",
        &[(0x04DC, 0x04DC), (0x04EB, 0x04EE)],
    ),
    writer(
        0x0D,
        0x86AD,
        AddressingMode::AbsoluteY,
        0x04DB,
        "class-profile restored cells",
        &[(0x04EB, 0x04EE)],
    ),
    writer(
        0x0D,
        0x86F5,
        AddressingMode::AbsoluteY,
        0x0571,
        "class-profile frame bytes",
        &[(0x0571, 0x0574)],
    ),
    writer(
        0x0D,
        0xAC38,
        AddressingMode::AbsoluteY,
        0x04DB,
        "title initialization cells",
        &[(0x04DC, 0x04DE)],
    ),
    writer(
        0x0D,
        0xAE14,
        AddressingMode::AbsoluteX,
        0x0591,
        "title saved cells",
        &[(0x0592, 0x05B0)],
    ),
    writer(
        0x0D,
        0xAE1E,
        AddressingMode::AbsoluteX,
        0x04DB,
        "title blanked cells",
        &[(0x04DC, 0x04FA)],
    ),
    writer(
        0x0D,
        0xAE35,
        AddressingMode::AbsoluteX,
        0x04DB,
        "title restored cells",
        &[(0x04DC, 0x04FA)],
    ),
];

const fn writer(
    bank: u8,
    address: u16,
    mode: AddressingMode,
    base: u16,
    role: &'static str,
    destination_ranges: &'static [(u16, u16)],
) -> WriterSpec {
    WriterSpec {
        bank,
        address,
        mode,
        base,
        role,
        destination_ranges,
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(super) struct AbsoluteIndexedWriteDestinationBounds {
    role: &'static str,
    destination_ranges: Vec<RangeInclusive<u16>>,
}

impl AbsoluteIndexedWriteDestinationBounds {
    pub(super) fn role(&self) -> &'static str {
        self.role
    }

    pub(super) fn destination_ranges(&self) -> &[RangeInclusive<u16>] {
        &self.destination_ranges
    }

    #[cfg(test)]
    pub(super) fn for_synthetic_test(
        role: &'static str,
        destination_ranges: Vec<RangeInclusive<u16>>,
    ) -> Self {
        Self {
            role,
            destination_ranges,
        }
    }
}

pub(super) fn bind_pending_request_disjoint_indexed_writes(
    source: &Rom,
) -> Result<BTreeMap<(u8, u16), AbsoluteIndexedWriteDestinationBounds>> {
    for region in &TYPED_REGIONS {
        let bytes = source_bytes(
            source,
            region.bank,
            region.start,
            usize::from(region.end - region.start),
        )?;
        ensure!(
            sha1_hex(bytes) == region.expected_sha1,
            "{} source bytes changed",
            region.role
        );
        decode_rp2a03_sequence(bytes, region.start, region.role)?;
    }

    let mut bounds = BTreeMap::new();
    for spec in &WRITERS {
        let instruction = decode_bytes(source_bytes(source, spec.bank, spec.address, 3)?)
            .with_context(|| {
                format!(
                    "decode {} at {:02X}:${:04X}",
                    spec.role, spec.bank, spec.address
                )
            })?;
        let semantics = Rp2A03::semantics(&instruction, &spec.address)
            .expect("RP2A03 static semantics are infallible");
        ensure!(
            semantics.location_accesses.into_iter().any(|access| {
                access.kind == AccessKind::Write
                    && access.location
                        == Location::Memory(MemoryAddress::Effective {
                            mode: spec.mode,
                            operand: Operand::Word(spec.base),
                        })
            }),
            "{} no longer writes through {:?} base ${:04X}",
            spec.role,
            spec.mode,
            spec.base,
        );
        let destination_ranges = spec
            .destination_ranges
            .iter()
            .map(|&(start, end)| start..=end)
            .collect::<Vec<_>>();
        ensure!(
            destination_ranges.iter().all(|range| {
                range.start() <= range.end()
                    && *range.end() < 0x8000
                    && !range.contains(&PENDING_SHARED_MENU_REQUEST_STATE)
                    && (0..=u8::MAX)
                        .any(|index| spec.base.wrapping_add(u16::from(index)) == *range.start())
                    && (0..=u8::MAX)
                        .any(|index| spec.base.wrapping_add(u16::from(index)) == *range.end())
            }),
            "{} destination bounds are invalid or reach pending request state $05CC",
            spec.role,
        );
        ensure!(
            destination_ranges
                .windows(2)
                .all(|pair| pair[0].end() < pair[1].start()),
            "{} destination bounds overlap or are unordered",
            spec.role,
        );
        ensure!(
            bounds
                .insert(
                    (spec.bank, spec.address),
                    AbsoluteIndexedWriteDestinationBounds {
                        role: spec.role,
                        destination_ranges,
                    }
                )
                .is_none(),
            "duplicate bounded absolute-indexed writer at {:02X}:${:04X}",
            spec.bank,
            spec.address,
        );
    }
    ensure!(
        bounds.len() == WRITERS.len(),
        "bounded absolute-indexed writer count changed"
    );
    Ok(bounds)
}

fn source_bytes(source: &Rom, bank: u8, address: u16, byte_count: usize) -> Result<&[u8]> {
    let physical_bank = if address >= 0xC000 {
        FIXED_PRG_BANK
    } else {
        bank
    };
    let cpu_start = if address >= 0xC000 { 0xC000 } else { 0x8000 };
    let offset = usize::from(physical_bank)
        .checked_mul(PRG_BANK_BYTE_COUNT)
        .and_then(|base| base.checked_add(usize::from(address - cpu_start)))
        .context("absolute-indexed writer source offset overflow")?;
    source
        .prg()
        .get(offset..offset + byte_count)
        .with_context(|| {
            format!("absolute-indexed writer source range exceeds PRG at {bank:02X}:${address:04X}")
        })
}
