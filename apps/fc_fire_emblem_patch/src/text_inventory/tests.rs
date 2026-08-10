use super::*;

fn synthetic_spec(table_file_offset: usize, consumer_file_offset: usize) -> TextTableSpec {
    let table_cpu_address = fixed_file_to_cpu_address(table_file_offset).unwrap();
    let [low, high] = table_cpu_address.to_le_bytes();
    let [next_low, next_high] = (table_cpu_address + 1).to_le_bytes();
    TextTableSpec {
        id: "synthetic-names",
        role: "synthetic names",
        table_file_offset,
        pointer_count: 2,
        terminator: 0xEF,
        consumer_file_offset,
        consumer_bytes: [
            0xB9, low, high, 0x85, 0x00, 0xB9, next_low, next_high, 0x85, 0x01,
        ],
        transfer: TextTransferSpec {
            source_pointer: "0x00/0x01",
            destination: "synthetic-buffer",
            recognized_stop_codes: &[0xEF],
            destination_end_code: 0xEF,
            destination_end_origin: "copied_source_terminator",
            explicit_copy_byte_limit: None,
            code_regions: &[TransferCodeSpec {
                role: "copy_loop",
                file_offset: HEADER_SIZE + 0x0300,
                bytes: &[0xA0, 0x00, 0xB1, 0x00, 0xC9, 0xEF, 0x60],
            }],
        },
        protected_positions: &[],
    }
}

fn write_declared_code(source: &mut [u8], spec: &TextTableSpec) {
    source[spec.consumer_file_offset..spec.consumer_file_offset + spec.consumer_bytes.len()]
        .copy_from_slice(&spec.consumer_bytes);
    for region in spec.transfer.code_regions {
        source[region.file_offset..region.file_offset + region.bytes.len()]
            .copy_from_slice(region.bytes);
    }
}

fn write_code_regions(source: &mut [u8], regions: &[TransferCodeSpec]) {
    for region in regions {
        source[region.file_offset..region.file_offset + region.bytes.len()]
            .copy_from_slice(region.bytes);
    }
}

#[test]
fn resolves_requested_tables_in_caller_order() {
    let specs = requested_text_table_specs(&["enemy-names", "class-names"]).unwrap();
    assert_eq!(
        specs.iter().map(|spec| spec.id).collect::<Vec<_>>(),
        ["enemy-names", "class-names"]
    );
}

#[test]
fn includes_transformed_class_alias_and_stone_wall_terrain_pointers() {
    let specs = requested_text_table_specs(&["class-names", "terrain-names"]).unwrap();

    assert_eq!(specs[0].pointer_count, 24);
    assert_eq!(specs[1].pointer_count, 16);
}

#[test]
fn rejects_unknown_or_duplicate_requested_tables() {
    let unknown = requested_text_table_specs(&["missing-names"])
        .err()
        .expect("unknown table must fail")
        .to_string();
    assert!(unknown.contains("unknown text table id missing-names"));

    let duplicate = requested_text_table_specs(&["unit-names", "unit-names"])
        .err()
        .expect("duplicate table must fail")
        .to_string();
    assert!(duplicate.contains("duplicate text table id unit-names"));
}

