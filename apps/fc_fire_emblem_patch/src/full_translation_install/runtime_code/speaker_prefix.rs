//! 주 대사의 화자명 앞에 원본 일본어 표식이 다시 나타나지 않게 한다.
//!
//! 원본 `{EA}` 처리기는 스크립트에 저장되지 않은 `9E AB`를 줄 버퍼에 합성한다.
//! `AB`는 일본어 여는 괄호 `「`이고, 한글 화자명 바로 앞에 남으면 이름이 일본어와
//! 섞인 것처럼 보인다. 처리기의 길이와 두 칸 들여쓰기는 그대로 두고 출력만 빈 타일로
//! 투영한다.

use anyhow::{Result, ensure};

use super::{DialogueRuntimeHook, DialogueRuntimeHookRole, DialogueRuntimeHookSite};
use crate::{
    dialogue_inventory::switchable_cpu_to_file_offset,
    rom::Rom,
    rp2a03::{Instruction, assemble_at},
    typed_source::decode_rp2a03_sequence,
};

const MAIN_DIALOGUE_BANK: u8 = 0x0A;
const SPEAKER_PREFIX_OUTPUT_SITE: u16 = 0x8215;
const LINE_BUFFER_POINTER: u8 = 0x06;
const BLANK_TEXT_CODE: u8 = 0xFF;

const SOURCE_SPEAKER_PREFIX_OUTPUT: [u8; 11] = [
    0xA0, 0x00, // LDY #$00
    0xA9, 0x9E, // LDA #$9E
    0x91, 0x06, // STA ($06),Y
    0xC8, // INY
    0xA9, 0xAB, // LDA #$AB (`「`)
    0x91, 0x06, // STA ($06),Y
];

pub(super) fn bind_speaker_prefix_output(source: &Rom, candidate: &Rom) -> Result<()> {
    let file_offset =
        switchable_cpu_to_file_offset(MAIN_DIALOGUE_BANK, SPEAKER_PREFIX_OUTPUT_SITE)?;
    for (image_role, rom) in [("source", source), ("candidate", candidate)] {
        ensure!(
            rom.data()
                .get(file_offset..file_offset + SOURCE_SPEAKER_PREFIX_OUTPUT.len())
                == Some(SOURCE_SPEAKER_PREFIX_OUTPUT.as_slice()),
            "{image_role} dialogue speaker-prefix output changed at {MAIN_DIALOGUE_BANK:02X}:{SPEAKER_PREFIX_OUTPUT_SITE:04X}"
        );
    }
    decode_rp2a03_sequence(
        &SOURCE_SPEAKER_PREFIX_OUTPUT,
        SPEAKER_PREFIX_OUTPUT_SITE,
        "source dialogue speaker-prefix output",
    )?;
    Ok(())
}

pub(super) fn blank_speaker_prefix_output_hook() -> Result<DialogueRuntimeHook> {
    let bytes = assemble_at(
        SPEAKER_PREFIX_OUTPUT_SITE,
        &[
            Instruction::LdyImmediate(0),
            Instruction::LdaImmediate(BLANK_TEXT_CODE),
            Instruction::StaIndirectY(LINE_BUFFER_POINTER),
            Instruction::Iny,
            Instruction::LdaImmediate(BLANK_TEXT_CODE),
            Instruction::StaIndirectY(LINE_BUFFER_POINTER),
        ],
    )?;
    ensure!(
        bytes.len() == SOURCE_SPEAKER_PREFIX_OUTPUT.len(),
        "blank dialogue speaker-prefix projection changed handler length"
    );
    Ok(DialogueRuntimeHook {
        role: DialogueRuntimeHookRole::DialogueSpeakerPrefixProjection,
        write_role: "blank Japanese dialogue speaker-prefix output",
        site: DialogueRuntimeHookSite::Switchable {
            bank: MAIN_DIALOGUE_BANK,
            address: SPEAKER_PREFIX_OUTPUT_SITE,
        },
        bytes,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rom_with_source_speaker_prefix() -> Rom {
        let mut bytes = crate::test_support::synthetic_mapper165_rom_bytes(0xFF);
        let offset =
            switchable_cpu_to_file_offset(MAIN_DIALOGUE_BANK, SPEAKER_PREFIX_OUTPUT_SITE).unwrap();
        bytes[offset..offset + SOURCE_SPEAKER_PREFIX_OUTPUT.len()]
            .copy_from_slice(&SOURCE_SPEAKER_PREFIX_OUTPUT);
        Rom::parse(bytes).unwrap()
    }

    #[test]
    fn translated_speaker_prefix_outputs_two_blank_tiles_without_moving_the_line() {
        let hook = blank_speaker_prefix_output_hook().unwrap();

        assert_eq!(
            hook.bytes,
            [
                0xA0, 0x00, 0xA9, 0xFF, 0x91, 0x06, 0xC8, 0xA9, 0xFF, 0x91, 0x06,
            ]
        );
        assert_eq!(hook.bytes.len(), SOURCE_SPEAKER_PREFIX_OUTPUT.len());
        assert!(!hook.bytes.windows(2).any(|pair| pair == [0xA9, 0x9E]));
        assert!(!hook.bytes.windows(2).any(|pair| pair == [0xA9, 0xAB]));
    }

    #[test]
    fn speaker_prefix_projection_is_bound_to_the_exact_source_handler() {
        let source = rom_with_source_speaker_prefix();
        let candidate = rom_with_source_speaker_prefix();
        bind_speaker_prefix_output(&source, &candidate).unwrap();

        let mut drifted = crate::test_support::synthetic_mapper165_rom_bytes(0xFF);
        let offset =
            switchable_cpu_to_file_offset(MAIN_DIALOGUE_BANK, SPEAKER_PREFIX_OUTPUT_SITE).unwrap();
        drifted[offset..offset + SOURCE_SPEAKER_PREFIX_OUTPUT.len()]
            .copy_from_slice(&SOURCE_SPEAKER_PREFIX_OUTPUT);
        drifted[offset + 8] ^= 1;
        let drifted = Rom::parse(drifted).unwrap();

        let error = bind_speaker_prefix_output(&source, &drifted).unwrap_err();
        assert!(error.to_string().contains("speaker-prefix output changed"));
    }
}
