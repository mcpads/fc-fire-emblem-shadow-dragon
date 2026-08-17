use anyhow::{Context, Result, ensure};

use crate::{rom::Rom, sha1_hex, typed_source::decode_rp2a03_sequence};

const FIXED_PRG_BANK: u8 = 0x0F;
const PRG_BANK_BYTE_COUNT: usize = 16 * 1024;

struct TypedRegion {
    bank: u8,
    start: u16,
    end: u16,
    expected_sha1: &'static str,
    role: &'static str,
}

const TYPED_REGIONS: [TypedRegion; 16] = [
    TypedRegion {
        bank: 0x06,
        start: 0xAC70,
        end: 0xAD2D,
        expected_sha1: "5f8013e39e648d5783a4490641996dbac2420474",
        role: "project shared-menu records into map work RAM",
    },
    TypedRegion {
        bank: 0x06,
        start: 0xB8F1,
        end: 0xB96D,
        expected_sha1: "d2099030107a4089e45647b3ad10fe48c8b4c62f",
        role: "project shared-menu pairs into map work RAM",
    },
    TypedRegion {
        bank: 0x0B,
        start: 0x8E3C,
        end: 0x8E6B,
        expected_sha1: "ecadac0d1b51d3ba4f445c5c6fbc32cbcfdcd77c",
        role: "initialize a shared-menu request",
    },
    TypedRegion {
        bank: 0x0B,
        start: 0x9265,
        end: 0x93D8,
        expected_sha1: "89420c6f225400271b6fe209db5130a1500983b9",
        role: "advance shared-menu request states one through five",
    },
    TypedRegion {
        bank: 0x0B,
        start: 0x93E0,
        end: 0x93F0,
        expected_sha1: "84f66bcba557d405491819c2a3c5a527923e37d8",
        role: "select one shared-menu record",
    },
    TypedRegion {
        bank: 0x0B,
        start: 0x93FA,
        end: 0x942B,
        expected_sha1: "e80e2fe5c93cbf52f3fe9171f376976c71562a2c",
        role: "populate one shared-menu record",
    },
    TypedRegion {
        bank: 0x0B,
        start: 0x942B,
        end: 0x948B,
        expected_sha1: "7c9cd61d47b6a8fbc9e91104542ef48e7d9ed5c7",
        role: "derive shared-menu vertical and horizontal spans",
    },
    TypedRegion {
        bank: 0x0B,
        start: 0x9500,
        end: 0x952B,
        expected_sha1: "a9896526260e0907df25901dee1c1f61d4f89ee7",
        role: "write and advance shared-menu row markers",
    },
    TypedRegion {
        bank: 0x0B,
        start: 0x95B0,
        end: 0x9608,
        expected_sha1: "198f7fd4a8e08779e60d139569a5c27a605b99b7",
        role: "normalize and copy shared-menu PPU queue payloads",
    },
    TypedRegion {
        bank: 0x0B,
        start: 0x98C7,
        end: 0x992D,
        expected_sha1: "73164ff5829b316eab693d75f9b49b57b1a4ef54",
        role: "copy shared-menu screen cells into one cache slot",
    },
    TypedRegion {
        bank: 0x0B,
        start: 0x97F7,
        end: 0x9812,
        expected_sha1: "c0dcc69c82d2aecbaae57ac8d0cb19d4932f3c62",
        role: "advance one shared-menu horizontal cell",
    },
    TypedRegion {
        bank: 0x0B,
        start: 0x9840,
        end: 0x984D,
        expected_sha1: "de347e1c07d2e6c16043a2150283667ac2063645",
        role: "count shared-menu selection bits",
    },
    TypedRegion {
        bank: 0x0B,
        start: 0x86D0,
        end: 0x871C,
        expected_sha1: "aa1df0341d0f6b71415ed1a5c9fa0f168ecd8ca8",
        role: "derive a shared-menu row count from selection bits",
    },
    TypedRegion {
        bank: FIXED_PRG_BANK,
        start: 0xC81C,
        end: 0xC842,
        expected_sha1: "ae1d0cb8efbe5355583e9e5c68f4113a8a78c20e",
        role: "advance one shared-menu vertical cell",
    },
    TypedRegion {
        bank: FIXED_PRG_BANK,
        start: 0xC8CD,
        end: 0xC95E,
        expected_sha1: "5c27ee5ee8e3a15d7780921ff57581a54036bd25",
        role: "serialize a bounded shared-menu PPU queue command",
    },
    TypedRegion {
        bank: FIXED_PRG_BANK,
        start: 0xE65C,
        end: 0xE690,
        expected_sha1: "893baab352cce1749f26bb216da93ea6f724b91a",
        role: "publish a pending shared-menu request to bank eleven",
    },
];

