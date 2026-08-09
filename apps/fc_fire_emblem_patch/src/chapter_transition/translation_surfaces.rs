use anyhow::{Result, ensure};
use serde::Serialize;

use crate::{
    dialogue_inventory::{
        TranslationSurfaceDialogueTableBinding, inspect_translation_surface_dialogue_tables,
    },
    rom::Rom,
};

use super::{
    battle_translation::{
        BattleAnimationTranslationSurface, bind_battle_animation_translation_surface,
    },
    ending_epilogue::{EndingCharacterEpilogueTranslationSurface, bind_ending_character_epilogue},
    ending_scroll::{
        EndingChapterRecordTranslationSurface, bind_ending_chapter_record_translation_surface,
    },
};

#[derive(Debug, Serialize)]
pub(super) struct TranslationSurfaceContracts {
    battle_animation: BattleAnimationTranslationSurface,
    ending_chapter_record_scroll: EndingChapterRecordTranslationSurface,
    ending_character_epilogue: EndingCharacterEpilogueTranslationSurface,
    dialogue_tables: Vec<TranslationSurfaceDialogueTableBinding>,
    proof_boundary: &'static str,
}

pub(super) fn bind_translation_surfaces(rom: &Rom) -> Result<TranslationSurfaceContracts> {
    let dialogue_tables = inspect_translation_surface_dialogue_tables(rom.data())?;
    ensure!(
        dialogue_tables.len() == 3,
        "translation-surface dialogue table count changed"
    );

    Ok(TranslationSurfaceContracts {
        battle_animation: bind_battle_animation_translation_surface(rom, &dialogue_tables)?,
        ending_chapter_record_scroll: bind_ending_chapter_record_translation_surface(rom)?,
        ending_character_epilogue: bind_ending_character_epilogue(rom, &dialogue_tables)?,
        dialogue_tables,
        proof_boundary: "the supported Japanese ROM binds the common battle engine to five fixed text tables, twenty-two short message templates, one inline forecast label, and the separate battle-dialogue loader; it also binds the ending record stream and automatic character epilogue selectors 0x40 and 0x41; only code sets and structural counts are emitted",
    })
}
