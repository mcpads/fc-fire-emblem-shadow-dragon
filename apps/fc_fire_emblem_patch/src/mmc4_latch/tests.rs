use super::*;

#[test]
fn trigger_tile_uses_the_previous_bank_and_changes_following_tiles() {
    let mut nametable = vec![0; NAMETABLE_PAGE_LEN];
    nametable[1] = 0xFD;
    nametable[3] = 0xFE;

    let projection = project_attributes(&nametable, 0x00, 0x18, Mmc4Latch::Fe).unwrap();

    assert_eq!(&projection.bytes[..5], &[0x18, 0x18, 0x00, 0x00, 0x18]);
    assert_eq!(projection.fd_trigger_count, 1);
    assert_eq!(projection.fe_trigger_count, 1);
    assert_eq!(projection.fd_tile_occurrence_count, 2);
    assert_eq!(projection.fe_tile_occurrence_count, TILE_COUNT - 2);
    assert_eq!(
        projection.fd_tile_occurrences,
        BTreeMap::from([(0x00, 1), (0xFE, 1)])
    );
    assert_eq!(
        projection.fe_tile_occurrences,
        BTreeMap::from([(0x00, TILE_COUNT - 3), (0xFD, 1)])
    );
    assert_eq!(projection.ending_latch, Mmc4Latch::Fe);
}

#[test]
fn tile_usage_is_sorted_and_bound_to_the_latch_before_each_trigger() {
    let mut nametable = vec![0x20; NAMETABLE_PAGE_LEN];
    nametable[0] = 0xFD;
    nametable[1] = 0x31;
    nametable[2] = 0x10;
    nametable[3] = 0xFE;
    nametable[4] = 0x42;

    let projection = project_attributes(&nametable, 0x00, 0x15, Mmc4Latch::Fe).unwrap();

    assert_eq!(projection.fd_tile_occurrence_count, 3);
    assert_eq!(projection.fe_tile_occurrence_count, TILE_COUNT - 3);
    assert_eq!(
        hex_codes(&projection.fd_tile_occurrences),
        ["10", "31", "FE"]
    );
    assert_eq!(
        hex_codes(&projection.fe_tile_occurrences),
        ["20", "42", "FD"]
    );
    assert_eq!(projection.fd_tile_occurrences[&0x10], 1);
    assert_eq!(projection.fd_tile_occurrences[&0x31], 1);
    assert_eq!(projection.fd_tile_occurrences[&0xFE], 1);
}

#[test]
fn original_attribute_quadrants_are_preserved_in_the_exram_high_bits() {
    let mut nametable = vec![0; NAMETABLE_PAGE_LEN];
    nametable[ATTRIBUTE_TABLE_OFFSET] = 0b11_10_01_00;

    let projection = project_attributes(&nametable, 0x07, 0x18, Mmc4Latch::Fd).unwrap();

    assert_eq!(projection.bytes[0], 0x07);
    assert_eq!(projection.bytes[2], 0x47);
    assert_eq!(projection.bytes[TILE_COLUMN_COUNT * 2], 0x87);
    assert_eq!(projection.bytes[TILE_COLUMN_COUNT * 2 + 2], 0xC7);
}

#[test]
fn ppu_transfers_update_the_selected_physical_nametable_before_projection() {
    let mut nametable = vec![0; NAMETABLE_PAGE_LEN];
    nametable[1] = 0xFD;
    nametable[3] = 0xFE;
    nametable[ATTRIBUTE_TABLE_OFFSET] = 0b11_10_01_00;
    let expected = project_attributes(&nametable, 0x00, 0x18, Mmc4Latch::Fe)
        .unwrap()
        .bytes;
    let mut shadow = Mmc4NametableShadow::filled(0xFF);

    let write_count = shadow
        .apply_ppu_transfer(
            0x2400,
            PpuAddressIncrement::Across,
            &nametable,
            NametableMirroring::Vertical,
        )
        .unwrap();
    let actual = shadow
        .project_zero_scroll_attributes(1, NametableMirroring::Vertical, 0x00, 0x18, Mmc4Latch::Fe)
        .unwrap();

    assert_eq!(write_count, NAMETABLE_PAGE_LEN);
    assert_eq!(actual, expected);
}

#[test]
fn mirroring_selects_the_physical_page_that_owns_a_logical_write() {
    let mut horizontal = Mmc4NametableShadow::filled(0);
    horizontal
        .apply_ppu_transfer(
            0x2400,
            PpuAddressIncrement::Across,
            &[0xFD],
            NametableMirroring::Horizontal,
        )
        .unwrap();
    let horizontal_page_zero = horizontal
        .project_zero_scroll_attributes(
            0,
            NametableMirroring::Horizontal,
            0x00,
            0x18,
            Mmc4Latch::Fe,
        )
        .unwrap();

    let mut vertical = Mmc4NametableShadow::filled(0);
    vertical
        .apply_ppu_transfer(
            0x2400,
            PpuAddressIncrement::Across,
            &[0xFD],
            NametableMirroring::Vertical,
        )
        .unwrap();
    let vertical_page_zero = vertical
        .project_zero_scroll_attributes(0, NametableMirroring::Vertical, 0x00, 0x18, Mmc4Latch::Fe)
        .unwrap();

    assert_eq!(&horizontal_page_zero[..2], &[0x18, 0x00]);
    assert_eq!(&vertical_page_zero[..2], &[0x18, 0x18]);
}

