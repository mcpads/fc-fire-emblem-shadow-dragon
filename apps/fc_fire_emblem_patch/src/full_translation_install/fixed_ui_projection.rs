//! 고정 문자열 포인터 표가 소유한 일본어 슬롯을 화면별 코드북 바이트로 다시 묶는다.
//!
//! 일부 한국어 명령은 자기 원문 슬롯보다 길다. 새 코드 동굴을 소비하지 않고, 같은
//! 포인터 표가 가리키는 일본어 전용 슬롯 29개를 길이 내림차순으로 일대일 대응한 뒤
//! 포인터를 갱신한다. 가장 긴 목표가 가장 긴 슬롯에 들어가는지 모두 확인하므로 이
//! 대응은 단순 탐욕 추정이 아니라 일대일 용량 조건의 완전한 판정이다. 요약 라벨은
//! 원문의 오른쪽 정렬과 `$8D,$EF` 꼬리를, 명령·아이템 동작은 `$ED` 종단을 지킨다.

use std::cmp::Reverse;

use anyhow::{Context, Result, ensure};
use serde::Serialize;

use crate::{
    dialogue_inventory::switchable_cpu_to_file_offset,
    item_flow::ITEM_ACTION_LABELS,
    map_menu::MapMenuPlan,
    rom::Rom,
    semantic_translation::SemanticTranslationPlan,
    sha1_hex,
    unit_ui_text::{
        COMMAND_LABEL_SPECS, FixedLabelSpec, SUMMARY_AND_STATUS_LABEL_SPECS,
        composite_payload_display_cell_count, terminated_composite_display_cell_count,
    },
};

use super::{consumer_catalog::ConsumerCatalogPlan, consumer_codebook::ConsumerCodebookPlan};

const FIXED_UI_BANK: u8 = 0x0B;
const FIXED_STRING_POINTER_TABLE: u16 = 0x8FC2;
const JAPANESE_ONLY: &str = "japanese_only";
const SEGMENT_END: u8 = 0xED;
const SUMMARY_TRAILING_CELL: u8 = 0x8D;
const STRING_END: u8 = 0xEF;

pub(super) struct FixedUiProjectionInputs<'a> {
    pub(super) candidate: &'a Rom,
    pub(super) unit_ui: &'a SemanticTranslationPlan,
    pub(super) item_actions: &'a SemanticTranslationPlan,
    pub(super) map_menu: &'a MapMenuPlan,
    pub(super) consumer_codebook: &'a ConsumerCodebookPlan,
    pub(super) consumer_catalog: &'a ConsumerCatalogPlan,
}

#[derive(Serialize)]
pub(super) struct FixedUiProjectionPlan {
    strategy: &'static str,
    pointer_table_cpu_address_hex: &'static str,
    source_slot_count: usize,
    projected_pointer_entry_count: usize,
    projected_map_menu_entry_count: usize,
    projected_summary_status_label_count: usize,
    source_slot_capacity_byte_count: usize,
    projected_string_byte_count: usize,
    longest_source_slot_byte_count: usize,
    longest_projected_string_byte_count: usize,
    storage_write_count: usize,
    pointer_write_count: usize,
    map_menu_write_count: usize,
    assignment_sha1: String,
    every_source_slot_bound_to_candidate: bool,
    every_projected_string_fits_one_source_slot: bool,
    every_summary_status_label_preserves_source_display_cell_count: bool,
    every_pointer_source_bound: bool,
    every_map_menu_entry_fits_owned_storage: bool,
    #[serde(skip)]
    writes: Vec<FixedUiExpectedWrite>,
}

impl FixedUiProjectionPlan {
    pub(super) fn writes(&self) -> &[FixedUiExpectedWrite] {
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
            "unit_ui_labels" => &["unit_command_menu", "unit_status", "unit_summary"],
            "item_action_labels" => &["item_action_menu"],
            "map_menu_labels" => &["map_menu", "map_funds_summary"],
            _ => &[],
        }
    }
}

pub(super) struct FixedUiExpectedWrite {
    pub(super) domain: &'static str,
    pub(super) role: String,
    pub(super) file_offset: usize,
    pub(super) expected: Vec<u8>,
    pub(super) replacement: Vec<u8>,
}

