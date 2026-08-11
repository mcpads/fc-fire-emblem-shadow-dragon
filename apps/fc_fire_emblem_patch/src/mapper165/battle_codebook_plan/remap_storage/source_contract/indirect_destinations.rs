use std::collections::BTreeSet;

use anyhow::{Context, Result, ensure};
use serde::Serialize;

use crate::{rom::Rom, sha1_hex, typed_source::decode_rp2a03_sequence};

use super::super::super::source_window::source_bytes;
use super::{REMAP_STORAGE_END, REMAP_STORAGE_START};

pub(super) const EXPECTED_INDIRECT_STORES: [(u8, u16, u8); 24] = [
    (0x04, 0x80E1, 0x06),
    (0x04, 0x811C, 0x06),
    (0x04, 0x816B, 0x06),
    (0x04, 0x818A, 0x06),
    (0x04, 0x81AF, 0x06),
    (0x04, 0x81B7, 0x06),
    (0x04, 0x81F0, 0x06),
    (0x04, 0x820A, 0x06),
    (0x04, 0x8352, 0x06),
    (0x04, 0x835C, 0x06),
    (0x05, 0x8771, 0x04),
    (0x05, 0x912F, 0x08),
    (0x05, 0x9400, 0x00),
    (0x05, 0x9406, 0x00),
    (0x05, 0x940C, 0x00),
    (0x05, 0x9412, 0x00),
    (0x05, 0xAE09, 0x3E),
    (0x05, 0xAE0B, 0x40),
    (0x0F, 0xC213, 0x02),
    (0x0F, 0xC22D, 0x00),
    (0x0F, 0xC7D1, 0x08),
    (0x0F, 0xC7E0, 0x08),
    (0x0F, 0xCFF8, 0x08),
    (0x0F, 0xD110, 0x08),
];

#[derive(Clone, Copy)]
struct SourceRegionSpec {
    bank: u8,
    address: u16,
    byte_count: usize,
}

#[derive(Clone, Copy)]
struct TypedRegionSpec {
    bank: u8,
    address: u16,
    byte_count: usize,
    role: &'static str,
}

#[derive(Clone, Copy)]
struct DestinationRangeSpec {
    start: u16,
    end: u16,
}

struct IndirectStoreClassSpec {
    role: &'static str,
    destination_basis: &'static str,
    sites: &'static [(u8, u16, u8)],
    source_regions: &'static [SourceRegionSpec],
    typed_regions: &'static [TypedRegionSpec],
    destination_ranges: &'static [DestinationRangeSpec],
    expected_source_sha1: &'static str,
}

const DIALOGUE_BUFFER_SITES: [(u8, u16, u8); 10] = [
    (0x04, 0x80E1, 0x06),
    (0x04, 0x811C, 0x06),
    (0x04, 0x816B, 0x06),
    (0x04, 0x818A, 0x06),
    (0x04, 0x81AF, 0x06),
    (0x04, 0x81B7, 0x06),
    (0x04, 0x81F0, 0x06),
    (0x04, 0x820A, 0x06),
    (0x04, 0x8352, 0x06),
    (0x04, 0x835C, 0x06),
];
const COMPARISON_CELL_SITES: [(u8, u16, u8); 1] = [(0x05, 0x8771, 0x04)];
const UNIT_STATUS_TRANSFER_SITES: [(u8, u16, u8); 1] = [(0x05, 0x912F, 0x08)];
const UNIT_SLOT_INITIALIZER_SITES: [(u8, u16, u8); 4] = [
    (0x05, 0x9400, 0x00),
    (0x05, 0x9406, 0x00),
    (0x05, 0x940C, 0x00),
    (0x05, 0x9412, 0x00),
];
const COMBATANT_SHADOW_SITES: [(u8, u16, u8); 2] = [(0x05, 0xAE09, 0x3E), (0x05, 0xAE0B, 0x40)];
const GENERIC_COPY_SITES: [(u8, u16, u8); 1] = [(0x0F, 0xC213, 0x02)];
const BATTLE_ZERO_FILL_SITES: [(u8, u16, u8); 1] = [(0x0F, 0xC22D, 0x00)];
const FIXED_GLYPH_FLAG_SITES: [(u8, u16, u8); 2] = [(0x0F, 0xC7D1, 0x08), (0x0F, 0xC7E0, 0x08)];
const UNIT_CALCULATION_SITES: [(u8, u16, u8); 2] = [(0x0F, 0xCFF8, 0x08), (0x0F, 0xD110, 0x08)];