#[test]
fn vertical_ppu_increment_changes_latch_state_between_rows() {
    let mut shadow = Mmc4NametableShadow::filled(0);
    shadow
        .apply_ppu_transfer(
            0x2000,
            PpuAddressIncrement::Down,
            &[0xFD, 0xFE],
            NametableMirroring::Vertical,
        )
        .unwrap();

    let projection = shadow
        .project_zero_scroll_attributes(0, NametableMirroring::Vertical, 0x00, 0x18, Mmc4Latch::Fe)
        .unwrap();

    assert_eq!(projection[0], 0x18);
    assert!(
        projection[1..=TILE_COLUMN_COUNT]
            .iter()
            .all(|byte| *byte == 0)
    );
    assert_eq!(projection[TILE_COLUMN_COUNT + 1], 0x18);
}

#[test]
fn attribute_table_writes_update_palette_bits_without_changing_chr_banks() {
    let mut shadow = Mmc4NametableShadow::filled(0);
    shadow
        .apply_ppu_transfer(
            0x23C0,
            PpuAddressIncrement::Across,
            &[0b11_10_01_00],
            NametableMirroring::Vertical,
        )
        .unwrap();

    let projection = shadow
        .project_zero_scroll_attributes(0, NametableMirroring::Vertical, 0x07, 0x18, Mmc4Latch::Fd)
        .unwrap();

    assert_eq!(projection[0] & 0x3F, 0x07);
    assert_eq!(projection[2], 0x47);
    assert_eq!(projection[TILE_COLUMN_COUNT * 2], 0x87);
    assert_eq!(projection[TILE_COLUMN_COUNT * 2 + 2], 0xC7);
}

#[test]
fn mirrored_ppu_addresses_update_nametables_and_palette_addresses_do_not() {
    let mut shadow = Mmc4NametableShadow::filled(0);
    let mirrored_count = shadow
        .apply_ppu_transfer(
            0x3000,
            PpuAddressIncrement::Across,
            &[0xFD],
            NametableMirroring::Vertical,
        )
        .unwrap();
    let palette_count = shadow
        .apply_ppu_transfer(
            0x3F00,
            PpuAddressIncrement::Across,
            &[0xFE],
            NametableMirroring::Vertical,
        )
        .unwrap();
    let projection = shadow
        .project_zero_scroll_attributes(0, NametableMirroring::Vertical, 0x00, 0x18, Mmc4Latch::Fe)
        .unwrap();

    assert_eq!(mirrored_count, 1);
    assert_eq!(palette_count, 0);
    assert_eq!(&projection[..2], &[0x18, 0x00]);
}

#[test]
fn ppu_transfer_replay_input_is_typed_and_fail_closed() {
    let input: PpuTransferReplayInput = serde_json::from_str(
        r#"{
                "schema": 1,
                "initial_nametable_byte": 255,
                "mirroring": "horizontal",
                "selected_logical_nametable": 1,
                "fd_bank": 0,
                "fe_bank": 24,
                "initial_latch": "fe",
                "transfers": [{
                    "start_address": 8192,
                    "increment": "down",
                    "data_hex": "FDfe"
                }]
            }"#,
    )
    .unwrap();

    assert_eq!(input.mirroring, NametableMirroring::Horizontal);
    assert_eq!(input.transfers[0].increment, PpuAddressIncrement::Down);
    assert_eq!(
        decode_hex(&input.transfers[0].data_hex).unwrap(),
        [0xFD, 0xFE]
    );
    assert!(decode_hex("f").is_err());
    assert!(decode_hex("fg").is_err());
    assert!(
        serde_json::from_str::<PpuTransferReplayInput>(
            r#"{
                    "schema": 1,
                    "initial_nametable_byte": 0,
                    "mirroring": "vertical",
                    "selected_logical_nametable": 0,
                    "fd_bank": 0,
                    "fe_bank": 24,
                    "initial_latch": "fd",
                    "transfers": [],
                    "unexpected": true
                }"#,
        )
        .is_err()
    );
}

#[test]
fn unused_attribute_tail_is_filled_with_the_initial_latch_bank() {
    let nametable = vec![0; NAMETABLE_PAGE_LEN];

    let projection = project_attributes(&nametable, 0x07, 0x18, Mmc4Latch::Fe).unwrap();

    assert!(
        projection.bytes[TILE_COUNT..]
            .iter()
            .all(|byte| *byte == 0x18)
    );
}

#[test]
fn input_size_index_and_six_bit_bank_limits_are_fail_closed() {
    assert!(select_nametable_page(&vec![0; NAMETABLE_PAGE_LEN], 1).is_err());
    assert!(select_nametable_page(&vec![0; NAMETABLE_PAGE_LEN * 2], 2).is_err());
    assert!(select_nametable_page(&vec![0; 1000], 0).is_err());
    assert!(project_attributes(&vec![0; NAMETABLE_PAGE_LEN], 0x40, 0x18, Mmc4Latch::Fe).is_err());
}
