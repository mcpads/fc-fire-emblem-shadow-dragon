use std::{
    collections::{BTreeMap, BTreeSet},
    fs,
    path::Path,
};

use anyhow::{Context, Result, ensure};

use super::*;

pub(crate) fn import_entry_mode_draft(
    source_path: &Path,
    workspace_path: &Path,
    draft_path: &Path,
) -> Result<EntryModeDraftImportSummary> {
    validate_main_dialogue_entry_mode_workspace(source_path, workspace_path)?;
    let workspace_bytes = fs::read(workspace_path)
        .with_context(|| format!("read entry-mode workspace {}", workspace_path.display()))?;
    let mut workspace: EntryModeWorkspace = serde_json::from_slice(&workspace_bytes)
        .with_context(|| format!("parse entry-mode workspace {}", workspace_path.display()))?;
    let draft = fs::read_to_string(draft_path)
        .with_context(|| format!("read entry-mode draft {}", draft_path.display()))?;
    let translations = parse_entry_mode_draft(&draft)?;
    let expected_ids = workspace
        .records
        .iter()
        .flat_map(EntryModeRecord::parts)
        .filter(|part| {
            part.japanese_source_byte_count > 0 && part.status == TranslationStatus::Untranslated
        })
        .map(|part| part.id.clone())
        .collect::<BTreeSet<_>>();
    ensure_draft_covers_expected_parts(&translations, &expected_ids)?;
    for part in workspace
        .records
        .iter_mut()
        .flat_map(EntryModeRecord::parts_mut)
    {
        if let Some(korean) = translations.get(&part.id) {
            part.korean.clone_from(korean);
            part.status = TranslationStatus::NeedsHumanReview;
        }
    }
    validate_workspace_translations(&workspace)?;
    let mut output =
        serde_json::to_vec_pretty(&workspace).context("serialize imported entry-mode workspace")?;
    output.push(b'\n');
    write_file_atomically(workspace_path, &output)?;
    Ok(EntryModeDraftImportSummary {
        workspace_sha1: sha1_hex(&output),
        imported_part_count: translations.len(),
    })
}

fn parse_entry_mode_draft(draft: &str) -> Result<BTreeMap<String, String>> {
    let mut translations = BTreeMap::new();
    for (line_number, row) in draft.lines().enumerate() {
        if row.is_empty() || row.starts_with('#') {
            continue;
        }
        let (id, korean) = row
            .split_once('\t')
            .with_context(|| format!("draft line {} has no tab separator", line_number + 1))?;
        ensure!(!korean.is_empty(), "draft translation {id} is empty");
        ensure!(
            translations
                .insert(id.to_owned(), korean.to_owned())
                .is_none(),
            "entry-mode draft contains duplicate part {id}"
        );
    }
    Ok(translations)
}

fn ensure_draft_covers_expected_parts(
    translations: &BTreeMap<String, String>,
    expected_ids: &BTreeSet<String>,
) -> Result<()> {
    let actual_ids = translations.keys().cloned().collect::<BTreeSet<_>>();
    ensure!(
        &actual_ids == expected_ids,
        "entry-mode draft must cover every currently untranslated Japanese part exactly once"
    );
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn draft_parser_rejects_duplicate_part_ids() {
        let error = parse_entry_mode_draft("record:direct\t하나\nrecord:direct\t둘\n").unwrap_err();

        assert!(error.to_string().contains("duplicate part record:direct"));
    }

    #[test]
    fn draft_set_must_equal_every_expected_untranslated_part() {
        let translations = parse_entry_mode_draft("record:direct\t하나\n").unwrap();
        let expected = ["record:direct".to_owned(), "record:transition".to_owned()]
            .into_iter()
            .collect();

        let error = ensure_draft_covers_expected_parts(&translations, &expected).unwrap_err();

        assert!(
            error
                .to_string()
                .contains("cover every currently untranslated")
        );
    }
}