#[test]
fn extracts_aliases_without_translating_preserved_latin() {
    let table_file_offset = FIXED_BANK_FILE_OFFSET + 0x0100;
    let consumer_file_offset = HEADER_SIZE + 0x0200;
    let spec = synthetic_spec(table_file_offset, consumer_file_offset);
    let mut source = vec![0_u8; PRG_FILE_END + FIRST_FONT_PAGE_BYTES];
    write_declared_code(&mut source, &spec);
    let text_cpu_address = FIXED_BANK_CPU_BASE + 0x0200;
    let pointer = text_cpu_address.to_le_bytes();
    source[table_file_offset..table_file_offset + 2].copy_from_slice(&pointer);
    source[table_file_offset + 2..table_file_offset + 4].copy_from_slice(&pointer);
    let text_file_offset = FIXED_BANK_FILE_OFFSET + 0x0200;
    source[text_file_offset..text_file_offset + 4].copy_from_slice(&[0x6A, 0x30, 0x60, 0xEF]);
    source[PRG_FILE_END + 0x30 * CHR_TILE_BYTES] = 0x80;

    let report = extract_table(&source, &spec).unwrap();

    assert_eq!(report.pointer_count, 2);
    assert_eq!(report.unique_string_count, 1);
    assert_eq!(report.entries[0].alias_entry_indices, vec![1]);
    assert_eq!(report.entries[1].alias_entry_indices, vec![0]);
    assert_eq!(report.entries[0].protected_original.len(), 2);
    assert_eq!(report.entries[0].protected_original[0].glyph, "A");
    assert_eq!(report.entries[0].protected_original[1].glyph, "0");
    assert_eq!(report.entries[0].unresolved_byte_count, 1);
    assert_eq!(
        report.referenced_unresolved_nonblank_font_tile_byte_count,
        2
    );
    assert_eq!(report.unique_unresolved_nonblank_font_tile_byte_count, 1);
    let usage = report
        .source_code_usage
        .iter()
        .find(|usage| usage.code == 0x30)
        .unwrap();
    assert!(!usage.font_tile_all_zero);
    assert_eq!(usage.referenced_byte_count, 2);
    assert_eq!(usage.unique_storage_byte_count, 1);
}

#[test]
fn protects_punctuation_only_at_a_declared_token_position() {
    let table_file_offset = FIXED_BANK_FILE_OFFSET + 0x0100;
    let consumer_file_offset = HEADER_SIZE + 0x0200;
    let mut spec = synthetic_spec(table_file_offset, consumer_file_offset);
    spec.protected_positions = &[ProtectedPosition {
        entry_index: 0,
        byte_offset: 1,
        code: 0x9B,
        glyph: ".",
    }];
    let mut source = vec![0_u8; PRG_FILE_END + FIRST_FONT_PAGE_BYTES];
    write_declared_code(&mut source, &spec);
    for (index, text_offset) in [0x0200_u16, 0x0210].iter().enumerate() {
        let pointer = (FIXED_BANK_CPU_BASE + *text_offset).to_le_bytes();
        let pointer_offset = table_file_offset + index * 2;
        source[pointer_offset..pointer_offset + 2].copy_from_slice(&pointer);
        let text_file_offset = FIXED_BANK_FILE_OFFSET + usize::from(*text_offset);
        source[text_file_offset..text_file_offset + 4].copy_from_slice(&[0x76, 0x9B, 0x30, 0xEF]);
    }

    let report = extract_table(&source, &spec).unwrap();

    assert_eq!(report.entries[0].protected_original.len(), 2);
    assert_eq!(report.entries[0].protected_original[0].glyph, "M");
    assert_eq!(report.entries[0].protected_original[1].glyph, ".");
    assert_eq!(report.entries[1].protected_original.len(), 1);
    assert_eq!(report.entries[1].unresolved_byte_count, 2);
    assert_eq!(report.referenced_unresolved_blank_font_tile_byte_count, 3);
    assert_eq!(
        report.referenced_unresolved_nonblank_font_tile_byte_count,
        0
    );
}

#[test]
fn rejects_consumer_bytes_that_no_longer_load_the_table() {
    let table_file_offset = FIXED_BANK_FILE_OFFSET + 0x0100;
    let consumer_file_offset = HEADER_SIZE + 0x0200;
    let spec = synthetic_spec(table_file_offset, consumer_file_offset);
    let mut source = vec![0_u8; PRG_FILE_END + FIRST_FONT_PAGE_BYTES];
    source[consumer_file_offset..consumer_file_offset + 10].copy_from_slice(&spec.consumer_bytes);
    source[consumer_file_offset + 1] ^= 0x01;

    let error = validate_consumer(&source, &spec).unwrap_err().to_string();

    assert!(error.contains("consumer bytes changed for synthetic-names"));
}

