use std::collections::{BTreeMap, BTreeSet};

use anyhow::{Context, Result, ensure};
use serde::Serialize;

use crate::{
    dialogue_assets::MainDialogueDisplayPlan,
    dialogue_inventory::{
        MainDialogueRuntimeIdentityBinding, inspect_main_dialogue_runtime_identities,
    },
    sha1_hex,
};

const MATERIAL_MAGIC: &[u8; 4] = b"FDID";
const MATERIAL_SCHEMA: u8 = 1;
const MATERIAL_HEADER_BYTE_COUNT: usize = 16;
const SELECTOR_DIRECTORY_BYTE_COUNT: usize = 256;
const TABLE_DESCRIPTOR_BYTE_COUNT: usize = 4;
/// 엔트리 하나는 표시 경로 색인 하나다. 전에는 직접·전이 두 색인을 넣어 네 바이트였다.
/// 두 모드의 차이는 레코드 프리픽스 파서 결함이 만든 것이어서 폐기했다.
/// 의사결정 59번을 따른다.
const ENTRY_BYTE_COUNT: usize = 2;
const MISSING_INDEX: u16 = u16::MAX;
/// 지원 원본의 selector 디렉터리 수다. 원본 고정 자료라 값이 바뀌면 원본이 바뀐 것이다.
const DIRECTORY_SELECTOR_COUNT: usize = 8;
/// 지원 원본에서 여덟 디렉터리가 가진 포인터 슬롯 총수다.
const POINTER_SLOT_COUNT: usize = 523;
/// 그중 실제로 레코드가 걸린 슬롯 수다. 나머지는 핸들러 빈칸이다.
const ENTRY_BINDING_COUNT: usize = 517;

#[derive(Serialize)]
pub(super) struct DialogueRuntimeIdentityPlan {
    lookup_state: RuntimeLookupState,
    canonical_record_count: usize,
    addressable_record_count: usize,
    directory_selector_count: usize,
    pointer_slot_count: usize,
    entry_binding_count: usize,
    handler_hole_count: usize,
    every_canonical_record_addressable: bool,
    material_schema: u8,
    material_byte_count: usize,
    material_sha1: String,
    material_serialized: bool,
    #[serde(skip)]
    pub(super) material: Vec<u8>,
}

#[derive(Serialize)]
struct RuntimeLookupState {
    directory_selector_address_hex: &'static str,
    entry_index_address_hex: &'static str,
}

struct IdentityTable {
    selector: u8,
    entries: Vec<Option<u16>>,
}

pub(super) fn plan_dialogue_runtime_identity(
    source: &[u8],
    display: &MainDialogueDisplayPlan,
) -> Result<DialogueRuntimeIdentityPlan> {
    let bindings = inspect_main_dialogue_runtime_identities(source)?;
    ensure!(
        bindings.len() == display.canonical_record_count,
        "runtime identity bindings and canonical dialogue population disagree"
    );
    let plan = build_runtime_identity_plan(&bindings, display)?;
    ensure!(
        plan.directory_selector_count == DIRECTORY_SELECTOR_COUNT
            && plan.pointer_slot_count == POINTER_SLOT_COUNT
            && plan.entry_binding_count == ENTRY_BINDING_COUNT,
        "main-dialogue runtime identity population changed"
    );
    Ok(plan)
}

