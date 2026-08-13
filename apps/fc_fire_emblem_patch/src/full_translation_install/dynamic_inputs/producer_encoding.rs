//! `{EC:xx}`가 읽는 RAM 문자열의 실제 바이트 계약을 결속한다.
//!
//! 페이지 리맵은 입력 바이트가 전용 정규 코드북으로 쓰였을 때만 유효하다. 현재
//! 생산자는 아이템·인물·지명 표의 바이트를 그대로 복사하므로, 원문 표나 전투용
//! 색칠 코드북을 전용 코드북이라고 가정하면 소비자 훅이 정상 문자열까지 망가뜨린다.

use anyhow::{Context, Result, ensure};
use serde::Serialize;

use crate::{
    full_translation_install::runtime_code::DialogueRuntimeHookRole,
    rom::{HEADER_SIZE, PRG_SIZE, Rom},
    sha1_hex,
    text_inventory::FixedTextPlannedEntry,
};

use super::DynamicDialogueInputPlan;

const FIXED_BANK_BYTE_COUNT: usize = 16 * 1024;
const STRING_TERMINATOR: u8 = 0xEF;
const MATERIAL_HEADER_BYTE_COUNT: usize = 16;
const MATERIAL_MAGIC: &[u8; 4] = b"FDPE";
const MATERIAL_SCHEMA: u8 = 1;
const DYNAMIC_SLOT_BYTE_COUNT: usize = 16;

#[derive(Debug, Serialize)]
pub(in crate::full_translation_install) struct DynamicProducerEncodingPlan {
    strategy: &'static str,
    canonical_code_count: usize,
    target_entry_count: usize,
    target_encoded_byte_count: usize,
    exact_candidate_entry_count: usize,
    mismatched_candidate_entry_count: usize,
    comparison_catalog_sha1: String,
    source_route_binding_is_not_output_encoding_binding: bool,
    producer_output_normalization_required: bool,
    producer_output_normalization_hook_bound: bool,
    every_candidate_producer_entry_uses_canonical_encoding: bool,
    complete: bool,
    #[serde(skip)]
    pub(in crate::full_translation_install) material: Vec<u8>,
    #[serde(skip)]
    pub(in crate::full_translation_install) item_directory_offset: usize,
    #[serde(skip)]
    pub(in crate::full_translation_install) unit_directory_offset: usize,
    #[serde(skip)]
    pub(in crate::full_translation_install) location_directory_offset: usize,
}

impl DynamicProducerEncodingPlan {
    pub(in crate::full_translation_install) fn canonical_outputs_ready(&self) -> bool {
        self.complete
    }

    pub(in crate::full_translation_install) fn bind_runtime_hooks(
        &mut self,
        emitted: &[DialogueRuntimeHookRole],
    ) -> Result<()> {
        let required = [
            DialogueRuntimeHookRole::DynamicItemSlotProducer,
            DialogueRuntimeHookRole::DynamicUnitSlotProducer,
            DialogueRuntimeHookRole::DynamicVillageItemProducer,
            DialogueRuntimeHookRole::DynamicEpilogueUnitProducer,
            DialogueRuntimeHookRole::DynamicEpilogueLocationProducer,
        ];
        self.producer_output_normalization_hook_bound =
            required.iter().all(|role| emitted.contains(role));
        self.complete = self.producer_output_normalization_hook_bound && !self.material.is_empty();
        ensure!(
            self.complete,
            "dynamic producer canonicalization did not emit every producer hook"
        );
        Ok(())
    }
}