#[test]
fn rejects_transfer_code_that_no_longer_implements_the_declared_path() {
    let table_file_offset = FIXED_BANK_FILE_OFFSET + 0x0100;
    let consumer_file_offset = HEADER_SIZE + 0x0200;
    let spec = synthetic_spec(table_file_offset, consumer_file_offset);
    let mut source = vec![0_u8; PRG_FILE_END + FIRST_FONT_PAGE_BYTES];
    write_declared_code(&mut source, &spec);
    source[HEADER_SIZE + 0x0300] ^= 0x01;

    let error = build_transfer_evidence(&source, &spec)
        .unwrap_err()
        .to_string();

    assert!(error.contains("transfer code copy_loop changed for synthetic-names"));
}

#[test]
fn rejects_composite_layout_code_that_no_longer_preserves_combining_width() {
    let mut source = vec![0_u8; PRG_FILE_END + FIRST_FONT_PAGE_BYTES];
    for region in &COMPOSITE_TEXT_LAYOUT_CODE_REGIONS {
        source[region.file_offset..region.file_offset + region.bytes.len()]
            .copy_from_slice(region.bytes);
    }
    source[COMPOSITE_TEXT_LAYOUT_CODE_REGIONS[0].file_offset + 28] ^= 0x01;

    let error = build_code_region_evidence(
        &source,
        &COMPOSITE_TEXT_LAYOUT_CODE_REGIONS,
        "layout control",
        "bank_0B_composite_text_parser",
    )
    .unwrap_err()
    .to_string();

    assert!(error.contains(
            "layout control code first_pass_decrement_before_append changed for bank_0B_composite_text_parser"
        ));
}

#[test]
fn declares_composite_overlay_and_base_passes_separately() {
    let mut source = vec![0_u8; PRG_FILE_END + FIRST_FONT_PAGE_BYTES];
    for region in &COMPOSITE_TEXT_LAYOUT_CODE_REGIONS {
        source[region.file_offset..region.file_offset + region.bytes.len()]
            .copy_from_slice(region.bytes);
    }
    for region in &COMPOSITE_TEXT_CONSUMER_CODE_REGIONS {
        source[region.file_offset..region.file_offset + region.bytes.len()]
            .copy_from_slice(region.bytes);
    }
    for region in &COMPOSITE_TEXT_PPU_CODE_REGIONS {
        source[region.file_offset..region.file_offset + region.bytes.len()]
            .copy_from_slice(region.bytes);
    }
    for region in &COMPOSITE_PLANE_PACKING_CODE_REGIONS {
        source[region.file_offset..region.file_offset + region.bytes.len()]
            .copy_from_slice(region.bytes);
    }
    let source_code_usage = COMPOSITE_TEXT_LAYOUT_CODES.map(|code| SourceCodeUsage {
        code,
        code_hex: format!("{code:02X}"),
        font_tile_sha1: String::new(),
        font_tile_all_zero: false,
        referenced_byte_count: 1,
        unique_storage_byte_count: 1,
        referenced_protected_original_byte_count: 0,
        unique_protected_original_byte_count: 0,
        referenced_unresolved_nonblank_font_tile_byte_count: 1,
        unique_unresolved_nonblank_font_tile_byte_count: 1,
        referenced_unresolved_blank_font_tile_byte_count: 0,
        unique_unresolved_blank_font_tile_byte_count: 0,
    });

    let evidence = build_layout_control_evidence(&source, &source_code_usage).unwrap();
    let composite = &evidence[0];

    assert_eq!(composite.source_buffer, "0x0451");
    assert_eq!(composite.output_buffer, "0x0311");
    assert_eq!(composite.segment_separator_code, 0xED);
    assert_eq!(composite.end_code, 0xEF);
    assert_eq!(composite.overlay_blank_code, 0xFF);
    assert_eq!(
        composite.first_pass_behavior,
        "emit_blank_cells_and_replace_the_previous_blank_with_a_zero_cell_combining_code"
    );
    assert_eq!(
        composite.second_pass_behavior,
        "emit_base_codes_while_skipping_combining_codes"
    );
    assert_eq!(
        composite.segment_output_order,
        "combining_overlay_then_base_codes"
    );
    assert_eq!(
        composite.downstream_consumer.source_buffer_pointer,
        "0x06/0x07 = 0x0311"
    );
    assert_eq!(composite.downstream_consumer.source_cursor, "0x0310");
    assert_eq!(composite.downstream_consumer.output_stage_call_count, 2);
    assert_eq!(
        composite
            .downstream_consumer
            .segment_separator_replacement_code,
        0xFF
    );
    assert_eq!(
        composite
            .downstream_consumer
            .ppu_transfer
            .stage_descriptor_buffer,
        "0x0700"
    );
    assert_eq!(
        composite
            .downstream_consumer
            .ppu_transfer
            .queued_command_buffer,
        "0x0781"
    );
    assert_eq!(
        composite
            .downstream_consumer
            .ppu_transfer
            .ppu_address_register,
        "0x2006"
    );
    assert_eq!(
        composite.downstream_consumer.ppu_transfer.ppu_data_register,
        "0x2007"
    );
    assert_eq!(
        composite
            .downstream_consumer
            .ppu_transfer
            .queue_consumer_cpu_address,
        0xC3E7
    );
    assert_eq!(
        composite
            .downstream_consumer
            .ppu_transfer
            .descriptor_length_mask,
        0x3F
    );
    assert_eq!(
        composite
            .downstream_consumer
            .ppu_transfer
            .descriptor_vertical_increment_mask,
        0x80
    );
    assert_eq!(
        composite
            .downstream_consumer
            .ppu_transfer
            .ppu_data_write_cpu_address,
        0xC3DD
    );
    assert_eq!(composite.plane_packing.entry_cpu_address, 0x8163);
    assert_eq!(
        composite.plane_packing.caller_cpu_addresses,
        vec![0x8137, 0x8B7A]
    );
    assert_eq!(composite.plane_packing.copy_source, "byte_after_first_0xED");
    assert_eq!(composite.plane_packing.copy_destination, "first_0xED");
    assert_eq!(
        composite.plane_packing.output_layout,
        "combining_overlay_then_base_codes_without_interplane_separator"
    );
    assert_eq!(
        composite
            .direct_jsr_candidates
            .iter()
            .map(|candidate| candidate.cpu_address)
            .collect::<Vec<_>>(),
        composite.plane_packing.caller_cpu_addresses
    );
}