pub(super) fn bind_source_regions(source: &Rom) -> Result<()> {
    for region in &TYPED_REGIONS {
        let byte_count = usize::from(region.end - region.start);
        let bytes = source_bytes(source, region.bank, region.start, byte_count)?;
        ensure!(
            sha1_hex(bytes) == region.expected_sha1,
            "{} source bytes changed",
            region.role
        );
        decode_rp2a03_sequence(bytes, region.start, region.role)?;
    }
    Ok(())
}

pub(super) fn bind_request_state_landmarks(source: &Rom) -> Result<()> {
    let landmarks: [(u8, u16, &[u8], &str); 6] = [
        (
            0x0B,
            0x8E3C,
            &[0xA9, 0x01, 0x8D, 0xCC, 0x05],
            "shared-menu state-one producer",
        ),
        (
            0x0B,
            0x929E,
            &[0xEE, 0xCC, 0x05],
            "shared-menu state-one-to-two transition",
        ),
        (
            FIXED_PRG_BANK,
            0xE66E,
            &[
                0xAD, 0xCE, 0x05, 0xF0, 0x10, 0x8D, 0xCD, 0x05, 0x8D, 0xD3, 0x05, 0xA9, 0x00, 0x8D,
                0xD4, 0x05, 0xA9, 0x03, 0x8D, 0xCC, 0x05, 0x60,
            ],
            "shared-menu state-three producer",
        ),
        (
            0x0B,
            0x92F7,
            &[0xEE, 0xCC, 0x05],
            "shared-menu state-three-to-four transition",
        ),
        (
            0x0B,
            0x92B9,
            &[0xA9, 0x05, 0x8D, 0xCC, 0x05],
            "shared-menu state-two-to-five transition",
        ),
        (
            0x0B,
            0x9315,
            &[0xA9, 0x05, 0x8D, 0xCC, 0x05],
            "shared-menu state-four-to-five transition",
        ),
    ];
    for (bank, address, expected, role) in landmarks {
        ensure!(
            source_bytes(source, bank, address, expected.len())? == expected,
            "{role} changed"
        );
        decode_rp2a03_sequence(expected, address, role)?;
    }
    Ok(())
}

pub(super) fn source_bytes(
    source: &Rom,
    bank: u8,
    address: u16,
    byte_count: usize,
) -> Result<&[u8]> {
    let relative = if address >= 0xC000 {
        ensure!(
            bank == FIXED_PRG_BANK,
            "fixed shared-menu source region uses a non-fixed physical bank"
        );
        usize::from(address - 0xC000)
    } else {
        ensure!(
            bank < FIXED_PRG_BANK && address >= 0x8000,
            "switchable shared-menu source region is outside source PRG space"
        );
        usize::from(address - 0x8000)
    };
    let physical_bank = if address >= 0xC000 {
        FIXED_PRG_BANK
    } else {
        bank
    };
    let start = usize::from(physical_bank)
        .checked_mul(PRG_BANK_BYTE_COUNT)
        .and_then(|base| base.checked_add(relative))
        .context("shared-menu source offset overflow")?;
    let end = start
        .checked_add(byte_count)
        .context("shared-menu source range overflow")?;
    source.prg().get(start..end).with_context(|| {
        format!("shared-menu source range exceeds PRG at {bank:02X}:${address:04X}")
    })
}
