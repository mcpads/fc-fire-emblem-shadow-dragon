use super::*;
use crate::rom::PRG_SIZE;

use super::{
    chapter_events::{CHAPTER_MAP_BANK, install_event_and_map_fixture},
    source_regions::{install_source_fixture, prg_offset},
};

#[test]
fn binds_only_the_chapter_seven_castle_record_to_c0_18() {
    let prg = fixture_prg();
    let binding = bind_maximum_dialogue_source_records(&prg, &fixture_maximum()).unwrap();

    assert_eq!(binding.runtime_directory_selector, 0xC0);
    assert_eq!(binding.producer.chapter_number, 7);
    assert_eq!((binding.producer.row, binding.producer.column), (27, 10));
    assert_eq!(binding.producer.terrain_tile_code, 0x4B);
    assert_eq!(binding.producer.selected_main_state, 0x3C);
    assert_eq!(binding.producer.selected_stage, 2);
    assert_eq!(binding.same_entry_other_selector.chapter_number, 11);
    assert_eq!(
        binding
            .same_entry_other_selector
            .selected_directory_selector,
        0x30
    );
    assert!(!binding.screen_lifetime_bound);
    assert!(
        binding
            .source_regions
            .iter()
            .filter(|region| region.region_kind == "rp2a03_code")
            .all(|region| !region.typed_instructions.is_empty())
    );
}

#[test]
fn rejects_a_target_record_that_is_no_longer_on_the_castle() {
    let mut prg = fixture_prg();
    let map_offset = prg_offset(CHAPTER_MAP_BANK, MAXIMUM_PRODUCER_MAP_POINTER).unwrap();
    let column_count = usize::from(MAXIMUM_PRODUCER_MAP_HEADER[1]) + 1;
    prg[map_offset
        + 4
        + usize::from(MAXIMUM_PRODUCER_ROW) * column_count
        + usize::from(MAXIMUM_PRODUCER_COLUMN)] = 0x46;

    assert!(
        bind_maximum_dialogue_source_records(&prg, &fixture_maximum())
            .unwrap_err()
            .to_string()
            .contains("castle tile")
    );
}

fn fixture_maximum() -> MaximumTransitionChainReport {
    MaximumTransitionChainReport {
        start_table_id: TABLE_ID.to_owned(),
        start_canonical_entry_index: ENTRY_INDEX,
        record_count: 1,
        table_ids: vec![TABLE_ID.to_owned()],
        unique_glyph_count: 175,
    }
}

fn fixture_prg() -> Vec<u8> {
    let mut prg = vec![0; PRG_SIZE];
    install_source_fixture(&mut prg);
    install_event_and_map_fixture(&mut prg);
    prg
}