#[test]
fn rejects_changed_composite_downstream_consumer_code() {
    let mut source = vec![0_u8; PRG_FILE_END + FIRST_FONT_PAGE_BYTES];
    for region in &COMPOSITE_TEXT_CONSUMER_CODE_REGIONS {
        source[region.file_offset..region.file_offset + region.bytes.len()]
            .copy_from_slice(region.bytes);
    }
    source[COMPOSITE_TEXT_CONSUMER_CODE_REGIONS[1].file_offset + 1] ^= 0x01;

    let error = build_code_region_evidence(
        &source,
        &COMPOSITE_TEXT_CONSUMER_CODE_REGIONS,
        "downstream consumer",
        "bank_0B_composite_text_parser",
    )
    .unwrap_err()
    .to_string();

    assert!(error.contains(
            "downstream consumer code bind_composite_output_pointer changed for bank_0B_composite_text_parser"
        ));
}

#[test]
fn rejects_changed_composite_ppu_transfer_code() {
    let mut source = vec![0_u8; PRG_FILE_END + FIRST_FONT_PAGE_BYTES];
    for region in &COMPOSITE_TEXT_PPU_CODE_REGIONS {
        source[region.file_offset..region.file_offset + region.bytes.len()]
            .copy_from_slice(region.bytes);
    }
    source[COMPOSITE_TEXT_PPU_CODE_REGIONS[5].file_offset + 31] ^= 0x01;

    let error = build_code_region_evidence(
        &source,
        &COMPOSITE_TEXT_PPU_CODE_REGIONS,
        "PPU transfer",
        "bank_0B_composite_text_parser",
    )
    .unwrap_err()
    .to_string();

    assert!(error.contains(
        "PPU transfer code write_queued_codes_to_ppu changed for bank_0B_composite_text_parser"
    ));
}

