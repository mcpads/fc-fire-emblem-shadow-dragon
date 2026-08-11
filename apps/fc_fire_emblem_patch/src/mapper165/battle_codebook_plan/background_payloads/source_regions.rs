use anyhow::{Result, ensure};

use crate::{rom::Rom, sha1_hex, typed_source::decode_rp2a03_sequence};

use super::super::source_window::source_bytes;

pub(super) const SOURCE_REGION_COUNT: usize = SOURCE_REGIONS.len();

#[rustfmt::skip]
const SOURCE_REGIONS: [SourceRegionSpec; 29] = [
    SourceRegionSpec::code(0x05, 0x83D3,  42, "2bcb39cb6651c75bbd63ed36f044f953f17b777a", "experience label publisher"),
    SourceRegionSpec::code(0x05, 0x849D,  56, "5d2abf193233b689ab457d8d44711cf320cbdb23", "experience meter publisher"),
    SourceRegionSpec::code(0x05, 0x8522, 116, "2c416a90cba77c44dec27f9272e44f6d78ada3c7", "battle wipe publisher"),
    SourceRegionSpec::code(0x05, 0x8874,  70, "86e2d279c7ff4f4c002ed167d51332200323d971", "unit panel template publisher"),
    SourceRegionSpec::code(0x05, 0x89D7,  98, "cd0c6e8d2004abeef07e0b069575f80554049ced", "unit panel stat publisher"),
    SourceRegionSpec::code(0x05, 0x8A94,  38, "32a4daf087b58ee2861d524cd96df2fb9f1ab1c7", "unit panel marker publisher"),
    SourceRegionSpec::code(0x05, 0x8AD8, 205, "38ed52b436885b2cd7a931c8b6133ee23f6116e9", "unit panel HP bar publisher"),
    SourceRegionSpec::code(0x05, 0x8BA5, 118, "ae8d913575c1b1e74b08309a954524be82592055", "unit panel meter publisher"),
    SourceRegionSpec::code(0x05, 0x8CD3,  31, "e632709c8b375f72244287ddf4d94c54aa161395", "battle attribute publisher"),
    SourceRegionSpec::code(0x05, 0x950B,  52, "a19d9f3bca8514582eb9437ab683d1e8304d5634", "animation border publisher"),
    SourceRegionSpec::code(0x05, 0x9648,  87, "b19a35d3abb82c2b1e619dc03b5cf2908e814fa3", "damage digit publisher"),
    SourceRegionSpec::code(0x05, 0x96A3,  19, "9957d56610d0b9fef55ceffa87da886f9f19505f", "critical message publisher"),
    SourceRegionSpec::code(0x05, 0x9C05,  50, "c844a954565be4c04dafb504b78d65a6b5732405", "animation clear publisher"),
    SourceRegionSpec::code(0x05, 0xAE15,  39, "3ddf7392df142d6cf3001382a94dfa768f1a13f0", "effect overlay publisher"),
    SourceRegionSpec::code(0x05, 0xAF80, 124, "bebc8568ad84958b84293bbc8e8991e7d2506c78", "staggered clear publisher"),
    SourceRegionSpec::code(0x05, 0xAFFC,  42, "ecf5922625686b4442f35721cf504f243677569d", "animation reset publisher"),
    SourceRegionSpec::code(0x07, 0x8012,  61, "a161589cd29f891d12ec168901d23ca9b2d49770", "battle dialogue box publisher"),
    SourceRegionSpec::code(0x05, 0x8C29,  52, "3660b787330f657f522d04af6ad99c450b9fabf5", "bounded meter tile helper"),
    SourceRegionSpec::code(0x0F, 0xC6C9,  68, "b7cc8f60733be56bfb8d48bcd267150f649cee95", "numeric division helpers"),
    SourceRegionSpec::code(0x0F, 0xC7BA,  48, "825f8e40f4734f1570ad8bb1f74651ce75044035", "digit tile helper"),
    SourceRegionSpec::data(0x05, 0x8596,  15, "49857864c3e29617fa0b83ce68ff3129bc28f10e", "battle wipe tile table"),
    SourceRegionSpec::data(0x05, 0x88BA, 140, "82ce259a1c33a9cf98cb77731e2267da14b6070d", "unit panel queue templates"),
    SourceRegionSpec::data(0x05, 0x8CF2,  44, "bb8d6d88b63c9d920ccccc7295d98c59ec7455a9", "attribute and latch queue template"),
    SourceRegionSpec::data(0x05, 0x953F,  78, "b1949a505d3117dbfd10468be36b74401af48ffe", "animation border queue templates"),
    SourceRegionSpec::data(0x05, 0x969F,   4, "9f797e8a53f3995e611d5d5e331c2b8fc14c440c", "damage digit target table"),
    SourceRegionSpec::data(0x05, 0x96B6,  10, "588fe2f250aacef9782fa42f51ad8ee4a0fc4c72", "critical message queue template"),
    SourceRegionSpec::data(0x05, 0xA1D4,  92, "54d9493978bc6daad5750847a0e2332c9b470dc6", "animation clear queue templates"),
    SourceRegionSpec::data(0x05, 0xAE3C,  52, "3aca87442efbd1a358655f09538ac4ea78e1aa97", "effect overlay queue templates"),
    SourceRegionSpec::data(0x07, 0x804F, 137, "a633834596654ed704e6271548736fafd3a006a9", "battle dialogue box queue templates"),
];

#[derive(Clone, Copy)]
struct SourceRegionSpec {
    bank: u8,
    address: u16,
    byte_count: usize,
    expected_sha1: &'static str,
    role: &'static str,
    code: bool,
}

impl SourceRegionSpec {
    const fn code(
        bank: u8,
        address: u16,
        byte_count: usize,
        expected_sha1: &'static str,
        role: &'static str,
    ) -> Self {
        Self {
            bank,
            address,
            byte_count,
            expected_sha1,
            role,
            code: true,
        }
    }

    const fn data(
        bank: u8,
        address: u16,
        byte_count: usize,
        expected_sha1: &'static str,
        role: &'static str,
    ) -> Self {
        Self {
            bank,
            address,
            byte_count,
            expected_sha1,
            role,
            code: false,
        }
    }
}

pub(super) fn bind_source_regions(rom: &Rom) -> Result<String> {
    let mut catalog = Vec::new();
    for spec in SOURCE_REGIONS {
        let bytes = source_bytes(rom, spec.bank, spec.address, spec.byte_count)?;
        ensure!(
            sha1_hex(bytes) == spec.expected_sha1,
            "{} source region changed",
            spec.role
        );
        if spec.code {
            decode_rp2a03_sequence(bytes, spec.address, spec.role)?;
        }
        catalog.push(spec.bank);
        catalog.extend_from_slice(&spec.address.to_le_bytes());
        catalog.extend_from_slice(&(spec.byte_count as u64).to_le_bytes());
        catalog.extend_from_slice(spec.expected_sha1.as_bytes());
        catalog.push(0);
    }
    Ok(sha1_hex(&catalog))
}
