//! 소비자 카탈로그 CHR 페이지와 같은 코드로 인코딩한 실행용 문자열 재료다.
//!
//! `KTX1`은 공유 atlas의 의미 셀을 보존하는 정적 자료라 실행 중 바로 읽을 수 없다.
//! 이 재료는 그 원천 엔트리를 현재 두 카탈로그 페이지의 실제 1바이트 코드로 투영한다.
//! 전투가 공유 원문 표를 자기 코드북으로 계속 쓸 수 있도록 원문 표 자체는 바꾸지 않는다.

use std::collections::BTreeMap;

use anyhow::{Context, Result, ensure};
use serde::Serialize;

use crate::{
    sha1_hex,
    text_inventory::{FixedTextLogicalByte, FixedTextPlan, FixedTextPlannedEntry},
    unit_names::UnitNamePlan,
};

use super::ConsumerCatalogPlan;

const MATERIAL_MAGIC: &[u8; 4] = b"FCCM";
const MATERIAL_SCHEMA: u8 = 1;
const MATERIAL_HEADER_BYTE_COUNT: usize = 16;
const MMC3_PAGE_BYTE_COUNT: usize = 8 * 1024;
const STRING_TERMINATOR: u8 = 0xEF;
const MAXIMUM_DISPLAY_STRING_BYTE_COUNT: usize = 16;

const ITEM_ENTRY_COUNT: usize = 91;
const CLASS_ENTRY_COUNT: usize = 22;
const UNIT_ENTRY_COUNT: usize = 53;
const ENEMY_ENTRY_COUNT: usize = 69;

pub(in crate::full_translation_install) struct ConsumerCatalogRuntimeMaterialInputs<'a> {
    pub(in crate::full_translation_install) file_offset: usize,
    pub(in crate::full_translation_install) mmc3_page: u8,
    pub(in crate::full_translation_install) fixed: &'a FixedTextPlan,
    pub(in crate::full_translation_install) unit_names: &'a UnitNamePlan,
    pub(in crate::full_translation_install) catalog: &'a ConsumerCatalogPlan,
}

#[derive(Clone, Copy)]
pub(in crate::full_translation_install) struct ConsumerCatalogRuntimeLayout {
    pub(in crate::full_translation_install) material_page: u8,
    pub(in crate::full_translation_install) material_base: u16,
    pub(in crate::full_translation_install) item_directory: u16,
    pub(in crate::full_translation_install) class_directory: u16,
    pub(in crate::full_translation_install) unit_directory: u16,
    pub(in crate::full_translation_install) enemy_directory: u16,
}

#[derive(Serialize)]
pub(in crate::full_translation_install) struct ConsumerCatalogRuntimeMaterialPlan {
    schema: u8,
    strategy: &'static str,
    pub(in crate::full_translation_install) file_offset: usize,
    mmc3_page: u8,
    byte_count: usize,
    item_entry_count: usize,
    class_entry_count: usize,
    unit_entry_count: usize,
    enemy_entry_count: usize,
    item_directory_offset: usize,
    class_directory_offset: usize,
    unit_directory_offset: usize,
    enemy_directory_offset: usize,
    string_payload_offset: usize,
    maximum_encoded_string_byte_count: usize,
    content_sha1: String,
    every_entry_uses_the_selected_catalog_page_codes: bool,
    every_name_carries_its_mapper_register: bool,
    one_mmc3_page_bound: bool,
    #[serde(skip)]
    pub(in crate::full_translation_install) bytes: Vec<u8>,
}

impl ConsumerCatalogRuntimeMaterialPlan {
    pub(in crate::full_translation_install) fn layout(
        &self,
    ) -> Result<ConsumerCatalogRuntimeLayout> {
        let address = |offset: usize| {
            u16::try_from(0x8000 + offset)
                .context("consumer catalog runtime address exceeds the 8000 window")
        };
        Ok(ConsumerCatalogRuntimeLayout {
            material_page: self.mmc3_page,
            material_base: 0x8000,
            item_directory: address(self.item_directory_offset)?,
            class_directory: address(self.class_directory_offset)?,
            unit_directory: address(self.unit_directory_offset)?,
            enemy_directory: address(self.enemy_directory_offset)?,
        })
    }
}

