use anyhow::{Context, Result, ensure};

use crate::rom::PRG_SIZE;

const SOURCE_PRG_BANK_BYTE_COUNT: usize = 16 * 1024;
const SWITCHABLE_CPU_START: u16 = 0x8000;
const SWITCHABLE_CPU_END: u16 = 0xC000;
const CHAPTER_MAP_HEADER_BYTE_COUNT: usize = 4;

pub(crate) const CHAPTER_MAP_COUNT: usize = 25;
pub(crate) const EARLY_CHAPTER_MAP_BANK: u8 = 0x02;
pub(crate) const EARLY_CHAPTER_MAP_POINTER_TABLE: u16 = 0x8000;
pub(crate) const EARLY_CHAPTER_MAP_COUNT: usize = 13;
pub(crate) const LATE_CHAPTER_MAP_BANK: u8 = 0x09;
pub(crate) const LATE_CHAPTER_MAP_POINTER_TABLE: u16 = 0x8000;
pub(crate) const LATE_CHAPTER_MAP_COUNT: usize = 12;

#[derive(Clone, Copy)]
struct ChapterMapSourceSpec {
    prg_bank: u8,
    cpu_address: u16,
    header: [u8; CHAPTER_MAP_HEADER_BYTE_COUNT],
}

const CHAPTER_MAP_SOURCE_SPECS: [ChapterMapSourceSpec; CHAPTER_MAP_COUNT] = [
    ChapterMapSourceSpec {
        prg_bank: 0x02,
        cpu_address: 0x801A,
        header: [0x0E, 0x1F, 0x00, 0x10],
    },
    ChapterMapSourceSpec {
        prg_bank: 0x02,
        cpu_address: 0x81FE,
        header: [0x0E, 0x1F, 0x00, 0x10],
    },
    ChapterMapSourceSpec {
        prg_bank: 0x02,
        cpu_address: 0x83E2,
        header: [0x15, 0x1F, 0x00, 0x10],
    },
    ChapterMapSourceSpec {
        prg_bank: 0x02,
        cpu_address: 0x86A6,
        header: [0x15, 0x1F, 0x07, 0x0E],
    },
    ChapterMapSourceSpec {
        prg_bank: 0x02,
        cpu_address: 0x896A,
        header: [0x15, 0x1F, 0x07, 0x03],
    },
    ChapterMapSourceSpec {
        prg_bank: 0x02,
        cpu_address: 0x8C2E,
        header: [0x16, 0x1F, 0x08, 0x06],
    },
    ChapterMapSourceSpec {
        prg_bank: 0x02,
        cpu_address: 0x8F12,
        header: [0x1D, 0x0F, 0x0E, 0x00],
    },
    ChapterMapSourceSpec {
        prg_bank: 0x02,
        cpu_address: 0x90F6,
        header: [0x1D, 0x1F, 0x0F, 0x00],
    },
    ChapterMapSourceSpec {
        prg_bank: 0x02,
        cpu_address: 0x94BA,
        header: [0x1D, 0x1F, 0x04, 0x00],
    },
    ChapterMapSourceSpec {
        prg_bank: 0x02,
        cpu_address: 0x987E,
        header: [0x1D, 0x1F, 0x00, 0x10],
    },
    ChapterMapSourceSpec {
        prg_bank: 0x02,
        cpu_address: 0x9C42,
        header: [0x18, 0x1F, 0x0A, 0x00],
    },
    ChapterMapSourceSpec {
        prg_bank: 0x02,
        cpu_address: 0x9F66,
        header: [0x1D, 0x1F, 0x00, 0x00],
    },
    ChapterMapSourceSpec {
        prg_bank: 0x02,
        cpu_address: 0xA32A,
        header: [0x18, 0x1F, 0x00, 0x10],
    },
    ChapterMapSourceSpec {
        prg_bank: 0x09,
        cpu_address: 0x8018,
        header: [0x1D, 0x1F, 0x00, 0x10],
    },
    ChapterMapSourceSpec {
        prg_bank: 0x09,
        cpu_address: 0x83DC,
        header: [0x1D, 0x1F, 0x0F, 0x00],
    },
    ChapterMapSourceSpec {
        prg_bank: 0x09,
        cpu_address: 0x87A0,
        header: [0x1D, 0x1F, 0x00, 0x10],
    },
    ChapterMapSourceSpec {
        prg_bank: 0x09,
        cpu_address: 0x8B64,
        header: [0x1D, 0x1F, 0x0C, 0x0F],
    },
    ChapterMapSourceSpec {
        prg_bank: 0x09,
        cpu_address: 0x8F28,
        header: [0x1D, 0x1F, 0x00, 0x10],
    },
    ChapterMapSourceSpec {
        prg_bank: 0x09,
        cpu_address: 0x92EC,
        header: [0x1D, 0x0F, 0x0F, 0x00],
    },
    ChapterMapSourceSpec {
        prg_bank: 0x09,
        cpu_address: 0x94D0,
        header: [0x1D, 0x1F, 0x04, 0x10],
    },
    ChapterMapSourceSpec {
        prg_bank: 0x09,
        cpu_address: 0x9894,
        header: [0x1D, 0x1F, 0x0F, 0x0A],
    },
    ChapterMapSourceSpec {
        prg_bank: 0x09,
        cpu_address: 0x9C58,
        header: [0x1D, 0x1F, 0x0F, 0x10],
    },
    ChapterMapSourceSpec {
        prg_bank: 0x09,
        cpu_address: 0xA01C,
        header: [0x1D, 0x1F, 0x0F, 0x00],
    },
    ChapterMapSourceSpec {
        prg_bank: 0x09,
        cpu_address: 0xA3E0,
        header: [0x1D, 0x1F, 0x00, 0x00],
    },
    ChapterMapSourceSpec {
        prg_bank: 0x09,
        cpu_address: 0xA7A4,
        header: [0x1D, 0x1F, 0x0F, 0x0A],
    },
];

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct ChapterMapSourceRecord {
    chapter_number: u8,
    prg_bank: u8,
    cpu_address: u16,
    header: [u8; CHAPTER_MAP_HEADER_BYTE_COUNT],
    prg_offset: usize,
    storage_byte_count: usize,
}

