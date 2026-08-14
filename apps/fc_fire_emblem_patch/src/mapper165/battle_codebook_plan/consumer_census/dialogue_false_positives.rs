use std::collections::BTreeSet;

use anyhow::{Result, ensure};

use crate::{dialogue_inventory::inspect_main_dialogue_storage, rom::HEADER_SIZE};

use super::{AbsoluteOperandCandidate, PRG_BANK_SIZE, RecordPointerCandidate};

#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
struct DialogueLineIdentity {
    table_id: &'static str,
    canonical_entry_index: usize,
    line_index: usize,
    file_offset: usize,
    storage_byte_count: usize,
    storage_sha1: &'static str,
}

#[derive(Clone, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub(super) struct ObservedDialogueLineIdentity {
    pub(super) table_id: &'static str,
    pub(super) canonical_entry_index: usize,
    pub(super) line_index: usize,
    pub(super) file_offset: usize,
    pub(super) storage_byte_count: usize,
    pub(super) storage_sha1: String,
}

const DIALOGUE_DATA_FALSE_POSITIVES: [DialogueLineIdentity; 6] = [
    DialogueLineIdentity {
        table_id: "recruitment-dialogue",
        canonical_entry_index: 34,
        line_index: 4,
        file_offset: 0x1DAFA,
        storage_byte_count: 15,
        storage_sha1: "c22b971c6d901ae96700773b64b814a111e814c3",
    },
    DialogueLineIdentity {
        table_id: "chapter-intro-dialogue",
        canonical_entry_index: 2,
        line_index: 2,
        file_offset: 0x21FED,
        storage_byte_count: 15,
        storage_sha1: "ddbe6e88117e2615df6fca21eb7202b0d6378217",
    },
    DialogueLineIdentity {
        table_id: "chapter-intro-dialogue",
        canonical_entry_index: 48,
        line_index: 4,
        file_offset: 0x22C26,
        storage_byte_count: 16,
        storage_sha1: "de2208124e97900ce1482a8ec6477c333a0f90d2",
    },
    DialogueLineIdentity {
        table_id: "village-and-outro-dialogue",
        canonical_entry_index: 33,
        line_index: 4,
        file_offset: 0x3185A,
        storage_byte_count: 11,
        storage_sha1: "552d6a7ddb1d186e9199036e48505d87999bbd75",
    },
    DialogueLineIdentity {
        table_id: "recruitment-dialogue",
        canonical_entry_index: 47,
        line_index: 4,
        file_offset: 0x1D7A8,
        storage_byte_count: 19,
        storage_sha1: "f432554fb8d4b2d1e3af921d7bcba0b6842b5095",
    },
    DialogueLineIdentity {
        table_id: "recruitment-dialogue",
        canonical_entry_index: 77,
        line_index: 15,
        file_offset: 0x1E2EA,
        storage_byte_count: 11,
        storage_sha1: "05635eb49619c8024986556cd9c3f55b0de87d7c",
    },
];

const DIALOGUE_DATA_RAW_CANDIDATES: [AbsoluteOperandCandidate; 4] = [
    AbsoluteOperandCandidate {
        target: 0xE60F,
        prg_bank: 0x07,
        cpu_address: 0x9AF4,
        opcode: 0x0D,
    },
    AbsoluteOperandCandidate {
        target: 0xE605,
        prg_bank: 0x08,
        cpu_address: 0x9FE7,
        opcode: 0x19,
    },
    AbsoluteOperandCandidate {
        target: 0xE60F,
        prg_bank: 0x08,
        cpu_address: 0xAC21,
        opcode: 0x0E,
    },
    AbsoluteOperandCandidate {
        target: 0xE60F,
        prg_bank: 0x0C,
        cpu_address: 0x9850,
        opcode: 0x0D,
    },
];

const RECORD_POINTER_DIALOGUE_DATA_FALSE_POSITIVES: [RecordPointerCandidate; 2] = [
    RecordPointerCandidate {
        target: 0xE628,
        prg_bank: 0x07,
        cpu_address: 0x97A7,
    },
    RecordPointerCandidate {
        target: 0xE628,
        prg_bank: 0x07,
        cpu_address: 0xA2E1,
    },
];

pub(super) fn absolute_operand_candidates() -> impl Iterator<Item = AbsoluteOperandCandidate> {
    DIALOGUE_DATA_RAW_CANDIDATES.into_iter()
}

pub(super) fn record_pointer_candidates() -> impl Iterator<Item = RecordPointerCandidate> {
    RECORD_POINTER_DIALOGUE_DATA_FALSE_POSITIVES.into_iter()
}