struct TargetString {
    id: String,
    domain: &'static str,
    pointer_index: u8,
    source_pointer: u16,
    bytes: Vec<u8>,
}

struct SourceSlot {
    pointer: u16,
    expected: Vec<u8>,
}

pub(super) fn plan_fixed_ui_projection(
    inputs: FixedUiProjectionInputs<'_>,
) -> Result<FixedUiProjectionPlan> {
    let mut targets = Vec::new();
    let mut slots = Vec::new();

    for spec in SUMMARY_AND_STATUS_LABEL_SPECS
        .iter()
        .filter(|spec| spec.translation_scope == JAPANESE_ONLY)
    {
        let id = format!("unit-ui-label:{:02X}", spec.index);
        let logical = inputs
            .unit_ui
            .entry_logical_bytes(&id)
            .with_context(|| format!("unit UI plan lost {id}"))?;
        let encoded = inputs.consumer_catalog.encode_base_logical(logical)?;
        let bytes = project_summary_status_label(spec, &encoded, &id)?;
        targets.push(TargetString {
            id,
            domain: "unit_ui_labels",
            pointer_index: spec.index,
            source_pointer: spec.pointer,
            bytes,
        });
        slots.push(SourceSlot {
            pointer: spec.pointer,
            expected: spec.expected.to_vec(),
        });
    }
    for spec in COMMAND_LABEL_SPECS
        .iter()
        .filter(|spec| spec.translation_scope == JAPANESE_ONLY)
    {
        let id = format!("unit-ui-label:{:02X}", spec.index);
        let logical = inputs
            .unit_ui
            .entry_logical_bytes(&id)
            .with_context(|| format!("unit UI plan lost {id}"))?;
        let mut bytes = inputs
            .consumer_codebook
            .encode_fixed_ui_for("unit_command_menu", logical)?;
        bytes.push(SEGMENT_END);
        targets.push(TargetString {
            id,
            domain: "unit_ui_labels",
            pointer_index: spec.index,
            source_pointer: spec.pointer,
            bytes,
        });
        slots.push(SourceSlot {
            pointer: spec.pointer,
            expected: spec.expected.to_vec(),
        });
    }
    for spec in ITEM_ACTION_LABELS
        .iter()
        .filter(|spec| spec.translation_scope == JAPANESE_ONLY)
    {
        let id = format!("item-action-label:{:02X}", spec.index);
        let logical = inputs
            .item_actions
            .entry_logical_bytes(&id)
            .with_context(|| format!("item-action plan lost {id}"))?;
        let mut bytes = inputs.consumer_catalog.encode_base_logical(logical)?;
        bytes.push(SEGMENT_END);
        targets.push(TargetString {
            id,
            domain: "item_action_labels",
            pointer_index: spec.index,
            source_pointer: spec.pointer,
            bytes,
        });
        slots.push(SourceSlot {
            pointer: spec.pointer,
            expected: spec.expected.to_vec(),
        });
    }
    ensure!(
        targets.len() == 29 && slots.len() == targets.len(),
        "fixed UI projection requires twenty-nine Japanese pointer-table labels"
    );

    match_targets_to_slots(&mut targets, &mut slots)?;

    let mut writes = Vec::new();
    let mut assignment_identity = Vec::new();
    for (target, slot) in targets.iter().zip(&slots) {
        let storage_offset = switchable_cpu_to_file_offset(FIXED_UI_BANK, slot.pointer)?;
        bind_candidate(
            inputs.candidate,
            storage_offset,
            &slot.expected,
            &format!("fixed UI source slot {:04X}", slot.pointer),
        )?;
        let mut replacement = vec![0xFF; slot.expected.len()];
        replacement[..target.bytes.len()].copy_from_slice(&target.bytes);
        writes.push(FixedUiExpectedWrite {
            domain: target.domain,
            role: format!("{} storage projection", target.id),
            file_offset: storage_offset,
            expected: slot.expected.clone(),
            replacement: replacement.clone(),
        });

        let pointer_address = FIXED_STRING_POINTER_TABLE + u16::from(target.pointer_index) * 2;
        let pointer_offset = switchable_cpu_to_file_offset(FIXED_UI_BANK, pointer_address)?;
        let expected_pointer = target.source_pointer.to_le_bytes();
        bind_candidate(
            inputs.candidate,
            pointer_offset,
            &expected_pointer,
            &format!("{} source pointer", target.id),
        )?;
        let projected_pointer = slot.pointer.to_le_bytes();
        writes.push(FixedUiExpectedWrite {
            domain: target.domain,
            role: format!("{} pointer projection", target.id),
            file_offset: pointer_offset,
            expected: expected_pointer.to_vec(),
            replacement: projected_pointer.to_vec(),
        });

        assignment_identity.extend_from_slice(target.domain.as_bytes());
        assignment_identity.extend_from_slice(target.id.as_bytes());
        assignment_identity.extend_from_slice(&slot.pointer.to_le_bytes());
        assignment_identity.extend_from_slice(&replacement);
    }

    for entry in &inputs.map_menu.entries {
        let encoded = inputs
            .consumer_codebook
            .encode_fixed_ui_for("map_menu", entry.logical_bytes())?;
        bind_candidate(
            inputs.candidate,
            entry.source_file_offset,
            &entry.source_storage,
            &format!("{} source storage", entry.id),
        )?;
        let replacement = project_map_menu_entry(entry, &encoded)?;
        assignment_identity.extend_from_slice(entry.id.as_bytes());
        assignment_identity.extend_from_slice(&replacement);
        writes.push(FixedUiExpectedWrite {
            domain: "map_menu_labels",
            role: format!("{} storage projection", entry.id),
            file_offset: entry.source_file_offset,
            expected: entry.source_storage.clone(),
            replacement,
        });
    }

    ensure_disjoint_writes(&writes)?;
    let source_slot_capacity_byte_count = slots.iter().map(|slot| slot.expected.len()).sum();
    let projected_string_byte_count = targets.iter().map(|target| target.bytes.len()).sum();
    Ok(FixedUiProjectionPlan {
        strategy: "preserve each summary/status label's source display-cell span, match every encoded Japanese fixed-label target to one source-owned pointer-table slot by descending storage length, then project the map-menu block in place",
        pointer_table_cpu_address_hex: "0x8FC2",
        source_slot_count: slots.len(),
        projected_pointer_entry_count: targets.len(),
        projected_map_menu_entry_count: inputs.map_menu.entries.len(),
        projected_summary_status_label_count: SUMMARY_AND_STATUS_LABEL_SPECS
            .iter()
            .filter(|spec| spec.translation_scope == JAPANESE_ONLY)
            .count(),
        source_slot_capacity_byte_count,
        projected_string_byte_count,
        longest_source_slot_byte_count: slots.first().map_or(0, |slot| slot.expected.len()),
        longest_projected_string_byte_count: targets.first().map_or(0, |target| target.bytes.len()),
        storage_write_count: targets.len(),
        pointer_write_count: targets.len(),
        map_menu_write_count: inputs.map_menu.entries.len(),
        assignment_sha1: sha1_hex(&assignment_identity),
        every_source_slot_bound_to_candidate: true,
        every_projected_string_fits_one_source_slot: true,
        every_summary_status_label_preserves_source_display_cell_count: true,
        every_pointer_source_bound: true,
        every_map_menu_entry_fits_owned_storage: true,
        writes,
    })
}

