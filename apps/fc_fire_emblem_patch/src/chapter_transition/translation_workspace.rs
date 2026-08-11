use std::{collections::BTreeSet, path::Path};

use anyhow::{Context, Result, ensure};

use crate::{
    battle_text_workset::{FORECAST_LABEL_FILE_OFFSET, FORECAST_LABEL_SOURCE},
    font_slots::active_hangul_codes,
    japanese_encoding::is_japanese_text_code,
    rom::Rom,
    semantic_translation::{ExpectedSemanticEntry, plan_semantic_translation},
    text_inventory::decode_source_markup,
};

use super::{
    ending_scroll::bind_ending_aggregate_label_source,
    inspect_chapter_transition_translation_population, source_spec::SAVE_OFFER_LABEL_BYTES,
};

pub(crate) struct TransitionTranslationInput {
    pub(crate) workspace_sha1: String,
    pub(crate) entry_count: usize,
    pub(crate) review_complete: bool,
    pub(crate) target_glyphs: BTreeSet<char>,
    pub(crate) source_reclaimable_active_codes: BTreeSet<u8>,
}

pub(crate) struct TransitionTranslationPlans {
    pub(crate) forecast: TransitionTranslationInput,
    pub(crate) save_offer: TransitionTranslationInput,
    pub(crate) ending_record: TransitionTranslationInput,
}

pub(crate) fn plan_transition_labels(
    rom: &Rom,
    workspace_path: &Path,
) -> Result<TransitionTranslationPlans> {
    let population = inspect_chapter_transition_translation_population(rom)?;
    ensure!(
        rom.data().get(
            FORECAST_LABEL_FILE_OFFSET..FORECAST_LABEL_FILE_OFFSET + FORECAST_LABEL_SOURCE.len()
        ) == Some(FORECAST_LABEL_SOURCE.as_slice()),
        "battle forecast label source changed"
    );
    let ending = bind_ending_aggregate_label_source(rom)?;
    let expected_entries = [
        ExpectedSemanticEntry {
            id: "battle-forecast-label".to_owned(),
            japanese_markup: decode_source_markup(&FORECAST_LABEL_SOURCE[3..9]),
            max_visible_cells: usize::from(FORECAST_LABEL_SOURCE[2]),
        },
        ExpectedSemanticEntry {
            id: "chapter-save-offer-label".to_owned(),
            japanese_markup: decode_source_markup(
                &SAVE_OFFER_LABEL_BYTES[..SAVE_OFFER_LABEL_BYTES.len() - 1],
            ),
            max_visible_cells: SAVE_OFFER_LABEL_BYTES.len() - 1,
        },
        ExpectedSemanticEntry {
            id: "ending-total-turn-label".to_owned(),
            japanese_markup: ending.japanese_markup,
            max_visible_cells: ending.max_visible_cells,
        },
    ];
    let plan = plan_semantic_translation(workspace_path, &expected_entries)?;
    ensure!(
        population.battle_forecast_label_count == 1
            && population.save_offer_label_count == 1
            && population.ending_record_additional_record_count == 1
            && plan.entry_count == 3,
        "transition translation population changed"
    );
    let forecast_review_complete = plan.entry_review_complete("battle-forecast-label");
    let save_offer_review_complete = plan.entry_review_complete("chapter-save-offer-label");
    let ending_record_review_complete = plan.entry_review_complete("ending-total-turn-label");
    let active_codes = active_hangul_codes().into_iter().collect::<BTreeSet<_>>();
    let reclaimable = |codes: &[u8]| {
        codes
            .iter()
            .copied()
            .filter(|code| is_japanese_text_code(*code) && active_codes.contains(code))
            .collect::<BTreeSet<_>>()
    };
    let forecast_target_glyphs = plan
        .entry_target_glyphs("battle-forecast-label")
        .context("transition plan lost battle forecast target glyphs")?
        .clone();
    let save_offer_target_glyphs = plan
        .entry_target_glyphs("chapter-save-offer-label")
        .context("transition plan lost chapter save target glyphs")?
        .clone();
    let ending_record_target_glyphs = plan
        .entry_target_glyphs("ending-total-turn-label")
        .context("transition plan lost ending-record target glyphs")?
        .clone();
    Ok(TransitionTranslationPlans {
        forecast: TransitionTranslationInput {
            workspace_sha1: plan.workspace_sha1.clone(),
            entry_count: 1,
            review_complete: forecast_review_complete,
            target_glyphs: forecast_target_glyphs,
            source_reclaimable_active_codes: reclaimable(&FORECAST_LABEL_SOURCE[3..9]),
        },
        save_offer: TransitionTranslationInput {
            workspace_sha1: plan.workspace_sha1.clone(),
            entry_count: 1,
            review_complete: save_offer_review_complete,
            target_glyphs: save_offer_target_glyphs,
            source_reclaimable_active_codes: reclaimable(
                &SAVE_OFFER_LABEL_BYTES[..SAVE_OFFER_LABEL_BYTES.len() - 1],
            ),
        },
        ending_record: TransitionTranslationInput {
            workspace_sha1: plan.workspace_sha1,
            entry_count: 1,
            review_complete: ending_record_review_complete,
            target_glyphs: ending_record_target_glyphs,
            source_reclaimable_active_codes: ending.source_reclaimable_active_codes,
        },
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn public_workspace_covers_forecast_save_and_ending_labels() {
        let source = Path::new(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../../roms/Fire Emblem - Ankoku Ryuu to Hikari no Tsurugi (Japan).nes"
        ));
        if !source.exists() {
            return;
        }
        let workspace = Path::new(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../../assets/translation/transition-labels.ko.json"
        ));
        let rom = Rom::from_path(source).unwrap();
        let plan = plan_transition_labels(&rom, workspace).unwrap();
        assert_eq!(plan.forecast.entry_count, 1);
        assert_eq!(plan.save_offer.entry_count, 1);
        assert_eq!(plan.ending_record.entry_count, 1);
    }
}
