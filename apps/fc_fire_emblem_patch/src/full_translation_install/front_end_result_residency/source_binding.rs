use std::collections::BTreeSet;

use anyhow::{Context, Result, ensure};

use crate::{
    mmc5_chr::switchable_bank_file_offset, rom::Rom, sha1_hex, typed_source::decode_rp2a03_sequence,
};

const SOURCE_PRG_BANK: u8 = 0x02;
const RESULT_INDEX_ADDRESS: u16 = 0x77F1;
const RESULT_DIRECTORY_ADDRESS: u16 = 0x77F4;
const RESULT_DIRECTORY: u8 = 0xB1;

const RESULT_INDEX_WRITERS: [(u16, u8); 4] = [
    (0xA7B0, 0x53),
    (0xA7E6, 0x54),
    (0xA8B5, 0x55),
    (0xA862, 0x56),
];

const ROUTE_SLICES: &[RouteSlice] = &[
    RouteSlice {
        address: 0xA7A2,
        role: "publish front-end delete-confirmation dialogue",
        expected: &[
            0xA9, 0xB1, 0x8D, 0xF4, 0x77, 0xA9, 0x01, 0x8D, 0xF7, 0x77, 0xA9, 0x07, 0x85, 0x26,
            0xA9, 0x53, 0x8D, 0xF1, 0x77, 0xEE, 0xDB, 0x05, 0x60,
        ],
    },
    RouteSlice {
        address: 0xA7E6,
        role: "publish front-end delete-complete dialogue",
        expected: &[0xA9, 0x54, 0x8D, 0xF1, 0x77, 0xEE, 0xDB, 0x05, 0x60],
    },
    RouteSlice {
        address: 0xA862,
        role: "publish front-end data-error dialogue",
        expected: &[0xA9, 0x56, 0x8D, 0xF1, 0x77, 0xD0, 0x51],
    },
    RouteSlice {
        address: 0xA8B5,
        role: "publish front-end copy-complete dialogue",
        expected: &[
            0xA9, 0x55, 0x8D, 0xF1, 0x77, 0x20, 0x7D, 0xC7, 0xA9, 0x00, 0x8D, 0xF0, 0x77, 0xA9,
            0xB1, 0x8D, 0xF4, 0x77, 0xA9, 0x01, 0x8D, 0xF7, 0x77, 0xA9, 0x07, 0x85, 0x26, 0xEE,
            0xDB, 0x05, 0x60,
        ],
    },
];

struct RouteSlice {
    address: u16,
    role: &'static str,
    expected: &'static [u8],
}

pub(super) struct FrontEndResultSourceBinding {
    pub(super) result_index_writer_count: usize,
    pub(super) directory_writer_count: usize,
    pub(super) route_binding_sha1: String,
}

pub(super) fn bind_front_end_result_routes(source: &Rom) -> Result<FrontEndResultSourceBinding> {
    source.verify_supported_japanese()?;
    let expected_index_writers = RESULT_INDEX_WRITERS
        .iter()
        .copied()
        .map(|(address, index)| (SOURCE_PRG_BANK, address, index))
        .collect::<BTreeSet<_>>();
    let target_indices = RESULT_INDEX_WRITERS
        .iter()
        .map(|(_, index)| *index)
        .collect::<BTreeSet<_>>();
    let mut actual_index_writers = BTreeSet::new();
    for (bank, bytes) in source.prg().chunks_exact(0x4000).enumerate() {
        for (offset, candidate) in bytes.windows(5).enumerate() {
            if candidate[0] == 0xA9
                && target_indices.contains(&candidate[1])
                && candidate[2..]
                    == [
                        0x8D,
                        RESULT_INDEX_ADDRESS as u8,
                        (RESULT_INDEX_ADDRESS >> 8) as u8,
                    ]
            {
                actual_index_writers.insert((bank as u8, 0x8000 + offset as u16, candidate[1]));
            }
        }
    }
    ensure!(
        actual_index_writers == expected_index_writers,
        "front-end result dialogue index-writer census changed"
    );

    let mut bound_bytes = Vec::new();
    let mut directory_writer_count = 0;
    for route in ROUTE_SLICES {
        let offset = switchable_bank_file_offset(SOURCE_PRG_BANK, route.address)?;
        let actual = source
            .data()
            .get(offset..offset + route.expected.len())
            .with_context(|| format!("{} is outside the source image", route.role))?;
        ensure!(
            actual == route.expected,
            "{} source bytes changed",
            route.role
        );
        decode_rp2a03_sequence(actual, route.address, route.role)?;
        directory_writer_count += actual
            .windows(5)
            .filter(|bytes| {
                *bytes
                    == [
                        0xA9,
                        RESULT_DIRECTORY,
                        0x8D,
                        RESULT_DIRECTORY_ADDRESS as u8,
                        (RESULT_DIRECTORY_ADDRESS >> 8) as u8,
                    ]
            })
            .count();
        bound_bytes.extend_from_slice(&route.address.to_le_bytes());
        bound_bytes.extend_from_slice(actual);
    }
    ensure!(
        directory_writer_count == 2,
        "front-end result dialogue directory-writer routes changed"
    );

    Ok(FrontEndResultSourceBinding {
        result_index_writer_count: actual_index_writers.len(),
        directory_writer_count,
        route_binding_sha1: sha1_hex(&bound_bytes),
    })
}