fn project_map_menu_entry(
    entry: &crate::map_menu::MapMenuPlannedEntry,
    encoded: &[u8],
) -> Result<Vec<u8>> {
    project_map_menu_storage(
        &entry.id,
        &entry.source_storage,
        &entry.preserved_suffix,
        entry.source_display_cell_count,
        encoded,
    )
}

fn project_map_menu_storage(
    id: &str,
    source_storage: &[u8],
    preserved_suffix: &[u8],
    source_display_cell_count: Option<usize>,
    encoded: &[u8],
) -> Result<Vec<u8>> {
    ensure!(
        !encoded.contains(&STRING_END) && !encoded.contains(&SEGMENT_END),
        "translated map-menu label contains a structural terminator for {}",
        id
    );
    ensure!(
        !preserved_suffix.is_empty(),
        "map-menu label lost its structural suffix for {}",
        id
    );

    let mut replacement = vec![0xFF; source_storage.len()];
    if let Some(source_display_cell_count) = source_display_cell_count {
        let suffix_display_cell_count =
            terminated_composite_display_cell_count(preserved_suffix, STRING_END)?;
        let target_display_cell_count = composite_payload_display_cell_count(encoded)
            .checked_add(suffix_display_cell_count)
            .context("map funds-summary display span overflow")?;
        ensure!(
            target_display_cell_count <= source_display_cell_count,
            "map funds-summary projection exceeds the source display span for {}",
            id
        );
        let padding = source_display_cell_count - target_display_cell_count;
        let projected_len = encoded.len() + padding + preserved_suffix.len();
        ensure!(
            projected_len <= replacement.len(),
            "map funds-summary projection exceeds the source storage for {}",
            id
        );
        let mut cursor = 0;
        replacement[cursor..cursor + encoded.len()].copy_from_slice(encoded);
        cursor += encoded.len() + padding;
        replacement[cursor..cursor + preserved_suffix.len()].copy_from_slice(preserved_suffix);
        ensure!(
            terminated_composite_display_cell_count(
                &replacement[..cursor + preserved_suffix.len()],
                STRING_END,
            )? == source_display_cell_count,
            "map funds-summary projection changed the source display span for {}",
            id
        );
    } else {
        ensure!(
            encoded.len() + preserved_suffix.len() <= replacement.len(),
            "map menu projection exceeds the source storage for {}",
            id
        );
        replacement[..encoded.len()].copy_from_slice(encoded);
        let suffix_start = replacement.len() - preserved_suffix.len();
        replacement[suffix_start..].copy_from_slice(preserved_suffix);
    }
    Ok(replacement)
}

