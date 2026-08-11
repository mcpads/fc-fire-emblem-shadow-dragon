use std::collections::BTreeSet;

use anyhow::{Context, Result, ensure};
use serde::Serialize;

use crate::{
    rom::{HEADER_SIZE, Rom},
    sha1_hex,
    typed_source::decode_rp2a03_sequence,
};

const PRG_BANK_SIZE: usize = 16 * 1024;
const SWITCHABLE_CPU_START: u16 = 0x8000;
const SWITCHABLE_CPU_END_EXCLUSIVE: u16 = 0xC000;
const FIXED_CPU_START: u16 = 0xC000;
pub(super) const CHAPTER_COUNT: usize = 25;
pub(super) const ENEMY_RECORD_BYTE_COUNT: usize = 11;
const CLASS_STAT_POINTER_TABLE_ADDRESS: u16 = 0xEC04;
const CLASS_STAT_RECORD_ADDRESS: u16 = 0xEC30;
const CLASS_STAT_RECORD_COUNT: usize = 22;
const CLASS_STAT_RECORD_BYTE_COUNT: usize = 9;
const CLASS_BASE_HP_OFFSET: usize = 7;
const CLASS_STAT_POINTER_TABLE_SHA1: &str = "f114f8d67a8ce47a95e7a483db3e92d952245fac";
const CLASS_STAT_RECORDS_SHA1: &str = "d11d5561529dca9eec5d85666e2fca9bbf249ffa";

const INITIAL_ENEMY_POINTER_TABLE: PointerTableSpec = PointerTableSpec {
    role: "initial_enemy_records_by_chapter",
    prg_bank: 0x08,
    cpu_address: 0x8AA3,
    expected_pointer_table_sha1: "b70ffb5873b3e81ff5c5f43598f5466f668f4d7f",
    expected_record_count: 471,
    expected_record_data_sha1: "bcbe74fb0bc58db8144409ecd00d883fbe330531",
};
const REINFORCEMENT_POINTER_TABLE: PointerTableSpec = PointerTableSpec {
    role: "reinforcement_enemy_records_by_chapter",
    prg_bank: 0x03,
    cpu_address: 0x959C,
    expected_pointer_table_sha1: "7e84396a65317ce22f25c342e8957e464806bb9f",
    expected_record_count: 93,
    expected_record_data_sha1: "2e663ba686d9d7972f0428f9589149343153c354",
};
const INITIAL_ENEMY_LOADER: RoutineSpec = RoutineSpec {
    role: "load_initial_enemy_records_for_chapter",
    prg_bank: 0x08,
    cpu_address: 0xBB85,
    byte_count: 0x6F,
    expected_sha1: "8780fce3d3f272f0b768614c25d912c289a418a0",
};
const INITIAL_ENEMY_RECORD_BUILDER: RoutineSpec = RoutineSpec {
    role: "initialize_enemy_unit_from_initial_record",
    prg_bank: 0x08,
    cpu_address: 0xBBF4,
    byte_count: 0x8D,
    expected_sha1: "0e1802fa2310cffcaafa7e7be817586f88bf0239",
};
const REINFORCEMENT_SELECTOR: RoutineSpec = RoutineSpec {
    role: "select_reinforcement_enemy_record",
    prg_bank: 0x03,
    cpu_address: 0x91D0,
    byte_count: 0xA1,
    expected_sha1: "055988d54cadf98073021f476234d6f003112dec",
};
const REINFORCEMENT_RECORD_BUILDER: RoutineSpec = RoutineSpec {
    role: "initialize_enemy_unit_from_reinforcement_record",
    prg_bank: 0x03,
    cpu_address: 0x9271,
    byte_count: 0xB9,
    expected_sha1: "65f06ab741b8c5d2f75ba433742059517d2aae12",
};
const INITIAL_LOADER_POINTER_FRAGMENT: [u8; 9] =
    [0xB9, 0xA3, 0x8A, 0x85, 0x76, 0xB9, 0xA4, 0x8A, 0x85];
