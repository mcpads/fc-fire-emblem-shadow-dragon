use std::collections::BTreeSet;

use crate::rom::PRG_SIZE;

use super::{
    PRG_BANK_SIZE, TERRAIN_NAME_COUNT, dialogue_false_positives, false_positive_regions,
    pointer_reference_census::{
        TERRAIN_POINTER_REFERENCES, TERRAIN_POINTER_TABLE, TERRAIN_POINTER_TABLE_START,
        bind_terrain_pointer_reference_candidates, bind_terrain_record_pointer_candidates,
        bind_terrain_record_pointer_pair_census, bind_unique_terrain_pointer_table,
        expected_raw_candidates, scan_terrain_pointer_reference_candidates,
        scan_terrain_record_pointer_candidates, terrain_record_targets,
    },
    terrain_population_ids,
};

fn put(prg: &mut [u8], bank: u8, address: u16, bytes: &[u8]) {
    let cpu_base: u16 = if bank == 0x0F { 0xC000 } else { 0x8000 };
    let offset = usize::from(bank) * PRG_BANK_SIZE + usize::from(address - cpu_base);
    prg[offset..offset + bytes.len()].copy_from_slice(bytes);
}

fn synthetic_prg() -> Vec<u8> {
    let mut prg = vec![0xFF; PRG_SIZE];
    for reference in TERRAIN_POINTER_REFERENCES {
        let [low, high] = reference.target.to_le_bytes();
        put(
            &mut prg,
            reference.prg_bank,
            reference.cpu_address,
            &[reference.opcode, low, high],
        );
    }
    false_positive_regions::populate_synthetic_false_positive_regions(&mut prg);
    put(
        &mut prg,
        0x0F,
        TERRAIN_POINTER_TABLE_START,
        &TERRAIN_POINTER_TABLE,
    );
    prg
}

#[test]
fn binds_exact_typed_terrain_pointer_reference_population() {
    let prg = synthetic_prg();
    assert_eq!(
        scan_terrain_pointer_reference_candidates(&prg).unwrap(),
        expected_raw_candidates()
    );
    false_positive_regions::bind_title_followup_stream_false_positive(&prg).unwrap();
    bind_unique_terrain_pointer_table(&prg).unwrap();
    bind_terrain_record_pointer_pair_census(&prg).unwrap();
    assert_eq!(
        terrain_population_ids(TERRAIN_NAME_COUNT),
        (0..TERRAIN_NAME_COUNT)
            .map(|index| format!("terrain-names:{index:03}"))
            .collect::<Vec<_>>()
    );
}

#[test]
fn rejects_missing_extra_and_wrong_typed_terrain_pointer_references() {
    let mut missing = synthetic_prg();
    let second = TERRAIN_POINTER_REFERENCES[1];
    let second_offset =
        usize::from(second.prg_bank) * PRG_BANK_SIZE + usize::from(second.cpu_address - 0x8000);
    missing[second_offset..second_offset + 3].fill(0xFF);
    assert!(
        bind_terrain_pointer_reference_candidates(
            &scan_terrain_pointer_reference_candidates(&missing).unwrap()
        )
        .is_err()
    );

    let mut extra = synthetic_prg();
    extra[0x20..0x23].copy_from_slice(&[0xAD, 0xF3, 0xE5]);
    assert!(
        bind_terrain_pointer_reference_candidates(
            &scan_terrain_pointer_reference_candidates(&extra).unwrap()
        )
        .is_err()
    );

    let mut wrong_typed = synthetic_prg();
    let first = TERRAIN_POINTER_REFERENCES[0];
    let first_offset =
        usize::from(first.prg_bank) * PRG_BANK_SIZE + usize::from(first.cpu_address - 0x8000);
    wrong_typed[first_offset] = 0xAD;
    assert!(
        bind_terrain_pointer_reference_candidates(
            &scan_terrain_pointer_reference_candidates(&wrong_typed).unwrap()
        )
        .is_err()
    );
}

#[test]
fn rejects_duplicate_table_and_unclassified_record_pointer_root() {
    let mut duplicate_table = synthetic_prg();
    duplicate_table[0x200..0x200 + TERRAIN_POINTER_TABLE.len()]
        .copy_from_slice(&TERRAIN_POINTER_TABLE);
    assert!(bind_unique_terrain_pointer_table(&duplicate_table).is_err());

    let mut extra_record_root = synthetic_prg();
    extra_record_root[0x300..0x302].copy_from_slice(&0xE611_u16.to_le_bytes());
    let record_targets = terrain_record_targets();
    assert!(
        bind_terrain_record_pointer_candidates(
            &scan_terrain_record_pointer_candidates(&extra_record_root, &record_targets).unwrap()
        )
        .is_err()
    );
}

#[test]
fn rejects_drifted_title_stream_and_dialogue_false_positive_identity() {
    let mut broken_stream = synthetic_prg();
    put(
        &mut broken_stream,
        false_positive_regions::TITLE_STREAM_RAW_CANDIDATE.prg_bank,
        false_positive_regions::TITLE_FOLLOWUP_STREAM_ADDRESS + 24,
        &[0x01],
    );
    assert!(
        false_positive_regions::bind_title_followup_stream_false_positive(&broken_stream).is_err()
    );

    let expected = dialogue_false_positives::expected_dialogue_data_false_positive_identities();
    dialogue_false_positives::bind_dialogue_data_false_positive_identities(&expected).unwrap();

    let mut missing = expected.clone();
    missing.pop_first();
    assert!(
        dialogue_false_positives::bind_dialogue_data_false_positive_identities(&missing).is_err()
    );

    let mut drifted = expected;
    let mut first = drifted.pop_first().unwrap();
    first.storage_sha1 = "drifted".to_owned();
    drifted.insert(first);
    assert!(
        dialogue_false_positives::bind_dialogue_data_false_positive_identities(&drifted).is_err()
    );
    assert!(dialogue_false_positives::mismatched_dialogue_candidate_is_rejected());
}

#[test]
fn record_target_population_is_unique_and_complete() {
    assert_eq!(terrain_record_targets().len(), TERRAIN_NAME_COUNT);
    assert_eq!(
        terrain_record_targets(),
        TERRAIN_POINTER_TABLE
            .chunks_exact(2)
            .map(|bytes| u16::from_le_bytes([bytes[0], bytes[1]]))
            .collect::<BTreeSet<_>>()
    );
}
