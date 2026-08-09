use super::*;

pub(super) fn extract_table(source: &[u8], spec: &TextTableSpec) -> Result<TextTableReport> {
    ensure!(
        source.len() >= PRG_FILE_END + FIRST_FONT_PAGE_BYTES,
        "source is shorter than the PRG region and first CHR font page"
    );
    validate_consumer(source, spec)?;
    let transfer = build_transfer_evidence(source, spec)?;

    let table_byte_length = spec
        .pointer_count
        .checked_mul(2)
        .context("pointer table length overflow")?;
    let table_end = spec
        .table_file_offset
        .checked_add(table_byte_length)
        .context("pointer table range overflow")?;
    ensure!(
        (FIXED_BANK_FILE_OFFSET..=PRG_FILE_END).contains(&spec.table_file_offset)
            && table_end <= PRG_FILE_END,
        "text table {} is outside the fixed PRG bank",
        spec.id
    );
    let table_bytes = &source[spec.table_file_offset..table_end];
    let pointers: Vec<u16> = table_bytes
        .chunks_exact(2)
        .map(|bytes| u16::from_le_bytes([bytes[0], bytes[1]]))
        .collect();
    let mut pointer_indices: BTreeMap<u16, Vec<usize>> = BTreeMap::new();
    for (index, pointer) in pointers.iter().enumerate() {
        pointer_indices.entry(*pointer).or_default().push(index);
    }

    let mut ranges = Vec::new();
    let mut entries = Vec::with_capacity(pointers.len());
    let mut code_usage_counts: BTreeMap<u8, CodeUsageCounts> = BTreeMap::new();
    for (index, pointer) in pointers.iter().enumerate() {
        let file_offset = fixed_cpu_to_file_offset(*pointer)
            .with_context(|| format!("{} entry {index}", spec.id))?;
        ensure!(
            file_offset >= table_end,
            "{} entry {index} points into or before its pointer table",
            spec.id
        );
        let search_end = file_offset
            .checked_add(MAX_ENTRY_BYTES + 1)
            .unwrap_or(PRG_FILE_END)
            .min(PRG_FILE_END);
        let terminator_offset = source[file_offset..search_end]
            .iter()
            .position(|byte| *byte == spec.terminator)
            .map(|relative| file_offset + relative)
            .with_context(|| {
                format!(
                    "{} entry {index} has no {:02X} terminator within {MAX_ENTRY_BYTES} bytes",
                    spec.id, spec.terminator
                )
            })?;
        let raw = &source[file_offset..terminator_offset];
        if let Some(limit) = spec.transfer.explicit_copy_byte_limit {
            ensure!(
                raw.len() < limit,
                "{} entry {index} needs {} bytes including its terminator, beyond the consumer limit {limit}",
                spec.id,
                raw.len() + 1
            );
        }
        for position in spec
            .protected_positions
            .iter()
            .filter(|position| position.entry_index == index)
        {
            ensure!(
                raw.get(position.byte_offset) == Some(&position.code),
                "protected original byte changed for {} entry {index} at byte {}",
                spec.id,
                position.byte_offset
            );
        }
        let alias_entry_indices = pointer_indices[pointer]
            .iter()
            .copied()
            .filter(|other| *other != index)
            .collect();
        let mut protected_original = Vec::new();
        for (byte_offset, code) in raw.iter().enumerate() {
            let declared = spec.protected_positions.iter().find(|position| {
                position.entry_index == index && position.byte_offset == byte_offset
            });
            let glyph = if let Some(position) = declared {
                ensure!(
                    *code == position.code,
                    "protected original byte changed for {} entry {index} at byte {byte_offset}",
                    spec.id
                );
                Some(position.glyph)
            } else {
                protected_alphanumeric_glyph(*code)
            };
            if let Some(glyph) = glyph {
                protected_original.push(ProtectedByte {
                    byte_offset,
                    code: *code,
                    code_hex: format!("{code:02X}"),
                    glyph: glyph.to_owned(),
                });
            }
        }
        let unresolved_byte_count = raw.len() - protected_original.len();
        let protected_offsets: BTreeSet<usize> = protected_original
            .iter()
            .map(|protected| protected.byte_offset)
            .collect();
        let is_unique_storage = pointer_indices[pointer][0] == index;
        for (byte_offset, code) in raw.iter().enumerate() {
            let counts = code_usage_counts.entry(*code).or_default();
            counts.referenced_byte_count += 1;
            counts.unique_storage_byte_count += usize::from(is_unique_storage);
            if protected_offsets.contains(&byte_offset) {
                counts.referenced_protected_original_byte_count += 1;
                counts.unique_protected_original_byte_count += usize::from(is_unique_storage);
            } else if font_tile(source, *code)?.iter().all(|byte| *byte == 0) {
                counts.referenced_unresolved_blank_font_tile_byte_count += 1;
                counts.unique_unresolved_blank_font_tile_byte_count +=
                    usize::from(is_unique_storage);
            } else {
                counts.referenced_unresolved_nonblank_font_tile_byte_count += 1;
                counts.unique_unresolved_nonblank_font_tile_byte_count +=
                    usize::from(is_unique_storage);
            }
        }

        ranges.push((file_offset, terminator_offset + 1));
        entries.push(TextEntryReport {
            index,
            pointer_cpu_address: *pointer,
            pointer_cpu_address_hex: format!("0x{pointer:04X}"),
            file_offset,
            file_offset_hex: format!("0x{file_offset:05X}"),
            byte_length: raw.len(),
            raw_bytes_hex: hex_bytes(raw),
            raw_sha1: sha1_hex(raw),
            alias_entry_indices,
            protected_original,
            unresolved_byte_count,
        });
    }
    validate_unique_ranges(spec.id, &ranges)?;

    let data_file_start = ranges
        .iter()
        .map(|(start, _)| *start)
        .min()
        .context("text table has no entries")?;
    let data_file_end_exclusive = ranges
        .iter()
        .map(|(_, end)| *end)
        .max()
        .context("text table has no entries")?;
    let referenced_protected_original_byte_count = entries
        .iter()
        .map(|entry| entry.protected_original.len())
        .sum();
    let referenced_unresolved_byte_count = entries
        .iter()
        .map(|entry| entry.unresolved_byte_count)
        .sum();
    let referenced_text_byte_count = entries.iter().map(|entry| entry.byte_length).sum();
    let first_entry_for_pointer = pointers
        .iter()
        .enumerate()
        .filter(|(index, pointer)| pointer_indices[pointer][0] == *index)
        .map(|(index, _)| &entries[index])
        .collect::<Vec<_>>();
    let unique_text_storage_byte_count = first_entry_for_pointer
        .iter()
        .map(|entry| entry.byte_length)
        .sum();
    let unique_protected_original_byte_count = first_entry_for_pointer
        .iter()
        .map(|entry| entry.protected_original.len())
        .sum();
    let unique_unresolved_byte_count = first_entry_for_pointer
        .iter()
        .map(|entry| entry.unresolved_byte_count)
        .sum();
    let source_code_usage = source_code_usage(source, code_usage_counts)?;
    let referenced_unresolved_nonblank_font_tile_byte_count = source_code_usage
        .iter()
        .map(|usage| usage.referenced_unresolved_nonblank_font_tile_byte_count)
        .sum();
    let unique_unresolved_nonblank_font_tile_byte_count = source_code_usage
        .iter()
        .map(|usage| usage.unique_unresolved_nonblank_font_tile_byte_count)
        .sum();
    let referenced_unresolved_blank_font_tile_byte_count = source_code_usage
        .iter()
        .map(|usage| usage.referenced_unresolved_blank_font_tile_byte_count)
        .sum();
    let unique_unresolved_blank_font_tile_byte_count = source_code_usage
        .iter()
        .map(|usage| usage.unique_unresolved_blank_font_tile_byte_count)
        .sum();
    ensure!(
        referenced_unresolved_byte_count
            == referenced_unresolved_nonblank_font_tile_byte_count
                + referenced_unresolved_blank_font_tile_byte_count,
        "font-tile classification does not cover referenced unresolved bytes for {}",
        spec.id
    );
    ensure!(
        unique_unresolved_byte_count
            == unique_unresolved_nonblank_font_tile_byte_count
                + unique_unresolved_blank_font_tile_byte_count,
        "font-tile classification does not cover unique unresolved bytes for {}",
        spec.id
    );
    let table_cpu_address = fixed_file_to_cpu_address(spec.table_file_offset)?;
    let (consumer_prg_bank, consumer_cpu_address) = prg_file_location(spec.consumer_file_offset)?;
    let destination_pointer = format!(
        "0x{:02X}/0x{:02X}",
        spec.consumer_bytes[4], spec.consumer_bytes[9]
    );

    Ok(TextTableReport {
        id: spec.id,
        role: spec.role,
        table_file_offset: spec.table_file_offset,
        table_file_offset_hex: format!("0x{:05X}", spec.table_file_offset),
        table_cpu_address,
        table_cpu_address_hex: format!("0x{table_cpu_address:04X}"),
        pointer_count: spec.pointer_count,
        unique_string_count: pointer_indices.len(),
        pointer_table_sha1: sha1_hex(table_bytes),
        terminator: spec.terminator,
        terminator_hex: format!("{:02X}", spec.terminator),
        consumer: ConsumerEvidence {
            file_offset: spec.consumer_file_offset,
            file_offset_hex: format!("0x{:05X}", spec.consumer_file_offset),
            prg_bank: consumer_prg_bank,
            prg_bank_hex: format!("0x{consumer_prg_bank:02X}"),
            cpu_address: consumer_cpu_address,
            cpu_address_hex: format!("0x{consumer_cpu_address:04X}"),
            instruction_bytes_hex: hex_bytes(&spec.consumer_bytes),
            pointer_load_mode: if spec.consumer_bytes[0] == 0xBD {
                "absolute_x"
            } else {
                "absolute_y"
            },
            destination_pointer,
        },
        transfer,
        data_file_start,
        data_file_start_hex: format!("0x{data_file_start:05X}"),
        data_file_end_exclusive,
        data_file_end_exclusive_hex: format!("0x{data_file_end_exclusive:05X}"),
        referenced_text_byte_count,
        unique_text_storage_byte_count,
        referenced_protected_original_byte_count,
        unique_protected_original_byte_count,
        referenced_unresolved_byte_count,
        unique_unresolved_byte_count,
        referenced_unresolved_nonblank_font_tile_byte_count,
        unique_unresolved_nonblank_font_tile_byte_count,
        referenced_unresolved_blank_font_tile_byte_count,
        unique_unresolved_blank_font_tile_byte_count,
        source_code_usage,
        entries,
    })
}

