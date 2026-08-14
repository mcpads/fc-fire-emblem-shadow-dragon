//! 장 저장 질문과 저장 완료 선택지를 한 고정 코드 계약으로 저장소에 투영한다.

use std::collections::BTreeMap;

use anyhow::{Context, Result, ensure};
use serde::Serialize;

use crate::{
    chapter_transition::{
        SAVE_OFFER_LABEL_ADDRESS, SAVE_OFFER_LABEL_BYTES, TransitionTranslationPlans,
    },
    choice_labels::{ChoiceLabelPlan, POINTER_TABLE_ADDRESS},
    dialogue_inventory::switchable_cpu_to_file_offset,
    rom::Rom,
    sha1_hex,
};

use super::consumer_codebook::ConsumerCodebookPlan;

const FIXED_UI_BANK: u8 = 0x0B;
const SEGMENT_END: u8 = 0xED;

pub(super) struct ChapterSaveProjectionInputs<'a> {
    pub(super) candidate: &'a Rom,
    pub(super) choices: &'a ChoiceLabelPlan,
    pub(super) choice_glyph_codes: &'a BTreeMap<char, u8>,
    pub(super) transitions: &'a TransitionTranslationPlans,
    pub(super) consumer_codebook: &'a ConsumerCodebookPlan,
}

#[derive(Serialize)]
pub(super) struct ChapterSaveProjectionPlan {
    strategy: &'static str,
    choice_entry_count: usize,
    save_offer_entry_count: usize,
    storage_write_count: usize,
    projected_byte_count: usize,
    projection_sha1: String,
    choice_pointer_table_source_bound: bool,
    save_offer_pointer_source_bound: bool,
    every_projected_string_fits_source_storage: bool,
    choice_codes_match_continue_prompt_residency: bool,
    #[serde(skip)]
    writes: Vec<ChapterSaveExpectedWrite>,
}

impl ChapterSaveProjectionPlan {
    pub(super) fn writes(&self) -> &[ChapterSaveExpectedWrite] {
        &self.writes
    }

    pub(super) fn write_count(&self) -> usize {
        self.writes.len()
    }

    pub(super) fn write_count_for_domain(&self, domain: &str) -> usize {
        self.writes
            .iter()
            .filter(|write| write.domain == domain)
            .count()
    }

    pub(super) fn projected_screen_roles(&self, domain: &str) -> &'static [&'static str] {
        match domain {
            "chapter_save_offer_label" => &["chapter_save_offer"],
            "choice_labels" => &[
                "chapter_save_offer",
                "chapter_save_complete_continue_prompt",
            ],
            _ => &[],
        }
    }
}

pub(super) struct ChapterSaveExpectedWrite {
    pub(super) domain: &'static str,
    pub(super) role: String,
    pub(super) file_offset: usize,
    pub(super) expected: Vec<u8>,
    pub(super) replacement: Vec<u8>,
}