#[test]
fn rejects_changed_composite_plane_packing_code() {
    let mut source = vec![0_u8; PRG_FILE_END + FIRST_FONT_PAGE_BYTES];
    for region in &COMPOSITE_PLANE_PACKING_CODE_REGIONS {
        source[region.file_offset..region.file_offset + region.bytes.len()]
            .copy_from_slice(region.bytes);
    }
    source[COMPOSITE_PLANE_PACKING_CODE_REGIONS[1].file_offset + 19] ^= 0x01;

    let error = build_code_region_evidence(
        &source,
        &COMPOSITE_PLANE_PACKING_CODE_REGIONS,
        "plane packing",
        "bank_0B_composite_text_parser",
    )
    .unwrap_err()
    .to_string();

    assert!(error.contains(
            "plane packing code find_separator_and_prepare_overlapping_copy changed for bank_0B_composite_text_parser"
        ));
}

#[test]
fn declares_dialogue_script_and_progressive_two_plane_renderer() {
    let mut source = vec![0_u8; PRG_FILE_END + FIRST_FONT_PAGE_BYTES];
    write_code_regions(&mut source, &DIALOGUE_SCRIPT_CODE_REGIONS);
    write_code_regions(&mut source, &DIALOGUE_PACKED_STATE_BIT_CODE_REGIONS);
    write_code_regions(&mut source, &DIALOGUE_RENDERER_CODE_REGIONS);

    let evidence = build_dialogue_text_path_evidence(&source).unwrap();

    assert_eq!(evidence.script.reader_entry_cpu_address, 0xE69C);
    assert_eq!(evidence.script.source_bank_state, "0x77F2");
    assert_eq!(evidence.script.source_pointer, "0x76/0x77");
    assert_eq!(evidence.script.line_destination_pointer, "0x06/0x07");
    assert_eq!(
        evidence.script.line_buffer_addresses,
        DIALOGUE_LINE_BUFFER_ADDRESSES
    );
    assert_eq!(evidence.script.line_buffer_stride_bytes, 0x20);
    assert_eq!(evidence.script.line_end_code, 0xED);
    assert_eq!(
        evidence.script.recognized_control_codes,
        DIALOGUE_SCRIPT_CONTROL_CODES
    );
    let finish_with_transition = evidence
        .script
        .controls
        .iter()
        .find(|control| control.code == 0xE4)
        .unwrap();
    assert_eq!(finish_with_transition.stream_storage_byte_count, 3);
    assert_eq!(finish_with_transition.current_pointer_advance_bytes, 1);
    assert_eq!(finish_with_transition.inline_operand_byte_count, 0);
    assert_eq!(finish_with_transition.transition_target_byte_count, 2);
    assert_eq!(
        finish_with_transition.line_effect,
        "finish_current_line_with_0xED"
    );
    let insert_sram_string = evidence
        .script
        .controls
        .iter()
        .find(|control| control.code == 0xEC)
        .unwrap();
    assert_eq!(insert_sram_string.current_pointer_advance_bytes, 2);
    assert_eq!(insert_sram_string.stream_storage_byte_count, 2);
    assert_eq!(insert_sram_string.inline_operand_byte_count, 1);
    assert_eq!(
        insert_sram_string.output_effect,
        "append_selected_sram_string_excluding_0xEF"
    );
    assert_eq!(evidence.script.synthesized_pair_control_code, 0xEA);
    assert_eq!(evidence.script.synthesized_pair_codes, vec![0x9E, 0xAB]);
    assert_eq!(evidence.renderer.entry_cpu_address, 0x83BA);
    assert_eq!(evidence.renderer.combining_codes, vec![0x0F, 0x1F]);
    assert_eq!(evidence.renderer.line_width_mask, 0x1F);
    assert_eq!(evidence.renderer.two_plane_header_flag, 0x40);
    assert_eq!(evidence.renderer.encoded_stage_count, 2);
    assert_eq!(evidence.renderer.stage_serializer_entry_cpu_address, 0xC842);
    assert_eq!(evidence.runtime_observation.source_prg_bank, 0x08);
    assert_eq!(evidence.runtime_observation.source_cpu_address, 0x9FCE);
    assert_eq!(evidence.runtime_observation.source_file_offset, 0x21FDE);
    assert_eq!(
        evidence.runtime_observation.destination_line_buffer_address,
        0x78B2
    );
    assert_eq!(evidence.runtime_observation.source_write_event_pc, 0x821B);
    assert_eq!(evidence.runtime_observation.observed_stage_descriptor, 0x52);
    assert_eq!(evidence.runtime_observation.observed_line_width, 18);
    assert_eq!(evidence.runtime_observation.observed_stage_count, 2);
    assert_eq!(
        evidence.runtime_observation.stage_descriptor_write_event_pc,
        0x846C
    );
}

