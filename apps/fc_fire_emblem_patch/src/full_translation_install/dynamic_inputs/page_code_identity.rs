//! 동적 문자열의 canonical 코드와 페이지 물리 코드가 같은지 결속한다.

use std::collections::{BTreeMap, BTreeSet};

use anyhow::{Context, Result, ensure};

use crate::{mapper165::battle_codebook_plan::GlyphWorksetPagePlan, sha1_hex};

use super::DynamicDialogueInputPlan;

/// 동적 문자열의 canonical 바이트가 페이지의 실제 물리 코드와 같은지 결속한다.
///
/// 이 조건이 성립하면 `{EC}` 소비자는 별도 remap 훅이나 자료 없이 원본 reader를
/// 그대로 쓴다. 페이지 선택자도 상위 비트를 빌리지 않고 순수 그룹 색인만 가진다.
pub(in crate::full_translation_install) struct DynamicStringPageCodePlan {
    pub(in crate::full_translation_install) canonical_code_count: usize,
    pub(in crate::full_translation_install) translated_dynamic_page_group_count: usize,
    pub(in crate::full_translation_install) identity_entry_count: usize,
    pub(in crate::full_translation_install) selected_material_byte_count: usize,
    pub(in crate::full_translation_install) selected_strategy: &'static str,
    pub(in crate::full_translation_install) material_sha1: String,
    pub(in crate::full_translation_install) workset_page_selectors: Vec<u8>,
    pub(in crate::full_translation_install) canonical_codes_are_page_physical_codes: bool,
    pub(in crate::full_translation_install) page_selectors_use_plain_group_indices: bool,
    pub(in crate::full_translation_install) every_translated_dynamic_page_directly_consumable: bool,
}

pub(in crate::full_translation_install) fn bind_dynamic_string_page_codes(
    dynamic_inputs: &DynamicDialogueInputPlan,
    codebook: &GlyphWorksetPagePlan,
) -> Result<DynamicStringPageCodePlan> {
    ensure!(
        dynamic_inputs.dynamic_glyphs_by_workset.len() == codebook.workset_page_indices.len(),
        "dynamic dialogue page-code binding lost page worksets"
    );
    ensure!(
        dynamic_inputs.translated_dynamic_by_workset.len()
            == dynamic_inputs.preserved_numeric_by_workset.len()
            && dynamic_inputs.translated_dynamic_by_workset.len()
                == codebook.workset_page_indices.len(),
        "dynamic dialogue page-code flags lost page worksets"
    );
    ensure!(
        codebook.page_assignments.len() <= usize::from(u8::MAX) + 1,
        "dialogue page groups do not fit a plain one-byte selector"
    );

    let mut dynamic_groups = BTreeSet::new();
    let mut identity_entries = BTreeMap::<(usize, u8), char>::new();
    for (workset_index, dynamic_glyphs) in
        dynamic_inputs.dynamic_glyphs_by_workset.iter().enumerate()
    {
        if dynamic_glyphs.is_empty() {
            continue;
        }
        let group_index = codebook.workset_page_indices[workset_index];
        let assignments = codebook
            .page_assignments
            .get(group_index)
            .context("dynamic dialogue workset selects an unknown page group")?;
        dynamic_groups.insert(group_index);
        for glyph in dynamic_glyphs {
            let canonical = dynamic_inputs.canonical_dynamic_codes[glyph];
            let physical = assignments.get(glyph).copied().with_context(|| {
                format!("dynamic dialogue page group {group_index} lost glyph {glyph:?}")
            })?;
            ensure!(
                physical == canonical,
                "dynamic dialogue glyph {glyph:?} changes from canonical code {canonical:02X} to page code {physical:02X} in group {group_index}"
            );
            if let Some(previous) = identity_entries.insert((group_index, canonical), *glyph) {
                ensure!(
                    previous == *glyph,
                    "dynamic dialogue group {group_index} assigns canonical code {canonical:02X} to two glyphs"
                );
            }
        }
    }

    let workset_page_selectors = codebook
        .workset_page_indices
        .iter()
        .map(|group_index| {
            u8::try_from(*group_index).context("dialogue page-group selector does not fit u8")
        })
        .collect::<Result<Vec<_>>>()?;
    let material = Vec::<u8>::new();

    Ok(DynamicStringPageCodePlan {
        canonical_code_count: dynamic_inputs.canonical_dynamic_codes.len(),
        translated_dynamic_page_group_count: dynamic_groups.len(),
        identity_entry_count: identity_entries.len(),
        selected_material_byte_count: material.len(),
        selected_strategy: "assign every dynamic glyph one canonical physical code valid across all of its pages; keep the shared glyph reader unchanged",
        material_sha1: sha1_hex(&material),
        workset_page_selectors,
        canonical_codes_are_page_physical_codes: true,
        page_selectors_use_plain_group_indices: true,
        every_translated_dynamic_page_directly_consumable: true,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::mapper165::battle_codebook_plan::GlyphWorkset;

    #[test]
    fn workset_page_packing_keeps_a_fixed_dynamic_code_as_the_physical_code() {
        let worksets = vec![GlyphWorkset {
            target_glyphs: BTreeSet::from(['가', '나']),
            preserved_active_codes: BTreeSet::new(),
            fixed_glyph_codes: BTreeMap::from([('가', 0x42)]),
        }];

        let codebook =
            crate::mapper165::battle_codebook_plan::plan_glyph_workset_page_upper_bound(&worksets)
                .unwrap();

        assert_eq!(codebook.page_assignments[0][&'가'], 0x42);
    }
}