pub(in crate::full_translation_install) fn bind_dynamic_producer_encoding(
    candidate: &Rom,
    dynamic: &DynamicDialogueInputPlan,
    fixed_entries: &[FixedTextPlannedEntry],
    unit_name_entries: &[FixedTextPlannedEntry],
    location_name_entries: &[FixedTextPlannedEntry],
) -> Result<DynamicProducerEncodingPlan> {
    let domains = [
        selected_entries(fixed_entries, "item-names"),
        selected_entries(unit_name_entries, "unit-names"),
        selected_entries(location_name_entries, "location-names"),
    ];
    ensure!(
        domains.iter().all(|entries| !entries.is_empty()),
        "dynamic producer encoding lost a translated string domain"
    );

    ensure!(
        domains[0].len() == 91 && domains[1].len() == 53 && domains[2].len() == 24,
        "dynamic producer canonical domain population changed"
    );
    for entries in &domains {
        ensure_contiguous_source_indices(entries)?;
    }
    let (material, directory_offsets) = encode_material(&domains, dynamic)?;
    let entries = domains.iter().flatten().copied().collect::<Vec<_>>();
    let mut exact_candidate_entry_count = 0;
    let mut target_encoded_byte_count = 0;
    let mut comparison_catalog = Vec::new();
    for entry in &entries {
        let mut expected = entry.encoded_bytes(&dynamic.canonical_dynamic_codes)?;
        expected.push(STRING_TERMINATOR);
        target_encoded_byte_count += expected.len();
        let runtime_offset = runtime_candidate_file_offset(candidate, entry.file_offset)?;
        let actual = candidate
            .data()
            .get(runtime_offset..runtime_offset + expected.len())
            .with_context(|| {
                format!(
                    "dynamic producer entry {} is outside the current candidate",
                    entry.id
                )
            })?;
        let exact = actual == expected;
        exact_candidate_entry_count += usize::from(exact);
        comparison_catalog.extend_from_slice(entry.table_id.as_bytes());
        comparison_catalog.push(0);
        comparison_catalog.extend_from_slice(&(entry.source_index as u64).to_le_bytes());
        comparison_catalog.extend_from_slice(&sha1_bytes(&expected));
        comparison_catalog.extend_from_slice(&sha1_bytes(actual));
        comparison_catalog.push(u8::from(exact));
    }

    let target_entry_count = entries.len();
    let mismatched_candidate_entry_count =
        target_entry_count.saturating_sub(exact_candidate_entry_count);
    let every_candidate_producer_entry_uses_canonical_encoding =
        mismatched_candidate_entry_count == 0;
    // 일치가 우연히 생겨도 전용 생산자 경로가 설치됐다는 뜻은 아니다. 원본 표를
    // 공유하는 다른 소비자와 분리된 쓰기 경로가 생겨야 이 플래그를 올릴 수 있다.
    let producer_output_normalization_hook_bound = false;
    let complete = every_candidate_producer_entry_uses_canonical_encoding
        && producer_output_normalization_hook_bound;

    Ok(DynamicProducerEncodingPlan {
        strategy: "normalize translated item, playable-unit, and location strings into one dedicated injective codebook before copying them to EC slots; never infer producer encoding from a source-route binding",
        canonical_code_count: dynamic.canonical_dynamic_codes.len(),
        target_entry_count,
        target_encoded_byte_count,
        exact_candidate_entry_count,
        mismatched_candidate_entry_count,
        comparison_catalog_sha1: sha1_hex(&comparison_catalog),
        source_route_binding_is_not_output_encoding_binding: true,
        producer_output_normalization_required: true,
        producer_output_normalization_hook_bound,
        every_candidate_producer_entry_uses_canonical_encoding,
        complete,
        material,
        item_directory_offset: directory_offsets[0],
        unit_directory_offset: directory_offsets[1],
        location_directory_offset: directory_offsets[2],
    })
}

fn ensure_contiguous_source_indices(entries: &[&FixedTextPlannedEntry]) -> Result<()> {
    for (index, entry) in entries.iter().enumerate() {
        ensure!(
            entry.source_index == index && entry.alias_indices.is_empty(),
            "dynamic producer table {} is not one contiguous unaliased source-index domain at {index}",
            entry.table_id
        );
    }
    Ok(())
}