#[test]
fn rejects_changed_dialogue_script_or_renderer_code() {
    let mut source = vec![0_u8; PRG_FILE_END + FIRST_FONT_PAGE_BYTES];
    write_code_regions(&mut source, &DIALOGUE_SCRIPT_CODE_REGIONS);
    write_code_regions(&mut source, &DIALOGUE_PACKED_STATE_BIT_CODE_REGIONS);
    write_code_regions(&mut source, &DIALOGUE_RENDERER_CODE_REGIONS);
    source[DIALOGUE_SCRIPT_CODE_REGIONS[2].file_offset + 9] ^= 0x01;

    let error = build_dialogue_text_path_evidence(&source)
        .unwrap_err()
        .to_string();

    assert!(error.contains(
            "dialogue script code copy_literal_script_byte_to_line changed for bank_0A_dialogue_script_loader"
        ));

    write_code_regions(&mut source, &DIALOGUE_SCRIPT_CODE_REGIONS);
    source[DIALOGUE_RENDERER_CODE_REGIONS[3].file_offset + 31] ^= 0x01;

    let error = build_dialogue_text_path_evidence(&source)
        .unwrap_err()
        .to_string();

    assert!(error.contains(
            "dialogue renderer code serialize_two_plane_line_to_ppu_queue changed for bank_0A_progressive_dialogue_renderer"
        ));
}

#[test]
fn rejects_changed_dialogue_packed_state_bit_routine() {
    let mut source = vec![0_u8; PRG_FILE_END + FIRST_FONT_PAGE_BYTES];
    write_code_regions(&mut source, &DIALOGUE_SCRIPT_CODE_REGIONS);
    write_code_regions(&mut source, &DIALOGUE_PACKED_STATE_BIT_CODE_REGIONS);
    write_code_regions(&mut source, &DIALOGUE_RENDERER_CODE_REGIONS);
    source[DIALOGUE_PACKED_STATE_BIT_CODE_REGIONS[0].file_offset + 18] ^= 0x01;

    let error = build_dialogue_text_path_evidence(&source)
        .unwrap_err()
        .to_string();

    assert!(error.contains(
            "dialogue state bit code select_sram_state_slot_and_bit changed for packed_dialogue_state_operand"
        ));
}

#[test]
fn rejects_text_that_exceeds_the_consumers_explicit_copy_limit() {
    let table_file_offset = FIXED_BANK_FILE_OFFSET + 0x0100;
    let consumer_file_offset = HEADER_SIZE + 0x0200;
    let mut spec = synthetic_spec(table_file_offset, consumer_file_offset);
    spec.transfer.explicit_copy_byte_limit = Some(3);
    let mut source = vec![0_u8; PRG_FILE_END + FIRST_FONT_PAGE_BYTES];
    write_declared_code(&mut source, &spec);
    let text_cpu_address = FIXED_BANK_CPU_BASE + 0x0200;
    let pointer = text_cpu_address.to_le_bytes();
    source[table_file_offset..table_file_offset + 2].copy_from_slice(&pointer);
    source[table_file_offset + 2..table_file_offset + 4].copy_from_slice(&pointer);
    let text_file_offset = FIXED_BANK_FILE_OFFSET + 0x0200;
    source[text_file_offset..text_file_offset + 4].copy_from_slice(&[0x30, 0x31, 0x32, 0xEF]);

    let error = extract_table(&source, &spec).unwrap_err().to_string();

    assert!(error.contains("needs 4 bytes including its terminator"));
    assert!(error.contains("consumer limit 3"));
}