pub(super) fn plan_chapter_save_projection(
    inputs: ChapterSaveProjectionInputs<'_>,
) -> Result<ChapterSaveProjectionPlan> {
    ensure!(
        inputs.choices.entries.len() == 2
            && inputs.transitions.save_offer.entry_count == 1
            && !inputs.choice_glyph_codes.is_empty(),
        "chapter-save projection input population changed"
    );
    let mut writes = Vec::new();
    let mut identity = Vec::new();
    for entry in &inputs.choices.entries {
        let pointer_offset = switchable_cpu_to_file_offset(
            FIXED_UI_BANK,
            POINTER_TABLE_ADDRESS + u16::from(entry.fixed_string_index) * 2,
        )?;
        bind_candidate(
            inputs.candidate,
            pointer_offset,
            &entry.source_pointer.to_le_bytes(),
            &format!("{} fallback pointer", entry.id),
        )?;
        let encoded = entry.encoded_bytes(inputs.choice_glyph_codes)?;
        ensure!(
            encoded.len() <= entry.source_storage.len(),
            "chapter-save choice projection exceeds source storage for {}",
            entry.id
        );
        bind_candidate(
            inputs.candidate,
            entry.source_file_offset,
            &entry.source_storage,
            &format!("{} source storage", entry.id),
        )?;
        let mut replacement = vec![0xFF; entry.source_storage.len()];
        replacement[..encoded.len()].copy_from_slice(&encoded);
        identity.extend_from_slice(entry.id.as_bytes());
        identity.extend_from_slice(&replacement);
        writes.push(ChapterSaveExpectedWrite {
            domain: "choice_labels",
            role: format!("{} chapter-save storage projection", entry.id),
            file_offset: entry.source_file_offset,
            expected: entry.source_storage.clone(),
            replacement,
        });
    }

    let save_pointer_offset = switchable_cpu_to_file_offset(FIXED_UI_BANK, 0x9026)?;
    bind_candidate(
        inputs.candidate,
        save_pointer_offset,
        &SAVE_OFFER_LABEL_ADDRESS.to_le_bytes(),
        "chapter-save offer source pointer",
    )?;
    let save_offset = switchable_cpu_to_file_offset(FIXED_UI_BANK, SAVE_OFFER_LABEL_ADDRESS)?;
    bind_candidate(
        inputs.candidate,
        save_offset,
        SAVE_OFFER_LABEL_BYTES,
        "chapter-save offer source storage",
    )?;
    let mut encoded_save = inputs.consumer_codebook.encode_fixed_ui_for(
        "chapter_save_offer",
        &inputs.transitions.save_offer.logical_bytes,
    )?;
    encoded_save.push(SEGMENT_END);
    ensure!(
        encoded_save.len() <= SAVE_OFFER_LABEL_BYTES.len(),
        "chapter-save offer projection exceeds source storage"
    );
    let mut save_replacement = vec![0xFF; SAVE_OFFER_LABEL_BYTES.len()];
    save_replacement[..encoded_save.len()].copy_from_slice(&encoded_save);
    identity.extend_from_slice(b"chapter-save-offer-label");
    identity.extend_from_slice(&save_replacement);
    writes.push(ChapterSaveExpectedWrite {
        domain: "chapter_save_offer_label",
        role: "chapter-save offer label storage projection".to_owned(),
        file_offset: save_offset,
        expected: SAVE_OFFER_LABEL_BYTES.to_vec(),
        replacement: save_replacement,
    });

    ensure_disjoint(&writes)?;
    Ok(ChapterSaveProjectionPlan {
        strategy: "keep weapon-shop choices in their existing lifetime-specific cave, project the original fallback choices with continue-prompt fixed codes, and use those same codes on a dedicated chapter-save page",
        choice_entry_count: inputs.choices.entries.len(),
        save_offer_entry_count: inputs.transitions.save_offer.entry_count,
        storage_write_count: writes.len(),
        projected_byte_count: writes.iter().map(|write| write.replacement.len()).sum(),
        projection_sha1: sha1_hex(&identity),
        choice_pointer_table_source_bound: true,
        save_offer_pointer_source_bound: true,
        every_projected_string_fits_source_storage: true,
        choice_codes_match_continue_prompt_residency: true,
        writes,
    })
}

fn bind_candidate(candidate: &Rom, offset: usize, expected: &[u8], role: &str) -> Result<()> {
    ensure!(
        candidate.data().get(offset..offset + expected.len()) == Some(expected),
        "{role} changed in the exact candidate"
    );
    Ok(())
}

fn ensure_disjoint(writes: &[ChapterSaveExpectedWrite]) -> Result<()> {
    for (index, left) in writes.iter().enumerate() {
        let left_end = left
            .file_offset
            .checked_add(left.expected.len())
            .context("chapter-save projection range overflow")?;
        for right in &writes[index + 1..] {
            let right_end = right
                .file_offset
                .checked_add(right.expected.len())
                .context("chapter-save projection range overflow")?;
            ensure!(
                left_end <= right.file_offset || right_end <= left.file_offset,
                "chapter-save projection writes overlap"
            );
        }
    }
    Ok(())
}