pub(super) fn validate_consumer(source: &[u8], spec: &TextTableSpec) -> Result<()> {
    let end = spec
        .consumer_file_offset
        .checked_add(spec.consumer_bytes.len())
        .context("consumer range overflow")?;
    ensure!(end <= PRG_FILE_END, "consumer {} is outside PRG", spec.id);
    ensure!(
        source[spec.consumer_file_offset..end] == spec.consumer_bytes,
        "consumer bytes changed for {} at {:#X}",
        spec.id,
        spec.consumer_file_offset
    );

    let table_cpu_address = fixed_file_to_cpu_address(spec.table_file_offset)?;
    let next_address = table_cpu_address + 1;
    let opcode = spec.consumer_bytes[0];
    ensure!(
        [0xBD, 0xB9].contains(&opcode)
            && spec.consumer_bytes[3] == 0x85
            && spec.consumer_bytes[5] == opcode
            && spec.consumer_bytes[8] == 0x85,
        "consumer {} is not the declared indexed pointer load",
        spec.id
    );
    ensure!(
        spec.consumer_bytes[1..3] == table_cpu_address.to_le_bytes()
            && spec.consumer_bytes[6..8] == next_address.to_le_bytes(),
        "consumer {} does not load its pointer table",
        spec.id
    );
    ensure!(
        spec.consumer_bytes[9] == spec.consumer_bytes[4] + 1,
        "consumer {} does not store an adjacent pointer pair",
        spec.id
    );
    Ok(())
}