const INDIRECT_STORE_CLASS_SPECS: [IndirectStoreClassSpec; 9] = [
    IndirectStoreClassSpec {
        role: "battle_dialogue_composition_buffers",
        destination_basis: "selector table 04:$83E8 and forty-byte dialogue record bound",
        sites: &DIALOGUE_BUFFER_SITES,
        source_regions: &[
            SourceRegionSpec {
                bank: 0x04,
                address: 0x80D9,
                byte_count: 0x0298,
            },
            SourceRegionSpec {
                bank: 0x04,
                address: 0x83D4,
                byte_count: 0x001E,
            },
        ],
        typed_regions: &[TypedRegionSpec {
            bank: 0x04,
            address: 0x83D4,
            byte_count: 20,
            role: "battle dialogue destination selector",
        }],
        destination_ranges: &[DestinationRangeSpec {
            start: 0x7953,
            end: 0x7A1A,
        }],
        expected_source_sha1: "1e45ef153e0e1f3f675692ea3c632fcb4aeefba5",
    },
    IndirectStoreClassSpec {
        role: "battle_comparison_cells",
        destination_basis: "seven-entry destination table 05:$87E6 and one-byte store",
        sites: &COMPARISON_CELL_SITES,
        source_regions: &[
            SourceRegionSpec {
                bank: 0x05,
                address: 0x873D,
                byte_count: 0x0060,
            },
            SourceRegionSpec {
                bank: 0x05,
                address: 0x87E6,
                byte_count: 0x000E,
            },
        ],
        typed_regions: &[],
        destination_ranges: &[DestinationRangeSpec {
            start: 0x032A,
            end: 0x0331,
        }],
        expected_source_sha1: "7d22db2a03409b7d3f0563eae7b3bdd7f2ee7733",
    },
    IndirectStoreClassSpec {
        role: "unit_status_field_transfer",
        destination_basis: "destination table 05:$9135, four-byte copy, and second pass at +$10",
        sites: &UNIT_STATUS_TRANSFER_SITES,
        source_regions: &[
            SourceRegionSpec {
                bank: 0x05,
                address: 0x90BF,
                byte_count: 0x0037,
            },
            SourceRegionSpec {
                bank: 0x05,
                address: 0x912B,
                byte_count: 0x000A,
            },
            SourceRegionSpec {
                bank: 0x05,
                address: 0x9135,
                byte_count: 0x0004,
            },
        ],
        typed_regions: &[TypedRegionSpec {
            bank: 0x05,
            address: 0x912B,
            byte_count: 10,
            role: "four-byte unit status transfer",
        }],
        destination_ranges: &[DestinationRangeSpec {
            start: 0x04DB,
            end: 0x04F2,
        }],
        expected_source_sha1: "8b8ff2b1755b677ddeb8146ca5ee85a57d83c115",
    },
    IndirectStoreClassSpec {
        role: "unit_slot_initializer",
        destination_basis: "two-entry destination table 05:$9431 and four field stores",
        sites: &UNIT_SLOT_INITIALIZER_SITES,
        source_regions: &[
            SourceRegionSpec {
                bank: 0x05,
                address: 0x93E9,
                byte_count: 0x0032,
            },
            SourceRegionSpec {
                bank: 0x05,
                address: 0x9431,
                byte_count: 0x0004,
            },
        ],
        typed_regions: &[TypedRegionSpec {
            bank: 0x05,
            address: 0x93E9,
            byte_count: 0x002B,
            role: "unit slot field initializer",
        }],
        destination_ranges: &[DestinationRangeSpec {
            start: 0x035B,
            end: 0x0364,
        }],
        expected_source_sha1: "c90c61c3ca82d6f98d94afd11327a3327757e66e",
    },
    IndirectStoreClassSpec {
        role: "combatant_shadow_fields",
        destination_basis: "three literal pairs in 05:$ADC8..$AE13 and four-byte copies",
        sites: &COMBATANT_SHADOW_SITES,
        source_regions: &[SourceRegionSpec {
            bank: 0x05,
            address: 0xADC8,
            byte_count: 0x004C,
        }],
        typed_regions: &[],
        destination_ranges: &[DestinationRangeSpec {
            start: 0x04DB,
            end: 0x04F6,
        }],
        expected_source_sha1: "65617e8139ecffa44fa929bf2789053b04d5411a",
    },
    IndirectStoreClassSpec {
        role: "bounded_battle_copy",
        destination_basis: "three callers: two $0781 queue copies of 32 or 37 bytes and one forty-byte $79xx dialogue copy",
        sites: &GENERIC_COPY_SITES,
        source_regions: &[
            SourceRegionSpec {
                bank: 0x0F,
                address: 0xC209,
                byte_count: 0x001C,
            },
            SourceRegionSpec {
                bank: 0x07,
                address: 0x8012,
                byte_count: 0x0029,
            },
            SourceRegionSpec {
                bank: 0x07,
                address: 0x804F,
                byte_count: 0x0009,
            },
            SourceRegionSpec {
                bank: 0x05,
                address: 0x950B,
                byte_count: 0x0022,
            },
            SourceRegionSpec {
                bank: 0x05,
                address: 0x953F,
                byte_count: 0x0004,
            },
            SourceRegionSpec {
                bank: 0x04,
                address: 0x8312,
                byte_count: 0x0028,
            },
            SourceRegionSpec {
                bank: 0x04,
                address: 0x83E8,
                byte_count: 0x000A,
            },
        ],
        typed_regions: &[TypedRegionSpec {
            bank: 0x0F,
            address: 0xC209,
            byte_count: 28,
            role: "bounded generic battle copy",
        }],
        destination_ranges: &[
            DestinationRangeSpec {
                start: 0x0781,
                end: 0x07A5,
            },
            DestinationRangeSpec {
                start: 0x7953,
                end: 0x79F2,
            },
        ],
        expected_source_sha1: "253a3d78d40d2b573968a5d1bda60430fa39b39f",
    },
    IndirectStoreClassSpec {
        role: "battle_state_zero_fill",
        destination_basis: "literal $0329 destination and $0151-byte count ending at $0479",
        sites: &BATTLE_ZERO_FILL_SITES,
        source_regions: &[
            SourceRegionSpec {
                bank: 0x0F,
                address: 0xC225,
                byte_count: 0x0018,
            },
            SourceRegionSpec {
                bank: 0x05,
                address: 0x8276,
                byte_count: 0x0015,
            },
        ],
        typed_regions: &[
            TypedRegionSpec {
                bank: 0x0F,
                address: 0xC225,
                byte_count: 24,
                role: "battle state zero fill",
            },
            TypedRegionSpec {
                bank: 0x05,
                address: 0x8276,
                byte_count: 21,
                role: "battle state zero-fill caller",
            },
        ],
        destination_ranges: &[DestinationRangeSpec {
            start: 0x0329,
            end: 0x0479,
        }],
        expected_source_sha1: "e83d6242e82e74764c36a6b00b049917b652bc5a",
    },
    IndirectStoreClassSpec {
        role: "fixed_glyph_flags",
        destination_basis: "all four reachable callers load literal pointer $041E before fixed-bank routine",
        sites: &FIXED_GLYPH_FLAG_SITES,
        source_regions: &[SourceRegionSpec {
            bank: 0x0F,
            address: 0xC7BA,
            byte_count: 0x0030,
        }],
        typed_regions: &[TypedRegionSpec {
            bank: 0x0F,
            address: 0xC7BA,
            byte_count: 48,
            role: "fixed glyph flag writer",
        }],
        destination_ranges: &[DestinationRangeSpec {
            start: 0x041E,
            end: 0x0422,
        }],
        expected_source_sha1: "825f8e40f4734f1570ad8bb1f74651ce75044035",
    },
    IndirectStoreClassSpec {
        role: "unit_calculation_fields",
        destination_basis: "fixed pointer loader 0F:$D04E, seven-entry $D077 table, and index bound six",
        sites: &UNIT_CALCULATION_SITES,
        source_regions: &[SourceRegionSpec {
            bank: 0x0F,
            address: 0xCFF3,
            byte_count: 0x0130,
        }],
        typed_regions: &[TypedRegionSpec {
            bank: 0x0F,
            address: 0xD04E,
            byte_count: 27,
            role: "unit calculation pointer loader",
        }],
        destination_ranges: &[DestinationRangeSpec {
            start: 0x030C,
            end: 0x0337,
        }],
        expected_source_sha1: "630cb7cde0792b2505c01ddae897ebb20904eb5e",
    },
];

