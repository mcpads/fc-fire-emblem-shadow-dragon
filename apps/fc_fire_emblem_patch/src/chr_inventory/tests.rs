use super::*;
use crate::rom::HEADER_SIZE;

fn source_with_known_references() -> Vec<u8> {
    let mut source = vec![0_u8; STATUS_LABELS_OFFSET + SOURCE_STATUS_LABELS.len()];
    source[OPTIONS_TABLE_OFFSET..OPTIONS_TABLE_OFFSET + SOURCE_OPTIONS_TABLE.len()]
        .copy_from_slice(&SOURCE_OPTIONS_TABLE);
    source[STATUS_LABELS_OFFSET..STATUS_LABELS_OFFSET + SOURCE_STATUS_LABELS.len()]
        .copy_from_slice(&SOURCE_STATUS_LABELS);
    source
}

#[test]
fn protects_declared_latin_and_confirmed_english_punctuation() {
    let slots = describe_font_page(&vec![0_u8; CHR_PAGE_SIZE]);

    for slot in &slots[0x60..=0x83] {
        assert_eq!(slot.code_assignment, Decision::Protected);
        assert_eq!(slot.tile_reuse, Decision::Protected);
    }
    for code in [0x8D, 0x9B] {
        assert_eq!(slots[code].code_assignment, Decision::Protected);
        assert_eq!(slots[code].tile_reuse, Decision::Protected);
    }
}

#[test]
fn leaves_a_blank_pattern_unresolved_instead_of_calling_it_available() {
    let slots = describe_font_page(&vec![0_u8; CHR_PAGE_SIZE]);
    let slot = &slots[0x95];

    assert_eq!(slot.plane_usage, PlaneUsage::Blank);
    assert_eq!(slot.code_assignment, Decision::Unresolved);
    assert_eq!(slot.tile_reuse, Decision::Unresolved);
    assert!(
        slot.tile_reuse_reasons
            .contains(&"blank pattern is not free-space proof")
    );
}

#[test]
fn reserves_confirmed_composite_layout_codes_from_the_hangul_slot_ceiling() {
    let slots = describe_font_page(&vec![0_u8; CHR_PAGE_SIZE]);
    let ceiling = calculate_active_slot_ceiling(&slots).unwrap();

    assert_eq!(ceiling.total_font_code_count, 256);
    assert_eq!(ceiling.confirmed_protected_code_count, 43);
    assert_eq!(
        ceiling.provisional_layout_reserved_codes,
        [0x0F, 0x1F, 0xFF]
    );
    assert_eq!(ceiling.current_reserved_code_count, 46);
    assert_eq!(ceiling.current_hangul_slot_ceiling, 210);
}

#[test]
fn rejects_a_known_reference_when_its_source_bytes_change() {
    let mut source = source_with_known_references();
    source[STATUS_LABELS_OFFSET] ^= 0x01;

    let error = validate_known_references(&source).unwrap_err().to_string();
    assert!(error.contains("status-label-table bytes changed"));
}

#[test]
fn sheet_contains_all_codes_at_the_requested_scale() {
    let page = vec![0_u8; CHR_PAGE_SIZE];
    let slots = describe_font_page(&page);
    let png = render_font_page_sheet(&page, &slots, 2).unwrap();

    assert_eq!(&png[..8], b"\x89PNG\r\n\x1a\n");
    assert_eq!(u32::from_be_bytes(png[16..20].try_into().unwrap()), 320);
    assert_eq!(u32::from_be_bytes(png[20..24].try_into().unwrap()), 448);
}

#[test]
fn page_summary_separates_storage_planes_and_duplicate_patterns() {
    let mut page = vec![0_u8; CHR_PAGE_SIZE];
    page[TILE_SIZE] = 0x80;
    page[TILE_SIZE * 2 + 8] = 0x80;
    page[TILE_SIZE * 3] = 0x80;
    page[TILE_SIZE * 3 + 8] = 0x80;
    let summary = summarize_page(2, &page);

    assert_eq!(summary.page_index, 2);
    assert_eq!(summary.blank_pattern_count, 253);
    assert_eq!(summary.low_plane_only_count, 1);
    assert_eq!(summary.high_plane_only_count, 1);
    assert_eq!(summary.dual_plane_count, 1);
    assert_eq!(summary.distinct_pattern_count, 4);
}

#[test]
fn direct_jsr_candidate_inventory_preserves_bank_coordinates() {
    let mut prg = vec![0_u8; PRG_SIZE];
    prg[0x0123..0x0126].copy_from_slice(&[0x20, 0xBE, 0xC9]);
    let fixed_call = PRG_SIZE - PRG_BANK_SIZE + 0x0234;
    prg[fixed_call..fixed_call + 3].copy_from_slice(&[0x20, 0xBE, 0xC9]);

    let candidates = find_absolute_transfer_candidates(&prg, 0xC9BE, 0x20);

    assert_eq!(candidates.len(), 2);
    assert_eq!(candidates[0].prg_bank, 0);
    assert_eq!(candidates[0].file_offset, HEADER_SIZE + 0x0123);
    assert_eq!(candidates[0].cpu_address, 0x8123);
    assert_eq!(candidates[1].prg_bank, 15);
    assert_eq!(candidates[1].file_offset, HEADER_SIZE + fixed_call);
    assert_eq!(candidates[1].cpu_address, 0xC234);
}

