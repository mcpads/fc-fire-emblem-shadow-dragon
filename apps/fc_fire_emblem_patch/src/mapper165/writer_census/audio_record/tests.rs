use super::{
    continuation_grammar::{AddressedBytes, locate_command_after_nested_return},
    range_contains,
};

#[test]
fn candidate_must_fit_wholly_inside_the_record() {
    assert!(range_contains(0x9000, 7, 0x9000, 3).unwrap());
    assert!(range_contains(0x9000, 7, 0x9004, 3).unwrap());
    assert!(!range_contains(0x9000, 7, 0x9005, 3).unwrap());
    assert!(!range_contains(0x9000, 7, 0x8FFF, 3).unwrap());
}

#[test]
fn nested_fd_fe_return_exposes_the_next_parent_opcode() {
    const BASE: u16 = 0x8000;
    const INNER: u16 = BASE + 0x20;
    const NESTED: u16 = BASE + 0x10;
    let mut bytes = [0_u8; 0x30];
    bytes[..10].copy_from_slice(&[
        0xBE,
        0x55,
        0x82,
        0x7F,
        0xFD,
        NESTED as u8,
        (NESTED >> 8) as u8,
        0xC1,
        0x34,
        0x12,
    ]);
    bytes[0x10..0x16].copy_from_slice(&[0x80, 0xFD, INNER as u8, (INNER >> 8) as u8, 0x01, 0xFE]);
    bytes[0x20..0x22].copy_from_slice(&[0x7F, 0xFE]);
    let boundary =
        locate_command_after_nested_return(&AddressedBytes::new(BASE, &bytes), BASE).unwrap();

    assert_eq!(boundary.deferred_fd_address, BASE + 4);
    assert_eq!(boundary.nested_stream_address, NESTED);
    assert_eq!(boundary.nested_stream_return_address, NESTED + 5);
    assert_eq!(boundary.nested_fd_call_count, 1);
    assert_eq!(boundary.command_address, BASE + 7);
    assert_eq!(boundary.command, 0xC1);
}

#[test]
fn nested_stream_termination_cannot_stand_in_for_an_fe_return() {
    const BASE: u16 = 0x8000;
    const NESTED: u16 = BASE + 0x10;
    let mut bytes = [0_u8; 0x20];
    bytes[..8].copy_from_slice(&[
        0x80,
        0x7F,
        0xFD,
        NESTED as u8,
        (NESTED >> 8) as u8,
        0xC1,
        0x34,
        0x12,
    ]);
    bytes[0x10] = 0xFF;

    let error =
        locate_command_after_nested_return(&AddressedBytes::new(BASE, &bytes), BASE).unwrap_err();

    assert!(
        error
            .to_string()
            .contains("terminates instead of returning with FE")
    );
}