pub(super) fn build_transfer_evidence(
    source: &[u8],
    spec: &TextTableSpec,
) -> Result<TextTransferEvidence> {
    ensure!(
        spec.transfer
            .recognized_stop_codes
            .contains(&spec.terminator),
        "transfer for {} does not recognize its declared terminator",
        spec.id
    );
    ensure!(
        !spec.transfer.code_regions.is_empty(),
        "transfer for {} has no code evidence",
        spec.id
    );

    let code_regions =
        build_code_region_evidence(source, spec.transfer.code_regions, "transfer", spec.id)?;

    Ok(TextTransferEvidence {
        source_pointer: spec.transfer.source_pointer,
        destination: spec.transfer.destination,
        recognized_stop_codes: spec.transfer.recognized_stop_codes.to_vec(),
        recognized_stop_codes_hex: spec
            .transfer
            .recognized_stop_codes
            .iter()
            .map(|code| format!("{code:02X}"))
            .collect(),
        declared_source_terminator: spec.terminator,
        declared_source_terminator_hex: format!("{:02X}", spec.terminator),
        destination_end_code: spec.transfer.destination_end_code,
        destination_end_code_hex: format!("{:02X}", spec.transfer.destination_end_code),
        destination_end_origin: spec.transfer.destination_end_origin,
        explicit_copy_byte_limit: spec.transfer.explicit_copy_byte_limit,
        code_regions,
    })
}