pub(in crate::full_translation_install) fn plan_consumer_catalog_runtime_material(
    inputs: ConsumerCatalogRuntimeMaterialInputs<'_>,
) -> Result<ConsumerCatalogRuntimeMaterialPlan> {
    let items = table_entries(inputs.fixed, "item-names");
    let classes = table_entries(inputs.fixed, "class-names");
    let enemies = table_entries(inputs.fixed, "enemy-names");
    let units = inputs.unit_names.entries.iter().collect::<Vec<_>>();
    ensure!(
        [items.len(), classes.len(), units.len(), enemies.len()]
            == [
                ITEM_ENTRY_COUNT,
                CLASS_ENTRY_COUNT,
                UNIT_ENTRY_COUNT,
                ENEMY_ENTRY_COUNT,
            ],
        "consumer catalog runtime population changed"
    );
    for entries in [&items, &classes, &units, &enemies] {
        ensure_contiguous_source_indices(entries)?;
    }

    let item_directory_offset = MATERIAL_HEADER_BYTE_COUNT;
    let class_directory_offset = item_directory_offset + items.len() * 2;
    let unit_directory_offset = class_directory_offset + classes.len() * 2;
    let enemy_directory_offset = unit_directory_offset + units.len() * 2;
    let string_payload_offset = enemy_directory_offset + enemies.len() * 2;
    let mut bytes = vec![0; string_payload_offset];
    bytes[..4].copy_from_slice(MATERIAL_MAGIC);
    bytes[4] = MATERIAL_SCHEMA;
    bytes[5] = u8::try_from(items.len()).context("item count exceeds u8")?;
    bytes[6] = u8::try_from(classes.len()).context("class count exceeds u8")?;
    bytes[7] = u8::try_from(units.len()).context("unit count exceeds u8")?;
    bytes[8] = u8::try_from(enemies.len()).context("enemy count exceeds u8")?;
    write_u16(&mut bytes[10..12], string_payload_offset)?;

    let mut maximum_encoded_string_byte_count = 0usize;
    encode_domain(
        &mut bytes,
        item_directory_offset,
        &items,
        |_| Ok((None, inputs.catalog.base_assignments().clone())),
        &mut maximum_encoded_string_byte_count,
    )?;
    encode_domain(
        &mut bytes,
        class_directory_offset,
        &classes,
        |_| Ok((None, inputs.catalog.base_assignments().clone())),
        &mut maximum_encoded_string_byte_count,
    )?;
    encode_domain(
        &mut bytes,
        unit_directory_offset,
        &units,
        |entry| {
            let page = inputs
                .catalog
                .page_for_name("unit_names", entry.source_index)?;
            Ok((Some(page.mapper_register()), page.assignments().clone()))
        },
        &mut maximum_encoded_string_byte_count,
    )?;
    encode_domain(
        &mut bytes,
        enemy_directory_offset,
        &enemies,
        |entry| {
            let page = inputs
                .catalog
                .page_for_name("enemy_names", entry.source_index)?;
            Ok((Some(page.mapper_register()), page.assignments().clone()))
        },
        &mut maximum_encoded_string_byte_count,
    )?;
    ensure!(
        bytes.len() <= MMC3_PAGE_BYTE_COUNT,
        "consumer catalog runtime material needs {} bytes but one MMC3 page has only {MMC3_PAGE_BYTE_COUNT}",
        bytes.len()
    );
    let content_sha1 = sha1_hex(&bytes);

    Ok(ConsumerCatalogRuntimeMaterialPlan {
        schema: MATERIAL_SCHEMA,
        strategy: "encode item, class, playable-unit, and enemy strings with the exact physical codes of their consumer catalog page; prefix variable names with the mapper register selected by their source identity",
        file_offset: inputs.file_offset,
        mmc3_page: inputs.mmc3_page,
        byte_count: bytes.len(),
        item_entry_count: items.len(),
        class_entry_count: classes.len(),
        unit_entry_count: units.len(),
        enemy_entry_count: enemies.len(),
        item_directory_offset,
        class_directory_offset,
        unit_directory_offset,
        enemy_directory_offset,
        string_payload_offset,
        maximum_encoded_string_byte_count,
        content_sha1,
        every_entry_uses_the_selected_catalog_page_codes: true,
        every_name_carries_its_mapper_register: true,
        one_mmc3_page_bound: true,
        bytes,
    })
}

fn encode_domain<F>(
    output: &mut Vec<u8>,
    directory_offset: usize,
    entries: &[&FixedTextPlannedEntry],
    mut assignment: F,
    maximum_encoded_string_byte_count: &mut usize,
) -> Result<()>
where
    F: FnMut(&FixedTextPlannedEntry) -> Result<(Option<u8>, BTreeMap<char, u8>)>,
{
    for (entry_index, entry) in entries.iter().enumerate() {
        let relative = output.len();
        let pointer_offset = directory_offset + entry_index * 2;
        write_u16(&mut output[pointer_offset..pointer_offset + 2], relative)?;
        let (mapper_register, assignments) = assignment(entry)?;
        if let Some(mapper_register) = mapper_register {
            output.push(mapper_register);
        }
        let encoded = entry
            .logical_bytes
            .iter()
            .map(|logical| match logical {
                FixedTextLogicalByte::Encoded(value) => Ok(*value),
                FixedTextLogicalByte::TargetGlyph(glyph) => {
                    assignments.get(glyph).copied().with_context(|| {
                        format!("consumer catalog page lost {glyph:?} for {}", entry.id)
                    })
                }
            })
            .collect::<Result<Vec<_>>>()?;
        ensure!(
            encoded.len() + 1 <= MAXIMUM_DISPLAY_STRING_BYTE_COUNT,
            "consumer catalog entry {} needs {} bytes including EF but the bounded display contract allows only {MAXIMUM_DISPLAY_STRING_BYTE_COUNT}",
            entry.id,
            encoded.len() + 1
        );
        ensure!(
            !encoded.contains(&STRING_TERMINATOR),
            "consumer catalog entry {} contains an early EF terminator",
            entry.id
        );
        *maximum_encoded_string_byte_count =
            (*maximum_encoded_string_byte_count).max(encoded.len() + 1);
        output.extend(encoded);
        output.push(STRING_TERMINATOR);
    }
    Ok(())
}

fn table_entries<'a>(plan: &'a FixedTextPlan, table_id: &str) -> Vec<&'a FixedTextPlannedEntry> {
    plan.entries
        .iter()
        .filter(|entry| entry.table_id == table_id)
        .collect()
}

fn ensure_contiguous_source_indices(entries: &[&FixedTextPlannedEntry]) -> Result<()> {
    ensure!(
        entries
            .iter()
            .enumerate()
            .all(|(index, entry)| entry.source_index == index),
        "consumer catalog runtime domain is not contiguous from source index zero"
    );
    Ok(())
}

fn write_u16(destination: &mut [u8], value: usize) -> Result<()> {
    destination.copy_from_slice(
        &u16::try_from(value)
            .context("consumer catalog runtime offset exceeds u16")?
            .to_le_bytes(),
    );
    Ok(())
}