const REINFORCEMENT_POINTER_FRAGMENT: [u8; 9] =
    [0xB9, 0x9C, 0x95, 0x85, 0x76, 0xB9, 0x9D, 0x95, 0x85];
const RECORD_FIELD_COPY_FRAGMENT: [u8; 31] = [
    0xB1, 0x76, 0x8D, 0xF4, 0x76, 0xC8, 0xB1, 0x76, 0x8D, 0xF5, 0x76, 0xC8, 0xB1, 0x76, 0x8D, 0xF6,
    0x76, 0x38, 0xE9, 0x01, 0x4A, 0x85, 0x0B, 0xC8, 0xB1, 0x76, 0x8D, 0x07, 0x77, 0xC8, 0xB1,
];

pub(super) struct EnemySourceDomain {
    pub(super) records: Vec<EnemyRecord>,
    pub(super) initial_records: PointerTableBinding,
    pub(super) reinforcement_records: PointerTableBinding,
    pub(super) initial_loader: SourceRoutineBinding,
    pub(super) initial_record_builder: SourceRoutineBinding,
    pub(super) reinforcement_selector: SourceRoutineBinding,
    pub(super) reinforcement_record_builder: SourceRoutineBinding,
}

#[derive(Clone, Debug, Serialize)]
pub(in crate::mapper165::battle_codebook_plan) struct EnemyGeneratedHpBound {
    class_stat_pointer_count: usize,
    class_stat_pointer_table_sha1: String,
    class_stat_record_byte_count: usize,
    class_stat_records_sha1: String,
    source_record_count: usize,
    maximum_source_level: u8,
    maximum_generated_hp: u8,
    generation_formula: &'static str,
    every_source_record_class_bound: bool,
}

impl EnemyGeneratedHpBound {
    pub(in crate::mapper165::battle_codebook_plan) fn maximum_generated_hp(&self) -> u8 {
        self.maximum_generated_hp
    }
}

#[derive(Debug, Serialize)]
pub(super) struct PointerTableBinding {
    role: &'static str,
    prg_bank: u8,
    cpu_address: u16,
    pointer_count: usize,
    distinct_pointer_count: usize,
    pointer_table_sha1: String,
    record_count: usize,
    record_data_sha1: String,
    all_lists_zero_terminated: bool,
}

#[derive(Debug, Serialize)]
pub(super) struct SourceRoutineBinding {
    role: &'static str,
    prg_bank: u8,
    cpu_address: u16,
    byte_count: usize,
    source_sha1: String,
    typed_instruction_count: usize,
}

#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub(super) struct EnemyRecord {
    pub(super) bytes: [u8; ENEMY_RECORD_BYTE_COUNT],
}

#[derive(Clone, Copy)]
struct PointerTableSpec {
    role: &'static str,
    prg_bank: u8,
    cpu_address: u16,
    expected_pointer_table_sha1: &'static str,
    expected_record_count: usize,
    expected_record_data_sha1: &'static str,
}

#[derive(Clone, Copy)]
struct RoutineSpec {
    role: &'static str,
    prg_bank: u8,
    cpu_address: u16,
    byte_count: usize,
    expected_sha1: &'static str,
}

