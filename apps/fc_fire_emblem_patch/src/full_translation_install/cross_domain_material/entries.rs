use anyhow::{Context, Result, ensure};

use crate::{
    semantic_translation::SemanticTranslationPlan,
    sha1_hex,
    text_inventory::{FixedTextLogicalByte, FixedTextPlan, FixedTextPlannedEntry},
};

use super::CrossDomainMaterialInputs;

const EXPECTED_SECTION_COUNT: usize = 13;

#[derive(Clone)]
pub(super) struct MaterialEntry {
    pub(super) id: String,
    pub(super) source_binding: String,
    pub(super) logical_bytes: Vec<FixedTextLogicalByte>,
}

pub(super) struct SectionInput {
    pub(super) id: &'static str,
    pub(super) translation_input_sha1: String,
    pub(super) entries: Vec<MaterialEntry>,
}

pub(super) fn collect_section_inputs(
    inputs: &CrossDomainMaterialInputs<'_>,
) -> Result<Vec<SectionInput>> {
    let sections = vec![
        section(
            "chapter_save_offer_label",
            &inputs.transitions.save_offer.workspace_sha1,
            vec![MaterialEntry {
                id: "chapter-save-offer-label".to_owned(),
                source_binding: "chapter-save-offer-label".to_owned(),
                logical_bytes: inputs.transitions.save_offer.logical_bytes.clone(),
            }],
        ),
        section(
            "chapter_titles",
            &inputs.chapter_titles.workspace_sha1,
            inputs
                .chapter_titles
                .entries
                .iter()
                .map(|entry| MaterialEntry {
                    id: entry.id.clone(),
                    source_binding: format!(
                        "chapter={}:offset={:X}:bytes={}",
                        entry.chapter_index, entry.file_offset, entry.source_storage_byte_count
                    ),
                    logical_bytes: entry.logical_bytes().to_vec(),
                })
                .collect(),
        ),
        section(
            "choice_labels",
            &inputs.choices.workspace_sha1,
            inputs
                .choices
                .entries
                .iter()
                .map(|entry| MaterialEntry {
                    id: entry.id.clone(),
                    source_binding: format!("fixed-string-index={:02X}", entry.fixed_string_index),
                    logical_bytes: entry.logical_bytes().to_vec(),
                })
                .collect(),
        ),
        section(
            "class_names",
            &inputs.fixed.workspace_sha1,
            fixed_entries(inputs.fixed, "class-names"),
        ),
        section(
            "ending_record_labels",
            &inputs.transitions.ending_record.workspace_sha1,
            vec![MaterialEntry {
                id: "ending-total-turn-label".to_owned(),
                source_binding: "ending-total-turn-label".to_owned(),
                logical_bytes: inputs.transitions.ending_record.logical_bytes.clone(),
            }],
        ),
        section(
            "enemy_names",
            &inputs.fixed.workspace_sha1,
            fixed_entries(inputs.fixed, "enemy-names"),
        ),
        section(
            "fixed_menu_labels",
            &inputs.fixed_menu_labels.workspace_sha1,
            semantic_entries(inputs.fixed_menu_labels)?,
        ),
        section(
            "item_action_labels",
            &inputs.item_actions.workspace_sha1,
            semantic_entries(inputs.item_actions)?,
        ),
        section(
            "item_names",
            &inputs.fixed.workspace_sha1,
            fixed_entries(inputs.fixed, "item-names"),
        ),
        section(
            "location_names",
            &inputs.locations.workspace_sha1,
            fixed_entries(inputs.locations, "location-names"),
        ),
        section(
            "map_menu_labels",
            &inputs.map_menu.workspace_sha1,
            inputs
                .map_menu
                .entries
                .iter()
                .map(|entry| MaterialEntry {
                    id: entry.id.clone(),
                    source_binding: format!(
                        "cpu={:04X}:offset={:X}:source={}",
                        entry.source_cpu_address,
                        entry.source_file_offset,
                        sha1_hex(&entry.source_storage)
                    ),
                    logical_bytes: entry.logical_bytes().to_vec(),
                })
                .collect(),
        ),
        section(
            "unit_names",
            &inputs.unit_names.workspace_sha1,
            inputs
                .unit_names
                .entries
                .iter()
                .map(material_entry_from_fixed)
                .collect(),
        ),
        section(
            "unit_ui_labels",
            &inputs.unit_ui.workspace_sha1,
            semantic_entries(inputs.unit_ui)?,
        ),
    ];
    let expected_entry_counts = [1, 25, 2, 22, 1, 69, 7, 4, 91, 24, 8, 53, 25];
    ensure!(
        sections.len() == EXPECTED_SECTION_COUNT
            && sections
                .iter()
                .map(|section| section.entries.len())
                .eq(expected_entry_counts),
        "cross-domain material population changed"
    );
    Ok(sections)
}

pub(super) fn entry_identity_sha1(
    translation_input_sha1: &str,
    entries: &[MaterialEntry],
) -> String {
    let mut bytes = translation_input_sha1.as_bytes().to_vec();
    for entry in entries {
        bytes.push(0);
        bytes.extend_from_slice(entry.id.as_bytes());
        bytes.push(0);
        bytes.extend_from_slice(entry.source_binding.as_bytes());
    }
    sha1_hex(&bytes)
}

fn section(
    id: &'static str,
    translation_input_sha1: &str,
    entries: Vec<MaterialEntry>,
) -> SectionInput {
    SectionInput {
        id,
        translation_input_sha1: translation_input_sha1.to_owned(),
        entries,
    }
}

fn fixed_entries(plan: &FixedTextPlan, table_id: &str) -> Vec<MaterialEntry> {
    plan.entries
        .iter()
        .filter(|entry| entry.table_id == table_id)
        .map(material_entry_from_fixed)
        .collect()
}

fn material_entry_from_fixed(entry: &FixedTextPlannedEntry) -> MaterialEntry {
    MaterialEntry {
        id: entry.id.clone(),
        source_binding: format!(
            "table={}:index={}:aliases={:?}:offset={:X}:bytes={}",
            entry.table_id,
            entry.source_index,
            entry.alias_indices,
            entry.file_offset,
            entry.source_storage_byte_count
        ),
        logical_bytes: entry.logical_bytes.clone(),
    }
}

fn semantic_entries(plan: &SemanticTranslationPlan) -> Result<Vec<MaterialEntry>> {
    plan.entry_ids()
        .map(|id| {
            Ok(MaterialEntry {
                id: id.to_owned(),
                source_binding: id.to_owned(),
                logical_bytes: plan
                    .entry_logical_bytes(id)
                    .with_context(|| format!("semantic material lost {id}"))?
                    .to_vec(),
            })
        })
        .collect()
}
