use std::collections::BTreeMap;

use anyhow::{Context, Result, ensure};

use crate::{mapper165::battle_codebook_plan::GlyphWorksetPagePlan, sha1_hex};

use super::DynamicDialogueInputPlan;

pub(in crate::full_translation_install) struct DynamicStringRemapPlan {
    pub(in crate::full_translation_install) canonical_code_count: usize,
    pub(in crate::full_translation_install) remapped_page_group_count: usize,
    pub(in crate::full_translation_install) remap_entry_count: usize,
    pub(in crate::full_translation_install) non_identity_remap_entry_count: usize,
    pub(in crate::full_translation_install) dense_remap_byte_count: usize,
    pub(in crate::full_translation_install) sparse_remap_byte_count: usize,
    pub(in crate::full_translation_install) sparse_non_identity_remap_byte_count: usize,
    pub(in crate::full_translation_install) selected_dense_remap_byte_count: usize,
    pub(in crate::full_translation_install) selected_strategy: &'static str,
    pub(in crate::full_translation_install) remap_material_sha1: String,
    pub(in crate::full_translation_install) selected_dense_material: Vec<u8>,
    pub(in crate::full_translation_install) workset_page_selectors: Vec<u8>,
    pub(in crate::full_translation_install) page_selector_remap_flag_sufficient: bool,
    pub(in crate::full_translation_install) every_translated_dynamic_page_remappable: bool,
}

pub(in crate::full_translation_install) fn plan_dynamic_string_remap(
    dynamic_inputs: &DynamicDialogueInputPlan,
    codebook: &GlyphWorksetPagePlan,
) -> Result<DynamicStringRemapPlan> {
    ensure!(
        dynamic_inputs.dynamic_glyphs_by_workset.len() == codebook.workset_page_indices.len(),
        "dynamic dialogue remap lost page worksets"
    );
    ensure!(
        dynamic_inputs.translated_dynamic_by_workset.len()
            == dynamic_inputs.preserved_numeric_by_workset.len()
            && dynamic_inputs.translated_dynamic_by_workset.len()
                == codebook.workset_page_indices.len(),
        "dynamic dialogue remap flags lost page worksets"
    );
    let page_selector_remap_flag_sufficient = dynamic_inputs
        .translated_dynamic_by_workset
        .iter()
        .zip(&dynamic_inputs.preserved_numeric_by_workset)
        .all(|(translated, numeric)| !(*translated && *numeric));
    ensure!(
        page_selector_remap_flag_sufficient,
        "one dialogue page mixes translated and numeric-only EC controls"
    );
    ensure!(
        codebook.page_assignments.len() < 0x80,
        "dialogue page groups consume the selector's dynamic-remap flag bit"
    );

    let remaps_by_group = collect_group_remaps(dynamic_inputs, codebook)?;
    let remap_entry_count = remaps_by_group.values().map(BTreeMap::len).sum::<usize>();
    let non_identity_remap_entry_count = remaps_by_group
        .values()
        .flat_map(BTreeMap::iter)
        .filter(|(canonical, physical)| canonical != physical)
        .count();
    let sparse_directory_byte_count = (codebook.page_assignments.len() + 1) * 2;
    let dense_remap_byte_count =
        codebook.page_assignments.len() + remaps_by_group.len() * (usize::from(u8::MAX) + 1);
    let sparse_remap_byte_count = sparse_directory_byte_count + remap_entry_count * 2;
    let sparse_non_identity_remap_byte_count =
        sparse_directory_byte_count + non_identity_remap_entry_count * 2;

    let sparse =
        encode_sparse_non_identity_remaps(codebook.page_assignments.len(), &remaps_by_group)?;
    ensure!(
        sparse.len() == sparse_non_identity_remap_byte_count,
        "dynamic dialogue sparse remap measurement differs from its encoding"
    );
    let selected_dense = encode_dense_remaps(codebook.page_assignments.len(), &remaps_by_group)?;
    ensure!(
        selected_dense.len() == dense_remap_byte_count,
        "dynamic dialogue dense remap measurement differs from its encoding"
    );

    let workset_page_selectors = codebook
        .workset_page_indices
        .iter()
        .zip(&dynamic_inputs.translated_dynamic_by_workset)
        .map(|(group_index, translated_dynamic)| {
            let group_index = u8::try_from(*group_index)
                .context("dynamic dialogue page-group selector does not fit u8")?;
            if *translated_dynamic {
                ensure!(
                    remaps_by_group.contains_key(&usize::from(group_index)),
                    "translated dynamic page has no remap table"
                );
                Ok(group_index | 0x80)
            } else {
                Ok(group_index)
            }
        })
        .collect::<Result<Vec<_>>>()?;

    Ok(DynamicStringRemapPlan {
        canonical_code_count: dynamic_inputs.canonical_dynamic_codes.len(),
        remapped_page_group_count: remaps_by_group.len(),
        remap_entry_count,
        non_identity_remap_entry_count,
        dense_remap_byte_count,
        sparse_remap_byte_count,
        sparse_non_identity_remap_byte_count,
        selected_dense_remap_byte_count: selected_dense.len(),
        selected_strategy: "page-selector high-bit remap flag plus group-indexed 256-byte canonical-to-physical lookup",
        remap_material_sha1: sha1_hex(&selected_dense),
        selected_dense_material: selected_dense,
        workset_page_selectors,
        page_selector_remap_flag_sufficient,
        every_translated_dynamic_page_remappable: true,
    })
}

