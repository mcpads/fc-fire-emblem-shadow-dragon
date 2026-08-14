//! 엔딩 전적 스트림의 장 제목 복제본과 턴 라벨을 한 고정 페이지 코드로 투영한다.

use anyhow::{Context, Result, ensure};
use serde::Serialize;

use crate::{
    chapter_transition::{
        ChapterTitlePlan, EndingChapterRecordStorageSource, EndingChapterRowStorageSource,
        TransitionTranslationPlans, bind_ending_chapter_record_storage_source,
    },
    rom::Rom,
    sha1_hex,
    text_inventory::FixedTextLogicalByte,
};

use super::consumer_codebook::ConsumerCodebookPlan;

const ENDING_PAGE_ID: &str = "ending_chapter_record";
const TURN_INTERPOLATION: u8 = 0xED;
const AGGREGATE_TURN_SLOT: u8 = 0x19;
const TURN_UNIT_GLYPH: char = '턴';
const PADDING: u8 = 0xFF;

pub(super) struct EndingRecordProjectionInputs<'a> {
    pub(super) source: &'a Rom,
    pub(super) candidate: &'a Rom,
    pub(super) chapter_titles: &'a ChapterTitlePlan,
    pub(super) transitions: &'a TransitionTranslationPlans,
    pub(super) consumer_codebook: &'a ConsumerCodebookPlan,
}

#[derive(Serialize)]
pub(super) struct EndingRecordProjectionPlan {
    strategy: &'static str,
    source_stream_sha1: String,
    chapter_row_count: usize,
    chapter_title_write_count: usize,
    turn_suffix_write_count: usize,
    aggregate_label_write_count: usize,
    storage_write_count: usize,
    projected_byte_count: usize,
    projection_sha1: String,
    every_source_control_is_candidate_bound: bool,
    every_projection_preserves_owned_extent: bool,
    every_chapter_title_uses_intro_resident_codes: bool,
    turn_unit_comes_from_approved_aggregate_translation: bool,
    #[serde(skip)]
    writes: Vec<EndingRecordExpectedWrite>,
}

impl EndingRecordProjectionPlan {
    pub(super) fn writes(&self) -> &[EndingRecordExpectedWrite] {
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
            "chapter_titles" | "ending_record_labels" => &["ending_chapter_record_scroll"],
            _ => &[],
        }
    }
}

pub(super) struct EndingRecordExpectedWrite {
    pub(super) domain: &'static str,
    pub(super) role: String,
    pub(super) file_offset: usize,
    pub(super) expected: Vec<u8>,
    pub(super) replacement: Vec<u8>,
}

