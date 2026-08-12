use std::collections::BTreeMap;

use anyhow::{Context, Result, ensure};

use super::*;

pub(super) fn preserve_translations(
    fresh: &mut EntryModeWorkspace,
    existing: &EntryModeWorkspace,
) -> Result<usize> {
    validate_workspace_binding(existing, fresh)?;
    validate_workspace_translations(existing)?;
    let existing_parts = existing
        .records
        .iter()
        .flat_map(EntryModeRecord::parts)
        .map(|part| (part.id.as_str(), part))
        .collect::<BTreeMap<_, _>>();
    ensure!(
        existing_parts.len() == existing.records.len() * 3,
        "entry-mode workspace contains duplicate part IDs"
    );
    let mut preserved = 0;
    for part in fresh
        .records
        .iter_mut()
        .flat_map(EntryModeRecord::parts_mut)
    {
        let existing_part = existing_parts
            .get(part.id.as_str())
            .with_context(|| format!("existing entry-mode part {} is absent", part.id))?;
        if existing_part.status != TranslationStatus::Untranslated {
            part.korean = existing_part.korean.clone();
            part.status = existing_part.status;
            preserved += 1;
        }
    }
    validate_workspace_translations(fresh)?;
    Ok(preserved)
}

pub(super) fn validate_workspace_binding(
    actual: &EntryModeWorkspace,
    expected: &EntryModeWorkspace,
) -> Result<()> {
    let mut actual = actual.clone();
    clear_targets(&mut actual);
    let mut expected = expected.clone();
    clear_targets(&mut expected);
    ensure!(
        actual == expected,
        "entry-mode workspace source binding or closed record population changed"
    );
    Ok(())
}

fn clear_targets(workspace: &mut EntryModeWorkspace) {
    for part in workspace
        .records
        .iter_mut()
        .flat_map(EntryModeRecord::parts_mut)
    {
        part.korean.clear();
        part.status = TranslationStatus::Untranslated;
    }
}

pub(super) fn validate_workspace_translations(
    workspace: &EntryModeWorkspace,
) -> Result<TranslationCounts> {
    let mut counts = TranslationCounts::default();
    for part in workspace.records.iter().flat_map(EntryModeRecord::parts) {
        match part.status {
            TranslationStatus::Untranslated => {
                ensure!(
                    part.korean.is_empty(),
                    "{} is untranslated but its korean field is not empty",
                    part.id
                );
                counts.untranslated_japanese_part_count +=
                    usize::from(part.japanese_source_byte_count > 0);
            }
            _ => {
                ensure!(
                    part.japanese_source_byte_count > 0,
                    "{} translates a source part that contains no Japanese",
                    part.id
                );
                ensure!(
                    !part.korean.is_empty(),
                    "{} has an empty translation",
                    part.id
                );
                counts.filled_part_count += 1;
                counts.complete_part_count +=
                    usize::from(part.status == TranslationStatus::Complete);
                counts.target_glyph_count += validate_translation_markup_pair(
                    &part.id,
                    &part.source_markup,
                    &part.korean,
                    true,
                )?;
            }
        }
    }
    Ok(counts)
}