fn build_runtime_identity_plan(
    bindings: &[MainDialogueRuntimeIdentityBinding],
    display: &MainDialogueDisplayPlan,
) -> Result<DialogueRuntimeIdentityPlan> {
    let record_indices = display
        .record_ids
        .iter()
        .enumerate()
        .map(|(index, record_id)| {
            Ok((
                record_id.as_str(),
                u16::try_from(index).context("dialogue record index does not fit u16")?,
            ))
        })
        .collect::<Result<BTreeMap<_, _>>>()?;
    ensure!(
        record_indices.len() == display.record_ids.len(),
        "dialogue runtime identity has duplicate record IDs"
    );

    let mut table_sizes = BTreeMap::<u8, usize>::new();
    for binding in bindings {
        if let Some(previous) =
            table_sizes.insert(binding.directory_selector, binding.pointer_count)
        {
            ensure!(
                previous == binding.pointer_count,
                "dialogue selector {:02X} has conflicting pointer counts",
                binding.directory_selector
            );
        }
    }
    let mut tables = table_sizes
        .into_iter()
        .map(|(selector, pointer_count)| IdentityTable {
            selector,
            entries: vec![None; pointer_count],
        })
        .collect::<Vec<_>>();
    let table_indices = tables
        .iter()
        .enumerate()
        .map(|(index, table)| (table.selector, index))
        .collect::<BTreeMap<_, _>>();

    for binding in bindings {
        let identity = record_indices
            .get(binding.record_id.as_str())
            .copied()
            .with_context(|| format!("{} has no display order index", binding.record_id))?;
        let table_index = *table_indices
            .get(&binding.directory_selector)
            .context("runtime identity selector lost its table")?;
        let table = &mut tables[table_index];
        for entry_index in &binding.entry_indices {
            let slot = table.entries.get_mut(*entry_index).with_context(|| {
                format!(
                    "dialogue selector {:02X} entry {} exceeds its identity table",
                    binding.directory_selector, entry_index
                )
            })?;
            ensure!(
                slot.is_none(),
                "dialogue selector {:02X} entry {} has multiple runtime identities",
                binding.directory_selector,
                entry_index
            );
            *slot = Some(identity);
        }
    }

    let pointer_slot_count = tables
        .iter()
        .map(|table| table.entries.len())
        .sum::<usize>();
    let entry_binding_count = tables
        .iter()
        .flat_map(|table| &table.entries)
        .filter(|entry| entry.is_some())
        .count();
    let handler_hole_count = pointer_slot_count - entry_binding_count;
    let referenced_record_indices = tables
        .iter()
        .flat_map(|table| &table.entries)
        .flatten()
        .copied()
        .collect::<BTreeSet<_>>();
    ensure!(
        referenced_record_indices.len() == record_indices.len(),
        "runtime identity table does not address every canonical dialogue record"
    );

    let material = encode_identity_material(&tables)?;
    Ok(DialogueRuntimeIdentityPlan {
        lookup_state: RuntimeLookupState {
            directory_selector_address_hex: "$77F4",
            entry_index_address_hex: "$77F1",
        },
        canonical_record_count: bindings.len(),
        addressable_record_count: record_indices.len(),
        directory_selector_count: tables.len(),
        pointer_slot_count,
        entry_binding_count,
        handler_hole_count,
        every_canonical_record_addressable: true,
        material_schema: MATERIAL_SCHEMA,
        material_byte_count: material.len(),
        material_sha1: sha1_hex(&material),
        material_serialized: true,
        material,
    })
}

fn encode_identity_material(tables: &[IdentityTable]) -> Result<Vec<u8>> {
    ensure!(
        tables.len() <= usize::from(u8::MAX),
        "runtime identity table count does not fit u8"
    );
    ensure!(
        tables
            .iter()
            .map(|table| table.selector)
            .collect::<BTreeSet<_>>()
            .len()
            == tables.len(),
        "runtime identity selectors are not unique"
    );
    let descriptor_offset = MATERIAL_HEADER_BYTE_COUNT + SELECTOR_DIRECTORY_BYTE_COUNT;
    let entries_offset = descriptor_offset + tables.len() * TABLE_DESCRIPTOR_BYTE_COUNT;
    let total_length = entries_offset
        + tables
            .iter()
            .map(|table| table.entries.len() * ENTRY_BYTE_COUNT)
            .sum::<usize>();
    let mut material = Vec::with_capacity(total_length);
    material.extend_from_slice(MATERIAL_MAGIC);
    material.push(MATERIAL_SCHEMA);
    material.push(u8::try_from(tables.len()).expect("checked identity table count"));
    push_u16(
        &mut material,
        total_length,
        "runtime identity material length",
    )?;
    push_u16(
        &mut material,
        MATERIAL_HEADER_BYTE_COUNT,
        "runtime identity selector directory offset",
    )?;
    push_u16(
        &mut material,
        descriptor_offset,
        "runtime identity descriptor offset",
    )?;
    push_u16(
        &mut material,
        entries_offset,
        "runtime identity entry offset",
    )?;
    material.extend_from_slice(&[0, 0]);
    ensure!(
        material.len() == MATERIAL_HEADER_BYTE_COUNT,
        "runtime identity header length changed"
    );

    let mut selector_directory = [u8::MAX; SELECTOR_DIRECTORY_BYTE_COUNT];
    for (table_index, table) in tables.iter().enumerate() {
        selector_directory[usize::from(table.selector)] =
            u8::try_from(table_index).expect("checked identity table count");
    }
    material.extend_from_slice(&selector_directory);

    let mut entry_block_offset = entries_offset;
    for table in tables {
        ensure!(
            table.entries.len() <= usize::from(u8::MAX),
            "runtime identity pointer count does not fit u8"
        );
        material.push(table.selector);
        material.push(u8::try_from(table.entries.len()).expect("checked pointer count"));
        push_u16(
            &mut material,
            entry_block_offset,
            "runtime identity table entry offset",
        )?;
        entry_block_offset += table.entries.len() * ENTRY_BYTE_COUNT;
    }
    for table in tables {
        for entry in &table.entries {
            material.extend_from_slice(&entry.unwrap_or(MISSING_INDEX).to_le_bytes());
        }
    }
    ensure!(
        material.len() == total_length && entry_block_offset == total_length,
        "runtime identity material length changed during serialization"
    );
    Ok(material)
}