fn encode_material(
    domains: &[Vec<&FixedTextPlannedEntry>; 3],
    dynamic: &DynamicDialogueInputPlan,
) -> Result<(Vec<u8>, [usize; 3])> {
    let directory_offsets = [
        MATERIAL_HEADER_BYTE_COUNT,
        MATERIAL_HEADER_BYTE_COUNT + domains[0].len() * 2,
        MATERIAL_HEADER_BYTE_COUNT + (domains[0].len() + domains[1].len()) * 2,
    ];
    let string_offset = MATERIAL_HEADER_BYTE_COUNT
        + domains
            .iter()
            .map(|entries| entries.len() * 2)
            .sum::<usize>();
    let mut material = vec![0; string_offset];
    material[..4].copy_from_slice(MATERIAL_MAGIC);
    material[4] = MATERIAL_SCHEMA;
    material[5] = u8::try_from(domains[0].len()).context("item directory count exceeds u8")?;
    material[6] = u8::try_from(domains[1].len()).context("unit directory count exceeds u8")?;
    material[7] = u8::try_from(domains[2].len()).context("location directory count exceeds u8")?;
    for (index, offset) in directory_offsets.into_iter().enumerate() {
        write_u16(&mut material[8 + index * 2..10 + index * 2], offset)?;
    }
    write_u16(&mut material[14..16], string_offset)?;

    for (domain_index, entries) in domains.iter().enumerate() {
        for (entry_index, entry) in entries.iter().enumerate() {
            let relative = material.len();
            let pointer_offset = directory_offsets[domain_index] + entry_index * 2;
            write_u16(&mut material[pointer_offset..pointer_offset + 2], relative)?;
            let encoded = entry.encoded_bytes(&dynamic.canonical_dynamic_codes)?;
            ensure!(
                encoded.len() < DYNAMIC_SLOT_BYTE_COUNT,
                "dynamic producer entry {} needs {} bytes including EF but a slot has only {DYNAMIC_SLOT_BYTE_COUNT}",
                entry.id,
                encoded.len() + 1
            );
            material.extend(encoded);
            material.push(STRING_TERMINATOR);
        }
    }
    ensure!(
        material.len() < 8 * 1024,
        "dynamic producer canonical material exceeds one MMC3 page"
    );
    Ok((material, directory_offsets))
}

fn write_u16(destination: &mut [u8], value: usize) -> Result<()> {
    let value = u16::try_from(value).context("dynamic producer material offset exceeds u16")?;
    destination.copy_from_slice(&value.to_le_bytes());
    Ok(())
}

fn selected_entries<'a>(
    entries: &'a [FixedTextPlannedEntry],
    table_id: &str,
) -> Vec<&'a FixedTextPlannedEntry> {
    entries
        .iter()
        .filter(|entry| entry.table_id == table_id)
        .collect()
}

/// 원본 마지막 16 KiB에 있던 자료는 PRG 확장 뒤 활성 마지막 뱅크로 옮겨 읽는다.
/// 나머지 원본 PRG 주소는 확장 전후가 같다.
fn runtime_candidate_file_offset(candidate: &Rom, source_file_offset: usize) -> Result<usize> {
    let source_fixed_start = HEADER_SIZE + PRG_SIZE - FIXED_BANK_BYTE_COUNT;
    let source_fixed_end = HEADER_SIZE + PRG_SIZE;
    if (source_fixed_start..source_fixed_end).contains(&source_file_offset) {
        let within_fixed = source_file_offset - source_fixed_start;
        ensure!(
            candidate.prg().len() >= FIXED_BANK_BYTE_COUNT,
            "current candidate has no active fixed bank"
        );
        Ok(HEADER_SIZE + candidate.prg().len() - FIXED_BANK_BYTE_COUNT + within_fixed)
    } else {
        Ok(source_file_offset)
    }
}

fn sha1_bytes(bytes: &[u8]) -> [u8; 20] {
    let encoded = sha1_hex(bytes);
    let mut digest = [0; 20];
    for (index, pair) in encoded.as_bytes().chunks_exact(2).enumerate() {
        let hex = std::str::from_utf8(pair).expect("SHA-1 hex is ASCII");
        digest[index] = u8::from_str_radix(hex, 16).expect("SHA-1 hex is valid");
    }
    digest
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn source_fixed_offsets_follow_the_expanded_active_fixed_bank() {
        let candidate = crate::test_support::release_rom();
        let source_fixed_start = HEADER_SIZE + PRG_SIZE - FIXED_BANK_BYTE_COUNT;

        let mapped = runtime_candidate_file_offset(&candidate, source_fixed_start + 0x123).unwrap();

        assert_eq!(
            mapped,
            HEADER_SIZE + candidate.prg().len() - FIXED_BANK_BYTE_COUNT + 0x123
        );
    }

    #[test]
    fn switchable_source_offsets_do_not_move_after_prg_expansion() {
        let candidate = crate::test_support::release_rom();
        let source_offset = HEADER_SIZE + 0x1234;

        assert_eq!(
            runtime_candidate_file_offset(&candidate, source_offset).unwrap(),
            source_offset
        );
    }
}
