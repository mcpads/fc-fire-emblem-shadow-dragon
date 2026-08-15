use std::collections::BTreeSet;

use anyhow::{Context, Result, ensure};

use crate::{
    mmc5_chr::switchable_bank_file_offset, rom::Rom, sha1_hex, typed_source::decode_rp2a03_sequence,
};

const SOURCE_PRG_BANK: u8 = 0x02;
const SAVE_SUMMARY_PRG_BANK: u8 = 0x0B;
const SAVE_SUMMARY_COMPOSER_ADDRESS: u16 = 0x8D4B;
const RESULT_INDEX_ADDRESS: u16 = 0x77F1;
const RESULT_DIRECTORY_ADDRESS: u16 = 0x77F4;
const RESULT_DIRECTORY: u8 = 0xB1;

/// 선택한 기록의 5바이트 유닛 레코드를 `$76F4..=$76F8`로 복사한 뒤 공용 유닛
/// 요약 작성기 `$826C`를 부른다. 그 작성기 안의 이름·병종 appender는 소비자
/// 카탈로그 런타임이 별도로 source-bound한다.
const SAVE_SUMMARY_COMPOSER: &[u8] = &[
    0xA9, 0x00, 0x85, 0x00, 0xA9, 0x60, 0x85, 0x01, 0xA9, 0x19, 0x85, 0x02, 0xA9, 0x65, 0x85, 0x03,
    0xA5, 0x67, 0xF0, 0x10, 0xA9, 0x44, 0x85, 0x00, 0xA9, 0x65, 0x85, 0x01, 0xA9, 0x5D, 0x85, 0x02,
    0xA9, 0x6A, 0x85, 0x03, 0xA0, 0x04, 0xB1, 0x00, 0x99, 0xF4, 0x76, 0x88, 0x10, 0xF8, 0xAD, 0x7E,
    0x76, 0x8D, 0x0D, 0x05, 0xA0, 0x0D, 0xB1, 0x02, 0x8D, 0x7E, 0x76, 0x20, 0x6C, 0x82, 0xAD, 0x0D,
    0x05, 0x8D, 0x7E, 0x76, 0xA9, 0x30, 0x85, 0x70, 0xA9, 0x90, 0x85, 0x71, 0x60,
];

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
    pub(super) summary_composer_count: usize,
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

    let summary_offset =
        switchable_bank_file_offset(SAVE_SUMMARY_PRG_BANK, SAVE_SUMMARY_COMPOSER_ADDRESS)?;
    let summary_actual = source
        .data()
        .get(summary_offset..summary_offset + SAVE_SUMMARY_COMPOSER.len())
        .context("front-end save-summary composer is outside the source image")?;
    ensure!(
        summary_actual == SAVE_SUMMARY_COMPOSER,
        "front-end save-summary composer source bytes changed"
    );
    decode_rp2a03_sequence(
        summary_actual,
        SAVE_SUMMARY_COMPOSER_ADDRESS,
        "compose front-end selected-save summary",
    )?;
    bound_bytes.extend_from_slice(&SAVE_SUMMARY_COMPOSER_ADDRESS.to_le_bytes());
    bound_bytes.extend_from_slice(summary_actual);

    Ok(FrontEndResultSourceBinding {
        result_index_writer_count: actual_index_writers.len(),
        directory_writer_count,
        summary_composer_count: 1,
        route_binding_sha1: sha1_hex(&bound_bytes),
    })
}