fn project_summary_status_label(
    spec: &FixedLabelSpec,
    encoded: &[u8],
    id: &str,
) -> Result<Vec<u8>> {
    ensure!(
        spec.expected.get(spec.expected.len().saturating_sub(2)) == Some(&SUMMARY_TRAILING_CELL),
        "source summary/status label lost its trailing colon for {id}"
    );
    ensure!(
        !encoded.contains(&STRING_END) && !encoded.contains(&SEGMENT_END),
        "translated summary/status label contains a structural terminator for {id}"
    );
    let source_display_cell_count =
        terminated_composite_display_cell_count(spec.expected, STRING_END)?;
    let target_display_cell_count = composite_payload_display_cell_count(encoded)
        .checked_add(1)
        .context("translated summary/status label display span overflow")?;
    ensure!(
        target_display_cell_count <= source_display_cell_count,
        "translated summary/status label exceeds its source display-cell span for {id}"
    );

    let mut bytes = vec![0xFF; source_display_cell_count - target_display_cell_count];
    bytes.extend_from_slice(encoded);
    bytes.extend([SUMMARY_TRAILING_CELL, STRING_END]);
    ensure!(
        terminated_composite_display_cell_count(&bytes, STRING_END)? == source_display_cell_count,
        "translated summary/status label changed its source display-cell span for {id}"
    );
    Ok(bytes)
}

fn bind_candidate(candidate: &Rom, offset: usize, expected: &[u8], role: &str) -> Result<()> {
    ensure!(
        candidate.data().get(offset..offset + expected.len()) == Some(expected),
        "{role} changed in the exact candidate"
    );
    Ok(())
}

fn match_targets_to_slots(targets: &mut [TargetString], slots: &mut [SourceSlot]) -> Result<()> {
    ensure!(
        targets.len() == slots.len(),
        "fixed UI target and source-slot counts disagree"
    );
    targets.sort_by_key(|target| (Reverse(target.bytes.len()), target.id.clone()));
    slots.sort_by_key(|slot| (Reverse(slot.expected.len()), slot.pointer));
    ensure!(
        targets
            .iter()
            .zip(slots.iter())
            .all(|(target, slot)| target.bytes.len() <= slot.expected.len()),
        "fixed UI strings cannot be matched one-to-one with their source-owned slots"
    );
    Ok(())
}