#[test]
fn absolute_jump_candidates_are_separate_from_jsr_candidates() {
    let mut prg = vec![0_u8; PRG_SIZE];
    prg[0x0123..0x0126].copy_from_slice(&[0x20, 0xC6, 0xC9]);
    prg[0x0456..0x0459].copy_from_slice(&[0x4C, 0xC6, 0xC9]);

    let jsr = find_absolute_transfer_candidates(&prg, 0xC9C6, 0x20);
    let jmp = find_absolute_transfer_candidates(&prg, 0xC9C6, 0x4C);

    assert_eq!(jsr.len(), 1);
    assert_eq!(jsr[0].cpu_address, 0x8123);
    assert_eq!(jmp.len(), 1);
    assert_eq!(jmp[0].cpu_address, 0x8456);
}

#[test]
fn absolute_mapper_write_candidates_preserve_opcode_and_bank_coordinates() {
    let mut prg = vec![0_u8; PRG_SIZE];
    prg[0x0123..0x0126].copy_from_slice(&[0x8D, 0x00, 0xA0]);
    let fixed_write = PRG_SIZE - PRG_BANK_SIZE + 0x0234;
    prg[fixed_write..fixed_write + 3].copy_from_slice(&[0x8C, 0x00, 0xA0]);

    let candidates = find_absolute_write_candidates(&prg, 0xA000);

    assert_eq!(candidates.len(), 2);
    assert_eq!(candidates[0].mnemonic, "sta");
    assert_eq!(candidates[0].prg_bank, 0);
    assert_eq!(candidates[0].cpu_address, 0x8123);
    assert_eq!(candidates[1].mnemonic, "sty");
    assert_eq!(candidates[1].prg_bank, 15);
    assert_eq!(candidates[1].cpu_address, 0xC234);
}

#[test]
fn adjacent_chr_write_groups_keep_short_runs_and_exclude_singletons() {
    let mut prg = vec![0_u8; PRG_SIZE];
    prg[0x0123..0x0126].copy_from_slice(&[0x8D, 0x00, 0xB0]);
    prg[0x0128..0x012B].copy_from_slice(&[0x8E, 0x00, 0xC0]);
    prg[0x0140..0x0143].copy_from_slice(&[0x8C, 0x00, 0xD0]);

    let groups = find_adjacent_chr_write_candidate_groups(&prg);

    assert_eq!(groups.len(), 1);
    assert_eq!(groups[0].start_cpu_address, 0x8123);
    assert_eq!(groups[0].last_cpu_address, 0x8128);
    assert_eq!(groups[0].instruction_count, 2);
    assert_eq!(groups[0].largest_gap_byte_count, 2);
    assert_eq!(groups[0].writes[0].register_address, 0xB000);
    assert_eq!(groups[0].writes[1].register_address, 0xC000);
}

#[test]
fn adjacent_chr_write_groups_do_not_cross_prg_banks() {
    let mut prg = vec![0_u8; PRG_SIZE];
    let bank_end = PRG_BANK_SIZE - 3;
    prg[bank_end..bank_end + 3].copy_from_slice(&[0x8D, 0x00, 0xB0]);
    prg[PRG_BANK_SIZE..PRG_BANK_SIZE + 3].copy_from_slice(&[0x8D, 0x00, 0xC0]);

    assert!(find_adjacent_chr_write_candidate_groups(&prg).is_empty());
}

#[test]
fn writer_inventory_rejects_a_changed_fixed_bank_routine() {
    let mut prg = vec![0_u8; PRG_SIZE];
    for writer in &MMC4_CHR_WRITERS {
        let offset = fixed_bank_prg_offset(writer.cpu_address).unwrap();
        prg[offset..offset + writer.expected.len()].copy_from_slice(&writer.expected);
    }
    describe_mmc4_chr_writers(&prg).unwrap();

    let offset = fixed_bank_prg_offset(0xC9BE).unwrap();
    prg[offset] ^= 0x01;
    let error = describe_mmc4_chr_writers(&prg).unwrap_err().to_string();

    assert!(error.contains("MMC4 CHR writer at $C9BE changed"));
}

#[test]
fn control_inventory_rejects_a_changed_prg_bank_routine() {
    let mut prg = vec![0_u8; PRG_SIZE];
    for routine in &MMC4_CONTROL_ROUTINES {
        let offset = fixed_bank_prg_offset(routine.cpu_address).unwrap();
        prg[offset..offset + routine.expected.len()].copy_from_slice(routine.expected);
    }
    describe_mmc4_control_routines(&prg).unwrap();

    let offset = fixed_bank_prg_offset(0xC9A6).unwrap();
    prg[offset + 4] ^= 0x01;
    let error = describe_mmc4_control_routines(&prg)
        .unwrap_err()
        .to_string();

    assert!(error.contains("MMC4 control routine select_prg_bank_and_update_shadows"));
}