#[cfg(test)]
pub(super) const DESTINATION_CLASS_COUNT: usize = INDIRECT_STORE_CLASS_SPECS.len();

#[derive(Clone, Debug, Serialize)]
struct IndirectStoreDestinationRange {
    start_hex: String,
    end_hex: String,
}

#[derive(Clone, Debug, Serialize)]
pub(super) struct IndirectStoreDestinationClass {
    role: &'static str,
    destination_basis: &'static str,
    store_instruction_count: usize,
    source_binding_byte_count: usize,
    source_binding_sha1: String,
    typed_source_instruction_count: usize,
    destination_ranges: Vec<IndirectStoreDestinationRange>,
    pub(super) every_destination_range_outside_remap_storage: bool,
}

pub(super) fn bind_indirect_store_destination_classes(
    rom: &Rom,
) -> Result<Vec<IndirectStoreDestinationClass>> {
    let declared_sites = INDIRECT_STORE_CLASS_SPECS
        .iter()
        .flat_map(|spec| spec.sites.iter().copied())
        .collect::<BTreeSet<_>>();
    let expected_sites = EXPECTED_INDIRECT_STORES
        .into_iter()
        .collect::<BTreeSet<_>>();
    ensure!(
        declared_sites == expected_sites,
        "indirect-store destination classes do not partition the exact site catalog"
    );
    bind_indirect_destination_inputs(rom)?;

    INDIRECT_STORE_CLASS_SPECS
        .iter()
        .map(|spec| {
            let mut source_binding = Vec::new();
            for region in spec.source_regions {
                source_binding.extend_from_slice(source_bytes(
                    rom,
                    region.bank,
                    region.address,
                    region.byte_count,
                )?);
            }
            let source_binding_sha1 = sha1_hex(&source_binding);
            ensure!(
                source_binding_sha1 == spec.expected_source_sha1,
                "{} source binding changed",
                spec.role
            );
            let typed_source_instruction_count =
                spec.typed_regions
                    .iter()
                    .try_fold(0_usize, |count, region| -> Result<usize> {
                        let bytes =
                            source_bytes(rom, region.bank, region.address, region.byte_count)?;
                        let typed = decode_rp2a03_sequence(bytes, region.address, region.role)?;
                        count
                            .checked_add(typed.len())
                            .context("indirect-store typed instruction count overflow")
                    })?;
            let every_destination_range_outside_remap_storage =
                spec.destination_ranges.iter().all(|range| {
                    range.start <= range.end
                        && (range.end < REMAP_STORAGE_START || range.start > REMAP_STORAGE_END)
                });
            ensure!(
                every_destination_range_outside_remap_storage,
                "{} destination overlaps remap storage",
                spec.role
            );
            Ok(IndirectStoreDestinationClass {
                role: spec.role,
                destination_basis: spec.destination_basis,
                store_instruction_count: spec.sites.len(),
                source_binding_byte_count: source_binding.len(),
                source_binding_sha1,
                typed_source_instruction_count,
                destination_ranges: spec
                    .destination_ranges
                    .iter()
                    .map(|range| IndirectStoreDestinationRange {
                        start_hex: format!("0x{:04X}", range.start),
                        end_hex: format!("0x{:04X}", range.end),
                    })
                    .collect(),
                every_destination_range_outside_remap_storage,
            })
        })
        .collect()
}