fn ensure_disjoint_writes(writes: &[FixedUiExpectedWrite]) -> Result<()> {
    for (index, left) in writes.iter().enumerate() {
        let left_end = left
            .file_offset
            .checked_add(left.expected.len())
            .context("fixed UI write range overflow")?;
        for right in &writes[index + 1..] {
            let right_end = right
                .file_offset
                .checked_add(right.expected.len())
                .context("fixed UI write range overflow")?;
            ensure!(
                left_end <= right.file_offset || right_end <= left.file_offset,
                "fixed UI projection writes overlap: {} and {}",
                left.role,
                right.role
            );
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn target(id: &str, byte_count: usize) -> TargetString {
        TargetString {
            id: id.to_owned(),
            domain: "synthetic",
            pointer_index: 0,
            source_pointer: 0x9000,
            bytes: vec![0x40; byte_count],
        }
    }

    fn slot(pointer: u16, byte_count: usize) -> SourceSlot {
        SourceSlot {
            pointer,
            expected: vec![0x20; byte_count],
        }
    }

    #[test]
    fn descending_capacity_matching_finds_the_complete_threshold_assignment() {
        let mut targets = [target("short", 2), target("long", 5), target("middle", 3)];
        let mut slots = [slot(0x9000, 3), slot(0x9010, 6), slot(0x9020, 4)];

        match_targets_to_slots(&mut targets, &mut slots).unwrap();

        assert_eq!(
            targets
                .iter()
                .zip(&slots)
                .map(|(target, slot)| (target.bytes.len(), slot.expected.len()))
                .collect::<Vec<_>>(),
            [(5, 6), (3, 4), (2, 3)]
        );
    }

    #[test]
    fn a_target_larger_than_every_remaining_slot_is_rejected() {
        let mut targets = [target("too-long", 6), target("short", 2)];
        let mut slots = [slot(0x9000, 5), slot(0x9010, 3)];

        let error = match_targets_to_slots(&mut targets, &mut slots).unwrap_err();

        assert!(error.to_string().contains("cannot be matched one-to-one"));
    }

    #[test]
    fn source_status_labels_share_one_eight_cell_span() {
        let status_indices = [0x00, 0x01, 0x02, 0x03, 0x04, 0x05, 0x06, 0x27, 0x28];

        for index in status_indices {
            let spec = SUMMARY_AND_STATUS_LABEL_SPECS
                .iter()
                .find(|spec| spec.index == index)
                .unwrap();
            assert_eq!(
                terminated_composite_display_cell_count(spec.expected, STRING_END).unwrap(),
                8,
                "status label {index:02X} lost its shared display span"
            );
        }
    }

    #[test]
    fn voiced_source_bytes_do_not_shift_translated_status_values() {
        for index in [0x02, 0x27] {
            let spec = SUMMARY_AND_STATUS_LABEL_SPECS
                .iter()
                .find(|spec| spec.index == index)
                .unwrap();

            let projected =
                project_summary_status_label(spec, &[0x40, 0x41, 0x42, 0x43], "fixture").unwrap();

            assert_eq!(
                projected,
                [0xFF, 0xFF, 0xFF, 0x40, 0x41, 0x42, 0x43, 0x8D, 0xEF]
            );
            assert_eq!(
                terminated_composite_display_cell_count(&projected, STRING_END).unwrap(),
                8
            );
        }
    }

    #[test]
    fn summary_level_keeps_its_original_four_cell_span() {
        let spec = SUMMARY_AND_STATUS_LABEL_SPECS
            .iter()
            .find(|spec| spec.index == 0x08)
            .unwrap();

        let projected = project_summary_status_label(spec, &[0x40, 0x41], "fixture").unwrap();

        assert_eq!(projected, [0xFF, 0x40, 0x41, 0x8D, 0xEF]);
        assert_eq!(
            terminated_composite_display_cell_count(&projected, STRING_END).unwrap(),
            4
        );
    }

    #[test]
    fn map_and_turn_labels_keep_the_number_column_while_replacing_japanese() {
        let projected = project_map_menu_storage(
            "map-funds-summary:map",
            &[0x50, 0x89, 0x4C, 0x1F, 0x8D, 0xEF],
            &[0x8D, 0xEF],
            Some(4),
            &[0xA0],
        )
        .unwrap();

        assert_eq!(projected, [0xA0, 0xFF, 0xFF, 0x8D, 0xEF, 0xFF]);
        assert_eq!(
            terminated_composite_display_cell_count(&projected[..5], STRING_END).unwrap(),
            4
        );
    }
}