pub(super) fn bind_enemy_source_domain(rom: &Rom) -> Result<EnemySourceDomain> {
    let initial_loader = bind_routine(rom, INITIAL_ENEMY_LOADER)?;
    let initial_record_builder = bind_routine(rom, INITIAL_ENEMY_RECORD_BUILDER)?;
    let reinforcement_selector = bind_routine(rom, REINFORCEMENT_SELECTOR)?;
    let reinforcement_record_builder = bind_routine(rom, REINFORCEMENT_RECORD_BUILDER)?;
    ensure!(
        source_slice(rom, INITIAL_ENEMY_LOADER)?
            .windows(INITIAL_LOADER_POINTER_FRAGMENT.len())
            .any(|window| window == INITIAL_LOADER_POINTER_FRAGMENT),
        "initial enemy loader no longer reads the declared chapter pointer table"
    );
    ensure!(
        source_slice(rom, REINFORCEMENT_SELECTOR)?
            .windows(REINFORCEMENT_POINTER_FRAGMENT.len())
            .any(|window| window == REINFORCEMENT_POINTER_FRAGMENT),
        "reinforcement selector no longer reads the declared chapter pointer table"
    );
    for spec in [INITIAL_ENEMY_RECORD_BUILDER, REINFORCEMENT_RECORD_BUILDER] {
        ensure!(
            source_slice(rom, spec)?
                .windows(RECORD_FIELD_COPY_FRAGMENT.len())
                .any(|window| window == RECORD_FIELD_COPY_FRAGMENT),
            "{} no longer copies identity, class, level, and first item from the compact record",
            spec.role
        );
    }
    let (initial, initial_records) = bind_pointer_table(rom, INITIAL_ENEMY_POINTER_TABLE)?;
    let (reinforcements, reinforcement_records) =
        bind_pointer_table(rom, REINFORCEMENT_POINTER_TABLE)?;
    let records = initial
        .into_iter()
        .chain(reinforcements)
        .collect::<Vec<_>>();
    Ok(EnemySourceDomain {
        records,
        initial_records,
        reinforcement_records,
        initial_loader,
        initial_record_builder,
        reinforcement_selector,
        reinforcement_record_builder,
    })
}

pub(in crate::mapper165::battle_codebook_plan) fn bind_enemy_generated_hp_bound(
    rom: &Rom,
) -> Result<EnemyGeneratedHpBound> {
    let source = bind_enemy_source_domain(rom)?;
    let fixed = prg_bank(rom, 0x0F)?;
    let pointer_offset = usize::from(CLASS_STAT_POINTER_TABLE_ADDRESS - FIXED_CPU_START);
    let pointer_byte_count = CLASS_STAT_RECORD_COUNT * 2;
    let pointer_bytes = fixed
        .get(pointer_offset..pointer_offset + pointer_byte_count)
        .context("enemy class-stat pointer table is outside fixed PRG")?;
    ensure!(
        sha1_hex(pointer_bytes) == CLASS_STAT_POINTER_TABLE_SHA1,
        "enemy class-stat pointer table changed"
    );
    let pointers = pointer_bytes
        .chunks_exact(2)
        .map(|pair| u16::from_le_bytes([pair[0], pair[1]]))
        .collect::<Vec<_>>();
    let expected_pointers = (0..CLASS_STAT_RECORD_COUNT)
        .map(|index| CLASS_STAT_RECORD_ADDRESS + (index * CLASS_STAT_RECORD_BYTE_COUNT) as u16)
        .collect::<Vec<_>>();
    ensure!(
        pointers == expected_pointers,
        "enemy class-stat records are no longer contiguous"
    );
    let record_offset = usize::from(CLASS_STAT_RECORD_ADDRESS - FIXED_CPU_START);
    let class_byte_count = CLASS_STAT_RECORD_COUNT * CLASS_STAT_RECORD_BYTE_COUNT;
    let classes = fixed
        .get(record_offset..record_offset + class_byte_count)
        .context("enemy class-stat records are outside fixed PRG")?;
    ensure!(
        sha1_hex(classes) == CLASS_STAT_RECORDS_SHA1,
        "enemy class-stat records changed"
    );

    let mut maximum_source_level = 0_u8;
    let mut maximum_generated_hp = 0_u8;
    for record in &source.records {
        let class_index = usize::from(
            record.bytes[1]
                .checked_sub(1)
                .context("enemy source record has class zero")?,
        );
        let class = classes
            .get(
                class_index * CLASS_STAT_RECORD_BYTE_COUNT
                    ..(class_index + 1) * CLASS_STAT_RECORD_BYTE_COUNT,
            )
            .context("enemy source record class exceeds the class-stat table")?;
        let level = record.bytes[2];
        ensure!(level > 0, "enemy source record has level zero");
        let half_level_bonus = (level - 1) >> 1;
        let hp = class[CLASS_BASE_HP_OFFSET]
            .checked_add(
                half_level_bonus
                    .checked_mul(3)
                    .context("enemy generated HP bonus overflow")?,
            )
            .context("enemy generated HP overflow")?;
        maximum_source_level = maximum_source_level.max(level);
        maximum_generated_hp = maximum_generated_hp.max(hp);
    }
    ensure!(
        maximum_source_level == 20 && maximum_generated_hp == 45,
        "enemy generated HP domain changed"
    );

    Ok(EnemyGeneratedHpBound {
        class_stat_pointer_count: pointers.len(),
        class_stat_pointer_table_sha1: sha1_hex(pointer_bytes),
        class_stat_record_byte_count: classes.len(),
        class_stat_records_sha1: sha1_hex(classes),
        source_record_count: source.records.len(),
        maximum_source_level,
        maximum_generated_hp,
        generation_formula: "class_base_hp + 3 * ((level - 1) >> 1)",
        every_source_record_class_bound: true,
    })
}