fn collect_group_remaps(
    dynamic_inputs: &DynamicDialogueInputPlan,
    codebook: &GlyphWorksetPagePlan,
) -> Result<BTreeMap<usize, BTreeMap<u8, u8>>> {
    let mut remaps_by_group = BTreeMap::<usize, BTreeMap<u8, u8>>::new();
    for (workset_index, dynamic_glyphs) in
        dynamic_inputs.dynamic_glyphs_by_workset.iter().enumerate()
    {
        if dynamic_glyphs.is_empty() {
            continue;
        }
        let group_index = codebook.workset_page_indices[workset_index];
        let assignments = &codebook.page_assignments[group_index];
        let group_remaps = remaps_by_group.entry(group_index).or_default();
        for glyph in dynamic_glyphs {
            let canonical = dynamic_inputs.canonical_dynamic_codes[glyph];
            let physical = assignments.get(glyph).copied().ok_or_else(|| {
                anyhow::anyhow!(
                    "dynamic dialogue page group {group_index} lost a domain glyph assignment"
                )
            })?;
            if let Some(previous) = group_remaps.insert(canonical, physical) {
                ensure!(
                    previous == physical,
                    "dynamic dialogue page group maps one canonical code twice"
                );
            }
        }
    }
    Ok(remaps_by_group)
}

fn encode_dense_remaps(
    group_count: usize,
    remaps_by_group: &BTreeMap<usize, BTreeMap<u8, u8>>,
) -> Result<Vec<u8>> {
    ensure!(
        remaps_by_group.keys().all(|group| *group < group_count),
        "dynamic dialogue dense remap contains an unknown page group"
    );
    ensure!(
        remaps_by_group.len() < usize::from(u8::MAX),
        "dynamic dialogue dense remap has no table-index sentinel"
    );
    let mut directory = vec![u8::MAX; group_count];
    let mut tables = Vec::with_capacity(remaps_by_group.len() * 256);
    for (table_index, (group_index, remaps)) in remaps_by_group.iter().enumerate() {
        directory[*group_index] =
            u8::try_from(table_index).context("dynamic dialogue dense remap index overflow")?;
        let mut table = (u8::MIN..=u8::MAX).collect::<Vec<_>>();
        for (canonical, physical) in remaps {
            table[usize::from(*canonical)] = *physical;
        }
        tables.extend_from_slice(&table);
    }
    directory.extend_from_slice(&tables);
    Ok(directory)
}

fn encode_sparse_non_identity_remaps(
    group_count: usize,
    remaps_by_group: &BTreeMap<usize, BTreeMap<u8, u8>>,
) -> Result<Vec<u8>> {
    ensure!(
        remaps_by_group.keys().all(|group| *group < group_count),
        "dynamic dialogue sparse remap contains an unknown page group"
    );
    let mut material = Vec::new();
    let mut offsets = Vec::with_capacity(group_count + 1);
    for group_index in 0..group_count {
        offsets.push(
            u16::try_from(material.len())
                .context("dynamic dialogue sparse remap exceeds a 16-bit directory offset")?,
        );
        if let Some(remaps) = remaps_by_group.get(&group_index) {
            for (canonical, physical) in remaps {
                if canonical != physical {
                    material.extend_from_slice(&[*canonical, *physical]);
                }
            }
        }
    }
    offsets.push(
        u16::try_from(material.len())
            .context("dynamic dialogue sparse remap exceeds a 16-bit end offset")?,
    );
    let mut encoded = Vec::with_capacity((group_count + 1) * 2 + material.len());
    for offset in offsets {
        encoded.extend_from_slice(&offset.to_le_bytes());
    }
    encoded.extend_from_slice(&material);
    Ok(encoded)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sparse_remap_omits_identity_pairs_and_keeps_empty_group_offsets() {
        let remaps = BTreeMap::from([
            (0, BTreeMap::from([(10, 10), (11, 12)])),
            (2, BTreeMap::from([(20, 21)])),
        ]);

        let encoded = encode_sparse_non_identity_remaps(3, &remaps).unwrap();

        assert_eq!(encoded, [0, 0, 2, 0, 2, 0, 4, 0, 11, 12, 20, 21]);
    }

    #[test]
    fn dense_remap_uses_identity_defaults_and_group_table_indices() {
        let remaps = BTreeMap::from([
            (0, BTreeMap::from([(10, 10), (11, 12)])),
            (2, BTreeMap::from([(20, 21)])),
        ]);

        let encoded = encode_dense_remaps(3, &remaps).unwrap();

        assert_eq!(encoded.len(), 3 + 2 * 256);
        assert_eq!(&encoded[..3], &[0, u8::MAX, 1]);
        assert_eq!(encoded[3 + 10], 10);
        assert_eq!(encoded[3 + 11], 12);
        assert_eq!(encoded[3 + 256 + 20], 21);
    }
}
