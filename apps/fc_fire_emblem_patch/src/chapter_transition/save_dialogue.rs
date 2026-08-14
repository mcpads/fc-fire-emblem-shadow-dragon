use anyhow::Result;

use crate::rom::Rom;

use super::{
    bind_source_region,
    source_spec::{
        OPEN_SAVE_COMPLETE_CONTINUE_PROMPT_BYTES, OPEN_SAVE_COMPLETE_POWER_OFF_NOTICE_BYTES,
        SourceRegionSpec,
    },
};

pub(crate) struct SaveCompleteDialogueRecords {
    pub(crate) continue_prompt: &'static str,
    pub(crate) power_off_notice: &'static str,
}

pub(crate) fn bind_save_complete_dialogue_records(
    rom: &Rom,
) -> Result<SaveCompleteDialogueRecords> {
    bind_source_region(
        rom,
        SourceRegionSpec::code(
            "open_save_complete_continue_prompt",
            0x0B,
            0x9AFC,
            OPEN_SAVE_COMPLETE_CONTINUE_PROMPT_BYTES,
        ),
    )?;
    bind_source_region(
        rom,
        SourceRegionSpec::code(
            "open_save_complete_power_off_notice",
            0x0B,
            0x9B8A,
            OPEN_SAVE_COMPLETE_POWER_OFF_NOTICE_BYTES,
        ),
    )?;
    Ok(SaveCompleteDialogueRecords {
        continue_prompt: "victory-and-defeat-dialogue:000",
        power_off_notice: "victory-and-defeat-dialogue:001",
    })
}
