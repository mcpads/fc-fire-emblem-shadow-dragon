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
const ENTRY_BYTE_COUNT: usize = 4;
const MISSING_INDEX: u16 = u16::MAX;

#[derive(Serialize)]
pub(super) struct DialogueRuntimeIdentityPlan {
    lookup_state: RuntimeLookupState,
    canonical_record_count: usize,
    display_path_count: usize,
    directory_selector_count: usize,
    pointer_slot_count: usize,
    direct_entry_binding_count: usize,
    handler_hole_count: usize,
    transition_record_count: usize,
    transition_entry_binding_count: usize,
    every_display_path_addressable: bool,
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
    transition_marker_address_hex: &'static str,
    transition_marker_mask_hex: &'static str,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct EntryIdentity {
    direct_path_index: u16,
    transition_path_index: u16,
}

struct IdentityTable {
    selector: u8,
    entries: Vec<Option<EntryIdentity>>,
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
        plan.canonical_record_count == 504
            && plan.display_path_count == 643
            && plan.directory_selector_count == 8
            && plan.pointer_slot_count == 523
            && plan.direct_entry_binding_count == 517
            && plan.handler_hole_count == 6
            && plan.transition_record_count == 139
            && plan.transition_entry_binding_count == 140
            && plan.material_byte_count == 2_396,
        "main-dialogue runtime identity population changed"
    );
    Ok(plan)
}

fn build_runtime_identity_plan(
    bindings: &[MainDialogueRuntimeIdentityBinding],
    display: &MainDialogueDisplayPlan,
) -> Result<DialogueRuntimeIdentityPlan> {
    let path_indices = display
        .display_paths
        .iter()
        .enumerate()
        .map(|(index, path)| {
            Ok((
                path.display_path_id.as_str(),
                u16::try_from(index).context("dialogue display-path index does not fit u16")?,
            ))
        })
        .collect::<Result<BTreeMap<_, _>>>()?;
    ensure!(
        path_indices.len() == display.display_paths.len(),
        "dialogue runtime identity has duplicate display-path IDs"
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

    let mut transition_path_indices = BTreeSet::new();
    let mut transition_entry_binding_count = 0usize;
    for binding in bindings {
        let canonical_path_id = binding.record_id.as_str();
        let direct_path_id = format!("{}@direct", binding.record_id);
        let transition_path_id = format!("{}@transition", binding.record_id);
        let direct_path_index = path_indices
            .get(direct_path_id.as_str())
            .or_else(|| path_indices.get(canonical_path_id))
            .copied()
            .with_context(|| format!("{} has no direct display path", binding.record_id))?;
        let transition_path_index = path_indices
            .get(transition_path_id.as_str())
            .copied()
            .unwrap_or(MISSING_INDEX);
        ensure!(
            transition_path_index != MISSING_INDEX
                || !path_indices.contains_key(direct_path_id.as_str()),
            "{} has a direct mode path but no transition mode path",
            binding.record_id
        );
        if transition_path_index != MISSING_INDEX {
            transition_path_indices.insert(transition_path_index);
            transition_entry_binding_count += binding.entry_indices.len();
        }
        let identity = EntryIdentity {
            direct_path_index,
            transition_path_index,
        };
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
    let direct_entry_binding_count = tables
        .iter()
        .flat_map(|table| &table.entries)
        .filter(|entry| entry.is_some())
        .count();
    let handler_hole_count = pointer_slot_count - direct_entry_binding_count;
    let referenced_path_indices = tables
        .iter()
        .flat_map(|table| &table.entries)
        .flatten()
        .flat_map(|entry| {
            [entry.direct_path_index, entry.transition_path_index]
                .into_iter()
                .filter(|index| *index != MISSING_INDEX)
        })
        .collect::<BTreeSet<_>>();
    ensure!(
        referenced_path_indices.len() == path_indices.len(),
        "runtime identity table does not address every dialogue display path"
    );

    let material = encode_identity_material(&tables)?;
    Ok(DialogueRuntimeIdentityPlan {
        lookup_state: RuntimeLookupState {
            directory_selector_address_hex: "$77F4",
            entry_index_address_hex: "$77F1",
            transition_marker_address_hex: "$77F2",
            transition_marker_mask_hex: "$80",
        },
        canonical_record_count: bindings.len(),
        display_path_count: path_indices.len(),
        directory_selector_count: tables.len(),
        pointer_slot_count,
        direct_entry_binding_count,
        handler_hole_count,
        transition_record_count: transition_path_indices.len(),
        transition_entry_binding_count,
        every_display_path_addressable: true,
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
            let identity = entry.unwrap_or(EntryIdentity {
                direct_path_index: MISSING_INDEX,
                transition_path_index: MISSING_INDEX,
            });
            material.extend_from_slice(&identity.direct_path_index.to_le_bytes());
            material.extend_from_slice(&identity.transition_path_index.to_le_bytes());
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
                entries: vec![
                    Some(EntryIdentity {
                        direct_path_index: 2,
                        transition_path_index: MISSING_INDEX,
                    }),
                    None,
                ],
            },
            IdentityTable {
                selector: 0x80,
                entries: vec![Some(EntryIdentity {
                    direct_path_index: 7,
                    transition_path_index: 8,
                })],
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
        assert_eq!(
            &material[entries_offset..entries_offset + 12],
            &[2, 0, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 7, 0, 8, 0]
        );
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