pub(super) fn plan_ending_record_projection(
    inputs: EndingRecordProjectionInputs<'_>,
) -> Result<EndingRecordProjectionPlan> {
    let storage = bind_ending_chapter_record_storage_source(inputs.source)?;
    ensure!(
        storage.chapter_rows.len() == 25
            && inputs.chapter_titles.entries.len() == storage.chapter_rows.len()
            && inputs.transitions.ending_record.entry_count == 1,
        "ending-record projection input population changed"
    );
    ensure!(
        inputs
            .transitions
            .ending_record
            .logical_bytes
            .iter()
            .filter(|byte| **byte == FixedTextLogicalByte::TargetGlyph(TURN_UNIT_GLYPH))
            .count()
            == 1,
        "approved ending aggregate translation no longer contains one turn-unit glyph"
    );

    let mut writes = Vec::with_capacity(storage.chapter_rows.len() * 2 + 1);
    let mut identity = Vec::new();
    for row in &storage.chapter_rows {
        let title = inputs.chapter_titles.entry(row.chapter_index)?;
        bind_row_source(inputs.candidate, row)?;

        let mut encoded_title = inputs
            .consumer_codebook
            .encode_chapter_title_for(ENDING_PAGE_ID, title.logical_bytes())?;
        ensure!(
            encoded_title.len() <= row.title_source_storage.len(),
            "{} needs {} ending-row title cells but owns only {}",
            title.id,
            encoded_title.len(),
            row.title_source_storage.len()
        );
        encoded_title.resize(row.title_source_storage.len(), PADDING);
        push_write(
            &mut writes,
            &mut identity,
            EndingRecordExpectedWrite {
                domain: "chapter_titles",
                role: format!("{} ending-row title projection", title.id),
                file_offset: row.title_file_offset,
                expected: row.title_source_storage.clone(),
                replacement: encoded_title,
            },
        );

        let mut encoded_turn_unit = inputs.consumer_codebook.encode_fixed_ui_for(
            ENDING_PAGE_ID,
            &[FixedTextLogicalByte::TargetGlyph(TURN_UNIT_GLYPH)],
        )?;
        ensure!(
            encoded_turn_unit.len() <= row.turn_suffix_source_storage.len(),
            "{} ending-row turn unit exceeds its source suffix",
            title.id
        );
        encoded_turn_unit.resize(row.turn_suffix_source_storage.len(), PADDING);
        push_write(
            &mut writes,
            &mut identity,
            EndingRecordExpectedWrite {
                domain: "ending_record_labels",
                role: format!("{} ending-row turn-unit projection", title.id),
                file_offset: row.turn_suffix_file_offset,
                expected: row.turn_suffix_source_storage.clone(),
                replacement: encoded_turn_unit,
            },
        );
    }

    let aggregate_replacement =
        encode_aggregate_record(&storage, inputs.transitions, inputs.consumer_codebook)?;
    bind_candidate(
        inputs.candidate,
        storage.aggregate.payload_file_offset,
        &storage.aggregate.source_storage,
        "ending aggregate source storage",
    )?;
    push_write(
        &mut writes,
        &mut identity,
        EndingRecordExpectedWrite {
            domain: "ending_record_labels",
            role: "ending aggregate label projection".to_owned(),
            file_offset: storage.aggregate.payload_file_offset,
            expected: storage.aggregate.source_storage.clone(),
            replacement: aggregate_replacement,
        },
    );

    ensure_disjoint(&writes)?;
    let chapter_title_write_count = writes
        .iter()
        .filter(|write| write.domain == "chapter_titles")
        .count();
    let ending_label_write_count = writes
        .iter()
        .filter(|write| write.domain == "ending_record_labels")
        .count();
    ensure!(
        chapter_title_write_count == 25 && ending_label_write_count == 26,
        "ending-record projection write population changed"
    );

    Ok(EndingRecordProjectionPlan {
        strategy: "keep every ending record and interpolation slot in place; encode the twenty-five duplicated titles with their intro-resident codes, replace each Japanese turn suffix with the approved Korean turn glyph, and project the aggregate label around its fixed runtime slot",
        source_stream_sha1: storage.stream_sha1,
        chapter_row_count: storage.chapter_rows.len(),
        chapter_title_write_count,
        turn_suffix_write_count: storage.chapter_rows.len(),
        aggregate_label_write_count: 1,
        storage_write_count: writes.len(),
        projected_byte_count: writes.iter().map(|write| write.replacement.len()).sum(),
        projection_sha1: sha1_hex(&identity),
        every_source_control_is_candidate_bound: true,
        every_projection_preserves_owned_extent: true,
        every_chapter_title_uses_intro_resident_codes: true,
        turn_unit_comes_from_approved_aggregate_translation: true,
        writes,
    })
}

fn bind_row_source(candidate: &Rom, row: &EndingChapterRowStorageSource) -> Result<()> {
    bind_candidate(
        candidate,
        row.title_file_offset,
        &row.title_source_storage,
        &format!("ending chapter {} title source", row.chapter_index + 1),
    )?;
    bind_candidate(
        candidate,
        row.turn_control_file_offset,
        &row.turn_control_source,
        &format!("ending chapter {} turn control", row.chapter_index + 1),
    )?;
    bind_candidate(
        candidate,
        row.turn_suffix_file_offset,
        &row.turn_suffix_source_storage,
        &format!("ending chapter {} turn suffix", row.chapter_index + 1),
    )
}