fn bind_indirect_destination_inputs(rom: &Rom) -> Result<()> {
    ensure!(
        read_u16_table(rom, 0x04, 0x83E8, 5)? == [0x7953, 0x797B, 0x79A3, 0x79CB, 0x79F3],
        "battle dialogue destination table changed"
    );
    ensure!(
        read_u16_table(rom, 0x05, 0x87E6, 7)?
            == [0x032C, 0x032D, 0x032E, 0x032F, 0x0330, 0x0331, 0x032A],
        "battle comparison destination table changed"
    );
    ensure!(
        read_u16_table(rom, 0x05, 0x9135, 2)? == [0x04DB, 0x04DF],
        "unit status-transfer destination table changed"
    );
    ensure!(
        read_u16_table(rom, 0x05, 0x9431, 2)? == [0x035B, 0x0361],
        "unit slot destination table changed"
    );
    ensure!(
        read_u16_table(rom, 0x0F, 0xD077, 7)?
            == [0x032C, 0x032D, 0x032E, 0x032F, 0x0330, 0x0331, 0x032A],
        "unit calculation destination table changed"
    );
    let fixed_glyph_pointer_setup = [
        0xA9, 0x1E, 0x85, 0x08, 0xA9, 0x04, 0x85, 0x09, 0x20, 0xBA, 0xC7,
    ];
    let fixed_glyph_callers: [(u8, u16); 4] = [
        (0x05, 0x86BB),
        (0x05, 0x89E8),
        (0x05, 0x965C),
        (0x07, 0x814A),
    ];
    for (bank, call_address) in fixed_glyph_callers {
        let setup_address = call_address
            .checked_sub(8)
            .context("fixed glyph pointer setup address underflow")?;
        ensure!(
            source_bytes(rom, bank, setup_address, fixed_glyph_pointer_setup.len())?
                == fixed_glyph_pointer_setup,
            "fixed glyph pointer setup changed at {bank:02X}:${setup_address:04X}"
        );
    }
    Ok(())
}

fn read_u16_table(rom: &Rom, bank: u8, address: u16, count: usize) -> Result<Vec<u16>> {
    let byte_count = count
        .checked_mul(2)
        .context("indirect destination table byte count overflow")?;
    Ok(source_bytes(rom, bank, address, byte_count)?
        .chunks_exact(2)
        .map(|pair| u16::from_le_bytes([pair[0], pair[1]]))
        .collect())
}
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn indirect_store_classes_partition_sites_and_exclude_remap_storage() {
        let classified = INDIRECT_STORE_CLASS_SPECS
            .iter()
            .flat_map(|spec| spec.sites.iter().copied())
            .collect::<BTreeSet<_>>();
        assert_eq!(
            classified,
            EXPECTED_INDIRECT_STORES
                .into_iter()
                .collect::<BTreeSet<_>>()
        );
        assert!(INDIRECT_STORE_CLASS_SPECS.iter().all(|spec| {
            spec.destination_ranges.iter().all(|range| {
                range.start <= range.end
                    && (range.end < REMAP_STORAGE_START || range.start > REMAP_STORAGE_END)
            })
        }));
    }
}