fn bind_pointer_table(
    rom: &Rom,
    spec: PointerTableSpec,
) -> Result<(Vec<EnemyRecord>, PointerTableBinding)> {
    let bank = prg_bank(rom, spec.prg_bank)?;
    let table_offset = cpu_offset(spec.cpu_address)?;
    let table_byte_count = CHAPTER_COUNT * 2;
    let table = bank
        .get(table_offset..table_offset + table_byte_count)
        .with_context(|| format!("{} pointer table is outside its PRG bank", spec.role))?;
    let pointer_table_sha1 = sha1_hex(table);
    ensure!(
        pointer_table_sha1 == spec.expected_pointer_table_sha1,
        "{} pointer table changed: expected {}, found {}",
        spec.role,
        spec.expected_pointer_table_sha1,
        pointer_table_sha1
    );
    let pointers = table
        .chunks_exact(2)
        .map(|bytes| u16::from_le_bytes([bytes[0], bytes[1]]))
        .collect::<Vec<_>>();
    let records = parse_record_lists(bank, &pointers, spec.role)?;
    ensure!(
        records.len() == spec.expected_record_count,
        "{} record count changed: expected {}, found {}",
        spec.role,
        spec.expected_record_count,
        records.len()
    );
    let record_bytes = records
        .iter()
        .flat_map(|record| record.bytes)
        .collect::<Vec<_>>();
    let record_data_sha1 = sha1_hex(&record_bytes);
    ensure!(
        record_data_sha1 == spec.expected_record_data_sha1,
        "{} record data changed: expected {}, found {}",
        spec.role,
        spec.expected_record_data_sha1,
        record_data_sha1
    );
    let distinct_pointer_count = pointers.iter().copied().collect::<BTreeSet<_>>().len();
    Ok((
        records,
        PointerTableBinding {
            role: spec.role,
            prg_bank: spec.prg_bank,
            cpu_address: spec.cpu_address,
            pointer_count: pointers.len(),
            distinct_pointer_count,
            pointer_table_sha1,
            record_count: spec.expected_record_count,
            record_data_sha1,
            all_lists_zero_terminated: true,
        },
    ))
}

fn parse_record_lists(bank: &[u8], pointers: &[u16], role: &str) -> Result<Vec<EnemyRecord>> {
    let mut records = Vec::new();
    for (chapter_index, pointer) in pointers.iter().copied().enumerate() {
        ensure!(
            (SWITCHABLE_CPU_START..SWITCHABLE_CPU_END_EXCLUSIVE).contains(&pointer),
            "{role} chapter {} pointer {pointer:04X} is outside the switchable bank",
            chapter_index + 1
        );
        let mut offset = cpu_offset(pointer)?;
        loop {
            let identity = *bank.get(offset).with_context(|| {
                format!(
                    "{role} chapter {} has no zero terminator",
                    chapter_index + 1
                )
            })?;
            if identity == 0 {
                break;
            }
            let bytes: [u8; ENEMY_RECORD_BYTE_COUNT] = bank
                .get(offset..offset + ENEMY_RECORD_BYTE_COUNT)
                .with_context(|| {
                    format!(
                        "{role} chapter {} record crosses its PRG bank",
                        chapter_index + 1
                    )
                })?
                .try_into()
                .expect("validated compact enemy record length");
            records.push(EnemyRecord { bytes });
            offset = offset
                .checked_add(ENEMY_RECORD_BYTE_COUNT)
                .context("compact enemy record offset overflow")?;
        }
    }
    Ok(records)
}