pub(super) fn build_layout_control_evidence(
    source: &[u8],
    source_code_usage: &[SourceCodeUsage],
) -> Result<Vec<LayoutControlEvidence>> {
    let mut inventory_referenced_byte_count = 0;
    let mut inventory_unique_storage_byte_count = 0;
    for code in COMPOSITE_TEXT_LAYOUT_CODES {
        let usage = source_code_usage
            .iter()
            .find(|usage| usage.code == code)
            .with_context(|| format!("layout code {code:02X} is absent from the text inventory"))?;
        inventory_referenced_byte_count += usage.referenced_byte_count;
        inventory_unique_storage_byte_count += usage.unique_storage_byte_count;
    }

    Ok(vec![LayoutControlEvidence {
        scope: "bank_0B_composite_text_parser",
        entry_cpu_address: 0x8F39,
        entry_cpu_address_hex: "0x8F39".to_owned(),
        source_buffer: "0x0451",
        output_buffer: "0x0311",
        segment_separator_code: COMPOSITE_SEGMENT_SEPARATOR_CODE,
        segment_separator_code_hex: format!("{COMPOSITE_SEGMENT_SEPARATOR_CODE:02X}"),
        end_code: COMPOSITE_END_CODE,
        end_code_hex: format!("{COMPOSITE_END_CODE:02X}"),
        overlay_blank_code: COMPOSITE_OVERLAY_BLANK_CODE,
        overlay_blank_code_hex: format!("{COMPOSITE_OVERLAY_BLANK_CODE:02X}"),
        first_pass_behavior: "emit_blank_cells_and_replace_the_previous_blank_with_a_zero_cell_combining_code",
        second_pass_behavior: "emit_base_codes_while_skipping_combining_codes",
        segment_output_order: "combining_overlay_then_base_codes",
        codes: COMPOSITE_TEXT_LAYOUT_CODES.to_vec(),
        codes_hex: COMPOSITE_TEXT_LAYOUT_CODES
            .iter()
            .map(|code| format!("{code:02X}"))
            .collect(),
        observed_behavior: "zero_cell_combining_diacritic",
        inventory_referenced_byte_count,
        inventory_unique_storage_byte_count,
        code_regions: build_code_region_evidence(
            source,
            &COMPOSITE_TEXT_LAYOUT_CODE_REGIONS,
            "layout control",
            "bank_0B_composite_text_parser",
        )?,
        downstream_consumer: CompositeTextConsumerEvidence {
            entry_cpu_address: 0x9608,
            entry_cpu_address_hex: "0x9608".to_owned(),
            source_buffer_pointer: "0x06/0x07 = 0x0311",
            source_cursor: "0x0310",
            stage_output_buffer: "0x0701,X",
            output_stage_call_count: 2,
            segment_separator_replacement_code: COMPOSITE_OVERLAY_BLANK_CODE,
            segment_separator_replacement_code_hex: format!("{COMPOSITE_OVERLAY_BLANK_CODE:02X}"),
            observed_behavior: "two_output_stage_calls_consume_the_composite_buffer_through_one_shared_cursor",
            code_regions: build_code_region_evidence(
                source,
                &COMPOSITE_TEXT_CONSUMER_CODE_REGIONS,
                "downstream consumer",
                "bank_0B_composite_text_parser",
            )?,
            ppu_transfer: PpuTransferEvidence {
                stage_descriptor_buffer: "0x0700",
                queued_command_buffer: "0x0781",
                queued_command_length: "0x0780",
                ready_flag: "0x21",
                serializer_cpu_address: 0xC84E,
                serializer_cpu_address_hex: "0xC84E".to_owned(),
                flush_cpu_address: 0xC3A5,
                flush_cpu_address_hex: "0xC3A5".to_owned(),
                queue_consumer_cpu_address: 0xC3E7,
                queue_consumer_cpu_address_hex: "0xC3E7".to_owned(),
                command_pointer: "0x00/0x01",
                command_terminator: 0,
                command_address_byte_order: "PPU address high byte, then low byte",
                descriptor_byte_offset: 2,
                descriptor_length_mask: 0x3F,
                descriptor_length_mask_hex: "0x3F".to_owned(),
                descriptor_vertical_increment_mask: 0x80,
                descriptor_vertical_increment_mask_hex: "0x80".to_owned(),
                descriptor_bit_6_behavior: "clear consumes one encoded byte per output byte; set repeats one encoded byte for the declared output length",
                data_byte_offset: 3,
                ppu_address_register: "0x2006",
                ppu_data_register: "0x2007",
                ppu_data_write_cpu_address: 0xC3DD,
                ppu_data_write_cpu_address_hex: "0xC3DD".to_owned(),
                observed_behavior: "serialize_stage_codes_into_a_command_queue_then_flush_them_to_ppu",
                code_regions: build_code_region_evidence(
                    source,
                    &COMPOSITE_TEXT_PPU_CODE_REGIONS,
                    "PPU transfer",
                    "bank_0B_composite_text_parser",
                )?,
            },
        },
        plane_packing: CompositePlanePackingEvidence {
            entry_cpu_address: 0x8163,
            entry_cpu_address_hex: "0x8163".to_owned(),
            caller_cpu_addresses: vec![0x8137, 0x8B7A],
            caller_cpu_addresses_hex: vec!["0x8137".to_owned(), "0x8B7A".to_owned()],
            input_buffer: "0x0311",
            separator_scan_start_index: 1,
            separator_code: COMPOSITE_SEGMENT_SEPARATOR_CODE,
            separator_code_hex: format!("{COMPOSITE_SEGMENT_SEPARATOR_CODE:02X}"),
            copy_source: "byte_after_first_0xED",
            copy_destination: "first_0xED",
            copy_byte_count: "first_0xED_index",
            copy_routine_cpu_address: 0xC209,
            copy_routine_cpu_address_hex: "0xC209".to_owned(),
            output_layout: "combining_overlay_then_base_codes_without_interplane_separator",
            observed_behavior: "shift_base_plane_left_over_first_separator_by_overlay_plane_width",
            code_regions: build_code_region_evidence(
                source,
                &COMPOSITE_PLANE_PACKING_CODE_REGIONS,
                "plane packing",
                "bank_0B_composite_text_parser",
            )?,
        },
        direct_jsr_candidates: find_absolute_transfer_candidates(
            &source[HEADER_SIZE..PRG_FILE_END],
            0x8F39,
            0x20,
        ),
        direct_jmp_candidates: find_absolute_transfer_candidates(
            &source[HEADER_SIZE..PRG_FILE_END],
            0x8F39,
            0x4C,
        ),
    }])
}
