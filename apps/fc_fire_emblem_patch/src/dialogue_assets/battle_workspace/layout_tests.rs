use super::layout::pack_record_sizes;

#[test]
fn battle_records_move_wholly_to_the_next_owned_segment() {
    let placements = pack_record_sizes(&[8, 5, 4], &[(0x100, 0x10A), (0x120, 0x129)]).unwrap();

    assert_eq!(placements, [0x100, 0x120, 0x125]);
}

#[test]
fn battle_records_never_cross_the_preserved_storage() {
    let error = pack_record_sizes(&[11], &[(0x100, 0x10A), (0x120, 0x12A)]).unwrap_err();

    assert!(error.to_string().contains("crosses preserved storage"));
}

#[test]
fn battle_record_layout_rejects_capacity_exhaustion() {
    let error = pack_record_sizes(&[10, 11], &[(0x100, 0x10A), (0x120, 0x12A)]).unwrap_err();

    assert!(error.to_string().contains("crosses preserved storage"));
}