fn encode_aggregate_record(
    storage: &EndingChapterRecordStorageSource,
    transitions: &TransitionTranslationPlans,
    consumer_codebook: &ConsumerCodebookPlan,
) -> Result<Vec<u8>> {
    let encoded = consumer_codebook
        .encode_fixed_ui_for(ENDING_PAGE_ID, &transitions.ending_record.logical_bytes)?;
    preserve_interpolation_column(
        &encoded,
        storage.aggregate.source_storage.len(),
        storage.aggregate.interpolation_offset,
    )
}

fn preserve_interpolation_column(
    encoded: &[u8],
    source_storage_len: usize,
    source_interpolation_offset: usize,
) -> Result<Vec<u8>> {
    let interpolation_offset = encoded
        .iter()
        .position(|byte| *byte == TURN_INTERPOLATION)
        .context("encoded ending aggregate label has no turn interpolation")?;
    ensure!(
        encoded.get(interpolation_offset..interpolation_offset + 2)
            == Some([TURN_INTERPOLATION, AGGREGATE_TURN_SLOT].as_slice())
            && !encoded[interpolation_offset + 2..].contains(&TURN_INTERPOLATION),
        "encoded ending aggregate interpolation changed"
    );
    ensure!(
        interpolation_offset <= source_interpolation_offset,
        "encoded ending aggregate prefix exceeds its source display column"
    );
    let suffix_capacity = source_storage_len
        .checked_sub(source_interpolation_offset + 2)
        .context("ending aggregate source has no interpolation capacity")?;
    let encoded_suffix = &encoded[interpolation_offset + 2..];
    ensure!(
        encoded_suffix.len() <= suffix_capacity,
        "encoded ending aggregate suffix exceeds its source storage"
    );

    let mut replacement = encoded[..interpolation_offset].to_vec();
    replacement.resize(source_interpolation_offset, PADDING);
    replacement.extend_from_slice(&[TURN_INTERPOLATION, AGGREGATE_TURN_SLOT]);
    replacement.extend_from_slice(encoded_suffix);
    replacement.resize(source_storage_len, PADDING);
    Ok(replacement)
}

fn bind_candidate(candidate: &Rom, offset: usize, expected: &[u8], role: &str) -> Result<()> {
    ensure!(
        candidate.data().get(offset..offset + expected.len()) == Some(expected),
        "{role} changed in the exact candidate"
    );
    Ok(())
}

fn push_write(
    writes: &mut Vec<EndingRecordExpectedWrite>,
    identity: &mut Vec<u8>,
    write: EndingRecordExpectedWrite,
) {
    identity.extend_from_slice(write.role.as_bytes());
    identity.extend_from_slice(&write.file_offset.to_le_bytes());
    identity.extend_from_slice(&write.replacement);
    writes.push(write);
}

fn ensure_disjoint(writes: &[EndingRecordExpectedWrite]) -> Result<()> {
    for (index, left) in writes.iter().enumerate() {
        let left_end = left
            .file_offset
            .checked_add(left.expected.len())
            .context("ending-record projection range overflow")?;
        for right in &writes[index + 1..] {
            let right_end = right
                .file_offset
                .checked_add(right.expected.len())
                .context("ending-record projection range overflow")?;
            ensure!(
                left_end <= right.file_offset || right_end <= left.file_offset,
                "ending-record projection writes overlap"
            );
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn aggregate_projection_keeps_the_runtime_turn_slot_at_its_source_column() {
        let projected = preserve_interpolation_column(
            &[0xC0, 0xC1, 0xC2, TURN_INTERPOLATION, AGGREGATE_TURN_SLOT],
            10,
            8,
        )
        .unwrap();

        assert_eq!(
            projected,
            [
                0xC0,
                0xC1,
                0xC2,
                PADDING,
                PADDING,
                PADDING,
                PADDING,
                PADDING,
                TURN_INTERPOLATION,
                AGGREGATE_TURN_SLOT,
            ]
        );
    }

    #[test]
    fn aggregate_projection_rejects_a_prefix_that_crosses_the_runtime_slot() {
        let error = preserve_interpolation_column(
            &[0xC0, 0xC1, 0xC2, TURN_INTERPOLATION, AGGREGATE_TURN_SLOT],
            6,
            2,
        )
        .unwrap_err();

        assert!(
            error
                .to_string()
                .contains("exceeds its source display column")
        );
    }
}