fn bind_routine(rom: &Rom, spec: RoutineSpec) -> Result<SourceRoutineBinding> {
    let bytes = source_slice(rom, spec)?;
    let source_sha1 = sha1_hex(bytes);
    ensure!(
        source_sha1 == spec.expected_sha1,
        "{} source changed: expected {}, found {}",
        spec.role,
        spec.expected_sha1,
        source_sha1
    );
    let instructions = decode_rp2a03_sequence(bytes, spec.cpu_address, spec.role)?;
    Ok(SourceRoutineBinding {
        role: spec.role,
        prg_bank: spec.prg_bank,
        cpu_address: spec.cpu_address,
        byte_count: spec.byte_count,
        source_sha1,
        typed_instruction_count: instructions.len(),
    })
}

fn source_slice(rom: &Rom, spec: RoutineSpec) -> Result<&[u8]> {
    let bank = prg_bank(rom, spec.prg_bank)?;
    let offset = cpu_offset(spec.cpu_address)?;
    bank.get(offset..offset + spec.byte_count)
        .with_context(|| format!("{} source is outside its PRG bank", spec.role))
}

fn prg_bank(rom: &Rom, bank: u8) -> Result<&[u8]> {
    let start = HEADER_SIZE + usize::from(bank) * PRG_BANK_SIZE;
    rom.data()
        .get(start..start + PRG_BANK_SIZE)
        .with_context(|| format!("PRG bank {bank:02X} is outside the ROM"))
}

fn cpu_offset(address: u16) -> Result<usize> {
    ensure!(
        (SWITCHABLE_CPU_START..SWITCHABLE_CPU_END_EXCLUSIVE).contains(&address),
        "CPU address {address:04X} is outside the switchable PRG window"
    );
    Ok(usize::from(address - SWITCHABLE_CPU_START))
}

#[cfg(test)]
pub(super) fn test_routine(role: &'static str) -> SourceRoutineBinding {
    SourceRoutineBinding {
        role,
        prg_bank: 3,
        cpu_address: 0x8000,
        byte_count: 1,
        source_sha1: "source".to_owned(),
        typed_instruction_count: 1,
    }
}

#[cfg(test)]
pub(super) fn test_table(role: &'static str) -> PointerTableBinding {
    PointerTableBinding {
        role,
        prg_bank: 3,
        cpu_address: 0x8000,
        pointer_count: CHAPTER_COUNT,
        distinct_pointer_count: 1,
        pointer_table_sha1: "pointers".to_owned(),
        record_count: 1,
        record_data_sha1: "records".to_owned(),
        all_lists_zero_terminated: true,
    }
}

#[cfg(test)]
pub(in crate::mapper165::battle_codebook_plan) fn test_hp_bound() -> EnemyGeneratedHpBound {
    EnemyGeneratedHpBound {
        class_stat_pointer_count: CLASS_STAT_RECORD_COUNT,
        class_stat_pointer_table_sha1: "pointers".to_owned(),
        class_stat_record_byte_count: CLASS_STAT_RECORD_COUNT * CLASS_STAT_RECORD_BYTE_COUNT,
        class_stat_records_sha1: "classes".to_owned(),
        source_record_count: 1,
        maximum_source_level: 20,
        maximum_generated_hp: 45,
        generation_formula: "test",
        every_source_record_class_bound: true,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn record_lists_follow_every_pointer_and_stop_on_zero_identity() {
        let mut bank = vec![0; PRG_BANK_SIZE];
        bank[0x100] = 0x81;
        bank[0x101] = 1;
        bank[0x103] = 2;
        bank[0x10B] = 0;
        let records = parse_record_lists(&bank, &[0x8100, 0x810B], "test").unwrap();

        assert_eq!(records.len(), 1);
        assert_eq!(records[0].bytes[0], 0x81);
        assert_eq!(records[0].bytes[3], 2);
    }
}
