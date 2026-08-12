use std::{collections::BTreeMap, fs, path::Path};

use anyhow::{Context, Result, ensure};

use super::*;

mod model;
mod source_split;
#[cfg(test)]
mod tests;
mod validation;

use model::*;
use source_split::{build_entry_mode_workspace_without_seed, seed_entry_mode_translations};
use validation::{
    preserve_translations, validate_workspace_binding, validate_workspace_translations,
};

const ENTRY_MODE_WORKSPACE_FORMAT_VERSION: u8 = 1;
const REQUIRED_ENTRY_MODES: [&str; 2] = ["direct", "transition"];
const WORKSPACE_PURPOSE: &str = "private_normalized_entry_segment_translation_workspace";
const REACHABILITY_POLICY: &str =
    "require both direct and transition modes unless executable source-flow proof removes one";

#[derive(Debug)]
pub struct EntryModeWorkspaceSummary {
    pub workspace_sha1: String,
    pub record_count: usize,
    pub part_count: usize,
    pub differing_entry_start_japanese_source_byte_count: usize,
    pub leading_japanese_source_byte_count: usize,
    pub common_body_japanese_source_byte_count: usize,
    pub preserved_translation_part_count: usize,
}

#[derive(Debug)]
pub struct EntryModeWorkspaceValidationSummary {
    pub workspace_sha1: String,
    pub record_count: usize,
    pub part_count: usize,
    pub differing_entry_start_japanese_source_byte_count: usize,
    pub leading_japanese_source_byte_count: usize,
    pub common_body_japanese_source_byte_count: usize,
    pub filled_part_count: usize,
    pub complete_part_count: usize,
    pub untranslated_japanese_part_count: usize,
    pub target_glyph_count: usize,
    pub translation_input_complete: bool,
    pub review_complete: bool,
}

pub(crate) fn extract_main_dialogue_entry_mode_workspace(
    source_path: &Path,
    main_workspace_path: &Path,
    output_path: &Path,
) -> Result<EntryModeWorkspaceSummary> {
    let rom = Rom::from_path(source_path)?;
    rom.verify_supported_japanese()?;
    let main_workspace = load_main_workspace(rom.data(), main_workspace_path)?;
    let mut workspace = build_entry_mode_workspace(rom.data(), &main_workspace)?;
    if output_path.exists() {
        let existing_bytes = fs::read(output_path)
            .with_context(|| format!("read entry-mode workspace {}", output_path.display()))?;
        let existing: EntryModeWorkspace = serde_json::from_slice(&existing_bytes)
            .with_context(|| format!("parse entry-mode workspace {}", output_path.display()))?;
        preserve_translations(&mut workspace, &existing)?;
    }
    validate_workspace_translations(&workspace)?;
    let preserved_translation_part_count = workspace
        .records
        .iter()
        .flat_map(EntryModeRecord::parts)
        .filter(|part| part.status != TranslationStatus::Untranslated)
        .count();

    let mut bytes = serde_json::to_vec_pretty(&workspace)
        .context("serialize main-dialogue entry-mode workspace")?;
    bytes.push(b'\n');
    write_file_atomically(output_path, &bytes)?;
    Ok(summary(
        &workspace,
        sha1_hex(&bytes),
        preserved_translation_part_count,
    ))
}

pub(crate) fn validate_main_dialogue_entry_mode_workspace(
    source_path: &Path,
    workspace_path: &Path,
) -> Result<EntryModeWorkspaceValidationSummary> {
    let rom = Rom::from_path(source_path)?;
    rom.verify_supported_japanese()?;
    let bytes = fs::read(workspace_path)
        .with_context(|| format!("read entry-mode workspace {}", workspace_path.display()))?;
    let workspace: EntryModeWorkspace = serde_json::from_slice(&bytes)
        .with_context(|| format!("parse entry-mode workspace {}", workspace_path.display()))?;
    let expected = build_entry_mode_workspace_without_seed(rom.data())?;
    validate_workspace_binding(&workspace, &expected)?;
    let counts = validate_workspace_translations(&workspace)?;
    let (leading_japanese_source_byte_count, common_body_japanese_source_byte_count) =
        japanese_source_byte_counts(&workspace);
    Ok(EntryModeWorkspaceValidationSummary {
        workspace_sha1: sha1_hex(&bytes),
        record_count: workspace.records.len(),
        part_count: workspace.records.len() * 3,
        differing_entry_start_japanese_source_byte_count: workspace
            .differing_entry_start_japanese_source_byte_count,
        leading_japanese_source_byte_count,
        common_body_japanese_source_byte_count,
        filled_part_count: counts.filled_part_count,
        complete_part_count: counts.complete_part_count,
        untranslated_japanese_part_count: counts.untranslated_japanese_part_count,
        target_glyph_count: counts.target_glyph_count,
        translation_input_complete: counts.untranslated_japanese_part_count == 0,
        review_complete: counts.untranslated_japanese_part_count == 0
            && counts.filled_part_count == counts.complete_part_count,
    })
}

fn load_main_workspace(source: &[u8], workspace_path: &Path) -> Result<MainDialogueWorkspace> {
    let bytes = fs::read(workspace_path)
        .with_context(|| format!("read main dialogue workspace {}", workspace_path.display()))?;
    let workspace: MainDialogueWorkspace = serde_json::from_slice(&bytes)
        .with_context(|| format!("parse main dialogue workspace {}", workspace_path.display()))?;
    let expected = build_workspace(source)?;
    super::validate_workspace_binding(&workspace, &expected)?;
    super::validate_workspace_translations(&workspace)?;
    Ok(workspace)
}

fn build_entry_mode_workspace(
    source: &[u8],
    main_workspace: &MainDialogueWorkspace,
) -> Result<EntryModeWorkspace> {
    let mut workspace = build_entry_mode_workspace_without_seed(source)?;
    let main_records = main_workspace
        .records
        .iter()
        .map(|record| (record.id.as_str(), record))
        .collect::<BTreeMap<_, _>>();
    ensure!(
        main_records.len() == main_workspace.records.len(),
        "main dialogue workspace contains duplicate record IDs"
    );
    for record in &mut workspace.records {
        let main_record = main_records.get(record.id.as_str()).with_context(|| {
            format!(
                "entry-mode record {} is absent from main workspace",
                record.id
            )
        })?;
        seed_entry_mode_translations(record, main_record)?;
    }
    Ok(workspace)
}

fn japanese_source_byte_counts(workspace: &EntryModeWorkspace) -> (usize, usize) {
    let leading = workspace
        .records
        .iter()
        .flat_map(|record| [&record.direct_leading, &record.transition_leading])
        .map(|part| part.japanese_source_byte_count)
        .sum();
    let common = workspace
        .records
        .iter()
        .map(|record| record.common_body.japanese_source_byte_count)
        .sum();
    (leading, common)
}

fn summary(
    workspace: &EntryModeWorkspace,
    workspace_sha1: String,
    preserved_translation_part_count: usize,
) -> EntryModeWorkspaceSummary {
    let (leading_japanese_source_byte_count, common_body_japanese_source_byte_count) =
        japanese_source_byte_counts(workspace);
    EntryModeWorkspaceSummary {
        workspace_sha1,
        record_count: workspace.records.len(),
        part_count: workspace.records.len() * 3,
        differing_entry_start_japanese_source_byte_count: workspace
            .differing_entry_start_japanese_source_byte_count,
        leading_japanese_source_byte_count,
        common_body_japanese_source_byte_count,
        preserved_translation_part_count,
    }
}
