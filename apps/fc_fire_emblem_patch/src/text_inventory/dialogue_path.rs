use super::*;

pub(super) fn build_dialogue_text_path_evidence(source: &[u8]) -> Result<DialogueTextPathEvidence> {
    let recognized_control_codes = DIALOGUE_SCRIPT_CONTROL_CODES.to_vec();
    let controls = DIALOGUE_CONTROL_SPECS
        .iter()
        .map(|control| DialogueControlEvidence {
            code: control.code,
            code_hex: format!("{:02X}", control.code),
            stream_storage_byte_count: 1
                + control.inline_operand_byte_count
                + control.transition_target_byte_count,
            current_pointer_advance_bytes: control.current_pointer_advance_bytes,
            inline_operand_byte_count: control.inline_operand_byte_count,
            transition_target_byte_count: control.transition_target_byte_count,
            line_effect: control.line_effect,
            output_effect: control.output_effect,
            state_effect: control.state_effect,
            operand_contract: control.operand_contract,
        })
        .collect::<Vec<_>>();
    ensure!(
        controls
            .iter()
            .map(|control| control.code)
            .eq(DIALOGUE_SCRIPT_CONTROL_CODES),
        "dialogue control declarations do not match the dispatcher order"
    );
    ensure!(
        controls
            .iter()
            .all(|control| control.current_pointer_advance_bytes
                == control.inline_operand_byte_count + 1),
        "dialogue control source advance does not match its consumed operands"
    );
    ensure!(
        controls
            .iter()
            .all(|control| control.stream_storage_byte_count
                == control.inline_operand_byte_count + control.transition_target_byte_count + 1),
        "dialogue control storage length does not cover its inline and transition bytes"
    );
    let synthesized_pair_codes = vec![0x9E, 0xAB];
    let combining_codes = COMPOSITE_TEXT_LAYOUT_CODES.to_vec();

    Ok(DialogueTextPathEvidence {
        script: DialogueScriptEvidence {
            reader_entry_cpu_address: 0xE69C,
            reader_entry_cpu_address_hex: "0xE69C".to_owned(),
            source_bank_state: "0x77F2",
            source_pointer: "0x76/0x77",
            source_index: "0x77FA",
            readback_byte: "0x7934",
            restored_dialogue_prg_bank: 0x0A,
            restored_dialogue_prg_bank_hex: "0x0A".to_owned(),
            line_destination_pointer: "0x06/0x07",
            destination_index: "0x77FB",
            line_buffer_addresses: DIALOGUE_LINE_BUFFER_ADDRESSES.to_vec(),
            line_buffer_addresses_hex: DIALOGUE_LINE_BUFFER_ADDRESSES
                .iter()
                .map(|address| format!("0x{address:04X}"))
                .collect(),
            line_buffer_stride_bytes: 0x20,
            line_end_code: DIALOGUE_LINE_END_CODE,
            line_end_code_hex: format!("{DIALOGUE_LINE_END_CODE:02X}"),
            recognized_control_codes,
            recognized_control_codes_hex: DIALOGUE_SCRIPT_CONTROL_CODES
                .iter()
                .map(|code| format!("{code:02X}"))
                .collect(),
            controls,
            synthesized_pair_control_code: 0xEA,
            synthesized_pair_control_code_hex: "EA".to_owned(),
            synthesized_pair_codes: synthesized_pair_codes.clone(),
            synthesized_pair_codes_hex: synthesized_pair_codes
                .iter()
                .map(|code| format!("{code:02X}"))
                .collect(),
            code_regions: build_code_region_evidence(
                source,
                &DIALOGUE_SCRIPT_CODE_REGIONS,
                "dialogue script",
                "bank_0A_dialogue_script_loader",
            )?,
            packed_state_bit_code_regions: build_code_region_evidence(
                source,
                &DIALOGUE_PACKED_STATE_BIT_CODE_REGIONS,
                "dialogue state bit",
                "packed_dialogue_state_operand",
            )?,
        },
        renderer: DialogueRendererEvidence {
            entry_cpu_address: 0x83BA,
            entry_cpu_address_hex: "0x83BA".to_owned(),
            source_pointer: "0x06/0x07",
            line_end_code: DIALOGUE_LINE_END_CODE,
            line_end_code_hex: format!("{DIALOGUE_LINE_END_CODE:02X}"),
            combining_codes: combining_codes.clone(),
            combining_codes_hex: combining_codes
                .iter()
                .map(|code| format!("{code:02X}"))
                .collect(),
            overlay_blank_code: COMPOSITE_OVERLAY_BLANK_CODE,
            overlay_blank_code_hex: format!("{COMPOSITE_OVERLAY_BLANK_CODE:02X}"),
            line_width_state: "0x781A",
            line_width_mask: DIALOGUE_STAGE_WIDTH_MASK,
            line_width_mask_hex: format!("{DIALOGUE_STAGE_WIDTH_MASK:02X}"),
            visible_code_count: "0x77FC",
            processed_code_count: "0x77FD",
            stage_descriptor_buffer: "0x0310",
            stage_payload_buffer: "0x0311",
            two_plane_header_flag: DIALOGUE_TWO_PLANE_HEADER_FLAG,
            two_plane_header_flag_hex: format!("{DIALOGUE_TWO_PLANE_HEADER_FLAG:02X}"),
            encoded_stage_count: usize::from(DIALOGUE_TWO_PLANE_HEADER_FLAG >> 5),
            stage_serializer_entry_cpu_address: 0xC842,
            stage_serializer_entry_cpu_address_hex: "0xC842".to_owned(),
            queued_command_buffer: "0x0781",
            output_layout: "combining_overlay_then_base_codes_with_equal_masked_width",
            code_regions: build_code_region_evidence(
                source,
                &DIALOGUE_RENDERER_CODE_REGIONS,
                "dialogue renderer",
                "bank_0A_progressive_dialogue_renderer",
            )?,
        },
        runtime_observation: DialogueRuntimeObservation {
            screen: "chapter_1_intro_dialogue",
            source_prg_bank: 0x08,
            source_prg_bank_hex: "0x08".to_owned(),
            source_cpu_address: 0x9FCE,
            source_cpu_address_hex: "0x9FCE".to_owned(),
            source_file_offset: 0x21FDE,
            source_file_offset_hex: "0x21FDE".to_owned(),
            destination_line_buffer_address: 0x78B2,
            destination_line_buffer_address_hex: "0x78B2".to_owned(),
            observed_control_code: 0xEA,
            observed_control_code_hex: "EA".to_owned(),
            observed_written_code: 0x9E,
            observed_written_code_hex: "9E".to_owned(),
            source_write_instruction_cpu_address: 0x8219,
            source_write_instruction_cpu_address_hex: "0x8219".to_owned(),
            source_write_event_pc: 0x821B,
            source_write_event_pc_hex: "0x821B".to_owned(),
            source_write_dropped_event_count: 0,
            observed_stage_descriptor: 0x52,
            observed_stage_descriptor_hex: "52".to_owned(),
            observed_line_width: usize::from(0x52 & DIALOGUE_STAGE_WIDTH_MASK),
            observed_stage_count: usize::from(0x52_u8 >> 5),
            stage_descriptor_write_instruction_cpu_address: 0x8469,
            stage_descriptor_write_instruction_cpu_address_hex: "0x8469".to_owned(),
            stage_descriptor_write_event_pc: 0x846C,
            stage_descriptor_write_event_pc_hex: "0x846C".to_owned(),
            stage_descriptor_write_dropped_event_count: 0,
        },
    })
}
