use serde::Serialize;

use super::{CodeLocation, SourceRegionSpec, location};

pub(super) const ROSTER_BUFFER_ADDRESS: u16 = 0x906A;
pub(super) const ROSTER_RECORD_CAPACITY: usize = 54;
pub(super) const ROSTER_RECORD_STRIDE: u8 = 0x1B;
pub(super) const ROSTER_IDENTITY_OFFSET: u8 = 0x00;
pub(super) const ROSTER_LOCATION_OFFSET: u8 = 0x0E;
pub(super) const ROSTER_ACTION_OFFSET: u8 = 0x12;
pub(super) const INACTIVE_ACTION_VALUE: u8 = 0xFF;

pub(super) const SOURCE_REGIONS: &[SourceRegionSpec] = &[
    SourceRegionSpec::code_sha1(
        "call_inactive_unit_location_recorder",
        0x06,
        0xB5C8,
        0x11,
        "77cc4d0817bb51b72f44d4552caaa6e9a843bb1d",
    ),
    SourceRegionSpec::code_sha1(
        "record_inactive_unit_locations",
        0x06,
        0xB67B,
        0x22,
        "7f89c168cb33e2425af50f7f68cab47ac2e67c4b",
    ),
];

#[derive(Debug, Serialize)]
pub(super) struct UnitRecordHistoryContract {
    role: &'static str,
    chapter_transition_outer_state: u8,
    chapter_transition_outer_state_hex: &'static str,
    chapter_transition_main_state: u8,
    chapter_transition_main_state_hex: &'static str,
    current_chapter_address: u16,
    current_chapter_address_hex: &'static str,
    roster_buffer_address: u16,
    roster_buffer_address_hex: &'static str,
    roster_record_capacity: usize,
    roster_record_stride: u8,
    roster_record_stride_hex: &'static str,
    identity_offset: u8,
    identity_offset_hex: &'static str,
    location_offset: u8,
    location_offset_hex: &'static str,
    action_offset: u8,
    action_offset_hex: &'static str,
    inactive_action_value: u8,
    inactive_action_value_hex: &'static str,
    caller: CodeLocation,
    producer: CodeLocation,
    producer_effect: &'static str,
    ending_consumer: CodeLocation,
    semantic_boundary: &'static str,
}

pub(super) fn unit_record_history_contract() -> UnitRecordHistoryContract {
    UnitRecordHistoryContract {
        role: "inactive_unit_record_chapter_history",
        chapter_transition_outer_state: 0x0D,
        chapter_transition_outer_state_hex: "0x0D",
        chapter_transition_main_state: 0x00,
        chapter_transition_main_state_hex: "0x00",
        current_chapter_address: 0x7674,
        current_chapter_address_hex: "0x7674",
        roster_buffer_address: ROSTER_BUFFER_ADDRESS,
        roster_buffer_address_hex: "0x906A",
        roster_record_capacity: ROSTER_RECORD_CAPACITY,
        roster_record_stride: ROSTER_RECORD_STRIDE,
        roster_record_stride_hex: "0x1B",
        identity_offset: ROSTER_IDENTITY_OFFSET,
        identity_offset_hex: "0x00",
        location_offset: ROSTER_LOCATION_OFFSET,
        location_offset_hex: "0x0E",
        action_offset: ROSTER_ACTION_OFFSET,
        action_offset_hex: "0x12",
        inactive_action_value: INACTIVE_ACTION_VALUE,
        inactive_action_value_hex: "0xFF",
        caller: location(0x06, 0xB5C8),
        producer: location(0x06, 0xB67B),
        producer_effect: "chapter-transition main state 0 scans 27-byte roster records until identity 0 and writes the current chapter from 0x7674 to offset 0x0E of every record whose action byte at offset 0x12 is 0xFF",
        ending_consumer: location(0x04, 0xA1DE),
        semantic_boundary: "source and gameplay evidence establish 0xFF as inactive or defeated and establish the chapter-history write; they do not prove that every inactive record represents character death",
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unit_record_layout_matches_ending_selector_scan() {
        let contract = unit_record_history_contract();

        assert_eq!(contract.roster_buffer_address, ROSTER_BUFFER_ADDRESS);
        assert_eq!(contract.roster_record_capacity, ROSTER_RECORD_CAPACITY);
        assert_eq!(contract.roster_record_stride, ROSTER_RECORD_STRIDE);
        assert_eq!(contract.action_offset, ROSTER_ACTION_OFFSET);
        assert_eq!(contract.inactive_action_value, INACTIVE_ACTION_VALUE);
    }
}