impl ChapterMapSourceRecord {
    pub(crate) fn chapter_number(&self) -> u8 {
        self.chapter_number
    }

    pub(crate) fn prg_bank(&self) -> u8 {
        self.prg_bank
    }

    pub(crate) fn cpu_address(&self) -> u16 {
        self.cpu_address
    }

    pub(crate) fn header(&self) -> [u8; CHAPTER_MAP_HEADER_BYTE_COUNT] {
        self.header
    }

    pub(crate) fn prg_offset(&self) -> usize {
        self.prg_offset
    }

    pub(crate) fn row_count(&self) -> usize {
        usize::from(self.header[0]) + 1
    }

    pub(crate) fn column_count(&self) -> usize {
        usize::from(self.header[1]) + 1
    }

    pub(crate) fn storage_bytes<'a>(&self, prg: &'a [u8]) -> Result<&'a [u8]> {
        prg.get(self.prg_offset..self.prg_offset + self.storage_byte_count)
            .context("chapter map storage is outside PRG")
    }

    pub(crate) fn tile_code(&self, prg: &[u8], row: u8, column: u8) -> Result<u8> {
        ensure!(
            usize::from(row) < self.row_count() && usize::from(column) < self.column_count(),
            "chapter-map coordinate is outside chapter {}",
            self.chapter_number,
        );
        let tile_offset = CHAPTER_MAP_HEADER_BYTE_COUNT
            + usize::from(row) * self.column_count()
            + usize::from(column);
        self.storage_bytes(prg)?
            .get(tile_offset)
            .copied()
            .context("chapter map tile is outside source storage")
    }
}

