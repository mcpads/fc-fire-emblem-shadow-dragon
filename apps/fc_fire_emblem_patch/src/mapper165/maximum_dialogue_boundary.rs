use std::{
    collections::BTreeSet,
    fs,
    path::{Component, Path},
};

use anyhow::{Context, Result, ensure};
use serde::Deserialize;

use crate::{dialogue_assets::MainDialogueSlicePlan, rom::EXPECTED_SOURCE_SHA1, sha1_hex};

use super::maximum_dialogue_page::{COMPLETED_PAGE_COUNT, SCREEN_ROLE, TARGET_RECORD_ID};

#[derive(Debug, Deserialize)]
struct PageBoundaryManifest {
    format_version: u8,
    screen_role: String,
    target_record_id: String,
    source_sha1: String,
    workspace_sha1: String,
    observation_output_sha1: String,
    source_pointer_cpu_address: String,
    runtime_binding: RuntimeBinding,
    completed_pages: Vec<CompletedPage>,
}

#[derive(Debug, Deserialize)]
struct RuntimeBinding {
    dialogue_selector: String,
    completed_page_state: String,
    completed_line_count: u8,
    current_pointer_low_address: String,
    current_pointer_high_address: String,
    proceed_input: String,
}

#[derive(Debug, Deserialize)]
struct CompletedPage {
    page_index: usize,
    current_pointer: String,
    state_path: String,
    state_sha1: String,
}

pub(super) struct ObservedPageBoundaries {
    pub(super) manifest_sha1: String,
    pub(super) observation_output_sha1: String,
    pub(super) completed_page_pointers: Vec<u16>,
}

pub(super) fn load_observed_page_boundaries(
    path: &Path,
    record: &MainDialogueSlicePlan,
) -> Result<ObservedPageBoundaries> {
    let bytes = fs::read(path)
        .with_context(|| format!("read maximum dialogue page boundaries {}", path.display()))?;
    let manifest: PageBoundaryManifest = serde_json::from_slice(&bytes)
        .with_context(|| format!("parse maximum dialogue page boundaries {}", path.display()))?;
    ensure!(
        manifest.format_version == 1,
        "unsupported maximum dialogue page-boundary format"
    );
    ensure!(
        manifest.screen_role == SCREEN_ROLE && manifest.target_record_id == TARGET_RECORD_ID,
        "maximum dialogue page-boundary target changed"
    );
    ensure!(
        manifest.source_sha1 == EXPECTED_SOURCE_SHA1
            && manifest.workspace_sha1 == record.workspace_sha1,
        "maximum dialogue page boundaries are bound to different inputs"
    );
    ensure!(
        parse_hex_u16(&manifest.source_pointer_cpu_address, "source pointer")?
            == record.source_pointer_cpu_address(),
        "maximum dialogue page-boundary source pointer changed"
    );
    ensure!(
        is_lower_hex_sha1(&manifest.observation_output_sha1),
        "maximum dialogue observation output SHA-1 is malformed"
    );
    ensure!(
        manifest.runtime_binding.dialogue_selector == "C0:18"
            && manifest.runtime_binding.completed_page_state == "0x0E"
            && manifest.runtime_binding.completed_line_count == 4
            && manifest.runtime_binding.current_pointer_low_address == "0x7812"
            && manifest.runtime_binding.current_pointer_high_address == "0x7814"
            && manifest.runtime_binding.proceed_input == "A",
        "maximum dialogue page-boundary runtime binding changed"
    );
    ensure!(
        manifest.completed_pages.len() == COMPLETED_PAGE_COUNT,
        "maximum dialogue page-boundary coverage changed"
    );

    let parent = path
        .parent()
        .context("maximum dialogue page-boundary manifest has no parent")?;
    let mut state_paths = BTreeSet::new();
    let mut completed_page_pointers = Vec::with_capacity(COMPLETED_PAGE_COUNT);
    for (zero_based_index, page) in manifest.completed_pages.iter().enumerate() {
        ensure!(
            page.page_index == zero_based_index + 1,
            "maximum dialogue page-boundary order changed"
        );
        let relative = Path::new(&page.state_path);
        ensure!(
            !relative.is_absolute()
                && relative
                    .components()
                    .all(|component| !matches!(component, Component::ParentDir)),
            "maximum dialogue page-boundary state path escapes the manifest"
        );
        ensure!(
            state_paths.insert(page.state_path.as_str()),
            "maximum dialogue page-boundary state is duplicated"
        );
        let state_path = parent.join(relative);
        let state = fs::read(&state_path).with_context(|| {
            format!(
                "read maximum dialogue completed-page state {}",
                state_path.display()
            )
        })?;
        ensure!(
            sha1_hex(&state) == page.state_sha1,
            "maximum dialogue completed-page state SHA-1 changed"
        );
        completed_page_pointers.push(parse_hex_u16(
            &page.current_pointer,
            "completed-page pointer",
        )?);
    }
    record.page_unique_glyphs(&completed_page_pointers)?;

    Ok(ObservedPageBoundaries {
        manifest_sha1: sha1_hex(&bytes),
        observation_output_sha1: manifest.observation_output_sha1,
        completed_page_pointers,
    })
}

fn parse_hex_u16(value: &str, role: &str) -> Result<u16> {
    let digits = value
        .strip_prefix("0x")
        .with_context(|| format!("maximum dialogue {role} is not 0x-prefixed"))?;
    ensure!(
        digits.len() == 4,
        "maximum dialogue {role} must have four hexadecimal digits"
    );
    u16::from_str_radix(digits, 16)
        .with_context(|| format!("parse maximum dialogue {role} {value}"))
}

fn is_lower_hex_sha1(value: &str) -> bool {
    value.len() == 40
        && value
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sha1_shape_requires_forty_lowercase_hex_digits() {
        assert!(is_lower_hex_sha1(
            "0123456789abcdef0123456789abcdef01234567"
        ));
        assert!(!is_lower_hex_sha1(
            "0123456789ABCDEF0123456789ABCDEF01234567"
        ));
        assert!(!is_lower_hex_sha1("0123456789abcdef"));
    }

    #[test]
    fn pointer_text_has_a_fixed_unambiguous_shape() {
        assert_eq!(parse_hex_u16("0x901C", "pointer").unwrap(), 0x901C);
        assert!(parse_hex_u16("901C", "pointer").is_err());
        assert!(parse_hex_u16("0x901", "pointer").is_err());
    }
}
