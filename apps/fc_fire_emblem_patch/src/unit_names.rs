use std::{collections::BTreeSet, path::Path};

use anyhow::{Result, ensure};

use crate::{
    rom::Rom,
    text_inventory::{FixedTextPlannedEntry, plan_unit_name_text},
};

pub(crate) const UNIT_NAME_ENTRY_COUNT: usize = 53;

pub(crate) struct UnitNamePlan {
    pub(crate) workspace_sha1: String,
    pub(crate) review_complete: bool,
    pub(crate) entries: Vec<FixedTextPlannedEntry>,
}

impl UnitNamePlan {
    pub(crate) fn unique_glyphs(&self) -> BTreeSet<char> {
        self.entries
            .iter()
            .flat_map(FixedTextPlannedEntry::unique_glyphs)
            .collect()
    }
}

pub(crate) fn plan_unit_names(rom: &Rom, workspace_path: &Path) -> Result<UnitNamePlan> {
    let plan = plan_unit_name_text(rom, workspace_path)?;
    ensure!(
        plan.entries.len() == UNIT_NAME_ENTRY_COUNT,
        "playable-unit name count changed"
    );
    for (expected_index, entry) in plan.entries.iter().enumerate() {
        ensure!(
            entry.table_id == "unit-names"
                && entry.source_index == expected_index
                && entry.alias_indices.is_empty(),
            "playable-unit name table order or aliasing changed at {expected_index}"
        );
    }
    Ok(UnitNamePlan {
        workspace_sha1: plan.workspace_sha1,
        review_complete: plan.review_complete,
        entries: plan.entries,
    })
}