pub(crate) fn bind_chapter_map_source_records(prg: &[u8]) -> Result<Vec<ChapterMapSourceRecord>> {
    ensure!(
        prg.len() == PRG_SIZE,
        "chapter map source requires the supported 256 KiB PRG layout"
    );
    ensure!(
        EARLY_CHAPTER_MAP_COUNT + LATE_CHAPTER_MAP_COUNT == CHAPTER_MAP_COUNT,
        "chapter map pointer-table populations do not cover every chapter"
    );
    bind_pointer_table(
        prg,
        EARLY_CHAPTER_MAP_BANK,
        EARLY_CHAPTER_MAP_POINTER_TABLE,
        &CHAPTER_MAP_SOURCE_SPECS[..EARLY_CHAPTER_MAP_COUNT],
    )?;
    bind_pointer_table(
        prg,
        LATE_CHAPTER_MAP_BANK,
        LATE_CHAPTER_MAP_POINTER_TABLE,
        &CHAPTER_MAP_SOURCE_SPECS[EARLY_CHAPTER_MAP_COUNT..],
    )?;

    let mut records = Vec::with_capacity(CHAPTER_MAP_COUNT);
    for (index, spec) in CHAPTER_MAP_SOURCE_SPECS.iter().enumerate() {
        let prg_offset = chapter_map_prg_offset(spec.prg_bank, spec.cpu_address)?;
        let header = prg
            .get(prg_offset..prg_offset + CHAPTER_MAP_HEADER_BYTE_COUNT)
            .context("chapter map header is outside PRG")?;
        ensure!(
            header == spec.header,
            "chapter {} map header changed",
            index + 1,
        );
        let row_count = usize::from(header[0]) + 1;
        let column_count = usize::from(header[1]) + 1;
        let payload_byte_count = row_count
            .checked_mul(column_count)
            .context("chapter map dimensions overflow")?;
        let storage_byte_count = CHAPTER_MAP_HEADER_BYTE_COUNT
            .checked_add(payload_byte_count)
            .context("chapter map storage length overflow")?;
        let storage_end = spec
            .cpu_address
            .checked_add(u16::try_from(storage_byte_count)?)
            .context("chapter map storage address overflow")?;
        ensure!(
            storage_end <= SWITCHABLE_CPU_END,
            "chapter {} map crosses its switchable PRG bank",
            index + 1,
        );
        prg.get(prg_offset..prg_offset + storage_byte_count)
            .context("chapter map payload is outside PRG")?;
        if let Some(next) = CHAPTER_MAP_SOURCE_SPECS.get(index + 1)
            && next.prg_bank == spec.prg_bank
        {
            ensure!(
                storage_end == next.cpu_address,
                "chapter {} map no longer ends at the next map record",
                index + 1,
            );
        }
        records.push(ChapterMapSourceRecord {
            chapter_number: u8::try_from(index + 1)?,
            prg_bank: spec.prg_bank,
            cpu_address: spec.cpu_address,
            header: spec.header,
            prg_offset,
            storage_byte_count,
        });
    }
    ensure!(
        records.len() == CHAPTER_MAP_COUNT
            && records
                .iter()
                .enumerate()
                .all(|(index, record)| usize::from(record.chapter_number) == index + 1),
        "chapter map source population changed"
    );
    Ok(records)
}

fn bind_pointer_table(
    prg: &[u8],
    prg_bank: u8,
    cpu_address: u16,
    specs: &[ChapterMapSourceSpec],
) -> Result<()> {
    ensure!(
        specs.iter().all(|spec| spec.prg_bank == prg_bank),
        "chapter map pointer-table bank and record bank disagree"
    );
    let offset = chapter_map_prg_offset(prg_bank, cpu_address)?;
    let byte_count = specs
        .len()
        .checked_mul(2)
        .context("chapter map pointer-table length overflow")?;
    let actual = prg
        .get(offset..offset + byte_count)
        .context("chapter map pointer table is outside PRG")?
        .chunks_exact(2)
        .map(|bytes| u16::from_le_bytes([bytes[0], bytes[1]]))
        .collect::<Vec<_>>();
    let expected = specs
        .iter()
        .map(|spec| spec.cpu_address)
        .collect::<Vec<_>>();
    ensure!(actual == expected, "chapter map pointer table changed");
    Ok(())
}