pub(super) fn bind_dialogue_data_false_positives(source: &[u8]) -> Result<()> {
    let inspection = inspect_main_dialogue_storage(source)?;
    let actual = inspection
        .records
        .iter()
        .flat_map(|record| {
            record
                .lines
                .iter()
                .enumerate()
                .map(move |(line_index, line)| ObservedDialogueLineIdentity {
                    table_id: record.table_id,
                    canonical_entry_index: record.canonical_entry_index,
                    line_index,
                    file_offset: line.file_offset,
                    storage_byte_count: line.storage_byte_count,
                    storage_sha1: line.storage_sha1.clone(),
                })
        })
        .filter(|line| {
            DIALOGUE_DATA_FALSE_POSITIVES.iter().any(|expected| {
                expected.table_id == line.table_id
                    && expected.canonical_entry_index == line.canonical_entry_index
                    && expected.line_index == line.line_index
            })
        })
        .collect::<BTreeSet<_>>();
    bind_dialogue_data_false_positive_identities(&actual)?;
    bind_dialogue_candidate_line_membership()?;
    Ok(())
}

fn bind_dialogue_candidate_line_membership() -> Result<()> {
    for (candidate, line) in DIALOGUE_DATA_RAW_CANDIDATES
        .into_iter()
        .zip(DIALOGUE_DATA_FALSE_POSITIVES[..4].iter())
    {
        ensure_candidate_inside_dialogue_line(candidate.prg_bank, candidate.cpu_address, 3, line)?;
    }
    for (candidate, line) in RECORD_POINTER_DIALOGUE_DATA_FALSE_POSITIVES
        .into_iter()
        .zip(DIALOGUE_DATA_FALSE_POSITIVES[4..].iter())
    {
        ensure_candidate_inside_dialogue_line(candidate.prg_bank, candidate.cpu_address, 2, line)?;
    }
    Ok(())
}

fn ensure_candidate_inside_dialogue_line(
    bank: u8,
    address: u16,
    byte_count: usize,
    line: &DialogueLineIdentity,
) -> Result<()> {
    let cpu_base: u16 = if bank == 0x0F { 0xC000 } else { 0x8000 };
    ensure!(
        address >= cpu_base,
        "terrain-table raw dialogue candidate is outside its PRG bank"
    );
    let file_offset =
        HEADER_SIZE + usize::from(bank) * PRG_BANK_SIZE + usize::from(address - cpu_base);
    ensure!(
        file_offset >= line.file_offset
            && file_offset + byte_count <= line.file_offset + line.storage_byte_count,
        "terrain-table raw dialogue candidate is outside its source-bound line"
    );
    Ok(())
}

pub(super) fn expected_dialogue_data_false_positive_identities()
-> BTreeSet<ObservedDialogueLineIdentity> {
    DIALOGUE_DATA_FALSE_POSITIVES
        .into_iter()
        .map(|line| ObservedDialogueLineIdentity {
            table_id: line.table_id,
            canonical_entry_index: line.canonical_entry_index,
            line_index: line.line_index,
            file_offset: line.file_offset,
            storage_byte_count: line.storage_byte_count,
            storage_sha1: line.storage_sha1.to_owned(),
        })
        .collect()
}

pub(super) fn bind_dialogue_data_false_positive_identities(
    actual: &BTreeSet<ObservedDialogueLineIdentity>,
) -> Result<()> {
    let expected = expected_dialogue_data_false_positive_identities();
    ensure!(
        *actual == expected,
        "terrain-table raw dialogue-data false-positive classification changed"
    );
    Ok(())
}

#[cfg(test)]
pub(super) fn populate_synthetic_dialogue_regions(prg: &mut [u8]) {
    for candidate in DIALOGUE_DATA_RAW_CANDIDATES {
        let [low, high] = candidate.target.to_le_bytes();
        put(
            prg,
            candidate.prg_bank,
            candidate.cpu_address,
            &[candidate.opcode, low, high],
        );
    }
    for candidate in RECORD_POINTER_DIALOGUE_DATA_FALSE_POSITIVES {
        put(
            prg,
            candidate.prg_bank,
            candidate.cpu_address,
            &candidate.target.to_le_bytes(),
        );
    }
}

#[cfg(test)]
fn put(prg: &mut [u8], bank: u8, address: u16, bytes: &[u8]) {
    let cpu_base: u16 = if bank == 0x0F { 0xC000 } else { 0x8000 };
    let offset = usize::from(bank) * PRG_BANK_SIZE + usize::from(address - cpu_base);
    prg[offset..offset + bytes.len()].copy_from_slice(bytes);
}

#[cfg(test)]
pub(super) fn mismatched_dialogue_candidate_is_rejected() -> bool {
    let candidate = DIALOGUE_DATA_RAW_CANDIDATES[0];
    ensure_candidate_inside_dialogue_line(
        candidate.prg_bank,
        candidate.cpu_address,
        3,
        &DIALOGUE_DATA_FALSE_POSITIVES[1],
    )
    .is_err()
}