fn push_u16(output: &mut Vec<u8>, value: usize, role: &str) -> Result<()> {
    output.extend_from_slice(
        &u16::try_from(value)
            .with_context(|| format!("{role} does not fit u16"))?
            .to_le_bytes(),
    );
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn identity_material_has_dense_selector_lookup_and_explicit_handler_holes() {
        let tables = vec![
            IdentityTable {
                selector: 0x41,
                entries: vec![Some(2), None],
            },
            IdentityTable {
                selector: 0x80,
                entries: vec![Some(7)],
            },
        ];

        let material = encode_identity_material(&tables).unwrap();
        let descriptor_offset = MATERIAL_HEADER_BYTE_COUNT + SELECTOR_DIRECTORY_BYTE_COUNT;
        let entries_offset = descriptor_offset + 2 * TABLE_DESCRIPTOR_BYTE_COUNT;

        assert_eq!(&material[..4], MATERIAL_MAGIC);
        assert_eq!(material[4], MATERIAL_SCHEMA);
        assert_eq!(material[5], 2);
        assert_eq!(material[MATERIAL_HEADER_BYTE_COUNT + 0x41], 0);
        assert_eq!(material[MATERIAL_HEADER_BYTE_COUNT + 0x80], 1);
        assert_eq!(material[MATERIAL_HEADER_BYTE_COUNT], u8::MAX);
        // 빈칸은 값이 없는 것이 아니라 `MISSING_INDEX`를 명시적으로 적는다.
        // 런타임이 빈칸을 읽고도 경로를 고르지 않게 하려면 자리가 있어야 한다.
        assert_eq!(
            &material[entries_offset..entries_offset + 6],
            &[2, 0, 0xFF, 0xFF, 7, 0]
        );
    }

    /// 런타임 식별표에서 상위 니블은 플래그가 아니라 디렉터리 정체성의 일부다.
    /// 실제 아이템 결과 경로가 쓰는 `B1`을 `01`로 줄이면 정상 레코드가 누락되어
    /// 한글 타일 코드가 원본 CHR로 표시된다.
    #[test]
    fn identity_material_keeps_the_full_directory_selector_byte() {
        let material = encode_identity_material(&[IdentityTable {
            selector: 0xB1,
            entries: vec![Some(27)],
        }])
        .unwrap();

        assert_eq!(material[MATERIAL_HEADER_BYTE_COUNT + 0xB1], 0);
        assert_eq!(material[MATERIAL_HEADER_BYTE_COUNT + 0x01], u8::MAX);
    }

    #[test]
    fn identity_material_rejects_duplicate_selectors() {
        let error = encode_identity_material(&[
            IdentityTable {
                selector: 0x41,
                entries: Vec::new(),
            },
            IdentityTable {
                selector: 0x41,
                entries: Vec::new(),
            },
        ])
        .unwrap_err();

        assert!(error.to_string().contains("selectors are not unique"));
    }
}