fn chapter_map_prg_offset(prg_bank: u8, cpu_address: u16) -> Result<usize> {
    ensure!(
        (SWITCHABLE_CPU_START..SWITCHABLE_CPU_END).contains(&cpu_address),
        "chapter map address is outside the switchable PRG window"
    );
    usize::from(prg_bank)
        .checked_mul(SOURCE_PRG_BANK_BYTE_COUNT)
        .and_then(|base| base.checked_add(usize::from(cpu_address - SWITCHABLE_CPU_START)))
        .context("chapter map PRG offset overflow")
}

#[cfg(test)]
pub(crate) fn install_chapter_map_source_fixture(prg: &mut [u8]) {
    for (bank, table, specs) in [
        (
            EARLY_CHAPTER_MAP_BANK,
            EARLY_CHAPTER_MAP_POINTER_TABLE,
            &CHAPTER_MAP_SOURCE_SPECS[..EARLY_CHAPTER_MAP_COUNT],
        ),
        (
            LATE_CHAPTER_MAP_BANK,
            LATE_CHAPTER_MAP_POINTER_TABLE,
            &CHAPTER_MAP_SOURCE_SPECS[EARLY_CHAPTER_MAP_COUNT..],
        ),
    ] {
        let table_offset = chapter_map_prg_offset(bank, table).unwrap();
        for (index, spec) in specs.iter().enumerate() {
            prg[table_offset + index * 2..table_offset + index * 2 + 2]
                .copy_from_slice(&spec.cpu_address.to_le_bytes());
        }
    }
    for spec in CHAPTER_MAP_SOURCE_SPECS {
        let offset = chapter_map_prg_offset(spec.prg_bank, spec.cpu_address).unwrap();
        prg[offset..offset + CHAPTER_MAP_HEADER_BYTE_COUNT].copy_from_slice(&spec.header);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fixture_prg() -> Vec<u8> {
        let mut prg = vec![0; PRG_SIZE];
        install_chapter_map_source_fixture(&mut prg);
        prg
    }

    #[test]
    fn binds_the_complete_ordered_chapter_map_population() {
        let records = bind_chapter_map_source_records(&fixture_prg()).unwrap();

        assert_eq!(records.len(), CHAPTER_MAP_COUNT);
        assert_eq!(
            records
                .iter()
                .map(|record| (
                    record.chapter_number(),
                    record.prg_bank(),
                    record.cpu_address()
                ))
                .collect::<Vec<_>>(),
            CHAPTER_MAP_SOURCE_SPECS
                .iter()
                .enumerate()
                .map(|(index, spec)| (index as u8 + 1, spec.prg_bank, spec.cpu_address))
                .collect::<Vec<_>>()
        );
        assert_eq!(
            records
                .iter()
                .map(|record| (record.row_count(), record.column_count()))
                .max(),
            Some((30, 32))
        );
    }

    #[test]
    fn rejects_a_pointer_that_no_longer_selects_its_map_record() {
        let mut prg = fixture_prg();
        let offset =
            chapter_map_prg_offset(EARLY_CHAPTER_MAP_BANK, EARLY_CHAPTER_MAP_POINTER_TABLE)
                .unwrap();
        prg[offset..offset + 2].copy_from_slice(&0x801B_u16.to_le_bytes());

        assert!(
            bind_chapter_map_source_records(&prg)
                .unwrap_err()
                .to_string()
                .contains("pointer table changed")
        );
    }

    #[test]
    fn rejects_dimensions_that_no_longer_end_at_the_next_record() {
        let mut prg = fixture_prg();
        let offset = chapter_map_prg_offset(0x02, 0x801A).unwrap();
        prg[offset] = 0x0D;

        assert!(
            bind_chapter_map_source_records(&prg)
                .unwrap_err()
                .to_string()
                .contains("header changed")
        );
    }
}
